//! K8sCiv GUI: the observed world rendered as a windowed strategy map —
//! the same `k8sciv-core` models as the TUI, painted with macroquad.
//! With `--warm`, the standby cluster appears as a second archipelago
//! east of the hot one, with sync chips on every city.
//!
//!   make gui            # hot only
//!   make gui-pair       # hot + warm
//!
//! Controls: WASD/arrows or right-drag pan · wheel zoom · hover for
//! tooltips · click to inspect (city screen / node panel) · ]/[ sail
//! between cities · N fly to the next concern · ?/F1 Almanac (in-app
//! reference) · Esc close · Q quit.

mod almanac;
mod draw;
mod net;
mod panels;
mod sprites;
mod text;
mod theme;
mod window;

use std::path::PathBuf;

use almanac::{Almanac, AlmanacAction};
use clap::Parser;
use draw::{
    Camera, SceneWorld, draw_minimap, draw_sea, draw_selection, draw_world, locate, minimap_layout,
    scene, scene_size,
};
use k8sciv_core::events::ClusterId;
use k8sciv_core::state::attention::Target;
use k8sciv_core::state::world::Region;
use macroquad::prelude::*;
use net::LogReq;
use panels::{
    Panel, PodRowHit, draw_attention_strip, draw_logs, draw_panel, draw_tooltip, panel_layout,
};
use text::{text, text_bold, text_size};
use theme::*;

#[derive(Debug, Parser)]
#[command(name = "k8sciv-gui", version, about)]
struct Args {
    /// Kubeconfig context (defaults to current-context)
    #[arg(long)]
    context: Option<String>,
    /// Warm-standby context: a second archipelago with sync chips
    #[arg(long)]
    warm: Option<String>,
    /// Path to kubeconfig
    #[arg(long)]
    kubeconfig: Option<PathBuf>,
    /// Project a CRD's instances onto the map (repeatable)
    #[arg(long = "project", value_name = "CRD")]
    project: Vec<String>,
    /// Directory of replacement sprite PNGs (grass.png, house.png, …)
    #[arg(long)]
    tileset: Option<PathBuf>,

    /// Render until synced, save a PNG, exit (development verification)
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// On sync, select the first city whose name contains this and open
    /// its panel (development verification)
    #[arg(long)]
    inspect: Option<String>,
    /// Open the context picker on sync (development verification)
    #[arg(long)]
    pick: bool,
    /// Override the initial zoom after fit (development verification)
    #[arg(long)]
    zoom: Option<f32>,
    /// After --inspect opens a panel, tail its first pod's logs (verification)
    #[arg(long)]
    tail: bool,
    /// Open the Almanac (in-app reference) on sync (development verification)
    #[arg(long)]
    almanac: bool,
}

fn window_conf() -> Conf {
    Conf {
        window_title: "K8sCiv".into(),
        window_width: 1380,
        window_height: 860,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = Args::parse();
    text::init();
    sprites::init(args.tileset.as_deref());
    let shot = args.screenshot.clone();
    let inspect = args.inspect.clone();
    let want_warm = args.warm.is_some();
    let net = net::Net::new();
    net::spawn(
        net::NetArgs {
            context: args.context.clone(),
            kubeconfig: args.kubeconfig.clone(),
            warm: args.warm.clone(),
            projections: args.project.clone(),
        },
        net.clone(),
    );

    let mut cam = Camera::new();
    let mut selected: Option<(u16, u16)> = None;
    let mut panel: Option<Panel> = None;
    let mut concern_idx: usize = 0;
    let mut city_idx: usize = 0;
    let mut frames_synced: u32 = 0;
    let mut prev_had_snap = false;
    let mut inspected = false;
    let mut drag_anchor: Option<Vec2> = None;
    let mut picker = false;
    let mut picker_idx = 0usize;
    // Log tailing: clickable pod rows captured during panel draw, the open
    // overlay, and a headless auto-open after --inspect for verification.
    let mut pod_hits: Vec<PodRowHit> = Vec::new();
    let mut log_open = false;
    let mut auto_tail = args.tail;
    // The Almanac (in-app reference) — a modal window; None = closed.
    let mut almanac: Option<Almanac> = None;

    loop {
        let snap = net.snapshot();
        let status = net.status();
        let mouse = Vec2::from(mouse_position());
        let had_snap = prev_had_snap;
        prev_had_snap = snap.is_some();

        // Context list for the picker (from the hot world's kubeconfig).
        let contexts: Vec<String> = snap
            .as_ref()
            .map(|s| s.hot.observed.meta.all_contexts.clone())
            .unwrap_or_default();
        let current_ctx = snap
            .as_ref()
            .map(|s| s.hot.observed.meta.context.clone())
            .unwrap_or_default();

        // ---- input ------------------------------------------------------
        if is_key_pressed(KeyCode::Q) {
            break;
        }
        // ?, /, or F1 toggle the Almanac (in-app reference). Track an open
        // *this frame* so the same click/press doesn't immediately dismiss it.
        let mut almanac_just_opened = false;
        if is_key_pressed(KeyCode::F1) || is_key_pressed(KeyCode::Slash) {
            if almanac.is_some() {
                almanac = None;
            } else {
                almanac = Some(Almanac::new());
                almanac_just_opened = true;
            }
        }
        if is_key_pressed(KeyCode::Escape) {
            if almanac.is_some() {
                almanac = None;
            } else if picker {
                picker = false;
            } else if log_open {
                log_open = false;
                net.clear_logs();
            } else if panel.is_some() {
                panel = None;
            } else {
                break;
            }
        }
        // While the context picker is open it swallows navigation.
        if picker {
            let n = contexts.len();
            if is_key_pressed(KeyCode::C) {
                picker = false;
            } else if n > 0 {
                if is_key_pressed(KeyCode::J) || is_key_pressed(KeyCode::Down) {
                    picker_idx = (picker_idx + 1) % n;
                }
                if is_key_pressed(KeyCode::K) || is_key_pressed(KeyCode::Up) {
                    picker_idx = (picker_idx + n - 1) % n;
                }
                if is_key_pressed(KeyCode::Enter) && picker_idx < n {
                    net.request_switch(contexts[picker_idx].clone());
                    picker = false;
                    selected = None;
                    panel = None;
                    concern_idx = 0;
                }
            }
        }

        // The Almanac swallows the wheel (scroll its content, not zoom).
        if let Some(a) = almanac.as_mut() {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                a.scroll_by(wheel);
            }
        }

        let mut manual_pan = false;
        if !picker && almanac.is_none() {
            let pan = 14.0;
            if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
                cam.pos.x -= pan;
                manual_pan = true;
            }
            if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
                cam.pos.x += pan;
                manual_pan = true;
            }
            if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
                cam.pos.y -= pan;
                manual_pan = true;
            }
            if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
                cam.pos.y += pan;
                manual_pan = true;
            }
            if is_mouse_button_down(MouseButton::Right) || is_mouse_button_down(MouseButton::Middle)
            {
                if let Some(anchor) = drag_anchor {
                    let d = anchor - mouse;
                    if d.length() > 0.0 {
                        cam.pos += d;
                        manual_pan = true;
                    }
                }
                drag_anchor = Some(mouse);
            } else {
                drag_anchor = None;
            }
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                let factor = if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 };
                let before = (mouse + cam.pos) / cam.zoom;
                cam.zoom = (cam.zoom * factor).clamp(0.30, 3.0);
                cam.pos = before * cam.zoom - mouse;
            }
        }
        cam.tick(manual_pan);

        if let Some(s) = snap.as_ref() {
            let worlds = scene(s);
            let bounds = scene_size(&worlds);

            // Frame the whole world whenever a snapshot first appears —
            // initial sync, a reconnect, or after a context switch (which
            // clears the snapshot). Skipped when --inspect will fly us in.
            if !had_snap && inspect.is_none() {
                cam.fit(bounds);
                if let Some(z) = args.zoom {
                    cam.zoom = z.clamp(0.3, 3.0);
                    let (cw, ch) = cam.cell_px();
                    cam.pos = vec2(
                        (bounds.0 as f32 * cw - screen_width()) / 2.0,
                        (bounds.1 as f32 * ch - screen_height()) / 2.0 - 10.0,
                    );
                }
                if args.pick && !contexts.is_empty() {
                    picker = true;
                    picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                }
                if args.almanac {
                    almanac = Some(Almanac::new());
                }
            }
            if picker || almanac.is_some() {
                // A modal is open: world navigation is suspended this frame.
            } else {
                if is_key_pressed(KeyCode::F) {
                    cam.fit(bounds);
                }
                if is_key_pressed(KeyCode::C) && !contexts.is_empty() {
                    picker = true;
                    picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                }

                if is_key_pressed(KeyCode::RightBracket) || is_key_pressed(KeyCode::LeftBracket) {
                    // All cities across the scene, in archipelago order.
                    let cities: Vec<(u16, u16)> = worlds
                        .iter()
                        .flat_map(|sw| sw.world.cities().map(move |c| (c.x + sw.off, c.y)))
                        .collect();
                    if !cities.is_empty() {
                        if is_key_pressed(KeyCode::RightBracket) {
                            city_idx = (city_idx + 1) % cities.len();
                        } else {
                            city_idx = (city_idx + cities.len() - 1) % cities.len();
                        }
                        selected = Some(cities[city_idx]);
                        cam.fly_to(cities[city_idx]);
                    }
                }
                if is_key_pressed(KeyCode::N) && !s.attention.is_empty() {
                    concern_idx = (concern_idx + 1) % s.attention.len();
                    let concern = &s.attention[concern_idx];
                    if let Some(sw) = worlds.iter().find(|w| w.id == concern.cluster) {
                        let local = match &concern.target {
                            Target::Workload(r) => {
                                sw.world.city_pos(r).or_else(|| sw.world.structure_pos(r))
                            }
                            Target::Node(name) => sw.world.province_pos(name),
                            Target::WorkloadList => None,
                        };
                        if let Some(p) = local {
                            let global = (p.0 + sw.off, p.1);
                            selected = Some(global);
                            cam.fly_to(global);
                            panel = match &concern.target {
                                Target::Workload(r) => Some(Panel::City(sw.id, r.clone())),
                                Target::Node(name) => Some(Panel::Node(sw.id, name.clone())),
                                Target::WorkloadList => None,
                            };
                        }
                    }
                }
                if is_key_pressed(KeyCode::Enter)
                    && let Some(sel) = selected
                {
                    panel = panel_for(&worlds, sel);
                }

                if is_mouse_button_pressed(MouseButton::Left) {
                    let pl = panel_layout();
                    let ml = minimap_layout(bounds);
                    let over_panel = panel.is_some() && pl.frame.contains(mouse);
                    if log_open {
                        // The log overlay swallows clicks; Esc closes it.
                    } else if panel.is_some() && pl.close.contains(mouse) {
                        panel = None;
                    } else if let Some(hit) = pod_hits.iter().find(|h| h.rect.contains(mouse)) {
                        if let Some(p) = &panel {
                            net.request_logs(LogReq {
                                cluster: panel_cluster(p),
                                namespace: hit.namespace.clone(),
                                pod: hit.pod.clone(),
                            });
                            log_open = true;
                        }
                    } else if panel.is_none()
                        && let Some(cell) = ml.world_cell(mouse, bounds)
                    {
                        cam.fly_to(cell);
                    } else if !over_panel && mouse.y > panels::CHROME_H {
                        selected = cam.cell_at(mouse, bounds);
                        if let Some(sel) = selected {
                            panel = panel_for(&worlds, sel);
                        }
                    }
                }

                // Development verification: select and open something
                // specific — a city by name, else a province (node).
                if !inspected && let Some(needle) = &inspect {
                    'outer: for sw in &worlds {
                        for c in sw.world.cities() {
                            if c.r.name.contains(needle.as_str()) {
                                let global = (c.x + sw.off, c.y);
                                selected = Some(global);
                                cam.jump_to(global);
                                panel = Some(Panel::City(sw.id, c.r.clone()));
                                break 'outer;
                            }
                        }
                        for cont in &sw.world.continents {
                            for p in &cont.provinces {
                                if p.tile.name.contains(needle.as_str()) {
                                    let global = (p.x + sw.off + 2, p.y + 1);
                                    selected = Some(global);
                                    cam.jump_to(global);
                                    panel = Some(Panel::Node(sw.id, p.tile.name.clone()));
                                    break 'outer;
                                }
                            }
                        }
                    }
                    inspected = true;
                }
            } // end world navigation (suspended while the picker is open)
        }

        // ---- draw ---------------------------------------------------------
        clear_background(OCEAN);
        match snap.as_ref() {
            None => {
                text(ascii(&status), 40.0, 60.0, 30.0, PARCHMENT);
                text(
                    "the world is unexplored - fog of war",
                    40.0,
                    100.0,
                    24.0,
                    DIM,
                );
            }
            Some(s) => {
                let worlds = scene(s);
                let bounds = scene_size(&worlds);
                let paired = s.warm.is_some();
                if !want_warm || paired {
                    frames_synced += 1;
                }

                draw_sea(&cam);
                for sw in &worlds {
                    let wc = cam.shifted(sw.off);
                    let banner = paired.then_some((sw.label.as_str(), sw.id));
                    draw_world(sw.world, &wc, banner, s.pair.as_deref());
                }
                if let Some(sel) = selected {
                    draw_selection(&cam, sel);
                }

                // Hover tooltip (suppressed while dragging / over chrome).
                let pl = panel_layout();
                let over_panel = panel.is_some() && pl.frame.contains(mouse);
                let ml = minimap_layout(bounds);
                let over_minimap = panel.is_none() && ml.frame.contains(mouse);
                if !picker
                    && almanac.is_none()
                    && drag_anchor.is_none()
                    && !over_panel
                    && !over_minimap
                    && mouse.y > panels::CHROME_H
                    && mouse.y < screen_height() - panels::STRIP_H
                    && let Some(cell) = cam.cell_at(mouse, bounds)
                    && let Some((sw, local)) = locate(&worlds, cell)
                {
                    draw_tooltip(sw, local, s, mouse);
                }

                if let Some(p) = &panel {
                    pod_hits = draw_panel(p, s, &pl);
                    // Headless verification: open the first pod's log tail
                    // once the panel's rows are known.
                    if auto_tail && !log_open && !pod_hits.is_empty() {
                        let h = &pod_hits[0];
                        net.request_logs(LogReq {
                            cluster: panel_cluster(p),
                            namespace: h.namespace.clone(),
                            pod: h.pod.clone(),
                        });
                        log_open = true;
                        auto_tail = false;
                    }
                } else {
                    pod_hits.clear();
                    if log_open {
                        log_open = false;
                        net.clear_logs();
                    }
                    draw_minimap(&worlds, &cam, &ml);
                }
                if log_open {
                    draw_logs(&net.log_tail());
                }
                draw_attention_strip(&s.attention, paired, concern_idx);
            }
        }

        // Top chrome.
        draw_rectangle(0.0, 0.0, screen_width(), panels::CHROME_H - 2.0, PANEL);
        draw_rectangle(0.0, panels::CHROME_H - 2.0, screen_width(), 2.0, PARCHMENT);
        text_bold(
            ascii(&format!("K8SCIV — {status}")),
            12.0,
            21.0,
            20.0,
            PARCHMENT,
        );
        // Almanac button (top-right); the help line ends to its left.
        let help_btn = Rect::new(screen_width() - 30.0, 5.0, 22.0, 22.0);
        draw_rectangle(help_btn.x, help_btn.y, help_btn.w, help_btn.h, PLATE);
        draw_rectangle_lines(
            help_btn.x, help_btn.y, help_btn.w, help_btn.h, 1.0, PARCHMENT,
        );
        text_bold("?", help_btn.x + 7.0, help_btn.y + 16.0, 16.0, PARCHMENT);
        if is_mouse_button_pressed(MouseButton::Left)
            && help_btn.contains(mouse)
            && almanac.is_none()
        {
            almanac = Some(Almanac::new());
            almanac_just_opened = true;
        }
        let help = "drag/WASD pan . wheel zoom . F fit . click inspect . ]/[ cities . N concern . C context . ? almanac";
        let hm = text_size(help, 14.0);
        text(help, help_btn.x - hm.width - 10.0, 21.0, 14.0, DIM);

        // Context picker, drawn on top of everything.
        if picker {
            let layout = panels::draw_picker(&contexts, &current_ctx, picker_idx);
            if is_mouse_button_pressed(MouseButton::Left) {
                for (i, r) in layout.rows.iter().enumerate() {
                    if r.contains(mouse) && i < contexts.len() {
                        net.request_switch(contexts[i].clone());
                        picker = false;
                        selected = None;
                        panel = None;
                        concern_idx = 0;
                    }
                }
            }
        }

        // The Almanac, drawn on top of everything; it handles its own clicks
        // (but not the click that opened it this frame).
        if almanac.is_some() {
            let click = is_mouse_button_pressed(MouseButton::Left) && !almanac_just_opened;
            let action = almanac.as_mut().map(|a| a.draw(mouse, click));
            match action {
                Some(AlmanacAction::Close) => almanac = None,
                Some(AlmanacAction::Page(p)) => {
                    if let Some(a) = almanac.as_mut() {
                        a.go(p);
                    }
                }
                _ => {}
            }
        }

        // When tailing, wait long enough for the net thread's first fetch
        // (first_container + tail, two API round-trips) to land.
        let shot_at = if args.tail { 240 } else { 45 };
        if let Some(path) = &shot
            && frames_synced > shot_at
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        next_frame().await;
    }
}

fn panel_for(worlds: &[SceneWorld], sel: (u16, u16)) -> Option<Panel> {
    let (sw, local) = locate(worlds, sel)?;
    // A coast marker opens the city it serves.
    if let Some((_, m)) = sw.world.coast_at(local.0, local.1) {
        return Some(Panel::City(sw.id, m.workload.clone()));
    }
    match sw.world.region_at(local.0, local.1) {
        Region::City(_, c) => Some(Panel::City(sw.id, c.r.clone())),
        Region::Province(p) => Some(Panel::Node(sw.id, p.tile.name.clone())),
        Region::Structure(_, s) => s.workload.clone().map(|r| Panel::City(sw.id, r)),
        _ => None,
    }
}

fn panel_cluster(panel: &Panel) -> ClusterId {
    match panel {
        Panel::City(id, _) | Panel::Node(id, _) => *id,
    }
}
