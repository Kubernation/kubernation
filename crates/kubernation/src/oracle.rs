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
//! A second **Settings** face manages named ENDPOINT PROFILES (a local Ollama, a
//! corporate frontier model, …): pick a pulled local model, point at a remote
//! endpoint with a URL + masked token, switch between profiles, and persist them
//! (the token on disk by explicit opt-in — see `oracle_config_io`). Switching to
//! a REMOTE endpoint still requires the per-session ARM (egress is publishing),
//! and a remote Consult sends only the **frozen** previewed bytes + writes a
//! one-shot egress audit. Pure draw-decision fns are unit-tested (testability
//! policy); macroquad rendering is covered by gui-smoke.

use std::sync::Arc;

use kubernation_core::k8s::oracle_client::{Endpoint, LlmConfig};
use kubernation_core::state::attention::Concern;
use kubernation_core::state::oracle::{
    self, Caps, DeepenLens, LensState, Scope, available_lenses, deepen_button_order,
    deepen_chip_states, default_lenses, parse_follow_up, representative_pod, strip_machine_blocks,
};
use kubernation_core::state::oracle_config::{
    self, DEFAULT_LLM_MODEL, DEFAULT_LLM_URL, OracleConfigFile, Profile, endpoint_kind,
};
use kubernation_core::state::oracle_investigate::{self, InvestigateTarget};
use kubernation_core::state::oracle_suggest::{self, ValidatedSuggestion};
use kubernation_core::state::planned::Intervention;
use macroquad::prelude::*;

use crate::net::{Net, OracleLogReq, OracleReply, Snapshot};
use crate::oracle_config_io;
use crate::text::{text, text_bold, text_size};
use crate::textfield::{FieldId, TextField};
use crate::theme::*;
use crate::window::draw_window;

pub enum OracleAction {
    None,
    Close,
    /// Stage a model-proposed, validated intervention into the planning turn (the
    /// operator then reviews + commits it through the existing dry-run/RBAC gate).
    Stage(Intervention),
    /// Copy the raw consult reply to the clipboard (main shows the toast).
    Copy(String),
    /// Export the consult (header + raw reply) to a local file (main toasts the path).
    Export(String),
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

/// Which face of the modal is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OracleFace {
    Consult,
    Settings,
}

/// PURE draw-decision fn: the setup-band lines — active profile / endpoint /
/// model / location / token SOURCE (never the token value). Unit-tested.
pub fn oracle_setup_lines(
    cfg: Option<&LlmConfig>,
    active_name: Option<&str>,
    token_on_disk: bool,
) -> Vec<(String, Role)> {
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
            let token = if c.api_key.is_none() {
                "token: none".to_string()
            } else if token_on_disk {
                "token: on disk (this profile)".to_string()
            } else {
                "token: from env".to_string()
            };
            vec![
                (
                    format!(
                        "profile: {}",
                        active_name.unwrap_or("(command-line / built-in)")
                    ),
                    Role::Body,
                ),
                (format!("endpoint: {}", c.base_url), Role::Body),
                (format!("model: {}", c.model), Role::Body),
                (format!("location: {loc}"), lr),
                (token, Role::Dim),
            ]
        }
    }
}

/// PURE draw-decision fn: the profile-list rows (active marked, REMOTE flagged).
pub fn profile_rows(config: &OracleConfigFile) -> Vec<(String, Role)> {
    let active = config.active.as_deref();
    config
        .profiles
        .iter()
        .map(|p| {
            let is_active = Some(p.name.as_str()) == active;
            let mark = if is_active { "> " } else { "  " };
            let loc = if endpoint_kind(&p.base_url) == Endpoint::Remote {
                " · REMOTE"
            } else {
                ""
            };
            let role = if is_active { Role::Good } else { Role::Body };
            (format!("{mark}{}{loc}", p.name), role)
        })
        .collect()
}

/// PURE draw-decision fn: the model-picker rows (current marked; in-flight /
/// error states shown DIM, never red).
pub fn model_picker_rows(
    out: Option<&Result<Arc<Vec<String>>, String>>,
    current: &str,
) -> Vec<(String, Role)> {
    match out {
        None => vec![(
            "(click test to reach this endpoint and list its models)".into(),
            Role::Dim,
        )],
        Some(Err(e)) => vec![(format!("(could not list models: {e})"), Role::Dim)],
        Some(Ok(list)) if list.is_empty() => vec![("(no models reported)".into(), Role::Dim)],
        Some(Ok(list)) => list
            .iter()
            .map(|m| {
                let cur = m == current;
                let mark = if cur { "> " } else { "  " };
                let role = if cur { Role::Good } else { Role::Body };
                (format!("{mark}{m}"), role)
            })
            .collect(),
    }
}

/// PURE draw-decision fn: a one-line pass/fail verdict for the **Test** button,
/// derived from a `GET /v1/models` probe (`out`) + the configured `model`. It
/// validates the WHOLE config in one shot: endpoint reachable, token accepted
/// (a 401 surfaces as the classified error), and the chosen model actually
/// available. `None` ⇒ no test has run yet. Unit-tested.
pub fn connection_verdict(
    out: Option<&Result<Arc<Vec<String>>, String>>,
    model: &str,
) -> Option<(String, Role)> {
    match out {
        None => None,
        Some(Err(e)) => Some((format!("test: FAILED — {e}"), Role::Warn)),
        Some(Ok(list)) if model.is_empty() => {
            Some(("test: reachable — enter a model name".into(), Role::Good))
        }
        Some(Ok(list)) if list.iter().any(|m| m == model) => Some((
            format!("test: OK — reachable, model '{model}' is available"),
            Role::Good,
        )),
        Some(Ok(_)) => Some((
            format!(
                "test: reachable, but model '{model}' is NOT available — pull it or pick one below"
            ),
            Role::Warn,
        )),
    }
}

/// PURE draw-decision fn: the level-2 chat-test verdict (a real completion). It
/// proves the model actually GENERATES — the strongest endpoint check. `None` ⇒
/// not run. Unit-tested.
pub fn chat_verdict(out: Option<&Result<String, String>>) -> Option<(String, Role)> {
    match out {
        None => None,
        Some(Err(e)) => Some((format!("chat test: FAILED — {e}"), Role::Warn)),
        Some(Ok(reply)) => {
            let snippet: String = reply.trim().chars().take(40).collect();
            Some((
                format!("chat test: OK — the model replied: {snippet}"),
                Role::Good,
            ))
        }
    }
}

/// PURE draw-decision fn: the in-flight progress line — "consulting… {n}s
/// (timeout {t}s)". The model can take 60–90s on a local model; an elapsed counter
/// beats a static spinner. Unit-tested.
pub fn consult_progress_line(elapsed_secs: u64, timeout_secs: u64) -> String {
    if timeout_secs > 0 {
        format!("consulting the Oracle… {elapsed_secs}s (timeout {timeout_secs}s)")
    } else {
        format!("consulting the Oracle… {elapsed_secs}s")
    }
}

/// PURE draw-decision fn: the "model-generated; verify" caveat, sharpened when a
/// Stage-able suggestion is on screen (a proposed change deserves a stronger
/// reminder). Pinned as a footer outside the scroll. Unit-tested.
pub fn disclaimer_text(has_suggestions: bool) -> &'static str {
    if has_suggestions {
        "model-generated — VERIFY before staging; the model can be wrong."
    } else {
        "model-generated — verify before acting."
    }
}

/// PURE draw-decision fn: map a consult error message to a one-line operator hint.
/// The message is the classified `LlmError` Display text from the net layer — so
/// the arms match the REAL strings: timeout = "did not respond in time", auth =
/// "rejected the API token (401/403)", connection = "could not reach the model
/// endpoint", a not-pulled model = an Ollama "HTTP 404: model '…' not found".
/// `None` ⇒ no specific hint (the raw error still shows). Unit-tested.
pub fn error_hint(msg: &str) -> Option<&'static str> {
    let m = msg.to_ascii_lowercase();
    if m.contains("respond in time") || m.contains("timed out") || m.contains("timeout") {
        Some(
            "the model took too long — raise the per-profile timeout in Settings, or pick a smaller/faster model.",
        )
    } else if m.contains("rejected the api token")
        || m.contains("401")
        || m.contains("403")
        || m.contains("unauthor")
        || m.contains("forbidden")
    {
        Some(
            "authentication failed — check the API token (Settings ▸ token, or the KUBERNATION_LLM_TOKEN env var).",
        )
    } else if m.contains("not found")
        || m.contains("404")
        || m.contains("no such model")
        || (m.contains("model") && m.contains("pull"))
    {
        Some(
            "the model isn't available at this endpoint — pull it (e.g. ollama pull <model>) or pick another in Settings.",
        )
    } else if m.contains("could not reach")
        || m.contains("connection")
        || m.contains("refused")
        || m.contains("dns")
    {
        Some(
            "the endpoint is unreachable — is it running? Check the URL in Settings (default: a local Ollama at localhost:11434).",
        )
    } else if m.contains("rate limited") || m.contains("429") {
        Some("rate limited by the endpoint — wait a moment, or check your plan's quota.")
    } else if m.contains("misconfigured") {
        Some("the endpoint looks misconfigured — check the URL in Settings.")
    } else {
        None
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

/// Truncate to `n` chars with an ellipsis.
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
    preview: String,
    wire_bytes: usize,
    redacted: usize,
}

/// The bundle the consult face builds for the active scope:
/// (bundle, redaction report, model, base_url, is_remote, offered-deepen-lenses,
/// offer_investigate). `offered` + `offer_investigate` are threaded into
/// render_prompt/consent_preview/bundle_hash so BOTH the follow-up-ranking and the
/// "investigate" instructions are part of the byte-identical payload (a divergent
/// arg at any caller would break the P2 byte-frozen-consent guarantee).
type Built = (
    oracle::ContextBundle,
    oracle::RedactionReport,
    String,
    String,
    bool,
    Vec<DeepenLens>,
    bool,
);

/// Sentinel `editing` value for an unsaved NEW profile.
const NEW_PROFILE: usize = usize::MAX;

/// The Oracle modal. Carries the consult state + the editable endpoint config.
pub struct OracleView {
    face: OracleFace,
    scopes: Vec<Scope>,
    scope_idx: usize,
    show_preview: bool,
    frozen: Option<Frozen>,
    pending: Option<u64>,
    /// Wall-clock (get_time) when the in-flight consult was dispatched — drives the
    /// elapsed counter on the spinner.
    pending_started: f64,
    reply: Option<String>,
    /// A failed consult's classified error message (kept separate from `reply` so
    /// it renders as a WARN card with a hint + Retry, not as fake "answer" prose).
    reply_error: Option<String>,
    suggestions: Vec<ValidatedSuggestion>,
    rejects: Vec<String>,
    staged: std::collections::HashSet<usize>,
    audit_note: Option<String>,
    auto: bool,
    demo_suggest: bool,
    scroll: f32,
    max_scroll: f32,
    // --- endpoint config (Settings face) -------------------------------------
    config: OracleConfigFile,
    env_token: Option<String>,
    /// Which profile is being edited (`NEW_PROFILE` ⇒ an unsaved new one);
    /// `None` ⇒ no edit form shown.
    editing: Option<usize>,
    f_name: TextField,
    f_url: TextField,
    f_model: TextField,
    f_token: TextField,
    f_timeout: TextField,
    focus: Option<FieldId>,
    delete_armed: bool,
    /// A short result note shown on the Settings face (save/activate outcome).
    settings_note: Option<String>,
    /// One-shot guard: auto-discover a LOCAL profile's models once when it's
    /// selected for editing (a remote profile waits for an explicit discover —
    /// its `/v1/models` is token-bearing egress). Reset on each `load_edit`.
    models_attempted: bool,
    /// True between clicking **Test** and the probe landing — shows "testing…".
    testing: bool,
    /// True between clicking **chat** and the completion landing.
    chat_testing: bool,
    // --- deepen (Consult face) ----------------------------------------------
    /// Active deepen lenses (sections folded into the consult). Seeded from
    /// `default_lenses` on open / scope change; grown by chip clicks.
    deepen: Vec<DeepenLens>,
    /// The subset the operator EXPLICITLY clicked (vs default-on) — promoted in
    /// the budget so a requested lens isn't silently dropped.
    explicit: Vec<DeepenLens>,
    /// The fetched probe-pod log tail for the Logs lens (None until it lands).
    deepen_log: Option<String>,
    /// The in-flight log fetch (the Logs lens is "Fetching" while this is Some).
    pending_log: Option<OracleLogReq>,
    /// A consult deferred until the in-flight log fetch lands (so the FIRST
    /// consult on a crash concern carries the logs).
    want_consult: bool,
    /// The model's parsed follow-up ranking (reorders/highlights the chips).
    follow_up: Vec<DeepenLens>,
    /// One-shot guard: seed `default_lenses` for the active scope once a snapshot
    /// is available (re-armed on scope change).
    deepen_seeded: bool,
    /// Dev (`--oracle-deepen <lens>`): pre-activate a lens on seed for a headless
    /// capture of the deepen chips.
    dev_deepen: Option<DeepenLens>,
    /// Validated "investigate" targets from the latest reply (the CONSULT NEXT
    /// links). Re-derived per reply; cleared on scope/payload change.
    investigate: Vec<InvestigateTarget>,
    /// Dev (`--oracle-investigate`): synthesize a deterministic investigate target
    /// through the real validator for a headless capture of the CONSULT NEXT row.
    dev_investigate: bool,
}

impl OracleView {
    pub fn new(scopes: Vec<Scope>) -> Self {
        let config = oracle_config_io::load();
        let env_token = std::env::var("KUBERNATION_LLM_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        OracleView {
            face: OracleFace::Consult,
            scopes: if scopes.is_empty() {
                vec![Scope::Realm]
            } else {
                scopes
            },
            scope_idx: 0,
            show_preview: false,
            frozen: None,
            pending: None,
            pending_started: 0.0,
            reply: None,
            reply_error: None,
            suggestions: Vec::new(),
            rejects: Vec::new(),
            staged: std::collections::HashSet::new(),
            audit_note: None,
            auto: false,
            demo_suggest: false,
            scroll: 0.0,
            max_scroll: 0.0,
            config,
            env_token,
            editing: None,
            f_name: TextField::default(),
            f_url: TextField::default(),
            f_model: TextField::default(),
            f_token: TextField::new("", true),
            f_timeout: TextField::default(),
            focus: None,
            delete_armed: false,
            settings_note: None,
            models_attempted: false,
            testing: false,
            chat_testing: false,
            deepen: Vec::new(),
            explicit: Vec::new(),
            deepen_log: None,
            pending_log: None,
            want_consult: false,
            follow_up: Vec::new(),
            deepen_seeded: false,
            dev_deepen: None,
            investigate: Vec::new(),
            dev_investigate: false,
        }
    }

    /// Dev: enable the synthetic investigate target (the `--oracle-investigate` flag).
    pub fn dev_investigate(&mut self) {
        self.dev_investigate = true;
    }

    /// Dev: pre-activate a deepen lens by key (the `--oracle-deepen <lens>` flag).
    pub fn dev_deepen(&mut self, key: &str) {
        self.dev_deepen = DeepenLens::from_key(key);
    }

    /// Clear the consult-result state when the active payload changes (a deepen
    /// lens added / logs landed / a profile or scope switch) — so a stale frozen
    /// consent or reply never carries to the new payload (the remote re-consent
    /// guard). Does NOT clear the deepen set itself.
    fn apply_deepen_change(&mut self) {
        self.frozen = None;
        self.show_preview = false;
        self.reply = None;
        self.reply_error = None;
        self.suggestions.clear();
        self.rejects.clear();
        self.staged.clear();
        self.follow_up.clear();
        self.investigate.clear();
        self.scroll = 0.0;
    }

    /// Seed the default lenses for the current scope and fire the logs fetch if
    /// Logs is default-on (a crash/error concern). Idempotent per scope.
    fn seed_deepen(&mut self, snap: Option<&Snapshot>, net: &Net) {
        let Some(s) = snap else { return };
        self.deepen_seeded = true;
        let scope = &self.scopes[self.scope_idx];
        self.deepen = default_lenses(&s.hot.observed, scope);
        self.explicit.clear();
        self.deepen_log = None;
        self.pending_log = None;
        // Dev: pre-activate a lens (only if it's actually offered for this scope).
        if let Some(l) = self.dev_deepen
            && available_lenses(&s.hot.observed, scope).contains(&l)
            && !self.deepen.contains(&l)
        {
            self.deepen.push(l);
            self.explicit.push(l);
        }
        if self.deepen.contains(&DeepenLens::Logs) {
            self.fire_log_fetch(snap, net);
        }
    }

    /// Request the representative pod's log tail for the Logs lens (hot-only).
    fn fire_log_fetch(&mut self, snap: Option<&Snapshot>, net: &Net) {
        let Some(s) = snap else { return };
        let scope = &self.scopes[self.scope_idx];
        if let Some(p) = representative_pod(&s.hot.observed, scope) {
            let req = OracleLogReq {
                namespace: p.namespace,
                pod: p.pod,
                previous: p.previous,
            };
            self.pending_log = Some(req.clone());
            self.deepen_log = None;
            net.request_oracle_log(req);
        }
    }

    /// The lens (if any) whose log fetch is in flight — drives the "Fetching" chip.
    fn fetching_lens(&self) -> Option<DeepenLens> {
        self.pending_log.as_ref().map(|_| DeepenLens::Logs)
    }

    /// Apply a deepen chip click: activate the lens (explicitly), fetch its data
    /// if async (logs), and queue a re-consult (local → send once ready; remote →
    /// re-Preview the enriched payload for re-consent).
    fn add_lens(&mut self, lens: DeepenLens, snap: Option<&Snapshot>, net: &Net) {
        if !self.deepen.contains(&lens) {
            self.deepen.push(lens);
        }
        if !self.explicit.contains(&lens) {
            self.explicit.push(lens);
        }
        self.apply_deepen_change();
        if lens == DeepenLens::Logs {
            self.fire_log_fetch(snap, net);
        }
        // The drain (in draw_consult) sends (local) or re-Previews (remote) once
        // any in-flight logs fetch lands.
        self.want_consult = true;
    }

    /// The shared reset for a scope switch — the ◀▶ chip AND a CONSULT NEXT jump.
    /// Clears all consult-result state + any in-flight log fetch so nothing from
    /// the old scope carries to the new one.
    fn reset_for_scope_switch(&mut self, net: &Net) {
        self.show_preview = false;
        self.frozen = None;
        self.reply = None;
        self.reply_error = None;
        self.suggestions.clear();
        self.rejects.clear();
        self.staged.clear();
        self.follow_up.clear();
        self.investigate.clear();
        self.audit_note = None;
        self.scroll = 0.0;
        self.want_consult = false;
        net.clear_oracle_log();
    }

    /// Jump the active scope to a validated CONSULT NEXT target and run ONE
    /// consult. Dedup by `Scope::label()` (Scope has no Eq — it embeds a
    /// Clone-only Concern); Realm + the originals stay in `scopes` so the ◀▶ chip
    /// can return. Re-seeds deepen for the new scope, then sets `want_consult` so
    /// the existing drain fires exactly one consult (local → send; remote →
    /// re-Preview the new payload for re-consent). Builds the bundle fresh from the
    /// world — the model's untrusted `why` is never folded in.
    fn jump_to_scope(&mut self, scope: Scope, snap: Option<&Snapshot>, net: &Net) {
        if let Some(i) = self.scopes.iter().position(|s| s.label() == scope.label()) {
            self.scope_idx = i;
        } else {
            self.scopes.push(scope);
            self.scope_idx = self.scopes.len() - 1;
        }
        self.reset_for_scope_switch(net);
        self.seed_deepen(snap, net);
        self.want_consult = true;
    }

    /// Merge the CONSULT NEXT targets: the app's OWN attention queue (the floor —
    /// so a clearly identified concern always yields a drill-down link) plus any
    /// model-named extras the queue didn't already flag. App concerns seed only at
    /// REALM scope (where the model is asked to name OTHER objects); at node scope
    /// the model block stands alone. Deduped by label, capped.
    fn merge_consult_next(
        &self,
        model: Vec<InvestigateTarget>,
        attention: &[Concern],
    ) -> Vec<InvestigateTarget> {
        let mut targets = if matches!(self.scopes[self.scope_idx], Scope::Realm) {
            oracle_investigate::concern_targets(attention, oracle_investigate::CONSULT_NEXT_CAP)
        } else {
            Vec::new()
        };
        for mt in model {
            if !targets.iter().any(|x| x.scope.label() == mt.scope.label()) {
                targets.push(mt);
            }
        }
        targets.truncate(oracle_investigate::CONSULT_NEXT_CAP);
        targets
    }

    /// Dev: auto-consult on the next draw (the `--oracle-go` headless round-trip).
    pub fn auto_consult(&mut self) {
        self.auto = true;
    }

    /// Dev: open the Settings face (the `--oracle-settings` headless capture).
    pub fn open_settings(&mut self) {
        self.enter_settings();
    }

    /// True once a consult has produced a terminal outcome (a reply OR an error
    /// card) — both are "landed" for the `--oracle-go` screenshot gate.
    pub fn reply_landed(&self) -> bool {
        self.reply.is_some() || self.reply_error.is_some()
    }

    /// Whether a Settings text field currently owns the keyboard (the `main`
    /// `typing` gate ORs this in so typed text never fires a shortcut).
    pub fn field_focused(&self) -> bool {
        self.focus.is_some()
    }

    /// Defocus the active field (the first Esc when editing — main closes on the
    /// second).
    pub fn blur(&mut self) {
        self.focus = None;
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

    /// Dev: select the first available scope of a kind.
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

    fn field_mut(&mut self, id: FieldId) -> &mut TextField {
        match id {
            FieldId::Name => &mut self.f_name,
            FieldId::Url => &mut self.f_url,
            FieldId::Model => &mut self.f_model,
            FieldId::Token => &mut self.f_token,
            FieldId::Timeout => &mut self.f_timeout,
        }
    }

    /// The token source for the active profile (does it carry a saved token?).
    fn active_token_on_disk(&self) -> bool {
        self.config
            .active_profile()
            .map(|p| p.token.is_some())
            .unwrap_or(false)
    }

    /// Enter the Settings face, reloading the file (to pick up external edits) and
    /// selecting the active profile for editing.
    fn enter_settings(&mut self) {
        self.config = oracle_config_io::load();
        self.face = OracleFace::Settings;
        self.settings_note = None;
        self.delete_armed = false;
        self.focus = None;
        // Edit the active profile if any, else the first, else nothing.
        let idx = self
            .config
            .active
            .as_ref()
            .and_then(|n| self.config.profiles.iter().position(|p| &p.name == n))
            .or(if self.config.profiles.is_empty() {
                None
            } else {
                Some(0)
            });
        match idx {
            Some(i) => self.load_edit(i),
            // No saved profiles yet — start a NEW one pre-filled with the built-in
            // local default so the form (+ local model discovery) is ready.
            None => self.load_edit(NEW_PROFILE),
        }
    }

    /// Load a profile (or the NEW blank) into the edit fields.
    fn load_edit(&mut self, idx: usize) {
        self.editing = Some(idx);
        self.delete_armed = false;
        self.focus = None;
        self.models_attempted = false;
        self.testing = false;
        self.chat_testing = false;
        let p = if idx == NEW_PROFILE {
            Profile {
                name: "new endpoint".into(),
                base_url: DEFAULT_LLM_URL.into(),
                model: DEFAULT_LLM_MODEL.into(),
                token: None,
                timeout_secs: None,
            }
        } else {
            self.config.profiles[idx].clone()
        };
        self.f_name = TextField::new(&p.name, false);
        self.f_url = TextField::new(&p.base_url, false);
        self.f_model = TextField::new(&p.model, false);
        self.f_token = TextField::new(p.token.as_deref().unwrap_or(""), true);
        self.f_timeout = TextField::new(
            &p.timeout_secs.map(|t| t.to_string()).unwrap_or_default(),
            false,
        );
    }

    /// The Profile currently being composed in the edit fields.
    fn edit_profile(&self) -> Profile {
        use kubernation_core::k8s::oracle_client::{MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS};
        let tok = self.f_token.buf.trim();
        // A blank/invalid timeout ⇒ None (uses the default); a valid one is
        // clamped to the accepted range.
        let timeout_secs = self
            .f_timeout
            .buf
            .trim()
            .parse::<u64>()
            .ok()
            .map(|t| t.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS));
        Profile {
            name: self.f_name.buf.trim().to_string(),
            base_url: self.f_url.buf.trim().to_string(),
            model: self.f_model.buf.trim().to_string(),
            token: if tok.is_empty() {
                None
            } else {
                Some(tok.to_string())
            },
            timeout_secs,
        }
    }

    /// Persist the config file, recording the outcome in `settings_note`.
    fn persist(&mut self) {
        match oracle_config_io::save(&self.config) {
            Ok(path) => {
                self.settings_note = Some(format!("saved -> {}", path.display()));
            }
            Err(e) => self.settings_note = Some(format!("save failed: {e}")),
        }
    }

    /// Push the active profile's resolved config to the net thread (which
    /// re-disarms remote egress + bumps the cfg-gen when the endpoint changes).
    /// Also tears down the deepen async state: an endpoint change bumps the net
    /// `oracle_log_gen`, orphaning any in-flight deepen-log fetch — without this
    /// the Consult face would hang on a permanent "gathering logs" spinner. The
    /// deepen set re-seeds (+ re-fires the default-on logs fetch) on the next
    /// draw.
    fn apply_active(&mut self, net: &Net) {
        let (cfg, _) =
            oracle_config::resolve_active(&self.config, None, None, self.env_token.as_deref());
        net.set_oracle_config(Some(cfg));
        net.clear_oracle_log();
        self.pending_log = None;
        self.deepen_log = None;
        self.want_consult = false;
        self.deepen_seeded = false;
        // An endpoint change invalidates the prior reply → its CONSULT NEXT links +
        // any error card.
        self.investigate.clear();
        self.reply_error = None;
    }

    /// Resolve the endpoint a test (level 1 or 2) should probe, applying the SAME
    /// egress gate for both: a LOCAL edit endpoint is probed directly (loopback,
    /// nothing leaves the box); a REMOTE one is allowed ONLY when it is the
    /// ACTIVE, ARMED endpoint (probing the active config — never an arbitrary
    /// edit-form URL while the arm is held for a different one). `Err(note)`
    /// explains why a remote test is blocked.
    fn resolve_test_target(&self, net: &Net) -> Result<LlmConfig, String> {
        let cfg = self.edit_profile().to_llm_config(self.env_token.as_deref());
        if cfg.endpoint == Endpoint::Local {
            return Ok(cfg);
        }
        let active = net.oracle_config();
        let armed = net.oracle_egress_armed();
        let same = active.as_ref().map(|c| c.base_url.as_str()) == Some(cfg.base_url.as_str());
        if armed
            && same
            && let Some(ac) = active
        {
            return Ok(ac);
        }
        Err(
            "remote: click \"Use this endpoint\", then Arm it (in the consult view), before testing"
                .into(),
        )
    }

    /// Reset the consult state (frozen consent / reply / suggestions) — called
    /// when the active endpoint changes so a payload framed for A never carries
    /// over to B.
    fn reset_consult(&mut self) {
        self.show_preview = false;
        self.frozen = None;
        self.reply = None;
        self.suggestions.clear();
        self.rejects.clear();
        self.staged.clear();
        self.audit_note = None;
        self.pending = None;
        self.scroll = 0.0;
    }

    /// Save the edit fields as a profile. Returns the saved index, or an error
    /// note. Does NOT change the active selection.
    fn save_edit(&mut self, net: &Net) -> Result<usize, String> {
        let p = self.edit_profile();
        if p.name.is_empty() {
            return Err("name is required".into());
        }
        if p.base_url.is_empty() {
            return Err("URL is required".into());
        }
        let editing = self.editing.unwrap_or(NEW_PROFILE);
        let dup = self
            .config
            .profiles
            .iter()
            .enumerate()
            .any(|(i, q)| q.name == p.name && (editing == NEW_PROFILE || i != editing));
        if dup {
            return Err(format!("a profile named \"{}\" already exists", p.name));
        }
        let idx = if editing == NEW_PROFILE {
            self.config.profiles.push(p.clone());
            self.config.profiles.len() - 1
        } else {
            let old_name = self.config.profiles[editing].name.clone();
            // If the active profile was renamed, follow it.
            if self.config.active.as_deref() == Some(old_name.as_str()) && old_name != p.name {
                self.config.active = Some(p.name.clone());
            }
            self.config.profiles[editing] = p.clone();
            editing
        };
        self.editing = Some(idx);
        self.persist();
        // If we just edited the active profile, push the change to the net config.
        if self.config.active.as_deref() == Some(p.name.as_str()) {
            self.apply_active(net);
        }
        Ok(idx)
    }

    /// Make the edited profile active (saving it first), apply it, and return to
    /// the Consult face.
    fn activate_edit(&mut self, net: &Net) {
        match self.save_edit(net) {
            Ok(idx) => {
                self.config.active = Some(self.config.profiles[idx].name.clone());
                self.persist();
                self.apply_active(net);
                self.reset_consult();
                self.face = OracleFace::Consult;
            }
            Err(e) => self.settings_note = Some(e),
        }
    }

    /// Delete the edited profile (two-click armed). If it was active, fall back to
    /// the built-in default.
    fn delete_edit(&mut self, net: &Net) {
        let Some(idx) = self.editing else { return };
        if idx == NEW_PROFILE {
            self.editing = None;
            return;
        }
        let name = self.config.profiles[idx].name.clone();
        self.config.profiles.remove(idx);
        if self.config.active.as_deref() == Some(name.as_str()) {
            self.config.active = None;
            self.apply_active(net);
            self.reset_consult();
        }
        self.persist();
        self.editing = None;
        self.delete_armed = false;
        self.settings_note = Some(format!("deleted \"{name}\""));
    }
}

impl OracleView {
    pub fn draw(
        &mut self,
        snap: Option<&Snapshot>,
        net: &Net,
        mouse: Vec2,
        click: bool,
    ) -> OracleAction {
        // Feed the focused Settings field (the single char-queue owner) FIRST so
        // typed text never reaches a global shortcut.
        if self.face == OracleFace::Settings
            && let Some(fid) = self.focus
        {
            self.field_mut(fid).update_focused();
            if is_key_pressed(KeyCode::Tab) {
                let nxt = match fid {
                    FieldId::Name => FieldId::Url,
                    FieldId::Url => FieldId::Model,
                    FieldId::Model => FieldId::Token,
                    FieldId::Token => FieldId::Timeout,
                    FieldId::Timeout => FieldId::Name,
                };
                self.focus = Some(nxt);
                crate::textfield::flush_char_queue();
            }
        }

        let cfg = net.oracle_config();
        let remote = cfg.as_ref().is_some_and(|c| c.endpoint == Endpoint::Remote);
        let armed = net.oracle_egress_armed();
        let arm_mode = remote && !armed;

        // Poll an in-flight consult.
        if let Some(h) = self.pending
            && let Some(r) = net.oracle_reply(h)
        {
            self.suggestions.clear();
            self.rejects.clear();
            self.staged.clear();
            self.follow_up.clear();
            self.investigate.clear();
            match &*r {
                OracleReply::Ok(t) => {
                    self.reply = Some(t.clone());
                    self.reply_error = None;
                    if let Some(s) = snap {
                        if let Some(env) = oracle_suggest::parse_suggestions(t) {
                            let (ok, rej) =
                                oracle_suggest::validate_envelope(&env, &s.hot.observed);
                            self.suggestions = ok;
                            self.rejects = rej;
                        }
                        // Parse the model's follow-up ranking, intersected with the
                        // lenses actually OFFERED for this scope (the security
                        // boundary — an unknown/injected key is a no-op).
                        let offered =
                            available_lenses(&s.hot.observed, &self.scopes[self.scope_idx]);
                        self.follow_up = parse_follow_up(t, &offered);
                        // CONSULT NEXT links = the app's attention queue (the floor,
                        // so a clear concern always yields a link) + the model's
                        // VALIDATED "investigate" extras (hallucinated/garbage
                        // dropped — the security boundary). The app curates; the
                        // model only adds.
                        let model = oracle_investigate::parse_investigate(t)
                            .map(|env| oracle_investigate::validate_envelope(&env, &s.hot.observed))
                            .unwrap_or_default();
                        self.investigate = self.merge_consult_next(model, &s.hot.models.attention);
                    }
                }
                OracleReply::Err(e) => {
                    // Errors render as a WARN card with a hint + Retry, not as fake
                    // answer prose.
                    self.reply = None;
                    self.reply_error = Some(e.clone());
                }
            }
            self.pending = None;
            self.scroll = 0.0;
        }

        let action_label = if self.pending.is_some() {
            "Cancel"
        } else if arm_mode {
            "Arm remote egress\u{2026}"
        } else if self.reply_error.is_some() {
            "Retry"
        } else {
            "Consult"
        };
        let buttons: Vec<&str> = match self.face {
            OracleFace::Consult => vec!["Settings\u{2026}", "Preview", action_label, "Close"],
            OracleFace::Settings => vec!["+ new", "Back", "Close"],
        };
        let win = draw_window(
            "Oracle of KuberNation — HOT",
            vec2(760.0, 580.0),
            &buttons,
            usize::MAX,
        );
        let b = win.body;

        // --- setup band (shared) -------------------------------------------
        let mut y = b.y + 4.0;
        for (line, role) in oracle_setup_lines(
            cfg.as_ref(),
            self.config.active.as_deref(),
            self.active_token_on_disk(),
        ) {
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
        y += 6.0;

        match self.face {
            OracleFace::Settings => self.draw_settings(net, b, y, mouse, click, &win),
            OracleFace::Consult => {
                self.draw_consult(snap, net, b, y, mouse, click, &win, &cfg, remote, arm_mode)
            }
        }
    }

    /// The endpoint-configuration face: profile list + edit form + model picker.
    #[allow(clippy::too_many_arguments)]
    fn draw_settings(
        &mut self,
        net: &Net,
        b: Rect,
        y0: f32,
        mouse: Vec2,
        click: bool,
        win: &crate::window::WinLayout,
    ) -> OracleAction {
        let mut y = y0;
        text_bold("ENDPOINT PROFILES", b.x, y + 12.0, 14.0, PARCHMENT);
        text(
            "(click to edit; > = active)",
            b.x + 170.0,
            y + 12.0,
            12.0,
            DIM,
        );
        y += 22.0;

        // Profile rows (left side), capped.
        let mut prof_rects: Vec<(usize, Rect)> = Vec::new();
        let rows = profile_rows(&self.config);
        if rows.is_empty() {
            text(
                "(no profiles — use \"+ new\")",
                b.x + 8.0,
                y + 12.0,
                13.0,
                DIM,
            );
            y += 18.0;
        }
        for (i, (label, role)) in rows.iter().enumerate().take(8) {
            let r = Rect::new(b.x, y, 250.0, 18.0);
            let editing_this = self.editing == Some(i);
            if editing_this {
                draw_rectangle(r.x, r.y, r.w, r.h, lighter(PLATE, 1.5));
            } else if r.contains(mouse) {
                draw_rectangle(r.x, r.y, r.w, r.h, lighter(PLATE, 1.2));
            }
            text(ascii(label), r.x + 4.0, y + 13.0, 13.0, role_color(*role));
            prof_rects.push((i, r));
            y += 19.0;
        }
        y += 8.0;

        // Edit form for the selected profile.
        let mut field_rects: Vec<(FieldId, Rect)> = Vec::new();
        let mut model_rects: Vec<(String, Rect)> = Vec::new();
        let mut discover_btn = Rect::new(0.0, 0.0, 0.0, 0.0);
        let mut chat_btn = Rect::new(0.0, 0.0, 0.0, 0.0);
        let mut activate_btn = Rect::new(0.0, 0.0, 0.0, 0.0);
        let mut save_btn = Rect::new(0.0, 0.0, 0.0, 0.0);
        let mut clear_btn = Rect::new(0.0, 0.0, 0.0, 0.0);
        let mut delete_btn = Rect::new(0.0, 0.0, 0.0, 0.0);

        if self.editing.is_some() {
            // Auto-discover a LOCAL profile's models once on select (a remote
            // profile waits for an explicit discover — its /v1/models GET sends
            // the token off-box). For a non-local profile, CLEAR the list so a
            // remote endpoint never shows the previously-discovered endpoint's
            // models (stale + clickable).
            if !self.models_attempted {
                self.models_attempted = true;
                // A prior profile's chat-test result must not linger on this one.
                net.clear_chat_test();
                let p = self.edit_profile();
                if !p.base_url.is_empty() && endpoint_kind(&p.base_url) == Endpoint::Local {
                    net.request_models(p.to_llm_config(self.env_token.as_deref()));
                } else {
                    net.clear_models();
                }
            }
            let labels = [
                (FieldId::Name, "name"),
                (FieldId::Url, "URL"),
                (FieldId::Model, "model"),
                (FieldId::Token, "token"),
                (FieldId::Timeout, "timeout"),
            ];
            for (id, lbl) in labels {
                text(lbl, b.x, y + 13.0, 13.0, DIM);
                let fr = Rect::new(b.x + 56.0, y, 360.0, 18.0);
                let focused = self.focus == Some(id);
                draw_rectangle(
                    fr.x,
                    fr.y,
                    fr.w,
                    fr.h,
                    if focused { lighter(PLATE, 1.6) } else { PLATE },
                );
                draw_rectangle_lines(
                    fr.x,
                    fr.y,
                    fr.w,
                    fr.h,
                    1.0,
                    if focused { PARCHMENT } else { DIM },
                );
                let disp = match id {
                    FieldId::Name => self.f_name.display(),
                    FieldId::Url => self.f_url.display(),
                    FieldId::Model => self.f_model.display(),
                    FieldId::Token => self.f_token.display(),
                    FieldId::Timeout => self.f_timeout.display(),
                };
                // The timeout field shows the default as placeholder when blank.
                let placeholder = if id == FieldId::Timeout {
                    "(default 180s)"
                } else {
                    "(empty)"
                };
                let shown = if disp.is_empty() && !focused {
                    placeholder.to_string()
                } else {
                    truncate(&disp, 48)
                };
                let col = if disp.is_empty() && !focused {
                    DIM
                } else {
                    INK
                };
                text(ascii(&shown), fr.x + 4.0, y + 13.0, 13.0, col);
                if focused {
                    let cw = text_size(truncate(&disp, 48).as_str(), 13.0).width;
                    draw_rectangle(fr.x + 5.0 + cw, fr.y + 3.0, 1.5, 12.0, PARCHMENT);
                }
                field_rects.push((id, fr));
                // URL row: live Local|Remote badge.
                if id == FieldId::Url {
                    let kind = endpoint_kind(self.f_url.buf.trim());
                    let (t, c) = match kind {
                        Endpoint::Local => ("LOCAL", GOOD),
                        Endpoint::Remote => ("REMOTE — publishing", WARN),
                    };
                    text(t, fr.x + fr.w + 8.0, y + 13.0, 12.0, c);
                }
                // Model row: two tests — "test" (level 1: GET /v1/models, fast)
                // and "chat" (level 2: a real tiny completion proving the model
                // generates).
                if id == FieldId::Model {
                    discover_btn = Rect::new(fr.x + fr.w + 8.0, fr.y, 56.0, 18.0);
                    draw_btn(discover_btn, "test", PARCHMENT, mouse);
                    chat_btn = Rect::new(fr.x + fr.w + 68.0, fr.y, 56.0, 18.0);
                    draw_btn(chat_btn, "chat", PARCHMENT, mouse);
                }
                // Token row: a clear button.
                if id == FieldId::Token {
                    clear_btn = Rect::new(fr.x + fr.w + 8.0, fr.y, 60.0, 18.0);
                    draw_btn(clear_btn, "clear", DIM, mouse);
                }
                // Timeout row: a units hint.
                if id == FieldId::Timeout {
                    text("seconds (5–600)", fr.x + fr.w + 8.0, y + 13.0, 12.0, DIM);
                }
                y += 22.0;
            }

            // Model picker rows (from the net discovery), click to set the model.
            let models = net.models_out();
            if let Some(Ok(list)) = &models {
                for m in list.iter().take(6) {
                    let cur = *m == self.f_model.buf;
                    let r = Rect::new(b.x + 56.0, y, 360.0, 16.0);
                    if r.contains(mouse) {
                        draw_rectangle(r.x, r.y, r.w, r.h, lighter(PLATE, 1.2));
                    }
                    let mark = if cur { "> " } else { "  " };
                    let role = if cur { GOOD } else { INK };
                    text(
                        ascii(&format!("{mark}{m}")),
                        r.x + 4.0,
                        y + 12.0,
                        12.0,
                        role,
                    );
                    model_rects.push((m.clone(), r));
                    y += 16.0;
                }
            } else {
                let line = model_picker_rows(models.as_ref(), &self.f_model.buf);
                if let Some((t, _)) = line.first() {
                    text(ascii(t), b.x + 56.0, y + 12.0, 12.0, DIM);
                    y += 16.0;
                }
            }

            // Test verdict: reachability + auth + model-availability in one line.
            // A landed probe (Some) clears the in-flight flag.
            if models.is_some() {
                self.testing = false;
            }
            if let Some((vtext, vrole)) =
                connection_verdict(models.as_ref(), self.f_model.buf.trim())
            {
                text(ascii(&vtext), b.x + 56.0, y + 13.0, 12.0, role_color(vrole));
                y += 16.0;
            } else if self.testing {
                text("test: testing\u{2026}", b.x + 56.0, y + 13.0, 12.0, DIM);
                y += 16.0;
            }

            // Chat-test verdict (level 2: a real completion landed).
            let chat_out = net.chat_test_out();
            if chat_out.is_some() {
                self.chat_testing = false;
            }
            if let Some((vtext, vrole)) = chat_verdict(chat_out.as_ref()) {
                text(ascii(&vtext), b.x + 56.0, y + 13.0, 12.0, role_color(vrole));
                y += 16.0;
            } else if self.chat_testing {
                text(
                    "chat test: sending\u{2026}",
                    b.x + 56.0,
                    y + 13.0,
                    12.0,
                    DIM,
                );
                y += 16.0;
            }
            y += 6.0;

            // Action buttons.
            activate_btn = Rect::new(b.x, y, 150.0, 20.0);
            draw_btn(activate_btn, "Use this endpoint", GOOD, mouse);
            save_btn = Rect::new(b.x + 160.0, y, 160.0, 20.0);
            draw_btn(save_btn, "Save (incl. token)", PARCHMENT, mouse);
            delete_btn = Rect::new(b.x + 330.0, y, 110.0, 20.0);
            let del_label = if self.delete_armed {
                "delete?"
            } else {
                "Delete"
            };
            draw_btn(delete_btn, del_label, WARN, mouse);
            y += 26.0;

            // Honest plaintext-on-disk disclosure.
            for (line, col) in [
                ("Save writes the token as PLAINTEXT to", DIM),
                (
                    oracle_config_io::config_path().to_string_lossy().as_ref(),
                    DIM,
                ),
                (
                    "(file mode 0600). Readable via your home dir, a backup, a disk",
                    DIM,
                ),
                (
                    "image, or a synced ~/.config. For high-sensitivity tokens prefer",
                    DIM,
                ),
                ("KUBERNATION_LLM_TOKEN (env, never persisted).", DIM),
            ] {
                text(line, b.x, y + 12.0, 12.0, col);
                y += 15.0;
            }
        }

        if let Some(note) = &self.settings_note {
            y += 4.0;
            text(ascii(note), b.x, y + 12.0, 13.0, GOOD);
        }

        // --- input ---------------------------------------------------------
        if click {
            // Profile rows.
            for (i, r) in &prof_rects {
                if r.contains(mouse) {
                    self.load_edit(*i);
                    return OracleAction::None;
                }
            }
            // Field focus (flush the stale char queue on acquire).
            for (id, r) in &field_rects {
                if r.contains(mouse) {
                    self.focus = Some(*id);
                    crate::textfield::flush_char_queue();
                    return OracleAction::None;
                }
            }
            // Model row → set the model field.
            for (m, r) in &model_rects {
                if r.contains(mouse) {
                    self.f_model = TextField::new(m, false);
                    return OracleAction::None;
                }
            }
            if self.editing.is_some() {
                if discover_btn.contains(mouse) {
                    // Level-1 test: GET /v1/models. Same egress gate for both
                    // tests (see resolve_test_target).
                    match self.resolve_test_target(net) {
                        Ok(cfg) => {
                            self.testing = true;
                            if cfg.endpoint == Endpoint::Remote {
                                self.settings_note = Some(write_test_audit(
                                    &cfg,
                                    "model discovery (GET /v1/models)",
                                ));
                            }
                            net.request_models(cfg);
                        }
                        Err(note) => self.settings_note = Some(note),
                    }
                    return OracleAction::None;
                }
                if chat_btn.contains(mouse) {
                    // Level-2 test: a real tiny chat completion (proves the model
                    // generates). Same gate; token-bearing for remote.
                    match self.resolve_test_target(net) {
                        Ok(cfg) => {
                            self.chat_testing = true;
                            if cfg.endpoint == Endpoint::Remote {
                                self.settings_note = Some(write_test_audit(
                                    &cfg,
                                    "chat test (POST /v1/chat/completions)",
                                ));
                            }
                            net.request_chat_test(cfg, oracle::chat_test_messages());
                        }
                        Err(note) => self.settings_note = Some(note),
                    }
                    return OracleAction::None;
                }
                if clear_btn.contains(mouse) {
                    self.f_token = TextField::new("", true);
                    self.settings_note = Some("token cleared (Save to persist)".into());
                    return OracleAction::None;
                }
                if activate_btn.contains(mouse) {
                    self.activate_edit(net);
                    return OracleAction::None;
                }
                if save_btn.contains(mouse) {
                    match self.save_edit(net) {
                        Ok(_) => {}
                        Err(e) => self.settings_note = Some(e),
                    }
                    return OracleAction::None;
                }
                if delete_btn.contains(mouse) {
                    if self.delete_armed {
                        self.delete_edit(net);
                    } else {
                        self.delete_armed = true;
                        self.settings_note = Some("click Delete again to confirm".into());
                    }
                    return OracleAction::None;
                }
            }
            // A click elsewhere in the modal clears field focus (keeps it open).
            if win.frame.contains(mouse) {
                self.focus = None;
            }
            match win.button_at(mouse) {
                Some(0) => {
                    // + new
                    self.load_edit(NEW_PROFILE);
                }
                Some(1) => {
                    // Back to Consult
                    self.face = OracleFace::Consult;
                    self.focus = None;
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

    /// The consult face (scope → preview → consult → reply/suggestions).
    #[allow(clippy::too_many_arguments)]
    fn draw_consult(
        &mut self,
        snap: Option<&Snapshot>,
        net: &Net,
        b: Rect,
        y0: f32,
        mouse: Vec2,
        click: bool,
        win: &crate::window::WinLayout,
        cfg: &Option<LlmConfig>,
        remote: bool,
        arm_mode: bool,
    ) -> OracleAction {
        // Seed default lenses (+ fire the crash-concern logs fetch) once a
        // snapshot is available.
        if !self.deepen_seeded {
            self.seed_deepen(snap, net);
        }
        // Poll the in-flight deepen-log fetch; fold it in when it lands (matching
        // the request so a stale fetch is ignored).
        if let Some(req) = self.pending_log.clone()
            && let Some((got, res)) = net.oracle_log_out()
            && got == req
        {
            self.pending_log = None;
            match res {
                Ok(tail) => self.deepen_log = Some(tail),
                Err(_) => {
                    // A fetch failure is NOT a budget drop — drop the Logs lens so
                    // the chip reverts to a clickable "include logs" (retry) rather
                    // than a misleading "dropped to fit — narrow scope".
                    self.deepen_log = None;
                    self.deepen.retain(|l| *l != DeepenLens::Logs);
                    self.explicit.retain(|l| *l != DeepenLens::Logs);
                    self.audit_note = Some("(log fetch failed — consulting without logs)".into());
                }
            }
            net.clear_oracle_log();
            self.apply_deepen_change();
        }

        let mut y = y0;
        // --- scope chip (◀ scope ▶) ---------------------------------------
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

        let built: Option<Built> = snap.zip(cfg.as_ref()).map(|(s, c)| {
            let scope = &self.scopes[self.scope_idx];
            let offered = available_lenses(&s.hot.observed, scope);
            // Logs are folded in only when the Logs lens is active AND its tail
            // has been fetched (the GUI requested it; deepen_log holds the result).
            let log_body = if self.deepen.contains(&DeepenLens::Logs) {
                self.deepen_log.as_deref()
            } else {
                None
            };
            let ctx = oracle::BundleCtx {
                cluster: &s.hot.observed.meta.context,
                log_body,
                slo: Some(&s.hot.slo),
                lenses: &self.deepen,
                explicit_lenses: &self.explicit,
            };
            let caps = if self.explicit.is_empty() {
                Caps::default()
            } else {
                Caps::deepened()
            };
            let (bundle, report) =
                oracle::build_bundle(&s.hot.models, &s.hot.observed, scope, &ctx, &caps);
            // Offer the "investigate" block (→ CONSULT NEXT links) only where the
            // model names OTHER drillable targets: realm (the prose list lives here)
            // and node (it may name stationed workloads). Off for workload/concern
            // (already narrowest — naming siblings is noise + injection surface).
            let offer_investigate = matches!(scope, Scope::Realm | Scope::Node(_));
            (
                bundle,
                report,
                c.model.clone(),
                c.base_url.clone(),
                c.endpoint == Endpoint::Remote,
                offered,
                offer_investigate,
            )
        });

        // A consult is deferred while a Logs fetch is in flight, so the FIRST
        // consult on a crash concern carries the logs once they land.
        let logs_pending = self.deepen.contains(&DeepenLens::Logs) && self.pending_log.is_some();

        // Dev auto-consult (`--oracle-go`): fire once (deferred past a logs fetch).
        if self.auto && self.pending.is_none() {
            self.auto = false;
            self.want_consult = true;
        }
        // Drain a deferred consult once logs are ready: LOCAL sends; REMOTE
        // re-Previews the enriched (now log-bearing) payload for re-consent.
        if self.want_consult && self.pending.is_none() && !logs_pending {
            self.want_consult = false;
            if remote {
                self.frozen = freeze(&built);
                self.show_preview = self.frozen.is_some();
                self.scroll = 0.0;
            } else if let Some(h) = self.dispatch(net, &built, cfg) {
                self.pending = Some(h);
                self.pending_started = get_time();
                self.reply_error = None;
                self.show_preview = false;
            }
        }
        if self.show_preview && self.frozen.is_none() {
            self.frozen = freeze(&built);
        }

        // Dev (`--oracle-suggest`): synthesize a deterministic validated suggestion.
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

        // Dev (`--oracle-investigate`): synthesize a deterministic CONSULT NEXT
        // target through the REAL validator (so the demo can't bypass the store
        // check), then stop at the rendered links (no auto-click — economy).
        if self.dev_investigate && self.reply.is_none() {
            self.dev_investigate = false;
            if let Some(s) = snap
                && let Some(wr) = s
                    .hot
                    .models
                    .workloads
                    .iter()
                    .find(|w| !kubernation_core::state::chaos::ns_protected(&w.r.namespace))
                    .map(|w| w.r.clone())
            {
                let env = oracle_investigate::InvestigateEnvelope {
                    investigate: vec![oracle_investigate::InvestigateJson {
                        kind: wr.kind.to_string(),
                        namespace: wr.namespace.clone(),
                        name: wr.name.clone(),
                        why: "(demo) worth a focused look".into(),
                    }],
                };
                self.reply = Some(
                    "(demo reply) Here is what I would investigate first — click a link to consult that object."
                        .into(),
                );
                let model = oracle_investigate::validate_envelope(&env, &s.hot.observed);
                self.investigate = self.merge_consult_next(model, &s.hot.models.attention);
            }
        }

        // --- body ----------------------------------------------------------
        // Reserve a footer strip for the pinned "model-generated; verify"
        // disclaimer whenever an answer/error is shown (kept outside the scroll so
        // the caveat can't scroll away — a posture reminder).
        let footer_on = self.reply.is_some() || self.reply_error.is_some();
        let footer_h = if footer_on { 22.0 } else { 0.0 };
        let mut stage_btns: Vec<(usize, Rect)> = Vec::new();
        let mut cx = Ctx {
            body: Rect::new(b.x, y, b.w, b.h - (y - b.y) - footer_h),
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
            let elapsed = (get_time() - self.pending_started).max(0.0) as u64;
            let timeout = cfg.as_ref().map(|c| c.timeout().as_secs()).unwrap_or(0);
            cx.row(&consult_progress_line(elapsed, timeout), DIM);
            cx.row("(local models can take a while — Cancel to stop)", DIM);
        } else if logs_pending {
            cx.row("gathering the pod's logs to include in the consult…", DIM);
        } else if let Some(err) = self.reply_error.clone() {
            cx.row("The consult could not complete:", WARN);
            cx.gap();
            for line in wrap(&err, 92) {
                cx.row(&ascii(&line), INK);
            }
            if let Some(hint) = error_hint(&err) {
                cx.gap();
                for line in wrap(hint, 92) {
                    cx.row(&ascii(&line), DIM);
                }
            }
            cx.gap();
            cx.row("Press Retry to try again, or open Settings.", DIM);
        } else if let Some(reply) = &self.reply {
            // Show the prose only — the machine-readable blocks (investigate /
            // suggestions / follow_up) are already rendered as links/buttons/chips
            // below, so strip them from the displayed answer.
            let shown = strip_machine_blocks(reply);
            if shown.trim().is_empty() {
                cx.row(
                    "(the answer is in the actions below — no extra prose returned)",
                    DIM,
                );
            } else {
                for line in wrap(&shown, 96) {
                    cx.row(&ascii(&line), INK);
                }
            }
            if let Some(note) = &self.audit_note {
                cx.gap();
                cx.row(&ascii(note), DIM);
            }
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
            cx.row(
                "  • Settings — pick a local model or point at a remote endpoint",
                DIM,
            );
            if remote {
                cx.row(
                    "  (remote endpoint — Preview is required before a Consult)",
                    DIM,
                );
            }
            if let Some((bundle, _, _, _, _, _, _)) = &built {
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

        // --- CONSULT NEXT (investigate links) ------------------------------
        // The model's "what to investigate first" list, validated against the live
        // store (hallucinated names dropped). Clicking JUMPS the scope to that
        // workload/node and re-consults — distinct from INVESTIGATE FURTHER, which
        // adds context to the SAME scope. The `why` is untrusted model output:
        // display-only (ascii()+truncate), never republished (the jump rebuilds
        // the bundle fresh from the world).
        let mut consult_btns: Vec<(Scope, Rect)> = Vec::new();
        if self.reply.is_some() && !self.investigate.is_empty() {
            cx.gap();
            cx.row("CONSULT NEXT — drill into one of these:", PARCHMENT);
            for t in &self.investigate {
                cx.y += 20.0;
                if !cx.visible() {
                    continue;
                }
                let txt = ascii(&truncate(&oracle_investigate::investigate_label(t, 64), 84));
                let x = cx.body.x + 8.0;
                let w = text_size(&txt, 13.0).width + 20.0;
                let r = Rect::new(x, cx.y - 13.0, w, 18.0);
                draw_btn(r, &txt, PARCHMENT, mouse);
                consult_btns.push((t.scope.clone(), r));
            }
        }

        // --- INVESTIGATE FURTHER (deepen chips) ----------------------------
        // After a reply, offer the app-curated lenses that fold more context in
        // and re-consult. Chips are derived from the ACTUAL bundle (deepen_chip_
        // states) so a budget-dropped lens reads "dropped", never a false receipt.
        let mut deepen_btns: Vec<(DeepenLens, Rect)> = Vec::new();
        if self.reply.is_some()
            && let Some((bundle, _, _, _, _, offered, _)) = &built
            && !offered.is_empty()
        {
            cx.gap();
            cx.row("INVESTIGATE FURTHER", PARCHMENT);
            let states = deepen_chip_states(bundle, offered, &self.deepen, self.fetching_lens());
            let state_of = |l: DeepenLens| states.iter().find(|(x, _)| *x == l).map(|(_, s)| *s);
            for (lens, highlight) in deepen_button_order(offered, &self.follow_up) {
                let st = state_of(lens).unwrap_or(LensState::Available);
                cx.y += 20.0;
                if !cx.visible() {
                    continue;
                }
                let x = cx.body.x + 8.0;
                match st {
                    LensState::Included => {
                        text(
                            ascii(&format!("v {}: included", lens.label())),
                            x,
                            cx.y,
                            13.0,
                            GOOD,
                        );
                    }
                    LensState::Fetching => {
                        text(
                            ascii(&format!("{}: gathering…", lens.label())),
                            x,
                            cx.y,
                            13.0,
                            DIM,
                        );
                    }
                    LensState::Available | LensState::Dropped => {
                        let (txt, col) = match st {
                            LensState::Dropped => (
                                format!("{} (dropped to fit — narrow scope)", lens.label()),
                                WARN,
                            ),
                            _ => {
                                let mark = if highlight { "> " } else { "" };
                                (format!("{mark}{}", lens.label()), PARCHMENT)
                            }
                        };
                        let w = text_size(&txt, 13.0).width + 20.0;
                        let r = Rect::new(x, cx.y - 13.0, w, 18.0);
                        draw_btn(r, &ascii(&txt), col, mouse);
                        deepen_btns.push((lens, r));
                    }
                }
            }
        }

        let content_h = cx.y - (y - self.scroll);
        self.max_scroll = (content_h - cx.body.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);

        // Pinned footer (outside the scroll). The "model-generated; verify" caveat
        // belongs ONLY to model output — the error card has no model prose to verify.
        if footer_on {
            let fy = b.y + b.h - 6.0;
            draw_line(
                b.x,
                b.y + b.h - footer_h + 2.0,
                b.x + b.w,
                b.y + b.h - footer_h + 2.0,
                1.0,
                PLATE,
            );
            if self.reply.is_some() {
                let warn = !self.suggestions.is_empty();
                let col = if warn { WARN } else { DIM };
                text(disclaimer_text(warn), b.x + 8.0, fy, 12.0, col);
                let hint = "c copy · w export";
                let hw = text_size(hint, 12.0).width;
                text(hint, b.x + b.w - hw - 8.0, fy, 12.0, DIM);
            } else {
                text(
                    "the consult failed — Retry, or open Settings.",
                    b.x + 8.0,
                    fy,
                    12.0,
                    DIM,
                );
            }
        }

        // --- input ---------------------------------------------------------
        // Copy / export the consult (the RAW reply, so the actions are reproducible).
        // Only with an answer on the Consult face and no Settings field focused.
        if self.face == OracleFace::Consult
            && !self.field_focused()
            && !self.show_preview
            && let Some(reply) = self.reply.clone()
        {
            if is_key_pressed(KeyCode::C) {
                return OracleAction::Copy(reply);
            }
            if is_key_pressed(KeyCode::W) {
                let header = format!(
                    "# Oracle consult — {}\n# endpoint: {}  ·  model: {}\n# model-generated; verify before acting.\n\n",
                    self.scopes[self.scope_idx].label(),
                    cfg.as_ref().map(|c| c.base_url.as_str()).unwrap_or("?"),
                    cfg.as_ref().map(|c| c.model.as_str()).unwrap_or("?"),
                );
                return OracleAction::Export(format!("{header}{reply}"));
            }
        }
        if click {
            for (i, br) in &stage_btns {
                if br.contains(mouse) {
                    self.staged.insert(*i);
                    return OracleAction::Stage(self.suggestions[*i].intervention.clone());
                }
            }
            // A deepen chip → fold in that lens + re-consult.
            for (lens, r) in &deepen_btns {
                if r.contains(mouse) {
                    self.add_lens(*lens, snap, net);
                    return OracleAction::None;
                }
            }
            // A CONSULT NEXT link → jump the scope to that validated target +
            // consult. (Purely Oracle-internal — the map `selected` is untouched.)
            for (scope, r) in &consult_btns {
                if r.contains(mouse) {
                    self.jump_to_scope(scope.clone(), snap, net);
                    return OracleAction::None;
                }
            }
            if self.scopes.len() > 1 && (prev.contains(mouse) || next.contains(mouse)) {
                let n = self.scopes.len();
                let delta = if prev.contains(mouse) { n - 1 } else { 1 };
                self.scope_idx = (self.scope_idx + delta) % n;
                // New scope → re-seed deepen lenses (+ fire its logs fetch) and
                // drop any in-flight fetch for the old scope's pod. (No auto-consult
                // on a chip switch — the operator clicks Consult.)
                self.reset_for_scope_switch(net);
                self.seed_deepen(snap, net);
                return OracleAction::None;
            }
            match win.button_at(mouse) {
                Some(0) => {
                    // Settings
                    self.enter_settings();
                }
                Some(1) => {
                    // Preview — freeze the consent snapshot.
                    self.frozen = freeze(&built);
                    self.show_preview = self.frozen.is_some();
                    self.scroll = 0.0;
                }
                Some(2) => {
                    if let Some(h) = self.pending {
                        // Cancel — stop waiting; bump the gen guard so a late reply
                        // lands nowhere + drop this hash's cache so a re-consult
                        // doesn't serve the cancelled reply. (A remote consult was
                        // already published — Cancel can't un-send.)
                        net.cancel_oracle(h);
                        self.pending = None;
                    } else if arm_mode {
                        net.arm_oracle_egress();
                    } else if remote && self.frozen.is_none() {
                        self.frozen = freeze(&built);
                        self.show_preview = true;
                        self.scroll = 0.0;
                    } else if let Some(sent) = self.dispatch(net, &built, cfg) {
                        // Consult / Retry — same path.
                        self.pending = Some(sent);
                        self.pending_started = get_time();
                        self.reply = None;
                        self.reply_error = None;
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

    /// Send a consult, preferring the frozen consent snapshot. A REMOTE consult is
    /// sent ONLY from a frozen preview and writes a one-shot egress audit.
    fn dispatch(
        &mut self,
        net: &Net,
        built: &Option<Built>,
        cfg: &Option<LlmConfig>,
    ) -> Option<u64> {
        if let Some(f) = self.frozen.take() {
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
        // No frozen preview: only a LOCAL endpoint may build-and-send fresh.
        if let Some((bundle, _, model, base_url, remote, offered, offer_investigate)) = built {
            if *remote {
                return None;
            }
            let messages = oracle::render_prompt(bundle, "", offered, *offer_investigate);
            let hash = oracle::bundle_hash(
                bundle,
                "",
                model,
                base_url,
                *remote,
                offered,
                *offer_investigate,
            );
            net.request_oracle(hash, messages);
            return Some(hash);
        }
        None
    }
}

/// A small filled button with a label, highlighted on hover.
fn draw_btn(r: Rect, label: &str, col: Color, mouse: Vec2) {
    let bg = if r.contains(mouse) {
        lighter(PLATE, 1.7)
    } else {
        PLATE
    };
    draw_rectangle(r.x, r.y, r.w, r.h, bg);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, col);
    let tw = text_size(label, 13.0).width;
    text(label, r.x + (r.w - tw) / 2.0, r.y + r.h - 5.0, 13.0, col);
}

/// Snapshot the current bundle into a `Frozen` consent record.
fn freeze(built: &Option<Built>) -> Option<Frozen> {
    built.as_ref().map(
        |(bundle, report, model, base_url, remote, offered, offer_investigate)| {
            let messages = oracle::render_prompt(bundle, "", offered, *offer_investigate);
            let wire_bytes =
                oracle::request_json(&oracle::chat_request(model, messages.clone())).len();
            Frozen {
                hash: oracle::bundle_hash(
                    bundle,
                    "",
                    model,
                    base_url,
                    *remote,
                    offered,
                    *offer_investigate,
                ),
                messages,
                preview: oracle::consent_preview(bundle, "", model, offered, *offer_investigate),
                wire_bytes,
                redacted: report.sections_masked,
            }
        },
    )
}

/// Write a one-shot, metadata-only egress audit for a remote consult.
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

/// Write a one-shot egress audit for a remote TEST (model-discovery GET or
/// chat-test POST) — both send the token off-box, same posture as a consult.
fn write_test_audit(cfg: &LlmConfig, scope: &str) -> String {
    let now = kubernation_core::util::now();
    let content = egress_audit_content(
        cfg,
        scope,
        0,
        0,
        &now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
    );
    let fname = format!("oracle-egress-{}.txt", now.strftime("%Y%m%d-%H%M%S"));
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join(&fname);
    match std::fs::write(&path, content) {
        Ok(_) => format!("{scope} (remote egress recorded -> {fname})"),
        Err(e) => format!("(test audit not written: {e})"),
    }
}

/// PURE: the audit record body. Records WHEN/WHERE/HOW MUCH — never the prompt,
/// the reply, or the API token. Unit-tested for the no-token invariant.
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
    fn setup_lines_show_profile_location_and_token_source_never_value() {
        let cfg = LlmConfig {
            base_url: "http://localhost:11434/v1".into(),
            model: "qwen3.5:35b".into(),
            api_key: Some("sk-DO-NOT-LEAK".into()),
            endpoint: Endpoint::Local,
            timeout_secs: 180,
        };
        let lines = oracle_setup_lines(Some(&cfg), Some("local Ollama"), true);
        let joined: String = lines
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            !joined.contains("DO-NOT-LEAK"),
            "the token value must never render"
        );
        assert!(joined.contains("profile: local Ollama"));
        assert!(joined.contains("token: on disk"));
        assert!(joined.contains("local"));
        // From-env token source when the active profile carries none.
        let env_lines = oracle_setup_lines(Some(&cfg), Some("local Ollama"), false);
        assert!(env_lines.iter().any(|(s, _)| s.contains("token: from env")));
        // A remote endpoint flags itself.
        let remote = LlmConfig {
            endpoint: Endpoint::Remote,
            ..cfg
        };
        let rl = oracle_setup_lines(Some(&remote), Some("corp"), true);
        assert!(
            rl.iter()
                .any(|(s, r)| s.contains("REMOTE") && *r == Role::Warn)
        );
        // No config → a single warn line.
        assert_eq!(oracle_setup_lines(None, None, false).len(), 1);
    }

    #[test]
    fn profile_rows_marks_active_and_flags_remote() {
        let config = OracleConfigFile {
            version: 1,
            profiles: vec![
                Profile::local_default(),
                Profile {
                    name: "corp".into(),
                    base_url: "https://api.corp/v1".into(),
                    model: "gpt-4o".into(),
                    token: Some("x".into()),
                    timeout_secs: None,
                },
            ],
            active: Some("corp".into()),
        };
        let rows = profile_rows(&config);
        assert_eq!(rows.len(), 2);
        // The active "corp" row is marked + flagged REMOTE + Good role.
        let corp = rows.iter().find(|(s, _)| s.contains("corp")).unwrap();
        assert!(corp.0.starts_with("> "));
        assert!(corp.0.contains("REMOTE"));
        assert_eq!(corp.1, Role::Good);
        // The inactive local row is not marked + not flagged.
        let local = rows.iter().find(|(s, _)| s.contains("Ollama")).unwrap();
        assert!(!local.0.contains("REMOTE"));
        assert_eq!(local.1, Role::Body);
    }

    #[test]
    fn consult_progress_line_shows_elapsed_and_timeout() {
        assert_eq!(
            consult_progress_line(7, 180),
            "consulting the Oracle… 7s (timeout 180s)"
        );
        // No timeout known → just the elapsed.
        assert!(!consult_progress_line(3, 0).contains("timeout"));
    }

    #[test]
    fn disclaimer_text_sharpens_with_a_suggestion() {
        assert!(disclaimer_text(false).contains("verify"));
        let with = disclaimer_text(true);
        assert!(with.contains("VERIFY") && with.contains("stag"));
        assert_ne!(disclaimer_text(false), disclaimer_text(true));
    }

    #[test]
    fn error_hint_maps_the_common_failures() {
        // Assert against the REAL LlmError Display strings (k8s/oracle_client.rs).
        assert!(
            error_hint("the model did not respond in time")
                .unwrap()
                .contains("timeout")
        );
        assert!(
            error_hint("the endpoint rejected the API token (401/403)")
                .unwrap()
                .contains("token")
        );
        // A not-pulled model surfaces as an Ollama 404 via BadStatus.
        assert!(
            error_hint("HTTP 404: model 'qwen3:30b' not found")
                .unwrap()
                .contains("pull")
        );
        assert!(
            error_hint("could not reach the model endpoint: connection refused")
                .unwrap()
                .contains("unreachable")
        );
        assert!(
            error_hint("rate limited by the endpoint (429)")
                .unwrap()
                .contains("rate limited")
        );
        // An unclassifiable error → no specific hint (the raw error still shows).
        assert!(error_hint("could not read the model response: bad json").is_none());
    }

    #[test]
    fn connection_verdict_validates_endpoint_auth_and_model() {
        // Not run yet.
        assert!(connection_verdict(None, "gpt-4o").is_none());
        // A classified error (e.g. 401) → FAILED.
        let err: Result<Arc<Vec<String>>, String> =
            Err("the endpoint rejected the API token (401/403)".into());
        let (t, r) = connection_verdict(Some(&err), "gpt-4o").unwrap();
        assert!(t.contains("FAILED") && t.contains("401"));
        assert_eq!(r, Role::Warn);
        // Reachable + the model is in the list → OK (Good).
        let ok: Result<Arc<Vec<String>>, String> =
            Ok(Arc::new(vec!["qwen3.5:35b".into(), "gpt-4o".into()]));
        let (t, r) = connection_verdict(Some(&ok), "gpt-4o").unwrap();
        assert!(t.contains("OK") && t.contains("available"));
        assert_eq!(r, Role::Good);
        // Reachable but the model is NOT available → Warn (actionable).
        let (t, r) = connection_verdict(Some(&ok), "llama3.1").unwrap();
        assert!(t.contains("NOT available") && t.contains("llama3.1"));
        assert_eq!(r, Role::Warn);
    }

    #[test]
    fn chat_verdict_reports_generation_outcome() {
        assert!(chat_verdict(None).is_none());
        let err: Result<String, String> = Err("the model did not respond in time".into());
        let (t, r) = chat_verdict(Some(&err)).unwrap();
        assert!(t.contains("FAILED") && t.contains("did not respond"));
        assert_eq!(r, Role::Warn);
        let ok: Result<String, String> = Ok("OK".into());
        let (t, r) = chat_verdict(Some(&ok)).unwrap();
        assert!(t.contains("OK") && t.contains("replied"));
        assert_eq!(r, Role::Good);
    }

    #[test]
    fn model_picker_rows_states() {
        assert_eq!(model_picker_rows(None, "x").len(), 1); // hint
        let err: Result<Arc<Vec<String>>, String> = Err("offline".into());
        assert!(model_picker_rows(Some(&err), "x")[0].0.contains("offline"));
        let ok: Result<Arc<Vec<String>>, String> = Ok(Arc::new(vec!["a".into(), "b".into()]));
        let rows = model_picker_rows(Some(&ok), "b");
        assert!(
            rows.iter()
                .any(|(s, r)| s.contains("> b") && *r == Role::Good)
        );
        assert!(rows.iter().any(|(s, _)| s.contains("  a")));
    }

    #[test]
    fn egress_audit_records_metadata_never_the_token() {
        let cfg = LlmConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o".into(),
            api_key: Some("sk-SUPER-SECRET-TOKEN".into()),
            endpoint: Endpoint::Remote,
            timeout_secs: 180,
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
        let hard = wrap("xxxxxxxxxx", 4);
        assert_eq!(hard.join(""), "xxxxxxxxxx");
    }
}
