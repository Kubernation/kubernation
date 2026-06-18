//! The docked right column — the classic-4X right panel reframed for K8s:
//!
//!   WORLD      → the isometric minimap (overview + click-to-jump)
//!   STATUS     → cluster identity, node/pod counts, the concern rollup,
//!                the gauge source, and the active namespace filter
//!   FORWARDS   → live port-forwards (shown only when any exist), each with a
//!                stop button — the always-visible home for the tunnels
//!   SELECTION  → whatever tile is selected/hovered (4X's "moving unit" box),
//!                reusing the same lines as the hover tooltip
//!
//! Always visible; the drill-down modals dim it behind their scrim. The map
//! fills everything to the column's left.

use macroquad::prelude::*;

use kubernation_core::state::attention::{Severity, severity_counts};
use kubernation_core::state::filter::NamespaceFilter;

use crate::draw::{Camera, MinimapLayout, Overlay, SceneWorld, draw_minimap};
use crate::net::{ForwardInfo, Snapshot};
use crate::panels::{self, region_lines, truncate_str};
use crate::text::{text, text_bold};
use crate::theme::*;

/// Draw the column. `forwards` are the live port-forwards (a FORWARDS section
/// appears when non-empty); a click on a forward's stop button returns its
/// local port for the caller to stop. `interactive` is false while a modal is
/// up (the column is dimmed behind the scrim, so its stops mustn't fire).
#[allow(clippy::too_many_arguments)]
pub fn draw_sidebar(
    worlds: &[SceneWorld],
    cam: &Camera,
    snap: &Snapshot,
    sel: Option<(&SceneWorld, (u16, u16))>,
    ns_filter: &NamespaceFilter,
    ml: &MinimapLayout,
    overlay: Overlay,
    forwards: &[ForwardInfo],
    mouse: Vec2,
    click: bool,
    interactive: bool,
) -> Option<u16> {
    let mut stop: Option<u16> = None;
    let col = panels::sidebar_rect();
    stone_panel(col.x, col.y, col.w, col.h);
    let x = col.x + 14.0;
    let divider = |y: f32| {
        draw_rectangle(col.x + 8.0, y, col.w - 16.0, 1.0, STONE_SHADOW);
        draw_rectangle(col.x + 8.0, y + 1.0, col.w - 16.0, 1.0, STONE_LIGHT);
    };

    // --- WORLD ------------------------------------------------------------
    text_bold("WORLD", x, col.y + 18.0, 15.0, STONE_INK);
    draw_minimap(worlds, cam, ml, overlay); // ml is positioned inside this column

    // --- STATUS -----------------------------------------------------------
    let mut y = ml.frame.y + ml.frame.h + 14.0;
    divider(y);
    y += 16.0;
    text_bold("STATUS", x, y, 15.0, STONE_INK);
    y += 20.0;

    let meta = &snap.hot.observed.meta;
    let map = &snap.hot.models.map;
    text(
        ascii(&truncate_str(&meta.context, 28)),
        x,
        y,
        14.0,
        STONE_INK,
    );
    y += 18.0;
    text(
        ascii(&format!(
            "{} . {} nodes . {} pods",
            meta.platform.label(),
            map.total_nodes,
            map.total_pods
        )),
        x,
        y,
        13.0,
        STONE_INK_DIM,
    );
    y += 18.0;

    // Concern rollup — three colored tokens (dim when zero).
    let counts = severity_counts(&snap.attention);
    let n = |s: Severity| counts.get(&s).copied().unwrap_or(0);
    let mut tx = x;
    for (count, label, col_on) in [
        (n(Severity::Critical), "crit", STONE_CRIT),
        (n(Severity::Warning), "warn", STONE_WARN),
        (n(Severity::Info), "info", STONE_INK),
    ] {
        let token = format!("{count} {label}");
        let color = if count > 0 { col_on } else { STONE_INK_DIM };
        text(&token, tx, y, 13.0, color);
        tx += crate::text::text_size(&token, 13.0).width + 14.0;
    }
    y += 18.0;

    text(
        if map.metrics_live {
            "gauges: live usage"
        } else {
            "gauges: scheduling pressure"
        },
        x,
        y,
        13.0,
        STONE_INK_DIM,
    );
    y += 18.0;
    // Cluster usage trend (when metrics-server reports) — cpu + mem
    // sparklines self-scaled to their own 15-min peak: an at-a-glance "is the
    // realm heating up", complementing the capacity-relative node-window ones.
    let hist = snap.hot.observed.cluster_usage_history();
    if !hist.is_empty() {
        let cpu: Vec<f32> = hist.iter().map(|u| u.cpu as f32).collect();
        let mem: Vec<f32> = hist.iter().map(|u| u.mem as f32).collect();
        let cpu_max = cpu
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(f32::EPSILON);
        let mem_max = mem
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(f32::EPSILON);
        // The current value rides at the right — the sparkline is self-scaled to
        // its own peak (a trend, not a magnitude), so the readout keeps a flat
        // steady cluster from reading as "maxed out".
        let cpu_val = format!("{:.0}m", cpu.last().copied().unwrap_or(0.0) * 1000.0);
        let mem_val =
            kubernation_core::util::human_bytes(mem.last().copied().unwrap_or(0.0) as f64);
        for (lbl, series, scale, val) in [
            ("cpu", &cpu, cpu_max, cpu_val.as_str()),
            ("mem", &mem, mem_max, mem_val.as_str()),
        ] {
            text(lbl, x, y + 11.0, 11.0, STONE_INK_DIM);
            let vw = crate::text::text_size(val, 11.0).width;
            let val_x = col.x + col.w - 10.0 - vw;
            text(val, val_x, y + 11.0, 11.0, STONE_INK_DIM);
            let sx = x + 30.0;
            let sw = (val_x - 6.0 - sx).max(20.0);
            panels::draw_sparkline(Rect::new(sx, y, sw, 14.0), series, scale, STRUCT);
            y += 18.0;
        }
    }
    // The map overlay is labeled when non-default so a pressure-recolored
    // terrain (red/amber by load) isn't mistaken for node health.
    if overlay != Overlay::Terrain {
        text(format!("view: {}", overlay.label()), x, y, 13.0, STONE_WARN);
        y += 18.0;
    }
    if ns_filter.is_active() {
        text(ascii(&ns_filter.label()), x, y, 13.0, STONE_WARN);
        y += 18.0;
    }

    // --- FORWARDS (only when any are live) --------------------------------
    if !forwards.is_empty() {
        y += 6.0;
        divider(y);
        y += 16.0;
        text_bold(
            format!("FORWARDS ({})", forwards.len()),
            x,
            y,
            15.0,
            STONE_INK,
        );
        y += 20.0;
        let cap = 4;
        for f in forwards.iter().take(cap) {
            // Stop button at the column's right edge; the line shows the
            // local→pod mapping (HOT/WARM-tagged in pair mode).
            let stop_btn = Rect::new(col.x + col.w - 26.0, y - 11.0, 18.0, 15.0);
            let tag = match (snap.warm.is_some(), f.cluster) {
                (true, kubernation_core::events::ClusterId::Warm) => "W ",
                (true, _) => "H ",
                _ => "",
            };
            // Local port (what you connect to) → pod port, then the pod.
            let line = format!(
                "{tag}:{}>{} {}/{}",
                f.local_port, f.pod_port, f.namespace, f.pod
            );
            text(ascii(&truncate_str(&line, 26)), x, y, 13.0, STONE_INK);
            let on = stop_btn.contains(mouse) && interactive;
            draw_rectangle(
                stop_btn.x,
                stop_btn.y,
                stop_btn.w,
                stop_btn.h,
                if on { STONE_CRIT } else { darker(STONE, 0.85) },
            );
            draw_rectangle_lines(
                stop_btn.x, stop_btn.y, stop_btn.w, stop_btn.h, 1.0, STONE_EDGE,
            );
            text("x", stop_btn.x + 6.0, stop_btn.y + 12.0, 13.0, STONE_INK);
            if on && click {
                stop = Some(f.local_port);
            }
            y += 18.0;
        }
        if forwards.len() > cap {
            text(
                format!("+{} more (stop from a pod row)", forwards.len() - cap),
                x,
                y,
                12.0,
                STONE_INK_DIM,
            );
            y += 16.0;
        }
    }

    // --- SELECTION --------------------------------------------------------
    y += 6.0;
    divider(y);
    y += 16.0;
    text_bold("SELECTION", x, y, 15.0, STONE_INK);
    y += 20.0;
    // Compute the lines first so an empty result (e.g. open sea in a single
    // cluster) falls back to the placeholder rather than a bare header.
    let lines = sel
        .map(|(sw, local)| region_lines(sw, local, snap))
        .unwrap_or_default();
    if lines.is_empty() {
        text("click a tile to inspect", x, y, 13.0, STONE_INK_DIM);
    } else {
        let bottom = col.y + col.h - 6.0; // stop before spilling off the column
        for (content, color) in lines.into_iter().take(12) {
            if y > bottom {
                break;
            }
            text(ascii(&content), x, y, 14.0, color);
            y += 17.0;
        }
    }

    stop
}
