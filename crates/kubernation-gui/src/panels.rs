//! Chrome that floats over the world: the hover tooltip, the attention
//! strip, the context picker, and the shared helpers the drill-down windows
//! reuse. Everything is cluster-aware: in pair mode it says which world it
//! belongs to. (Detail drill-downs themselves live in `city.rs` / `node.rs`.)

use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::Concern;
use kubernation_core::state::model::{NodeHealth, PodState, WorkloadRef};
use kubernation_core::state::world::{CoastKind, Region};
use macroquad::prelude::*;

use crate::draw::SceneWorld;
use crate::net::{LogTail, Snapshot};
use crate::text::{name_text, name_text_size, text, text_bold, text_size};
use crate::theme::*;

pub const STRIP_H: f32 = 64.0;
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

/// The play area to the left of the column (where the map + attention strip
/// live).
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
    let tw = name_text_size(title, fs).width;
    let sub = subtitle.unwrap_or("");
    let sw = if sub.is_empty() {
        0.0
    } else {
        text_size(sub, sub_fs).width + 12.0
    };
    let pad = 24.0;
    let bw = tw + sw + pad * 2.0;
    let bx = (map_w / 2.0 - bw / 2.0).max(2.0);
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
pub fn region_lines(sw: &SceneWorld, local: (u16, u16), snap: &Snapshot) -> Vec<(String, Color)> {
    let paired = snap.warm.is_some();
    let mut lines: Vec<(String, Color)> = Vec::new();
    if paired {
        let (tag, color) = cluster_tag(sw.id);
        lines.push((format!("{tag} {}", sw.label), color));
    }
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
            Region::City(_, c) => {
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

pub fn draw_tooltip(sw: &SceneWorld, local: (u16, u16), snap: &Snapshot, mouse: Vec2) {
    let lines = region_lines(sw, local, snap);
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
    let y = (mouse.y + 18.0).min(screen_height() - STRIP_H - h - 8.0);
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
/// `filter` (case-insensitive substring) narrows the shown lines; `previous`
/// reflects the `--previous` toggle, `filter_active` the live filter editor.
pub fn draw_logs(tail: &LogTail, filter: &str, filter_active: bool, previous: bool) {
    let w = (screen_width() * 0.72).min(940.0);
    let h = (screen_height() - STRIP_H - CHROME_H - 40.0).max(200.0);
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
            format!("logs · {tag}{}/{}{prev}", t.namespace, t.pod)
        }
        None => "logs".into(),
    };
    text_bold(ascii(&title), x + 14.0, y + 22.0, 16.0, PARCHMENT);
    text(
        "Esc close · / filter · p previous · last 500 lines · live",
        x + 14.0,
        y + 40.0,
        12.0,
        DIM,
    );
    draw_line(x, y + 48.0, x + w, y + 48.0, 1.0, darker(PARCHMENT, 0.5));

    let body_top = y + 64.0;
    let line_h = 15.0;
    if let Some(err) = &tail.error {
        text(
            ascii(&format!("error: {err}")),
            x + 14.0,
            body_top,
            14.0,
            CRIT,
        );
        return;
    }
    if tail.text.is_empty() {
        text("(waiting for log lines…)", x + 14.0, body_top, 14.0, DIM);
        return;
    }

    // Apply the substring filter (case-insensitive) over the fetched tail.
    let needle = filter.to_lowercase();
    let total = tail.text.lines().count();
    let all: Vec<&str> = if needle.is_empty() {
        tail.text.lines().collect()
    } else {
        tail.text
            .lines()
            .filter(|l| l.to_lowercase().contains(&needle))
            .collect()
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
            ascii(&format!("(no lines match \"{filter}\")")),
            x + 14.0,
            body_top,
            14.0,
            DIM,
        );
        return;
    }

    // Show the tail end that fits — newest lines are most useful.
    let rows = (((y + h - 12.0) - body_top) / line_h).floor().max(1.0) as usize;
    let start = all.len().saturating_sub(rows);
    let mut ly = body_top;
    for raw in &all[start..] {
        let s = truncate_str(raw, 150);
        text(
            ascii(&s),
            x + 14.0,
            ly,
            13.0,
            Color::new(0.80, 0.84, 0.80, 1.0),
        );
        ly += line_h;
    }
    if start > 0 {
        text(
            ascii(&format!("… {start} earlier lines",)),
            x + w - 170.0,
            y + 22.0,
            12.0,
            DIM,
        );
    }
}

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}~")
    }
}

// --- attention strip ------------------------------------------------------

pub fn draw_attention_strip(attention: &[Concern], paired: bool, concern_idx: usize, width: f32) {
    let base = screen_height() - STRIP_H;
    draw_rectangle(0.0, base, width, STRIP_H, STONE);
    draw_rectangle(0.0, base, width, 2.0, STONE_LIGHT);
    draw_rectangle(0.0, base + STRIP_H - 2.0, width, 2.0, STONE_SHADOW);
    if attention.is_empty() {
        text(
            "all quiet - no concerns",
            16.0,
            base + 26.0,
            18.0,
            STONE_INK_DIM,
        );
        return;
    }
    for (i, c) in attention.iter().take(3).enumerate() {
        let marker = if i == concern_idx % attention.len() {
            "> "
        } else {
            "  "
        };
        let tag = if paired {
            match c.cluster {
                ClusterId::Hot => "[H] ",
                ClusterId::Warm => "[W] ",
            }
        } else {
            ""
        };
        text(
            ascii(&format!("{marker}{tag}{} - {}", c.title, c.detail)),
            16.0,
            base + 20.0 + i as f32 * 19.0,
            16.0,
            severity_on_stone(c.severity),
        );
    }
    if attention.len() > 3 {
        text(
            format!("+{}", attention.len() - 3),
            width - 44.0,
            base + 20.0,
            14.0,
            STONE_INK_DIM,
        );
    }
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
