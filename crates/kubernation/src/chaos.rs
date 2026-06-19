//! The Game Day window — a chaos drill console. Pick a target (a workload "city"
//! to raid, or a node "province" to fail), choose an experiment, preview its
//! blast radius + the budget it'll spend, then run it (a confirmed, real write).
//! After it runs, a scorecard shows the cluster's response (recovery time +
//! budget spent), and a reversible drill offers a Restore. The drill logic +
//! guards are pure in `kubernation_core::state::chaos`; this is the modal on
//! `window.rs`.

use kubernation_core::state::blast::Subject;
use kubernation_core::state::chaos::{
    ChaosScorecard, Experiment, PartitionDir, ScoreRole, node_protected, ns_protected, plan_chaos,
    plan_summary, preview_lines, scorecard_lines,
};
use kubernation_core::state::model::WorkloadRef;
use macroquad::prelude::*;

use crate::net::{ChaosSession, Snapshot};
use crate::panels::truncate_str;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

const W: f32 = 780.0;
const H: f32 = 600.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChaosKind {
    Outage,
    KillOne,
    KillAll,
    KillPercent,
    ScaleSpike,
    BrokenImage,
    Partition,
    NodeFailure,
    CordonFreeze,
}

impl ChaosKind {
    const ALL: [ChaosKind; 9] = [
        ChaosKind::Outage,
        ChaosKind::KillOne,
        ChaosKind::KillAll,
        ChaosKind::KillPercent,
        ChaosKind::ScaleSpike,
        ChaosKind::BrokenImage,
        ChaosKind::Partition,
        ChaosKind::NodeFailure,
        ChaosKind::CordonFreeze,
    ];
    fn label(self) -> &'static str {
        match self {
            ChaosKind::Outage => "Outage (scale to 0)",
            ChaosKind::KillOne => "Kill one pod",
            ChaosKind::KillAll => "Kill all pods",
            ChaosKind::KillPercent => "Kill a percentage",
            ChaosKind::ScaleSpike => "Scale spike (surge)",
            ChaosKind::BrokenImage => "Broken image roll",
            ChaosKind::Partition => "Partition (network)",
            ChaosKind::NodeFailure => "Node failure (drain)",
            ChaosKind::CordonFreeze => "Cordon (freeze)",
        }
    }
    /// Node-scoped experiments pick a node, not a workload.
    fn is_node(self) -> bool {
        matches!(self, ChaosKind::NodeFailure | ChaosKind::CordonFreeze)
    }
    /// Parse the dev `--chaos-exp` flag value.
    pub fn from_flag(s: &str) -> Option<ChaosKind> {
        Some(match s {
            "kill-one" => ChaosKind::KillOne,
            "kill-all" => ChaosKind::KillAll,
            "kill-percent" => ChaosKind::KillPercent,
            "scale-spike" => ChaosKind::ScaleSpike,
            "outage" => ChaosKind::Outage,
            "broken-image" => ChaosKind::BrokenImage,
            "partition" => ChaosKind::Partition,
            "node-failure" => ChaosKind::NodeFailure,
            "cordon-freeze" => ChaosKind::CordonFreeze,
            _ => return None,
        })
    }
}

/// What a frame's interaction asks the caller to do.
pub enum ChaosAction {
    None,
    Close,
    /// Raise the confirm for this experiment, then run it. `auto_restore` arms
    /// the net thread to auto-undo a restorable drill after a delay.
    Run {
        exp: Experiment,
        auto_restore: bool,
    },
    /// Re-submit the live session's restore (undo the drill now).
    Restore,
}

/// The Game Day modal's state: the chosen target(s) + experiment + the
/// per-experiment knobs (kill %, surge factor, partition direction).
pub struct Chaos {
    pub target: Option<WorkloadRef>,
    node_target: Option<String>,
    kind: ChaosKind,
    kill_pct: u8,
    spike_factor: u32,
    partition_dir: PartitionDir,
    /// Arm the net thread to auto-undo a restorable drill after a delay.
    auto_restore: bool,
}

impl Chaos {
    pub fn new(target: Option<WorkloadRef>) -> Self {
        Chaos {
            target,
            node_target: None,
            kind: ChaosKind::Outage,
            kill_pct: 50,
            spike_factor: 3,
            partition_dir: PartitionDir::Both,
            auto_restore: false,
        }
    }

    /// Pre-select an experiment kind (dev `--chaos-exp`).
    pub fn set_kind(&mut self, kind: ChaosKind) {
        self.kind = kind;
    }

    /// The experiment for the current kind + selection, if a target is chosen.
    fn experiment(&self) -> Option<Experiment> {
        let wl = || self.target.clone();
        match self.kind {
            ChaosKind::Outage => wl().map(|w| Experiment::Outage { workload: w }),
            ChaosKind::KillOne => wl().map(|w| Experiment::KillOne { workload: w }),
            ChaosKind::KillAll => wl().map(|w| Experiment::KillAll { workload: w }),
            ChaosKind::KillPercent => wl().map(|w| Experiment::KillPercent {
                workload: w,
                pct: self.kill_pct,
            }),
            ChaosKind::ScaleSpike => wl().map(|w| Experiment::ScaleSpike {
                workload: w,
                factor: self.spike_factor,
            }),
            ChaosKind::BrokenImage => wl().map(|w| Experiment::BrokenImage { workload: w }),
            ChaosKind::Partition => wl().map(|w| Experiment::Partition {
                workload: w,
                dir: self.partition_dir,
            }),
            ChaosKind::NodeFailure => self
                .node_target
                .clone()
                .map(|n| Experiment::NodeFailure { node: n }),
            ChaosKind::CordonFreeze => self
                .node_target
                .clone()
                .map(|n| Experiment::CordonFreeze { node: n }),
        }
    }

    /// The subject the current experiment targets (for scorecard matching).
    fn subject(&self) -> Option<Subject> {
        self.experiment().map(|e| e.subject())
    }

    pub fn draw(
        &mut self,
        snap: &Snapshot,
        session: Option<&ChaosSession>,
        mouse: Vec2,
        click: bool,
    ) -> ChaosAction {
        let win = draw_window("Game Day — chaos drill", vec2(W, H), &[], usize::MAX);
        let b = win.body;
        text(
            "Inject a real failure on the hot cluster, then watch the realm respond.",
            b.x,
            b.y + 12.0,
            13.0,
            DIM,
        );

        let top = b.y + 30.0;
        let bottom = b.y + b.h - 4.0;
        let left_x = b.x;
        let left_w = b.w * 0.42;
        let right_x = b.x + b.w * 0.47;

        // --- LEFT: target picker (workloads, or nodes for a node drill) -------
        if self.kind.is_node() {
            self.draw_node_picker(snap, left_x, left_w, top, bottom, mouse, click);
        } else {
            self.draw_workload_picker(snap, left_x, left_w, top, bottom, mouse, click);
        }

        // --- RIGHT: experiment + preview + run --------------------------------
        let mut ry = top;
        text_bold("EXPERIMENT", right_x, ry + 12.0, 14.0, PARCHMENT);
        ry += 22.0;
        for k in ChaosKind::ALL {
            let rect = Rect::new(right_x, ry, b.w * 0.5, 20.0);
            let on = self.kind == k;
            if on {
                draw_rectangle(rect.x, rect.y, rect.w, rect.h, darker(CRIT, 0.7));
            } else if rect.contains(mouse) {
                draw_rectangle(rect.x, rect.y, rect.w, rect.h, lighter(PLATE, 1.6));
            }
            text(
                if on { "(*)" } else { "( )" },
                right_x + 6.0,
                ry + 15.0,
                13.0,
                INK,
            );
            text(k.label(), right_x + 38.0, ry + 15.0, 13.0, INK);
            if click && rect.contains(mouse) {
                self.kind = k;
            }
            ry += 22.0;
        }
        ry += 4.0;

        // Per-experiment knobs (kill %, surge factor, partition direction).
        ry = self.draw_knobs(right_x, ry, b.w, mouse, click);
        ry += 6.0;

        // Preview the drill for the chosen target.
        text_bold("PREVIEW", right_x, ry + 12.0, 14.0, PARCHMENT);
        ry += 20.0;
        let mut runnable: Option<Experiment> = None;
        let mut restorable = false;
        match self.experiment() {
            None => {
                let hint = if self.kind.is_node() {
                    "pick a node on the left"
                } else {
                    "pick a target on the left"
                };
                text(hint, right_x, ry + 12.0, 13.0, DIM);
                ry += 18.0;
            }
            Some(exp) => {
                let plan = plan_chaos(&snap.hot.observed, &exp);
                if let Some(why) = &plan.refused {
                    text(
                        ascii(&format!("refused: {why}")),
                        right_x,
                        ry + 12.0,
                        13.0,
                        WARN,
                    );
                    ry += 18.0;
                } else {
                    // Dry-run: list the concrete steps that would run (capped).
                    text(
                        ascii(&format!("dry run - {} step(s):", plan.steps.len())),
                        right_x,
                        ry + 12.0,
                        13.0,
                        INK,
                    );
                    ry += 17.0;
                    for line in plan_summary(&plan, 5) {
                        text(
                            ascii(&format!("- {line}")),
                            right_x + 8.0,
                            ry + 11.0,
                            12.0,
                            DIM,
                        );
                        ry += 15.0;
                    }
                    ry += 3.0;
                    text(
                        ascii(&format!("blast radius: {} affected", plan.blast)),
                        right_x,
                        ry + 12.0,
                        13.0,
                        STRUCT,
                    );
                    ry += 18.0;
                    // The per-workload budget, only when a single workload is hit.
                    if !self.kind.is_node()
                        && let Some(wr) = &self.target
                        && let Some(st) = snap.hot.slo.get(wr)
                    {
                        text(
                            ascii(&format!(
                                "error budget now {:.0}%",
                                st.budget_remaining * 100.0
                            )),
                            right_x,
                            ry + 12.0,
                            13.0,
                            DIM,
                        );
                        ry += 18.0;
                    }
                    // Experiment-specific notes (broken-image ref, CNI caveat, …).
                    for (line, role) in preview_lines(&exp, &plan) {
                        text(ascii(&line), right_x, ry + 12.0, 12.0, role_color(role));
                        ry += 16.0;
                    }
                    if !plan.restore.is_empty() {
                        text("(restorable)", right_x, ry + 12.0, 12.0, GOOD);
                        ry += 16.0;
                        restorable = true;
                    }
                    runnable = Some(exp);
                }
            }
        }
        ry += 6.0;

        // Auto-restore toggle (only meaningful for a restorable drill).
        if restorable {
            let box_r = Rect::new(right_x, ry, 16.0, 16.0);
            draw_rectangle(
                box_r.x,
                box_r.y,
                box_r.w,
                box_r.h,
                if self.auto_restore {
                    darker(GOOD, 0.7)
                } else {
                    darker(PLATE, 1.2)
                },
            );
            draw_rectangle_lines(box_r.x, box_r.y, box_r.w, box_r.h, 1.0, STONE_EDGE);
            if self.auto_restore {
                text("x", box_r.x + 4.0, box_r.y + 13.0, 14.0, INK);
            }
            text(
                "auto-restore after 60s",
                right_x + 22.0,
                ry + 13.0,
                13.0,
                PARCHMENT,
            );
            if click && box_r.contains(mouse) {
                self.auto_restore = !self.auto_restore;
            }
            ry += 22.0;
        }
        ry += 2.0;

        // Run button (CRIT — destructive).
        let run_btn = Rect::new(right_x, ry, 170.0, 26.0);
        let enabled = runnable.is_some();
        let bg = if !enabled {
            darker(PLATE, 1.3)
        } else if run_btn.contains(mouse) {
            CRIT
        } else {
            darker(CRIT, 0.8)
        };
        draw_rectangle(run_btn.x, run_btn.y, run_btn.w, run_btn.h, bg);
        draw_rectangle_lines(
            run_btn.x,
            run_btn.y,
            run_btn.w,
            run_btn.h,
            1.0,
            if enabled { CRIT } else { DIM },
        );
        let rm = text_size("Run drill", 15.0);
        text(
            "Run drill",
            run_btn.x + (run_btn.w - rm.width) / 2.0,
            ry + 18.0,
            15.0,
            if enabled { INK } else { DIM },
        );
        let mut act_run = None;
        if click && enabled && run_btn.contains(mouse) {
            act_run = runnable.map(|exp| (exp, restorable && self.auto_restore));
        }

        // --- SCORECARD (after a drill) — spans the bottom ---------------------
        // Only for the currently-selected subject, so a lingering session from a
        // different target doesn't show under an unrelated preview.
        let mut restore_clicked = false;
        if let Some(sess) = session.filter(|s| self.subject().as_ref() == Some(&s.subject)) {
            let sy = bottom - 124.0;
            draw_line(b.x, sy, b.x + b.w, sy, 1.0, darker(PARCHMENT, 0.5));
            text_bold("SCORECARD", b.x, sy + 16.0, 14.0, PARCHMENT);
            let card = ChaosScorecard {
                kind: sess.score_kind,
                experiment: sess.experiment.clone(),
                target: sess.target_label.clone(),
                blast: sess.blast,
                budget_before: sess.budget_before,
                budget_after: sess.budget_after,
                dipped: sess.dipped,
                recovered: sess.recovered,
                recover_secs: sess.recover_secs,
                healthy_before: sess.healthy_before,
                detect_secs: sess.detect_secs(),
            };
            let mut cy = sy + 34.0;
            for (line, role) in scorecard_lines(&card) {
                text(ascii(&line), b.x + 8.0, cy, 13.0, role_color(role));
                cy += 16.0;
            }
            // Recovery curve (the watch set's ready-fraction over the drill).
            if sess.recovery_series.len() >= 2 {
                let spark = Rect::new(b.x + b.w - 150.0, sy + 38.0, 142.0, 30.0);
                text("recovery", spark.x, spark.y - 3.0, 11.0, DIM);
                crate::panels::draw_sparkline(spark, &sess.recovery_series, 1.0, GOOD);
            }
            // Per-step errors, if any.
            if let Some(out) = &sess.outcome {
                for row in out.rows.iter().filter(|r| !r.ok) {
                    text(
                        ascii(&format!("! {}: {}", row.label, row.detail)),
                        b.x + 8.0,
                        cy,
                        12.0,
                        CRIT,
                    );
                    cy += 15.0;
                }
            }
            if !sess.restore.is_empty() {
                let rb = Rect::new(b.x + b.w - 170.0, sy + 8.0, 170.0, 24.0);
                let on = rb.contains(mouse);
                draw_rectangle(
                    rb.x,
                    rb.y,
                    rb.w,
                    rb.h,
                    if on { GOOD } else { darker(GOOD, 0.7) },
                );
                draw_rectangle_lines(rb.x, rb.y, rb.w, rb.h, 1.0, GOOD);
                let m = text_size("Restore", 14.0);
                text(
                    "Restore",
                    rb.x + (rb.w - m.width) / 2.0,
                    sy + 24.0,
                    14.0,
                    INK,
                );
                if click && on {
                    restore_clicked = true;
                }
            }
        }

        // Action precedence: run > restore > close.
        if let Some((exp, auto_restore)) = act_run {
            return ChaosAction::Run { exp, auto_restore };
        }
        if restore_clicked {
            return ChaosAction::Restore;
        }
        if click && (win.close.contains(mouse) || !win.frame.contains(mouse)) {
            return ChaosAction::Close;
        }
        ChaosAction::None
    }

    /// The per-experiment knob row (kill %, surge factor, partition direction),
    /// shown only for the kinds that take a parameter. Returns the new `y`.
    fn draw_knobs(&mut self, x: f32, y: f32, bw: f32, mouse: Vec2, click: bool) -> f32 {
        // A small [-]/[+] stepper; returns -1/0/+1 for the click.
        let stepper = |label: &str, val: String, sx: f32| -> i32 {
            text(label, sx, y + 14.0, 13.0, PARCHMENT);
            let lw = text_size(label, 13.0).width;
            let minus = Rect::new(sx + lw + 8.0, y, 20.0, 20.0);
            let val_x = minus.x + 26.0;
            let plus = Rect::new(
                val_x + text_size(&val, 13.0).width.max(28.0) + 8.0,
                y,
                20.0,
                20.0,
            );
            let mut delta = 0;
            for (rect, sign, glyph) in [(minus, -1, "-"), (plus, 1, "+")] {
                let on = rect.contains(mouse);
                draw_rectangle(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    if on {
                        lighter(PLATE, 1.8)
                    } else {
                        darker(PLATE, 1.2)
                    },
                );
                draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, STONE_EDGE);
                text(glyph, rect.x + 7.0, rect.y + 15.0, 14.0, INK);
                if on && click {
                    delta = sign;
                }
            }
            text(&val, val_x, y + 14.0, 13.0, INK);
            delta
        };
        match self.kind {
            ChaosKind::KillPercent => {
                let d = stepper("kill", format!("{}%", self.kill_pct), x);
                if d != 0 {
                    self.kill_pct = (self.kill_pct as i32 + d * 10).clamp(10, 100) as u8;
                }
                y + 24.0
            }
            ChaosKind::ScaleSpike => {
                let d = stepper("factor", format!("{}x", self.spike_factor), x);
                if d != 0 {
                    self.spike_factor = (self.spike_factor as i32 + d).clamp(2, 10) as u32;
                }
                y + 24.0
            }
            ChaosKind::Partition => {
                text("deny", x, y + 14.0, 13.0, PARCHMENT);
                let mut bx = x + 42.0;
                for (dir, lbl) in [
                    (PartitionDir::Both, "both"),
                    (PartitionDir::Ingress, "in"),
                    (PartitionDir::Egress, "out"),
                ] {
                    let w = text_size(lbl, 13.0).width + 14.0;
                    let rect = Rect::new(bx, y, w, 20.0);
                    let on = self.partition_dir == dir;
                    draw_rectangle(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        if on {
                            darker(STRUCT, 0.7)
                        } else if rect.contains(mouse) {
                            lighter(PLATE, 1.8)
                        } else {
                            darker(PLATE, 1.2)
                        },
                    );
                    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, STONE_EDGE);
                    text(lbl, rect.x + 7.0, rect.y + 15.0, 13.0, INK);
                    if click && rect.contains(mouse) {
                        self.partition_dir = dir;
                    }
                    bx += w + 6.0;
                }
                let _ = bw;
                y + 24.0
            }
            _ => y,
        }
    }

    /// The hot workloads list (protected namespaces filtered out).
    #[allow(clippy::too_many_arguments)]
    fn draw_workload_picker(
        &mut self,
        snap: &Snapshot,
        left_x: f32,
        left_w: f32,
        top: f32,
        bottom: f32,
        mouse: Vec2,
        click: bool,
    ) {
        text_bold("RAID TARGET", left_x, top + 12.0, 14.0, PARCHMENT);
        let row_h = 18.0;
        let mut ly = top + 26.0;
        let max_rows = (((bottom - ly) / row_h) as usize).max(1);
        let workloads: Vec<&_> = snap
            .hot
            .models
            .workloads
            .iter()
            .filter(|w| !ns_protected(&w.r.namespace))
            .collect();
        for wl in workloads.iter().take(max_rows) {
            let rect = Rect::new(left_x, ly, left_w, row_h);
            let is_target = self.target.as_ref() == Some(&wl.r);
            if rect.contains(mouse) {
                draw_rectangle(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Color::new(1.0, 1.0, 1.0, 0.06),
                );
            }
            if is_target {
                draw_rectangle(rect.x, rect.y + 1.0, 3.0, row_h - 2.0, CRIT);
            }
            let label = format!(
                "{} {}/{}",
                wl.r.kind,
                wl.r.namespace,
                truncate_str(&wl.r.name, 22)
            );
            text(
                ascii(&label),
                left_x + 8.0,
                ly + 13.0,
                12.0,
                if is_target { INK } else { PARCHMENT },
            );
            if click && rect.contains(mouse) {
                self.target = Some(wl.r.clone());
            }
            ly += row_h;
        }
        if workloads.len() > max_rows {
            text(
                format!("+{} more", workloads.len() - max_rows),
                left_x + 8.0,
                ly + 12.0,
                12.0,
                DIM,
            );
        }
    }

    /// The hot nodes list (control-plane nodes filtered out).
    #[allow(clippy::too_many_arguments)]
    fn draw_node_picker(
        &mut self,
        snap: &Snapshot,
        left_x: f32,
        left_w: f32,
        top: f32,
        bottom: f32,
        mouse: Vec2,
        click: bool,
    ) {
        text_bold("TARGET NODE", left_x, top + 12.0, 14.0, PARCHMENT);
        let row_h = 18.0;
        let mut ly = top + 26.0;
        let max_rows = (((bottom - ly) / row_h) as usize).max(1);
        let nodes: Vec<String> = snap
            .hot
            .observed
            .nodes
            .state()
            .iter()
            .filter(|n| !node_protected(n))
            .filter_map(|n| n.metadata.name.clone())
            .collect();
        for name in nodes.iter().take(max_rows) {
            let rect = Rect::new(left_x, ly, left_w, row_h);
            let is_target = self.node_target.as_deref() == Some(name.as_str());
            if rect.contains(mouse) {
                draw_rectangle(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Color::new(1.0, 1.0, 1.0, 0.06),
                );
            }
            if is_target {
                draw_rectangle(rect.x, rect.y + 1.0, 3.0, row_h - 2.0, CRIT);
            }
            text(
                ascii(&truncate_str(name, 32)),
                left_x + 8.0,
                ly + 13.0,
                12.0,
                if is_target { INK } else { PARCHMENT },
            );
            if click && rect.contains(mouse) {
                self.node_target = Some(name.clone());
            }
            ly += row_h;
        }
        if nodes.is_empty() {
            text("no drainable nodes", left_x + 8.0, ly + 12.0, 12.0, DIM);
        } else if nodes.len() > max_rows {
            text(
                format!("+{} more", nodes.len() - max_rows),
                left_x + 8.0,
                ly + 12.0,
                12.0,
                DIM,
            );
        }
    }
}

fn role_color(role: ScoreRole) -> Color {
    match role {
        ScoreRole::Good => GOOD,
        ScoreRole::Warn => WARN,
        ScoreRole::Bad => CRIT,
        ScoreRole::Info => INK,
    }
}
