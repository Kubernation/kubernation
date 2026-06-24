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
        // Per-write clone; on a clone failure (fd exhaustion mid-frame) fall back to
        // a sink rather than panicking inside the logging machinery (which could
        // take down whichever thread happened to emit the event).
        .with_writer(move || -> Box<dyn std::io::Write> {
            match file.try_clone() {
                Ok(f) => Box::new(f),
                Err(_) => Box::new(std::io::sink()),
            }
        })
        .with_ansi(false)
        .init();
    Ok(path)
}

/// Install a global panic hook that logs the panic (thread · location · message)
/// to the file log BEFORE unwinding, then chains the default hook (stderr). A
/// macroquad GUI launched from a launcher has no terminal, so an un-logged panic
/// is undiagnosable — this makes every crash land in `kubernation.log`. The hook
/// is process-wide, so it covers the render thread AND the net thread.
pub fn install_panic_hook() {
    // Installed from `main` → this IS the render thread. Any panic OFF it means a
    // background thread died — the net world loop runs on `kn-net`, but its tokio
    // reflectors/tasks run on `tokio-runtime-worker` threads (the runtime is
    // multi-threaded), so we key on the THREAD ID, not the name, to catch them all.
    let render_id = std::thread::current().id();
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".into());
        let cur = std::thread::current();
        let thread = cur.name().unwrap_or("unnamed").to_string();
        tracing::error!(thread = %thread, location = %loc, "PANIC: {}", panic_message(info));
        // A panic on any background thread freezes the world silently — flag it so
        // the GUI shows a fatal banner (the render thread keeps running). A
        // render-thread panic aborts the app anyway, so there is no banner to show.
        if cur.id() != render_id {
            crate::net::NET_PANICKED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        default(info);
    }));
}

/// Extract the human-readable message from a panic payload.
fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let p = info.payload();
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".into()
    }
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
