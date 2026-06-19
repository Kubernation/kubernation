//! NetworkPolicy coverage — the "unwalled cities" segmentation map (OWASP K07,
//! Missing Network Segmentation Controls). PURE, read-only.
//!
//! A workload is **walled (for ingress)** when ≥1 NetworkPolicy in its namespace
//! selects its pods (podSelector match) and that policy's *effective* policyTypes
//! include "Ingress". Coverage means **isolation presence** — a deny-by-default
//! wall exists — NOT what the rules allow (an `ipBlock`-only ingress rule still
//! isolates). A workload selected by no ingress policy is **unwalled**: it accepts
//! traffic from every pod in the cluster (the lateral-movement risk). Egress is
//! tracked the same way but is advisory-only (egress posture is often intentional).
//!
//! Complements (does NOT duplicate) the chaos `apply_partition` *write*, which
//! adds a deny-all policy to break connectivity — this only *reads* where walls
//! are missing. Honest limits (stated in the UI): `namespaceSelector` / `ipBlock`
//! / port-level allow-analysis are not inspected; CNI *enforcement* is not
//! verified (a policy on a non-enforcing CNI reads "walled" but isn't); Cilium/
//! Calico CRD policies are not read. Matching is against the workload's
//! **pod-template** labels (like `build_exposure`), so a policy keyed on a
//! pod-only label (e.g. `pod-template-hash`) reads as unwalled. The selector
//! match **fails closed** (an un-evaluable selector ⇒ no match ⇒ unwalled) —
//! never a silent false "walled".

use std::collections::{BTreeMap, HashMap, HashSet};

use k8s_openapi::api::networking::v1::{NetworkPolicy, NetworkPolicySpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;

use super::model::{WorkloadRef, build_exposure, build_workloads, workload_template_labels};
use super::observed::ObservedWorld;

/// Per-workload isolation. Headline "walled" = ingress (the K07 direction).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Coverage {
    pub ingress: bool,
    pub egress: bool,
}

impl Coverage {
    pub fn walled(self) -> bool {
        self.ingress
    }
}

/// One workload's wall row — the advisor, the queue, and the city-mark read this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallRow {
    pub r: WorkloadRef,
    pub cov: Coverage,
    /// Reachable: fronted by a Service harbor or Ingress gate (`build_exposure`).
    pub exposed: bool,
    /// Names of the policies that wall it (for the advisor detail).
    pub policies: Vec<String>,
}

impl WallRow {
    pub fn unwalled_ingress(&self) -> bool {
        !self.cov.ingress
    }
    /// The K07 finding: routable from outside its own pods, with no ingress wall.
    pub fn critical(&self) -> bool {
        !self.cov.ingress && self.exposed
    }
}

/// Per-namespace rollup, including the wide-open (zero-policy) continents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NsRollup {
    pub namespace: String,
    pub policies: usize,
    pub workloads: usize,
    pub walled: usize,
    /// `policies == 0 && workloads > 0` — a continent with no walls at all.
    pub wide_open: bool,
}

/// The whole-cluster segmentation report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetpolReport {
    pub policies: usize,
    pub workloads: usize,
    pub walled_ingress: usize,
    pub egress_isolated: usize,
    /// Every Deploy/STS/DS, sorted by namespace/name (the map-join order).
    pub rows: Vec<WallRow>,
    /// The concern source — unwalled AND exposed, sorted by namespace/name.
    pub unwalled_exposed: Vec<WallRow>,
    pub unwalled_unexposed: usize,
    /// Zero-policy namespaces with ≥1 workload.
    pub open_namespaces: Vec<NsRollup>,
}

/// A concern's shape at the net boundary (keeps core attention-enum-free, like
/// `harden::workload_concern`). `net.rs` turns this into an attention `Concern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetpolConcern {
    pub title: String,
    pub detail: String,
    pub key: String,
}

/// The concern for an unwalled-and-exposed workload, else `None`.
pub fn workload_concern(row: &WallRow) -> Option<NetpolConcern> {
    if !row.critical() {
        return None;
    }
    Some(NetpolConcern {
        title: format!("{} {}/{}", row.r.kind, row.r.namespace, row.r.name),
        detail: "unwalled — exposed, but no NetworkPolicy isolates its ingress".into(),
        key: format!("netpol:{}/{}", row.r.namespace, row.r.name),
    })
}

/// Per-workload coverage lookup for the map (hung on `Models`, mirroring
/// `workload_severity`). The overlay, the city breach-mark, and the advisor all
/// read coverage derived from the same builder so they cannot disagree.
pub fn coverage_map(world: &ObservedWorld) -> HashMap<WorkloadRef, Coverage> {
    coverage_report(world)
        .rows
        .into_iter()
        .map(|row| (row.r, row.cov))
        .collect()
}

/// THE pure builder: every workload's wall coverage + namespace rollups.
pub fn coverage_report(world: &ObservedWorld) -> NetpolReport {
    // Index policies by namespace, pre-resolving effective (ingress, egress).
    let mut by_ns: BTreeMap<String, Vec<(&NetworkPolicy, bool, bool)>> = BTreeMap::new();
    let policies = world.networkpolicies.state();
    for np in &policies {
        let Some(ns) = np.metadata.namespace.clone() else {
            continue;
        };
        let Some(spec) = np.spec.as_ref() else {
            continue;
        };
        let (ing, eg) = effective_policy_types(spec);
        by_ns.entry(ns).or_default().push((np.as_ref(), ing, eg));
    }

    // Which workloads are reachable (Service/Ingress-fronted).
    let exposed: HashSet<WorkloadRef> = build_exposure(world)
        .into_iter()
        .map(|e| e.workload)
        .collect();

    let mut rows: Vec<WallRow> = Vec::new();
    // Per-namespace workload counts (for the wide-open rollups).
    let mut ns_workloads: BTreeMap<String, usize> = BTreeMap::new();

    for w in build_workloads(world) {
        let wr = w.r;
        let labels = workload_template_labels(world, &wr);
        let mut cov = Coverage::default();
        let mut names: Vec<String> = Vec::new();
        if let Some(nps) = by_ns.get(&wr.namespace) {
            for (np, ing, eg) in nps {
                let spec = np.spec.as_ref().expect("indexed only specs with Some");
                if selector_matches(spec.pod_selector.as_ref(), &labels) {
                    cov.ingress |= *ing;
                    cov.egress |= *eg;
                    if let Some(n) = np.metadata.name.clone() {
                        names.push(n);
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        *ns_workloads.entry(wr.namespace.clone()).or_default() += 1;
        let is_exposed = exposed.contains(&wr);
        rows.push(WallRow {
            r: wr,
            cov,
            exposed: is_exposed,
            policies: names,
        });
    }
    rows.sort_by(|a, b| {
        a.r.namespace
            .cmp(&b.r.namespace)
            .then(a.r.name.cmp(&b.r.name))
    });

    let walled_ingress = rows.iter().filter(|r| r.cov.ingress).count();
    let egress_isolated = rows.iter().filter(|r| r.cov.egress).count();
    let unwalled_exposed: Vec<WallRow> = rows.iter().filter(|r| r.critical()).cloned().collect();
    let unwalled_unexposed = rows
        .iter()
        .filter(|r| r.unwalled_ingress() && !r.exposed)
        .count();

    // Per-namespace policy counts (incl. namespaces with policies but no workloads
    // are irrelevant; we only roll up namespaces that have workloads).
    let mut ns_policies: BTreeMap<String, usize> = BTreeMap::new();
    for np in &policies {
        if let Some(ns) = np.metadata.namespace.clone() {
            *ns_policies.entry(ns).or_default() += 1;
        }
    }
    let mut open_namespaces: Vec<NsRollup> = Vec::new();
    for (ns, &workloads) in &ns_workloads {
        let pol = ns_policies.get(ns).copied().unwrap_or(0);
        if pol == 0 && workloads > 0 {
            open_namespaces.push(NsRollup {
                namespace: ns.clone(),
                policies: 0,
                workloads,
                walled: 0,
                wide_open: true,
            });
        }
    }
    open_namespaces.sort_by(|a, b| a.namespace.cmp(&b.namespace));

    NetpolReport {
        policies: policies.len(),
        workloads: rows.len(),
        walled_ingress,
        egress_isolated,
        rows,
        unwalled_exposed,
        unwalled_unexposed,
        open_namespaces,
    }
}

/// A policy's effective (ingress, egress) directions. `policyTypes` verbatim when
/// present; else defaults to `[Ingress]` plus `Egress` iff it has egress rules.
pub(crate) fn effective_policy_types(spec: &NetworkPolicySpec) -> (bool, bool) {
    match spec.policy_types.as_ref() {
        Some(types) => (
            types.iter().any(|t| t == "Ingress"),
            types.iter().any(|t| t == "Egress"),
        ),
        None => {
            let has_egress = spec.egress.as_ref().is_some_and(|e| !e.is_empty());
            (true, has_egress)
        }
    }
}

/// Whether `labels` satisfies a NetworkPolicy `podSelector`. An empty/None
/// selector selects ALL pods (a namespace-wide default). `match_labels` is exact
/// AND `match_expressions` (In/NotIn/Exists/DoesNotExist), all AND-combined. An
/// unrecognized operator **fails closed** (no match) — never a false "walled".
pub(crate) fn selector_matches(
    sel: Option<&LabelSelector>,
    labels: &BTreeMap<String, String>,
) -> bool {
    let Some(sel) = sel else {
        return true; // no selector ⇒ namespace-wide
    };
    // matchLabels: every (k, v) must be present + equal.
    if let Some(ml) = sel.match_labels.as_ref()
        && !ml.iter().all(|(k, v)| labels.get(k) == Some(v))
    {
        return false;
    }
    // matchExpressions: every requirement must hold.
    if let Some(exprs) = sel.match_expressions.as_ref() {
        for req in exprs {
            let present = labels.get(&req.key);
            let vals = req.values.as_deref().unwrap_or(&[]);
            let ok = match req.operator.as_str() {
                // In/NotIn require a non-empty values list (the apiserver enforces
                // this); an empty list is malformed → fail CLOSED (never a false
                // "walled"), like an unknown operator. NotIn-empty in particular
                // would otherwise match everything.
                "In" if !vals.is_empty() => present.is_some_and(|v| vals.iter().any(|x| x == v)),
                "NotIn" if !vals.is_empty() => present.is_none_or(|v| !vals.iter().any(|x| x == v)),
                "Exists" => present.is_some(),
                "DoesNotExist" => present.is_none(),
                _ => false, // unknown operator / malformed In|NotIn → fail closed
            };
            if !ok {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use crate::state::model::WorkloadKind;

    fn dep(world_seed: &mut fx::Seeds, ns: &str, name: &str, app: &str) {
        // A Deployment whose pod template carries `app=<app>` (the netpol target).
        let mut d = fx::deployment(ns, name, 1, 1);
        d.spec.as_mut().unwrap().template.metadata =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                labels: Some(BTreeMap::from([("app".to_string(), app.to_string())])),
                ..Default::default()
            });
        world_seed.deployment(d);
    }

    fn wr(ns: &str, name: &str) -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: ns.into(),
            name: name.into(),
        }
    }

    fn cov(report: &NetpolReport, ns: &str, name: &str) -> Coverage {
        report
            .rows
            .iter()
            .find(|r| r.r == wr(ns, name))
            .unwrap()
            .cov
    }

    #[test]
    fn empty_selector_walls_whole_namespace() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        dep(&mut s, "demo", "db", "db");
        dep(&mut s, "other", "api", "api");
        s.networkpolicy(fx::networkpolicy_empty(
            "demo",
            "default-deny",
            &["Ingress"],
        ));
        let r = coverage_report(&world);
        assert!(cov(&r, "demo", "web").ingress);
        assert!(cov(&r, "demo", "db").ingress);
        // ...but ONLY that namespace — a policy in demo never walls `other`.
        assert!(!cov(&r, "other", "api").ingress);
    }

    #[test]
    fn matchlabels_walls_only_selected() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        dep(&mut s, "demo", "crashy", "crashy");
        s.networkpolicy(fx::networkpolicy(
            "demo",
            "web-iso",
            &[("app", "web")],
            &["Ingress"],
        ));
        let r = coverage_report(&world);
        assert!(cov(&r, "demo", "web").ingress);
        assert!(
            !cov(&r, "demo", "crashy").ingress,
            "a non-matching pod is unwalled"
        );
    }

    #[test]
    fn policytypes_default_is_ingress_only() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        // policy_types omitted, NO egress rules ⇒ Ingress only.
        let mut np = fx::networkpolicy("demo", "p", &[("app", "web")], &[]);
        np.spec.as_mut().unwrap().policy_types = None;
        s.networkpolicy(np);
        let c = cov(&coverage_report(&world), "demo", "web");
        assert!(c.ingress && !c.egress);
    }

    #[test]
    fn none_policytypes_with_egress_rules_includes_egress() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        s.networkpolicy(fx::networkpolicy_egress_rules(
            "demo",
            "egr",
            &[("app", "web")],
        ));
        let c = cov(&coverage_report(&world), "demo", "web");
        assert!(c.ingress && c.egress, "default + egress rules ⇒ both");
    }

    #[test]
    fn egress_only_policy_does_not_wall_ingress() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        s.networkpolicy(fx::networkpolicy(
            "demo",
            "egr",
            &[("app", "web")],
            &["Egress"],
        ));
        let c = cov(&coverage_report(&world), "demo", "web");
        assert!(!c.ingress && c.egress, "egress-only never walls ingress");
    }

    #[test]
    fn matchexpressions_all_operators() {
        let labels = BTreeMap::from([("app".to_string(), "web".to_string())]);
        let sel = |op: &str, vals: &[&str]| {
            Some(LabelSelector {
                match_expressions: Some(vec![
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelectorRequirement {
                        key: "app".into(),
                        operator: op.into(),
                        values: if vals.is_empty() {
                            None
                        } else {
                            Some(vals.iter().map(|s| s.to_string()).collect())
                        },
                    },
                ]),
                ..Default::default()
            })
        };
        assert!(selector_matches(
            sel("In", &["web", "db"]).as_ref(),
            &labels
        ));
        assert!(!selector_matches(sel("In", &["db"]).as_ref(), &labels));
        assert!(!selector_matches(sel("NotIn", &["web"]).as_ref(), &labels));
        assert!(selector_matches(sel("NotIn", &["db"]).as_ref(), &labels));
        assert!(selector_matches(sel("Exists", &[]).as_ref(), &labels));
        assert!(!selector_matches(
            sel("DoesNotExist", &[]).as_ref(),
            &labels
        ));
        // NotIn on an ABSENT key matches (the value is trivially not in the set).
        assert!(selector_matches(
            sel("NotIn", &["x"]).as_ref(),
            &BTreeMap::new()
        ));
        // matchLabels AND matchExpressions must BOTH hold.
        let both = Some(LabelSelector {
            match_labels: Some(BTreeMap::from([("tier".to_string(), "fe".to_string())])),
            match_expressions: sel("Exists", &[]).unwrap().match_expressions,
        });
        assert!(
            !selector_matches(both.as_ref(), &labels),
            "tier=fe absent ⇒ no match"
        );
    }

    #[test]
    fn broken_selector_fails_closed() {
        let labels = BTreeMap::from([("app".to_string(), "web".to_string())]);
        let bad = Some(LabelSelector {
            match_expressions: Some(vec![
                k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelectorRequirement {
                    key: "app".into(),
                    operator: "Bogus".into(),
                    values: None,
                },
            ]),
            ..Default::default()
        });
        assert!(
            !selector_matches(bad.as_ref(), &labels),
            "unknown op ⇒ no match (unwalled)"
        );
        // A malformed NotIn with an EMPTY values list must also fail closed —
        // otherwise it would match every pod (a false "walled").
        let notin_empty = Some(LabelSelector {
            match_expressions: Some(vec![
                k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelectorRequirement {
                    key: "app".into(),
                    operator: "NotIn".into(),
                    values: None,
                },
            ]),
            ..Default::default()
        });
        assert!(
            !selector_matches(notin_empty.as_ref(), &labels),
            "NotIn [] ⇒ no match (never a false walled)"
        );
        // An empty selector still selects all.
        assert!(selector_matches(Some(&LabelSelector::default()), &labels));
        assert!(selector_matches(None, &labels));
    }

    #[test]
    fn unwalled_and_exposed_is_the_concern_source() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        dep(&mut s, "demo", "db", "db");
        // `web` is fronted by a Service ⇒ exposed; `db` is not.
        s.service(fx::service("demo", "web-svc", &[("app", "web")]));
        let r = coverage_report(&world);
        assert!(r.unwalled_exposed.iter().any(|x| x.r == wr("demo", "web")));
        assert!(!r.unwalled_exposed.iter().any(|x| x.r == wr("demo", "db")));
        // The concern fires for web, not db.
        let web = r.rows.iter().find(|x| x.r == wr("demo", "web")).unwrap();
        assert!(workload_concern(web).is_some());
        let db = r.rows.iter().find(|x| x.r == wr("demo", "db")).unwrap();
        assert!(workload_concern(db).is_none());
    }

    #[test]
    fn wide_open_namespace_rollup() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        // No policies anywhere ⇒ demo is wide-open.
        let r = coverage_report(&world);
        assert_eq!(r.open_namespaces.len(), 1);
        assert!(r.open_namespaces[0].wide_open && r.open_namespaces[0].namespace == "demo");
    }

    #[test]
    fn walled_namespace_no_findings() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        s.service(fx::service("demo", "web-svc", &[("app", "web")]));
        s.networkpolicy(fx::networkpolicy_empty(
            "demo",
            "default-deny",
            &["Ingress"],
        ));
        let r = coverage_report(&world);
        assert!(r.unwalled_exposed.is_empty(), "all walled ⇒ no K07 finding");
        assert!(
            r.open_namespaces.is_empty(),
            "the ns has a policy ⇒ not wide-open"
        );
        assert_eq!(r.walled_ingress, 1);
    }

    #[test]
    fn multiple_policies_or_combine() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", "web");
        s.networkpolicy(fx::networkpolicy(
            "demo",
            "ing",
            &[("app", "web")],
            &["Ingress"],
        ));
        s.networkpolicy(fx::networkpolicy(
            "demo",
            "egr",
            &[("app", "web")],
            &["Egress"],
        ));
        let c = cov(&coverage_report(&world), "demo", "web");
        assert!(
            c.ingress && c.egress,
            "two single-direction policies OR-combine"
        );
    }

    #[test]
    fn statefulset_and_daemonset_covered_like_deployment() {
        let (world, mut s) = fx::world();
        let mut sts = fx::statefulset("demo", "db", 1, 1);
        sts.spec.as_mut().unwrap().template.metadata =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                labels: Some(BTreeMap::from([("app".to_string(), "db".to_string())])),
                ..Default::default()
            });
        s.statefulset(sts);
        s.networkpolicy(fx::networkpolicy(
            "demo",
            "db-iso",
            &[("app", "db")],
            &["Ingress"],
        ));
        let r = coverage_report(&world);
        let row = r
            .rows
            .iter()
            .find(|x| x.r.kind == WorkloadKind::StatefulSet && x.r.name == "db")
            .unwrap();
        assert!(row.cov.ingress);
    }
}
