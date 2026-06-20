//! The Oracle of KuberNation — the BYO-LLM "Wonder" consult modal.
//!
//! A window over the pure `state::oracle` pipeline — pick a SCOPE (realm / a
//! selected workload / node / a focused concern), see the EXACT redacted + fenced
//! prompt that will be sent (the mandatory pre-send preview), Consult, and read
//! the advisory reply. The model may also PROPOSE an intervention, which is
//! validated against the live store (`state::oracle_suggest`) and offered as a
//! **Stage** button — the model NEVER acts; a staged suggestion enters the
//! planning turn and is committed only through the existing dry-run/RBAC gate.
//!
//! Config is local-default (Ollama). A LOCAL endpoint keeps everything on the
//! laptop. A REMOTE endpoint is publishing, so it is OFF by default behind an
//! explicit per-session ARM, and a remote Consult sends only the **frozen**
//! previewed bytes (what you reviewed is what is published) and writes a
//! one-shot, metadata-only egress audit. The pure draw-decision fns are
//! unit-tested (testability policy); macroquad rendering is covered by gui-smoke.

use kubernation_core::k8s::oracle_client::{Endpoint, LlmConfig};
use kubernation_core::state::oracle::{self, Caps, Scope};
use kubernation_core::state::oracle_suggest::{self, ValidatedSuggestion};
use kubernation_core::state::planned::Intervention;
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
    /// Stage a model-proposed, validated intervention into the planning turn (the
    /// operator then reviews + commits it through the existing dry-run/RBAC gate).
    Stage(Intervention),
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
                    "REMOTE — publishing off-laptop (must be armed)".to_string(),
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

/// Truncate to `n` chars with an ellipsis (keeps a suggestion row clear of its
/// Stage button).
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// A previewed payload, frozen at Preview time so a Consult sends EXACTLY the
/// bytes the operator reviewed — the consent must not drift if the live snapshot
/// refreshes between viewing and clicking (mandatory for a remote/publishing
/// consult; nice for local).
struct Frozen {
    hash: u64,
    messages: Vec<oracle::ChatMessage>,
    /// The legible consent rendering shown to the operator.
    preview: String,
    /// The actual wire-payload size (the audit's "request bytes" — distinct from
    /// the legible `preview` length).
    wire_bytes: usize,
    redacted: usize,
}

/// The Oracle consult modal. `scopes` is captured at open from the current
/// selection (realm always available).
pub struct OracleView {
    scopes: Vec<Scope>,
    scope_idx: usize,
    show_preview: bool,
    /// The frozen previewed payload (the consent snapshot). Cleared on scope
    /// change; required before a remote consult.
    frozen: Option<Frozen>,
    /// Hash of an in-flight consult (drives the "consulting…" state + button gate).
    pending: Option<u64>,
    reply: Option<String>,
    /// Validated model-proposed interventions parsed from the reply (stage-able),
    /// the model suggestions that failed validation (shown as rejected), and the
    /// indices the operator has staged this session.
    suggestions: Vec<ValidatedSuggestion>,
    rejects: Vec<String>,
    staged: std::collections::HashSet<usize>,
    /// A note shown after a remote consult writes its egress audit record.
    audit_note: Option<String>,
    /// Dev (`--oracle-go`): auto-fire the consult on the next draw.
    auto: bool,
    /// Dev (`--oracle-suggest`): synthesize a deterministic validated suggestion
    /// so the suggest→stage UI is screenshot/gui-smoke verifiable without a model.
    demo_suggest: bool,
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
            frozen: None,
            pending: None,
            reply: None,
            suggestions: Vec::new(),
            rejects: Vec::new(),
            staged: std::collections::HashSet::new(),
            audit_note: None,
            auto: false,
            demo_suggest: false,
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    /// Dev: auto-consult on the next draw (the `--oracle-go` headless round-trip).
    pub fn auto_consult(&mut self) {
        self.auto = true;
    }

    /// Dev: synthesize a deterministic validated suggestion (the `--oracle-suggest`
    /// headless capture of the suggest→stage UI, no model required).
    pub fn demo_suggest(&mut self) {
        self.demo_suggest = true;
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
        let cfg = net.oracle_config();
        let remote = cfg.as_ref().is_some_and(|c| c.endpoint == Endpoint::Remote);
        let armed = net.oracle_egress_armed();
        // A remote endpoint that isn't armed yet shows "Arm remote egress" in the
        // action slot instead of Consult — egress is a deliberate opt-in.
        let arm_mode = remote && !armed;
        let action_label = if arm_mode {
            "Arm remote egress\u{2026}"
        } else {
            "Consult"
        };
        let win = draw_window(
            "Oracle of KuberNation — HOT",
            vec2(760.0, 580.0),
            &["Preview", action_label, "Close"],
            usize::MAX,
        );
        let b = win.body;

        // Poll an in-flight consult.
        if let Some(h) = self.pending
            && let Some(r) = net.oracle_reply(h)
        {
            self.suggestions.clear();
            self.rejects.clear();
            self.staged.clear();
            match &*r {
                OracleReply::Ok(t) => {
                    self.reply = Some(t.clone());
                    // Parse + VALIDATE any proposed suggestion against the LIVE
                    // store (the model output never becomes an Intervention except
                    // through this validator — hallucinations/protected targets are
                    // rejected here, not staged).
                    if let Some(s) = snap
                        && let Some(env) = oracle_suggest::parse_suggestions(t)
                    {
                        let (ok, rej) = oracle_suggest::validate_envelope(&env, &s.hot.observed);
                        self.suggestions = ok;
                        self.rejects = rej;
                    }
                }
                OracleReply::Err(e) => {
                    self.reply = Some(format!("could not consult the Oracle: {e}"))
                }
            }
            self.pending = None;
            self.scroll = 0.0;
        }

        // --- setup band -----------------------------------------------------
        let mut y = b.y + 4.0;
        for (line, role) in oracle_setup_lines(cfg.as_ref()) {
            text(ascii(&line), b.x, y + 12.0, 13.0, role_color(role));
            y += 16.0;
        }
        if remote {
            let (txt, col) = if armed {
                ("remote egress: ARMED (publishing this session)", WARN)
            } else {
                (
                    "remote egress: disarmed (off-laptop sending is blocked)",
                    DIM,
                )
            };
            text(txt, b.x, y + 12.0, 13.0, col);
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
        // FREEZES the rendered payload (see `Frozen`), and a Consult sends exactly
        // the frozen bytes — so what the operator reviewed is what is published,
        // even if the live snapshot refreshes between viewing and clicking.
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

        // Dev auto-consult (`--oracle-go`): fire once, as if Consult were clicked
        // (local only — `dispatch` refuses a remote endpoint without a frozen
        // preview).
        if self.auto && self.pending.is_none() {
            self.auto = false;
            if let Some(h) = self.dispatch(net, &built, &cfg) {
                self.pending = Some(h);
                self.show_preview = false;
            }
        }

        // The preview pane always shows a frozen snapshot — freeze lazily if it
        // was opened without a click (the `--oracle-ask` dev flag).
        if self.show_preview && self.frozen.is_none() {
            self.frozen = freeze(&built);
        }

        // Dev (`--oracle-suggest`): synthesize a deterministic validated suggestion
        // (a restart of the first workload) so the suggest→stage UI renders without
        // a model. Goes through the SAME validator as a real model suggestion.
        if self.demo_suggest && self.reply.is_none() {
            self.demo_suggest = false;
            if let Some(s) = snap
                && let Some(wr) = s
                    .hot
                    .models
                    .workloads
                    .iter()
                    .find(|w| !kubernation_core::state::chaos::ns_protected(&w.r.namespace))
                    .map(|w| w.r.clone())
            {
                let env = oracle_suggest::SuggestionEnvelope {
                    rationale: String::new(),
                    suggestions: vec![oracle_suggest::SuggestionJson {
                        verb: "restart".into(),
                        kind: wr.kind.to_string(),
                        namespace: wr.namespace.clone(),
                        name: wr.name.clone(),
                        rationale: Some("(demo) the pods look unhealthy".into()),
                        ..Default::default()
                    }],
                };
                let (ok, rej) = oracle_suggest::validate_envelope(&env, &s.hot.observed);
                self.reply = Some(
                    "(demo reply) Based on the observed data, here is a suggested change you can stage and review.".into(),
                );
                self.suggestions = ok;
                self.rejects = rej;
            }
        }

        // --- body -----------------------------------------------------------
        // Stage-button rects captured during the reply render (only for visible,
        // not-yet-staged suggestions), hit-tested in the input section.
        let mut stage_btns: Vec<(usize, Rect)> = Vec::new();
        let mut cx = Ctx {
            body: Rect::new(b.x, y, b.w, b.h - (y - b.y)),
            y: y - self.scroll,
        };
        if snap.is_none() {
            cx.row("waiting for the cluster to sync…", DIM);
        } else if arm_mode {
            cx.row(
                "This endpoint is REMOTE — consulting it sends data OFF this",
                WARN,
            );
            cx.row("laptop to a third party (publishing it).", WARN);
            cx.gap();
            cx.row(
                "Kubernation redacts credential-shaped text and shows you the",
                DIM,
            );
            cx.row(
                "EXACT payload before sending — but review it: anything that",
                DIM,
            );
            cx.row(
                "survives redaction will be published. The API token is sent as",
                DIM,
            );
            cx.row(
                "a bearer header and each remote consult is recorded to a local",
                DIM,
            );
            cx.row("audit file (metadata only).", DIM);
            cx.gap();
            cx.row(
                "Click \"Arm remote egress\" to enable remote consults this session.",
                PARCHMENT,
            );
        } else if self.show_preview {
            match &self.frozen {
                Some(f) => {
                    cx.row(
                        &format!(
                            "exactly what will be sent to the {} model ({} bytes) — review before consulting:",
                            if remote { "REMOTE" } else { "local" },
                            f.wire_bytes
                        ),
                        if remote { WARN } else { PARCHMENT },
                    );
                    if f.redacted > 0 {
                        cx.row(
                            &format!(
                                "({} section(s) had credential-shaped text masked; redaction is best-effort)",
                                f.redacted
                            ),
                            DIM,
                        );
                    }
                    cx.gap();
                    for line in wrap(&f.preview, 96) {
                        cx.row(&ascii(&line), INK);
                    }
                }
                None => cx.row(
                    "preview unavailable (waiting for sync / not configured).",
                    WARN,
                ),
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
            if let Some(note) = &self.audit_note {
                cx.row(&ascii(note), DIM);
            }
            // Validated suggestions — each stage-able into the planning turn (the
            // operator reviews + commits through the existing dry-run/RBAC gate).
            if !self.suggestions.is_empty() {
                cx.gap();
                cx.row(
                    "Suggested changes — Stage to review in Orders ▸ End of Turn:",
                    PARCHMENT,
                );
                for (i, s) in self.suggestions.iter().enumerate() {
                    cx.y += 20.0;
                    if cx.visible() {
                        let staged = self.staged.contains(&i);
                        let label = truncate(&s.summary, 72);
                        text(ascii(&label), cx.body.x + 8.0, cx.y, 13.0, INK);
                        let bw = 70.0;
                        let br = Rect::new(cx.body.x + cx.body.w - bw - 6.0, cx.y - 13.0, bw, 18.0);
                        let (lbl, col) = if staged {
                            ("staged", DIM)
                        } else {
                            ("Stage", GOOD)
                        };
                        let bg = if !staged && br.contains(mouse) {
                            lighter(PLATE, 1.7)
                        } else {
                            PLATE
                        };
                        draw_rectangle(br.x, br.y, br.w, br.h, bg);
                        draw_rectangle_lines(br.x, br.y, br.w, br.h, 1.0, col);
                        text(lbl, br.x + 14.0, br.y + 14.0, 13.0, col);
                        if !staged {
                            stage_btns.push((i, br));
                        }
                    }
                }
            }
            if !self.rejects.is_empty() {
                cx.gap();
                cx.row(
                    "Rejected (the model proposed these; not safe to stage):",
                    DIM,
                );
                for r in &self.rejects {
                    cx.row(&ascii(&format!("  x {r}")), DIM);
                }
            }
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
            if remote {
                cx.row(
                    "  (remote endpoint — Preview is required before a Consult)",
                    DIM,
                );
            }
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
            // Stage a validated suggestion (its own buttons, below the reply).
            for (i, br) in &stage_btns {
                if br.contains(mouse) {
                    self.staged.insert(*i);
                    return OracleAction::Stage(self.suggestions[*i].intervention.clone());
                }
            }
            if self.scopes.len() > 1 && (prev.contains(mouse) || next.contains(mouse)) {
                let n = self.scopes.len();
                let delta = if prev.contains(mouse) { n - 1 } else { 1 };
                self.scope_idx = (self.scope_idx + delta) % n;
                // The frozen consent + preview + suggestions belonged to the old scope.
                self.show_preview = false;
                self.frozen = None;
                self.reply = None;
                self.suggestions.clear();
                self.rejects.clear();
                self.staged.clear();
                self.audit_note = None;
                self.scroll = 0.0;
                return OracleAction::None;
            }
            match win.button_at(mouse) {
                Some(0) => {
                    // Preview — FREEZE the current payload as the consent snapshot,
                    // then show it.
                    self.frozen = freeze(&built);
                    self.show_preview = self.frozen.is_some();
                    self.scroll = 0.0;
                }
                Some(1) => {
                    if arm_mode {
                        // Arm remote egress (the deliberate per-session opt-in).
                        net.arm_oracle_egress();
                    } else if self.pending.is_none() {
                        // Consult. A remote consult REQUIRES a frozen preview (you
                        // must see the exact payload first); a local one may use the
                        // frozen snapshot or build fresh.
                        if remote && self.frozen.is_none() {
                            // Force a preview first instead of sending blind.
                            self.frozen = freeze(&built);
                            self.show_preview = true;
                            self.scroll = 0.0;
                        } else if let Some(sent) = self.dispatch(net, &built, &cfg) {
                            self.pending = Some(sent);
                            self.reply = None;
                            self.show_preview = false;
                            self.scroll = 0.0;
                        }
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

    /// Send a consult, preferring the frozen consent snapshot (what the operator
    /// reviewed); a REMOTE consult is sent ONLY from a frozen preview and writes a
    /// one-shot egress audit. Returns the bundle hash to poll, or `None` if it
    /// couldn't send (no config / a remote endpoint with no frozen preview).
    fn dispatch(
        &mut self,
        net: &Net,
        built: &Option<(oracle::ContextBundle, oracle::RedactionReport, String, bool)>,
        cfg: &Option<LlmConfig>,
    ) -> Option<u64> {
        if let Some(f) = self.frozen.take() {
            // Audit a remote egress ONLY when this will be a real send — a cached
            // reply (e.g. returning to a previously-consulted scope) is served
            // without any POST, so it must not write a second "egress recorded".
            if let Some(c) = cfg
                && c.endpoint == Endpoint::Remote
                && net.oracle_reply(f.hash).is_none()
            {
                self.audit_note = Some(write_egress_audit(
                    c,
                    &self.scopes[self.scope_idx].label(),
                    &f,
                ));
            }
            net.request_oracle(f.hash, f.messages);
            return Some(f.hash);
        }
        // No frozen preview: only a LOCAL endpoint may build-and-send fresh. A
        // remote consult must go through Preview first (the consent snapshot).
        if let Some((bundle, _, model, remote)) = built {
            if *remote {
                return None;
            }
            let messages = oracle::render_prompt(bundle, "");
            let hash = oracle::bundle_hash(bundle, "", model, *remote);
            net.request_oracle(hash, messages);
            return Some(hash);
        }
        None
    }
}

/// Snapshot the current bundle into a `Frozen` consent record (the messages sent,
/// their legible preview, the wire size, and the cache hash) — what the operator
/// reviews IS what a Consult sends. `None` when there's nothing to send (no
/// config / no snapshot).
fn freeze(
    built: &Option<(oracle::ContextBundle, oracle::RedactionReport, String, bool)>,
) -> Option<Frozen> {
    built.as_ref().map(|(bundle, report, model, remote)| {
        let messages = oracle::render_prompt(bundle, "");
        let wire_bytes = oracle::request_json(&oracle::chat_request(model, messages.clone())).len();
        Frozen {
            hash: oracle::bundle_hash(bundle, "", model, *remote),
            messages,
            preview: oracle::consent_preview(bundle, "", model),
            wire_bytes,
            redacted: report.sections_masked,
        }
    })
}

/// Write a one-shot, metadata-only egress audit for a remote consult (the
/// sanctioned local file export — like the postmortem). Records WHEN, WHERE, and
/// HOW MUCH was published; never the prompt, the reply, or the API token.
fn write_egress_audit(cfg: &LlmConfig, scope: &str, f: &Frozen) -> String {
    let now = kubernation_core::util::now();
    let content = egress_audit_content(
        cfg,
        scope,
        f.wire_bytes,
        f.redacted,
        &now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
    );
    let fname = format!("oracle-egress-{}.txt", now.strftime("%Y%m%d-%H%M%S"));
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join(&fname);
    match std::fs::write(&path, content) {
        Ok(_) => format!("remote egress recorded -> {fname}"),
        Err(e) => format!("(egress audit not written: {e})"),
    }
}

/// PURE: the audit record body. Records WHEN/WHERE/HOW MUCH — never the prompt,
/// the reply, or the API token (only `base_url` + `model` are referenced).
/// Unit-tested for the no-token invariant.
fn egress_audit_content(
    cfg: &LlmConfig,
    scope: &str,
    bytes: usize,
    redacted: usize,
    when: &str,
) -> String {
    format!(
        "kubernation oracle — remote egress audit\n\
         when: {when}\n\
         endpoint: {}\n\
         model: {}\n\
         scope: {scope}\n\
         request bytes: {bytes}\n\
         sections with credential-shaped text masked: {redacted}\n\
         (metadata only — the prompt, the reply, and the API token are NOT recorded)\n",
        cfg.base_url, cfg.model,
    )
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
    fn egress_audit_records_metadata_never_the_token() {
        let cfg = LlmConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o".into(),
            api_key: Some("sk-SUPER-SECRET-TOKEN".into()),
            endpoint: Endpoint::Remote,
        };
        let c = egress_audit_content(&cfg, "the whole realm", 1234, 2, "2026-06-19T00:00:00Z");
        assert!(
            !c.contains("SUPER-SECRET"),
            "the API token must never be audited"
        );
        assert!(c.contains("api.openai.com"));
        assert!(c.contains("gpt-4o"));
        assert!(c.contains("request bytes: 1234"));
        assert!(c.contains("masked: 2"));
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
