pub mod cli;

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// The Civ palette: parchment chrome, green terrain, blue ocean. Named
    /// ANSI colors only — safe on 256-color and truecolor terminals alike.
    #[default]
    Auto,
    /// The pre-civ restrained palette: healthy state carries no color.
    Plain,
    /// No color at all; meaning carried by modifiers (bold/reverse/dim).
    Mono,
}

/// Loaded from `~/.config/kubernation/config.toml` when present; every field has
/// a default so the file is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// UI coalescing tick in milliseconds (world changes are re-derived at
    /// this cadence; input is always handled immediately).
    pub tick_ms: Option<u64>,
    pub color: ColorMode,
    /// Start with the attention panel expanded.
    pub attention_expanded: bool,
    /// Warm-standby context to observe alongside the hot cluster
    /// (`--warm` overrides).
    pub warm_context: Option<String>,
    /// CRDs to project onto the world map as island structures, by CRD
    /// name, e.g. "gizmos.example.com" (`--project` adds more).
    pub projections: Vec<String>,
}

impl Config {
    pub fn tick_ms(&self) -> u64 {
        self.tick_ms.unwrap_or(250).clamp(50, 5000)
    }

    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("kubernation").join("config.toml"))
    }

    pub fn load() -> color_eyre::Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&raw)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert_eq!(c.tick_ms(), 250);
        assert_eq!(c.color, ColorMode::Auto);
    }

    #[test]
    fn parses_partial_config() {
        let c: Config = toml::from_str("tick_ms = 100\ncolor = \"mono\"").unwrap();
        assert_eq!(c.tick_ms(), 100);
        assert_eq!(c.color, ColorMode::Mono);
        assert!(!c.attention_expanded);
    }
}
