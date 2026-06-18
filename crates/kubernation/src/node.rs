//! The node drill-down — a province window, sibling to the city window.
//! A node reframed as 4X terrain (observe-only):
//!
//!   title bar  →  node name (+ HOT/WARM)
//!   status band→  zone, health, cpu/mem gauges (live usage or pressure)
//!   garrison   →  the pods stationed here (census grid + list, tail logs)
//!   terrain    →  runtime / kubelet / OS / arch attributes
//!   conditions →  node conditions

use macroquad::prelude::*;

use kubernation_core::events::ClusterId;
use kubernation_core::state::model::{MetricSource, NodeHealth, PodState, build_node_detail};
use kubernation_core::state::planned::{Intervention, PlannedWorld};

use kubernation_core::util::format_usage;

use crate::net::Snapshot;
use crate::panels::{draw_sparkline, observed_for, pod_color, truncate_str};
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::{ForwardBtn, WinAction, draw_window};

const W: f32 = 900.0;
const H: f32 = 580.0;

/// Draw the province (node) window and resolve this frame's clicks.
#[allow(clippy::too_many_arguments)]
pub fn draw_node(
    id: ClusterId,
    name: &str,
    snap: &Snapshot,
    planned: &PlannedWorld,
    mouse: Vec2,
    click: bool,
    auto_log: bool,
    net: &crate::net::Net,
) -> WinAction {
    let mut act = WinAction::default();
    let tag = match (snap.warm.is_some(), id) {
        (true, ClusterId::Hot) => " — HOT",
        (true, ClusterId::Warm) => " — WARM",
        _ => "",
    };
    let win = draw_window(&ascii(&format!("{name}{tag}")), vec2(W, H), &[], usize::MAX);
    let b = win.body;
    let close_hit = |a: &mut WinAction| {
        if click && (win.close.contains(mouse) || !win.frame.contains(mouse)) {
            a.close = true;
        }
    };

    let observed = match observed_for(snap, id) {
        Some(o) => o,
        None => {
            text("world detached", b.x, b.y + 18.0, 16.0, DIM);
            close_hit(&mut act);
            return act;
        }
    };
    let Some(detail) = build_node_detail(observed, name) else {
        text("node is no longer observed", b.x, b.y + 18.0, 16.0, DIM);
        close_hit(&mut act);
        return act;
    };
    let t = &detail.tile;

    // --- Status band ------------------------------------------------------
    let mut y = b.y + 6.0;
    let zone_line = format!("province of {}", t.zone);
    text(zone_line.as_str(), b.x, y + 12.0, 14.0, PARCHMENT);
    let (hword, hcol) = match t.health {
        NodeHealth::Healthy => ("healthy", INK),
        NodeHealth::Cordoned => ("cordoned", WARN),
        NodeHealth::Pressure => ("under pressure", WARN),
        NodeHealth::NotReady => ("NotReady", CRIT),
    };
    let hm = text_size(zone_line.as_str(), 14.0);
    text(hword, b.x + hm.width + 24.0, y + 12.0, 14.0, hcol);
    if !t.abnormal.is_empty() {
        let m2 = text_size(hword, 14.0);
        text(
            format!("{} pressure", t.abnormal.join("/")),
            b.x + hm.width + 24.0 + m2.width + 16.0,
            y + 12.0,
            13.0,
            WARN,
        );
    }
    y += 22.0;

    // cpu / mem gauges (live usage or scheduling pressure).
    let src = match t.metric_source {
        MetricSource::Usage => "live usage",
        MetricSource::Requests => "scheduling pressure",
    };
    text(src, b.x, y + 11.0, 12.0, DIM);
    y += 16.0;
    ratio_gauge(b.x, y, 300.0, "cpu", t.cpu_ratio);
    ratio_gauge(b.x + 340.0, y, 300.0, "mem", t.mem_ratio);
    y += 20.0;
    // Live-usage trend sparklines, aligned under each gauge bar (only when
    // metrics-server has reported history). Each is scaled to allocatable
    // (max = 1.0) so its height reads like the gauge, and coloured by the
    // latest sample's pressure bucket.
    if !detail.cpu_history.is_empty() || !detail.mem_history.is_empty() {
        let sh = 22.0;
        let bw = 260.0; // matches the gauge bar width (w - 40)
        text("trend", b.x, y + 14.0, 11.0, DIM);
        draw_sparkline(
            Rect::new(b.x + 40.0, y, bw, sh),
            &detail.cpu_history,
            1.0,
            bucket_color(t.cpu_ratio),
        );
        draw_sparkline(
            Rect::new(b.x + 380.0, y, bw, sh),
            &detail.mem_history,
            1.0,
            bucket_color(t.mem_ratio),
        );
        y += sh + 4.0;
    }
    y += 18.0;
    text(
        format!("{} pods stationed", detail.pods.len()),
        b.x,
        y + 12.0,
        13.0,
        PARCHMENT,
    );
    y += 22.0;

    // PLAN: stage cordon / uncordon. Preview-only — records intent, no writes.
    {
        let observed_cordon = t.cordoned;
        let staged = planned.cordoned(name);
        let effective = staged.unwrap_or(observed_cordon);
        text("plan", b.x, y + 13.0, 14.0, PARCHMENT);
        let label = if effective { "uncordon" } else { "cordon" };
        let btn = Rect::new(b.x + 44.0, y, 90.0, 18.0);
        let bg = if btn.contains(mouse) {
            lighter(PLATE, 1.7)
        } else {
            PLATE
        };
        draw_rectangle(btn.x, btn.y, btn.w, btn.h, bg);
        draw_rectangle_lines(btn.x, btn.y, btn.w, btn.h, 1.0, PARCHMENT);
        text(label, btn.x + 8.0, y + 14.0, 13.0, INK);
        if staged.is_some_and(|s| s != observed_cordon) {
            let word = if effective { "cordoned" } else { "schedulable" };
            text(
                format!("staged -> {word}"),
                btn.x + 104.0,
                y + 13.0,
                13.0,
                WARN,
            );
        }
        if click && btn.contains(mouse) {
            act.stage = Some(Intervention::Cordon {
                node: name.to_string(),
                on: !effective,
            });
        }
        y += 24.0;
    }

    draw_line(b.x, y, b.x + b.w, y, 1.0, darker(PARCHMENT, 0.5));
    y += 8.0;

    // --- Two columns ------------------------------------------------------
    let col_top = y;
    let col_bottom = b.y + b.h - 4.0;
    let left_x = b.x;
    let left_w = b.w * 0.55;
    let right_x = b.x + b.w * 0.58;

    // Left: GARRISON (pods on this node) — census grid + clickable list.
    let mut ly = col_top;
    text_bold(
        format!("GARRISON ({})", detail.pods.len()),
        left_x,
        ly + 12.0,
        15.0,
        PARCHMENT,
    );
    ly += 22.0;
    let chip = 11.0;
    let cols = ((left_w + 3.0) / (chip + 3.0)).floor().max(1.0) as usize;
    let census_cap = cols * 4;
    for (i, p) in detail.pods.iter().take(census_cap).enumerate() {
        let cxp = left_x + (i % cols) as f32 * (chip + 3.0);
        let cyp = ly + (i / cols) as f32 * (chip + 3.0);
        draw_rectangle(cxp, cyp, chip, chip, pod_color(p.state));
        draw_rectangle_lines(cxp, cyp, chip, chip, 1.0, darker(pod_color(p.state), 0.6));
    }
    let census_rows = detail.pods.len().min(census_cap).div_ceil(cols);
    ly += census_rows as f32 * (chip + 3.0) + 8.0;

    let row_h = 18.0;
    let max_rows = (((col_bottom - ly) / row_h) as usize).saturating_sub(1);
    for p in detail.pods.iter().take(max_rows) {
        let rect = Rect::new(left_x, ly, left_w, row_h);
        let evict_btn = Rect::new(left_x + left_w - 52.0, ly + 1.0, 50.0, row_h - 2.0);
        let yaml_btn = Rect::new(left_x + left_w - 104.0, ly + 1.0, 48.0, row_h - 2.0);
        let fwd_btn = Rect::new(left_x + left_w - 156.0, ly + 1.0, 48.0, row_h - 2.0);
        let row_hover = rect.contains(mouse);
        if row_hover {
            draw_rectangle(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                Color::new(1.0, 1.0, 1.0, 0.06),
            );
        }
        draw_circle(left_x + 5.0, ly + row_h / 2.0, 4.0, pod_color(p.state));
        let use_suffix = p
            .usage
            .map(|u| format!("  {}", format_usage(u.cpu, u.mem)))
            .unwrap_or_default();
        let label = format!(
            "{}/{}{}",
            p.namespace,
            truncate_str(&p.name, 28 - p.namespace.len().min(16)),
            use_suffix
        );
        let col = if p.state == PodState::Failing {
            CRIT
        } else {
            INK
        };
        text(ascii(&label), left_x + 16.0, ly + 13.0, 13.0, col);
        // Per-pod RBAC: garrison pods can span namespaces.
        if row_hover {
            let perm = net.evict_allowed(id, &p.namespace);
            let fwd_perm = net.forward_allowed(id, &p.namespace);
            let fwd_active = net
                .forward_for(id, &p.namespace, &p.name)
                .map(|f| f.local_port);
            let fwd = crate::window::forward_button(fwd_btn, mouse, click, fwd_perm, fwd_active);
            let ev = crate::window::evict_button(evict_btn, mouse, click, perm);
            let ya = crate::window::row_button(yaml_btn, mouse, click, "yaml");
            if let Some(fb) = fwd {
                match fb {
                    ForwardBtn::Start => act.forward = Some((p.namespace.clone(), p.name.clone())),
                    ForwardBtn::Stop => act.stop_forward = fwd_active,
                }
            } else if ev {
                act.evict = Some((p.namespace.clone(), p.name.clone()));
            } else if ya {
                act.inspect = Some((p.namespace.clone(), p.name.clone()));
            } else if click
                && !fwd_btn.contains(mouse)
                && !evict_btn.contains(mouse)
                && !yaml_btn.contains(mouse)
            {
                act.log = Some((
                    p.namespace.clone(),
                    p.name.clone(),
                    kubernation_core::state::model::prefer_previous(p.state, &p.reason, p.restarts),
                ));
            }
        }
        ly += row_h;
    }
    if detail.pods.len() > max_rows {
        text(
            format!("+{} more", detail.pods.len() - max_rows),
            left_x + 16.0,
            ly + 13.0,
            13.0,
            DIM,
        );
    }
    text(
        "click a pod = logs · hover: fwd / yaml / evict · y: node yaml",
        left_x,
        col_bottom,
        12.0,
        DIM,
    );

    // Right: TERRAIN (runtime attributes) then CONDITIONS.
    let mut ry = col_top;
    draw_line(
        right_x - 14.0,
        col_top - 4.0,
        right_x - 14.0,
        col_bottom,
        1.0,
        darker(PARCHMENT, 0.5),
    );
    text_bold("TERRAIN", right_x, ry + 12.0, 15.0, PARCHMENT);
    ry += 22.0;
    for (k, v) in &detail.info {
        text(k, right_x, ry + 12.0, 13.0, DIM);
        text(
            ascii(&truncate_str(v, 30)),
            right_x + 96.0,
            ry + 12.0,
            13.0,
            INK,
        );
        ry += row_h;
    }

    ry += 10.0;
    text_bold("CONDITIONS", right_x, ry + 12.0, 15.0, PARCHMENT);
    ry += 22.0;
    if detail.conditions.is_empty() {
        text("all nominal", right_x, ry + 12.0, 13.0, DIM);
    } else {
        let cond_max = (((col_bottom - ry) / row_h) as usize).max(1);
        for (k, v) in detail.conditions.iter().take(cond_max) {
            let col = if v == "True" && k != "Ready" {
                WARN
            } else {
                DIM
            };
            text(ascii(k), right_x, ry + 12.0, 13.0, INK);
            text(v, right_x + 150.0, ry + 12.0, 13.0, col);
            ry += row_h;
        }
    }

    // Headless verification: tail the first pod without a click.
    if auto_log && act.log.is_none() && !detail.pods.is_empty() {
        let p = &detail.pods[0];
        act.log = Some((
            p.namespace.clone(),
            p.name.clone(),
            kubernation_core::state::model::prefer_previous(p.state, &p.reason, p.restarts),
        ));
    }
    if click
        && act.log.is_none()
        && act.evict.is_none()
        && act.forward.is_none()
        && act.stop_forward.is_none()
        && (win.close.contains(mouse) || !win.frame.contains(mouse))
    {
        act.close = true;
    }
    act
}

/// A cpu/mem ratio bar: green calm, yellow ≥70%, red ≥90%.
/// The pressure-bucket colour for a usage ratio: calm green &lt;0.7, elevated
/// WARN 0.7–0.9, high CRIT ≥0.9 (the documented gauge buckets). Shared by the
/// gauge fill and its trend sparkline so they read consistently.
fn bucket_color(ratio: f64) -> Color {
    if ratio >= 0.9 {
        CRIT
    } else if ratio >= 0.7 {
        WARN
    } else {
        Color::new(0.35, 0.60, 0.30, 1.0)
    }
}

fn ratio_gauge(x: f32, y: f32, w: f32, label: &str, ratio: f64) {
    text(label, x, y + 11.0, 13.0, DIM);
    let bx = x + 40.0;
    let bw = w - 40.0;
    let bh = 12.0;
    let by = y + 1.0;
    let col = bucket_color(ratio);
    draw_rectangle(bx, by, bw, bh, darker(PANEL, 0.6));
    draw_rectangle(bx, by, bw * (ratio.clamp(0.0, 1.0) as f32), bh, col);
    draw_rectangle_lines(bx, by, bw, bh, 1.0, darker(PARCHMENT, 0.6));
    let n = format!("{:.0}%", ratio * 100.0);
    let m = text_size(&n, 12.0);
    text(&n, bx + bw - m.width - 4.0, y + 11.0, 12.0, INK);
}
