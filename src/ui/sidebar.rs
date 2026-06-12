//! The Civ-style right sidebar on the map screen: WORLD (minimap), STATUS
//! (people/gold ≈ nodes/pods/concerns), and ORDERS (the selected tile —
//! Civ's "Moving Unit" box). Shows the focused world; auto-hidden on
//! narrow terminals, where the floating world overlay takes back over.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::map::{MapView, draw_world_cells};
use super::symbols::{node_glyph, pod_glyph};
use super::{OverlayMode, RenderCtx};
use crate::state::attention::{Severity, severity_counts};
use crate::state::model::PodState;
use crate::util::truncate;

pub const SIDEBAR_W: u16 = 30;

pub fn render(f: &mut Frame, area: Rect, ctx: &RenderCtx, map: &MapView) {
    let theme = ctx.theme;
    let zones = &ctx.models.map.zones;
    let max_rows = zones.iter().map(|z| z.nodes.len()).max().unwrap_or(0) as u16;

    // WORLD gets what its grid wants, capped at half the sidebar.
    let world_h = (max_rows + 4).clamp(5, (area.height / 2).max(5));
    let [world_a, status_a, orders_a] = Layout::vertical([
        Constraint::Length(world_h),
        Constraint::Length(8),
        Constraint::Min(4),
    ])
    .areas(area);

    // --- WORLD --------------------------------------------------------
    let block = Block::bordered()
        .border_style(theme.chrome())
        .title(" WORLD ")
        .title_style(theme.title());
    let inner = block.inner(world_a);
    f.render_widget(block, world_a);
    if ctx.ready {
        draw_world_cells(f.buffer_mut(), inner, ctx, map.cursor, map.viewport());
    } else {
        let buf = f.buffer_mut();
        for yy in inner.top()..inner.bottom() {
            for xx in inner.left()..inner.right() {
                buf.set_string(xx, yy, "▒", theme.fog());
            }
        }
    }

    // --- STATUS -------------------------------------------------------
    let meta = &ctx.world.meta;
    let counts = severity_counts(ctx.attention);
    let mut concern_spans: Vec<Span> = Vec::new();
    if ctx.attention.is_empty() {
        concern_spans.push(Span::styled("all quiet", theme.dim()));
    } else {
        for sev in [Severity::Critical, Severity::Warning, Severity::Info] {
            if let Some(n) = counts.get(&sev) {
                concern_spans.push(Span::styled(
                    format!("{}{n} ", sev.glyph()),
                    theme.severity(sev),
                ));
            }
        }
        concern_spans.push(Span::styled("concerns", theme.dim()));
    }
    let overlay = match ctx.overlay {
        OverlayMode::Pressure => "pressure",
        OverlayMode::ReplicaHealth => "replica health",
        OverlayMode::Namespace => "namespace",
    };
    let status_lines = vec![
        Line::from(Span::styled(truncate(&meta.context, 26), theme.tile_name())),
        Line::from(Span::styled(
            format!("{} · {}", meta.platform.label(), truncate(&meta.server, 18)),
            theme.dim(),
        )),
        Line::from(vec![
            Span::styled(format!("{}", ctx.models.map.total_nodes), theme.title()),
            Span::styled(" nodes  ", theme.dim()),
            Span::styled(format!("{}", ctx.models.map.total_pods), theme.title()),
            Span::styled(" pods", theme.dim()),
        ]),
        Line::from(concern_spans),
        Line::from(vec![
            Span::styled("overlay ", theme.dim()),
            Span::raw(overlay),
        ]),
        Line::from(Span::styled(
            if ctx.ready { "" } else { "exploring…" },
            theme.dim(),
        )),
    ];
    f.render_widget(
        Paragraph::new(status_lines).block(
            Block::bordered()
                .border_style(theme.chrome())
                .title(" STATUS ")
                .title_style(theme.title()),
        ),
        status_a,
    );

    // --- ORDERS (selected tile) ----------------------------------------
    let mut orders: Vec<Line> = Vec::new();
    if let Some(tile) = ctx.models.map.tile(map.cursor.0, map.cursor.1) {
        orders.push(Line::from(vec![
            Span::styled(
                format!("{} ", node_glyph(tile.health)),
                theme.node(tile.health),
            ),
            Span::styled(truncate(&tile.name, 24), theme.tile_name()),
        ]));
        orders.push(Line::from(Span::styled(
            format!("zone {}", tile.zone),
            theme.zone(),
        )));
        if !tile.ready {
            orders.push(Line::from(Span::styled(
                "NotReady",
                theme.severity(Severity::Critical),
            )));
        }
        for cond in &tile.abnormal {
            orders.push(Line::from(Span::styled(
                cond.to_string(),
                theme.severity(Severity::Warning),
            )));
        }
        if tile.cordoned {
            orders.push(Line::from(Span::styled(
                "cordoned ⊘",
                theme.node(crate::state::model::NodeHealth::Cordoned),
            )));
        }
        orders.push(Line::from(vec![
            Span::styled("cpu ", theme.dim()),
            Span::styled(
                format!("{:>3.0}% ", (tile.cpu_ratio * 100.0).min(999.0)),
                theme.ratio(tile.cpu_ratio),
            ),
            Span::styled("mem ", theme.dim()),
            Span::styled(
                format!("{:>3.0}%", (tile.mem_ratio * 100.0).min(999.0)),
                theme.ratio(tile.mem_ratio),
            ),
        ]));
        // Pod census by state, like a unit roster.
        let mut census: Vec<Span> = vec![Span::styled(
            format!("{} pods ", tile.pods.len()),
            theme.dim(),
        )];
        for state in [
            PodState::Ok,
            PodState::Starting,
            PodState::Pending,
            PodState::Failing,
            PodState::Terminating,
            PodState::Succeeded,
        ] {
            let n = tile.pods.iter().filter(|p| p.state == state).count();
            if n > 0 {
                census.push(Span::styled(
                    format!("{}{n} ", pod_glyph(state)),
                    theme.pod(state),
                ));
            }
        }
        orders.push(Line::from(census));
        orders.push(Line::from(""));
        orders.push(Line::from(Span::styled("[Enter] inspect", theme.dim())));
    } else {
        orders.push(Line::from(Span::styled("no tile selected", theme.dim())));
    }
    f.render_widget(
        Paragraph::new(orders).block(
            Block::bordered()
                .border_style(theme.chrome())
                .title(" ORDERS ")
                .title_style(theme.title()),
        ),
        orders_a,
    );
}
