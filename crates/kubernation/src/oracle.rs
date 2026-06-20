//! The Oracle of KuberNation — the BYO-LLM "Wonder" consult modal.
//!
//! P1: **local, explain-only**. A window over the pure `state::oracle` pipeline —
//! pick a SCOPE (realm / a selected workload / node / a focused concern), see the
//! EXACT redacted + fenced prompt that will be sent (the mandatory pre-send
//! preview — the egress-safety habit from day one), Consult a local model, and
//! read the advisory reply. The model NEVER acts; replies are labelled
//! model-generated. Config is local-default (Ollama); remote endpoints arrive in
//! a later version. The pure draw-decision fns are unit-tested (testability
//! policy); macroquad rendering is covered by gui-smoke.

use kubernation_core::k8s::oracle_client::{Endpoint, LlmConfig};
use kubernation_core::state::oracle::{self, Caps, Scope};
use macroquad::prelude::*;

use crate::net::{Net, OracleReply, Snapshot};
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

/// Default endpoint — a local Ollama (OpenAI-compatible at `/v1`).
pub const DEFAULT_LLM_URL: &str = "http://localhost:11434/v1";
/// A broadly-pullable local instruct model as the seed default.
pub const DEFAULT_LLM_MODEL: &str = "llama3.1";

pub enum OracleAction {
    None,
    Close,
}

/// Theme role for a rendered line (no GL — mapped to a colour at draw time).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Body,
    Dim,
    Warn,
    Good,
}

fn role_color(r: Role) -> Color {
    match r {
        Role::Body => INK,
        Role::Dim => DIM,
        Role::Warn => WARN,
        Role::Good => GOOD,
    }
}

/// PURE: resolve the Oracle launch config from flags + env. The base URL defaults
/// to a local Ollama; the API token is read from `KUBERNATION_LLM_TOKEN` ONLY
/// (never a flag, never written to disk). Always returns a config (local default).
pub fn resolve_config(llm_url: Option<&str>, llm_model: Option<&str>) -> Option<LlmConfig> {
    let base_url = llm_url
        .unwrap_or(DEFAULT_LLM_URL)
        .trim_end_matches('/')
        .to_string();
    let model = llm_model.unwrap_or(DEFAULT_LLM_MODEL).to_string();
    let api_key = std::env::var("KUBERNATION_LLM_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    let endpoint = endpoint_kind(&base_url);
    Some(LlmConfig {
        base_url,
        model,
        api_key,
        endpoint,
    })
}

/// PURE: classify a base URL as on-laptop (Local — no egress off the box) vs
/// publishing (Remote). This is the load-bearing egress gate, so it parses the
/// real HOST and matches it EXACTLY (case-insensitive), failing closed — a raw
/// `starts_with` prefix check is bypassable (`localhost.evil.com`,
/// `127.0.0.1.evil.com`, `localhost@evil.com` would all read "local" and leak
/// the bundle + token off-box). Unit-tested against those bypass attempts.
pub fn endpoint_kind(base_url: &str) -> Endpoint {
    let after = base_url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(base_url);
    // Authority = up to the first '/', '?', or '#'.
    let authority = after.split(['/', '?', '#']).next().unwrap_or("");
    // Drop userinfo (everything up to and including the last '@' — the real host
    // follows it).
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    // Strip the port, handling [ipv6]:port and host:port.
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    }
    .to_ascii_lowercase();

    let local = host == "localhost" || host == "0.0.0.0" || host == "::1" || is_loopback_v4(&host);
    if local {
        Endpoint::Local
    } else {
        Endpoint::Remote
    }
}

/// True only for an exact dotted-quad IPv4 in 127.0.0.0/8 (the loopback block) —
/// so `127.0.0.1.evil.com` (not four octets) is NOT loopback.
fn is_loopback_v4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts[0] == "127" && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// PURE draw-decision fn: the setup-band lines — endpoint / model / location /
/// token PRESENCE (never the token value). Unit-tested.
pub fn oracle_setup_lines(cfg: Option<&LlmConfig>) -> Vec<(String, Role)> {
    match cfg {
        None => vec![(
            "the Oracle is not configured (set --llm-url / KUBERNATION_LLM_TOKEN)".into(),
            Role::Warn,
        )],
        Some(c) => {
            let (loc, lr) = match c.endpoint {
                Endpoint::Local => ("local (stays on this laptop)".to_string(), Role::Good),
                Endpoint::Remote => (
                    "REMOTE — not enabled in this build (use a local model)".to_string(),
                    Role::Warn,
                ),
            };
            vec![
                (format!("endpoint: {}", c.base_url), Role::Body),
                (format!("model: {}", c.model), Role::Body),
                (format!("location: {loc}"), lr),
                (
                    if c.api_key.is_some() {
                        "API token: set (from env)".to_string()
                    } else {
                        "API token: none".to_string()
                    },
                    Role::Dim,
                ),
            ]
        }
    }
}

/// Wrap text to ~`width` chars per line (whitespace-greedy), preserving existing
/// newlines. Keeps long prompt/reply lines inside the modal body.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in s.split('\n') {
        if line.chars().count() <= width {
            out.push(line.to_string());
            continue;
        }
        let mut cur = String::new();
        for word in line.split(' ') {
            if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            // A single over-long word (no spaces) is hard-split.
            if word.chars().count() > width {
                for chunk in word.chars().collect::<Vec<_>>().chunks(width) {
                    out.push(chunk.iter().collect());
                }
            } else {
                cur.push_str(word);
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// The Oracle consult modal. `scopes` is captured at open from the current
/// selection (realm always available).
pub struct OracleView {
    scopes: Vec<Scope>,
    scope_idx: usize,
    show_preview: bool,
    /// Hash of an in-flight consult (drives the "consulting…" state + button gate).
    pending: Option<u64>,
    reply: Option<String>,
    /// Dev (`--oracle-go`): auto-fire the consult on the next draw.
    auto: bool,
    scroll: f32,
    max_scroll: f32,
}

impl OracleView {
    pub fn new(scopes: Vec<Scope>) -> Self {
        OracleView {
            scopes: if scopes.is_empty() {
                vec![Scope::Realm]
            } else {
                scopes
            },
            scope_idx: 0,
            show_preview: false,
            pending: None,
            reply: None,
            auto: false,
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    /// Dev: auto-consult on the next draw (the `--oracle-go` headless round-trip).
    pub fn auto_consult(&mut self) {
        self.auto = true;
    }

    /// Dev: force the preview pane open (the `--oracle-ask` headless capture).
    pub fn force_preview(&mut self) {
        self.show_preview = true;
    }

    /// Dev: select the first available scope of a kind ("realm"/"workload"/
    /// "node"/"concern") — drives the `--oracle <scope>` headless flag.
    pub fn focus_kind(&mut self, kind: &str) {
        let k = |s: &Scope| match s {
            Scope::Realm => "realm",
            Scope::Workload(_) => "workload",
            Scope::Node(_) => "node",
            Scope::Concern(_) => "concern",
        };
        if let Some(i) = self.scopes.iter().position(|s| k(s) == kind) {
            self.scope_idx = i;
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(
        &mut self,
        snap: Option<&Snapshot>,
        net: &Net,
        mouse: Vec2,
        click: bool,
    ) -> OracleAction {
        let win = draw_window(
            "Oracle of KuberNation — HOT",
            vec2(760.0, 580.0),
            &["Preview", "Consult", "Close"],
            usize::MAX,
        );
        let b = win.body;
        let cfg = net.oracle_config();

        // Poll an in-flight consult.
        if let Some(h) = self.pending
            && let Some(r) = net.oracle_reply(h)
        {
            self.reply = Some(match &*r {
                OracleReply::Ok(t) => t.clone(),
                OracleReply::Err(e) => format!("could not consult the Oracle: {e}"),
            });
            self.pending = None;
            self.scroll = 0.0;
        }

        // --- setup band -----------------------------------------------------
        let mut y = b.y + 4.0;
        for (line, role) in oracle_setup_lines(cfg.as_ref()) {
            text(ascii(&line), b.x, y + 12.0, 13.0, role_color(role));
            y += 16.0;
        }
        y += 4.0;

        // --- scope chip (◀ scope ▶) ----------------------------------------
        text("scope:", b.x, y + 12.0, 14.0, DIM);
        let sw = text_size("scope:", 14.0).width;
        let prev = Rect::new(b.x + sw + 10.0, y, 18.0, 18.0);
        let label = ascii(&self.scopes[self.scope_idx].label());
        let lx = prev.x + prev.w + 6.0;
        text_bold(&label, lx, y + 13.0, 14.0, PARCHMENT);
        let lw = text_size(&label, 14.0).width;
        let next = Rect::new(lx + lw + 6.0, y, 18.0, 18.0);
        if self.scopes.len() > 1 {
            for (r, sym) in [(prev, "<"), (next, ">")] {
                let bg = if r.contains(mouse) {
                    lighter(PLATE, 1.7)
                } else {
                    PLATE
                };
                draw_rectangle(r.x, r.y, r.w, r.h, bg);
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, PARCHMENT);
                text(sym, r.x + 6.0, r.y + 14.0, 14.0, INK);
            }
        }
        y += 26.0;

        // Build the bundle for the active scope (cheap for real sizes). Preview
        // and Consult render/send from the SAME `built` each frame, so within a
        // frame the preview IS the sent bytes; across frames a live snapshot
        // refresh can change it (acceptable for a local explain-only consult —
        // P2's formal remote-egress consent will freeze the previewed bytes).
        let built = snap.zip(cfg.as_ref()).map(|(s, c)| {
            let ctx = oracle::BundleCtx {
                cluster: &s.hot.observed.meta.context,
                log_body: None,
                slo: Some(&s.hot.slo),
            };
            let (bundle, report) = oracle::build_bundle(
                &s.hot.models,
                &s.hot.observed,
                &self.scopes[self.scope_idx],
                &ctx,
                &Caps::default(),
            );
            (
                bundle,
                report,
                c.model.clone(),
                c.endpoint == Endpoint::Remote,
            )
        });

        // Dev auto-consult (`--oracle-go`): fire once, as if Consult were clicked.
        if self.auto && self.pending.is_none() {
            self.auto = false;
            if let Some((bundle, _, model, remote)) = &built
                && !*remote
            {
                let messages = oracle::render_prompt(bundle, "");
                let hash = oracle::bundle_hash(bundle, "", model, *remote);
                net.request_oracle(hash, messages);
                self.pending = Some(hash);
                self.show_preview = false;
            }
        }

        // --- body -----------------------------------------------------------
        let mut cx = Ctx {
            body: Rect::new(b.x, y, b.w, b.h - (y - b.y)),
            y: y - self.scroll,
        };
        if snap.is_none() {
            cx.row("waiting for the cluster to sync…", DIM);
        } else if self.show_preview {
            match &built {
                Some((bundle, report, model, remote)) => {
                    cx.row(
                        &format!(
                            "this EXACT text will be sent to the {} model — review before consulting:",
                            if *remote { "REMOTE" } else { "local" }
                        ),
                        if *remote { WARN } else { PARCHMENT },
                    );
                    if report.sections_masked > 0 {
                        cx.row(
                            &format!(
                                "({} section(s) had credential-shaped text masked; redaction is best-effort)",
                                report.sections_masked
                            ),
                            DIM,
                        );
                    }
                    cx.gap();
                    let preview = oracle::consent_preview(bundle, "", model);
                    for line in wrap(&preview, 96) {
                        cx.row(&ascii(&line), INK);
                    }
                }
                None => cx.row("the Oracle is not configured.", WARN),
            }
        } else if self.pending.is_some() {
            cx.row(
                "consulting the Oracle… (this can take a moment on a local model)",
                DIM,
            );
        } else if let Some(reply) = &self.reply {
            for line in wrap(reply, 96) {
                cx.row(&ascii(&line), INK);
            }
            cx.gap();
            cx.row("— model-generated; verify before acting.", DIM);
        } else {
            cx.row("Pick a scope, then:", PARCHMENT);
            cx.row(
                "  • Preview — see the exact (redacted, fenced) text that will be sent",
                DIM,
            );
            cx.row(
                "  • Consult — ask the model to explain it (it cannot change anything)",
                DIM,
            );
            if let Some((bundle, _, _, _)) = &built {
                cx.gap();
                cx.row(
                    &format!(
                        "context: {} section(s), ~{} tokens{}",
                        bundle.sections.len(),
                        bundle.est_tokens,
                        if bundle.truncated {
                            " (truncated to fit)"
                        } else {
                            ""
                        }
                    ),
                    DIM,
                );
            }
        }

        let content_h = cx.y - (y - self.scroll);
        self.max_scroll = (content_h - cx.body.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);

        // --- input ----------------------------------------------------------
        if click {
            if self.scopes.len() > 1 && (prev.contains(mouse) || next.contains(mouse)) {
                let n = self.scopes.len();
                let delta = if prev.contains(mouse) { n - 1 } else { 1 };
                self.scope_idx = (self.scope_idx + delta) % n;
                self.show_preview = false;
                self.reply = None;
                self.scroll = 0.0;
                return OracleAction::None;
            }
            match win.button_at(mouse) {
                Some(0) => {
                    // Preview — toggle.
                    self.show_preview = !self.show_preview;
                    self.scroll = 0.0;
                }
                Some(1) => {
                    // Consult — local only, not while one is in flight.
                    if self.pending.is_none()
                        && let Some((bundle, _, model, remote)) = &built
                        && !*remote
                    {
                        let messages = oracle::render_prompt(bundle, "");
                        let hash = oracle::bundle_hash(bundle, "", model, *remote);
                        net.request_oracle(hash, messages);
                        self.pending = Some(hash);
                        self.reply = None;
                        self.show_preview = false;
                        self.scroll = 0.0;
                    }
                }
                Some(_) => return OracleAction::Close, // Close
                None => {
                    if win.close.contains(mouse) || !win.frame.contains(mouse) {
                        return OracleAction::Close;
                    }
                }
            }
        }
        OracleAction::None
    }
}

// Minimal scroll-aware text cursor (mirrors charter.rs::Ctx).
struct Ctx {
    body: Rect,
    y: f32,
}

impl Ctx {
    fn visible(&self) -> bool {
        self.y > self.body.y - 18.0 && self.y < self.body.y + self.body.h
    }
    fn gap(&mut self) {
        self.y += 8.0;
    }
    fn row(&mut self, s: &str, color: Color) {
        self.y += 16.0;
        if self.visible() {
            text(s, self.body.x + 2.0, self.y, 13.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_kind_classifies_local_vs_remote() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1",
            "http://127.5.6.7/v1", // anywhere in 127.0.0.0/8
            "https://0.0.0.0/v1",
            "http://[::1]:11434/v1",
            "HTTP://LocalHost:11434/v1", // case-insensitive
        ] {
            assert_eq!(endpoint_kind(url), Endpoint::Local, "{url} should be local");
        }
        for url in [
            "https://api.openai.com/v1",
            "https://openrouter.ai/api/v1",
            "http://10.0.0.5:8080/v1",
            // Bypass attempts the old prefix check mis-read as local:
            "http://localhost.evil.com/v1",
            "http://127.0.0.1.evil.com/v1",
            "http://0.0.0.0.attacker.net/v1",
            "http://localhost@evil.com/v1",
            "http://notlocalhost.com/v1",
        ] {
            assert_eq!(
                endpoint_kind(url),
                Endpoint::Remote,
                "{url} must be remote (no bypass)"
            );
        }
    }

    #[test]
    fn resolve_config_defaults_local_and_reads_no_token_flag() {
        // SAFETY: no env set in the test → token None; defaults to local Ollama.
        let cfg = resolve_config(None, None).unwrap();
        assert_eq!(cfg.base_url, DEFAULT_LLM_URL);
        assert_eq!(cfg.model, DEFAULT_LLM_MODEL);
        assert_eq!(cfg.endpoint, Endpoint::Local);
        let remote = resolve_config(Some("https://api.openai.com/v1"), Some("gpt-4o")).unwrap();
        assert_eq!(remote.endpoint, Endpoint::Remote);
        assert_eq!(remote.model, "gpt-4o");
    }

    #[test]
    fn setup_lines_show_location_and_token_presence_never_value() {
        let cfg = LlmConfig {
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3.1".into(),
            api_key: Some("sk-DO-NOT-LEAK".into()),
            endpoint: Endpoint::Local,
        };
        let lines = oracle_setup_lines(Some(&cfg));
        let joined: String = lines
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            !joined.contains("DO-NOT-LEAK"),
            "the token value must never render"
        );
        assert!(joined.contains("token: set"));
        assert!(joined.contains("local"));
        // A remote endpoint flags itself as not-enabled.
        let remote = LlmConfig {
            endpoint: Endpoint::Remote,
            ..cfg
        };
        let rl = oracle_setup_lines(Some(&remote));
        assert!(
            rl.iter()
                .any(|(s, r)| s.contains("REMOTE") && *r == Role::Warn)
        );
        // No config → a single warn line.
        assert_eq!(oracle_setup_lines(None).len(), 1);
    }

    #[test]
    fn wrap_breaks_long_lines_and_keeps_newlines() {
        let w = wrap("alpha beta gamma", 5);
        assert!(w.len() >= 3);
        assert!(wrap("a\nb", 80) == vec!["a".to_string(), "b".to_string()]);
        // A single over-long word is hard-split, never dropped.
        let hard = wrap("xxxxxxxxxx", 4);
        assert_eq!(hard.join(""), "xxxxxxxxxx");
    }
}
