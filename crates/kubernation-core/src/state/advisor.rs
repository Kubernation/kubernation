//! Advisor reports — the classic-4X "advisors" (Civ's Berater / F1 screens)
//! reframed for Kubernetes. Each is a **pure function of `ObservedWorld`**
//! (no I/O, unit-testable without a cluster), summarizing one facet of the
//! realm: Health (state of the realm), Storage (granaries), Network (harbors &
//! gates). They are cluster-wide reports — deliberately *not* scoped by the
//! map's namespace filter, since an advisor reports on the whole realm.
//!
//! These complement the attention queue (which surfaces *what needs orders*);
//! the advisors give the at-a-glance rollups.

use crate::state::model::{
    NodeHealth, PodState, build_map, build_workloads, ingress_backends, pod_state,
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
            PodState::Pending | PodState::Terminating => r.pods_pending += 1,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

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

        let r = health_report(&world);
        assert_eq!(r.nodes_total, 2);
        assert_eq!(r.nodes_healthy, 2);
        assert_eq!(r.pods_total, 2);
        assert_eq!(r.pods_running, 2);
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
