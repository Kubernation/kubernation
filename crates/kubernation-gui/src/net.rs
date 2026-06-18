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
use kubernation_core::k8s::{actions, browse, client, logs, watch};
use kubernation_core::state::attention::Concern;
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::Models;
use kubernation_core::state::observed::ObservedWorld;
use kubernation_core::state::pair::PairSync;
use kubernation_core::state::planned::Intervention;

/// Which pod's logs the UI wants tailed.
#[derive(Clone, PartialEq, Eq)]
pub struct LogReq {
    pub cluster: ClusterId,
    pub namespace: String,
    pub pod: String,
    /// Tail the previously-terminated container (`kubectl logs --previous`).
    /// Flipping it changes the request, so the poll re-fetches automatically.
    pub previous: bool,
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

/// The End-of-Turn commit result now lives in the core write file
/// (`k8s::actions`), shared with the TUI; the GUI keeps its familiar name.
pub use kubernation_core::k8s::actions::CommitOutcome as PlanOutcome;

pub struct WorldSnap {
    pub models: Arc<Models>,
    pub observed: ObservedWorld,
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
    kinds: Mutex<Option<Vec<browse::KindEntry>>>,
    discover_warnings: Mutex<Vec<String>>,
    browse_req: Mutex<Option<browse::KindEntry>>,
    browse_out: Mutex<BrowseOut>,
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
            plan_req: Mutex::new(None),
            plan_outcome: Mutex::new(None),
            ns_filter: Mutex::new(NamespaceFilter::All),
            discover_req: AtomicBool::new(false),
            kinds: Mutex::new(None),
            discover_warnings: Mutex::new(Vec::new()),
            browse_req: Mutex::new(None),
            browse_out: Mutex::new(BrowseOut::default()),
        })
    }

    /// Ask the net thread to discover resource kinds (once).
    pub fn request_discover(&self) {
        self.discover_req.store(true, Ordering::Relaxed);
    }

    /// Discovered kinds, if discovery has completed.
    pub fn kinds(&self) -> Option<Vec<browse::KindEntry>> {
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
            let mut evict_set: Option<u64> = None;
            let mut last_filter = NamespaceFilter::All;
            let mut last_browse: Option<String> = None;
            loop {
                // Hot-context switch: connect the new cluster, then drop the
                // old handle (its informers abort) by reassigning. Snapshot
                // is cleared so the UI shows fog until the new world syncs.
                let requested = net.switch.lock().unwrap().take();
                if let Some(ctx) = requested {
                    *net.status.lock().unwrap() = format!("switching → {ctx} …");
                    match client::connect(args.kubeconfig.as_deref(), Some(&ctx)).await {
                        Ok(c) => {
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
                    let container =
                        logs::first_container(client.clone(), &r.namespace, &r.pod).await;
                    let res = logs::tail(client, &r.namespace, &r.pod, container, r.previous).await;
                    // Only store if still the requested target.
                    if net.log_req.lock().unwrap().as_ref() == Some(&r) {
                        let mut g = net.log_tail.lock().unwrap();
                        g.target = Some(r.clone());
                        match res {
                            Ok(t) => {
                                g.text = t;
                                g.error = None;
                            }
                            Err(e) => g.error = Some(e),
                        }
                    }
                }
                last_log = req;

                // Resource browser: one-shot discovery, then LIST the requested
                // kind (re-LIST on change or every ~2s, hot cluster).
                if net.discover_req.swap(false, Ordering::Relaxed) {
                    let d = browse::discover(&hot_client).await;
                    *net.discover_warnings.lock().unwrap() = d.warnings;
                    *net.kinds.lock().unwrap() = Some(d.kinds);
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
                let warm = warm_handle
                    .as_ref()
                    .filter(|_| ready_warm.load(Ordering::Relaxed))
                    .map(|h| WorldSnap {
                        models: Arc::new(Models::build_filtered(&h.world, &filter)),
                        observed: h.world.clone(),
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
                    },
                    warm,
                    pair,
                    attention: Arc::new(merged),
                }));
            }
        });
    });
}
