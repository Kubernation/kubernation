//! The 4X-style right sidebar on the map screen: WORLD (the chart),
//! STATUS (people/gold ≈ nodes/pods/concerns), and ORDERS — whatever the
//! explorer's cursor is standing on (city, province, structure, or open
//! sea), 4X's "Moving Unit" box. Shows the focused world; auto-hidden on
//! narrow terminals, where the floating chart takes back over.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::map::{CITY, MapView, draw_world_cells};
use super::symbols::{node_glyph, pod_glyph};
use super::{OverlayMode, RenderCtx};
use kubernation_core::state::attention::{Severity, severity_counts};
use kubernation_core::state::model::{PodState, RolloutStatus};
use kubernation_core::state::world::{Province, Region};
use kubernation_core::util::truncate;

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
        let (cursor_cell, viewport) = map.chart_state(&ctx.models.world);
        draw_world_cells(f.buffer_mut(), inner, ctx, cursor_cell, viewport);
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
        OverlayMode::Pressure => "terrain",
        OverlayMode::ReplicaHealth => "cities",
        OverlayMode::Namespace => "nations",
    };
    let status_lines = vec![
        Line::from(Span::styled(truncate(&meta.context, 26), theme.tile_name())),
        Line::from(Span::styled(
            format!("{} · {}", meta.platform.label(), truncate(&meta.server, 18)),
            theme.dim(),
        )),
        Line::from(vec![
            Span::styled(format!("{}", ctx.models.map.total_nodes), theme.title()),
            Span::styled(" provinces  ", theme.dim()),
            Span::styled(format!("{}", ctx.models.world.city_count), theme.title()),
            Span::styled(" cities", theme.dim()),
        ]),
        Line::from(vec![
            Span::styled(format!("{}", ctx.models.map.total_pods), theme.title()),
            Span::styled(" pods", theme.dim()),
        ]),
        Line::from(concern_spans),
        Line::from(vec![Span::styled("lens ", theme.dim()), Span::raw(overlay)]),
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

    // --- ORDERS: what the cursor stands on -------------------------------
    let world = &ctx.models.world;
    let mut orders: Vec<Line> = Vec::new();
    match world.region_at(map.cursor.0, map.cursor.1) {
        Region::City(p, c) => {
            orders.push(Line::from(vec![
                Span::styled(format!("{CITY} "), theme.city()),
                Span::styled(truncate(&c.r.name, 22), theme.tile_name()),
            ]));
            orders.push(Line::from(Span::styled(
                format!("{} {}/{}", c.r.kind, c.r.namespace, c.r.name),
                theme.dim(),
            )));
            let gap_style = if c.ready < c.desired {
                theme.severity(Severity::Warning)
            } else {
                theme.dim()
            };
            orders.push(Line::from(vec![
                Span::styled(format!("pop {}", c.ready), theme.title()),
                Span::styled(format!(" of {} desired", c.desired), gap_style),
            ]));
            if let Some(row) = ctx.models.workloads.iter().find(|w| w.r == c.r) {
                let style = match row.status {
                    RolloutStatus::Complete => theme.dim(),
                    RolloutStatus::Stalled => theme.severity(Severity::Critical),
                    _ => theme.severity(Severity::Info),
                };
                orders.push(Line::from(Span::styled(
                    format!("rollout {}", row.status),
                    style,
                )));
            }
            if let Some(sev) = c.severity {
                orders.push(Line::from(Span::styled(
                    format!("{} needs attention", sev.glyph()),
                    theme.severity(sev),
                )));
            }
            if let Some(pair) = ctx.pair
                && let Some(st) = pair.state(&c.r)
            {
                orders.push(Line::from(Span::styled(
                    st.describe(ctx.cluster),
                    theme.sync(st),
                )));
            }
            orders.push(Line::from(Span::styled(
                format!("on {}", truncate(&p.tile.name, 24)),
                theme.dim(),
            )));
            orders.push(Line::from(""));
            orders.push(Line::from(Span::styled("[Enter] city screen", theme.dim())));
        }
        Region::Province(p) => province_orders(&mut orders, p, ctx),
        Region::Structure(isl, s) => {
            orders.push(Line::from(vec![
                Span::styled(format!("{} ", s.glyph), theme.structure()),
                Span::styled(truncate(&s.name, 22), theme.tile_name()),
            ]));
            orders.push(Line::from(Span::styled(
                format!("{} · isle of {}", s.kind, isl.label),
                theme.dim(),
            )));
            if s.workload.is_some() {
                orders.push(Line::from(Span::styled(
                    "no pods on any land",
                    theme.severity(Severity::Warning),
                )));
                orders.push(Line::from(""));
                orders.push(Line::from(Span::styled("[Enter] city screen", theme.dim())));
            }
        }
        Region::Island(isl) => {
            orders.push(Line::from(Span::styled(
                format!("≈ isle of {} ≈", isl.label),
                theme.zone(),
            )));
            orders.push(Line::from(Span::styled(
                format!("{} structures", isl.structures.len() + isl.more),
                theme.dim(),
            )));
        }
        Region::Ocean => {
            orders.push(Line::from(Span::styled("open sea", theme.sea())));
            orders.push(Line::from(Span::styled(
                format!("sector {},{}", map.cursor.0, map.cursor.1),
                theme.dim(),
            )));
            orders.push(Line::from(""));
            orders.push(Line::from(Span::styled(
                "] sails to the next city",
                theme.dim(),
            )));
        }
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

fn province_orders(orders: &mut Vec<Line>, p: &Province, ctx: &RenderCtx) {
    let theme = ctx.theme;
    let tile = &p.tile;
    orders.push(Line::from(vec![
        Span::styled(
            format!("{} ", node_glyph(tile.health)),
            theme.node(tile.health),
        ),
        Span::styled(truncate(&tile.name, 24), theme.tile_name()),
    ]));
    orders.push(Line::from(Span::styled(
        format!("province of {}", tile.zone),
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
            theme.node(kubernation_core::state::model::NodeHealth::Cordoned),
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
        let n = tile.pods.iter().filter(|q| q.state == state).count();
        if n > 0 {
            census.push(Span::styled(
                format!("{}{n} ", pod_glyph(state)),
                theme.pod(state),
            ));
        }
    }
    orders.push(Line::from(census));
    if p.infra > 0 {
        orders.push(Line::from(Span::styled(
            format!("≣ {} daemonset roads", p.infra),
            theme.dim(),
        )));
    }
    orders.push(Line::from(""));
    orders.push(Line::from(Span::styled("[Enter] inspect", theme.dim())));
}
