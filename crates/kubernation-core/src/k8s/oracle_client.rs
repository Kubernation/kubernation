//! The Oracle's HTTP egress — the ONLY networked Oracle code and the project's
//! first general outbound call. It is **non-mutating** (it changes nothing on
//! the cluster), so it sits BESIDE the one write file `actions.rs` rather than
//! in it — exactly like `portforward.rs` (active-but-non-mutating, gated).
//! Gated behind the `oracle` cargo feature so the headless core smoke example
//! never links an HTTP egress.
//!
//! It does ONE thing: a single non-streaming POST to an OpenAI-compatible
//! `/v1/chat/completions` (Ollama, llama.cpp, vLLM, LM Studio, OpenRouter,
//! Anthropic-via-shim, …) under a timeout, returning the assistant text. The
//! request body + the prompt are built by the PURE `state::oracle` module, so
//! the consent preview the operator approves is byte-identical to what is sent.
//!
//! Reuses the hyper + hyper-rustls(ring) stack kube already pulls (zero new
//! crates). The rustls process-default crypto provider is installed once, to
//! match kube's `ring` choice (a mismatch would panic at runtime).

use std::fmt;
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::Request;
use hyper::body::Bytes;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::state::oracle::{self, ChatMessage};

/// Default wall-clock cap on a whole consult (connect + send + receive) when a
/// profile sets no `timeout_secs`. Sized for a LARGE local model: a realm-scope
/// consult on a 35B model (the default) measured 60–90s+ including the cold model
/// load, so a 60s cap cut real replies off. The consult runs on a spawned task
/// (it never blocks the world loop), so a generous ceiling only means a longer
/// "consulting…" — the operator can Close at any time. The effective timeout is
/// `LlmConfig.timeout_secs` (per-profile, clamped to a sane range).
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// The accepted per-profile timeout range (seconds): ≥5 (a 0 would time out
/// instantly) and ≤600 (the hard ceiling the run loop tolerates).
pub const MIN_TIMEOUT_SECS: u64 = 5;
pub const MAX_TIMEOUT_SECS: u64 = 600;

/// Hard cap on a buffered response body — a chat completion is kilobytes; this
/// bounds a hostile/runaway endpoint so it cannot OOM the net loop.
const MAX_RESP_BYTES: usize = 8 * 1024 * 1024;

/// Whether the endpoint is on the operator's laptop (no egress off-box) or a
/// remote service (publishing). The GUI gates the consent preview on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Local,
    Remote,
}

/// BYO-LLM connection config. The API key is **env-only** and never written to
/// disk; the `Debug` impl below redacts it.
#[derive(Clone)]
pub struct LlmConfig {
    /// OpenAI-compatible base, e.g. `http://localhost:11434/v1`.
    pub base_url: String,
    pub model: String,
    /// From `KUBERNATION_LLM_TOKEN` only; never logged, never persisted.
    pub api_key: Option<String>,
    pub endpoint: Endpoint,
    /// Wall-clock cap (seconds) on a consult/probe/chat-test against this
    /// endpoint. Clamped to [`MIN_TIMEOUT_SECS`, `MAX_TIMEOUT_SECS`] at build.
    pub timeout_secs: u64,
}

impl LlmConfig {
    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS))
    }
}

impl fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            // NEVER print the token — only whether one is set.
            .field(
                "api_key",
                &self.api_key.as_ref().map(|_| "<set>").unwrap_or("<unset>"),
            )
            .field("endpoint", &self.endpoint)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

/// A classified consult failure — each maps to a distinct calm GUI message
/// (degrade-dark; never a fabricated answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// The request exceeded `TIMEOUT`.
    Timeout,
    /// Could not reach / connect to the endpoint (offline, wrong URL, TLS).
    Connection(String),
    /// 401/403 — the endpoint rejected the credential (or wants one).
    Auth,
    /// 429 — rate limited / out of quota.
    RateLimited,
    /// Any other non-2xx status, with the endpoint's own error message when it
    /// gave one (e.g. a 404 "model 'llama3.1' not found" — far more actionable
    /// than a bare status code).
    BadStatus { status: u16, detail: String },
    /// The response body was not a usable OpenAI completion.
    Decode(String),
    /// Misconfiguration (bad URL, TLS roots unavailable) before any request.
    Config(String),
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Timeout => write!(f, "the model did not respond in time"),
            LlmError::Connection(e) => write!(f, "could not reach the model endpoint: {e}"),
            LlmError::Auth => write!(f, "the endpoint rejected the API token (401/403)"),
            LlmError::RateLimited => write!(f, "rate limited by the endpoint (429)"),
            LlmError::BadStatus { status, detail } if !detail.is_empty() => {
                write!(f, "HTTP {status}: {detail}")
            }
            LlmError::BadStatus { status, .. } => write!(f, "the endpoint returned HTTP {status}"),
            LlmError::Decode(e) => write!(f, "could not read the model response: {e}"),
            LlmError::Config(e) => write!(f, "endpoint misconfigured: {e}"),
        }
    }
}

/// PURE: map an HTTP status to an error (`None` ⇒ a 2xx success). Unit-tested.
pub fn classify_status(status: u16) -> Option<LlmError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(LlmError::Auth),
        429 => Some(LlmError::RateLimited),
        s => Some(LlmError::BadStatus {
            status: s,
            detail: String::new(),
        }),
    }
}

/// Install the `ring` rustls crypto provider as the process default (once),
/// matching kube's choice. Idempotent — an already-installed provider is fine.
fn ensure_crypto_provider() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn https_client() -> Result<
    Client<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        Full<Bytes>,
    >,
    LlmError,
> {
    ensure_crypto_provider();
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|e| LlmError::Config(format!("system TLS roots unavailable: {e}")))?
        .https_or_http()
        .enable_http1()
        .build();
    Ok(Client::builder(TokioExecutor::new()).build(https))
}

/// POST one chat completion and return the assistant text. The request body is
/// the EXACT bytes `state::oracle::consent_preview` showed the operator (same
/// builder). Non-streaming, under `TIMEOUT`. Writes nothing to the cluster.
pub async fn consult(cfg: &LlmConfig, messages: Vec<ChatMessage>) -> Result<String, LlmError> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let body = oracle::request_json(&oracle::chat_request(&cfg.model, messages));

    let client = https_client()?;
    let mut builder = Request::builder()
        .method("POST")
        .uri(&url)
        .header("content-type", "application/json");
    if let Some(key) = &cfg.api_key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    let req = builder
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| LlmError::Config(format!("bad request URL: {e}")))?;

    // ONE timeout around the whole sequence (request + status + bounded body
    // read), so the true wall-clock cap is TIMEOUT — not 2×.
    let outcome = tokio::time::timeout(cfg.timeout(), async {
        let resp = client
            .request(req)
            .await
            .map_err(|e| LlmError::Connection(e.to_string()))?;
        let status = resp.status().as_u16();
        // Always read the (size-bounded) body — on an error we mine it for the
        // endpoint's own message (a 404 model-not-found, etc.); on success we
        // parse the completion.
        let collected = Limited::new(resp.into_body(), MAX_RESP_BYTES)
            .collect()
            .await
            .map_err(|_| LlmError::Decode("response too large or truncated".into()))?;
        Ok((status, collected.to_bytes()))
    })
    .await;

    let (status, bytes) = match outcome {
        Err(_) => return Err(LlmError::Timeout),
        Ok(Err(e)) => return Err(e),
        Ok(Ok(v)) => v,
    };
    let text = String::from_utf8_lossy(&bytes);
    if let Some(err) = classify_status(status) {
        // Enrich a generic non-2xx with the endpoint's own error message.
        return Err(match err {
            LlmError::BadStatus { status, .. } => LlmError::BadStatus {
                status,
                detail: oracle::extract_error_message(&text),
            },
            other => other,
        });
    }
    oracle::parse_chat_response(&text).map_err(|e| LlmError::Decode(e.to_string()))
}

/// A lightweight reachability/auth check for the setup screen — a GET to
/// `{base_url}/models`. Returns Ok on a 2xx (the endpoint is up and the token,
/// if any, is accepted).
pub async fn probe(cfg: &LlmConfig) -> Result<(), LlmError> {
    let url = format!("{}/models", cfg.base_url.trim_end_matches('/'));
    let client = https_client()?;
    let mut builder = Request::builder().method("GET").uri(&url);
    if let Some(key) = &cfg.api_key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    let req = builder
        .body(Full::new(Bytes::new()))
        .map_err(|e| LlmError::Config(format!("bad URL: {e}")))?;
    let resp = match tokio::time::timeout(cfg.timeout(), client.request(req)).await {
        Err(_) => return Err(LlmError::Timeout),
        Ok(Err(e)) => return Err(LlmError::Connection(e.to_string())),
        Ok(Ok(r)) => r,
    };
    match classify_status(resp.status().as_u16()) {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

/// List the model ids the endpoint serves — a GET to `{base_url}/models`, parsed
/// by the pure `oracle::parse_models`. Drives the in-app model picker. For a
/// REMOTE endpoint this sends the `Authorization` token off-box (it is itself
/// token-bearing egress), so the caller gates it behind the egress arm.
pub async fn list_models(cfg: &LlmConfig) -> Result<Vec<String>, LlmError> {
    let url = format!("{}/models", cfg.base_url.trim_end_matches('/'));
    let client = https_client()?;
    let mut builder = Request::builder().method("GET").uri(&url);
    if let Some(key) = &cfg.api_key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    let req = builder
        .body(Full::new(Bytes::new()))
        .map_err(|e| LlmError::Config(format!("bad URL: {e}")))?;

    let outcome = tokio::time::timeout(cfg.timeout(), async {
        let resp = client
            .request(req)
            .await
            .map_err(|e| LlmError::Connection(e.to_string()))?;
        let status = resp.status().as_u16();
        let collected = Limited::new(resp.into_body(), MAX_RESP_BYTES)
            .collect()
            .await
            .map_err(|_| LlmError::Decode("response too large or truncated".into()))?;
        Ok((status, collected.to_bytes()))
    })
    .await;

    let (status, bytes) = match outcome {
        Err(_) => return Err(LlmError::Timeout),
        Ok(Err(e)) => return Err(e),
        Ok(Ok(v)) => v,
    };
    let text = String::from_utf8_lossy(&bytes);
    if let Some(err) = classify_status(status) {
        return Err(match err {
            LlmError::BadStatus { status, .. } => LlmError::BadStatus {
                status,
                detail: oracle::extract_error_message(&text),
            },
            other => other,
        });
    }
    oracle::parse_models(&text).map_err(|e| LlmError::Decode(e.to_string()))
}

// These tests need the `oracle` feature (the whole module is gated). They run
// under `cargo test --workspace` (the `kubernation` bin enables the feature, so
// unification turns it on for core) and `cargo test -p kubernation-core
// --features oracle`; a bare `cargo test -p kubernation-core` skips the module.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_status_maps_codes() {
        assert_eq!(classify_status(200), None);
        assert_eq!(classify_status(201), None);
        assert_eq!(classify_status(401), Some(LlmError::Auth));
        assert_eq!(classify_status(403), Some(LlmError::Auth));
        assert_eq!(classify_status(429), Some(LlmError::RateLimited));
        assert_eq!(
            classify_status(500),
            Some(LlmError::BadStatus {
                status: 500,
                detail: String::new()
            })
        );
        assert_eq!(
            classify_status(404),
            Some(LlmError::BadStatus {
                status: 404,
                detail: String::new()
            })
        );
        // The Display surfaces an endpoint message when present.
        assert_eq!(
            LlmError::BadStatus {
                status: 404,
                detail: "model 'llama3.1' not found".into()
            }
            .to_string(),
            "HTTP 404: model 'llama3.1' not found"
        );
    }

    #[test]
    fn debug_redacts_the_token() {
        let cfg = LlmConfig {
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3".into(),
            api_key: Some("sk-supersecret-DO-NOT-LEAK".into()),
            endpoint: Endpoint::Local,
            timeout_secs: 180,
        };
        let dbg = format!("{cfg:?}");
        assert!(
            !dbg.contains("supersecret"),
            "the token must never appear in Debug"
        );
        assert!(dbg.contains("<set>"));
        let unset = LlmConfig {
            api_key: None,
            ..cfg
        };
        assert!(format!("{unset:?}").contains("<unset>"));
    }
}
