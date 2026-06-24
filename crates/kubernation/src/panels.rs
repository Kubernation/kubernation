//! Chrome that floats over the world: the hover tooltip, the blast banner,
//! the context picker, and the shared helpers the drill-down windows reuse.
//! Everything is cluster-aware: in pair mode it says which world it belongs to.
//! (Detail drill-downs themselves live in `city.rs` / `node.rs`; the attention
//! queue now lives in the right column's ATTENTION section — see `sidebar.rs`.)

use kubernation_core::events::ClusterId;
use kubernation_core::state::cost::{self, CostBasis, NodeCost};
use kubernation_core::state::logline::{self, FilterExpr, Level};
use kubernation_core::state::model::{NodeHealth, PodState, WorkloadRef};
use kubernation_core::state::saturation::{NodeSaturation, SatLevel};
use kubernation_core::state::world::{CoastKind, Region};
use macroquad::prelude::*;

use crate::draw::{Overlay, SceneWorld};
use crate::net::{ConnState, LogTail, Snapshot};
use crate::text::{
    mono_text, mono_text_size, name_text, name_text_size, text, text_bold, text_size,
};
use crate::theme::*;

pub const CHROME_H: f32 = 32.0;
/// Width of the docked right column (the WORLD / STATUS / SELECTION sidebar,
/// after the classic-4X right panel). The map fills everything to its left.
pub const COL_W: f32 = 264.0;

/// The right column's rect (below the top chrome, full height to the bottom).
pub fn sidebar_rect() -> Rect {
    Rect::new(
        screen_width() - COL_W,
        CHROME_H,
        COL_W,
        screen_height() - CHROME_H,
    )
}

/// The play area to the left of the column (where the map lives, now full
/// height — the attention queue moved into the column's ATTENTION section).
pub fn map_width() -> f32 {
    (screen_width() - COL_W).max(0.0)
}

/// A cartographic title cartouche centered over the top of the play area —
/// classic-4X "<realm> map" labeling. `title` is the realm name (serif);
/// `subtitle` is an optional small suffix (the active map view), dimmed. A
/// small iso-diamond flourish sits at each end so it reads as a map title.
pub fn draw_map_title(title: &str, subtitle: Option<&str>, map_w: f32) {
    let fs = 21.0;
    let sub_fs = 13.0;
    let pad = 24.0;
    let sub = subtitle.unwrap_or("");
    let sw = if sub.is_empty() {
        0.0
    } else {
        text_size(sub, sub_fs).width + 12.0
    };
    // Keep the cartouche inside the play area: truncate the (serif) title to the
    // width left after padding + the subtitle, so a long context / narrow window
    // can't overdraw the right column. The realm readout does the same.
    let max_bw = (map_w - 6.0).max(60.0);
    let avail_title = (max_bw - pad * 2.0 - sw).max(0.0);
    let mut title = title.to_string();
    let mut tw = name_text_size(&title, fs).width;
    if tw > avail_title && avail_title > 0.0 {
        let budget = ((title.chars().count() as f32) * (avail_title / tw)) as usize;
        title = truncate_str(&title, budget.max(3));
        tw = name_text_size(&title, fs).width;
    }
    let bw = (tw + sw + pad * 2.0).min(max_bw);
    let bx = (map_w / 2.0 - bw / 2.0).clamp(2.0, (map_w - bw - 2.0).max(2.0));
    let by = CHROME_H + 5.0;
    let bh = 27.0;
    stone_panel(bx, by, bw, bh);

    // Iso-diamond flourishes tucked into the side padding.
    let cy = by + bh / 2.0;
    let diamond = |dx: f32| {
        let d = 4.0;
        draw_triangle(
            vec2(dx - d, cy),
            vec2(dx, cy - d),
            vec2(dx + d, cy),
            PARCHMENT,
        );
        draw_triangle(
            vec2(dx - d, cy),
            vec2(dx, cy + d),
            vec2(dx + d, cy),
            PARCHMENT,
        );
    };
    diamond(bx + 11.0);
    diamond(bx + bw - 11.0);

    let ty = by + 20.0;
    name_text(title, bx + pad, ty, fs, STONE_INK);
    if !sub.is_empty() {
        text(sub, bx + pad + tw + 12.0, ty - 2.0, sub_fs, STONE_INK_DIM);
    }
}

pub(crate) fn pod_color(s: PodState) -> Color {
    match s {
        PodState::Ok => Color::new(0.45, 0.70, 0.40, 1.0),
        PodState::Starting => Color::new(0.40, 0.75, 0.80, 1.0),
        PodState::Pending => DIM,
        PodState::Terminating => DIM,
        PodState::Failing => CRIT,
        PodState::Succeeded => Color::new(0.55, 0.55, 0.50, 1.0),
    }
}

fn cluster_tag(id: ClusterId) -> (&'static str, Color) {
    match id {
        ClusterId::Hot => ("HOT", Color::new(0.95, 0.65, 0.35, 1.0)),
        ClusterId::Warm => ("WARM", Color::new(0.55, 0.78, 0.92, 1.0)),
    }
}

// --- hover tooltip ------------------------------------------------------

/// The text lines describing whatever is at `local` in `sw` — shared by the
/// hover tooltip and the right column's SELECTION panel. Empty for open sea in
/// a single-cluster session (nothing worth saying).
pub fn region_lines(
    sw: &SceneWorld,
    local: (u16, u16),
    snap: &Snapshot,
    overlay: Overlay,
) -> Vec<(String, Color)> {
    let paired = snap.warm.is_some();
    let mut lines: Vec<(String, Color)> = Vec::new();
    if paired {
        let (tag, color) = cluster_tag(sw.id);
        lines.push((format!("{tag} {}", sw.label), color));
    }
    // This world's upkeep (for the Cost-overlay SELECTION line) — already on the snap.
    let cost = match sw.id {
        ClusterId::Hot => &snap.hot.cost,
        ClusterId::Warm => snap.warm.as_ref().map_or(&snap.hot.cost, |w| &w.cost),
    };
    if let Some((_, m)) = sw.world.coast_at(local.0, local.1) {
        // A coast marker (not a land region): the city's harbor / gate.
        let (title, what) = match m.kind {
            CoastKind::Harbor => ("harbor", format!("service {} . {}", m.name, m.detail)),
            CoastKind::Gate => ("gate", format!("ingress {} . {}", m.name, m.detail)),
        };
        lines.push((title.into(), STONE_STRUCT));
        lines.push((what, STONE_INK));
        lines.push((format!("-> {}", m.workload.name), STONE_INK_DIM));
    } else {
        match sw.world.region_at(local.0, local.1) {
            Region::City(p, c) => {
                lines.push((c.r.name.clone(), STONE_INK));
                let gap = if c.ready < c.desired {
                    STONE_WARN
                } else {
                    STONE_INK_DIM
                };
                lines.push((
                    format!(
                        "{} {} . pop {}/{}",
                        c.r.kind, c.r.namespace, c.ready, c.desired
                    ),
                    gap,
                ));
                if let Some(sev) = c.severity {
                    lines.push(("needs attention".into(), severity_on_stone(sev)));
                }
                if let Some(store) = c.storage {
                    let (txt, col) = if store.pending > 0 {
                        (
                            format!("{} PVCs . {} pending", store.claims, store.pending),
                            STONE_WARN,
                        )
                    } else {
                        (format!("{} PVCs", store.claims), STONE_STRUCT)
                    };
                    lines.push((txt, col));
                }
                if let Some(pair) = &snap.pair
                    && let Some(st) = pair.state(&c.r)
                {
                    lines.push((st.describe(sw.id), sync_on_stone(st)));
                }
                // The city sits on the tinted province — show its host node's
                // strain / upkeep too, so the distinguisher isn't lost on the settlement.
                if overlay == Overlay::Saturation {
                    lines.extend(saturation_lines(&p.tile.saturation));
                }
                if overlay == Overlay::Cost
                    && let Some(nc) = cost.by_node.get(&p.tile.name)
                {
                    lines.extend(cost_lines(nc));
                }
            }
            Region::Province(p) => {
                lines.push((p.tile.name.clone(), STONE_INK));
                let health = match p.tile.health {
                    NodeHealth::Healthy => ("healthy", STONE_INK_DIM),
                    NodeHealth::Cordoned => ("cordoned", STONE_WARN),
                    NodeHealth::Pressure => ("under pressure", STONE_WARN),
                    NodeHealth::NotReady => ("NotReady", STONE_CRIT),
                };
                lines.push((
                    format!("{} . {} pods", health.0, p.tile.pods.len()),
                    health.1,
                ));
                // Under the Saturation overlay, name the binding strain
                // dimension(s) — the distinguisher the Pressure overlay lacks.
                if overlay == Overlay::Saturation {
                    lines.extend(saturation_lines(&p.tile.saturation));
                }
                // Under the Cost overlay, name the node's upkeep + idle drain.
                if overlay == Overlay::Cost
                    && let Some(nc) = cost.by_node.get(&p.tile.name)
                {
                    lines.extend(cost_lines(nc));
                }
            }
            Region::Structure(_, s) => {
                lines.push((format!("{}/{}", s.kind, s.name), STONE_INK));
                if s.workload.is_some() {
                    lines.push(("encampment - no pods on any land".into(), STONE_WARN));
                }
            }
            Region::Island(isl) => {
                lines.push((format!("isle of {}", isl.label), STONE_INK));
            }
            Region::Ocean => {
                if !paired {
                    return Vec::new();
                }
                lines.push(("open sea".into(), STONE_INK_DIM));
            }
        }
    }
    lines
}

/// PURE draw-decision fn: the per-dimension saturation breakdown for a province
/// — the strain dimensions that are non-calm (worst first), each named + colored
/// by its own level on the stone column. A fully-calm node yields one "calm"
/// line. Unit-tested (the testability policy). Conditions render "(pegged)"; an
/// omitted dimension (no honest source) simply isn't in `sat.dims`.
/// SELECTION/tooltip lines for a node's upkeep, shown under the Cost overlay.
/// PURE + unit-tested. Unitless shows "cost units" (no `$`); the idle line is the
/// actionable bit (on-stone cyan when notable, matching the map's idle coin).
pub fn cost_lines(nc: &NodeCost) -> Vec<(String, Color)> {
    if !nc.priced {
        return vec![("upkeep: unpriced".into(), STONE_INK_DIM)];
    }
    let idle = 1.0 - nc.used_frac;
    let idle_col = if idle >= cost::IDLE_NOTABLE {
        STONE_STRUCT
    } else {
        STONE_INK_DIM
    };
    let mut lines = vec![
        (
            format!("upkeep: {}", cost::fmt_monthly(nc.per_hour, nc.mode)),
            STONE_INK,
        ),
        (
            format!(
                "idle {:.0}% · {}",
                idle * 100.0,
                cost::fmt_monthly(nc.idle_per_hour, nc.mode)
            ),
            idle_col,
        ),
    ];
    if nc.basis == CostBasis::OpenCost {
        lines.push(("(from OpenCost)".into(), STONE_STRUCT));
    } else {
        if nc.basis == CostBasis::Requests {
            lines.push(("(idle est. from requests)".into(), STONE_INK_DIM));
        }
        // The only on-map $ figure carries the same honesty caveat the advisor does.
        if nc.mode == cost::CostMode::Currency {
            lines.push(("(est., not a cloud bill)".into(), STONE_INK_DIM));
        }
    }
    lines
}

pub fn saturation_lines(sat: &NodeSaturation) -> Vec<(String, Color)> {
    let ink = |l: SatLevel| match l {
        SatLevel::Calm => STONE_INK_DIM,
        SatLevel::Elevated => STONE_WARN,
        SatLevel::High => STONE_CRIT,
    };
    let mut strained: Vec<_> = sat
        .dims
        .iter()
        .filter(|d| d.level > SatLevel::Calm)
        .collect();
    strained.sort_by_key(|d| std::cmp::Reverse(d.level));
    if strained.is_empty() {
        return vec![("strain: calm".into(), STONE_INK_DIM)];
    }
    let mut lines = vec![("strain:".into(), ink(sat.worst_level()))];
    for d in strained {
        lines.push((format!("  {}", d.label), ink(d.level)));
    }
    lines
}

pub fn draw_tooltip(
    sw: &SceneWorld,
    local: (u16, u16),
    snap: &Snapshot,
    overlay: Overlay,
    mouse: Vec2,
) {
    let lines = region_lines(sw, local, snap, overlay);
    if lines.is_empty() {
        return;
    }
    let fs = 14.0;
    let w = lines
        .iter()
        .map(|(t, _)| text_size(ascii(t), fs).width)
        .fold(0.0_f32, f32::max)
        + 16.0;
    let h = lines.len() as f32 * 17.0 + 10.0;
    let x = (mouse.x + 16.0).min(screen_width() - w - 8.0);
    let y = (mouse.y + 18.0).min(screen_height() - h - 8.0);
    stone_panel(x, y, w, h);
    for (i, (content, color)) in lines.iter().enumerate() {
        text(
            ascii(content),
            x + 8.0,
            y + 17.0 + i as f32 * 17.0,
            fs,
            *color,
        );
    }
}

// --- detail panels -------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    City(ClusterId, WorkloadRef),
    Node(ClusterId, String),
}

pub(crate) fn observed_for(
    snap: &Snapshot,
    id: ClusterId,
) -> Option<&kubernation_core::state::observed::ObservedWorld> {
    match id {
        ClusterId::Hot => Some(&snap.hot.observed),
        ClusterId::Warm => snap.warm.as_ref().map(|w| &w.observed),
    }
}

// --- log tail overlay -----------------------------------------------------

/// A centered scrollback panel showing the tail of one pod's logs. The net
/// thread keeps `tail` fresh on a ~2s poll; this just paints the latest.
/// `filter` narrows the shown lines (terms AND; `!term` excludes); `previous`
/// reflects the `--previous` toggle, `filter_active` the live filter editor,
/// `timestamps`/`window` the ts and history-window state (for the title; the
/// fetched lines already carry inline timestamps when on).
#[allow(clippy::too_many_arguments)]
pub fn draw_logs(
    tail: &LogTail,
    filter: &str,
    filter_active: bool,
    previous: bool,
    timestamps: bool,
    window: kubernation_core::k8s::logs::LogWindow,
    // The pod's containers (for the in-overlay picker; a tab row shows only when
    // there's more than one) and the active container name. Returns the clicked
    // container, if any, so the caller can re-issue the tail.
    containers: &[String],
    active: Option<&str>,
    // Scrollback: when `follow`, pin to the tail; else `scroll` is the top
    // visible line. Clamped here against the fetched/filtered length and
    // written back so the caller's state stays in range.
    scroll: &mut usize,
    follow: &mut bool,
) -> Option<String> {
    let w = (screen_width() * 0.72).min(940.0);
    let h = (screen_height() - CHROME_H - 40.0).max(200.0);
    let x = (screen_width() - w) / 2.0;
    let y = CHROME_H + 20.0;
    draw_rectangle(x, y, w, h, Color::new(0.06, 0.07, 0.09, 0.97));
    draw_rectangle_lines(x, y, w, h, 2.0, PARCHMENT);

    let title = match &tail.target {
        Some(t) => {
            let tag = if t.cluster == ClusterId::Warm {
                "WARM "
            } else {
                ""
            };
            let prev = if previous { " <previous>" } else { "" };
            let win = if window == kubernation_core::k8s::logs::LogWindow::default() {
                String::new()
            } else {
                format!(" [{}]", window.label())
            };
            let ts = if timestamps { " (ts)" } else { "" };
            format!("logs · {tag}{}/{}{prev}{win}{ts}", t.namespace, t.pod)
        }
        None => "logs".into(),
    };
    text_bold(ascii(&title), x + 14.0, y + 22.0, 16.0, PARCHMENT);
    text(
        "Esc · / filter · p prev · T ts · s window · j/k/g scroll · f follow · c copy · w export",
        x + 14.0,
        y + 40.0,
        12.0,
        DIM,
    );
    draw_line(x, y + 48.0, x + w, y + 48.0, 1.0, darker(PARCHMENT, 0.5));

    // Container picker: a tab row, shown only for a multi-container pod. Drawn
    // before the early returns so a tab click is honoured even while waiting/erroring.
    let mut clicked: Option<String> = None;
    let mut picker_h = 0.0;
    if containers.len() > 1 {
        picker_h = 24.0;
        let ty = y + 52.0;
        let (mx, my) = mouse_position();
        let pressed = is_mouse_button_pressed(MouseButton::Left);
        let mut tx = x + 14.0;
        for name in containers {
            let label = ascii(name);
            let tw = text_size(&label, 12.0).width + 16.0;
            let r = Rect::new(tx, ty, tw, 18.0);
            let hover = r.contains(vec2(mx, my));
            let is_active = active == Some(name.as_str());
            let bg = if is_active {
                PARCHMENT
            } else if hover {
                Color::new(0.18, 0.20, 0.24, 1.0)
            } else {
                Color::new(0.10, 0.12, 0.14, 1.0)
            };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            let fg = if is_active {
                Color::new(0.06, 0.07, 0.09, 1.0)
            } else {
                PARCHMENT
            };
            text(&label, tx + 8.0, ty + 13.0, 12.0, fg);
            if pressed && hover {
                clicked = Some(name.clone());
            }
            tx += tw + 6.0;
        }
    }

    let body_top = y + 64.0 + picker_h;
    let line_h = 15.0;
    // Inner width available to a body line (left + right margin off the panel).
    let body_w = w - 28.0;
    if let Some(err) = &tail.error {
        text(
            ascii(&fit_width(&format!("error: {err}"), 14.0, body_w)),
            x + 14.0,
            body_top,
            14.0,
            CRIT,
        );
        return clicked;
    }
    if tail.text.is_empty() {
        text("(waiting for log lines…)", x + 14.0, body_top, 14.0, DIM);
        return clicked;
    }

    // Apply the filter expression (space-separated AND; `!term` excludes).
    let expr = FilterExpr::parse(filter);
    let total = tail.text.lines().count();
    let all: Vec<&str> = if expr.is_empty() {
        tail.text.lines().collect()
    } else {
        tail.text.lines().filter(|l| expr.matches(l)).collect()
    };

    // The live filter editor / active-filter summary, on the right of the
    // subtitle row.
    if filter_active {
        text(
            ascii(&format!("filter: {filter}_")),
            x + w - 320.0,
            y + 40.0,
            13.0,
            PARCHMENT,
        );
    } else if !filter.is_empty() {
        text(
            ascii(&format!("filter: {filter}  ({}/{total})", all.len())),
            x + w - 320.0,
            y + 40.0,
            12.0,
            DIM,
        );
    }

    if all.is_empty() {
        text(
            ascii(&fit_width(
                &format!("(no lines match \"{filter}\")"),
                14.0,
                body_w,
            )),
            x + 14.0,
            body_top,
            14.0,
            DIM,
        );
        return clicked;
    }

    // Window: `follow` pins to the tail (newest), else `scroll` is the top
    // visible line — clamped against the fitted row count and written back.
    let rows = (((y + h - 12.0) - body_top) / line_h).floor().max(1.0) as usize;
    let max_top = all.len().saturating_sub(rows);
    if *follow {
        *scroll = max_top;
    } else {
        *scroll = (*scroll).min(max_top);
    }
    let start = *scroll;
    let end = (start + rows).min(all.len());
    let plain = Color::new(0.80, 0.84, 0.80, 1.0);
    let mut ly = body_top;
    for raw in &all[start..end] {
        // Bound to the panel width (no clipping in macroquad) — monospace so
        // timestamps + columns line up the way logs read.
        let s = fit_width_mono(raw, 13.0, body_w);
        // Tint by guessed severity so an error stands out (text unchanged).
        let color = match logline::classify(raw) {
            Level::Error => CRIT,
            Level::Warn => WARN,
            Level::Debug => DIM,
            Level::Info | Level::Plain => plain,
        };
        mono_text(ascii(&s), x + 14.0, ly, 13.0, color);
        ly += line_h;
    }
    // Position readout: hidden-above count, or "following" at the tail.
    let pos = if start > 0 {
        format!("↑ {start} earlier")
    } else if *follow {
        "following".to_string()
    } else {
        String::new()
    };
    if !pos.is_empty() {
        text(ascii(&pos), x + w - 170.0, y + 22.0, 12.0, DIM);
    }
    clicked
}

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}~")
    }
}

/// Truncate `s` (appending `…`) to fit within `max_w` pixels at font `size`.
/// macroquad has no clipping, so a long unbroken line would otherwise run past
/// the panel edge; a char-count cap can't bound a proportional font. Binary
/// searches the longest char prefix that fits.
pub(crate) fn fit_width(s: &str, size: f32, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if text_size(s, size).width <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2); // upper-biased so it makes progress
        let mut cand: String = chars[..mid].iter().collect();
        cand.push('…');
        if text_size(&cand, size).width <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut out: String = chars[..lo].iter().collect();
    out.push('…');
    out
}

/// `fit_width` for the fixed-width log face: every glyph has the same advance,
/// so the fit is a single char-width division — no binary search.
pub(crate) fn fit_width_mono(s: &str, size: f32, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if mono_text_size(s, size).width <= max_w {
        return s.to_string();
    }
    let cw = mono_text_size("M", size).width.max(1.0);
    let max_chars = (max_w / cw).floor() as usize;
    if max_chars <= 1 {
        return "…".into();
    }
    let cut: String = s.chars().take(max_chars - 1).collect();
    format!("{cut}…")
}

// --- evict confirm ------------------------------------------------------

#[derive(Default)]
pub struct Confirm {
    pub yes: bool,
    pub cancel: bool,
}

/// A destructive-action confirm modal for pod eviction (the app's only write).
/// `tag` is "" or "WARM " in pair mode. Returns which button was clicked.
pub fn draw_evict_confirm(tag: &str, ns: &str, pod: &str, mouse: Vec2, click: bool) -> Confirm {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    let w = 480.0;
    let h = 158.0;
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    stone_panel(x, y, w, h);
    text_bold("Evict pod?", x + 16.0, y + 28.0, 18.0, CRIT);
    text(
        ascii(&format!("{tag}{ns}/{pod}")),
        x + 16.0,
        y + 52.0,
        14.0,
        STONE_INK,
    );
    text(
        "Deletes the pod from the cluster now.",
        x + 16.0,
        y + 72.0,
        13.0,
        STONE_INK_DIM,
    );
    text(
        "A managed pod is recreated by its controller; a bare pod is gone.",
        x + 16.0,
        y + 89.0,
        12.0,
        STONE_INK_DIM,
    );

    let bh = 28.0;
    let by = y + h - bh - 12.0;
    let cancel = Rect::new(x + 16.0, by, 150.0, bh);
    let evict = Rect::new(x + w - 166.0, by, 150.0, bh);
    let cbg = if cancel.contains(mouse) {
        lighter(STONE_DARK, 1.4)
    } else {
        STONE_DARK
    };
    draw_rectangle(cancel.x, cancel.y, cancel.w, cancel.h, cbg);
    draw_rectangle_lines(cancel.x, cancel.y, cancel.w, cancel.h, 1.0, STONE_EDGE);
    let cm = text_size("Cancel", 15.0);
    text(
        "Cancel",
        cancel.x + (cancel.w - cm.width) / 2.0,
        by + 19.0,
        15.0,
        STONE_LIGHT,
    );
    let ebg = if evict.contains(mouse) {
        CRIT
    } else {
        darker(CRIT, 0.8)
    };
    draw_rectangle(evict.x, evict.y, evict.w, evict.h, ebg);
    draw_rectangle_lines(evict.x, evict.y, evict.w, evict.h, 1.0, CRIT);
    let em = text_size("Evict", 15.0);
    text(
        "Evict",
        evict.x + (evict.w - em.width) / 2.0,
        by + 19.0,
        15.0,
        INK,
    );
    Confirm {
        yes: click && evict.contains(mouse),
        cancel: click && cancel.contains(mouse),
    }
}

/// Confirm modal for committing the planning turn (applies N changes to the
/// cluster). Returns (commit, cancel).
pub fn draw_commit_confirm(n: usize, mouse: Vec2, click: bool) -> Confirm {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    let w = 480.0;
    let h = 150.0;
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    stone_panel(x, y, w, h);
    text_bold("Commit the turn?", x + 16.0, y + 28.0, 18.0, WARN);
    text(
        format!("Apply {n} staged change(s) to the cluster."),
        x + 16.0,
        y + 54.0,
        14.0,
        STONE_INK,
    );
    text(
        "Each is dry-run validated first; anything rejected is blocked.",
        x + 16.0,
        y + 74.0,
        12.0,
        STONE_INK_DIM,
    );
    let bh = 28.0;
    let by = y + h - bh - 12.0;
    let cancel = Rect::new(x + 16.0, by, 150.0, bh);
    let commit = Rect::new(x + w - 166.0, by, 150.0, bh);
    let cbg = if cancel.contains(mouse) {
        lighter(STONE_DARK, 1.4)
    } else {
        STONE_DARK
    };
    draw_rectangle(cancel.x, cancel.y, cancel.w, cancel.h, cbg);
    draw_rectangle_lines(cancel.x, cancel.y, cancel.w, cancel.h, 1.0, STONE_EDGE);
    let cm = text_size("Cancel", 15.0);
    text(
        "Cancel",
        cancel.x + (cancel.w - cm.width) / 2.0,
        by + 19.0,
        15.0,
        STONE_LIGHT,
    );
    let ebg = if commit.contains(mouse) {
        WARN
    } else {
        darker(WARN, 0.8)
    };
    draw_rectangle(commit.x, commit.y, commit.w, commit.h, ebg);
    draw_rectangle_lines(commit.x, commit.y, commit.w, commit.h, 1.0, WARN);
    let em = text_size("Commit", 15.0);
    text(
        "Commit",
        commit.x + (commit.w - em.width) / 2.0,
        by + 19.0,
        15.0,
        PLATE,
    );
    Confirm {
        yes: click && commit.contains(mouse),
        cancel: click && cancel.contains(mouse),
    }
}

/// Confirm modal for a chaos drill (a real, deliberate failure injection).
/// CRIT-red, blunt copy: `title` names the drill, `line1` the concrete effect,
/// `line2` the blast/impact, `action` the button label. Returns (yes, cancel).
pub fn draw_chaos_confirm(
    title: &str,
    line1: &str,
    line2: &str,
    action: &str,
    mouse: Vec2,
    click: bool,
) -> Confirm {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    let w = 520.0;
    let h = 158.0;
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    stone_panel(x, y, w, h);
    text_bold(ascii(title), x + 16.0, y + 28.0, 18.0, CRIT);
    text(ascii(line1), x + 16.0, y + 54.0, 14.0, STONE_INK);
    text(ascii(line2), x + 16.0, y + 74.0, 13.0, STONE_INK_DIM);
    text(
        "A real action on the live cluster.",
        x + 16.0,
        y + 91.0,
        12.0,
        STONE_INK_DIM,
    );
    let bh = 28.0;
    let by = y + h - bh - 12.0;
    let cancel = Rect::new(x + 16.0, by, 170.0, bh);
    let run = Rect::new(x + w - 186.0, by, 170.0, bh);
    let cbg = if cancel.contains(mouse) {
        lighter(STONE_DARK, 1.4)
    } else {
        STONE_DARK
    };
    draw_rectangle(cancel.x, cancel.y, cancel.w, cancel.h, cbg);
    draw_rectangle_lines(cancel.x, cancel.y, cancel.w, cancel.h, 1.0, STONE_EDGE);
    let cm = text_size("Cancel", 15.0);
    text(
        "Cancel",
        cancel.x + (cancel.w - cm.width) / 2.0,
        by + 19.0,
        15.0,
        STONE_LIGHT,
    );
    let rbg = if run.contains(mouse) {
        CRIT
    } else {
        darker(CRIT, 0.8)
    };
    draw_rectangle(run.x, run.y, run.w, run.h, rbg);
    draw_rectangle_lines(run.x, run.y, run.w, run.h, 1.0, CRIT);
    let rm = text_size(action, 15.0);
    text(
        action,
        run.x + (run.w - rm.width) / 2.0,
        by + 19.0,
        15.0,
        INK,
    );
    Confirm {
        yes: click && run.contains(mouse),
        cancel: click && cancel.contains(mouse),
    }
}

// --- context picker -----------------------------------------------------

pub struct PickerLayout {
    pub rows: Vec<Rect>,
}

/// Modal single-select list; the dot marks the active item, the highlight bar
/// the keyboard cursor. `title`/`hint` chrome it (so the same widget serves the
/// context switcher and the namespace filter). Returns row rects for click hits.
pub fn draw_picker(
    items: &[String],
    current: &str,
    idx: usize,
    title: &str,
    hint: &str,
) -> PickerLayout {
    let contexts = items;
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.45),
    );
    let w = 480.0_f32;
    let row_h = 26.0;
    let h = 58.0 + contexts.len().max(1) as f32 * row_h;
    let x = (screen_width() - w) / 2.0;
    let y = (screen_height() - h) / 2.0;
    stone_panel(x, y, w, h);
    text_bold(ascii(title), x + 16.0, y + 26.0, 18.0, STONE_INK);
    text(ascii(hint), x + 16.0, y + 45.0, 13.0, STONE_INK_DIM);
    let mut rows = Vec::new();
    let list_y = y + 58.0;
    if contexts.is_empty() {
        text(
            "no contexts in kubeconfig",
            x + 16.0,
            list_y + 18.0,
            14.0,
            STONE_INK_DIM,
        );
    }
    for (i, ctx) in contexts.iter().enumerate() {
        let ry = list_y + i as f32 * row_h;
        let r = Rect::new(x + 8.0, ry, w - 16.0, row_h);
        if i == idx {
            stone_well(r.x, r.y, r.w, r.h);
        }
        if ctx == current {
            draw_circle(
                r.x + 12.0,
                ry + 13.0,
                4.0,
                Color::new(0.45, 0.78, 0.45, 1.0),
            );
        }
        let row_ink = if i == idx { INK } else { STONE_INK };
        text(ascii(ctx), r.x + 26.0, ry + 18.0, 15.0, row_ink);
        rows.push(r);
    }
    PickerLayout { rows }
}

/// A small banner announcing the blast-radius overlay is active — the affected
/// count, or a hint when no subject resolves. Sits at the bottom-left of the
/// play area.
pub fn draw_blast_banner(affected: Option<usize>, _map_w: f32) {
    let msg = match affected {
        Some(0) => "BLAST RADIUS · nothing downstream derivable · B to clear".to_string(),
        Some(n) => format!("BLAST RADIUS · {n} affected · B to clear"),
        None => "BLAST RADIUS · select a city/node or focus a concern · B".to_string(),
    };
    let fs = 13.0;
    let bw = text_size(&msg, fs).width + 20.0;
    let bx = 6.0;
    let by = screen_height() - 26.0 - 8.0; // just above the screen bottom
    stone_panel(bx, by, bw, 22.0);
    let col = if affected.unwrap_or(0) > 0 {
        STONE_CRIT
    } else {
        STONE_INK
    };
    text(&msg, bx + 10.0, by + 15.0, fs, col);
}

/// PURE draw-decision: the connection banner text + whether it's an error (red),
/// or `None` when the API is live (no banner). Unit-tested.
pub fn conn_banner(conn: &ConnState, context: &str) -> Option<(String, bool)> {
    match conn {
        ConnState::Live => None,
        ConnState::Connecting => Some((format!("connecting to {context}…"), false)),
        ConnState::Lost(why) => Some((format!("reconnecting to {context} — {why}"), true)),
    }
}

/// Draw the connection banner (a strip just under the chrome) when not live.
pub fn draw_conn_banner(conn: &ConnState, context: &str) {
    let Some((msg, is_err)) = conn_banner(conn, context) else {
        return;
    };
    let msg = ascii(&msg);
    let fs = 13.0;
    let h = 22.0;
    let y = CHROME_H + 2.0;
    let w = map_width();
    // A semi-opaque dark strip so it reads over the map; meaning colour on top.
    let bg = if is_err {
        Color::new(0.22, 0.05, 0.05, 0.92)
    } else {
        Color::new(0.10, 0.09, 0.05, 0.90)
    };
    draw_rectangle(0.0, y, w, h, bg);
    draw_line(0.0, y + h, w, y + h, 1.0, darker(bg, 0.5));
    let col = if is_err { CRIT } else { WARN };
    text(&msg, 12.0, y + 15.0, fs, col);
}

/// A persistent CRIT banner under the chrome — used when the net thread has
/// crashed (the world is frozen). Takes precedence over the connection banner.
pub fn draw_fatal_banner(msg: &str) {
    let h = 22.0;
    let y = CHROME_H + 2.0;
    let w = map_width();
    let bg = Color::new(0.24, 0.04, 0.04, 0.96);
    draw_rectangle(0.0, y, w, h, bg);
    draw_line(0.0, y + h, w, y + h, 1.0, darker(bg, 0.5));
    text(ascii(msg), 12.0, y + 15.0, 13.0, CRIT);
}

/// Map a value series to polyline points inside `rect` — x runs oldest→newest
/// left to right, y is bottom (0) to top (`max`), each value clamped to
/// `[0, max]`. Empty series or a non-positive `max` yields no points. Pure +
/// unit-tested (the sparkline draw-decision per the GUI testability policy).
pub fn sparkline_points(values: &[f32], max: f32, rect: Rect) -> Vec<Vec2> {
    if values.is_empty() || max <= 0.0 {
        return Vec::new();
    }
    let n = values.len();
    let dx = if n > 1 {
        rect.w / (n as f32 - 1.0)
    } else {
        0.0
    };
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let t = (v / max).clamp(0.0, 1.0);
            vec2(rect.x + i as f32 * dx, rect.y + rect.h - t * rect.h)
        })
        .collect()
}

/// Draw a small trend sparkline: a faint well + top (100%/`max`) reference
/// line, the value polyline in `line`, and a dot on the latest sample. A
/// single sample renders as just the dot; an empty series draws only the well.
pub fn draw_sparkline(rect: Rect, values: &[f32], max: f32, line: Color) {
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Color::new(0.0, 0.0, 0.0, 0.28),
    );
    // A faint frame so the chart area reads even when the trace hugs the floor
    // (a near-idle node), plus a baseline + top (max) reference.
    draw_rectangle_lines(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        1.0,
        Color::new(1.0, 1.0, 1.0, 0.10),
    );
    draw_line(
        rect.x,
        rect.y + 0.5,
        rect.x + rect.w,
        rect.y + 0.5,
        1.0,
        Color::new(1.0, 1.0, 1.0, 0.12),
    );
    let pts = sparkline_points(values, max, rect);
    // A flat single-sample series still shows a short stub, not just a dot.
    if pts.len() == 1 {
        let p = pts[0];
        draw_line(rect.x, p.y, rect.x + rect.w, p.y, 1.5, line);
    }
    for w in pts.windows(2) {
        draw_line(w[0].x, w[0].y, w[1].x, w[1].y, 1.5, line);
    }
    if let Some(last) = pts.last() {
        draw_circle(last.x, last.y, 2.0, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::scene;
    use crate::net::{Snapshot, WorldSnap};
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::Models;
    use std::sync::Arc;

    #[test]
    fn conn_banner_states() {
        assert_eq!(conn_banner(&ConnState::Live, "kind"), None);
        let (t, err) = conn_banner(&ConnState::Connecting, "kind").unwrap();
        assert!(
            t.contains("connecting") && t.contains("kind") && !err,
            "{t}"
        );
        let (t, err) = conn_banner(
            &ConnState::Lost("can't reach the API server".into()),
            "prod",
        )
        .unwrap();
        assert!(t.contains("reconnecting") && t.contains("prod") && t.contains("reach") && err);
    }

    /// The tooltip / SELECTION text is pure draw-decision logic — testable
    /// without a GL context (it formats strings + picks colors, no macroquad
    /// calls). This is the Option-A pattern: every GUI view's *decisions*
    /// should live in a fn like this, asserted against a fixture world.
    #[test]
    fn region_lines_name_the_workload_under_a_city() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        let models = Arc::new(Models::build(&world));
        let (cx, cy) = {
            let c = models.world.cities().next().expect("a city was sited");
            (c.x, c.y)
        };
        let posture = kubernation_core::state::posture::posture_report(&world);
        let cost = kubernation_core::state::cost::cost_report(
            &world,
            &kubernation_core::state::cost::CostRates::default(),
        );
        let snap = Snapshot {
            hot: WorldSnap {
                models,
                observed: world,
                slo: Arc::new(std::collections::HashMap::new()),
                posture,
                cost,
                opencost_note: None,
            },
            warm: None,
            pair: None,
            attention: Arc::new(Vec::new()),
        };
        let worlds = scene(&snap);
        let lines = region_lines(&worlds[0], (cx, cy), &snap, Overlay::Terrain);
        assert!(
            lines.iter().any(|(t, _)| t.contains("web")),
            "the SELECTION/tooltip lines should name the workload: {lines:?}"
        );
    }

    #[test]
    fn saturation_lines_name_strained_dims_and_peg_conditions() {
        use kubernation_core::state::saturation::saturate_node;
        // A pod-bound + DiskPressure node: the binding dims are named, the
        // condition is "(pegged)", and calm cpu/mem are omitted.
        let sat = saturate_node(0.20, 0.30, 108, Some(110.0), &["Disk"]);
        let lines = saturation_lines(&sat);
        let joined: String = lines
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            joined.contains("pods 108/110"),
            "names the pod-slot strain: {joined}"
        );
        assert!(
            joined.contains("DiskPressure (pegged)"),
            "condition pegged, no %: {joined}"
        );
        assert!(!joined.contains("cpu"), "calm cpu omitted: {joined}");
        // High dims color CRIT.
        assert!(lines.iter().any(|(_, c)| *c == STONE_CRIT));

        // A fully-calm node yields one calm line.
        let calm = saturate_node(0.2, 0.3, 10, Some(110.0), &[]);
        let cl = saturation_lines(&calm);
        assert_eq!(cl.len(), 1);
        assert!(cl[0].0.contains("calm"));
    }

    #[test]
    fn cost_lines_selection_unitless_and_unpriced() {
        let nc = NodeCost {
            per_hour: 6.0,
            idle_per_hour: 3.0,
            used_frac: 0.5,
            priced: true,
            mode: cost::CostMode::Unitless,
            basis: CostBasis::Requests,
            overcommitted: false,
        };
        let lines = cost_lines(&nc);
        let txt: String = lines
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            txt.contains("upkeep") && txt.contains("units") && !txt.contains('$'),
            "{txt}"
        );
        assert!(txt.contains("idle 50%"), "{txt}");
        // The idle is notable (50% ≥ 40%) → on-stone cyan.
        assert!(lines.iter().any(|(_, c)| *c == STONE_STRUCT));
        // An unpriced node says so, never a false 0.
        let up = cost_lines(&NodeCost {
            priced: false,
            ..Default::default()
        });
        assert!(up[0].0.contains("unpriced"));
    }

    #[test]
    fn sparkline_points_map_values_into_the_rect() {
        let r = Rect::new(10.0, 20.0, 100.0, 40.0);
        // 0 → bottom, max → top; evenly spaced across the width.
        let pts = sparkline_points(&[0.0, 0.5, 1.0], 1.0, r);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0], vec2(10.0, 60.0)); // value 0 → bottom (y0+h)
        assert_eq!(pts[1], vec2(60.0, 40.0)); // value .5 → middle
        assert_eq!(pts[2], vec2(110.0, 20.0)); // value 1 → top (y0), right edge
        // Over-max clamps to the top, not above it.
        let over = sparkline_points(&[2.0], 1.0, r);
        assert_eq!(over[0].y, 20.0);
        // Degenerate inputs yield nothing (the draw helper then skips the line).
        assert!(sparkline_points(&[], 1.0, r).is_empty());
        assert!(sparkline_points(&[0.5], 0.0, r).is_empty());
    }
}
