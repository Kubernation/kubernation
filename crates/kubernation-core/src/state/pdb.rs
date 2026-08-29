//! PodDisruptionBudgets — *can this node be drained?*
//!
//! A node is drainable when nothing refuses to give up a pod running on it. The
//! apiserver enforces that constraint on the `pods/eviction` subresource, which
//! is what `actions::evict_pod` now uses; this module *describes* the same
//! constraint ahead of time, so the map can say why a drain would stall.
//!
//! Three honesty rules, in order of how badly getting them wrong would mislead.
//!
//! **1. Unprotected is not unknown.** An empty PDB store means "no budgets" only
//! once the reflector has completed a list. Until then — and forever, if RBAC
//! denied the LIST — it means nothing was read. Reporting a node drainable on
//! the strength of a denied LIST is the unearned all-clear this codebase refuses
//! everywhere, so every entry point takes [`ObservedWorld::pdbs_observed`] first
//! and reports [`Drain::Unknown`] when it is false.
//!
//! **2. A stale `disruptionsAllowed` is not a number.** The API is explicit:
//! *"DisruptionsAllowed and other status information is valid only if
//! observedGeneration equals to PDB's object generation."* A budget the
//! disruption controller has not caught up with — or has never reconciled, so
//! there is no status at all — cannot say how much headroom it has. That is a
//! per-budget unknown, and it makes the node unknown rather than drainable.
//!
//! **3. A null selector is not an empty one.** `policy/v1` says: *"A null
//! selector will match no pods, while an empty ({}) selector will select all
//! pods within the namespace."* [`netpol::selector_matches`] answers the
//! opposite for `None`, because a NetworkPolicy's absent `podSelector` IS
//! namespace-wide. The expression semantics are shared — matchLabels,
//! matchExpressions, and the fail-closed unknown operator — and the null case is
//! handled here, by the caller that knows which resource it is reading.
//!
//! **What this does NOT claim.** `disruptionsAllowed` is a cluster-wide,
//! moment-in-time count for a budget, not a per-node allowance. `Drain::Allowed`
//! means no covering budget refuses *right now*, which is exactly what the next
//! eviction will meet — not that a whole drain will run to completion. A budget
//! allowing one disruption will refuse the second pod on a node holding three.

use std::collections::{BTreeMap, HashMap};

use k8s_openapi::api::policy::v1::PodDisruptionBudget;

use super::model::pod_terminal;
use super::netpol::selector_matches;
use super::observed::ObservedWorld;

/// A budget, named the way an operator would go find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRef {
    pub namespace: String,
    pub name: String,
    /// How many pods on the node in question this budget covers.
    pub pods_here: usize,
}

impl BudgetRef {
    pub fn label(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

/// Whether a node's pods can currently be given up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drain {
    /// No covering budget refuses a disruption. (See the module's "does NOT
    /// claim" note: this is about the next eviction, not the whole drain.)
    Allowed,
    /// At least one covering budget currently allows zero disruptions.
    Blocked,
    /// The budgets were not read, or a covering budget's headroom is not valid.
    Unknown,
}

/// One node's drain constraint.
#[derive(Debug, Clone)]
pub struct NodeDrain {
    pub node: String,
    pub state: Drain,
    /// Budgets covering pods here that currently refuse. Non-empty exactly when
    /// `state == Blocked`.
    pub blocking: Vec<BudgetRef>,
    /// Budgets covering pods here whose headroom could not be trusted (rule 2).
    /// These are why a node reads `Unknown` rather than `Allowed`.
    pub unreadable: Vec<BudgetRef>,
}

impl NodeDrain {
    /// One line for a panel or a concern's detail. Names the budgets, because
    /// "blocked" without a name leaves the operator exactly where they started.
    pub fn detail(&self) -> String {
        match self.state {
            Drain::Allowed => "no budget blocks a drain".into(),
            Drain::Blocked => {
                let names: Vec<String> = self.blocking.iter().map(|b| b.label()).collect();
                format!("draining blocked by {}", names.join(", "))
            }
            Drain::Unknown if self.unreadable.is_empty() => {
                "disruption budgets not read - drain cost unknown".into()
            }
            Drain::Unknown => {
                let names: Vec<String> = self.unreadable.iter().map(|b| b.label()).collect();
                format!("budget headroom unknown for {}", names.join(", "))
            }
        }
    }
}

/// Per-node drain constraints, plus whether the budgets were read at all.
#[derive(Debug, Clone, Default)]
pub struct DrainReport {
    /// False when the PDB store never completed a list. Every node is then
    /// `Unknown`, and a surface must say so rather than "no budgets".
    pub observed: bool,
    /// How many budgets were seen. Meaningless when `!observed`.
    pub budgets: usize,
    nodes: HashMap<String, NodeDrain>,
}

impl DrainReport {
    /// The constraint on one node. `None` for a node not in the report at all
    /// (it has no pods, or is not in the store) — deliberately not an invented
    /// `Allowed`, which would be a claim about a node this never examined.
    pub fn node(&self, name: &str) -> Option<&NodeDrain> {
        self.nodes.get(name)
    }

    /// Nodes a drain would currently stall on, sorted by name.
    pub fn blocked(&self) -> Vec<&NodeDrain> {
        let mut v: Vec<&NodeDrain> = self
            .nodes
            .values()
            .filter(|n| n.state == Drain::Blocked)
            .collect();
        v.sort_by(|a, b| a.node.cmp(&b.node));
        v
    }
}

/// Does this budget cover a pod carrying `labels`?
///
/// Rule 3: a **null** selector matches nothing, which is the reverse of
/// [`selector_matches`]'s `None` case. Everything else — matchLabels,
/// matchExpressions, and the fail-closed unknown operator — is shared with the
/// walls overlay rather than written twice.
fn covers(pdb: &PodDisruptionBudget, labels: &BTreeMap<String, String>) -> bool {
    let Some(sel) = pdb.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
        return false; // policy/v1: a null selector will match no pods
    };
    selector_matches(Some(sel), labels)
}

/// A budget's headroom — `Some(allowed)` only when the status is valid.
///
/// Rule 2. `None` means the disruption controller has not caught up (or has
/// never run), so `disruptionsAllowed` describes a different generation of this
/// object and must not be read as a number.
fn headroom(pdb: &PodDisruptionBudget) -> Option<i32> {
    let status = pdb.status.as_ref()?;
    let observed = status.observed_generation?;
    let generation = pdb.metadata.generation?;
    (observed == generation).then_some(status.disruptions_allowed)
}

/// Whether this budget could refuse an eviction — the only budgets worth
/// matching against pods.
///
/// A budget with headroom to spare cannot change any node's verdict, so it is
/// filtered out **before** the pod walk. That is what keeps the cost of this
/// derivation proportional to the trouble on the cluster rather than to its
/// size: a healthy realm does no per-pod work at all.
fn could_refuse(pdb: &PodDisruptionBudget) -> bool {
    !matches!(headroom(pdb), Some(n) if n > 0)
}

/// Build the per-node drain constraints.
///
/// Cluster-wide and unfiltered, like `netpol::coverage_report` and
/// `substrate::coverage_report`: whether a node can be drained is a physical
/// fact about the fleet, not a namespace view.
pub fn drain_report(world: &ObservedWorld) -> DrainReport {
    // Seed from the NODE store, not from the pods: a node running nothing is
    // trivially drainable, and a report built only from pods would be silent
    // about it — which a surface cannot tell apart from "not examined".
    let mut nodes: HashMap<String, NodeDrain> = world
        .nodes
        .state()
        .iter()
        .filter_map(|n| n.metadata.name.clone())
        .map(|node| {
            (
                node.clone(),
                NodeDrain {
                    node,
                    state: Drain::Allowed,
                    blocking: Vec::new(),
                    unreadable: Vec::new(),
                },
            )
        })
        .collect();

    if !world.pdbs_observed() {
        // Rule 1. Nothing was read, so nothing is known — including about a node
        // with no pods, whose drainability we would otherwise be asserting from
        // a fact we do not have.
        for n in nodes.values_mut() {
            n.state = Drain::Unknown;
        }
        return DrainReport {
            observed: false,
            budgets: 0,
            nodes,
        };
    }

    let all = world.pdbs.state();
    // Only budgets that could refuse; indexed by namespace, since a budget only
    // ever covers pods in its own.
    let mut by_ns: HashMap<&str, Vec<&PodDisruptionBudget>> = HashMap::new();
    for p in all.iter().filter(|p| could_refuse(p)) {
        let Some(ns) = p.metadata.namespace.as_deref() else {
            continue;
        };
        by_ns.entry(ns).or_default().push(p);
    }

    if by_ns.is_empty() {
        // Nothing on this cluster could refuse, so no pod can be covered by a
        // budget that matters and every seeded node stands as Allowed. Skipping
        // the walk is what makes the healthy case cost nothing — without it this
        // iterates every pod on the fleet to reach the same answer.
        return DrainReport {
            observed: true,
            budgets: all.len(),
            nodes,
        };
    }

    // Pods of one workload share a label set exactly, so which budgets cover a
    // pod is a function of (namespace, labels) and not of the pod. Memoizing it
    // turns the worst case — every workload at its limit — from
    // O(pods x budgets) into O(distinct label sets x budgets), measured at a
    // O(distinct label sets x budgets). Measured, not assumed: it took the
    // 5000-pod worst case from 9.6ms to 3.4ms (`drain_report_cost_at_scale`).
    // The remainder is the pod walk itself, not selector evaluation.
    let pods = world.pods.state();
    let empty = BTreeMap::new();
    let mut covered_by: HashMap<(&str, &BTreeMap<String, String>), Vec<usize>> = HashMap::new();
    // Keyed by (node, namespace, index into that namespace's candidates) so the
    // hot loop allocates nothing; the names are resolved once in the fold below.
    let mut hits: HashMap<(&str, &str, usize), usize> = HashMap::new();
    for pod in pods.iter() {
        let Some(node) = pod.spec.as_ref().and_then(|s| s.node_name.as_deref()) else {
            continue; // unscheduled: not on any node, so not this node's problem
        };
        if pod_terminal(pod) {
            continue; // a finished pod is not a disruption
        }
        let Some(ns) = pod.metadata.namespace.as_deref() else {
            continue;
        };
        let Some(candidates) = by_ns.get(ns) else {
            continue;
        };
        let labels = pod.metadata.labels.as_ref().unwrap_or(&empty);
        let matched = covered_by.entry((ns, labels)).or_insert_with(|| {
            candidates
                .iter()
                .enumerate()
                .filter(|(_, p)| covers(p, labels))
                .map(|(i, _)| i)
                .collect()
        });
        for &i in matched.iter() {
            *hits.entry((node, ns, i)).or_insert(0) += 1;
        }
    }

    // Fold the hits into each node, splitting refusal from unreadability.
    let refuses: HashMap<(&str, &str), bool> = all
        .iter()
        .filter_map(|p| {
            let ns = p.metadata.namespace.as_deref()?;
            let name = p.metadata.name.as_deref()?;
            Some(((ns, name), headroom(p) == Some(0)))
        })
        .collect();
    for ((node, ns, i), pods_here) in hits {
        // A pod scheduled to a node the store has not seen has nowhere to be
        // reported — it is not on the map either.
        let Some(entry) = nodes.get_mut(node) else {
            continue;
        };
        let name = by_ns[ns][i].metadata.name.as_deref().unwrap_or_default();
        let r = BudgetRef {
            namespace: ns.to_string(),
            name: name.to_string(),
            pods_here,
        };
        if *refuses.get(&(ns, name)).unwrap_or(&false) {
            entry.blocking.push(r);
        } else {
            entry.unreadable.push(r);
        }
    }
    for n in nodes.values_mut() {
        n.blocking.sort_by_key(|b| b.label());
        n.unreadable.sort_by_key(|b| b.label());
        // A definite refusal outranks an unreadable one: we know it is blocked.
        n.state = if !n.blocking.is_empty() {
            Drain::Blocked
        } else if !n.unreadable.is_empty() {
            Drain::Unknown
        } else {
            Drain::Allowed
        };
    }

    DrainReport {
        observed: true,
        budgets: all.len(),
        nodes,
    }
}

/// "draining blocked by demo/web-strict" — when that is worth saying.
///
/// PURE, unit-tested. The `pool_confinement` shape: a fact appended to an
/// existing concern's `detail`, riding the sidebar, the Oracle bundle and the
/// postmortem for free, rather than a concern of its own.
///
/// **Why not a concern of its own.** A budget written `minAvailable: 3` on a
/// 3-replica workload sits at `disruptionsAllowed: 0` permanently and by design.
/// A concern per blocked node would therefore squat the queue forever on a
/// perfectly healthy cluster — the failure the hardening round had to fix by
/// excluding system namespaces, in a form no exclusion could reach. So this
/// enriches the node concern that already exists.
///
/// **Why only a cordoned node.** The queue answers *what needs orders*, and a
/// blocked drain is only an obstacle to someone draining. A cordon is the
/// observable signal that the operator is taking the node out of service; a
/// blocked budget on a node nobody is touching is a standing fact, which the
/// province panel reports unconditionally.
///
/// `None` for [`Drain::Allowed`] — a caveat on a value that carries none is
/// noise, the rule `pool_line` and `extent_line` already follow.
pub fn drain_note(d: &NodeDrain) -> Option<String> {
    match d.state {
        Drain::Allowed => None,
        _ => Some(d.detail()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    /// A pod on `node` carrying `app=<app>`.
    fn labelled_pod(
        ns: &str,
        name: &str,
        node: &str,
        app: &str,
    ) -> k8s_openapi::api::core::v1::Pod {
        let mut p = fx::pod(ns, name, Some(node));
        p.metadata.labels = Some(BTreeMap::from([("app".to_string(), app.to_string())]));
        p
    }

    fn world_with_pods() -> (ObservedWorld, fx::Seeds) {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.node(fx::node("n2", Some("z-a")));
        s.pod(labelled_pod("demo", "web-1", "n1", "web"));
        s.pod(labelled_pod("demo", "web-2", "n2", "web"));
        s.pod(labelled_pod("demo", "other-1", "n2", "other"));
        (world, s)
    }

    #[test]
    fn a_blocking_budget_names_itself_on_the_nodes_it_covers() {
        let (world, mut s) = world_with_pods();
        s.pdb(fx::pdb("demo", "web-strict", &[("app", "web")], 0));
        let r = drain_report(&world);
        assert!(r.observed);
        assert_eq!(r.node("n1").unwrap().state, Drain::Blocked);
        assert_eq!(r.node("n2").unwrap().state, Drain::Blocked);
        assert_eq!(r.node("n1").unwrap().blocking[0].label(), "demo/web-strict");
        assert_eq!(r.node("n1").unwrap().blocking[0].pods_here, 1);
        assert!(
            r.node("n1").unwrap().detail().contains("web-strict"),
            "a refusal that does not name the budget leaves the operator where they started"
        );
    }

    #[test]
    fn a_permissive_budget_leaves_the_node_drainable() {
        let (world, mut s) = world_with_pods();
        s.pdb(fx::pdb("demo", "web-loose", &[("app", "web")], 1));
        let r = drain_report(&world);
        assert_eq!(r.node("n1").unwrap().state, Drain::Allowed);
        assert!(r.node("n1").unwrap().blocking.is_empty());
    }

    /// §3.3, the phase's central constraint: an unprotected cluster and an
    /// unread one must not produce the same answer.
    #[test]
    fn unprotected_is_not_unknown() {
        let (world, _s) = world_with_pods();
        let read = drain_report(&world);
        assert!(read.observed);
        assert_eq!(read.node("n1").unwrap().state, Drain::Allowed);

        let (world, mut s) = world_with_pods();
        s.pdbs_unread();
        let unread = drain_report(&world);
        assert!(!unread.observed);
        assert_eq!(unread.node("n1").unwrap().state, Drain::Unknown);
        assert!(
            unread.node("n1").unwrap().detail().contains("not read"),
            "an unread store must say so, not report 'no budget blocks a drain'"
        );
    }

    /// Rule 2. The API: "DisruptionsAllowed ... valid only if observedGeneration
    /// equals to PDB's object generation."
    #[test]
    fn a_stale_status_is_unknown_not_a_number() {
        let (world, mut s) = world_with_pods();
        // Headroom of 5 — but describing generation 1 while the object is at 7.
        s.pdb(fx::pdb_stale("demo", "web-lagging", &[("app", "web")]));
        let r = drain_report(&world);
        assert_eq!(r.node("n1").unwrap().state, Drain::Unknown);
        assert_eq!(
            r.node("n1").unwrap().unreadable[0].label(),
            "demo/web-lagging"
        );
        assert!(r.node("n1").unwrap().blocking.is_empty());

        // A budget the controller has never reconciled at all.
        let (world, mut s) = world_with_pods();
        let mut never = fx::pdb("demo", "web-new", &[("app", "web")], 3);
        never.status = None;
        s.pdb(never);
        assert_eq!(
            drain_report(&world).node("n1").unwrap().state,
            Drain::Unknown
        );
    }

    /// A definite refusal outranks an unreadable budget — we know it is blocked.
    #[test]
    fn a_refusal_outranks_an_unreadable_budget() {
        let (world, mut s) = world_with_pods();
        s.pdb(fx::pdb("demo", "web-strict", &[("app", "web")], 0));
        s.pdb(fx::pdb_stale("demo", "web-lagging", &[("app", "web")]));
        let n = drain_report(&world);
        let n = n.node("n1").unwrap();
        assert_eq!(n.state, Drain::Blocked);
        assert_eq!(n.blocking.len(), 1);
        assert_eq!(n.unreadable.len(), 1);
    }

    /// Rule 3, and the reason `covers` exists rather than calling
    /// `selector_matches` directly: policy/v1 says a null selector matches NO
    /// pods, where a NetworkPolicy's absent podSelector is namespace-wide.
    #[test]
    fn a_null_selector_covers_nothing() {
        let (world, mut s) = world_with_pods();
        s.pdb(fx::pdb_null_selector("demo", "web-null", 0));
        assert_eq!(
            drain_report(&world).node("n1").unwrap().state,
            Drain::Allowed
        );

        // ...while an EMPTY selector is namespace-wide, and does block.
        let (world, mut s) = world_with_pods();
        s.pdb(fx::pdb("demo", "web-all", &[], 0));
        assert_eq!(
            drain_report(&world).node("n1").unwrap().state,
            Drain::Blocked
        );
    }

    /// Selector semantics are `netpol`'s, not a second implementation — so the
    /// fail-closed unknown operator holds here too.
    #[test]
    fn selector_semantics_agree_with_netpol() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::{
            LabelSelector, LabelSelectorRequirement,
        };
        let labels = BTreeMap::from([("app".to_string(), "web".to_string())]);
        let bogus = LabelSelector {
            match_expressions: Some(vec![LabelSelectorRequirement {
                key: "app".into(),
                operator: "Sideways".into(),
                values: None,
            }]),
            ..Default::default()
        };
        let mut p = fx::pdb("demo", "x", &[], 0);
        p.spec.as_mut().unwrap().selector = Some(bogus.clone());
        assert_eq!(
            covers(&p, &labels),
            selector_matches(Some(&bogus), &labels),
            "PDB coverage must not diverge from the walls overlay on selector semantics"
        );
        assert!(!covers(&p, &labels), "an unknown operator must fail closed");
    }

    /// The coverage memo is keyed on (namespace, labels) and holds indices into
    /// THAT namespace's candidate list. Two namespaces routinely run pods with
    /// identical labels — `app=web` is the most common label in Kubernetes — so
    /// a key that drops the namespace hands one namespace's answer to another,
    /// naming a budget that does not cover the pod at all.
    ///
    /// The single-namespace fixtures above cannot see this: with one candidate
    /// list, every index means the same thing.
    #[test]
    fn identical_labels_in_two_namespaces_do_not_share_an_answer() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.node(fx::node("n2", Some("z-a")));
        // Both namespaces run `app=web`; only team-a's budget covers it.
        s.pod(labelled_pod("team-a", "web-1", "n1", "web"));
        s.pod(labelled_pod("team-b", "web-1", "n2", "web"));
        s.pdb(fx::pdb("team-a", "a-strict", &[("app", "web")], 0));
        s.pdb(fx::pdb("team-b", "b-strict", &[("app", "other")], 0));

        let r = drain_report(&world);
        assert_eq!(r.node("n1").unwrap().state, Drain::Blocked);
        assert_eq!(r.node("n1").unwrap().blocking[0].label(), "team-a/a-strict");
        assert_eq!(
            r.node("n2").unwrap().state,
            Drain::Allowed,
            "team-b's budget selects app=other; its web pod is not covered"
        );
    }

    #[test]
    fn a_budget_in_another_namespace_covers_nothing_here() {
        let (world, mut s) = world_with_pods();
        s.pdb(fx::pdb("elsewhere", "web-strict", &[("app", "web")], 0));
        assert_eq!(
            drain_report(&world).node("n1").unwrap().state,
            Drain::Allowed
        );
    }

    #[test]
    fn a_terminal_pod_is_not_a_disruption() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let mut done = labelled_pod("demo", "job-1", "n1", "web");
        done.status.as_mut().unwrap().phase = Some("Succeeded".into());
        s.pod(done);
        s.pdb(fx::pdb("demo", "web-strict", &[("app", "web")], 0));
        assert_eq!(
            drain_report(&world).node("n1").unwrap().state,
            Drain::Allowed
        );
    }

    /// A node running nothing is trivially drainable — and must say so rather
    /// than being absent, which a surface cannot tell from "not examined".
    #[test]
    fn an_empty_node_is_reported_drainable() {
        let (world, mut s) = fx::world();
        s.node(fx::node("idle", Some("z-a")));
        let r = drain_report(&world);
        assert_eq!(r.node("idle").unwrap().state, Drain::Allowed);
        assert!(r.node("no-such-node").is_none());
    }

    /// §3.2 asks for the derivation's cost to be measured, not predicted — and
    /// measured on the case that costs, not the one that does not. Every
    /// workload gets a REFUSING budget, so the per-pod matching runs at full
    /// width: 5000 pods against the budgets of their namespace.
    #[test]
    fn drain_report_cost_at_scale() {
        use std::time::Instant;
        let (world, mut s) = fx::world();
        for i in 0..500 {
            s.node(fx::node(
                &format!("node-{i}"),
                Some(&format!("z-{}", i % 25)),
            ));
        }
        for w in 0..100 {
            let app = format!("app-{w}");
            s.pdb(fx::pdb("demo", &app, &[("app", &app)], 0));
            for p in 0..50 {
                s.pod(labelled_pod(
                    "demo",
                    &format!("{app}-{p}"),
                    &format!("node-{}", (w * 50 + p) % 500),
                    &app,
                ));
            }
        }
        let r = drain_report(&world);
        assert_eq!(r.budgets, 100);
        assert_eq!(r.blocked().len(), 500, "every node holds a covered pod");

        let iters = 5;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = drain_report(&world);
        }
        let per = start.elapsed() / iters;
        println!("drain_report: 500 nodes / 5000 pods / 100 blocking budgets → {per:?}");
        if !cfg!(debug_assertions) {
            // The whole model rebuild's budget is 100ms; this is one derivation
            // inside it, on a cluster where every workload is at its limit.
            assert!(per.as_millis() < 25, "drain_report over budget: {per:?}");
        }
    }
}
