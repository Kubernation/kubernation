//! The Game Day window — a chaos drill console. Pick a workload (a "city" to
//! raid), choose an experiment (outage / kill one / kill all), preview its
//! blast radius + the budget it'll spend, then run it (a confirmed, real write).
//! After it runs, a scorecard shows the cluster's response (recovery time +
//! budget spent), and an outage offers a Restore. The drill logic + guards are
//! pure in `kubernation_core::state::chaos`; this is the modal on `window.rs`.

use kubernation_core::state::chaos::{
    ChaosScorecard, Experiment, ScoreRole, ns_protected, plan_chaos, scorecard_lines,
};
use kubernation_core::state::model::WorkloadRef;
use macroquad::prelude::*;

use crate::net::{ChaosSession, Snapshot};
use crate::panels::truncate_str;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

const W: f32 = 760.0;
const H: f32 = 560.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChaosKind {
    Outage,
    KillOne,
    KillAll,
}

impl ChaosKind {
    const ALL: [ChaosKind; 3] = [ChaosKind::Outage, ChaosKind::KillOne, ChaosKind::KillAll];
    fn label(self) -> &'static str {
        match self {
            ChaosKind::Outage => "Outage (scale to 0)",
            ChaosKind::KillOne => "Kill one pod",
            ChaosKind::KillAll => "Kill all pods",
        }
    }
    fn experiment(self, wr: WorkloadRef) -> Experiment {
        match self {
            ChaosKind::Outage => Experiment::Outage { workload: wr },
            ChaosKind::KillOne => Experiment::KillOne { workload: wr },
            ChaosKind::KillAll => Experiment::KillAll { workload: wr },
        }
    }
}

/// What a frame's interaction asks the caller to do.
pub enum ChaosAction {
    None,
    Close,
    /// Raise the confirm for this experiment, then run it.
    Run(Experiment),
    /// Re-submit the live session's restore (scale back up).
    Restore,
}

/// The Game Day modal's state: the chosen target + experiment.
pub struct Chaos {
    pub target: Option<WorkloadRef>,
    kind: ChaosKind,
}

impl Chaos {
    pub fn new(target: Option<WorkloadRef>) -> Self {
        Chaos {
            target,
            kind: ChaosKind::Outage,
        }
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

        // --- LEFT: target picker (hot workloads) ------------------------------
        text_bold("RAID TARGET", left_x, top + 12.0, 14.0, PARCHMENT);
        let row_h = 18.0;
        let mut ly = top + 26.0;
        let max_rows = (((bottom - ly) / row_h) as usize).max(1);
        // Protected (control-plane / system) namespaces aren't targetable.
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

        // --- RIGHT: experiment + preview + run --------------------------------
        let mut ry = top;
        text_bold("EXPERIMENT", right_x, ry + 12.0, 14.0, PARCHMENT);
        ry += 24.0;
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
            ry += 24.0;
        }
        ry += 10.0;

        // Preview the drill for the chosen target.
        text_bold("PREVIEW", right_x, ry + 12.0, 14.0, PARCHMENT);
        ry += 22.0;
        let mut runnable: Option<Experiment> = None;
        match &self.target {
            None => {
                text("pick a target on the left", right_x, ry + 12.0, 13.0, DIM);
                ry += 18.0;
            }
            Some(wr) => {
                let exp = self.kind.experiment(wr.clone());
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
                    text(
                        ascii(&format!("{} cluster step(s)", plan.steps.len())),
                        right_x,
                        ry + 12.0,
                        13.0,
                        INK,
                    );
                    ry += 18.0;
                    text(
                        ascii(&format!("blast radius: {} affected", plan.blast)),
                        right_x,
                        ry + 12.0,
                        13.0,
                        STRUCT,
                    );
                    ry += 18.0;
                    if let Some(st) = snap.hot.slo.get(wr) {
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
                    if !plan.restore.is_empty() {
                        text("(restorable)", right_x, ry + 12.0, 12.0, GOOD);
                        ry += 16.0;
                    }
                    runnable = Some(exp);
                }
            }
        }
        ry += 8.0;

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
            act_run = runnable;
        }

        // --- SCORECARD (after a drill) — spans the bottom ---------------------
        // Only for the currently-open target, so a lingering session from a
        // different workload doesn't show under an unrelated preview.
        let mut restore_clicked = false;
        if let Some(sess) = session.filter(|s| self.target.as_ref() == Some(&s.target)) {
            let sy = bottom - 120.0;
            draw_line(b.x, sy, b.x + b.w, sy, 1.0, darker(PARCHMENT, 0.5));
            text_bold("SCORECARD", b.x, sy + 16.0, 14.0, PARCHMENT);
            let card = ChaosScorecard {
                experiment: sess.experiment.clone(),
                target: format!("{}/{}", sess.target.namespace, sess.target.name),
                blast: sess.blast,
                budget_before: sess.budget_before,
                budget_after: sess.budget_after,
                dipped: sess.dipped,
                recovered: sess.recovered,
                recover_secs: sess.recover_secs,
            };
            let mut cy = sy + 34.0;
            for (line, role) in scorecard_lines(&card) {
                text(ascii(&line), b.x + 8.0, cy, 13.0, role_color(role));
                cy += 17.0;
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
        if let Some(exp) = act_run {
            return ChaosAction::Run(exp);
        }
        if restore_clicked {
            return ChaosAction::Restore;
        }
        if click && (win.close.contains(mouse) || !win.frame.contains(mouse)) {
            return ChaosAction::Close;
        }
        ChaosAction::None
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
