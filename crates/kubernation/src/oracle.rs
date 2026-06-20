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
use kubernation_core::state::oracle::{self, Caps, Scope};
use kubernation_core::state::oracle_config::{
    self, DEFAULT_LLM_MODEL, DEFAULT_LLM_URL, OracleConfigFile, Profile, endpoint_kind,
};
use kubernation_core::state::oracle_suggest::{self, ValidatedSuggestion};
use kubernation_core::state::planned::Intervention;
use macroquad::prelude::*;

use crate::net::{Net, OracleReply, Snapshot};
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
/// (bundle, redaction report, model, base_url, is_remote).
type Built = (
    oracle::ContextBundle,
    oracle::RedactionReport,
    String,
    String,
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
    reply: Option<String>,
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
            reply: None,
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
            focus: None,
            delete_armed: false,
            settings_note: None,
            models_attempted: false,
            testing: false,
        }
    }

    /// Dev: auto-consult on the next draw (the `--oracle-go` headless round-trip).
    pub fn auto_consult(&mut self) {
        self.auto = true;
    }

    /// Dev: open the Settings face (the `--oracle-settings` headless capture).
    pub fn open_settings(&mut self) {
        self.enter_settings();
    }

    /// True once a consult has produced a reply (success OR error).
    pub fn reply_landed(&self) -> bool {
        self.reply.is_some()
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
        let p = if idx == NEW_PROFILE {
            Profile {
                name: "new endpoint".into(),
                base_url: DEFAULT_LLM_URL.into(),
                model: DEFAULT_LLM_MODEL.into(),
                token: None,
            }
        } else {
            self.config.profiles[idx].clone()
        };
        self.f_name = TextField::new(&p.name, false);
        self.f_url = TextField::new(&p.base_url, false);
        self.f_model = TextField::new(&p.model, false);
        self.f_token = TextField::new(p.token.as_deref().unwrap_or(""), true);
    }

    /// The Profile currently being composed in the edit fields.
    fn edit_profile(&self) -> Profile {
        let tok = self.f_token.buf.trim();
        Profile {
            name: self.f_name.buf.trim().to_string(),
            base_url: self.f_url.buf.trim().to_string(),
            model: self.f_model.buf.trim().to_string(),
            token: if tok.is_empty() {
                None
            } else {
                Some(tok.to_string())
            },
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
    fn apply_active(&self, net: &Net) {
        let (cfg, _) =
            oracle_config::resolve_active(&self.config, None, None, self.env_token.as_deref());
        net.set_oracle_config(Some(cfg));
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
                    FieldId::Token => FieldId::Name,
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
            match &*r {
                OracleReply::Ok(t) => {
                    self.reply = Some(t.clone());
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

        let action_label = if arm_mode {
            "Arm remote egress\u{2026}"
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
                };
                let shown = if disp.is_empty() && !focused {
                    "(empty)".to_string()
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
                // Model row: a test/discover button (probe the endpoint + list
                // its models).
                if id == FieldId::Model {
                    discover_btn = Rect::new(fr.x + fr.w + 8.0, fr.y, 80.0, 18.0);
                    draw_btn(discover_btn, "test", PARCHMENT, mouse);
                }
                // Token row: a clear button.
                if id == FieldId::Token {
                    clear_btn = Rect::new(fr.x + fr.w + 8.0, fr.y, 60.0, 18.0);
                    draw_btn(clear_btn, "clear", DIM, mouse);
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
                    let p = self.edit_profile();
                    let cfg = p.to_llm_config(self.env_token.as_deref());
                    if cfg.endpoint == Endpoint::Local {
                        // Loopback — listing models sends nothing off-box.
                        self.testing = true;
                        net.request_models(cfg);
                    } else {
                        // Remote: token-bearing egress. Only the ACTIVE, ARMED
                        // endpoint may be probed (never a different edit-form URL
                        // while the arm is held for another) — probe the active
                        // config, and record the egress like a consult.
                        let active = net.oracle_config();
                        let armed = net.oracle_egress_armed();
                        let same = active.as_ref().map(|c| c.base_url.as_str())
                            == Some(cfg.base_url.as_str());
                        if armed
                            && same
                            && let Some(ac) = active
                        {
                            self.testing = true;
                            self.settings_note = Some(write_discovery_audit(&ac));
                            net.request_models(ac);
                        } else {
                            self.settings_note = Some(
                                "remote: click \"Use this endpoint\", then Arm it (in the consult \
                                 view), before listing its models"
                                    .into(),
                            );
                        }
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
                c.base_url.clone(),
                c.endpoint == Endpoint::Remote,
            )
        });

        // Dev auto-consult (`--oracle-go`): fire once.
        if self.auto && self.pending.is_none() {
            self.auto = false;
            if let Some(h) = self.dispatch(net, &built, cfg) {
                self.pending = Some(h);
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

        // --- body ----------------------------------------------------------
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
            if let Some((bundle, _, _, _, _)) = &built {
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

        // --- input ---------------------------------------------------------
        if click {
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
                    if arm_mode {
                        net.arm_oracle_egress();
                    } else if self.pending.is_none() {
                        if remote && self.frozen.is_none() {
                            self.frozen = freeze(&built);
                            self.show_preview = true;
                            self.scroll = 0.0;
                        } else if let Some(sent) = self.dispatch(net, &built, cfg) {
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
        if let Some((bundle, _, model, base_url, remote)) = built {
            if *remote {
                return None;
            }
            let messages = oracle::render_prompt(bundle, "");
            let hash = oracle::bundle_hash(bundle, "", model, base_url, *remote);
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
    built
        .as_ref()
        .map(|(bundle, report, model, base_url, _remote)| {
            let messages = oracle::render_prompt(bundle, "");
            let wire_bytes =
                oracle::request_json(&oracle::chat_request(model, messages.clone())).len();
            Frozen {
                hash: oracle::bundle_hash(bundle, "", model, base_url, *_remote),
                messages,
                preview: oracle::consent_preview(bundle, "", model),
                wire_bytes,
                redacted: report.sections_masked,
            }
        })
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

/// Write a one-shot egress audit for a remote model-discovery GET (it sends the
/// token off-box too — same posture as a consult, smaller payload).
fn write_discovery_audit(cfg: &LlmConfig) -> String {
    let now = kubernation_core::util::now();
    let content = egress_audit_content(
        cfg,
        "model discovery (GET /v1/models)",
        0,
        0,
        &now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
    );
    let fname = format!("oracle-egress-{}.txt", now.strftime("%Y%m%d-%H%M%S"));
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join(&fname);
    match std::fs::write(&path, content) {
        Ok(_) => format!("listing models (remote egress recorded -> {fname})"),
        Err(e) => format!("(discovery audit not written: {e})"),
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
