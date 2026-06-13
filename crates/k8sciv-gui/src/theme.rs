//! GUI palette and tiny text helpers. Same philosophy as the TUI theme:
//! terrain colors for the living world, saturated red/yellow reserved for
//! attention.

use k8sciv_core::state::attention::Severity;
use k8sciv_core::state::model::NodeHealth;
use k8sciv_core::util::fnv1a64;
use macroquad::prelude::*;

pub const OCEAN: Color = Color::new(0.06, 0.17, 0.30, 1.0);
pub const WAVE: Color = Color::new(0.11, 0.25, 0.41, 1.0);
pub const SAND: Color = Color::new(0.77, 0.70, 0.47, 1.0);
pub const SAND_DARK: Color = Color::new(0.62, 0.55, 0.34, 1.0);
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

pub fn terrain(h: NodeHealth) -> Color {
    match h {
        NodeHealth::Healthy => Color::new(0.30, 0.50, 0.24, 1.0),
        NodeHealth::Cordoned => Color::new(0.55, 0.50, 0.24, 1.0),
        NodeHealth::Pressure => Color::new(0.62, 0.42, 0.18, 1.0),
        NodeHealth::NotReady => Color::new(0.42, 0.15, 0.12, 1.0),
    }
}

/// Deterministic per-cell shade variation — the hand-painted mosaic.
pub fn terrain_cell(h: NodeHealth, wx: u16, wy: u16) -> Color {
    let base = terrain(h);
    let n = fnv1a64(&format!("{wx}:{wy}")) % 5;
    let delta = match n {
        0 => -0.035,
        1 => -0.015,
        2 => 0.0,
        3 => 0.02,
        _ => 0.04,
    };
    Color::new(
        (base.r + delta).clamp(0.0, 1.0),
        (base.g + delta * 1.4).clamp(0.0, 1.0),
        (base.b + delta).clamp(0.0, 1.0),
        1.0,
    )
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

/// macroquad's built-in font is ASCII-ish; swap the TUI glyph vocabulary
/// for plain characters so nothing renders as tofu.
pub fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '▸' => '>',
            '‼' => '!',
            '⊘' => 'o',
            '≣' => '=',
            c if c.is_ascii() || "·—–…×≠↔−≈".contains(c) => c,
            _ => '?',
        })
        .collect()
}

/// Chip color for a pair sync state.
pub fn sync_color(state: &k8sciv_core::state::pair::SyncState) -> Color {
    use k8sciv_core::state::pair::SyncState;
    match state {
        SyncState::InSync => Color::new(0.50, 0.65, 0.45, 1.0),
        SyncState::Drift { .. } => WARN,
        SyncState::OnlyHot => CRIT,
        SyncState::OnlyWarm => STRUCT,
    }
}
