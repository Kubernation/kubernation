//! Kubernation: the observed world rendered as a windowed strategy map — the
//! pure `kubernation-core` models painted with macroquad. (This is the product;
//! a ratatui TUI over the same core was removed 2026-06-18.)
//! With `--warm`, the standby cluster appears as a second archipelago
//! east of the hot one, with sync chips on every city.
//!
//!   make run            # hot only
//!   make pair           # hot + warm
//!
//! Controls: WASD/arrows or right-drag pan · wheel zoom · hover for
//! tooltips · click to inspect (city / province window) · ]/[ sail
//! between cities · N fly to the next concern · L tail its logs ·
//! `:` resource browser · ?/F1 Almanac (in-app reference) · Esc close · Q quit.

mod advisor;
mod almanac;
mod browse;
mod chaos;
mod charter;
mod city;
mod draw;
mod inspect;
mod logging;
mod logo;
mod menu;
mod net;
mod node;
mod panels;
mod plan;
mod sidebar;
mod text;
mod theme;
mod window;

use std::path::PathBuf;

use advisor::{Advisor, AdvisorAction, AdvisorTab};
use almanac::{Almanac, AlmanacAction};
use browse::{BrowseAction, Browser};
use chaos::{Chaos, ChaosAction};
use charter::{CharterAction, CharterView};
use clap::Parser;
use draw::{
    Camera, Overlay, SceneWorld, draw_blast, draw_sea, draw_selection, draw_world, locate,
    minimap_layout, scene, scene_size,
};
use inspect::Inspector;
use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::{Concern, Target};
use kubernation_core::state::blast::{Subject, blast_radius};
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::WorkloadRef;
use kubernation_core::state::observed::ObservedWorld;
use kubernation_core::state::slo;
use kubernation_core::state::world::Region;
use macroquad::prelude::*;
use menu::{MenuAction, MenuCtx};
use net::{EvictReq, ForwardReq, LogReq};
use panels::{
    Panel, draw_chaos_confirm, draw_commit_confirm, draw_evict_confirm, draw_logs, draw_tooltip,
};
use text::{text, text_size};
use theme::*;

#[derive(Debug, Parser)]
#[command(name = "kubernation", version, about)]
struct Args {
    /// Kubeconfig context (defaults to current-context)
    #[arg(long)]
    context: Option<String>,
    /// Warm-standby context: a second archipelago with sync chips
    #[arg(long)]
    warm: Option<String>,
    /// Path to kubeconfig
    #[arg(long)]
    kubeconfig: Option<PathBuf>,
    /// Log level for the file log (RUST_LOG overrides it)
    #[arg(long, default_value = "info")]
    log_level: String,
    /// Project a CRD's instances onto the map (repeatable)
    #[arg(long = "project", value_name = "CRD")]
    project: Vec<String>,

    /// Render until synced, save a PNG, exit (development verification)
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// On sync, select the first city whose name contains this and open
    /// its panel (development verification)
    #[arg(long)]
    inspect: Option<String>,
    /// Open the context picker on sync (development verification)
    #[arg(long)]
    pick: bool,
    /// Override the initial zoom after fit (development verification)
    #[arg(long)]
    zoom: Option<f32>,
    /// After --inspect opens a panel, tail its first pod's logs (verification)
    #[arg(long)]
    tail: bool,
    /// Open the Almanac (in-app reference) on sync (development verification)
    #[arg(long)]
    almanac: bool,
    /// Stage a demo scale + cordon and open the End-of-Turn review on sync
    /// (development verification)
    #[arg(long)]
    plan: bool,
    /// Stage a rollback of the first matching Deployment (SUBSTR) to its
    /// previous revision and open the End-of-Turn review (dev verification of
    /// the rollback verb; pair with --plan-go to commit it).
    #[arg(long, value_name = "SUBSTR")]
    rollback: Option<String>,
    /// Center the camera on a named city / node / island at --zoom (default
    /// 1.4) without opening a panel, so coast & island marks render
    /// (development verification of map shots)
    #[arg(long)]
    center: Option<String>,
    /// With --center, shift the framed point east (+) / west (−) by N cells —
    /// e.g. to frame a city's offshore harbors (development verification)
    #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
    pan_dx: i32,
    /// Open the first matching workload's city and raise the evict confirm on
    /// its first pod (development verification of the eviction UI)
    #[arg(long)]
    evict: Option<String>,
    /// With --evict, auto-confirm the eviction (REALLY deletes the pod) —
    /// development verification of the write path
    #[arg(long)]
    evict_go: bool,
    /// Start a port-forward on the first matching workload's first pod and stay
    /// on the map (so the FORWARDS column section shows) — dev verification
    #[arg(long, value_name = "SUBSTR")]
    forward: Option<String>,
    /// Select the first node/city matching SUBSTR and turn on the blast-radius
    /// overlay (dev verification of the dependency fan-out highlight)
    #[arg(long, value_name = "SUBSTR")]
    blast: Option<String>,
    /// Open the Game Day chaos window pre-targeted at the first workload
    /// matching SUBSTR (development verification of the chaos UI)
    #[arg(long, value_name = "SUBSTR")]
    chaos: Option<String>,
    /// With --chaos, auto-run the drill (REALLY injects the failure) — dev
    /// verification of the chaos write path
    #[arg(long)]
    chaos_go: bool,
    /// With --chaos-go, which experiment to run (kill-one [default] / kill-all /
    /// outage / broken-image / partition / node-failure). node-failure targets
    /// the first non-control-plane node hosting the --chaos target's pods.
    #[arg(long, value_name = "KIND")]
    chaos_exp: Option<String>,
    /// With --chaos, preselect a compound tier (skirmish / raid / siege); with
    /// --chaos-go, auto-run it on the --chaos target. Overrides --chaos-exp.
    #[arg(long, value_name = "TIER")]
    chaos_tier: Option<String>,
    /// Hold the intro splash (the full Kubernation scene) — replays it, and
    /// with --screenshot captures it (development verification / demo)
    #[arg(long)]
    splash: bool,
    /// With --plan, auto-commit the staged turn (REALLY applies scale/cordon)
    /// — development verification of the apply path
    #[arg(long)]
    plan_go: bool,
    /// With --tail, open the log overlay on the *previous* container
    /// (development verification of the --previous toggle)
    #[arg(long)]
    log_previous: bool,
    /// With --tail, pre-fill the log filter with this substring
    /// (development verification of the grep/filter)
    #[arg(long, value_name = "SUBSTR")]
    log_filter: Option<String>,
    /// With --tail, open with timestamps on (development verification).
    #[arg(long)]
    log_timestamps: bool,
    /// Open the first concern's pod logs via the `L` jump (dev verification).
    #[arg(long)]
    concern_logs: bool,
    /// Launch scoped to a single namespace (the namespace filter; you can
    /// still change it from the World menu). Also used for verification.
    #[arg(long, value_name = "NS")]
    namespace: Option<String>,
    /// Global default SLO availability target — a percent ("99.9") or fraction
    /// ("0.999"). Per-workload overrides come from the
    /// `kubernation.io/slo-target` annotation or the city-window stepper.
    #[arg(long, value_name = "PCT")]
    slo_target: Option<String>,
    /// Start with a map overlay active: "terrain" (default), "pressure"
    /// (cpu/mem heat), "replicas" (workload health) or "namespace" (territory).
    /// Set from the View menu at runtime; flag is for shots.
    #[arg(long, value_name = "MODE")]
    overlay: Option<String>,
    /// Open a chrome menu on sync — game / view / orders / advisors / world /
    /// help (development verification of the menu bar dropdowns)
    #[arg(long, value_name = "NAME")]
    menu: Option<String>,
    /// Open an advisor screen on sync — health / storage / network / rightsizing
    /// (development verification of the advisor windows)
    #[arg(long, value_name = "NAME")]
    advisor: Option<String>,
    /// Open the Charter (self-scoped RBAC) on sync, optionally scoped to a
    /// namespace (development verification of the RBAC matrix)
    #[arg(long, value_name = "NS", num_args = 0..=1, default_missing_value = "")]
    charter: Option<String>,
    /// With --inspect, also open the object inspector (YAML) on the inspected
    /// city/node (development verification of the inspector)
    #[arg(long)]
    yaml: bool,
    /// With --inspect <node>, hold the screenshot long enough for metrics-server
    /// to accumulate a few samples so the trend sparklines render (dev verif).
    #[arg(long)]
    spark: bool,
    /// Open the resource browser on sync; with a value, select that kind's
    /// table (e.g. "configmaps") — development verification of the browser
    #[arg(long, value_name = "KIND", num_args = 0..=1, default_missing_value = "")]
    browse: Option<String>,
}

/// A chaos drill awaiting its confirm modal — the run to submit plus the
/// confirm copy (so the confirm is built once, when the drill is chosen).
struct PendingChaos {
    run: net::ChaosRun,
    title: String,
    line1: String,
    line2: String,
    action: String,
}

/// Build the net-thread `ChaosRun` for a (non-refused) experiment + plan:
/// resolve the subject, the readiness-watch set, and the scorecard class. Shared
/// by the Game Day window's Run handler and the `--chaos-go` dev path.
fn build_chaos_run(
    observed: &ObservedWorld,
    exp: &kubernation_core::state::chaos::Experiment,
    plan: &kubernation_core::state::chaos::ChaosPlan,
    auto_restore_secs: Option<f64>,
) -> net::ChaosRun {
    use kubernation_core::state::chaos::{ChaosStep, Experiment, ScoreKind};
    let subject = exp.subject();
    let (watch, score_kind) = match &subject {
        Subject::Workload(wr) => {
            let kind = if matches!(exp, Experiment::Partition { .. }) {
                ScoreKind::Isolation
            } else {
                ScoreKind::Workload
            };
            (vec![wr.clone()], kind)
        }
        Subject::Node(node) => {
            // The node's hosted workloads are what recovers; watch their
            // readiness. Match the *drained* population (see `pods_on_node`):
            // skip system namespaces, and skip DaemonSets — a DS pod reschedules
            // onto the same cordoned node, so its counts are noise, not recovery.
            use kubernation_core::state::model::WorkloadKind;
            let watch: Vec<WorkloadRef> = blast_radius(observed, &Subject::Node(node.clone()))
                .items
                .iter()
                .filter_map(|b| match &b.item {
                    kubernation_core::state::blast::Affected::Workload(w) => Some(w.clone()),
                    _ => None,
                })
                .filter(|w| {
                    !kubernation_core::state::chaos::ns_protected(&w.namespace)
                        && w.kind != WorkloadKind::DaemonSet
                })
                .collect();
            let pods_drained = plan
                .steps
                .iter()
                .filter(|st| matches!(st, ChaosStep::Evict { .. }))
                .count();
            (
                watch,
                ScoreKind::Node {
                    pods_drained,
                    cordoned: true,
                },
            )
        }
    };
    net::ChaosRun {
        cluster: ClusterId::Hot,
        experiment: exp.label().to_string(),
        subject,
        score_kind,
        blast: plan.blast,
        steps: plan.steps.clone(),
        restore: plan.restore.clone(),
        watch,
        auto_restore_secs,
        is_restore: false,
    }
}

/// Parse the dev `--chaos-tier` flag value.
fn parse_tier(s: &str) -> Option<kubernation_core::state::chaos::Tier> {
    use kubernation_core::state::chaos::Tier;
    match s {
        "skirmish" => Some(Tier::Skirmish),
        "raid" => Some(Tier::Raid),
        "siege" => Some(Tier::Siege),
        _ => None,
    }
}

/// The namespace the Charter focuses on: a single active filter namespace if one
/// is selected, else `default` (if present) or the first observed namespace.
fn charter_focus_ns(filter: &NamespaceFilter, snap: Option<&net::Snapshot>) -> String {
    if let NamespaceFilter::Only(set) = filter
        && set.len() == 1
    {
        return set.iter().next().cloned().unwrap();
    }
    let nss: Vec<String> = snap
        .map(|s| s.hot.observed.namespaces().into_iter().collect())
        .unwrap_or_default();
    if nss.iter().any(|n| n == "default") {
        "default".to_string()
    } else {
        nss.first()
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    }
}

/// Build the net-thread `ChaosRun` for a compound difficulty tier. Tiers always
/// target a workload, so the subject + scorecard are Workload and the watch set
/// is the target itself (the dip/recover model fits the kills the tiers do).
fn build_tier_run(
    target: &WorkloadRef,
    tier: kubernation_core::state::chaos::Tier,
    plan: &kubernation_core::state::chaos::ChaosPlan,
    auto_restore_secs: Option<f64>,
) -> net::ChaosRun {
    net::ChaosRun {
        cluster: ClusterId::Hot,
        experiment: tier.label().to_string(),
        subject: Subject::Workload(target.clone()),
        score_kind: kubernation_core::state::chaos::ScoreKind::Workload,
        blast: plan.blast,
        steps: plan.steps.clone(),
        restore: plan.restore.clone(),
        watch: vec![target.clone()],
        auto_restore_secs,
        is_restore: false,
    }
}

/// Focus concern `idx`: park the cursor on it, fly the camera there, and open
/// its drill-down — the single path both the `N` key and a click on the column's
/// ATTENTION section take, so keyboard and mouse can't drift. A `WorkloadList`
/// concern (no map cell) just updates `concern_idx`.
fn focus_concern(
    idx: usize,
    worlds: &[SceneWorld],
    attention: &[Concern],
    concern_idx: &mut usize,
    selected: &mut Option<(u16, u16)>,
    cam: &mut Camera,
    panel: &mut Option<Panel>,
) {
    let Some(concern) = attention.get(idx) else {
        return;
    };
    *concern_idx = idx;
    if let Some(sw) = worlds.iter().find(|w| w.id == concern.cluster) {
        let local = match &concern.target {
            Target::Workload(r) => sw.world.city_pos(r).or_else(|| sw.world.structure_pos(r)),
            Target::Node(name) => sw.world.province_pos(name),
            Target::WorkloadList => None,
        };
        if let Some(p) = local {
            let global = (p.0 + sw.off, p.1);
            *selected = Some(global);
            cam.fly_to(global);
            *panel = match &concern.target {
                Target::Workload(r) => Some(Panel::City(sw.id, r.clone())),
                Target::Node(name) => Some(Panel::Node(sw.id, name.clone())),
                Target::WorkloadList => None,
            };
        }
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Kubernation".into(),
        window_width: 1380,
        window_height: 860,
        high_dpi: true,
        icon: logo::window_icon(),
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = Args::parse();
    // Capture core's tracing diagnostics to a file (the window has no console).
    if let Err(e) = logging::init(&args.log_level) {
        eprintln!("kubernation: could not open the log file: {e}");
    }
    text::init();
    logo::init();
    let shot = args.screenshot.clone();
    let inspect = args.inspect.clone();
    let want_warm = args.warm.is_some();
    let net = net::Net::new();
    let slo_default = args
        .slo_target
        .as_deref()
        .and_then(|s| slo::parse_target(s).ok())
        .unwrap_or(slo::DEFAULT_TARGET);
    net::spawn(
        net::NetArgs {
            context: args.context.clone(),
            kubeconfig: args.kubeconfig.clone(),
            warm: args.warm.clone(),
            projections: args.project.clone(),
            slo_default,
        },
        net.clone(),
    );
    if let Some(ns) = &args.namespace {
        net.set_namespace_filter(NamespaceFilter::only(ns.clone()));
    }

    let mut cam = Camera::new();
    let mut selected: Option<(u16, u16)> = None;
    let mut panel: Option<Panel> = None;
    let mut concern_idx: usize = 0;
    let mut city_idx: usize = 0;
    // Blast-radius overlay: when on, highlight the dependency fan-out of the
    // selected tile (else the focused concern's subject). Toggled with `B`.
    let mut blast_on = args.blast.is_some();
    let mut blast_armed = args.blast.is_some();
    let mut chaos_armed = args.chaos.is_some();
    // Memoized blast radius: (cluster, subject, result), recomputed only when
    // the subject or the snapshot changes (keyed by the snapshot's Arc pointer).
    let mut blast_cache: Option<(
        ClusterId,
        Subject,
        kubernation_core::state::blast::BlastRadius,
    )> = None;
    let mut blast_cache_snap: usize = 0;
    let mut frames_synced: u32 = 0;
    let mut prev_had_snap = false;
    let mut inspected = false;
    // Fire the --forward dev verification once.
    let mut forward_armed = args.forward.is_some();
    let mut drag_anchor: Option<Vec2> = None;
    let mut picker = false;
    let mut picker_idx = 0usize;
    // Namespace-filter picker (single-select: "all" or one namespace).
    let mut ns_picker = false;
    let mut ns_picker_idx = 0usize;
    // Dragging the minimap viewport box to recenter the main view.
    let mut minimap_drag = false;
    // The Civ-style chrome menu bar: which top-level menu is open (None = all
    // closed). An open menu suspends map navigation, like the other modals.
    let mut open_menu: Option<usize> = None;
    // The active map overlay (the View menu's "map display"): how terrain is
    // colored. A --overlay dev flag seeds it for headless shots.
    let mut overlay = match args.overlay.as_deref() {
        Some("pressure") => Overlay::Pressure,
        Some("replicas") => Overlay::Replicas,
        Some("namespace") => Overlay::Namespace,
        _ => Overlay::Terrain,
    };
    // Menu "Fit view" can't reach `bounds` from the chrome draw, so it defers
    // the camera fit to the next frame's input block (where bounds is in scope).
    let mut pending_fit = false;
    // While Some, the city window's image field is capturing a new image string
    // (the "set image" planning verb); global single-key shortcuts are text.
    let mut city_image_edit: Option<String> = None;
    // Log tailing: the open overlay + a headless auto-open after --inspect.
    let mut log_open = false;
    // Log overlay state: --previous container toggle + substring filter editor +
    // timestamps toggle + history window.
    let mut log_previous = false;
    let mut log_timestamps = false;
    let mut log_window = kubernation_core::k8s::logs::LogWindow::default();
    let mut log_filter = String::new();
    let mut log_filter_active = false;
    // Scrollback: `log_follow` sticks to the tail; otherwise `log_scroll` is the
    // top visible line (draw_logs clamps it to the fetched/filtered length).
    let mut log_scroll: usize = 0;
    let mut log_follow = true;
    let mut concern_logs_armed = false;
    let mut auto_tail = args.tail;
    // The Almanac (in-app reference) — a modal window; None = closed.
    let mut almanac: Option<Almanac> = None;
    // The advisor screens (Health / Storage / Network) — a modal window.
    let mut advisor: Option<Advisor> = None;
    // The Charter — self-scoped RBAC ("what can I do here?") — a modal window.
    let mut charter: Option<CharterView> = None;
    // The object inspector (read-only YAML dossier) — a modal window.
    let mut inspector: Option<Inspector> = None;
    // The resource browser (`:` — any kind) — a modal window.
    let mut browser: Option<Browser> = None;
    // Dev: one-shot arm for the --browse verification flag.
    let mut browse_armed = false;
    // A transient toast (copy / export feedback): (message, expiry time).
    let mut toast: Option<(String, f64)> = None;
    // The planning turn: staged interventions (preview-only) + the open
    // End-of-Turn review modal.
    let mut planned = kubernation_core::state::planned::PlannedWorld::default();
    let mut plan_open = false;
    // The one mutation: a pod awaiting evict confirmation (cluster, ns, pod).
    let mut pending_evict: Option<(ClusterId, String, String)> = None;
    // End-of-Turn commit awaiting confirmation.
    let mut pending_commit = false;
    // Game Day: the chaos drill console (modal) + a confirmed drill awaiting
    // its confirm modal. (`chaos_just_opened` is frame-local, declared below.)
    let mut chaos: Option<Chaos> = None;
    let mut pending_chaos: Option<PendingChaos> = None;
    // Intro splash: hold the full Kubernation scene a few moments on launch.
    let mut splash_start: Option<f64> = None;
    let mut splash_skipped = false;
    let mut splash_frames: u32 = 0;
    const SPLASH_SECS: f64 = 2.4;

    // Restore-on-exit: never strand the cluster. When the operator quits with a
    // live, restorable chaos drill, undo it (uncordon / scale back / unpartition)
    // before the process exits. `prevent_quit` makes the window-close button set
    // `is_quit_requested` instead of killing us, so we get a frame to react.
    if shot.is_none() {
        prevent_quit();
    }
    let mut want_quit = false;
    let mut quitting: Option<f64> = None;

    loop {
        let snap = net.snapshot();
        let status = net.status();
        let mouse = Vec2::from(mouse_position());
        let had_snap = prev_had_snap;
        prev_had_snap = snap.is_some();

        // ---- intro splash -------------------------------------------------
        // Give the full Kubernation scene a few moments on launch (it would
        // otherwise vanish the instant the world syncs). Fades in, drifts a
        // slow zoom, fades out; any key / click skips it. Suppressed for
        // headless captures unless `--splash` asks to hold (and shoot) it.
        let now = get_time();
        if splash_start.is_none() {
            splash_start = Some(now);
        }
        let elapsed = now - splash_start.unwrap_or(now);
        let splash_active =
            !splash_skipped && (args.splash || (shot.is_none() && elapsed < SPLASH_SECS));
        if splash_active {
            // Q or the window-close button quit during the splash too (no live
            // drill can exist this early, so an immediate exit is safe).
            if is_key_pressed(KeyCode::Q) || is_quit_requested() {
                break;
            }
            if is_mouse_button_pressed(MouseButton::Left)
                || is_key_pressed(KeyCode::Escape)
                || is_key_pressed(KeyCode::Enter)
                || is_key_pressed(KeyCode::Space)
            {
                splash_skipped = true;
            }
            clear_background(Color::new(0.05, 0.06, 0.09, 1.0));
            let fade_in = (elapsed / 0.5).clamp(0.0, 1.0) as f32;
            let fade_out = if args.splash {
                1.0
            } else {
                ((SPLASH_SECS - elapsed) / 0.5).clamp(0.0, 1.0) as f32
            };
            let reveal = fade_in.min(fade_out);
            let zoom = 1.0 + (elapsed.min(6.0) as f32) * 0.022;
            let cx = screen_width() / 2.0;
            let cy = screen_height() / 2.0;
            logo::draw_full(
                vec2(cx, cy - 16.0),
                (screen_height() * 0.6).min(500.0) * zoom,
            );
            // Fade veil (black → clear → black).
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                Color::new(0.05, 0.06, 0.09, 1.0 - reveal),
            );
            if reveal > 0.4 {
                let st = ascii(&status);
                let sm = text_size(&st, 20.0);
                text(&st, cx - sm.width / 2.0, cy + 232.0, 20.0, PARCHMENT);
                let hint = "press any key";
                let hm = text_size(hint, 14.0);
                text(hint, cx - hm.width / 2.0, cy + 256.0, 14.0, DIM);
            }
            splash_frames += 1;
            if let Some(path) = &shot
                && args.splash
                && splash_frames > 30
            {
                get_screen_data().export_png(&path.to_string_lossy());
                break;
            }
            next_frame().await;
            continue;
        }

        // Context list for the picker (from the hot world's kubeconfig).
        let contexts: Vec<String> = snap
            .as_ref()
            .map(|s| s.hot.observed.meta.all_contexts.clone())
            .unwrap_or_default();
        let current_ctx = snap
            .as_ref()
            .map(|s| s.hot.observed.meta.context.clone())
            .unwrap_or_default();
        // Namespace list for the filter picker: a synthetic "all namespaces"
        // row, then every namespace the hot world holds.
        let ns_filter_now = net.namespace_filter();
        let mut ns_items: Vec<String> = vec!["all namespaces".to_string()];
        if let Some(s) = snap.as_ref() {
            ns_items.extend(s.hot.observed.namespaces());
        }
        // Every drill-down (city or node) is a centered modal window: it
        // suspends map nav like the picker.
        let panel_modal = panel.is_some();
        // Track a panel opened by *this frame's* click so the window doesn't
        // read that same click as a click-outside dismiss.
        let mut panel_just_opened = false;
        let mut plan_just_opened = false;
        // Track an evict / commit confirm opened *this frame* so the opening
        // click can't also hit the confirm's buttons.
        let mut evict_just_opened = false;
        let mut commit_just_opened = false;
        // Track a context / namespace picker opened by *this frame's* menu click
        // — the picker draws same-frame and would otherwise read that click as a
        // row select if its rows ever overlapped the dropdown (e.g. on resize).
        let mut picker_just_opened = false;
        let mut ns_picker_just_opened = false;

        // Dev verification: open the resource browser (and, with a value,
        // select that kind's table) once discovery lands.
        if args.browse.is_some() && !browse_armed {
            if browser.is_none() {
                browser = Some(Browser::new());
                net.request_discover();
            }
            match args.browse.as_deref() {
                Some(label) if !label.is_empty() => {
                    if let Some(b) = browser.as_mut()
                        && let Some(kinds) = net.kinds()
                    {
                        if let Some(k) = kinds.iter().find(|k| k.label() == label) {
                            net.request_browse(k.clone());
                            b.force_table(k.label());
                        }
                        browse_armed = true;
                    }
                }
                _ => browse_armed = true, // empty value → pick mode, done
            }
        }

        // ---- input ------------------------------------------------------
        // A minimap drag ends the moment the button is up — checked here,
        // outside the modal-suspended nav block, so opening a modal mid-drag
        // (which suspends that block) can't latch the flag into the next click.
        if !is_mouse_button_down(MouseButton::Left) {
            minimap_drag = false;
        }
        // While typing into the log filter or the city image field, single-key
        // shortcuts are text, not commands.
        let typing = (log_open && log_filter_active) || city_image_edit.is_some();
        if (is_key_pressed(KeyCode::Q) && !typing) || is_quit_requested() {
            want_quit = true;
        }
        // ?, /, or F1 toggle the Almanac (in-app reference). Track an open
        // *this frame* so the same click/press doesn't immediately dismiss it.
        // When a log overlay or a text editor is open, `/` is text instead.
        let mut almanac_just_opened = false;
        let mut advisor_just_opened = false;
        let mut charter_just_opened = false;
        let mut inspector_just_opened = false;
        let mut browser_just_opened = false;
        // Frame-local: true only on the frame the chaos window / its confirm
        // opened, so the opening click can't reach a button.
        let mut chaos_just_opened = false;
        let mut chaos_confirm_just_opened = false;
        if (is_key_pressed(KeyCode::F1) || is_key_pressed(KeyCode::Slash))
            && !log_open
            && !typing
            && advisor.is_none()
            && charter.is_none()
            && chaos.is_none()
            && browser.is_none()
        {
            if almanac.is_some() {
                almanac = None;
            } else {
                almanac = Some(Almanac::new());
                almanac_just_opened = true;
            }
        }
        // `t` opens the End-of-Turn review (planning turn) from the map.
        if is_key_pressed(KeyCode::T)
            && snap.is_some()
            && panel.is_none()
            && almanac.is_none()
            && advisor.is_none()
            && charter.is_none()
            && chaos.is_none()
            && browser.is_none()
            && !picker
            && !ns_picker
        {
            plan_open = !plan_open;
            plan_just_opened = plan_open;
        }
        // `:` opens the resource browser — discover kinds if needed. Detect the
        // produced ':' CHARACTER rather than a physical Shift+Semicolon chord, so
        // it works on keyboard layouts where ':' isn't Shift+; (AZERTY, etc.).
        let map_input_free = !typing
            && !log_open
            && panel.is_none()
            && browser.is_none()
            && inspector.is_none()
            && almanac.is_none()
            && advisor.is_none()
            && charter.is_none()
            && chaos.is_none()
            && !plan_open
            && !picker
            && !ns_picker
            && open_menu.is_none()
            && pending_evict.is_none()
            && !pending_commit
            && pending_chaos.is_none();
        if map_input_free {
            let mut open_browser = false;
            // Draining here also keeps a stray char from leaking into the log
            // filter the next time it opens.
            while let Some(c) = get_char_pressed() {
                if c == ':' {
                    open_browser = true;
                }
            }
            if open_browser {
                if net.kinds().is_none() {
                    net.request_discover();
                }
                browser = Some(Browser::new());
                browser_just_opened = true;
            }
        } else if !log_open {
            // A non-char modal owns the screen — discard typed chars so a stray
            // ':' can't pop the browser open when the modal later closes. (The
            // log overlay consumes its own chars, so leave its queue alone.)
            while get_char_pressed().is_some() {}
        }
        // `y` inspects the open city/node window's object (read-only YAML).
        // Not while a log overlay is up — the inspector would stack on top of it
        // and Esc would close the hidden overlay first.
        if is_key_pressed(KeyCode::Y)
            && !typing
            && !log_open
            && inspector.is_none()
            && almanac.is_none()
            && advisor.is_none()
            && charter.is_none()
            && chaos.is_none()
            && browser.is_none()
            && !plan_open
            && pending_evict.is_none()
            && !pending_commit
            && pending_chaos.is_none()
            && let Some(s) = snap.as_ref()
        {
            let obs = &s.hot.observed;
            let doc = match &panel {
                Some(Panel::City(_, r)) => kubernation_core::state::inspect::workload_yaml(obs, r)
                    .map(|y| {
                        (
                            inspect::title(&r.kind.to_string(), &r.namespace, &r.name),
                            y,
                        )
                    }),
                Some(Panel::Node(_, name)) => {
                    kubernation_core::state::inspect::node_yaml(obs, name)
                        .map(|y| (inspect::title("node", "", name), y))
                }
                None => None,
            };
            if let Some((title, yaml)) = doc {
                inspector = Some(Inspector::new(title, yaml));
                inspector_just_opened = true;
            }
        }
        if is_key_pressed(KeyCode::Escape) {
            if pending_chaos.is_some() {
                // The chaos confirm sits on top of the chaos window.
                pending_chaos = None;
            } else if pending_commit {
                pending_commit = false;
            } else if pending_evict.is_some() {
                pending_evict = None;
            } else if chaos.is_some() {
                // Net thread owns the session (see the Close arm).
                chaos = None;
            } else if almanac.is_some() {
                almanac = None;
            } else if advisor.is_some() {
                advisor = None;
            } else if charter.is_some() {
                charter = None;
            } else if inspector.is_some() {
                // The inspector sits on top of its panel / the browser — close
                // it first.
                inspector = None;
            } else if let Some(b) = browser.as_mut() {
                // Esc backs the table out to the kind list, then closes. Either
                // way drop the in-flight LIST so the net thread stops re-polling
                // that kind every ~2s once no table is shown.
                if !b.back() {
                    browser = None;
                }
                net.clear_browse();
            } else if plan_open {
                plan_open = false;
            } else if ns_picker {
                ns_picker = false;
            } else if picker {
                picker = false;
            } else if log_open && log_filter_active {
                // First Esc leaves the filter editor; a second closes the log.
                log_filter_active = false;
            } else if log_open {
                log_open = false;
                net.clear_logs();
            } else if city_image_edit.is_some() {
                // First Esc leaves the image editor; a second closes the window.
                city_image_edit = None;
            } else if open_menu.is_some() {
                // Esc dismisses an open dropdown before it can quit the app.
                open_menu = None;
            } else if panel.is_some() {
                panel = None;
            } else {
                break;
            }
        }
        // Log overlay owns its keys: `/` edits a filter, `p` toggles previous.
        if log_open {
            if log_filter_active {
                while let Some(c) = get_char_pressed() {
                    if !c.is_control() {
                        log_filter.push(c);
                    }
                }
                if is_key_pressed(KeyCode::Backspace) {
                    log_filter.pop();
                }
                if is_key_pressed(KeyCode::Enter) {
                    log_filter_active = false;
                }
            } else {
                // Drain any stray typed chars so the queue is empty when the
                // editor opens next frame (no leading `/`).
                while get_char_pressed().is_some() {}
                if is_key_pressed(KeyCode::Slash) {
                    log_filter_active = true;
                }
                if is_key_pressed(KeyCode::P) {
                    // Drive the re-fetch off the live request (set the instant
                    // the overlay opened), not the tail (None until a fetch
                    // lands) — and flip the flag only when we actually re-issue,
                    // so the title can never run ahead of the fetched container.
                    if let Some(mut r) = net.log_request() {
                        log_previous = !log_previous;
                        r.previous = log_previous;
                        net.request_logs(r);
                    }
                }
                // `T` toggles timestamps, `s` cycles the history window — both
                // change the LogReq, so the poll re-fetches on its own.
                if is_key_pressed(KeyCode::T)
                    && let Some(mut r) = net.log_request()
                {
                    log_timestamps = !log_timestamps;
                    r.timestamps = log_timestamps;
                    net.request_logs(r);
                }
                if is_key_pressed(KeyCode::S)
                    && let Some(mut r) = net.log_request()
                {
                    log_window = log_window.next();
                    r.window = log_window;
                    net.request_logs(r);
                }
                // `c` copies the tail to the clipboard, `w` exports it to a file.
                if is_key_pressed(KeyCode::C) {
                    let tail = net.log_tail();
                    if !tail.text.is_empty() {
                        let msg = clipboard_copy(&tail.text, tail.text.lines().count());
                        toast = Some((msg, get_time() + 3.0));
                    }
                }
                if is_key_pressed(KeyCode::W) {
                    let tail = net.log_tail();
                    if let Some(t) = &tail.target {
                        let prev = if t.previous { "-previous" } else { "" };
                        let fname = format!("{}-{}{prev}.log", t.namespace, t.pod);
                        toast = Some((export_to_file(&tail.text, &fname), get_time() + 4.0));
                    }
                }
                // Scrollback: wheel + j/k scroll, g top, f back to following.
                // (draw_logs clamps log_scroll to the fetched/filtered length.)
                let (_, wheel) = mouse_wheel();
                if wheel != 0.0 {
                    log_follow = false;
                    if wheel > 0.0 {
                        log_scroll = log_scroll.saturating_sub(3);
                    } else {
                        log_scroll += 3;
                    }
                }
                if is_key_pressed(KeyCode::J) || is_key_pressed(KeyCode::Down) {
                    log_follow = false;
                    log_scroll += 1;
                }
                if is_key_pressed(KeyCode::K) || is_key_pressed(KeyCode::Up) {
                    log_follow = false;
                    log_scroll = log_scroll.saturating_sub(1);
                }
                if is_key_pressed(KeyCode::G) {
                    log_follow = false;
                    log_scroll = 0;
                }
                if is_key_pressed(KeyCode::F) {
                    log_follow = true;
                }
            }
        } else {
            // Overlay closed — next open starts at the tail, following.
            log_scroll = 0;
            log_follow = true;
        }
        // While the context picker is open it swallows navigation.
        if picker {
            let n = contexts.len();
            if is_key_pressed(KeyCode::C) {
                picker = false;
            } else if n > 0 {
                if is_key_pressed(KeyCode::J) || is_key_pressed(KeyCode::Down) {
                    picker_idx = (picker_idx + 1) % n;
                }
                if is_key_pressed(KeyCode::K) || is_key_pressed(KeyCode::Up) {
                    picker_idx = (picker_idx + n - 1) % n;
                }
                if is_key_pressed(KeyCode::Enter) && picker_idx < n {
                    net.request_switch(contexts[picker_idx].clone());
                    picker = false;
                    selected = None;
                    panel = None;
                    concern_idx = 0;
                    // The log overlay's target belonged to the old cluster.
                    log_open = false;
                    net.clear_logs();
                }
            }
        }
        // The namespace-filter picker: row 0 = all namespaces, else focus one.
        if ns_picker {
            let n = ns_items.len();
            if n > 0 {
                if is_key_pressed(KeyCode::J) || is_key_pressed(KeyCode::Down) {
                    ns_picker_idx = (ns_picker_idx + 1) % n;
                }
                if is_key_pressed(KeyCode::K) || is_key_pressed(KeyCode::Up) {
                    ns_picker_idx = (ns_picker_idx + n - 1) % n;
                }
                if is_key_pressed(KeyCode::Enter) && ns_picker_idx < n {
                    let f = if ns_picker_idx == 0 {
                        NamespaceFilter::All
                    } else {
                        NamespaceFilter::only(ns_items[ns_picker_idx].clone())
                    };
                    net.set_namespace_filter(f);
                    ns_picker = false;
                }
            }
        }

        // The Almanac swallows the wheel (scroll its content, not zoom) and
        // takes 1-4 / ←→ to switch pages.
        if let Some(a) = almanac.as_mut() {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                a.scroll_by(wheel);
            }
            for (k, i) in [
                (KeyCode::Key1, 0),
                (KeyCode::Key2, 1),
                (KeyCode::Key3, 2),
                (KeyCode::Key4, 3),
            ] {
                if is_key_pressed(k) {
                    a.go_idx(i);
                }
            }
            if is_key_pressed(KeyCode::Left) {
                a.cycle(-1);
            }
            if is_key_pressed(KeyCode::Right) {
                a.cycle(1);
            }
        }
        // The advisor window likewise swallows the wheel and 1-4 / ←→ tabs.
        if let Some(a) = advisor.as_mut() {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                a.scroll_by(wheel);
            }
            for (k, t) in [
                (KeyCode::Key1, AdvisorTab::Health),
                (KeyCode::Key2, AdvisorTab::Storage),
                (KeyCode::Key3, AdvisorTab::Network),
                (KeyCode::Key4, AdvisorTab::RightSizing),
            ] {
                if is_key_pressed(k) {
                    a.go(t);
                }
            }
            if is_key_pressed(KeyCode::Left) {
                a.cycle(-1);
            }
            if is_key_pressed(KeyCode::Right) {
                a.cycle(1);
            }
        }
        // The Charter swallows the wheel to scroll its grid.
        if let Some(c) = charter.as_mut() {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                c.scroll_by(wheel);
            }
        }
        // The inspector swallows the wheel to scroll its YAML; `c` copies the
        // document to the clipboard, `w` exports it to a file.
        if let Some(i) = inspector.as_mut() {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                i.scroll_by(wheel);
            }
            if is_key_pressed(KeyCode::C) {
                let text = i.text();
                let msg = clipboard_copy(&text, text.lines().count());
                toast = Some((msg, get_time() + 3.0));
            }
            if is_key_pressed(KeyCode::W) {
                toast = Some((export_to_file(&i.text(), &i.filename()), get_time() + 4.0));
            }
        }
        // The resource browser swallows the wheel to scroll its list/table —
        // but only when the inspector isn't drilled in on top of it (else one
        // wheel tick would scroll both, since `mouse_wheel()` isn't a drain).
        if inspector.is_none()
            && let Some(b) = browser.as_mut()
        {
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 {
                b.scroll_by(wheel);
            }
        }

        let mut manual_pan = false;
        if !picker
            && !ns_picker
            && almanac.is_none()
            && advisor.is_none()
            && charter.is_none()
            && chaos.is_none()
            && browser.is_none()
            && !panel_modal
            && !plan_open
            && open_menu.is_none()
            && !log_open
        {
            let pan = 14.0;
            if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
                cam.pos.x -= pan;
                manual_pan = true;
            }
            if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
                cam.pos.x += pan;
                manual_pan = true;
            }
            if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
                cam.pos.y -= pan;
                manual_pan = true;
            }
            if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
                cam.pos.y += pan;
                manual_pan = true;
            }
            if is_mouse_button_down(MouseButton::Right) || is_mouse_button_down(MouseButton::Middle)
            {
                if let Some(anchor) = drag_anchor {
                    let d = anchor - mouse;
                    if d.length() > 0.0 {
                        cam.pos += d;
                        manual_pan = true;
                    }
                }
                drag_anchor = Some(mouse);
            } else {
                drag_anchor = None;
            }
            // Wheel-zoom anchors at the cursor, so only over the play area —
            // over the column or chrome it would anchor on a hidden cell and
            // jolt the map sideways.
            let over_map = mouse.x < panels::map_width() && mouse.y > panels::CHROME_H;
            let (_, wheel) = mouse_wheel();
            if wheel.abs() > 0.0 && over_map {
                let factor = if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 };
                let before = (mouse + cam.pos) / cam.zoom;
                cam.zoom = (cam.zoom * factor).clamp(0.30, 3.0);
                cam.pos = before * cam.zoom - mouse;
            }
        }
        cam.tick(manual_pan);

        if let Some(s) = snap.as_ref() {
            let worlds = scene(s);
            let bounds = scene_size(&worlds);
            // Keep the focused-concern index in range as the queue shrinks, so
            // the ATTENTION highlight and the `L` jump always point at the same row.
            if !s.attention.is_empty() {
                concern_idx = concern_idx.min(s.attention.len() - 1);
            }

            // Menu "Fit view" deferred from the chrome draw (bounds wasn't in
            // scope there).
            if std::mem::take(&mut pending_fit) {
                cam.fit(bounds);
            }

            // Frame the whole world whenever a snapshot first appears —
            // initial sync, a reconnect, or after a context switch (which
            // clears the snapshot). Skipped when --inspect will fly us in.
            if !had_snap && inspect.is_none() {
                cam.fit(bounds);
                if let Some(needle) = &args.center {
                    // Headless map framing: zoom in and center on a named
                    // city / node / island so coast & island marks render
                    // (no panel, unlike --inspect).
                    cam.zoom = args.zoom.unwrap_or(1.4).clamp(0.3, 3.0);
                    let cell = worlds.iter().find_map(|sw| {
                        sw.world
                            .cities()
                            .find(|c| c.r.name.contains(needle.as_str()))
                            .map(|c| (c.x + sw.off, c.y))
                            .or_else(|| {
                                sw.world.continents.iter().find_map(|cont| {
                                    cont.provinces
                                        .iter()
                                        .find(|p| p.tile.name.contains(needle.as_str()))
                                        .map(|p| (p.x + sw.off + 2, p.y + 1))
                                })
                            })
                            .or_else(|| {
                                sw.world
                                    .islands
                                    .iter()
                                    .find(|isl| isl.label.contains(needle.as_str()))
                                    .map(|isl| (isl.x + sw.off + isl.w / 2, isl.y + isl.h / 2))
                            })
                    });
                    if let Some((cx, cy)) = cell {
                        let cx = (cx as i32 + args.pan_dx).max(0) as u16;
                        cam.jump_to((cx, cy));
                    }
                } else if let Some(z) = args.zoom {
                    cam.zoom = z.clamp(0.3, 3.0);
                    cam.jump_to((bounds.0 / 2, bounds.1 / 2));
                }
                if args.pick && !contexts.is_empty() {
                    picker = true;
                    picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                }
                if args.almanac {
                    almanac = Some(Almanac::new());
                }
                if let Some(a) = &args.advisor {
                    advisor = Some(Advisor::new(match a.as_str() {
                        "storage" => AdvisorTab::Storage,
                        "network" => AdvisorTab::Network,
                        "rightsizing" => AdvisorTab::RightSizing,
                        _ => AdvisorTab::Health,
                    }));
                }
                if let Some(ns) = &args.charter {
                    let focus = if ns.is_empty() {
                        charter_focus_ns(&net.namespace_filter(), Some(s))
                    } else {
                        ns.clone()
                    };
                    charter = Some(CharterView::new(focus));
                }
                if let Some(m) = &args.menu {
                    open_menu = match m.as_str() {
                        "game" => Some(0),
                        "view" => Some(1),
                        "orders" => Some(2),
                        "gameday" => Some(3),
                        "advisors" => Some(4),
                        "world" => Some(5),
                        "help" => Some(6),
                        _ => None,
                    };
                }
                if args.plan {
                    let w = &s.hot.models.world;
                    let mut cities = w.cities();
                    if let Some(c) = cities.next() {
                        planned.stage_scale(c.r.clone(), c.desired + 2);
                    }
                    if let Some(c) = cities.next() {
                        planned.stage_restart(c.r.clone());
                    }
                    if let Some(p) = w.continents.iter().flat_map(|c| &c.provinces).next() {
                        planned.stage_cordon(p.tile.name.clone(), true);
                    }
                    plan_open = true;
                }
                if let Some(sub) = &args.rollback {
                    // Stage a rollback of the first matching Deployment to its
                    // previous (last-known-good) revision, then open the review.
                    if let Some(r) = s
                        .hot
                        .models
                        .world
                        .cities()
                        .map(|c| c.r.clone())
                        .find(|r| r.name.contains(sub.as_str()))
                    {
                        let revs = kubernation_core::state::rollout::revisions(&s.hot.observed, &r);
                        if let Some(prev) = kubernation_core::state::rollout::previous(&revs) {
                            planned.stage_rollback(r, prev.number);
                            plan_open = true;
                        }
                    }
                }
            }
            // Dev: exercise the concern→logs `L` jump once attention lands.
            if args.concern_logs && !concern_logs_armed && !s.attention.is_empty() {
                if let Some((i, cluster, probe)) = s
                    .attention
                    .iter()
                    .enumerate()
                    .find_map(|(i, c)| c.probe.clone().map(|p| (i, c.cluster, p)))
                {
                    concern_idx = i;
                    log_previous = probe.previous;
                    log_timestamps = args.log_timestamps;
                    log_window = kubernation_core::k8s::logs::LogWindow::default();
                    log_filter = String::new();
                    log_filter_active = false;
                    net.request_logs(LogReq {
                        cluster,
                        namespace: probe.namespace,
                        pod: probe.pod,
                        previous: log_previous,
                        timestamps: log_timestamps,
                        window: log_window,
                    });
                    log_open = true;
                    auto_tail = false;
                }
                concern_logs_armed = true;
            }
            if picker
                || ns_picker
                || almanac.is_some()
                || advisor.is_some()
                || charter.is_some()
                || chaos.is_some()
                || pending_chaos.is_some()
                || browser.is_some()
                || panel_modal
                || plan_open
                || open_menu.is_some()
                || log_open
            {
                // A modal (or an open chrome menu, or the log overlay — which
                // can be open over the bare map via the concern `L` jump) is up:
                // world navigation is suspended this frame.
            } else {
                if is_key_pressed(KeyCode::F) {
                    cam.fit(bounds);
                }
                if is_key_pressed(KeyCode::C) && !contexts.is_empty() {
                    picker = true;
                    picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                }

                if is_key_pressed(KeyCode::RightBracket) || is_key_pressed(KeyCode::LeftBracket) {
                    // All cities across the scene, in archipelago order.
                    let cities: Vec<(u16, u16)> = worlds
                        .iter()
                        .flat_map(|sw| sw.world.cities().map(move |c| (c.x + sw.off, c.y)))
                        .collect();
                    if !cities.is_empty() {
                        if is_key_pressed(KeyCode::RightBracket) {
                            city_idx = (city_idx + 1) % cities.len();
                        } else {
                            city_idx = (city_idx + cities.len() - 1) % cities.len();
                        }
                        selected = Some(cities[city_idx]);
                        cam.fly_to(cities[city_idx]);
                    }
                }
                if is_key_pressed(KeyCode::N) && !s.attention.is_empty() {
                    let next = (concern_idx + 1) % s.attention.len();
                    focus_concern(
                        next,
                        &worlds,
                        &s.attention,
                        &mut concern_idx,
                        &mut selected,
                        &mut cam,
                        &mut panel,
                    );
                }
                if is_key_pressed(KeyCode::Enter)
                    && let Some(sel) = selected
                {
                    panel = panel_for(&worlds, sel);
                }

                // Minimap navigation: click or drag to recenter the main view
                // on that spot. Holding the button lets you scrub the viewport
                // box around the chart; the cursor is clamped to the frame so a
                // drag past its edge keeps tracking.
                let ml = minimap_layout(bounds);
                if is_mouse_button_pressed(MouseButton::Left) && ml.frame.contains(mouse) {
                    minimap_drag = true;
                }
                // (minimap_drag is cleared on button-up at the top of input.)
                if minimap_drag && is_mouse_button_down(MouseButton::Left) {
                    let cm = vec2(
                        mouse.x.clamp(ml.frame.x, ml.frame.x + ml.frame.w),
                        mouse.y.clamp(ml.frame.y, ml.frame.y + ml.frame.h),
                    );
                    if let Some(cell) = ml.world_cell(cm, bounds) {
                        cam.jump_to(cell);
                    }
                }
                // A map-cell inspect (left of the column, not the minimap) opens
                // a drill-down window.
                if is_mouse_button_pressed(MouseButton::Left)
                    && !ml.frame.contains(mouse)
                    && mouse.y > panels::CHROME_H
                    && mouse.x < panels::map_width()
                {
                    selected = cam.cell_at(mouse, bounds);
                    if let Some(sel) = selected {
                        panel = panel_for(&worlds, sel);
                        panel_just_opened = panel.is_some();
                    }
                }

                // Development verification: select and open something
                // specific — a city by name, else a province (node).
                if !inspected && let Some(needle) = &inspect {
                    'outer: for sw in &worlds {
                        for c in sw.world.cities() {
                            if c.r.name.contains(needle.as_str()) {
                                let global = (c.x + sw.off, c.y);
                                selected = Some(global);
                                cam.jump_to(global);
                                panel = Some(Panel::City(sw.id, c.r.clone()));
                                break 'outer;
                            }
                        }
                        for cont in &sw.world.continents {
                            for p in &cont.provinces {
                                if p.tile.name.contains(needle.as_str()) {
                                    let global = (p.x + sw.off + 2, p.y + 1);
                                    selected = Some(global);
                                    cam.jump_to(global);
                                    panel = Some(Panel::Node(sw.id, p.tile.name.clone()));
                                    break 'outer;
                                }
                            }
                        }
                    }
                    // --yaml: also open the inspector on the inspected object.
                    if args.yaml {
                        let obs = &s.hot.observed;
                        let doc = match &panel {
                            Some(Panel::City(_, r)) => {
                                kubernation_core::state::inspect::workload_yaml(obs, r).map(|y| {
                                    (
                                        inspect::title(&r.kind.to_string(), &r.namespace, &r.name),
                                        y,
                                    )
                                })
                            }
                            Some(Panel::Node(_, name)) => {
                                kubernation_core::state::inspect::node_yaml(obs, name)
                                    .map(|y| (inspect::title("node", "", name), y))
                            }
                            None => None,
                        };
                        if let Some((title, yaml)) = doc {
                            inspector = Some(Inspector::new(title, yaml));
                        }
                    }
                    inspected = true;
                }

                // Development verification: open the first matching workload's
                // city and raise the evict confirm on its first pod (and, with
                // --evict-go, auto-confirm it a few frames later).
                if let Some(needle) = &args.evict
                    && pending_evict.is_none()
                    && panel.is_none()
                {
                    'ev: for sw in &worlds {
                        for c in sw.world.cities() {
                            if c.r.name.contains(needle.as_str())
                                && let Some(obs) = panels::observed_for(s, sw.id)
                                && let Some(city) =
                                    kubernation_core::state::model::build_city(obs, &c.r)
                                && let Some(p0) = city.pods.first()
                            {
                                let global = (c.x + sw.off, c.y);
                                selected = Some(global);
                                cam.jump_to(global);
                                panel = Some(Panel::City(sw.id, c.r.clone()));
                                pending_evict =
                                    Some((sw.id, c.r.namespace.clone(), p0.name.clone()));
                                break 'ev;
                            }
                        }
                    }
                }

                // Development verification: start a port-forward on the first
                // matching workload's first pod, staying on the map so the
                // FORWARDS column section is captured.
                if let Some(needle) = &args.forward
                    && net.forwards().is_empty()
                    && forward_armed
                {
                    forward_armed = false;
                    'fw: for sw in &worlds {
                        for c in sw.world.cities() {
                            if c.r.name.contains(needle.as_str())
                                && let Some(obs) = panels::observed_for(s, sw.id)
                                && let Some(city) =
                                    kubernation_core::state::model::build_city(obs, &c.r)
                                && let Some(p0) = city.pods.first()
                            {
                                let global = (c.x + sw.off, c.y);
                                selected = Some(global);
                                cam.jump_to(global);
                                net.request_forward(ForwardReq {
                                    cluster: sw.id,
                                    namespace: c.r.namespace.clone(),
                                    pod: p0.name.clone(),
                                });
                                break 'fw;
                            }
                        }
                    }
                }

                // Development verification: select the first node (preferred,
                // for the multi-hop cascade) or city matching --blast, so the
                // overlay (already on via `blast_on`) has a subject to fan out.
                if let Some(needle) = &args.blast
                    && blast_armed
                {
                    blast_armed = false;
                    'bl: for sw in &worlds {
                        for cont in &sw.world.continents {
                            for p in &cont.provinces {
                                if p.tile.name.contains(needle.as_str()) {
                                    let global = (p.x + sw.off, p.y);
                                    selected = Some(global);
                                    cam.jump_to(global);
                                    break 'bl;
                                }
                            }
                        }
                    }
                    if selected.is_none() {
                        'bc: for sw in &worlds {
                            for c in sw.world.cities() {
                                if c.r.name.contains(needle.as_str()) {
                                    let global = (c.x + sw.off, c.y);
                                    selected = Some(global);
                                    cam.jump_to(global);
                                    break 'bc;
                                }
                            }
                        }
                    }
                }

                // Development verification: open the Game Day window pre-targeted
                // at the first workload matching --chaos.
                if let Some(needle) = &args.chaos
                    && chaos_armed
                {
                    chaos_armed = false;
                    let target = s
                        .hot
                        .models
                        .workloads
                        .iter()
                        .find(|w| w.r.name.contains(needle.as_str()))
                        .map(|w| w.r.clone());
                    let mut c = Chaos::new(target);
                    if let Some(kind) = args
                        .chaos_exp
                        .as_deref()
                        .and_then(chaos::ChaosKind::from_flag)
                    {
                        c.set_kind(kind);
                    }
                    if let Some(tier) = args.chaos_tier.as_deref().and_then(parse_tier) {
                        c.set_tier(tier);
                    }
                    chaos = Some(c);
                    chaos_just_opened = true;
                }
            } // end world navigation (suspended while the picker is open)

            // `L` tails the focused concern's offending pod directly (the "city
            // in trouble → and here's why" jump). Unlike map navigation it works
            // even with a city/node panel open — matching the TUI's global `L`,
            // so the `N`-then-`L` flow works — but not while another overlay owns
            // input. Concerns with no log-worthy pod (replica gaps, nodes, …)
            // carry no probe.
            if is_key_pressed(KeyCode::L)
                && !s.attention.is_empty()
                && !typing
                && !log_open
                && !picker
                && !ns_picker
                && almanac.is_none()
                && advisor.is_none()
                && charter.is_none()
                && chaos.is_none()
                && browser.is_none()
                && inspector.is_none()
                && !plan_open
                && open_menu.is_none()
                && pending_evict.is_none()
                && !pending_commit
                && pending_chaos.is_none()
            {
                let c = &s.attention[concern_idx.min(s.attention.len() - 1)];
                if let Some(p) = &c.probe {
                    log_previous = p.previous;
                    log_timestamps = args.log_timestamps;
                    log_window = kubernation_core::k8s::logs::LogWindow::default();
                    log_filter = String::new();
                    log_filter_active = false;
                    net.request_logs(LogReq {
                        cluster: c.cluster,
                        namespace: p.namespace.clone(),
                        pod: p.pod.clone(),
                        previous: log_previous,
                        timestamps: log_timestamps,
                        window: log_window,
                    });
                    log_open = true;
                    auto_tail = false;
                } else {
                    toast = Some(("this concern has no pod to tail".into(), get_time() + 2.5));
                }
            }

            // `B` toggles the blast-radius overlay — the dependency fan-out of
            // the selected tile (else the focused concern's subject). A
            // lightweight map decoration, so it lives outside the nav-suspend
            // block but is still gated off while an overlay owns input.
            if is_key_pressed(KeyCode::B)
                && !typing
                && !log_open
                && !picker
                && !ns_picker
                && almanac.is_none()
                && advisor.is_none()
                && charter.is_none()
                && chaos.is_none()
                && browser.is_none()
                && inspector.is_none()
                && !plan_open
                && open_menu.is_none()
                && !panel_modal
                && pending_evict.is_none()
                && !pending_commit
                && pending_chaos.is_none()
            {
                blast_on = !blast_on;
            }
        }

        // ---- draw ---------------------------------------------------------
        clear_background(OCEAN);
        match snap.as_ref() {
            None => {
                // Splash: the full logo over the fog, status centered below.
                let cx = screen_width() / 2.0;
                let cy = screen_height() / 2.0;
                logo::draw_full(vec2(cx, cy - 30.0), (screen_height() * 0.55).min(440.0));
                let st = ascii(&status);
                let sm = text_size(&st, 24.0);
                text(&st, cx - sm.width / 2.0, cy + 210.0, 24.0, PARCHMENT);
                let fog = "the world is unexplored - fog of war";
                let fm = text_size(fog, 18.0);
                text(fog, cx - fm.width / 2.0, cy + 238.0, 18.0, DIM);
            }
            Some(s) => {
                let worlds = scene(s);
                let bounds = scene_size(&worlds);
                let paired = s.warm.is_some();
                if !want_warm || paired {
                    frames_synced += 1;
                }

                draw_sea(&cam);
                for sw in &worlds {
                    let wc = cam.shifted(sw.off);
                    let banner = paired.then_some((sw.label.as_str(), sw.id));
                    draw_world(sw.world, &wc, banner, s.pair.as_deref(), overlay);
                }
                if let Some(sel) = selected {
                    draw_selection(&cam, sel);
                }

                // Flip-watch: while a Game Day raid is announced in the queue
                // (key "chaos-raid", added by the net thread for ~30s), auto-show
                // its blast so you can watch the cities flip. Manual selection
                // overrides; it auto-disengages when the raid concern drops.
                let raid_subject: Option<(ClusterId, Subject)> = s
                    .attention
                    .iter()
                    .find(|c| c.key == "chaos-raid")
                    .and_then(|c| match &c.target {
                        Target::Workload(wr) => Some((c.cluster, Subject::Workload(wr.clone()))),
                        Target::Node(n) => Some((c.cluster, Subject::Node(n.clone()))),
                        Target::WorkloadList => None,
                    });

                // Blast-radius overlay: the dependency fan-out of the selected
                // tile (a city/node), else the live raid, else the focused
                // concern's subject. Drawn over the world; recomputed each frame
                // (cheap for real sizes).
                if blast_on || raid_subject.is_some() {
                    let subject: Option<(ClusterId, Subject)> = selected
                        .and_then(|cell| locate(&worlds, cell))
                        .and_then(|(sw, local)| match sw.world.region_at(local.0, local.1) {
                            Region::City(_, c) => Some((sw.id, Subject::Workload(c.r.clone()))),
                            Region::Province(p) => {
                                Some((sw.id, Subject::Node(p.tile.name.clone())))
                            }
                            _ => None,
                        })
                        .or_else(|| raid_subject.clone())
                        .or_else(|| {
                            (!s.attention.is_empty())
                                .then(|| &s.attention[concern_idx.min(s.attention.len() - 1)])
                                .and_then(|c| match &c.target {
                                    Target::Workload(wr) => {
                                        Some((c.cluster, Subject::Workload(wr.clone())))
                                    }
                                    Target::Node(n) => Some((c.cluster, Subject::Node(n.clone()))),
                                    Target::WorkloadList => None,
                                })
                        });
                    let mut affected = None;
                    if let Some((cid, subj)) = &subject {
                        // Memoize the (expensive-ish) topology walk: recompute
                        // only when the subject or the world snapshot changes,
                        // not every frame while the overlay is held on.
                        let snap_ptr = std::sync::Arc::as_ptr(s) as usize;
                        let stale = blast_cache
                            .as_ref()
                            .map(|(c, sb, _)| c != cid || sb != subj)
                            .unwrap_or(true)
                            || blast_cache_snap != snap_ptr;
                        if stale {
                            let obs = match cid {
                                ClusterId::Hot => Some(&s.hot.observed),
                                ClusterId::Warm => s.warm.as_ref().map(|w| &w.observed),
                            };
                            blast_cache =
                                obs.map(|obs| (*cid, subj.clone(), blast_radius(obs, subj)));
                            blast_cache_snap = snap_ptr;
                        }
                        if let Some((_, _, blast)) = &blast_cache
                            && let Some(sw) = worlds.iter().find(|w| w.id == *cid)
                        {
                            affected = draw_blast(&cam.shifted(sw.off), sw, blast);
                        }
                    } else {
                        blast_cache = None;
                    }
                    panels::draw_blast_banner(affected, panels::map_width());
                }

                // The docked right column (WORLD / STATUS / SELECTION) — always
                // shown; the drill-down modals dim it behind their scrim. The
                // SELECTION box follows the clicked tile, else the hovered one.
                let ml = minimap_layout(bounds);
                let over_map = mouse.x < panels::map_width() && mouse.y > panels::CHROME_H;
                let hovered = over_map
                    .then(|| cam.cell_at(mouse, bounds))
                    .flatten()
                    .and_then(|cell| locate(&worlds, cell));
                let sidebar_sel = selected.and_then(|cell| locate(&worlds, cell)).or(hovered);
                // The FORWARDS section's stop buttons act only when no modal is
                // up (the column is dimmed behind a scrim otherwise).
                let forwards = net.forwards();
                let sidebar_interactive = !picker
                    && almanac.is_none()
                    && advisor.is_none()
                    && charter.is_none()
                    && chaos.is_none()
                    && browser.is_none()
                    && !panel_modal
                    && !plan_open
                    && !log_open
                    && open_menu.is_none()
                    && inspector.is_none()
                    && pending_evict.is_none()
                    && !pending_commit
                    && pending_chaos.is_none();
                let sidebar_click =
                    is_mouse_button_pressed(MouseButton::Left) && sidebar_interactive;
                let hit = sidebar::draw_sidebar(
                    &worlds,
                    &cam,
                    s,
                    sidebar_sel,
                    &ns_filter_now,
                    &ml,
                    overlay,
                    concern_idx,
                    &forwards,
                    mouse,
                    sidebar_click,
                    sidebar_interactive,
                );
                if let Some(lp) = hit.stop_forward {
                    net.stop_forward(lp);
                }
                if let Some(i) = hit.focus_concern {
                    focus_concern(
                        i,
                        &worlds,
                        &s.attention,
                        &mut concern_idx,
                        &mut selected,
                        &mut cam,
                        &mut panel,
                    );
                }

                // Cartographic title cartouche over the top of the map (a
                // centered modal's scrim dims it, like the rest of the board).
                // In pair mode it spans both continents, so it names the pair
                // generically rather than the hot context (the per-side HOT/WARM
                // banners label each continent).
                let view_sub =
                    (overlay != Overlay::Terrain).then(|| format!("{} view", overlay.label()));
                let map_title = if paired {
                    "Cluster Map — Hot / Warm pair".to_string()
                } else {
                    format!("Cluster Map — {current_ctx}")
                };
                panels::draw_map_title(&map_title, view_sub.as_deref(), panels::map_width());

                // Hover tooltip over the map (not the column / chrome /
                // an open overlay — incl. a panel-less concern-`L` log).
                if !picker
                    && almanac.is_none()
                    && advisor.is_none()
                    && charter.is_none()
                    && chaos.is_none()
                    && browser.is_none()
                    && !panel_modal
                    && !plan_open
                    && !log_open
                    && open_menu.is_none()
                    && drag_anchor.is_none()
                    && !minimap_drag
                    && let Some((sw, local)) = hovered
                {
                    draw_tooltip(sw, local, s, mouse);
                }

                // The End-of-Turn review takes over the center when open;
                // otherwise the drill-down windows / minimap show. Drill-downs
                // are modals (the log overlay, when open, sits on top and
                // swallows clicks via `!log_open`).
                let click = is_mouse_button_pressed(MouseButton::Left)
                    && !panel_just_opened
                    && !log_open
                    && pending_evict.is_none()
                    && inspector.is_none();
                let mut close_panel = false;
                if plan_open {
                    let outcome = net.plan_outcome();
                    // A fully-applied commit: clear the turn and close.
                    if outcome
                        .as_ref()
                        .is_some_and(|o| o.applied && o.rows.iter().all(|r| r.ok))
                    {
                        planned.clear();
                        plan_open = false;
                        net.clear_plan_outcome();
                    } else {
                        let pclick = is_mouse_button_pressed(MouseButton::Left)
                            && !plan_just_opened
                            && !pending_commit
                            && pending_chaos.is_none();
                        let act =
                            plan::draw_plan(&planned, Some(s), outcome.as_ref(), mouse, pclick);
                        if let Some(i) = act.unstage {
                            planned.unstage(i);
                            net.clear_plan_outcome();
                        }
                        if act.commit {
                            pending_commit = true;
                            commit_just_opened = true;
                        }
                        if act.discard {
                            planned.clear();
                            plan_open = false;
                            net.clear_plan_outcome();
                        }
                        if act.close {
                            plan_open = false;
                            net.clear_plan_outcome();
                        }
                    }
                } else {
                    match &panel {
                        Some(Panel::City(cid, cr)) => {
                            let act = city::draw_city(
                                *cid,
                                cr,
                                s,
                                &planned,
                                mouse,
                                click,
                                auto_tail && !log_open,
                                &net,
                                &mut city_image_edit,
                            );
                            if let Some(iv) = act.stage {
                                planned.stage(iv);
                            }
                            if let Some(wr) = act.restart_toggle {
                                if planned.restarting(&wr) {
                                    planned.unstage_restart(&wr);
                                } else {
                                    planned.stage_restart(wr);
                                }
                            }
                            if let Some((wr, target)) = act.slo_target {
                                net.set_slo_target(*cid, wr, target);
                            }
                            if let Some((ns, pod, prefer_prev)) = act.log {
                                // Smart crash-loop default; --log-previous forces it.
                                log_previous = prefer_prev || args.log_previous;
                                log_timestamps = args.log_timestamps;
                                log_window = kubernation_core::k8s::logs::LogWindow::default();
                                log_filter = args.log_filter.clone().unwrap_or_default();
                                log_filter_active = false;
                                net.request_logs(LogReq {
                                    cluster: *cid,
                                    namespace: ns,
                                    pod,
                                    previous: log_previous,
                                    timestamps: log_timestamps,
                                    window: log_window,
                                });
                                log_open = true;
                                auto_tail = false;
                            }
                            if let Some((ns, pod)) = act.evict {
                                pending_evict = Some((*cid, ns, pod));
                                evict_just_opened = true;
                            }
                            // Port-forward starts immediately — not a write, and
                            // RBAC is pre-checked, so no confirm (unlike evict).
                            if let Some((ns, pod)) = act.forward {
                                net.request_forward(ForwardReq {
                                    cluster: *cid,
                                    namespace: ns,
                                    pod,
                                });
                            }
                            if let Some(lp) = act.stop_forward {
                                net.stop_forward(lp);
                            }
                            if let Some((ns, pod)) = act.inspect
                                && let Some(y) = kubernation_core::state::inspect::pod_yaml(
                                    &s.hot.observed,
                                    &ns,
                                    &pod,
                                )
                            {
                                inspector =
                                    Some(Inspector::new(inspect::title("pod", &ns, &pod), y));
                                inspector_just_opened = true;
                            }
                            close_panel = act.close;
                        }
                        Some(Panel::Node(nid, nname)) => {
                            let act = node::draw_node(
                                *nid,
                                nname,
                                s,
                                &planned,
                                mouse,
                                click,
                                auto_tail && !log_open,
                                &net,
                            );
                            if let Some(iv) = act.stage {
                                planned.stage(iv);
                            }
                            if let Some((ns, pod, prefer_prev)) = act.log {
                                // Smart crash-loop default; --log-previous forces it.
                                log_previous = prefer_prev || args.log_previous;
                                log_timestamps = args.log_timestamps;
                                log_window = kubernation_core::k8s::logs::LogWindow::default();
                                log_filter = args.log_filter.clone().unwrap_or_default();
                                log_filter_active = false;
                                net.request_logs(LogReq {
                                    cluster: *nid,
                                    namespace: ns,
                                    pod,
                                    previous: log_previous,
                                    timestamps: log_timestamps,
                                    window: log_window,
                                });
                                log_open = true;
                                auto_tail = false;
                            }
                            if let Some((ns, pod)) = act.evict {
                                pending_evict = Some((*nid, ns, pod));
                                evict_just_opened = true;
                            }
                            if let Some((ns, pod)) = act.forward {
                                net.request_forward(ForwardReq {
                                    cluster: *nid,
                                    namespace: ns,
                                    pod,
                                });
                            }
                            if let Some(lp) = act.stop_forward {
                                net.stop_forward(lp);
                            }
                            if let Some((ns, pod)) = act.inspect
                                && let Some(y) = kubernation_core::state::inspect::pod_yaml(
                                    &s.hot.observed,
                                    &ns,
                                    &pod,
                                )
                            {
                                inspector =
                                    Some(Inspector::new(inspect::title("pod", &ns, &pod), y));
                                inspector_just_opened = true;
                            }
                            close_panel = act.close;
                        }
                        None => {
                            // No drill-down panel. A log overlay may still be up
                            // legitimately — the concern `L` jump opens one with
                            // no backing panel — so DON'T auto-close it here; Esc
                            // (which closes the log first) and `close_panel`
                            // handle a panel-backed log's teardown.
                        }
                    }
                }
                if close_panel {
                    panel = None;
                    log_open = false;
                    city_image_edit = None;
                    net.clear_logs();
                }
                if log_open {
                    draw_logs(
                        &net.log_tail(),
                        &log_filter,
                        log_filter_active,
                        log_previous,
                        log_timestamps,
                        log_window,
                        &mut log_scroll,
                        &mut log_follow,
                    );
                }
            }
        }

        // Top chrome: a carved tan-stone bar.
        draw_rectangle(0.0, 0.0, screen_width(), panels::CHROME_H - 2.0, STONE);
        draw_rectangle(0.0, 0.0, screen_width(), 1.5, STONE_LIGHT);
        draw_rectangle(
            0.0,
            panels::CHROME_H - 2.0,
            screen_width(),
            2.0,
            STONE_SHADOW,
        );
        logo::draw_mark(vec2(17.0, panels::CHROME_H / 2.0 - 1.0), 24.0);

        // The classic-4X dropdown menu bar — Game / View / Orders / World /
        // Help — replaces the scattered chrome buttons. It's interactive only
        // when no centered modal owns input; otherwise its titles draw inert
        // and any open dropdown is dismissed.
        let menu_live = !picker
            && !ns_picker
            && almanac.is_none()
            && advisor.is_none()
            && charter.is_none()
            && chaos.is_none()
            && browser.is_none()
            && !plan_open
            && panel.is_none()
            && !log_open
            && pending_evict.is_none()
            && !pending_commit
            && pending_chaos.is_none();
        if !menu_live {
            open_menu = None;
        }
        let mctx = MenuCtx {
            overlay,
            staged: planned.len(),
            ns_active: ns_filter_now.is_active(),
        };
        let menu_click = is_mouse_button_pressed(MouseButton::Left) && menu_live;
        let (menu_action, bar_right) =
            menu::draw_menu_bar(42.0, mouse, menu_click, &mut open_menu, &mctx);
        match menu_action {
            Some(MenuAction::SwitchContext) => {
                // No-op when there are no contexts (an empty picker); picker_idx
                // is harmless then.
                picker_idx = contexts.iter().position(|c| *c == current_ctx).unwrap_or(0);
                picker = !contexts.is_empty();
                picker_just_opened = picker;
            }
            Some(MenuAction::Fit) => pending_fit = true,
            Some(MenuAction::Quit) => want_quit = true,
            Some(MenuAction::SetOverlay(o)) => overlay = o,
            Some(MenuAction::EndTurn) => {
                // The review draws next frame; by then the press edge is gone,
                // so the opening click can't reach it as a click-outside
                // dismiss (no plan_just_opened guard needed on this path).
                plan_open = true;
            }
            Some(MenuAction::DiscardTurn) => planned.clear(),
            Some(MenuAction::ChaosOpen) => {
                // Pre-target the focused concern's workload (the "city in
                // trouble → raid it" flow); else open with an empty picker.
                let target = snap.as_ref().and_then(|s| {
                    s.attention.get(concern_idx).and_then(|c| match &c.target {
                        Target::Workload(wr) => Some(wr.clone()),
                        _ => None,
                    })
                });
                chaos = Some(Chaos::new(target));
                chaos_just_opened = true;
            }
            Some(MenuAction::NamespaceFilter) => {
                ns_picker = true;
                ns_picker_just_opened = true;
                ns_picker_idx = match &ns_filter_now {
                    NamespaceFilter::Only(s) => s
                        .iter()
                        .next()
                        .and_then(|ns| ns_items.iter().position(|i| i == ns))
                        .unwrap_or(0),
                    _ => 0,
                };
            }
            Some(MenuAction::Advisor(tab)) => {
                advisor = Some(Advisor::new(tab));
                advisor_just_opened = true;
            }
            Some(MenuAction::Charter) => {
                charter = Some(CharterView::new(charter_focus_ns(
                    &ns_filter_now,
                    snap.as_deref(),
                )));
                charter_just_opened = true;
            }
            Some(MenuAction::Almanac) => {
                almanac = Some(Almanac::new());
                almanac_just_opened = true;
            }
            None => {}
        }

        // The realm readout (context · platform · counts) right-aligned, like a
        // 4X title bar's status line. Truncated to the space left of the screen
        // edge and right of the menu bar so a long paired/error label can't
        // overdraw the rightmost menu titles on a narrow window.
        let mut st = ascii(&status);
        let mut sm = text_size(&st, 14.0);
        let avail = screen_width() - 12.0 - (bar_right + 12.0);
        if sm.width > avail && !st.is_empty() {
            let budget = ((st.chars().count() as f32) * (avail / sm.width)) as usize;
            st = panels::truncate_str(&st, budget.max(3));
            sm = text_size(&st, 14.0);
        }
        text(
            &st,
            (screen_width() - sm.width - 12.0).max(bar_right + 12.0),
            21.0,
            14.0,
            STONE_INK_DIM,
        );

        // Context picker, drawn on top of everything.
        if picker {
            let layout = panels::draw_picker(
                &contexts,
                &current_ctx,
                picker_idx,
                "SWITCH CONTEXT",
                "enter switch . j/k move . c or esc cancel",
            );
            if is_mouse_button_pressed(MouseButton::Left) && !picker_just_opened {
                for (i, r) in layout.rows.iter().enumerate() {
                    if r.contains(mouse) && i < contexts.len() {
                        net.request_switch(contexts[i].clone());
                        picker = false;
                        selected = None;
                        panel = None;
                        concern_idx = 0;
                        // The log overlay's target belonged to the old cluster.
                        log_open = false;
                        net.clear_logs();
                    }
                }
            }
        }
        // Namespace-filter picker. The "current" marker is the focused
        // namespace (or "all namespaces" when unfiltered).
        if ns_picker {
            let current = match &ns_filter_now {
                NamespaceFilter::Only(s) => s
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| ns_items[0].clone()),
                NamespaceFilter::All => ns_items[0].clone(),
            };
            let layout = panels::draw_picker(
                &ns_items,
                &current,
                ns_picker_idx,
                "NAMESPACE FILTER",
                "enter apply . j/k move . esc cancel",
            );
            if is_mouse_button_pressed(MouseButton::Left) && !ns_picker_just_opened {
                for (i, r) in layout.rows.iter().enumerate() {
                    if r.contains(mouse) && i < ns_items.len() {
                        let f = if i == 0 {
                            NamespaceFilter::All
                        } else {
                            NamespaceFilter::only(ns_items[i].clone())
                        };
                        net.set_namespace_filter(f);
                        ns_picker = false;
                    }
                }
            }
        }

        // The Almanac, drawn on top of everything; it handles its own clicks
        // (but not the click that opened it this frame).
        if almanac.is_some() {
            let click = is_mouse_button_pressed(MouseButton::Left) && !almanac_just_opened;
            let action = almanac
                .as_mut()
                .map(|a| a.draw(snap.as_deref(), mouse, click));
            match action {
                Some(AlmanacAction::Close) => almanac = None,
                Some(AlmanacAction::Page(p)) => {
                    if let Some(a) = almanac.as_mut() {
                        a.go(p);
                    }
                }
                // Cross-reference: fly to a live example, then close.
                Some(AlmanacAction::Locate(cell)) => {
                    cam.fly_to(cell);
                    selected = Some(cell);
                    almanac = None;
                }
                _ => {}
            }
        }

        // The advisor screens, drawn on top (own clicks, not the opening one).
        if advisor.is_some() {
            let click = is_mouse_button_pressed(MouseButton::Left) && !advisor_just_opened;
            let action = advisor
                .as_mut()
                .map(|a| a.draw(snap.as_deref(), mouse, click));
            if let Some(AdvisorAction::Close) = action {
                advisor = None;
            }
        }

        // The Charter (self-scoped RBAC), drawn on top (own clicks, not the
        // opening one). It requests/reads its grid from the net thread itself.
        if charter.is_some() {
            let click = is_mouse_button_pressed(MouseButton::Left) && !charter_just_opened;
            let action = charter
                .as_mut()
                .map(|c| c.draw(snap.as_deref(), &net, mouse, click));
            if let Some(CharterAction::Close) = action {
                charter = None;
            }
        }

        // The Game Day chaos console, drawn on top. A "Run drill" / "Restore"
        // raises the chaos confirm (pending_chaos); the net thread runs it.
        if chaos.is_some() {
            // Gate the window's clicks while its confirm is up (the confirm only
            // paints a scrim — without this, the click reaches the window too).
            let click = is_mouse_button_pressed(MouseButton::Left)
                && !chaos_just_opened
                && pending_chaos.is_none();
            let session = net.chaos_session();
            let history = net.chaos_history();
            let action = snap
                .as_ref()
                .zip(chaos.as_mut())
                .map(|(s, c)| c.draw(s, session.as_ref(), &history, mouse, click));
            match action {
                Some(ChaosAction::Close) => {
                    // Leave the session for the net thread to own (cleared on
                    // context switch / overwritten by the next run) — clearing it
                    // here races a still-in-flight drill that re-creates it. The
                    // window only shows a session matching the open target.
                    chaos = None;
                }
                Some(ChaosAction::Run { exp, auto_restore }) => {
                    if let Some(s) = snap.as_ref() {
                        let plan =
                            kubernation_core::state::chaos::plan_chaos(&s.hot.observed, &exp);
                        if !plan.is_refused() {
                            use kubernation_core::state::chaos::Experiment;
                            let evicts = plan
                                .steps
                                .iter()
                                .filter(|st| {
                                    matches!(
                                        st,
                                        kubernation_core::state::chaos::ChaosStep::Evict { .. }
                                    )
                                })
                                .count();
                            let line1 = match &exp {
                                Experiment::Outage { workload } => format!(
                                    "Scale {}/{} to 0 — a real outage.",
                                    workload.namespace, workload.name
                                ),
                                Experiment::KillOne { workload } => format!(
                                    "Delete one pod of {}/{}.",
                                    workload.namespace, workload.name
                                ),
                                Experiment::KillAll { workload } => format!(
                                    "Delete all {evicts} pod(s) of {}/{}.",
                                    workload.namespace, workload.name
                                ),
                                Experiment::KillPercent { workload, pct } => format!(
                                    "Delete {evicts} pod(s) (~{pct}%) of {}/{}.",
                                    workload.namespace, workload.name
                                ),
                                Experiment::ScaleSpike { workload, factor } => format!(
                                    "Surge {}/{} by {factor}x.",
                                    workload.namespace, workload.name
                                ),
                                Experiment::BrokenImage { workload } => format!(
                                    "Roll {}/{} onto an unresolvable image.",
                                    workload.namespace, workload.name
                                ),
                                Experiment::Partition { workload, dir } => format!(
                                    "Isolate {}/{} — {} NetworkPolicy.",
                                    workload.namespace,
                                    workload.name,
                                    dir.label()
                                ),
                                Experiment::NodeFailure { node } => {
                                    format!("Cordon {node} and drain its {evicts} pod(s).")
                                }
                                Experiment::CordonFreeze { node } => {
                                    format!("Cordon {node} (freeze scheduling, no drain).")
                                }
                            };
                            let auto = auto_restore.then_some(60.0);
                            pending_chaos = Some(PendingChaos {
                                run: build_chaos_run(&s.hot.observed, &exp, &plan, auto),
                                title: "Run chaos drill?".into(),
                                line1,
                                line2: format!("Blast radius: {} affected.", plan.blast),
                                action: "Run drill".into(),
                            });
                            chaos_confirm_just_opened = true;
                        }
                    }
                }
                Some(ChaosAction::RunTier {
                    tier,
                    target,
                    auto_restore,
                }) => {
                    if let Some(s) = snap.as_ref() {
                        let plan = kubernation_core::state::chaos::plan_tier(
                            &s.hot.observed,
                            tier,
                            &target,
                        );
                        if !plan.is_refused() {
                            let auto = auto_restore.then_some(60.0);
                            pending_chaos = Some(PendingChaos {
                                run: build_tier_run(&target, tier, &plan, auto),
                                title: "Run compound drill?".into(),
                                line1: format!(
                                    "{} on {}/{} — {}.",
                                    tier.label(),
                                    target.namespace,
                                    target.name,
                                    tier.detail()
                                ),
                                line2: format!(
                                    "{} step(s) · blast radius: {} affected.",
                                    plan.steps.len(),
                                    plan.blast
                                ),
                                action: "Run drill".into(),
                            });
                            chaos_confirm_just_opened = true;
                        }
                    }
                }
                Some(ChaosAction::Restore) => {
                    if let Some(sess) = net.chaos_session()
                        && !sess.restore.is_empty()
                    {
                        pending_chaos = Some(PendingChaos {
                            run: net::ChaosRun {
                                cluster: ClusterId::Hot,
                                experiment: format!("restore {}", sess.experiment),
                                subject: sess.subject.clone(),
                                // A restore's honest frame is "did the cluster
                                // come back" — the recovery line — not the
                                // original injection's static notes (which would
                                // wrongly claim "still cordoned" / "policy
                                // applied" after the undo).
                                score_kind: kubernation_core::state::chaos::ScoreKind::Workload,
                                blast: 0,
                                steps: sess.restore.clone(),
                                restore: Vec::new(),
                                watch: sess.watch.clone(),
                                auto_restore_secs: None,
                                is_restore: true,
                            },
                            title: "Restore?".into(),
                            line1: format!("Undo the drill on {}.", sess.target_label),
                            line2: "Restore the cluster.".into(),
                            action: "Restore".into(),
                        });
                        chaos_confirm_just_opened = true;
                    }
                }
                Some(ChaosAction::None) | None => {}
            }
        }

        // The object inspector (YAML dossier), drawn on top of its panel.
        if inspector.is_some() {
            let click = is_mouse_button_pressed(MouseButton::Left) && !inspector_just_opened;
            if inspector
                .as_mut()
                .map(|i| i.draw(mouse, click))
                .unwrap_or(false)
            {
                inspector = None;
            }
        }

        // The resource browser (`:` — any kind). A row click drills into the
        // inspector (which then draws on top next frame).
        if browser.is_some() && inspector.is_none() {
            let click = is_mouse_button_pressed(MouseButton::Left) && !browser_just_opened;
            match browser.as_mut().map(|b| b.draw(&net, mouse, click)) {
                Some(BrowseAction::Close) => {
                    browser = None;
                    net.clear_browse();
                }
                Some(BrowseAction::Back) => {
                    // Returned to the kind picker — stop the net thread re-LISTing.
                    net.clear_browse();
                }
                Some(BrowseAction::Inspect(obj)) => {
                    let kind = obj
                        .types
                        .as_ref()
                        .map(|t| t.kind.clone())
                        .unwrap_or_default();
                    let ns = obj.metadata.namespace.clone().unwrap_or_default();
                    let name = obj.metadata.name.clone().unwrap_or_default();
                    let title = inspect::title(&kind, &ns, &name);
                    inspector = Some(Inspector::new(
                        title,
                        kubernation_core::state::inspect::dynamic_yaml(&obj),
                    ));
                    // (No just-opened guard needed: the inspector draws next
                    // frame, by which point this click's press edge is gone.)
                }
                _ => {}
            }
        }

        // Development verification: auto-confirm the staged evict (REAL delete)
        // a few frames after it's raised, so the write path can be exercised
        // headlessly.
        if args.evict_go
            && frames_synced == 20
            && let Some((cid, ns, pod)) = pending_evict.take()
        {
            net.request_evict(EvictReq {
                cluster: cid,
                namespace: ns,
                pod,
            });
        }
        // Development verification: auto-commit the staged turn (REAL apply).
        if args.plan_go && frames_synced == 20 && plan_open && !planned.is_empty() {
            net.clear_plan_outcome();
            net.request_commit(planned.interventions().to_vec());
        }

        // Dev: auto-run the chaos drill (REALLY injects — by default a KillOne,
        // which the controller recovers) a few frames after the window opens.
        // `--chaos-exp` picks which of the six experiments to run.
        if args.chaos_go
            && frames_synced == 30
            && let Some(c) = &chaos
            && let Some(wr) = &c.target
            && let Some(s) = snap.as_ref()
        {
            // A tier overrides the single-experiment path.
            if let Some(tier) = args.chaos_tier.as_deref().and_then(parse_tier) {
                let plan = kubernation_core::state::chaos::plan_tier(&s.hot.observed, tier, wr);
                if !plan.is_refused() {
                    net.request_chaos(build_tier_run(wr, tier, &plan, Some(3.0)));
                }
            } else {
                use kubernation_core::state::chaos::Experiment;
                let w = wr.clone();
                let exp = match args.chaos_exp.as_deref() {
                    Some("kill-all") => Experiment::KillAll { workload: w },
                    Some("kill-percent") => Experiment::KillPercent {
                        workload: w,
                        pct: 50,
                    },
                    Some("scale-spike") => Experiment::ScaleSpike {
                        workload: w,
                        factor: 3,
                    },
                    Some("outage") => Experiment::Outage { workload: w },
                    Some("broken-image") => Experiment::BrokenImage { workload: w },
                    Some("partition") => Experiment::Partition {
                        workload: w,
                        dir: kubernation_core::state::chaos::PartitionDir::Both,
                    },
                    Some(node_exp @ ("node-failure" | "cordon-freeze")) => {
                        // The first non-control-plane node hosting the target's pods.
                        let node = s
                            .hot
                            .observed
                            .pods
                            .state()
                            .iter()
                            .filter(|p| {
                                p.metadata.namespace.as_deref() == Some(wr.namespace.as_str())
                                    && p.metadata
                                        .name
                                        .as_deref()
                                        .is_some_and(|n| n.starts_with(&wr.name))
                            })
                            .find_map(|p| p.spec.as_ref().and_then(|sp| sp.node_name.clone()))
                            .unwrap_or_default();
                        if node_exp == "cordon-freeze" {
                            Experiment::CordonFreeze { node }
                        } else {
                            Experiment::NodeFailure { node }
                        }
                    }
                    _ => Experiment::KillOne { workload: w },
                };
                let plan = kubernation_core::state::chaos::plan_chaos(&s.hot.observed, &exp);
                if !plan.is_refused() {
                    // Dev: arm a short auto-restore so the auto-undo is observable
                    // in the screenshot hold (a no-op for non-restorable kills).
                    net.request_chaos(build_chaos_run(&s.hot.observed, &exp, &plan, Some(3.0)));
                }
            }
        }

        // Evict confirm — the one destructive action, on top of everything.
        // Esc cancels (handled above); the opening click can't reach a button.
        if let Some((cid, ns, pod)) = pending_evict.clone() {
            let paired = snap.as_ref().is_some_and(|s| s.warm.is_some());
            let tag = if paired && cid == ClusterId::Warm {
                "WARM "
            } else {
                ""
            };
            let cclick = is_mouse_button_pressed(MouseButton::Left) && !evict_just_opened;
            let act = draw_evict_confirm(tag, &ns, &pod, mouse, cclick);
            if act.yes {
                net.request_evict(EvictReq {
                    cluster: cid,
                    namespace: ns,
                    pod,
                });
                pending_evict = None;
            } else if act.cancel {
                pending_evict = None;
            }
        }

        // Commit confirm — applies the planning turn to the cluster.
        if pending_commit {
            let cclick = is_mouse_button_pressed(MouseButton::Left) && !commit_just_opened;
            let act = draw_commit_confirm(planned.len(), mouse, cclick);
            if act.yes {
                net.clear_plan_outcome();
                net.request_commit(planned.interventions().to_vec());
                pending_commit = false;
            } else if act.cancel {
                pending_commit = false;
            }
        }

        // Chaos drill confirm — a real failure injection (CRIT). On confirm the
        // net thread runs it; the chaos window stays open to show the scorecard.
        if let Some(pc) = &pending_chaos {
            let cclick = is_mouse_button_pressed(MouseButton::Left) && !chaos_confirm_just_opened;
            let act =
                draw_chaos_confirm(&pc.title, &pc.line1, &pc.line2, &pc.action, mouse, cclick);
            if act.yes {
                net.request_chaos(pc.run.clone());
                pending_chaos = None;
            } else if act.cancel {
                pending_chaos = None;
            }
        }

        // Eviction result toast (auto-cleared by the net thread after a few s).
        if let Some(msg) = net.evict_status() {
            let fs = 15.0;
            let tm = text_size(&msg, fs);
            let bw = tm.width + 24.0;
            let bx = (screen_width() - bw) / 2.0;
            let by = panels::CHROME_H + 8.0;
            draw_rectangle(bx, by, bw, 26.0, STONE);
            draw_rectangle_lines(bx, by, bw, 26.0, 1.0, STONE_EDGE);
            text(ascii(&msg), bx + 12.0, by + 18.0, fs, STONE_INK);
        }

        // Copy / export toast (a row below the evict toast; auto-expires).
        if let Some((msg, exp)) = &toast
            && get_time() < *exp
        {
            let fs = 14.0;
            let tm = text_size(msg, fs);
            let bw = tm.width + 24.0;
            let bx = (screen_width() - bw) / 2.0;
            let by = panels::CHROME_H + 42.0;
            draw_rectangle(bx, by, bw, 24.0, STONE);
            draw_rectangle_lines(bx, by, bw, 24.0, 1.0, STONE_EDGE);
            text(ascii(msg), bx + 12.0, by + 17.0, fs, STONE_INK);
        }
        if toast.as_ref().is_some_and(|(_, exp)| get_time() >= *exp) {
            toast = None;
        }

        // When tailing, wait long enough for the net thread's first fetch
        // (first_container + tail, two API round-trips) to land.
        let shot_at = if args.spark {
            // Long hold so metrics polls (15s) draw a trend AND SLO samples
            // (2s) reach a verdict; headless frame rate varies, so over-wait.
            2600
        } else if args.tail || args.concern_logs {
            240
        } else if args.forward.is_some() {
            // Resolve the default port (a get + maybe a Service LIST) + bind +
            // appear in net.forwards() — a few net ticks (250ms each).
            180
        } else if args.chaos_go {
            600 // run at frame 30 + watch recovery + the scorecard settle
        } else if args.plan_go || args.blast.is_some() || args.chaos.is_some() {
            120
        } else if args.browse.is_some() {
            // Discovery (per-group enumeration) and, with a kind, a LIST need a
            // few net-thread ticks (250ms each) to land before the modal has
            // content.
            180
        } else if args.charter.is_some() {
            // The SSAR probe burst needs a couple net-thread ticks to land.
            150
        } else {
            45
        };
        if let Some(path) = &shot
            && frames_synced > shot_at
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        // Restore-on-exit dance: on a quit request, if a live hot drill still has
        // un-undone restore steps, run the restore first, then exit (with an 8s
        // backstop so quitting can never hang). Otherwise exit immediately.
        if want_quit && quitting.is_none() {
            want_quit = false;
            let pending = net
                .chaos_session()
                .filter(|s| s.cluster == ClusterId::Hot && !s.restore.is_empty());
            match pending {
                Some(sess) => {
                    net.request_chaos(net::ChaosRun {
                        cluster: ClusterId::Hot,
                        experiment: format!("restore {}", sess.experiment),
                        subject: sess.subject.clone(),
                        score_kind: kubernation_core::state::chaos::ScoreKind::Workload,
                        blast: 0,
                        steps: sess.restore.clone(),
                        restore: Vec::new(),
                        watch: sess.watch.clone(),
                        auto_restore_secs: None,
                        is_restore: true,
                    });
                    quitting = Some(get_time());
                }
                None => break,
            }
        }
        if let Some(started) = quitting {
            // A small "restoring…" banner while we wait for the undo to land.
            let msg = "Restoring the drill before exit…";
            let m = text_size(msg, 18.0);
            let bw = m.width + 32.0;
            let bx = (screen_width() - bw) / 2.0;
            let by = screen_height() / 2.0 - 20.0;
            stone_panel(bx, by, bw, 40.0);
            text(msg, bx + 16.0, by + 26.0, 18.0, STONE_INK);
            let done = net
                .chaos_session()
                .map(|s| s.restore.is_empty())
                .unwrap_or(true);
            // Backstop must exceed the net thread's 25s run_chaos timeout so we
            // don't exit mid-restore (the common case still exits instantly once
            // `done` flips — the restore lands in well under a second).
            if done || get_time() - started > 27.0 {
                break;
            }
        }

        next_frame().await;
    }
}

/// Copy `text` to the OS clipboard by piping to the platform tool (macOS
/// `pbcopy`, Linux `wl-copy`/`xclip`/`xsel`, Windows `clip`). Returns true on
/// success. This is more reliable than the windowing layer's clipboard, which
/// is a no-op or flaky on some platforms.
fn os_clipboard_copy(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (cmd, args) in candidates {
        let Ok(mut child) = Command::new(cmd)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
            // Drop stdin to send EOF before waiting.
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// Copy to the clipboard, falling back to the windowing layer if no CLI tool
/// is present. Returns the toast message.
fn clipboard_copy(text: &str, lines: usize) -> String {
    if os_clipboard_copy(text) {
        format!("copied {lines} lines")
    } else {
        macroquad::miniquad::window::clipboard_set(text);
        format!("copied {lines} lines (fallback)")
    }
}

/// Write `text` to `filename` in the working directory; returns a toast
/// message (the path on success, the error otherwise).
fn export_to_file(text: &str, filename: &str) -> String {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join(filename);
    match std::fs::write(&path, text) {
        Ok(()) => format!("exported → {}", path.display()),
        Err(e) => format!("export failed: {e}"),
    }
}

fn panel_for(worlds: &[SceneWorld], sel: (u16, u16)) -> Option<Panel> {
    let (sw, local) = locate(worlds, sel)?;
    // A coast marker opens the city it serves.
    if let Some((_, m)) = sw.world.coast_at(local.0, local.1) {
        return Some(Panel::City(sw.id, m.workload.clone()));
    }
    match sw.world.region_at(local.0, local.1) {
        Region::City(_, c) => Some(Panel::City(sw.id, c.r.clone())),
        Region::Province(p) => Some(Panel::Node(sw.id, p.tile.name.clone())),
        Region::Structure(_, s) => s.workload.clone().map(|r| Panel::City(sw.id, r)),
        _ => None,
    }
}
