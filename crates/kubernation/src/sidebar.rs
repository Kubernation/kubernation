//! The docked right column — the classic-4X right panel reframed for K8s:
//!
//!   WORLD      → the isometric minimap (overview + click-to-jump)
//!   STATUS     → cluster identity, node/pod counts, the concern rollup,
//!                the gauge source, and the active namespace filter
//!   ATTENTION  → the attention queue (relocated from the old bottom strip);
//!                click a concern to fly there + open its drill-down (= `N`)
//!   IMPACT     → (while the blast overlay is active) the navigable dependency
//!                fan-out of the troubled subject — click a row to fly to it
//!   FORWARDS   → live port-forwards (shown only when any exist), each with a
//!                stop button — the always-visible home for the tunnels
//!   SELECTION  → whatever tile is selected/hovered (4X's "moving unit" box),
//!                reusing the same lines as the hover tooltip
//!
//! Always visible; the drill-down modals dim it behind their scrim. The map
//! fills everything to the column's left (now full-height — no bottom strip).

use macroquad::prelude::*;

use std::collections::HashMap;

use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::{Concern, Severity, severity_counts};
use kubernation_core::state::blast::{Affected, BlastRadius};
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::WorkloadRef;
use kubernation_core::state::posture::{PostureReport, PostureTier};
use kubernation_core::state::world::WorldModel;

use crate::draw::{Camera, MinimapLayout, Overlay, SceneWorld, affected_cell, draw_minimap};
use crate::net::{ForwardInfo, Snapshot};
use crate::panels::{self, region_lines, truncate_str};
use crate::text::{text, text_bold};
use crate::theme::*;

/// How many concerns the docked ATTENTION section shows before "+N more". Larger
/// than the old bottom strip's 3 — the column has the room, and the user moved
/// it here to use that space. A render-time break-guard keeps it from spilling
/// the column on a short window.
const ATTN_CAP: usize = 6;

/// How many IMPACT rows the column shows before "+N more".
const IMPACT_CAP: usize = 8;

/// What a frame's interaction with the column asks the caller to do.
#[derive(Default)]
pub struct SidebarHit {
    /// A FORWARDS stop button was clicked → stop this local port.
    pub stop_forward: Option<u16>,
    /// An ATTENTION row was clicked → focus this concern (index into
    /// `snap.attention`), flying to it + opening its drill-down (same as `N`).
    pub focus_concern: Option<usize>,
    /// An IMPACT row was clicked → fly to + select this (local) cell (the
    /// affected resource's map position). Open the city if it's a workload.
    pub focus_impact: Option<(u16, u16)>,
}

/// The kind of an affected resource (4X noun).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactKind {
    City,
    Harbor,
    Gate,
}

/// One rendered IMPACT row.
#[derive(Debug, Clone)]
pub struct ImpactRow {
    pub label: String,
    pub color: Color,
    pub hop: u8,
    /// The affected workload's own severity (a route inherits its `via`'s); None
    /// when the affected thing is itself healthy (you'd lose it, but it's up).
    pub health: Option<Severity>,
    /// The affected resource's on-map cell (LOCAL coords) to fly to; None when it
    /// has no position (DaemonSet road, dropped coast marker) → not clickable.
    pub cell: Option<(u16, u16)>,
    pub clickable: bool,
}

/// What the IMPACT section renders for — the memoized blast radius + the cluster
/// it belongs to (selects the subject world + its `workload_severity`).
pub struct BlastView<'a> {
    pub radius: &'a BlastRadius,
    pub cluster: ClusterId,
}

fn sev_rank(s: Option<Severity>) -> u8 {
    match s {
        Some(Severity::Critical) => 3,
        Some(Severity::Warning) => 2,
        Some(Severity::Info) => 1,
        None => 0,
    }
}

/// Build the IMPACT rows from a (memoized) blast radius — health cross-referenced
/// against the subject cluster's `workload_severity`, cell-resolved against its
/// `WorldModel` (the same `affected_cell` the map highlight uses, so the list and
/// the flash can't disagree). **PURE** (no GL) — unit-tested. Order: hop asc,
/// then worst-health DESC within a hop (a failing dependent floats to the top of
/// its tier and survives the cap), then label for stability.
pub fn impact_rows(
    blast: &BlastRadius,
    severity: &HashMap<WorkloadRef, Severity>,
    world: &WorldModel,
    cap: usize,
) -> Vec<ImpactRow> {
    if blast.items.is_empty() {
        // Honest: no fabricated downstream edges (topology-only).
        return vec![ImpactRow {
            label: "nothing downstream derivable".into(),
            color: STONE_INK_DIM,
            hop: 0,
            health: None,
            cell: None,
            clickable: false,
        }];
    }
    let mut rows: Vec<ImpactRow> = blast
        .items
        .iter()
        .map(|it| {
            let (kind, ns, name, via, health) = match &it.item {
                Affected::Workload(wr) => (
                    ImpactKind::City,
                    &wr.namespace,
                    &wr.name,
                    None,
                    severity.get(wr).copied(),
                ),
                Affected::Service {
                    namespace,
                    name,
                    via,
                } => (
                    ImpactKind::Harbor,
                    namespace,
                    name,
                    Some(&via.name),
                    severity.get(via).copied(),
                ),
                Affected::Ingress {
                    namespace,
                    name,
                    via,
                } => (
                    ImpactKind::Gate,
                    namespace,
                    name,
                    Some(&via.name),
                    severity.get(via).copied(),
                ),
            };
            let kind_word = match kind {
                ImpactKind::City => "city",
                ImpactKind::Harbor => "harbor",
                ImpactKind::Gate => "gate",
            };
            let glyph = health
                .map(|s| format!("{} ", s.glyph()))
                .unwrap_or_default();
            let via_suffix = via.map(|v| format!(" via {v}")).unwrap_or_default();
            // Hop is front-loaded so right-truncation eats the long ns/name (and
            // the `via` tail) before the diagnostic cascade depth.
            let label = format!("{glyph}h{} {kind_word} {ns}/{name}{via_suffix}", it.hop);
            let color = health.map(severity_on_stone).unwrap_or(STONE_INK);
            let cell = affected_cell(world, &it.item);
            ImpactRow {
                label,
                color,
                hop: it.hop,
                health,
                cell,
                clickable: cell.is_some(),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        a.hop
            .cmp(&b.hop)
            .then_with(|| sev_rank(b.health).cmp(&sev_rank(a.health)))
            .then_with(|| a.label.cmp(&b.label))
    });
    if rows.len() > cap {
        let extra = rows.len() - cap;
        rows.truncate(cap);
        rows.push(ImpactRow {
            label: format!("+{extra} more"),
            color: STONE_INK_DIM,
            hop: 0,
            health: None,
            cell: None,
            clickable: false,
        });
    }
    rows
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

/// PURE: the realm-defense chip (text + tier) for the STATUS column — the glance
/// (the Posture advisor tab is the diagnosis). Unit-tested.
pub fn posture_chip(r: &PostureReport) -> (String, PostureTier) {
    let text = match r.score {
        Some(s) => format!("DEFENSE  {s}  {}", r.tier.label()),
        None => "DEFENSE  — not scanned".to_string(),
    };
    (text, r.tier)
}

/// Tier → a stone-palette colour that reads on the tan chrome (trouble pops;
/// calm stays calm — colour discipline).
fn posture_tier_stone(tier: PostureTier) -> Color {
    match tier {
        PostureTier::Fortified => darker(GOOD, 0.55),
        PostureTier::Defended => STONE_INK,
        PostureTier::Exposed => STONE_WARN,
        PostureTier::Breached => STONE_CRIT,
        PostureTier::Unscanned => STONE_INK_DIM,
    }
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
    blast: Option<&BlastView>,
    mouse: Vec2,
    click: bool,
    interactive: bool,
) -> SidebarHit {
    let mut stop: Option<u16> = None;
    let mut focus: Option<usize> = None;
    let mut focus_impact: Option<(u16, u16)> = None;
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

    // Realm-defense posture chip — the glance (the Posture advisor tab is the
    // diagnosis). Reads the memoized score (never re-scans per frame).
    let (chip, tier) = posture_chip(&snap.hot.posture);
    text(&chip, x, y, 13.0, posture_tier_stone(tier));
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

    // --- IMPACT (blast radius — only while the overlay is active) ----------
    // The navigable dependency fan-out of the troubled subject; the on-map flash
    // + banner stay visible beside it. Click a row to fly to that resource.
    if let Some(bv) = blast
        && let Some(sw) = worlds.iter().find(|w| w.id == bv.cluster)
    {
        let severity = match bv.cluster {
            ClusterId::Hot => Some(&snap.hot.models.workload_severity),
            ClusterId::Warm => snap.warm.as_ref().map(|w| &w.models.workload_severity),
        };
        if let Some(severity) = severity {
            y += 6.0;
            divider(y);
            y += 16.0;
            text_bold(
                format!("IMPACT ({})", bv.radius.len()),
                x,
                y,
                15.0,
                STONE_INK,
            );
            y += 20.0;
            // Reserve the SELECTION slot below (a short window won't starve it).
            let impact_bottom = col.y + col.h - 56.0;
            for row in impact_rows(bv.radius, severity, sw.world, IMPACT_CAP) {
                if y > impact_bottom {
                    break;
                }
                let rect = Rect::new(col.x + 6.0, y - 13.0, col.w - 12.0, 17.0);
                if row.clickable && interactive && rect.contains(mouse) {
                    draw_rectangle(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        Color::new(0.0, 0.0, 0.0, 0.06),
                    );
                }
                text(
                    ascii(&panels::fit_width(&row.label, 12.0, col.w - 20.0)),
                    x,
                    y,
                    12.0,
                    row.color,
                );
                if row.clickable && interactive && click && rect.contains(mouse) {
                    focus_impact = row.cell;
                }
                y += 16.0;
            }
        }
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
        // Reserve the always-present SELECTION slot below (the IMPACT section
        // above can push FORWARDS down on a short window).
        let fwd_bottom = col.y + col.h - 40.0;
        for f in forwards.iter().take(cap) {
            if y > fwd_bottom {
                break;
            }
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
        focus_impact,
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

    // --- IMPACT (blast list) tests ----------------------------------------
    use kubernation_core::state::blast::{Affected, BlastItem, BlastRadius, Subject, blast_radius};
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::{Models, WorkloadKind};

    fn wr(name: &str) -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: name.into(),
        }
    }
    /// An empty WorldModel (no cities) — affected_cell resolves to None, fine for
    /// tests that only exercise row content / health / ordering / caps.
    fn empty_world() -> WorldModel {
        Models::build(&fx::world().0).world
    }

    #[test]
    fn impact_rows_orders_labels_and_resolves_cells() {
        // node n1 hosts web (pod) which has a Service + Ingress → city h1 / harbor
        // h2 via web / gate h3 via web, all placed (cells resolve, clickable).
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        s.service(fx::service("demo", "web", &[("app", "web")]));
        s.ingress(fx::ingress("demo", "web-ing", "web.example", "web"));
        let m = Models::build(&world);
        let blast = blast_radius(&world, &Subject::Node("n1".into()));
        let rows = impact_rows(&blast, &m.workload_severity, &m.world, IMPACT_CAP);

        let city = rows
            .iter()
            .find(|r| r.label.contains("city demo/web"))
            .unwrap();
        assert_eq!(city.hop, 1);
        assert!(
            city.cell.is_some() && city.clickable,
            "the city is placed → navigable"
        );
        let harbor = rows.iter().find(|r| r.label.contains("harbor")).unwrap();
        assert!(harbor.label.contains("via web") && harbor.hop == 2);
        let gate = rows.iter().find(|r| r.label.contains("gate")).unwrap();
        assert_eq!(gate.hop, 3);
        // ordering is hop-ascending.
        assert!(rows.windows(2).all(|w| w[0].hop <= w[1].hop));
    }

    #[test]
    fn impact_rows_route_inherits_via_health_and_orders_by_it() {
        let world = empty_world();
        let bad = wr("api");
        let mut severity = HashMap::new();
        severity.insert(bad.clone(), Severity::Critical);
        // Two hop-1 workloads (one Critical), and a Service fronting the troubled one.
        let blast = BlastRadius {
            subject: Subject::Node("n1".into()),
            items: vec![
                BlastItem {
                    item: Affected::Workload(wr("calm")),
                    hop: 1,
                },
                BlastItem {
                    item: Affected::Workload(bad.clone()),
                    hop: 1,
                },
                BlastItem {
                    item: Affected::Service {
                        namespace: "demo".into(),
                        name: "api".into(),
                        via: bad.clone(),
                    },
                    hop: 2,
                },
            ],
        };
        let rows = impact_rows(&blast, &severity, &world, IMPACT_CAP);
        // The troubled workload floats to the top of its hop tier.
        assert!(rows[0].label.contains("api") && rows[0].health == Some(Severity::Critical));
        assert!(rows[1].label.contains("calm") && rows[1].health.is_none());
        // The Service inherits its `via` workload's Critical health (so its row
        // also carries the severity glyph, hence `contains` not `starts_with`).
        let svc = rows.iter().find(|r| r.label.contains("harbor")).unwrap();
        assert_eq!(svc.health, Some(Severity::Critical));
    }

    #[test]
    fn impact_rows_empty_is_honest() {
        let blast = BlastRadius {
            subject: Subject::Workload(wr("lonely")),
            items: vec![],
        };
        let rows = impact_rows(&blast, &HashMap::new(), &empty_world(), IMPACT_CAP);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].label.contains("nothing downstream") && !rows[0].clickable);
    }

    #[test]
    fn impact_rows_caps_with_overflow() {
        let items: Vec<BlastItem> = (0..IMPACT_CAP + 5)
            .map(|i| BlastItem {
                item: Affected::Workload(wr(&format!("w{i:02}"))),
                hop: 1,
            })
            .collect();
        let blast = BlastRadius {
            subject: Subject::Node("n1".into()),
            items,
        };
        let rows = impact_rows(&blast, &HashMap::new(), &empty_world(), IMPACT_CAP);
        assert_eq!(rows.len(), IMPACT_CAP + 1); // cap rows + one overflow
        assert!(rows.last().unwrap().label == "+5 more" && !rows.last().unwrap().clickable);
    }

    #[test]
    fn posture_chip_text_and_tier() {
        use kubernation_core::state::posture::{AxisScore, PostureReport};
        let mk = |score: Option<i32>, tier: PostureTier| PostureReport {
            score,
            tier,
            scanned: score.is_some(),
            fortifications: AxisScore::default(),
            walls: AxisScore::default(),
            workloads_total: 0,
            system_critical: 0,
            system_warning: 0,
            factors: vec![],
        };
        let (t, tier) = posture_chip(&mk(Some(34), PostureTier::Breached));
        assert!(t.contains("DEFENSE") && t.contains("34") && t.contains("BREACHED"));
        assert_eq!(tier, PostureTier::Breached);
        let (t2, tier2) = posture_chip(&mk(None, PostureTier::Unscanned));
        assert!(t2.contains("not scanned"));
        assert_eq!(tier2, PostureTier::Unscanned);
    }
}
