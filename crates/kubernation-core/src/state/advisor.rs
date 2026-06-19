//! Advisor reports — the classic-4X "advisors" (Civ's Berater / F1 screens)
//! reframed for Kubernetes. Each is a **pure function of `ObservedWorld`**
//! (no I/O, unit-testable without a cluster), summarizing one facet of the
//! realm: Health (state of the realm), Storage (granaries), Network (harbors &
//! gates). They are cluster-wide reports — deliberately *not* scoped by the
//! map's namespace filter, since an advisor reports on the whole realm.
//!
//! These complement the attention queue (which surfaces *what needs orders*);
//! the advisors give the at-a-glance rollups.

use std::collections::HashMap;

use crate::k8s::quantity;
use crate::state::model::{
    NodeHealth, OwnerIndex, PodState, WorkloadKind, WorkloadRef, build_map, build_workloads,
    ingress_backends, pod_state, sum_pod_limits, sum_pod_requests,
};
use crate::state::observed::ObservedWorld;

/// State-of-the-realm rollup: node health, pod phases, workload strength.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthReport {
    pub nodes_total: usize,
    pub nodes_healthy: usize,
    pub nodes_cordoned: usize,
    pub nodes_pressure: usize,
    pub nodes_notready: usize,
    pub pods_total: usize,
    pub pods_running: usize,
    pub pods_starting: usize,
    pub pods_pending: usize,
    pub pods_terminating: usize,
    pub pods_failing: usize,
    pub pods_succeeded: usize,
    pub workloads_total: usize,
    /// Workloads with fewer ready than desired replicas (understrength).
    pub workloads_degraded: usize,
    /// Whether the node gauges reflect live metrics-server usage.
    pub metrics_live: bool,
}

/// One persistent-volume claim, for the Storage advisor's trouble list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub namespace: String,
    pub name: String,
    pub phase: String,
}

/// Persistent storage rollup: granaries bound vs. pending.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageReport {
    pub total: usize,
    pub bound: usize,
    pub pending: usize,
    /// The not-Bound claims (sorted by namespace/name), the trouble list.
    pub pending_claims: Vec<ClaimRow>,
}

/// One connectivity route flagged as trouble (orphan ingress / idle service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRow {
    pub namespace: String,
    pub name: String,
    pub detail: String,
}

/// Connectivity rollup: harbors (services) and gates (ingresses), plus routes
/// that lead nowhere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkReport {
    pub services: usize,
    pub ingresses: usize,
    /// Ingresses whose backend Service is absent (a gate to nowhere).
    pub orphan_ingresses: Vec<RouteRow>,
    /// Services whose selector matches no pod (a harbor with no city).
    pub idle_services: Vec<RouteRow>,
}

/// Build the Health advisor report (cluster-wide).
pub fn health_report(world: &ObservedWorld) -> HealthReport {
    let map = build_map(world);
    let mut r = HealthReport {
        nodes_total: map.total_nodes,
        pods_total: map.total_pods,
        metrics_live: map.metrics_live,
        ..Default::default()
    };
    for zone in &map.zones {
        for n in &zone.nodes {
            match n.health {
                NodeHealth::Healthy => r.nodes_healthy += 1,
                NodeHealth::Cordoned => r.nodes_cordoned += 1,
                NodeHealth::Pressure => r.nodes_pressure += 1,
                NodeHealth::NotReady => r.nodes_notready += 1,
            }
        }
    }
    for p in world.pods.state() {
        match pod_state(&p).0 {
            PodState::Ok => r.pods_running += 1,
            PodState::Starting => r.pods_starting += 1,
            PodState::Pending => r.pods_pending += 1,
            PodState::Terminating => r.pods_terminating += 1,
            PodState::Failing => r.pods_failing += 1,
            PodState::Succeeded => r.pods_succeeded += 1,
        }
    }
    let workloads = build_workloads(world);
    r.workloads_total = workloads.len();
    r.workloads_degraded = workloads.iter().filter(|w| w.ready < w.desired).count();
    r
}

/// Build the Storage advisor report (cluster-wide).
pub fn storage_report(world: &ObservedWorld) -> StorageReport {
    let mut r = StorageReport::default();
    for pvc in world.pvcs.state() {
        r.total += 1;
        let phase = pvc
            .status
            .as_ref()
            .and_then(|s| s.phase.clone())
            .unwrap_or_else(|| "Unknown".into());
        if phase == "Bound" {
            r.bound += 1;
        } else {
            r.pending += 1;
            r.pending_claims.push(ClaimRow {
                namespace: pvc.metadata.namespace.clone().unwrap_or_default(),
                name: pvc.metadata.name.clone().unwrap_or_default(),
                phase,
            });
        }
    }
    r.pending_claims
        .sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
    r
}

/// Build the Network advisor report (cluster-wide).
pub fn network_report(world: &ObservedWorld) -> NetworkReport {
    let services = world.services.state();
    let ingresses = world.ingresses.state();
    let mut r = NetworkReport {
        services: services.len(),
        ingresses: ingresses.len(),
        ..Default::default()
    };

    // Orphan ingress: a backend Service name absent in the ingress's namespace.
    let svc_names: std::collections::HashSet<(String, String)> = services
        .iter()
        .filter_map(|s| Some((s.metadata.namespace.clone()?, s.metadata.name.clone()?)))
        .collect();
    for ing in &ingresses {
        let ns = ing.metadata.namespace.clone().unwrap_or_default();
        let name = ing.metadata.name.clone().unwrap_or_default();
        let mut missing: Vec<String> = ingress_backends(ing)
            .into_iter()
            .filter(|b| !svc_names.contains(&(ns.clone(), b.clone())))
            .collect();
        if missing.is_empty() {
            continue;
        }
        missing.sort();
        r.orphan_ingresses.push(RouteRow {
            namespace: ns,
            name,
            detail: format!("backend {} has no service", missing.join(", ")),
        });
    }

    // Idle service: a non-empty selector matching no pod (headless / external
    // services with no selector are skipped).
    let pods = world.pods.state();
    for svc in &services {
        let ns = svc.metadata.namespace.clone().unwrap_or_default();
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
        if !has_endpoint {
            r.idle_services.push(RouteRow {
                namespace: ns,
                name,
                detail: "selects no pods".into(),
            });
        }
    }
    r.orphan_ingresses
        .sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
    r.idle_services
        .sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
    r
}

// ---------------------------------------------------------------------------
// Right-sizing advisor: per-workload requests vs metrics-server usage.
//
// Compares each workload's per-replica resource *requests* against actual
// *usage* (latest metrics-server sample, summed per pod, averaged over the pods
// that reported) and flags over-provisioning (waste), under-provisioning
// (throttle/OOM risk) and unrequested (scheduler-blind) workloads, with a
// directional suggested request. PURE; degrades dark without metrics-server
// (only the spec-derived "unrequested" findings survive). Single-sample —
// honest, not a multi-day VPA fit; the struct is shaped so a future per-pod
// history ring only changes how usage/peak are derived.

// Classification thresholds (test-pinned, like model.rs PRESSURE buckets).
const CPU_FLOOR: f64 = 0.010; // 10m cores — don't nag on trivially small cpu
const MEM_FLOOR: f64 = 16.0 * 1024.0 * 1024.0; // 16 MiB
const OVER_RATIO: f64 = 0.50; // usage/request below this → Over (waste)
const UNDER_RATIO_CPU: f64 = 0.90; // usage/request at/above → Under (throttle)
const UNDER_RATIO_MEM: f64 = 0.80; // mem stricter (incompressible → OOM)
const LIMIT_RATIO_CPU: f64 = 0.80; // usage/limit above → throttle escalation note
const LIMIT_RATIO_MEM: f64 = 0.85; // usage/limit above → OOM escalation note
// Recommendation: target request = usage ÷ target-utilization (headroom).
const TARGET_UTIL_CPU: f64 = 0.65;
const TARGET_UTIL_MEM: f64 = 0.50; // more headroom — memory is incompressible
const CPU_MIN: f64 = 0.025; // VPA PodMinCPUMillicores = 25m
const MEM_MIN: f64 = 250.0 * 1024.0 * 1024.0; // VPA PodMinMemoryMb = 250Mi
const CPU_STEP: f64 = 0.010; // round suggestions up to 10m
const MEM_STEP: f64 = 16.0 * 1024.0 * 1024.0; // round suggestions up to 16Mi
// Headroom over the *peak* replica when sizing memory up. A memory-Under fires
// when peak ≥ 0.80·request, so peak·(1+headroom) is always ≥ request — i.e. the
// recommended memory request for an Under is always a genuine *raise*, never a
// number below the current request.
const MEM_PEAK_HEADROOM: f64 = 0.25;

/// The right-sizing verdict for one (workload, resource).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RsVerdict {
    /// In the healthy band — dropped from the trouble rows.
    #[default]
    RightSized,
    /// Usage far below request (reclaimable waste).
    Over,
    /// Usage near/over request (CPU throttle / memory OOM risk).
    Under,
    /// No request declared though the workload runs (scheduler-blind).
    Unrequested,
    /// No usage sample this poll (excluded from rows — never a false Over).
    Unknown,
}

impl RsVerdict {
    fn rank(self) -> u8 {
        match self {
            RsVerdict::Unrequested => 4,
            RsVerdict::Under => 3,
            RsVerdict::Over => 2,
            RsVerdict::RightSized => 1,
            RsVerdict::Unknown => 0,
        }
    }
}

/// QoS class (informational — never changes the verdict bucket).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RsQos {
    #[default]
    Burstable,
    Guaranteed,
    BestEffort,
}

impl RsQos {
    pub fn label(self) -> &'static str {
        match self {
            RsQos::Guaranteed => "guaranteed",
            RsQos::Burstable => "burstable",
            RsQos::BestEffort => "besteffort",
        }
    }
}

/// One resource (cpu or mem) assessment for a workload (per-replica values;
/// cpu in cores, mem in bytes).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RsResource {
    pub request: f64,
    pub limit: f64,
    pub usage: f64, // mean over measured pods
    pub peak: f64,  // max single-pod usage (mem sizing)
    pub verdict: RsVerdict,
    pub suggested: Option<f64>, // per-replica target request; None when not actionable
    pub note: Option<&'static str>,
}

/// One workload's right-sizing row (at most cpu + mem).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RsRow {
    pub kind: WorkloadKind,
    pub namespace: String,
    pub name: String,
    pub qos: RsQos,
    pub measured_pods: usize,
    pub running_pods: usize,
    pub cpu: RsResource,
    pub mem: RsResource,
    pub worst: RsVerdict,
}

/// Cluster-wide right-sizing rollup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RightSizingReport {
    /// metrics-server reporting — false → degrade-dark (only `unrequested`).
    pub metrics_available: bool,
    pub workloads_total: usize,
    pub over: Vec<RsRow>,
    pub under: Vec<RsRow>,
    pub unrequested: Vec<RsRow>,
    pub right_sized_count: usize,
    /// Workloads not assessed into any bucket — scaled-to-zero (no running pods)
    /// or running but unsampled this poll (so the GUI's parts sum to total).
    pub unmeasured: usize,
    /// Σ over the `over` rows of (request − suggested) × measured_pods.
    pub reclaimable_cpu: f64, // cores
    pub reclaimable_mem: f64, // bytes
    /// reclaimable_cpu ÷ median node allocatable cpu (0.0 if unknown).
    pub node_equiv: f64,
}

#[derive(Default)]
struct Acc {
    kind: WorkloadKind,
    namespace: String,
    name: String,
    req_cpu: f64,
    req_mem: f64,
    lim_cpu: f64,
    lim_mem: f64,
    running: usize,
    measured: usize,
    usum_cpu: f64,
    usum_mem: f64,
    upeak_cpu: f64,
    upeak_mem: f64,
}

fn round_up(x: f64, step: f64) -> f64 {
    if step <= 0.0 {
        x
    } else {
        (x / step).ceil() * step
    }
}

fn suggest_cpu(u_mean: f64) -> f64 {
    round_up((u_mean / TARGET_UTIL_CPU).max(CPU_MIN), CPU_STEP)
}

fn suggest_mem(u_mean: f64, u_peak: f64) -> f64 {
    // Size for the hottest replica + headroom — memory is incompressible, so the
    // peak pod (not the mean) is what OOMs. `peak·(1+headroom)` also guarantees an
    // Under recommendation is always a raise above the request (peak ≥ 0.8·req).
    round_up(
        (u_mean / TARGET_UTIL_MEM)
            .max(u_peak * (1.0 + MEM_PEAK_HEADROOM))
            .max(MEM_MIN),
        MEM_STEP,
    )
}

fn derive_qos(req_cpu: f64, lim_cpu: f64, req_mem: f64, lim_mem: f64) -> RsQos {
    if req_cpu == 0.0 && req_mem == 0.0 && lim_cpu == 0.0 && lim_mem == 0.0 {
        return RsQos::BestEffort;
    }
    // Relative tolerance — absolute EPSILON is meaningless for byte-scale values.
    let eq = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.max(b).max(1.0);
    if req_cpu > 0.0 && req_mem > 0.0 && eq(req_cpu, lim_cpu) && eq(req_mem, lim_mem) {
        return RsQos::Guaranteed;
    }
    RsQos::Burstable
}

/// Classify one resource. `over_usage`/`under_usage` are per-replica usage:
/// over-provisioning is judged on the MEAN (waste is aggregate), but
/// under-provisioning on a per-resource signal — CPU on the mean (compressible,
/// recoverable throttle), MEMORY on the PEAK pod (incompressible → the hottest
/// replica OOMs regardless of the average). `has_usage` false → Unknown (never a
/// false Over). `req == 0` with running pods → Unrequested regardless of metrics
/// (a static, scheduler-blind fact — this is what survives degrade-dark).
fn resource_verdict(
    req: f64,
    over_usage: f64,
    under_usage: f64,
    has_usage: bool,
    floor: f64,
    under_ratio: f64,
) -> RsVerdict {
    if req <= 0.0 {
        return RsVerdict::Unrequested;
    }
    if !has_usage {
        return RsVerdict::Unknown;
    }
    if under_usage >= req * under_ratio && under_usage > floor {
        return RsVerdict::Under;
    }
    if over_usage < req * OVER_RATIO && (req - over_usage) > floor {
        return RsVerdict::Over;
    }
    RsVerdict::RightSized
}

fn assess(acc: &Acc) -> RsRow {
    let qos = derive_qos(acc.req_cpu, acc.lim_cpu, acc.req_mem, acc.lim_mem);
    let has_usage = acc.measured > 0;
    let n = acc.measured.max(1) as f64;
    let (u_cpu, u_mem) = (acc.usum_cpu / n, acc.usum_mem / n);

    let mut cpu = RsResource {
        request: acc.req_cpu,
        limit: acc.lim_cpu,
        usage: u_cpu,
        peak: acc.upeak_cpu,
        // CPU: mean for both over and under (compressible).
        verdict: resource_verdict(
            acc.req_cpu,
            u_cpu,
            u_cpu,
            has_usage,
            CPU_FLOOR,
            UNDER_RATIO_CPU,
        ),
        ..Default::default()
    };
    if has_usage
        && matches!(
            cpu.verdict,
            RsVerdict::Over | RsVerdict::Under | RsVerdict::Unrequested
        )
    {
        cpu.suggested = Some(suggest_cpu(u_cpu));
    }
    if cpu.verdict == RsVerdict::Under
        && acc.lim_cpu > 0.0
        && u_cpu >= acc.lim_cpu * LIMIT_RATIO_CPU
    {
        cpu.note = Some("CFS throttling likely");
    }

    let mut mem = RsResource {
        request: acc.req_mem,
        limit: acc.lim_mem,
        usage: u_mem,
        peak: acc.upeak_mem,
        // MEM: over on the mean (aggregate waste), under on the PEAK pod
        // (incompressible — the hottest replica OOMs, not the average).
        verdict: resource_verdict(
            acc.req_mem,
            u_mem,
            acc.upeak_mem,
            has_usage,
            MEM_FLOOR,
            UNDER_RATIO_MEM,
        ),
        ..Default::default()
    };
    if has_usage
        && matches!(
            mem.verdict,
            RsVerdict::Over | RsVerdict::Under | RsVerdict::Unrequested
        )
    {
        mem.suggested = Some(suggest_mem(u_mem, acc.upeak_mem));
    }
    if mem.verdict == RsVerdict::Under
        && acc.lim_mem > 0.0
        && u_mem >= acc.lim_mem * LIMIT_RATIO_MEM
    {
        mem.note = Some("OOMKill risk");
    }

    // Consistency guards so a verdict never contradicts its own suggestion:
    //  • Over whose "down-size" would *raise* the request (the VPA floor exceeds
    //    a tiny request) isn't waste — demote to RightSized (reclaimable already
    //    clamps at 0; this fixes the display).
    //  • Under whose recommended request isn't actually higher than the current
    //    one — and which isn't a limit-proximity risk (no escalation note) — is
    //    not actionably under-provisioned, so never show a "~raise" ≤ the request.
    for res in [&mut cpu, &mut mem] {
        match res.verdict {
            RsVerdict::Over if res.suggested.is_some_and(|s| s >= res.request) => {
                res.verdict = RsVerdict::RightSized;
                res.suggested = None;
                res.note = None;
            }
            RsVerdict::Under
                if res.note.is_none() && res.suggested.is_some_and(|s| s <= res.request) =>
            {
                res.verdict = RsVerdict::RightSized;
                res.suggested = None;
            }
            _ => {}
        }
    }

    let worst = if cpu.verdict.rank() >= mem.verdict.rank() {
        cpu.verdict
    } else {
        mem.verdict
    };
    // A waste-only row that is Guaranteed: lowering the request without lowering
    // the limit demotes QoS — surface that tradeoff on the over resource.
    if worst == RsVerdict::Over && qos == RsQos::Guaranteed {
        if cpu.verdict == RsVerdict::Over {
            cpu.note = Some("lowering request drops Guaranteed QoS");
        } else if mem.verdict == RsVerdict::Over {
            mem.note = Some("lowering request drops Guaranteed QoS");
        }
    }

    RsRow {
        kind: acc.kind,
        namespace: acc.namespace.clone(),
        name: acc.name.clone(),
        qos,
        measured_pods: acc.measured,
        running_pods: acc.running,
        cpu,
        mem,
        worst,
    }
}

/// Median node allocatable cpu (cores), for the reclaimable→node-equivalent
/// framing. 0.0 when no node reports allocatable.
fn median_node_alloc_cpu(world: &ObservedWorld) -> f64 {
    let mut v: Vec<f64> = world
        .nodes
        .state()
        .iter()
        .filter_map(|n| {
            n.status
                .as_ref()
                .and_then(|s| s.allocatable.as_ref())
                .and_then(|a| a.get("cpu"))
                .and_then(quantity::value)
        })
        .filter(|c| *c > 0.0)
        .collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Build the Right-sizing advisor report (cluster-wide).
pub fn rightsizing_report(world: &ObservedWorld) -> RightSizingReport {
    let metrics_available = world.metrics_available();
    let idx = OwnerIndex::build(world);
    let workloads = build_workloads(world);
    let mut report = RightSizingReport {
        metrics_available,
        workloads_total: workloads.len(),
        ..Default::default()
    };

    // Group running member pods by owning workload, summing requests/limits
    // (per-replica, via the running max — robust mid-rollout) and usage.
    let mut accs: HashMap<WorkloadRef, Acc> = HashMap::new();
    for p in world.pods.state() {
        let pod = p.as_ref();
        let phase = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("");
        if phase == "Succeeded" || phase == "Failed" {
            continue; // terminal pods don't reserve or use
        }
        let Some(wr) = idx.workload_of(pod) else {
            continue; // bare / Job pods — no city, same scope as build_workloads
        };
        let ns = pod.metadata.namespace.clone().unwrap_or_default();
        let name = pod.metadata.name.clone().unwrap_or_default();
        let (mut creq, mut mreq) = sum_pod_requests(pod);
        let (clim, mlim) = sum_pod_limits(pod);
        // k8s defaults request := limit when only a limit is set — so a
        // limits-only pod isn't mis-flagged as Unrequested.
        if creq == 0.0 && clim > 0.0 {
            creq = clim;
        }
        if mreq == 0.0 && mlim > 0.0 {
            mreq = mlim;
        }
        let acc = accs.entry(wr.clone()).or_default();
        acc.kind = wr.kind;
        acc.namespace = wr.namespace.clone();
        acc.name = wr.name.clone();
        acc.running += 1;
        acc.req_cpu = acc.req_cpu.max(creq);
        acc.req_mem = acc.req_mem.max(mreq);
        acc.lim_cpu = acc.lim_cpu.max(clim);
        acc.lim_mem = acc.lim_mem.max(mlim);
        // Only *Ready* (steady-state) pods inform the usage mean. A warming or
        // crash-looping replica reads ~0 and would drag the mean below the Over
        // threshold, flagging a healthy workload as waste — so it's excluded from
        // the measurement (it still counts toward running_pods for honesty).
        if matches!(pod_state(pod).0, PodState::Ok)
            && let Some(u) = world.pod_usage(&ns, &name)
        {
            acc.measured += 1;
            acc.usum_cpu += u.cpu;
            acc.usum_mem += u.mem;
            acc.upeak_cpu = acc.upeak_cpu.max(u.cpu);
            acc.upeak_mem = acc.upeak_mem.max(u.mem);
        }
    }

    // NodeMetrics can be up (so `metrics_available`) while PodMetrics is empty
    // (best-effort — it can fail independently). Without per-pod usage every
    // workload is Unknown, which would otherwise read as a false "all right-sized,
    // 0 reclaimable". Treat that as degrade-dark (show only scheduler-blind).
    let has_pod_metrics = accs.values().any(|a| a.measured > 0);
    report.metrics_available = metrics_available && has_pod_metrics;

    for acc in accs.values() {
        let row = assess(acc);
        // Reclaimable is summed per *resource* that's Over, independent of which
        // bucket `worst` puts the row in — so a row that's Under on memory but
        // Over on cpu still contributes its cpu saving to the headline.
        if row.cpu.verdict == RsVerdict::Over
            && let Some(s) = row.cpu.suggested
        {
            report.reclaimable_cpu += (row.cpu.request - s).max(0.0) * row.measured_pods as f64;
        }
        if row.mem.verdict == RsVerdict::Over
            && let Some(s) = row.mem.suggested
        {
            report.reclaimable_mem += (row.mem.request - s).max(0.0) * row.measured_pods as f64;
        }
        match row.worst {
            RsVerdict::Over => report.over.push(row),
            RsVerdict::Under => report.under.push(row),
            RsVerdict::Unrequested => report.unrequested.push(row),
            RsVerdict::RightSized => report.right_sized_count += 1,
            RsVerdict::Unknown => {} // no usage sample + has requests → no finding
        }
    }

    let alloc = median_node_alloc_cpu(world);
    report.node_equiv = if alloc > 0.0 {
        report.reclaimable_cpu / alloc
    } else {
        0.0
    };
    let by_name = |a: &RsRow, b: &RsRow| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name));
    report.over.sort_by(by_name);
    report.under.sort_by(by_name);
    report.unrequested.sort_by(by_name);
    // Workloads that landed in no bucket: scaled-to-zero (no running pods) or
    // running-but-unsampled-with-requests (Unknown). Tracked so the GUI's parts
    // sum to `workloads_total` instead of an inflated "right-sized: X / Y".
    report.unmeasured = report.workloads_total.saturating_sub(
        report.over.len()
            + report.under.len()
            + report.unrequested.len()
            + report.right_sized_count,
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    const MI: f64 = 1024.0 * 1024.0;

    /// Add a Deployment `name` with `n` member pods (via a ReplicaSet) carrying
    /// the given requests/limits, seeding per-pod usage where `usage[i]` exists.
    #[allow(clippy::too_many_arguments)]
    fn deploy_with_pods(
        world: &ObservedWorld,
        s: &mut fx::Seeds,
        name: &str,
        n: usize,
        cpu_req: &str,
        mem_req: &str,
        cpu_lim: &str,
        mem_lim: &str,
        usage: &[(f64, f64)],
    ) {
        s.deployment(fx::deployment("demo", name, n as i32, n as i32));
        let rs = format!("{name}-rs");
        s.replicaset(fx::replicaset("demo", &rs, name));
        for i in 0..n {
            let pod = format!("{rs}-{i}");
            s.pod(fx::pod_requests_limits(
                fx::pod_owned(fx::pod("demo", &pod, Some("n1")), "ReplicaSet", &rs),
                cpu_req,
                mem_req,
                cpu_lim,
                mem_lim,
            ));
            if let Some(&(c, m)) = usage.get(i) {
                fx::set_pod_usage(world, "demo", &pod, c, m);
            }
        }
    }

    #[test]
    fn rightsizing_over_provisioned_flags_idle_workload() {
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "web",
            2,
            "500m",
            "512Mi",
            "",
            "",
            &[(0.05, 64.0 * MI), (0.05, 64.0 * MI)],
        );
        let r = rightsizing_report(&world);
        assert_eq!(r.over.len(), 1);
        let row = &r.over[0];
        assert_eq!(row.name, "web");
        assert_eq!(row.cpu.verdict, RsVerdict::Over);
        assert_eq!(row.mem.verdict, RsVerdict::Over);
        let sug = row.cpu.suggested.unwrap();
        assert!(
            (0.05 / 0.65..0.5).contains(&sug),
            "down-size, above usage/util: {sug}"
        );
        assert_eq!(row.measured_pods, 2);
        assert_eq!(row.qos, RsQos::Burstable); // requests but no limits
        assert!(r.reclaimable_cpu > 0.0 && r.reclaimable_mem > 0.0);
    }

    #[test]
    fn rightsizing_under_memory_uses_peak_and_clamps_suggestion() {
        let (world, mut s) = fx::world();
        // mean 170Mi (< 0.8·256), but peak 240Mi (≥ 0.8·256) → Under by peak.
        deploy_with_pods(
            &world,
            &mut s,
            "db",
            2,
            "100m",
            "256Mi",
            "",
            "",
            &[(0.01, 100.0 * MI), (0.01, 240.0 * MI)],
        );
        let r = rightsizing_report(&world);
        assert_eq!(r.under.len(), 1);
        let row = &r.under[0];
        assert_eq!(row.mem.verdict, RsVerdict::Under);
        // Never size below the hottest pod (memory is incompressible).
        assert!(row.mem.suggested.unwrap() >= 240.0 * MI);
    }

    #[test]
    fn rightsizing_limit_ratio_escalation_notes() {
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "hot",
            1,
            "100m",
            "200Mi",
            "120m",
            "256Mi",
            &[(0.110, 230.0 * MI)],
        );
        let r = rightsizing_report(&world);
        let row = &r.under[0];
        assert_eq!(row.cpu.note, Some("CFS throttling likely"));
        assert_eq!(row.mem.note, Some("OOMKill risk"));
    }

    #[test]
    fn rightsizing_unrequested_needs_no_metrics() {
        let (world, mut s) = fx::world();
        deploy_with_pods(&world, &mut s, "blind", 1, "", "", "", "", &[]);
        let r = rightsizing_report(&world);
        assert!(!r.metrics_available); // degrade-dark, yet still flagged
        assert_eq!(r.unrequested.len(), 1);
        let row = &r.unrequested[0];
        assert_eq!(row.qos, RsQos::BestEffort);
        assert_eq!(row.cpu.suggested, None); // no usage → no suggestion
    }

    #[test]
    fn rightsizing_right_sized_is_quiet() {
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "good",
            1,
            "200m",
            "64Mi",
            "",
            "",
            &[(0.130, 40.0 * MI)],
        );
        let r = rightsizing_report(&world);
        assert!(r.over.is_empty() && r.under.is_empty() && r.unrequested.is_empty());
        assert_eq!(r.right_sized_count, 1);
    }

    #[test]
    fn rightsizing_measured_zero_is_not_false_over() {
        let (world, mut s) = fx::world();
        deploy_with_pods(&world, &mut s, "noobs", 3, "500m", "512Mi", "", "", &[]);
        let r = rightsizing_report(&world);
        assert!(r.over.is_empty() && r.under.is_empty());
        assert_eq!(r.right_sized_count, 0); // Unknown is not "right-sized"
    }

    #[test]
    fn rightsizing_partial_metrics_reclaimable_over_measured() {
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "web",
            3,
            "500m",
            "512Mi",
            "",
            "",
            &[(0.05, 64.0 * MI)],
        );
        let r = rightsizing_report(&world);
        let row = &r.over[0];
        assert_eq!(row.running_pods, 3);
        assert_eq!(row.measured_pods, 1);
        let expect = (0.5 - row.cpu.suggested.unwrap()) * 1.0;
        assert!(
            (r.reclaimable_cpu - expect).abs() < 1e-9,
            "reclaimable × measured, not running"
        );
    }

    #[test]
    fn rightsizing_terminal_pods_excluded() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        let done = fx::pod_phase(
            fx::pod_requests_limits(
                fx::pod_owned(
                    fx::pod("demo", "web-rs-done", Some("n1")),
                    "ReplicaSet",
                    "web-rs",
                ),
                "2000m",
                "2Gi",
                "",
                "",
            ),
            "Succeeded",
        );
        s.pod(done);
        s.pod(fx::pod_requests_limits(
            fx::pod_owned(
                fx::pod("demo", "web-rs-a", Some("n1")),
                "ReplicaSet",
                "web-rs",
            ),
            "500m",
            "512Mi",
            "",
            "",
        ));
        fx::set_pod_usage(&world, "demo", "web-rs-a", 0.05, 64.0 * MI);
        let r = rightsizing_report(&world);
        let row = &r.over[0];
        assert_eq!(row.running_pods, 1); // the Succeeded pod is excluded
        assert!(
            (row.cpu.request - 0.5).abs() < 1e-9,
            "request is 0.5, not 2.0"
        );
    }

    #[test]
    fn rightsizing_limits_without_requests_not_unrequested() {
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "lim",
            1,
            "",
            "",
            "500m",
            "512Mi",
            &[(0.05, 64.0 * MI)],
        );
        let r = rightsizing_report(&world);
        assert!(r.unrequested.is_empty()); // request defaulted to limit
        assert_eq!(r.over.len(), 1);
        assert_eq!(r.over[0].qos, RsQos::Guaranteed); // req == lim
        let demoted = r.over[0].cpu.note == Some("lowering request drops Guaranteed QoS")
            || r.over[0].mem.note == Some("lowering request drops Guaranteed QoS");
        assert!(demoted, "Guaranteed + Over carries the demotion note");
    }

    #[test]
    fn rightsizing_daemonset_uses_pod_count() {
        let (world, mut s) = fx::world();
        s.daemonset(fx::daemonset("demo", "agent", 3, 3));
        for i in 0..3 {
            let pod = format!("agent-{i}");
            s.pod(fx::pod_requests_limits(
                fx::pod_owned(fx::pod("demo", &pod, Some("n1")), "DaemonSet", "agent"),
                "200m",
                "128Mi",
                "",
                "",
            ));
            fx::set_pod_usage(&world, "demo", &pod, 0.01, 16.0 * MI);
        }
        let r = rightsizing_report(&world);
        let row = &r.over[0];
        assert_eq!(row.kind, WorkloadKind::DaemonSet);
        assert_eq!(row.measured_pods, 3);
        let expect = (0.2 - row.cpu.suggested.unwrap()) * 3.0;
        assert!(
            (r.reclaimable_cpu - expect).abs() < 1e-9,
            "reclaimable scales with the fleet"
        );
    }

    #[test]
    fn rightsizing_qos_and_worst() {
        assert_eq!(
            derive_qos(0.1, 0.1, 64.0 * MI, 64.0 * MI),
            RsQos::Guaranteed
        );
        assert_eq!(derive_qos(0.1, 0.0, 64.0 * MI, 0.0), RsQos::Burstable);
        assert_eq!(derive_qos(0.0, 0.0, 0.0, 0.0), RsQos::BestEffort);

        // cpu Over (idle) + mem Under (peak) → worst Under → lands in `under`.
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "mix",
            1,
            "500m",
            "100Mi",
            "",
            "",
            &[(0.01, 95.0 * MI)],
        );
        let r = rightsizing_report(&world);
        assert!(r.over.is_empty());
        assert_eq!(r.under.len(), 1);
        assert_eq!(r.under[0].worst, RsVerdict::Under);
    }

    #[test]
    fn rightsizing_counts_native_sidecar_requests() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "mesh", 1, 1));
        s.replicaset(fx::replicaset("demo", "mesh-rs", "mesh"));
        // main 100m + a native sidecar 100m = 200m request; usage 150m → 0.75
        // (right-sized). If the sidecar request were ignored (100m), 150m usage
        // would falsely read as Under (>0.9·request).
        let pod = fx::pod_native_sidecar(
            fx::pod_requests(
                fx::pod_owned(
                    fx::pod("demo", "mesh-rs-0", Some("n1")),
                    "ReplicaSet",
                    "mesh-rs",
                ),
                "100m",
                "64Mi",
            ),
            "100m",
            "64Mi",
        );
        s.pod(pod);
        fx::set_pod_usage(&world, "demo", "mesh-rs-0", 0.15, 100.0 * MI);
        let r = rightsizing_report(&world);
        assert!(
            r.under.is_empty(),
            "sidecar request counted → not a false Under"
        );
        assert_eq!(r.right_sized_count, 1);
    }

    #[test]
    fn rightsizing_excludes_not_ready_replica_from_mean() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 2, 2));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        // Healthy pod at 300m of a 500m request (0.6 → right-sized); a
        // crash-looping (not-ready) pod reading ~0. Including the latter drags
        // the mean to 150m → false Over; excluding it keeps it right-sized.
        s.pod(fx::pod_requests(
            fx::pod_owned(
                fx::pod("demo", "web-rs-a", Some("n1")),
                "ReplicaSet",
                "web-rs",
            ),
            "500m",
            "256Mi",
        ));
        fx::set_pod_usage(&world, "demo", "web-rs-a", 0.30, 150.0 * MI);
        let bad = fx::pod_not_ready(fx::pod_requests(
            fx::pod_owned(
                fx::pod("demo", "web-rs-b", Some("n1")),
                "ReplicaSet",
                "web-rs",
            ),
            "500m",
            "256Mi",
        ));
        s.pod(bad);
        fx::set_pod_usage(&world, "demo", "web-rs-b", 0.0, 5.0 * MI);
        let r = rightsizing_report(&world);
        assert!(
            r.over.is_empty(),
            "not-ready replica must not drag the mean into a false Over"
        );
        assert_eq!(r.right_sized_count, 1);
    }

    #[test]
    fn rightsizing_memory_under_never_suggests_a_lower_request_and_cpu_reclaim_counts() {
        let (world, mut s) = fx::world();
        // mem: req 300Mi, mean 90Mi (low), peak 245Mi (≥0.8·300 → Under). Pre-fix
        // this suggested 256Mi < 300Mi (a "raise" below the request!). cpu: idle →
        // Over. The row's worst is Under, but its cpu saving must still count.
        deploy_with_pods(
            &world,
            &mut s,
            "skew",
            2,
            "100m",
            "300Mi",
            "",
            "",
            &[(0.01, 90.0 * MI), (0.01, 245.0 * MI)],
        );
        let r = rightsizing_report(&world);
        let row = r
            .under
            .iter()
            .find(|w| w.name == "skew")
            .expect("skew is Under");
        assert!(
            row.mem.suggested.unwrap() > row.mem.request,
            "an Under must recommend a genuine raise, never a value ≤ the request"
        );
        assert!(
            r.reclaimable_cpu > 0.0,
            "cpu saving counts even though the row is in the Under bucket"
        );
    }

    #[test]
    fn rightsizing_node_metrics_up_but_no_pod_usage_degrades_dark() {
        let (world, mut s) = fx::world();
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true; // NodeMetrics up...
        }
        deploy_with_pods(&world, &mut s, "web", 1, "500m", "512Mi", "", "", &[]); // ...but no PodMetrics
        let r = rightsizing_report(&world);
        assert!(
            !r.metrics_available,
            "no per-pod usage → degrade-dark, not a false all-right-sized"
        );
        assert!(r.over.is_empty() && r.right_sized_count == 0);
    }

    #[test]
    fn rightsizing_unmeasured_counts_unsampled_workloads() {
        let (world, mut s) = fx::world();
        deploy_with_pods(
            &world,
            &mut s,
            "seen",
            1,
            "500m",
            "512Mi",
            "",
            "",
            &[(0.05, 64.0 * MI)],
        );
        deploy_with_pods(&world, &mut s, "unseen", 1, "500m", "512Mi", "", "", &[]);
        let r = rightsizing_report(&world);
        assert_eq!(r.workloads_total, 2);
        assert_eq!(r.over.len(), 1);
        // 'unseen' has requests but no usage → Unknown → counted as unmeasured so
        // the parts sum to workloads_total.
        assert_eq!(r.unmeasured, 1);
        assert_eq!(
            r.over.len() + r.under.len() + r.unrequested.len() + r.right_sized_count + r.unmeasured,
            r.workloads_total
        );
    }

    #[test]
    fn rightsizing_floor_negated_over_is_not_waste() {
        let (world, mut s) = fx::world();
        // cpu genuinely reclaimable (0.5 → ~0.08), but mem request (70Mi) is
        // already below the 250Mi floor, so the "down-size" would raise it →
        // not waste → mem demoted to RightSized, only cpu stays Over.
        deploy_with_pods(
            &world,
            &mut s,
            "mix2",
            1,
            "500m",
            "70Mi",
            "",
            "",
            &[(0.05, 24.0 * MI)],
        );
        let r = rightsizing_report(&world);
        assert_eq!(r.over.len(), 1);
        let row = &r.over[0];
        assert_eq!(row.cpu.verdict, RsVerdict::Over);
        assert_eq!(
            row.mem.verdict,
            RsVerdict::RightSized,
            "floor negates the mem waste"
        );
        assert!(r.reclaimable_cpu > 0.0);
        assert_eq!(r.reclaimable_mem, 0.0);
    }

    #[test]
    fn rightsizing_suggestions_respect_floors_and_round_up() {
        assert!((suggest_cpu(0.002) - 0.03).abs() < 1e-9); // 25m floor → round up 10m
        assert!(suggest_mem(1.0 * MI, 1.0 * MI) >= 250.0 * MI); // 250Mi floor
    }

    #[test]
    fn health_rolls_up_nodes_pods_and_workloads() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.node(fx::node("n2", Some("z-b")));
        // A healthy deployment (2/2) and an understrength one (1/3).
        s.deployment(fx::deployment("demo", "web", 2, 2));
        s.deployment(fx::deployment("demo", "api", 3, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-a", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-b", Some("n2")),
            "ReplicaSet",
            "web-rs",
        ));

        // A pending pod and a terminating pod must land in distinct buckets —
        // a benign terminating pod must NOT read as pending.
        s.pod(fx::pod_phase(fx::pod("demo", "queued", None), "Pending"));
        s.pod(fx::pod_terminating(fx::pod("demo", "draining", Some("n1"))));

        let r = health_report(&world);
        assert_eq!(r.nodes_total, 2);
        assert_eq!(r.nodes_healthy, 2);
        assert_eq!(r.pods_total, 4);
        assert_eq!(r.pods_running, 2);
        assert_eq!(r.pods_pending, 1);
        assert_eq!(r.pods_terminating, 1);
        assert_eq!(r.workloads_total, 2);
        assert_eq!(r.workloads_degraded, 1); // api is 1/3
    }

    #[test]
    fn storage_separates_bound_from_pending() {
        let (world, mut s) = fx::world();
        s.pvc(fx::pvc("demo", "data-db-0", "Bound"));
        s.pvc(fx::pvc("demo", "stuck", "Pending"));
        let r = storage_report(&world);
        assert_eq!(r.total, 2);
        assert_eq!(r.bound, 1);
        assert_eq!(r.pending, 1);
        assert_eq!(r.pending_claims.len(), 1);
        assert_eq!(r.pending_claims[0].name, "stuck");
        assert_eq!(r.pending_claims[0].phase, "Pending");
    }

    #[test]
    fn network_flags_orphan_ingress_and_idle_service() {
        let (world, mut s) = fx::world();
        // A healthy service with a matching pod.
        s.service(fx::service("demo", "web", &[("app", "web")]));
        let mut p = fx::pod("demo", "web-1", Some("n1"));
        p.metadata.labels = Some([("app".to_string(), "web".to_string())].into());
        s.pod(p);
        // An idle service (selector matches nothing).
        s.service(fx::service("demo", "lonely", &[("app", "ghost")]));
        // An ingress whose backend service is absent.
        s.ingress(fx::ingress("demo", "edge", "web.example", "missing-svc"));

        let r = network_report(&world);
        assert_eq!(r.services, 2);
        assert_eq!(r.ingresses, 1);
        assert_eq!(r.idle_services.len(), 1);
        assert_eq!(r.idle_services[0].name, "lonely");
        assert_eq!(r.orphan_ingresses.len(), 1);
        assert_eq!(r.orphan_ingresses[0].name, "edge");
    }
}
