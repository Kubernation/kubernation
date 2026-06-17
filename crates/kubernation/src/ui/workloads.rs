use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::Span;
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::{Action, Component, RenderCtx};
use kubernation_core::state::model::RolloutStatus;
use kubernation_core::util::format_age_opt;

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
        let title = match ctx.cluster_label {
            Some(l) => format!(" WORKLOADS — {l} "),
            None => " WORKLOADS ".to_string(),
        };
        let block = Block::bordered()
            .border_style(theme.chrome())
            .title(title)
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
                    theme.severity(kubernation_core::state::attention::Severity::Warning)
                } else {
                    Default::default()
                };
                let status_style = match w.status {
                    RolloutStatus::Complete => theme.dim(),
                    RolloutStatus::Stalled => {
                        theme.severity(kubernation_core::state::attention::Severity::Critical)
                    }
                    _ => Default::default(),
                };
                let mut status = w.status.to_string();
                if !w.note.is_empty() {
                    status = format!("{status} — {}", w.note);
                }
                let mut cells = vec![
                    sev_span,
                    Span::styled(w.r.kind.to_string(), theme.dim()),
                    Span::raw(w.r.namespace.clone()),
                    Span::raw(w.r.name.clone()),
                    Span::styled(format!("{}/{}", w.ready, w.desired), ready_style),
                    Span::raw(w.updated.to_string()),
                    Span::raw(w.available.to_string()),
                    Span::raw(format_age_opt(w.age.as_ref())),
                ];
                if let Some(pair) = ctx.pair {
                    cells.push(match pair.state(&w.r) {
                        Some(st) => Span::styled(st.badge(), theme.sync(st)),
                        None => Span::raw(""),
                    });
                }
                cells.push(Span::styled(status, status_style));
                Row::new(cells)
            })
            .collect();

        let mut header = vec![
            " ",
            "KIND",
            "NAMESPACE",
            "NAME",
            "READY",
            "UPD",
            "AVL",
            "AGE",
        ];
        let mut widths = vec![
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Min(12),
            Constraint::Min(16),
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(5),
        ];
        if ctx.pair.is_some() {
            header.push("SYNC");
            widths.push(Constraint::Length(4));
        }
        header.push("STATUS");
        widths.push(Constraint::Min(18));

        let table = Table::new(table_rows, widths)
            .header(Row::new(header).style(theme.dim()))
            .block(block)
            .row_highlight_style(theme.selection())
            .highlight_symbol("▸");
        f.render_stateful_widget(table, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::events::ClusterId;
    use crate::ui::theme::Theme;
    use crate::ui::{Component, OverlayMode, RenderCtx};
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::Models;
    use kubernation_core::state::pair::PairSync;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// With a pair attached, the list grows a SYNC column with badges.
    #[test]
    fn sync_column_shows_badges() {
        let (hot, mut hs) = fx::world();
        let (warm, mut ws) = fx::world();
        hs.deployment(fx::deployment("demo", "web", 3, 3));
        ws.deployment(fx::deployment("demo", "web", 1, 1)); // replica drift
        hs.deployment(fx::deployment("demo", "crashy", 2, 0)); // missing on warm
        hs.deployment(fx::deployment("demo", "same", 1, 1));
        ws.deployment(fx::deployment("demo", "same", 1, 1));

        let models = Models::build(&hot);
        let pair = PairSync::build(
            &hot,
            &warm,
            &kubernation_core::state::filter::NamespaceFilter::All,
        );
        let theme = Theme::new(ColorMode::Auto);
        let planned = kubernation_core::state::planned::PlannedWorld::default();
        let ctx = RenderCtx {
            models: &models,
            world: &hot,
            theme: &theme,
            overlay: OverlayMode::Pressure,
            ready: true,
            cluster: ClusterId::Hot,
            focused: true,
            pair: Some(&pair),
            cluster_label: Some("HOT"),
            attention: &[],
            planned: &planned,
        };
        let mut view = WorkloadListView::default();
        let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }

        assert!(text.contains("WORKLOADS — HOT"), "title missing:\n{text}");
        assert!(text.contains("SYNC"), "sync header missing:\n{text}");
        assert!(text.contains("≠r"), "replica-drift badge missing:\n{text}");
        assert!(
            text.contains("−w"),
            "missing-on-warm badge missing:\n{text}"
        );
        assert!(text.contains('='), "in-sync badge missing:\n{text}");
    }
}
