//! The node drill-down — a province window, sibling to the city window.
//! A node reframed as 4X terrain (observe-only):
//!
//!   title bar  →  node name (+ HOT/WARM)
//!   status band→  zone, health, cpu/mem gauges (live usage or pressure)
//!   garrison   →  the pods stationed here (census grid + list, tail logs)
//!   terrain    →  runtime / kubelet / OS / arch attributes
//!   conditions →  node conditions
//!   annals     →  recent changes touching this province (events + its pods)

use macroquad::prelude::*;

use kubernation_core::events::ClusterId;
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::{
    MetricSource, NodeDetailModel, NodeHealth, PodState, build_node_detail,
};
use kubernation_core::state::planned::{Intervention, PlannedWorld};
use kubernation_core::state::saturation::SatLevel;
use kubernation_core::state::timeline::{
    SUBJECT_CAP, TIMELINE_WINDOW_MIN, TimelineOpts, TimelineScope, build_timeline,
};

use kubernation_core::util::format_usage;

use crate::net::Snapshot;
use crate::panels::{
    clamp_scroll, draw_sparkline, fit_width, observed_for, panel_size, pod_color, scroll_thumb,
    truncate_str,
};
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::{ForwardBtn, WinAction, draw_window};

/// Draw the province (node) window and resolve this frame's clicks. `scroll_l` /
/// PURE draw-decision fn: the SUBSTRATE section's lines — the node's DaemonSet
/// inventory by name, what the rest of the fleet has that it LACKS, and any
/// kubelet pressure conditions flagged on the tile.
///
/// Substrate pressure is the point of pairing them: `MemoryPressure` /
/// `DiskPressure` / `PIDPressure` are the kubelet refusing or evicting work for
/// reasons that have nothing to do with any workload's own pods, so they belong
/// beside the things that run underneath. Unit-tested (the testability policy).
///
/// `missing` comes from the cluster-wide `SubstrateReport` (a node can't know it
/// alone — the expectation is set by the fleet). Shown here regardless of the
/// active map overlay: once you have drilled into a node, what it is missing is
/// a fact worth having, not something you should need the right view turned on
/// to see. The Substrate overlay's SELECTION line is the at-a-glance twin.
///
/// `fleet_known` is that report's `has_data()` — false when NO daemonset reaches
/// the fleet-wide bar, so there is no expectation to measure this node against.
/// That is NOT the same as "covered", and rendering both as silence would be an
/// unearned all-clear; the SELECTION line already distinguishes them.
pub fn substrate_lines(
    detail: &NodeDetailModel,
    missing: &[String],
    fleet_known: bool,
) -> Vec<(String, Color)> {
    let mut out: Vec<(String, Color)> = Vec::new();
    // Pressure first — it is the actionable half when present.
    if detail.tile.abnormal.is_empty() {
        out.push(("no substrate pressure".into(), DIM));
    } else {
        for a in &detail.tile.abnormal {
            out.push((format!("{a}Pressure"), WARN));
        }
    }
    if detail.daemonsets.is_empty() {
        // Honest: a node with no DaemonSets is unusual but legal (a tainted or
        // freshly-joined node), and says something.
        out.push(("no daemonsets stationed".into(), DIM));
    } else {
        out.push((
            format!("{} daemonsets", detail.daemonsets.len()),
            STONE_INK_DIM,
        ));
        for d in &detail.daemonsets {
            out.push((format!("  {d}"), INK));
        }
    }
    // The gap: what the fleet runs and this node doesn't.
    if !fleet_known {
        // Nothing is fleet-wide, so "no gaps" would claim a check that never ran.
        out.push(("no fleet-wide daemonsets to compare".into(), DIM));
    } else if !missing.is_empty() {
        out.push((
            format!("{} missing vs the fleet", missing.len()),
            if missing.len() > 1 { CRIT } else { WARN },
        ));
        for m in missing {
            out.push((
                format!("  {m}"),
                if missing.len() > 1 { CRIT } else { WARN },
            ));
        }
    }
    // A covered node spends no row on the gap — absence is the common case.
    out
}

/// `scroll_r` are the per-column scroll offsets (GARRISON / TERRAIN+…); the
/// caller adjusts them on the wheel and this clamps them to content height.
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
    scroll_l: &mut f32,
    scroll_r: &mut f32,
) -> WinAction {
    let mut act = WinAction::default();
    let tag = match (snap.warm.is_some(), id) {
        (true, ClusterId::Hot) => " — HOT",
        (true, ClusterId::Warm) => " — WARM",
        _ => "",
    };
    let win = draw_window(
        &ascii(&format!("{name}{tag}")),
        panel_size(screen_width(), screen_height()),
        &[],
        usize::MAX,
    );
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
    // Saturation strain — the worst-dimension verdict + the pod-slot ratio (the
    // 4th-golden-signal axis cpu/mem can't show; cpu/mem are the gauges above,
    // the kubelet conditions the "pressure" flags). Calm reads dim.
    {
        let sat = &t.saturation;
        let (word, col) = match sat.worst_level() {
            SatLevel::Calm => ("calm", DIM),
            SatLevel::Elevated => ("elevated", WARN),
            SatLevel::High => ("high", CRIT),
        };
        let mut txt = format!("strain: {word}");
        if let Some(lbl) = sat.pod_label() {
            txt.push_str(&format!(" · {lbl}"));
        }
        text(txt.as_str(), b.x, y + 12.0, 13.0, col);
        y += 18.0;
    }
    text(
        format!("{} pods stationed", detail.pods.len()),
        b.x,
        y + 12.0,
        13.0,
        PARCHMENT,
    );
    y += 18.0;
    // Why-not-Ready explainer for the worst pod stationed here (root cause + fix).
    if let Some(d) = detail
        .pods
        .iter()
        .find(|p| p.state == kubernation_core::state::model::PodState::Failing && p.diag.is_some())
        .or_else(|| detail.pods.iter().find(|p| p.diag.is_some()))
        .and_then(|p| p.diag.as_ref())
    {
        text(
            ascii(&format!("why: {} - {}", d.reason, d.explain)),
            b.x,
            y + 12.0,
            12.0,
            WARN,
        );
        y += 15.0;
        text(ascii(&format!("fix: {}", d.hint)), b.x, y + 12.0, 12.0, DIM);
        y += 16.0;
    }
    y += 4.0;

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
    // The pod list scrolls within its column; the header + census above and the
    // hint below stay pinned. EVERY draw AND hit-test sits inside the `vis`
    // gate, so a scrolled-off row can't be hovered or (critically) evicted.
    let list_top = ly;
    let list_bottom = col_bottom - 16.0;
    let list_view_h = (list_bottom - list_top).max(0.0);
    let content_h = detail.pods.len() as f32 * row_h;
    *scroll_l = clamp_scroll(*scroll_l, content_h, list_view_h);
    let mut ly = list_top - *scroll_l;
    for p in detail.pods.iter() {
        let vis = ly + row_h > list_top && ly < list_bottom;
        if vis {
            let rect = Rect::new(left_x, ly, left_w, row_h);
            let evict_btn = Rect::new(left_x + left_w - 52.0, ly + 1.0, 50.0, row_h - 2.0);
            let yaml_btn = Rect::new(left_x + left_w - 104.0, ly + 1.0, 48.0, row_h - 2.0);
            let fwd_btn = Rect::new(left_x + left_w - 156.0, ly + 1.0, 48.0, row_h - 2.0);
            // Only a fully-visible row is interactive: a row straddling the band
            // edge still draws (no scissor), but its off-band evict/fwd/yaml
            // buttons must not be hoverable or clickable.
            let row_hover = ly >= list_top && ly + row_h <= list_bottom && rect.contains(mouse);
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
                let fwd =
                    crate::window::forward_button(fwd_btn, mouse, click, fwd_perm, fwd_active);
                let ev = crate::window::evict_button(evict_btn, mouse, click, perm);
                let ya = crate::window::row_button(yaml_btn, mouse, click, "yaml");
                if let Some(fb) = fwd {
                    match fb {
                        ForwardBtn::Start => {
                            act.forward = Some((p.namespace.clone(), p.name.clone()))
                        }
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
                        kubernation_core::state::model::prefer_previous(
                            p.state, &p.reason, p.restarts,
                        ),
                    ));
                }
            }
        }
        ly += row_h;
    }
    // Scrollbar for the pod list, in the gutter before the column divider.
    if let Some((ty, th)) = scroll_thumb(list_top, list_view_h, content_h, *scroll_l) {
        let bx = left_x + left_w + 2.0;
        draw_rectangle(bx, list_top, 3.0, list_view_h, darker(PANEL, 0.6));
        draw_rectangle(bx, ty, 3.0, th, PARCHMENT);
    }
    text(
        "scroll · click a pod = logs · hover: fwd / yaml / evict · y: node yaml",
        left_x,
        col_bottom,
        12.0,
        DIM,
    );

    // Right: TERRAIN → CONDITIONS → ANNALS. Display-only (no hit-tests), so the
    // scroll+cull is purely visual. The column divider stays pinned.
    draw_line(
        right_x - 14.0,
        col_top - 4.0,
        right_x - 14.0,
        col_bottom,
        1.0,
        darker(PARCHMENT, 0.5),
    );
    let r_top = col_top;
    let r_view_h = (col_bottom - r_top).max(0.0);
    let r_origin = r_top - *scroll_r;
    let mut ry = r_origin;
    let visr = |yy: f32, h: f32| yy + h > r_top && yy < col_bottom;

    if visr(ry, 18.0) {
        text_bold("TERRAIN", right_x, ry + 12.0, 15.0, PARCHMENT);
    }
    ry += 22.0;
    for (k, v) in &detail.info {
        if visr(ry, row_h) {
            text(k, right_x, ry + 12.0, 13.0, DIM);
            text(
                ascii(&truncate_str(v, 30)),
                right_x + 96.0,
                ry + 12.0,
                13.0,
                INK,
            );
        }
        ry += row_h;
    }

    ry += 10.0;
    if visr(ry, 18.0) {
        text_bold("CONDITIONS", right_x, ry + 12.0, 15.0, PARCHMENT);
    }
    ry += 22.0;
    if detail.conditions.is_empty() {
        if visr(ry, row_h) {
            text("all nominal", right_x, ry + 12.0, 13.0, DIM);
        }
        ry += row_h;
    } else {
        // Right-align the value at the column edge and truncate the (often long)
        // condition name to the gap, so "FilesystemCorruptionProblem" can't run
        // over "False".
        let val_right = b.x + b.w - 4.0;
        for (k, v) in detail.conditions.iter() {
            if visr(ry, row_h) {
                let col = if v == "True" && k != "Ready" {
                    WARN
                } else {
                    DIM
                };
                let vm = text_size(v, 13.0);
                let name_max = (val_right - vm.width - 10.0) - right_x;
                text(
                    ascii(&fit_width(k, 13.0, name_max)),
                    right_x,
                    ry + 12.0,
                    13.0,
                    INK,
                );
                text(v, val_right - vm.width, ry + 12.0, 13.0, col);
            }
            ry += row_h;
        }
    }

    // SUBSTRATE — what runs UNDER the workloads: the DaemonSets stationed here
    // (CNI, kube-proxy, log/metric agents) plus any kubelet pressure conditions.
    // On the map this is only an anonymous road count; the names are what you
    // actually need when a node misbehaves in a way its own pods don't explain.
    ry += 10.0;
    if visr(ry, 18.0) {
        text_bold("SUBSTRATE", right_x, ry + 12.0, 15.0, PARCHMENT);
    }
    ry += 22.0;
    // The fleet-wide report is per-world (a hot/warm pair runs different
    // infrastructure), and lives on `Models` — no new plumbing needed.
    let substrate = match id {
        ClusterId::Hot => &snap.hot.models.substrate,
        ClusterId::Warm => snap
            .warm
            .as_ref()
            .map_or(&snap.hot.models.substrate, |w| &w.models.substrate),
    };
    for (line, col) in substrate_lines(&detail, substrate.missing(name), substrate.has_data()) {
        if visr(ry, row_h) {
            text(ascii(&line), right_x, ry + 12.0, 13.0, col);
        }
        ry += row_h;
    }

    // ANNALS — recent changes touching this province: its node events, the pods
    // stationed on it, and this session's operator actions on it.
    ry += 10.0;
    if visr(ry, 18.0) {
        text_bold("ANNALS", right_x, ry + 12.0, 15.0, PARCHMENT);
    }
    ry += 22.0;
    let now = kubernation_core::util::now();
    let ops = net.operator_actions();
    let tl = build_timeline(
        observed,
        &TimelineOpts {
            scope: TimelineScope::Node(name.to_string()),
            filter: &NamespaceFilter::All,
            window_min: TIMELINE_WINDOW_MIN,
            cap: SUBJECT_CAP,
        },
        &ops,
        now,
    );
    if tl.entries.is_empty() {
        if visr(ry, 16.0) {
            text("no recent changes", right_x, ry + 12.0, 13.0, DIM);
        }
        ry += 16.0;
    } else {
        for ln in crate::timeline::annals_lines(&tl, now, SUBJECT_CAP) {
            if ln.fault_line_above {
                if visr(ry, 6.0) {
                    draw_line(right_x, ry + 3.0, b.x + b.w - 8.0, ry + 3.0, 1.0, CRIT);
                }
                ry += 6.0;
            }
            if visr(ry, 16.0) {
                let mut s = format!("{} {}", ln.glyph, ln.text);
                if ln.suspect {
                    s.push_str("  (before failure)");
                }
                text(
                    ascii(&truncate_str(&s, 44)),
                    right_x,
                    ry + 12.0,
                    12.0,
                    crate::timeline::role_color(ln.role),
                );
            }
            ry += 16.0;
        }
    }
    // Right-column scrollbar + clamp (content height = the drawn extent).
    let r_content_h = ry - r_origin;
    *scroll_r = clamp_scroll(*scroll_r, r_content_h, r_view_h);
    if let Some((ty, th)) = scroll_thumb(r_top, r_view_h, r_content_h, *scroll_r) {
        let bx = b.x + b.w + 2.0;
        draw_rectangle(bx, r_top, 3.0, r_view_h, darker(PANEL, 0.6));
        draw_rectangle(bx, ty, 3.0, th, PARCHMENT);
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
        gauge_ok()
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

#[cfg(test)]
mod substrate_tests {
    use super::*;
    use kubernation_core::state::{fixtures as fx, model::build_node_detail};

    #[test]
    fn substrate_names_daemonsets_and_flags_pressure() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.daemonset(fx::daemonset("kube-system", "agent", 1, 1));
        s.pod(fx::pod_owned(
            fx::pod("kube-system", "agent-1", Some("n1")),
            "DaemonSet",
            "agent",
        ));
        let d = build_node_detail(&world, "n1").expect("node detail");
        assert_eq!(
            d.daemonsets,
            vec!["kube-system/agent"],
            "the qualified NAME, not a count"
        );
        let join = |m: &[String]| {
            substrate_lines(&d, m, true)
                .iter()
                .map(|(t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join("|")
        };
        let joined = join(&[]);
        assert!(
            joined.contains("kube-system/agent"),
            "names the daemonset, namespace-qualified: {joined}"
        );
        assert!(joined.contains("1 daemonsets"), "counts them too: {joined}");
        assert!(
            joined.contains("no substrate pressure"),
            "states the absence explicitly: {joined}"
        );
        // A fully-covered node spends no row on the gap — absence is the norm.
        assert!(!joined.contains("missing"), "silent when covered: {joined}");

        // But "nothing is fleet-wide" must NOT render the same as "covered" —
        // that would claim a check the report could not run.
        let unknown: String = substrate_lines(&d, &[], false)
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            unknown.contains("no fleet-wide daemonsets to compare"),
            "an unearned all-clear: {unknown}"
        );
        assert_ne!(unknown, joined, "the two states must be distinguishable");
    }

    /// The other half of the section: what the FLEET has that this node doesn't.
    /// A node can't know this alone — the expectation comes from the cluster-wide
    /// report — so the names have to be passed in and rendered here.
    #[test]
    fn substrate_names_what_the_fleet_has_and_this_node_lacks() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let d = build_node_detail(&world, "n1").expect("node detail");
        let join = |m: &[String]| {
            substrate_lines(&d, m, true)
                .iter()
                .map(|(t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join("|")
        };
        let one = join(&["cilium".to_string()]);
        assert!(one.contains("cilium"), "names the missing one: {one}");
        assert!(one.contains("1 missing vs the fleet"), "counted: {one}");
        // A bare node also says it has none stationed — the two halves coexist.
        assert!(one.contains("no daemonsets stationed"), "{one}");

        let two = join(&["cilium".to_string(), "fluent-bit".to_string()]);
        assert!(
            two.contains("cilium") && two.contains("fluent-bit"),
            "{two}"
        );
        // Two gaps escalate to CRIT, one is a WARN.
        let sev = |m: &[String]| {
            substrate_lines(&d, m, true)
                .into_iter()
                .filter(|(t, _)| t.contains("missing") || t.contains("cilium"))
                .map(|(_, c)| c)
                .collect::<Vec<_>>()
        };
        assert!(sev(&["cilium".to_string()]).iter().all(|c| *c == WARN));
        assert!(
            sev(&["cilium".to_string(), "fluent-bit".to_string()])
                .iter()
                .all(|c| *c == CRIT)
        );
    }
}
