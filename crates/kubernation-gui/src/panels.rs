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
use crate::text::{text, text_bold, text_size};
use crate::theme::*;

pub const STRIP_H: f32 = 64.0;
pub const CHROME_H: f32 = 32.0;

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

pub fn draw_tooltip(sw: &SceneWorld, local: (u16, u16), snap: &Snapshot, mouse: Vec2) {
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
        lines.push((title.into(), STRUCT));
        lines.push((what, STONE_INK));
        lines.push((format!("-> {}", m.workload.name), STONE_INK_DIM));
    } else {
        match sw.world.region_at(local.0, local.1) {
            Region::City(_, c) => {
                lines.push((c.r.name.clone(), STONE_INK));
                let gap = if c.ready < c.desired {
                    WARN
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
                    lines.push(("needs attention".into(), severity_color(sev)));
                }
                if let Some(store) = c.storage {
                    let (txt, col) = if store.pending > 0 {
                        (
                            format!("{} PVCs . {} pending", store.claims, store.pending),
                            WARN,
                        )
                    } else {
                        (format!("{} PVCs", store.claims), STRUCT)
                    };
                    lines.push((txt, col));
                }
                if let Some(pair) = &snap.pair
                    && let Some(st) = pair.state(&c.r)
                {
                    lines.push((st.describe(sw.id), sync_color(st)));
                }
            }
            Region::Province(p) => {
                lines.push((p.tile.name.clone(), STONE_INK));
                let health = match p.tile.health {
                    NodeHealth::Healthy => ("healthy", STONE_INK_DIM),
                    NodeHealth::Cordoned => ("cordoned", WARN),
                    NodeHealth::Pressure => ("under pressure", WARN),
                    NodeHealth::NotReady => ("NotReady", CRIT),
                };
                lines.push((
                    format!("{} . {} pods", health.0, p.tile.pods.len()),
                    health.1,
                ));
            }
            Region::Structure(_, s) => {
                lines.push((format!("{}/{}", s.kind, s.name), STONE_INK));
                if s.workload.is_some() {
                    lines.push(("encampment - no pods on any land".into(), WARN));
                }
            }
            Region::Island(isl) => {
                lines.push((format!("isle of {}", isl.label), STONE_INK));
            }
            Region::Ocean => {
                if !paired {
                    return;
                }
                lines.push(("open sea".into(), STONE_INK_DIM));
            }
        }
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
pub fn draw_logs(tail: &LogTail) {
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
            format!("logs · {tag}{}/{}", t.namespace, t.pod)
        }
        None => "logs".into(),
    };
    text_bold(ascii(&title), x + 14.0, y + 22.0, 16.0, PARCHMENT);
    text(
        "Esc to close · last 500 lines · live",
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
    // Show the tail end that fits — newest lines are most useful.
    let rows = (((y + h - 12.0) - body_top) / line_h).floor().max(1.0) as usize;
    let all: Vec<&str> = tail.text.lines().collect();
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
            y + 40.0,
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

pub fn draw_attention_strip(attention: &[Concern], paired: bool, concern_idx: usize) {
    let base = screen_height() - STRIP_H;
    draw_rectangle(0.0, base, screen_width(), STRIP_H, STONE);
    draw_rectangle(0.0, base, screen_width(), 2.0, STONE_LIGHT);
    draw_rectangle(0.0, base + STRIP_H - 2.0, screen_width(), 2.0, STONE_SHADOW);
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
            severity_color(c.severity),
        );
    }
    if attention.len() > 3 {
        text(
            format!("+{}", attention.len() - 3),
            screen_width() - 50.0,
            base + 20.0,
            14.0,
            STONE_INK_DIM,
        );
    }
}

// --- context picker -----------------------------------------------------

pub struct PickerLayout {
    pub rows: Vec<Rect>,
}

/// Modal list of kubeconfig contexts; the dot marks the active one, the
/// highlight bar the keyboard cursor. Returns row rects for click hits.
pub fn draw_picker(contexts: &[String], current: &str, idx: usize) -> PickerLayout {
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
    text_bold("SWITCH CONTEXT", x + 16.0, y + 26.0, 18.0, STONE_INK);
    text(
        "enter switch . j/k move . c or esc cancel",
        x + 16.0,
        y + 45.0,
        13.0,
        STONE_INK_DIM,
    );
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
