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
use ratatui::widgets::Paragraph;
use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};

use super::symbols::{bar, node_glyph, pod_glyph};
use super::{Action, Component, OverlayMode, RenderCtx};
use crate::state::model::{NodeTile, PodState};
use crate::util::truncate;

pub const TILE_W: u16 = 22;
pub const TILE_H: u16 = 4;
const COL_GAP: u16 = 2;
const ROW_GAP: u16 = 1;

#[derive(Default)]
pub struct MapView {
    /// (zone column index, node index within column)
    pub cursor: (usize, usize),
    scroll_col: usize,
    scroll_row: usize,
}

impl MapView {
    pub fn selected_node(&self, ctx: &RenderCtx) -> Option<String> {
        ctx.models
            .map
            .tile(self.cursor.0, self.cursor.1)
            .map(|t| t.name.clone())
    }

    fn move_zone(&mut self, delta: isize, ctx: &RenderCtx) {
        let zones = &ctx.models.map.zones;
        if zones.is_empty() {
            return;
        }
        let z = (self.cursor.0 as isize + delta).clamp(0, zones.len() as isize - 1) as usize;
        let max_node = zones[z].nodes.len().saturating_sub(1);
        self.cursor = (z, self.cursor.1.min(max_node));
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
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.move_zone(-1, ctx),
            KeyCode::Right | KeyCode::Char('l') => self.move_zone(1, ctx),
            KeyCode::Up | KeyCode::Char('k') => self.move_node(-1, ctx),
            KeyCode::Down | KeyCode::Char('j') => self.move_node(1, ctx),
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
            let msg = format!("⟳ syncing observed world from {} …", ctx.world.meta.context);
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

            // Zone header: ─ z-a · 3 ───────────
            let label = format!("─ {} · {} ", truncate(&zone.name, 14), zone.nodes.len());
            let mut header: String = label.chars().take(TILE_W as usize).collect();
            while (header.chars().count() as u16) < TILE_W {
                header.push('─');
            }
            buf.set_string(x, area.y, header, ctx.theme.zone());

            for (vr, ni) in (self.scroll_row..zone.nodes.len())
                .take(visible_rows)
                .enumerate()
            {
                let tile = &zone.nodes[ni];
                let y = area.y + 1 + vr as u16 * (TILE_H + ROW_GAP);
                let selected = (zi, ni) == self.cursor;
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
    }
}

fn vcenter(area: Rect) -> Rect {
    Rect {
        y: area.y + area.height / 2,
        height: 1,
        ..area
    }
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

    // Line 0: glyph + name + condition marker, colored by the active overlay.
    let name_style = match ctx.overlay {
        OverlayMode::Pressure => theme.node(tile.health),
        OverlayMode::ReplicaHealth => theme.pod(worst_pod_state(tile)),
        OverlayMode::Namespace => tile
            .dominant_ns
            .as_deref()
            .map(|ns| theme.namespace(ns))
            .unwrap_or_default(),
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
    let head = format!(
        "{} {:<w$.w$}{}",
        node_glyph(tile.health),
        truncate(&tile.name, w - 4),
        marker,
        w = w - 4,
    );
    let head_style = if selected {
        theme.selection()
    } else {
        name_style
    };
    buf.set_stringn(x, y, head, w, head_style);

    // Lines 1-2: request-pressure gauges.
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

    // Line 3: pod glyphs (overlay decides their coloring) + count.
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
    buf.set_string(x + (w - count.len()) as u16, y + 3, count, theme.dim());
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
        };
        let mut view = MapView::default();
        use ratatui_crossterm::crossterm::event::{KeyCode, KeyModifiers};
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        // Right into z-b, Enter opens that node.
        assert_eq!(view.handle_key(key(KeyCode::Char('l')), &ctx), None);
        let action = view.handle_key(key(KeyCode::Enter), &ctx);
        assert_eq!(action, Some(Action::OpenNode("n-bravo".into())));
        // Clamp at the edge.
        assert_eq!(view.handle_key(key(KeyCode::Char('l')), &ctx), None);
        assert_eq!(view.cursor, (1, 0));
        // Down clamps within a 1-node column.
        assert_eq!(view.handle_key(key(KeyCode::Char('j')), &ctx), None);
        assert_eq!(view.cursor, (1, 0));
    }

    /// Criterion 6 evidence: at 100 nodes / 1000 pods, a full world rebuild
    /// (map + workloads + attention) plus a rendered frame must fit the
    /// 100ms input-latency budget. Asserted in release (`make perf-test`);
    /// debug builds only report the numbers.
    #[test]
    fn scale_rebuild_and_render_within_budget() {
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
