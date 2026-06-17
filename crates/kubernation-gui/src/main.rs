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
mod logo;
mod menu;
mod net;
mod node;
mod panels;
mod plan;
mod sidebar;
mod text;
mod theme;
mod window;

use std::path::PathBuf;

use almanac::{Almanac, AlmanacAction};
use clap::Parser;
use draw::{
    Camera, Overlay, SceneWorld, draw_sea, draw_selection, draw_world, locate, minimap_layout,
    scene, scene_size,
};
use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::Target;
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::world::Region;
use macroquad::prelude::*;
use menu::{MenuAction, MenuCtx};
use net::{EvictReq, LogReq};
use panels::{
    Panel, draw_attention_strip, draw_commit_confirm, draw_evict_confirm, draw_logs, draw_tooltip,
};
use text::{text, text_size};
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
    /// Hold the intro splash (the full Kubernation scene) — replays it, and
    /// with --screenshot captures it (development verification / demo)
    #[arg(long)]
    splash: bool,
    /// With --plan, auto-commit the staged turn (REALLY applies scale/cordon)
    /// — development verification of the apply path
    #[arg(long)]
    plan_go: bool,
    /// With --tail, open the log overlay on the *previous* container
    /// (development verification of the --previous toggle)
    #[arg(long)]
    log_previous: bool,
    /// With --tail, pre-fill the log filter with this substring
    /// (development verification of the grep/filter)
    #[arg(long, value_name = "SUBSTR")]
    log_filter: Option<String>,
    /// Launch scoped to a single namespace (the namespace filter; you can
    /// still change it from the World menu). Also used for verification.
    #[arg(long, value_name = "NS")]
    namespace: Option<String>,
    /// Start with a map overlay active: "terrain" (default), "pressure"
    /// (cpu/mem heat), "replicas" (workload health) or "namespace" (territory).
    /// Set from the View menu at runtime; flag is for shots.
    #[arg(long, value_name = "MODE")]
    overlay: Option<String>,
    /// Open a chrome menu on sync — game / view / orders / world / help
    /// (development verification of the menu bar dropdowns)
    #[arg(long, value_name = "NAME")]
    menu: Option<String>,
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Kubernation".into(),
        window_width: 1380,
        window_height: 860,
        high_dpi: true,
        icon: logo::window_icon(),
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = Args::parse();
    text::init();
    logo::init();
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
    if let Some(ns) = &args.namespace {
        net.set_namespace_filter(NamespaceFilter::only(ns.clone()));
    }

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
    // Namespace-filter picker (single-select: "all" or one namespace).
    let mut ns_picker = false;
    let mut ns_picker_idx = 0usize;
    // Dragging the minimap viewport box to recenter the main view.
    let mut minimap_drag = false;
    // The Civ-style chrome menu bar: which top-level menu is open (None = all
    // closed). An open menu suspends map navigation, like the other modals.
    let mut open_menu: Option<usize> = None;
    // The active map overlay (the View menu's "map display"): how terrain is
    // colored. A --overlay dev flag seeds it for headless shots.
    let mut overlay = match args.overlay.as_deref() {
        Some("pressure") => Overlay::Pressure,
        Some("replicas") => Overlay::Replicas,
        Some("namespace") => Overlay::Namespace,
        _ => Overlay::Terrain,
    };
    // Menu "Fit view" can't reach `bounds` from the chrome draw, so it defers
    // the camera fit to the next frame's input block (where bounds is in scope).
    let mut pending_fit = false;
    // While Some, the city window's image field is capturing a new image string
    // (the "set image" planning verb); global single-key shortcuts are text.
    let mut city_image_edit: Option<String> = None;
    // Log tailing: the open overlay + a headless auto-open after --inspect.
    let mut log_open = false;
    // Log overlay state: --previous container toggle + substring filter editor.
    let mut log_previous = false;
    let mut log_filter = String::new();
    let mut log_filter_active = false;
    let mut auto_tail = args.tail;
    // The Almanac (in-app reference) — a modal window; None = closed.
    let mut almanac: Option<Almanac> = None;
    // The planning turn: staged interventions (preview-only) + the open
    // End-of-Turn review modal.
    let mut planned = kubernation_core::state::planned::PlannedWorld::default();
    let mut plan_open = false;
    // The one mutation: a pod awaiting evict confirmation (cluster, ns, pod).
    let mut pending_evict: Option<(ClusterId, String, String)> = None;
    // End-of-Turn commit awaiting confirmation.
    let mut pending_commit = false;
    // Intro splash: hold the full Kubernation scene a few moments on launch.
    let mut splash_start: Option<f64> = None;
    let mut splash_skipped = false;
    let mut splash_frames: u32 = 0;
    const SPLASH_SECS: f64 = 2.4;

    loop {
        let snap = net.snapshot();
        let status = net.status();
        let mouse = Vec2::from(mouse_position());
        let had_snap = prev_had_snap;
        prev_had_snap = snap.is_some();

        // ---- intro splash -------------------------------------------------
        // Give the full Kubernation scene a few moments on launch (it would
        // otherwise vanish the instant the world syncs). Fades in, drifts a
        // slow zoom, fades out; any key / click skips it. Suppressed for
        // headless captures unless `--splash` asks to hold (and shoot) it.
        let now = get_time();
        if splash_start.is_none() {
            splash_start = Some(now);
        }
        let elapsed = now - splash_start.unwrap_or(now);
        let splash_active =
            !splash_skipped && (args.splash || (shot.is_none() && elapsed < SPLASH_SECS));
        if splash_active {
            if is_key_pressed(KeyCode::Q) {
                break;
            }
            if is_mouse_button_pressed(MouseButton::Left)
                || is_key_pressed(KeyCode::Escape)
                || is_key_pressed(KeyCode::Enter)
                || is_key_pressed(KeyCode::Space)
            {
                splash_skipped = true;
            }
            clear_background(Color::new(0.05, 0.06, 0.09, 1.0));
            let fade_in = (elapsed / 0.5).clamp(0.0, 1.0) as f32;
            let fade_out = if args.splash {
                1.0
            } else {
                ((SPLASH_SECS - elapsed) / 0.5).clamp(0.0, 1.0) as f32
            };
            let reveal = fade_in.min(fade_out);
            let zoom = 1.0 + (elapsed.min(6.0) as f32) * 0.022;
            let cx = screen_width() / 2.0;
            let cy = screen_height() / 2.0;
            logo::draw_full(
                vec2(cx, cy - 16.0),
                (screen_height() * 0.6).min(500.0) * zoom,
            );
            // Fade veil (black → clear → black).
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                Color::new(0.05, 0.06, 0.09, 1.0 - reveal),
            );
            if reveal > 0.4 {
                let st = ascii(&status);
                let sm = text_size(&st, 20.0);
                text(&st, cx - sm.width / 2.0, cy + 232.0, 20.0, PARCHMENT);
                let hint = "press any key";
                let hm = text_size(hint, 14.0);
                text(hint, cx - hm.width / 2.0, cy + 256.0, 14.0, DIM);
            }
            splash_frames += 1;
            if let Some(path) = &shot
                && args.splash
                && splash_frames > 30
            {
                get_screen_data().export_png(&path.to_string_lossy());
                break;
            }
            next_frame().await;
            continue;
        }

        // Context list for the picker (from the hot world's kubeconfig).
        let contexts: Vec<String> = snap
            .as_ref()
            .map(|s| s.hot.observed.meta.all_contexts.clone())
            .unwrap_or_default();
        let current_ctx = snap
            .as_ref()
            .map(|s| s.hot.observed.meta.context.clone())
            .unwrap_or_default();
        // Namespace list for the filter picker: a synthetic "all namespaces"
        // row, then every namespace the hot world holds.
        let ns_filter_now = net.namespace_filter();
        let mut ns_items: Vec<String> = vec!["all namespaces".to_string()];
        if let Some(s) = snap.as_ref() {
            ns_items.extend(s.hot.observed.namespaces());
        }
        // Every drill-down (city or node) is a centered modal window: it
        // suspends map nav like the picker.
        let panel_modal = panel.is_some();
        // Track a panel opened by *this frame's* click so the window doesn't
        // read that same click as a click-outside dismiss.
        let mut panel_just_opened = false;
        let mut plan_just_opened = false;
        // Track an evict / commit confirm opened *this frame* so the opening
        // click can't also hit the confirm's buttons.
        let mut evict_just_opened = false;
        let mut commit_just_opened = false;
        // Track a context / namespace picker opened by *this frame's* menu click
        // — the picker draws same-frame and would otherwise read that click as a
        // row select if its rows ever overlapped the dropdown (e.g. on resize).
        let mut picker_just_opened = false;
        let mut ns_picker_just_opened = false;

        // ---- input ------------------------------------------------------
        // A minimap drag ends the moment the button is up — checked here,
        // outside the modal-suspended nav block, so opening a modal mid-drag
        // (which suspends that block) can't latch the flag into the next click.
        if !is_mouse_button_down(MouseButton::Left) {
            minimap_drag = false;
        }
        // While typing into the log filter or the city image field, single-key
        // shortcuts are text, not commands.
        let typing = (log_open && log_filter_active) || city_image_edit.is_some();
        if is_key_pressed(KeyCode::Q) && !typing {
            break;
        }
        // ?, /, or F1 toggle the Almanac (in-app reference). Track an open
        // *this frame* so the same click/press doesn't immediately dismiss it.
        // When a log overlay or a text editor is open, `/` is text instead.
        let mut almanac_just_opened = false;
        if (is_key_pressed(KeyCode::F1) || is_key_pressed(KeyCode::Slash)) && !log_open && !typing {
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
            && !ns_picker
        {
            plan_open = !plan_open;
            plan_just_opened = plan_open;
        }
        if is_key_pressed(KeyCode::Escape) {
            if pending_commit {
                pending_commit = false;
            } else if pending_evict.is_some() {
                pending_evict = None;
            } else if almanac.is_some() {
                almanac = None;
            } else if plan_open {
                plan_open = false;
            } else if ns_picker {
                ns_picker = false;
            } else if picker {
                picker = false;
            } else if log_open && log_filter_active {
                // First Esc leaves the filter editor; a second closes the log.
                log_filter_active = false;
            } else if log_open {
                log_open = false;
                net.clear_logs();
            } else if city_image_edit.is_some() {
                // First Esc leaves the image editor; a second closes the window.
                city_image_edit = None;
            } else if open_menu.is_some() {
                // Esc dismisses an open dropdown before it can quit the app.
                open_menu = None;
            } else if panel.is_some() {
                panel = None;
            } else {
                break;
            }
        }
        // Log overlay owns its keys: `/` edits a filter, `p` toggles previous.
        if log_open {
            if log_filter_active {
                while let Some(c) = get_char_pressed() {
                    if !c.is_control() {
                        log_filter.push(c);
                    }
                }
                if is_key_pressed(KeyCode::Backspace) {
                    log_filter.pop();
                }
                if is_key_pressed(KeyCode::Enter) {
                    log_filter_active = false;
                }
            } else {
                // Drain any stray typed chars so the queue is empty when the
                // editor opens next frame (no leading `/`).
                while get_char_pressed().is_some() {}
                if is_key_pressed(KeyCode::Slash) {
                    log_filter_active = true;
                }
                if is_key_pressed(KeyCode::P) {
                    // Drive the re-fetch off the live request (set the instant
                    // the overlay opened), not the tail (None until a fetch
                    // lands) — and flip the flag only when we actually re-issue,
                    // so the title can never run ahead of the fetched container.
                    if let Some(mut r) = net.log_request() {
                        log_previous = !log_previous;
                        r.previous = log_previous;
                        net.request_logs(r);
                    }
                }
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
        // The namespace-filter picker: row 0 = all namespaces, else focus one.
        if ns_picker {
            let n = ns_items.len();
            if n > 0 {
                if is_key_pressed(KeyCode::J) || is_key_pressed(KeyCode::Down) {
                    ns_picker_idx = (ns_picker_idx + 1) % n;
                }
                if is_key_pressed(KeyCode::K) || is_key_pressed(KeyCode::Up) {
                    ns_picker_idx = (ns_picker_idx + n - 1) % n;
                }
                if is_key_pressed(KeyCode::Enter) && ns_picker_idx < n {
                    let f = if ns_picker_idx == 0 {
                        NamespaceFilter::All
                    } else {
                        NamespaceFilter::only(ns_items[ns_picker_idx].clone())
                    };
                    net.set_namespace_filter(f);
                    ns_picker = false;
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
        if !picker
            && !ns_picker
            && almanac.is_none()
            && !panel_modal
            && !plan_open
            && open_menu.is_none()
        {
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
            // Wheel-zoom anchors at the cursor, so only over the play area —
            // over the column/chrome/strip it would anchor on a hidden cell and
            // jolt the map sideways.
            let over_map = mouse.x < panels::map_width()
                && mouse.y > panels::CHROME_H
                && mouse.y < screen_height() - panels::STRIP_H;
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 && over_map {
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

            // Menu "Fit view" deferred from the chrome draw (bounds wasn't in
            // scope there).
            if std::mem::take(&mut pending_fit) {
                cam.fit(bounds);
            }

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
                if let Some(m) = &args.menu {
                    open_menu = match m.as_str() {
                        "game" => Some(0),
                        "view" => Some(1),
                        "orders" => Some(2),
                        "world" => Some(3),
                        "help" => Some(4),
                        _ => None,
                    };
                }
                if args.plan {
                    let w = &s.hot.models.world;
                    let mut cities = w.cities();
                    if let Some(c) = cities.next() {
                        planned.stage_scale(c.r.clone(), c.desired + 2);
                    }
                    if let Some(c) = cities.next() {
                        planned.stage_restart(c.r.clone());
                    }
                    if let Some(p) = w.continents.iter().flat_map(|c| &c.provinces).next() {
                        planned.stage_cordon(p.tile.name.clone(), true);
                    }
                    plan_open = true;
                }
            }
            if picker
                || ns_picker
                || almanac.is_some()
                || panel_modal
                || plan_open
                || open_menu.is_some()
            {
                // A modal (or an open chrome menu) is up: world navigation is
                // suspended this frame.
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

                // Minimap navigation: click or drag to recenter the main view
                // on that spot. Holding the button lets you scrub the viewport
                // box around the chart; the cursor is clamped to the frame so a
                // drag past its edge keeps tracking.
                let ml = minimap_layout(bounds);
                if is_mouse_button_pressed(MouseButton::Left) && ml.frame.contains(mouse) {
                    minimap_drag = true;
                }
                // (minimap_drag is cleared on button-up at the top of input.)
                if minimap_drag && is_mouse_button_down(MouseButton::Left) {
                    let cm = vec2(
                        mouse.x.clamp(ml.frame.x, ml.frame.x + ml.frame.w),
                        mouse.y.clamp(ml.frame.y, ml.frame.y + ml.frame.h),
                    );
                    if let Some(cell) = ml.world_cell(cm, bounds) {
                        cam.jump_to(cell);
                    }
                }
                // A map-cell inspect (left of the column, not the minimap) opens
                // a drill-down window.
                if is_mouse_button_pressed(MouseButton::Left)
                    && !ml.frame.contains(mouse)
                    && mouse.y > panels::CHROME_H
                    && mouse.x < panels::map_width()
                {
                    selected = cam.cell_at(mouse, bounds);
                    if let Some(sel) = selected {
                        panel = panel_for(&worlds, sel);
                        panel_just_opened = panel.is_some();
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
                // Splash: the full logo over the fog, status centered below.
                let cx = screen_width() / 2.0;
                let cy = screen_height() / 2.0;
                logo::draw_full(vec2(cx, cy - 30.0), (screen_height() * 0.55).min(440.0));
                let st = ascii(&status);
                let sm = text_size(&st, 24.0);
                text(&st, cx - sm.width / 2.0, cy + 210.0, 24.0, PARCHMENT);
                let fog = "the world is unexplored - fog of war";
                let fm = text_size(fog, 18.0);
                text(fog, cx - fm.width / 2.0, cy + 238.0, 18.0, DIM);
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
                    draw_world(sw.world, &wc, banner, s.pair.as_deref(), overlay);
                }
                if let Some(sel) = selected {
                    draw_selection(&cam, sel);
                }

                // The docked right column (WORLD / STATUS / SELECTION) — always
                // shown; the drill-down modals dim it behind their scrim. The
                // SELECTION box follows the clicked tile, else the hovered one.
                let ml = minimap_layout(bounds);
                let over_map = mouse.x < panels::map_width()
                    && mouse.y > panels::CHROME_H
                    && mouse.y < screen_height() - panels::STRIP_H;
                let hovered = over_map
                    .then(|| cam.cell_at(mouse, bounds))
                    .flatten()
                    .and_then(|cell| locate(&worlds, cell));
                let sidebar_sel = selected.and_then(|cell| locate(&worlds, cell)).or(hovered);
                sidebar::draw_sidebar(&worlds, &cam, s, sidebar_sel, &ns_filter_now, &ml, overlay);

                // Cartographic title cartouche over the top of the map (a
                // centered modal's scrim dims it, like the rest of the board).
                let view_sub =
                    (overlay != Overlay::Terrain).then(|| format!("{} view", overlay.label()));
                panels::draw_map_title(
                    &format!("Cluster Map — {current_ctx}"),
                    view_sub.as_deref(),
                    panels::map_width(),
                );

                // Hover tooltip over the map (not the column / chrome / strip).
                if !picker
                    && almanac.is_none()
                    && !panel_modal
                    && !plan_open
                    && open_menu.is_none()
                    && drag_anchor.is_none()
                    && !minimap_drag
                    && let Some((sw, local)) = hovered
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
                    let outcome = net.plan_outcome();
                    // A fully-applied commit: clear the turn and close.
                    if outcome
                        .as_ref()
                        .is_some_and(|o| o.applied && o.rows.iter().all(|r| r.ok))
                    {
                        planned.clear();
                        plan_open = false;
                        net.clear_plan_outcome();
                    } else {
                        let pclick = is_mouse_button_pressed(MouseButton::Left)
                            && !plan_just_opened
                            && !pending_commit;
                        let act =
                            plan::draw_plan(&planned, Some(s), outcome.as_ref(), mouse, pclick);
                        if let Some(i) = act.unstage {
                            planned.unstage(i);
                            net.clear_plan_outcome();
                        }
                        if act.commit {
                            pending_commit = true;
                            commit_just_opened = true;
                        }
                        if act.discard {
                            planned.clear();
                            plan_open = false;
                            net.clear_plan_outcome();
                        }
                        if act.close {
                            plan_open = false;
                            net.clear_plan_outcome();
                        }
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
                                &net,
                                &mut city_image_edit,
                            );
                            if let Some(iv) = act.stage {
                                planned.stage(iv);
                            }
                            if let Some(wr) = act.restart_toggle {
                                if planned.restarting(&wr) {
                                    planned.unstage_restart(&wr);
                                } else {
                                    planned.stage_restart(wr);
                                }
                            }
                            if let Some((ns, pod)) = act.log {
                                log_previous = args.log_previous;
                                log_filter = args.log_filter.clone().unwrap_or_default();
                                log_filter_active = false;
                                net.request_logs(LogReq {
                                    cluster: *cid,
                                    namespace: ns,
                                    pod,
                                    previous: log_previous,
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
                                &net,
                            );
                            if let Some(iv) = act.stage {
                                planned.stage(iv);
                            }
                            if let Some((ns, pod)) = act.log {
                                log_previous = args.log_previous;
                                log_filter = args.log_filter.clone().unwrap_or_default();
                                log_filter_active = false;
                                net.request_logs(LogReq {
                                    cluster: *nid,
                                    namespace: ns,
                                    pod,
                                    previous: log_previous,
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
                            // The minimap now lives in the always-on right
                            // column (drawn above), not a floating overlay.
                        }
                    }
                }
                if close_panel {
                    panel = None;
                    log_open = false;
                    city_image_edit = None;
                    net.clear_logs();
                }
                if log_open {
                    draw_logs(
                        &net.log_tail(),
                        &log_filter,
                        log_filter_active,
                        log_previous,
                    );
                }
                draw_attention_strip(&s.attention, paired, concern_idx, panels::map_width());
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
        logo::draw_mark(vec2(17.0, panels::CHROME_H / 2.0 - 1.0), 24.0);

        // The classic-4X dropdown menu bar — Game / View / Orders / World /
        // Help — replaces the scattered chrome buttons. It's interactive only
        // when no centered modal owns input; otherwise its titles draw inert
        // and any open dropdown is dismissed.
        let menu_live = !picker
            && !ns_picker
            && almanac.is_none()
            && !plan_open
            && panel.is_none()
            && !log_open
            && pending_evict.is_none()
            && !pending_commit;
        if !menu_live {
            open_menu = None;
        }
        let mctx = MenuCtx {
            overlay,
            staged: planned.len(),
            ns_active: ns_filter_now.is_active(),
        };
        let menu_click = is_mouse_button_pressed(MouseButton::Left) && menu_live;
        let (menu_action, bar_right) =
            menu::draw_menu_bar(42.0, mouse, menu_click, &mut open_menu, &mctx);
        match menu_action {
            Some(MenuAction::SwitchContext) => {
                // No-op when there are no contexts (an empty picker); picker_idx
                // is harmless then.
                picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                picker = !contexts.is_empty();
                picker_just_opened = picker;
            }
            Some(MenuAction::Fit) => pending_fit = true,
            Some(MenuAction::Quit) => break,
            Some(MenuAction::SetOverlay(o)) => overlay = o,
            Some(MenuAction::EndTurn) => {
                // The review draws next frame; by then the press edge is gone,
                // so the opening click can't reach it as a click-outside
                // dismiss (no plan_just_opened guard needed on this path).
                plan_open = true;
            }
            Some(MenuAction::DiscardTurn) => planned.clear(),
            Some(MenuAction::NamespaceFilter) => {
                ns_picker = true;
                ns_picker_just_opened = true;
                ns_picker_idx = match &ns_filter_now {
                    NamespaceFilter::Only(s) => s
                        .iter()
                        .next()
                        .and_then(|ns| ns_items.iter().position(|i| i == ns))
                        .unwrap_or(0),
                    _ => 0,
                };
            }
            Some(MenuAction::Almanac) => {
                almanac = Some(Almanac::new());
                almanac_just_opened = true;
            }
            None => {}
        }

        // The realm readout (context · platform · counts) right-aligned, like a
        // 4X title bar's status line. Truncated to the space left of the screen
        // edge and right of the menu bar so a long paired/error label can't
        // overdraw the rightmost menu titles on a narrow window.
        let mut st = ascii(&status);
        let mut sm = text_size(&st, 14.0);
        let avail = screen_width() - 12.0 - (bar_right + 12.0);
        if sm.width > avail && !st.is_empty() {
            let budget = ((st.chars().count() as f32) * (avail / sm.width)) as usize;
            st = panels::truncate_str(&st, budget.max(3));
            sm = text_size(&st, 14.0);
        }
        text(
            &st,
            (screen_width() - sm.width - 12.0).max(bar_right + 12.0),
            21.0,
            14.0,
            STONE_INK_DIM,
        );

        // Context picker, drawn on top of everything.
        if picker {
            let layout = panels::draw_picker(
                &contexts,
                &current_ctx,
                picker_idx,
                "SWITCH CONTEXT",
                "enter switch . j/k move . c or esc cancel",
            );
            if is_mouse_button_pressed(MouseButton::Left) && !picker_just_opened {
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
        // Namespace-filter picker. The "current" marker is the focused
        // namespace (or "all namespaces" when unfiltered).
        if ns_picker {
            let current = match &ns_filter_now {
                NamespaceFilter::Only(s) => s
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| ns_items[0].clone()),
                NamespaceFilter::All => ns_items[0].clone(),
            };
            let layout = panels::draw_picker(
                &ns_items,
                &current,
                ns_picker_idx,
                "NAMESPACE FILTER",
                "enter apply . j/k move . esc cancel",
            );
            if is_mouse_button_pressed(MouseButton::Left) && !ns_picker_just_opened {
                for (i, r) in layout.rows.iter().enumerate() {
                    if r.contains(mouse) && i < ns_items.len() {
                        let f = if i == 0 {
                            NamespaceFilter::All
                        } else {
                            NamespaceFilter::only(ns_items[i].clone())
                        };
                        net.set_namespace_filter(f);
                        ns_picker = false;
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
        // Development verification: auto-commit the staged turn (REAL apply).
        if args.plan_go && frames_synced == 20 && plan_open && !planned.is_empty() {
            net.clear_plan_outcome();
            net.request_commit(planned.interventions().to_vec());
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
            if act.yes {
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

        // Commit confirm — applies the planning turn to the cluster.
        if pending_commit {
            let cclick = is_mouse_button_pressed(MouseButton::Left) && !commit_just_opened;
            let act = draw_commit_confirm(planned.len(), mouse, cclick);
            if act.yes {
                net.clear_plan_outcome();
                net.request_commit(planned.interventions().to_vec());
                pending_commit = false;
            } else if act.cancel {
                pending_commit = false;
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
        let shot_at = if args.tail {
            240
        } else if args.plan_go {
            120
        } else {
            45
        };
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
