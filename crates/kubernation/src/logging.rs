//! Diagnostics to a file under the XDG state dir. `kubernation-core` emits
//! `tracing` events (watch reconnects, discovery failures, metrics polls, …);
//! without a subscriber they're dropped, so the client installs one at startup.
//! A file (not stderr) keeps a launched-from-a-launcher window's logs somewhere
//! findable; `RUST_LOG` overrides the `--log-level` default for ad-hoc debugging.

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

/// Initialise the file logger; returns the log path (surfaced on errors).
pub fn init(level: &str) -> std::io::Result<PathBuf> {
    let dir = state_dir().join("kubernation");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("kubernation.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    // RUST_LOG wins over --log-level so ad-hoc debugging needs no flag change.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(move || file.try_clone().expect("clone log file handle"))
        .with_ansi(false)
        .init();
    Ok(path)
}

fn state_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(p);
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/state");
    }
    PathBuf::from(".")
}
