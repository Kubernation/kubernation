//! The world painter: an isometric (2:1 diamond) projection of the
//! rectangular world grid, all original procedural geometry — dithered
//! terrain diamonds, inked shorelines, procedural settlements with classic-4X
//! population boxes + serif name banners, namespace islands, and the
//! (top-down) minimap. All geometry comes from `kubernation_core::state::world`.
//!
//! Rendering is a back-to-front two-pass painter's algorithm (all terrain,
//! then settlements/labels) so south-east tiles and tall buildings overlap
//! correctly. A paired session is a *scene* of two worlds on one sea: the warm
//! archipelago sits south-east of the hot one. Each world is drawn with the
//! camera shifted by its offset, so every painter stays world-local.

use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::NodeHealth;
use kubernation_core::state::pair::PairSync;
use kubernation_core::state::world::{City, CoastKind, Continent, Island, Province, WorldModel};
use macroquad::prelude::*;

use crate::net::Snapshot;
use crate::text::{name_text, name_text_size, text, text_bold, text_outline, text_size};
use crate::theme::*;
use kubernation_core::util::fnv1a64;

// The world map is an isometric 2:1 diamond grid (classic 4X). A tile is
// TILE_W wide and TILE_H tall at zoom 1.0. Integer cell coords land on a
// diamond's NORTH vertex; `to_screen(x + 0.5, y + 0.5)` is the cell CENTER —
// so every existing painter that already passed fractional `+0.5` offsets
// keeps landing on the tile center unchanged.
pub const TILE_W: f32 = 32.0;
pub const TILE_H: f32 = 16.0;
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
            pos: vec2(-300.0, -80.0),
            zoom: 1.0,
            target: None,
        }
    }
    /// Tile diamond HALF-extents in screen pixels: (half_width, half_height).
    /// Under iso a cell is a diamond, not an axis-aligned box, so this is the
    /// primitive the forward/inverse transforms and the selection diamond all
    /// build on — not "the screen size of one cell".
    pub fn cell_px(&self) -> (f32, f32) {
        (TILE_W * self.zoom * 0.5, TILE_H * self.zoom * 0.5)
    }
    /// World cell (wx, wy) → screen point. Iso 2:1 projection: world-x grows
    /// down-right, world-y grows down-left. Integer coords hit the diamond's
    /// north vertex; pass `+0.5, +0.5` for the cell center.
    pub fn to_screen(&self, wx: f32, wy: f32) -> Vec2 {
        let (hw, hh) = self.cell_px();
        vec2((wx - wy) * hw - self.pos.x, (wx + wy) * hh - self.pos.y)
    }
    /// A copy whose origin is shifted east by `off` world cells — drawing a
    /// world through it lands it at its scene offset. East-by-`off` in iso is
    /// the down-right diagonal `off·(hw, hh)`, baked into `pos` (subtracted in
    /// `to_screen`, so we subtract it here, mirroring the old `-off·cw`).
    pub fn shifted(&self, off: u16) -> Camera {
        let (hw, hh) = self.cell_px();
        let d = off as f32;
        Camera {
            pos: self.pos - vec2(d * hw, d * hh),
            zoom: self.zoom,
            target: None,
        }
    }
    /// Screen point → world cell. Invert the iso projection, then floor: with
    /// the "integer = north vertex / center = +0.5" convention, the diamond
    /// that owns a pixel is `floor` of the solved continuous coords.
    pub fn cell_at(&self, screen: Vec2, bounds: (u16, u16)) -> Option<(u16, u16)> {
        let (hw, hh) = self.cell_px();
        let a = (screen.x + self.pos.x) / hw; // = wx - wy
        let b = (screen.y + self.pos.y) / hh; // = wx + wy
        let wx = (a + b) * 0.5;
        let wy = (b - a) * 0.5;
        (wx >= 0.0 && wy >= 0.0 && wx < bounds.0 as f32 && wy < bounds.1 as f32)
            .then_some((wx as u16, wy as u16))
    }
    /// Glide so `cell`'s diamond center sits at the screen middle.
    pub fn fly_to(&mut self, cell: (u16, u16)) {
        let (hw, hh) = self.cell_px();
        let (cx, cy) = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
        let proj = vec2((cx - cy) * hw, (cx + cy) * hh); // pre-`pos`
        self.target = Some(proj - vec2(screen_width() / 2.0, screen_height() / 2.0));
    }
    pub fn jump_to(&mut self, cell: (u16, u16)) {
        self.fly_to(cell);
        if let Some(t) = self.target.take() {
            self.pos = t;
        }
    }
    /// Zoom and position so the whole iso scene is on screen. The scene of a
    /// (W,H) grid projects to a big diamond whose screen AABB is (W+H)·hw wide
    /// by (W+H)·hh tall; fit that, then center on the projected centroid.
    pub fn fit(&mut self, bounds: (u16, u16)) {
        let (w, h) = (bounds.0 as f32, bounds.1 as f32);
        let span = w + h;
        let margin = 60.0;
        let scene_w = span * (TILE_W * 0.5);
        let scene_h = span * (TILE_H * 0.5);
        let zx = (screen_width() - margin) / scene_w.max(1.0);
        let zy = (screen_height() - margin * 2.0) / scene_h.max(1.0);
        self.zoom = zx.min(zy).clamp(0.30, 2.0);
        let (hw, hh) = self.cell_px();
        // AABB centroid in pre-`pos` projected space: x in [-h·hw, w·hw],
        // y in [0, (w+h)·hh].
        let center = vec2((w - h) * 0.5 * hw, (w + h) * 0.5 * hh);
        self.pos = center - vec2(screen_width() * 0.5, screen_height() * 0.5 - 10.0);
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

// --- isometric diamond primitives -----------------------------------------

/// The four screen corners (N, E, S, W) of the diamond whose CENTER is `c`.
fn diamond_pts(c: Vec2, hw: f32, hh: f32) -> [Vec2; 4] {
    [
        vec2(c.x, c.y - hh),
        vec2(c.x + hw, c.y),
        vec2(c.x, c.y + hh),
        vec2(c.x - hw, c.y),
    ]
}

/// Fill an iso diamond (two triangles) centered at `c`.
fn fill_diamond(c: Vec2, hw: f32, hh: f32, fill: Color) {
    let p = diamond_pts(c, hw, hh);
    draw_triangle(p[0], p[1], p[2], fill);
    draw_triangle(p[0], p[2], p[3], fill);
}

/// Stroke an iso diamond's four edges.
fn stroke_diamond(c: Vec2, hw: f32, hh: f32, th: f32, col: Color) {
    let p = diamond_pts(c, hw, hh);
    draw_line(p[0].x, p[0].y, p[1].x, p[1].y, th, col);
    draw_line(p[1].x, p[1].y, p[2].x, p[2].y, th, col);
    draw_line(p[2].x, p[2].y, p[3].x, p[3].y, th, col);
    draw_line(p[3].x, p[3].y, p[0].x, p[0].y, th, col);
}

/// Map labels stay near a constant screen size: they shrink a little when
/// zoomed out but never balloon when zoomed in (the classic-map convention —
/// the world scales, the lettering doesn't).
fn label_scale(zoom: f32) -> f32 {
    zoom.clamp(0.85, 1.2)
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

/// The open sea behind every world: a flat iso-ocean wash, a soft mottle of
/// overlapping faint swell patches (no grid), and sparse drifting wave specks.
/// Cost is O(screen pixels), not O(world cells) — we never emit a water
/// diamond for empty ocean.
pub fn draw_sea(cam: &Camera) {
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), ISO_OCEAN);
    // Soft swell patches instead of a hard checker: faint dark circles on a
    // world-anchored, jittered lattice. Heavily overlapping low-alpha circles
    // blend into a smooth mottle, and being world-anchored they drift
    // naturally as you pan.
    let spacing = 116.0;
    let i0 = (cam.pos.x / spacing).floor() as i32 - 1;
    let j0 = (cam.pos.y / spacing).floor() as i32 - 1;
    let cols = (screen_width() / spacing) as i32 + 3;
    let rows = (screen_height() / spacing) as i32 + 3;
    let patch = Color::new(ISO_OCEAN_DEEP.r, ISO_OCEAN_DEEP.g, ISO_OCEAN_DEEP.b, 0.16);
    for j in j0..j0 + rows {
        for i in i0..i0 + cols {
            let key = i as i64 * 7919 + j as i64;
            let h = hash01(0x5EA, key);
            let h2 = hash01(0x5EA7, key);
            let cx = i as f32 * spacing - cam.pos.x + (h - 0.5) * spacing;
            let cy = j as f32 * spacing - cam.pos.y + (h2 - 0.5) * spacing;
            draw_circle(cx, cy, spacing * (0.45 + h * 0.5), patch);
        }
    }
    // Gentle drifting wave specks for a little life.
    let t = get_time() as f32;
    let step = 104.0;
    let oy = (-cam.pos.y).rem_euclid(step);
    let mut y = oy - step;
    while y < screen_height() + step {
        let drift = (t * 0.5 + y * 0.01).sin() * 5.0;
        let mut x = (y * 0.13).rem_euclid(step) - step;
        while x < screen_width() + step {
            draw_circle(x + drift, y, 1.6, WAVE);
            x += step;
        }
        y += step;
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

    // Iso compositing needs a painter's-algorithm pass: nearer (more
    // south-east, larger wx+wy) things must overdraw farther ones, and tall
    // building sprites can overlap the diamond behind them. So: PASS 1 paints
    // every terrain diamond (continents + islands back-to-front), then PASS 2
    // paints features + settlements + labels on top. Coasts are cached so
    // `Coast::new` runs once per continent across both passes.
    let mut order: Vec<usize> = (0..world.continents.len()).collect();
    order.sort_by_key(|&i| world.continents[i].x as i32 + world.continents[i].y as i32);
    let coasts: Vec<Coast> = world.continents.iter().map(Coast::new).collect();
    let mut isl_order: Vec<usize> = (0..world.islands.len()).collect();
    isl_order.sort_by_key(|&i| world.islands[i].x as i32 + world.islands[i].y as i32);

    // Pass 1 — terrain. Shallows rings for a whole continent go down BEFORE
    // any of its land so the soft coastal band shows in the sea but is covered
    // on land (including the seams between stacked province bands).
    for &ci in &order {
        let cont = &world.continents[ci];
        for prov in &cont.provinces {
            draw_province_shallows(prov, cam, &coasts[ci]);
        }
        for prov in &cont.provinces {
            draw_province_terrain(prov, cam, &coasts[ci]);
        }
    }
    for &ii in &isl_order {
        draw_island_terrain(&world.islands[ii], cam);
    }

    // Pass 2 — features, settlements, labels. Labels placed this frame are
    // tracked so later (lesser) ones step around earlier (more important)
    // ones: continent → province → city.
    let mut occupied: Vec<Rect> = Vec::new();
    for &ci in &order {
        let cont = &world.continents[ci];
        let coast = &coasts[ci];
        if detail.province_labels {
            // Anchor the continent name above its north tip, but keep it fully
            // on-screen (zoomed in, the tip can sit far off the top edge).
            let tip = cam.to_screen(cont.x as f32, cont.y as f32);
            let label = ascii(&format!(
                "{}  ({} provinces)",
                cont.zone,
                cont.provinces.len()
            ));
            let fs = 18.0 * label_scale(cam.zoom);
            let tm = text_size(&label, fs);
            let lx =
                (tip.x - tm.width * 0.5).clamp(8.0, (screen_width() - tm.width - 8.0).max(8.0));
            let ly = (tip.y - 8.0).max(42.0 + fs * 0.5);
            occupied.push(Rect::new(lx, ly - tm.height, tm.width, tm.height + 4.0));
            text_outline(&label, lx, ly, fs, PARCHMENT, HALO);
        }
        for prov in &cont.provinces {
            draw_province_features(prov, cam, &detail, coast, &mut occupied);
        }
        draw_coast(cont, cam, &detail);
        // Settlements: one aggregate badge per province at world scale, else
        // the towns themselves, drawn south-east last so they overlap right.
        if detail.scale == Scale::World {
            for prov in &cont.provinces {
                draw_province_aggregate(prov, cam, coast);
            }
        } else {
            let mut cities: Vec<&City> = cont.provinces.iter().flat_map(|p| &p.cities).collect();
            cities.sort_by_key(|c| c.x as i32 + c.y as i32);
            for city in cities {
                draw_city(city, cam, &detail, pair, &mut occupied);
            }
        }
    }

    for &ii in &isl_order {
        draw_island_features(&world.islands[ii], cam, &detail, &mut occupied);
    }
}

pub fn draw_selection(cam: &Camera, sel: (u16, u16)) {
    let (hw, hh) = cam.cell_px();
    let t = get_time() as f32;
    let c = cam.to_screen(sel.0 as f32 + 0.5, sel.1 as f32 + 0.5); // diamond center
    let pulse = 1.0 + (t * 5.0).sin() * 0.12;
    stroke_diamond(c, hw * pulse, hh * pulse, 2.5, INK);
}

/// Cheap 4-corner screen-AABB cull for a province footprint; true = offscreen.
fn province_offscreen(prov: &Province, cam: &Camera) -> bool {
    let corners = [
        cam.to_screen(prov.x as f32, prov.y as f32),
        cam.to_screen((prov.x + prov.w) as f32, prov.y as f32),
        cam.to_screen(prov.x as f32, (prov.y + prov.h) as f32),
        cam.to_screen((prov.x + prov.w) as f32, (prov.y + prov.h) as f32),
    ];
    let minx = corners.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    let maxx = corners.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let miny = corners.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let maxy = corners.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    maxx < -TILE_W
        || minx > screen_width() + TILE_W
        || maxy < -TILE_H
        || miny > screen_height() + TILE_H
}

/// Shallows ring (PASS 1, before any land): two graded faint-blue diamonds,
/// oversized so they poke into the sea, drawn under each SHORELINE land cell.
/// Interior land is skipped, and the land pass paints over any that bled
/// inward — so a soft deep→shallow→beach band rings the whole coast without a
/// hard diamond edge. Must run before the continent's land (see `draw_world`).
fn draw_province_shallows(prov: &Province, cam: &Camera, coast: &Coast) {
    if province_offscreen(prov, cam) {
        return;
    }
    let (hw, hh) = cam.cell_px();
    let x0 = prov.x as i32;
    let w = prov.w as f32;
    let y1 = (prov.y + prov.h) as i32;
    for wy in prov.y as i32..y1 {
        let (li, ri) = coast.insets(wy);
        let up_in = wy > coast.y0;
        let dn_in = wy + 1 < coast.y0 + coast.h;
        let (li_up, ri_up) = if up_in {
            coast.insets(wy - 1)
        } else {
            (f32::MAX, f32::MAX)
        };
        let (li_dn, ri_dn) = if dn_in {
            coast.insets(wy + 1)
        } else {
            (f32::MAX, f32::MAX)
        };
        for wx in x0..(prov.x + prov.w) as i32 {
            let rel = (wx - x0) as f32;
            if rel < li || rel >= w - ri {
                continue; // sea cell
            }
            let shore = rel - 1.0 < li
                || rel + 1.0 >= w - ri
                || !up_in
                || rel < li_up
                || rel >= w - ri_up
                || !dn_in
                || rel < li_dn
                || rel >= w - ri_dn;
            if !shore {
                continue; // interior land — no shallows needed
            }
            let c = cam.to_screen(wx as f32 + 0.5, wy as f32 + 0.5);
            if c.x < -TILE_W * 2.0
                || c.x > screen_width() + TILE_W * 2.0
                || c.y < -TILE_H * 2.0
                || c.y > screen_height() + TILE_H * 2.0
            {
                continue;
            }
            fill_diamond(c, hw * 1.75, hh * 1.75, SHALLOWS_DEEP);
            fill_diamond(c, hw * 1.38, hh * 1.38, SHALLOWS);
        }
    }
}

/// One province painted as iso terrain (PASS 1): a health-tinted, dithered
/// diamond per LAND cell, with sea-facing shoreline cells drawn as sand. Sea
/// cells emit nothing — the ocean (and the shallows ring drawn just before)
/// show through. Land/sea is the same per-row `Coast` inset the rectangular
/// map used; the continent's vertical extent (`coast.y0`/`coast.h`) marks the
/// north/south shore so inter-province band seams stay interior land.
fn draw_province_terrain(prov: &Province, cam: &Camera, coast: &Coast) {
    if province_offscreen(prov, cam) {
        return;
    }
    let (hw, hh) = cam.cell_px();
    let x0 = prov.x as i32;
    let w = prov.w as f32;
    let y1 = (prov.y + prov.h) as i32;
    for wy in prov.y as i32..y1 {
        let (li, ri) = coast.insets(wy);
        // Per-row neighbour insets (cheap vs. per-cell): a cell is shoreline
        // if its N/S neighbour row is outside the continent or sea there.
        let up_in = wy > coast.y0;
        let dn_in = wy + 1 < coast.y0 + coast.h;
        let (li_up, ri_up) = if up_in {
            coast.insets(wy - 1)
        } else {
            (f32::MAX, f32::MAX)
        };
        let (li_dn, ri_dn) = if dn_in {
            coast.insets(wy + 1)
        } else {
            (f32::MAX, f32::MAX)
        };
        for wx in x0..(prov.x + prov.w) as i32 {
            let rel = (wx - x0) as f32;
            if rel < li || rel >= w - ri {
                continue; // sea cell — ocean shows through
            }
            let c = cam.to_screen(wx as f32 + 0.5, wy as f32 + 0.5);
            if c.x < -TILE_W
                || c.x > screen_width() + TILE_W
                || c.y < -TILE_H
                || c.y > screen_height() + TILE_H
            {
                continue;
            }
            // Sea-facing neighbours → a sand beach cell; the shallows ring
            // drawn beneath already softens the transition into the sea.
            let left_sea = rel - 1.0 < li;
            let right_sea = rel + 1.0 >= w - ri;
            let up_sea = !up_in || rel < li_up || rel >= w - ri_up;
            let dn_sea = !dn_in || rel < li_dn || rel >= w - ri_dn;
            if left_sea || right_sea || up_sea || dn_sea {
                let j = cell_jitter(wx as u16, wy as u16) * 0.6;
                let sand = Color::new(
                    (ISO_SAND.r + j).clamp(0.0, 1.0),
                    (ISO_SAND.g + j).clamp(0.0, 1.0),
                    (ISO_SAND.b + j).clamp(0.0, 1.0),
                    1.0,
                );
                fill_diamond(c, hw, hh, sand);
            } else {
                land_diamond(c, hw, hh, prov.tile.health, wx as u16, wy as u16);
            }
        }
    }
}

/// A single health-tinted land diamond with a 2-shade grassland checker plus a
/// cheap per-cell micro-jitter, so big fields read as textured, not a grid.
fn land_diamond(c: Vec2, hw: f32, hh: f32, h: NodeHealth, wx: u16, wy: u16) {
    let (a, b) = iso_terrain_pair(h);
    let base = if (wx as u32 + wy as u32) & 1 == 0 {
        a
    } else {
        b
    };
    let d = cell_jitter(wx, wy);
    let col = Color::new(
        (base.r + d).clamp(0.0, 1.0),
        (base.g + d * 1.3).clamp(0.0, 1.0),
        (base.b + d).clamp(0.0, 1.0),
        1.0,
    );
    fill_diamond(c, hw, hh, col);
}

/// One province's over-terrain detail (PASS 2): forests, daemonset roads, and
/// the province name label. Settlements are drawn by `draw_world` so they can
/// be depth-sorted across the whole continent.
fn draw_province_features(
    prov: &Province,
    cam: &Camera,
    detail: &Lod,
    coast: &Coast,
    occupied: &mut Vec<Rect>,
) {
    draw_forest_iso(prov, cam, coast, detail);
    draw_road_iso(prov, cam, coast, detail);

    if detail.province_labels {
        let (top_li, _) = coast.land_span(prov.y as i32, prov.w as f32);
        let anchor = cam.to_screen(prov.x as f32 + top_li + 0.5, prov.y as f32 + 0.5);
        let ls = label_scale(cam.zoom);
        let fs = 15.0 * ls;
        let name = ascii(&prov.tile.name);
        let pods = format!("{} pods", prov.tile.pods.len());
        let nm = text_size(&name, fs);
        let block_w = nm.width.max(text_size(&pods, fs * 0.8).width);
        let lx = anchor.x - block_w * 0.5;
        let row_h = 28.0 * ls;
        let r = place(
            occupied,
            &[
                Rect::new(lx, anchor.y - nm.height, block_w + 4.0, row_h),
                Rect::new(lx, anchor.y - nm.height - row_h, block_w + 4.0, row_h),
            ],
        );
        text_outline(&name, r.x, r.y + nm.height, fs, INK, HALO);
        text_outline(
            &pods,
            r.x,
            r.y + nm.height + 13.0 * ls,
            12.0 * ls,
            Color::new(0.90, 0.92, 0.85, 1.0),
            HALO,
        );
    }
}

/// A small procedural tree, base at the tile's lower area.
fn draw_tree(base: Vec2, z: f32) {
    let s = 6.0 * z;
    draw_rectangle(
        base.x - 0.8 * z,
        base.y - 1.0 * z,
        1.6 * z,
        4.0 * z,
        ISO_TRUNK,
    );
    draw_triangle(
        vec2(base.x - s * 0.9, base.y),
        vec2(base.x + s * 0.9, base.y),
        vec2(base.x, base.y - s * 1.6),
        ISO_TREE,
    );
    draw_triangle(
        vec2(base.x - s * 0.6, base.y - s * 0.4),
        vec2(base.x + s * 0.6, base.y - s * 0.4),
        vec2(base.x, base.y - s * 1.7),
        ISO_TREE_HI,
    );
}

/// A few trees on hashed inland cells of a healthy province (dropped at world
/// scale, like the old sprite trees).
fn draw_forest_iso(prov: &Province, cam: &Camera, coast: &Coast, detail: &Lod) {
    if prov.tile.health != NodeHealth::Healthy || detail.scale == Scale::World {
        return;
    }
    let z = cam.zoom;
    let (_, hh) = cam.cell_px();
    for i in 0..4u64 {
        let hx = fnv1a64(&format!("{}forest{i}", prov.tile.name));
        let cy = prov.y as i32 + (hx % prov.h.max(1) as u64) as i32;
        let (li, lw) = coast.land_span(cy, prov.w as f32);
        if lw < 3.0 {
            continue;
        }
        let cx = prov.x as f32 + li + 1.0 + ((hx >> 8) % (lw as u64).max(1)) as f32;
        let c = cam.to_screen(cx + 0.5, cy as f32 + 0.5);
        draw_tree(vec2(c.x, c.y + hh * 0.35), z);
    }
}

/// Daemonset roads: short dashes along the +wx (down-right) iso edge on the
/// province's widest land row.
fn draw_road_iso(prov: &Province, cam: &Camera, coast: &Coast, detail: &Lod) {
    if prov.infra == 0 || detail.scale == Scale::World {
        return;
    }
    let z = cam.zoom;
    let row = (prov.y..prov.y + prov.h)
        .max_by(|a, b| {
            coast
                .land_span(*a as i32, prov.w as f32)
                .1
                .total_cmp(&coast.land_span(*b as i32, prov.w as f32).1)
        })
        .unwrap_or(prov.y);
    let (li, lw) = coast.land_span(row as i32, prov.w as f32);
    let n = prov.infra.min(10).min(lw as usize);
    for i in 0..n {
        let cx = prov.x as f32 + li + 0.5 + i as f32;
        let a = cam.to_screen(cx, row as f32 + 0.5);
        let b = cam.to_screen(cx + 0.7, row as f32 + 0.5);
        draw_line(a.x, a.y, b.x, b.y, (2.0 * z).max(1.5), ROAD);
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
    let _ = coast;
    let center = cam.to_screen(
        prov.x as f32 + prov.w as f32 / 2.0,
        prov.y as f32 + prov.h as f32 / 2.0,
    );
    let z = cam.zoom.max(0.55);

    // A small procedural town stands for "settlements here"; the worst concern
    // still reads through the count chip + flag below.
    let tier: u8 = match count {
        0 => return,
        1 => 1,
        2..=3 => 2,
        _ => 3,
    };
    draw_settlement(center, z, tier);

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

/// One iso "block" building standing on the tile: a shaded left wall, a lit
/// right wall, and a top face (terracotta for dwellings, stone for towers).
/// `base` is the block's front (south) ground vertex; `w`/`d`/`hgt` are pixel
/// extents already scaled. Original geometry — no sprites.
fn iso_block(base: Vec2, w: f32, d: f32, hgt: f32, roof: bool) {
    let hw = w * 0.5;
    let hd = d * 0.5;
    let f = base; // front (south)
    let l = vec2(base.x - hw, base.y - hd); // left (west)
    let r = vec2(base.x + hw, base.y - hd); // right (east)
    let bk = vec2(base.x, base.y - d); // back (north)
    let lift = |p: Vec2| vec2(p.x, p.y - hgt);
    let quad = |a: Vec2, b: Vec2, c: Vec2, e: Vec2, col: Color| {
        draw_triangle(a, b, c, col);
        draw_triangle(a, c, e, col);
    };
    // Walls: front-left in shadow, front-right sunlit (the iso depth read).
    quad(l, f, lift(f), lift(l), WALL_SHADE);
    quad(f, r, lift(r), lift(f), WALL);
    draw_line(f.x, f.y, f.x, f.y - hgt, 1.0, WALL_DARK);
    // Top face.
    let (tl, tf, tr, tbk) = (lift(l), lift(f), lift(r), lift(bk));
    quad(tl, tf, tr, tbk, if roof { TILE_ROOF } else { WALL });
    if roof {
        // A shaded north-west slope + ridge line reads as a pitched roof.
        draw_triangle(tl, tbk, tf, TILE_ROOF_S);
        draw_line(tf.x, tf.y, tbk.x, tbk.y, 1.5, WALL_DARK);
    } else {
        // Tower: a crenellated cap across the top face.
        draw_line(tl.x, tl.y, tr.x, tr.y, 2.0, TOWER_CAP);
        draw_line(tf.x, tf.y, tbk.x, tbk.y, 2.0, TOWER_CAP);
    }
}

/// A low iso ring wall around a tier-3 city, drawn before the buildings so
/// they stand inside it.
fn draw_city_wall(c: Vec2, z: f32) {
    let hw = 22.0 * z;
    let hh = 11.0 * z;
    let band = 4.0 * z;
    let p = diamond_pts(c, hw, hh);
    let (n, e, s, w) = (p[0], p[1], p[2], p[3]);
    let lift = |q: Vec2| vec2(q.x, q.y - band);
    // Outer wall faces (front-left shaded, front-right lit).
    draw_triangle(w, s, lift(s), WALL_SHADE);
    draw_triangle(w, lift(s), lift(w), WALL_SHADE);
    draw_triangle(s, e, lift(e), WALL);
    draw_triangle(s, lift(e), lift(s), WALL);
    let _ = n;
    stroke_diamond(vec2(c.x, c.y - band), hw, hh, 2.0 * z, TOWER_CAP);
}

/// The procedural settlement: a cluster of iso blocks that grows from a lone
/// hut (tier 0) to a walled keep (tier 3), drawn back-to-front. `c` is the
/// diamond center on screen; `z` the zoom. Original geometry — no sprites.
fn draw_settlement(c: Vec2, z: f32, tier: u8) {
    let blk = |dx: f32, dy: f32, w: f32, d: f32, h: f32, roof: bool| {
        iso_block(vec2(c.x + dx * z, c.y + dy * z), w * z, d * z, h * z, roof)
    };
    match tier {
        0 => {
            blk(0.0, 3.0, 13.0, 7.0, 9.0, true);
        }
        1 => {
            blk(-6.0, 1.0, 12.0, 7.0, 9.0, true);
            blk(6.0, 4.0, 13.0, 7.0, 10.0, true);
        }
        2 => {
            blk(8.0, -3.0, 9.0, 6.0, 15.0, false); // rear tower
            blk(-8.0, 0.0, 13.0, 7.0, 10.0, true);
            blk(0.0, 5.0, 15.0, 8.0, 12.0, true);
            blk(10.0, 7.0, 12.0, 7.0, 10.0, true);
        }
        _ => {
            draw_city_wall(c, z);
            blk(-10.0, -2.0, 9.0, 6.0, 16.0, false); // back-left tower
            blk(10.0, -2.0, 9.0, 6.0, 16.0, false); // back-right tower
            blk(0.0, -1.0, 15.0, 8.0, 22.0, false); // central keep
            blk(-7.0, 4.0, 13.0, 7.0, 11.0, true); // front-left hall
            blk(8.0, 5.0, 13.0, 7.0, 11.0, true); // front-right hall
            blk(0.0, 8.0, 12.0, 6.0, 18.0, false); // gate tower (frontmost)
        }
    }
}

/// A serif place-name banner centered below a settlement (classic-4X city
/// label): a parchment plate, a thin stone frame, the serif name. De-
/// conflicted via `place` so crowded columns fan their names out.
fn draw_name_banner(c: Vec2, hh: f32, label: String, z: f32, occupied: &mut Vec<Rect>) {
    let fs = (15.0 * label_scale(z)).max(11.0);
    let tm = name_text_size(&label, fs);
    let pad_x = 6.0;
    let pw = tm.width + pad_x * 2.0;
    let ph = tm.height + 6.0;
    let below_y = c.y + hh * 0.7 + 4.0;
    let cx = c.x - pw * 0.5;
    let nr = place(
        occupied,
        &[
            Rect::new(cx, below_y, pw, ph),             // centered below
            Rect::new(cx - pw * 0.5, below_y, pw, ph),  // below-left
            Rect::new(cx + pw * 0.5, below_y, pw, ph),  // below-right
            Rect::new(cx, c.y - hh * 1.6 - ph, pw, ph), // above (last resort)
        ],
    );
    draw_rectangle(nr.x, nr.y, pw, ph, POP_CALM);
    draw_rectangle(nr.x, nr.y, pw, 1.0, STONE_LIGHT);
    draw_rectangle(nr.x, nr.y + ph - 1.0, pw, 1.0, STONE_SHADOW);
    draw_rectangle_lines(nr.x, nr.y, pw, ph, 1.0, STONE_EDGE);
    name_text(&label, nr.x + pad_x, nr.y + ph - 5.0, fs, STONE_INK);
}

/// A settlement, classic-4X style: an iso diamond ground plate, a procedural
/// building cluster that grows with population, a solid lower-left population
/// box, a serif name banner centered below, an attention flag, a granary, and
/// a sync chip when a warm twin exists. All original geometry — no sprites.
fn draw_city(
    city: &City,
    cam: &Camera,
    detail: &Lod,
    pair: Option<&PairSync>,
    occupied: &mut Vec<Rect>,
) {
    let z = cam.zoom;
    let c = cam.to_screen(city.x as f32 + 0.5, city.y as f32 + 0.5); // diamond center
    let (hw, hh) = cam.cell_px();
    let tier: u8 = match city.ready {
        0 => 0,
        1..=3 => 1,
        4..=9 => 2,
        _ => 3,
    };

    // Ground plate: the tile diamond, darkened, with a severity wash so the
    // town reads as sitting ON the tile.
    fill_diamond(c, hw, hh, Color::new(0.0, 0.0, 0.0, 0.16));
    if let Some(sev) = city.severity {
        let col = severity_color(sev);
        fill_diamond(c, hw, hh, Color::new(col.r, col.g, col.b, 0.20));
    }

    // Storage granary inland (north-west = up-left in iso).
    if detail.scale != Scale::World
        && let Some(st) = city.storage
    {
        let col = if st.pending > 0 { WARN } else { STRUCT };
        draw_granary(vec2(c.x - hw * 0.6, c.y - hh * 0.35), z, col);
    }

    // The town itself.
    draw_settlement(c, z, tier);

    // Attention flag: a waving pennant on a pole above the tallest building.
    if let Some(sev) = city.severity {
        let col = severity_color(sev);
        let t = get_time() as f32;
        let wave = (t * 6.0).sin() * 2.0 * z;
        let fx = c.x + 2.0 * z;
        let fy = c.y - 26.0 * z;
        draw_line(fx, fy, fx, fy + 13.0 * z, 1.5, WALL_DARK);
        draw_triangle(
            vec2(fx, fy),
            vec2(fx + 10.0 * z + wave, fy + 4.0 * z),
            vec2(fx, fy + 8.0 * z),
            col,
        );
    }

    // Population box: a solid colored square at the diamond's lower-left
    // (classic-4X convention). Color = health/severity; calm = parchment.
    let (box_col, num_col) = match city.severity {
        Some(Severity::Critical) => (CRIT, INK),
        Some(Severity::Warning) => (WARN, PLATE),
        _ => (POP_CALM, STONE_INK),
    };
    let pop = city.ready.to_string();
    let fs = (14.0 * label_scale(z)).max(10.0);
    let bw = (text_size(&pop, fs).width + 8.0).max(fs + 4.0);
    let bh = fs + 4.0;
    let ax = c.x - hw * 0.80;
    let ay = c.y + hh * 0.30;
    let chip = place(
        occupied,
        &[
            Rect::new(ax, ay, bw, bh),             // lower-left (classic)
            Rect::new(ax, ay - bh - 2.0, bw, bh),  // bump up
            Rect::new(c.x + hw * 0.4, ay, bw, bh), // lower-right fallback
        ],
    );
    draw_rectangle(chip.x, chip.y, bw, bh, box_col);
    draw_rectangle(chip.x, chip.y, bw, 1.0, lighter(box_col, 1.2));
    draw_rectangle(chip.x, chip.y + bh - 1.0, bw, 1.0, darker(box_col, 0.6));
    draw_rectangle_lines(chip.x, chip.y, bw, bh, 1.0, STONE_EDGE);
    let tw = text_size(&pop, fs).width;
    text(
        &pop,
        chip.x + (bw - tw) * 0.5,
        chip.y + bh - 4.0,
        fs,
        num_col,
    );

    // Sync chip glued to the right of the pop box, when a warm twin exists.
    if let Some(p) = pair
        && let Some(st) = p.state(&city.r)
    {
        let badge = ascii(&st.badge());
        let chip_w = text_size(&badge, fs).width + 6.0;
        let sr = place(
            occupied,
            &[
                Rect::new(chip.x + bw + 2.0, chip.y, chip_w, bh),
                Rect::new(chip.x - chip_w - 2.0, chip.y, chip_w, bh),
            ],
        );
        draw_rectangle(sr.x, sr.y, chip_w, bh, STONE_DARK);
        draw_rectangle_lines(sr.x, sr.y, chip_w, bh, 1.0, sync_color(st));
        text(&badge, sr.x + 3.0, sr.y + bh - 4.0, fs, sync_color(st));
    }

    // Name banner. At regional scale only the noteworthy keep labels (troubled
    // or populous); at local scale every settlement is named in full.
    let named = detail.name_plates
        && (detail.scale == Scale::Local
            || detail.name_all
            || city.severity.is_some()
            || city.ready >= 4);
    if named {
        let full = detail.scale == Scale::Local;
        let label = ascii(&abbrev(&city.r.name, if full { 64 } else { 11 }));
        draw_name_banner(c, hh, label, z, occupied);
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

/// Connectivity moored off a continent's east coast: Service harbors and
/// Ingress gates, each on its city's latitude. Dropped at world scale (the
/// aggregate view); small line-marks at regional and local scale. Names are
/// left to the hover tooltip and the city screen, so the coast stays clean.
fn draw_coast(cont: &Continent, cam: &Camera, detail: &Lod) {
    if detail.scale == Scale::World {
        return;
    }
    let z = cam.zoom;
    for m in &cont.coast {
        let c = cam.to_screen(m.x as f32 + 0.5, m.y as f32 + 0.5);
        match m.kind {
            CoastKind::Harbor => draw_harbor(c, z),
            CoastKind::Gate => draw_gate(c, z),
        }
    }
}

/// A small anchor — the Service harbor mark.
pub(crate) fn draw_harbor(c: Vec2, z: f32) {
    let u = (4.5 * z).clamp(3.0, 14.0);
    let th = (z * 1.6).clamp(1.0, 3.0);
    draw_circle(c.x, c.y, u * 1.4, Color::new(0.04, 0.06, 0.10, 0.55));
    draw_circle_lines(c.x, c.y - u, u * 0.45, th, STRUCT);
    draw_line(c.x, c.y - u * 0.6, c.x, c.y + u, th, STRUCT);
    draw_line(
        c.x - u * 0.55,
        c.y - u * 0.3,
        c.x + u * 0.55,
        c.y - u * 0.3,
        th,
        STRUCT,
    );
    draw_line(c.x, c.y + u, c.x - u * 0.8, c.y + u * 0.35, th, STRUCT);
    draw_line(c.x, c.y + u, c.x + u * 0.8, c.y + u * 0.35, th, STRUCT);
}

/// A small arch — the Ingress gate mark.
pub(crate) fn draw_gate(c: Vec2, z: f32) {
    let u = (4.5 * z).clamp(3.0, 14.0);
    let th = (z * 1.6).clamp(1.0, 3.0);
    let w = u * 0.8;
    draw_circle(c.x, c.y, u * 1.4, Color::new(0.04, 0.06, 0.10, 0.55));
    draw_line(c.x - w, c.y + u, c.x - w, c.y - u, th, STRUCT);
    draw_line(c.x + w, c.y + u, c.x + w, c.y - u, th, STRUCT);
    draw_line(
        c.x - w - th * 0.5,
        c.y - u,
        c.x + w + th * 0.5,
        c.y - u,
        th,
        STRUCT,
    );
}

/// A small silo — the persistent-storage granary mark. `col` carries the
/// binding state: cyan when all claims are Bound, yellow when any pends.
pub(crate) fn draw_granary(c: Vec2, z: f32, col: Color) {
    let u = (4.0 * z).clamp(2.5, 12.0);
    let th = (z * 1.4).clamp(1.0, 2.5);
    draw_circle(c.x, c.y, u * 1.5, Color::new(0.04, 0.06, 0.10, 0.5));
    draw_rectangle_lines(c.x - u * 0.8, c.y - u * 0.7, u * 1.6, u * 1.5, th, col);
    draw_line(c.x - u * 0.8, c.y - u * 0.7, c.x, c.y - u * 1.3, th, col);
    draw_line(c.x + u * 0.8, c.y - u * 0.7, c.x, c.y - u * 1.3, th, col);
    draw_line(
        c.x - u * 0.8,
        c.y + u * 0.2,
        c.x + u * 0.8,
        c.y + u * 0.2,
        th,
        col,
    );
}

/// A pennant on a pole — a Job expedition.
pub(crate) fn draw_job(c: Vec2, z: f32, col: Color) {
    let u = (6.0 * z).clamp(4.0, 16.0);
    let th = (z * 1.4).clamp(1.0, 2.5);
    draw_line(
        c.x - u * 0.4,
        c.y - u * 0.8,
        c.x - u * 0.4,
        c.y + u * 0.8,
        th,
        col,
    );
    draw_triangle(
        vec2(c.x - u * 0.4, c.y - u * 0.8),
        vec2(c.x + u * 0.7, c.y - u * 0.35),
        vec2(c.x - u * 0.4, c.y + u * 0.1),
        col,
    );
}

/// A clock face — a CronJob's recurring schedule.
pub(crate) fn draw_cronjob(c: Vec2, z: f32, col: Color) {
    let r = (5.0 * z).clamp(3.5, 13.0);
    let th = (z * 1.4).clamp(1.0, 2.5);
    draw_circle_lines(c.x, c.y, r, th, col);
    draw_line(c.x, c.y, c.x, c.y - r * 0.7, th, col);
    draw_line(c.x, c.y, c.x + r * 0.5, c.y, th, col);
}

/// Island terrain (PASS 1): a small cluster of sand diamonds with a darker
/// rim, ringed by the same graded shallows as continents so the sandbar
/// blends into the sea.
fn draw_island_terrain(isl: &Island, cam: &Camera) {
    let (hw, hh) = cam.cell_px();
    let x1 = (isl.x + isl.w) as i32;
    let y1 = (isl.y + isl.h) as i32;
    let on_screen = |c: Vec2, m: f32| {
        c.x > -TILE_W * m
            && c.x < screen_width() + TILE_W * m
            && c.y > -TILE_H * m
            && c.y < screen_height() + TILE_H * m
    };
    // Shallows ring under the island's border cells (covered on land by the
    // sand pass below; pokes into the sea as a soft band).
    for wy in isl.y as i32..y1 {
        for wx in isl.x as i32..x1 {
            let edge = wx == isl.x as i32 || wx == x1 - 1 || wy == isl.y as i32 || wy == y1 - 1;
            if !edge {
                continue;
            }
            let c = cam.to_screen(wx as f32 + 0.5, wy as f32 + 0.5);
            if !on_screen(c, 2.0) {
                continue;
            }
            fill_diamond(c, hw * 1.7, hh * 1.7, SHALLOWS_DEEP);
            fill_diamond(c, hw * 1.35, hh * 1.35, SHALLOWS);
        }
    }
    // Sand body.
    for wy in isl.y as i32..y1 {
        for wx in isl.x as i32..x1 {
            let c = cam.to_screen(wx as f32 + 0.5, wy as f32 + 0.5);
            if !on_screen(c, 1.0) {
                continue;
            }
            let edge = wx == isl.x as i32 || wx == x1 - 1 || wy == isl.y as i32 || wy == y1 - 1;
            let base = if edge { ISO_SAND_DARK } else { ISO_SAND };
            let j = cell_jitter(wx as u16, wy as u16) * 0.6;
            let col = Color::new(
                (base.r + j).clamp(0.0, 1.0),
                (base.g + j).clamp(0.0, 1.0),
                (base.b + j).clamp(0.0, 1.0),
                1.0,
            );
            fill_diamond(c, hw, hh, col);
        }
    }
}

/// Island over-terrain detail (PASS 2): the isle label, the world-scale count
/// badge, the structure marks, and the "+N more" overflow.
fn draw_island_features(isl: &Island, cam: &Camera, detail: &Lod, _occupied: &mut Vec<Rect>) {
    let ls = label_scale(cam.zoom);
    let center_top = cam.to_screen(isl.x as f32 + isl.w as f32 * 0.5, isl.y as f32);
    if detail.structures_labels {
        let s = ascii(&format!("isle of {}", isl.label));
        let fs = 13.0 * ls;
        let tm = text_size(&s, fs);
        text_outline(
            &s,
            center_top.x - tm.width * 0.5,
            center_top.y - 4.0,
            fs,
            INK,
            HALO,
        );
    }
    // World scale: generalize the isle's structures into one count badge.
    if detail.scale == Scale::World {
        let total = isl.structures.len() + isl.more;
        if total > 0 {
            let center = cam.to_screen(
                isl.x as f32 + isl.w as f32 / 2.0,
                isl.y as f32 + isl.h as f32 / 2.0,
            );
            let label = total.to_string();
            let fs = (13.0 * ls).max(11.0);
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
    let mark_color = |s: &kubernation_core::state::world::Structure| {
        if s.alert {
            WARN
        } else if s.glyph == '◌' {
            DIM
        } else {
            STRUCT
        }
    };
    // Below the label threshold: just dot the marks on the band.
    if !detail.structures_labels {
        for s in &isl.structures {
            let p = cam.to_screen(isl.x as f32 + isl.w as f32 * 0.5, s.y as f32 + 0.5);
            draw_struct_mark(s.glyph, p, cam.zoom, mark_color(s));
        }
        return;
    }
    // Labels on: a tidy scrim-backed legend list (mark + name per row),
    // centered below the band — long names stack instead of overprinting, and
    // the dark scrim keeps them readable over both sand and sea.
    let fs = 12.0 * ls;
    let line_h = (fs + 6.0).max(16.0 * ls);
    let mark_w = 16.0 * ls;
    let mut rows: Vec<(char, Color, String, Color)> = isl
        .structures
        .iter()
        .map(|s| {
            let mut t = format!("{}/{}", s.kind, s.name);
            if !s.detail.is_empty() {
                t.push_str(&format!(" {}", s.detail));
            }
            let tc = if s.alert { WARN } else { INK };
            (s.glyph, mark_color(s), ascii(&t), tc)
        })
        .collect();
    if isl.more > 0 {
        rows.push((' ', DIM, format!("+{} more", isl.more), DIM));
    }
    if rows.is_empty() {
        return;
    }
    let maxw = rows
        .iter()
        .map(|(_, _, t, _)| text_size(t, fs).width)
        .fold(0.0_f32, f32::max);
    let bw = mark_w + maxw + 12.0;
    let bh = rows.len() as f32 * line_h + 8.0;
    let last_y = isl.structures.iter().map(|s| s.y).max().unwrap_or(isl.y);
    let base = cam.to_screen(isl.x as f32 + isl.w as f32 * 0.5, last_y as f32 + 1.0);
    let bx = base.x - bw * 0.5;
    let by = base.y;
    draw_rectangle(bx, by, bw, bh, Color::new(0.08, 0.09, 0.07, 0.76));
    draw_rectangle_lines(bx, by, bw, bh, 1.0, darker(PARCHMENT, 0.55));
    let mut ly = by + 4.0 + fs;
    for (glyph, mcol, t, tcol) in &rows {
        if *glyph != ' ' {
            draw_struct_mark(
                *glyph,
                vec2(bx + 4.0 + mark_w * 0.5, ly - fs * 0.32),
                ls,
                *mcol,
            );
        }
        text(t, bx + 4.0 + mark_w, ly, fs, *tcol);
        ly += line_h;
    }
}

/// Draw one namespace-island structure mark (CRD gem, encampment tent, Job
/// pennant, CronJob clock) centered at `p`.
fn draw_struct_mark(glyph: char, p: Vec2, z: f32, color: Color) {
    match glyph {
        '✦' => {
            draw_poly(p.x, p.y, 4, 6.0 * z, 45.0, color);
            draw_poly_lines(p.x, p.y, 4, 6.0 * z, 45.0, 1.5, darker(color, 0.5));
        }
        '◌' => {
            draw_poly(p.x, p.y, 3, 6.0 * z, 0.0, color);
        }
        '◈' => draw_job(p, z, color),
        '◷' => draw_cronjob(p, z, color),
        _ => draw_poly(p.x, p.y, 4, 6.0 * z, 45.0, color),
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
    let mh = (bounds.1 as f32 * scale * (TILE_H / TILE_W)).min(190.0);
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
    // Viewport indicator: the visible region is a sheared parallelogram of
    // cells under iso, so inverse-project the four screen corners and box
    // their AABB onto the (top-down) chart.
    let (hw, hh) = cam.cell_px();
    let inv = |sx: f32, sy: f32| {
        let a = (sx + cam.pos.x) / hw;
        let b = (sy + cam.pos.y) / hh;
        ((a + b) * 0.5, (b - a) * 0.5)
    };
    let cs = [
        inv(0.0, 0.0),
        inv(screen_width(), 0.0),
        inv(0.0, screen_height()),
        inv(screen_width(), screen_height()),
    ];
    let minx = cs.iter().map(|c| c.0).fold(f32::MAX, f32::min).max(0.0);
    let maxx = cs.iter().map(|c| c.0).fold(f32::MIN, f32::max);
    let miny = cs.iter().map(|c| c.1).fold(f32::MAX, f32::min).max(0.0);
    let maxy = cs.iter().map(|c| c.1).fold(f32::MIN, f32::max);
    draw_rectangle_lines(
        ml.inner.x + minx * ml.scale_x,
        ml.inner.y + miny * ml.scale_y,
        ((maxx - minx) * ml.scale_x).min(ml.inner.w),
        ((maxy - miny) * ml.scale_y).min(ml.inner.h),
        2.0,
        INK,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // The load-bearing iso invariant: a click on a tile must resolve back to
    // that exact tile. With "integer = north vertex / center = +0.5", the
    // cell that owns a screen point is `floor` of the inverted coords. If this
    // ever fails, clicks/hover land on the wrong diamond near tile edges.
    fn roundtrip(zoom: f32, pos: Vec2, bounds: (u16, u16), cells: &[(u16, u16)]) {
        let cam = Camera {
            pos,
            zoom,
            target: None,
        };
        for &(wx, wy) in cells {
            let center = cam.to_screen(wx as f32 + 0.5, wy as f32 + 0.5);
            assert_eq!(
                cam.cell_at(center, bounds),
                Some((wx, wy)),
                "cell ({wx},{wy}) center misrouted at zoom {zoom}"
            );
        }
    }

    #[test]
    fn cell_at_inverts_to_screen_for_every_cell() {
        let bounds = (40u16, 30u16);
        let cells: Vec<(u16, u16)> = (0..bounds.1)
            .flat_map(|y| (0..bounds.0).map(move |x| (x, y)))
            .collect();
        roundtrip(1.0, vec2(0.0, 0.0), bounds, &cells);
    }

    #[test]
    fn cell_at_inverts_under_zoom_and_pan() {
        let bounds = (60u16, 50u16);
        let cells = [(0, 0), (1, 0), (0, 1), (7, 3), (59, 49), (25, 10)];
        roundtrip(1.7, vec2(-123.0, 45.0), bounds, &cells);
        roundtrip(0.43, vec2(311.0, -88.0), bounds, &cells);
    }
}
