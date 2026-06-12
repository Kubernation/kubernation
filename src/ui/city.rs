//! The city screen: one workload, full context, no mode switching. The Civ
//! analog is exact — population (replicas), production (rollout), buildings
//! (owned resources), recent history (events).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::symbols::pod_glyph;
use super::{Action, Component, RenderCtx};
use crate::state::attention::Severity;
use crate::state::model::{CityModel, RolloutStatus, WorkloadRef, build_city};
use crate::util::{format_age_opt, truncate};

#[derive(Default)]
pub struct CityView {
    pub current: Option<WorkloadRef>,
    model: Option<CityModel>,
    pods: TableState,
}

impl CityView {
    pub fn open(&mut self, r: WorkloadRef) {
        self.current = Some(r);
        self.model = None; // rebuilt on next update
        self.pods.select(Some(0));
    }

    pub fn close(&mut self) {
        self.current = None;
        self.model = None;
    }
}

impl Component for CityView {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &RenderCtx) -> Option<Action> {
        let len = self.model.as_ref().map(|m| m.pods.len()).unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                let i = self.pods.selected().unwrap_or(0);
                self.pods.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                let i = self.pods.selected().unwrap_or(0);
                self.pods.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Char('g') if len > 0 => self.pods.select(Some(0)),
            KeyCode::Char('G') if len > 0 => self.pods.select(Some(len - 1)),
            KeyCode::Enter => {
                // Drill from a pod to the node it landed on.
                if let Some(pod) = self
                    .pods
                    .selected()
                    .and_then(|i| self.model.as_ref()?.pods.get(i))
                    && !pod.node.is_empty()
                {
                    return Some(Action::OpenNode(pod.node.clone()));
                }
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, ctx: &RenderCtx) {
        self.model = self.current.as_ref().and_then(|r| build_city(ctx.world, r));
        let len = self.model.as_ref().map(|m| m.pods.len()).unwrap_or(0);
        match self.pods.selected() {
            _ if len == 0 => self.pods.select(None),
            None => self.pods.select(Some(0)),
            Some(i) if i >= len => self.pods.select(Some(len - 1)),
            _ => {}
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        let Some(m) = self.model.as_ref() else {
            let title = self
                .current
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_else(|| "workload".into());
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("{title} is no longer observed"),
                    theme.dim(),
                ))
                .block(Block::bordered()),
                area,
            );
            return;
        };

        let [head_a, mid_a, ev_a] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(6),
            Constraint::Length(8),
        ])
        .areas(area);

        // --- Header -----------------------------------------------------
        let status_style = match m.status {
            RolloutStatus::Complete => theme.dim(),
            RolloutStatus::Stalled => theme.severity(Severity::Critical),
            RolloutStatus::Paused => theme.severity(Severity::Warning),
            RolloutStatus::Progressing => theme.severity(Severity::Info),
        };
        let mut rollout = format!(" rollout: {} ", m.status);
        if !m.note.is_empty() {
            rollout = format!(" rollout: {} ({}) ", m.status, m.note);
        }
        let head_block = Block::bordered()
            .title(format!(" {} ", m.r))
            .title_style(theme.title())
            .title_top(Line::styled(rollout, status_style).right_aligned());
        let gap_style = if m.ready < m.desired {
            theme.severity(Severity::Warning)
        } else {
            Default::default()
        };
        let lines = vec![
            Line::from(vec![
                Span::raw("replicas  "),
                Span::styled(format!("{} desired", m.desired), theme.title()),
                Span::raw(" · "),
                Span::styled(format!("{} ready", m.ready), gap_style),
                Span::raw(format!(
                    " · {} available · {} updated",
                    m.available, m.updated
                )),
            ]),
            Line::from(Span::styled(
                format!(
                    "strategy  {} · age {}",
                    m.strategy,
                    format_age_opt(m.age.as_ref())
                ),
                theme.dim(),
            )),
        ];
        f.render_widget(Paragraph::new(lines).block(head_block), head_a);

        // --- Middle: pods | owned ----------------------------------------
        let [pods_a, owned_a] =
            Layout::horizontal([Constraint::Min(46), Constraint::Length(34)]).areas(mid_a);

        let pod_rows: Vec<Row> = m
            .pods
            .iter()
            .map(|p| {
                let style = theme.pod(p.state);
                Row::new(vec![
                    Span::styled(pod_glyph(p.state).to_string(), style),
                    Span::raw(p.name.clone()),
                    Span::styled(p.reason.clone(), style),
                    Span::raw(p.restarts.to_string()),
                    Span::raw(format_age_opt(p.age.as_ref())),
                    Span::styled(p.node.clone(), theme.dim()),
                ])
            })
            .collect();
        let pods_table = Table::new(
            pod_rows,
            [
                Constraint::Length(1),
                Constraint::Min(20),
                Constraint::Length(18),
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(12),
            ],
        )
        .header(Row::new(vec![" ", "POD", "STATUS", "RST", "AGE", "NODE"]).style(theme.dim()))
        .block(
            Block::bordered()
                .title(format!(" PODS ({}) ", m.pods.len()))
                .title_style(theme.title()),
        )
        .row_highlight_style(theme.selection())
        .highlight_symbol("▸");
        f.render_stateful_widget(pods_table, pods_a, &mut self.pods);

        let owned_lines: Vec<Line> = if m.owned.is_empty() {
            vec![Line::styled("nothing owned", theme.dim())]
        } else {
            m.owned
                .iter()
                .map(|o| {
                    let note_style = if o.kind == "pvc" && o.note != "Bound" {
                        theme.severity(Severity::Warning)
                    } else {
                        theme.dim()
                    };
                    Line::from(vec![
                        Span::styled(format!("{:>6}/", o.kind), theme.dim()),
                        Span::raw(o.name.clone()),
                        Span::styled(
                            if o.note.is_empty() {
                                String::new()
                            } else {
                                format!("  {}", o.note)
                            },
                            note_style,
                        ),
                    ])
                })
                .collect()
        };
        f.render_widget(
            Paragraph::new(owned_lines).block(
                Block::bordered()
                    .title(" OWNED ")
                    .title_style(theme.title()),
            ),
            owned_a,
        );

        // --- Events -------------------------------------------------------
        let ev_lines: Vec<Line> = if m.events.is_empty() {
            vec![Line::styled("no recent events", theme.dim())]
        } else {
            m.events
                .iter()
                .map(|e| {
                    Line::from(vec![
                        Span::styled(
                            format!("{:>4} ", format_age_opt(e.when.as_ref())),
                            theme.dim(),
                        ),
                        Span::styled(
                            format!("{:<18}", truncate(&e.reason, 18)),
                            theme.event(e.warning),
                        ),
                        Span::raw(" "),
                        Span::raw(truncate(&e.message, 120)),
                    ])
                })
                .collect()
        };
        f.render_widget(
            Paragraph::new(ev_lines).block(
                Block::bordered()
                    .title(" RECENT EVENTS ")
                    .title_style(theme.title()),
            ),
            ev_a,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::state::fixtures as fx;
    use crate::state::model::{Models, WorkloadKind};
    use crate::ui::OverlayMode;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Snapshot-style: city screen shows header numbers, rollout status,
    /// pods, and owned resources for a fixture deployment.
    #[test]
    fn city_screen_renders_full_context() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 1));
        s.replicaset(fx::replicaset("demo", "web-7d4b", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-1", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        s.pod(fx::pod_owned(
            fx::pod_waiting(
                fx::pod("demo", "web-7d4b-2", Some("n1")),
                "CrashLoopBackOff",
            ),
            "ReplicaSet",
            "web-7d4b",
        ));
        s.service(fx::service("demo", "web", &[("app", "web")]));

        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let ctx = RenderCtx {
            models: &models,
            world: &world,
            theme: &theme,
            overlay: OverlayMode::Pressure,
            ready: true,
        };
        let mut view = CityView::default();
        view.open(WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        });
        view.update(&ctx);

        let mut term = Terminal::new(TestBackend::new(110, 22)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }

        assert!(text.contains("deploy demo/web"), "title missing:\n{text}");
        assert!(
            text.contains("2 desired"),
            "replica counts missing:\n{text}"
        );
        assert!(text.contains("1 ready"), "ready count missing:\n{text}");
        assert!(text.contains("PODS (2)"), "pod table missing:\n{text}");
        assert!(text.contains("web-7d4b-2"), "pod row missing:\n{text}");
        assert!(
            text.contains("CrashLoopBackOff"),
            "pod reason missing:\n{text}"
        );
        assert!(text.contains("svc/web"), "owned service missing:\n{text}");
        assert!(
            text.contains("RECENT EVENTS"),
            "events panel missing:\n{text}"
        );
    }
}
