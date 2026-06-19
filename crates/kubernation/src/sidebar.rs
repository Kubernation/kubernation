//! The docked right column — the classic-4X right panel reframed for K8s:
//!
//!   WORLD      → the isometric minimap (overview + click-to-jump)
//!   STATUS     → cluster identity, node/pod counts, the concern rollup,
//!                the gauge source, and the active namespace filter
//!   ATTENTION  → the attention queue (relocated from the old bottom strip);
//!                click a concern to fly there + open its drill-down (= `N`)
//!   FORWARDS   → live port-forwards (shown only when any exist), each with a
//!                stop button — the always-visible home for the tunnels
//!   SELECTION  → whatever tile is selected/hovered (4X's "moving unit" box),
//!                reusing the same lines as the hover tooltip
//!
//! Always visible; the drill-down modals dim it behind their scrim. The map
//! fills everything to the column's left (now full-height — no bottom strip).

use macroquad::prelude::*;

use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::{Concern, Severity, severity_counts};
use kubernation_core::state::filter::NamespaceFilter;

use crate::draw::{Camera, MinimapLayout, Overlay, SceneWorld, draw_minimap};
use crate::net::{ForwardInfo, Snapshot};
use crate::panels::{self, region_lines, truncate_str};
use crate::text::{text, text_bold};
use crate::theme::*;

/// How many concerns the docked ATTENTION section shows before "+N more". Larger
/// than the old bottom strip's 3 — the column has the room, and the user moved
/// it here to use that space. A render-time break-guard keeps it from spilling
/// the column on a short window.
const ATTN_CAP: usize = 6;

/// What a frame's interaction with the column asks the caller to do.
#[derive(Default)]
pub struct SidebarHit {
    /// A FORWARDS stop button was clicked → stop this local port.
    pub stop_forward: Option<u16>,
    /// An ATTENTION row was clicked → focus this concern (index into
    /// `snap.attention`), flying to it + opening its drill-down (same as `N`).
    pub focus_concern: Option<usize>,
}

/// The attention queue rendered as `(line, color)` rows for the column's
/// ATTENTION section — **pure** (no GL / no `screen_*`), unit-tested per the GUI
/// testability policy. Mirrors the old bottom strip's format:
/// `"{marker}{tag}{title} - {detail}"`, a `> ` marker on the focused concern
/// (`concern_idx`, wrapped), `[H]`/`[W]` tags in pair mode, colour by
/// `severity_on_stone`. Up to `cap` concern rows, then a `+N more` overflow row;
/// a single "all quiet" row when empty. The renderer truncates each line to the
/// column width (`fit_width`) and attaches the click hit-rects.
pub fn attention_rows(
    attention: &[Concern],
    paired: bool,
    concern_idx: usize,
    cap: usize,
) -> Vec<(String, Color)> {
    if attention.is_empty() {
        return vec![("all quiet - no concerns".into(), STONE_INK_DIM)];
    }
    let focus = concern_idx % attention.len();
    let mut rows: Vec<(String, Color)> = attention
        .iter()
        .take(cap)
        .enumerate()
        .map(|(i, c)| {
            let marker = if i == focus { "> " } else { "  " };
            let tag = if paired {
                match c.cluster {
                    ClusterId::Hot => "[H] ",
                    ClusterId::Warm => "[W] ",
                }
            } else {
                ""
            };
            (
                format!("{marker}{tag}{} - {}", c.title, c.detail),
                severity_on_stone(c.severity),
            )
        })
        .collect();
    if attention.len() > cap {
        rows.push((
            format!("+{} more (N to cycle)", attention.len() - cap),
            STONE_INK_DIM,
        ));
    }
    rows
}

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
    concern_idx: usize,
    forwards: &[ForwardInfo],
    mouse: Vec2,
    click: bool,
    interactive: bool,
) -> SidebarHit {
    let mut stop: Option<u16> = None;
    let mut focus: Option<usize> = None;
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

    // --- ATTENTION (the queue — relocated from the old bottom strip) -------
    // Clickable: a row does exactly what `N` does (focus + fly + open). The
    // focused concern wears the picker-cursor well; hovering a row washes it.
    y += 6.0;
    divider(y);
    y += 16.0;
    let attn = &snap.attention;
    let header = if attn.is_empty() {
        "ATTENTION".to_string()
    } else {
        format!("ATTENTION ({})", attn.len())
    };
    text_bold(header, x, y, 15.0, STONE_INK);
    y += 20.0;
    // Stop short of the column bottom, reserving a slot for the SELECTION
    // header below (so a short window can't starve "what am I looking at").
    // On a normal window the few rows never reach this, so no space is wasted.
    let attn_bottom = col.y + col.h - 46.0;
    let concern_rows = attn.len().min(ATTN_CAP); // rows mapping to a real concern
    let focus_row = concern_idx % attn.len().max(1);
    for (i, (content, color)) in attention_rows(attn, snap.warm.is_some(), concern_idx, ATTN_CAP)
        .into_iter()
        .enumerate()
    {
        if y > attn_bottom {
            break;
        }
        let clickable = i < concern_rows; // skip the empty / "+N more" line
        let rect = Rect::new(col.x + 6.0, y - 13.0, col.w - 12.0, 17.0);
        if clickable && i == focus_row {
            stone_well(rect.x, rect.y, rect.w, rect.h);
        } else if clickable && interactive && rect.contains(mouse) {
            draw_rectangle(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                Color::new(0.0, 0.0, 0.0, 0.06),
            );
        }
        text(
            ascii(&panels::fit_width(&content, 13.0, col.w - 22.0)),
            x,
            y,
            13.0,
            color,
        );
        if clickable && interactive && click && rect.contains(mouse) {
            focus = Some(i);
        }
        y += 17.0;
    }
    // Runbook hint for the focused concern: the next action / in-app verb to take.
    if !attn.is_empty()
        && y <= attn_bottom
        && let Some(hint) = kubernation_core::state::attention::next_action(&attn[focus_row])
    {
        text(
            ascii(&panels::fit_width(
                &format!("next: {hint}"),
                11.0,
                col.w - 16.0,
            )),
            x,
            y,
            11.0,
            STONE_WARN,
        );
        y += 15.0;
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

    SidebarHit {
        stop_forward: stop,
        focus_concern: focus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubernation_core::state::attention::Target;

    fn concern(sev: Severity, title: &str, detail: &str, cluster: ClusterId) -> Concern {
        Concern {
            severity: sev,
            title: title.into(),
            detail: detail.into(),
            target: Target::WorkloadList,
            probe: None,
            key: title.into(),
            cluster,
        }
    }

    #[test]
    fn attention_rows_empty_is_all_quiet() {
        let rows = attention_rows(&[], false, 0, ATTN_CAP);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "all quiet - no concerns");
        assert_eq!(rows[0].1, STONE_INK_DIM);
    }

    #[test]
    fn attention_rows_format_focus_color_and_tag() {
        let cs = [
            concern(
                Severity::Critical,
                "crashy",
                "CrashLoopBackOff",
                ClusterId::Hot,
            ),
            concern(Severity::Warning, "stuck-pvc", "Pending", ClusterId::Hot),
        ];
        let rows = attention_rows(&cs, false, 0, ATTN_CAP);
        assert_eq!(rows.len(), 2);
        // Focus marker tracks concern_idx (0 → first row).
        assert!(rows[0].0.starts_with("> "));
        assert!(rows[1].0.starts_with("  "));
        // Colour is the on-stone severity ink (the region_lines analogue).
        assert_eq!(rows[0].1, severity_on_stone(Severity::Critical));
        // The row names the concern (title + detail).
        assert!(rows[0].0.contains("crashy") && rows[0].0.contains("CrashLoopBackOff"));
        // Single-cluster → no pair tag.
        assert!(!rows[0].0.contains("[H]"));
    }

    #[test]
    fn attention_rows_pair_tag_and_overflow_and_wrap() {
        let cs: Vec<Concern> = (0..5)
            .map(|i| {
                let cl = if i == 1 {
                    ClusterId::Warm
                } else {
                    ClusterId::Hot
                };
                concern(Severity::Info, &format!("c{i}"), "detail", cl)
            })
            .collect();
        // Pair mode tags each row with its cluster.
        let paired = attention_rows(&cs, true, 0, ATTN_CAP);
        assert!(paired[0].0.contains("[H] "));
        assert!(paired[1].0.contains("[W] "));
        // cap < len → a "+N more" overflow row in dim ink.
        let capped = attention_rows(&cs, false, 0, 3);
        assert_eq!(capped.len(), 4);
        assert_eq!(
            *capped.last().unwrap(),
            ("+2 more (N to cycle)".to_string(), STONE_INK_DIM)
        );
        // concern_idx wraps modulo the queue length: idx 6 over 5 concerns
        // marks row 1 (6 % 5), shown since cap (6) covers all five.
        let wrapped = attention_rows(&cs, false, 6, ATTN_CAP);
        assert!(wrapped[1].0.starts_with("> "));
        assert!(wrapped[0].0.starts_with("  "));
    }
}
