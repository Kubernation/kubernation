//! Persisted UI preferences — a small `~/.config/kubernation/prefs.json` restored
//! at the next launch so you don't re-set them every run. **CLI flags always win**
//! over the saved value. NON-SECRET, NON-CLUSTER convenience state ONLY: the
//! colour-blind palette choice, the last map overlay and the map style — never
//! any cluster data
//! (no cross-run cluster state) and no secrets (the Oracle token lives in its own
//! file). Written atomically (temp + rename) so a crash mid-write can't truncate
//! it; a corrupt file is renamed aside (never deleted) and we fall back to defaults.
//!
//! Deliberately small: context + namespace-filter persistence is deferred — they
//! create a "pin the saved value vs. follow your kubeconfig / show an empty world"
//! tension that wants its own decision, not a silent default.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Bump on an incompatible schema change; a newer file loads best-effort.
pub const PREFS_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub version: u32,
    /// The colour-blind-safe palette (red-green safe).
    pub colorblind: bool,
    /// Last map overlay, in its `--overlay` string form (e.g. "pressure"); `None`
    /// or unrecognised → the default terrain view.
    pub overlay: Option<String>,
    /// Last map style, in its `--map-style` string form ("plain" / "relief");
    /// `None` or unrecognised → the default plain chart.
    pub map_style: Option<String>,
    /// Draw the reference frame (column letters + row numbers). `None` means no
    /// preference expressed, and the frame stays off until asked for.
    #[serde(default)]
    pub graticule: Option<bool>,
    /// How long ground stays marked after it changes hands, in minutes.
    ///
    /// `Some(0)` means **never mark** and is a real supported value, not a
    /// degenerate one. `None` means the operator has expressed no preference and
    /// the declared default applies.
    #[serde(default)]
    pub fresh_minutes: Option<u64>,
}

/// How long ground stays marked after a node replaced its predecessor.
///
/// **A judgment, not a measurement**, and said so where a user can see it: long
/// enough to survive stepping away from the screen during a rollout, and gone by
/// the next working day so a morning map is not covered in marks from yesterday's
/// routine churn. Nothing measured picks this number, and it is a setting
/// precisely because it is arguable.
pub const DEFAULT_FRESH_MINUTES: u64 = 60;

/// `$XDG_CONFIG_HOME` else `$HOME/.config` else the cwd — the same dir as
/// `oracle.json` (kept independent so prefs never touch the token-bearing module).
fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(p).join("kubernation");
    }
    // Windows: user config lives under %APPDATA% (roaming).
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("APPDATA") {
        return PathBuf::from(p).join("kubernation");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".config/kubernation");
    }
    PathBuf::from(".")
}

pub fn config_path() -> PathBuf {
    config_dir().join("prefs.json")
}

/// Load prefs. NEVER errors to the caller: a missing file → the default; a corrupt
/// file is renamed aside (`prefs.json.corrupt-<unix-secs>`) and we return defaults.
pub fn load() -> Prefs {
    let path = config_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Prefs::default(),
        Err(e) => {
            tracing::warn!("prefs unreadable ({}): {e}; using defaults", path.display());
            return Prefs::default();
        }
    };
    match serde_json::from_slice::<Prefs>(&bytes) {
        Ok(p) => p,
        Err(e) => {
            let aside = path.with_extension(format!("json.corrupt-{}", unix_secs()));
            let _ = std::fs::rename(&path, &aside);
            tracing::warn!(
                "prefs were corrupt ({e}); moved to {} and using defaults",
                aside.display()
            );
            Prefs::default()
        }
    }
}

/// Persist prefs (best-effort — a failure is logged, never fatal; prefs are
/// convenience). Atomic: write a temp file then rename into place.
pub fn save(p: &Prefs) {
    if let Err(e) = try_save(p) {
        tracing::warn!("could not save prefs: {e}");
    }
}

fn try_save(p: &Prefs) -> io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = config_path();
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp); // clear a stale temp from a prior crash
    let json =
        serde_json::to_vec_pretty(p).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &path)?; // atomic swap on the same filesystem
    Ok(())
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let p = Prefs {
            graticule: Some(true),
            version: PREFS_VERSION,
            colorblind: true,
            overlay: Some("cost".into()),
            map_style: Some("relief".into()),
            fresh_minutes: Some(0),
        };
        let json = serde_json::to_vec(&p).unwrap();
        let back: Prefs = serde_json::from_slice(&json).unwrap();
        assert!(back.colorblind);
        assert_eq!(back.overlay.as_deref(), Some("cost"));
        assert_eq!(back.map_style.as_deref(), Some("relief"));
        assert_eq!(back.version, PREFS_VERSION);
        // `0` means NEVER MARK and is a real supported value — it has to survive
        // the round trip as `Some(0)` and not collapse into "unset", which would
        // silently restore the default window.
        assert_eq!(back.fresh_minutes, Some(0));
    }

    #[test]
    fn missing_and_partial_fields_default() {
        // An empty object → all defaults (forward/backward compat).
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(!p.colorblind && p.overlay.is_none() && p.map_style.is_none());
        // A partial object keeps the present field, defaults the rest.
        let p: Prefs = serde_json::from_str(r#"{"colorblind":true}"#).unwrap();
        assert!(p.colorblind && p.overlay.is_none() && p.map_style.is_none());
        // An old file with no map_style loads → None → the default style (so no
        // PREFS_VERSION bump was needed for this field).
        let p: Prefs = serde_json::from_str(r#"{"overlay":"cost"}"#).unwrap();
        assert!(p.map_style.is_none());
        // Garbage is rejected (the loader renames it aside → defaults).
        assert!(serde_json::from_str::<Prefs>("not json").is_err());
    }
}
