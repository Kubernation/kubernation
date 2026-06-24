//! The End-of-Turn review — the planning turn's staged-diff window, and the
//! one place the staged interventions are *committed*.
//!
//! It lists what would change (`plan_diff`), lets the operator unstage a row
//! or discard the turn, and **Commit** (behind a confirm) applies the staged
//! changes to the hot cluster. Every change is validated with a server-side
//! dry-run first — which also enforces RBAC — so a turn that the cluster would
//! reject is blocked before any real write, and the per-row outcome is shown
//! here.

use macroquad::prelude::*;

use kubernation_core::state::planned::{PlannedWorld, plan_diff};

use crate::net::{PlanOutcome, Snapshot};
use crate::panels::truncate_str;
use crate::text::text;
use crate::theme::*;
use crate::window::draw_window;

/// What the review window asks the caller to do this frame.
#[derive(Default)]
pub struct PlanAction {
    pub close: bool,
    pub discard: bool,
    pub unstage: Option<usize>,
    /// The operator clicked Commit (the caller confirms, then applies).
    pub commit: bool,
}

pub fn draw_plan(
    planned: &PlannedWorld,
    snap: Option<&Snapshot>,
    outcome: Option<&PlanOutcome>,
    mouse: Vec2,
    click: bool,
) -> PlanAction {
    let mut act = PlanAction::default();
    let win = draw_window(
        "End of Turn — staged interventions",
        vec2(720.0, 540.0),
        &["Discard all", "Close"],
        usize::MAX,
    );
    let b = win.body;
    let mut y = b.y + 6.0;

    let changes = snap
        .map(|s| plan_diff(&s.hot.observed, planned))
        .unwrap_or_default();
    let appliable = changes.iter().filter(|c| !c.noop).count();

    if changes.is_empty() {
        text(
            "Nothing staged. Open a city or province and use its plan controls.",
            b.x,
            y + 14.0,
            15.0,
            DIM,
        );
    } else {
        // Header reflects the latest commit outcome, if any.
        let (header, hcol) = match outcome {
            Some(o) if o.applied => {
                let n_ok = o.rows.iter().filter(|r| r.ok).count();
                let col = if n_ok == o.rows.len() {
                    gauge_ok()
                } else {
                    WARN
                };
                (format!("Committed {n_ok}/{} change(s).", o.rows.len()), col)
            }
            Some(_) => (
                "Commit blocked by dry-run — fix the flagged change(s) and retry.".to_string(),
                CRIT,
            ),
            None => (
                format!("{appliable} change(s) to apply — review, then Commit."),
                PARCHMENT,
            ),
        };
        text(ascii(&header), b.x, y + 13.0, 14.0, hcol);
        y += 28.0;

        // Reserve a bottom strip for the per-row commit result, if present.
        let result_rows = outcome.map(|o| o.rows.len().min(4)).unwrap_or(0);
        let result_h = if result_rows > 0 {
            result_rows as f32 * 18.0 + 10.0
        } else {
            0.0
        };
        let list_bottom = b.y + b.h - 38.0 - result_h;

        let row_h = 24.0;
        for (i, c) in changes.iter().enumerate() {
            if y + row_h > list_bottom {
                text(
                    format!("+{} more", changes.len() - i),
                    b.x + 6.0,
                    y + 16.0,
                    13.0,
                    DIM,
                );
                break;
            }
            if Rect::new(b.x, y, b.w, row_h).contains(mouse) {
                draw_rectangle(
                    b.x - 4.0,
                    y,
                    b.w + 8.0,
                    row_h,
                    Color::new(1.0, 1.0, 1.0, 0.05),
                );
            }
            text(ascii(&c.target), b.x + 6.0, y + 16.0, 14.0, INK);
            let chg = format!("{}  {} -> {}", c.field, c.from, c.to);
            let col = if c.noop { DIM } else { WARN };
            text(&chg, b.x + 300.0, y + 16.0, 14.0, col);
            if c.noop {
                text("(no change)", b.x + 500.0, y + 16.0, 12.0, DIM);
            }
            let xbtn = Rect::new(b.x + b.w - 24.0, y + 2.0, 20.0, 20.0);
            let xbg = if xbtn.contains(mouse) {
                lighter(PLATE, 1.8)
            } else {
                PLATE
            };
            draw_rectangle(xbtn.x, xbtn.y, xbtn.w, xbtn.h, xbg);
            draw_rectangle_lines(xbtn.x, xbtn.y, xbtn.w, xbtn.h, 1.0, PARCHMENT);
            text("x", xbtn.x + 6.0, xbtn.y + 15.0, 14.0, INK);
            if click && xbtn.contains(mouse) {
                act.unstage = Some(i);
            }
            y += row_h;
        }

        // Per-row commit result.
        if let Some(o) = outcome {
            let mut ry = list_bottom + 6.0;
            draw_line(b.x, ry, b.x + b.w, ry, 1.0, darker(PARCHMENT, 0.5));
            ry += 4.0;
            for r in o.rows.iter().take(4) {
                let (mark, col) = if r.ok {
                    ("ok ", gauge_ok())
                } else {
                    ("x  ", CRIT)
                };
                let line = if r.detail.is_empty() {
                    format!("{mark}{}", r.label)
                } else {
                    format!("{mark}{} — {}", r.label, truncate_str(&r.detail, 60))
                };
                text(ascii(&line), b.x + 6.0, ry + 14.0, 12.0, col);
                ry += 18.0;
            }
        }
    }

    // Commit — enabled when there's something to apply. Behind a confirm in the
    // caller; the apply itself is server-side dry-run-gated.
    let commit = Rect::new(b.x, b.y + b.h - 28.0, 150.0, 24.0);
    let enabled = appliable > 0;
    let cbg = if !enabled {
        darker(PLATE, 0.7)
    } else if commit.contains(mouse) {
        lighter(gauge_ok(), 1.25)
    } else {
        gauge_ok()
    };
    draw_rectangle(commit.x, commit.y, commit.w, commit.h, cbg);
    draw_rectangle_lines(
        commit.x,
        commit.y,
        commit.w,
        commit.h,
        1.0,
        darker(PARCHMENT, 0.5),
    );
    let clabel = format!("Commit ({appliable})");
    text(
        &clabel,
        commit.x + 12.0,
        commit.y + 16.0,
        14.0,
        if enabled { INK } else { DIM },
    );
    text(
        "applies to the cluster — dry-run validated, then confirmed",
        commit.x + 164.0,
        commit.y + 16.0,
        13.0,
        DIM,
    );
    if enabled && click && commit.contains(mouse) {
        act.commit = true;
    }

    if act.unstage.is_none() && !act.commit && click {
        if win.close.contains(mouse) {
            act.close = true;
        } else if let Some(bi) = win.button_at(mouse) {
            if bi == 0 {
                act.discard = true;
            } else {
                act.close = true;
            }
        } else if !win.frame.contains(mouse) {
            act.close = true;
        }
    }
    act
}
