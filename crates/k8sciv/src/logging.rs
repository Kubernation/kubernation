use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

/// Diagnostics go to a file, never stderr — stderr would corrupt the TUI.
/// Returns the log path so it can be surfaced to the user on errors.
pub fn init(level: &str) -> color_eyre::Result<PathBuf> {
    let dir = state_dir().join("k8sciv");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("k8sciv.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    // RUST_LOG wins over --log-level so ad-hoc debugging needs no flag changes.
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
