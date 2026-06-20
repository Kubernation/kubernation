//! The Oracle endpoint config file — the project's FIRST persisted config (today
//! the app writes only one-shot exports + a log file). It holds named endpoint
//! profiles and, by the operator's explicit per-profile opt-in, their API tokens
//! in PLAINTEXT. So:
//!
//! - the directory is created `0700` and the file `0600` AT CREATE time (not
//!   create-then-chmod, which would leave a world-readable window under the
//!   default umask);
//! - writes are atomic (temp file in the same dir + fsync + rename), so a crash
//!   mid-write can't truncate the only copy;
//! - a corrupt file is renamed aside (never deleted — a recoverable token may be
//!   inside) and we degrade to the built-in default rather than panicking.
//!
//! The pure schema + resolver live in `kubernation_core::state::oracle_config`;
//! this module is the only thing that touches disk.

use std::io;
use std::path::PathBuf;

use kubernation_core::state::oracle_config::OracleConfigFile;

/// `$XDG_CONFIG_HOME` else `$HOME/.config` else the cwd — the sibling of
/// `logging.rs::state_dir` (config is user-authored; state/logs is separate).
fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(p).join("kubernation");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".config/kubernation");
    }
    PathBuf::from(".")
}

/// The config file path: `<config_dir>/oracle.json`.
pub fn config_path() -> PathBuf {
    config_dir().join("oracle.json")
}

/// Load the config. NEVER errors to the caller: a missing file yields the default
/// (which resolves to the built-in local profile); a corrupt file is renamed
/// aside (`oracle.json.corrupt-<unix-secs>`) and we warn + return the default.
pub fn load() -> OracleConfigFile {
    let path = config_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return OracleConfigFile::default(),
        Err(e) => {
            tracing::warn!(
                "oracle config unreadable ({}): {e}; using defaults",
                path.display()
            );
            return OracleConfigFile::default();
        }
    };
    match serde_json::from_slice::<OracleConfigFile>(&bytes) {
        Ok(f) => {
            if f.version > kubernation_core::state::oracle_config::CONFIG_VERSION {
                tracing::warn!(
                    "oracle config version {} is newer than this build understands; loading best-effort",
                    f.version
                );
            }
            f
        }
        Err(e) => {
            let aside = path.with_extension(format!("json.corrupt-{}", unix_secs()));
            let _ = std::fs::rename(&path, &aside);
            tracing::warn!(
                "oracle config was corrupt ({e}); moved to {} and using defaults",
                aside.display()
            );
            OracleConfigFile::default()
        }
    }
}

/// Persist the config atomically with hardened permissions. Returns the saved
/// path on success (for the toast) or an io error.
pub fn save(file: &OracleConfigFile) -> io::Result<PathBuf> {
    let path = config_path();
    let dir = path.parent().unwrap_or(&path).to_path_buf();
    create_dir_0700(&dir)?;

    let tmp = path.with_extension("json.tmp");
    // Clear any stale temp from a prior crash so the O_EXCL create below can't
    // fail spuriously (and so a leftover symlink can't be followed).
    let _ = std::fs::remove_file(&tmp);
    let json = serde_json::to_vec_pretty(file)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_file_0600(&tmp, &json)?;
    // Atomic swap into place (same filesystem).
    std::fs::rename(&tmp, &path)?;
    harden_perms(&path);
    Ok(path)
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(unix)]
fn create_dir_0700(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.exists() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_dir_0700(dir: &std::path::Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn write_file_0600(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // create_new (O_EXCL) — fail rather than follow a pre-existing symlink at the
    // temp path, which could redirect the plaintext token to an attacker-chosen
    // file. The caller removes a stale temp first so this can't fail spuriously.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600) // perms set AT CREATE — no world-readable window
        .open(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_file_0600(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    f.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn harden_perms(path: &std::path::Path) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn harden_perms(_path: &std::path::Path) {}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
