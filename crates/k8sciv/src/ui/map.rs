//! The world view: the game board. Continents of node-provinces with
//! health-textured terrain, workload cities with population badges and
//! name labels, daemonset roads, and namespace islands for the abstract
//! things. The cursor explores cell by cell; the camera follows.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::symbols::node_glyph;
use super::{Action, Component, Edge, OverlayMode, RenderCtx};
use k8sciv_core::state::model::NodeHealth;
use k8sciv_core::state::world::{City, Province, Region, WorldModel};
use k8sciv_core::util::truncate;

pub const CITY: char = '◍';
pub const INFRA: char = '≣';

/// Minimap inputs: cursor province cell and visible-province viewport.
pub type ChartState = (Option<(usize, usize)>, (usize, usize, usize, usize));

pub struct MapView {
    /// Absolute world cell under the explorer's cursor.
    pub cursor: (u16, u16),
    cam: (u16, u16),
    /// (w, h) of the last rendered viewport — paging distances.
    last_view: (u16, u16),
    /// True when the sidebar WORLD panel is showing this view's chart, so
    /// the floating overlay stays out of the way.
    pub external_minimap: bool,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            cursor: (2, 1), // first province label cell, not open sea
            cam: (0, 0),
            last_view: (80, 20),
            external_minimap: false,
        }
    }
}

impl MapView {
    /// Jump the explorer to a world position (attention routing).
    pub fn jump_to(&mut self, pos: (u16, u16)) {
        self.cursor = pos;
    }

    /// Minimap inputs: cursor province (col,row) and visible-province
    /// viewport, both in node-grid coordinates.
    pub fn chart_state(&self, world: &WorldModel) -> ChartState {
        (
            world.province_index_at(self.cursor.0, self.cursor.1),
            world.visible_provinces(self.cam, self.last_view),
        )
    }

    fn clamp_cursor(&mut self, world: &WorldModel) {
        self.cursor.0 = self.cursor.0.min(world.width.saturating_sub(1));
        self.cursor.1 = self.cursor.1.min(world.height.saturating_sub(1));
    }

    fn move_by(&mut self, dx: i32, dy: i32, world: &WorldModel) {
        let x = (self.cursor.0 as i32 + dx).clamp(0, world.width as i32 - 1);
        let y = (self.cursor.1 as i32 + dy).clamp(0, world.height as i32 - 1);
        self.cursor = (x as u16, y as u16);
    }

    /// Cycle to the next/previous city in stable world order.
    fn cycle_city(&mut self, world: &WorldModel, dir: i32) {
        let cities: Vec<&City> = world.cities().collect();
        if cities.is_empty() {
            return;
        }
        let key = |c: &City| (c.y, c.x);
        let cur = (self.cursor.1, self.cursor.0);
        let pos = if dir > 0 {
            cities.iter().position(|c| key(c) > cur).unwrap_or(0)
        } else {
            cities
                .iter()
                .rposition(|c| key(c) < cur)
                .unwrap_or(cities.len() - 1)
        };
        self.cursor = (cities[pos].x, cities[pos].y);
    }
}

impl Component for MapView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<Action> {
        let world = &ctx.models.world;
        if world.width == 0 {
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.last_view.1 as i32;
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if self.cursor.0 == 0 {
                    return Some(Action::EdgeReached(Edge::Left));
                }
                self.move_by(-1, 0, world);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.cursor.0 + 1 >= world.width {
                    return Some(Action::EdgeReached(Edge::Right));
                }
                self.move_by(1, 0, world);
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_by(0, -1, world),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(0, 1, world),
            KeyCode::PageDown => self.move_by(0, page, world),
            KeyCode::PageUp => self.move_by(0, -page, world),
            KeyCode::Char('d') if ctrl => self.move_by(0, (page / 2).max(1), world),
            KeyCode::Char('u') if ctrl => self.move_by(0, -(page / 2).max(1), world),
            KeyCode::Char(']') => self.cycle_city(world, 1),
            KeyCode::Char('[') => self.cycle_city(world, -1),
            KeyCode::Home => {
                if let Some(c) = world.continents.first() {
                    self.cursor.0 = c.x + 2;
                }
            }
            KeyCode::End => {
                if let Some(c) = world.continents.last() {
                    self.cursor.0 = c.x + 2;
                }
                self.clamp_cursor(world);
            }
            KeyCode::Char('g') => self.cursor.1 = 1.min(world.height.saturating_sub(1)),
            KeyCode::Char('G') => self.cursor.1 = world.height.saturating_sub(2),
            KeyCode::Enter => {
                return match world.region_at(self.cursor.0, self.cursor.1) {
                    Region::City(_, c) => Some(Action::OpenWorkload(c.r.clone())),
                    Region::Province(p) => Some(Action::OpenNode(p.tile.name.clone())),
                    Region::Structure(_, s) => s.workload.clone().map(Action::OpenWorkload),
                    _ => None,
                };
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, ctx: &RenderCtx) {
        self.clamp_cursor(&ctx.models.world);
    }

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        let theme = ctx.theme;
        if !ctx.ready {
            // Fog of war: the world is unexplored until the first sync lands.
            let buf = f.buffer_mut();
            for yy in area.top()..area.bottom() {
                for xx in area.left()..area.right() {
                    buf.set_string(xx, yy, "▒", theme.fog());
                }
            }
            let msg = format!(" exploring {} … ", ctx.world.meta.context);
            f.render_widget(
                Paragraph::new(Line::styled(msg, theme.dim())).centered(),
                vcenter(area),
            );
            return;
        }
        let world = &ctx.models.world;
        if world.continents.is_empty() {
            f.render_widget(
                Paragraph::new(Line::styled(
                    "no land sighted — no nodes observed",
                    theme.dim(),
                ))
                .centered(),
                vcenter(area),
            );
            return;
        }
        if area.width < 30 || area.height < 6 {
            f.render_widget(Paragraph::new("terminal too small"), area);
            return;
        }

        self.last_view = (area.width, area.height);
        // Camera follows the cursor with the window clamped to the world.
        if self.cursor.0 < self.cam.0 {
            self.cam.0 = self.cursor.0;
        }
        if self.cursor.0 >= self.cam.0 + area.width {
            self.cam.0 = self.cursor.0 + 1 - area.width;
        }
        if self.cursor.1 < self.cam.1 {
            self.cam.1 = self.cursor.1;
        }
        if self.cursor.1 >= self.cam.1 + area.height {
            self.cam.1 = self.cursor.1 + 1 - area.height;
        }
        self.cam.0 = self.cam.0.min(world.width.saturating_sub(area.width));
        self.cam.1 = self.cam.1.min(world.height.saturating_sub(area.height));

        let buf = f.buffer_mut();
        let (cam_x, cam_y) = self.cam;

        // --- Open sea ------------------------------------------------------
        let sea = theme.sea();
        for vy in 0..area.height {
            let wy = cam_y + vy;
            for vx in 0..area.width {
                let wx = cam_x + vx;
                if (wx as u32 * 7 + wy as u32 * 13).is_multiple_of(19) {
                    buf.set_string(area.x + vx, area.y + vy, "~", sea);
                }
            }
        }

        // --- Continents ------------------------------------------------------
        for cont in &world.continents {
            // The landmass name on the shore above it.
            if let Some((sx, sy)) =
                project(area, (cam_x, cam_y), cont.x + 1, cont.y.saturating_sub(1))
            {
                let label = format!("≈ {} · {} ≈", cont.zone, cont.provinces.len());
                let width = (area.right() - sx).min(cont.w) as usize;
                buf.set_stringn(sx, sy, &label, width, theme.zone());
            }
            for p in &cont.provinces {
                draw_province(buf, area, (cam_x, cam_y), p, ctx);
            }
        }

        // --- Islands ---------------------------------------------------------
        for isl in &world.islands {
            draw_island(buf, area, (cam_x, cam_y), isl, ctx);
        }

        // --- Cursor ----------------------------------------------------------
        if ctx.focused
            && let (Some(vx), Some(vy)) = (
                self.cursor.0.checked_sub(cam_x),
                self.cursor.1.checked_sub(cam_y),
            )
            && vx < area.width
            && vy < area.height
            && let Some(cell) = buf.cell_mut((area.x + vx, area.y + vy))
        {
            let patched = cell.style().patch(theme.selection());
            cell.set_style(patched);
        }

        // --- Scroll hints ------------------------------------------------------
        let hint = theme.dim();
        if cam_x > 0 {
            buf.set_string(area.x, area.y, "◂", hint);
        }
        if cam_x + area.width < world.width {
            buf.set_string(area.x + area.width - 1, area.y, "▸", hint);
        }
        if cam_y > 0 {
            buf.set_string(area.x + area.width - 1, area.y + 1, "▴", hint);
        }
        if cam_y + area.height < world.height {
            buf.set_string(area.x + area.width - 1, area.y + area.height - 1, "▾", hint);
        }

        // Floating world chart when the sidebar isn't already showing it.
        let overflows = world.width > area.width || world.height > area.height;
        if overflows && !self.external_minimap {
            let (cursor_cell, viewport) = self.chart_state(world);
            draw_minimap(buf, area, ctx, cursor_cell, viewport);
        }
    }
}

/// World cell → viewport cell, if visible.
fn project(area: Rect, cam: (u16, u16), wx: u16, wy: u16) -> Option<(u16, u16)> {
    let vx = wx.checked_sub(cam.0)?;
    let vy = wy.checked_sub(cam.1)?;
    (vx < area.width && vy < area.height).then_some((area.x + vx, area.y + vy))
}

fn terrain_char(h: NodeHealth) -> char {
    match h {
        NodeHealth::Healthy => ',',
        NodeHealth::Cordoned => '=',
        NodeHealth::Pressure => '∩',
        NodeHealth::NotReady => '×',
    }
}

fn draw_province(buf: &mut Buffer, area: Rect, cam: (u16, u16), p: &Province, ctx: &RenderCtx) {
    let theme = ctx.theme;
    let dim_terrain = ctx.overlay == OverlayMode::ReplicaHealth;

    // Land: solid ground (no sea showing through) with a sparse terrain
    // texture keyed to the province's health.
    let tex = terrain_char(p.tile.health);
    let tex_style = if dim_terrain {
        theme.dim()
    } else {
        theme.terrain(p.tile.health)
    };
    for wy in p.y..p.y + p.h {
        for wx in p.x..p.x + p.w {
            if let Some((sx, sy)) = project(area, cam, wx, wy) {
                if (wx as u32 * 31 + wy as u32 * 17).is_multiple_of(5) {
                    buf.set_string(sx, sy, tex.to_string(), tex_style);
                } else {
                    buf.set_string(sx, sy, " ", ratatui::style::Style::new());
                }
            }
        }
    }

    // Province label: glyph + name + pod count + infrastructure roads.
    if let Some((sx, sy)) = project(area, cam, p.x + 1, p.y) {
        let max = (p.w - 2) as usize;
        let mut label = format!(
            "{} {} ●{}",
            node_glyph(p.tile.health),
            truncate(&p.tile.name, max.saturating_sub(8)),
            p.tile.pods.len()
        );
        if p.infra > 0 {
            label.push_str(&format!(" {INFRA}{}", p.infra));
        }
        let width = (area.right() - sx).min(max as u16) as usize;
        buf.set_stringn(sx, sy, &label, width, theme.province(p.tile.health));
    }

    // Cities.
    for c in &p.cities {
        draw_city(buf, area, cam, c, ctx);
    }
}

fn draw_city(buf: &mut Buffer, area: Rect, cam: (u16, u16), c: &City, ctx: &RenderCtx) {
    let theme = ctx.theme;
    let pop_style = match c.severity {
        Some(sev) => theme.severity(sev),
        None if c.ready < c.desired => {
            theme.severity(k8sciv_core::state::attention::Severity::Warning)
        }
        None => theme.city(),
    };
    if let Some((sx, sy)) = project(area, cam, c.x, c.y) {
        let mut badge = format!("{CITY}{}", c.ready);
        if let Some(sev) = c.severity {
            badge.push_str(sev.glyph());
        }
        let width = (area.right() - sx).min(8) as usize;
        buf.set_stringn(sx, sy, &badge, width, pop_style);
    }
    if let Some((sx, sy)) = project(area, cam, c.x, c.y + 1) {
        let name_style = match ctx.overlay {
            OverlayMode::Namespace => theme.namespace(&c.r.namespace),
            _ => theme.tile_name(),
        };
        let width = (area.right() - sx).min(14) as usize;
        buf.set_stringn(sx, sy, truncate(&c.r.name, 14), width, name_style);
    }
}

fn draw_island(
    buf: &mut Buffer,
    area: Rect,
    cam: (u16, u16),
    isl: &k8sciv_core::state::world::Island,
    ctx: &RenderCtx,
) {
    let theme = ctx.theme;
    // Sandy shore: solid island ground with sparse texture.
    for wy in isl.y..isl.y + isl.h {
        for wx in isl.x..isl.x + isl.w {
            if let Some((sx, sy)) = project(area, cam, wx, wy) {
                if (wx as u32 * 13 + wy as u32 * 7).is_multiple_of(4) {
                    buf.set_string(sx, sy, "·", theme.shore());
                } else {
                    buf.set_string(sx, sy, " ", ratatui::style::Style::new());
                }
            }
        }
    }
    if let Some((sx, sy)) = project(area, cam, isl.x + 1, isl.y) {
        let label = format!("≈ {} ≈", truncate(&isl.label, (isl.w - 6) as usize));
        let width = (area.right() - sx).min(isl.w - 2) as usize;
        buf.set_stringn(sx, sy, &label, width, theme.zone());
    }
    for s in &isl.structures {
        if let Some((sx, sy)) = project(area, cam, isl.x + 2, s.y) {
            let label = format!("{} {}/{}", s.glyph, s.kind, s.name);
            let style = if s.glyph == '✦' {
                theme.structure()
            } else {
                theme.dim()
            };
            let width = (area.right() - sx).min(isl.w - 3) as usize;
            buf.set_stringn(sx, sy, truncate(&label, width), width, style);
        }
    }
    if isl.more > 0
        && let Some((sx, sy)) = project(area, cam, isl.x + 2, isl.y + isl.h - 1)
    {
        buf.set_stringn(
            sx,
            sy,
            format!("+{} more", isl.more),
            (isl.w - 3) as usize,
            theme.dim(),
        );
    }
}

fn vcenter(area: Rect) -> Rect {
    Rect {
        y: area.y + area.height / 2,
        height: 1,
        ..area
    }
}

/// Floating world chart (when no sidebar is present): bottom-right,
/// auto-sized, parchment-framed.
fn draw_minimap(
    buf: &mut Buffer,
    map_area: Rect,
    ctx: &RenderCtx,
    cursor_cell: Option<(usize, usize)>,
    viewport: (usize, usize, usize, usize),
) {
    let zones = &ctx.models.map.zones;
    let zcount = zones.len() as u16;
    let max_rows = zones.iter().map(|z| z.nodes.len()).max().unwrap_or(0) as u16;
    if zcount == 0 || max_rows == 0 {
        return;
    }
    let avail_h = map_area.height.saturating_sub(6);
    if avail_h == 0 {
        return;
    }
    let k = max_rows.div_ceil(avail_h).max(1);
    let grid_w = zcount * 2 - 1;
    let grid_h = max_rows.div_ceil(k);
    let w = grid_w + 4;
    let h = grid_h + 4;
    if map_area.width < w + 4 || map_area.height < h + 1 {
        return; // not enough room without smothering the board
    }
    let area = Rect {
        x: map_area.right() - w - 1,
        y: map_area.bottom() - h,
        width: w,
        height: h,
    };
    for yy in area.top()..area.bottom() {
        for xx in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((xx, yy)) {
                cell.reset();
            }
        }
    }
    let block = Block::bordered()
        .border_style(ctx.theme.chrome())
        .title(" WORLD ")
        .title_style(ctx.theme.title());
    let inner = block.inner(area);
    block.render(area, buf);
    draw_world_cells(buf, inner, ctx, cursor_cell, viewport);
}

/// The world chart grid — dark ocean, land cells, a framed viewport.
/// Shared by the floating overlay and the sidebar WORLD panel. `inner`
/// includes a one-cell margin all around for the viewport frame.
pub(crate) fn draw_world_cells(
    buf: &mut Buffer,
    inner: Rect,
    ctx: &RenderCtx,
    cursor_cell: Option<(usize, usize)>,
    viewport: (usize, usize, usize, usize),
) {
    let theme = ctx.theme;
    let zones = &ctx.models.map.zones;
    let zcount = zones.len() as u16;
    let max_rows = zones.iter().map(|z| z.nodes.len()).max().unwrap_or(0) as u16;
    if zcount == 0 || max_rows == 0 || inner.width < 3 || inner.height < 3 {
        return;
    }
    // Ocean fill.
    for yy in inner.top()..inner.bottom() {
        for xx in inner.left()..inner.right() {
            buf.set_string(xx, yy, " ", theme.ocean());
        }
    }
    let grid_w = zcount * 2 - 1;
    if grid_w + 2 > inner.width {
        return; // too many zones for this panel; ocean stays empty
    }
    let k = max_rows.div_ceil(inner.height - 2).max(1);
    let origin = (inner.x + 1, inner.y + 1);

    // Land cells, worst-state-wins within a compressed cell.
    for (zi, zone) in zones.iter().enumerate() {
        let cx = origin.0 + zi as u16 * 2;
        for (ci, chunk) in zone.nodes.chunks(k as usize).enumerate() {
            let worst = chunk
                .iter()
                .map(|t| t.health)
                .max_by_key(|h| health_rank(*h))
                .unwrap_or(NodeHealth::Healthy);
            let (ch, style) = theme.land_cell(worst);
            let is_cursor = cursor_cell.is_some_and(|(zc, nr)| zc == zi && nr / k as usize == ci);
            let style = if is_cursor {
                style.patch(theme.selection())
            } else {
                style
            };
            buf.set_string(cx, origin.1 + ci as u16, ch, style);
        }
    }

    // Viewport frame in the margin columns, hugging the first and last
    // visible cell rows exactly (no half-row exists to sit between). A
    // single-row viewport borrows the margin rows above and below.
    let (sc, sr, vc, vr) = viewport;
    let last_col = (sc + vc).min(zones.len()).saturating_sub(1);
    let last_row = ((sr + vr).min(max_rows as usize)).saturating_sub(1);
    let x0 = origin.0 + (sc as u16).min(zcount - 1) * 2 - 1;
    let x1 = origin.0 + (last_col as u16).min(zcount - 1) * 2 + 1;
    let mut y0 = origin.1 + (sr as u16).min(max_rows - 1) / k;
    let mut y1 = origin.1 + (last_row as u16).min(max_rows - 1) / k;
    if y0 == y1 {
        y0 -= 1;
        y1 += 1;
    }
    let bstyle = theme.viewport();
    buf.set_string(x0, y0, "┌", bstyle);
    buf.set_string(x1, y0, "┐", bstyle);
    buf.set_string(x0, y1, "└", bstyle);
    buf.set_string(x1, y1, "┘", bstyle);
}

fn health_rank(h: NodeHealth) -> u8 {
    match h {
        NodeHealth::Healthy => 0,
        NodeHealth::Cordoned => 1,
        NodeHealth::Pressure => 2,
        NodeHealth::NotReady => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::events::ClusterId;
    use crate::ui::theme::Theme;
    use k8sciv_core::state::fixtures as fx;
    use k8sciv_core::state::model::Models;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn buffer_text(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    macro_rules! ctx {
        ($models:expr, $world:expr, $theme:expr) => {
            RenderCtx {
                models: &$models,
                world: &$world,
                theme: &$theme,
                overlay: OverlayMode::Pressure,
                ready: true,
                cluster: ClusterId::Hot,
                focused: true,
                pair: None,
                cluster_label: None,
                attention: &[],
            }
        };
    }

    fn demo_world() -> (k8sciv_core::state::observed::ObservedWorld, fx::Seeds) {
        let (world, mut s) = fx::world();
        s.node(fx::node("n-alpha", Some("z-a")));
        s.node(fx::node("n-bravo", Some("z-b")));
        s.deployment(fx::deployment("demo", "web", 2, 2));
        s.replicaset(fx::replicaset("demo", "web-abc", "web"));
        for i in 0..2 {
            s.pod(fx::pod_owned(
                fx::pod("demo", &format!("web-abc-{i}"), Some("n-alpha")),
                "ReplicaSet",
                "web-abc",
            ));
        }
        (world, s)
    }

    /// Snapshot-style: provinces, a sited city with population badge and
    /// name label, and open sea all render.
    #[test]
    fn world_renders_provinces_city_and_sea() {
        let (world, _s) = demo_world();
        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let ctx = ctx!(models, world, theme);
        let mut view = MapView::default();
        let mut term = Terminal::new(TestBackend::new(80, 16)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let text = buffer_text(&term);

        assert!(text.contains("▣ n-alpha ●2"), "province label:\n{text}");
        assert!(text.contains("▣ n-bravo ●0"), "empty province:\n{text}");
        assert!(text.contains(&format!("{CITY}2")), "city badge:\n{text}");
        assert!(text.contains("web"), "city name label:\n{text}");
        assert!(text.contains('~'), "open sea:\n{text}");
        assert!(text.contains(','), "grassland texture:\n{text}");
    }

    #[test]
    fn explorer_cursor_walks_cities_and_opens_regions() {
        let (world, _s) = demo_world();
        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let ctx = ctx!(models, world, theme);
        let mut view = MapView::default();
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        // ']' jumps straight onto the city; Enter opens its city screen.
        assert_eq!(view.handle_key(key(KeyCode::Char(']')), &ctx), None);
        let city = models.world.cities().next().unwrap();
        assert_eq!(view.cursor, (city.x, city.y));
        assert_eq!(
            view.handle_key(key(KeyCode::Enter), &ctx),
            Some(Action::OpenWorkload(city.r.clone()))
        );

        // Standing on plain land opens the province's node.
        view.cursor = (2, 1);
        assert_eq!(
            view.handle_key(key(KeyCode::Enter), &ctx),
            Some(Action::OpenNode("n-alpha".into()))
        );

        // World edges report for continent crossing in pair mode.
        view.cursor = (0, 1);
        assert_eq!(
            view.handle_key(key(KeyCode::Char('h')), &ctx),
            Some(Action::EdgeReached(Edge::Left))
        );
        view.cursor = (models.world.width - 1, 1);
        assert_eq!(
            view.handle_key(key(KeyCode::Char('l')), &ctx),
            Some(Action::EdgeReached(Edge::Right))
        );
    }

    /// 100 nodes across 5 zones, 1000 pods in 20 deployments — the shared
    /// at-scale fixture for perf and minimap tests.
    fn big_world() -> (k8sciv_core::state::observed::ObservedWorld, fx::Seeds) {
        const ZONES: [&str; 5] = ["z-a", "z-b", "z-c", "z-d", "z-e"];
        let (world, mut s) = fx::world();
        for n in 0..100 {
            s.node(fx::node(&format!("perf-node-{n:03}"), Some(ZONES[n % 5])));
        }
        for d in 0..20 {
            let deploy = format!("app-{d:02}");
            let rs = format!("app-{d:02}-abc");
            s.deployment(fx::deployment("perf", &deploy, 50, 50));
            s.replicaset(fx::replicaset("perf", &rs, &deploy));
            for p in 0..50 {
                let node = format!("perf-node-{:03}", (d * 50 + p) % 100);
                let mut pod = fx::pod_requests(
                    fx::pod("perf", &format!("{rs}-{p:02}"), Some(&node)),
                    "100m",
                    "128Mi",
                );
                pod = fx::pod_owned(pod, "ReplicaSet", &rs);
                if p == 0 {
                    pod = fx::pod_waiting(pod, "CrashLoopBackOff"); // keep attention busy
                }
                s.pod(pod);
            }
        }
        (world, s)
    }

    #[test]
    fn minimap_appears_on_overflow_with_viewport_brackets() {
        let (world, mut s) = big_world();
        s.node(fx::node_with_condition(
            fx::node("perf-node-000", Some("z-a")),
            "Ready",
            "False",
        ));
        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let ctx = ctx!(models, world, theme);
        let mut view = MapView::default();
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let text = buffer_text(&term);

        assert!(text.contains("WORLD"), "world chart missing:\n{text}");
        assert!(
            text.matches('┌').count() >= 2 && text.matches('┘').count() >= 2,
            "viewport brackets missing:\n{text}"
        );
    }

    #[test]
    fn paging_keys_jump_by_viewport() {
        let (world, _s) = big_world();
        let models = Models::build(&world);
        let theme = Theme::new(ColorMode::Auto);
        let ctx = ctx!(models, world, theme);
        let mut view = MapView::default();
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        let y0 = view.cursor.1;
        view.handle_key(key(KeyCode::PageDown), &ctx);
        assert_eq!(view.cursor.1, y0 + 40);
        view.handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            &ctx,
        );
        assert_eq!(view.cursor.1, y0 + 20);
        view.handle_key(key(KeyCode::End), &ctx);
        let last = models.world.continents.last().unwrap();
        assert_eq!(view.cursor.0, last.x + 2);
        view.handle_key(key(KeyCode::Home), &ctx);
        assert_eq!(view.cursor.0, 2);
    }

    /// Criterion 6 evidence: at 100 nodes / 1000 pods, a full world rebuild
    /// (map + workloads + attention + world) plus a rendered frame must fit
    /// the 100ms input-latency budget. Asserted in release
    /// (`make perf-test`); debug builds only report the numbers.
    #[test]
    fn scale_rebuild_and_render_within_budget() {
        let (world, _s) = big_world();
        let theme = Theme::new(ColorMode::Auto);
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        let mut view = MapView::default();

        let mut worst = std::time::Duration::ZERO;
        let t_all = std::time::Instant::now();
        for i in 0..20u16 {
            let t = std::time::Instant::now();
            let models = Models::build(&world);
            let ctx = ctx!(models, world, theme);
            // Wiggle the cursor so the camera-follow paths run too.
            view.cursor = ((i * 13) % models.world.width, (i * 7) % models.world.height);
            term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
            worst = worst.max(t.elapsed());
            assert_eq!(models.map.total_nodes, 100);
            assert_eq!(models.map.total_pods, 1000);
        }
        let avg = t_all.elapsed() / 20;
        println!("scale 100n/1000p: avg {avg:?}, worst {worst:?} per rebuild+frame");
        if !cfg!(debug_assertions) {
            assert!(
                worst < std::time::Duration::from_millis(100),
                "worst rebuild+frame {worst:?} exceeds the 100ms budget"
            );
        }
    }
}
