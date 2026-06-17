//! Modal namespace-filter picker. Multi-select: Space toggles a namespace in
//! the working set, Enter applies it, the top "all namespaces" row clears the
//! filter. Mirrors the context picker's shape.

use std::collections::BTreeSet;

use ratatui::Frame;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::theme::Theme;
use super::{Action, centered};
use kubernation_core::state::filter::NamespaceFilter;

#[derive(Default)]
pub struct NamespacePicker {
    pub open: bool,
    /// Row 0 is the synthetic "all namespaces"; the rest are real namespaces.
    namespaces: Vec<String>,
    /// The working selection edited in-place before Enter applies it.
    sel: NamespaceFilter,
    state: ListState,
}

impl NamespacePicker {
    pub fn open_with(&mut self, namespaces: BTreeSet<String>, current: &NamespaceFilter) {
        self.namespaces = namespaces.into_iter().collect();
        self.sel = current.clone();
        self.state.select(Some(0));
        self.open = true;
    }

    fn len(&self) -> usize {
        self.namespaces.len() + 1 // +1 for the "all namespaces" row
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let len = self.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('N') | KeyCode::Char('q') => self.open = false,
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(i.saturating_sub(1)));
            }
            // Space toggles a namespace in/out (row 0 clears to All).
            KeyCode::Char(' ') => match self.state.selected() {
                Some(0) => self.sel = NamespaceFilter::All,
                Some(i) => {
                    if let Some(ns) = self.namespaces.get(i - 1) {
                        self.sel.toggle(ns);
                    }
                }
                None => {}
            },
            // Enter applies. On the "all" row, force All regardless of toggles.
            KeyCode::Enter => {
                if self.state.selected() == Some(0) {
                    self.sel = NamespaceFilter::All;
                }
                self.open = false;
                return Some(Action::SetNamespaceFilter(self.sel.clone()));
            }
            _ => {}
        }
        None
    }

    pub fn render(&mut self, f: &mut Frame, theme: &Theme) {
        let h = (self.len() as u16 + 2).clamp(4, 18);
        let area = centered(f.area(), 48, h);
        f.render_widget(Clear, area);

        let mut items: Vec<ListItem> = Vec::with_capacity(self.len());
        let all_on = !self.sel.is_active();
        items.push(ListItem::new(format!(
            "{} all namespaces",
            if all_on { "●" } else { " " }
        )));
        for ns in &self.namespaces {
            let on = self.sel.contains(ns);
            items.push(ListItem::new(format!(
                "{} {ns}",
                if on { "●" } else { " " }
            )));
        }

        let list = List::new(items)
            .block(
                Block::bordered()
                    .border_style(theme.chrome())
                    .title(" NAMESPACE FILTER ")
                    .title_style(theme.title())
                    .title_bottom(
                        ratatui::text::Line::styled(
                            " Space toggle · Enter apply · Esc cancel ",
                            theme.dim(),
                        )
                        .right_aligned(),
                    ),
            )
            .highlight_style(theme.selection())
            .highlight_symbol("▸ ");
        f.render_stateful_widget(list, area, &mut self.state);
    }
}
