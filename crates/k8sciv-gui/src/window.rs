//! A reusable centered modal window — the chrome that drill-down content
//! shares (the Almanac now; richer city/node detail next). One modal is open
//! at a time; it draws a dimmed scrim, a parchment-framed panel with a title
//! bar + icon, a button/tab row, and hands back a clipped body rect the
//! caller fills (culling + scrolling its own content, since macroquad has no
//! easy scissor). Mirrors the Civ II window structure (titlebar / body /
//! buttons) in the K8sCiv parchment palette.

use k8sciv_core::state::planned::Intervention;
use macroquad::prelude::*;

use crate::text::{text, text_bold, text_size};
use crate::theme::*;

const TITLE_H: f32 = 30.0;
const BTN_H: f32 = 26.0;
const PAD: f32 = 14.0;

/// Hit regions a drawn window hands back for this frame.
pub struct WinLayout {
    pub frame: Rect,
    /// Content region — draw here, culling rows outside it yourself.
    pub body: Rect,
    /// Bottom button/tab rects, in the order passed.
    pub buttons: Vec<Rect>,
    /// The top-right close box.
    pub close: Rect,
}

impl WinLayout {
    /// Index of the button under `p`, if any.
    pub fn button_at(&self, p: Vec2) -> Option<usize> {
        self.buttons.iter().position(|r| r.contains(p))
    }
}

/// What a frame's interaction on a drill-down window asks the caller to do.
/// Shared by the city and node windows.
#[derive(Default)]
pub struct WinAction {
    pub close: bool,
    /// A pod whose logs to tail: (namespace, pod).
    pub log: Option<(String, String)>,
    /// An intervention the operator staged from this window (planning turn).
    pub stage: Option<Intervention>,
}

/// Draw the scrim, frame, title bar (with icon), and bottom button row;
/// return the hit regions. `active` highlights that button as the current
/// tab (pass `usize::MAX` for none).
pub fn draw_window(title: &str, size: Vec2, buttons: &[&str], active: usize) -> WinLayout {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.5),
    );
    let w = size.x.min(screen_width() - 40.0);
    let h = size.y.min(screen_height() - 40.0);
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    let frame = Rect::new(x, y, w, h);
    let mp = Vec2::from(mouse_position());

    draw_rectangle(x, y, w, h, PANEL);
    draw_rectangle_lines(x, y, w, h, 2.0, PARCHMENT);

    // Title bar.
    draw_rectangle(x, y, w, TITLE_H, darker(PANEL, 0.7));
    draw_rectangle(x, y + TITLE_H, w, 1.5, PARCHMENT);
    draw_icon(vec2(x + 8.0, y + 6.0), 18.0);
    text_bold(title, x + 34.0, y + 21.0, 18.0, PARCHMENT);

    // Close box (Esc also closes; this mirrors Civ II's window button).
    let close = Rect::new(x + w - 26.0, y + 5.0, 20.0, 20.0);
    let close_bg = if close.contains(mp) {
        lighter(PLATE, 1.9)
    } else {
        PLATE
    };
    draw_rectangle(close.x, close.y, close.w, close.h, close_bg);
    draw_rectangle_lines(close.x, close.y, close.w, close.h, 1.0, PARCHMENT);
    text("x", close.x + 6.0, close.y + 15.0, 16.0, INK);

    // Bottom button / tab row.
    let row_y = y + h - BTN_H - 8.0;
    let mut rects = Vec::new();
    if !buttons.is_empty() {
        let n = buttons.len() as f32;
        let bw = (w - PAD * 2.0 - (n - 1.0) * 8.0) / n;
        for (i, label) in buttons.iter().enumerate() {
            let bx = x + PAD + i as f32 * (bw + 8.0);
            let r = Rect::new(bx, row_y, bw, BTN_H);
            let on = i == active;
            let bg = if on {
                PARCHMENT
            } else if r.contains(mp) {
                lighter(PLATE, 1.7)
            } else {
                PLATE
            };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, PARCHMENT);
            let tc = if on { PANEL } else { INK };
            let tm = text_size(ascii(label), 14.0);
            text(
                ascii(label),
                r.x + (r.w - tm.width) / 2.0,
                r.y + 17.0,
                14.0,
                tc,
            );
            rects.push(r);
        }
    }

    let body_top = y + TITLE_H + 10.0;
    let body_bottom = if buttons.is_empty() {
        y + h - 10.0
    } else {
        row_y - 8.0
    };
    let body = Rect::new(
        x + PAD,
        body_top,
        w - PAD * 2.0,
        (body_bottom - body_top).max(0.0),
    );
    WinLayout {
        frame,
        body,
        buttons: rects,
        close,
    }
}

/// A little book/scroll glyph for the title bar.
fn draw_icon(p: Vec2, s: f32) {
    draw_rectangle(p.x, p.y, s, s, PARCHMENT);
    draw_rectangle_lines(p.x, p.y, s, s, 1.0, darker(PARCHMENT, 0.5));
    draw_line(
        p.x + s * 0.5,
        p.y + 2.0,
        p.x + s * 0.5,
        p.y + s - 2.0,
        1.0,
        darker(PARCHMENT, 0.45),
    );
    draw_line(
        p.x + 3.0,
        p.y + s * 0.35,
        p.x + s * 0.45,
        p.y + s * 0.35,
        1.0,
        darker(PARCHMENT, 0.45),
    );
}
