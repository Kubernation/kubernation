//! Chrome that floats over the world: hover tooltip, the right-hand detail
//! panels (city screen / node panel, built on demand from the pure core
//! builders), and the attention strip. Panels and tooltips are cluster-
//! aware: in pair mode everything says which world it belongs to.

use k8sciv_core::events::ClusterId;
use k8sciv_core::state::attention::Concern;
use k8sciv_core::state::model::{NodeHealth, PodState, WorkloadRef, build_city, build_node_detail};
use k8sciv_core::state::world::Region;
use k8sciv_core::util::format_age_opt;
use macroquad::prelude::*;

use crate::draw::SceneWorld;
use crate::net::Snapshot;
use crate::theme::*;

pub const STRIP_H: f32 = 64.0;
pub const CHROME_H: f32 = 32.0;
const PANEL_W: f32 = 390.0;

fn pod_color(s: PodState) -> Color {
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
    match sw.world.region_at(local.0, local.1) {
        Region::City(_, c) => {
            lines.push((c.r.name.clone(), INK));
            let gap = if c.ready < c.desired { WARN } else { DIM };
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
            if let Some(pair) = &snap.pair
                && let Some(st) = pair.state(&c.r)
            {
                lines.push((st.describe(sw.id), sync_color(st)));
            }
        }
        Region::Province(p) => {
            lines.push((p.tile.name.clone(), INK));
            let health = match p.tile.health {
                NodeHealth::Healthy => ("healthy", DIM),
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
            lines.push((format!("{}/{}", s.kind, s.name), INK));
            if s.workload.is_some() {
                lines.push(("encampment - no pods on any land".into(), WARN));
            }
        }
        Region::Island(isl) => {
            lines.push((format!("isle of {}", isl.label), INK));
        }
        Region::Ocean => {
            if !paired {
                return;
            }
            lines.push(("open sea".into(), DIM));
        }
    }
    let fs = 14.0;
    let w = lines
        .iter()
        .map(|(t, _)| measure_text(ascii(t), None, fs as u16, 1.0).width)
        .fold(0.0_f32, f32::max)
        + 16.0;
    let h = lines.len() as f32 * 17.0 + 10.0;
    let x = (mouse.x + 16.0).min(screen_width() - w - 8.0);
    let y = (mouse.y + 18.0).min(screen_height() - STRIP_H - h - 8.0);
    draw_rectangle(x, y, w, h, PLATE);
    draw_rectangle_lines(x, y, w, h, 1.5, PARCHMENT);
    for (i, (text, color)) in lines.iter().enumerate() {
        draw_text(ascii(text), x + 8.0, y + 17.0 + i as f32 * 17.0, fs, *color);
    }
}

// --- detail panels -------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    City(ClusterId, WorkloadRef),
    Node(ClusterId, String),
}

pub struct PanelLayout {
    pub frame: Rect,
    pub close: Rect,
}

pub fn panel_layout() -> PanelLayout {
    let x = screen_width() - PANEL_W - 10.0;
    let y = CHROME_H + 10.0;
    let h = screen_height() - STRIP_H - y - 10.0;
    PanelLayout {
        frame: Rect::new(x, y, PANEL_W, h),
        close: Rect::new(x + PANEL_W - 26.0, y + 6.0, 20.0, 20.0),
    }
}

fn observed_for(
    snap: &Snapshot,
    id: ClusterId,
) -> Option<&k8sciv_core::state::observed::ObservedWorld> {
    match id {
        ClusterId::Hot => Some(&snap.hot.observed),
        ClusterId::Warm => snap.warm.as_ref().map(|w| &w.observed),
    }
}

/// Draw the open panel. Content is rebuilt from the observed world every
/// frame — the builders are pure and microsecond-cheap at this scale.
pub fn draw_panel(panel: &Panel, snap: &Snapshot, pl: &PanelLayout) {
    let f = pl.frame;
    draw_rectangle(f.x, f.y, f.w, f.h, PANEL);
    draw_rectangle_lines(f.x, f.y, f.w, f.h, 2.0, PARCHMENT);
    draw_rectangle(pl.close.x, pl.close.y, pl.close.w, pl.close.h, PLATE);
    draw_text("x", pl.close.x + 6.0, pl.close.y + 15.0, 16.0, INK);

    let paired = snap.warm.is_some();
    let mut y = f.y + 26.0;
    let line = |text: &str, fs: f32, color: Color, y: &mut f32| {
        draw_text(ascii(text), f.x + 14.0, *y, fs, color);
        *y += fs * 1.25;
    };

    let id = match panel {
        Panel::City(id, _) | Panel::Node(id, _) => *id,
    };
    if paired {
        let (tag, color) = cluster_tag(id);
        draw_text(tag, f.x + f.w - 70.0, f.y + 20.0, 16.0, color);
    }
    let Some(observed) = observed_for(snap, id) else {
        line("world detached", 16.0, DIM, &mut y);
        return;
    };
    let models = match id {
        ClusterId::Hot => &snap.hot.models,
        ClusterId::Warm => &snap.warm.as_ref().unwrap().models,
    };

    match panel {
        Panel::City(_, r) => {
            let Some(city) = build_city(observed, r) else {
                line("workload is no longer observed", 16.0, DIM, &mut y);
                return;
            };
            line(&city.r.name, 22.0, INK, &mut y);
            line(
                &format!("{} {}/{}", city.r.kind, city.r.namespace, city.r.name),
                14.0,
                DIM,
                &mut y,
            );
            y += 4.0;
            let gap = if city.ready < city.desired { WARN } else { INK };
            line(
                &format!(
                    "pop {} of {} desired . {} available . {} updated",
                    city.ready, city.desired, city.available, city.updated
                ),
                15.0,
                gap,
                &mut y,
            );
            let mut rollout = format!("rollout {}", city.status);
            if !city.note.is_empty() {
                rollout.push_str(&format!(" ({})", city.note));
            }
            line(&rollout, 15.0, DIM, &mut y);
            if let Some(sev) = models.workload_severity.get(r) {
                line("needs attention", 15.0, severity_color(*sev), &mut y);
            }
            if let Some(pair) = &snap.pair
                && let Some(st) = pair.state(r)
            {
                line(
                    &format!("pair: {}", st.describe(id)),
                    15.0,
                    sync_color(st),
                    &mut y,
                );
            }
            line(
                &format!(
                    "strategy {} . age {}",
                    city.strategy,
                    format_age_opt(city.age.as_ref())
                ),
                14.0,
                DIM,
                &mut y,
            );

            y += 8.0;
            line(
                &format!("PODS ({})", city.pods.len()),
                15.0,
                PARCHMENT,
                &mut y,
            );
            let max_pods = (((f.y + f.h - y) / 18.0) as usize)
                .saturating_sub(8)
                .min(18);
            for p in city.pods.iter().take(max_pods) {
                draw_circle(f.x + 20.0, y - 4.0, 4.0, pod_color(p.state));
                let reason = if p.reason.is_empty() {
                    String::new()
                } else {
                    format!("  {}", p.reason)
                };
                draw_text(
                    ascii(&format!(
                        "{}{} . r{} . {}",
                        truncate_str(&p.name, 30),
                        reason,
                        p.restarts,
                        format_age_opt(p.age.as_ref())
                    )),
                    f.x + 30.0,
                    y,
                    13.0,
                    if p.state == PodState::Failing {
                        CRIT
                    } else {
                        INK
                    },
                );
                y += 18.0;
            }
            if city.pods.len() > max_pods {
                line(
                    &format!("+{} more", city.pods.len() - max_pods),
                    13.0,
                    DIM,
                    &mut y,
                );
            }

            if !city.owned.is_empty() {
                y += 8.0;
                line("OWNED", 15.0, PARCHMENT, &mut y);
                for o in city.owned.iter().take(6) {
                    let note = if o.note.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", o.note)
                    };
                    line(&format!("{}/{}{}", o.kind, o.name, note), 13.0, DIM, &mut y);
                }
            }

            if !city.events.is_empty() {
                y += 8.0;
                line("RECENT EVENTS", 15.0, PARCHMENT, &mut y);
                for e in city.events.iter().rev().take(5) {
                    let color = if e.warning { WARN } else { DIM };
                    line(
                        &format!(
                            "{} x{} {}",
                            e.reason,
                            e.count.max(1),
                            truncate_str(&e.message, 38)
                        ),
                        12.0,
                        color,
                        &mut y,
                    );
                }
            }
        }
        Panel::Node(_, name) => {
            let Some(detail) = build_node_detail(observed, name) else {
                line("node is no longer observed", 16.0, DIM, &mut y);
                return;
            };
            let t = &detail.tile;
            line(&t.name, 22.0, INK, &mut y);
            line(&format!("province of {}", t.zone), 14.0, PARCHMENT, &mut y);
            y += 4.0;
            let health = match t.health {
                NodeHealth::Healthy => ("healthy", INK),
                NodeHealth::Cordoned => ("cordoned", WARN),
                NodeHealth::Pressure => ("under pressure", WARN),
                NodeHealth::NotReady => ("NotReady", CRIT),
            };
            line(health.0, 15.0, health.1, &mut y);

            for (label, ratio) in [("cpu", t.cpu_ratio), ("mem", t.mem_ratio)] {
                let bw = 200.0;
                let fill = (ratio as f32).clamp(0.0, 1.0) * bw;
                let color = if ratio >= 0.9 {
                    CRIT
                } else if ratio >= 0.7 {
                    WARN
                } else {
                    Color::new(0.35, 0.60, 0.30, 1.0)
                };
                draw_text(label, f.x + 14.0, y, 13.0, DIM);
                draw_rectangle(f.x + 50.0, y - 10.0, bw, 10.0, darker(PANEL, 0.7));
                draw_rectangle(f.x + 50.0, y - 10.0, fill, 10.0, color);
                draw_text(
                    format!("{:>3.0}%", ratio * 100.0),
                    f.x + 58.0 + bw,
                    y,
                    13.0,
                    DIM,
                );
                y += 18.0;
            }
            for (k, v) in detail.info.iter().take(5) {
                line(&format!("{k} {v}"), 12.0, DIM, &mut y);
            }

            y += 8.0;
            line(
                &format!("PODS ({})", detail.pods.len()),
                15.0,
                PARCHMENT,
                &mut y,
            );
            let max_pods = (((f.y + f.h - y) / 18.0) as usize)
                .saturating_sub(1)
                .min(22);
            for p in detail.pods.iter().take(max_pods) {
                draw_circle(f.x + 20.0, y - 4.0, 4.0, pod_color(p.state));
                draw_text(
                    ascii(&format!("{}/{}", p.namespace, truncate_str(&p.name, 34))),
                    f.x + 30.0,
                    y,
                    13.0,
                    if p.state == PodState::Failing {
                        CRIT
                    } else {
                        INK
                    },
                );
                y += 18.0;
            }
            if detail.pods.len() > max_pods {
                line(
                    &format!("+{} more", detail.pods.len() - max_pods),
                    13.0,
                    DIM,
                    &mut y,
                );
            }
        }
    }
}

fn truncate_str(s: &str, max: usize) -> String {
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
    draw_rectangle(0.0, base, screen_width(), STRIP_H, PANEL);
    draw_rectangle(0.0, base, screen_width(), 2.0, PARCHMENT);
    if attention.is_empty() {
        draw_text("all quiet - no concerns", 16.0, base + 26.0, 18.0, DIM);
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
        draw_text(
            ascii(&format!("{marker}{tag}{} - {}", c.title, c.detail)),
            16.0,
            base + 20.0 + i as f32 * 19.0,
            16.0,
            severity_color(c.severity),
        );
    }
    if attention.len() > 3 {
        draw_text(
            format!("+{}", attention.len() - 3),
            screen_width() - 50.0,
            base + 20.0,
            14.0,
            DIM,
        );
    }
}
