//! The docked right column — the classic-4X right panel reframed for K8s:
//!
//!   WORLD      → the isometric minimap (overview + click-to-jump)
//!   STATUS     → cluster identity, node/pod counts, the concern rollup,
//!                the gauge source, and the active namespace filter
//!   SELECTION  → whatever tile is selected/hovered (4X's "moving unit" box),
//!                reusing the same lines as the hover tooltip
//!
//! Always visible; the drill-down modals dim it behind their scrim. The map
//! fills everything to the column's left.

use macroquad::prelude::*;

use kubernation_core::state::attention::{Severity, severity_counts};
use kubernation_core::state::filter::NamespaceFilter;

use crate::draw::{Camera, MinimapLayout, SceneWorld, draw_minimap};
use crate::net::Snapshot;
use crate::panels::{self, region_lines, truncate_str};
use crate::text::{text, text_bold};
use crate::theme::*;

pub fn draw_sidebar(
    worlds: &[SceneWorld],
    cam: &Camera,
    snap: &Snapshot,
    sel: Option<(&SceneWorld, (u16, u16))>,
    ns_filter: &NamespaceFilter,
    ml: &MinimapLayout,
) {
    let col = panels::sidebar_rect();
    stone_panel(col.x, col.y, col.w, col.h);
    let x = col.x + 14.0;
    let divider = |y: f32| {
        draw_rectangle(col.x + 8.0, y, col.w - 16.0, 1.0, STONE_SHADOW);
        draw_rectangle(col.x + 8.0, y + 1.0, col.w - 16.0, 1.0, STONE_LIGHT);
    };

    // --- WORLD ------------------------------------------------------------
    text_bold("WORLD", x, col.y + 18.0, 15.0, STONE_INK);
    draw_minimap(worlds, cam, ml); // ml is positioned inside this column

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
        (n(Severity::Critical), "crit", CRIT),
        (n(Severity::Warning), "warn", WARN),
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
    if ns_filter.is_active() {
        text(ascii(&ns_filter.label()), x, y, 13.0, WARN);
        y += 18.0;
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
}
