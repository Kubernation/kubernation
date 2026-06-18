//! The GUI resource browser — the `:` command. A modal with two modes: pick a
//! kind (a scrollable, clickable list of everything the cluster serves), then a
//! generic table of that kind's objects; click a row to drill into the YAML
//! inspector. Mouse + wheel driven (the TUI has the keyboard/filter idiom). The
//! data (discovery + LIST) comes from the net thread via `Net`.

use kubernation_core::k8s::browse::{Object, row};
use macroquad::prelude::*;

use crate::net::Net;
use crate::text::text;
use crate::theme::*;
use crate::window::draw_window;

pub enum BrowseAction {
    None,
    Close,
    /// Boxed — `Object` is large vs. the unit variants.
    Inspect(Box<Object>),
}

#[derive(PartialEq)]
enum Mode {
    Pick,
    Table,
}

pub struct Browser {
    mode: Mode,
    kind_label: String,
    scroll: f32,
    max_scroll: f32,
}

impl Browser {
    pub fn new() -> Self {
        Browser {
            mode: Mode::Pick,
            kind_label: String::new(),
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    /// Jump straight to a kind's table (used by the `--browse` dev flag).
    pub fn force_table(&mut self, label: String) {
        self.kind_label = label;
        self.mode = Mode::Table;
        self.scroll = 0.0;
    }

    /// Esc: in the table, go back to the kind list (returns true = stay open);
    /// in the kind list, ask to close (returns false).
    pub fn back(&mut self) -> bool {
        if self.mode == Mode::Table {
            self.mode = Mode::Pick;
            self.scroll = 0.0;
            true
        } else {
            false
        }
    }

    pub fn draw(&mut self, net: &Net, mouse: Vec2, click: bool) -> BrowseAction {
        match self.mode {
            Mode::Pick => self.draw_pick(net, mouse, click),
            Mode::Table => self.draw_table(net, mouse, click),
        }
    }

    fn draw_pick(&mut self, net: &Net, mouse: Vec2, click: bool) -> BrowseAction {
        let win = draw_window(
            ":resource — pick a kind",
            vec2(560.0, 560.0),
            &["Close"],
            usize::MAX,
        );
        let b = win.body;
        let row_h = 18.0;
        let fs = 13.0;
        let mut act = BrowseAction::None;
        match net.kinds() {
            None => {
                text("discovering kinds…", b.x + 4.0, b.y + 18.0, fs, DIM);
            }
            Some(kinds) => {
                let mut y = b.y - self.scroll + 14.0;
                for k in &kinds {
                    let rect = Rect::new(b.x, y - 13.0, b.w, row_h);
                    if y > b.y && y < b.y + b.h {
                        if rect.contains(mouse) {
                            draw_rectangle(
                                rect.x,
                                rect.y,
                                rect.w,
                                rect.h,
                                Color::new(1.0, 1.0, 1.0, 0.06),
                            );
                        }
                        let scope = if k.namespaced { "" } else { "  ·  cluster" };
                        text(format!("{}{scope}", k.label()), b.x + 6.0, y, fs, INK);
                    }
                    if click && rect.contains(mouse) {
                        net.request_browse(k.clone());
                        self.kind_label = k.label();
                        self.mode = Mode::Table;
                        self.scroll = 0.0;
                    }
                    y += row_h;
                }
                self.max_scroll = ((kinds.len() as f32 * row_h + 18.0) - b.h).max(0.0);
                self.scroll = self.scroll.min(self.max_scroll);
                scrollbar(b, self.scroll, self.max_scroll);
            }
        }
        if click
            && self.mode == Mode::Pick
            && (win.close.contains(mouse) || !win.frame.contains(mouse))
        {
            act = BrowseAction::Close;
        }
        act
    }

    fn draw_table(&mut self, net: &Net, mouse: Vec2, click: bool) -> BrowseAction {
        let win = draw_window(
            &format!("browse: {}", self.kind_label),
            vec2(660.0, 560.0),
            &["Close"],
            usize::MAX,
        );
        let b = win.body;
        let row_h = 18.0;
        let fs = 13.0;
        let mut act = BrowseAction::None;

        // A clickable "‹ kinds" back link on the first row.
        let back = Rect::new(b.x, b.y, 120.0, 16.0);
        if back.contains(mouse) {
            draw_rectangle(
                back.x,
                back.y,
                back.w,
                back.h,
                Color::new(1.0, 1.0, 1.0, 0.06),
            );
        }
        text("‹ kinds", b.x + 4.0, b.y + 12.0, fs, STRUCT);
        if click && back.contains(mouse) {
            self.mode = Mode::Pick;
            self.scroll = 0.0;
            return BrowseAction::None;
        }
        let top = b.y + 22.0;
        let view_h = (b.y + b.h - top).max(0.0);

        let out = net.browse_out();
        match &out.result {
            None => {
                text("listing…", b.x + 6.0, top + 14.0, fs, DIM);
            }
            Some(Err(e)) => {
                text(
                    format!("could not list: {e}"),
                    b.x + 6.0,
                    top + 14.0,
                    fs,
                    CRIT,
                );
            }
            Some(Ok(lr)) if lr.items.is_empty() => {
                text("(no objects)", b.x + 6.0, top + 14.0, fs, DIM);
            }
            Some(Ok(lr)) => {
                let mut y = top - self.scroll + 14.0;
                for o in &lr.items {
                    let r = row(o);
                    let mut name = if r.namespace.is_empty() {
                        r.name.clone()
                    } else {
                        format!("{}/{}", r.namespace, r.name)
                    };
                    // Keep the age column aligned: clip an over-long name so it
                    // can't overrun into the age (padding alone never truncates).
                    // Char-based — `String::truncate` is byte-indexed and would
                    // panic mid-codepoint (browsed objects can be any kind).
                    if name.chars().count() > 56 {
                        name = name.chars().take(55).collect::<String>();
                        name.push('…');
                    }
                    let rect = Rect::new(b.x, y - 13.0, b.w, row_h);
                    if y > top && y < b.y + b.h {
                        if rect.contains(mouse) {
                            draw_rectangle(
                                rect.x,
                                rect.y,
                                rect.w,
                                rect.h,
                                Color::new(1.0, 1.0, 1.0, 0.06),
                            );
                        }
                        text(format!("{name:<58}{}", r.age), b.x + 6.0, y, fs, INK);
                    }
                    if click && rect.contains(mouse) {
                        act = BrowseAction::Inspect(Box::new(o.clone()));
                    }
                    y += row_h;
                }
                // A capped LIST: tell the user the view is incomplete.
                if lr.truncated {
                    text(
                        format!("… showing first {} (more on the server)", lr.items.len()),
                        b.x + 6.0,
                        y,
                        fs,
                        DIM,
                    );
                    y += row_h;
                }
                self.max_scroll = ((y - (top - self.scroll)) - view_h).max(0.0);
                self.scroll = self.scroll.min(self.max_scroll);
                let area = Rect::new(b.x, top, b.w, view_h);
                scrollbar(area, self.scroll, self.max_scroll);
            }
        }
        if click
            && !matches!(act, BrowseAction::Inspect(_))
            && (win.close.contains(mouse) || !win.frame.contains(mouse))
        {
            act = BrowseAction::Close;
        }
        act
    }
}

fn scrollbar(b: Rect, scroll: f32, max_scroll: f32) {
    if max_scroll <= 0.0 {
        return;
    }
    let content = b.h + max_scroll;
    let frac = (b.h / content).clamp(0.05, 1.0);
    let thumb = b.h * frac;
    let t = scroll / max_scroll;
    let ty = b.y + t * (b.h - thumb);
    draw_rectangle(b.x + b.w + 2.0, b.y, 3.0, b.h, darker(PANEL, 0.6));
    draw_rectangle(b.x + b.w + 2.0, ty, 3.0, thumb, PARCHMENT);
}
