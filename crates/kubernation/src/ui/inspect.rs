//! The object inspector: a read-only, scrollable YAML "dossier" of one
//! resource (workload / node / pod). The app resolves the YAML from the
//! observed store (least-privilege: only watched kinds) and hands it here; this
//! component just holds and scrolls the text.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Action, Component, RenderCtx};

#[derive(Default)]
pub struct InspectView {
    title: String,
    lines: Vec<String>,
    scroll: u16,
    last_h: u16,
}

impl InspectView {
    pub fn open(&mut self, title: String, yaml: String) {
        self.title = title;
        self.lines = yaml.lines().map(|l| l.to_string()).collect();
        self.scroll = 0;
    }

    fn max_scroll(&self) -> u16 {
        let view = self.last_h.saturating_sub(2); // borders
        (self.lines.len() as u16).saturating_sub(view)
    }
}

impl Component for InspectView {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &RenderCtx) -> Option<Action> {
        let page = self.last_h.saturating_sub(3).max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = (self.scroll + 1).min(self.max_scroll());
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = (self.scroll + page).min(self.max_scroll()),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::Char('d') if ctrl => self.scroll = (self.scroll + page).min(self.max_scroll()),
            KeyCode::Char('u') if ctrl => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll = self.max_scroll(),
            _ => {}
        }
        None
    }

    fn update(&mut self, _ctx: &RenderCtx) {}

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        self.last_h = area.height;
        let scroll = self.scroll.min(self.max_scroll());
        self.scroll = scroll;

        let body: Vec<Line> = if self.lines.is_empty() {
            vec![Line::styled("(nothing to inspect)", theme.dim())]
        } else {
            self.lines.iter().map(|l| Line::raw(l.clone())).collect()
        };
        let hint = " j/k scroll · g/G top/bottom · Esc back ";
        let block = Block::bordered()
            .border_style(theme.chrome())
            .title(format!(" {} ", self.title))
            .title_style(theme.title())
            .title_bottom(Line::styled(hint, theme.dim()).right_aligned());
        f.render_widget(Paragraph::new(body).block(block).scroll((scroll, 0)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::events::ClusterId;
    use crate::ui::OverlayMode;
    use crate::ui::theme::Theme;
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::Models;
    use kubernation_core::state::planned::PlannedWorld;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_title_and_yaml() {
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
        let mut view = InspectView::default();
        view.open(
            "pod demo/web".into(),
            "apiVersion: v1\nkind: Pod\nmetadata:\n  name: web\n".into(),
        );
        let mut term = Terminal::new(TestBackend::new(60, 10)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("pod demo/web"), "title shown:\n{text}");
        assert!(text.contains("kind: Pod"), "yaml shown:\n{text}");
    }
}
