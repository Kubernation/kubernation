//! The attention queue: pure detectors over the observed world, aggregated
//! per workload/node so the operator sees "city in trouble", not a hundred
//! identical pod alarms. This is 4X's "next unit needs orders" loop.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use k8s_openapi::jiff;

use crate::events::ClusterId;

use super::filter::NamespaceFilter;
use super::model::{
    MapModel, OwnerIndex, PRESSURE_HIGH, PodState, RolloutStatus, WorkloadRef, WorkloadRow,
    ingress_backends, pod_oom_killed, pod_restarts, pod_state, prefer_previous,
};
use super::observed::ObservedWorld;
use super::saturation::SAT_PODS_HIGH;

/// How long ago a Warning event may have fired and still surface here.
const EVENT_WINDOW_MIN: i64 = 15;
/// Restart count at which a pod is "flapping" even without a waiting reason.
const RESTART_THRESHOLD: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn glyph(self) -> &'static str {
        match self {
            Severity::Critical => "‼",
            Severity::Warning => "!",
            Severity::Info => "·",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Node(String),
    Workload(WorkloadRef),
    /// No better destination known — land on the workload list.
    WorkloadList,
}

/// The offending pod a concern can take you straight into the logs of — the
/// detectors know it while aggregating, so the "city in trouble → and here's
/// why" jump is one key, not a hunt through the pod list. `None` for concerns
/// with no single log-worthy pod (replica gaps, nodes, connectivity, events).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogProbe {
    pub namespace: String,
    pub pod: String,
    /// Start on the previous container (a crash-looper's last words).
    pub previous: bool,
}

#[derive(Debug, Clone)]
pub struct Concern {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub target: Target,
    /// A representative offending pod to tail (the `L` verb), when one exists.
    pub probe: Option<LogProbe>,
    /// Stable identity for cycling; also the sort tiebreaker.
    pub key: String,
    /// Which member of the pair this belongs to. `build` is single-world
    /// and always tags Hot; the app re-tags the warm world's list.
    pub cluster: ClusterId,
}

/// The runbook hint for a concern: the next action to take, pointing at the
/// in-app verb where one fits ("the page points to the next action"). PURE +
/// testable — keyed on the concern's stable `key` prefix + whether it has a
/// log-worthy pod. `None` when there's no specific action beyond opening it.
pub fn next_action(c: &Concern) -> Option<String> {
    let logs = c.probe.is_some();
    let s = match c.key.split(':').next().unwrap_or("") {
        // Workload / bare-pod / job concerns: logs are the first move; the
        // timeline (`T`) answers "what changed right before this broke".
        "w" => {
            if logs {
                "L: tail logs · B: blast radius · T: what changed · click: open the city"
            } else {
                "B: blast radius · T: what changed · click: open the city"
            }
        }
        "b" | "j" => {
            if logs {
                "L: tail the offending pod's logs · T: what changed"
            } else {
                "open the target to see its pods + events · T: what changed"
            }
        }
        "n" => "B: blast radius · T: what changed · click: open the province",
        "p" => "check the PVC's StorageClass and whether a PV can bind",
        "i" => "the Ingress backend Service is missing — fix the backend name",
        "s" => "the Service selector matches no pods — check its selector / pod labels",
        "e" => "open the target to read its recent events",
        "harden" => "insecure config — open Advisors ▸ Hardening (realm defense)",
        "netpol" => {
            "unwalled & exposed — add a deny-by-default ingress NetworkPolicy · Advisors ▸ Network (WALLS)"
        }
        "slo" => "error budget burning — open the city's TREASURY (SLO) band",
        "pair" => "compare HOT vs WARM (sync chips + the workload list)",
        "chaos-raid" => "drill underway — B: watch the blast; the scorecard tracks recovery",
        // Fallback: at least point at the structural verbs that apply.
        _ => {
            if logs {
                "L: tail logs"
            } else {
                return None;
            }
        }
    };
    Some(s.to_string())
}

#[derive(Default)]
struct Agg {
    crash: u32,
    image: u32,
    config: u32,
    failed: u32,
    unsched: u32,
    oom: u32,
    flapping: u32,
    /// A representative log-worthy pod for this workload (first seen, but a
    /// crash-looper takes precedence so `previous` lands on the last words).
    probe: Option<LogProbe>,
    /// Which nodepools the failing pods landed in, and how many landed nowhere.
    ///
    /// T2-pre measured that pod-level failures cluster by WORKLOAD and scatter
    /// across the map's geography, so a pool-confined incident — a bad node
    /// image rolled to one nodepool — is invisible on the map AND unnamed in the
    /// queue, which aggregates by workload. This is the fact that was missing;
    /// see [`pool_confinement`].
    pools: BTreeSet<String>,
    /// Failing pods WITH a node, and without. Counted rather than derived from
    /// the per-reason tallies above: those can double-count one pod (a
    /// crash-looper past the restart threshold increments `crash` and
    /// `flapping`), so a sum of them is not a pod count.
    placed: u32,
    unplaced: u32,
}

/// "all N on pool X" — when that is a real claim about a real grouping.
///
/// PURE, unit-tested. The queue aggregates a workload's failing pods into one
/// concern, which is right ("city in trouble, not 40 pod alarms") but drops the
/// one thing a pool-confined incident is recognisable by. T2-pre measured a
/// failure confined to 100% of one nodepool and found NEITHER surface named it:
/// the map scatters it (zone-wide ordinals interleave pools, so it renders as 8
/// disconnected pieces) and the concern reads `ds churn/node-agent —
/// CrashLoopBackOff ×29` with no mention of where. This is that sentence.
///
/// `None` — say nothing — in four cases, because each would be a claim the data
/// does not support:
///
/// - **fewer than two placed pods.** One pod is trivially in one pool.
/// - **more than one pool.** Not confined.
/// - **the unpooled sentinel.** An absence is not a pool, so "all on pool
///   unpooled" would dress a missing label as a grouping (the `pool_line` rule).
/// - **a single-pool fleet.** True but vacuous: every node is in it, so the
///   sentence carries no information. This is the DEGENERATE refusal the
///   measuring instrument makes, in the product.
///
/// Unplaced pods (unschedulable, so no node and no pool) are excluded from the
/// claim and change its wording, because "all" must range over something real.
pub(crate) fn pool_confinement(
    pools: &BTreeSet<String>,
    placed: u32,
    unplaced: u32,
    fleet_pools: usize,
) -> Option<String> {
    if placed < 2 || fleet_pools < 2 {
        return None;
    }
    let only = match pools.iter().next() {
        Some(p) if pools.len() == 1 && p != crate::state::model::DEFAULT_POOL => p,
        _ => return None,
    };
    Some(if unplaced > 0 {
        format!("all {placed} placed on pool {only}")
    } else {
        format!("all {placed} on pool {only}")
    })
}

/// `"restarting repeatedly ×2 pods"` — the count with its UNIT.
///
/// Every counter in [`Agg`] counts PODS: the queue aggregates a workload's
/// failing pods into one concern ("city in trouble, not 40 pod alarms"), so `×N`
/// is how many pods, never how many restarts or how many times.
///
/// The unit used to be implicit, and `×N` is not read that way. `restarting
/// repeatedly ×1` on the dev cluster is one pod that has restarted **five**
/// times — `RESTART_THRESHOLD` is 5 — and it reads as one restart, which
/// understates it. Two lines below it in the same queue,
/// `events: ProvisioningFailed ×522` means 522 occurrences: the same suffix,
/// a different unit, adjacent on screen.
fn tally(label: &str, n: u32) -> String {
    format!("{label} ×{n} {}", if n == 1 { "pod" } else { "pods" })
}

impl Agg {
    fn any(&self) -> bool {
        self.crash
            + self.image
            + self.config
            + self.failed
            + self.unsched
            + self.oom
            + self.flapping
            > 0
    }

    fn classify(&mut self, reason: &str, state: PodState) {
        match reason {
            "CrashLoopBackOff" => self.crash += 1,
            "ImagePullBackOff" | "ErrImagePull" | "InvalidImageName" => self.image += 1,
            "CreateContainerConfigError" | "CreateContainerError" | "RunContainerError" => {
                self.config += 1
            }
            "Unschedulable" => self.unsched += 1,
            _ if state == PodState::Failing => self.failed += 1,
            _ => {}
        }
    }

    /// The single most important thing to say about this group of pods — the
    /// label and HOW MANY PODS it covers, not a rendered string.
    ///
    /// The caller renders it, because only the caller knows whether the count
    /// is worth saying: a workload concern aggregates its failing pods and the
    /// number is the point, while a bare-pod concern is about one named pod and
    /// "×1 pod" only repeats its own title.
    fn primary(&self) -> Option<(Severity, &'static str, u32)> {
        let crit = [
            (self.crash, "CrashLoopBackOff"),
            (self.image, "image pull failing"),
            (self.config, "container create failing"),
            (self.failed, "Failed"),
        ];
        for (n, label) in crit {
            if n > 0 {
                return Some((Severity::Critical, label, n));
            }
        }
        let warn = [
            (self.unsched, "unschedulable"),
            (self.oom, "OOM-killed recently"),
            (self.flapping, "restarting repeatedly"),
        ];
        for (n, label) in warn {
            if n > 0 {
                return Some((Severity::Warning, label, n));
            }
        }
        None
    }
}

/// Render a possibly-unknown ratio as a percentage. `None` is "unknown", never
/// `0%` — the whole point of the ratios being optional.
fn pct_or_unknown(r: Option<f64>) -> String {
    match r {
        Some(v) => format!("{:.0}%", v * 100.0),
        None => "unknown".to_string(),
    }
}

pub fn build(
    world: &ObservedWorld,
    map: &MapModel,
    workloads: &[WorkloadRow],
    filter: &NamespaceFilter,
    drain: &crate::state::pdb::DrainReport,
) -> Vec<Concern> {
    let idx = OwnerIndex::build(world);
    // Where each node sits, and how many pools the fleet has at all — the
    // second is what makes "all on pool X" informative rather than vacuous.
    let pool_of: HashMap<&str, &str> = map
        .zones
        .iter()
        .flat_map(|z| &z.nodes)
        .map(|t| (t.name.as_str(), t.pool.as_str()))
        .collect();
    let fleet_pools = pool_of
        .values()
        .filter(|p| **p != crate::state::model::DEFAULT_POOL)
        .collect::<BTreeSet<_>>()
        .len();
    let mut concerns: Vec<Concern> = Vec::new();
    // One snapshot for the whole pass — Store::state() clones a Vec per call.
    let pods = world.pods.state();

    // --- Jobs: object-level failure (the Job lost, not just one pod) --------
    // Jobs have no city screen, so a Job's own failure surfaces here as its own
    // line — and its failing pods fold under it (the pod loop below defers to
    // `covered_jobs`), keeping it one concern, not one-per-failed-pod.
    let mut covered_jobs: HashSet<(String, String)> = HashSet::new();
    // A Job has no city screen, so the `L` jump is the only path to its failed
    // pods' logs. Collect a representative pod per Job during the pod loop
    // (which classifies them) and patch the Job concerns afterward; record where
    // each Job concern landed so we can attach its probe.
    let mut job_probes: HashMap<(String, String), LogProbe> = HashMap::new();
    let mut job_concern_idx: Vec<(usize, (String, String))> = Vec::new();
    for job in world.jobs.state() {
        let ns = job.metadata.namespace.clone().unwrap_or_default();
        if !filter.matches(&ns) {
            continue;
        }
        let name = job.metadata.name.clone().unwrap_or_default();
        let st = job.status.as_ref();
        let cond = |t: &str| {
            st.and_then(|s| s.conditions.as_ref()).is_some_and(|cs| {
                cs.iter()
                    .any(|c| c.type_ == t && c.status.eq_ignore_ascii_case("true"))
            })
        };
        // A completed Job is quiet, even if it had retries along the way.
        let completions = job.spec.as_ref().and_then(|s| s.completions).unwrap_or(1);
        let succeeded = st.and_then(|s| s.succeeded).unwrap_or(0);
        if cond("Complete") || succeeded >= completions.max(1) {
            continue;
        }
        let failed = st.and_then(|s| s.failed).unwrap_or(0);
        let (severity, msg) = if cond("Failed") {
            (
                Severity::Critical,
                "failed (backoff limit reached)".to_string(),
            )
        } else if failed >= 1 {
            (
                Severity::Warning,
                format!("{failed} pod failure{}", if failed == 1 { "" } else { "s" }),
            )
        } else {
            continue;
        };
        covered_jobs.insert((ns.clone(), name.clone()));
        job_concern_idx.push((concerns.len(), (ns.clone(), name.clone())));
        concerns.push(Concern {
            cluster: ClusterId::Hot,
            severity,
            title: format!("job {ns}/{name} — {msg}"),
            detail: String::new(),
            target: Target::WorkloadList,
            probe: None, // filled in after the pod loop if a failed pod exists
            key: format!("j:{ns}/{name}"),
        });
    }

    // --- Pod-level signals, aggregated per owning workload -----------------
    let mut by_workload: BTreeMap<WorkloadRef, Agg> = BTreeMap::new();
    for pod in &pods {
        if !filter.matches_opt(pod.metadata.namespace.as_deref()) {
            continue;
        }
        let (state, reason) = pod_state(pod);
        let mut agg = Agg::default();
        agg.classify(&reason, state);
        if pod_oom_killed(pod) {
            agg.oom += 1;
        }
        if pod_restarts(pod) >= RESTART_THRESHOLD {
            agg.flapping += 1;
        }
        if !agg.any() {
            continue;
        }
        // A pod worth tailing — one that actually ran (so it has logs): a
        // crash loop, an OOM kill, a Failed pod, or a flapper. Pending /
        // Unschedulable / image-pull / config-error pods never started a
        // container, so there's nothing to tail.
        let ns = pod.metadata.namespace.clone().unwrap_or_default();
        let name = pod.metadata.name.clone().unwrap_or_default();
        let pod_probe =
            (agg.crash > 0 || agg.failed > 0 || agg.oom > 0 || agg.flapping > 0).then(|| {
                LogProbe {
                    namespace: ns.clone(),
                    pod: name.clone(),
                    previous: prefer_previous(state, &reason, pod_restarts(pod)),
                }
            });
        match idx.workload_of(pod) {
            Some(r) => {
                let e = by_workload.entry(r).or_default();
                e.crash += agg.crash;
                e.image += agg.image;
                e.config += agg.config;
                e.failed += agg.failed;
                e.unsched += agg.unsched;
                e.oom += agg.oom;
                e.flapping += agg.flapping;
                // Keep one representative pod; prefer a crash-looper so the
                // `L` jump opens its previous-container last words.
                if let Some(p) = pod_probe {
                    let better = match &e.probe {
                        None => true,
                        Some(cur) => p.previous && !cur.previous,
                    };
                    if better {
                        e.probe = Some(p);
                    }
                }
                match pod.spec.as_ref().and_then(|s| s.node_name.as_deref()) {
                    Some(n) => {
                        e.placed += 1;
                        if let Some(pool) = pool_of.get(n) {
                            e.pools.insert((*pool).to_string());
                        }
                    }
                    // Unschedulable: no node, so no pool. T2-pre §2.1 — this
                    // class has no position at all, and must not be folded into
                    // whatever pool the others landed in.
                    None => e.unplaced += 1,
                }
            }
            None => {
                // Bare pod, or Job-owned. A pod whose Job already has its own
                // concern folds under it (no per-pod spam for a failed Job) —
                // but lend that Job concern a log probe so the `L` jump can
                // still reach the failed batch pod's logs (preferring a
                // crash-looper, like the workload path).
                if let Some(j) = job_owner(pod)
                    && covered_jobs.contains(&(ns.clone(), j.clone()))
                {
                    if let Some(p) = pod_probe {
                        let slot = job_probes
                            .entry((ns.clone(), j))
                            .or_insert_with(|| p.clone());
                        if p.previous && !slot.previous {
                            *slot = p;
                        }
                    }
                    continue;
                }
                // One named pod — its title already says which, so the tally
                // would only restate it.
                let (severity, msg, _) = agg.primary().expect("agg.any() checked");
                let target = pod
                    .spec
                    .as_ref()
                    .and_then(|s| s.node_name.clone())
                    .map_or(Target::WorkloadList, Target::Node);
                concerns.push(Concern {
                    cluster: ClusterId::Hot,
                    severity,
                    title: format!("pod {ns}/{name} — {msg}"),
                    detail: reason.clone(),
                    target,
                    probe: pod_probe,
                    key: format!("b:{ns}/{name}"),
                });
            }
        }
    }

    // Attach each Job concern's representative failed pod (so `L` reaches it).
    for (i, key) in job_concern_idx {
        if let Some(p) = job_probes.remove(&key) {
            concerns[i].probe = Some(p);
        }
    }

    // --- Workload rows: merge pod aggregates with rollout/replica state ----
    let mut covered_workloads: HashSet<(String, String)> = HashSet::new();
    for row in workloads {
        let agg = by_workload.remove(&row.r);
        let gap = row.ready < row.desired;
        let stalled = row.status == RolloutStatus::Stalled;
        let pod_issue = agg.as_ref().and_then(Agg::primary);
        if !gap && !stalled && pod_issue.is_none() {
            continue;
        }
        let (severity, headline) = if let Some((sev, label, n)) = pod_issue {
            // Aggregated across the workload's pods, so the count is the point.
            (
                if stalled { Severity::Critical } else { sev },
                tally(label, n),
            )
        } else if stalled {
            (Severity::Critical, "rollout stalled".into())
        } else {
            (
                Severity::Warning,
                format!("{}/{} ready", row.ready, row.desired),
            )
        };
        let mut detail = format!(
            "{}/{} ready · rollout {}",
            row.ready, row.desired, row.status
        );
        if !row.note.is_empty() {
            detail.push_str(&format!(" ({})", row.note));
        }
        if let Some(a) = &agg
            && let Some(c) = pool_confinement(&a.pools, a.placed, a.unplaced, fleet_pools)
        {
            detail.push_str(&format!(" · {c}"));
        }
        covered_workloads.insert((row.r.namespace.clone(), row.r.name.clone()));
        concerns.push(Concern {
            cluster: ClusterId::Hot,
            severity,
            title: format!("{} — {headline}", row.r),
            detail,
            target: Target::Workload(row.r.clone()),
            // A pod-level issue carries its representative pod; a pure replica
            // gap / stalled rollout has no single pod to tail.
            probe: agg.and_then(|a| a.probe),
            key: format!("w:{}/{}/{}", row.r.kind, row.r.namespace, row.r.name),
        });
    }
    // Aggregates whose workload row vanished (e.g. workload deleted while
    // pods linger) still deserve a line.
    for (r, agg) in by_workload {
        if let Some((severity, label, n)) = agg.primary() {
            let msg = tally(label, n);
            covered_workloads.insert((r.namespace.clone(), r.name.clone()));
            concerns.push(Concern {
                cluster: ClusterId::Hot,
                severity,
                title: format!("{r} — {msg}"),
                detail: pool_confinement(&agg.pools, agg.placed, agg.unplaced, fleet_pools)
                    .unwrap_or_default(),
                probe: agg.probe,
                key: format!("w:{}/{}/{}", r.kind, r.namespace, r.name),
                target: Target::Workload(r),
            });
        }
    }

    // --- Nodes --------------------------------------------------------------
    let mut covered_nodes: HashSet<String> = HashSet::new();
    for zone in &map.zones {
        for tile in &zone.nodes {
            let (severity, headline) = if !tile.ready {
                (Severity::Critical, "NotReady".to_string())
            } else if !tile.abnormal.is_empty() {
                (
                    Severity::Warning,
                    format!("{} pressure", tile.abnormal.join("/")),
                )
            // An UNKNOWN ratio is not pressure: a node reporting no allocatable
            // gives `None`, and the old fabricated 0.0 made it silently pass
            // this test as if it were idle.
            } else if tile.cpu_ratio.is_some_and(|r| r >= PRESSURE_HIGH)
                || tile.mem_ratio.is_some_and(|r| r >= PRESSURE_HIGH)
            {
                (
                    Severity::Warning,
                    format!(
                        "requests cpu {} mem {}",
                        pct_or_unknown(tile.cpu_ratio),
                        pct_or_unknown(tile.mem_ratio)
                    ),
                )
            } else if tile
                .saturation
                .pod_ratio()
                .is_some_and(|r| r >= SAT_PODS_HIGH)
            {
                // Pod-slot exhaustion — the silent scheduling failure cpu/mem
                // can't show (a node at max-pods refuses new pods regardless).
                (
                    Severity::Warning,
                    format!(
                        "{} (near max)",
                        tile.saturation.pod_label().unwrap_or("pods")
                    ),
                )
            } else if tile.cpu_ratio.is_none() && tile.mem_ratio.is_none() {
                // The node publishes no allocatable, so nothing ratio-derived
                // can be said about it. Info, not Warning: it is usually a node
                // mid-registration, and the map already hatches it.
                (
                    Severity::Info,
                    "capacity not reported — load unknown".to_string(),
                )
            } else if tile.cordoned {
                (Severity::Info, "cordoned".to_string())
            } else {
                continue;
            };
            covered_nodes.insert(tile.name.clone());
            let mut detail = format!(
                "zone {} · {} pods · cpu {} mem {}",
                tile.zone,
                tile.pods.len(),
                pct_or_unknown(tile.cpu_ratio),
                pct_or_unknown(tile.mem_ratio)
            );
            // A cordoned node is one someone is taking out of service, so what a
            // disruption budget would refuse is the next thing they need. See
            // `pdb::drain_note` for why this enriches rather than raising its own
            // concern, and why only when cordoned.
            if tile.cordoned
                && let Some(note) = drain
                    .node(&tile.name)
                    .and_then(crate::state::pdb::drain_note)
            {
                detail.push_str(" · ");
                detail.push_str(&note);
            }
            concerns.push(Concern {
                cluster: ClusterId::Hot,
                severity,
                title: format!("node {} — {headline}", tile.name),
                detail,
                target: Target::Node(tile.name.clone()),
                probe: None,
                key: format!("n:{}", tile.name),
            });
        }
    }

    // --- PVCs ----------------------------------------------------------------
    for pvc in world.pvcs.state() {
        let phase = pvc
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("");
        if phase != "Pending" && phase != "Lost" {
            continue;
        }
        let ns = pvc.metadata.namespace.clone().unwrap_or_default();
        if !filter.matches(&ns) {
            continue;
        }
        let name = pvc.metadata.name.clone().unwrap_or_default();
        let owner = pvc_owner(world, &idx, &ns, &name);
        let sc = pvc
            .spec
            .as_ref()
            .and_then(|s| s.storage_class_name.clone())
            .unwrap_or_else(|| "default".into());
        concerns.push(Concern {
            cluster: ClusterId::Hot,
            severity: Severity::Warning,
            title: format!("pvc {ns}/{name} — {phase}"),
            detail: format!("storageClass {sc}"),
            target: owner.map_or(Target::WorkloadList, Target::Workload),
            probe: None,
            key: format!("p:{ns}/{name}"),
        });
    }

    // --- Connectivity: routes that lead nowhere -----------------------------
    // Orphan Ingress: a backend Service that doesn't exist (a gate to nowhere).
    let svc_names: HashSet<(String, String)> = world
        .services
        .state()
        .iter()
        .filter_map(|s| Some((s.metadata.namespace.clone()?, s.metadata.name.clone()?)))
        .collect();
    for ing in world.ingresses.state() {
        let ns = ing.metadata.namespace.clone().unwrap_or_default();
        if !filter.matches(&ns) {
            continue;
        }
        let name = ing.metadata.name.clone().unwrap_or_default();
        let mut missing: Vec<String> = ingress_backends(&ing)
            .into_iter()
            .filter(|b| !svc_names.contains(&(ns.clone(), b.clone())))
            .collect();
        if missing.is_empty() {
            continue;
        }
        missing.sort();
        concerns.push(Concern {
            cluster: ClusterId::Hot,
            severity: Severity::Warning,
            title: format!(
                "ingress {ns}/{name} — backend {} has no Service",
                missing.join(", ")
            ),
            detail: "route points at a missing Service".into(),
            target: Target::WorkloadList,
            probe: None,
            key: format!("i:{ns}/{name}"),
        });
    }
    // Harbor with no city: a Service whose selector matches no pod (no
    // endpoints). Info — it can be transient mid-rollout; headless/external
    // Services (no selector) are skipped.
    for svc in world.services.state() {
        let ns = svc.metadata.namespace.clone().unwrap_or_default();
        if !filter.matches(&ns) {
            continue;
        }
        let name = svc.metadata.name.clone().unwrap_or_default();
        let Some(sel) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
            continue;
        };
        if sel.is_empty() {
            continue;
        }
        let has_endpoint = pods.iter().any(|p| {
            p.metadata.namespace.as_deref() == Some(ns.as_str())
                && p.metadata
                    .labels
                    .as_ref()
                    .is_some_and(|l| sel.iter().all(|(k, v)| l.get(k) == Some(v)))
        });
        if has_endpoint {
            continue;
        }
        concerns.push(Concern {
            cluster: ClusterId::Hot,
            severity: Severity::Info,
            title: format!("service {ns}/{name} — selects no pods"),
            detail: "harbor with no city".into(),
            target: Target::WorkloadList,
            probe: None,
            key: format!("s:{ns}/{name}"),
        });
    }

    // --- Recent Warning events not already covered above ---------------------
    let now = jiff::Timestamp::now();
    let mut event_groups: BTreeMap<(String, String, String), (u32, String)> = BTreeMap::new();
    for ev in world.recent_events() {
        if !ev.warning {
            continue;
        }
        // Keep cluster-scoped Node events; filter the rest by namespace.
        if ev.kind != "Node" && !filter.matches(&ev.namespace) {
            continue;
        }
        let stale = ev
            .when
            .as_ref()
            .is_none_or(|t| now.duration_since(t.0).as_secs() > EVENT_WINDOW_MIN * 60);
        if stale {
            continue;
        }
        if ev.kind == "Node" && covered_nodes.contains(&ev.name) {
            continue;
        }
        if ev.kind == "Job" && covered_jobs.contains(&(ev.namespace.clone(), ev.name.clone())) {
            continue;
        }
        if covered_workloads.contains(&(ev.namespace.clone(), ev.name.clone())) {
            continue;
        }
        if ev.kind == "Pod" {
            // Skip if the pod's workload already has a concern.
            let owned = pods.iter().any(|p| {
                p.metadata.name.as_deref() == Some(&ev.name)
                    && p.metadata.namespace.as_deref() == Some(&ev.namespace)
                    && idx.workload_of(p).is_some_and(|r| {
                        covered_workloads.contains(&(r.namespace.clone(), r.name.clone()))
                    })
            });
            if owned {
                continue;
            }
        }
        let entry = event_groups
            .entry((ev.kind.clone(), ev.namespace.clone(), ev.name.clone()))
            .or_insert((0, ev.reason.clone()));
        entry.0 += ev.count.max(1) as u32;
        entry.1 = ev.reason.clone();
    }
    for ((kind, ns, name), (count, reason)) in event_groups.into_iter().take(20) {
        let target = event_target(world, &idx, &kind, &ns, &name);
        let place = if ns.is_empty() {
            name.clone()
        } else {
            format!("{ns}/{name}")
        };
        concerns.push(Concern {
            cluster: ClusterId::Hot,
            severity: Severity::Info,
            title: format!(
                "events: {reason} ×{count} on {} {place}",
                kind.to_lowercase()
            ),
            detail: String::new(),
            target,
            probe: None,
            key: format!("e:{kind}/{ns}/{name}"),
        });
    }

    concerns.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.key.cmp(&b.key)));
    concerns
}

/// The name of the Job that owns this pod, if any (Jobs aren't `WorkloadRef`s,
/// so `OwnerIndex` skips them — this is the lightweight lookup we need to fold
/// a failed Job's pods under its object-level concern).
fn job_owner(pod: &k8s_openapi::api::core::v1::Pod) -> Option<String> {
    pod.metadata
        .owner_references
        .as_ref()?
        .iter()
        .find(|o| o.kind == "Job")
        .map(|o| o.name.clone())
}

/// Find the StatefulSet a PVC belongs to (claim-template naming), or the
/// workload of any pod mounting it.
fn pvc_owner(world: &ObservedWorld, idx: &OwnerIndex, ns: &str, name: &str) -> Option<WorkloadRef> {
    for s in world.statefulsets.state() {
        if s.metadata.namespace.as_deref() != Some(ns) {
            continue;
        }
        let sts_name = s.metadata.name.as_deref().unwrap_or_default();
        for t in s
            .spec
            .as_ref()
            .and_then(|sp| sp.volume_claim_templates.as_deref())
            .unwrap_or(&[])
        {
            let tmpl = t.metadata.name.as_deref().unwrap_or_default();
            if name.starts_with(&format!("{tmpl}-{sts_name}-")) {
                return Some(WorkloadRef {
                    kind: super::model::WorkloadKind::StatefulSet,
                    namespace: ns.to_string(),
                    name: sts_name.to_string(),
                });
            }
        }
    }
    for p in world.pods.state() {
        if p.metadata.namespace.as_deref() != Some(ns) {
            continue;
        }
        let mounts = p
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_deref())
            .unwrap_or(&[])
            .iter()
            .any(|v| {
                v.persistent_volume_claim
                    .as_ref()
                    .is_some_and(|c| c.claim_name == name)
            });
        if mounts && let Some(r) = idx.workload_of(&p) {
            return Some(r);
        }
    }
    None
}

fn event_target(
    world: &ObservedWorld,
    idx: &OwnerIndex,
    kind: &str,
    ns: &str,
    name: &str,
) -> Target {
    match kind {
        "Node" => Target::Node(name.to_string()),
        "Deployment" => Target::Workload(WorkloadRef {
            kind: super::model::WorkloadKind::Deployment,
            namespace: ns.into(),
            name: name.into(),
        }),
        "StatefulSet" => Target::Workload(WorkloadRef {
            kind: super::model::WorkloadKind::StatefulSet,
            namespace: ns.into(),
            name: name.into(),
        }),
        "DaemonSet" => Target::Workload(WorkloadRef {
            kind: super::model::WorkloadKind::DaemonSet,
            namespace: ns.into(),
            name: name.into(),
        }),
        "Pod" => world
            .pods
            .state()
            .iter()
            .find(|p| {
                p.metadata.name.as_deref() == Some(name)
                    && p.metadata.namespace.as_deref() == Some(ns)
            })
            .and_then(|p| idx.workload_of(p))
            .map_or(Target::WorkloadList, Target::Workload),
        _ => Target::WorkloadList,
    }
}

/// Counts per severity, for the collapsed panel summary.
pub fn severity_counts(concerns: &[Concern]) -> HashMap<Severity, usize> {
    let mut out = HashMap::new();
    for c in concerns {
        *out.entry(c.severity).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    /// The four refusals, and the one thing it does say.
    ///
    /// Each `None` is a claim the data does not support, and each has bitten
    /// somewhere in this project already: the unpooled sentinel is `pool_line`'s
    /// rule, and the single-pool fleet is the DEGENERATE case the measuring
    /// instrument refuses (a fleet where every node is in one pool makes "all on
    /// pool X" true of everything, which is not information).
    #[test]
    fn pool_confinement_only_claims_a_real_grouping() {
        let one = |p: &str| BTreeSet::from([p.to_string()]);

        assert_eq!(
            pool_confinement(&one("sys"), 29, 0, 4).as_deref(),
            Some("all 29 on pool sys")
        );
        // Unplaced pods are excluded from the claim AND change its wording —
        // "all" has to range over something real. This is the churn fleet's
        // actual case: 29 agents on sys, 1 unschedulable with no node at all.
        assert_eq!(
            pool_confinement(&one("sys"), 29, 1, 4).as_deref(),
            Some("all 29 placed on pool sys")
        );

        // One pod is trivially in one pool.
        assert_eq!(pool_confinement(&one("sys"), 1, 0, 4), None);
        // Two pools is not confinement.
        let two = BTreeSet::from(["sys".to_string(), "mem".to_string()]);
        assert_eq!(pool_confinement(&two, 9, 0, 4), None);
        // An absence is not a pool.
        assert_eq!(
            pool_confinement(&one(crate::state::model::DEFAULT_POOL), 9, 0, 4),
            None
        );
        // A single-pool fleet: true, and vacuous.
        assert_eq!(pool_confinement(&one("sys"), 9, 0, 1), None);
        // Nothing placed at all.
        assert_eq!(pool_confinement(&BTreeSet::new(), 0, 9, 4), None);
    }

    /// End to end: the sentence that T2-pre found missing.
    ///
    /// A DaemonSet whose pods crash-loop on every node of ONE pool. The queue
    /// correctly aggregates them into one workload concern; before this, that
    /// concern said only how many, never where — and the map does not show it
    /// either, because the failures scatter across zone-wide ordinals.
    #[test]
    fn a_pool_confined_failure_says_so_in_the_concern() {
        use crate::state::fixtures as fx;
        let (world, mut s) = fx::world();
        for i in 0..4 {
            s.node(fx::node_in_pool(
                fx::node(&format!("sys{i}"), Some("z-a")),
                "sys",
            ));
            s.node(fx::node_in_pool(
                fx::node(&format!("mem{i}"), Some("z-b")),
                "mem",
            ));
        }
        s.daemonset(fx::daemonset("infra", "agent", 8, 4));
        // Crash-looping on every sys node; healthy on the mem nodes.
        for i in 0..4 {
            s.pod(fx::pod_owned(
                fx::pod_waiting(
                    fx::pod("infra", &format!("agent-sys{i}"), Some(&format!("sys{i}"))),
                    "CrashLoopBackOff",
                ),
                "DaemonSet",
                "agent",
            ));
            s.pod(fx::pod_owned(
                fx::pod("infra", &format!("agent-mem{i}"), Some(&format!("mem{i}"))),
                "DaemonSet",
                "agent",
            ));
        }
        let map = crate::state::model::build_map(&world);
        let wl = crate::state::model::build_workloads(&world);
        let cs = build(
            &world,
            &map,
            &wl,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(&world),
        );
        let c = cs
            .iter()
            .find(|c| c.title.contains("agent"))
            .expect("the daemonset is flagged");
        assert!(
            c.detail.contains("all 4 on pool sys"),
            "the concern must name the pool the failures are confined to: {:?}",
            c.detail
        );

        // Now add an UNSCHEDULABLE pod of the same workload: no node, so no
        // pool. It must not be folded into the pool the others landed in — the
        // churn fleet's real case, where one agent cannot be placed at all.
        s.pod(fx::pod_owned(
            fx::pod_unschedulable(fx::pod("infra", "agent-nowhere", None)),
            "DaemonSet",
            "agent",
        ));
        let map = crate::state::model::build_map(&world);
        let wl = crate::state::model::build_workloads(&world);
        let cs = build(
            &world,
            &map,
            &wl,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(&world),
        );
        let c = cs
            .iter()
            .find(|c| c.title.contains("agent"))
            .expect("still flagged");
        assert!(
            c.detail.contains("all 4 placed on pool sys"),
            "an unplaced pod must not be claimed onto a pool: {:?}",
            c.detail
        );
        // And it stays ONE concern — naming the pool must not undo the
        // "city in trouble, not 40 pod alarms" aggregation.
        assert_eq!(
            cs.iter().filter(|c| c.title.contains("agent")).count(),
            1,
            "still one concern per workload"
        );
    }

    use super::*;
    use crate::state::fixtures as fx;
    use crate::state::model::{WorkloadKind, build_map, build_workloads};
    use crate::state::observed::ObservedWorld;

    fn concerns(world: &ObservedWorld) -> Vec<Concern> {
        let map = build_map(world);
        let rows = build_workloads(world);
        build(
            world,
            &map,
            &rows,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(world),
        )
    }

    #[test]
    fn pod_slot_exhaustion_fires_one_warning() {
        let (world, mut s) = fx::world();
        let mut n = fx::node("crowded", Some("z-a"));
        n.status.as_mut().unwrap().allocatable = Some(fx::quantities(&[
            ("cpu", "4"),
            ("memory", "8Gi"),
            ("pods", "10"),
        ]));
        s.node(n);
        // 10 running (1.0 ratio ≥ 0.95) + 2 terminal that must NOT count.
        for i in 0..10 {
            s.pod(fx::pod("demo", &format!("p{i}"), Some("crowded")));
        }
        s.pod(fx::pod_phase(
            fx::pod("demo", "done", Some("crowded")),
            "Succeeded",
        ));
        s.pod(fx::pod_phase(
            fx::pod("demo", "bad", Some("crowded")),
            "Failed",
        ));

        let cs = concerns(&world);
        let slot: Vec<_> = cs.iter().filter(|c| c.title.contains("near max")).collect();
        assert_eq!(
            slot.len(),
            1,
            "exactly one pod-slot concern (city in trouble)"
        );
        assert_eq!(slot[0].severity, Severity::Warning);
        assert!(slot[0].key.starts_with("n:crowded"));
        assert!(slot[0].title.contains("pods 10/10"));
        assert!(next_action(slot[0]).unwrap().contains("open the province"));
    }

    #[test]
    fn pod_slot_concern_absent_without_allocatable_pods() {
        let (world, mut s) = fx::world();
        s.node(fx::node("plain", Some("z-a"))); // fixture node has no "pods" key
        for i in 0..50 {
            s.pod(fx::pod("demo", &format!("p{i}"), Some("plain")));
        }
        let cs = concerns(&world);
        assert!(
            !cs.iter().any(|c| c.title.contains("near max")),
            "no fabricated pod-slot concern when allocatable[pods] is absent"
        );
    }

    #[test]
    fn cpu_high_outranks_pod_slot_exhaustion() {
        // A node that is BOTH cpu-bound and pod-bound surfaces the cpu headline
        // (the if/else order), not "near max" — and its saturation is still High.
        let (world, mut s) = fx::world();
        let mut n = fx::node("dual", Some("z-a"));
        n.status.as_mut().unwrap().allocatable = Some(fx::quantities(&[
            ("cpu", "1"),
            ("memory", "8Gi"),
            ("pods", "10"),
        ]));
        s.node(n);
        // 10 pods each requesting ~0.5 cpu → 5 cores requested vs 1 allocatable
        // (cpu_ratio ≫ 0.9) AND 10/10 pods (pod_ratio 1.0 ≥ 0.95).
        for i in 0..10 {
            s.pod(fx::pod_requests(
                fx::pod("demo", &format!("p{i}"), Some("dual")),
                "500m",
                "16Mi",
            ));
        }
        let cs = concerns(&world);
        let node_concerns: Vec<_> = cs.iter().filter(|c| c.key.starts_with("n:dual")).collect();
        assert_eq!(node_concerns.len(), 1, "one concern per node");
        assert!(
            node_concerns[0].title.contains("cpu"),
            "cpu-high outranks pod-slot: {}",
            node_concerns[0].title
        );
        assert!(!node_concerns[0].title.contains("near max"));
    }

    #[test]
    fn not_ready_outranks_pod_slot_exhaustion() {
        let (world, mut s) = fx::world();
        let mut n = fx::node_with_condition(fx::node("down", Some("z-a")), "Ready", "False");
        n.status.as_mut().unwrap().allocatable = Some(fx::quantities(&[
            ("cpu", "4"),
            ("memory", "8Gi"),
            ("pods", "10"),
        ]));
        s.node(n);
        for i in 0..10 {
            s.pod(fx::pod("demo", &format!("p{i}"), Some("down")));
        }
        let cs = concerns(&world);
        let node_concerns: Vec<_> = cs.iter().filter(|c| c.key.starts_with("n:down")).collect();
        assert_eq!(node_concerns.len(), 1, "one concern per node");
        assert_eq!(node_concerns[0].severity, Severity::Critical);
        assert!(node_concerns[0].title.contains("NotReady"));
    }

    #[test]
    fn next_action_keys_off_the_concern_kind() {
        let mk = |key: &str, probe: bool| Concern {
            severity: Severity::Warning,
            title: "x".into(),
            detail: "y".into(),
            target: Target::WorkloadList,
            probe: probe.then(|| LogProbe {
                namespace: "demo".into(),
                pod: "p".into(),
                previous: false,
            }),
            key: key.into(),
            cluster: ClusterId::Hot,
        };
        // A workload concern with a log-worthy pod leads with the logs verb.
        assert!(
            next_action(&mk("w:Deployment/demo/web", true))
                .unwrap()
                .contains("L: tail logs")
        );
        // Without a probe it still points at blast + open.
        assert!(
            next_action(&mk("w:Deployment/demo/web", false))
                .unwrap()
                .contains("B: blast")
        );
        // Type-specific runbook hints by key prefix.
        assert!(
            next_action(&mk("p:demo/data", false))
                .unwrap()
                .contains("StorageClass")
        );
        assert!(
            next_action(&mk("i:demo/web", false))
                .unwrap()
                .contains("backend")
        );
        assert!(
            next_action(&mk("s:demo/web", false))
                .unwrap()
                .contains("selector")
        );
        assert!(
            next_action(&mk("slo:demo/web", false))
                .unwrap()
                .contains("budget")
        );
        assert!(
            next_action(&mk("pair:drift", false))
                .unwrap()
                .contains("WARM")
        );
        // A bare concern with nothing actionable returns None.
        assert!(next_action(&mk("e:Event/demo/x", false)).is_some()); // events → open target
    }

    fn concerns_filtered(world: &ObservedWorld, filter: &NamespaceFilter) -> Vec<Concern> {
        let map = build_map(world);
        let mut rows = build_workloads(world);
        rows.retain(|w| filter.matches(&w.r.namespace));
        build(
            world,
            &map,
            &rows,
            filter,
            &crate::state::pdb::drain_report(world),
        )
    }

    #[test]
    fn crashloop_pods_aggregate_into_one_workload_concern() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "crashy", 3, 1));
        s.replicaset(fx::replicaset("demo", "crashy-abc", "crashy"));
        for i in 0..2 {
            s.pod(fx::pod_owned(
                fx::pod_waiting(
                    fx::pod("demo", &format!("crashy-abc-{i}"), Some("n1")),
                    "CrashLoopBackOff",
                ),
                "ReplicaSet",
                "crashy-abc",
            ));
        }
        let cs = concerns(&world);
        let workload: Vec<&Concern> = cs.iter().filter(|c| c.key.starts_with("w:")).collect();
        assert_eq!(workload.len(), 1, "one aggregated concern, got {cs:?}");
        let c = workload[0];
        assert_eq!(c.severity, Severity::Critical);
        assert!(c.title.contains("deploy demo/crashy"), "{}", c.title);
        assert!(c.title.contains("CrashLoopBackOff ×2"), "{}", c.title);
        assert!(matches!(&c.target, Target::Workload(r) if r.name == "crashy"));
        // No per-pod entries for owned pods.
        assert!(cs.iter().all(|c| !c.key.starts_with("b:")));
    }

    #[test]
    fn crashloop_concern_carries_a_previous_log_probe() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "crashy", 1, 1));
        s.replicaset(fx::replicaset("demo", "crashy-abc", "crashy"));
        s.pod(fx::pod_owned(
            fx::pod_waiting(
                fx::pod("demo", "crashy-abc-0", Some("n1")),
                "CrashLoopBackOff",
            ),
            "ReplicaSet",
            "crashy-abc",
        ));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key.starts_with("w:"))
            .expect("workload concern");
        // The `L` jump lands on the offending pod, on its previous container.
        let p = c.probe.as_ref().expect("crash concern carries a log probe");
        assert_eq!(p.namespace, "demo");
        assert_eq!(p.pod, "crashy-abc-0");
        assert!(p.previous, "crash-loop probe opens the previous container");
    }

    #[test]
    fn replica_gap_only_concern_has_no_log_probe() {
        // A healthy-but-understrength workload (no failing pod) has nothing to
        // tail — the probe is None, so `L` is a no-op there.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 3, 1));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key.starts_with("w:"))
            .expect("replica-gap concern");
        assert!(c.probe.is_none(), "a pure replica gap carries no probe");
    }

    #[test]
    fn bare_pod_concern_targets_its_node() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod_waiting(
            fx::pod("demo", "loner", Some("n1")),
            "ImagePullBackOff",
        ));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "b:demo/loner")
            .expect("bare pod concern");
        assert_eq!(c.severity, Severity::Critical);
        assert_eq!(c.target, Target::Node("n1".into()));
    }

    #[test]
    fn pending_pvc_targets_owning_statefulset() {
        let (world, mut s) = fx::world();
        let mut sts = fx::statefulset("demo", "db", 1, 1);
        sts.spec.as_mut().unwrap().volume_claim_templates =
            Some(vec![k8s_openapi::api::core::v1::PersistentVolumeClaim {
                metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                    name: Some("data".into()),
                    ..Default::default()
                },
                ..Default::default()
            }]);
        s.statefulset(sts);
        s.pvc(fx::pvc("demo", "data-db-0", "Pending"));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "p:demo/data-db-0")
            .expect("pvc concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(
            matches!(&c.target, Target::Workload(r) if r.kind == WorkloadKind::StatefulSet && r.name == "db"),
            "{:?}",
            c.target
        );
    }

    #[test]
    fn flapping_daemonset_pod_is_a_warning() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.daemonset(fx::daemonset("demo", "agent", 1, 1));
        s.pod(fx::pod_owned(
            fx::pod_restarting(fx::pod("demo", "agent-x1", Some("n1")), 7),
            "DaemonSet",
            "agent",
        ));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key.contains("agent"))
            .expect("flapping concern");
        assert_eq!(c.severity, Severity::Warning);
        // The UNIT, not just the count: every Agg counter counts PODS, and `×1`
        // alone reads as one restart when it means one pod past a 5-restart
        // threshold. Two lines below in the same queue, `events: X ×522` counts
        // occurrences — same suffix, different unit.
        assert!(
            c.title.contains("restarting repeatedly ×1 pod"),
            "the count must carry its unit: {}",
            c.title
        );
        assert!(matches!(&c.target, Target::Workload(r) if r.kind == WorkloadKind::DaemonSet));
    }

    /// A bare pod says the label; a workload says the label AND how many pods.
    ///
    /// Same `Agg::primary`, two renderings, because only the caller knows
    /// whether the count carries information: a bare-pod concern is titled
    /// `pod ns/name`, so "×1 pod" restates its own subject, while a workload
    /// concern aggregates and the number is the whole point. Asserted in both
    /// directions — a single rendering would be wrong at one of the two sites.
    #[test]
    fn the_tally_is_for_aggregates_not_for_one_named_pod() {
        // A bare pod (no owner) past the restart threshold.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let mut p = fx::pod("demo", "lonely", Some("n1"));
        p.status
            .as_mut()
            .unwrap()
            .container_statuses
            .as_mut()
            .unwrap()[0]
            .restart_count = 9;
        s.pod(p);
        let map = build_map(&world);
        let rows = crate::state::model::build_workloads(&world);
        let cs = build(
            &world,
            &map,
            &rows,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(&world),
        );
        let bare = cs
            .iter()
            .find(|c| c.title.starts_with("pod demo/lonely"))
            .expect("a bare-pod concern");
        assert!(
            bare.title.ends_with("restarting repeatedly"),
            "a bare pod restates its own subject with a tally: {}",
            bare.title
        );
    }

    #[test]
    fn replica_gap_is_warning_and_stall_is_critical() {
        let (world, mut s) = fx::world();
        let mut gap = fx::deployment("demo", "gappy", 3, 1);
        gap.status.as_mut().unwrap().updated_replicas = Some(3);
        s.deployment(gap);
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key.contains("gappy"))
            .expect("gap concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(c.title.contains("1/3 ready"), "{}", c.title);
    }

    #[test]
    fn node_states_and_global_ordering() {
        let (world, mut s) = fx::world();
        s.node(fx::node_with_condition(
            fx::node("n-bad", Some("z-a")),
            "Ready",
            "False",
        ));
        s.node(fx::cordoned(fx::node("n-cord", Some("z-a"))));
        s.node(fx::node("n-ok", Some("z-a")));
        let cs = concerns(&world);
        assert_eq!(cs.len(), 2);
        // Critical (NotReady) sorts before Info (cordoned).
        assert_eq!(cs[0].severity, Severity::Critical);
        assert!(cs[0].title.contains("n-bad"));
        assert!(cs[0].title.contains("NotReady"));
        assert_eq!(cs[1].severity, Severity::Info);
        assert!(cs[1].title.contains("cordoned"));
        // Healthy node contributes nothing.
        assert!(!cs.iter().any(|c| c.title.contains("n-ok")));
    }

    /// The `pool_confinement` shape: a cordoned node's concern names what would
    /// refuse the drain, rather than a concern of its own (which a permanently
    /// tight budget would make squat the queue forever).
    #[test]
    fn a_cordoned_node_names_the_budget_that_would_block_its_drain() {
        use std::collections::BTreeMap;
        let (world, mut s) = fx::world();
        s.node(fx::cordoned(fx::node("n-cord", Some("z-a"))));
        s.node(fx::node("n-open", Some("z-a")));
        for (name, node) in [("web-1", "n-cord"), ("web-2", "n-open")] {
            let mut p = fx::pod("demo", name, Some(node));
            p.metadata.labels = Some(BTreeMap::from([("app".into(), "web".into())]));
            s.pod(p);
        }
        s.pdb(fx::pdb("demo", "web-strict", &[("app", "web")], 0));

        let map = build_map(&world);
        let rows = crate::state::model::build_workloads(&world);
        let cs = build(
            &world,
            &map,
            &rows,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(&world),
        );
        let cord = cs
            .iter()
            .find(|c| c.title.contains("n-cord"))
            .expect("the cordoned node has a concern");
        assert!(
            cord.detail.contains("demo/web-strict"),
            "the concern must name the budget: {}",
            cord.detail
        );
        // The uncordoned node is equally blocked, and says nothing — a standing
        // fact, not something needing orders. The panel reports it instead.
        assert!(
            !cs.iter().any(|c| c.title.contains("n-open")),
            "a blocked-but-untouched node must not squat the queue"
        );
    }

    /// ...and a cordoned node nothing would refuse spends no words on it.
    ///
    /// The `None` arm of `drain_note` exists for exactly this: a caveat that
    /// carries no information is noise, and the queue is the surface where noise
    /// costs the most. Without this the arm can be deleted and every other test
    /// still passes.
    #[test]
    fn a_cordoned_node_with_nothing_blocking_it_says_nothing_about_draining() {
        let (world, mut s) = fx::world();
        s.node(fx::cordoned(fx::node("n-cord", Some("z-a"))));
        s.pod(fx::pod("demo", "p1", Some("n-cord")));
        let map = build_map(&world);
        let rows = crate::state::model::build_workloads(&world);
        let cs = build(
            &world,
            &map,
            &rows,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(&world),
        );
        let cord = cs.iter().find(|c| c.title.contains("n-cord")).unwrap();
        assert!(
            !cord.detail.contains("drain"),
            "nothing refuses, so nothing to say: {}",
            cord.detail
        );
    }

    /// An unread budget set is not silence: a cordon you cannot cost is worth
    /// saying so about.
    #[test]
    fn a_cordoned_node_says_when_the_budgets_were_not_read() {
        let (world, mut s) = fx::world();
        s.node(fx::cordoned(fx::node("n-cord", Some("z-a"))));
        s.pdbs_unread();
        let map = build_map(&world);
        let rows = crate::state::model::build_workloads(&world);
        let cs = build(
            &world,
            &map,
            &rows,
            &NamespaceFilter::All,
            &crate::state::pdb::drain_report(&world),
        );
        let cord = cs.iter().find(|c| c.title.contains("n-cord")).unwrap();
        assert!(cord.detail.contains("not read"), "{}", cord.detail);
    }

    #[test]
    fn failed_job_surfaces_as_its_own_concern() {
        let (world, mut s) = fx::world();
        // 3 pod failures, not yet complete (0 succeeded of 1).
        s.job(fx::job("demo", "migrate", 1, 0, 0, 3));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "j:demo/migrate")
            .expect("job concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(c.title.contains("3 pod failures"), "{}", c.title);
        assert_eq!(c.target, Target::WorkloadList);
    }

    #[test]
    fn completed_job_is_quiet() {
        let (world, mut s) = fx::world();
        // succeeded == completions → nothing to surface, even if it had retries.
        s.job(fx::job("demo", "done", 1, 1, 0, 2));
        let cs = concerns(&world);
        assert!(!cs.iter().any(|c| c.key.starts_with("j:")), "{cs:?}");
    }

    #[test]
    fn failed_jobs_pods_fold_under_the_job_concern() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.job(fx::job("demo", "doomed", 1, 0, 0, 2)); // 2 failures, not complete
        for i in 0..2 {
            s.pod(fx::pod_owned(
                fx::pod_waiting(
                    fx::pod("demo", &format!("doomed-{i}"), Some("n1")),
                    "CrashLoopBackOff",
                ),
                "Job",
                "doomed",
            ));
        }
        let cs = concerns(&world);
        // One Job concern — no per-pod (b:) spam for the Job's own pods.
        let job: Vec<&Concern> = cs.iter().filter(|c| c.key.starts_with("j:")).collect();
        assert_eq!(job.len(), 1, "{cs:?}");
        assert!(!cs.iter().any(|c| c.key.starts_with("b:")), "{cs:?}");
        // …but the folded pod lends the Job concern a log probe, so `L` can
        // reach the failed batch pod's last words.
        let p = job[0]
            .probe
            .as_ref()
            .expect("job concern carries a log probe from its failed pod");
        assert!(p.pod.starts_with("doomed-"));
        assert!(
            p.previous,
            "a crash-looping job pod opens its previous container"
        );
    }

    #[test]
    fn orphan_ingress_points_at_missing_service() {
        let (world, mut s) = fx::world();
        // Ingress backends "ghost-svc", which doesn't exist.
        s.ingress(fx::ingress(
            "demo",
            "web-ing",
            "web.example.com",
            "ghost-svc",
        ));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "i:demo/web-ing")
            .expect("orphan ingress concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(c.title.contains("ghost-svc"), "{}", c.title);
    }

    #[test]
    fn ingress_with_existing_backend_is_quiet() {
        let (world, mut s) = fx::world();
        s.service(fx::service("demo", "web", &[("app", "web")]));
        s.ingress(fx::ingress("demo", "web-ing", "web.example.com", "web"));
        let cs = concerns(&world);
        assert!(!cs.iter().any(|c| c.key.starts_with("i:")), "{cs:?}");
    }

    #[test]
    fn service_selecting_no_pods_is_info() {
        let (world, mut s) = fx::world();
        s.service(fx::service("demo", "lonely", &[("app", "nobody")]));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "s:demo/lonely")
            .expect("orphan harbor concern");
        assert_eq!(c.severity, Severity::Info);
    }

    #[test]
    fn namespace_filter_scopes_concerns_but_keeps_nodes() {
        let (world, mut s) = fx::world();
        // A cluster-scoped concern: a NotReady node.
        s.node(fx::node_with_condition(
            fx::node("n-bad", Some("z-a")),
            "Ready",
            "False",
        ));
        // A crashing workload in `demo`.
        s.deployment(fx::deployment("demo", "crashy", 1, 0));
        s.replicaset(fx::replicaset("demo", "crashy-abc", "crashy"));
        s.pod(fx::pod_owned(
            fx::pod_waiting(
                fx::pod("demo", "crashy-abc-0", Some("n-bad")),
                "CrashLoopBackOff",
            ),
            "ReplicaSet",
            "crashy-abc",
        ));
        // A crashing workload in `other`.
        s.deployment(fx::deployment("other", "broken", 1, 0));
        s.replicaset(fx::replicaset("other", "broken-xyz", "broken"));
        s.pod(fx::pod_owned(
            fx::pod_waiting(
                fx::pod("other", "broken-xyz-0", Some("n-bad")),
                "CrashLoopBackOff",
            ),
            "ReplicaSet",
            "broken-xyz",
        ));

        let cs = concerns_filtered(&world, &NamespaceFilter::only("demo"));
        assert!(
            cs.iter().any(|c| c.title.contains("demo/crashy")),
            "demo concern missing: {cs:?}"
        );
        assert!(
            !cs.iter().any(|c| c.title.contains("other/broken")),
            "other-namespace concern leaked: {cs:?}"
        );
        // Cluster-scoped node concern stays regardless of the filter.
        assert!(
            cs.iter().any(|c| c.title.contains("node n-bad")),
            "node concern dropped: {cs:?}"
        );
    }

    #[test]
    fn severity_counts_tally() {
        let (world, mut s) = fx::world();
        s.node(fx::node_with_condition(
            fx::node("n-bad", None),
            "Ready",
            "False",
        ));
        s.node(fx::cordoned(fx::node("n-cord", None)));
        let cs = concerns(&world);
        let counts = severity_counts(&cs);
        assert_eq!(counts.get(&Severity::Critical), Some(&1));
        assert_eq!(counts.get(&Severity::Info), Some(&1));
    }
    /// A node that reports no capacity raises its own Info concern, and every
    /// ratio it prints reads "unknown" — never "0%", which is the whole point.
    #[test]
    fn a_node_reporting_no_capacity_is_surfaced_and_never_says_zero_percent() {
        let (world, mut s) = fx::world();
        let mut bare = fx::node("bare", Some("z-a"));
        bare.status.as_mut().unwrap().allocatable = None;
        s.node(bare);
        let models = crate::state::model::Models::build(&world);
        let c = models
            .attention
            .iter()
            .find(|c| c.key == "n:bare")
            .expect("the unmeasurable node raises a concern");
        assert_eq!(
            c.severity,
            Severity::Info,
            "not a failure, but worth seeing"
        );
        assert!(
            c.title.contains("capacity not reported"),
            "says why: {}",
            c.title
        );
        assert!(
            c.detail.contains("cpu unknown") && c.detail.contains("mem unknown"),
            "unknown, never 0%: {}",
            c.detail
        );
        assert!(
            !c.detail.contains("0%"),
            "a fabricated zero leaked: {}",
            c.detail
        );
    }

    #[test]
    fn pct_or_unknown_never_renders_a_missing_ratio_as_zero() {
        assert_eq!(
            pct_or_unknown(Some(0.0)),
            "0%",
            "a real zero is a real zero"
        );
        assert_eq!(pct_or_unknown(Some(0.955)), "96%");
        assert_eq!(pct_or_unknown(None), "unknown");
    }
}
