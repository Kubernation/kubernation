//! The network side: tokio + the core watchers on a background thread,
//! publishing snapshots the render loop reads without ever blocking on
//! the cluster. `ObservedWorld` rides along (its stores are cheap Arc
//! clones) so detail panels can run the pure city/node builders on demand.
//! With `--warm`, a second world is watched and compared — the GUI shows
//! it as a second archipelago east of the hot one.
//!
//! The hot cluster is switchable at runtime: the render loop drops a
//! requested context into `switch`; the net thread drops the old hot
//! WorldHandle (its informers abort) and spawns a fresh one.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kubernation_core::events::{ClusterId, WorldDelta};
use kubernation_core::k8s::{actions, browse, client, logs, portforward, watch};
use kubernation_core::state::attention::{Concern, Severity, Target};
use kubernation_core::state::blast::Subject;
use kubernation_core::state::chaos::{self, ScoreKind};
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::{Models, WorkloadRef, WorkloadRow, build_workloads};
use kubernation_core::state::observed::ObservedWorld;
use kubernation_core::state::pair::PairSync;
use kubernation_core::state::planned::Intervention;
use kubernation_core::state::slo::{self, SloConfig, SloStatus, SloTracker};

/// Which pod's logs the UI wants tailed, and how. The poll re-fetches whenever
/// any of these change (`PartialEq`), so toggling previous/timestamps/window
/// triggers a refresh for free.
#[derive(Clone, PartialEq, Eq)]
pub struct LogReq {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
    /// Tail the previously-terminated container (`kubectl logs --previous`).
    pub previous: bool,
    /// Prefix each line with the server timestamp.
    pub timestamps: bool,
    /// How much history to pull.
    pub window: logs::LogWindow,
}

/// The latest tail for the requested pod.
#[derive(Default, Clone)]
pub struct LogTail {
    pub target: Option<LogReq>,
    pub text: String,
    pub error: Option<String>,
}

/// A confirmed request to evict (delete) a pod. The project's only write —
/// queued by the GUI after an explicit confirm, executed once by the net
/// thread (see `k8s::actions::evict_pod`).
#[derive(Clone, PartialEq, Eq)]
pub struct EvictReq {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
}

/// The End-of-Turn commit result lives in the core write file (`k8s::actions`,
/// `commit_interventions`); the client keeps this familiar alias.
pub use kubernation_core::k8s::actions::CommitOutcome as PlanOutcome;

/// A request to start a port-forward for a pod. The net thread resolves the
/// pod's default port (`portforward::default_port`) and binds a local listener.
#[derive(Clone, PartialEq, Eq)]
pub struct ForwardReq {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
}

/// A live port-forward, as the UI sees it. The net thread keeps the matching
/// `portforward::Forward` handle privately (dropping it tears the tunnel down);
/// this is the cloneable view rendered in the forwards strip + pod rows.
#[derive(Clone)]
pub struct ForwardInfo {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
    pub pod_port: u16,
    /// The local `127.0.0.1` port now listening — also the stop key.
    pub local_port: u16,
}

/// A confirmed chaos drill the UI has queued (the net thread runs it once,
/// reusing the gated write primitives). Carries enough to seed the scorecard.
#[derive(Clone)]
pub struct ChaosRun {
    pub cluster: ClusterId,
    pub experiment: String,
    /// What the drill targets (a workload or a node).
    pub subject: Subject,
    /// Which scorecard class to render for this experiment.
    pub score_kind: ScoreKind,
    pub blast: usize,
    pub steps: Vec<chaos::ChaosStep>,
    /// Steps that undo the drill (Outage → scale back, node → uncordon,
    /// partition → delete the policy); the scorecard's Restore re-submits them as
    /// another run. Empty for kills (the controller recreates pods).
    pub restore: Vec<chaos::ChaosStep>,
    /// Workloads whose readiness to watch for the recovery signal — the target
    /// itself, or a node's hosted workloads.
    pub watch: Vec<WorkloadRef>,
    /// If set, the net thread auto-runs the restore this many seconds after the
    /// drill (opt-in "auto-undo"). Ignored when there's nothing to restore.
    pub auto_restore_secs: Option<f64>,
    /// This run is itself an undo (manual/quit/switch restore), not a new drill:
    /// the session is marked `restored` and the previous one isn't chronicled.
    pub is_restore: bool,
}

/// The live game-day session — the net thread tracks the cluster's response
/// (recovery + budget spend) so the chaos window can show a scorecard.
#[derive(Clone)]
pub struct ChaosSession {
    pub cluster: ClusterId,
    pub experiment: String,
    pub subject: Subject,
    pub score_kind: ScoreKind,
    /// Display label for the target (a `ns/name` workload or `node <n>`).
    pub target_label: String,
    pub blast: usize,
    pub budget_before: Option<SloStatus>,
    pub budget_after: Option<SloStatus>,
    /// The target was observed degraded after the drill (a workload outage:
    /// `ready == 0`; a node drill: watched workloads below desired).
    pub dipped: bool,
    pub recovered: bool,
    pub recover_secs: Option<f64>,
    pub restore: Vec<chaos::ChaosStep>,
    /// Workloads whose readiness drives the recovery signal.
    pub watch: Vec<WorkloadRef>,
    /// The run's per-step result (errors surface here).
    pub outcome: Option<PlanOutcome>,
    /// Net-tick at run start (recovery time is measured from here).
    started_tick: u64,
    /// If set, the net thread auto-runs the restore at this tick (opt-in undo).
    auto_restore_tick: Option<u64>,
    /// Was the watch set at full strength before the drill (steady-state gate)?
    pub healthy_before: bool,
    /// The watch set's ready-fraction over time (a recovery curve to sparkline).
    pub recovery_series: Vec<f32>,
    /// Net-tick the attention queue first flagged the target (→ MTTD).
    detect_tick: Option<u64>,
    /// The operator undid this drill (manual / auto / exit / switch restore) —
    /// the scorecard says "restored", not "self-healed".
    pub restored: bool,
}

impl ChaosSession {
    /// Seconds from drill start until the queue first flagged it (MTTD), if ever.
    pub fn detect_secs(&self) -> Option<f64> {
        self.detect_tick
            .map(|t| t.saturating_sub(self.started_tick) as f64 * 0.25)
    }
}

/// Does a chaos step touch a protected target (a system namespace, or a
/// control-plane node for a Cordon)? The fail-closed re-check the net thread
/// runs on every step before executing a drill — defense-in-depth behind the
/// pure `plan_chaos` guards.
fn chaos_step_protected(step: &chaos::ChaosStep, world: &ObservedWorld) -> bool {
    match step {
        chaos::ChaosStep::Evict { namespace, .. } => chaos::ns_protected(namespace),
        chaos::ChaosStep::Partition(spec) => chaos::ns_protected(&spec.namespace),
        chaos::ChaosStep::Unpartition { namespace, .. } => chaos::ns_protected(namespace),
        chaos::ChaosStep::Apply(iv) => match chaos::iv_namespace(iv) {
            Some(ns) => chaos::ns_protected(ns),
            // A node-scoped Cordon — fail closed: allow only if the node resolves
            // to a known, non-control-plane node. Unknown/renamed ⇒ protected.
            None => {
                if let Intervention::Cordon { node, .. } = iv {
                    let known_safe = world.nodes.state().iter().any(|n| {
                        n.metadata.name.as_deref() == Some(node.as_str())
                            && !chaos::node_protected(n)
                    });
                    !known_safe
                } else {
                    false
                }
            }
        },
    }
}

pub struct WorldSnap {
    pub models: Arc<Models>,
    pub observed: ObservedWorld,
    /// Per-workload error-budget readings (the treasury), keyed by workload.
    /// `Arc` so the per-frame city-window lookup is a refcount bump.
    pub slo: Arc<HashMap<WorkloadRef, SloStatus>>,
}

pub struct Snapshot {
    pub hot: WorldSnap,
    pub warm: Option<WorldSnap>,
    pub pair: Option<Arc<PairSync>>,
    /// Merged severity-ordered concerns across both worlds (tagged with
    /// their cluster), plus the single aggregate pair-drift concern.
    pub attention: Arc<Vec<Concern>>,
}

pub struct Net {
    pub snapshot: Mutex<Option<Arc<Snapshot>>>,
    pub status: Mutex<String>,
    /// A pending hot-context switch requested by the UI.
    switch: Mutex<Option<String>>,
    /// The pod whose logs to tail (None = log panel closed).
    log_req: Mutex<Option<LogReq>>,
    /// The latest fetched tail.
    log_tail: Mutex<LogTail>,
    /// A confirmed pod eviction the UI has queued (the only write path).
    evict_req: Mutex<Option<EvictReq>>,
    /// Transient result of the last eviction, shown as a toast then cleared.
    evict_status: Mutex<Option<String>>,
    /// RBAC cache: can the user delete pods in (cluster, namespace)? Filled by
    /// the net thread from `SelfSubjectAccessReview` probes; drives whether the
    /// evict control is enabled.
    evict_perm: Mutex<HashMap<(ClusterId, String), bool>>,
    /// Namespaces awaiting a permission probe.
    evict_perm_pending: Mutex<HashSet<(ClusterId, String)>>,
    /// A request to start a port-forward (the net thread resolves the port,
    /// binds the listener, and appends to `forwards`).
    forward_req: Mutex<Option<ForwardReq>>,
    /// Local ports the UI asked to stop (drained each tick).
    forward_stop: Mutex<Vec<u16>>,
    /// Live forwards, mirrored from the net thread's private handle list for
    /// the UI to render + stop.
    forwards: Mutex<Vec<ForwardInfo>>,
    /// RBAC cache for `create pods/portforward` (mirrors `evict_perm`).
    forward_perm: Mutex<HashMap<(ClusterId, String), bool>>,
    forward_perm_pending: Mutex<HashSet<(ClusterId, String)>>,
    /// A confirmed End-of-Turn commit: the staged interventions to apply to
    /// the hot cluster (dry-run-validated, then applied).
    plan_req: Mutex<Option<Vec<Intervention>>>,
    /// The result of the last commit (per-row), shown in the review window.
    plan_outcome: Mutex<Option<PlanOutcome>>,
    /// The namespace filter the net thread applies when building Models.
    ns_filter: Mutex<NamespaceFilter>,
    /// Resource browser: a one-shot discovery request + its cached result (the
    /// kinds, and any groups that failed to enumerate), the kind currently being
    /// LISTed, and that LIST's output.
    discover_req: AtomicBool,
    // `Arc` so the per-frame `kinds()` pull (the picker reads it every frame) is
    // a refcount bump, not a deep clone of the whole kind list.
    kinds: Mutex<Option<Arc<Vec<browse::KindEntry>>>>,
    discover_warnings: Mutex<Vec<String>>,
    browse_req: Mutex<Option<browse::KindEntry>>,
    browse_out: Mutex<BrowseOut>,
    /// In-session per-workload SLO target overrides the UI set (the city-window
    /// stepper). Drained each tick into the per-cluster `SloConfig`.
    slo_override_req: Mutex<Vec<(ClusterId, WorkloadRef, Option<f64>)>>,
    /// A confirmed chaos drill to run (one at a time).
    chaos_req: Mutex<Option<ChaosRun>>,
    /// The live game-day session (run result + recovery/budget tracking).
    chaos_session: Mutex<Option<ChaosSession>>,
    /// In-session chronicle of finished drills (newest first, capped). Archived
    /// when a new drill starts; cleared on context switch. No cross-run history.
    chaos_history: Mutex<Vec<ChaosRecord>>,
}

/// One finished game-day drill, for the in-session chronicle.
#[derive(Clone)]
pub struct ChaosRecord {
    pub experiment: String,
    pub target: String,
    pub summary: String,
}

impl ChaosRecord {
    /// Summarize a finished session into a one-line chronicle outcome.
    fn from_session(s: &ChaosSession) -> Self {
        ChaosRecord {
            experiment: s.experiment.clone(),
            target: s.target_label.clone(),
            summary: chaos_outcome_summary(s.recovered, s.dipped, s.recover_secs),
        }
    }
}

/// The one-line chronicle outcome for a finished drill — PURE + testable.
fn chaos_outcome_summary(recovered: bool, dipped: bool, recover_secs: Option<f64>) -> String {
    if recovered {
        match recover_secs {
            Some(secs) => format!("self-healed in {secs:.0}s"),
            None => "self-healed".into(),
        }
    } else if dipped {
        "degraded".into()
    } else {
        "stayed up".into()
    }
}

/// The resource browser's current LIST state (the net thread fills it; a
/// `None` result means "listing in progress"). The payload is an `Arc` so the
/// per-frame `browse_out()` pull is a refcount bump, not a deep copy of up to
/// `LIST_LIMIT` (possibly large) objects.
#[derive(Default, Clone)]
pub struct BrowseOut {
    pub result: Option<Result<Arc<browse::ListResult>, String>>,
}

impl Net {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(None),
            status: Mutex::new("starting…".into()),
            switch: Mutex::new(None),
            log_req: Mutex::new(None),
            log_tail: Mutex::new(LogTail::default()),
            evict_req: Mutex::new(None),
            evict_status: Mutex::new(None),
            evict_perm: Mutex::new(HashMap::new()),
            evict_perm_pending: Mutex::new(HashSet::new()),
            forward_req: Mutex::new(None),
            forward_stop: Mutex::new(Vec::new()),
            forwards: Mutex::new(Vec::new()),
            forward_perm: Mutex::new(HashMap::new()),
            forward_perm_pending: Mutex::new(HashSet::new()),
            plan_req: Mutex::new(None),
            plan_outcome: Mutex::new(None),
            ns_filter: Mutex::new(NamespaceFilter::All),
            discover_req: AtomicBool::new(false),
            kinds: Mutex::new(None),
            discover_warnings: Mutex::new(Vec::new()),
            browse_req: Mutex::new(None),
            browse_out: Mutex::new(BrowseOut::default()),
            slo_override_req: Mutex::new(Vec::new()),
            chaos_req: Mutex::new(None),
            chaos_session: Mutex::new(None),
            chaos_history: Mutex::new(Vec::new()),
        })
    }

    /// Set (or clear, with `None`) an in-session SLO target override for a
    /// workload — the city-window treasury stepper.
    pub fn set_slo_target(&self, cluster: ClusterId, wr: WorkloadRef, target: Option<f64>) {
        self.slo_override_req
            .lock()
            .unwrap()
            .push((cluster, wr, target));
    }

    /// Queue a confirmed chaos drill (the net thread runs it once).
    pub fn request_chaos(&self, run: ChaosRun) {
        *self.chaos_req.lock().unwrap() = Some(run);
    }

    /// The live game-day session (scorecard source), if any. The net thread
    /// owns its lifecycle (created on run, cleared on context switch), so the
    /// GUI never clears it — that would race a still-in-flight drill.
    pub fn chaos_session(&self) -> Option<ChaosSession> {
        self.chaos_session.lock().unwrap().clone()
    }

    /// The in-session chronicle of finished drills (newest first).
    pub fn chaos_history(&self) -> Vec<ChaosRecord> {
        self.chaos_history.lock().unwrap().clone()
    }

    /// Ask the net thread to discover resource kinds (once).
    pub fn request_discover(&self) {
        self.discover_req.store(true, Ordering::Relaxed);
    }

    /// Discovered kinds, if discovery has completed (an `Arc` — the per-frame
    /// picker pull is a refcount bump, not a deep copy).
    pub fn kinds(&self) -> Option<Arc<Vec<browse::KindEntry>>> {
        self.kinds.lock().unwrap().clone()
    }

    /// Groups discovery couldn't enumerate (for a "N unavailable" picker note).
    pub fn discover_warnings(&self) -> Vec<String> {
        self.discover_warnings.lock().unwrap().clone()
    }

    /// Ask the net thread to LIST `kind` (replaces any in-flight browse).
    pub fn request_browse(&self, kind: browse::KindEntry) {
        *self.browse_req.lock().unwrap() = Some(kind);
        // `result: None` is the "listing in progress" state.
        *self.browse_out.lock().unwrap() = BrowseOut::default();
    }

    pub fn browse_out(&self) -> BrowseOut {
        self.browse_out.lock().unwrap().clone()
    }

    pub fn clear_browse(&self) {
        *self.browse_req.lock().unwrap() = None;
        *self.browse_out.lock().unwrap() = BrowseOut::default();
    }

    /// Scope the built models to these namespaces (the net thread rebuilds on
    /// the next tick because the filter changed).
    pub fn set_namespace_filter(&self, filter: NamespaceFilter) {
        *self.ns_filter.lock().unwrap() = filter;
    }

    /// The active namespace filter.
    pub fn namespace_filter(&self) -> NamespaceFilter {
        self.ns_filter.lock().unwrap().clone()
    }

    /// Queue a confirmed End-of-Turn commit (the net thread dry-runs then
    /// applies it to the hot cluster).
    pub fn request_commit(&self, interventions: Vec<Intervention>) {
        *self.plan_req.lock().unwrap() = Some(interventions);
    }

    /// The result of the last commit attempt, if any.
    pub fn plan_outcome(&self) -> Option<PlanOutcome> {
        self.plan_outcome.lock().unwrap().clone()
    }

    pub fn clear_plan_outcome(&self) {
        *self.plan_outcome.lock().unwrap() = None;
    }

    /// Queue a confirmed pod eviction (the net thread runs it once).
    pub fn request_evict(&self, req: EvictReq) {
        *self.evict_req.lock().unwrap() = Some(req);
    }

    /// The transient result of the last eviction (a toast), if any.
    pub fn evict_status(&self) -> Option<String> {
        self.evict_status.lock().unwrap().clone()
    }

    /// May the user evict pods in (cluster, namespace)? `Some(true/false)` once
    /// the RBAC probe has answered, `None` while it's pending — asking also
    /// enqueues the probe, so the UI just polls this each frame.
    pub fn evict_allowed(&self, cluster: ClusterId, namespace: &str) -> Option<bool> {
        let key = (cluster, namespace.to_string());
        if let Some(b) = self.evict_perm.lock().unwrap().get(&key) {
            return Some(*b);
        }
        self.evict_perm_pending.lock().unwrap().insert(key);
        None
    }

    /// Start a port-forward for a pod (the net thread resolves the port).
    pub fn request_forward(&self, req: ForwardReq) {
        *self.forward_req.lock().unwrap() = Some(req);
    }

    /// Stop the forward listening on `local_port`.
    pub fn stop_forward(&self, local_port: u16) {
        self.forward_stop.lock().unwrap().push(local_port);
    }

    /// The live port-forwards, for the strip + pod rows.
    pub fn forwards(&self) -> Vec<ForwardInfo> {
        self.forwards.lock().unwrap().clone()
    }

    /// The live forward for (cluster, namespace, pod), if one exists — drives a
    /// pod row's button between "fwd" (start) and "stop :PORT".
    pub fn forward_for(
        &self,
        cluster: ClusterId,
        namespace: &str,
        pod: &str,
    ) -> Option<ForwardInfo> {
        self.forwards
            .lock()
            .unwrap()
            .iter()
            .find(|f| f.cluster == cluster && f.namespace == namespace && f.pod == pod)
            .cloned()
    }

    /// May the user port-forward in (cluster, namespace)? `Some(true/false)`
    /// once the RBAC probe answers, `None` while pending — asking enqueues the
    /// probe (mirrors `evict_allowed`).
    pub fn forward_allowed(&self, cluster: ClusterId, namespace: &str) -> Option<bool> {
        let key = (cluster, namespace.to_string());
        if let Some(b) = self.forward_perm.lock().unwrap().get(&key) {
            return Some(*b);
        }
        self.forward_perm_pending.lock().unwrap().insert(key);
        None
    }

    /// Tail this pod's logs (re-fetched on a poll until cleared).
    pub fn request_logs(&self, req: LogReq) {
        *self.log_req.lock().unwrap() = Some(req);
        *self.log_tail.lock().unwrap() = LogTail::default();
    }

    pub fn clear_logs(&self) {
        *self.log_req.lock().unwrap() = None;
    }

    /// The pod whose logs are currently requested — set the instant
    /// `request_logs` is called and held across tail resets, so the `p`
    /// toggle can re-issue even before the first fetch lands (unlike
    /// `log_tail().target`, which is None until a fetch completes).
    pub fn log_request(&self) -> Option<LogReq> {
        self.log_req.lock().unwrap().clone()
    }

    pub fn log_tail(&self) -> LogTail {
        self.log_tail.lock().unwrap().clone()
    }

    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    /// Ask the net thread to switch the hot cluster to `ctx`.
    pub fn request_switch(&self, ctx: String) {
        *self.switch.lock().unwrap() = Some(ctx);
    }
}

pub struct NetArgs {
    pub context: Option<String>,
    pub kubeconfig: Option<PathBuf>,
    pub warm: Option<String>,
    pub projections: Vec<String>,
    /// Global default SLO availability target (`--slo-target`, else 0.99).
    pub slo_default: f64,
}

pub fn spawn(args: NetArgs, net: Arc<Net>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            *net.status.lock().unwrap() = "connecting…".into();
            let hot_cluster =
                match client::connect(args.kubeconfig.as_deref(), args.context.as_deref()).await {
                    Ok(c) => c,
                    Err(err) => {
                        *net.status.lock().unwrap() = format!("connect failed: {err}");
                        return;
                    }
                };
            // A warm failure degrades to single-world rather than aborting.
            let warm_cluster = match &args.warm {
                Some(w) if *w == hot_cluster.meta.context => {
                    *net.status.lock().unwrap() =
                        "warm context equals hot; running single-world".into();
                    None
                }
                Some(w) => match client::connect(args.kubeconfig.as_deref(), Some(w)).await {
                    Ok(c) => Some(c),
                    Err(err) => {
                        *net.status.lock().unwrap() = format!("warm connect failed: {err}");
                        None
                    }
                },
                None => None,
            };
            let warm_ctx = warm_cluster.as_ref().map(|c| c.meta.context.clone());
            let make_label = |hot_ctx: &str, platform: &str| match &warm_ctx {
                Some(w) => format!("HOT {hot_ctx} / WARM {w}"),
                None => format!("{hot_ctx} · {platform}"),
            };
            let mut label =
                make_label(&hot_cluster.meta.context, hot_cluster.meta.platform.label());
            *net.status.lock().unwrap() = format!("{label} · exploring…");

            let dirty = Arc::new(AtomicBool::new(false));
            let ready_hot = Arc::new(AtomicBool::new(false));
            let ready_warm = Arc::new(AtomicBool::new(false));
            let sink = {
                let dirty = dirty.clone();
                let ready_hot = ready_hot.clone();
                let ready_warm = ready_warm.clone();
                move |id: ClusterId, delta: WorldDelta| {
                    if delta == WorldDelta::Ready {
                        match id {
                            ClusterId::Hot => ready_hot.store(true, Ordering::Relaxed),
                            ClusterId::Warm => ready_warm.store(true, Ordering::Relaxed),
                        }
                    }
                    dirty.store(true, Ordering::Relaxed);
                }
            };

            let hot_proj =
                client::resolve_projections(&hot_cluster.client, &args.projections).await;
            let mut hot_handle =
                watch::spawn(&hot_cluster, ClusterId::Hot, sink.clone(), &hot_proj);
            let warm_handle = match &warm_cluster {
                Some(c) => {
                    let proj = client::resolve_projections(&c.client, &args.projections).await;
                    Some(watch::spawn(c, ClusterId::Warm, sink.clone(), &proj))
                }
                None => None,
            };

            // Clients kept for on-demand log tails (hot follows switches).
            let mut hot_client = hot_cluster.client.clone();
            let warm_client = warm_cluster.as_ref().map(|c| c.client.clone());

            let mut tick = tokio::time::interval(Duration::from_millis(250));
            let mut ticks: u64 = 0;
            let mut last_log: Option<LogReq> = None;
            // The resolved container is cached per (cluster, ns, pod) so the
            // ~2s poll (and p/T/s toggles) don't re-issue an `Api::get` every
            // time — the container can't change mid-session.
            let mut log_container: Option<String> = None;
            let mut log_target: Option<(ClusterId, String, String)> = None;
            let mut evict_set: Option<u64> = None;
            let mut last_filter = NamespaceFilter::All;
            let mut last_browse: Option<String> = None;
            // Live port-forwards: the private handles (dropping one aborts its
            // accept loop + in-flight tunnels). `net.forwards` mirrors these for
            // the UI; the two stay in lock-step.
            let mut forwards: Vec<(ClusterId, portforward::Forward)> = Vec::new();
            // Treasury: per-workload availability rings, sampled every
            // `SLO_SAMPLE_TICKS` ticks (≈2s) from the *unfiltered* workloads.
            const SLO_SAMPLE_TICKS: u64 = 8;
            let mut slo = SloTracker::default();
            let mut slo_warm = SloTracker::default();
            // Per-cluster SLO config (default + in-session overrides) and the
            // latest annotation-declared targets (captured at each sample).
            let mut slo_cfg = SloConfig::new(args.slo_default);
            let mut slo_cfg_warm = SloConfig::new(args.slo_default);
            let mut slo_ann: HashMap<WorkloadRef, f64> = HashMap::new();
            let mut slo_ann_warm: HashMap<WorkloadRef, f64> = HashMap::new();
            loop {
                // Hot-context switch: connect the new cluster, then drop the
                // old handle (its informers abort) by reassigning. Snapshot
                // is cleared so the UI shows fog until the new world syncs.
                let requested = net.switch.lock().unwrap().take();
                if let Some(ctx) = requested {
                    *net.status.lock().unwrap() = format!("switching → {ctx} …");
                    match client::connect(args.kubeconfig.as_deref(), Some(&ctx)).await {
                        Ok(c) => {
                            // Don't strand the cluster we're leaving: if a live
                            // hot drill still has restore steps, undo them with the
                            // OLD client before we drop it (mirrors restore-on-exit).
                            let leaving_restore = net
                                .chaos_session
                                .lock()
                                .unwrap()
                                .as_ref()
                                .filter(|s| s.cluster == ClusterId::Hot && !s.restore.is_empty())
                                .map(|s| s.restore.clone());
                            if let Some(steps) = leaving_restore {
                                *net.status.lock().unwrap() =
                                    "restoring drill before switch…".into();
                                let _ = tokio::time::timeout(
                                    Duration::from_secs(25),
                                    actions::run_chaos(hot_client.clone(), &steps),
                                )
                                .await;
                            }
                            let proj =
                                client::resolve_projections(&c.client, &args.projections).await;
                            ready_hot.store(false, Ordering::Relaxed);
                            hot_client = c.client.clone();
                            hot_handle = watch::spawn(&c, ClusterId::Hot, sink.clone(), &proj);
                            label = make_label(&c.meta.context, c.meta.platform.label());
                            *net.status.lock().unwrap() = format!("{label} · exploring…");
                            *net.snapshot.lock().unwrap() = None;
                            // RBAC answers were for the old cluster.
                            net.evict_perm.lock().unwrap().clear();
                            net.evict_perm_pending.lock().unwrap().clear();
                            // Hot forwards point at the cluster we're leaving —
                            // drop them (abort their tunnels); warm survives.
                            forwards.retain(|(c, _)| *c != ClusterId::Hot);
                            net.forwards
                                .lock()
                                .unwrap()
                                .retain(|f| f.cluster != ClusterId::Hot);
                            net.forward_perm.lock().unwrap().clear();
                            net.forward_perm_pending.lock().unwrap().clear();
                            // Error budgets + SLO config belong to the old cluster.
                            slo.clear();
                            slo_cfg.clear_overrides();
                            slo_ann.clear();
                            // Any chaos drill belonged to the old cluster.
                            *net.chaos_req.lock().unwrap() = None;
                            *net.chaos_session.lock().unwrap() = None;
                            net.chaos_history.lock().unwrap().clear();
                            // Namespaces differ across clusters — reset.
                            *net.ns_filter.lock().unwrap() = NamespaceFilter::All;
                            // Discovered kinds + any open browse are the old
                            // cluster's — drop them so the browser re-discovers
                            // against the new cluster (a CRD on A may be absent
                            // on B). `last_browse` self-resets next tick (the
                            // cleared browse_req makes `breq` None).
                            *net.kinds.lock().unwrap() = None;
                            *net.discover_warnings.lock().unwrap() = Vec::new();
                            *net.browse_req.lock().unwrap() = None;
                            *net.browse_out.lock().unwrap() = BrowseOut::default();
                            // A same-named pod on the new cluster must re-resolve.
                            log_target = None;
                            log_container = None;
                        }
                        Err(err) => {
                            *net.status.lock().unwrap() = format!("switch failed: {err}");
                        }
                    }
                }

                tick.tick().await;
                ticks += 1;

                // Live log tail: fetch on first request and then every ~2s.
                let req = net.log_req.lock().unwrap().clone();
                if let Some(r) = req.clone()
                    && (req != last_log || ticks.is_multiple_of(8))
                {
                    let client = match r.cluster {
                        ClusterId::Warm => {
                            warm_client.clone().unwrap_or_else(|| hot_client.clone())
                        }
                        ClusterId::Hot => hot_client.clone(),
                    };
                    // Resolve the container once per pod target, not every poll.
                    // Retry while still unresolved (`None`) — first_container
                    // returns None on a transient get failure or a pod still
                    // ContainerCreating, and a multi-container pod NEEDS a name
                    // (else the tail errors); a single-container pod resolves to
                    // Some immediately so this doesn't spin.
                    let target = (r.cluster, r.namespace.clone(), r.pod.clone());
                    if log_target.as_ref() != Some(&target) || log_container.is_none() {
                        log_container =
                            logs::first_container(client.clone(), &r.namespace, &r.pod).await;
                        log_target = Some(target);
                    }
                    let opts = logs::LogOpts {
                        previous: r.previous,
                        timestamps: r.timestamps,
                        window: r.window,
                    };
                    let res =
                        logs::tail(client, &r.namespace, &r.pod, log_container.clone(), &opts)
                            .await;
                    // Only store if still the requested target.
                    if net.log_req.lock().unwrap().as_ref() == Some(&r) {
                        let mut g = net.log_tail.lock().unwrap();
                        g.target = Some(r.clone());
                        match res {
                            Ok(t) => {
                                g.text = t;
                                g.error = None;
                            }
                            Err(e) => {
                                // Drop stale text so a later copy/export can't
                                // grab the previous tail behind the error.
                                g.text.clear();
                                g.error = Some(e);
                            }
                        }
                    }
                }
                last_log = req;

                // Resource browser: one-shot discovery, then LIST the requested
                // kind (re-LIST on change or every ~2s, hot cluster).
                if net.discover_req.swap(false, Ordering::Relaxed) {
                    let d = browse::discover(&hot_client).await;
                    *net.discover_warnings.lock().unwrap() = d.warnings;
                    *net.kinds.lock().unwrap() = Some(Arc::new(d.kinds));
                }
                let breq = net.browse_req.lock().unwrap().clone();
                if let Some(k) = breq.clone() {
                    // (Re-)LIST when the kind changed, when the result slot was
                    // just blanked by a fresh request (incl. re-selecting the
                    // SAME kind — `request_browse` resets `browse_out`), or on
                    // the periodic refresh. Keying only off `last_browse` would
                    // strand a same-kind re-request on "listing…" forever.
                    let pending = net.browse_out.lock().unwrap().result.is_none();
                    if pending
                        || last_browse.as_deref() != Some(k.label().as_str())
                        || ticks.is_multiple_of(8)
                    {
                        let filter = net.namespace_filter();
                        // Client-side deadline so a hung LIST can't freeze the
                        // whole net loop (logs/evict/commit/snapshot all run
                        // after this in the same tick).
                        let res = match tokio::time::timeout(
                            Duration::from_secs(25),
                            browse::list_kind(&hot_client, &k, &filter),
                        )
                        .await
                        {
                            Ok(r) => r,
                            Err(_) => Err("list timed out".to_string()),
                        };
                        let stored = BrowseOut {
                            result: Some(res.map(Arc::new)),
                        };
                        // Store only if still the requested kind.
                        if net.browse_req.lock().unwrap().as_ref().map(|r| r.label())
                            == Some(k.label())
                        {
                            *net.browse_out.lock().unwrap() = stored;
                        }
                    }
                }
                last_browse = breq.map(|k| k.label());

                // Confirmed eviction — the only write the app performs. Run it
                // once, report the result as a transient toast; the watch will
                // observe the pod's disappearance on a later tick. (Take the
                // request into a local first so the lock isn't held over the
                // await.)
                let evict = net.evict_req.lock().unwrap().take();
                if let Some(ev) = evict {
                    let client = match ev.cluster {
                        ClusterId::Warm => {
                            warm_client.clone().unwrap_or_else(|| hot_client.clone())
                        }
                        ClusterId::Hot => hot_client.clone(),
                    };
                    *net.evict_status.lock().unwrap() =
                        Some(format!("evicting {}/{} …", ev.namespace, ev.pod));
                    let res = actions::evict_pod(client, &ev.namespace, &ev.pod).await;
                    *net.evict_status.lock().unwrap() = Some(match res {
                        Ok(()) => format!("evicted {}/{}", ev.namespace, ev.pod),
                        Err(e) => format!("evict failed: {e}"),
                    });
                    evict_set = Some(ticks);
                    dirty.store(true, Ordering::Relaxed);
                }
                if let Some(t0) = evict_set
                    && ticks.saturating_sub(t0) > 12
                {
                    *net.evict_status.lock().unwrap() = None;
                    evict_set = None;
                }

                // Port-forward: start a requested forward (resolve the port,
                // bind a local listener) and stop any the UI asked to close.
                // Not a cluster write — but gated like one (RBAC pre-checked,
                // explicit start, visible + stoppable). The transient toast
                // reuses `evict_status` (the shared action-toast slot, as commit
                // does); `forwards` is the persistent live list.
                let freq = net.forward_req.lock().unwrap().take();
                if let Some(fr) = freq {
                    let client = match fr.cluster {
                        ClusterId::Warm => {
                            warm_client.clone().unwrap_or_else(|| hot_client.clone())
                        }
                        ClusterId::Hot => hot_client.clone(),
                    };
                    // Skip if already forwarding this exact pod (no duplicates).
                    let dup = net.forwards.lock().unwrap().iter().any(|f| {
                        f.cluster == fr.cluster && f.namespace == fr.namespace && f.pod == fr.pod
                    });
                    if dup {
                        // Leave the existing one; nothing to do.
                    } else {
                        // Bound the port lookup (a get + maybe a Service LIST) so
                        // a hung apiserver can't freeze the net loop — same guard
                        // the resource browser's LIST got in the FMEA pass.
                        let resolved = tokio::time::timeout(
                            Duration::from_secs(15),
                            portforward::default_port(client.clone(), &fr.namespace, &fr.pod),
                        )
                        .await;
                        match resolved {
                            Ok(Some(port)) => {
                                match portforward::start(client, &fr.namespace, &fr.pod, port).await
                                {
                                    Ok(fwd) => {
                                        let local = fwd.local_port;
                                        *net.evict_status.lock().unwrap() = Some(format!(
                                            "forwarding 127.0.0.1:{local} -> {}/{}:{port}",
                                            fr.namespace, fr.pod
                                        ));
                                        net.forwards.lock().unwrap().push(ForwardInfo {
                                            cluster: fr.cluster,
                                            namespace: fr.namespace.clone(),
                                            pod: fr.pod.clone(),
                                            pod_port: port,
                                            local_port: local,
                                        });
                                        forwards.push((fr.cluster, fwd));
                                    }
                                    Err(e) => {
                                        *net.evict_status.lock().unwrap() =
                                            Some(format!("forward failed: {e}"));
                                    }
                                }
                            }
                            Ok(None) => {
                                *net.evict_status.lock().unwrap() = Some(format!(
                                    "{}/{}: no forwardable port found",
                                    fr.namespace, fr.pod
                                ));
                            }
                            Err(_) => {
                                *net.evict_status.lock().unwrap() = Some(format!(
                                    "{}/{}: port lookup timed out",
                                    fr.namespace, fr.pod
                                ));
                            }
                        }
                        evict_set = Some(ticks);
                    }
                }
                let stops: Vec<u16> = net.forward_stop.lock().unwrap().drain(..).collect();
                for lp in stops {
                    if let Some(pos) = forwards.iter().position(|(_, f)| f.local_port == lp) {
                        // Remove → drop → abort the accept loop + its tunnels.
                        let (_, fwd) = forwards.remove(pos);
                        drop(fwd);
                        net.forwards.lock().unwrap().retain(|f| f.local_port != lp);
                        *net.evict_status.lock().unwrap() = Some(format!("stopped forward :{lp}"));
                        evict_set = Some(ticks);
                    }
                }
                // Reap forwards whose backing pod is gone (evicted / rescheduled).
                // A forward targets a *specific* pod, so once the pod disappears
                // the tunnel is dead — drop it rather than leave a black-holing
                // "stop :PORT" in the UI. Guarded on readiness so an unsynced
                // store (initial sync, post-switch) can't wrongly reap.
                let hot_ready = ready_hot.load(Ordering::Relaxed);
                let warm_ready = ready_warm.load(Ordering::Relaxed);
                let dead: Vec<u16> = forwards
                    .iter()
                    .filter(|(cluster, f)| {
                        let (world, ready) = match cluster {
                            ClusterId::Hot => (&hot_handle.world, hot_ready),
                            ClusterId::Warm => match warm_handle.as_ref() {
                                Some(h) => (&h.world, warm_ready),
                                None => return true, // warm world gone → dead
                            },
                        };
                        ready && !world.pod_exists(&f.namespace, &f.pod)
                    })
                    .map(|(_, f)| f.local_port)
                    .collect();
                if !dead.is_empty() {
                    forwards.retain(|(_, f)| !dead.contains(&f.local_port));
                    net.forwards
                        .lock()
                        .unwrap()
                        .retain(|f| !dead.contains(&f.local_port));
                    *net.evict_status.lock().unwrap() = Some(if dead.len() == 1 {
                        format!("forward :{} ended — pod gone", dead[0])
                    } else {
                        format!("{} forwards ended — pods gone", dead.len())
                    });
                    evict_set = Some(ticks);
                    dirty.store(true, Ordering::Relaxed);
                }

                // End-of-Turn commit (hot cluster): the shared write file
                // dry-runs every staged change (also enforcing RBAC) and only
                // applies for real if all pass. The per-row outcome goes back
                // to the review window; the toast summarizes it.
                let commit = net.plan_req.lock().unwrap().take();
                if let Some(ivs) = commit {
                    let outcome = actions::commit_interventions(hot_client.clone(), &ivs).await;
                    *net.evict_status.lock().unwrap() = Some(if outcome.applied {
                        let n_ok = outcome.rows.iter().filter(|r| r.ok).count();
                        format!("committed {n_ok}/{} change(s)", outcome.rows.len())
                    } else {
                        format!(
                            "commit blocked — {} change(s) failed dry-run",
                            outcome.rows.len()
                        )
                    });
                    *net.plan_outcome.lock().unwrap() = Some(outcome);
                    evict_set = Some(ticks);
                    dirty.store(true, Ordering::Relaxed);
                }

                // Chaos drill (hot cluster): run a confirmed game-day experiment
                // once, reusing the gated write primitives. Fail-closed
                // protected-namespace re-check (a UI bug can't aim chaos at the
                // control plane). Seed the scorecard: budget before, then track
                // recovery + spend on the SLO samples below.
                let chaos_run = net.chaos_req.lock().unwrap().take();
                if let Some(run) = chaos_run {
                    // Fail-closed re-check of EVERY step — inject AND restore —
                    // so a (future) UI bug that decoupled a step's namespace/node
                    // from the target can't slip a control-plane mutation past
                    // the guard. Covers all four step kinds (incl. a Cordon's
                    // node, re-verified against the live node objects).
                    let protected = run
                        .steps
                        .iter()
                        .chain(run.restore.iter())
                        .any(|s| chaos_step_protected(s, &hot_handle.world));
                    if protected {
                        *net.evict_status.lock().unwrap() =
                            Some("chaos refused: protected target".into());
                        evict_set = Some(ticks);
                    } else {
                        let target_label = match &run.subject {
                            Subject::Workload(wr) => format!("{}/{}", wr.namespace, wr.name),
                            Subject::Node(n) => format!("node {n}"),
                        };
                        // Budget tracking only makes sense for a single workload
                        // subject (a node drill spans many).
                        let budget_before = match &run.subject {
                            Subject::Workload(wr) => {
                                let (t, _) = slo_cfg.resolve(wr, slo_ann.get(wr).copied());
                                slo.status(wr, t)
                            }
                            Subject::Node(_) => None,
                        };
                        // Steady-state gate: was the watch set healthy before the
                        // drill? (Captured pre-inject from the live store.) An empty
                        // watch set asserts nothing → treat as healthy (no spurious
                        // "baseline noisy" warning, e.g. a cordon with no workloads).
                        let healthy_before = run.watch.is_empty()
                            || chaos::workloads_healthy(
                                &build_workloads(&hot_handle.world),
                                &run.watch,
                            );
                        *net.evict_status.lock().unwrap() = Some(format!(
                            "running chaos: {} on {target_label}",
                            run.experiment
                        ));
                        // Bounded so a hung call can't freeze the net loop.
                        let outcome = tokio::time::timeout(
                            Duration::from_secs(25),
                            actions::run_chaos(hot_client.clone(), &run.steps),
                        )
                        .await
                        .ok();
                        *net.evict_status.lock().unwrap() = Some(match &outcome {
                            Some(o) => format!(
                                "chaos: {}/{} step(s)",
                                o.rows.iter().filter(|r| r.ok).count(),
                                o.rows.len()
                            ),
                            None => "chaos: timed out".into(),
                        });
                        // Archive the previous (now-superseded) drill into the
                        // in-session chronicle before this one replaces it — but
                        // not when THIS run is just an undo (it's the same drill,
                        // not a new one), which would double-log it.
                        if !run.is_restore
                            && let Some(prev) = net.chaos_session.lock().unwrap().as_ref()
                        {
                            let mut h = net.chaos_history.lock().unwrap();
                            h.insert(0, ChaosRecord::from_session(prev));
                            h.truncate(10);
                        }
                        // Arm auto-restore only when there's something to undo.
                        let auto_restore_tick = run
                            .auto_restore_secs
                            .filter(|_| !run.restore.is_empty())
                            .map(|secs| ticks + (secs / 0.25).max(1.0) as u64);
                        *net.chaos_session.lock().unwrap() = Some(ChaosSession {
                            cluster: run.cluster,
                            experiment: run.experiment,
                            subject: run.subject,
                            score_kind: run.score_kind,
                            target_label,
                            blast: run.blast,
                            budget_before,
                            budget_after: budget_before,
                            dipped: false,
                            recovered: false,
                            recover_secs: None,
                            restore: run.restore,
                            watch: run.watch,
                            outcome,
                            started_tick: ticks,
                            auto_restore_tick,
                            healthy_before,
                            recovery_series: Vec::new(),
                            detect_tick: None,
                            restored: run.is_restore,
                        });
                        evict_set = Some(ticks);
                        dirty.store(true, Ordering::Relaxed);
                    }
                }

                // Auto-restore: if a live hot session armed it and the deadline
                // passed, run the restore now (opt-in "auto-undo"). The restore
                // steps were vetted fail-closed when the drill was created.
                let auto_restore: Option<Vec<chaos::ChaosStep>> = {
                    let g = net.chaos_session.lock().unwrap();
                    g.as_ref()
                        .filter(|s| s.cluster == ClusterId::Hot && !s.restore.is_empty())
                        .and_then(|s| s.auto_restore_tick)
                        .filter(|deadline| ticks >= *deadline)
                        .map(|_| g.as_ref().map(|s| s.restore.clone()).unwrap_or_default())
                };
                if let Some(restore_steps) = auto_restore {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(25),
                        actions::run_chaos(hot_client.clone(), &restore_steps),
                    )
                    .await;
                    if let Some(s) = net.chaos_session.lock().unwrap().as_mut() {
                        s.restore.clear(); // restored — drop the Restore button
                        s.auto_restore_tick = None;
                        // The injection's static notes ("still cordoned" / "policy
                        // applied") are stale post-undo; show the recovery frame,
                        // and mark it restored so the scorecard doesn't claim the
                        // cluster "self-healed" (we undid it).
                        s.score_kind = ScoreKind::Workload;
                        s.restored = true;
                    }
                    *net.evict_status.lock().unwrap() = Some("chaos: auto-restored".into());
                    evict_set = Some(ticks);
                    dirty.store(true, Ordering::Relaxed);
                }

                // Answer any pending evict-permission (RBAC) probes; cache the
                // result so the UI can enable/disable the evict control. Deny
                // on error (the safe default).
                let perm_todo: Vec<(ClusterId, String)> =
                    net.evict_perm_pending.lock().unwrap().drain().collect();
                for (cluster, ns) in perm_todo {
                    let client = match cluster {
                        ClusterId::Warm => {
                            warm_client.clone().unwrap_or_else(|| hot_client.clone())
                        }
                        ClusterId::Hot => hot_client.clone(),
                    };
                    let allowed = actions::can_evict_pod(client, &ns).await.unwrap_or(false);
                    net.evict_perm
                        .lock()
                        .unwrap()
                        .insert((cluster, ns), allowed);
                }

                // Answer pending port-forward (RBAC) probes the same way.
                let fperm_todo: Vec<(ClusterId, String)> =
                    net.forward_perm_pending.lock().unwrap().drain().collect();
                for (cluster, ns) in fperm_todo {
                    let client = match cluster {
                        ClusterId::Warm => {
                            warm_client.clone().unwrap_or_else(|| hot_client.clone())
                        }
                        ClusterId::Hot => hot_client.clone(),
                    };
                    let allowed = portforward::can_forward(client, &ns).await.unwrap_or(false);
                    net.forward_perm
                        .lock()
                        .unwrap()
                        .insert((cluster, ns), allowed);
                }

                // Drain in-session SLO target overrides (the city-window
                // stepper) into the per-cluster config; force a rebuild so the
                // new target shows immediately.
                let overrides: Vec<(ClusterId, WorkloadRef, Option<f64>)> =
                    net.slo_override_req.lock().unwrap().drain(..).collect();
                if !overrides.is_empty() {
                    for (cluster, wr, target) in overrides {
                        match cluster {
                            ClusterId::Hot => slo_cfg.set_override(wr, target),
                            ClusterId::Warm => slo_cfg_warm.set_override(wr, target),
                        }
                    }
                    dirty.store(true, Ordering::Relaxed);
                }

                // Treasury: sample each workload's availability into the SLO
                // rings every ~2s (from the *unfiltered* workloads — SLOs track
                // the whole cluster regardless of the namespace view) and force
                // a rebuild so the published budgets stay fresh on an idle
                // cluster. The first sample lands immediately so the city window
                // shows "warming" right away rather than blank. The same rows
                // carry the per-workload annotation target (captured here).
                if ready_hot.load(Ordering::Relaxed)
                    && (ticks == 1 || ticks.is_multiple_of(SLO_SAMPLE_TICKS))
                {
                    let rows = build_workloads(&hot_handle.world);
                    slo_ann = rows
                        .iter()
                        .filter_map(|r| r.slo_target.map(|t| (r.r.clone(), t)))
                        .collect();
                    slo.record(&rows);
                    // Chaos scorecard: track the cluster's response from the fresh
                    // hot rows (chaos is hot-only). Branches on the subject:
                    // a workload tracks its own outage/recovery + budget; a node
                    // drill tracks its drained workloads back to full strength.
                    if let Some(sess) = net.chaos_session.lock().unwrap().as_mut()
                        && sess.cluster == ClusterId::Hot
                    {
                        let secs = ticks.saturating_sub(sess.started_tick) as f64 * 0.25;
                        match &sess.subject {
                            Subject::Workload(wr) => {
                                let (t, _) = slo_cfg.resolve(wr, slo_ann.get(wr).copied());
                                // Keep the prior reading when the target has no
                                // SLO right now (an Outage scaled it to 0 → the
                                // ring is pruned) — else the budget line vanishes.
                                if let Some(s) = slo.status(wr, t) {
                                    sess.budget_after = Some(s);
                                }
                                // Recovery counts once the target actually went
                                // down (ready == 0). A kill it shrugged off (other
                                // replicas stayed up) never dips → "stayed up".
                                if let Some(row) = rows.iter().find(|r| &r.r == wr) {
                                    if row.ready == 0 {
                                        sess.dipped = true;
                                    }
                                    if sess.dipped && !sess.recovered && row.ready >= 1 {
                                        sess.recovered = true;
                                        sess.recover_secs = Some(secs);
                                    }
                                }
                            }
                            Subject::Node(_) => {
                                // Watch the drained workloads in aggregate: dip
                                // when below desired, recover when back to full.
                                let watched: Vec<&WorkloadRow> = sess
                                    .watch
                                    .iter()
                                    .filter_map(|wr| rows.iter().find(|r| &r.r == wr))
                                    .collect();
                                if !watched.is_empty() {
                                    let ready: i32 = watched.iter().map(|r| r.ready).sum();
                                    let desired: i32 =
                                        watched.iter().map(|r| r.desired.max(0)).sum();
                                    if ready < desired {
                                        sess.dipped = true;
                                    }
                                    if sess.dipped
                                        && !sess.recovered
                                        && ready >= desired
                                        && ready >= 1
                                    {
                                        sess.recovered = true;
                                        sess.recover_secs = Some(secs);
                                    }
                                }
                            }
                        }
                        // Recovery curve: the watch set's ready-fraction now.
                        let (r, d): (i32, i32) = match &sess.subject {
                            Subject::Workload(wr) => rows
                                .iter()
                                .find(|row| &row.r == wr)
                                .map(|row| (row.ready, row.desired.max(1)))
                                .unwrap_or((0, 1)),
                            Subject::Node(_) => {
                                let w: Vec<&WorkloadRow> = sess
                                    .watch
                                    .iter()
                                    .filter_map(|wr| rows.iter().find(|row| &row.r == wr))
                                    .collect();
                                let ready: i32 = w.iter().map(|row| row.ready).sum();
                                let desired: i32 =
                                    w.iter().map(|row| row.desired.max(0)).sum::<i32>().max(1);
                                (ready, desired)
                            }
                        };
                        let frac = (r as f32 / d as f32).clamp(0.0, 1.0);
                        if sess.recovery_series.len() >= 120 {
                            sess.recovery_series.remove(0);
                        }
                        sess.recovery_series.push(frac);
                    }
                    if let Some(h) = warm_handle
                        .as_ref()
                        .filter(|_| ready_warm.load(Ordering::Relaxed))
                    {
                        let wrows = build_workloads(&h.world);
                        slo_ann_warm = wrows
                            .iter()
                            .filter_map(|r| r.slo_target.map(|t| (r.r.clone(), t)))
                            .collect();
                        slo_warm.record(&wrows);
                    }
                    dirty.store(true, Ordering::Relaxed);
                }

                // Rebuild when the world changed (dirty) OR the namespace
                // filter changed under us; either way re-derive with it.
                if !ready_hot.load(Ordering::Relaxed) {
                    continue;
                }
                let was_dirty = dirty.swap(false, Ordering::Relaxed);
                let filter = net.namespace_filter();
                if !was_dirty && filter == last_filter {
                    continue;
                }
                last_filter = filter.clone();
                let hot_models = Arc::new(Models::build_filtered(&hot_handle.world, &filter));
                // MTTD: note the first tick the attention queue flags the drill's
                // subject (the fresh hot concerns are right here).
                if let Some(sess) = net.chaos_session.lock().unwrap().as_mut()
                    && sess.cluster == ClusterId::Hot
                    && sess.detect_tick.is_none()
                {
                    let flagged =
                        hot_models
                            .attention
                            .iter()
                            .any(|c| match (&sess.subject, &c.target) {
                                (Subject::Workload(wr), Target::Workload(t)) => t == wr,
                                (Subject::Node(n), Target::Node(t)) => t == n,
                                _ => false,
                            });
                    if flagged {
                        sess.detect_tick = Some(ticks);
                    }
                }
                let hot_slo: Arc<HashMap<WorkloadRef, SloStatus>> = Arc::new(
                    slo.statuses_with(|wr| slo_cfg.resolve(wr, slo_ann.get(wr).copied()))
                        .into_iter()
                        .collect(),
                );
                let warm = warm_handle
                    .as_ref()
                    .filter(|_| ready_warm.load(Ordering::Relaxed))
                    .map(|h| WorldSnap {
                        models: Arc::new(Models::build_filtered(&h.world, &filter)),
                        observed: h.world.clone(),
                        slo: Arc::new(
                            slo_warm
                                .statuses_with(|wr| {
                                    slo_cfg_warm.resolve(wr, slo_ann_warm.get(wr).copied())
                                })
                                .into_iter()
                                .collect(),
                        ),
                    });
                let pair = warm
                    .as_ref()
                    .map(|w| Arc::new(PairSync::build(&hot_handle.world, &w.observed, &filter)));

                let mut merged = hot_models.attention.clone();
                if let Some(w) = &warm {
                    merged.extend(w.models.attention.iter().cloned().map(|mut c| {
                        c.cluster = ClusterId::Warm;
                        c
                    }));
                }
                if let Some(c) = pair.as_ref().and_then(|p| p.concern()) {
                    merged.push(c);
                }
                // Treasury concerns: a workload burning / exhausting its error
                // budget — but only if a stronger point-in-time concern doesn't
                // already cover it (keeps the "city in trouble, not 40 alarms"
                // rule, and lets the budget surface the *flaky-but-up-now* cases
                // the instant detectors miss).
                let flagged: HashSet<(ClusterId, WorkloadRef)> = merged
                    .iter()
                    .filter_map(|c| match &c.target {
                        Target::Workload(wr) => Some((c.cluster, wr.clone())),
                        _ => None,
                    })
                    .collect();
                // The SLO map is unfiltered (every city window shows its budget),
                // but the *queue* concern respects the active namespace filter,
                // like every other concern — so a filtered-out workload can't leak
                // a budget alarm into the scoped view.
                for (wr, st) in hot_slo.iter() {
                    if filter.matches(&wr.namespace)
                        && !flagged.contains(&(ClusterId::Hot, wr.clone()))
                        && let Some(c) = slo::budget_concern(wr, st)
                    {
                        merged.push(c);
                    }
                }
                if let Some(w) = &warm {
                    for (wr, st) in w.slo.iter() {
                        if filter.matches(&wr.namespace)
                            && !flagged.contains(&(ClusterId::Warm, wr.clone()))
                            && let Some(mut c) = slo::budget_concern(wr, st)
                        {
                            c.cluster = ClusterId::Warm;
                            merged.push(c);
                        }
                    }
                }
                // Game Day: while a drill is fresh, announce the raid in the queue
                // (the product's spine) so `n`/`B` route to it; drops after ~30s.
                if let Some(sess) = net.chaos_session.lock().unwrap().as_ref()
                    && sess.cluster == ClusterId::Hot
                    && ticks.saturating_sub(sess.started_tick) < 120
                {
                    let target = match &sess.subject {
                        Subject::Workload(wr) => Target::Workload(wr.clone()),
                        Subject::Node(n) => Target::Node(n.clone()),
                    };
                    merged.push(Concern {
                        severity: Severity::Warning,
                        title: "Game Day: raid underway".into(),
                        detail: format!("{} on {}", sess.experiment, sess.target_label),
                        target,
                        probe: None,
                        key: "chaos-raid".into(),
                        cluster: ClusterId::Hot,
                    });
                }
                merged.sort_by(|a, b| {
                    b.severity
                        .cmp(&a.severity)
                        .then_with(|| a.key.cmp(&b.key))
                        .then_with(|| a.cluster.cmp(&b.cluster))
                });

                *net.status.lock().unwrap() = label.clone();
                *net.snapshot.lock().unwrap() = Some(Arc::new(Snapshot {
                    hot: WorldSnap {
                        models: hot_models,
                        observed: hot_handle.world.clone(),
                        slo: hot_slo,
                    },
                    warm,
                    pair,
                    attention: Arc::new(merged),
                }));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::chaos_outcome_summary;

    #[test]
    fn chaos_outcome_summary_classifies_the_drill() {
        assert_eq!(
            chaos_outcome_summary(true, true, Some(4.0)),
            "self-healed in 4s"
        );
        assert_eq!(chaos_outcome_summary(true, true, None), "self-healed");
        assert_eq!(chaos_outcome_summary(false, true, None), "degraded");
        assert_eq!(chaos_outcome_summary(false, false, None), "stayed up");
    }
}
