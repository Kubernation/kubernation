//! Color discipline: color encodes meaning, never decoration. A Running pod
//! is the *absence* of red. Saturated colors are reserved for things that
//! need the operator's attention. Mono mode carries the same meanings with
//! modifiers only.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ColorMode;
use crate::state::attention::Severity;
use crate::state::model::{NodeHealth, PodState};
use crate::util::fnv1a64;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    mono: bool,
}

impl Theme {
    pub fn new(mode: ColorMode) -> Self {
        Self {
            mono: mode == ColorMode::Mono,
        }
    }

    pub fn title(&self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    pub fn dim(&self) -> Style {
        if self.mono {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            Style::new().fg(Color::DarkGray)
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
            PodState::Ok => Style::new(), // running is the absence of red
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
            NodeHealth::Healthy => Style::new(),
            NodeHealth::Cordoned => Style::new().fg(Color::Yellow),
            NodeHealth::Pressure => Style::new().fg(Color::LightRed),
            NodeHealth::NotReady => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    /// Request-pressure gauge buckets; thresholds in `state::model`.
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
        } else {
            Style::new().fg(Color::DarkGray)
        }
    }

    pub fn selection(&self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }

    pub fn zone(&self) -> Style {
        if self.mono {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
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
