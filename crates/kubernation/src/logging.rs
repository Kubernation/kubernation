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

/// Install async-signal-safe handlers for the fatal signals a NATIVE fault
/// raises (SIGSEGV / SIGBUS / SIGILL / SIGFPE / SIGABRT). The Rust panic hook
/// covers Rust panics; a fault in the windowing / GL / Objective-C layer kills
/// the process with NO Rust unwind — before this handler, such a crash left no
/// trace in the log (undiagnosable on a GUI app with no console). Each handler
/// writes ONE fixed line to a pre-opened log fd — `write(2)` on static bytes is
/// async-signal-safe; no allocation, no formatting, no locks — then re-raises
/// with the default disposition (`SA_RESETHAND`) so the OS crash pipeline
/// (crash report, exit status) still runs. `SA_ONSTACK` keeps stack-overflow
/// faults survivable on threads with an alternate stack (Rust installs one per
/// thread it spawns). Unix-only; a no-op elsewhere.
#[cfg(unix)]
pub fn install_fault_handler(log_path: &std::path::Path) {
    use std::os::fd::IntoRawFd;
    let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    else {
        return;
    };
    // Deliberately leaked — the fd must outlive every thread for the whole
    // process lifetime (a handler can fire at any moment).
    FAULT_FD.store(f.into_raw_fd(), std::sync::atomic::Ordering::Relaxed);
    unsafe {
        for sig in [
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGILL,
            libc::SIGFPE,
            libc::SIGABRT,
        ] {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = fault_handler as *const () as usize;
            act.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_RESETHAND;
            libc::sigaction(sig, &act, std::ptr::null_mut());
        }
    }
}

#[cfg(not(unix))]
pub fn install_fault_handler(_log_path: &std::path::Path) {}

#[cfg(unix)]
static FAULT_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

#[cfg(unix)]
extern "C" fn fault_handler(sig: libc::c_int, _: *mut libc::siginfo_t, _: *mut libc::c_void) {
    // Async-signal-safe ONLY: raw write(2) of static bytes. The tracing
    // machinery (allocation, mutexes) must NOT be touched here.
    let fd = FAULT_FD.load(std::sync::atomic::Ordering::Relaxed);
    if fd >= 0 {
        let name: &[u8] = match sig {
            libc::SIGSEGV => b"SIGSEGV (segmentation fault)",
            libc::SIGBUS => b"SIGBUS (bus error)",
            libc::SIGILL => b"SIGILL (illegal instruction)",
            libc::SIGFPE => b"SIGFPE (arithmetic fault)",
            libc::SIGABRT => b"SIGABRT (abort)",
            _ => b"unknown fatal signal",
        };
        let pre: &[u8] = b"\nFATAL native fault: ";
        let post: &[u8] = b" \xE2\x80\x94 terminating (this line is from the signal handler; a native-layer crash has no Rust backtrace)\n";
        unsafe {
            libc::write(fd, pre.as_ptr().cast(), pre.len());
            libc::write(fd, name.as_ptr().cast(), name.len());
            libc::write(fd, post.as_ptr().cast(), post.len());
        }
    }
    // SA_RESETHAND restored the default disposition — re-raise so the OS
    // terminates the process exactly as it would have without us.
    unsafe {
        libc::raise(sig);
    }
}

/// Session marker for abnormal-exit detection: `begin` reports (true) when a
/// marker from a previous session is still present — that session ended without
/// reaching the clean-exit path (native crash, kill, power loss) — then writes
/// a fresh marker. Call [`session_end`] on clean exit to remove it. This
/// catches even the faults no handler can (SIGKILL). Single-instance
/// assumption: two concurrent sessions share the marker, so a report can be
/// missed or spurious in that (rare, dev-only) case — a diagnostic aid, not a
/// guarantee.
pub fn session_begin(dir: &std::path::Path) -> bool {
    let p = dir.join("session.marker");
    let abnormal = p.exists();
    let _ = std::fs::write(&p, b"running\n");
    abnormal
}

/// Remove the session marker — the session is ending cleanly.
pub fn session_end(dir: &std::path::Path) {
    let _ = std::fs::remove_file(dir.join("session.marker"));
}

/// The state directory holding the log + session marker (`~/.local/state/kubernation`).
pub fn log_dir() -> PathBuf {
    state_dir().join("kubernation")
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
    // Windows: per-machine state/logs live under %LOCALAPPDATA% (non-roaming).
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(p);
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/state");
    }
    PathBuf::from(".")
}
