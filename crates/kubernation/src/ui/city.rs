//! The city screen: one workload, full context, no mode switching. The 4X
//! analog is exact — population (replicas), production (rollout), buildings
//! (owned resources), recent history (events).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::symbols::{bar, pod_glyph};
use super::{Action, Component, RenderCtx};
use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::{
    CityModel, RolloutStatus, WorkloadKind, WorkloadRef, build_city,
};
use kubernation_core::state::planned::Intervention;
use kubernation_core::util::{format_age_opt, truncate};

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
    fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<Action> {
        let len = self.model.as_ref().map(|m| m.pods.len()).unwrap_or(0);
        match key.code {
            // --- Planning turn: stage scale / restart for this workload ----
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if let Some(m) = self.model.as_ref()
                    && m.r.kind != WorkloadKind::DaemonSet
                {
                    let cur = ctx.planned.scaled(&m.r).unwrap_or(m.desired);
                    return Some(Action::Stage(Intervention::Scale {
                        workload: m.r.clone(),
                        replicas: cur + 1,
                    }));
                }
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                if let Some(m) = self.model.as_ref()
                    && m.r.kind != WorkloadKind::DaemonSet
                {
                    let cur = ctx.planned.scaled(&m.r).unwrap_or(m.desired);
                    return Some(Action::Stage(Intervention::Scale {
                        workload: m.r.clone(),
                        replicas: (cur - 1).max(0),
                    }));
                }
            }
            KeyCode::Char('R') => {
                if let Some(m) = self.model.as_ref() {
                    return Some(Action::ToggleRestart(m.r.clone()));
                }
            }
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
            KeyCode::Char('l') => {
                if let Some(m) = self.model.as_ref()
                    && let Some(pod) = self.pods.selected().and_then(|i| m.pods.get(i))
                {
                    return Some(Action::OpenLogs {
                        namespace: m.r.namespace.clone(),
                        pod: pod.name.clone(),
                    });
                }
            }
            KeyCode::Char('e') => {
                if let Some(m) = self.model.as_ref()
                    && let Some(pod) = self.pods.selected().and_then(|i| m.pods.get(i))
                {
                    return Some(Action::EvictPod {
                        namespace: m.r.namespace.clone(),
                        pod: pod.name.clone(),
                    });
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
                .block(Block::bordered().border_style(theme.chrome())),
                area,
            );
            return;
        };

        // Header carries an extra line for the plan/hint and one for pair.
        let head_h = if ctx.pair.is_some() { 6 } else { 5 };
        let [head_a, mid_a, ev_a] = Layout::vertical([
            Constraint::Length(head_h),
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
        let title = match ctx.cluster_label {
            Some(l) => format!(" {} — {l} ", m.r),
            None => format!(" {} ", m.r),
        };
        let head_block = Block::bordered()
            .border_style(theme.chrome())
            .title(title)
            .title_style(theme.title())
            .title_top(Line::styled(rollout, status_style).right_aligned());
        let gap_style = if m.ready < m.desired {
            theme.severity(Severity::Warning)
        } else {
            Default::default()
        };
        // Replica storage bar — the city's granary.
        let fill = if m.desired > 0 {
            (m.ready as f64 / m.desired as f64).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let bar_style = if m.ready < m.desired {
            theme.severity(Severity::Warning)
        } else {
            theme.ratio(0.0)
        };
        let mut lines = vec![
            Line::from(vec![
                Span::raw("replicas  "),
                Span::styled(bar(fill, 10), bar_style),
                Span::raw(" "),
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
        if let Some(pair) = ctx.pair {
            lines.push(match pair.state(&m.r) {
                Some(st) => Line::from(vec![
                    Span::raw("pair      "),
                    Span::styled(st.describe(ctx.cluster), theme.sync(st)),
                ]),
                None => Line::styled("pair      unknown", theme.dim()),
            });
        }
        // Planning turn: the staged delta, or a dim hint if nothing's staged.
        let staged_scale = ctx.planned.scaled(&m.r);
        let restarting = ctx.planned.restarting(&m.r);
        if staged_scale.is_some() || restarting {
            let warn = theme.severity(Severity::Warning);
            let mut spans = vec![Span::styled("plan      ", theme.title())];
            if let Some(rep) = staged_scale {
                spans.push(Span::styled(format!("scale {} → {rep}", m.desired), warn));
            }
            if restarting {
                if staged_scale.is_some() {
                    spans.push(Span::raw(" · "));
                }
                spans.push(Span::styled("rolling restart", warn));
            }
            spans.push(Span::styled("  (t: end of turn)", theme.dim()));
            lines.push(Line::from(spans));
        } else {
            let hint = if m.r.kind == WorkloadKind::DaemonSet {
                "plan      R restart · t end of turn"
            } else {
                "plan      +/− scale · R restart · t end of turn"
            };
            lines.push(Line::styled(hint, theme.dim()));
        }
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
                .border_style(theme.chrome())
                .title(format!(" PODS ({}) — l logs · e evict ", m.pods.len()))
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
                    .border_style(theme.chrome())
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
                    .border_style(theme.chrome())
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
    use crate::ui::OverlayMode;
    use crate::ui::theme::Theme;
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::model::{Models, WorkloadKind};
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
        s.ingress(fx::ingress("demo", "web-ing", "web.example.com", "web"));

        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let planned = kubernation_core::state::planned::PlannedWorld::default();
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
            text.contains("ing/web-ing"),
            "owned ingress (gate) missing:\n{text}"
        );
        assert!(
            text.contains("RECENT EVENTS"),
            "events panel missing:\n{text}"
        );
    }
}
