//! The world painter: hand-shaded terrain mosaic, beveled coasts, animated
//! sea, settlement sprites with Civ-style population boxes, islands, and
//! the minimap. All geometry comes from `k8sciv_core::state::world`.
//!
//! A paired session is a *scene* of two worlds on one sea: the warm
//! archipelago sits east of the hot one. Each world is drawn with the
//! camera shifted by its offset, so every painter stays world-local.

use k8sciv_core::events::ClusterId;
use k8sciv_core::state::attention::Severity;
use k8sciv_core::state::model::NodeHealth;
use k8sciv_core::state::pair::PairSync;
use k8sciv_core::state::world::{City, Continent, Island, Province, WorldModel};
use macroquad::prelude::*;

use crate::net::Snapshot;
use crate::sprites::{self, sprite_at, tile_region};
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use k8sciv_core::util::fnv1a64;

// World cells assume terminal-ish aspect; keep that proportion in pixels.
pub const CELL_W: f32 = 13.0;
pub const CELL_H: f32 = 19.0;
/// Ocean strait between the hot and warm archipelagos, in cells.
pub const WORLD_GAP: u16 = 8;

// --- scene ----------------------------------------------------------------

pub struct SceneWorld<'a> {
    pub id: ClusterId,
    pub off: u16,
    pub world: &'a WorldModel,
    pub label: String,
}

pub fn scene(snap: &Snapshot) -> Vec<SceneWorld<'_>> {
    let mut worlds = vec![SceneWorld {
        id: ClusterId::Hot,
        off: 0,
        world: &snap.hot.models.world,
        label: snap.hot.observed.meta.context.clone(),
    }];
    if let Some(w) = &snap.warm {
        worlds.push(SceneWorld {
            id: ClusterId::Warm,
            off: snap.hot.models.world.width + WORLD_GAP,
            world: &w.models.world,
            label: w.observed.meta.context.clone(),
        });
    }
    worlds
}

pub fn scene_size(worlds: &[SceneWorld]) -> (u16, u16) {
    let w = worlds.last().map(|s| s.off + s.world.width).unwrap_or(1);
    let h = worlds.iter().map(|s| s.world.height).max().unwrap_or(1);
    (w.max(1), h.max(1))
}

/// Which world a scene cell falls in, with the world-local cell.
pub fn locate<'a, 'b>(
    worlds: &'b [SceneWorld<'a>],
    cell: (u16, u16),
) -> Option<(&'b SceneWorld<'a>, (u16, u16))> {
    worlds
        .iter()
        .rev()
        .find(|s| cell.0 >= s.off && cell.0 < s.off + s.world.width)
        .map(|s| (s, (cell.0 - s.off, cell.1)))
}

// --- camera ----------------------------------------------------------------

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
    /// A copy whose origin is shifted east by `off` world cells — drawing a
    /// world through it lands the world at its scene offset.
    pub fn shifted(&self, off: u16) -> Camera {
        let (cw, _) = self.cell_px();
        Camera {
            pos: self.pos - vec2(off as f32 * cw, 0.0),
            zoom: self.zoom,
            target: None,
        }
    }
    pub fn cell_at(&self, screen: Vec2, bounds: (u16, u16)) -> Option<(u16, u16)> {
        let (cw, ch) = self.cell_px();
        let wx = (screen.x + self.pos.x) / cw;
        let wy = (screen.y + self.pos.y) / ch;
        (wx >= 0.0 && wy >= 0.0 && wx < bounds.0 as f32 && wy < bounds.1 as f32)
            .then_some((wx as u16, wy as u16))
    }
    /// Glide toward a scene cell over the next ~20 frames.
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
    /// Zoom and position so the whole scene is on screen.
    pub fn fit(&mut self, bounds: (u16, u16)) {
        let margin = 60.0;
        let zx = (screen_width() - margin) / (bounds.0 as f32 * CELL_W);
        let zy = (screen_height() - margin * 2.0) / (bounds.1 as f32 * CELL_H);
        self.zoom = zx.min(zy).clamp(0.30, 1.5);
        let (cw, ch) = self.cell_px();
        self.pos = vec2(
            (bounds.0 as f32 * cw - screen_width()) / 2.0,
            (bounds.1 as f32 * ch - screen_height()) / 2.0 - 10.0,
        );
        self.target = None;
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

/// Cartographic scale tiers (after Monmonier's generalization-by-scale):
/// what the map shows thins out as you zoom away. World scale generalizes
/// settlements into per-province aggregates; Regional selects which labels
/// survive; Local shows everything.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    World,
    Regional,
    Local,
}

/// Zoom-driven level of detail.
pub struct Lod {
    pub scale: Scale,
    pub province_labels: bool,
    pub name_plates: bool,
    pub structures_labels: bool,
    /// Whether the focused world is sparse enough to label every city at
    /// regional scale; dense worlds fall back to selection (troubled or
    /// populous only). Set per-world in `draw_world`.
    pub name_all: bool,
}

/// Above this many cities a world is "dense" and regional labels are
/// selected rather than shown wholesale.
const DENSE_CITIES: usize = 12;

pub fn lod(zoom: f32) -> Lod {
    let scale = if zoom >= 0.9 {
        Scale::Local
    } else if zoom >= 0.5 {
        Scale::Regional
    } else {
        Scale::World
    };
    Lod {
        scale,
        province_labels: zoom >= 0.75,
        name_plates: zoom >= 0.55,
        structures_labels: zoom >= 0.65,
        name_all: true,
    }
}

// --- label de-confliction -------------------------------------------------
//
// Monmonier's displacement operator: a label takes the first of its
// candidate positions that clears every label already placed this frame.
// Continent > province > city priority (drawn in that order), so the most
// important labels keep their preferred spot and lesser ones step aside.

const LABEL_PAD: f32 = 2.0;

fn rect_hits(a: Rect, occ: &[Rect]) -> bool {
    occ.iter().any(|o| {
        a.x < o.x + o.w + LABEL_PAD
            && a.x + a.w + LABEL_PAD > o.x
            && a.y < o.y + o.h + LABEL_PAD
            && a.y + a.h + LABEL_PAD > o.y
    })
}

/// Reserve the first candidate rect that clears placed labels (or the last
/// if all collide). Returns the chosen rect.
fn place(occ: &mut Vec<Rect>, candidates: &[Rect]) -> Rect {
    for &c in candidates {
        if !rect_hits(c, occ) {
            occ.push(c);
            return c;
        }
    }
    let last = candidates
        .last()
        .copied()
        .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
    occ.push(last);
    last
}

// --- irregular coastlines -------------------------------------------------
//
// The core world model is a clean rectangular grid (the canonical
// coordinate system both frontends share). The GUI paints organic
// landmasses over it: each continent's east/west shores are displaced
// inward by smooth value noise, and the north/south ends taper into
// rounded capes — so a zone reads as geography, not a filing cabinet.
// Deterministic (seeded by zone name) so coasts never shimmer frame to
// frame, and the displacement only insets, so model hit-testing (which
// stays rectangular) keeps landing on real provinces.

const MAX_INSET: f32 = 5.0;
const COAST_PERIOD: f32 = 5.0;

fn hash01(seed: u64, n: i64) -> f32 {
    (fnv1a64(&format!("{seed}:{n}")) % 10_000) as f32 / 10_000.0
}

/// Smooth value noise in [0,1] sampled along `t`, one control point every
/// `period` units, smoothstep-interpolated.
fn vnoise(seed: u64, t: f32, period: f32) -> f32 {
    let x = t / period;
    let i = x.floor();
    let f = x - i;
    let a = hash01(seed, i as i64);
    let b = hash01(seed, i as i64 + 1);
    let u = f * f * (3.0 - 2.0 * f);
    a + (b - a) * u
}

/// Per-continent coastline: how far the land insets from its footprint on
/// each side, for any absolute world row. The noise wobble is clamped per
/// row so the shore never carves across a city — wherever a resource sits,
/// the land bulges out to keep it firmly inland.
pub struct Coast {
    seed_l: u64,
    seed_r: u64,
    y0: i32,
    h: i32,
    /// Per-row ceiling on the west / east inset (cells), so cities stay on
    /// land. Large where no city constrains the row.
    max_l: Vec<f32>,
    max_r: Vec<f32>,
}

/// Cells of clearance kept seaward of a settlement (its building footprint
/// plus the population chip riding off the upper-left).
const CITY_MARGIN: i32 = 4;

impl Coast {
    pub fn new(cont: &Continent) -> Self {
        let h = cont
            .provinces
            .iter()
            .map(|p| p.h as i32)
            .sum::<i32>()
            .max(1);
        let w = cont.w as i32;
        let big = MAX_INSET + 100.0;
        let mut max_l = vec![big; h as usize];
        let mut max_r = vec![big; h as usize];
        // Pull the shore back around every city (and the rows its sprite +
        // name plate touch) so it can never end up in the water.
        for p in &cont.provinces {
            for c in &p.cities {
                let lx = c.x as i32 - cont.x as i32;
                let ly = c.y as i32 - cont.y as i32;
                let l_cap = (lx - CITY_MARGIN).max(0) as f32;
                let r_cap = (w - 1 - (lx + CITY_MARGIN)).max(0) as f32;
                for ry in (ly - 1)..=(ly + 2) {
                    if (0..h).contains(&ry) {
                        let i = ry as usize;
                        max_l[i] = max_l[i].min(l_cap);
                        max_r[i] = max_r[i].min(r_cap);
                    }
                }
            }
        }
        Coast {
            seed_l: fnv1a64(&format!("{}~west", cont.zone)),
            seed_r: fnv1a64(&format!("{}~east", cont.zone)),
            y0: cont.y as i32,
            h,
            max_l,
            max_r,
        }
    }

    /// (left_inset, right_inset) in cells for `abs_row`.
    fn insets(&self, abs_row: i32) -> (f32, f32) {
        let ry = (abs_row - self.y0).clamp(0, self.h - 1);
        let mut l = vnoise(self.seed_l, ry as f32, COAST_PERIOD) * MAX_INSET;
        let mut r = vnoise(self.seed_r, ry as f32, COAST_PERIOD) * MAX_INSET;
        // Round the north/south ends into capes (only for tall continents;
        // a single-node island just gets the gentle wobble).
        let cap = (self.h / 4).clamp(0, 3);
        let end = ry.min(self.h - 1 - ry);
        let taper = (cap - end.min(cap)).max(0) as f32 * 2.4;
        l += taper;
        r += taper;
        // Keep every settlement on dry land.
        l = l.min(self.max_l[ry as usize]).max(0.0);
        r = r.min(self.max_r[ry as usize]).max(0.0);
        (l, r)
    }

    /// Land span (start, width) in cells for `abs_row`, for the minimap.
    pub fn land_span(&self, abs_row: i32, w: f32) -> (f32, f32) {
        let (li, ri) = self.insets(abs_row);
        (li, (w - li - ri).max(0.0))
    }
}

/// The open sea fills the screen behind every world: world-aligned water
/// tiles so panning feels physical, darkened to sit behind the chrome.
pub fn draw_sea(cam: &Camera) {
    let drew = sprites::with(|s| {
        let tile = 64.0 * cam.zoom;
        let tint = Color::new(0.34, 0.46, 0.66, 1.0);
        let ox = -cam.pos.x.rem_euclid(tile) - tile;
        let oy = -cam.pos.y.rem_euclid(tile) - tile;
        let mut y = oy;
        while y < screen_height() + tile {
            let mut x = ox;
            while x < screen_width() + tile {
                draw_texture_ex(
                    &s.water,
                    x,
                    y,
                    tint,
                    DrawTextureParams {
                        dest_size: Some(vec2(tile, tile)),
                        ..Default::default()
                    },
                );
                x += tile;
            }
            y += tile;
        }
    });
    if drew.is_none() {
        let (cw, ch) = cam.cell_px();
        let t = get_time() as f32;
        let x0 = (cam.pos.x / cw).floor() as i32;
        let y0 = (cam.pos.y / ch).floor() as i32;
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
    }
}

/// One world, drawn through an offset camera. `banner` names the
/// archipelago in pair mode; `pair` adds sync chips to cities.
pub fn draw_world(
    world: &WorldModel,
    cam: &Camera,
    banner: Option<(&str, ClusterId)>,
    pair: Option<&PairSync>,
) {
    let mut detail = lod(cam.zoom);
    detail.name_all = world.cities().take(DENSE_CITIES + 1).count() <= DENSE_CITIES;

    if let Some((label, id)) = banner {
        let p = cam.to_screen(1.0, 0.0);
        let fs = 26.0 * cam.zoom.max(0.7);
        let tag = match id {
            ClusterId::Hot => "HOT",
            ClusterId::Warm => "WARM",
        };
        let color = match id {
            ClusterId::Hot => Color::new(0.95, 0.65, 0.35, 1.0),
            ClusterId::Warm => Color::new(0.55, 0.78, 0.92, 1.0),
        };
        text_bold(tag, p.x, p.y - fs, fs, color);
        let tm = text_size(tag, fs);
        text(
            ascii(label),
            p.x + tm.width + 10.0,
            p.y - fs,
            fs * 0.7,
            PARCHMENT,
        );
    }

    let (cw, ch) = cam.cell_px();
    let x0 = (cam.pos.x / cw).floor().max(0.0) as i32;
    let y0 = (cam.pos.y / ch).floor().max(0.0) as i32;
    let cols = (screen_width() / cw) as i32 + 2;
    let rows = (screen_height() / ch) as i32 + 2;

    // Labels placed this frame, so later (lesser) ones step around earlier
    // (more important) ones. Continent names go first.
    let mut occupied: Vec<Rect> = Vec::new();

    for cont in &world.continents {
        if detail.province_labels {
            let p = cam.to_screen(cont.x as f32 + 1.0, cont.y as f32 - 1.0);
            let label = ascii(&format!(
                "{}  ({} provinces)",
                cont.zone,
                cont.provinces.len()
            ));
            let fs = 20.0 * cam.zoom.max(0.8);
            let tm = text_size(&label, fs);
            let y = p.y + ch * 0.7;
            occupied.push(Rect::new(p.x, y - tm.height, tm.width, tm.height + 4.0));
            text(&label, p.x, y, fs, PARCHMENT);
        }
        let coast = Coast::new(cont);
        for prov in &cont.provinces {
            draw_province(
                prov,
                cam,
                &detail,
                pair,
                &coast,
                &mut occupied,
                x0,
                y0,
                cols,
                rows,
            );
        }
    }

    for isl in &world.islands {
        draw_island(isl, cam, &detail, &mut occupied);
    }
}

pub fn draw_selection(cam: &Camera, sel: (u16, u16)) {
    let (cw, ch) = cam.cell_px();
    let t = get_time() as f32;
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

#[allow(clippy::too_many_arguments)]
fn draw_province(
    prov: &Province,
    cam: &Camera,
    detail: &Lod,
    pair: Option<&PairSync>,
    coast: &Coast,
    occupied: &mut Vec<Rect>,
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

    // Terrain: tiled ground textures keyed to health (light sprites tint
    // well, so drought/wasteland are tinted sand/stone). Falls back to the
    // procedural cell mosaic without sprites.
    let ground = Rect::new(tl.x, tl.y, w, h);
    let textured = sprites::with(|s| {
        let tile = 64.0 * cam.zoom;
        match prov.tile.health {
            NodeHealth::Healthy => {
                let tex = if fnv1a64(&prov.tile.name).is_multiple_of(2) {
                    &s.grass
                } else {
                    &s.grass2
                };
                tile_region(tex, ground, WHITE, tile);
            }
            NodeHealth::Cordoned => {
                tile_region(&s.sand, ground, Color::new(0.93, 0.86, 0.52, 1.0), tile)
            }
            NodeHealth::Pressure => {
                tile_region(&s.sand, ground, Color::new(1.0, 0.64, 0.38, 1.0), tile)
            }
            NodeHealth::NotReady => {
                tile_region(&s.stone, ground, Color::new(0.95, 0.48, 0.42, 1.0), tile)
            }
        }
        // A little life on healthy land — each tree placed inside its
        // row's land span so it never lands in the carved sea. Trees are
        // background detail: dropped at world scale (Monmonier selection).
        if prov.tile.health == NodeHealth::Healthy && detail.scale != Scale::World {
            for i in 0..3u64 {
                let hx = fnv1a64(&format!("{}t{i}", prov.tile.name));
                let cy = 1 + ((hx >> 8) % (prov.h as u64 - 1).max(1)) as u16;
                let (li, lw) = coast.land_span((prov.y + cy) as i32, prov.w as f32);
                if lw < 4.0 {
                    continue;
                }
                let cx = li + 1.5 + (hx % (lw as u64 - 2).max(1)) as f32;
                let c = cam.to_screen(prov.x as f32 + cx, prov.y as f32 + cy as f32 + 0.5);
                sprite_at(&s.tree, c, 20.0 * cam.zoom, WHITE);
            }
        }
    });
    if textured.is_none() {
        let cx0 = (prov.x as i32).max(vx0);
        let cx1 = ((prov.x + prov.w) as i32).min(vx0 + vcols);
        let cy0 = (prov.y as i32).max(vy0);
        let cy1 = ((prov.y + prov.h) as i32).min(vy0 + vrows);
        for wy in cy0..cy1 {
            let (li, ri) = coast.insets(wy);
            for wx in cx0..cx1 {
                // Clip the procedural mosaic to the irregular shore.
                let rel = (wx - prov.x as i32) as f32;
                if rel < li || rel >= prov.w as f32 - ri {
                    continue;
                }
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
    }

    // Carve the rectangular fill into an organic landmass: overdraw the
    // shore margins with sea, lay a sand beach just inside the waterline,
    // and ink a thin coast outline. Sea matches draw_sea so the carved
    // cells melt into the surrounding ocean.
    let sea_tint = Color::new(0.34, 0.46, 0.66, 1.0);
    let coast_line = Color::new(0.10, 0.20, 0.34, 1.0);
    let water = |r: Rect| {
        let drew = sprites::with(|s| tile_region(&s.water, r, sea_tint, 64.0 * cam.zoom));
        if drew.is_none() {
            draw_rectangle(r.x, r.y, r.w, r.h, OCEAN);
        }
    };
    let beach_w = (0.5 * cw).max(2.0);
    let (north, south) = (coast.y0, coast.y0 + coast.h - 1);
    for wy in prov.y..prov.y + prov.h {
        if (wy as i32) < vy0 - 1 || (wy as i32) > vy0 + vrows {
            continue;
        }
        let (li, ri) = coast.insets(wy as i32);
        let row_y = cam.to_screen(prov.x as f32, wy as f32).y;
        let left = cam.to_screen(prov.x as f32, wy as f32).x;
        let right = cam.to_screen((prov.x + prov.w) as f32, wy as f32).x;
        // Shore margins → sea.
        if li > 0.02 {
            water(Rect::new(left, row_y, li * cw + 0.5, ch + 0.5));
        }
        if ri > 0.02 {
            water(Rect::new(right - ri * cw, row_y, ri * cw + 0.5, ch + 0.5));
        }
        // Beach + waterline.
        let land_l = left + li * cw;
        let land_r = right - ri * cw;
        draw_rectangle(land_l, row_y, beach_w, ch + 0.5, SAND);
        draw_rectangle(land_r - beach_w, row_y, beach_w, ch + 0.5, SAND);
        draw_rectangle(land_l - 1.5, row_y, 1.5, ch + 0.5, coast_line);
        draw_rectangle(land_r, row_y, 1.5, ch + 0.5, coast_line);
        // Capped beaches on the north/south shores.
        if wy as i32 == north || wy as i32 == south {
            let edge_y = if wy as i32 == north {
                row_y
            } else {
                row_y + ch - beach_w
            };
            draw_rectangle(land_l, edge_y, land_r - land_l, beach_w, SAND);
        }
    }

    // Daemonset roads: a worn track laid on the province's widest land
    // row, anchored to that row's shore so it never spills into the sea.
    // Roads are background detail — dropped at world scale.
    if prov.infra > 0 && detail.scale != Scale::World {
        let road_row = (prov.y..prov.y + prov.h)
            .max_by(|a, b| {
                coast
                    .land_span(*a as i32, prov.w as f32)
                    .1
                    .total_cmp(&coast.land_span(*b as i32, prov.w as f32).1)
            })
            .unwrap_or(prov.y);
        let (li, lw) = coast.land_span(road_row as i32, prov.w as f32);
        let land_left = cam.to_screen(prov.x as f32 + li, road_row as f32).x;
        let road_y = cam.to_screen(prov.x as f32, road_row as f32).y + ch - 7.0;
        let seg = 14.0 * cam.zoom;
        let fit = ((lw * cw - 8.0) / seg).floor().max(0.0) as usize;
        for i in 0..prov.infra.min(10).min(fit) {
            draw_rectangle(
                land_left + 6.0 + i as f32 * seg,
                road_y,
                10.0 * cam.zoom,
                3.0,
                ROAD,
            );
        }
    }

    if detail.province_labels {
        // Inset the label to the top row's shore so it sits on land, and
        // reserve its two-line block so settlements step around it.
        let (top_li, _) = coast.land_span(prov.y as i32, prov.w as f32);
        let label_x = tl.x + top_li * cw + 6.0;
        let fs = 16.0 * cam.zoom.max(0.7);
        let name = ascii(&prov.tile.name);
        let pods = format!("{} pods", prov.tile.pods.len());
        let nm = text_size(&name, fs);
        let block_w = nm.width.max(text_size(&pods, fs * 0.8).width);
        occupied.push(Rect::new(
            label_x - 2.0,
            tl.y + 2.0,
            block_w + 4.0,
            32.0 * cam.zoom.max(0.7),
        ));
        text(&name, label_x, tl.y + 15.0 * cam.zoom.max(0.7), fs, INK);
        text(
            &pods,
            label_x,
            tl.y + 30.0 * cam.zoom.max(0.7),
            13.0 * cam.zoom.max(0.7),
            Color::new(0.88, 0.90, 0.82, 0.75),
        );
    }

    // Settlements: individually at regional/local scale, or generalized
    // into one per-province aggregate badge at world scale.
    if detail.scale == Scale::World {
        draw_province_aggregate(prov, cam, coast);
    } else {
        for city in &prov.cities {
            draw_city(city, cam, detail, pair, occupied);
        }
    }
}

/// World-scale generalization: instead of every settlement, one badge per
/// province carrying its city count and the worst concern among them
/// (Monmonier aggregation). Placed on the province's widest land row so it
/// sits firmly inland.
fn draw_province_aggregate(prov: &Province, cam: &Camera, coast: &Coast) {
    if prov.cities.is_empty() {
        return;
    }
    let count = prov.cities.len();
    let worst = prov.cities.iter().filter_map(|c| c.severity).max();
    let row = (prov.y..prov.y + prov.h)
        .max_by(|a, b| {
            coast
                .land_span(*a as i32, prov.w as f32)
                .1
                .total_cmp(&coast.land_span(*b as i32, prov.w as f32).1)
        })
        .unwrap_or(prov.y);
    let (li, lw) = coast.land_span(row as i32, prov.w as f32);
    let center = cam.to_screen(prov.x as f32 + li + lw / 2.0, row as f32 + 0.5);
    let z = cam.zoom.max(0.55);

    // A town icon stands for "settlements here", the same house sprite the
    // local scale uses; tinted by the worst concern so trouble still reads.
    let tint = match worst {
        Some(Severity::Critical) => Color::new(1.0, 0.55, 0.5, 1.0),
        Some(Severity::Warning) => Color::new(1.0, 0.9, 0.55, 1.0),
        _ => WHITE,
    };
    let drew = sprites::with(|s| sprite_at(&s.house, center, 20.0 * z, tint));
    if drew.is_none() {
        draw_rectangle(
            center.x - 6.0 * z,
            center.y - 5.0 * z,
            12.0 * z,
            10.0 * z,
            HOUSE,
        );
    }

    // Count chip riding the upper-left, colored by the worst concern.
    let (fill, ink) = match worst {
        Some(Severity::Critical) => (CRIT, INK),
        Some(Severity::Warning) => (WARN, PLATE),
        _ => (INK, PLATE),
    };
    let label = count.to_string();
    let fs = (14.0 * z).max(11.0);
    let m = text_size(&label, fs);
    let bw = m.width + 8.0;
    let bh = fs + 4.0;
    let bx = center.x - 11.0 * z - bw;
    let by = center.y - 9.0 * z - bh;
    draw_rectangle(bx, by, bw, bh, fill);
    draw_rectangle_lines(bx, by, bw, bh, 1.0, PLATE);
    text(&label, bx + 4.0, by + bh - 4.0, fs, ink);
    if let Some(sev) = worst {
        let flag = if sev == Severity::Critical { "!!" } else { "!" };
        text_bold(flag, bx - fs * 0.7, by + bh - 4.0, fs, severity_color(sev));
    }
}

/// A settlement, Civ-style: huts that grow with population, a white pop
/// box, walls once it's a real city, a flag and tint when it needs the
/// operator — and a sync chip when a warm twin exists.
fn draw_city(
    city: &City,
    cam: &Camera,
    detail: &Lod,
    pair: Option<&PairSync>,
    occupied: &mut Vec<Rect>,
) {
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

    // Buildings: Kenney sprites by population tier, with shape fallbacks.
    let sprited = sprites::with(|s| match tier {
        0 => sprite_at(&s.house, c, 18.0 * z, Color::new(0.5, 0.5, 0.5, 0.95)),
        1 => sprite_at(&s.house, c, 22.0 * z, WHITE),
        2 => {
            sprite_at(
                &s.house2,
                vec2(c.x - 11.0 * z, c.y + 3.0 * z),
                18.0 * z,
                WHITE,
            );
            sprite_at(
                &s.house,
                vec2(c.x + 11.0 * z, c.y + 3.0 * z),
                18.0 * z,
                WHITE,
            );
            sprite_at(&s.longhouse, vec2(c.x, c.y - 5.0 * z), 22.0 * z, WHITE);
        }
        _ => {
            sprite_at(
                &s.house2,
                vec2(c.x - 13.0 * z, c.y + 4.0 * z),
                17.0 * z,
                WHITE,
            );
            sprite_at(
                &s.house,
                vec2(c.x + 13.0 * z, c.y + 4.0 * z),
                17.0 * z,
                WHITE,
            );
            sprite_at(
                &s.longhouse,
                vec2(c.x + 1.0 * z, c.y + 8.0 * z),
                16.0 * z,
                WHITE,
            );
            sprite_at(&s.keep, vec2(c.x, c.y - 6.0 * z), 28.0 * z, WHITE);
        }
    });
    if sprited.is_none() {
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

    // Population box (Civ's white number chip), de-conflicted around the
    // building so stacked settlements don't pile their chips together.
    let (box_col, num_col) = match city.severity {
        Some(Severity::Critical) => (CRIT, INK),
        Some(Severity::Warning) => (WARN, PLATE),
        _ => (INK, PLATE),
    };
    let pop = city.ready.to_string();
    let fs = (14.0 * z).max(10.0);
    let bw = text_size(&pop, fs).width + 6.0;
    let bh = fs + 2.0;
    let chip = place(
        occupied,
        &[
            Rect::new(c.x - plate_r - bw + 4.0, c.y - plate_r - bh + 2.0, bw, bh),
            Rect::new(c.x + plate_r - 4.0, c.y - plate_r - bh + 2.0, bw, bh),
            Rect::new(c.x - plate_r - bw + 4.0, c.y + plate_r - 2.0, bw, bh),
            Rect::new(c.x + plate_r - 4.0, c.y + plate_r - 2.0, bw, bh),
        ],
    );
    draw_rectangle(chip.x, chip.y, bw, bh, box_col);
    draw_rectangle_lines(chip.x, chip.y, bw, bh, 1.0, PLATE);
    text(&pop, chip.x + 3.0, chip.y + bh - 4.0, fs, num_col);

    // Sync chip beside the pop box, when a warm twin exists.
    if let Some(p) = pair
        && let Some(st) = p.state(&city.r)
    {
        let badge = ascii(&st.badge());
        let chip_w = text_size(&badge, fs).width + 6.0;
        let sr = place(
            occupied,
            &[
                Rect::new(chip.x - chip_w - 3.0, chip.y, chip_w, bh),
                Rect::new(chip.x + bw + 3.0, chip.y, chip_w, bh),
            ],
        );
        draw_rectangle(sr.x, sr.y, chip_w, bh, PLATE);
        draw_rectangle_lines(sr.x, sr.y, chip_w, bh, 1.0, sync_color(st));
        text(&badge, sr.x + 3.0, sr.y + bh - 4.0, fs, sync_color(st));
    }

    // Name plate. At regional scale only the noteworthy keep their labels
    // (Monmonier selection — troubled or populous cities), abbreviated to
    // cut clutter; at local scale every settlement is named in full. The
    // label takes the first free position (Brewer prefers the right), so
    // stacked cities fan their names out instead of colliding.
    let named = detail.name_plates
        && (detail.scale == Scale::Local
            || detail.name_all
            || city.severity.is_some()
            || city.ready >= 4);
    if named {
        let full = detail.scale == Scale::Local;
        let label = ascii(&abbrev(&city.r.name, if full { 64 } else { 11 }));
        let fs = (15.0 * z).max(11.0);
        let tm = text_size(&label, fs);
        let pw = tm.width + 8.0;
        let ph = tm.height + 5.0;
        let gap = 4.0;
        let nr = place(
            occupied,
            &[
                Rect::new(c.x + plate_r + gap, c.y - ph / 2.0, pw, ph), // right
                Rect::new(c.x + plate_r + gap, c.y - plate_r - ph, pw, ph), // upper-right
                Rect::new(c.x + plate_r + gap, c.y + plate_r, pw, ph),  // lower-right
                Rect::new(c.x - plate_r - gap - pw, c.y - ph / 2.0, pw, ph), // left
                Rect::new(c.x - pw / 2.0, c.y + plate_r + gap, pw, ph), // below
            ],
        );
        draw_rectangle(nr.x, nr.y, pw, ph, PLATE);
        text(&label, nr.x + 4.0, nr.y + ph - 5.0, fs, INK);
    }
}

/// Truncate to `max` characters with an ellipsis, on a char boundary.
fn abbrev(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let cut: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{cut}…")
    }
}

fn draw_island(isl: &Island, cam: &Camera, detail: &Lod, occupied: &mut Vec<Rect>) {
    let (cw, ch) = cam.cell_px();
    let tl = cam.to_screen(isl.x as f32, isl.y as f32);
    let w = isl.w as f32 * cw;
    let h = isl.h as f32 * ch;
    if tl.x > screen_width() || tl.y > screen_height() || tl.x + w < 0.0 || tl.y + h < 0.0 {
        return;
    }
    draw_rectangle(tl.x, tl.y, w, h, SAND);
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
        text(
            ascii(&format!("isle of {}", isl.label)),
            tl.x + 6.0,
            tl.y + 15.0,
            14.0 * cam.zoom.max(0.85),
            darker(SAND, 0.35),
        );
    }
    // World scale: generalize the isle's structures into one count badge.
    if detail.scale == Scale::World {
        let total = isl.structures.len() + isl.more;
        if total > 0 {
            let center = cam.to_screen(isl.x as f32 + isl.w as f32 / 2.0, isl.y as f32 + 1.0);
            let label = total.to_string();
            let fs = (13.0 * cam.zoom).max(11.0);
            let m = text_size(&label, fs);
            let bw = m.width + 8.0;
            let bh = fs + 4.0;
            draw_rectangle(center.x - bw / 2.0, center.y, bw, bh, STRUCT);
            draw_rectangle_lines(center.x - bw / 2.0, center.y, bw, bh, 1.0, PLATE);
            text(
                &label,
                center.x - bw / 2.0 + 4.0,
                center.y + bh - 4.0,
                fs,
                PLATE,
            );
        }
        return;
    }
    for s in &isl.structures {
        let p = cam.to_screen(isl.x as f32 + 1.5, s.y as f32 + 0.5);
        let color = if s.glyph == '✦' { STRUCT } else { DIM };
        let sprited = sprites::with(|spr| {
            if s.glyph == '✦' {
                // A gray boulder tints into a cyan-glowing resource.
                sprite_at(
                    &spr.rock,
                    p,
                    15.0 * cam.zoom,
                    Color::new(0.55, 1.0, 1.05, 1.0),
                );
            } else {
                sprite_at(&spr.tent, p, 17.0 * cam.zoom, WHITE);
            }
        });
        if sprited.is_none() {
            draw_poly(p.x, p.y, 4, 6.0 * cam.zoom, 45.0, color);
            draw_poly_lines(p.x, p.y, 4, 6.0 * cam.zoom, 45.0, 1.5, darker(color, 0.5));
        }
        if detail.structures_labels {
            let label = ascii(&format!("{}/{}", s.kind, s.name));
            let fs = 13.0 * cam.zoom.max(0.8);
            let tm = text_size(&label, fs);
            let r = place(
                occupied,
                &[
                    Rect::new(p.x + 10.0, p.y - tm.height / 2.0, tm.width, tm.height),
                    Rect::new(p.x + 10.0, p.y + 6.0, tm.width, tm.height),
                    Rect::new(p.x + 10.0, p.y - tm.height - 6.0, tm.width, tm.height),
                ],
            );
            text(&label, r.x, r.y + tm.height - 2.0, fs, darker(SAND, 0.3));
        }
    }
    if isl.more > 0 {
        let p = cam.to_screen(isl.x as f32 + 2.0, (isl.y + isl.h - 1) as f32 + 0.5);
        text(
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
    pub fn world_cell(&self, screen: Vec2, bounds: (u16, u16)) -> Option<(u16, u16)> {
        if !self.inner.contains(screen) {
            return None;
        }
        let wx = ((screen.x - self.inner.x) / self.scale_x) as u16;
        let wy = ((screen.y - self.inner.y) / self.scale_y) as u16;
        Some((wx.min(bounds.0 - 1), wy.min(bounds.1 - 1)))
    }
}

pub fn minimap_layout(bounds: (u16, u16)) -> MinimapLayout {
    let scale = (220.0 / bounds.0.max(1) as f32).min(3.0);
    let mw = bounds.0 as f32 * scale;
    let mh = (bounds.1 as f32 * scale * (CELL_H / CELL_W)).min(190.0);
    let x0 = screen_width() - mw - 14.0;
    let y0 = 44.0;
    MinimapLayout {
        frame: Rect::new(x0 - 4.0, y0 - 4.0, mw + 8.0, mh + 8.0),
        inner: Rect::new(x0, y0, mw, mh),
        scale_x: scale,
        scale_y: mh / bounds.1.max(1) as f32,
    }
}

pub fn draw_minimap(worlds: &[SceneWorld], cam: &Camera, ml: &MinimapLayout) {
    draw_rectangle(ml.frame.x, ml.frame.y, ml.frame.w, ml.frame.h, PANEL);
    draw_rectangle_lines(
        ml.frame.x, ml.frame.y, ml.frame.w, ml.frame.h, 2.0, PARCHMENT,
    );
    draw_rectangle(ml.inner.x, ml.inner.y, ml.inner.w, ml.inner.h, OCEAN);
    for sw in worlds {
        let ox = sw.off as f32 * ml.scale_x;
        for cont in &sw.world.continents {
            // Carve the same noise coastline as the main map, row by row,
            // so the chart silhouette matches the land you're exploring.
            let coast = Coast::new(cont);
            for p in &cont.provinces {
                for row in p.y..p.y + p.h {
                    let (li, lw) = coast.land_span(row as i32, p.w as f32);
                    if lw <= 0.0 {
                        continue;
                    }
                    draw_rectangle(
                        ml.inner.x + ox + (p.x as f32 + li) * ml.scale_x,
                        ml.inner.y + row as f32 * ml.scale_y,
                        lw * ml.scale_x,
                        ml.scale_y + 0.6,
                        terrain(p.tile.health),
                    );
                }
            }
        }
        for isl in &sw.world.islands {
            draw_rectangle(
                ml.inner.x + ox + isl.x as f32 * ml.scale_x,
                ml.inner.y + isl.y as f32 * ml.scale_y,
                isl.w as f32 * ml.scale_x,
                isl.h as f32 * ml.scale_y,
                SAND,
            );
        }
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
