//! Cost cartography — "**upkeep**", the recurring coin a realm pays to *hold*
//! its assets (reserved capacity), whether or not it uses them. A pure, read-only
//! sibling of [`advisor::rightsizing_report`](crate::state::advisor): one
//! `cost_report(world, rates)` feeds both the map overlay and the Advisors ▸ Cost
//! tab, so they can never disagree.
//!
//! **Honest about what a laptop can know.** Kubernetes has no native cost API and
//! KuberNation reads no cloud billing. Cost is *derived* and the UI never implies a
//! real invoice:
//!  - **Base signal (any cluster):** resource *requests* — what the scheduler
//!    reserves, i.e. what you pay to hold. Computable from core API objects, no
//!    metrics-server.
//!  - **Refinement (metrics-server present):** actual *usage* sharpens the idle
//!    figure to "paid-for-but-unused" (the right-sizing gap, in cost).
//!  - **Pricing is operator-supplied or absent.** With no rates we report a
//!    relative **"cost units"** score — a cpu + mem/[`DEFAULT_MEM_WEIGHT`] weighted
//!    resource footprint, meaningful on any cluster with zero external data, but
//!    NOT money (no `$`, no monthly projection). With rates ([`CostRates`], from
//!    CLI flags / a `kubernation.io/cost-hourly` node annotation) we report `$`
//!    (hourly + a ×[`HOURS_PER_MONTH`] monthly). Even then it is an *estimate from
//!    your rates × reservation* — it excludes network/egress/storage/LB/committed-
//!    use discounts and is not a cloud invoice.
//!
//! **Allocation (share of capacity).** A node has a cost (its rate, or its
//! allocatable × the rates, or — unitless — its resource weight). That cost is
//! distributed to the node's non-terminal pods *by each pod's share of the node's
//! weighted capacity*; the unallocated remainder is **idle** — the actionable
//! drain you could consolidate away. (Share-of-*used* would make idle identically
//! zero and erase that signal.) For a not-overcommitted node this is an exact
//! partition: `Σ(pod cost) + idle == node cost`. An overcommitted node (requests >
//! capacity) clamps idle to 0; the pod shares then exceed the node cost (the
//! cluster is reserving more than it has).
//!
//! **Pair note:** the overlay ramp normalizes to the focused world's max node cost,
//! so a hot and a warm node painted the same bronze are not equal absolute cost
//! (the same self-scaled tradeoff as the cluster sparklines).

use std::collections::HashMap;

use crate::state::chaos::ns_protected;
use crate::state::model::{
    OwnerIndex, WorkloadKind, WorkloadRef, node_allocatable, pod_terminal, sum_pod_reserved,
};
use crate::state::observed::ObservedWorld;

/// Bytes per binary GiB — matches `quantity::value` (binary 1024³), so the
/// `--mem-rate` flag is honestly priced per *GiB* (an operator's per-decimal-GB
/// cloud rate would be ~7% off).
pub const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// Hours per month for the ×730 monthly projection (the cloud-cost convention).
pub const HOURS_PER_MONTH: f64 = 730.0;
/// Default cpu : mem-GiB weight for the unitless "cost units" score (cloud ~1:4).
pub const DEFAULT_MEM_WEIGHT: f64 = 4.0;
/// Per-node idle fraction at/above which a node's idle is "notable" — drives the
/// map idle coin + the SELECTION highlight.
pub const IDLE_NOTABLE: f64 = 0.40;
/// Cluster-mean idle fraction at/above which the advisor's headline idle line
/// warns — a softer, aggregate threshold than the per-node [`IDLE_NOTABLE`] coin
/// (they share the `1 − used_frac` metric but at different scopes, by design).
pub const IDLE_CLUSTER_WARN: f64 = 0.25;
/// Optional per-node hourly `$` override read from a Node annotation (a plain
/// float). Highest pricing precedence; read at the frontend boundary into
/// [`CostRates::node_overrides`] (so the pure core stays a fn of `(world, rates)`).
pub const COST_ANNOTATION: &str = "kubernation.io/cost-hourly";

/// Whether costs are unitless relative scores or real currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostMode {
    /// No operator pricing — a relative resource-weight score ("cost units").
    #[default]
    Unitless,
    /// Operator-supplied rates → real currency (`$`).
    Currency,
}

/// Which signal the cost shares come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostBasis {
    /// Shares from resource *requests* (scheduler reservation) — always available.
    #[default]
    Requests,
    /// Shares from metrics-server *usage* — sharpens idle to paid-for-but-unused.
    Usage,
    /// Imported from OpenCost — invoice-grade, amortized (incl. network/LB/storage).
    /// Not KuberNation's own derivation; the UI labels it "from OpenCost".
    OpenCost,
}

/// Operator-supplied pricing. All-absent ⇒ [`CostMode::Unitless`].
#[derive(Debug, Clone, Default)]
pub struct CostRates {
    /// `$` per cpu-core-hour (`None` ⇒ no cpu price).
    pub cpu_hour: Option<f64>,
    /// `$` per *GiB*-hour (binary, see [`GIB`]; `None` ⇒ no mem price).
    pub mem_gib_hour: Option<f64>,
    /// Per-node hourly `$` overrides (highest precedence), keyed by node name.
    pub node_overrides: HashMap<String, f64>,
    /// cpu : mem-GiB weight for the unitless score (0 ⇒ [`DEFAULT_MEM_WEIGHT`]).
    pub mem_weight: f64,
}

impl CostRates {
    /// Currency mode iff the operator supplied ANY rate.
    pub fn currency(&self) -> bool {
        self.cpu_hour.is_some() || self.mem_gib_hour.is_some() || !self.node_overrides.is_empty()
    }
    fn weight(&self) -> f64 {
        if self.mem_weight > 0.0 {
            self.mem_weight
        } else {
            DEFAULT_MEM_WEIGHT
        }
    }
}

/// One node's upkeep. All-scalar so it's `Copy` — the overlay + SELECTION read it
/// from the report by node name.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NodeCost {
    /// The node's cost per hour (`$` in Currency mode, weighted units in Unitless).
    pub per_hour: f64,
    /// The unrequested/unused remainder (idle) — the actionable drain.
    pub idle_per_hour: f64,
    /// Fraction of weighted capacity allocated, clamped to `0..=1` (idle ≈ 1−this).
    pub used_frac: f64,
    pub basis: CostBasis,
    pub mode: CostMode,
    /// False ⇒ couldn't price (no allocatable, or no applicable rate) → contributes 0.
    pub priced: bool,
    /// Requests exceed capacity (used > cap) → idle forced to 0; shares exceed cost.
    pub overcommitted: bool,
}

/// A workload's allocated upkeep.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkloadCost {
    pub kind: WorkloadKind,
    pub namespace: String,
    pub name: String,
    pub per_hour: f64,
}

/// A namespace's allocated upkeep.
#[derive(Debug, Clone, PartialEq)]
pub struct NamespaceCost {
    pub namespace: String,
    pub per_hour: f64,
    /// A protected/system namespace (kube-system/…) — surfaced, but not the
    /// operator's spend to chase.
    pub system: bool,
}

/// The whole-cluster upkeep rollup. One report feeds the overlay + the advisor tab.
#[derive(Debug, Clone, Default)]
pub struct CostReport {
    pub mode: CostMode,
    pub basis: CostBasis,
    /// Whole-cluster upkeep per hour (all priced nodes).
    pub total_per_hour: f64,
    /// Σ idle across nodes (unrequested/unused capacity you carry).
    pub idle_per_hour: f64,
    /// Σ over protected/system-namespace pods (a subtotal of the namespace costs).
    pub system_per_hour: f64,
    /// Σ over pods with no owning workload (bare pods).
    pub unowned_per_hour: f64,
    /// Max single priced-node cost (the overlay ramp normalizes to this).
    pub max_node_cost: f64,
    /// Per-node cost by node name — the overlay + SELECTION read this.
    pub by_node: HashMap<String, NodeCost>,
    /// Per-namespace rollup, descending by cost.
    pub by_namespace: Vec<NamespaceCost>,
    /// Workloads descending by cost (the GUI shows the top N).
    pub top_workloads: Vec<WorkloadCost>,
    pub nodes_priced: usize,
    pub nodes_total: usize,
    /// metrics-server is giving per-pod usage (Usage basis) vs request-only.
    pub metrics_available: bool,
}

/// The ×730 monthly projection of an hourly figure.
pub fn monthly(per_hour: f64) -> f64 {
    per_hour * HOURS_PER_MONTH
}

/// Format an hourly cost. Unitless NEVER shows "$"; currency shows "$X.XX/hr".
/// (The honesty guard — a relative unit is not money.)
pub fn fmt_hourly(per_hour: f64, mode: CostMode) -> String {
    match mode {
        CostMode::Currency => format!("${per_hour:.2}/hr"),
        CostMode::Unitless => format!("{per_hour:.1} units"),
    }
}

/// As [`fmt_hourly`], plus a ×730 monthly projection — but ONLY in currency mode
/// (a unitless score has no time dimension, so never a "/mo").
pub fn fmt_monthly(per_hour: f64, mode: CostMode) -> String {
    match mode {
        CostMode::Currency => format!("${per_hour:.2}/hr · ~${:.0}/mo", monthly(per_hour)),
        CostMode::Unitless => format!("{per_hour:.1} units"),
    }
}

/// Per-pod allocation inputs (no `Pod` ref kept — avoids lifetimes).
struct PodAlloc {
    namespace: String,
    owner: Option<WorkloadRef>,
    req_w: f64,
    usage_w: Option<f64>,
}

/// Build the cost report — pure over `(world, rates)`.
pub fn cost_report(world: &ObservedWorld, rates: &CostRates) -> CostReport {
    let mode = if rates.currency() {
        CostMode::Currency
    } else {
        CostMode::Unitless
    };
    let weight = rates.weight();
    let idx = OwnerIndex::build(world);

    // Pass 1 — index non-terminal scheduled pods by node, with request + usage weights.
    let mut pods_by_node: HashMap<String, Vec<PodAlloc>> = HashMap::new();
    let mut has_pod_metrics = false;
    for p in world.pods.state() {
        let pod = p.as_ref();
        if pod_terminal(pod) {
            continue; // terminal pods reserve nothing
        }
        let Some(node) = pod.spec.as_ref().and_then(|s| s.node_name.clone()) else {
            continue; // unscheduled — not on a node yet
        };
        let namespace = pod.metadata.namespace.clone().unwrap_or_default();
        let name = pod.metadata.name.clone().unwrap_or_default();
        // Per-container reserved request (k8s defaults request := limit per
        // container/resource — the scheduler's effective reservation).
        let (creq, mreq) = sum_pod_reserved(pod);
        let req_w = creq + (mreq / GIB) / weight;
        let usage_w = world
            .pod_usage(&namespace, &name)
            .map(|u| u.cpu + (u.mem / GIB) / weight);
        if usage_w.is_some() {
            has_pod_metrics = true;
        }
        pods_by_node.entry(node).or_default().push(PodAlloc {
            namespace,
            owner: idx.workload_of(pod),
            req_w,
            usage_w,
        });
    }

    // NodeMetrics can be up while PodMetrics is empty (best-effort, fails
    // independently) — without per-pod usage, treat as request-based (degrade-dark),
    // never a false "usage-refined".
    let metrics_available = world.metrics_available() && has_pod_metrics;
    let basis = if metrics_available {
        CostBasis::Usage
    } else {
        CostBasis::Requests
    };
    let pod_w = |pa: &PodAlloc| -> f64 {
        if basis == CostBasis::Usage {
            pa.usage_w.unwrap_or(pa.req_w) // a pod with no sample falls back to request, never 0
        } else {
            pa.req_w
        }
    };

    let mut report = CostReport {
        mode,
        basis,
        metrics_available,
        ..Default::default()
    };
    let mut ns_acc: HashMap<String, f64> = HashMap::new();
    let mut wl_acc: HashMap<WorkloadRef, f64> = HashMap::new();

    // Pass 2 — price each node, allocate its cost to its pods, surface idle.
    for n in world.nodes.state() {
        let node = n.as_ref();
        let name = node.metadata.name.clone().unwrap_or_default();
        report.nodes_total += 1;

        let alloc_cpu = node_allocatable(node, "cpu").unwrap_or(0.0);
        let alloc_mem = node_allocatable(node, "memory").unwrap_or(0.0);
        let cap_w = alloc_cpu + (alloc_mem / GIB) / weight;
        if cap_w <= 0.0 {
            // No allocatable → can't ALLOCATE to pods. But an operator-priced node
            // (override/annotation, currency mode) still has a known cost — record
            // it as all-idle so it isn't silently dropped from the cluster total.
            let priced_cost = (mode == CostMode::Currency)
                .then(|| {
                    rates
                        .node_overrides
                        .get(&name)
                        .copied()
                        .filter(|v| *v > 0.0)
                })
                .flatten();
            if let Some(c) = priced_cost {
                report.nodes_priced += 1;
                report.total_per_hour += c;
                report.idle_per_hour += c;
                report.max_node_cost = report.max_node_cost.max(c);
            }
            report.by_node.insert(
                name,
                NodeCost {
                    per_hour: priced_cost.unwrap_or(0.0),
                    idle_per_hour: priced_cost.unwrap_or(0.0),
                    used_frac: 0.0,
                    basis: CostBasis::Requests,
                    mode,
                    priced: priced_cost.is_some(),
                    overcommitted: false,
                },
            );
            continue;
        }

        let node_cost = match mode {
            CostMode::Unitless => cap_w, // the resource weight IS the cost-unit score
            CostMode::Currency => {
                if let Some(&ov) = rates.node_overrides.get(&name) {
                    ov
                } else {
                    alloc_cpu * rates.cpu_hour.unwrap_or(0.0)
                        + (alloc_mem / GIB) * rates.mem_gib_hour.unwrap_or(0.0)
                }
            }
        };
        let priced = node_cost > 0.0; // in currency mode a node with no applicable rate is unpriced

        let pods = pods_by_node
            .get(&name)
            .map(Vec::as_slice)
            .unwrap_or(&[][..]);
        // Per-node basis: this node is usage-priced only if it actually has a
        // sampled pod (under a globally-Usage report a node whose pods are all
        // unsampled allocates by request — label it honestly).
        let node_basis = if basis == CostBasis::Usage && pods.iter().any(|p| p.usage_w.is_some()) {
            CostBasis::Usage
        } else {
            CostBasis::Requests
        };
        let used_w: f64 = pods.iter().map(&pod_w).sum();
        let frac = used_w / cap_w;
        let overcommitted = frac > 1.0;
        let used_frac = frac.min(1.0);
        let idle = node_cost * (1.0 - used_frac); // overcommit ⇒ used_frac 1 ⇒ idle 0

        if priced {
            report.nodes_priced += 1;
            report.total_per_hour += node_cost;
            report.idle_per_hour += idle;
            report.max_node_cost = report.max_node_cost.max(node_cost);
            for pa in pods {
                let pc = node_cost * pod_w(pa) / cap_w; // share of capacity
                *ns_acc.entry(pa.namespace.clone()).or_default() += pc;
                if ns_protected(&pa.namespace) {
                    report.system_per_hour += pc;
                }
                match &pa.owner {
                    Some(wr) => *wl_acc.entry(wr.clone()).or_default() += pc,
                    None => report.unowned_per_hour += pc,
                }
            }
        }

        report.by_node.insert(
            name,
            NodeCost {
                per_hour: node_cost,
                idle_per_hour: idle,
                used_frac,
                basis: node_basis,
                mode,
                priced,
                overcommitted,
            },
        );
    }

    // Rollups, descending by cost (stable tie-break by name).
    report.by_namespace = ns_acc
        .into_iter()
        .map(|(namespace, per_hour)| NamespaceCost {
            system: ns_protected(&namespace),
            namespace,
            per_hour,
        })
        .collect();
    report.by_namespace.sort_by(|a, b| {
        b.per_hour
            .total_cmp(&a.per_hour)
            .then(a.namespace.cmp(&b.namespace))
    });

    report.top_workloads = wl_acc
        .into_iter()
        .map(|(wr, per_hour)| WorkloadCost {
            kind: wr.kind,
            namespace: wr.namespace,
            name: wr.name,
            per_hour,
        })
        .collect();
    report.top_workloads.sort_by(|a, b| {
        b.per_hour
            .total_cmp(&a.per_hour)
            .then((&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)))
    });

    report
}

/// Build a [`CostReport`] from imported OpenCost data — the same shape the overlay
/// and advisor render, so OpenCost simply *replaces* the requests-based estimate
/// (basis `OpenCost`, real `$`, amortized incl. network/LB/storage). PURE. The
/// per-node figure is the *allocated* cost on each node (sum of its allocations);
/// idle is OpenCost's cluster `__idle__`, not a per-node residual.
pub fn from_opencost(oc: &crate::state::opencost::OpenCostData) -> CostReport {
    let mut report = CostReport {
        mode: CostMode::Currency,
        basis: CostBasis::OpenCost,
        metrics_available: true,
        idle_per_hour: oc.idle_per_hour,
        total_per_hour: oc.total_per_hour,
        ..Default::default()
    };
    let mut ns_acc: HashMap<String, f64> = HashMap::new();
    let mut node_acc: HashMap<String, f64> = HashMap::new();

    for a in &oc.allocations {
        // A legitimate allocation always carries a namespace; an empty one is a
        // control/idle artifact (parse already drops those) — skip defensively so a
        // blank "" namespace row can never reach the advisor.
        if a.namespace.is_empty() {
            continue;
        }
        *ns_acc.entry(a.namespace.clone()).or_default() += a.per_hour;
        if ns_protected(&a.namespace) {
            report.system_per_hour += a.per_hour;
        }
        if let Some(node) = &a.node {
            *node_acc.entry(node.clone()).or_default() += a.per_hour;
        }
        match &a.controller {
            Some(name) => report.top_workloads.push(WorkloadCost {
                kind: kind_of(a.controller_kind.as_deref()),
                namespace: a.namespace.clone(),
                name: name.clone(),
                per_hour: a.per_hour,
            }),
            None => report.unowned_per_hour += a.per_hour,
        }
    }

    report.by_namespace = ns_acc
        .into_iter()
        .map(|(namespace, per_hour)| NamespaceCost {
            system: ns_protected(&namespace),
            namespace,
            per_hour,
        })
        .collect();
    report.by_namespace.sort_by(|a, b| {
        b.per_hour
            .total_cmp(&a.per_hour)
            .then(a.namespace.cmp(&b.namespace))
    });

    for (name, per_hour) in node_acc {
        report.max_node_cost = report.max_node_cost.max(per_hour);
        report.nodes_total += 1;
        report.nodes_priced += 1;
        report.by_node.insert(
            name,
            NodeCost {
                per_hour,
                idle_per_hour: 0.0, // OpenCost idle is cluster-level, not per-node here
                used_frac: 1.0,
                basis: CostBasis::OpenCost,
                mode: CostMode::Currency,
                priced: true,
                overcommitted: false,
            },
        );
    }
    report.top_workloads.sort_by(|a, b| {
        b.per_hour
            .total_cmp(&a.per_hour)
            .then((&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)))
    });
    report
}

/// OpenCost's `controllerKind` string → our [`WorkloadKind`] (replicaset folds to
/// Deployment — a bare RS is rare and reads as its Deployment).
fn kind_of(controller_kind: Option<&str>) -> WorkloadKind {
    match controller_kind.unwrap_or("").to_ascii_lowercase().as_str() {
        "statefulset" => WorkloadKind::StatefulSet,
        "daemonset" => WorkloadKind::DaemonSet,
        _ => WorkloadKind::Deployment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    const MI: f64 = 1024.0 * 1024.0;
    const GI: f64 = 1024.0 * MI;

    /// Seed a Deployment with `n` pods on `node` carrying `cpu_req`/`mem_req`,
    /// seeding per-pod usage where `usage[i]` exists.
    #[allow(clippy::too_many_arguments)]
    fn one_node_deploy(
        world: &ObservedWorld,
        s: &mut fx::Seeds,
        node: &str,
        ns: &str,
        name: &str,
        n: usize,
        cpu_req: &str,
        mem_req: &str,
        usage: &[(f64, f64)],
    ) {
        s.deployment(fx::deployment(ns, name, n as i32, n as i32));
        let rs = format!("{name}-rs");
        s.replicaset(fx::replicaset(ns, &rs, name));
        for i in 0..n {
            let pod = format!("{rs}-{i}");
            s.pod(fx::pod_requests(
                fx::pod_owned(fx::pod(ns, &pod, Some(node)), "ReplicaSet", &rs),
                cpu_req,
                mem_req,
            ));
            if let Some(&(c, m)) = usage.get(i) {
                fx::set_pod_usage(world, ns, &pod, c, m);
            }
        }
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "expected {b}, got {a}");
    }

    #[test]
    fn allocation_is_an_exact_partition() {
        // 1 node (cap_w = 4 + 8/4 = 6), 2 pods each 500m + 512Mi
        // (req_w = 0.5 + 0.5/4 = 0.625) → used 1.25, idle 4.75; partition = 6.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 2, "500m", "512Mi", &[]);
        let r = cost_report(&world, &CostRates::default());
        let nc = r.by_node["n1"];
        approx(nc.per_hour, 6.0);
        let web = r.top_workloads.iter().find(|w| w.name == "web").unwrap();
        approx(web.per_hour, 1.25);
        approx(nc.idle_per_hour, 4.75);
        // Σ(pod cost) + idle == node cost.
        approx(web.per_hour + nc.idle_per_hour, nc.per_hour);
        assert_eq!(r.mode, CostMode::Unitless);
        assert_eq!(r.basis, CostBasis::Requests);
        assert_eq!((r.nodes_priced, r.nodes_total), (1, 1));
    }

    #[test]
    fn idle_is_the_unrequested_remainder() {
        // Half the cap_w=6 requested (2 pods × 1500m = 3 units) → idle ≈ 50%.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 2, "1500m", "0", &[]);
        let r = cost_report(&world, &CostRates::default());
        let nc = r.by_node["n1"];
        approx(nc.idle_per_hour, 3.0);
        approx(nc.used_frac, 0.5);
        assert!(!nc.overcommitted);
    }

    #[test]
    fn overcommit_clamps_idle_to_zero() {
        // 3 pods × 3000m = 9 units > cap_w 6 → overcommitted, idle 0, shares > cost.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        one_node_deploy(&world, &mut s, "n1", "demo", "hog", 3, "3000m", "0", &[]);
        let r = cost_report(&world, &CostRates::default());
        let nc = r.by_node["n1"];
        assert!(nc.overcommitted);
        approx(nc.idle_per_hour, 0.0);
        let hog = r.top_workloads.iter().find(|w| w.name == "hog").unwrap();
        assert!(
            hog.per_hour > nc.per_hour,
            "shares exceed node cost when overcommitted"
        );
    }

    #[test]
    fn cluster_total_double_counts_nothing() {
        // 2 nodes, 2 workloads + a bare pod: Σ(workloads)+unowned+idle == total.
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        s.node(fx::node("n2", Some("z")));
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 2, "500m", "256Mi", &[]);
        one_node_deploy(
            &world,
            &mut s,
            "n2",
            "demo",
            "api",
            1,
            "1000m",
            "512Mi",
            &[],
        );
        s.pod(fx::pod_requests(
            fx::pod("demo", "loose", Some("n2")),
            "200m",
            "0",
        ));
        let r = cost_report(&world, &CostRates::default());
        let wl: f64 = r.top_workloads.iter().map(|w| w.per_hour).sum();
        approx(wl + r.unowned_per_hour + r.idle_per_hour, r.total_per_hour);
        assert!(
            r.unowned_per_hour > 0.0,
            "the bare pod is the 'unowned' line"
        );
    }

    #[test]
    fn currency_mode_scales_by_rates_and_node_override() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 1, "500m", "512Mi", &[]);
        // node_cost = 4·0.03 + 8·0.004 = 0.152 $/hr.
        let rates = CostRates {
            cpu_hour: Some(0.03),
            mem_gib_hour: Some(0.004),
            ..Default::default()
        };
        let r = cost_report(&world, &rates);
        assert_eq!(r.mode, CostMode::Currency);
        approx(r.by_node["n1"].per_hour, 0.152);
        approx(monthly(0.152), 0.152 * 730.0);
        // A per-node override supersedes the computed rate.
        let mut over = HashMap::new();
        over.insert("n1".to_string(), 0.5);
        let r2 = cost_report(
            &world,
            &CostRates {
                node_overrides: over,
                ..Default::default()
            },
        );
        assert_eq!(r2.mode, CostMode::Currency);
        approx(r2.by_node["n1"].per_hour, 0.5);
    }

    #[test]
    fn mem_rate_is_priced_per_binary_gib() {
        // mem-only rate on an 8Gi node = 8 × the per-GiB rate (binary).
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        let r = cost_report(
            &world,
            &CostRates {
                mem_gib_hour: Some(0.004),
                ..Default::default()
            },
        );
        approx(r.by_node["n1"].per_hour, 8.0 * 0.004);
    }

    #[test]
    fn no_pod_metrics_degrades_to_requests() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true; // NodeMetrics up...
        }
        // ...but no per-pod usage seeded.
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 1, "500m", "512Mi", &[]);
        let r = cost_report(&world, &CostRates::default());
        assert_eq!(r.basis, CostBasis::Requests, "no pod usage → request-based");
        assert!(!r.metrics_available);
    }

    #[test]
    fn usage_basis_when_pod_metrics_present() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        {
            let mut g = world.metrics.lock().unwrap();
            g.available = true;
        }
        // Pod requests 1000m but uses only 100m → usage share is much smaller.
        one_node_deploy(
            &world,
            &mut s,
            "n1",
            "demo",
            "web",
            1,
            "1000m",
            "0",
            &[(0.1, 0.0)],
        );
        let r = cost_report(&world, &CostRates::default());
        assert_eq!(r.basis, CostBasis::Usage);
        assert!(r.metrics_available);
        // used by usage (0.1) not request (1.0) → idle is larger than the request basis would give.
        assert!(r.by_node["n1"].used_frac < 0.1, "usage share is tiny");
    }

    #[test]
    fn system_namespace_cost_is_surfaced_not_charged() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 1, "500m", "0", &[]);
        one_node_deploy(
            &world,
            &mut s,
            "n1",
            "kube-system",
            "kproxy",
            1,
            "250m",
            "0",
            &[],
        );
        let r = cost_report(&world, &CostRates::default());
        assert!(r.system_per_hour > 0.0, "system cost surfaced");
        let sys = r
            .by_namespace
            .iter()
            .find(|n| n.namespace == "kube-system")
            .unwrap();
        assert!(sys.system, "kube-system tagged system");
        let demo = r
            .by_namespace
            .iter()
            .find(|n| n.namespace == "demo")
            .unwrap();
        assert!(!demo.system);
    }

    #[test]
    fn a_limits_only_container_reserves_its_limit() {
        // k8s defaults request := limit per container, so a limits-only pod still
        // reserves (and costs) — not treated as 0 (the per-container fix).
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        s.deployment(fx::deployment("demo", "lim", 1, 1));
        s.replicaset(fx::replicaset("demo", "lim-rs", "lim"));
        s.pod(fx::pod_requests_limits(
            fx::pod_owned(
                fx::pod("demo", "lim-rs-0", Some("n1")),
                "ReplicaSet",
                "lim-rs",
            ),
            "",
            "",
            "500m",
            "512Mi", // limit-only
        ));
        let r = cost_report(&world, &CostRates::default());
        let lim = r.top_workloads.iter().find(|w| w.name == "lim").unwrap();
        approx(lim.per_hour, 0.5 + 0.5 / 4.0); // reserved = the limit, weighted
    }

    #[test]
    fn override_node_without_allocatable_is_priced_all_idle() {
        // A node with a $ override but no allocatable can't allocate to pods, but
        // its cost is still counted (all idle) — not silently dropped.
        let (world, mut s) = fx::world();
        let mut bare = fx::node("n1", Some("z"));
        bare.status.as_mut().unwrap().allocatable = None;
        s.node(bare);
        let mut over = HashMap::new();
        over.insert("n1".to_string(), 0.5);
        let r = cost_report(
            &world,
            &CostRates {
                node_overrides: over,
                ..Default::default()
            },
        );
        assert_eq!(r.nodes_priced, 1);
        approx(r.total_per_hour, 0.5);
        approx(r.idle_per_hour, 0.5);
        assert!(r.by_node["n1"].priced);
    }

    #[test]
    fn unitless_formatting_never_shows_currency() {
        // The honesty guard: unitless = no "$", no "/mo"; currency = both.
        let u = fmt_monthly(18.4, CostMode::Unitless);
        assert!(
            u.contains("units") && !u.contains('$') && !u.contains("/mo"),
            "{u}"
        );
        let c = fmt_monthly(0.42, CostMode::Currency);
        assert!(c.contains('$') && c.contains("/mo"), "{c}");
        assert!(!fmt_hourly(0.42, CostMode::Currency).contains("/mo"));
    }

    #[test]
    fn unpriced_node_contributes_nothing() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z")));
        let mut bare = fx::node("n2", Some("z"));
        bare.status.as_mut().unwrap().allocatable = None; // no allocatable
        s.node(bare);
        one_node_deploy(&world, &mut s, "n1", "demo", "web", 1, "500m", "0", &[]);
        let r = cost_report(&world, &CostRates::default());
        assert_eq!((r.nodes_priced, r.nodes_total), (1, 2));
        assert!(!r.by_node["n2"].priced);
        approx(r.by_node["n2"].per_hour, 0.0);
        let _ = GI; // (keep the binary-GiB constant referenced)
    }

    #[test]
    fn opencost_report_maps_namespaces_workloads_nodes_and_idle() {
        use crate::state::opencost::{OcAllocation, OpenCostData};
        let alloc = |ns: &str, ctrl: &str, kind: &str, node: &str, ph: f64| OcAllocation {
            namespace: ns.into(),
            controller: Some(ctrl.into()),
            controller_kind: Some(kind.into()),
            node: Some(node.into()),
            per_hour: ph,
            ..Default::default()
        };
        let oc = OpenCostData {
            allocations: vec![
                alloc("demo", "web", "deployment", "n1", 0.20),
                alloc("demo", "db", "statefulset", "n2", 0.30),
                alloc("kube-system", "coredns", "deployment", "n1", 0.05),
            ],
            idle_per_hour: 0.40,
            total_per_hour: 0.95,
        };
        let r = from_opencost(&oc);
        assert_eq!(r.mode, CostMode::Currency);
        assert_eq!(r.basis, CostBasis::OpenCost);
        approx(r.total_per_hour, 0.95);
        approx(r.idle_per_hour, 0.40);
        // by namespace: demo = web+db = 0.50; kube-system tagged system.
        approx(
            r.by_namespace
                .iter()
                .find(|n| n.namespace == "demo")
                .unwrap()
                .per_hour,
            0.50,
        );
        assert!(
            r.by_namespace
                .iter()
                .any(|n| n.namespace == "kube-system" && n.system)
        );
        approx(r.system_per_hour, 0.05);
        // by node: n1 = web + coredns = 0.25; n2 = db = 0.30 (the allocated cost).
        approx(r.by_node["n1"].per_hour, 0.25);
        approx(r.by_node["n2"].per_hour, 0.30);
        assert_eq!(r.by_node["n2"].basis, CostBasis::OpenCost);
        // costliest workload + kind mapping.
        assert_eq!(r.top_workloads[0].name, "db");
        assert_eq!(r.top_workloads[0].kind, WorkloadKind::StatefulSet);
    }
}
