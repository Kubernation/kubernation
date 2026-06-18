//! The object inspector — a read-only, scrollable YAML "dossier" for a single
//! resource (workload / node / pod), over `kubernation_core::state::inspect`.
//! A modal window like the Almanac/advisor; opened with `y` (or a pod row's
//! `yaml` button). No fetch, no writes — it serializes what's already in the
//! reflector store.

use macroquad::prelude::*;

use crate::text::{text, text_size};
use crate::theme::*;
use crate::window::draw_window;

/// Truncate a line (with a trailing "…") so it fits `max_w` — macroquad has no
/// scissor, so a long YAML line would otherwise run past the window edge.
fn fit(line: &str, max_w: f32, fs: f32) -> String {
    if max_w <= 0.0 || text_size(line, fs).width <= max_w {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    // Proportional first guess, then correct (Fira Sans isn't monospace).
    let full = text_size(line, fs).width.max(1.0);
    let mut n = (((max_w / full) * chars.len() as f32) as usize).clamp(1, chars.len());
    let cut = |n: usize| -> String { chars[..n].iter().collect::<String>() + "…" };
    while n > 1 && text_size(cut(n), fs).width > max_w {
        n -= 1;
    }
    while n < chars.len() && text_size(cut(n + 1), fs).width <= max_w {
        n += 1;
    }
    cut(n)
}

pub struct Inspector {
    title: String,
    lines: Vec<String>,
    scroll: f32,
    max_scroll: f32,
}

impl Inspector {
    /// `title` is "kind ns/name"; `yaml` is the cleaned document.
    pub fn new(title: String, yaml: String) -> Self {
        Inspector {
            title,
            lines: yaml.lines().map(|l| l.to_string()).collect(),
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    /// Draw the window + YAML; returns true when it should close.
    pub fn draw(&mut self, mouse: Vec2, click: bool) -> bool {
        let win = draw_window(&self.title, vec2(720.0, 600.0), &["Close"], usize::MAX);
        let b = win.body;
        let line_h = 16.0;
        let fs = 13.0;
        let mut y = b.y - self.scroll + 12.0;
        // Leave room for the left margin + the scrollbar gutter on the right.
        let max_w = b.w - 10.0;
        for line in &self.lines {
            if y > b.y && y < b.y + b.h {
                // Replace tabs (rare in YAML) so columns don't jump, then clip
                // to the body width (no scissor in macroquad).
                let s = ascii(&line.replace('\t', "  "));
                text(fit(&s, max_w, fs), b.x + 4.0, y, fs, INK);
            }
            y += line_h;
        }
        let content_h = self.lines.len() as f32 * line_h + 24.0;
        self.max_scroll = (content_h - b.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        if self.max_scroll > 0.0 {
            let frac = (b.h / content_h).clamp(0.05, 1.0);
            let thumb_h = b.h * frac;
            let t = self.scroll / self.max_scroll;
            let ty = b.y + t * (b.h - thumb_h);
            draw_rectangle(b.x + b.w + 2.0, b.y, 3.0, b.h, darker(PANEL, 0.6));
            draw_rectangle(b.x + b.w + 2.0, ty, 3.0, thumb_h, PARCHMENT);
        }

        click
            && (win.close.contains(mouse)
                || win.button_at(mouse).is_some()
                || !win.frame.contains(mouse))
    }
}

/// Build the "kind ns/name" title shown in the inspector titlebar.
pub fn title(kind: &str, namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        format!("{kind} {name}")
    } else {
        format!("{kind} {namespace}/{name}")
    }
}
