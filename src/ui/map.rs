//! The main map: zone columns of node tiles. This is the game board.
//!
//! Tile anatomy (22×4):
//! ```text
//! ▣ kind-worker2     ⚠M
//! c ▓▓▓▓▓▓░░░░░░  52%
//! m ▓▓▓░░░░░░░░░  28%
//! ●●●●●◐○✗          12p
//! ```

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget};
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::symbols::{bar, node_glyph, pod_glyph};
use super::{Action, Component, Edge, OverlayMode, RenderCtx};
use crate::state::attention::Severity;
use crate::state::model::{NodeHealth, NodeTile, PodState, ZoneColumn};
use crate::util::truncate;

pub const TILE_W: u16 = 22;
pub const TILE_H: u16 = 4;
const COL_GAP: u16 = 2;
const ROW_GAP: u16 = 1;

pub struct MapView {
    /// (zone column index, node index within column)
    pub cursor: (usize, usize),
    scroll_col: usize,
    scroll_row: usize,
    /// (cols, rows) of the last rendered viewport — paging jump distances.
    last_visible: (usize, usize),
    /// True when the sidebar WORLD panel is showing this view's minimap, so
    /// the floating overlay stays out of the way.
    pub external_minimap: bool,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            cursor: (0, 0),
            scroll_col: 0,
            scroll_row: 0,
            last_visible: (1, 8),
            external_minimap: false,
        }
    }
}

impl MapView {
    /// (scroll_col, scroll_row, visible_cols, visible_rows) as of the last
    /// render — the sidebar uses this to frame the viewport on the world.
    pub fn viewport(&self) -> (usize, usize, usize, usize) {
        (
            self.scroll_col,
            self.scroll_row,
            self.last_visible.0,
            self.last_visible.1,
        )
    }
}

impl MapView {
    pub fn selected_node(&self, ctx: &RenderCtx) -> Option<String> {
        ctx.models
            .map
            .tile(self.cursor.0, self.cursor.1)
            .map(|t| t.name.clone())
    }

    /// Returns false when the cursor was already pinned at the edge.
    fn move_zone(&mut self, delta: isize, ctx: &RenderCtx) -> bool {
        let zones = &ctx.models.map.zones;
        if zones.is_empty() {
            return false;
        }
        let before = self.cursor.0;
        let z = (self.cursor.0 as isize + delta).clamp(0, zones.len() as isize - 1) as usize;
        let max_node = zones[z].nodes.len().saturating_sub(1);
        self.cursor = (z, self.cursor.1.min(max_node));
        delta == 0 || self.cursor.0 != before
    }

    fn move_node(&mut self, delta: isize, ctx: &RenderCtx) {
        let zones = &ctx.models.map.zones;
        let Some(col) = zones.get(self.cursor.0) else {
            return;
        };
        if col.nodes.is_empty() {
            return;
        }
        let n = (self.cursor.1 as isize + delta).clamp(0, col.nodes.len() as isize - 1) as usize;
        self.cursor.1 = n;
    }
}

impl Component for MapView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.last_visible.1 as isize;
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                let moved = self.move_zone(-1, ctx);
                if !moved {
                    return Some(Action::EdgeReached(Edge::Left));
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let moved = self.move_zone(1, ctx);
                if !moved {
                    return Some(Action::EdgeReached(Edge::Right));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_node(-1, ctx),
            KeyCode::Down | KeyCode::Char('j') => self.move_node(1, ctx),
            KeyCode::PageDown => self.move_node(page, ctx),
            KeyCode::PageUp => self.move_node(-page, ctx),
            KeyCode::Char('d') if ctrl => self.move_node((page / 2).max(1), ctx),
            KeyCode::Char('u') if ctrl => self.move_node(-((page / 2).max(1)), ctx),
            KeyCode::Home => {
                self.cursor.0 = 0;
                let _ = self.move_zone(0, ctx);
            }
            KeyCode::End => {
                let zones = ctx.models.map.zones.len();
                if zones > 0 {
                    self.cursor.0 = zones - 1;
                    let _ = self.move_zone(0, ctx);
                }
            }
            KeyCode::Char('g') => self.cursor.1 = 0,
            KeyCode::Char('G') => {
                if let Some(col) = ctx.models.map.zones.get(self.cursor.0) {
                    self.cursor.1 = col.nodes.len().saturating_sub(1);
                }
            }
            KeyCode::Enter => return self.selected_node(ctx).map(Action::OpenNode),
            _ => {}
        }
        None
    }

    fn update(&mut self, ctx: &RenderCtx) {
        let zones = &ctx.models.map.zones;
        if zones.is_empty() {
            self.cursor = (0, 0);
            return;
        }
        let z = self.cursor.0.min(zones.len() - 1);
        let n = self.cursor.1.min(zones[z].nodes.len().saturating_sub(1));
        self.cursor = (z, n);
    }

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx) {
        if !ctx.ready {
            // Fog of war: the world is unexplored until the first sync lands.
            let buf = f.buffer_mut();
            for yy in area.top()..area.bottom() {
                for xx in area.left()..area.right() {
                    buf.set_string(xx, yy, "▒", ctx.theme.fog());
                }
            }
            let msg = format!(" exploring {} … ", ctx.world.meta.context);
            f.render_widget(
                Paragraph::new(Line::styled(msg, ctx.theme.dim())).centered(),
                vcenter(area),
            );
            return;
        }
        let zones = &ctx.models.map.zones;
        if zones.is_empty() {
            f.render_widget(
                Paragraph::new(Line::styled("no nodes observed", ctx.theme.dim())).centered(),
                vcenter(area),
            );
            return;
        }
        if area.width < TILE_W || area.height < TILE_H + 2 {
            f.render_widget(Paragraph::new("terminal too small"), area);
            return;
        }

        // Keep the cursor inside the viewport.
        let visible_cols = (((area.width + COL_GAP) / (TILE_W + COL_GAP)) as usize).max(1);
        let visible_rows = (((area.height - 1) / (TILE_H + ROW_GAP)) as usize).max(1);
        self.last_visible = (visible_cols, visible_rows);
        if self.cursor.0 < self.scroll_col {
            self.scroll_col = self.cursor.0;
        }
        if self.cursor.0 >= self.scroll_col + visible_cols {
            self.scroll_col = self.cursor.0 + 1 - visible_cols;
        }
        if self.cursor.1 < self.scroll_row {
            self.scroll_row = self.cursor.1;
        }
        if self.cursor.1 >= self.scroll_row + visible_rows {
            self.scroll_row = self.cursor.1 + 1 - visible_rows;
        }

        let buf = f.buffer_mut();
        let max_rows_any = zones.iter().map(|z| z.nodes.len()).max().unwrap_or(0);

        for (vi, zi) in (self.scroll_col..zones.len())
            .take(visible_cols)
            .enumerate()
        {
            let zone = &zones[zi];
            let x = area.x + vi as u16 * (TILE_W + COL_GAP);
            draw_zone_header(buf, x, area.y, zone, ctx);

            for (vr, ni) in (self.scroll_row..zone.nodes.len())
                .take(visible_rows)
                .enumerate()
            {
                let tile = &zone.nodes[ni];
                let y = area.y + 1 + vr as u16 * (TILE_H + ROW_GAP);
                // The inactive continent keeps its cursor but mutes it.
                let selected = ctx.focused && (zi, ni) == self.cursor;
                draw_tile(buf, x, y, tile, selected, ctx);
            }
        }

        // Scroll hints.
        let hint_style = ctx.theme.dim();
        if self.scroll_col > 0 {
            buf.set_string(area.x, area.y, "◂", hint_style);
        }
        if self.scroll_col + visible_cols < zones.len() {
            buf.set_string(area.x + area.width - 1, area.y, "▸", hint_style);
        }
        if self.scroll_row > 0 {
            buf.set_string(area.x + area.width - 1, area.y + 1, "▴", hint_style);
        }
        if self.scroll_row + visible_rows < max_rows_any {
            buf.set_string(
                area.x + area.width - 1,
                area.y + area.height - 1,
                "▾",
                hint_style,
            );
        }

        // Minimap: only when the board exceeds the viewport (and the
        // sidebar isn't already showing it).
        let overflows = self.scroll_col > 0
            || self.scroll_row > 0
            || visible_cols < zones.len()
            || visible_rows < max_rows_any;
        if overflows && !self.external_minimap {
            draw_minimap(
                buf,
                area,
                ctx,
                self.cursor,
                (self.scroll_col, self.scroll_row, visible_cols, visible_rows),
            );
        }
    }
}

fn vcenter(area: Rect) -> Rect {
    Rect {
        y: area.y + area.height / 2,
        height: 1,
        ..area
    }
}

fn health_rank(h: NodeHealth) -> u8 {
    match h {
        NodeHealth::Healthy => 0,
        NodeHealth::Cordoned => 1,
        NodeHealth::Pressure => 2,
        NodeHealth::NotReady => 3,
    }
}

fn worst_health<'a>(tiles: impl Iterator<Item = &'a NodeTile>) -> NodeHealth {
    tiles
        .map(|t| t.health)
        .max_by_key(|h| health_rank(*h))
        .unwrap_or(NodeHealth::Healthy)
}

/// Zone header: `─ z-a · 20 ▪3 ──────` — the ▪N rollup (colored by the
/// worst node state) says "this zone needs a look" without scrolling.
fn draw_zone_header(buf: &mut Buffer, x: u16, y: u16, zone: &ZoneColumn, ctx: &RenderCtx) {
    let theme = ctx.theme;
    let base = format!("─ {} · {} ", truncate(&zone.name, 12), zone.nodes.len());
    let mut used = base.chars().count().min(TILE_W as usize);
    buf.set_stringn(x, y, &base, TILE_W as usize, theme.zone());

    let bad = zone
        .nodes
        .iter()
        .filter(|t| t.health != NodeHealth::Healthy)
        .count();
    if bad > 0 && used + 3 < TILE_W as usize {
        let worst = worst_health(zone.nodes.iter());
        let seg = format!("▪{bad} ");
        buf.set_stringn(
            x + used as u16,
            y,
            &seg,
            TILE_W as usize - used,
            theme.node(worst),
        );
        used += seg.chars().count();
    }
    if used < TILE_W as usize {
        buf.set_string(
            x + used as u16,
            y,
            "─".repeat(TILE_W as usize - used),
            theme.zone(),
        );
    }
}

/// Corner minimap: the whole board at one character per node (or per `k`
/// nodes when a zone is taller than the panel), zone columns left to right.
/// `┌┐└┘` brackets frame the visible viewport; the reversed cell is the
/// cursor. Appears bottom-right only when the board exceeds the viewport.
fn draw_minimap(
    buf: &mut Buffer,
    map_area: Rect,
    ctx: &RenderCtx,
    cursor: (usize, usize),
    viewport: (usize, usize, usize, usize),
) {
    let theme = ctx.theme;
    let zones = &ctx.models.map.zones;
    let zcount = zones.len() as u16;
    let max_rows = zones.iter().map(|z| z.nodes.len()).max().unwrap_or(0) as u16;
    if zcount == 0 || max_rows == 0 {
        return;
    }

    // Vertical compression: k nodes per cell so tall zones still fit.
    let avail_h = map_area.height.saturating_sub(6);
    if avail_h == 0 {
        return;
    }
    let k = max_rows.div_ceil(avail_h).max(1);
    let grid_w = zcount * 2 - 1;
    let grid_h = max_rows.div_ceil(k);
    let w = grid_w + 4; // bracket margin + border
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
        .border_style(theme.chrome())
        .title(" WORLD ")
        .title_style(theme.title());
    let inner = block.inner(area);
    block.render(area, buf);
    draw_world_cells(buf, inner, ctx, cursor, viewport);
}

/// The world grid itself — dark ocean, land cells, a framed viewport.
/// Shared by the floating overlay and the sidebar WORLD panel. `inner`
/// includes a one-cell margin all around for the viewport frame.
pub(crate) fn draw_world_cells(
    buf: &mut Buffer,
    inner: Rect,
    ctx: &RenderCtx,
    cursor: (usize, usize),
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
            let worst = worst_health(chunk.iter());
            let (ch, style) = theme.land_cell(worst);
            let is_cursor = zi == cursor.0 && cursor.1 / k as usize == ci;
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
    // single-row viewport would collapse the corners onto one cell row, so
    // it borrows the margin rows above and below instead.
    let (sc, sr, vc, vr) = viewport;
    let last_col = (sc + vc).min(zones.len()).saturating_sub(1);
    let last_row = ((sr + vr).min(max_rows as usize)).saturating_sub(1);
    let x0 = origin.0 + sc as u16 * 2 - 1;
    let x1 = origin.0 + last_col as u16 * 2 + 1;
    let mut y0 = origin.1 + sr as u16 / k;
    let mut y1 = origin.1 + last_row as u16 / k;
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

fn worst_pod_state(tile: &NodeTile) -> PodState {
    let mut worst = PodState::Ok;
    for p in &tile.pods {
        worst = match (worst, p.state) {
            (_, PodState::Failing) | (PodState::Failing, _) => PodState::Failing,
            (_, PodState::Pending) | (PodState::Pending, _) => PodState::Pending,
            (_, PodState::Starting) | (PodState::Starting, _) => PodState::Starting,
            (w, _) => w,
        };
        if worst == PodState::Failing {
            break;
        }
    }
    worst
}

fn draw_tile(buf: &mut Buffer, x: u16, y: u16, tile: &NodeTile, selected: bool, ctx: &RenderCtx) {
    let theme = ctx.theme;
    let w = TILE_W as usize;
    let name_w = w - 4;

    // Line 0: terrain glyph (health-colored), white city-name label, and a
    // condition marker — the overlays repaint the label with their signal.
    let (glyph_style, name_style) = match ctx.overlay {
        OverlayMode::Pressure => (theme.node(tile.health), theme.tile_name()),
        OverlayMode::ReplicaHealth => {
            let s = theme.pod(worst_pod_state(tile));
            (s, s)
        }
        OverlayMode::Namespace => {
            let s = tile
                .dominant_ns
                .as_deref()
                .map(|ns| theme.namespace(ns))
                .unwrap_or_default();
            (s, s)
        }
    };
    let marker = if !tile.ready {
        "✗"
    } else if !tile.abnormal.is_empty() {
        "⚠"
    } else if tile.cordoned {
        "⊘"
    } else {
        " "
    };
    let name = format!("{:<name_w$.name_w$}", truncate(&tile.name, name_w));
    if selected {
        let head = format!("{} {name}{marker}", node_glyph(tile.health));
        buf.set_stringn(x, y, head, w, theme.selection());
    } else {
        buf.set_string(x, y, node_glyph(tile.health).to_string(), glyph_style);
        buf.set_stringn(x + 2, y, name, name_w, name_style);
        let marker_style = if !tile.ready {
            theme.severity(Severity::Critical)
        } else if !tile.abnormal.is_empty() {
            theme.severity(Severity::Warning)
        } else {
            theme.node(NodeHealth::Cordoned)
        };
        buf.set_string(x + 2 + name_w as u16, y, marker, marker_style);
    }

    // Lines 1-2: request-pressure gauges (food-storage green when calm).
    let gauge_w = w - 8;
    let cpu = format!(
        "c {} {:>3.0}%",
        bar(tile.cpu_ratio, gauge_w),
        (tile.cpu_ratio * 100.0).min(999.0)
    );
    buf.set_stringn(x, y + 1, cpu, w, theme.ratio(tile.cpu_ratio));
    let mem = format!(
        "m {} {:>3.0}%",
        bar(tile.mem_ratio, gauge_w),
        (tile.mem_ratio * 100.0).min(999.0)
    );
    buf.set_stringn(x, y + 2, mem, w, theme.ratio(tile.mem_ratio));

    // Line 3: pod glyphs (overlay decides their coloring) + population badge.
    let count = format!("{}p", tile.pods.len());
    let max_glyphs = w.saturating_sub(count.len() + 1);
    let mut gx = x;
    for p in tile.pods.iter().take(max_glyphs) {
        let style = match ctx.overlay {
            OverlayMode::Namespace => theme.namespace(&p.namespace),
            _ => theme.pod(p.state),
        };
        buf.set_string(gx, y + 3, pod_glyph(p.state).to_string(), style);
        gx += 1;
    }
    if tile.pods.len() > max_glyphs {
        buf.set_string(gx, y + 3, "+", theme.dim());
    }
    buf.set_string(x + (w - count.len()) as u16, y + 3, count, theme.badge());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorMode;
    use crate::state::fixtures as fx;
    use crate::state::model::Models;
    use crate::ui::theme::Theme;
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

    /// Snapshot-style: the map renders zone headers, tiles with glyphs,
    /// gauges, and pod rows for a small fixture world.
    #[test]
    fn map_renders_zones_tiles_and_pods() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n-alpha", Some("z-a")));
        s.node(fx::node("n-bravo", Some("z-b")));
        s.pod(fx::pod_requests(
            fx::pod("demo", "p1", Some("n-alpha")),
            "2",
            "4Gi",
        ));
        s.pod(fx::pod_waiting(
            fx::pod("demo", "p2", Some("n-alpha")),
            "CrashLoopBackOff",
        ));

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
        };
        let mut view = MapView::default();
        let mut term = Terminal::new(TestBackend::new(50, 12)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let text = buffer_text(&term);

        assert!(text.contains("─ z-a · 1"), "zone header missing:\n{text}");
        assert!(text.contains("─ z-b · 1"), "second zone missing:\n{text}");
        assert!(text.contains("▣ n-alpha"), "tile head missing:\n{text}");
        assert!(text.contains("▣ n-bravo"), "tile head missing:\n{text}");
        assert!(text.contains("c ▓"), "cpu gauge missing:\n{text}");
        assert!(text.contains(" 50%"), "cpu percent missing:\n{text}");
        assert!(text.contains("✗"), "failing pod glyph missing:\n{text}");
        assert!(text.contains("●"), "ok pod glyph missing:\n{text}");
        assert!(text.contains("2p"), "pod count missing:\n{text}");
        // Board fits the viewport → no world overlay.
        assert!(!text.contains("WORLD"), "minimap should be hidden:\n{text}");
    }

    #[test]
    fn map_cursor_navigation_clamps_and_opens_nodes() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n-alpha", Some("z-a")));
        s.node(fx::node("n-bravo", Some("z-b")));
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
        };
        let mut view = MapView::default();
        use ratatui_crossterm::crossterm::event::{KeyCode, KeyModifiers};
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        // Right into z-b, Enter opens that node.
        assert_eq!(view.handle_key(key(KeyCode::Char('l')), &ctx), None);
        let action = view.handle_key(key(KeyCode::Enter), &ctx);
        assert_eq!(action, Some(Action::OpenNode("n-bravo".into())));
        // Pushing past the right edge reports it (pair mode crosses over;
        // single-cluster mode ignores it). Cursor stays clamped.
        assert_eq!(
            view.handle_key(key(KeyCode::Char('l')), &ctx),
            Some(Action::EdgeReached(Edge::Right))
        );
        assert_eq!(view.cursor, (1, 0));
        // Down clamps within a 1-node column.
        assert_eq!(view.handle_key(key(KeyCode::Char('j')), &ctx), None);
        assert_eq!(view.cursor, (1, 0));
        // And the left edge, after walking back.
        assert_eq!(view.handle_key(key(KeyCode::Char('h')), &ctx), None);
        assert_eq!(
            view.handle_key(key(KeyCode::Char('h')), &ctx),
            Some(Action::EdgeReached(Edge::Left))
        );
    }

    /// 100 nodes across 5 zones, 1000 pods in 20 deployments — the shared
    /// at-scale fixture for perf and minimap tests.
    fn big_world() -> (crate::state::observed::ObservedWorld, fx::Seeds) {
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
        // One degraded node so the minimap shows a ▪ cell and the zone
        // header carries a rollup.
        s.node(fx::node_with_condition(
            fx::node("perf-node-000", Some("z-a")),
            "Ready",
            "False",
        ));
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
        };
        let mut view = MapView::default();
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        let text = buffer_text(&term);

        assert!(text.contains("WORLD"), "world panel title missing:\n{text}");

        assert!(text.contains('▪'), "degraded minimap cell missing:\n{text}");
        // Block border + viewport brackets both contribute corners.
        assert!(
            text.matches('┌').count() >= 2 && text.matches('┘').count() >= 2,
            "viewport brackets missing:\n{text}"
        );
        // Zone header rollup for the NotReady node.
        assert!(text.contains("▪1"), "zone rollup missing:\n{text}");
    }

    #[test]
    fn paging_keys_jump_by_viewport() {
        let (world, _s) = big_world();
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
        };
        let mut view = MapView::default();
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| view.render(f, f.area(), &ctx)).unwrap();
        // 40 rows → header + 7 tile rows visible.
        use ratatui_crossterm::crossterm::event::{KeyCode, KeyModifiers};
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        view.handle_key(key(KeyCode::PageDown), &ctx);
        assert_eq!(view.cursor, (0, 7));
        view.handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            &ctx,
        );
        assert_eq!(view.cursor, (0, 4));
        view.handle_key(key(KeyCode::End), &ctx);
        assert_eq!(view.cursor.0, 4);
        view.handle_key(key(KeyCode::Home), &ctx);
        assert_eq!(view.cursor.0, 0);
        view.handle_key(key(KeyCode::PageUp), &ctx);
        assert_eq!(view.cursor, (0, 0));
    }

    /// Criterion 6 evidence: at 100 nodes / 1000 pods, a full world rebuild
    /// (map + workloads + attention) plus a rendered frame must fit the
    /// 100ms input-latency budget. Asserted in release (`make perf-test`);
    /// debug builds only report the numbers.
    #[test]
    fn scale_rebuild_and_render_within_budget() {
        let (world, _s) = big_world();
        let theme = Theme::new(ColorMode::Auto);
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        let mut view = MapView::default();

        let mut worst = std::time::Duration::ZERO;
        let t_all = std::time::Instant::now();
        for i in 0..20usize {
            let t = std::time::Instant::now();
            let models = Models::build(&world);
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
            };
            // Wiggle the cursor so the scroll-clamping paths run too.
            view.cursor = (i % 5, (i * 3) % 20);
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
