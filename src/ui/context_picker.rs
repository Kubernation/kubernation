use ratatui::Frame;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::theme::Theme;
use super::{Action, centered};

/// Modal context switcher fed from the kubeconfig's context list.
#[derive(Default)]
pub struct ContextPicker {
    pub open: bool,
    current: String,
    items: Vec<String>,
    state: ListState,
}

impl ContextPicker {
    pub fn open_with(&mut self, all: &[String], current: &str) {
        self.items = all.to_vec();
        self.current = current.to_string();
        let idx = self.items.iter().position(|c| c == current).unwrap_or(0);
        self.state.select(Some(idx));
        self.open = true;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let len = self.items.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('q') => self.open = false,
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Enter => {
                if let Some(name) = self.state.selected().and_then(|i| self.items.get(i)) {
                    let name = name.clone();
                    self.open = false;
                    if name != self.current {
                        return Some(Action::SwitchContext(name));
                    }
                }
            }
            _ => {}
        }
        None
    }

    pub fn render(&mut self, f: &mut Frame, theme: &Theme) {
        let h = (self.items.len() as u16 + 2).clamp(3, 16);
        let area = centered(f.area(), 44, h);
        f.render_widget(Clear, area);
        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|c| {
                let marker = if *c == self.current { "● " } else { "  " };
                ListItem::new(format!("{marker}{c}"))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(" SWITCH CONTEXT ")
                    .title_style(theme.title()),
            )
            .highlight_style(theme.selection())
            .highlight_symbol("▸ ");
        f.render_stateful_widget(list, area, &mut self.state);
    }
}
