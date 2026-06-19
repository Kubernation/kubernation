//! The Charter — your *writ* in this cluster: a curated, self-scoped RBAC grid.
//!
//! The pure half of the self-scoped RBAC feature (#6). The async SSAR probes
//! live in [`crate::k8s::rbac`]; this module owns the **curated probe set** (the
//! high-signal verbs × resources worth showing) and folds the probe verdicts
//! into a [`Charter`] grid + rollups. PURE + unit-tested — no cluster, no UI.
//!
//! The set covers the OWASP-K03 escalation primitives (exec, secrets-list,
//! rbac-write, node patch/proxy, SA-token) AND Kubernation's own write surface
//! (delete pods = evict, patch nodes = cordon, patch deployments = scale/
//! restart/image/rollback, create pods/portforward = the fwd button, create
//! networkpolicies = a chaos partition), so the Charter doubles as a "which
//! features will work for me here?" check.

use crate::k8s::rbac::{AccessProbe, Risk, Verdict};

/// Canonical verb order for rendering a resource's cells.
pub const VERBS: [&str; 6] = ["get", "list", "watch", "create", "update", "delete"];

const RBAC: &str = "rbac.authorization.k8s.io";

/// Convenience for the const tables below.
const fn p(
    verb: &'static str,
    group: &'static str,
    resource: &'static str,
    subresource: Option<&'static str>,
    namespaced: bool,
    risk: Risk,
) -> AccessProbe {
    AccessProbe {
        verb,
        group,
        resource,
        subresource,
        namespaced,
        risk,
    }
}

/// Namespaced probes — answered against the active namespace. Authored grouped
/// by resource, verbs in `VERBS` order (so the GUI groups consecutive cells).
static NS_PROBES: &[AccessProbe] = &[
    p("get", "", "pods", None, true, Risk::Normal),
    p("list", "", "pods", None, true, Risk::Normal),
    p("create", "", "pods", None, true, Risk::High),
    p("delete", "", "pods", None, true, Risk::High),
    p("create", "", "pods", Some("exec"), true, Risk::Critical),
    p("get", "", "pods", Some("log"), true, Risk::Normal),
    p("create", "", "pods", Some("portforward"), true, Risk::High),
    p("get", "", "secrets", None, true, Risk::High),
    p("list", "", "secrets", None, true, Risk::Critical),
    p("get", "", "configmaps", None, true, Risk::Normal),
    p("list", "", "configmaps", None, true, Risk::Normal),
    p("get", "apps", "deployments", None, true, Risk::Normal),
    p("create", "apps", "deployments", None, true, Risk::High),
    // PATCH, not UPDATE: every Kubernation deployment write (scale / restart /
    // image / rollback) is an HTTP PATCH, which RBAC authorizes under the `patch`
    // verb. Probing `update` would give a false ✓/✗ for the feature's own writes.
    p("patch", "apps", "deployments", None, true, Risk::High),
    p("delete", "apps", "deployments", None, true, Risk::High),
    p("create", "", "services", None, true, Risk::High),
    p("delete", "", "services", None, true, Risk::High),
    // Chaos Game Day creates a deny-all NetworkPolicy (the partition experiment).
    p(
        "create",
        "networking.k8s.io",
        "networkpolicies",
        None,
        true,
        Risk::High,
    ),
    p(
        "create",
        "",
        "persistentvolumeclaims",
        None,
        true,
        Risk::Normal,
    ),
    p(
        "delete",
        "",
        "persistentvolumeclaims",
        None,
        true,
        Risk::Normal,
    ),
    p("create", RBAC, "roles", None, true, Risk::Critical),
    p("update", RBAC, "roles", None, true, Risk::Critical),
    p("create", RBAC, "rolebindings", None, true, Risk::Critical),
    p(
        "create",
        "",
        "serviceaccounts",
        Some("token"),
        true,
        Risk::Critical,
    ),
    p("list", "", "events", None, true, Risk::Normal),
];

/// Cluster-scoped probes — answered with `namespace=None` (authoritative for
/// cluster-scoped resources; `secrets list` here means *across all namespaces*).
static CLUSTER_PROBES: &[AccessProbe] = &[
    p("get", "", "nodes", None, false, Risk::Normal),
    p("list", "", "nodes", None, false, Risk::Normal),
    p("patch", "", "nodes", None, false, Risk::High),
    p("get", "", "nodes", Some("proxy"), false, Risk::Critical),
    p("list", "", "secrets", None, false, Risk::Critical),
    p("create", RBAC, "clusterroles", None, false, Risk::Critical),
    p(
        "create",
        RBAC,
        "clusterrolebindings",
        None,
        false,
        Risk::Critical,
    ),
    p("create", "", "namespaces", None, false, Risk::High),
    p("delete", "", "namespaces", None, false, Risk::High),
    p("list", "", "persistentvolumes", None, false, Risk::Normal),
];

pub fn namespaced_probes() -> &'static [AccessProbe] {
    NS_PROBES
}
pub fn cluster_probes() -> &'static [AccessProbe] {
    CLUSTER_PROBES
}

/// One cell's resolved access: ✓ / ✗ / ?.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Denied,
    Unknown,
}

/// One resolved grid cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharterCell {
    pub probe: AccessProbe,
    pub access: Access,
    /// allowed AND risk != Normal — the audit finding (a granted dangerous verb).
    pub dangerous: bool,
}

/// Whether the apiserver answered access reviews at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    Full,
    /// Every probe came back Unknown (no authorization.k8s.io/v1, or all errored).
    Unavailable(String),
}

/// The resolved Charter grid + rollups for one (cluster, namespace) scope.
#[derive(Debug, Clone)]
pub struct Charter {
    pub namespace: String,
    pub ns_cells: Vec<CharterCell>,
    pub cluster_cells: Vec<CharterCell>,
    pub trust: Trust,
    pub allowed: usize,
    pub denied: usize,
    pub unknown: usize,
    /// Count of allowed dangerous (Critical/High) capabilities — the headline.
    pub dangerous_granted: usize,
}

/// Fold a probe slice + its positional verdicts into cells + counts. A verdict
/// vec shorter than the probe slice (a partial burst / timeout) degrades the
/// missing tail to `Unknown` — never a fabricated allow/deny, never a panic.
fn build_cells(
    probes: &[AccessProbe],
    verdicts: &[Verdict],
) -> (Vec<CharterCell>, usize, usize, usize, usize, Option<String>) {
    let mut cells = Vec::with_capacity(probes.len());
    let (mut allowed, mut denied, mut unknown, mut dangerous) = (0, 0, 0, 0);
    let mut first_err: Option<String> = None;
    for (i, probe) in probes.iter().enumerate() {
        let access = match verdicts.get(i) {
            Some(Verdict::Allowed) => Access::Allowed,
            Some(Verdict::Denied) => Access::Denied,
            Some(Verdict::Unknown(e)) => {
                if first_err.is_none() {
                    first_err = Some(e.clone());
                }
                Access::Unknown
            }
            None => Access::Unknown, // short vec → unknown, not a guess
        };
        let is_dangerous = access == Access::Allowed && probe.risk != Risk::Normal;
        match access {
            Access::Allowed => allowed += 1,
            Access::Denied => denied += 1,
            Access::Unknown => unknown += 1,
        }
        if is_dangerous {
            dangerous += 1;
        }
        cells.push(CharterCell {
            probe: *probe,
            access,
            dangerous: is_dangerous,
        });
    }
    (cells, allowed, denied, unknown, dangerous, first_err)
}

/// Build the Charter from the namespaced + cluster probe verdicts. PURE.
pub fn build_charter(
    namespace: &str,
    ns_verdicts: &[Verdict],
    cluster_verdicts: &[Verdict],
) -> Charter {
    let (ns_cells, a1, d1, u1, dg1, e1) = build_cells(namespaced_probes(), ns_verdicts);
    let (cluster_cells, a2, d2, u2, dg2, e2) = build_cells(cluster_probes(), cluster_verdicts);
    let total = ns_cells.len() + cluster_cells.len();
    let unknown = u1 + u2;
    // If the apiserver answered nothing (every cell Unknown), the grid can't be
    // trusted — say so honestly rather than rendering a wall of "?".
    let trust = if total > 0 && unknown == total {
        Trust::Unavailable(
            e1.or(e2)
                .unwrap_or_else(|| "no access-review answer".into()),
        )
    } else {
        Trust::Full
    };
    Charter {
        namespace: namespace.to_string(),
        ns_cells,
        cluster_cells,
        trust,
        allowed: a1 + a2,
        denied: d1 + d2,
        unknown,
        dangerous_granted: dg1 + dg2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn all_probes() -> Vec<AccessProbe> {
        namespaced_probes()
            .iter()
            .chain(cluster_probes())
            .copied()
            .collect()
    }

    #[test]
    fn probe_set_is_nonempty_and_has_no_duplicates() {
        let probes = all_probes();
        assert!(
            probes.len() >= 30,
            "curated set should be substantial: {}",
            probes.len()
        );
        // The scope is part of a probe's identity: namespaced `secrets list`
        // (this ns) and cluster `secrets list` (all namespaces) are the same
        // tuple but legitimately distinct questions.
        let mut seen = HashSet::new();
        for p in &probes {
            assert!(
                seen.insert((p.verb, p.group, p.resource, p.subresource, p.namespaced)),
                "duplicate probe: {} {}/{:?} {} (ns={})",
                p.verb,
                p.group,
                p.subresource,
                p.resource,
                p.namespaced
            );
        }
    }

    #[test]
    fn groups_are_correct_a_wrong_group_silently_denies() {
        for p in namespaced_probes().iter().chain(cluster_probes()) {
            let want = match p.resource {
                "deployments" => "apps",
                "roles" | "rolebindings" | "clusterroles" | "clusterrolebindings" => RBAC,
                "networkpolicies" => "networking.k8s.io",
                _ => "", // pods/secrets/configmaps/services/events/serviceaccounts/
                         // nodes/namespaces/PVCs/PVs are all core
            };
            assert_eq!(p.group, want, "{} has wrong group {}", p.resource, p.group);
        }
    }

    #[test]
    fn owasp_dangerous_tuples_are_present_with_the_right_risk() {
        let has = |verb, group, resource, sub, risk| {
            all_probes().iter().any(|p| {
                p.verb == verb
                    && p.group == group
                    && p.resource == resource
                    && p.subresource == sub
                    && p.risk == risk
            })
        };
        assert!(has("create", "", "pods", Some("exec"), Risk::Critical));
        assert!(has("list", "", "secrets", None, Risk::Critical));
        assert!(has("get", "", "secrets", None, Risk::High));
        assert!(has("create", RBAC, "rolebindings", None, Risk::Critical));
        assert!(has("create", RBAC, "roles", None, Risk::Critical));
        assert!(has(
            "create",
            "",
            "serviceaccounts",
            Some("token"),
            Risk::Critical
        ));
        assert!(has("patch", "", "nodes", None, Risk::High));
        assert!(has("get", "", "nodes", Some("proxy"), Risk::Critical));
        assert!(has(
            "create",
            RBAC,
            "clusterrolebindings",
            None,
            Risk::Critical
        ));
    }

    #[test]
    fn own_write_surface_uses_patch_not_update_for_deployments() {
        // Kubernation writes deployments via HTTP PATCH (scale/restart/image/
        // rollback), which RBAC authorizes under `patch`, never `update`. The grid
        // must probe the verb the dry-run commit actually issues, else it gives a
        // false ✓/✗ for the feature's own writes.
        let dep = |verb| {
            all_probes()
                .iter()
                .any(|p| p.verb == verb && p.group == "apps" && p.resource == "deployments")
        };
        assert!(dep("patch"), "deployments write surface must probe `patch`");
        assert!(
            !dep("update"),
            "must not probe `update` (no Kubernation write uses it)"
        );
    }

    #[test]
    fn build_charter_maps_verdicts_and_flags_dangerous() {
        let ns = namespaced_probes();
        // All allowed.
        let v: Vec<Verdict> = ns.iter().map(|_| Verdict::Allowed).collect();
        let c = build_charter("demo", &v, &vec![Verdict::Allowed; cluster_probes().len()]);
        assert_eq!(c.trust, Trust::Full);
        assert_eq!(c.allowed, ns.len() + cluster_probes().len());
        assert_eq!(c.denied, 0);
        assert_eq!(c.unknown, 0);
        // dangerous_granted = the non-Normal probes (all allowed here).
        let non_normal = ns
            .iter()
            .chain(cluster_probes())
            .filter(|p| p.risk != Risk::Normal)
            .count();
        assert_eq!(c.dangerous_granted, non_normal);
        // A denied dangerous verb is NOT counted as granted.
        let mut v2 = v.clone();
        v2[2] = Verdict::Denied; // pods create (High)
        let c2 = build_charter("demo", &v2, &vec![Verdict::Denied; cluster_probes().len()]);
        assert!(c2.dangerous_granted < c.dangerous_granted);
        assert_eq!(c2.ns_cells[2].access, Access::Denied);
        assert!(!c2.ns_cells[2].dangerous);
    }

    #[test]
    fn build_charter_never_fabricates_and_degrades_to_unavailable() {
        // A short verdict vec (partial burst) → the tail is Unknown, no panic.
        let c = build_charter("demo", &[Verdict::Allowed], &[]);
        assert_eq!(c.ns_cells[0].access, Access::Allowed);
        assert_eq!(c.ns_cells[1].access, Access::Unknown);
        assert!(c.unknown > 0);
        // All Unknown (e.g. no authz API) → Trust::Unavailable with the error.
        let nsv = vec![Verdict::Unknown("403".into()); namespaced_probes().len()];
        let clv = vec![Verdict::Unknown("403".into()); cluster_probes().len()];
        let c = build_charter("demo", &nsv, &clv);
        assert!(matches!(c.trust, Trust::Unavailable(_)));
        assert_eq!(c.allowed, 0);
        assert_eq!(c.dangerous_granted, 0);
    }
}
