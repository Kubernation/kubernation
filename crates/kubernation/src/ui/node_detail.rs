//! Node detail: the tile turned over. Terrain attributes (runtime, kubelet,
//! OS — the Runtime layer of the landscape), conditions, allocation, and
//! every pod standing on this ground.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::symbols::{bar, node_glyph, pod_glyph};
use super::{Action, Component, RenderCtx};
use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::{MetricSource, NodeDetailModel, build_node_detail};
use kubernation_core::util::{format_age_opt, human_bytes};

#[derive(Default)]
pub struct NodeDetailView {
    pub current: Option<String>,
    model: Option<NodeDetailModel>,
    pods: TableState,
}

impl NodeDetailView {
    pub fn open(&mut self, name: String) {
        self.current = Some(name);
        self.model = None;
        self.pods.select(Some(0));
    }

    pub fn close(&mut self) {
        self.current = None;
        self.model = None;
    }
}

impl Component for NodeDetailView {
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
                // Drill from a pod to its owning workload's city screen.
                if let Some(owner) = self
                    .pods
                    .selected()
                    .and_then(|i| self.model.as_ref()?.pods.get(i))
                    .and_then(|p| p.owner.clone())
                {
                    return Some(Action::OpenWorkload(owner));
                }
            }
            KeyCode::Char('l') => {
                if let Some(p) = self
                    .pods
                    .selected()
                    .and_then(|i| self.model.as_ref()?.pods.get(i))
                {
                    return Some(Action::OpenLogs {
                        namespace: p.namespace.clone(),
                        pod: p.name.clone(),
                    });
                }
            }
            KeyCode::Char('e') => {
                if let Some(p) = self
                    .pods
                    .selected()
                    .and_then(|i| self.model.as_ref()?.pods.get(i))
                {
                    return Some(Action::EvictPod {
                        namespace: p.namespace.clone(),
                        pod: p.name.clone(),
                    });
                }
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, ctx: &RenderCtx) {
        self.model = self
            .current
            .as_deref()
            .and_then(|n| build_node_detail(ctx.world, n));
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
            let name = self.current.clone().unwrap_or_default();
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("node {name} is no longer observed"),
                    theme.dim(),
                ))
                .block(Block::bordered().border_style(theme.chrome())),
                area,
            );
            return;
        };
        let t = &m.tile;

        let [head_a, pods_a] =
            Layout::vertical([Constraint::Length(6), Constraint::Min(5)]).areas(area);

        // --- Header -------------------------------------------------------
        let mut state_spans = vec![
            Span::styled(format!("{} ", node_glyph(t.health)), theme.node(t.health)),
            Span::styled(
                if t.ready { "Ready" } else { "NotReady" },
                if t.ready {
                    Default::default()
                } else {
                    theme.severity(Severity::Critical)
                },
            ),
        ];
        if t.cordoned {
            state_spans.push(Span::styled(
                " · cordoned",
                theme.severity(Severity::Warning),
            ));
        }
        if !t.abnormal.is_empty() {
            state_spans.push(Span::styled(
                format!(" · pressure: {}", t.abnormal.join(", ")),
                theme.severity(Severity::Warning),
            ));
        }

        let cpu_used = t.cpu_ratio * m.cpu_alloc;
        let mem_used = t.mem_ratio * m.mem_alloc;
        // Gauges measure live usage when metrics-server is present, else
        // scheduling pressure from requests.
        let tag = match t.metric_source {
            MetricSource::Usage => "use",
            MetricSource::Requests => "req",
        };
        let gauges = Line::from(vec![
            Span::raw(format!("cpu {tag} ")),
            Span::styled(bar(t.cpu_ratio, 14), theme.ratio(t.cpu_ratio)),
            Span::raw(format!(
                " {cpu_used:.1}/{:.0} cores   mem {tag} ",
                m.cpu_alloc
            )),
            Span::styled(bar(t.mem_ratio, 14), theme.ratio(t.mem_ratio)),
            Span::raw(format!(
                " {}/{}",
                human_bytes(mem_used),
                human_bytes(m.mem_alloc)
            )),
        ]);

        let terrain = m
            .info
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" · ");
        let conditions = m
            .conditions
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");

        let head = vec![
            Line::from(state_spans),
            gauges,
            Line::styled(terrain, theme.dim()),
            Line::styled(conditions, theme.dim()),
        ];
        let title = match ctx.cluster_label {
            Some(l) => format!(" NODE {} — {l} ", t.name),
            None => format!(" NODE {} ", t.name),
        };
        f.render_widget(
            Paragraph::new(head).block(
                Block::bordered()
                    .border_style(theme.chrome())
                    .title(title)
                    .title_style(theme.title())
                    .title_top(
                        Line::styled(format!(" zone {} ", t.zone), theme.zone()).right_aligned(),
                    ),
            ),
            head_a,
        );

        // --- Pods on this node ---------------------------------------------
        let rows: Vec<Row> = m
            .pods
            .iter()
            .map(|p| {
                let style = theme.pod(p.state);
                Row::new(vec![
                    Span::styled(pod_glyph(p.state).to_string(), style),
                    Span::raw(p.namespace.clone()),
                    Span::raw(p.name.clone()),
                    Span::styled(p.reason.clone(), style),
                    Span::raw(p.restarts.to_string()),
                    Span::raw(format_age_opt(p.age.as_ref())),
                    Span::styled(
                        p.owner
                            .as_ref()
                            .map(|o| o.to_string())
                            .unwrap_or_else(|| "—".into()),
                        theme.dim(),
                    ),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Min(22),
                Constraint::Length(18),
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(16),
            ],
        )
        .header(
            Row::new(vec![
                " ",
                "NAMESPACE",
                "POD",
                "STATUS",
                "RST",
                "AGE",
                "WORKLOAD",
            ])
            .style(theme.dim()),
        )
        .block(
            Block::bordered()
                .border_style(theme.chrome())
                .title(format!(
                    " PODS ({}) — Enter workload · l logs · e evict ",
                    m.pods.len()
                ))
                .title_style(theme.title()),
        )
        .row_highlight_style(theme.selection())
        .highlight_symbol("▸");
        f.render_stateful_widget(table, pods_a, &mut self.pods);
    }
}
