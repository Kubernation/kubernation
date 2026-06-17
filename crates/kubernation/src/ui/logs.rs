//! The log view: a tail of one pod's logs, refreshed on a poll so it
//! reads as a live tail. The app owns the fetching (it has the client and
//! the async runtime); this component just holds and renders what arrives.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Action, Component, RenderCtx};
use crate::events::ClusterId;

#[derive(Default)]
pub struct LogsView {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
    /// Tail the previously-terminated container (`kubectl logs --previous`).
    /// The app reads this when (re)fetching; toggled in-view with `p`.
    pub previous: bool,
    lines: Vec<String>,
    error: Option<String>,
    loading: bool,
    /// Stick to the bottom (tail) until the user scrolls up.
    follow: bool,
    scroll: u16,
    last_h: u16,
    /// Case-insensitive substring filter over the fetched tail (`/` to edit).
    filter: String,
    /// True while the user is typing into the filter (app routes keys here).
    filtering: bool,
}

impl LogsView {
    pub fn open(&mut self, cluster: ClusterId, namespace: String, pod: String) {
        self.cluster = cluster;
        self.namespace = namespace;
        self.pod = pod;
        self.previous = false;
        self.lines.clear();
        self.error = None;
        self.loading = true;
        self.follow = true;
        self.scroll = 0;
        self.filter.clear();
        self.filtering = false;
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

    /// Whether the filter edit-mode is capturing input (the app checks this to
    /// route keystrokes here instead of to its global bindings).
    pub fn filtering(&self) -> bool {
        self.filtering
    }

    /// Feed one keystroke to the filter while editing (`/` mode). Enter / Esc
    /// finish editing (keeping the text); Backspace deletes.
    pub fn filter_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.follow = true; // jump to the newest matches
            }
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Enter | KeyCode::Esc => self.filtering = false,
            _ => {}
        }
    }

    /// Lines currently shown — all of them, or only those matching the filter.
    fn visible(&self) -> Vec<&String> {
        if self.filter.is_empty() {
            return self.lines.iter().collect();
        }
        let needle = self.filter.to_lowercase();
        self.lines
            .iter()
            .filter(|l| l.to_lowercase().contains(&needle))
            .collect()
    }

    fn max_scroll_for(&self, visible_len: usize) -> u16 {
        let view = self.last_h.saturating_sub(2); // borders
        (visible_len as u16).saturating_sub(view)
    }

    fn max_scroll(&self) -> u16 {
        self.max_scroll_for(self.visible().len())
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
            // `/` opens the filter editor (the app then routes keys to us).
            KeyCode::Char('/') => self.filtering = true,
            // `p` toggles the previous-container tail and asks for a re-fetch.
            KeyCode::Char('p') => {
                self.previous = !self.previous;
                self.lines.clear();
                self.error = None;
                self.loading = true;
                self.follow = true;
                return Some(Action::RefetchLogs);
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, _ctx: &RenderCtx) {}

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        self.last_h = area.height;

        // Filter once per frame; reuse for both the scroll math and the body.
        let visible = self.visible();
        let max = self.max_scroll_for(visible.len());
        // Keep scroll in range even when not following — a live-tail poll can
        // shrink the (filtered) set under us, which would blank the body. The
        // write to `self.scroll` is deferred past `visible`'s last use below
        // (it borrows `self`); rendering uses this resolved value.
        let scroll = if self.follow {
            max
        } else {
            self.scroll.min(max)
        };

        let world = match ctx.cluster_label {
            Some(l) => format!(" — {l}"),
            None => String::new(),
        };
        let follow = if self.follow { " ▸following" } else { "" };
        let prev = if self.previous { " ‹previous›" } else { "" };
        let title = format!(
            " logs {}/{}{world}{prev}{follow} ",
            self.namespace, self.pod
        );

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
        } else if visible.is_empty() {
            vec![Line::styled(
                format!("(no lines match \"{}\")", self.filter),
                theme.dim(),
            )]
        } else {
            visible.iter().map(|l| Line::raw((*l).clone())).collect()
        };

        // The filter sits on the top border so the body's scroll math is
        // unaffected: editable while `/` is active, else a count summary.
        let filter_title: Option<Line> = if self.filtering {
            Some(Line::from(vec![
                Span::styled(" filter: ", theme.title()),
                Span::raw(self.filter.clone()),
                Span::styled("▏ ", theme.title()), // cursor
            ]))
        } else if !self.filter.is_empty() {
            Some(Line::from(vec![
                Span::styled(" filter: ", theme.title()),
                Span::raw(self.filter.clone()),
                Span::styled(
                    format!(" ({}/{}) ", visible.len(), self.lines.len()),
                    theme.dim(),
                ),
            ]))
        } else {
            None
        };

        // `visible` is no longer borrowed past here — persist the clamped scroll
        // so the next keypress operates on the in-range value.
        self.scroll = scroll;

        let hint = " j/k scroll · / filter · p previous · G/f follow · Esc back ";
        let mut block = Block::bordered()
            .border_style(theme.chrome())
            .title(title)
            .title_style(theme.title())
            .title_bottom(Line::styled(hint, theme.dim()).right_aligned());
        // The filter rides the top border (right) — the bottom is taken by the
        // hint, and the two collide on an 80-col terminal.
        if let Some(ft) = filter_title {
            block = block.title_top(ft.right_aligned());
        }
        f.render_widget(Paragraph::new(body).block(block).scroll((scroll, 0)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::ui::theme::Theme;
    use crate::ui::{Action, OverlayMode};
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::Models;
    use kubernation_core::state::planned::PlannedWorld;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_crossterm::crossterm::event::KeyModifiers;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn render_to_string(view: &mut LogsView) -> String {
        render_sized(view, 80, 12)
    }

    fn render_sized(view: &mut LogsView, w: u16, h: u16) -> String {
        let (world, _s) = fx::world();
        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let planned = PlannedWorld::default();
        let ctx = RenderCtx {
            models: &models,
            world: &world,
            theme: &theme,
            overlay: OverlayMode::Pressure,
            ready: true,
            cluster: ClusterId::Hot,
            focused: true,
            pair: None,
            cluster_label: None,
            attention: &[],
            planned: &planned,
        };
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    /// A live-tail poll can shrink the (filtered) set under a non-following
    /// viewport; render must re-clamp scroll so the body doesn't go blank.
    #[test]
    fn scroll_reclamps_when_set_shrinks_on_refetch() {
        let mut view = LogsView::default();
        view.open(ClusterId::Hot, "demo".into(), "web".into());
        let many: String = (0..10).map(|i| format!("log line {i}\n")).collect();
        view.set_result(Ok(many));
        // First render in a short viewport pins scroll to the bottom.
        let _ = render_sized(&mut view, 60, 6);
        // Scroll up one → stop following, scroll lands above the bottom.
        view.handle_key(key('k'), &dummy_ctx());
        // A poll refetch arrives with far fewer lines than the held scroll.
        view.set_result(Ok("only line\n".into()));
        let text = render_sized(&mut view, 60, 6);
        assert!(
            text.contains("only line"),
            "body blanked after the set shrank:\n{text}"
        );
    }

    #[test]
    fn filter_narrows_to_matching_lines_case_insensitively() {
        let mut view = LogsView::default();
        view.open(ClusterId::Hot, "demo".into(), "web-1".into());
        view.set_result(Ok("line one\nERROR boom\nline three\nerror again".into()));

        // `/` enters the editor; the app would route keys to filter_input.
        assert!(view.handle_key(key('/'), &dummy_ctx()).is_none());
        assert!(view.filtering());
        for c in "error".chars() {
            view.filter_input(key(c));
        }
        view.filter_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!view.filtering());

        let text = render_to_string(&mut view);
        assert!(text.contains("ERROR boom"), "match 1 missing:\n{text}");
        assert!(text.contains("error again"), "match 2 missing:\n{text}");
        assert!(!text.contains("line one"), "non-match shown:\n{text}");
        assert!(text.contains("filter: error"), "filter chrome:\n{text}");
        assert!(text.contains("(2/4)"), "match count:\n{text}");
    }

    #[test]
    fn previous_toggle_flips_flag_and_asks_for_refetch() {
        let mut view = LogsView::default();
        view.open(ClusterId::Hot, "demo".into(), "crashy-1".into());
        assert!(!view.previous);
        let action = view.handle_key(key('p'), &dummy_ctx());
        assert_eq!(action, Some(Action::RefetchLogs));
        assert!(view.previous);
        // The indicator renders.
        view.set_result(Ok("prev crash output".into()));
        let text = render_to_string(&mut view);
        assert!(text.contains("previous"), "previous indicator:\n{text}");
    }

    // handle_key ignores the ctx, so a throwaway is fine.
    fn dummy_ctx() -> RenderCtx<'static> {
        use std::sync::OnceLock;
        static THEME: OnceLock<Theme> = OnceLock::new();
        static MODELS: OnceLock<Models> = OnceLock::new();
        static WORLD: OnceLock<kubernation_core::state::observed::ObservedWorld> = OnceLock::new();
        static PLANNED: OnceLock<PlannedWorld> = OnceLock::new();
        let world = WORLD.get_or_init(|| fx::world().0);
        RenderCtx {
            models: MODELS.get_or_init(|| Models::build(world)),
            world,
            theme: THEME.get_or_init(|| Theme::new(ColorMode::Auto)),
            overlay: OverlayMode::Pressure,
            ready: true,
            cluster: ClusterId::Hot,
            focused: true,
            pair: None,
            cluster_label: None,
            attention: &[],
            planned: PLANNED.get_or_init(PlannedWorld::default),
        }
    }
}
