//! DaemonSet coverage — which nodes are missing infrastructure the rest of the
//! fleet has.
//!
//! The substrate is what runs *under* your workloads: CNI, kube-proxy, log and
//! metric agents. A node quietly missing one looks healthy — its own pods are
//! fine — while lacking something every other node has.
//!
//! **Deliberately DaemonSet coverage and nothing else.** Kubelet pressure
//! (Memory/Disk/PID) is already carried by [`crate::state::saturation`] and
//! rendered by the Saturation overlay; routing it through here too would give
//! the map two paths to one fact.
//!
//! **Coverage is PRESENCE, not health** (as [`crate::state::netpol`] coverage is
//! isolation presence, not allow-rules): a crash-looping DaemonSet pod still
//! counts. Pod phase is deliberately not consulted — a transient restart would
//! otherwise flap a node into a false gap, and "what is broken" is the attention
//! queue's job. This view answers only "what is missing".

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::state::model::{OwnerIndex, WorkloadKind};
use crate::state::observed::ObservedWorld;

/// A DaemonSet on at least this share of nodes is treated as fleet-wide, so its
/// absence is a finding rather than a `nodeSelector` doing its job.
///
/// A heuristic, deliberately. The report never reads a DaemonSet's spec, so
/// prevalence is the only evidence available that it was MEANT to be everywhere
/// — `nodeSelector`, taints and node affinity all produce legitimate absences
/// that would otherwise be reported as gaps.
pub const FLEET_PREVALENCE: f64 = 0.8;

/// Whole-cluster DaemonSet coverage. One report feeds the overlay, the province
/// window and the SELECTION box, so they cannot disagree.
#[derive(Debug, Clone, Default)]
pub struct SubstrateReport {
    /// DaemonSets on at least [`FLEET_PREVALENCE`] of nodes, sorted.
    ///
    /// **`namespace/name`**, not a bare name: two DaemonSets in different
    /// namespaces may share a name, and identity here is load-bearing — merging
    /// them unions their node sets, which both hides real gaps (one covers the
    /// node the other misses) and manufactures false ones (two half-covered sets
    /// sum past the threshold). The qualified form is also what the operator
    /// needs to go act on it, and matches the app's kubectl-greppable grammar.
    ///
    /// Empty when the cluster has no fleet-wide DaemonSets at all — the overlay
    /// falls back to terrain in that case rather than colouring every node
    /// "clean", which would be an unearned all-clear.
    pub expected: Vec<String>,
    /// Per node, the expected DaemonSets it lacks (sorted). A node absent from
    /// this map is fully covered.
    pub missing_by_node: HashMap<String, Vec<String>>,
    pub nodes_total: usize,
    /// Nodes with at least one gap — counts NODES, not gaps, so a node missing
    /// three DaemonSets counts once.
    pub nodes_with_gaps: usize,
}

impl SubstrateReport {
    /// The expected DaemonSets this node lacks, or `&[]` when fully covered.
    pub fn missing(&self, node: &str) -> &[String] {
        self.missing_by_node.get(node).map_or(&[], Vec::as_slice)
    }
    /// Is there anything to show? An empty `expected` means the cluster has no
    /// fleet-wide substrate to be missing from.
    pub fn has_data(&self) -> bool {
        !self.expected.is_empty()
    }
}

/// PURE: derive DaemonSet coverage from the observed world.
///
/// **Unfiltered on purpose**, mirroring [`crate::state::netpol::coverage_report`]:
/// substrate coverage is a physical fact about the fleet, so it must not be
/// scoped by the active namespace view. (`world::build_world` also collects
/// DaemonSet names per province, but *that* one is gated on the filtered
/// workload list because it decides which roads to pave — reusing it here would
/// silently compute prevalence over a filtered subset and report phantom gaps.)
pub fn coverage_report(world: &ObservedWorld) -> SubstrateReport {
    // Every node, including ones carrying no pods at all — a node with nothing
    // on it is the most complete gap there is.
    let nodes: Vec<String> = world
        .nodes
        .state()
        .iter()
        .filter_map(|n| n.metadata.name.clone())
        .collect();
    if nodes.is_empty() {
        return SubstrateReport::default();
    }

    // node -> the DaemonSets (namespace-qualified) with a pod stationed on it.
    let live: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
    let idx = OwnerIndex::build(world);
    let mut on_node: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let pods = world.pods.state();
    for pod in pods.iter() {
        let Some(node) = pod.spec.as_ref().and_then(|s| s.node_name.as_deref()) else {
            continue; // unscheduled — belongs to no node's substrate
        };
        // A pod can outlive its Node object (autoscaler scale-down, `kubectl
        // delete node` — PodGC runs on a delay). Counting it would put the
        // prevalence NUMERATOR over a wider set than the `nodes.len()`
        // DENOMINATOR, inflating the ratio and promoting a DaemonSet past the
        // threshold on the strength of a node that no longer exists — which
        // then fabricates gaps on the live nodes that legitimately lack it.
        let Some(&node) = live.get(node) else {
            continue;
        };
        if let Some(w) = idx.workload_of(pod)
            && w.kind == WorkloadKind::DaemonSet
        {
            on_node
                .entry(node)
                .or_default()
                .insert(format!("{}/{}", w.namespace, w.name));
        }
    }

    // Prevalence: how many nodes carry each DaemonSet.
    let mut prevalence: BTreeMap<&str, usize> = BTreeMap::new();
    for sets in on_node.values() {
        for name in sets {
            *prevalence.entry(name.as_str()).or_default() += 1;
        }
    }
    let threshold = FLEET_PREVALENCE * nodes.len() as f64;
    // BTreeMap keys are already sorted, so `expected` is stable across frames.
    let expected: Vec<String> = prevalence
        .iter()
        .filter(|&(_, &n)| n as f64 >= threshold)
        .map(|(name, _)| (*name).to_string())
        .collect();

    let mut missing_by_node: HashMap<String, Vec<String>> = HashMap::new();
    for node in &nodes {
        let have = on_node.get(node.as_str());
        let gaps: Vec<String> = expected
            .iter()
            .filter(|e| have.is_none_or(|h| !h.contains(*e)))
            .cloned()
            .collect();
        if !gaps.is_empty() {
            missing_by_node.insert(node.clone(), gaps);
        }
    }

    SubstrateReport {
        expected,
        nodes_total: nodes.len(),
        nodes_with_gaps: missing_by_node.len(),
        missing_by_node,
    }
}

#[cfg(all(test, feature = "fixtures"))]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    /// Seed `nodes` nodes, and put DaemonSet `ds` on the first `on` of them.
    fn world_with(nodes: usize, sets: &[(&str, usize)]) -> ObservedWorld {
        let (world, mut s) = fx::world();
        for i in 0..nodes {
            s.node(fx::node(&format!("n{i}"), Some("z-a")));
        }
        for (ds, on) in sets {
            s.daemonset(fx::daemonset("kube-system", ds, *on as i32, *on as i32));
            for i in 0..*on {
                s.pod(fx::pod_owned(
                    fx::pod("kube-system", &format!("{ds}-{i}"), Some(&format!("n{i}"))),
                    "DaemonSet",
                    ds,
                ));
            }
        }
        world
    }

    #[test]
    fn a_daemonset_on_every_node_is_expected_and_nobody_has_gaps() {
        let r = coverage_report(&world_with(10, &[("cni", 10)]));
        assert_eq!(r.expected, vec!["kube-system/cni"]);
        assert_eq!(r.nodes_total, 10);
        assert_eq!(r.nodes_with_gaps, 0);
        assert!(r.missing_by_node.is_empty());
        assert!(r.has_data());
    }

    /// The `nodeSelector` case: a targeted DaemonSet must NOT become an
    /// expectation, or every node it deliberately skips becomes a false finding.
    #[test]
    fn a_targeted_daemonset_is_not_expected() {
        let r = coverage_report(&world_with(10, &[("gpu-plugin", 1)]));
        assert!(
            r.expected.is_empty(),
            "1-of-10 is a nodeSelector doing its job, not fleet-wide: {:?}",
            r.expected
        );
        assert_eq!(r.nodes_with_gaps, 0);
        assert!(
            !r.has_data(),
            "nothing fleet-wide ⇒ the overlay must fall back"
        );
    }

    #[test]
    fn nine_of_ten_is_expected_and_the_tenth_node_has_the_gap() {
        let r = coverage_report(&world_with(10, &[("cni", 9)]));
        assert_eq!(
            r.expected,
            vec!["kube-system/cni"],
            "90% ≥ the 80% threshold"
        );
        assert_eq!(r.nodes_with_gaps, 1);
        assert_eq!(r.missing("n9"), ["kube-system/cni"]);
        assert!(r.missing("n0").is_empty());
    }

    #[test]
    fn expected_is_sorted_so_rendering_is_stable() {
        let r = coverage_report(&world_with(4, &[("zeta", 4), ("alpha", 4), ("mid", 4)]));
        assert_eq!(
            r.expected,
            vec!["kube-system/alpha", "kube-system/mid", "kube-system/zeta"]
        );
    }

    /// Counts NODES, not gaps — one badly-provisioned node is one finding.
    #[test]
    fn nodes_with_gaps_counts_nodes_not_gaps() {
        // Three fleet-wide DaemonSets, all absent from the last node.
        let r = coverage_report(&world_with(5, &[("a", 4), ("b", 4), ("c", 4)]));
        assert_eq!(
            r.expected,
            vec!["kube-system/a", "kube-system/b", "kube-system/c"]
        );
        assert_eq!(r.missing("n4").len(), 3, "that node lacks all three");
        assert_eq!(r.nodes_with_gaps, 1, "but it is ONE node with gaps");
    }

    #[test]
    fn empty_and_daemonset_free_clusters_produce_nothing_and_do_not_panic() {
        let (empty, _) = fx::world();
        let r = coverage_report(&empty);
        assert_eq!(r.nodes_total, 0);
        assert!(r.expected.is_empty() && !r.has_data());

        let r = coverage_report(&world_with(3, &[]));
        assert_eq!(r.nodes_total, 3);
        assert!(r.expected.is_empty(), "no DaemonSets ⇒ nothing expected");
        assert_eq!(r.nodes_with_gaps, 0);
    }

    /// Identity is the (namespace, name) PAIR, not the bare name. Merging two
    /// same-named DaemonSets unions their node sets, which breaks BOTH ways —
    /// so this pins both directions.
    #[test]
    fn same_named_daemonsets_in_different_namespaces_are_not_merged() {
        let ds = |s: &mut fx::Seeds, ns: &str, name: &str, on: &[usize]| {
            s.daemonset(fx::daemonset(ns, name, on.len() as i32, on.len() as i32));
            for i in on {
                s.pod(fx::pod_owned(
                    fx::pod(ns, &format!("{ns}-{name}-{i}"), Some(&format!("n{i}"))),
                    "DaemonSet",
                    name,
                ));
            }
        };

        // (a) HIDDEN GAP: monitoring/agent is genuinely fleet-wide (4/5) and
        // absent from n4; an unrelated tenant-a/agent happens to run ONLY on n4.
        // Merged, n4 would read "fully covered" — the unearned all-clear.
        let (world, mut s) = fx::world();
        for i in 0..5 {
            s.node(fx::node(&format!("n{i}"), Some("z-a")));
        }
        ds(&mut s, "monitoring", "agent", &[0, 1, 2, 3]);
        ds(&mut s, "tenant-a", "agent", &[4]);
        let r = coverage_report(&world);
        assert_eq!(
            r.expected,
            vec!["monitoring/agent"],
            "only the fleet-wide one"
        );
        assert_eq!(
            r.missing("n4"),
            ["monitoring/agent"],
            "the tenant's same-named daemonset must not cover the fleet's gap"
        );
        assert_eq!(r.nodes_with_gaps, 1);

        // (b) MANUFACTURED EXPECTATION: two half-covered same-named sets. Neither
        // is fleet-wide (4/10 each), but merged they read 8/10 = 80% and would
        // flag the two nodes running neither — precisely the nodeSelector false
        // positive FLEET_PREVALENCE exists to prevent.
        let (world, mut s) = fx::world();
        for i in 0..10 {
            s.node(fx::node(&format!("n{i}"), Some("z-a")));
        }
        ds(&mut s, "monitoring", "node-exporter", &[0, 1, 2, 3]);
        ds(&mut s, "legacy", "node-exporter", &[4, 5, 6, 7]);
        let r = coverage_report(&world);
        assert!(
            r.expected.is_empty(),
            "neither set is fleet-wide; merging them invents one: {:?}",
            r.expected
        );
        assert_eq!(r.nodes_with_gaps, 0);
    }

    /// The prevalence NUMERATOR must range over the same set as the
    /// `nodes.len()` DENOMINATOR. A pod can outlive its Node object (autoscaler
    /// scale-down; PodGC runs on a delay), and counting it inflates the ratio.
    #[test]
    fn a_pod_outliving_its_node_does_not_inflate_prevalence() {
        let (world, mut s) = fx::world();
        for i in 0..5 {
            s.node(fx::node(&format!("n{i}"), Some("z-a")));
        }
        s.daemonset(fx::daemonset("kube-system", "cni", 3, 3));
        for i in 0..3 {
            s.pod(fx::pod_owned(
                fx::pod("kube-system", &format!("cni-{i}"), Some(&format!("n{i}"))),
                "DaemonSet",
                "cni",
            ));
        }
        // 3 of 5 = 60%, under the bar: nothing to report.
        assert!(coverage_report(&world).expected.is_empty());

        // A pod bound to a node the store no longer has. It must not count: 4
        // would clear the 0.8*5 threshold and fabricate gaps on n3 and n4.
        s.pod(fx::pod_owned(
            fx::pod("kube-system", "cni-ghost", Some("n-deleted")),
            "DaemonSet",
            "cni",
        ));
        let r = coverage_report(&world);
        assert!(
            r.expected.is_empty(),
            "a departed node must not vote on what the fleet runs: {:?}",
            r.expected
        );
        assert_eq!(r.nodes_total, 5, "and never enters the denominator either");
        assert_eq!(r.nodes_with_gaps, 0);
    }

    /// Small clusters under-report, by design — and up to 4 nodes they report
    /// NOTHING, which is worth pinning because it is sharper than "under-report"
    /// and it is why a 4-node dev cluster looks permanently clean.
    ///
    /// At n nodes a DaemonSet is expected at `ceil(0.8n)` and a gap needs it on
    /// *fewer* than n. For n ≤ 4, `ceil(0.8n) == n` — being expected and having
    /// a gap are mutually exclusive, so no gap is representable at all. n = 5
    /// (threshold 4) is the smallest fleet where one can be reported.
    #[test]
    fn no_gap_is_representable_below_five_nodes() {
        for n in 1..=4 {
            for on in 1..=n {
                let r = coverage_report(&world_with(n, &[("cni", on)]));
                assert_eq!(
                    r.nodes_with_gaps, 0,
                    "{n} nodes, daemonset on {on}: sub-5 fleets can't express a gap"
                );
            }
        }
        // 3 of 4 = 75%, under the bar — not even expected.
        assert!(
            coverage_report(&world_with(4, &[("cni", 3)]))
                .expected
                .is_empty()
        );
        // 4 of 5 = 80% — expected, and the fifth node is a real finding.
        let five = coverage_report(&world_with(5, &[("cni", 4)]));
        assert_eq!(five.expected, vec!["kube-system/cni"]);
        assert_eq!(
            five.missing("n4"),
            ["kube-system/cni"],
            "the first reportable gap"
        );
    }
}
