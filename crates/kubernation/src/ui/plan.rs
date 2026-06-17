//! The End-of-Turn review (TUI): the planning turn's staged-diff screen, and
//! the one place its interventions are *committed*.
//!
//! It lists what would change (`plan_diff`), lets the operator unstage a row
//! or discard the turn, and **commit** — which the app runs through
//! `actions::commit_interventions` (server-side dry-run first, which also
//! enforces RBAC, then a real apply) behind a y/n confirm. Preview until then.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::RenderCtx;
use kubernation_core::k8s::actions::CommitOutcome;
use kubernation_core::state::attention::Severity;
use kubernation_core::state::planned::plan_diff;
use kubernation_core::util::truncate;

/// What the review asks the app to do (the app owns the planned world + client).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanCmd {
    Unstage(usize),
    Discard,
    Commit,
}

#[derive(Default)]
pub struct PlanView {
    rows: TableState,
    /// The latest commit result, shown until the turn is re-opened/changed.
    pub outcome: Option<CommitOutcome>,
}

impl PlanView {
    /// Reset selection + clear a stale outcome when the review is (re)opened.
    pub fn open(&mut self) {
        self.rows.select(Some(0));
        self.outcome = None;
    }

    pub fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<PlanCmd> {
        let len = plan_diff(ctx.world, ctx.planned).len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                let i = self.rows.selected().unwrap_or(0);
                self.rows.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                let i = self.rows.selected().unwrap_or(0);
                self.rows.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Char('x') if len > 0 => {
                return self.rows.selected().map(PlanCmd::Unstage);
            }
            KeyCode::Char('D') => return Some(PlanCmd::Discard),
            KeyCode::Char('c') | KeyCode::Enter => return Some(PlanCmd::Commit),
            _ => {}
        }
        None
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        let changes = plan_diff(ctx.world, ctx.planned);
        let appliable = changes.iter().filter(|c| !c.noop).count();

        // Keep selection in range as rows come and go.
        match self.rows.selected() {
            _ if changes.is_empty() => self.rows.select(None),
            None => self.rows.select(Some(0)),
            Some(i) if i >= changes.len() => self.rows.select(Some(changes.len() - 1)),
            _ => {}
        }

        let outcome_h = self
            .outcome
            .as_ref()
            .map(|o| o.rows.len().min(6) as u16 + 2)
            .unwrap_or(0);
        let [head_a, list_a, out_a, foot_a] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(outcome_h),
            Constraint::Length(1),
        ])
        .areas(area);

        // --- Header: a status line that reflects the last commit, if any ----
        let (head_text, head_style) = match &self.outcome {
            Some(o) if o.applied => {
                let n_ok = o.rows.iter().filter(|r| r.ok).count();
                (
                    format!("committed {n_ok}/{} change(s)", o.rows.len()),
                    if n_ok == o.rows.len() {
                        theme.ratio(0.0)
                    } else {
                        theme.severity(Severity::Warning)
                    },
                )
            }
            Some(o) => (
                format!(
                    "commit blocked — {} change(s) failed dry-run; fix and retry",
                    o.rows.len()
                ),
                theme.severity(Severity::Critical),
            ),
            None if changes.is_empty() => (
                "nothing staged — open a city (+/− scale, R restart) or node (C cordon)".into(),
                theme.dim(),
            ),
            None => (
                format!("{appliable} change(s) to apply — review, then commit"),
                theme.title(),
            ),
        };
        f.render_widget(
            Paragraph::new(Line::styled(head_text, head_style)).block(
                Block::bordered()
                    .border_style(theme.chrome())
                    .title(" End of Turn — staged interventions ")
                    .title_style(theme.title()),
            ),
            head_a,
        );

        // --- The diff -------------------------------------------------------
        let rows: Vec<Row> = changes
            .iter()
            .map(|c| {
                let chg_style = if c.noop {
                    theme.dim()
                } else {
                    theme.severity(Severity::Warning)
                };
                Row::new(vec![
                    Span::raw(c.target.clone()),
                    Span::styled(c.field.to_string(), theme.dim()),
                    Span::styled(format!("{} → {}", c.from, c.to), chg_style),
                    Span::styled(
                        if c.noop { "(no change)" } else { "" }.to_string(),
                        theme.dim(),
                    ),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Min(24),
                Constraint::Length(9),
                Constraint::Min(20),
                Constraint::Length(11),
            ],
        )
        .header(Row::new(vec!["TARGET", "FIELD", "CHANGE", ""]).style(theme.dim()))
        .block(
            Block::bordered()
                .border_style(theme.chrome())
                .title(" j/k move · x unstage · D discard all ")
                .title_style(theme.title()),
        )
        .row_highlight_style(theme.selection())
        .highlight_symbol("▸");
        f.render_stateful_widget(table, list_a, &mut self.rows);

        // --- Per-row commit result -----------------------------------------
        if let Some(o) = &self.outcome {
            let lines: Vec<Line> = o
                .rows
                .iter()
                .take(6)
                .map(|r| {
                    let (mark, st) = if r.ok {
                        ("ok ", theme.ratio(0.0))
                    } else {
                        ("✗  ", theme.severity(Severity::Critical))
                    };
                    let body = if r.detail.is_empty() {
                        r.label.clone()
                    } else {
                        format!("{} — {}", r.label, truncate(&r.detail, 60))
                    };
                    Line::from(vec![Span::styled(mark, st), Span::raw(body)])
                })
                .collect();
            f.render_widget(
                Paragraph::new(lines).block(
                    Block::bordered()
                        .border_style(theme.chrome())
                        .title(" RESULT ")
                        .title_style(theme.title()),
                ),
                out_a,
            );
        }

        // --- Footer: the commit affordance ----------------------------------
        let foot = if appliable > 0 {
            Line::from(vec![
                Span::styled(
                    format!(" c / Enter: commit {appliable} change(s) "),
                    theme.severity(Severity::Warning),
                ),
                Span::styled(
                    "— applies to the cluster (dry-run validated, then confirmed)",
                    theme.dim(),
                ),
            ])
        } else {
            Line::styled(" nothing to commit · Esc back", theme.dim())
        };
        f.render_widget(Paragraph::new(foot), foot_a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::ui::OverlayMode;
    use crate::ui::theme::Theme;
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::{Models, WorkloadKind, WorkloadRef};
    use kubernation_core::state::planned::PlannedWorld;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// The review lists staged changes with their from→to and the commit hint.
    #[test]
    fn plan_review_shows_staged_diff() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 2));

        let mut planned = PlannedWorld::default();
        planned.stage_scale(
            WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: "demo".into(),
                name: "web".into(),
            },
            5,
        );
        planned.stage_cordon("n1".into(), true);

        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let ctx = RenderCtx {
            models: &models,
            world: &world,
            theme: &theme,
            overlay: OverlayMode::Pressure,
            ready: true,
            cluster: crate::events::ClusterId::Hot,
            focused: true,
            pair: None,
            cluster_label: None,
            attention: &[],
            planned: &planned,
        };
        let mut view = PlanView::default();
        view.open();

        let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }

        assert!(text.contains("End of Turn"), "title missing:\n{text}");
        assert!(text.contains("2 change(s) to apply"), "header:\n{text}");
        assert!(text.contains("deploy demo/web"), "scale row:\n{text}");
        assert!(text.contains("2 → 5"), "scale from→to:\n{text}");
        assert!(text.contains("node n1"), "cordon row:\n{text}");
        assert!(text.contains("commit 2 change(s)"), "commit hint:\n{text}");
    }
}
