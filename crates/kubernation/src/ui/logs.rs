//! The log view: a tail of one pod's logs, refreshed on a poll so it
//! reads as a live tail. The app owns the fetching (it has the client and
//! the async runtime); this component just holds and renders what arrives.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Action, Component, RenderCtx};
use crate::events::ClusterId;

#[derive(Default)]
pub struct LogsView {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
    lines: Vec<String>,
    error: Option<String>,
    loading: bool,
    /// Stick to the bottom (tail) until the user scrolls up.
    follow: bool,
    scroll: u16,
    last_h: u16,
}

impl LogsView {
    pub fn open(&mut self, cluster: ClusterId, namespace: String, pod: String) {
        self.cluster = cluster;
        self.namespace = namespace;
        self.pod = pod;
        self.lines.clear();
        self.error = None;
        self.loading = true;
        self.follow = true;
        self.scroll = 0;
    }

    /// Result of a fetch (whole tail as one string).
    pub fn set_result(&mut self, result: Result<String, String>) {
        self.loading = false;
        match result {
            Ok(text) => {
                self.lines = text.lines().map(|l| l.to_string()).collect();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn max_scroll(&self) -> u16 {
        let view = self.last_h.saturating_sub(2); // borders
        (self.lines.len() as u16).saturating_sub(view)
    }
}

impl Component for LogsView {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &RenderCtx) -> Option<Action> {
        let page = self.last_h.saturating_sub(3).max(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.follow = false;
                self.scroll = (self.scroll + 1).min(self.max_scroll());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::PageDown | KeyCode::Char('d')
                if key.code == KeyCode::PageDown
                    || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.follow = false;
                self.scroll = (self.scroll + page).min(self.max_scroll());
            }
            KeyCode::PageUp | KeyCode::Char('u')
                if key.code == KeyCode::PageUp || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(page);
            }
            KeyCode::Char('g') => {
                self.follow = false;
                self.scroll = 0;
            }
            KeyCode::Char('G') | KeyCode::Char('f') => self.follow = true,
            _ => {}
        }
        None
    }

    fn update(&mut self, _ctx: &RenderCtx) {}

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        self.last_h = area.height;
        if self.follow {
            self.scroll = self.max_scroll();
        }

        let world = match ctx.cluster_label {
            Some(l) => format!(" — {l}"),
            None => String::new(),
        };
        let follow = if self.follow { " ▸following" } else { "" };
        let title = format!(" logs {}/{}{world}{follow} ", self.namespace, self.pod);

        let body: Vec<Line> = if let Some(err) = &self.error {
            vec![Line::styled(
                format!("could not read logs: {err}"),
                theme.severity(kubernation_core::state::attention::Severity::Critical),
            )]
        } else if self.lines.is_empty() {
            let msg = if self.loading {
                "loading…"
            } else {
                "(no log lines)"
            };
            vec![Line::styled(msg, theme.dim())]
        } else {
            self.lines.iter().map(|l| Line::raw(l.clone())).collect()
        };

        let hint = " j/k scroll · g top · G/f follow · Esc back ";
        let block = Block::bordered()
            .border_style(theme.chrome())
            .title(title)
            .title_style(theme.title())
            .title_bottom(Line::styled(hint, theme.dim()).right_aligned());
        f.render_widget(
            Paragraph::new(body).block(block).scroll((self.scroll, 0)),
            area,
        );
    }
}
