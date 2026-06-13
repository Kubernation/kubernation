//! The world painter: hand-shaded terrain mosaic, beveled coasts, animated
//! sea, settlement sprites with Civ-style population boxes, islands, and
//! the minimap. All geometry comes from `k8sciv_core::state::world`.

use k8sciv_core::state::attention::Severity;
use k8sciv_core::state::world::{City, Island, Province, WorldModel};
use macroquad::prelude::*;

use crate::theme::*;

// World cells assume terminal-ish aspect; keep that proportion in pixels.
pub const CELL_W: f32 = 13.0;
pub const CELL_H: f32 = 19.0;

pub struct Camera {
    pub pos: Vec2,
    pub zoom: f32,
    target: Option<Vec2>,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            pos: vec2(-40.0, -30.0),
            zoom: 1.0,
            target: None,
        }
    }
    pub fn cell_px(&self) -> (f32, f32) {
        (CELL_W * self.zoom, CELL_H * self.zoom)
    }
    pub fn to_screen(&self, wx: f32, wy: f32) -> Vec2 {
        let (cw, ch) = self.cell_px();
        vec2(wx * cw - self.pos.x, wy * ch - self.pos.y)
    }
    pub fn cell_at(&self, screen: Vec2, world: &WorldModel) -> Option<(u16, u16)> {
        let (cw, ch) = self.cell_px();
        let wx = (screen.x + self.pos.x) / cw;
        let wy = (screen.y + self.pos.y) / ch;
        (wx >= 0.0 && wy >= 0.0 && wx < world.width as f32 && wy < world.height as f32)
            .then_some((wx as u16, wy as u16))
    }
    /// Glide toward a world cell over the next ~20 frames.
    pub fn fly_to(&mut self, cell: (u16, u16)) {
        let (cw, ch) = self.cell_px();
        self.target = Some(vec2(
            cell.0 as f32 * cw - screen_width() / 2.0,
            cell.1 as f32 * ch - screen_height() / 2.0,
        ));
    }
    pub fn jump_to(&mut self, cell: (u16, u16)) {
        self.fly_to(cell);
        if let Some(t) = self.target.take() {
            self.pos = t;
        }
    }
    /// Per-frame: advance the flight, cancel it on manual pan.
    pub fn tick(&mut self, manual_pan: bool) {
        if manual_pan {
            self.target = None;
            return;
        }
        if let Some(t) = self.target {
            let d = t - self.pos;
            if d.length() < 2.0 {
                self.pos = t;
                self.target = None;
            } else {
                self.pos += d * 0.18;
            }
        }
    }
}

/// Zoom-driven level of detail.
pub struct Lod {
    pub province_labels: bool,
    pub name_plates: bool,
    pub structures_labels: bool,
}

pub fn lod(zoom: f32) -> Lod {
    Lod {
        province_labels: zoom >= 0.75,
        name_plates: zoom >= 0.55,
        structures_labels: zoom >= 0.65,
    }
}

pub fn draw_world(world: &WorldModel, cam: &Camera, selected: Option<(u16, u16)>) {
    let (cw, ch) = cam.cell_px();
    let t = get_time() as f32;
    let detail = lod(cam.zoom);

    // --- open sea: drifting wave dashes -------------------------------
    let x0 = (cam.pos.x / cw).floor().max(0.0) as i32;
    let y0 = (cam.pos.y / ch).floor().max(0.0) as i32;
    let cols = (screen_width() / cw) as i32 + 2;
    let rows = (screen_height() / ch) as i32 + 2;
    for wy in y0..y0 + rows {
        for wx in x0..x0 + cols {
            if (wx * 7 + wy * 13).rem_euclid(29) == 0 {
                let drift = (t * 0.7 + wy as f32 * 0.6).sin() * 3.0 * cam.zoom;
                let p = cam.to_screen(wx as f32, wy as f32);
                draw_rectangle(p.x + drift, p.y + ch * 0.45, cw * 0.55, 2.0, WAVE);
            }
        }
    }

    for cont in &world.continents {
        if detail.province_labels {
            let p = cam.to_screen(cont.x as f32 + 1.0, cont.y as f32 - 1.0);
            draw_text(
                ascii(&format!(
                    "{}  ({} provinces)",
                    cont.zone,
                    cont.provinces.len()
                )),
                p.x,
                p.y + ch * 0.7,
                20.0 * cam.zoom.max(0.8),
                PARCHMENT,
            );
        }
        for prov in &cont.provinces {
            draw_province(prov, cam, &detail, x0, y0, cols, rows);
        }
    }

    for isl in &world.islands {
        draw_island(isl, cam, &detail);
    }

    // --- selection ring -------------------------------------------------
    if let Some(sel) = selected {
        let p = cam.to_screen(sel.0 as f32, sel.1 as f32);
        let pulse = 2.0 + (t * 5.0).sin() * 1.2;
        draw_rectangle_lines(
            p.x - pulse,
            p.y - pulse,
            cw + pulse * 2.0,
            ch + pulse * 2.0,
            2.5,
            INK,
        );
    }
}

fn draw_province(
    prov: &Province,
    cam: &Camera,
    detail: &Lod,
    vx0: i32,
    vy0: i32,
    vcols: i32,
    vrows: i32,
) {
    let (cw, ch) = cam.cell_px();
    let tl = cam.to_screen(prov.x as f32, prov.y as f32);
    let w = prov.w as f32 * cw;
    let h = prov.h as f32 * ch;
    if tl.x > screen_width() || tl.y > screen_height() || tl.x + w < 0.0 || tl.y + h < 0.0 {
        return;
    }

    // Terrain mosaic: per-cell shade variation, clipped to the viewport.
    let cx0 = (prov.x as i32).max(vx0);
    let cx1 = ((prov.x + prov.w) as i32).min(vx0 + vcols);
    let cy0 = (prov.y as i32).max(vy0);
    let cy1 = ((prov.y + prov.h) as i32).min(vy0 + vrows);
    for wy in cy0..cy1 {
        for wx in cx0..cx1 {
            let p = cam.to_screen(wx as f32, wy as f32);
            draw_rectangle(
                p.x,
                p.y,
                cw + 0.5,
                ch + 0.5,
                terrain_cell(prov.tile.health, wx as u16, wy as u16),
            );
        }
    }

    // Coast bevel: sunlit north-west shore, shaded south-east cliff.
    let base = terrain(prov.tile.health);
    draw_rectangle(tl.x, tl.y, w, 2.5, lighter(base, 1.45));
    draw_rectangle(tl.x, tl.y, 2.5, h, lighter(base, 1.3));
    draw_rectangle(tl.x, tl.y + h - 2.5, w, 2.5, darker(base, 0.55));
    draw_rectangle(tl.x + w - 2.5, tl.y, 2.5, h, darker(base, 0.6));

    // Daemonset roads: a worn track along the southern edge.
    for i in 0..prov.infra.min(10) {
        draw_rectangle(
            tl.x + 8.0 + i as f32 * 14.0 * cam.zoom,
            tl.y + h - 7.0,
            10.0 * cam.zoom,
            3.0,
            ROAD,
        );
    }

    if detail.province_labels {
        draw_text(
            ascii(&prov.tile.name),
            tl.x + 7.0,
            tl.y + 15.0 * cam.zoom.max(0.7),
            16.0 * cam.zoom.max(0.7),
            INK,
        );
        draw_text(
            format!("{} pods", prov.tile.pods.len()),
            tl.x + 7.0,
            tl.y + 30.0 * cam.zoom.max(0.7),
            13.0 * cam.zoom.max(0.7),
            Color::new(0.88, 0.90, 0.82, 0.75),
        );
    }

    for city in &prov.cities {
        draw_city(city, cam, detail);
    }
}

/// A settlement, Civ-style: huts that grow with population, a white pop
/// box, walls once it's a real city, a flag and tint when it needs the
/// operator. The circle-click affordance survives underneath.
fn draw_city(city: &City, cam: &Camera, detail: &Lod) {
    let z = cam.zoom;
    let c = cam.to_screen(city.x as f32 + 0.5, city.y as f32 + 0.8);
    let tier = match city.ready {
        0 => 0,
        1..=3 => 1,
        4..=9 => 2,
        _ => 3,
    };

    // Ground plate.
    let plate_r = (14.0 + tier as f32 * 4.0) * z;
    draw_circle(c.x, c.y, plate_r, Color::new(0.0, 0.0, 0.0, 0.18));

    // Walls for large cities.
    if tier == 3 {
        draw_circle_lines(c.x, c.y, plate_r + 2.0, 2.5 * z, lighter(SAND_DARK, 1.1));
    }

    // Huts.
    let hut = |x: f32, y: f32, s: f32| {
        let hw = 11.0 * z * s;
        let hh = 7.5 * z * s;
        draw_rectangle(x - hw / 2.0, y - hh / 2.0, hw, hh, HOUSE);
        draw_triangle(
            vec2(x - hw / 2.0 - 1.5 * z, y - hh / 2.0),
            vec2(x + hw / 2.0 + 1.5 * z, y - hh / 2.0),
            vec2(x, y - hh / 2.0 - 6.0 * z * s),
            ROOF,
        );
    };
    match tier {
        0 => {
            // An emptied settlement: a single grey ruin.
            let hw = 10.0 * z;
            draw_rectangle(c.x - hw / 2.0, c.y - 4.0 * z, hw, 7.0 * z, DIM);
        }
        1 => hut(c.x, c.y, 1.0),
        2 => {
            hut(c.x - 8.0 * z, c.y + 2.0 * z, 0.9);
            hut(c.x + 8.0 * z, c.y + 2.0 * z, 0.9);
            hut(c.x, c.y - 5.0 * z, 1.0);
        }
        _ => {
            hut(c.x - 10.0 * z, c.y + 3.0 * z, 0.9);
            hut(c.x + 10.0 * z, c.y + 3.0 * z, 0.9);
            hut(c.x, c.y - 6.0 * z, 1.1);
            hut(c.x, c.y + 5.0 * z, 0.8);
        }
    }

    // Attention: tint + a waving banner.
    if let Some(sev) = city.severity {
        let col = severity_color(sev);
        draw_circle(c.x, c.y, plate_r, Color::new(col.r, col.g, col.b, 0.22));
        let t = get_time() as f32;
        let wave = (t * 6.0).sin() * 2.0 * z;
        let fx = c.x + plate_r * 0.7;
        let fy = c.y - plate_r - 8.0 * z;
        draw_line(fx, fy, fx, fy + 12.0 * z, 1.5, darker(INK, 0.6));
        draw_triangle(
            vec2(fx, fy),
            vec2(fx + 9.0 * z + wave, fy + 3.0 * z),
            vec2(fx, fy + 6.0 * z),
            col,
        );
    }

    // Population box (Civ's white number chip).
    let (box_col, num_col) = match city.severity {
        Some(Severity::Critical) => (CRIT, INK),
        Some(Severity::Warning) => (WARN, PLATE),
        _ => (INK, PLATE),
    };
    let pop = city.ready.to_string();
    let fs = (14.0 * z).max(10.0);
    let m = measure_text(&pop, None, fs as u16, 1.0);
    let bw = m.width + 6.0;
    let bh = fs + 2.0;
    let bx = c.x - plate_r - bw + 4.0;
    let by = c.y - plate_r - bh + 2.0;
    draw_rectangle(bx, by, bw, bh, box_col);
    draw_rectangle_lines(bx, by, bw, bh, 1.0, PLATE);
    draw_text(&pop, bx + 3.0, by + bh - 4.0, fs, num_col);

    // Name plate.
    if detail.name_plates {
        let label = ascii(&city.r.name);
        let fs = (15.0 * z).max(11.0);
        let tm = measure_text(&label, None, fs as u16, 1.0);
        let lx = c.x - tm.width / 2.0;
        let ly = c.y + plate_r + fs * 0.95;
        draw_rectangle(
            lx - 4.0,
            ly - tm.height,
            tm.width + 8.0,
            tm.height + 5.0,
            PLATE,
        );
        draw_text(&label, lx, ly, fs, INK);
    }
}

fn draw_island(isl: &Island, cam: &Camera, detail: &Lod) {
    let (cw, ch) = cam.cell_px();
    let tl = cam.to_screen(isl.x as f32, isl.y as f32);
    let w = isl.w as f32 * cw;
    let h = isl.h as f32 * ch;
    if tl.x > screen_width() || tl.y > screen_height() || tl.x + w < 0.0 || tl.y + h < 0.0 {
        return;
    }
    draw_rectangle(tl.x, tl.y, w, h, SAND);
    // Stippled dunes.
    for wy in isl.y..isl.y + isl.h {
        for wx in isl.x..isl.x + isl.w {
            if (wx as u32 * 13 + wy as u32 * 7).is_multiple_of(5) {
                let p = cam.to_screen(wx as f32 + 0.3, wy as f32 + 0.4);
                draw_rectangle(p.x, p.y, 3.0 * cam.zoom, 2.0 * cam.zoom, SAND_DARK);
            }
        }
    }
    draw_rectangle(tl.x, tl.y, w, 2.0, lighter(SAND, 1.18));
    draw_rectangle(tl.x, tl.y + h - 2.0, w, 2.0, darker(SAND, 0.62));

    if detail.structures_labels {
        draw_text(
            ascii(&format!("isle of {}", isl.label)),
            tl.x + 6.0,
            tl.y + 15.0,
            14.0 * cam.zoom.max(0.85),
            darker(SAND, 0.35),
        );
    }
    for s in &isl.structures {
        let p = cam.to_screen(isl.x as f32 + 1.5, s.y as f32 + 0.5);
        let color = if s.glyph == '✦' { STRUCT } else { DIM };
        draw_poly(p.x, p.y, 4, 6.0 * cam.zoom, 45.0, color);
        draw_poly_lines(p.x, p.y, 4, 6.0 * cam.zoom, 45.0, 1.5, darker(color, 0.5));
        if detail.structures_labels {
            draw_text(
                ascii(&format!("{}/{}", s.kind, s.name)),
                p.x + 11.0,
                p.y + 5.0,
                13.0 * cam.zoom.max(0.8),
                darker(SAND, 0.3),
            );
        }
    }
    if isl.more > 0 {
        let p = cam.to_screen(isl.x as f32 + 2.0, (isl.y + isl.h - 1) as f32 + 0.5);
        draw_text(
            format!("+{} more", isl.more),
            p.x,
            p.y,
            12.0,
            darker(SAND, 0.35),
        );
    }
}

// --- minimap -----------------------------------------------------------

pub struct MinimapLayout {
    pub frame: Rect,
    pub inner: Rect,
    scale_x: f32,
    scale_y: f32,
}

impl MinimapLayout {
    pub fn world_cell(&self, screen: Vec2, world: &WorldModel) -> Option<(u16, u16)> {
        if !self.inner.contains(screen) {
            return None;
        }
        let wx = ((screen.x - self.inner.x) / self.scale_x) as u16;
        let wy = ((screen.y - self.inner.y) / self.scale_y) as u16;
        Some((wx.min(world.width - 1), wy.min(world.height - 1)))
    }
}

pub fn minimap_layout(world: &WorldModel) -> MinimapLayout {
    let scale = 150.0 / world.width.max(1) as f32;
    let mw = world.width as f32 * scale;
    let mh = (world.height as f32 * scale * (CELL_H / CELL_W)).min(190.0);
    let x0 = screen_width() - mw - 14.0;
    let y0 = 44.0;
    MinimapLayout {
        frame: Rect::new(x0 - 4.0, y0 - 4.0, mw + 8.0, mh + 8.0),
        inner: Rect::new(x0, y0, mw, mh),
        scale_x: scale,
        scale_y: mh / world.height.max(1) as f32,
    }
}

pub fn draw_minimap(world: &WorldModel, cam: &Camera, ml: &MinimapLayout) {
    draw_rectangle(ml.frame.x, ml.frame.y, ml.frame.w, ml.frame.h, PANEL);
    draw_rectangle_lines(
        ml.frame.x, ml.frame.y, ml.frame.w, ml.frame.h, 2.0, PARCHMENT,
    );
    draw_rectangle(ml.inner.x, ml.inner.y, ml.inner.w, ml.inner.h, OCEAN);
    for cont in &world.continents {
        for p in &cont.provinces {
            draw_rectangle(
                ml.inner.x + p.x as f32 * ml.scale_x,
                ml.inner.y + p.y as f32 * ml.scale_y,
                p.w as f32 * ml.scale_x,
                p.h as f32 * ml.scale_y,
                terrain(p.tile.health),
            );
        }
    }
    for isl in &world.islands {
        draw_rectangle(
            ml.inner.x + isl.x as f32 * ml.scale_x,
            ml.inner.y + isl.y as f32 * ml.scale_y,
            isl.w as f32 * ml.scale_x,
            isl.h as f32 * ml.scale_y,
            SAND,
        );
    }
    let (cw, ch) = cam.cell_px();
    let vx = (cam.pos.x / cw).max(0.0) * ml.scale_x;
    let vy = (cam.pos.y / ch).max(0.0) * ml.scale_y;
    let vw = (screen_width() / cw) * ml.scale_x;
    let vh = (screen_height() / ch) * ml.scale_y;
    draw_rectangle_lines(
        ml.inner.x + vx,
        ml.inner.y + vy,
        vw.min(ml.inner.w),
        vh.min(ml.inner.h),
        2.0,
        INK,
    );
}
