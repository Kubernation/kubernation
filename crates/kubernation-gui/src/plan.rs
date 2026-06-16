//! The End-of-Turn review — the planning turn's staged-diff window.
//!
//! Preview-only: it lists what *would* change (`plan_diff`) and lets the
//! operator unstage a row or discard the whole turn. **Commit is shown but
//! disabled** — nothing here writes to the cluster, so the codebase keeps its
//! "no mutation paths" guarantee while the planning experience exists.

use macroquad::prelude::*;

use kubernation_core::state::planned::{PlannedWorld, plan_diff};

use crate::net::Snapshot;
use crate::text::text;
use crate::theme::*;
use crate::window::draw_window;

/// What the review window asks the caller to do this frame.
#[derive(Default)]
pub struct PlanAction {
    pub close: bool,
    pub discard: bool,
    pub unstage: Option<usize>,
}

pub fn draw_plan(
    planned: &PlannedWorld,
    snap: Option<&Snapshot>,
    mouse: Vec2,
    click: bool,
) -> PlanAction {
    let mut act = PlanAction::default();
    let win = draw_window(
        "End of Turn — staged interventions",
        vec2(720.0, 520.0),
        &["Discard all", "Close"],
        usize::MAX,
    );
    let b = win.body;
    let mut y = b.y + 6.0;

    let changes = snap
        .map(|s| plan_diff(&s.hot.observed, planned))
        .unwrap_or_default();

    if changes.is_empty() {
        text(
            "Nothing staged. Open a city or province and use its plan controls.",
            b.x,
            y + 14.0,
            15.0,
            DIM,
        );
    } else {
        text(
            format!(
                "{} staged change(s) — review before committing:",
                changes.len()
            ),
            b.x,
            y + 13.0,
            14.0,
            PARCHMENT,
        );
        y += 28.0;
        let row_h = 24.0;
        for (i, c) in changes.iter().enumerate() {
            if y + row_h > b.y + b.h - 36.0 {
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
            // Unstage [x].
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
    }

    // Commit — shown but disabled (preview-only; the apply path doesn't exist).
    let commit = Rect::new(b.x, b.y + b.h - 28.0, 150.0, 24.0);
    draw_rectangle(commit.x, commit.y, commit.w, commit.h, darker(PLATE, 0.7));
    draw_rectangle_lines(
        commit.x,
        commit.y,
        commit.w,
        commit.h,
        1.0,
        darker(PARCHMENT, 0.5),
    );
    text("Commit", commit.x + 12.0, commit.y + 16.0, 14.0, DIM);
    text(
        "preview only — nothing is applied to the cluster",
        commit.x + 164.0,
        commit.y + 16.0,
        13.0,
        DIM,
    );

    if act.unstage.is_none() && click {
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
