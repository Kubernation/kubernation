//! PURE Oracle endpoint configuration: named profiles (local Ollama + remote /
//! corporate endpoints), the active selection, and the precedence resolver. No
//! I/O — the bin owns the file (load/save with 0600 perms); this module is the
//! serde schema + the pure logic, so the interesting parts are unit-testable
//! without a disk.
//!
//! SAFETY POSTURE (load-bearing):
//! - The persisted token is plaintext-by-explicit-opt-in (the user chose to store
//!   it). `Profile` has a MANUAL redacting `Debug` so a stray log never prints it.
//! - The `Endpoint` (Local/Remote) classification is NEVER persisted or trusted —
//!   `to_llm_config` ALWAYS recomputes it via `endpoint_kind`/`host_is_local`, so
//!   a tampered file marking `api.openai.com` "Local" cannot bypass the egress arm.
//! - `KUBERNATION_LLM_TOKEN` (read in the bin) is a fallback/override that is
//!   NEVER persisted and does NOT override a deliberately-saved profile token.

use serde::{Deserialize, Serialize};

use super::oracle::host_is_local;
use crate::k8s::oracle_client::{Endpoint, LlmConfig};

/// Default endpoint — a local Ollama (OpenAI-compatible at `/v1`, so the wire
/// endpoint is `http://localhost:11434/v1/chat/completions`).
pub const DEFAULT_LLM_URL: &str = "http://localhost:11434/v1";
/// The seed default model. Must be a tag the local Ollama has pulled (else the
/// consult 404s with an actionable "model not found"); override with `--llm-model`.
pub const DEFAULT_LLM_MODEL: &str = "qwen3.5:35b";

/// Map a base URL to the egress classification. Always recomputed from the URL —
/// the result is never persisted (fail-closed: an unknown/garbled host is Remote).
pub fn endpoint_kind(base_url: &str) -> Endpoint {
    if host_is_local(base_url) {
        Endpoint::Local
    } else {
        Endpoint::Remote
    }
}

/// One saved endpoint. `name` is the unique key shown in the picker.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// OpenAI-compatible base, e.g. `http://localhost:11434/v1`.
    pub base_url: String,
    pub model: String,
    /// The PERSISTED token, by explicit per-profile opt-in. `None` ⇒ fall back to
    /// the env token (or none). Never logged; redacted by the manual `Debug`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

// Manual Debug — NEVER print the token (the file logger would capture it).
impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field(
                "token",
                &self.token.as_ref().map(|_| "<set>").unwrap_or("<unset>"),
            )
            .finish()
    }
}

/// The on-disk config document.
#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct OracleConfigFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// The active profile by name. `None` ⇒ the built-in local default.
    #[serde(default)]
    pub active: Option<String>,
}

fn default_version() -> u32 {
    1
}

/// The current schema version this build writes.
pub const CONFIG_VERSION: u32 = 1;

/// Where the active `LlmConfig` came from — for the honest "from: …" setup line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActiveSource {
    /// A transient overlay from `--llm-url` / `--llm-model` (never persisted).
    Flags,
    /// The active persisted profile.
    Profile,
    /// The built-in local default (no profiles configured).
    BuiltinDefault,
}

/// Build a runtime `LlmConfig`, ALWAYS recomputing the endpoint kind from the URL
/// (never trusting a stored class). An empty token collapses to `None`.
fn build_config(base_url: &str, model: &str, token: Option<String>) -> LlmConfig {
    let base_url = base_url.trim_end_matches('/').to_string();
    let api_key = token.filter(|s| !s.is_empty());
    let endpoint = endpoint_kind(&base_url);
    LlmConfig {
        base_url,
        model: model.to_string(),
        api_key,
        endpoint,
    }
}

impl Profile {
    /// The built-in local-Ollama profile used when nothing is configured.
    pub fn local_default() -> Profile {
        Profile {
            name: "local Ollama".into(),
            base_url: DEFAULT_LLM_URL.into(),
            model: DEFAULT_LLM_MODEL.into(),
            token: None,
        }
    }

    /// Resolve this profile to a runtime config. The profile's saved token wins;
    /// `env_token` fills in only when the profile has none.
    pub fn to_llm_config(&self, env_token: Option<&str>) -> LlmConfig {
        let token = self.token.clone().or_else(|| env_token.map(str::to_string));
        build_config(&self.base_url, &self.model, token)
    }
}

impl OracleConfigFile {
    /// The active profile object, if `active` names one that exists.
    pub fn active_profile(&self) -> Option<&Profile> {
        let name = self.active.as_ref()?;
        self.profiles.iter().find(|p| &p.name == name)
    }
}

/// PURE precedence resolver (highest first):
/// 1. **Flags** (`--llm-url`/`--llm-model`) — a transient, never-persisted
///    overlay on the active-or-default profile. If a flag URL is given (a new
///    endpoint) the token is env-only — a saved profile token is NEVER sent to a
///    flag-specified URL; if only the model flag is given, the active profile's
///    endpoint + token are kept.
/// 2. **Active profile** — its saved token wins, env fills a `None`.
/// 3. **Built-in local default** — token from env (or none).
pub fn resolve_active(
    file: &OracleConfigFile,
    flag_url: Option<&str>,
    flag_model: Option<&str>,
    env_token: Option<&str>,
) -> (LlmConfig, ActiveSource) {
    if flag_url.is_some() || flag_model.is_some() {
        let base = file
            .active_profile()
            .cloned()
            .unwrap_or_else(Profile::local_default);
        let base_url = flag_url.unwrap_or(&base.base_url);
        let model = flag_model.unwrap_or(&base.model);
        // A flag URL means a (possibly new) endpoint: token = env only, never a
        // saved token sent to a URL the operator typed on the command line.
        let token = if flag_url.is_some() {
            env_token.map(str::to_string)
        } else {
            base.token.clone().or_else(|| env_token.map(str::to_string))
        };
        return (build_config(base_url, model, token), ActiveSource::Flags);
    }
    if let Some(p) = file.active_profile() {
        return (p.to_llm_config(env_token), ActiveSource::Profile);
    }
    (
        Profile::local_default().to_llm_config(env_token),
        ActiveSource::BuiltinDefault,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str, url: &str) -> Profile {
        Profile {
            name: name.into(),
            base_url: url.into(),
            model: "gpt-4o".into(),
            token: Some("sk-SECRET".into()),
        }
    }

    #[test]
    fn serde_round_trips_and_tolerates_missing_fields() {
        let file = OracleConfigFile {
            version: 1,
            profiles: vec![
                Profile::local_default(),
                remote("corp", "https://api.corp/v1"),
            ],
            active: Some("corp".into()),
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: OracleConfigFile = serde_json::from_str(&json).unwrap();
        assert_eq!(file, back);

        // A minimal/old document loads via #[serde(default)].
        let sparse: OracleConfigFile = serde_json::from_str(r#"{"profiles":[]}"#).unwrap();
        assert_eq!(sparse.version, 1);
        assert!(sparse.active.is_none());

        // A future version still loads best-effort (the loader warns, never panics).
        let future: OracleConfigFile =
            serde_json::from_str(r#"{"version":99,"profiles":[]}"#).unwrap();
        assert_eq!(future.version, 99);
    }

    #[test]
    fn a_tokenless_profile_omits_the_token_key() {
        let json = serde_json::to_string(&Profile::local_default()).unwrap();
        assert!(
            !json.contains("token"),
            "token: None must not serialize a key"
        );
    }

    #[test]
    fn profile_debug_redacts_the_token() {
        let dbg = format!("{:?}", remote("corp", "https://api.corp/v1"));
        assert!(
            !dbg.contains("sk-SECRET"),
            "token must never appear in Debug"
        );
        assert!(dbg.contains("<set>"));
    }

    #[test]
    fn endpoint_is_always_recomputed_never_trusted() {
        // Even though no Endpoint is stored, a remote URL classifies Remote.
        let cfg = remote("corp", "https://api.openai.com/v1").to_llm_config(None);
        assert_eq!(cfg.endpoint, Endpoint::Remote);
        let local = Profile::local_default().to_llm_config(None);
        assert_eq!(local.endpoint, Endpoint::Local);
    }

    #[test]
    fn resolve_precedence_flags_then_profile_then_default() {
        let file = OracleConfigFile {
            version: 1,
            profiles: vec![remote("corp", "https://api.corp/v1")],
            active: Some("corp".into()),
        };
        // No flags ⇒ active profile, its saved token wins.
        let (cfg, src) = resolve_active(&file, None, None, Some("env-tok"));
        assert_eq!(src, ActiveSource::Profile);
        assert_eq!(cfg.base_url, "https://api.corp/v1");
        assert_eq!(cfg.api_key.as_deref(), Some("sk-SECRET"));
        assert_eq!(cfg.endpoint, Endpoint::Remote);

        // Only --llm-model ⇒ Flags overlay on the active endpoint, KEEPS its token.
        let (cfg, src) = resolve_active(&file, None, Some("gpt-4o-mini"), Some("env-tok"));
        assert_eq!(src, ActiveSource::Flags);
        assert_eq!(cfg.model, "gpt-4o-mini");
        assert_eq!(cfg.base_url, "https://api.corp/v1");
        assert_eq!(cfg.api_key.as_deref(), Some("sk-SECRET"));

        // A flag URL ⇒ token is env ONLY (never the saved token sent to a flag URL).
        let (cfg, src) = resolve_active(
            &file,
            Some("http://localhost:11434/v1"),
            None,
            Some("env-tok"),
        );
        assert_eq!(src, ActiveSource::Flags);
        assert_eq!(cfg.base_url, "http://localhost:11434/v1");
        assert_eq!(cfg.api_key.as_deref(), Some("env-tok"));
        assert_eq!(cfg.endpoint, Endpoint::Local);

        // Empty file ⇒ built-in default.
        let empty = OracleConfigFile::default();
        let (cfg, src) = resolve_active(&empty, None, None, None);
        assert_eq!(src, ActiveSource::BuiltinDefault);
        assert_eq!(cfg.base_url, DEFAULT_LLM_URL);
        assert_eq!(cfg.model, DEFAULT_LLM_MODEL);
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn env_token_fills_a_tokenless_profile_but_does_not_override_a_saved_one() {
        let mut file = OracleConfigFile {
            version: 1,
            profiles: vec![Profile {
                name: "corp".into(),
                base_url: "https://api.corp/v1".into(),
                model: "gpt-4o".into(),
                token: None,
            }],
            active: Some("corp".into()),
        };
        // token: None ⇒ env fills it.
        let (cfg, _) = resolve_active(&file, None, None, Some("env-tok"));
        assert_eq!(cfg.api_key.as_deref(), Some("env-tok"));
        // A saved token is NOT overridden by env.
        file.profiles[0].token = Some("saved-tok".into());
        let (cfg, _) = resolve_active(&file, None, None, Some("env-tok"));
        assert_eq!(cfg.api_key.as_deref(), Some("saved-tok"));
    }
}
