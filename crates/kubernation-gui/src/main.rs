//! Kubernation GUI: the observed world rendered as a windowed strategy map —
//! the same `kubernation-core` models as the TUI, painted with macroquad.
//! With `--warm`, the standby cluster appears as a second archipelago
//! east of the hot one, with sync chips on every city.
//!
//!   make gui            # hot only
//!   make gui-pair       # hot + warm
//!
//! Controls: WASD/arrows or right-drag pan · wheel zoom · hover for
//! tooltips · click to inspect (city / province window) · ]/[ sail
//! between cities · N fly to the next concern · ?/F1 Almanac (in-app
//! reference) · Esc close · Q quit.

mod almanac;
mod city;
mod draw;
mod net;
mod node;
mod panels;
mod plan;
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
use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::Target;
use kubernation_core::state::world::Region;
use macroquad::prelude::*;
use net::{EvictReq, LogReq};
use panels::{Panel, draw_attention_strip, draw_evict_confirm, draw_logs, draw_tooltip};
use text::{text, text_bold, text_size};
use theme::*;

#[derive(Debug, Parser)]
#[command(name = "kubernation-gui", version, about)]
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
    /// Stage a demo scale + cordon and open the End-of-Turn review on sync
    /// (development verification)
    #[arg(long)]
    plan: bool,
    /// Center the camera on a named city / node / island at --zoom (default
    /// 1.4) without opening a panel, so coast & island marks render
    /// (development verification of map shots)
    #[arg(long)]
    center: Option<String>,
    /// With --center, shift the framed point east (+) / west (−) by N cells —
    /// e.g. to frame a city's offshore harbors (development verification)
    #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
    pan_dx: i32,
    /// Open the first matching workload's city and raise the evict confirm on
    /// its first pod (development verification of the eviction UI)
    #[arg(long)]
    evict: Option<String>,
    /// With --evict, auto-confirm the eviction (REALLY deletes the pod) —
    /// development verification of the write path
    #[arg(long)]
    evict_go: bool,
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Kubernation".into(),
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
    // Log tailing: the open overlay + a headless auto-open after --inspect.
    let mut log_open = false;
    let mut auto_tail = args.tail;
    // The Almanac (in-app reference) — a modal window; None = closed.
    let mut almanac: Option<Almanac> = None;
    // The planning turn: staged interventions (preview-only) + the open
    // End-of-Turn review modal.
    let mut planned = kubernation_core::state::planned::PlannedWorld::default();
    let mut plan_open = false;
    // The one mutation: a pod awaiting evict confirmation (cluster, ns, pod).
    let mut pending_evict: Option<(ClusterId, String, String)> = None;

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
        // Every drill-down (city or node) is a centered modal window: it
        // suspends map nav like the picker.
        let panel_modal = panel.is_some();
        // Track a panel opened by *this frame's* click so the window doesn't
        // read that same click as a click-outside dismiss.
        let mut panel_just_opened = false;
        let mut plan_just_opened = false;
        // Track an evict confirm opened *this frame* so the opening click can't
        // also hit the confirm's buttons.
        let mut evict_just_opened = false;

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
        // `t` opens the End-of-Turn review (planning turn) from the map.
        if is_key_pressed(KeyCode::T)
            && snap.is_some()
            && panel.is_none()
            && almanac.is_none()
            && !picker
        {
            plan_open = !plan_open;
            plan_just_opened = plan_open;
        }
        if is_key_pressed(KeyCode::Escape) {
            if pending_evict.is_some() {
                pending_evict = None;
            } else if almanac.is_some() {
                almanac = None;
            } else if plan_open {
                plan_open = false;
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

        // The Almanac swallows the wheel (scroll its content, not zoom) and
        // takes 1-4 / ←→ to switch pages.
        if let Some(a) = almanac.as_mut() {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                a.scroll_by(wheel);
            }
            for (k, i) in [
                (KeyCode::Key1, 0),
                (KeyCode::Key2, 1),
                (KeyCode::Key3, 2),
                (KeyCode::Key4, 3),
            ] {
                if is_key_pressed(k) {
                    a.go_idx(i);
                }
            }
            if is_key_pressed(KeyCode::Left) {
                a.cycle(-1);
            }
            if is_key_pressed(KeyCode::Right) {
                a.cycle(1);
            }
        }

        let mut manual_pan = false;
        if !picker && almanac.is_none() && !panel_modal && !plan_open {
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
                if let Some(needle) = &args.center {
                    // Headless map framing: zoom in and center on a named
                    // city / node / island so coast & island marks render
                    // (no panel, unlike --inspect).
                    cam.zoom = args.zoom.unwrap_or(1.4).clamp(0.3, 3.0);
                    let cell = worlds.iter().find_map(|sw| {
                        sw.world
                            .cities()
                            .find(|c| c.r.name.contains(needle.as_str()))
                            .map(|c| (c.x + sw.off, c.y))
                            .or_else(|| {
                                sw.world.continents.iter().find_map(|cont| {
                                    cont.provinces
                                        .iter()
                                        .find(|p| p.tile.name.contains(needle.as_str()))
                                        .map(|p| (p.x + sw.off + 2, p.y + 1))
                                })
                            })
                            .or_else(|| {
                                sw.world
                                    .islands
                                    .iter()
                                    .find(|isl| isl.label.contains(needle.as_str()))
                                    .map(|isl| (isl.x + sw.off + isl.w / 2, isl.y + isl.h / 2))
                            })
                    });
                    if let Some((cx, cy)) = cell {
                        let cx = (cx as i32 + args.pan_dx).max(0) as u16;
                        cam.jump_to((cx, cy));
                    }
                } else if let Some(z) = args.zoom {
                    cam.zoom = z.clamp(0.3, 3.0);
                    cam.jump_to((bounds.0 / 2, bounds.1 / 2));
                }
                if args.pick && !contexts.is_empty() {
                    picker = true;
                    picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                }
                if args.almanac {
                    almanac = Some(Almanac::new());
                }
                if args.plan {
                    let w = &s.hot.models.world;
                    if let Some(c) = w.cities().next() {
                        planned.stage_scale(c.r.clone(), c.desired + 2);
                    }
                    if let Some(p) = w.continents.iter().flat_map(|c| &c.provinces).next() {
                        planned.stage_cordon(p.tile.name.clone(), true);
                    }
                    plan_open = true;
                }
            }
            if picker || almanac.is_some() || panel_modal || plan_open {
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

                // No modal is open here (the whole block is suspended when one
                // is), so a left click is either a minimap jump or a
                // map-cell inspect that opens a drill-down window.
                if is_mouse_button_pressed(MouseButton::Left) {
                    let ml = minimap_layout(bounds);
                    if let Some(cell) = ml.world_cell(mouse, bounds) {
                        cam.fly_to(cell);
                    } else if mouse.y > panels::CHROME_H {
                        selected = cam.cell_at(mouse, bounds);
                        if let Some(sel) = selected {
                            panel = panel_for(&worlds, sel);
                            panel_just_opened = panel.is_some();
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

                // Development verification: open the first matching workload's
                // city and raise the evict confirm on its first pod (and, with
                // --evict-go, auto-confirm it a few frames later).
                if let Some(needle) = &args.evict
                    && pending_evict.is_none()
                    && panel.is_none()
                {
                    'ev: for sw in &worlds {
                        for c in sw.world.cities() {
                            if c.r.name.contains(needle.as_str())
                                && let Some(obs) = panels::observed_for(s, sw.id)
                                && let Some(city) =
                                    kubernation_core::state::model::build_city(obs, &c.r)
                                && let Some(p0) = city.pods.first()
                            {
                                let global = (c.x + sw.off, c.y);
                                selected = Some(global);
                                cam.jump_to(global);
                                panel = Some(Panel::City(sw.id, c.r.clone()));
                                pending_evict =
                                    Some((sw.id, c.r.namespace.clone(), p0.name.clone()));
                                break 'ev;
                            }
                        }
                    }
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

                // Hover tooltip (suppressed under a modal / while dragging).
                let ml = minimap_layout(bounds);
                let over_minimap = panel.is_none() && ml.frame.contains(mouse);
                if !picker
                    && almanac.is_none()
                    && !panel_modal
                    && !plan_open
                    && drag_anchor.is_none()
                    && !over_minimap
                    && mouse.y > panels::CHROME_H
                    && mouse.y < screen_height() - panels::STRIP_H
                    && let Some(cell) = cam.cell_at(mouse, bounds)
                    && let Some((sw, local)) = locate(&worlds, cell)
                {
                    draw_tooltip(sw, local, s, mouse);
                }

                // The End-of-Turn review takes over the center when open;
                // otherwise the drill-down windows / minimap show. Drill-downs
                // are modals (the log overlay, when open, sits on top and
                // swallows clicks via `!log_open`).
                let click = is_mouse_button_pressed(MouseButton::Left)
                    && !panel_just_opened
                    && !log_open
                    && pending_evict.is_none();
                let mut close_panel = false;
                if plan_open {
                    let pclick = is_mouse_button_pressed(MouseButton::Left) && !plan_just_opened;
                    let act = plan::draw_plan(&planned, Some(s), mouse, pclick);
                    if let Some(i) = act.unstage {
                        planned.unstage(i);
                    }
                    if act.discard {
                        planned.clear();
                        plan_open = false;
                    }
                    if act.close {
                        plan_open = false;
                    }
                } else {
                    match &panel {
                        Some(Panel::City(cid, cr)) => {
                            let act = city::draw_city(
                                *cid,
                                cr,
                                s,
                                &planned,
                                mouse,
                                click,
                                auto_tail && !log_open,
                            );
                            if let Some(iv) = act.stage {
                                planned.stage(iv);
                            }
                            if let Some((ns, pod)) = act.log {
                                net.request_logs(LogReq {
                                    cluster: *cid,
                                    namespace: ns,
                                    pod,
                                });
                                log_open = true;
                                auto_tail = false;
                            }
                            if let Some((ns, pod)) = act.evict {
                                pending_evict = Some((*cid, ns, pod));
                                evict_just_opened = true;
                            }
                            close_panel = act.close;
                        }
                        Some(Panel::Node(nid, nname)) => {
                            let act = node::draw_node(
                                *nid,
                                nname,
                                s,
                                &planned,
                                mouse,
                                click,
                                auto_tail && !log_open,
                            );
                            if let Some(iv) = act.stage {
                                planned.stage(iv);
                            }
                            if let Some((ns, pod)) = act.log {
                                net.request_logs(LogReq {
                                    cluster: *nid,
                                    namespace: ns,
                                    pod,
                                });
                                log_open = true;
                                auto_tail = false;
                            }
                            if let Some((ns, pod)) = act.evict {
                                pending_evict = Some((*nid, ns, pod));
                                evict_just_opened = true;
                            }
                            close_panel = act.close;
                        }
                        None => {
                            if log_open {
                                log_open = false;
                                net.clear_logs();
                            }
                            draw_minimap(&worlds, &cam, &ml);
                        }
                    }
                }
                if close_panel {
                    panel = None;
                    log_open = false;
                    net.clear_logs();
                }
                if log_open {
                    draw_logs(&net.log_tail());
                }
                draw_attention_strip(&s.attention, paired, concern_idx);
            }
        }

        // Top chrome: a carved tan-stone bar.
        draw_rectangle(0.0, 0.0, screen_width(), panels::CHROME_H - 2.0, STONE);
        draw_rectangle(0.0, 0.0, screen_width(), 1.5, STONE_LIGHT);
        draw_rectangle(
            0.0,
            panels::CHROME_H - 2.0,
            screen_width(),
            2.0,
            STONE_SHADOW,
        );
        text_bold(
            ascii(&format!(
                "KUBERNATION v{} — {status}",
                env!("CARGO_PKG_VERSION")
            )),
            12.0,
            21.0,
            20.0,
            STONE_INK,
        );
        // Almanac button (top-right); the help line ends to its left.
        let help_btn = Rect::new(screen_width() - 30.0, 5.0, 22.0, 22.0);
        draw_rectangle(help_btn.x, help_btn.y, help_btn.w, help_btn.h, STONE_DARK);
        draw_rectangle_lines(
            help_btn.x, help_btn.y, help_btn.w, help_btn.h, 1.0, STONE_EDGE,
        );
        text_bold("?", help_btn.x + 7.0, help_btn.y + 16.0, 16.0, STONE_LIGHT);
        if is_mouse_button_pressed(MouseButton::Left)
            && help_btn.contains(mouse)
            && almanac.is_none()
        {
            almanac = Some(Almanac::new());
            almanac_just_opened = true;
        }
        // End-Turn button + staged-change count (planning turn), left of `?`.
        let mut chrome_right = help_btn.x - 10.0;
        if !planned.is_empty() {
            let label = format!("End Turn ({})", planned.len());
            let tw = text_size(&label, 14.0).width;
            let tb = Rect::new(help_btn.x - tw - 24.0, 5.0, tw + 14.0, 22.0);
            let bg = if tb.contains(mouse) {
                lighter(STONE_DARK, 1.4)
            } else {
                STONE_DARK
            };
            draw_rectangle(tb.x, tb.y, tb.w, tb.h, bg);
            draw_rectangle_lines(tb.x, tb.y, tb.w, tb.h, 1.0, WARN);
            text(&label, tb.x + 7.0, 21.0, 14.0, WARN);
            if is_mouse_button_pressed(MouseButton::Left)
                && tb.contains(mouse)
                && panel.is_none()
                && almanac.is_none()
                && !picker
                && !plan_open
            {
                // Opens next frame (this draw already ran), so the opening
                // click can't reach the review as a click-outside dismiss.
                plan_open = true;
            }
            chrome_right = tb.x - 10.0;
        }
        let help = "drag/WASD pan . wheel zoom . F fit . click inspect . ]/[ cities . N concern . C context . t end-turn . ? almanac";
        let hm = text_size(help, 14.0);
        text(help, chrome_right - hm.width, 21.0, 14.0, STONE_INK_DIM);

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
            let action = almanac
                .as_mut()
                .map(|a| a.draw(snap.as_deref(), mouse, click));
            match action {
                Some(AlmanacAction::Close) => almanac = None,
                Some(AlmanacAction::Page(p)) => {
                    if let Some(a) = almanac.as_mut() {
                        a.go(p);
                    }
                }
                // Cross-reference: fly to a live example, then close.
                Some(AlmanacAction::Locate(cell)) => {
                    cam.fly_to(cell);
                    selected = Some(cell);
                    almanac = None;
                }
                _ => {}
            }
        }

        // Development verification: auto-confirm the staged evict (REAL delete)
        // a few frames after it's raised, so the write path can be exercised
        // headlessly.
        if args.evict_go
            && frames_synced == 20
            && let Some((cid, ns, pod)) = pending_evict.take()
        {
            net.request_evict(EvictReq {
                cluster: cid,
                namespace: ns,
                pod,
            });
        }

        // Evict confirm — the one destructive action, on top of everything.
        // Esc cancels (handled above); the opening click can't reach a button.
        if let Some((cid, ns, pod)) = pending_evict.clone() {
            let paired = snap.as_ref().is_some_and(|s| s.warm.is_some());
            let tag = if paired && cid == ClusterId::Warm {
                "WARM "
            } else {
                ""
            };
            let cclick = is_mouse_button_pressed(MouseButton::Left) && !evict_just_opened;
            let act = draw_evict_confirm(tag, &ns, &pod, mouse, cclick);
            if act.evict {
                net.request_evict(EvictReq {
                    cluster: cid,
                    namespace: ns,
                    pod,
                });
                pending_evict = None;
            } else if act.cancel {
                pending_evict = None;
            }
        }

        // Eviction result toast (auto-cleared by the net thread after a few s).
        if let Some(msg) = net.evict_status() {
            let fs = 15.0;
            let tm = text_size(&msg, fs);
            let bw = tm.width + 24.0;
            let bx = (screen_width() - bw) / 2.0;
            let by = panels::CHROME_H + 8.0;
            draw_rectangle(bx, by, bw, 26.0, STONE);
            draw_rectangle_lines(bx, by, bw, 26.0, 1.0, STONE_EDGE);
            text(ascii(&msg), bx + 12.0, by + 18.0, fs, STONE_INK);
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
