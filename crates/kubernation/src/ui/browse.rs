//! The resource browser: a `:`-command kind picker (any built-in kind or CRD,
//! discovered from the cluster) and a generic table of the chosen kind, drilled
//! into the YAML inspector. Read-only; the data layer is
//! `kubernation_core::k8s::browse` (fetch-not-watch).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::theme::Theme;
use super::{Action, Component, RenderCtx, centered};
use kubernation_core::k8s::browse::{BrowseRow, KindEntry, Object, row};

// --- the `:` kind picker (a filterable modal) ------------------------------

#[derive(Default)]
pub struct ResourcePicker {
    pub open: bool,
    kinds: Vec<KindEntry>,
    discovering: bool,
    filter: String,
    state: ListState,
}

impl ResourcePicker {
    pub fn open_with(&mut self, kinds: Vec<KindEntry>, discovering: bool) {
        self.kinds = kinds;
        self.discovering = discovering;
        self.filter.clear();
        self.open = true;
        self.reset_selection();
    }

    /// Discovery landed while the picker is open — fill it in.
    pub fn set_kinds(&mut self, kinds: Vec<KindEntry>) {
        self.kinds = kinds;
        self.discovering = false;
        self.reset_selection();
    }

    fn reset_selection(&mut self) {
        self.state
            .select((!self.filtered().is_empty()).then_some(0));
    }

    fn filtered(&self) -> Vec<&KindEntry> {
        let f = self.filter.to_lowercase();
        self.kinds
            .iter()
            .filter(|k| f.is_empty() || k.label().to_lowercase().contains(&f))
            .collect()
    }

    pub fn selected_kind(&self) -> Option<KindEntry> {
        let f = self.filtered();
        self.state
            .selected()
            .and_then(|i| f.get(i))
            .map(|k| (*k).clone())
    }

    /// Keys (handled before the global bindings so chars type into the filter).
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let len = self.filtered().len();
        match key.code {
            KeyCode::Esc => self.open = false,
            KeyCode::Down if len > 0 => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some((i + 1) % len));
            }
            KeyCode::Up if len > 0 => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some((i + len - 1) % len));
            }
            KeyCode::Enter if self.selected_kind().is_some() => {
                self.open = false;
                return Some(Action::ListSelectedKind);
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.reset_selection();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.reset_selection();
            }
            _ => {}
        }
        None
    }

    pub fn render(&mut self, f: &mut Frame, theme: &Theme) {
        let area = centered(f.area(), 56, 20);
        f.render_widget(Clear, area);
        let items: Vec<ListItem> = self
            .filtered()
            .iter()
            .map(|k| {
                let scope = if k.namespaced { "" } else { "  ·  cluster" };
                ListItem::new(format!("{}{scope}", k.label()))
            })
            .collect();
        let title = if self.discovering {
            " :resource — discovering kinds… ".to_string()
        } else {
            format!(" :resource — {} kinds ", self.kinds.len())
        };
        let mut block = Block::bordered()
            .border_style(theme.chrome())
            .title(title)
            .title_style(theme.title())
            .title_bottom(
                Line::styled(
                    " type to filter · ↑/↓ · Enter open · Esc cancel ",
                    theme.dim(),
                )
                .right_aligned(),
            );
        if !self.filter.is_empty() {
            block = block.title_top(Line::from(format!(" / {} ", self.filter)).right_aligned());
        }
        let list = List::new(items)
            .block(block)
            .highlight_style(theme.selection())
            .highlight_symbol("▸ ");
        f.render_stateful_widget(list, area, &mut self.state);
    }
}

// --- the browse table ------------------------------------------------------

#[derive(Default)]
pub struct BrowseView {
    label: String,
    namespaced: bool,
    objects: Vec<Object>,
    rows: Vec<BrowseRow>,
    state: ListState,
    loading: bool,
    error: Option<String>,
}

impl BrowseView {
    /// Begin listing a kind (loading state until `set_result`).
    pub fn open(&mut self, kind: &KindEntry) {
        self.label = kind.label();
        self.namespaced = kind.namespaced;
        self.objects.clear();
        self.rows.clear();
        self.state.select(None);
        self.loading = true;
        self.error = None;
    }

    pub fn set_loading(&mut self) {
        self.loading = true;
    }

    pub fn set_result(&mut self, result: Result<Vec<Object>, String>) {
        self.loading = false;
        match result {
            Ok(objs) => {
                self.rows = objs.iter().map(row).collect();
                self.objects = objs;
                self.error = None;
                self.state.select((!self.rows.is_empty()).then_some(0));
            }
            Err(e) => {
                self.error = Some(e);
                self.objects.clear();
                self.rows.clear();
                self.state.select(None);
            }
        }
    }

    pub fn selected_object(&self) -> Option<Object> {
        self.state
            .selected()
            .and_then(|i| self.objects.get(i))
            .cloned()
    }
}

impl Component for BrowseView {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &RenderCtx) -> Option<Action> {
        let len = self.rows.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Char('g') if len > 0 => self.state.select(Some(0)),
            KeyCode::Char('G') if len > 0 => self.state.select(Some(len - 1)),
            KeyCode::Enter if self.selected_object().is_some() => {
                return Some(Action::InspectSelected);
            }
            KeyCode::Char('r') => return Some(Action::RefreshBrowse),
            _ => {}
        }
        None
    }

    fn update(&mut self, _ctx: &RenderCtx) {}

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        let items: Vec<ListItem> = if self.loading {
            vec![ListItem::new(Line::styled("listing…", theme.dim()))]
        } else if let Some(err) = &self.error {
            vec![ListItem::new(Line::styled(
                format!("could not list: {err}"),
                theme.severity(kubernation_core::state::attention::Severity::Critical),
            ))]
        } else if self.rows.is_empty() {
            vec![ListItem::new(Line::styled("(no objects)", theme.dim()))]
        } else {
            self.rows
                .iter()
                .map(|r| {
                    let name = if self.namespaced && !r.namespace.is_empty() {
                        format!("{}/{}", r.namespace, r.name)
                    } else {
                        r.name.clone()
                    };
                    ListItem::new(format!("{name:<54}{}", r.age))
                })
                .collect()
        };
        let title = format!(" browse: {} ({}) ", self.label, self.rows.len());
        let block = Block::bordered()
            .border_style(theme.chrome())
            .title(title)
            .title_style(theme.title())
            .title_bottom(
                Line::styled(
                    " j/k · Enter yaml · r refresh · : kind · Esc back ",
                    theme.dim(),
                )
                .right_aligned(),
            );
        let list = List::new(items)
            .block(block)
            .highlight_style(theme.selection())
            .highlight_symbol("▸ ");
        f.render_stateful_widget(list, area, &mut self.state);
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

    fn dump(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn browse_view_renders_loading_error_empty() {
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
        let mut v = BrowseView::default();
        v.set_loading();
        let mut term = Terminal::new(TestBackend::new(70, 8)).unwrap();
        term.draw(|f| v.render(f, f.area(), &ctx)).unwrap();
        assert!(dump(&term).contains("listing"), "loading state");

        v.set_result(Err("boom".into()));
        term.draw(|f| v.render(f, f.area(), &ctx)).unwrap();
        assert!(dump(&term).contains("could not list: boom"), "error state");

        v.set_result(Ok(Vec::new()));
        term.draw(|f| v.render(f, f.area(), &ctx)).unwrap();
        assert!(dump(&term).contains("no objects"), "empty state");
    }

    #[test]
    fn picker_shows_discovering_when_empty() {
        let theme = Theme::new(ColorMode::Auto);
        let mut p = ResourcePicker::default();
        p.open_with(Vec::new(), true);
        let mut term = Terminal::new(TestBackend::new(60, 8)).unwrap();
        term.draw(|f| p.render(f, &theme)).unwrap();
        assert!(dump(&term).contains("discovering"), "discovering title");
    }
}
