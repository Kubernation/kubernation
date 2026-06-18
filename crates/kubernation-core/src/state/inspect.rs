//! Read-only object inspection — the "dossier": a cleaned YAML rendering of a
//! resource we already hold in the reflector stores. No fetch, no client, no
//! writes; and only the **watched** kinds (workloads, nodes, pods, …) are
//! inspectable, so this preserves least privilege (Secrets/ConfigMaps are never
//! read). Pure functions of `ObservedWorld`, unit-testable without a cluster.

use serde::Serialize;

use crate::state::model::{WorkloadKind, WorkloadRef};
use crate::state::observed::ObservedWorld;

/// Serialize a resource to kubectl-style YAML, dropping the two big noise
/// sources — `metadata.managedFields` and the `last-applied-configuration`
/// annotation — so the dossier reads like `kubectl get -o yaml` after a tidy.
pub fn clean_yaml<T: Serialize>(obj: &T) -> String {
    let mut v = serde_json::to_value(obj).unwrap_or(serde_json::Value::Null);
    if let Some(meta) = v.get_mut("metadata").and_then(|m| m.as_object_mut()) {
        meta.remove("managedFields");
        if let Some(ann) = meta.get_mut("annotations").and_then(|a| a.as_object_mut()) {
            ann.remove("kubectl.kubernetes.io/last-applied-configuration");
            if ann.is_empty() {
                meta.remove("annotations");
            }
        }
    }
    serde_yaml::to_string(&v).unwrap_or_default()
}

fn is(
    meta: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta,
    ns: &str,
    name: &str,
) -> bool {
    meta.namespace.as_deref() == Some(ns) && meta.name.as_deref() == Some(name)
}

/// YAML for a workload object (Deployment / StatefulSet / DaemonSet).
pub fn workload_yaml(world: &ObservedWorld, r: &WorkloadRef) -> Option<String> {
    match r.kind {
        WorkloadKind::Deployment => world
            .deployments
            .state()
            .into_iter()
            .find(|o| is(&o.metadata, &r.namespace, &r.name))
            .map(|o| clean_yaml(&*o)),
        WorkloadKind::StatefulSet => world
            .statefulsets
            .state()
            .into_iter()
            .find(|o| is(&o.metadata, &r.namespace, &r.name))
            .map(|o| clean_yaml(&*o)),
        WorkloadKind::DaemonSet => world
            .daemonsets
            .state()
            .into_iter()
            .find(|o| is(&o.metadata, &r.namespace, &r.name))
            .map(|o| clean_yaml(&*o)),
    }
}

/// YAML for a node object (cluster-scoped, so no namespace).
pub fn node_yaml(world: &ObservedWorld, name: &str) -> Option<String> {
    world
        .nodes
        .state()
        .into_iter()
        .find(|o| o.metadata.name.as_deref() == Some(name))
        .map(|o| clean_yaml(&*o))
}

/// YAML for a pod object.
pub fn pod_yaml(world: &ObservedWorld, namespace: &str, name: &str) -> Option<String> {
    world
        .pods
        .state()
        .into_iter()
        .find(|o| is(&o.metadata, namespace, name))
        .map(|o| clean_yaml(&*o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    #[test]
    fn clean_yaml_strips_managed_fields_and_renders() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod("demo", "web-1", Some("n1")));

        let y = pod_yaml(&world, "demo", "web-1").expect("pod yaml");
        assert!(y.contains("kind: Pod"), "has kind: {y}");
        assert!(y.contains("name: web-1"), "has name");
        assert!(y.contains("namespace: demo"), "has namespace");
        assert!(!y.contains("managedFields"), "managedFields stripped");

        // Missing object → None (not a panic / empty string).
        assert!(pod_yaml(&world, "demo", "nope").is_none());
        assert!(node_yaml(&world, "n1").unwrap().contains("kind: Node"));
        assert!(node_yaml(&world, "ghost").is_none());
    }

    #[test]
    fn workload_yaml_resolves_by_kind() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 3, 3));
        s.statefulset(fx::statefulset("demo", "db", 1, 1));

        let dep = WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        };
        let y = workload_yaml(&world, &dep).expect("deploy yaml");
        assert!(y.contains("kind: Deployment") && y.contains("name: web"));

        // A StatefulSet ref must not match the Deployment store.
        let wrong = WorkloadRef {
            kind: WorkloadKind::DaemonSet,
            namespace: "demo".into(),
            name: "web".into(),
        };
        assert!(workload_yaml(&world, &wrong).is_none());

        let sts = WorkloadRef {
            kind: WorkloadKind::StatefulSet,
            namespace: "demo".into(),
            name: "db".into(),
        };
        assert!(
            workload_yaml(&world, &sts)
                .unwrap()
                .contains("kind: StatefulSet")
        );
    }
}
