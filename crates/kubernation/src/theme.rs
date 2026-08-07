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
/// The ambient pool a standing object casts where it meets the ground — a
/// neutral dark, alpha supplied per style by `MapStyle::shadow_alpha`.
///
/// Deliberately NOT in the `cb_*` colour-blind funnel: a contact shadow is a
/// DEPTH cue, not a meaning channel. Meaning colours (health, severity, sync)
/// are what the colour-blind palette remaps; remapping a shadow would be
/// noise. Do not add a variant.
pub const CONTACT_SHADOW: Color = Color::new(0.06, 0.07, 0.10, 1.0);
/// The hover marker — what the pointer is over, shown BEFORE you commit to a
/// click. A neutral bright warm-white: deliberately NOT `CRIT`/`WARN`, which
/// `draw_blast` owns and which mean danger. Hover is ambient, not an alert, so
/// if the two are ever on screen together they must be instantly distinguishable
/// (this is also why the marker never pulses).
pub const HOVER: Color = Color::new(0.98, 0.97, 0.90, 0.80);
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

/// Cliff faces for a raised land tile (`MapStyle::Relief`), DERIVED from the
/// colour its top was drawn with: `(sunlit_se, shadow_sw)`.
///
/// Deriving rather than tabulating is load-bearing. Land fill has nine
/// producers (`draw::overlay_pair` — one per overlay) plus the `ISO_SAND` beach
/// branch, and each has a colour-blind variant; a fixed cliff colour would be
/// wrong for eight of them and would need re-checking every time an overlay is
/// added. A pure function of the top colour composes with all of them, and with
/// `set_colorblind`, for free.
///
/// The two factors keep `iso_block`'s convention — front-right sunlit,
/// front-left shadowed (`WALL_SHADE / WALL` ≈ 0.71) — so a cliff and a city
/// wall catch the light the same way.
pub fn cliff_pair(top: Color) -> (Color, Color) {
    const SUNLIT: f32 = 0.78;
    const SHADOW: f32 = 0.55; // ≈ 0.71 × SUNLIT, matching WALL_SHADE / WALL
    let scale = |f: f32| Color::new(top.r * f, top.g * f, top.b * f, top.a);
    (scale(SUNLIT), scale(SHADOW))
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
/// The land pair for a province whose node reports NO allocatable, so no
/// ratio-derived reading exists for it. Deliberately OUTSIDE every overlay ramp
/// — a cold neutral that belongs to none of them — and always drawn with
/// diagonal hatching on top, because texture says "no data" while any hue would
/// be read as a data value. Not routed through the colour-blind funnel: it
/// carries no meaning on the good/bad axis, and the hatch is the real signal.
pub fn unmeasured_pair() -> (Color, Color) {
    (
        Color::new(0.30, 0.31, 0.35, 1.0),
        Color::new(0.35, 0.36, 0.40, 1.0),
    )
}

/// Ink for the "no data" hatch strokes — light enough to read over the
/// unmeasured fill at any zoom.
pub const HATCH: Color = Color::new(0.62, 0.64, 0.70, 0.85);

/// Ground a departed node still holds — reserved, not lost, not occupied.
///
/// Deliberately OUTSIDE the colour-blind funnel and outside the meaning
/// palette: a ghost carries no severity, no pressure and no coverage, so it
/// must not borrow a colour that does. A desaturated earth reads as bare land
/// under every overlay, which is exactly what it is.
pub fn ghost_land_pair() -> (Color, Color) {
    (
        Color::new(0.30, 0.30, 0.27, 1.0),
        Color::new(0.34, 0.34, 0.31, 1.0),
    )
}

/// The graticule — rules, row numbers and column letters.
///
/// **Scenery, not instrumentation.** It encodes no cluster state, so unlike
/// every meaning colour in this file it deliberately does NOT route through the
/// colour-blind funnel: there is nothing here to confuse with health, and a
/// reference frame that changed colour with the palette would be the one thing
/// on the map varying for no reason.
///
/// Faint by construction. A graticule that competes with terrain has failed at
/// being a reference — it is meant to be consulted, not read.
pub const GRATICULE: Color = Color::new(0.94, 0.92, 0.84, 0.20);
/// Row numbers and the reserved-column note.
///
/// Brighter than the rules: "recede" applies to the tessellation, which is
/// ambient, but a label the operator is meant to READ has to survive being
/// looked for. A hairline at a rule's alpha is a texture; a numeral at that
/// alpha is just illegible, which is one of §4.2's stated failure criteria.
pub const GRATICULE_INK: Color = Color::new(0.96, 0.94, 0.86, 0.55);
/// The column letter — a large mark over the column's visible span, so it
/// carries at any zoom while staying plainly behind the terrain.
pub const GRATICULE_MARK: Color = Color::new(0.98, 0.96, 0.88, 0.22);

/// Ground whose occupant just changed, in `steps` discrete tiers — brightest at
/// the moment of succession, fading to nothing at the end of the window.
///
/// **Quantised, not a smooth ramp.** A wave is perceived by its leading edge, and
/// a continuous fade has no edge — it has a gradient, which the eye reads as a
/// smear rather than as motion. Discrete steps put a boundary where the front is.
///
/// **A warm ochre, deliberately far from ghost grey.** The two are adjacent
/// during a surge — a wave leaves vacancies behind it and new occupancy ahead of
/// it — and they mean opposite things: ghost is *absence*, fresh is *recent
/// change*. Two shades of one colour would read as damage rather than motion.
///
/// Routes through the colour-blind funnel like every other meaning colour: this
/// encodes cluster state, so it is instrumentation, not scenery. Under the
/// red-green-safe palette the ochre moves to a violet that stays distinct from
/// both the steel-blue healthy land and the neutral ghost grey.
pub fn fresh_land_pair(freshness: f64, steps: u8) -> (Color, Color) {
    let steps = steps.max(1);
    let tier = fresh_tier(freshness, steps);
    let t = f64::from(tier) / f64::from(steps);
    // The tiers vary in INTENSITY within one hue, and the faintest is still
    // plainly the hue. The first version faded toward ghost grey, so the lowest
    // step landed 0.18 from it — the "two shades of the same thing" §2.2 warns
    // reads as damage rather than motion. The separability test caught it.
    let mix = |floor: f32, peak: f32| floor + (peak - floor) * t as f32;
    if colorblind() {
        (
            Color::new(mix(0.42, 0.66), mix(0.30, 0.38), mix(0.48, 0.76), 1.0),
            Color::new(mix(0.47, 0.72), mix(0.34, 0.44), mix(0.53, 0.82), 1.0),
        )
    } else {
        (
            Color::new(mix(0.52, 0.82), mix(0.42, 0.60), mix(0.24, 0.20), 1.0),
            Color::new(mix(0.58, 0.88), mix(0.47, 0.66), mix(0.28, 0.24), 1.0),
        )
    }
}

/// Which ageing tier a freshness falls in — `steps` (just changed hands) down to
/// 1 (nearly settled).
///
/// THE SINGLE AUTHORITY for that bucketing. The colour on the map and the words
/// in the SELECTION box both go through here, so a province cannot be painted at
/// one age and described at another. Two independent quantisations of the same
/// number is the drift this codebase keeps paying for.
pub fn fresh_tier(freshness: f64, steps: u8) -> u8 {
    let steps = steps.max(1);
    ((freshness * f64::from(steps)).ceil() as u8).clamp(1, steps)
}

/// How many tiers fresh ground fades through. Three: enough for a leading edge
/// and a body, few enough that the steps stay distinct when a many-partition
/// fleet has a lot of marked ground at once.
pub const FRESH_STEPS: u8 = 3;

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

/// Serialises the tests that mutate the process-global palette.
///
/// `COLOR_MODE` is one atomic for the whole process, so two palette tests
/// running in parallel can see each other's setting mid-assertion. This was
/// latent from the moment the palette became runtime-switchable and surfaced
/// only when an unrelated test changed the scheduling — the quantisation test
/// sampled a ramp that flipped palette halfway and counted four colours where
/// three were expected. A flake, not a wrong colour, which is worse: it would
/// have failed in CI at random and passed on every retry.
#[cfg(test)]
static PALETTE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take the palette lock, tolerating a poisoned mutex — a panicking test must
/// fail on its own assertion, not cascade into every other palette test.
#[cfg(test)]
fn palette_guard() -> std::sync::MutexGuard<'static, ()> {
    PALETTE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorblind_palette_swaps_meaning_greens_to_blue() {
        let _palette = super::palette_guard();
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

#[cfg(test)]
mod fresh_tests {
    use super::*;

    fn far(a: Color, b: Color) -> f32 {
        // Plain RGB distance. Crude, but the question is "can these be told
        // apart at a glance", and a perceptual metric would be precision the
        // decision does not need.
        ((a.r - b.r).powi(2) + (a.g - b.g).powi(2) + (a.b - b.b).powi(2)).sqrt()
    }

    /// **Fresh ground must be separable from ghost ground**, in every palette.
    ///
    /// They are adjacent during a surge — a wave leaves vacancies behind it and
    /// new occupancy ahead of it — and they mean opposite things: ghost is
    /// absence, fresh is recent change. §4.2 lists "fresh not separable from
    /// ghost" as a gate failure, so it is asserted rather than eyeballed.
    #[test]
    fn fresh_ground_is_separable_from_ghost_ground_in_both_palettes() {
        let _palette = super::palette_guard();
        for cb in [false, true] {
            set_colorblind(cb);
            let ghost = ghost_land_pair().0;
            // At full freshness, and at the faintest step that still marks.
            for f in [1.0, 1.0 / f64::from(FRESH_STEPS)] {
                let fresh = fresh_land_pair(f, FRESH_STEPS).0;
                let d = far(fresh, ghost);
                assert!(
                    d > 0.20,
                    "colorblind={cb} freshness={f}: fresh {fresh:?} is only {d:.3} from ghost"
                );
            }
            // And from healthy land, or the wave would be invisible on the
            // terrain it crosses.
            let healthy = iso_terrain_pair(NodeHealth::Healthy).0;
            let fresh = fresh_land_pair(1.0, FRESH_STEPS).0;
            assert!(
                far(fresh, healthy) > 0.20,
                "colorblind={cb}: fresh {fresh:?} is indistinguishable from healthy land"
            );
        }
        set_colorblind(false);
    }

    /// Quantisation is deterministic and actually quantises: the same inputs
    /// give the same step, and a range of freshness values collapses to exactly
    /// `FRESH_STEPS` distinct colours rather than a smooth ramp.
    #[test]
    fn ageing_is_quantised_into_distinct_steps() {
        let _palette = super::palette_guard();
        set_colorblind(false);
        let mut seen: Vec<(u8, u8, u8)> = (1..=200)
            .map(|i| {
                let c = fresh_land_pair(f64::from(i) / 200.0, FRESH_STEPS).0;
                (
                    (c.r * 255.0) as u8,
                    (c.g * 255.0) as u8,
                    (c.b * 255.0) as u8,
                )
            })
            .collect();
        seen.dedup();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            usize::from(FRESH_STEPS),
            "a continuous ramp, not steps: {} distinct colours",
            seen.len()
        );
        // Deterministic: same input, same output.
        assert_eq!(
            fresh_land_pair(0.5, FRESH_STEPS).0.r,
            fresh_land_pair(0.5, FRESH_STEPS).0.r
        );
    }

    /// A zero step count must not divide by zero or panic — the window setting
    /// already guards `0`, but the render side must not rely on that alone.
    #[test]
    fn a_zero_step_count_does_not_panic() {
        let _palette = super::palette_guard();
        set_colorblind(false);
        let _ = fresh_land_pair(1.0, 0);
        let _ = fresh_land_pair(0.0, 0);
    }
}
