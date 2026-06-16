//! The city drill-down — a workload rendered as a centered Civ II-style city
//! window on the shared window system. The Civ II city screen reframed for
//! Kubernetes (observe-only, so no Buy/Change):
//!
//!   title bar   →  deploy ns/name (+ HOT/WARM)
//!   status band →  replicas / updated gauges, rollout, strategy, attention
//!   citizens    →  a pod census grid + a clickable pod list (tail logs)
//!   improvements→  owned resources (svc / ingress / pvc / cm / secret)
//!   chronicle   →  recent events
//!
//! Fixed size with caps + "+N more" (Civ II's panels don't scroll).

use macroquad::prelude::*;

use kubernation_core::events::ClusterId;
use kubernation_core::state::model::{WorkloadRef, build_city};
use kubernation_core::state::planned::{Intervention, PlannedWorld};
use kubernation_core::util::format_age_opt;

use crate::net::Snapshot;
use crate::panels::{observed_for, pod_color, truncate_str};
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::{WinAction, draw_window};

const W: f32 = 920.0;
const H: f32 = 600.0;

/// Draw the city window for `r` and resolve this frame's clicks. `auto_log`
/// (headless verification) opens the first pod's logs without a click.
#[allow(clippy::too_many_arguments)]
pub fn draw_city(
    id: ClusterId,
    r: &WorkloadRef,
    snap: &Snapshot,
    planned: &PlannedWorld,
    mouse: Vec2,
    click: bool,
    auto_log: bool,
) -> WinAction {
    let mut act = WinAction::default();
    let tag = match (snap.warm.is_some(), id) {
        (true, ClusterId::Hot) => " — HOT",
        (true, ClusterId::Warm) => " — WARM",
        _ => "",
    };
    let title = format!("{} {}/{}{tag}", r.kind, r.namespace, r.name);
    let win = draw_window(&ascii(&title), vec2(W, H), &[], usize::MAX);
    let b = win.body;

    // Resolve the observed world + models for this cluster.
    let observed = match observed_for(snap, id) {
        Some(o) => o,
        None => {
            text("world detached", b.x, b.y + 18.0, 16.0, DIM);
            if click && (win.close.contains(mouse) || !win.frame.contains(mouse)) {
                act.close = true;
            }
            return act;
        }
    };
    let models = match id {
        ClusterId::Hot => &snap.hot.models,
        ClusterId::Warm => &snap.warm.as_ref().unwrap().models,
    };
    let Some(city) = build_city(observed, r) else {
        text("workload is no longer observed", b.x, b.y + 18.0, 16.0, DIM);
        if click && (win.close.contains(mouse) || !win.frame.contains(mouse)) {
            act.close = true;
        }
        return act;
    };

    // --- Status band (full width) -----------------------------------------
    let mut y = b.y + 6.0;
    let gap = if city.ready < city.desired { WARN } else { INK };
    // Two gauges side by side.
    let gw = 220.0;
    gauge(b.x, y, gw, "replicas", city.ready, city.desired, gap);
    gauge(
        b.x + gw + 40.0,
        y,
        gw,
        "updated",
        city.updated,
        city.desired,
        INK,
    );
    // Headline numbers to the right of the gauges.
    let nums = format!(
        "{} ready . {} available . {} updated  /  {} desired",
        city.ready, city.available, city.updated, city.desired
    );
    text(nums, b.x + gw * 2.0 + 90.0, y + 14.0, 14.0, gap);
    y += 38.0;

    // PLAN: stage a replica change. Preview-only — staging records intent and
    // shows the diff; nothing is written to the cluster here.
    {
        let desired = city.desired;
        let staged = planned.scaled(r);
        let shown = staged.unwrap_or(desired);
        text("plan", b.x, y + 13.0, 14.0, PARCHMENT);
        text("replicas", b.x + 44.0, y + 13.0, 13.0, DIM);
        let minus = Rect::new(b.x + 122.0, y, 20.0, 18.0);
        let plus = Rect::new(b.x + 186.0, y, 20.0, 18.0);
        for (rct, sym) in [(minus, "-"), (plus, "+")] {
            let bg = if rct.contains(mouse) {
                lighter(PLATE, 1.7)
            } else {
                PLATE
            };
            draw_rectangle(rct.x, rct.y, rct.w, rct.h, bg);
            draw_rectangle_lines(rct.x, rct.y, rct.w, rct.h, 1.0, PARCHMENT);
            text(sym, rct.x + 6.0, rct.y + 14.0, 16.0, INK);
        }
        let num_col = if staged.is_some_and(|s| s != desired) {
            WARN
        } else {
            INK
        };
        let ns = shown.to_string();
        let nm = text_size(&ns, 16.0);
        let cxn = (minus.x + minus.w + plus.x) / 2.0;
        text(&ns, cxn - nm.width / 2.0, y + 14.0, 16.0, num_col);
        if staged.is_some_and(|s| s != desired) {
            text(
                format!("staged  {desired} -> {shown}"),
                plus.x + 34.0,
                y + 13.0,
                13.0,
                WARN,
            );
        }
        if click {
            if minus.contains(mouse) {
                act.stage = Some(Intervention::Scale {
                    workload: r.clone(),
                    replicas: (shown - 1).max(0),
                });
            } else if plus.contains(mouse) {
                act.stage = Some(Intervention::Scale {
                    workload: r.clone(),
                    replicas: shown + 1,
                });
            }
        }
        y += 24.0;
    }

    let mut rollout = format!("rollout {}", city.status);
    if !city.note.is_empty() {
        rollout.push_str(&format!(" ({})", city.note));
    }
    text(ascii(&rollout), b.x, y + 12.0, 14.0, DIM);
    if let Some(sev) = models.workload_severity.get(r) {
        let m = text_size(ascii(&rollout), 14.0);
        text(
            "needs attention",
            b.x + m.width + 24.0,
            y + 12.0,
            14.0,
            severity_color(*sev),
        );
    }
    y += 18.0;
    text(
        ascii(&format!(
            "strategy {} . age {}",
            city.strategy,
            format_age_opt(city.age.as_ref())
        )),
        b.x,
        y + 12.0,
        13.0,
        DIM,
    );
    y += 16.0;
    if let Some(pair) = &snap.pair
        && let Some(st) = pair.state(r)
    {
        text(
            ascii(&format!("pair: {}", st.describe(id))),
            b.x,
            y + 12.0,
            13.0,
            sync_color(st),
        );
        y += 16.0;
    }
    // Census line: a quick tally chip strip.
    let svc = city.owned.iter().filter(|o| o.kind == "svc").count();
    let ing = city.owned.iter().filter(|o| o.kind == "ing").count();
    let pvc = city.owned.iter().filter(|o| o.kind == "pvc").count();
    text(
        format!(
            "{} pods . {} svc . {} ingress . {} PVC",
            city.pods.len(),
            svc,
            ing,
            pvc
        ),
        b.x,
        y + 12.0,
        13.0,
        PARCHMENT,
    );
    y += 22.0;
    draw_line(b.x, y, b.x + b.w, y, 1.0, darker(PARCHMENT, 0.5));
    y += 8.0;

    // --- Two columns ------------------------------------------------------
    let col_top = y;
    let col_bottom = b.y + b.h - 4.0;
    let left_x = b.x;
    let left_w = b.w * 0.55;
    let right_x = b.x + b.w * 0.58;

    // Left: CITIZENS — census grid + clickable pod list.
    let mut ly = col_top;
    text_bold(
        format!("CITIZENS ({})", city.pods.len()),
        left_x,
        ly + 12.0,
        15.0,
        PARCHMENT,
    );
    ly += 22.0;
    // Census grid (Civ II's food-storage grid; one chip per pod).
    let chip = 11.0;
    let cols = ((left_w + 3.0) / (chip + 3.0)).floor().max(1.0) as usize;
    let census_cap = cols * 4;
    for (i, p) in city.pods.iter().take(census_cap).enumerate() {
        let cxp = left_x + (i % cols) as f32 * (chip + 3.0);
        let cyp = ly + (i / cols) as f32 * (chip + 3.0);
        draw_rectangle(cxp, cyp, chip, chip, pod_color(p.state));
        draw_rectangle_lines(cxp, cyp, chip, chip, 1.0, darker(pod_color(p.state), 0.6));
    }
    let census_rows = city.pods.len().min(census_cap).div_ceil(cols);
    ly += census_rows as f32 * (chip + 3.0) + 8.0;

    // Detailed pod list (clickable → tail logs).
    let row_h = 18.0;
    let max_rows = (((col_bottom - ly) / row_h) as usize).saturating_sub(1);
    for p in city.pods.iter().take(max_rows) {
        let rect = Rect::new(left_x, ly, left_w, row_h);
        if rect.contains(mouse) {
            draw_rectangle(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                Color::new(1.0, 1.0, 1.0, 0.06),
            );
            if click {
                act.log = Some((r.namespace.clone(), p.name.clone()));
            }
        }
        draw_circle(left_x + 5.0, ly + row_h / 2.0, 4.0, pod_color(p.state));
        let reason = if p.reason.is_empty() {
            String::new()
        } else {
            format!("  {}", p.reason)
        };
        let label = format!(
            "{}{} . r{} . {}",
            truncate_str(&p.name, 30),
            reason,
            p.restarts,
            format_age_opt(p.age.as_ref())
        );
        let col = if p.state == kubernation_core::state::model::PodState::Failing {
            CRIT
        } else {
            INK
        };
        text(ascii(&label), left_x + 16.0, ly + 13.0, 13.0, col);
        ly += row_h;
    }
    if city.pods.len() > max_rows {
        text(
            format!("+{} more", city.pods.len() - max_rows),
            left_x + 16.0,
            ly + 13.0,
            13.0,
            DIM,
        );
    }
    text(
        "click a pod to tail its logs",
        left_x,
        col_bottom + 0.0,
        12.0,
        DIM,
    );

    // Right: IMPROVEMENTS (owned) then CHRONICLE (events).
    let mut ry = col_top;
    draw_line(
        right_x - 14.0,
        col_top - 4.0,
        right_x - 14.0,
        col_bottom,
        1.0,
        darker(PARCHMENT, 0.5),
    );
    text_bold(
        format!("IMPROVEMENTS ({})", city.owned.len()),
        right_x,
        ry + 12.0,
        15.0,
        PARCHMENT,
    );
    ry += 22.0;
    let imp_max = 10;
    for o in city.owned.iter().take(imp_max) {
        let note_col = if o.kind == "pvc" && o.note != "Bound" {
            WARN
        } else {
            DIM
        };
        text(
            format!("{:>6}/", o.kind),
            right_x,
            ry + 12.0,
            13.0,
            PARCHMENT,
        );
        text(
            ascii(&truncate_str(&o.name, 22)),
            right_x + 52.0,
            ry + 12.0,
            13.0,
            INK,
        );
        if !o.note.is_empty() {
            text(
                ascii(&truncate_str(&o.note, 18)),
                right_x + 200.0,
                ry + 12.0,
                12.0,
                note_col,
            );
        }
        ry += row_h;
    }
    if city.owned.len() > imp_max {
        text(
            format!("+{} more", city.owned.len() - imp_max),
            right_x,
            ry + 12.0,
            13.0,
            DIM,
        );
        ry += row_h;
    }
    if city.owned.is_empty() {
        text("nothing owned", right_x, ry + 12.0, 13.0, DIM);
        ry += row_h;
    }

    ry += 10.0;
    text_bold("CHRONICLE", right_x, ry + 12.0, 15.0, PARCHMENT);
    ry += 22.0;
    if city.events.is_empty() {
        text("no recent events", right_x, ry + 12.0, 13.0, DIM);
    } else {
        let ev_max = (((col_bottom - ry) / 16.0) as usize).max(1);
        for e in city.events.iter().take(ev_max) {
            let col = if e.warning { WARN } else { DIM };
            let line = format!("{} x{} {}", e.reason, e.count.max(1), e.message);
            text(
                ascii(&truncate_str(&line, 46)),
                right_x,
                ry + 12.0,
                12.0,
                col,
            );
            ry += 16.0;
        }
    }

    // Headless verification: tail the first pod without a click.
    if auto_log && act.log.is_none() && !city.pods.is_empty() {
        act.log = Some((r.namespace.clone(), city.pods[0].name.clone()));
    }

    // Close: the X, or a click anywhere outside the frame (when not on a pod).
    if click && act.log.is_none() && (win.close.contains(mouse) || !win.frame.contains(mouse)) {
        act.close = true;
    }
    act
}

/// A labelled progress bar: `value` of `max`, coloured `col` when filled.
fn gauge(x: f32, y: f32, w: f32, label: &str, value: i32, max: i32, col: Color) {
    text(label, x, y + 11.0, 13.0, DIM);
    let bx = x + 64.0;
    let bw = w - 64.0;
    let bh = 12.0;
    let by = y + 1.0;
    draw_rectangle(bx, by, bw, bh, darker(PANEL, 0.6));
    let frac = if max > 0 {
        (value as f32 / max as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let fill = if value >= max && max > 0 {
        Color::new(0.35, 0.60, 0.30, 1.0)
    } else {
        col
    };
    draw_rectangle(bx, by, bw * frac, bh, fill);
    draw_rectangle_lines(bx, by, bw, bh, 1.0, darker(PARCHMENT, 0.6));
    let n = format!("{value}/{max}");
    let m = text_size(&n, 12.0);
    text(&n, bx + bw - m.width - 4.0, y + 11.0, 12.0, INK);
}
