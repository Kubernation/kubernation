//! Reading and writing saved layouts — the only file that touches disk for
//! them.
//!
//! The pure half (the DTO, the version check, the identity rules) is
//! `state/layout_store.rs`, mirroring the split between `state/oracle_config.rs`
//! and `oracle_config_io.rs`. Mechanics follow `prefs.rs` and
//! `oracle_config_io.rs` exactly: atomic temp + rename so a crash mid-write
//! cannot truncate a layout, and a corrupt file renamed aside rather than
//! deleted.
//!
//! **A layout that cannot be read is a fresh map, not a crash** — and the caller
//! is told which, because the user is about to notice their world changed and
//! deserves to know why.
//!
//! Under `~/.local/state/kubernation/layouts/`, not beside `prefs.json`: a
//! layout is derived state about a cluster rather than a user-authored
//! preference, it grows, and one file per context means a corrupt layout for one
//! cluster cannot take out the others.

use std::io;
use std::path::{Path, PathBuf};

use kubernation_core::state::layout::Layout;
use kubernation_core::state::layout_store::{
    LoadRefusal, StoredLayout, Trust, from_stored, to_stored,
};

/// `$XDG_STATE_HOME` else `$HOME/.local/state` else the cwd — the sibling of
/// `logging::log_dir` (state, not config).
fn state_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(p).join("kubernation");
    }
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(p).join("kubernation");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/state/kubernation");
    }
    PathBuf::from(".")
}

pub fn layouts_dir() -> PathBuf {
    state_dir().join("layouts")
}

/// One file per context. Context names come from a kubeconfig and can contain
/// anything — an EKS context is a full ARN with slashes and colons — so the name
/// is sanitised into a single path-safe component rather than trusted as one.
/// Same treatment `postmortem`'s filename gives a context, for the same reason.
pub fn layout_path(context: &str) -> PathBuf {
    let safe: String = context
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_matches('.').to_string();
    let name = if safe.is_empty() {
        "unnamed".to_string()
    } else {
        safe
    };
    layouts_dir().join(format!("{name}.json"))
}

/// What happened when a layout was asked for. The caller reports this — a map
/// that silently changed is the failure mode this workstream exists to remove,
/// and arriving at it through the load path would be no better.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// No file yet — an ordinary first run, nothing to announce.
    Fresh,
    /// Loaded, with how much the identity check is worth.
    Restored { slots: usize, trust: Trust },
    /// A file existed and was not usable. The map starts fresh, and the reason
    /// is worth surfacing.
    Discarded(String),
}

impl Loaded {
    /// A line for the user, or `None` when there is nothing worth saying.
    pub fn announce(&self) -> Option<String> {
        match self {
            Loaded::Fresh => None,
            Loaded::Restored {
                trust: Trust::Verified,
                ..
            } => None,
            Loaded::Restored {
                slots,
                trust: Trust::Unverified,
            } => Some(format!(
                "restored {slots} map positions — could not confirm this is the same cluster"
            )),
            Loaded::Discarded(why) => Some(why.clone()),
        }
    }
}

/// Load the layout for a context. Never errors to the caller.
///
/// `fingerprint` is the cluster-scoped UID observed now, or `None` when it could
/// not be read — which is a supported state, not a failure. See
/// `layout_store::from_stored` for why absent is not mismatched.
pub fn load(context: &str, fingerprint: Option<&str>) -> (Layout, Loaded) {
    let path = layout_path(context);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Layout::default(), Loaded::Fresh),
        Err(e) => {
            tracing::warn!("could not read {}: {e}", path.display());
            return (
                Layout::default(),
                Loaded::Discarded(format!(
                    "could not read the saved map ({e}) — starting fresh"
                )),
            );
        }
    };
    let stored: StoredLayout = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(e) => {
            // Renamed aside, never deleted: a layout is cheap to rebuild but the
            // file may still be diagnosable, and deleting the evidence of a bug
            // is how the bug survives.
            let aside = path.with_extension(format!("json.corrupt-{}", unix_secs()));
            let _ = std::fs::rename(&path, &aside);
            tracing::warn!("saved map was corrupt ({e}); moved to {}", aside.display());
            return (
                Layout::default(),
                Loaded::Discarded(format!(
                    "the saved map was unreadable ({e}) — starting fresh; the old file is at {}",
                    aside.display()
                )),
            );
        }
    };
    match from_stored(stored, fingerprint) {
        Ok((layout, trust)) => {
            let slots = layout.slots().count();
            (layout, Loaded::Restored { slots, trust })
        }
        Err(refusal) => {
            if refusal == LoadRefusal::DifferentCluster {
                // Keep the old one: the context may be pointed back, and
                // overwriting would destroy a layout the user can still use.
                let aside = path.with_extension(format!("json.other-{}", unix_secs()));
                let _ = std::fs::rename(&path, &aside);
            }
            (Layout::default(), Loaded::Discarded(refusal.describe()))
        }
    }
}

/// Write the layout for a context. Atomic: temp file in the same directory, then
/// rename.
pub fn save(context: &str, layout: &Layout, fingerprint: Option<&str>) -> io::Result<PathBuf> {
    let path = layout_path(context);
    let dir = layouts_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(&to_stored(layout, fingerprint))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("layout")
    ));
    let _ = std::fs::remove_file(&tmp); // a stale temp from a crashed write
    write_then_sync(&tmp, &json)?;
    std::fs::rename(&tmp, &path)?; // atomic swap on the same filesystem
    Ok(path)
}

fn write_then_sync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
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

    /// EKS context names are ARNs. A path built from one naively would try to
    /// create directories, or escape the layouts directory entirely.
    #[test]
    fn a_context_name_becomes_one_path_safe_file() {
        let arn = "arn:aws:eks:eu-west-1:123456789012:cluster/prod";
        let p = layout_path(arn);
        assert_eq!(p.parent(), Some(layouts_dir().as_path()));
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains('/'), "{name}");
        assert!(!name.contains(':'), "{name}");
        assert!(name.ends_with(".json"));

        // Traversal cannot escape. The invariant is that the result is ONE
        // component inside the layouts directory — a `..` surviving inside a
        // longer name (`_.._etc_passwd`) is an odd filename, not a traversal,
        // because the separators are gone.
        let p = layout_path("../../etc/passwd");
        assert_eq!(p.parent(), Some(layouts_dir().as_path()));
        assert_eq!(
            p.components().count(),
            layouts_dir().components().count() + 1
        );
        // A name that is nothing but dots would otherwise become `..` itself.
        assert_eq!(
            layout_path("..").file_name().unwrap().to_str().unwrap(),
            "unnamed.json"
        );

        // A name with nothing usable still yields a file rather than a
        // directory path ending in `.json`.
        assert_eq!(
            layout_path("///").file_name().unwrap().to_str().unwrap(),
            "___.json"
        );
    }

    /// Distinct contexts must not collide after sanitising — otherwise two
    /// clusters share one layout and each overwrites the other's map.
    #[test]
    fn two_similar_context_names_do_not_share_a_file() {
        assert_ne!(layout_path("prod-eu"), layout_path("prod-us"));
        assert_ne!(layout_path("kind-a"), layout_path("kind-b"));
    }

    /// A missing file is an ordinary first run and must announce nothing.
    #[test]
    fn an_absent_layout_is_a_quiet_fresh_start() {
        let (layout, outcome) = load("no-such-context-xyzzy-4821", None);
        assert!(layout.is_empty());
        assert_eq!(outcome, Loaded::Fresh);
        assert_eq!(outcome.announce(), None);
    }

    /// An unverified restore is worth saying out loud; a verified one is not.
    #[test]
    fn only_the_outcomes_worth_reporting_announce_themselves() {
        assert_eq!(
            Loaded::Restored {
                slots: 12,
                trust: Trust::Verified
            }
            .announce(),
            None
        );
        let unverified = Loaded::Restored {
            slots: 12,
            trust: Trust::Unverified,
        }
        .announce()
        .expect("unverified restores are announced");
        assert!(unverified.contains("12"));
        assert!(Loaded::Discarded("because".into()).announce().is_some());
    }
}
