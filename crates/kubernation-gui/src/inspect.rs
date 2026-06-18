//! The object inspector — a read-only, scrollable YAML "dossier" for a single
//! resource (workload / node / pod), over `kubernation_core::state::inspect`.
//! A modal window like the Almanac/advisor; opened with `y` (or a pod row's
//! `yaml` button). No fetch, no writes — it serializes what's already in the
//! reflector store.

use macroquad::prelude::*;

use crate::text::text;
use crate::theme::*;
use crate::window::draw_window;

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
        for line in &self.lines {
            if y > b.y - line_h && y < b.y + b.h {
                // Replace tabs (rare in YAML) so columns don't jump.
                text(ascii(&line.replace('\t', "  ")), b.x + 4.0, y, fs, INK);
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
