//! GUI palette and tiny text helpers. Same philosophy as the TUI theme:
//! terrain colors for the living world, saturated red/yellow reserved for
//! attention.

use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::NodeHealth;
use macroquad::prelude::*;

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
        NodeHealth::Healthy => (
            Color::new(0.30, 0.49, 0.25, 1.0),
            Color::new(0.35, 0.55, 0.29, 1.0),
        ),
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
            c if c.is_ascii() || "·—–…×≠↔−≈✓✗".contains(c) => c,
            _ => '?',
        })
        .collect()
}

/// Chip color for a pair sync state.
pub fn sync_color(state: &kubernation_core::state::pair::SyncState) -> Color {
    use kubernation_core::state::pair::SyncState;
    match state {
        SyncState::InSync => Color::new(0.50, 0.65, 0.45, 1.0),
        SyncState::Drift { .. } => WARN,
        SyncState::OnlyHot => CRIT,
        SyncState::OnlyWarm => STRUCT,
    }
}
