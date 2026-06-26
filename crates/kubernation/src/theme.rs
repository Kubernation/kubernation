//! GUI palette and tiny text helpers. Same philosophy as the TUI theme:
//! terrain colors for the living world, saturated red/yellow reserved for
//! attention.

use std::sync::atomic::{AtomicU8, Ordering};

use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::NodeHealth;
use kubernation_core::state::saturation::SatLevel;
use kubernation_core::util::fnv1a64;
use macroquad::prelude::*;

// --- colour-vision mode ----------------------------------------------------
// The product's whole grammar rides on green (healthy/good/calm) vs red
// (critical/NotReady), which red-green colour-blindness (deuteranopia +
// protanopia, ~8% of men) cannot distinguish. The colour-blind palette moves the
// GREEN axis to a steel **blue** — blue / amber / red are all mutually
// distinguishable for both types — leaving red (CRIT) and amber (WARN), which are
// already distinguishable, untouched. Set once at startup from `--colorblind`;
// every meaning-green funnels through `cb_*` so a single switch covers the map
// terrain, the overlays, the marks, and the advisor text. (Tritanopia — rare
// blue-yellow — is out of scope; it would want a different remap.)
static COLOR_MODE: AtomicU8 = AtomicU8::new(0);

/// Switch the palette to the colour-blind-safe variant (call once at startup,
/// before any draw). `false` is the standard palette (the default).
pub fn set_colorblind(on: bool) {
    COLOR_MODE.store(u8::from(on), Ordering::Relaxed);
}

/// Is the colour-blind-safe palette active?
pub fn colorblind() -> bool {
    COLOR_MODE.load(Ordering::Relaxed) != 0
}

/// Bright "good/healthy" colour for text + marks on dark panels (the steel blue
/// in colour-blind mode, else the standard green).
pub const fn cb_good_default() -> Color {
    Color::new(0.52, 0.80, 0.47, 1.0)
}
/// Two-shade "healthy/calm" LAND pair for the map (steel blue in colour-blind
/// mode) — the shared substitute wherever terrain would otherwise be green.
fn cb_land(std: (Color, Color)) -> (Color, Color) {
    if colorblind() {
        (
            Color::new(0.22, 0.43, 0.62, 1.0),
            Color::new(0.27, 0.49, 0.68, 1.0),
        )
    } else {
        std
    }
}

pub const OCEAN: Color = Color::new(0.06, 0.17, 0.30, 1.0);
pub const WAVE: Color = Color::new(0.11, 0.25, 0.41, 1.0);
pub const SAND: Color = Color::new(0.77, 0.70, 0.47, 1.0);
pub const PARCHMENT: Color = Color::new(0.83, 0.70, 0.44, 1.0);
pub const PLATE: Color = Color::new(0.08, 0.09, 0.07, 0.82);
pub const PANEL: Color = Color::new(0.10, 0.095, 0.075, 0.96);
pub const INK: Color = Color::new(0.95, 0.94, 0.90, 1.0);
pub const DIM: Color = Color::new(0.62, 0.60, 0.55, 1.0);
pub const CRIT: Color = Color::new(0.83, 0.18, 0.13, 1.0);
pub const WARN: Color = Color::new(0.88, 0.72, 0.18, 1.0);
/// Readable "healthy / good" color for text on the dark window panels (advisor
/// screens). Meaning color, like CRIT/WARN — green by default, a steel blue under
/// the colour-blind palette (red-green safe). A fn, not a const, so it can switch.
pub fn good() -> Color {
    if colorblind() {
        Color::new(0.40, 0.68, 0.98, 1.0)
    } else {
        cb_good_default()
    }
}

/// "Healthy / low / full" fill for a gauge bar or legend mark (green by default,
/// the steel blue under the colour-blind palette) — the gauge analogue of [`good`].
pub fn gauge_ok() -> Color {
    if colorblind() {
        Color::new(0.34, 0.58, 0.86, 1.0)
    } else {
        Color::new(0.35, 0.60, 0.30, 1.0)
    }
}
pub const ROAD: Color = Color::new(0.42, 0.30, 0.18, 1.0);
pub const STRUCT: Color = Color::new(0.45, 0.85, 0.90, 1.0);
pub const HOUSE: Color = Color::new(0.82, 0.78, 0.68, 1.0);
pub const ROOF: Color = Color::new(0.55, 0.25, 0.16, 1.0);

// --- isometric world palette (muted classic-4X tones) ---------------------
// The map reprojected to 2:1 diamonds wants a calmer, dithered terrain than
// the old flat fills. Ocean is two cool blues (a coarse screen dither); land
// is two shades per health (a grassland checker). Saturated red/yellow stay
// reserved for attention and never appear here.
pub const ISO_OCEAN: Color = Color::new(0.13, 0.27, 0.42, 1.0);
pub const ISO_OCEAN_DEEP: Color = Color::new(0.10, 0.22, 0.36, 1.0);
// Two graded shallows tones ringing the coast (deep ocean → these → sand), so
// the shoreline blends instead of cutting a hard diamond edge.
pub const SHALLOWS_DEEP: Color = Color::new(0.18, 0.37, 0.47, 1.0);
pub const SHALLOWS: Color = Color::new(0.27, 0.47, 0.52, 1.0);
pub const ISO_SAND: Color = Color::new(0.78, 0.70, 0.49, 1.0);
pub const ISO_SAND_DARK: Color = Color::new(0.64, 0.56, 0.37, 1.0);
pub const ISO_TREE: Color = Color::new(0.16, 0.34, 0.18, 1.0);
pub const ISO_TREE_HI: Color = Color::new(0.24, 0.46, 0.24, 1.0);
pub const ISO_TRUNK: Color = Color::new(0.32, 0.22, 0.13, 1.0);
/// Dark halo behind un-plated map labels so they read on terrain OR sea.
pub const HALO: Color = Color::new(0.05, 0.06, 0.05, 0.88);

// --- tan-stone HUD chrome (classic-4X panels) -----------------------------
// Warm carved-stone panels replace the near-black plates for HUD chrome
// (tooltip, top bar, attention strip, picker). The meaning colors above are
// untouched and pop harder against warm stone than against black.
pub const STONE: Color = Color::new(0.74, 0.66, 0.50, 0.97);
pub const STONE_DARK: Color = Color::new(0.46, 0.39, 0.28, 0.98);
pub const STONE_LIGHT: Color = Color::new(0.86, 0.79, 0.62, 1.0);
pub const STONE_SHADOW: Color = Color::new(0.30, 0.25, 0.17, 1.0);
pub const STONE_EDGE: Color = Color::new(0.34, 0.28, 0.19, 1.0);
pub const STONE_INK: Color = Color::new(0.16, 0.12, 0.07, 1.0);
pub const STONE_INK_DIM: Color = Color::new(0.36, 0.30, 0.21, 1.0);
// Severity ink for *stone* backgrounds (the strip / column / tooltip). The
// bright map colors (CRIT/WARN/DIM) wash out on warm tan, so attention text on
// stone uses these darker, higher-contrast variants instead.
pub const STONE_CRIT: Color = Color::new(0.60, 0.09, 0.06, 1.0);
pub const STONE_WARN: Color = Color::new(0.52, 0.33, 0.02, 1.0);
/// Connectivity/structure ink for stone (a dark teal — the bright `STRUCT`
/// cyan washes out on tan), keeping the cyan hue but legible.
pub const STONE_STRUCT: Color = Color::new(0.06, 0.34, 0.38, 1.0);

// --- settlement masonry (warm neutral tones, NOT meaning-encoding) --------
pub const WALL: Color = Color::new(0.82, 0.76, 0.63, 1.0);
pub const WALL_SHADE: Color = Color::new(0.60, 0.54, 0.43, 1.0);
pub const WALL_DARK: Color = Color::new(0.40, 0.35, 0.27, 1.0);
pub const TILE_ROOF: Color = Color::new(0.68, 0.31, 0.21, 1.0);
pub const TILE_ROOF_S: Color = Color::new(0.49, 0.21, 0.14, 1.0);
pub const TOWER_CAP: Color = Color::new(0.34, 0.30, 0.24, 1.0);
/// Calm population box — a neutral parchment chip; severity overrides it.
pub const POP_CALM: Color = Color::new(0.88, 0.83, 0.66, 1.0);

pub fn terrain(h: NodeHealth) -> Color {
    match h {
        NodeHealth::Healthy if colorblind() => Color::new(0.24, 0.44, 0.62, 1.0),
        NodeHealth::Healthy => Color::new(0.30, 0.50, 0.24, 1.0),
        NodeHealth::Cordoned => Color::new(0.55, 0.50, 0.24, 1.0),
        NodeHealth::Pressure => Color::new(0.62, 0.42, 0.18, 1.0),
        NodeHealth::NotReady => Color::new(0.42, 0.15, 0.12, 1.0),
    }
}

/// Healthy-land base greens (two shades for the grassland checker dither).
/// Degraded states keep the sand/stone tones of `terrain()` — trouble still
/// reads as terrain, while saturated red/yellow stay reserved for attention.
pub fn iso_terrain_pair(h: NodeHealth) -> (Color, Color) {
    match h {
        NodeHealth::Healthy => cb_land((
            Color::new(0.30, 0.49, 0.25, 1.0),
            Color::new(0.35, 0.55, 0.29, 1.0),
        )),
        NodeHealth::Cordoned => (
            Color::new(0.55, 0.50, 0.26, 1.0),
            Color::new(0.60, 0.55, 0.30, 1.0),
        ),
        NodeHealth::Pressure => (
            Color::new(0.60, 0.42, 0.20, 1.0),
            Color::new(0.66, 0.47, 0.24, 1.0),
        ),
        NodeHealth::NotReady => (
            Color::new(0.42, 0.16, 0.13, 1.0),
            Color::new(0.47, 0.20, 0.16, 1.0),
        ),
    }
}

/// A two-shade land "heat" pair by severity level (0 calm green, 1 elevated
/// amber, 2 high red), shared by the Pressure and Replicas overlays. Two shades
/// so the iso terrain checker/jitter still reads as textured land.
pub fn heat_pair(level: u8) -> (Color, Color) {
    match level {
        2 => (
            Color::new(0.55, 0.16, 0.13, 1.0),
            Color::new(0.62, 0.21, 0.17, 1.0),
        ),
        1 => (
            Color::new(0.62, 0.46, 0.16, 1.0),
            Color::new(0.68, 0.52, 0.20, 1.0),
        ),
        _ => cb_land((
            Color::new(0.26, 0.46, 0.24, 1.0),
            Color::new(0.31, 0.52, 0.28, 1.0),
        )),
    }
}

/// Heat color pair for a scheduling/usage ratio — the **Pressure** map
/// overlay. Mirrors the documented pressure buckets (`state/model.rs`): <0.7
/// calm green, 0.7–0.9 elevated amber, ≥0.9 high red.
pub fn pressure_pair(ratio: f64) -> (Color, Color) {
    heat_pair(if ratio >= 0.9 {
        2
    } else if ratio >= 0.7 {
        1
    } else {
        0
    })
}

/// Land pair for the **Saturation** ("strain") overlay — by the node's worst
/// saturation level. Calm recedes to idle land (so a flagged province pops),
/// Elevated → amber, High → red — reusing the shared heat palette so it reads in
/// the same severity grammar as Pressure/Replicas.
pub fn sat_pair(level: SatLevel) -> (Color, Color) {
    match level {
        SatLevel::Calm => idle_land_pair(),
        SatLevel::Elevated => heat_pair(1),
        SatLevel::High => heat_pair(2),
    }
}

/// The **cost (upkeep)** overlay ramp — a coin/bronze "spend" gradient from pale
/// parchment-gold (cheap) to deep antique-bronze (dear). `pos` is `node_cost /
/// max_node_cost`, in `0..=1`. Terrain-family (warm metallic, green kept
/// substantial so it reads brown/gold) — deliberately NOT the saturated red/yellow
/// reserved for attention, so a "most expensive" province can't be mistaken for a
/// NotReady one. Returns `(base, lit)` so `land_diamond`'s dither reads as terrain.
pub fn cost_pair(pos: f64) -> (Color, Color) {
    let t = pos.clamp(0.0, 1.0) as f32;
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    // pale gold (cheap) → deep bronze (dear); g/r ≈ 0.76 at the dark end so it
    // stays brown, never approaching CRIT red (g ≪ r).
    let base = Color::new(lerp(0.62, 0.55), lerp(0.57, 0.42), lerp(0.41, 0.16), 1.0);
    let lit = Color::new(base.r + 0.06, base.g + 0.05, base.b + 0.05, 1.0);
    (base, lit)
}

/// Desaturated grey-green land for a province with nothing to encode under the
/// current overlay (no cities for Replicas / Namespace) — it recedes so the
/// flagged provinces pop.
pub fn idle_land_pair() -> (Color, Color) {
    (
        Color::new(0.34, 0.37, 0.34, 1.0),
        Color::new(0.39, 0.42, 0.39, 1.0),
    )
}

/// A calm slate-stone land pair meaning "walled / fortified" — the **Coverage**
/// (walls) overlay paints fully-isolated provinces with it (terrain-family, not
/// a trouble colour: the *gap*, not the wall, is what we flag).
pub fn walled_pair() -> (Color, Color) {
    (
        Color::new(0.40, 0.47, 0.45, 1.0),
        Color::new(0.45, 0.52, 0.50, 1.0),
    )
}

/// A stable two-shade land pair for a namespace — the **Namespace** "political"
/// overlay (each namespace a deterministic hue, muted to terrain saturation).
pub fn namespace_pair(ns: &str) -> (Color, Color) {
    let hue = (fnv1a64(ns) % 360) as f32;
    (hsv(hue, 0.42, 0.52), hsv(hue, 0.42, 0.60))
}

/// Minimal HSV→RGB (h in [0,360), s/v in [0,1]) for the namespace palette.
fn hsv(h: f32, s: f32, v: f32) -> Color {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::new(r + m, g + m, b + m, 1.0)
}

/// Cheap per-cell shade jitter (no allocation, unlike `terrain_cell`'s
/// `format!` hash) so large iso terrain fills don't read as a printed grid.
pub fn cell_jitter(wx: u16, wy: u16) -> f32 {
    let mut h = (wx as u32).wrapping_mul(73_856_093) ^ (wy as u32).wrapping_mul(19_349_663);
    h ^= h >> 13;
    h = h.wrapping_mul(0x9E37_79B1);
    h ^= h >> 16;
    match h % 5 {
        0 => -0.030,
        1 => -0.012,
        2 => 0.0,
        3 => 0.018,
        _ => 0.034,
    }
}

/// A carved tan-stone panel: fill, a 1px dark frame, and a highlight on the
/// top/left + shadow on the bottom/right so it reads as chiseled stone.
pub fn stone_panel(x: f32, y: f32, w: f32, h: f32) {
    draw_rectangle(x, y, w, h, STONE);
    draw_rectangle(x + 1.0, y + 1.0, w - 2.0, 2.0, STONE_LIGHT);
    draw_rectangle(x + 1.0, y + 1.0, 2.0, h - 2.0, STONE_LIGHT);
    draw_rectangle(x + 1.0, y + h - 3.0, w - 2.0, 2.0, STONE_SHADOW);
    draw_rectangle(x + w - 3.0, y + 1.0, 2.0, h - 2.0, STONE_SHADOW);
    draw_rectangle_lines(x, y, w, h, 1.0, STONE_EDGE);
}

/// A recessed well inside a stone panel (title strips, highlighted rows).
pub fn stone_well(x: f32, y: f32, w: f32, h: f32) {
    draw_rectangle(x, y, w, h, STONE_DARK);
    draw_rectangle(x, y, w, 1.5, STONE_SHADOW);
    draw_rectangle(x, y + h - 1.5, w, 1.5, STONE_LIGHT);
}

pub fn darker(c: Color, f: f32) -> Color {
    Color::new(c.r * f, c.g * f, c.b * f, c.a)
}

pub fn lighter(c: Color, f: f32) -> Color {
    Color::new(
        (c.r * f).clamp(0.0, 1.0),
        (c.g * f).clamp(0.0, 1.0),
        (c.b * f).clamp(0.0, 1.0),
        c.a,
    )
}

pub fn severity_color(s: Severity) -> Color {
    match s {
        Severity::Critical => CRIT,
        Severity::Warning => WARN,
        Severity::Info => DIM,
    }
}

/// Severity ink for text on a stone background (high-contrast dark variants).
pub fn severity_on_stone(s: Severity) -> Color {
    match s {
        Severity::Critical => STONE_CRIT,
        Severity::Warning => STONE_WARN,
        Severity::Info => STONE_INK,
    }
}

/// macroquad's built-in font is ASCII-ish; swap the TUI glyph vocabulary
/// for plain characters so nothing renders as tofu.
pub fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '▸' => '>',
            '‼' => '!',
            '⊘' => 'o',
            '≣' => '=',
            c if c.is_ascii() || "·—–…×≠↔−≈✓✗•".contains(c) => c,
            _ => '?',
        })
        .collect()
}

/// Chip color for a pair sync state.
pub fn sync_color(state: &kubernation_core::state::pair::SyncState) -> Color {
    use kubernation_core::state::pair::SyncState;
    match state {
        SyncState::InSync if colorblind() => Color::new(0.42, 0.62, 0.92, 1.0),
        SyncState::InSync => Color::new(0.50, 0.65, 0.45, 1.0),
        SyncState::Drift { .. } => WARN,
        SyncState::OnlyHot => CRIT,
        SyncState::OnlyWarm => STRUCT,
    }
}

/// Pair-sync ink for a stone background (high-contrast dark variants).
pub fn sync_on_stone(state: &kubernation_core::state::pair::SyncState) -> Color {
    use kubernation_core::state::pair::SyncState;
    match state {
        SyncState::InSync => STONE_INK_DIM,
        SyncState::Drift { .. } => STONE_WARN,
        SyncState::OnlyHot => STONE_CRIT,
        SyncState::OnlyWarm => STONE_STRUCT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorblind_palette_swaps_meaning_greens_to_blue() {
        // Standard: "good"/healthy reads green (green channel dominates blue).
        set_colorblind(false);
        assert!(good().g > good().b, "standard good is green");
        let t = terrain(NodeHealth::Healthy);
        assert!(t.g > t.b, "standard healthy land is green");
        assert!(heat_pair(0).0.g > heat_pair(0).0.b, "calm heat is green");

        // Colour-blind: those greens become blue (blue channel dominates green)…
        set_colorblind(true);
        assert!(good().b > good().g, "colour-blind good is blue");
        let tb = terrain(NodeHealth::Healthy);
        assert!(tb.b > tb.g, "colour-blind healthy land is blue");
        assert!(heat_pair(0).0.b > heat_pair(0).0.g, "calm heat is blue");
        // (Red CRIT + amber WARN are consts — untouched in both modes by design.)

        set_colorblind(false); // reset for the rest of the suite
    }

    #[test]
    fn cost_pair_is_a_monotonic_brown_ramp() {
        let cheap = cost_pair(0.0).0;
        let dear = cost_pair(1.0).0;
        // Dear is darker than cheap (lower luma) — a spend ramp.
        let luma = |c: Color| c.r + c.g + c.b;
        assert!(luma(dear) < luma(cheap), "dear should be darker");
        // Stays brown/gold (green substantial vs red) — never CRIT red (g << r).
        assert!(
            dear.g > dear.r * 0.6,
            "dark end stays brown, not red: {dear:?}"
        );
        // Clamps out of range.
        assert_eq!(cost_pair(2.0).0.r, cost_pair(1.0).0.r);
        assert_eq!(cost_pair(-1.0).0.r, cost_pair(0.0).0.r);
    }
}
