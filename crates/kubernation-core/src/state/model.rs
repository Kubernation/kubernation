//! Pure derivations from the observed world into render-ready view models.
//! Everything here is a function of `ObservedWorld` snapshots — no I/O, no
//! mutation — which is what makes the interesting logic unit-testable.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::{Container, Node, Pod, PodTemplateSpec};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};

use super::attention::{self, Concern, Severity, Target};
use super::filter::NamespaceFilter;
use super::observed::ObservedWorld;
use super::qos::QosClass;
use super::saturation::{self, NodeSaturation};
use super::world::{
    BatchEntry, BatchKind, CoastKind, ExposureEntry, StorageEntry, WorldModel, build_world,
};
use crate::k8s::metrics::NodeUsage;
use crate::k8s::quantity;
use crate::util::fnv1a64;

pub const ZONE_LABEL: &str = "topology.kubernetes.io/zone";
pub const ZONE_LABEL_LEGACY: &str = "failure-domain.beta.kubernetes.io/zone";
pub const UNZONED: &str = "unzoned";

/// Request-pressure buckets shared by tiles, gauges, and attention.
pub const PRESSURE_ELEVATED: f64 = 0.7;
pub const PRESSURE_HIGH: f64 = 0.9;

// ---------------------------------------------------------------------------
// Pods

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodState {
    Ok,
    Starting,
    Pending,
    Terminating,
    Failing,
    Succeeded,
}

/// Should a freshly-opened log view default to the *previous* container? Yes for
/// a crash-looping / repeatedly-restarting pod — its current container is in
/// backoff (its live tail is empty), so the useful "last words" are in the
/// previous instance (`kubectl logs --previous`). Everything else wants the live
/// tail. Mirrors the attention "flapping" threshold (5 restarts).
pub fn prefer_previous(state: PodState, reason: &str, restarts: i32) -> bool {
    reason.contains("CrashLoopBackOff") || (state == PodState::Failing && restarts >= 5)
}

/// Classify a pod and give the short reason shown in tables
/// ("CrashLoopBackOff", "ContainerCreating", "Running", ...).
pub fn pod_state(pod: &Pod) -> (PodState, String) {
    let status = pod.status.as_ref();
    let phase = status.and_then(|s| s.phase.as_deref()).unwrap_or("Unknown");
    if pod.metadata.deletion_timestamp.is_some() {
        return (PodState::Terminating, "Terminating".into());
    }
    match phase {
        "Succeeded" => return (PodState::Succeeded, "Succeeded".into()),
        "Failed" => {
            let reason = status
                .and_then(|s| s.reason.clone())
                .unwrap_or_else(|| "Failed".into());
            return (PodState::Failing, reason);
        }
        _ => {}
    }

    let container_statuses = status.and_then(|s| s.container_statuses.as_ref());
    if let Some(cs) = container_statuses {
        for c in cs {
            if let Some(w) = c.state.as_ref().and_then(|s| s.waiting.as_ref())
                && let Some(r) = w.reason.as_deref()
                && matches!(
                    r,
                    "CrashLoopBackOff"
                        | "ImagePullBackOff"
                        | "ErrImagePull"
                        | "InvalidImageName"
                        | "CreateContainerConfigError"
                        | "CreateContainerError"
                        | "RunContainerError"
                )
            {
                return (PodState::Failing, r.to_string());
            }
        }
    }

    match phase {
        "Running" => {
            let all_ready = container_statuses.is_none_or(|cs| cs.iter().all(|c| c.ready));
            if all_ready {
                (PodState::Ok, "Running".into())
            } else {
                (PodState::Starting, "NotReady".into())
            }
        }
        "Pending" => {
            if let Some(conds) = status.and_then(|s| s.conditions.as_ref())
                && conds.iter().any(|c| {
                    c.type_ == "PodScheduled"
                        && c.status == "False"
                        && c.reason.as_deref() == Some("Unschedulable")
                })
            {
                return (PodState::Pending, "Unschedulable".into());
            }
            if let Some(cs) = container_statuses {
                for c in cs {
                    if let Some(w) = c.state.as_ref().and_then(|s| s.waiting.as_ref())
                        && let Some(r) = w.reason.as_deref()
                    {
                        return (PodState::Pending, r.to_string());
                    }
                }
            }
            (PodState::Pending, "Pending".into())
        }
        other => (PodState::Pending, other.to_string()),
    }
}

pub fn pod_restarts(pod: &Pod) -> i32 {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

/// True when the pod's last container exit was an OOM kill.
pub fn pod_oom_killed(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .is_some_and(|cs| {
            cs.iter().any(|c| {
                c.last_state
                    .as_ref()
                    .and_then(|s| s.terminated.as_ref())
                    .and_then(|t| t.reason.as_deref())
                    == Some("OOMKilled")
            })
        })
}

// ---------------------------------------------------------------------------
// Workload identity & pod ownership

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum WorkloadKind {
    #[default]
    Deployment,
    StatefulSet,
    DaemonSet,
}

impl fmt::Display for WorkloadKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WorkloadKind::Deployment => "deploy",
            WorkloadKind::StatefulSet => "sts",
            WorkloadKind::DaemonSet => "ds",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkloadRef {
    pub kind: WorkloadKind,
    pub namespace: String,
    pub name: String,
}

impl fmt::Display for WorkloadRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}/{}", self.kind, self.namespace, self.name)
    }
}

/// Resolves pod → owning workload. ReplicaSet hops to its Deployment via an
/// index built once per rebuild, so resolution is O(1) per pod.
#[derive(Default)]
pub struct OwnerIndex {
    rs_to_deploy: HashMap<(String, String), String>,
}

impl OwnerIndex {
    pub fn build(world: &ObservedWorld) -> Self {
        let mut rs_to_deploy = HashMap::new();
        for rs in world.replicasets.state() {
            let ns = rs.metadata.namespace.clone().unwrap_or_default();
            let name = rs.metadata.name.clone().unwrap_or_default();
            if let Some(owner) = controller_owner(rs.metadata.owner_references.as_deref())
                && owner.0 == "Deployment"
            {
                rs_to_deploy.insert((ns, name), owner.1.to_string());
            }
        }
        Self { rs_to_deploy }
    }

    pub fn workload_of(&self, pod: &Pod) -> Option<WorkloadRef> {
        let ns = pod.metadata.namespace.clone().unwrap_or_default();
        let (kind, name) = controller_owner(pod.metadata.owner_references.as_deref())?;
        match kind {
            "ReplicaSet" => {
                let deploy = self.rs_to_deploy.get(&(ns.clone(), name.to_string()))?;
                Some(WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: ns,
                    name: deploy.clone(),
                })
            }
            "StatefulSet" => Some(WorkloadRef {
                kind: WorkloadKind::StatefulSet,
                namespace: ns,
                name: name.to_string(),
            }),
            "DaemonSet" => Some(WorkloadRef {
                kind: WorkloadKind::DaemonSet,
                namespace: ns,
                name: name.to_string(),
            }),
            _ => None,
        }
    }
}

pub(crate) fn controller_owner(
    refs: Option<&[k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference]>,
) -> Option<(&str, &str)> {
    refs?
        .iter()
        .find(|o| o.controller == Some(true))
        .map(|o| (o.kind.as_str(), o.name.as_str()))
}

// ---------------------------------------------------------------------------
// Map model

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeHealth {
    Healthy,
    Cordoned,
    Pressure,
    NotReady,
}

/// One pod's cpu (cores) / memory (bytes) figures. Same canonical units as
/// [`crate::k8s::metrics::NodeUsage`], so a requests value and a usage value are
/// directly comparable — which is the whole point of carrying both.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PodResources {
    pub cpu: f64,
    pub mem: f64,
}

#[derive(Debug, Clone)]
pub struct PodGlyph {
    pub namespace: String,
    pub name: String,
    pub state: PodState,
    /// Controller workload (through the RS chain), for city placement.
    pub owner: Option<WorkloadRef>,

    /// LITERAL declared requests — filled by [`sum_pod_requests`], **never**
    /// [`sum_pod_reserved`]. The reserved variant defaults request:=limit, which
    /// is right for cost (it is what the scheduler holds) and wrong here, because
    /// this value is one half of a requests-versus-usage comparison and one input
    /// to over/under judgement. Substituting it would silently move both.
    ///
    /// **Do not sum these and expect `NodeTile::cpu_request_ratio × allocatable`.**
    /// The glyph list is a *census* — it includes terminal (Succeeded/Failed)
    /// pods, which is deliberate and long-standing (the map draws `◆` for a
    /// completed pod) — while the node's request ratio is *scheduling load* and
    /// excludes them via `pod_terminal`, because a terminal pod reserves nothing.
    /// Both numbers are right; they answer different questions. Anything
    /// aggregating occupancy from glyphs must filter with that same shared
    /// authority rather than reimplement the test — note `PodState::Failing`
    /// cannot stand in for it, since it covers both a terminal `Failed` pod and a
    /// live CrashLoopBackOff one.
    pub requests: PodResources,
    /// The ceiling — filled by [`sum_pod_limits`]. Zero means *unset*, which is
    /// meaningful (no ceiling, so no throttle or OOM-kill boundary) rather than
    /// missing. This is the throttle/OOM input, not a request.
    ///
    /// **A nonzero total does not mean the POD is capped.** Limits are enforced
    /// per container: an unset limit contributes 0 to this sum, so a pod whose
    /// app container caps 1Gi beside an uncapped sidecar reports `mem: 1Gi` while
    /// only the app container is actually bounded. A pod-level cgroup ceiling
    /// exists solely when *every* container declares one. So this total is safe
    /// to compare against [`Self::usage`] as "headroom against declared limits",
    /// and unsafe to read as "the pod cannot exceed this".
    pub limits: PodResources,
    /// Live usage summed across containers, from metrics-server via
    /// `ObservedWorld::pod_usage`. `None` when metrics-server is absent or did
    /// not report this pod — deliberately NOT zero, which a reader would take as
    /// idle. A pod without metrics is *unknown*, and the map has to be able to
    /// say so rather than paint an unearned all-clear.
    pub usage: Option<PodResources>,
    /// The kubelet's eviction order, from the shared [`crate::state::qos`] —
    /// [`crate::state::qos::pod_qos`], which prefers the API server's own
    /// `status.qosClass` over any derivation. Not the advisor's totals-based
    /// approximation; see that module for why they differ.
    pub qos: QosClass,
    /// How many containers this pod runs — regular plus native sidecars, the
    /// same set the resource sums above cover, so a count and a total always
    /// describe the same thing. A COUNT ONLY: per-container resource figures are
    /// not available here, because `Metrics.pods` is summed across containers
    /// (see the note on `usage`).
    pub containers: usize,
}

#[derive(Debug, Clone)]
pub struct NodeTile {
    pub name: String,
    pub zone: String,
    pub health: NodeHealth,
    pub ready: bool,
    pub cordoned: bool,
    /// Abnormal condition short names: "Mem", "Disk", "PID", "Net".
    pub abnormal: Vec<&'static str>,
    /// CPU/mem gauge ratios. Live usage ÷ allocatable when metrics-server
    /// is present, else scheduling pressure (requests ÷ allocatable). The
    /// `metric_source` says which.
    ///
    /// **Derived** from the two pairs below, in `build_node_tile` and nowhere
    /// else — usage when it exists, requests otherwise, and `None` when the node
    /// reports no allocatable at all (see those fields). Retained as a field
    /// rather than an accessor purely so existing consumers keep compiling
    /// (Rust has no property syntax, so a method would change every call site);
    /// converting them is a separate mechanical sweep. Prefer the explicit pair
    /// in new code: this one's *meaning* changes with `metric_source`, which is
    /// exactly what makes a requests-versus-usage comparison inexpressible.
    pub cpu_ratio: Option<f64>,
    pub mem_ratio: Option<f64>,
    /// Which of the two pairs below `cpu_ratio`/`mem_ratio` were derived from.
    /// Meaningful only where that ratio is `Some`; a node reporting no
    /// allocatable has no source because it has no ratio.
    pub metric_source: MetricSource,

    /// Sum of pod requests ÷ allocatable. This is what determines
    /// schedulability: a node at 1.0 accepts nothing more, whatever it is
    /// actually using. Filled from `node_request_ratios`, which sums the LITERAL
    /// request (see [`sum_pod_requests`]).
    ///
    /// `None` means **the node does not report that allocatable key** — and note
    /// this is a DIFFERENT and more serious `None` than the usage pair's below.
    /// Usage is best-effort telemetry that a cluster without metrics-server
    /// simply lacks; capacity is something a healthy node always publishes, so a
    /// node missing it is malfunctioning or mid-registration, and that is worth
    /// seeing rather than rendering as an empty node.
    ///
    /// (A0 documented this field as "always present … never `Option`". That was
    /// wrong: it holds for every healthy node, but not for the case the churn
    /// fleet reproduces on demand.)
    pub cpu_request_ratio: Option<f64>,
    pub mem_request_ratio: Option<f64>,
    /// Live usage ÷ allocatable. `None` when metrics-server did not report this
    /// node, rather than `Some(0.0)` — an unmeasured node is unknown, not idle.
    /// This is what determines OOM risk, which requests cannot show: a node can
    /// be fully requested and idle, or barely requested and about to OOM.
    ///
    /// `Some(0.0)` now means a genuine "used ~nothing": a node whose
    /// `allocatable` is missing yields `None` here too, so zero is never a
    /// stand-in for unknown.
    pub cpu_usage_ratio: Option<f64>,
    pub mem_usage_ratio: Option<f64>,
    pub pods: Vec<PodGlyph>,
    /// Saturation rollup (the 4th golden signal) — worst of cpu/mem/pod-count +
    /// the kubelet Disk/Mem/PID-pressure conditions. Computed once here so it
    /// rides `Province.tile` into the Saturation overlay with no extra plumbing.
    pub saturation: NodeSaturation,
}

/// What the node gauges are measuring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetricSource {
    /// Sum of pod requests ÷ allocatable (always available).
    #[default]
    Requests,
    /// Live usage ÷ allocatable, from metrics-server.
    Usage,
}

#[derive(Debug, Clone, Default)]
pub struct ZoneColumn {
    pub name: String,
    pub nodes: Vec<NodeTile>,
}

#[derive(Debug, Clone, Default)]
pub struct MapModel {
    pub zones: Vec<ZoneColumn>,
    pub total_nodes: usize,
    pub total_pods: usize,
    /// True when gauges reflect live metrics-server usage this build.
    pub metrics_live: bool,
}

impl MapModel {}

pub fn node_zone(node: &Node) -> String {
    node.metadata
        .labels
        .as_ref()
        .and_then(|l| l.get(ZONE_LABEL).or_else(|| l.get(ZONE_LABEL_LEGACY)))
        .cloned()
        .unwrap_or_else(|| UNZONED.into())
}

fn node_condition(node: &Node, type_: &str) -> Option<String> {
    node.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .and_then(|cs| cs.iter().find(|c| c.type_ == type_))
        .map(|c| c.status.clone())
}

pub fn node_ready(node: &Node) -> bool {
    node_condition(node, "Ready").as_deref() == Some("True")
}

/// Sum of one pod's CPU (cores) / memory (bytes) *requests* over its
/// `spec.containers` plus any **native sidecar** initContainers
/// (`restartPolicy: Always`, GA since k8s 1.29) — those run concurrently with
/// the app containers for the pod's whole life, so the scheduler reserves them
/// and metrics-server reports their usage. Plain (run-to-completion) init
/// containers are excluded (they don't run concurrently). The shared
/// request-summing primitive (`node_request_ratios` + the right-sizing advisor).
pub(crate) fn sum_pod_requests(pod: &Pod) -> (f64, f64) {
    sum_pod_resources(pod, false)
}

/// Same shape as [`sum_pod_requests`] but for `limits` (used by the right-sizing
/// advisor for QoS + the throttle/OOM escalation).
pub(crate) fn sum_pod_limits(pod: &Pod) -> (f64, f64) {
    sum_pod_resources(pod, true)
}

/// Sum of one pod's **reserved** cpu/mem — each container's request, defaulting
/// to its limit *per container, per resource* when only a limit is set (the
/// scheduler's effective reservation, which k8s defaults `request := limit`).
/// Used by cost allocation. Deliberately NOT shared with the right-sizing advisor
/// or `node_request_ratios`, which need the *literal* request (a request:=limit
/// default would corrupt the over/under comparison + QoS).
pub(crate) fn sum_pod_reserved(pod: &Pod) -> (f64, f64) {
    let (mut cpu, mut mem) = (0.0, 0.0);
    let Some(spec) = pod.spec.as_ref() else {
        return (cpu, mem);
    };
    let sidecars = spec
        .init_containers
        .iter()
        .flatten()
        .filter(|c| c.restart_policy.as_deref() == Some("Always"));
    for c in spec.containers.iter().chain(sidecars) {
        let req = c.resources.as_ref().and_then(|r| r.requests.as_ref());
        let lim = c.resources.as_ref().and_then(|r| r.limits.as_ref());
        // request, else this container's own limit (the k8s default), else 0.
        let pick = |k: &str| {
            req.and_then(|m| m.get(k))
                .and_then(quantity::value)
                .or_else(|| lim.and_then(|m| m.get(k)).and_then(quantity::value))
                .unwrap_or(0.0)
        };
        cpu += pick("cpu");
        mem += pick("memory");
    }
    (cpu, mem)
}

fn sum_pod_resources(pod: &Pod, limits: bool) -> (f64, f64) {
    let (mut cpu, mut mem) = (0.0, 0.0);
    let Some(spec) = pod.spec.as_ref() else {
        return (cpu, mem);
    };
    // Regular containers + native sidecars (restartPolicy:Always initContainers),
    // which are the containers that hold a reservation for the pod's whole life.
    let sidecars = spec
        .init_containers
        .iter()
        .flatten()
        .filter(|c| c.restart_policy.as_deref() == Some("Always"));
    for c in spec.containers.iter().chain(sidecars) {
        let map = c.resources.as_ref().and_then(|r| {
            if limits {
                r.limits.as_ref()
            } else {
                r.requests.as_ref()
            }
        });
        cpu += map
            .and_then(|r| r.get("cpu"))
            .and_then(quantity::value)
            .unwrap_or(0.0);
        mem += map
            .and_then(|r| r.get("memory"))
            .and_then(quantity::value)
            .unwrap_or(0.0);
    }
    (cpu, mem)
}

/// A node `status.allocatable` quantity by key (e.g. "cpu", "memory", "pods",
/// "ephemeral-storage") in canonical units (cores / bytes / plain count).
/// `None` when the key is absent — callers must NOT fabricate a default. A
/// fabricated zero makes an unmeasurable node indistinguishable from an idle
/// one; see [`node_request_ratios`]. The single sanctioned exception is
/// `cost::cost_report`, which defaults only to feed an immediate
/// "no capacity" guard and says so inline.
pub fn node_allocatable(node: &Node, key: &str) -> Option<f64> {
    node.status
        .as_ref()
        .and_then(|s| s.allocatable.as_ref())
        .and_then(|a| a.get(key))
        .and_then(quantity::value)
}

/// True when a pod's phase is terminal (Succeeded/Failed) — excluded from the
/// node's scheduling load (requests + pod-count saturation + cost allocation).
pub(crate) fn pod_terminal(pod: &Pod) -> bool {
    matches!(
        pod.status.as_ref().and_then(|s| s.phase.as_deref()),
        Some("Succeeded") | Some("Failed")
    )
}

/// Sum of CPU/memory *requests* of non-terminal pods on this node, divided by
/// allocatable — **`None` per resource when the node does not report that
/// allocatable key**.
///
/// The ratio is UNKNOWN, not zero. A zero is indistinguishable from an idle
/// node, which is the unearned all-clear `cost_report` already refuses at its
/// `cap_w <= 0.0` branch. This helper used to fabricate one (`unwrap_or(0.0)`),
/// and its own doc comment used to document the fabrication while
/// `node_allocatable`'s said callers must not — two contradicting comments in
/// one file. Per-resource because cpu and memory are separate keys and one can
/// be present without the other.
pub fn node_request_ratios(node: &Node, pods: &[&Pod]) -> (Option<f64>, Option<f64>) {
    let alloc_cpu = node_allocatable(node, "cpu");
    let alloc_mem = node_allocatable(node, "memory");

    let (mut cpu, mut mem) = (0.0, 0.0);
    for pod in pods {
        if pod_terminal(pod) {
            continue;
        }
        let (c, m) = sum_pod_requests(pod);
        cpu += c;
        mem += m;
    }
    // A zero/absent denominator yields None, never 0.0.
    let ratio = |used: f64, alloc: Option<f64>| alloc.filter(|a| *a > 0.0).map(|a| used / a);
    (ratio(cpu, alloc_cpu), ratio(mem, alloc_mem))
}

/// Live usage ÷ allocatable for a node (cores and bytes already canonical).
/// `None` per resource when that allocatable key is absent — see
/// [`node_request_ratios`] for why this is not zero.
fn node_usage_ratios(node: &Node, usage: NodeUsage) -> (Option<f64>, Option<f64>) {
    let alloc_cpu = node_allocatable(node, "cpu");
    let alloc_mem = node_allocatable(node, "memory");
    let ratio = |used: f64, alloc: Option<f64>| alloc.filter(|a| *a > 0.0).map(|a| used / a);
    (ratio(usage.cpu, alloc_cpu), ratio(usage.mem, alloc_mem))
}

/// PURE: one node's map tile.
///
/// `usage` is the NODE's live usage; `pod_usage` looks up one POD's, keyed
/// `(namespace, name)`. The lookup is a closure rather than an `&ObservedWorld`
/// so this stays a pure function of its arguments and remains testable with
/// hand-built objects (callers pass `&|ns, n| world.pod_usage(ns, n)`; tests
/// pass `&|_, _| None`).
pub fn build_node_tile(
    node: &Node,
    pods_on_node: &[&Pod],
    idx: &OwnerIndex,
    usage: Option<NodeUsage>,
    pod_usage: &dyn Fn(&str, &str) -> Option<NodeUsage>,
) -> NodeTile {
    let ready = node_ready(node);
    let cordoned = node
        .spec
        .as_ref()
        .and_then(|s| s.unschedulable)
        .unwrap_or(false);
    let mut abnormal = Vec::new();
    for (cond, short) in [
        ("MemoryPressure", "Mem"),
        ("DiskPressure", "Disk"),
        ("PIDPressure", "PID"),
        ("NetworkUnavailable", "Net"),
    ] {
        if node_condition(node, cond).as_deref() == Some("True") {
            abnormal.push(short);
        }
    }
    // BOTH pairs, always. Requests are computed even when metrics-server is up:
    // they are a different fact (what is claimed, hence schedulable) from usage
    // (what is consumed, hence at risk), and the interesting finding is in their
    // divergence. Previously only one branch ran, which is what made a
    // requests-versus-usage comparison inexpressible.
    let (cpu_request_ratio, mem_request_ratio) = node_request_ratios(node, pods_on_node);
    let (cpu_usage_ratio, mem_usage_ratio) = match usage {
        // The helper is per-resource optional now, so a node WITH a usage sample
        // but no allocatable still yields None — unknown, not idle.
        Some(u) => node_usage_ratios(node, u),
        // No metrics sample for this node at all.
        None => (None, None),
    };
    // The legacy polymorphic pair, derived HERE and only here so it cannot drift
    // from the explicit ratios above. Semantics preserved exactly: usage when
    // metrics-server reported, requests otherwise, with `metric_source` saying
    // which — so every existing consumer reads what it read before.
    let (cpu_ratio, mem_ratio, metric_source) = match (cpu_usage_ratio, mem_usage_ratio) {
        (Some(c), Some(m)) => (Some(c), Some(m), MetricSource::Usage),
        _ => (cpu_request_ratio, mem_request_ratio, MetricSource::Requests),
    };

    let health = if !ready {
        NodeHealth::NotReady
    } else if cordoned {
        NodeHealth::Cordoned
    // `is_some_and`: an unknown ratio is not pressure. We do not know the node is
    // loaded — and equally do not know it is idle, which the fabricated 0.0
    // quietly asserted.
    } else if !abnormal.is_empty()
        || cpu_ratio.is_some_and(|r| r >= PRESSURE_HIGH)
        || mem_ratio.is_some_and(|r| r >= PRESSURE_HIGH)
    {
        NodeHealth::Pressure
    } else {
        NodeHealth::Healthy
    };

    // Saturation (4th golden signal): worst of cpu/mem/pod-count + the kubelet
    // pressure conditions. Pod-count uses NON-terminal scheduled pods over
    // allocatable["pods"] (omitted entirely when allocatable["pods"] is absent —
    // never assume a default). Computed once here so it rides Province.tile.
    let nonterminal = pods_on_node.iter().filter(|p| !pod_terminal(p)).count() as u32;
    let alloc_pods = node_allocatable(node, "pods");
    let saturation =
        saturation::saturate_node(cpu_ratio, mem_ratio, nonterminal, alloc_pods, &abnormal);

    let mut pods: Vec<PodGlyph> = pods_on_node
        .iter()
        .map(|p| {
            let namespace = p.metadata.namespace.clone().unwrap_or_default();
            let name = p.metadata.name.clone().unwrap_or_default();
            // LITERAL request + the ceiling. Deliberately not `sum_pod_reserved`
            // — see the field docs; that one is cost's and would move both the
            // over/under comparison and QoS.
            let (rc, rm) = sum_pod_requests(p);
            let (lc, lm) = sum_pod_limits(p);
            let usage = pod_usage(&namespace, &name).map(|u| PodResources {
                cpu: u.cpu,
                mem: u.mem,
            });
            // Same container set the sums above cover, so count and total agree.
            let containers = p.spec.as_ref().map_or(0, |sp| {
                sp.containers.len()
                    + sp.init_containers
                        .iter()
                        .flatten()
                        .filter(|c| c.restart_policy.as_deref() == Some("Always"))
                        .count()
            });
            PodGlyph {
                namespace,
                name,
                state: pod_state(p).0,
                owner: idx.workload_of(p),
                requests: PodResources { cpu: rc, mem: rm },
                limits: PodResources { cpu: lc, mem: lm },
                usage,
                qos: crate::state::qos::pod_qos(p),
                containers,
            }
        })
        .collect();
    pods.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));

    NodeTile {
        name: node.metadata.name.clone().unwrap_or_default(),
        zone: node_zone(node),
        health,
        ready,
        cordoned,
        abnormal,
        cpu_ratio,
        mem_ratio,
        metric_source,
        cpu_request_ratio,
        mem_request_ratio,
        cpu_usage_ratio,
        mem_usage_ratio,
        pods,
        saturation,
    }
}

/// Zone columns sorted by name; nodes within a zone ordered by stable hash
/// of the node name so the layout never reshuffles between reconciles.
pub fn build_map(world: &ObservedWorld) -> MapModel {
    let idx = OwnerIndex::build(world);
    let pods = world.pods.state();
    let mut by_node: HashMap<String, Vec<&Pod>> = HashMap::new();
    for p in &pods {
        if let Some(node) = p.spec.as_ref().and_then(|s| s.node_name.clone()) {
            by_node.entry(node).or_default().push(p.as_ref());
        }
    }

    let nodes = world.nodes.state();
    let mut zones: BTreeMap<String, Vec<NodeTile>> = BTreeMap::new();
    for node in &nodes {
        let name = node.metadata.name.clone().unwrap_or_default();
        let on_node = by_node.get(&name).map(Vec::as_slice).unwrap_or(&[]);
        let tile = build_node_tile(node, on_node, &idx, world.node_usage(&name), &|ns, n| {
            world.pod_usage(ns, n)
        });
        zones.entry(tile.zone.clone()).or_default().push(tile);
    }
    let mut zones: Vec<ZoneColumn> = zones
        .into_iter()
        .map(|(name, mut nodes)| {
            nodes.sort_by_key(|t| (fnv1a64(&t.name), t.name.clone()));
            ZoneColumn { name, nodes }
        })
        .collect();
    // "unzoned" sinks to the end rather than sorting alphabetically.
    zones.sort_by_key(|z| (z.name == UNZONED, z.name.clone()));

    MapModel {
        total_nodes: nodes.len(),
        total_pods: pods.len(),
        metrics_live: world.metrics_available(),
        zones,
    }
}

// ---------------------------------------------------------------------------
// Workload rows & rollout status

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutStatus {
    Complete,
    Progressing,
    Stalled,
    Paused,
}

impl fmt::Display for RolloutStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RolloutStatus::Complete => "Complete",
            RolloutStatus::Progressing => "Progressing",
            RolloutStatus::Stalled => "Stalled",
            RolloutStatus::Paused => "Paused",
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkloadRow {
    pub r: WorkloadRef,
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
    pub updated: i32,
    pub status: RolloutStatus,
    pub note: String,
    pub age: Option<Time>,
    /// Per-workload SLO availability target from the `kubernation.io/slo-target`
    /// annotation, parsed (a fraction in (0,1)); `None` → the global default.
    pub slo_target: Option<f64>,
}

pub fn deployment_status(d: &Deployment) -> (RolloutStatus, String) {
    if d.spec.as_ref().and_then(|s| s.paused) == Some(true) {
        return (RolloutStatus::Paused, "rollout paused".into());
    }
    let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
    let st = d.status.as_ref();
    let generation = d.metadata.generation.unwrap_or(0);
    let observed = st.and_then(|s| s.observed_generation).unwrap_or(0);
    if observed < generation {
        return (RolloutStatus::Progressing, "awaiting observation".into());
    }
    if let Some(conds) = st.and_then(|s| s.conditions.as_ref())
        && conds.iter().any(|c| {
            c.type_ == "Progressing" && c.reason.as_deref() == Some("ProgressDeadlineExceeded")
        })
    {
        return (RolloutStatus::Stalled, "progress deadline exceeded".into());
    }
    let updated = st.and_then(|s| s.updated_replicas).unwrap_or(0);
    let total = st.and_then(|s| s.replicas).unwrap_or(0);
    let available = st.and_then(|s| s.available_replicas).unwrap_or(0);
    if updated < desired {
        return (
            RolloutStatus::Progressing,
            format!("updating {updated}/{desired}"),
        );
    }
    if total > updated {
        return (
            RolloutStatus::Progressing,
            format!("terminating {} old", total - updated),
        );
    }
    if available < updated {
        return (
            RolloutStatus::Progressing,
            format!("available {available}/{updated}"),
        );
    }
    (RolloutStatus::Complete, String::new())
}

pub fn statefulset_status(s: &StatefulSet) -> (RolloutStatus, String) {
    let desired = s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1);
    let st = s.status.as_ref();
    let ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
    let current_rev = st.and_then(|s| s.current_revision.as_deref()).unwrap_or("");
    let update_rev = st.and_then(|s| s.update_revision.as_deref()).unwrap_or("");
    if !update_rev.is_empty() && current_rev != update_rev {
        return (RolloutStatus::Progressing, "rolling update".into());
    }
    if ready < desired {
        return (
            RolloutStatus::Progressing,
            format!("ready {ready}/{desired}"),
        );
    }
    (RolloutStatus::Complete, String::new())
}

pub fn daemonset_status(d: &DaemonSet) -> (RolloutStatus, String) {
    let st = d.status.as_ref();
    let desired = st.map(|s| s.desired_number_scheduled).unwrap_or(0);
    let ready = st.map(|s| s.number_ready).unwrap_or(0);
    let updated = st
        .and_then(|s| s.updated_number_scheduled)
        .unwrap_or(desired);
    if updated < desired {
        return (
            RolloutStatus::Progressing,
            format!("updating {updated}/{desired}"),
        );
    }
    if ready < desired {
        return (
            RolloutStatus::Progressing,
            format!("ready {ready}/{desired}"),
        );
    }
    (RolloutStatus::Complete, String::new())
}

pub fn build_workloads(world: &ObservedWorld) -> Vec<WorkloadRow> {
    let mut rows = Vec::new();
    for d in world.deployments.state() {
        let (status, note) = deployment_status(&d);
        let st = d.status.as_ref();
        rows.push(WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: d.metadata.namespace.clone().unwrap_or_default(),
                name: d.metadata.name.clone().unwrap_or_default(),
            },
            desired: d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1),
            ready: st.and_then(|s| s.ready_replicas).unwrap_or(0),
            available: st.and_then(|s| s.available_replicas).unwrap_or(0),
            updated: st.and_then(|s| s.updated_replicas).unwrap_or(0),
            status,
            note,
            age: d.metadata.creation_timestamp.clone(),
            slo_target: crate::state::slo::annotation_target(d.metadata.annotations.as_ref()),
        });
    }
    for s in world.statefulsets.state() {
        let (status, note) = statefulset_status(&s);
        let st = s.status.as_ref();
        rows.push(WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::StatefulSet,
                namespace: s.metadata.namespace.clone().unwrap_or_default(),
                name: s.metadata.name.clone().unwrap_or_default(),
            },
            desired: s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1),
            ready: st.and_then(|s| s.ready_replicas).unwrap_or(0),
            available: st.and_then(|s| s.available_replicas).unwrap_or(0),
            updated: st.and_then(|s| s.updated_replicas).unwrap_or(0),
            status,
            note,
            age: s.metadata.creation_timestamp.clone(),
            slo_target: crate::state::slo::annotation_target(s.metadata.annotations.as_ref()),
        });
    }
    for d in world.daemonsets.state() {
        let (status, note) = daemonset_status(&d);
        let st = d.status.as_ref();
        rows.push(WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::DaemonSet,
                namespace: d.metadata.namespace.clone().unwrap_or_default(),
                name: d.metadata.name.clone().unwrap_or_default(),
            },
            desired: st.map(|s| s.desired_number_scheduled).unwrap_or(0),
            ready: st.map(|s| s.number_ready).unwrap_or(0),
            available: st.and_then(|s| s.number_available).unwrap_or(0),
            updated: st.and_then(|s| s.updated_number_scheduled).unwrap_or(0),
            status,
            note,
            age: d.metadata.creation_timestamp.clone(),
            slo_target: crate::state::slo::annotation_target(d.metadata.annotations.as_ref()),
        });
    }
    rows.sort_by(|a, b| a.r.cmp(&b.r));
    rows
}

// ---------------------------------------------------------------------------
// City screen model

#[derive(Debug, Clone)]
pub struct CityPod {
    pub name: String,
    pub state: PodState,
    pub reason: String,
    pub restarts: i32,
    pub age: Option<Time>,
    pub node: String,
    /// Live usage from metrics-server, if reporting (cpu cores, mem bytes).
    pub usage: Option<NodeUsage>,
    /// Plain-English "why isn't this Ready" + next action (None when healthy).
    pub diag: Option<crate::state::diagnose::Diagnosis>,
}

#[derive(Debug, Clone)]
pub struct OwnedRes {
    pub kind: &'static str, // "svc" | "cm" | "secret" | "pvc"
    pub name: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct CityModel {
    pub r: WorkloadRef,
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
    pub updated: i32,
    pub status: RolloutStatus,
    pub note: String,
    pub strategy: String,
    pub age: Option<Time>,
    pub pods: Vec<CityPod>,
    pub owned: Vec<OwnedRes>,
    /// First container of the pod template — the default target for a staged
    /// image change (`kubectl set image`). None if the template is absent.
    pub primary_container: Option<String>,
}

fn template_labels(t: Option<&PodTemplateSpec>) -> BTreeMap<String, String> {
    t.and_then(|t| t.metadata.as_ref())
        .and_then(|m| m.labels.clone())
        .unwrap_or_default()
}

/// The pod template of a workload, from the right store. Shared by the chaos
/// helpers below.
pub(crate) fn workload_template(
    world: &ObservedWorld,
    wr: &WorkloadRef,
) -> Option<PodTemplateSpec> {
    let ns = wr.namespace.as_str();
    let name = wr.name.as_str();
    let is = |m: &ObjectMeta| m.namespace.as_deref() == Some(ns) && m.name.as_deref() == Some(name);
    match wr.kind {
        WorkloadKind::Deployment => world
            .deployments
            .state()
            .into_iter()
            .find(|d| is(&d.metadata))
            .and_then(|d| d.spec.as_ref().map(|s| s.template.clone())),
        WorkloadKind::StatefulSet => world
            .statefulsets
            .state()
            .into_iter()
            .find(|s| is(&s.metadata))
            .and_then(|s| s.spec.as_ref().map(|sp| sp.template.clone())),
        WorkloadKind::DaemonSet => world
            .daemonsets
            .state()
            .into_iter()
            .find(|d| is(&d.metadata))
            .and_then(|d| d.spec.as_ref().map(|s| s.template.clone())),
    }
}

/// A workload's pod-template labels — the `podSelector` a chaos partition uses.
pub(crate) fn workload_template_labels(
    world: &ObservedWorld,
    wr: &WorkloadRef,
) -> BTreeMap<String, String> {
    workload_template(world, wr)
        .as_ref()
        .map(|t| template_labels(Some(t)))
        .unwrap_or_default()
}

/// A workload's first container name (the broken-image drill's target — matches
/// `build_city`'s `primary_container`).
pub(crate) fn workload_primary_container(
    world: &ObservedWorld,
    wr: &WorkloadRef,
) -> Option<String> {
    workload_template(world, wr)?
        .spec?
        .containers
        .first()
        .map(|c| c.name.clone())
        .filter(|n| !n.is_empty())
}

fn collect_refs(containers: &[Container], out: &mut BTreeSet<(&'static str, String)>) {
    for c in containers {
        for e in c.env.as_deref().unwrap_or(&[]) {
            if let Some(v) = e.value_from.as_ref() {
                if let Some(r) = v.config_map_key_ref.as_ref() {
                    out.insert(("cm", r.name.clone()));
                }
                if let Some(r) = v.secret_key_ref.as_ref() {
                    out.insert(("secret", r.name.clone()));
                }
            }
        }
        for e in c.env_from.as_deref().unwrap_or(&[]) {
            if let Some(r) = e.config_map_ref.as_ref() {
                out.insert(("cm", r.name.clone()));
            }
            if let Some(r) = e.secret_ref.as_ref() {
                out.insert(("secret", r.name.clone()));
            }
        }
    }
}

/// ConfigMap/Secret references straight from the pod template — we observe
/// the *shape* of dependencies without ever watching Secret contents.
fn template_refs(t: Option<&PodTemplateSpec>) -> BTreeSet<(&'static str, String)> {
    let mut out = BTreeSet::new();
    let Some(spec) = t.and_then(|t| t.spec.as_ref()) else {
        return out;
    };
    collect_refs(&spec.containers, &mut out);
    collect_refs(spec.init_containers.as_deref().unwrap_or(&[]), &mut out);
    for v in spec.volumes.as_deref().unwrap_or(&[]) {
        if let Some(cm) = v.config_map.as_ref() {
            out.insert(("cm", cm.name.clone()));
        }
        if let Some(s) = v.secret.as_ref()
            && let Some(n) = s.secret_name.clone()
        {
            out.insert(("secret", n));
        }
    }
    out
}

/// The host of an Ingress's first rule, or `*` for a catch-all.
fn ingress_host(ing: &Ingress) -> String {
    ing.spec
        .as_ref()
        .and_then(|s| s.rules.as_ref())
        .and_then(|r| r.first())
        .and_then(|r| r.host.clone())
        .unwrap_or_else(|| "*".into())
}

/// Every Service an Ingress backends to (default backend + every rule path).
pub(crate) fn ingress_backends(ing: &Ingress) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(spec) = ing.spec.as_ref() else {
        return out;
    };
    if let Some(svc) = spec
        .default_backend
        .as_ref()
        .and_then(|b| b.service.as_ref())
    {
        out.insert(svc.name.clone());
    }
    for rule in spec.rules.as_deref().unwrap_or(&[]) {
        for path in rule
            .http
            .as_ref()
            .map(|h| h.paths.as_slice())
            .unwrap_or(&[])
        {
            if let Some(svc) = path.backend.service.as_ref() {
                out.insert(svc.name.clone());
            }
        }
    }
    out
}

/// The connectivity layer: which Services front which workloads (harbors)
/// and which Ingresses route to them (gates). Pure over `ObservedWorld`;
/// feeds the coast markers in `build_world`. The reverse of `build_city`'s
/// per-workload service match, computed once for the whole map.
pub fn build_exposure(world: &ObservedWorld) -> Vec<ExposureEntry> {
    // Each workload paired with its pod-template labels.
    let mut wl: Vec<(WorkloadRef, BTreeMap<String, String>)> = Vec::new();
    for d in world.deployments.state() {
        if let (Some(ns), Some(name)) = (d.metadata.namespace.clone(), d.metadata.name.clone()) {
            let labels = template_labels(d.spec.as_ref().map(|s| &s.template));
            wl.push((
                WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: ns,
                    name,
                },
                labels,
            ));
        }
    }
    for s in world.statefulsets.state() {
        if let (Some(ns), Some(name)) = (s.metadata.namespace.clone(), s.metadata.name.clone()) {
            let labels = template_labels(s.spec.as_ref().map(|s| &s.template));
            wl.push((
                WorkloadRef {
                    kind: WorkloadKind::StatefulSet,
                    namespace: ns,
                    name,
                },
                labels,
            ));
        }
    }
    for ds in world.daemonsets.state() {
        if let (Some(ns), Some(name)) = (ds.metadata.namespace.clone(), ds.metadata.name.clone()) {
            let labels = template_labels(ds.spec.as_ref().map(|s| &s.template));
            wl.push((
                WorkloadRef {
                    kind: WorkloadKind::DaemonSet,
                    namespace: ns,
                    name,
                },
                labels,
            ));
        }
    }

    let mut out: Vec<ExposureEntry> = Vec::new();
    // Service harbors, plus a name→workloads index for ingress resolution.
    let mut svc_targets: HashMap<(String, String), Vec<WorkloadRef>> = HashMap::new();
    for svc in world.services.state() {
        let Some(ns) = svc.metadata.namespace.clone() else {
            continue;
        };
        let Some(name) = svc.metadata.name.clone() else {
            continue;
        };
        let Some(sel) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
            continue;
        };
        if sel.is_empty() {
            continue;
        }
        let type_ = svc
            .spec
            .as_ref()
            .and_then(|s| s.type_.clone())
            .unwrap_or_else(|| "ClusterIP".into());
        for (wr, labels) in &wl {
            if wr.namespace != ns {
                continue;
            }
            if sel.iter().all(|(k, v)| labels.get(k) == Some(v)) {
                out.push(ExposureEntry {
                    workload: wr.clone(),
                    kind: CoastKind::Harbor,
                    name: name.clone(),
                    detail: type_.clone(),
                });
                svc_targets
                    .entry((ns.clone(), name.clone()))
                    .or_default()
                    .push(wr.clone());
            }
        }
    }

    // Ingress gates: resolve each backend service to the workloads it fronts.
    for ing in world.ingresses.state() {
        let Some(ns) = ing.metadata.namespace.clone() else {
            continue;
        };
        let Some(name) = ing.metadata.name.clone() else {
            continue;
        };
        let host = ingress_host(&ing);
        for backend in ingress_backends(&ing) {
            if let Some(targets) = svc_targets.get(&(ns.clone(), backend)) {
                for wr in targets {
                    out.push(ExposureEntry {
                        workload: wr.clone(),
                        kind: CoastKind::Gate,
                        name: name.clone(),
                        detail: host.clone(),
                    });
                }
            }
        }
    }

    // One marker per (workload, kind, name) — an ingress fanned across paths
    // to the same service must not stack gates.
    out.sort_by(|a, b| {
        (&a.workload, a.kind as u8, &a.name).cmp(&(&b.workload, b.kind as u8, &b.name))
    });
    out.dedup_by(|a, b| a.workload == b.workload && a.kind == b.kind && a.name == b.name);
    out
}

/// Batch workloads as expedition entries for the islands. A Job's detail
/// summarises completion/active/failed; a CronJob shows its schedule (and
/// running count). CronJob-spawned Jobs are folded into their CronJob rather
/// than listed individually, so job history doesn't flood the island.
pub fn build_batch(world: &ObservedWorld) -> Vec<BatchEntry> {
    let mut out: Vec<BatchEntry> = Vec::new();
    for j in world.jobs.state() {
        let (Some(ns), Some(name)) = (j.metadata.namespace.clone(), j.metadata.name.clone()) else {
            continue;
        };
        let owned_by_cron = j
            .metadata
            .owner_references
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .any(|o| o.kind == "CronJob");
        if owned_by_cron {
            continue;
        }
        let st = j.status.as_ref();
        let completions = j
            .spec
            .as_ref()
            .and_then(|s| s.completions)
            .unwrap_or(1)
            .max(1);
        let succeeded = st.and_then(|s| s.succeeded).unwrap_or(0);
        let active = st.and_then(|s| s.active).unwrap_or(0);
        let failed = st.and_then(|s| s.failed).unwrap_or(0);
        let (detail, alert) = if succeeded >= completions {
            (format!("{succeeded}/{completions} ✓"), false)
        } else if active > 0 {
            (format!("{active} active"), false)
        } else if failed > 0 {
            (format!("{failed} failed ✗"), true)
        } else {
            ("pending".to_string(), false)
        };
        out.push(BatchEntry {
            kind: BatchKind::Job,
            namespace: ns,
            name,
            detail,
            alert,
        });
    }
    for c in world.cronjobs.state() {
        let (Some(ns), Some(name)) = (c.metadata.namespace.clone(), c.metadata.name.clone()) else {
            continue;
        };
        let spec = c.spec.as_ref();
        let schedule = spec.map(|s| s.schedule.clone()).unwrap_or_default();
        let suspended = spec.and_then(|s| s.suspend).unwrap_or(false);
        let active = c
            .status
            .as_ref()
            .and_then(|s| s.active.as_ref())
            .map(|a| a.len())
            .unwrap_or(0);
        let mut detail = if suspended {
            format!("{schedule} (suspended)")
        } else {
            schedule
        };
        if active > 0 {
            detail = format!("{detail} . {active} active");
        }
        out.push(BatchEntry {
            kind: BatchKind::CronJob,
            namespace: ns,
            name,
            detail,
            alert: false,
        });
    }
    out.sort_by(|a, b| {
        (a.kind.label(), &a.namespace, &a.name).cmp(&(b.kind.label(), &b.namespace, &b.name))
    });
    out
}

/// Per-workload persistent storage: how many PVCs it mounts and how many
/// are not yet Bound. Feeds the granary mark beside each city. Pod volumes
/// plus, for StatefulSets, the volumeClaimTemplate-derived claims (which can
/// exist before/after their pods) — the same union `build_city` shows.
pub fn build_storage(world: &ObservedWorld) -> Vec<StorageEntry> {
    let idx = OwnerIndex::build(world);
    let mut claims: HashMap<WorkloadRef, BTreeSet<String>> = HashMap::new();
    for p in world.pods.state() {
        let Some(r) = idx.workload_of(&p) else {
            continue;
        };
        for v in p
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_deref())
            .unwrap_or(&[])
        {
            if let Some(c) = v.persistent_volume_claim.as_ref() {
                claims
                    .entry(r.clone())
                    .or_default()
                    .insert(c.claim_name.clone());
            }
        }
    }
    for s in world.statefulsets.state() {
        let (Some(ns), Some(name)) = (s.metadata.namespace.clone(), s.metadata.name.clone()) else {
            continue;
        };
        let r = WorkloadRef {
            kind: WorkloadKind::StatefulSet,
            namespace: ns.clone(),
            name: name.clone(),
        };
        for t in s
            .spec
            .as_ref()
            .and_then(|sp| sp.volume_claim_templates.as_deref())
            .unwrap_or(&[])
        {
            let prefix = format!("{}-{}-", t.metadata.name.clone().unwrap_or_default(), name);
            for pvc in world.pvcs.state() {
                if pvc.metadata.namespace.as_deref() == Some(&ns)
                    && pvc
                        .metadata
                        .name
                        .as_deref()
                        .is_some_and(|n| n.starts_with(&prefix))
                {
                    claims
                        .entry(r.clone())
                        .or_default()
                        .insert(pvc.metadata.name.clone().unwrap_or_default());
                }
            }
        }
    }
    let mut out: Vec<StorageEntry> = claims
        .into_iter()
        .map(|(r, names)| {
            let pending = names
                .iter()
                .filter(|n| pvc_phase(world, &r.namespace, n).as_deref() != Some("Bound"))
                .count();
            StorageEntry {
                workload: r,
                claims: names.len(),
                pending,
            }
        })
        .collect();
    out.sort_by(|a, b| a.workload.cmp(&b.workload));
    out
}

/// The `status.phase` of a PVC by namespace+name, if observed.
fn pvc_phase(world: &ObservedWorld, namespace: &str, name: &str) -> Option<String> {
    world
        .pvcs
        .state()
        .into_iter()
        .find(|p| {
            p.metadata.namespace.as_deref() == Some(namespace)
                && p.metadata.name.as_deref() == Some(name)
        })
        .and_then(|p| p.status.as_ref().and_then(|s| s.phase.clone()))
}

pub fn build_city(world: &ObservedWorld, r: &WorkloadRef) -> Option<CityModel> {
    let idx = OwnerIndex::build(world);

    // Header numbers + template, from whichever kind this is.
    let (desired, ready, available, updated, status, note, strategy, age, template);
    match r.kind {
        WorkloadKind::Deployment => {
            let d = world.deployments.state().into_iter().find(|d| {
                d.metadata.namespace.as_deref() == Some(&r.namespace)
                    && d.metadata.name.as_deref() == Some(&r.name)
            })?;
            let st = d.status.as_ref();
            desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
            ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
            available = st.and_then(|s| s.available_replicas).unwrap_or(0);
            updated = st.and_then(|s| s.updated_replicas).unwrap_or(0);
            (status, note) = deployment_status(&d);
            strategy = d
                .spec
                .as_ref()
                .and_then(|s| s.strategy.as_ref())
                .and_then(|s| s.type_.clone())
                .unwrap_or_else(|| "RollingUpdate".into());
            age = d.metadata.creation_timestamp.clone();
            template = d.spec.as_ref().map(|s| s.template.clone());
        }
        WorkloadKind::StatefulSet => {
            let s = world.statefulsets.state().into_iter().find(|s| {
                s.metadata.namespace.as_deref() == Some(&r.namespace)
                    && s.metadata.name.as_deref() == Some(&r.name)
            })?;
            let st = s.status.as_ref();
            desired = s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1);
            ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
            available = st.and_then(|s| s.available_replicas).unwrap_or(0);
            updated = st.and_then(|s| s.updated_replicas).unwrap_or(0);
            (status, note) = statefulset_status(&s);
            strategy = s
                .spec
                .as_ref()
                .and_then(|sp| sp.update_strategy.as_ref())
                .and_then(|u| u.type_.clone())
                .unwrap_or_else(|| "RollingUpdate".into());
            age = s.metadata.creation_timestamp.clone();
            template = s.spec.as_ref().map(|sp| sp.template.clone());
        }
        WorkloadKind::DaemonSet => {
            let d = world.daemonsets.state().into_iter().find(|d| {
                d.metadata.namespace.as_deref() == Some(&r.namespace)
                    && d.metadata.name.as_deref() == Some(&r.name)
            })?;
            let st = d.status.as_ref();
            desired = st.map(|s| s.desired_number_scheduled).unwrap_or(0);
            ready = st.map(|s| s.number_ready).unwrap_or(0);
            available = st.and_then(|s| s.number_available).unwrap_or(0);
            updated = st.and_then(|s| s.updated_number_scheduled).unwrap_or(0);
            (status, note) = daemonset_status(&d);
            strategy = d
                .spec
                .as_ref()
                .and_then(|sp| sp.update_strategy.as_ref())
                .and_then(|u| u.type_.clone())
                .unwrap_or_else(|| "RollingUpdate".into());
            age = d.metadata.creation_timestamp.clone();
            template = d.spec.as_ref().map(|sp| sp.template.clone());
        }
    }

    // Member pods via the ownership chain.
    let mut pods: Vec<CityPod> = Vec::new();
    let mut pvc_names: BTreeSet<String> = BTreeSet::new();
    for p in world.pods.state() {
        if idx.workload_of(&p).as_ref() != Some(r) {
            continue;
        }
        let (state, reason) = pod_state(&p);
        let name = p.metadata.name.clone().unwrap_or_default();
        for v in p
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_deref())
            .unwrap_or(&[])
        {
            if let Some(c) = v.persistent_volume_claim.as_ref() {
                pvc_names.insert(c.claim_name.clone());
            }
        }
        let usage = world.pod_usage(&r.namespace, &name);
        let restarts = pod_restarts(&p);
        let diag = crate::state::diagnose::diagnose(&reason, restarts, pod_oom_killed(&p));
        pods.push(CityPod {
            name,
            state,
            reason,
            restarts,
            age: p.metadata.creation_timestamp.clone(),
            node: p
                .spec
                .as_ref()
                .and_then(|s| s.node_name.clone())
                .unwrap_or_default(),
            usage,
            diag,
        });
    }
    pods.sort_by(|a, b| a.name.cmp(&b.name));

    // Owned resources.
    let labels = template_labels(template.as_ref());
    let mut owned: Vec<OwnedRes> = Vec::new();
    let mut my_services: BTreeSet<String> = BTreeSet::new();
    for svc in world.services.state() {
        if svc.metadata.namespace.as_deref() != Some(&r.namespace) {
            continue;
        }
        let Some(sel) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
            continue;
        };
        if !sel.is_empty() && sel.iter().all(|(k, v)| labels.get(k) == Some(v)) {
            let name = svc.metadata.name.clone().unwrap_or_default();
            my_services.insert(name.clone());
            owned.push(OwnedRes {
                kind: "svc",
                name,
                note: svc
                    .spec
                    .as_ref()
                    .and_then(|s| s.type_.clone())
                    .unwrap_or_default(),
            });
        }
    }
    // Ingress gates routing to any of this city's services.
    for ing in world.ingresses.state() {
        if ing.metadata.namespace.as_deref() != Some(&r.namespace) {
            continue;
        }
        if ingress_backends(&ing)
            .iter()
            .any(|b| my_services.contains(b))
        {
            owned.push(OwnedRes {
                kind: "ing",
                name: ing.metadata.name.clone().unwrap_or_default(),
                note: ingress_host(&ing),
            });
        }
    }
    // StatefulSet claims may exist before/after their pods do.
    if r.kind == WorkloadKind::StatefulSet
        && let Some(s) = world.statefulsets.state().into_iter().find(|s| {
            s.metadata.namespace.as_deref() == Some(&r.namespace)
                && s.metadata.name.as_deref() == Some(&r.name)
        })
    {
        for t in s
            .spec
            .as_ref()
            .and_then(|sp| sp.volume_claim_templates.as_deref())
            .unwrap_or(&[])
        {
            let prefix = format!(
                "{}-{}-",
                t.metadata.name.clone().unwrap_or_default(),
                r.name
            );
            for pvc in world.pvcs.state() {
                if pvc.metadata.namespace.as_deref() == Some(&r.namespace)
                    && pvc
                        .metadata
                        .name
                        .as_deref()
                        .is_some_and(|n| n.starts_with(&prefix))
                {
                    pvc_names.insert(pvc.metadata.name.clone().unwrap_or_default());
                }
            }
        }
    }
    for name in &pvc_names {
        let phase = pvc_phase(world, &r.namespace, name).unwrap_or_else(|| "?".into());
        owned.push(OwnedRes {
            kind: "pvc",
            name: name.clone(),
            note: phase,
        });
    }
    for (kind, name) in template_refs(template.as_ref()) {
        owned.push(OwnedRes {
            kind,
            name,
            note: String::new(),
        });
    }

    let primary_container = template
        .as_ref()
        .and_then(|t| t.spec.as_ref())
        .and_then(|s| s.containers.first())
        .map(|c| c.name.clone());

    Some(CityModel {
        r: r.clone(),
        desired,
        ready,
        available,
        updated,
        status,
        note,
        strategy,
        age,
        pods,
        owned,
        primary_container,
    })
}

// ---------------------------------------------------------------------------
// Node detail model

#[derive(Debug, Clone)]
pub struct NodePodRow {
    pub namespace: String,
    pub name: String,
    pub state: PodState,
    pub reason: String,
    pub restarts: i32,
    pub age: Option<Time>,
    pub owner: Option<WorkloadRef>,
    /// Live usage from metrics-server, if reporting (cpu cores, mem bytes).
    pub usage: Option<NodeUsage>,
    /// Plain-English "why isn't this Ready" + next action (None when healthy).
    pub diag: Option<crate::state::diagnose::Diagnosis>,
}

#[derive(Debug, Clone)]
pub struct NodeDetailModel {
    pub tile: NodeTile,
    /// Terrain attributes: runtime, kubelet, OS, arch, kernel, provider.
    pub info: Vec<(&'static str, String)>,
    pub conditions: Vec<(String, String)>,
    pub cpu_alloc: f64,
    pub mem_alloc: f64,
    /// Recent cpu/mem usage-ratio samples (oldest→newest, usage ÷ allocatable)
    /// for the trend sparklines beside the gauges. Empty when metrics-server
    /// isn't reporting; the latest value tracks `tile.cpu_ratio`/`mem_ratio`.
    pub cpu_history: Vec<f32>,
    pub mem_history: Vec<f32>,
    pub pods: Vec<NodePodRow>,
    /// The node's SUBSTRATE: distinct DaemonSets with pods stationed here, as
    /// `namespace/name`, sorted. What actually runs *under* the workloads — CNI,
    /// kube-proxy, log/metric agents — which is otherwise only visible as an
    /// anonymous road count on the map.
    ///
    /// Namespace-qualified to match `substrate::SubstrateReport`, whose gap list
    /// is rendered directly beneath this one in the same window: two DaemonSets
    /// in different namespaces may share a name, so a bare name can't tell the
    /// operator which one is meant (and there it would merge two identities).
    ///
    /// Deliberately derived here rather than shared with `world::build_world`'s
    /// `Province.infra`: that one is gated on the *filtered* workload list
    /// because it decides which roads to pave, while a node drill-down reports
    /// what is on the node regardless of the active namespace view.
    pub daemonsets: Vec<String>,
}

/// Turn a node's raw usage history into cpu/mem ratio series (usage ÷
/// allocatable), the sparkline inputs. Pure + unit-tested; a zero/absent
/// allocatable yields an empty series (nothing meaningful to chart).
fn usage_ratios_series(
    history: &[NodeUsage],
    cpu_alloc: f64,
    mem_alloc: f64,
) -> (Vec<f32>, Vec<f32>) {
    let cpu = if cpu_alloc > 0.0 {
        history.iter().map(|u| (u.cpu / cpu_alloc) as f32).collect()
    } else {
        Vec::new()
    };
    let mem = if mem_alloc > 0.0 {
        history.iter().map(|u| (u.mem / mem_alloc) as f32).collect()
    } else {
        Vec::new()
    };
    (cpu, mem)
}

pub fn build_node_detail(world: &ObservedWorld, name: &str) -> Option<NodeDetailModel> {
    let node = world
        .nodes
        .state()
        .into_iter()
        .find(|n| n.metadata.name.as_deref() == Some(name))?;

    let pods_arc = world.pods.state();
    let on_node: Vec<&Pod> = pods_arc
        .iter()
        .map(|p| p.as_ref())
        .filter(|p| p.spec.as_ref().and_then(|s| s.node_name.as_deref()) == Some(name))
        .collect();
    let idx = OwnerIndex::build(world);
    let tile = build_node_tile(&node, &on_node, &idx, world.node_usage(name), &|ns, n| {
        world.pod_usage(ns, n)
    });

    // Substrate: the DaemonSets stationed here, `namespace/name`. BTreeSet for a
    // stable sorted order (the window lists them verbatim).
    let daemonsets: Vec<String> = on_node
        .iter()
        .filter_map(|p| idx.workload_of(p))
        .filter(|o| o.kind == WorkloadKind::DaemonSet)
        .map(|o| format!("{}/{}", o.namespace, o.name))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut info = Vec::new();
    if let Some(ni) = node.status.as_ref().and_then(|s| s.node_info.as_ref()) {
        info.push(("runtime", ni.container_runtime_version.clone()));
        info.push(("kubelet", ni.kubelet_version.clone()));
        info.push(("os", ni.os_image.clone()));
        info.push(("arch", ni.architecture.clone()));
        info.push(("kernel", ni.kernel_version.clone()));
    }
    if let Some(pid) = node.spec.as_ref().and_then(|s| s.provider_id.as_ref()) {
        info.push(("provider", pid.clone()));
    }
    if let Some(addr) = node.status.as_ref().and_then(|s| s.addresses.as_ref())
        && let Some(ip) = addr.iter().find(|a| a.type_ == "InternalIP")
    {
        info.push(("internal-ip", ip.address.clone()));
    }

    let conditions = node
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|cs| {
            cs.iter()
                .map(|c| (c.type_.clone(), c.status.clone()))
                .collect()
        })
        .unwrap_or_default();

    let alloc = node.status.as_ref().and_then(|s| s.allocatable.as_ref());
    let cpu_alloc = alloc
        .and_then(|a| a.get("cpu"))
        .and_then(quantity::value)
        .unwrap_or(0.0);
    let mem_alloc = alloc
        .and_then(|a| a.get("memory"))
        .and_then(quantity::value)
        .unwrap_or(0.0);

    let mut pods: Vec<NodePodRow> = on_node
        .iter()
        .map(|p| {
            let (state, reason) = pod_state(p);
            let namespace = p.metadata.namespace.clone().unwrap_or_default();
            let name = p.metadata.name.clone().unwrap_or_default();
            let usage = world.pod_usage(&namespace, &name);
            let restarts = pod_restarts(p);
            let diag = crate::state::diagnose::diagnose(&reason, restarts, pod_oom_killed(p));
            NodePodRow {
                namespace,
                name,
                state,
                reason,
                restarts,
                age: p.metadata.creation_timestamp.clone(),
                owner: idx.workload_of(p),
                usage,
                diag,
            }
        })
        .collect();
    pods.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));

    let (cpu_history, mem_history) =
        usage_ratios_series(&world.node_usage_history(name), cpu_alloc, mem_alloc);

    Some(NodeDetailModel {
        daemonsets,
        tile,
        info,
        conditions,
        cpu_alloc,
        mem_alloc,
        cpu_history,
        mem_history,
        pods,
    })
}

// ---------------------------------------------------------------------------
// Aggregate

/// Everything the UI renders from, rebuilt wholesale on world change (at
/// tick cadence). Detail views additionally re-derive their own models.
#[derive(Default)]
pub struct Models {
    pub map: MapModel,
    pub workloads: Vec<WorkloadRow>,
    pub attention: Vec<Concern>,
    pub workload_severity: HashMap<WorkloadRef, Severity>,
    /// Per-workload NetworkPolicy coverage ("walls"), cluster-wide — the map
    /// overlay + city breach-mark read this. Built unfiltered (like the advisor)
    /// so a city always carries its true wall state.
    pub coverage: HashMap<WorkloadRef, crate::state::netpol::Coverage>,
    /// Workloads fronted by a Service/Ingress (reachable) — drives the
    /// "unwalled AND exposed" breach. Reuses `build_exposure`.
    pub exposed: HashSet<WorkloadRef>,
    /// Per-node DaemonSet coverage gaps — the Substrate overlay + the province
    /// window read this. Unfiltered, like `coverage`: which nodes lack the
    /// fleet's infrastructure is a physical fact, not a namespace view.
    pub substrate: crate::state::substrate::SubstrateReport,
    /// The explorable world projection of all of the above.
    pub world: WorldModel,
}

impl Models {
    /// Build the full model set across every namespace.
    pub fn build(world: &ObservedWorld) -> Self {
        Self::build_filtered(world, &NamespaceFilter::All)
    }

    /// Build the model set scoped to `filter`. Terrain (nodes are
    /// cluster-scoped) is unaffected; cities, the workload list, attention,
    /// and island structures all narrow together. Filtering is applied to the
    /// *derived* layer only — the observed stores are untouched.
    pub fn build_filtered(world: &ObservedWorld, filter: &NamespaceFilter) -> Self {
        let map = build_map(world);
        let mut workloads = build_workloads(world);
        workloads.retain(|w| filter.matches(&w.r.namespace));
        let attention = attention::build(world, &map, &workloads, filter);
        let mut workload_severity: HashMap<WorkloadRef, Severity> = HashMap::new();
        for c in &attention {
            if let Target::Workload(r) = &c.target {
                workload_severity
                    .entry(r.clone())
                    .and_modify(|s| *s = (*s).max(c.severity))
                    .or_insert(c.severity);
            }
        }
        // Narrow the island/coast inputs to the same namespaces (cities are
        // already narrowed via `workloads`).
        let customs: Vec<_> = world
            .custom_entries()
            .into_iter()
            .filter(|c| filter.matches_opt(c.namespace.as_deref()))
            .collect();
        let exposure: Vec<_> = build_exposure(world)
            .into_iter()
            .filter(|e| filter.matches(&e.workload.namespace))
            .collect();
        let storage: Vec<_> = build_storage(world)
            .into_iter()
            .filter(|s| filter.matches(&s.workload.namespace))
            .collect();
        let batch: Vec<_> = build_batch(world)
            .into_iter()
            .filter(|b| filter.matches(&b.namespace))
            .collect();
        let world_model = build_world(
            &map,
            &workloads,
            &workload_severity,
            &customs,
            &exposure,
            &storage,
            &batch,
        );
        // NetworkPolicy "walls" coverage — cluster-wide (the map shows a city's
        // true wall state regardless of the active namespace filter).
        let netpol = crate::state::netpol::coverage_report(world);
        let coverage: HashMap<WorkloadRef, crate::state::netpol::Coverage> =
            netpol.rows.iter().map(|r| (r.r.clone(), r.cov)).collect();
        let exposed: HashSet<WorkloadRef> = netpol
            .rows
            .iter()
            .filter(|r| r.exposed)
            .map(|r| r.r.clone())
            .collect();
        // DaemonSet coverage — cluster-wide for the same reason as `coverage`,
        // and NOT reusing `Province.infra` (which is gated on the filtered
        // workload list, so prevalence over it would report phantom gaps).
        let substrate = crate::state::substrate::coverage_report(world);
        Models {
            map,
            workloads,
            attention,
            workload_severity,
            coverage,
            exposed,
            substrate,
            world: world_model,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use k8s_openapi::jiff;

    #[test]
    fn usage_ratios_series_divides_by_allocatable() {
        let hist = [
            NodeUsage {
                cpu: 1.0,
                mem: 1_000.0,
            },
            NodeUsage {
                cpu: 2.0,
                mem: 3_000.0,
            },
        ];
        // cpu_alloc 4 cores, mem_alloc 4000 bytes.
        let (cpu, mem) = usage_ratios_series(&hist, 4.0, 4_000.0);
        assert_eq!(cpu, vec![0.25, 0.5]);
        assert_eq!(mem, vec![0.25, 0.75]);
        // Absent allocatable → empty series (nothing meaningful to chart).
        let (cpu0, mem0) = usage_ratios_series(&hist, 0.0, 0.0);
        assert!(cpu0.is_empty() && mem0.is_empty());
        // Empty history → empty series.
        let (c, m) = usage_ratios_series(&[], 4.0, 4_000.0);
        assert!(c.is_empty() && m.is_empty());
    }

    #[test]
    fn map_zone_columns_sorted_unzoned_last() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n-bravo", Some("z-b")));
        s.node(fx::node("n-alpha", Some("z-a")));
        s.node(fx::node("n-loner", None));
        let m = build_map(&world);
        let names: Vec<&str> = m.zones.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(names, ["z-a", "z-b", UNZONED]);
        assert_eq!(m.total_nodes, 3);
    }

    #[test]
    fn map_layout_is_stable_under_insertion() {
        let (world, mut s) = fx::world();
        for n in ["n-a", "n-b", "n-c", "n-d"] {
            s.node(fx::node(n, Some("z-a")));
        }
        let before: Vec<String> = build_map(&world).zones[0]
            .nodes
            .iter()
            .map(|t| t.name.clone())
            .collect();
        // Two rebuilds agree (determinism)…
        let again: Vec<String> = build_map(&world).zones[0]
            .nodes
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert_eq!(before, again);
        // …and inserting a node never reorders the existing ones.
        s.node(fx::node("n-e", Some("z-a")));
        let after: Vec<String> = build_map(&world).zones[0]
            .nodes
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let filtered: Vec<String> = after.iter().filter(|n| *n != "n-e").cloned().collect();
        assert_eq!(before, filtered);
        assert_eq!(after.len(), 5);
    }

    #[test]
    fn request_pressure_from_requests_vs_allocatable() {
        // Node allocatable: 4 cores / 8Gi (fixture default).
        let n = fx::node("n1", Some("z-a"));
        let p1 = fx::pod_requests(fx::pod("d", "p1", Some("n1")), "1", "2Gi");
        let p2 = fx::pod_requests(fx::pod("d", "p2", Some("n1")), "1000m", "2Gi");
        // Succeeded pods do not count toward scheduling pressure.
        let done = fx::pod_phase(
            fx::pod_requests(fx::pod("d", "p3", Some("n1")), "4", "8Gi"),
            "Succeeded",
        );
        let tile = build_node_tile(
            &n,
            &[&p1, &p2, &done],
            &OwnerIndex::default(),
            None,
            &|_, _| None,
        );
        assert!(
            (tile.cpu_ratio.expect("cpu ratio") - 0.5).abs() < 1e-9,
            "cpu {}",
            tile.cpu_ratio.unwrap_or(f64::NAN)
        );
        assert!(
            (tile.mem_ratio.expect("mem ratio") - 0.5).abs() < 1e-9,
            "mem {}",
            tile.mem_ratio.unwrap_or(f64::NAN)
        );
        assert_eq!(tile.pods.len(), 3); // glyphs still show all pods
        assert_eq!(tile.health, NodeHealth::Healthy);
        assert_eq!(tile.metric_source, MetricSource::Requests);
    }

    #[test]
    fn live_usage_overrides_request_pressure() {
        // Fixture node: 4 cores / 8Gi allocatable.
        let n = fx::node("n1", None);
        let usage = NodeUsage {
            cpu: 1.0,                            // 1 of 4 cores
            mem: 2.0 * 1024.0 * 1024.0 * 1024.0, // 2 of 8 GiB
        };
        let tile = build_node_tile(&n, &[], &OwnerIndex::default(), Some(usage), &|_, _| None);
        assert_eq!(tile.metric_source, MetricSource::Usage);
        assert!(
            (tile.cpu_ratio.expect("cpu ratio") - 0.25).abs() < 1e-9,
            "cpu {}",
            tile.cpu_ratio.unwrap_or(f64::NAN)
        );
        assert!(
            (tile.mem_ratio.expect("mem ratio") - 0.25).abs() < 1e-9,
            "mem {}",
            tile.mem_ratio.unwrap_or(f64::NAN)
        );
        // No usage → request-based pressure and source Requests.
        let bare = build_node_tile(&n, &[], &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(bare.metric_source, MetricSource::Requests);
    }

    #[test]
    fn namespace_filter_drops_out_of_namespace_cities_keeps_terrain() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-0", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        // A workload in another namespace whose pod also lands on n1.
        s.deployment(fx::deployment("kube-system", "coredns", 1, 1));
        s.replicaset(fx::replicaset("kube-system", "coredns-rs", "coredns"));
        s.pod(fx::pod_owned(
            fx::pod("kube-system", "coredns-rs-0", Some("n1")),
            "ReplicaSet",
            "coredns-rs",
        ));

        let all = Models::build(&world);
        let all_names: Vec<&str> = all.world.cities().map(|c| c.r.name.as_str()).collect();
        assert!(all_names.contains(&"web") && all_names.contains(&"coredns"));

        let demo = Models::build_filtered(&world, &NamespaceFilter::only("demo"));
        let names: Vec<&str> = demo.world.cities().map(|c| c.r.name.as_str()).collect();
        assert!(names.contains(&"web"), "in-ns city missing: {names:?}");
        assert!(
            !names.contains(&"coredns"),
            "out-of-ns city leaked onto the map: {names:?}"
        );
        // Terrain is physical: the node still reports both pods.
        assert_eq!(demo.map.total_nodes, 1);
        assert_eq!(
            demo.map.total_pods, 2,
            "terrain census should be unfiltered"
        );
    }

    #[test]
    fn pod_usage_flows_into_city_and_node_models() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-7d4b", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-1", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        // Seed metrics-server pod usage.
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
            g.pods.insert(
                ("demo".into(), "web-7d4b-1".into()),
                NodeUsage {
                    cpu: 0.05,
                    mem: 64.0 * 1024.0 * 1024.0,
                },
            );
        }
        let r = WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        };
        let city = build_city(&world, &r).expect("city");
        let cu = city.pods[0].usage.expect("city pod usage");
        assert!((cu.cpu - 0.05).abs() < 1e-9);

        let node = build_node_detail(&world, "n1").expect("node");
        let nu = node.pods[0].usage.expect("node pod usage");
        assert!((nu.mem - 64.0 * 1024.0 * 1024.0).abs() < 1.0);

        // When metrics-server is unavailable, usage is None.
        world.metrics.lock().unwrap().available = false;
        assert!(build_city(&world, &r).unwrap().pods[0].usage.is_none());
    }

    // --- A0: per-pod resources + the two-ratio split -----------------------

    /// Seed one node, one pod, and optionally metrics for it.
    fn a0_world(pod: Pod, usage: Option<NodeUsage>) -> ObservedWorld {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let (ns, name) = (
            pod.metadata.namespace.clone().unwrap_or_default(),
            pod.metadata.name.clone().unwrap_or_default(),
        );
        s.pod(pod);
        if let Some(u) = usage {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
            g.pods.insert((ns, name), u);
        }
        world
    }

    fn a0_tile(world: &ObservedWorld) -> NodeTile {
        build_map(world)
            .zones
            .into_iter()
            .flat_map(|z| z.nodes)
            .find(|t| t.name == "n1")
            .expect("the node tile")
    }

    /// §2's distinction, and the one place a plausible-but-wrong number could
    /// enter: `PodGlyph.requests` must be the LITERAL declared request, never
    /// cost's reserved view (which defaults request:=limit).
    ///
    /// NOTE the realism caveat: a live API server *defaults* requests to limits
    /// at admission, so a stored pod with limits and no requests does not occur
    /// on a real cluster — verified against one. This fixture is therefore
    /// pinning the two summing primitives apart, not reproducing a cluster
    /// state. It still matters: the two functions must not be interchanged.
    #[test]
    fn pod_requests_are_literal_not_cost_reserved() {
        let pod = fx::pod_requests_limits(
            fx::pod("demo", "only-limits", Some("n1")),
            "",
            "",
            "250m",
            "64Mi",
        );
        // The two primitives disagree, which is the whole point of having both.
        assert_eq!(
            sum_pod_requests(&pod),
            (0.0, 0.0),
            "literal request is unset"
        );
        let (rc, rm) = sum_pod_reserved(&pod);
        assert!(
            rc > 0.0 && rm > 0.0,
            "cost still sees the limit as reserved"
        );

        let glyph = a0_tile(&a0_world(pod, None)).pods.remove(0);
        assert_eq!(
            glyph.requests,
            PodResources { cpu: 0.0, mem: 0.0 },
            "the map must carry the literal request, not the reservation"
        );
        assert!(glyph.limits.cpu > 0.0 && glyph.limits.mem > 0.0);
    }

    /// A pod with no metrics is UNKNOWN, not idle. `Some(0.0)` would read as an
    /// idle pod and paint an unearned all-clear.
    #[test]
    fn usage_is_none_without_metrics_never_zero() {
        let world = a0_world(fx::pod("demo", "p", Some("n1")), None);
        let tile = a0_tile(&world);
        assert!(tile.pods[0].usage.is_none(), "no metrics ⇒ unknown");
        assert!(tile.cpu_usage_ratio.is_none() && tile.mem_usage_ratio.is_none());
        // ...while the request ratio is always available (scheduler-visible).
        assert_eq!(tile.metric_source, MetricSource::Requests);
    }

    /// metrics-server present but omitting one pod: that pod alone is unknown.
    #[test]
    fn a_pod_omitted_by_metrics_is_none_while_its_neighbour_is_some() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod("demo", "measured", Some("n1")));
        s.pod(fx::pod("demo", "omitted", Some("n1")));
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
            g.pods.insert(
                ("demo".into(), "measured".into()),
                NodeUsage {
                    cpu: 0.25,
                    mem: 128.0 * 1024.0 * 1024.0,
                },
            );
        }
        let tile = a0_tile(&world);
        let get = |n: &str| tile.pods.iter().find(|p| p.name == n).expect("pod").clone();
        assert_eq!(
            get("measured").usage.expect("measured").cpu,
            0.25,
            "the reported pod carries its usage"
        );
        assert!(
            get("omitted").usage.is_none(),
            "its neighbour is unknown, not zero"
        );
    }

    /// Both ratios present together — the thing that was previously
    /// inexpressible. A node can be lightly requested and heavily used (OOM
    /// risk) or the reverse (waste); one polymorphic number can show neither.
    #[test]
    fn node_carries_request_and_usage_ratios_at_the_same_time() {
        // 4 cpu / 8Gi allocatable; request 1 cpu, use 2.
        let pod = fx::pod_requests(fx::pod("demo", "p", Some("n1")), "1", "1Gi");
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(pod);
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
            g.nodes.insert(
                "n1".into(),
                NodeUsage {
                    cpu: 2.0,
                    mem: 2.0 * 1024.0 * 1024.0 * 1024.0,
                },
            );
        }
        let tile = a0_tile(&world);
        assert!(
            (tile.cpu_request_ratio.expect("cpu request ratio") - 0.25).abs() < 1e-9,
            "1 of 4 cores requested, computed even though metrics are up: {}",
            tile.cpu_request_ratio.unwrap_or(f64::NAN)
        );
        assert!(
            (tile.cpu_usage_ratio.expect("usage") - 0.5).abs() < 1e-9,
            "2 of 4 cores used"
        );
        // The 2×2 is now expressible: usage exceeds requests on this node.
        assert!(tile.cpu_usage_ratio.unwrap() > tile.cpu_request_ratio.unwrap());
    }

    /// MIGRATION SAFETY: the retained polymorphic pair must return exactly what
    /// it did before under BOTH metric_source values.
    #[test]
    fn legacy_ratios_still_derive_the_old_way() {
        // Usage present ⇒ cpu_ratio IS the usage ratio.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod_requests(
            fx::pod("demo", "p", Some("n1")),
            "1",
            "1Gi",
        ));
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
            g.nodes.insert(
                "n1".into(),
                NodeUsage {
                    cpu: 2.0,
                    mem: 4.0 * 1024.0 * 1024.0 * 1024.0,
                },
            );
        }
        let t = a0_tile(&world);
        assert_eq!(t.metric_source, MetricSource::Usage);
        assert_eq!(t.cpu_ratio, t.cpu_usage_ratio);
        assert_eq!(t.mem_ratio, t.mem_usage_ratio);

        // Usage absent ⇒ cpu_ratio IS the request ratio.
        let bare = a0_tile(&a0_world(
            fx::pod_requests(fx::pod("demo", "p", Some("n1")), "1", "1Gi"),
            None,
        ));
        assert_eq!(bare.metric_source, MetricSource::Requests);
        assert_eq!(bare.cpu_ratio, bare.cpu_request_ratio);
        assert_eq!(bare.mem_ratio, bare.mem_request_ratio);
    }

    /// Native sidecars are counted (they hold a reservation for the pod's life);
    /// run-to-completion init containers are not. Guards behaviour
    /// `sum_pod_requests` already gets right against a reimplementation losing it.
    #[test]
    fn sidecars_count_toward_requests_and_container_count() {
        let mut plain_init = fx::pod_requests(fx::pod("demo", "p", Some("n1")), "100m", "64Mi");
        plain_init.spec.as_mut().unwrap().init_containers = Some(vec![Container {
            name: "migrate".into(), // no restartPolicy ⇒ run-to-completion
            resources: Some(k8s_openapi::api::core::v1::ResourceRequirements {
                requests: Some(fx::quantities(&[("cpu", "900m"), ("memory", "1Gi")])),
                ..Default::default()
            }),
            ..Default::default()
        }]);
        let g = a0_tile(&a0_world(plain_init, None)).pods.remove(0);
        assert!(
            (g.requests.cpu - 0.1).abs() < 1e-9,
            "a run-to-completion init container is excluded: {}",
            g.requests.cpu
        );
        assert_eq!(g.containers, 1, "and not counted");

        let side = fx::pod_native_sidecar(
            fx::pod_requests(fx::pod("demo", "p2", Some("n1")), "100m", "64Mi"),
            "50m",
            "32Mi",
        );
        let g = a0_tile(&a0_world(side, None)).pods.remove(0);
        assert!(
            (g.requests.cpu - 0.15).abs() < 1e-9,
            "a native sidecar IS included: {}",
            g.requests.cpu
        );
        assert_eq!(g.containers, 2, "and counted");
    }

    /// The map must carry the AUTHORITATIVE per-pod class, not the advisor's
    /// totals-based approximation. A fully-specified container beside an
    /// unspecified sidecar is Burstable (verified against a live API server);
    /// summing first would call it Guaranteed and misstate eviction order —
    /// which the plan renders as building material, so it would be visibly wrong.
    #[test]
    fn pod_glyph_qos_is_per_container_not_summed() {
        let mut pod = fx::pod_requests_limits(
            fx::pod("demo", "uneven", Some("n1")),
            "100m",
            "64Mi",
            "100m",
            "64Mi",
        );
        pod.spec.as_mut().unwrap().containers.push(Container {
            name: "sidecar".into(), // specifies nothing
            ..Default::default()
        });
        // The totals look Guaranteed...
        let (rc, rm) = sum_pod_requests(&pod);
        let (lc, lm) = sum_pod_limits(&pod);
        assert_eq!(
            crate::state::qos::qos_from_totals(rc, lc, rm, lm),
            QosClass::Guaranteed,
            "the approximation's blind spot"
        );
        // ...but the pod is Burstable, and that is what the map must show.
        assert_eq!(
            a0_tile(&a0_world(pod, None)).pods.remove(0).qos,
            QosClass::Burstable
        );
    }

    /// CENSUS vs LOAD, pinned. The glyph list includes terminal pods; the node's
    /// request ratio excludes them. Both are right and they are not comparable —
    /// summing glyphs to get occupancy would read 150% of a half-claimed node.
    /// Documented on `PodGlyph.requests`; asserted here so the difference is a
    /// decision rather than an accident.
    #[test]
    fn glyph_requests_are_a_census_the_node_ratio_is_scheduling_load() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a"))); // 4 cores allocatable
        s.pod(fx::pod_requests(
            fx::pod("demo", "live", Some("n1")),
            "1",
            "1Gi",
        ));
        // A completed Job pod that once requested the whole node.
        s.pod(fx::pod_phase(
            fx::pod_requests(fx::pod("demo", "done", Some("n1")), "4", "4Gi"),
            "Succeeded",
        ));
        let tile = a0_tile(&world);
        assert!(
            (tile.cpu_request_ratio.expect("cpu request ratio") - 0.25).abs() < 1e-9,
            "load excludes the terminal pod: {}",
            tile.cpu_request_ratio.unwrap_or(f64::NAN)
        );
        let census: f64 = tile.pods.iter().map(|g| g.requests.cpu).sum();
        assert!(
            (census - 5.0).abs() < 1e-9,
            "census includes it, and exceeds allocatable: {census}"
        );
        assert_eq!(tile.pods.len(), 2, "the terminal pod is still drawn");
    }

    /// `build_node_detail`'s tile must carry per-pod usage too. §7 names
    /// `detail.tile.pods[*].usage` as this phase's first unlocked consumer, and
    /// it is filled through a DIFFERENT closure than the map's — so without this
    /// the province window would show "unknown" for every pod on a
    /// metrics-server cluster, an unearned degrade-dark.
    #[test]
    fn node_detail_tile_glyphs_carry_usage() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod("demo", "p", Some("n1")));
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
            g.pods.insert(
                ("demo".into(), "p".into()),
                NodeUsage {
                    cpu: 0.25,
                    mem: 32.0 * 1024.0 * 1024.0,
                },
            );
        }
        let detail = build_node_detail(&world, "n1").expect("node detail");
        let u = detail.tile.pods[0]
            .usage
            .expect("the tile's glyph must carry usage, not just NodePodRow");
        assert!((u.cpu - 0.25).abs() < 1e-9);
    }

    // --- Unmeasurable capacity must not read as idle -----------------------

    /// Build a node with only the allocatable keys named in `keys`.
    fn node_alloc(name: &str, keys: &[(&str, &str)]) -> Node {
        let mut n = fx::node(name, Some("z-a"));
        if keys.is_empty() {
            n.status.as_mut().unwrap().allocatable = None;
        } else {
            n.status.as_mut().unwrap().allocatable = Some(fx::quantities(keys));
        }
        n
    }

    /// THE DISCRIMINATION TEST, and the point of the whole change: a node that
    /// cannot be measured and a node that is genuinely empty must not produce
    /// the same number.
    #[test]
    fn an_unmeasurable_node_is_distinguishable_from_an_idle_one() {
        let idle = node_alloc("idle", &[("cpu", "4"), ("memory", "8Gi")]);
        let unknown = node_alloc("unknown", &[]);

        // Idle: allocatable present, nothing requested ⇒ a real zero.
        assert_eq!(node_request_ratios(&idle, &[]), (Some(0.0), Some(0.0)));
        // Unmeasurable: no denominator ⇒ no ratio at all.
        assert_eq!(node_request_ratios(&unknown, &[]), (None, None));
        assert_ne!(
            node_request_ratios(&idle, &[]),
            node_request_ratios(&unknown, &[]),
            "these are the two states the old 0.0 collapsed into one"
        );

        // Same through the usage helper, and same through the tile.
        let u = NodeUsage { cpu: 0.0, mem: 0.0 };
        assert_eq!(node_usage_ratios(&idle, u), (Some(0.0), Some(0.0)));
        assert_eq!(node_usage_ratios(&unknown, u), (None, None));

        let t_idle = build_node_tile(&idle, &[], &OwnerIndex::default(), None, &|_, _| None);
        let t_unk = build_node_tile(&unknown, &[], &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(t_idle.cpu_ratio, Some(0.0));
        assert_eq!(t_unk.cpu_ratio, None);
    }

    /// Per-resource, not per-node: cpu and memory are separate allocatable keys
    /// and one can be present without the other.
    #[test]
    fn allocatable_is_optional_per_resource() {
        let n = node_alloc("half", &[("cpu", "4")]); // memory absent
        let (c, m) = node_request_ratios(&n, &[]);
        assert_eq!((c, m), (Some(0.0), None));

        let t = build_node_tile(&n, &[], &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(t.cpu_request_ratio, Some(0.0));
        assert_eq!(t.mem_request_ratio, None);
    }

    /// An unknown ratio is not pressure — and equally not a guarantee of health.
    /// The fabricated 0.0 quietly asserted the latter.
    #[test]
    fn an_unmeasurable_node_is_not_reported_as_pressured() {
        let t = build_node_tile(
            &node_alloc("unknown", &[]),
            &[],
            &OwnerIndex::default(),
            None,
            &|_, _| None,
        );
        assert_eq!(t.health, NodeHealth::Healthy, "not Pressure on a guess");
        // ...and saturation OMITS the dims it cannot compute, rather than
        // reporting a calm 0% — same discipline it already applies to pod-count.
        assert!(
            t.saturation.dims.is_empty(),
            "an unmeasurable node has no strain dimensions: {:?}",
            t.saturation.dims
        );
        assert!(t.saturation.pod_ratio().is_none());
    }

    /// Nodes that DO report allocatable must behave exactly as before.
    #[test]
    fn measurable_nodes_are_unchanged() {
        let n = node_alloc("n", &[("cpu", "4"), ("memory", "8Gi")]);
        let pod = fx::pod_requests(fx::pod("demo", "p", Some("n")), "1", "2Gi");
        let (c, m) = node_request_ratios(&n, &[&pod]);
        assert_eq!(c, Some(0.25));
        assert_eq!(m, Some(0.25));
        let t = build_node_tile(&n, &[&pod], &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(t.cpu_ratio, Some(0.25));
        assert_eq!(t.metric_source, MetricSource::Requests);
        assert_eq!(t.saturation.dims.len(), 2, "cpu + mem, no pods key");
    }

    /// ANTI-DRIFT: the map and the advisor must classify the same pod the same
    /// way where they are asking the same question — a single-container pod, the
    /// only granularity at which the advisor's totals-based view is exact.
    /// (`state::qos` pins the case where they legitimately differ.)
    #[test]
    fn map_and_advisor_agree_on_qos_for_the_same_pod() {
        for (cr, mr, cl, ml, expect) in [
            ("100m", "64Mi", "100m", "64Mi", QosClass::Guaranteed),
            ("100m", "64Mi", "", "", QosClass::Burstable),
            ("", "", "", "", QosClass::BestEffort),
        ] {
            let pod = fx::pod_requests_limits(fx::pod("demo", "p", Some("n1")), cr, mr, cl, ml);
            let (rc, rm) = sum_pod_requests(&pod);
            let (lc, lm) = sum_pod_limits(&pod);
            let advisor = crate::state::qos::qos_from_totals(rc, lc, rm, lm);
            let map = a0_tile(&a0_world(pod, None)).pods.remove(0).qos;
            assert_eq!(map, expect);
            assert_eq!(map, advisor, "map and advisor must not disagree");
        }
    }

    #[test]
    fn node_allocatable_parses_pods() {
        let n = fx::node("n", None); // fixture has cpu/memory, no "pods"
        assert_eq!(node_allocatable(&n, "cpu"), Some(4.0));
        assert_eq!(node_allocatable(&n, "pods"), None);
        let mut n2 = fx::node("n2", None);
        n2.status.as_mut().unwrap().allocatable = Some(fx::quantities(&[
            ("cpu", "4"),
            ("memory", "8Gi"),
            ("pods", "110"),
        ]));
        assert_eq!(node_allocatable(&n2, "pods"), Some(110.0));
    }

    #[test]
    fn saturation_flows_into_node_tile() {
        use super::saturation::{SatDimKind, SatLevel};
        let mut n = fx::node("n", None);
        n.status.as_mut().unwrap().allocatable = Some(fx::quantities(&[
            ("cpu", "4"),
            ("memory", "8Gi"),
            ("pods", "10"),
        ]));
        // 10 running + 2 terminal (excluded) → 10/10 non-terminal = High.
        let mut pods: Vec<Pod> = (0..10)
            .map(|i| fx::pod("d", &format!("p{i}"), Some("n")))
            .collect();
        pods.push(fx::pod_phase(fx::pod("d", "ok", Some("n")), "Succeeded"));
        pods.push(fx::pod_phase(fx::pod("d", "bad", Some("n")), "Failed"));
        let refs: Vec<&Pod> = pods.iter().collect();
        let tile = build_node_tile(&n, &refs, &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(
            tile.saturation.pod_ratio(),
            Some(1.0),
            "terminal pods excluded"
        );
        assert_eq!(tile.saturation.worst, SatLevel::High);
        assert_eq!(tile.saturation.worst_dim().unwrap().0, SatDimKind::Pods);

        // A node without allocatable["pods"] omits the pod dimension entirely.
        let bare = build_node_tile(
            &fx::node("b", None),
            &refs,
            &OwnerIndex::default(),
            None,
            &|_, _| None,
        );
        assert_eq!(bare.saturation.pod_ratio(), None);
    }

    #[test]
    fn node_health_precedence() {
        let not_ready = fx::node_with_condition(fx::node("n1", None), "Ready", "False");
        assert_eq!(
            build_node_tile(&not_ready, &[], &OwnerIndex::default(), None, &|_, _| None).health,
            NodeHealth::NotReady
        );

        // NotReady outranks cordon.
        let both = fx::cordoned(fx::node_with_condition(
            fx::node("n2", None),
            "Ready",
            "False",
        ));
        assert_eq!(
            build_node_tile(&both, &[], &OwnerIndex::default(), None, &|_, _| None).health,
            NodeHealth::NotReady
        );

        let cordoned = fx::cordoned(fx::node("n3", None));
        let t = build_node_tile(&cordoned, &[], &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(t.health, NodeHealth::Cordoned);
        assert!(t.cordoned);

        let pressured = fx::node_with_condition(fx::node("n4", None), "MemoryPressure", "True");
        let t = build_node_tile(&pressured, &[], &OwnerIndex::default(), None, &|_, _| None);
        assert_eq!(t.health, NodeHealth::Pressure);
        assert_eq!(t.abnormal, vec!["Mem"]);
    }

    #[test]
    fn pod_state_classification() {
        let crash = fx::pod_waiting(fx::pod("d", "p", Some("n")), "CrashLoopBackOff");
        assert_eq!(
            pod_state(&crash),
            (PodState::Failing, "CrashLoopBackOff".to_string())
        );

        let unsched = fx::pod_unschedulable(fx::pod("d", "p", None));
        assert_eq!(
            pod_state(&unsched),
            (PodState::Pending, "Unschedulable".to_string())
        );

        let mut terminating = fx::pod("d", "p", Some("n"));
        terminating.metadata.deletion_timestamp = Some(Time(jiff::Timestamp::now()));
        assert_eq!(pod_state(&terminating).0, PodState::Terminating);

        let ok = fx::pod("d", "p", Some("n"));
        assert_eq!(pod_state(&ok), (PodState::Ok, "Running".to_string()));
    }

    #[test]
    fn deployment_rollout_states() {
        let complete = fx::deployment("d", "web", 3, 3);
        assert_eq!(deployment_status(&complete).0, RolloutStatus::Complete);

        let mut updating = fx::deployment("d", "web", 3, 1);
        updating.status.as_mut().unwrap().updated_replicas = Some(1);
        let (st, note) = deployment_status(&updating);
        assert_eq!(st, RolloutStatus::Progressing);
        assert_eq!(note, "updating 1/3");

        let mut stalled = fx::deployment("d", "web", 3, 1);
        stalled.status.as_mut().unwrap().conditions =
            Some(vec![k8s_openapi::api::apps::v1::DeploymentCondition {
                type_: "Progressing".into(),
                status: "False".into(),
                reason: Some("ProgressDeadlineExceeded".into()),
                ..Default::default()
            }]);
        assert_eq!(deployment_status(&stalled).0, RolloutStatus::Stalled);
    }

    #[test]
    fn daemonset_rollout_states() {
        let complete = fx::daemonset("d", "agent", 3, 3);
        assert_eq!(daemonset_status(&complete).0, RolloutStatus::Complete);
        let lagging = fx::daemonset("d", "agent", 3, 1);
        let (st, note) = daemonset_status(&lagging);
        assert_eq!(st, RolloutStatus::Progressing);
        assert_eq!(note, "ready 1/3");
    }

    #[test]
    fn city_assembles_pods_services_and_ownership() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 2));
        s.replicaset(fx::replicaset("demo", "web-7d4b", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-1", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-2", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        // Decoy pod from another workload.
        s.pod(fx::pod("demo", "other", Some("n1")));
        s.service(fx::service("demo", "web", &[("app", "web")]));
        s.service(fx::service("demo", "unrelated", &[("app", "nope")]));
        // An ingress routing to web → a gate on its city screen; one routing
        // to the unrelated service must not appear.
        s.ingress(fx::ingress("demo", "web-ing", "web.example.com", "web"));
        s.ingress(fx::ingress(
            "demo",
            "other-ing",
            "other.example.com",
            "unrelated",
        ));

        let r = WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        };
        let city = build_city(&world, &r).expect("city");
        assert_eq!(city.desired, 2);
        assert_eq!(city.pods.len(), 2);
        assert!(city.pods.iter().all(|p| p.name.starts_with("web-7d4b-")));
        let svcs: Vec<&str> = city
            .owned
            .iter()
            .filter(|o| o.kind == "svc")
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(svcs, ["web"]);
        let ings: Vec<&str> = city
            .owned
            .iter()
            .filter(|o| o.kind == "ing")
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(ings, ["web-ing"], "only the web ingress is a gate here");
        assert_eq!(city.status, RolloutStatus::Complete);
    }

    #[test]
    fn node_detail_resolves_pod_owners() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-7d4b", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-1", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        let detail = build_node_detail(&world, "n1").expect("detail");
        assert_eq!(detail.pods.len(), 1);
        let owner = detail.pods[0].owner.as_ref().expect("owner");
        assert_eq!(owner.name, "web");
        assert_eq!(owner.kind, WorkloadKind::Deployment);
        assert!(detail.info.iter().any(|(k, _)| *k == "runtime"));
    }

    #[test]
    fn prefer_previous_for_crashloops_only() {
        // A crash-looping pod → previous (its live container is in backoff).
        assert!(prefer_previous(PodState::Failing, "CrashLoopBackOff", 3));
        // High restarts even without the exact reason.
        assert!(prefer_previous(PodState::Failing, "Error", 5));
        // A healthy / pending / low-restart pod → live tail.
        assert!(!prefer_previous(PodState::Ok, "Running", 0));
        assert!(!prefer_previous(PodState::Pending, "ContainerCreating", 0));
        assert!(!prefer_previous(PodState::Failing, "Error", 1));
    }

    /// The frame budget (criterion 6): a synthetic **500-node / 5000-pod** world
    /// (a large real cluster) must rebuild the full `Models` (map + workloads +
    /// attention — what the GUI recomputes each tick) well under the 100ms tick
    /// budget. Asserted in release (`make perf-test`); debug just times it. Run via
    /// `cargo test --release scale_rebuild`.
    #[test]
    fn scale_rebuild_within_budget() {
        use std::time::Instant;
        let (world, mut s) = fx::world();
        // 500 nodes across 25 zones.
        for z in 0..25 {
            for i in 0..20 {
                s.node(fx::node(&format!("node-{z}-{i}"), Some(&format!("z-{z}"))));
            }
        }
        // 100 workloads × 50 pods = 5000 pods.
        for w in 0..100 {
            let name = format!("app-{w}");
            s.deployment(fx::deployment("demo", &name, 50, 50));
            let rs = format!("{name}-rs");
            s.replicaset(fx::replicaset("demo", &rs, &name));
            for p in 0..50 {
                let node = format!("node-{}-{}", w % 25, p % 20);
                s.pod(fx::pod_owned(
                    fx::pod("demo", &format!("{name}-{p}"), Some(&node)),
                    "ReplicaSet",
                    &rs,
                ));
            }
        }

        let _ = Models::build(&world); // warm caches
        let iters = 5;
        let start = Instant::now();
        for _ in 0..iters {
            let m = Models::build(&world);
            assert!(m.map.total_nodes >= 500 && m.map.total_pods >= 5000);
        }
        let per = start.elapsed() / iters;
        println!("scale_rebuild: 500 nodes / 5000 pods → {per:?}/rebuild");
        if !cfg!(debug_assertions) {
            assert!(
                per.as_millis() < 100,
                "rebuild over the 100ms budget: {per:?}"
            );
        }
    }
}
