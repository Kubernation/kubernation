//! Color discipline: color encodes meaning, never decoration — and in the
//! default "civ" palette, *terrain*. Healthy infrastructure reads as living
//! land (greens, parchment chrome, blue ocean on the world panel) the way a
//! healthy Civ empire does, while saturated red/yellow stay reserved for
//! things that need the operator's attention: trouble must pop against
//! terrain, never compete with it. `color = "plain"` keeps the old
//! restrained palette; `mono` carries the same meanings with modifiers only.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ColorMode;
use crate::state::attention::Severity;
use crate::state::model::{NodeHealth, PodState};
use crate::util::fnv1a64;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    mono: bool,
    plain: bool,
}

impl Theme {
    pub fn new(mode: ColorMode) -> Self {
        Self {
            mono: mode == ColorMode::Mono,
            plain: mode == ColorMode::Plain,
        }
    }

    fn civ(&self) -> bool {
        !self.mono && !self.plain
    }

    /// Panel borders — parchment, like Civ's window chrome.
    pub fn chrome(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::DIM)
        } else if self.mono {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            Style::new()
        }
    }

    /// Panel titles — gold on the parchment chrome.
    pub fn title(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        }
    }

    /// Node names on tiles — white city labels, exactly like the board.
    pub fn tile_name(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::White)
        } else {
            Style::new()
        }
    }

    pub fn dim(&self) -> Style {
        if self.mono {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            Style::new().fg(Color::DarkGray)
        }
    }

    /// Fog of war — the unsynced world.
    pub fn fog(&self) -> Style {
        if self.mono {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM)
        }
    }

    pub fn severity(&self, s: Severity) -> Style {
        if self.mono {
            return match s {
                Severity::Critical => {
                    Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
                }
                Severity::Warning => Style::new().add_modifier(Modifier::BOLD),
                Severity::Info => Style::new().add_modifier(Modifier::DIM),
            };
        }
        match s {
            Severity::Critical => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            Severity::Warning => Style::new().fg(Color::Yellow),
            Severity::Info => Style::new().fg(Color::DarkGray),
        }
    }

    pub fn pod(&self, s: PodState) -> Style {
        if self.mono {
            return match s {
                PodState::Failing => Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                PodState::Ok => Style::new(),
                _ => Style::new().add_modifier(Modifier::DIM),
            };
        }
        match s {
            // Running is still the absence of *alarm*: on the civ palette a
            // healthy pod is muted terrain green, never saturated.
            PodState::Ok if self.civ() => Style::new().fg(Color::Green).add_modifier(Modifier::DIM),
            PodState::Ok => Style::new(),
            PodState::Starting => Style::new().fg(Color::Cyan),
            PodState::Pending => Style::new().fg(Color::DarkGray),
            PodState::Terminating => Style::new().fg(Color::DarkGray),
            PodState::Failing => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            PodState::Succeeded => Style::new().fg(Color::DarkGray),
        }
    }

    pub fn node(&self, h: NodeHealth) -> Style {
        if self.mono {
            return match h {
                NodeHealth::NotReady => {
                    Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
                }
                NodeHealth::Healthy => Style::new(),
                _ => Style::new().add_modifier(Modifier::BOLD),
            };
        }
        match h {
            // Healthy land is green; trouble keeps the reserved colors.
            NodeHealth::Healthy if self.civ() => Style::new().fg(Color::Green),
            NodeHealth::Healthy => Style::new(),
            NodeHealth::Cordoned => Style::new().fg(Color::Yellow),
            NodeHealth::Pressure => Style::new().fg(Color::LightRed),
            NodeHealth::NotReady => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    /// Request-pressure gauge buckets; thresholds in `state::model`.
    /// Calm gauges on the civ palette are food-storage green.
    pub fn ratio(&self, r: f64) -> Style {
        use crate::state::model::{PRESSURE_ELEVATED, PRESSURE_HIGH};
        if self.mono {
            return if r >= PRESSURE_HIGH {
                Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::new()
            };
        }
        if r >= PRESSURE_HIGH {
            Style::new().fg(Color::Red)
        } else if r >= PRESSURE_ELEVATED {
            Style::new().fg(Color::Yellow)
        } else if self.civ() {
            Style::new().fg(Color::Green).add_modifier(Modifier::DIM)
        } else {
            Style::new().fg(Color::DarkGray)
        }
    }

    pub fn selection(&self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }

    /// Zone headers — landmass names.
    pub fn zone(&self) -> Style {
        if self.mono {
            Style::new().add_modifier(Modifier::BOLD)
        } else if self.civ() {
            Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
        }
    }

    // --- The world map ---------------------------------------------------

    /// Open-sea wave marks on the main board.
    pub fn sea(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::Blue)
        } else {
            self.dim()
        }
    }

    /// Terrain texture on a province, keyed to its health. Calm land is
    /// quiet green; trouble textures use the reserved colors, dimmed so
    /// city badges still outrank them.
    pub fn terrain(&self, h: NodeHealth) -> Style {
        let base = match h {
            NodeHealth::Healthy if self.civ() => Style::new().fg(Color::Green),
            NodeHealth::Healthy => self.dim(),
            NodeHealth::Cordoned => Style::new().fg(Color::Yellow),
            NodeHealth::Pressure => Style::new().fg(Color::LightRed),
            NodeHealth::NotReady => Style::new().fg(Color::Red),
        };
        if self.mono {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            base.add_modifier(Modifier::DIM)
        }
    }

    /// Province (node) label row on the map.
    pub fn province(&self, h: NodeHealth) -> Style {
        match h {
            NodeHealth::Healthy => self.dim(),
            other => self.node(other),
        }
    }

    /// A healthy city's badge — settlements are white like their labels.
    pub fn city(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        }
    }

    /// Island sand.
    pub fn shore(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::DIM)
        } else {
            self.dim()
        }
    }

    /// A projected custom-resource structure.
    pub fn structure(&self) -> Style {
        if self.mono {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::LightCyan)
        }
    }

    // --- World panel (minimap): dark ocean, green land, white viewport ----

    /// Ocean fill behind the world panel cells.
    pub fn ocean(&self) -> Style {
        if self.civ() {
            Style::new().fg(Color::LightBlue).bg(Color::Blue)
        } else {
            Style::new()
        }
    }

    /// A world-panel node cell, colored by the node's worst state.
    pub fn land(&self, h: NodeHealth) -> Style {
        let base = match h {
            NodeHealth::Healthy if self.civ() => Style::new().fg(Color::LightGreen),
            NodeHealth::Healthy => self.dim(),
            other => self.node(other),
        };
        if self.civ() {
            base.bg(Color::Blue)
        } else {
            base
        }
    }

    /// Glyph + style for a world-panel cell. Civ palette: solid green land
    /// on ocean; plain palette keeps the quiet dot field.
    pub fn land_cell(&self, h: NodeHealth) -> (&'static str, Style) {
        if self.civ() {
            ("▪", self.land(h))
        } else {
            match h {
                NodeHealth::Healthy => ("·", self.dim()),
                other => ("▪", self.node(other)),
            }
        }
    }

    /// The viewport rectangle on the world panel.
    pub fn viewport(&self) -> Style {
        if self.civ() {
            Style::new()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else {
            self.zone()
        }
    }

    pub fn bar(&self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED)
    }

    pub fn event(&self, warning: bool) -> Style {
        if warning {
            self.severity(Severity::Warning)
        } else {
            self.dim()
        }
    }

    /// Pair sync badges: in-sync is silence, drift is yellow, a workload
    /// missing on the warm side is the dangerous one.
    pub fn sync(&self, state: &crate::state::pair::SyncState) -> Style {
        use crate::state::pair::SyncState;
        match state {
            SyncState::InSync => self.dim(),
            SyncState::Drift { .. } => self.severity(Severity::Warning),
            SyncState::OnlyHot => self.severity(Severity::Critical),
            SyncState::OnlyWarm => {
                if self.mono {
                    Style::new().add_modifier(Modifier::DIM)
                } else {
                    Style::new().fg(Color::Cyan)
                }
            }
        }
    }

    /// Namespace-ownership overlay palette: muted, no reds (red is reserved).
    pub fn namespace(&self, ns: &str) -> Style {
        if self.mono {
            return Style::new();
        }
        const PALETTE: [Color; 8] = [
            Color::Blue,
            Color::Green,
            Color::Cyan,
            Color::Magenta,
            Color::LightBlue,
            Color::LightGreen,
            Color::LightCyan,
            Color::LightMagenta,
        ];
        PALETTE[(fnv1a64(ns) % 8) as usize].into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civ_palette_keeps_alarm_colors_reserved() {
        let civ = Theme::new(ColorMode::Auto);
        // Healthy terrain is green, not the reserved colors.
        assert_eq!(civ.node(NodeHealth::Healthy).fg, Some(Color::Green));
        // Trouble is identical across palettes.
        let plain = Theme::new(ColorMode::Plain);
        assert_eq!(
            civ.severity(Severity::Critical),
            plain.severity(Severity::Critical)
        );
        assert_eq!(
            civ.node(NodeHealth::NotReady),
            plain.node(NodeHealth::NotReady)
        );
        // Plain keeps the old restraint: healthy carries no color at all.
        assert_eq!(plain.node(NodeHealth::Healthy), Style::new());
        // Mono never emits color.
        let mono = Theme::new(ColorMode::Mono);
        assert_eq!(mono.zone().fg, None);
        assert_eq!(mono.land(NodeHealth::NotReady).fg, None);
    }
}
