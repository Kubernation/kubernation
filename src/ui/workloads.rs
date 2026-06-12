use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::Span;
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::{Action, Component, RenderCtx};
use crate::state::model::RolloutStatus;
use crate::util::format_age_opt;

/// Flat list of Deployments / StatefulSets / DaemonSets; Enter opens the
/// city screen for the selected workload.
#[derive(Default)]
pub struct WorkloadListView {
    state: TableState,
}

impl Component for WorkloadListView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<Action> {
        let len = ctx.models.workloads.len();
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
                if let Some(row) = self
                    .state
                    .selected()
                    .and_then(|i| ctx.models.workloads.get(i))
                {
                    return Some(Action::OpenWorkload(row.r.clone()));
                }
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, ctx: &RenderCtx) {
        let len = ctx.models.workloads.len();
        match self.state.selected() {
            _ if len == 0 => self.state.select(None),
            None => self.state.select(Some(0)),
            Some(i) if i >= len => self.state.select(Some(len - 1)),
            _ => {}
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        let block = Block::bordered()
            .title(" WORKLOADS ")
            .title_style(theme.title());
        let rows = &ctx.models.workloads;
        if rows.is_empty() {
            let msg = if ctx.ready {
                "no workloads observed"
            } else {
                "syncing…"
            };
            f.render_widget(
                Paragraph::new(Span::styled(msg, theme.dim())).block(block),
                area,
            );
            return;
        }

        let table_rows: Vec<Row> = rows
            .iter()
            .map(|w| {
                let sev = ctx.models.workload_severity.get(&w.r);
                let sev_span = match sev {
                    Some(s) => Span::styled(s.glyph(), theme.severity(*s)),
                    None => Span::raw(" "),
                };
                let ready_style = if w.ready < w.desired {
                    theme.severity(crate::state::attention::Severity::Warning)
                } else {
                    Default::default()
                };
                let status_style = match w.status {
                    RolloutStatus::Complete => theme.dim(),
                    RolloutStatus::Stalled => {
                        theme.severity(crate::state::attention::Severity::Critical)
                    }
                    _ => Default::default(),
                };
                let mut status = w.status.to_string();
                if !w.note.is_empty() {
                    status = format!("{status} — {}", w.note);
                }
                Row::new(vec![
                    sev_span,
                    Span::styled(w.r.kind.to_string(), theme.dim()),
                    Span::raw(w.r.namespace.clone()),
                    Span::raw(w.r.name.clone()),
                    Span::styled(format!("{}/{}", w.ready, w.desired), ready_style),
                    Span::raw(w.updated.to_string()),
                    Span::raw(w.available.to_string()),
                    Span::raw(format_age_opt(w.age.as_ref())),
                    Span::styled(status, status_style),
                ])
            })
            .collect();

        let header = Row::new(vec![
            " ",
            "KIND",
            "NAMESPACE",
            "NAME",
            "READY",
            "UPD",
            "AVL",
            "AGE",
            "STATUS",
        ])
        .style(theme.dim());

        let table = Table::new(
            table_rows,
            [
                Constraint::Length(1),
                Constraint::Length(6),
                Constraint::Min(12),
                Constraint::Min(16),
                Constraint::Length(6),
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(5),
                Constraint::Min(18),
            ],
        )
        .header(header)
        .block(block)
        .row_highlight_style(theme.selection())
        .highlight_symbol("▸");
        f.render_stateful_widget(table, area, &mut self.state);
    }
}
