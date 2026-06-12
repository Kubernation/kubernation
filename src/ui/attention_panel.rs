use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::{Action, Component, RenderCtx};
use crate::events::ClusterId;
use crate::state::attention::{Concern, Severity, Target, severity_counts};
use crate::util::truncate;

/// The persistent concern queue. Collapsed it is a one-line summary; `a`
/// expands it; `n` cycles to the next concern and jumps to its view — the
/// "next unit needing orders" key. In pair mode the queue is merged across
/// both worlds and entries carry an H/W tag.
pub struct AttentionPanel {
    pub expanded: bool,
    pub focused: bool,
    /// Position of the n-cycle / focused selection within the merged list.
    pub cycle: Option<usize>,
    state: TableState,
}

impl AttentionPanel {
    pub fn new(expanded: bool) -> Self {
        Self {
            expanded,
            focused: false,
            cycle: None,
            state: TableState::default(),
        }
    }

    pub fn height(&self, concerns: usize) -> u16 {
        if self.expanded {
            (concerns.clamp(1, 6) as u16) + 2
        } else {
            1
        }
    }

    /// Advance the cycle and return the owning cluster plus the action that
    /// opens that concern.
    pub fn next_action(&mut self, attention: &[Concern]) -> Option<(ClusterId, Action)> {
        if attention.is_empty() {
            self.cycle = None;
            self.state.select(None);
            return None;
        }
        let next = self.cycle.map(|i| (i + 1) % attention.len()).unwrap_or(0);
        self.cycle = Some(next);
        self.state.select(Some(next));
        let c = &attention[next];
        Some((c.cluster, action_for(&c.target)))
    }

    /// The concern under the cycle/selection, for cluster routing.
    pub fn current<'a>(&self, attention: &'a [Concern]) -> Option<&'a Concern> {
        self.cycle.and_then(|i| attention.get(i))
    }
}

pub fn action_for(t: &Target) -> Action {
    match t {
        Target::Node(name) => Action::OpenNode(name.clone()),
        Target::Workload(r) => Action::OpenWorkload(r.clone()),
        Target::WorkloadList => Action::OpenWorkloadList,
    }
}

fn tag(c: &Concern) -> &'static str {
    match c.cluster {
        ClusterId::Hot => "H ",
        ClusterId::Warm => "W ",
    }
}

impl Component for AttentionPanel {
    fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<Action> {
        let len = ctx.attention.len();
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
            KeyCode::Enter => {
                if let Some(i) = self.state.selected()
                    && let Some(c) = ctx.attention.get(i)
                {
                    self.cycle = Some(i);
                    return Some(action_for(&c.target));
                }
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, ctx: &RenderCtx) {
        let len = ctx.attention.len();
        if len == 0 {
            self.cycle = None;
            self.state.select(None);
            self.focused = false;
            return;
        }
        if let Some(i) = self.cycle
            && i >= len
        {
            self.cycle = Some(len - 1);
        }
        match self.state.selected() {
            Some(i) if i >= len => self.state.select(Some(len - 1)),
            None => self.state.select(self.cycle),
            _ => {}
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let concerns = ctx.attention;
        let theme = ctx.theme;
        let paired = ctx.pair.is_some();

        if !self.expanded {
            let line = if concerns.is_empty() {
                Line::styled(" · all quiet — no concerns", theme.dim())
            } else {
                let counts = severity_counts(concerns);
                let mut spans: Vec<Span> = vec![Span::styled(" ATTENTION ", theme.title())];
                for sev in [Severity::Critical, Severity::Warning, Severity::Info] {
                    if let Some(n) = counts.get(&sev) {
                        spans.push(Span::styled(
                            format!("{}{n} ", sev.glyph()),
                            theme.severity(sev),
                        ));
                    }
                }
                let top = &concerns[self.cycle.unwrap_or(0).min(concerns.len() - 1)];
                spans.push(Span::raw("▸ "));
                if paired {
                    spans.push(Span::styled(tag(top), theme.dim()));
                }
                spans.push(Span::styled(
                    truncate(&top.title, area.width.saturating_sub(32) as usize),
                    theme.severity(top.severity),
                ));
                spans.push(Span::styled("  [n]ext [a]ll", theme.dim()));
                Line::from(spans)
            };
            f.render_widget(Paragraph::new(line), area);
            return;
        }

        let hint = if self.focused {
            " j/k · Enter opens · Esc leaves "
        } else {
            " n cycles · Tab focuses · a collapses "
        };
        let block = Block::bordered()
            .border_style(theme.chrome())
            .title(format!(" ATTENTION ({}) ", concerns.len()))
            .title_style(theme.title())
            .title_bottom(Line::styled(hint, theme.dim()).right_aligned());
        if concerns.is_empty() {
            f.render_widget(
                Paragraph::new(Line::styled("all quiet", theme.dim())).block(block),
                area,
            );
            return;
        }
        let rows: Vec<Row> = concerns
            .iter()
            .map(|c| {
                let title = if paired {
                    format!("{}{}", tag(c), c.title)
                } else {
                    c.title.clone()
                };
                Row::new(vec![
                    Span::styled(c.severity.glyph(), theme.severity(c.severity)),
                    Span::styled(title, theme.severity(c.severity)),
                    Span::styled(c.detail.clone(), theme.dim()),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Percentage(58),
                Constraint::Min(16),
            ],
        )
        .block(block)
        .row_highlight_style(theme.selection())
        .highlight_symbol("▸");
        f.render_stateful_widget(table, area, &mut self.state);
    }
}
