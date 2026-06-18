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

/// High-confidence credential field names (lowercased, exact match). Exact —
/// not substring — so reference fields like `secretName` / `secretKeyRef` /
/// `tokenRequests` are left intact while `password` / `apiKey` / `clientSecret`
/// (any case) are masked. Covers operator CRs that embed credentials inline.
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "token",
    "apikey",
    "api_key",
    "privatekey",
    "private_key",
    "client_secret",
    "clientsecret",
    "secretkey",
    "secret_key",
    "accesskey",
    "access_key",
    "sessiontoken",
    "session_token",
    "connectionstring",
    "connection_string",
];

fn redacted(bytes: usize) -> serde_json::Value {
    serde_json::Value::String(format!("\u{2022}\u{2022}\u{2022}\u{2022} ({bytes} bytes)"))
}

/// Recursively mask string leaves whose key is a known credential name. Keeps
/// the key and byte size; descends into nested objects/arrays.
fn mask_sensitive(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if SENSITIVE_KEYS.contains(&k.to_ascii_lowercase().as_str())
                    && let serde_json::Value::String(s) = val
                {
                    *val = redacted(s.len());
                    continue;
                }
                mask_sensitive(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                mask_sensitive(val);
            }
        }
        _ => {}
    }
}

/// YAML for any browsed `DynamicObject` (the resource browser). It is the one
/// path that may render Secret-adjacent content, so it redacts defensively:
///
/// - A **Secret** of any group/version (and, fail-closed, any object whose
///   `kind` we couldn't determine) has every `data` / `stringData` value masked
///   (keys + byte sizes kept) and its `annotations` dropped — a Secret's
///   annotations are low value in a dossier and the likeliest carrier of a full
///   base64 copy (e.g. `last-applied`).
/// - **Every** browsed object additionally gets an inline-credential sweep
///   (`mask_sensitive`) so an operator CR with an embedded `password`/`token` is
///   masked even though it isn't a Secret.
/// - Everything else (ConfigMaps included) is shown in full.
///
/// Redaction no longer depends on a positive `kind == "Secret" && v1` match:
/// `browse::list_kind` stamps the picked kind onto every item, but if that were
/// ever missed the object arrives with `kind == None` and is treated as
/// Secret-like — so the privilege posture fails *closed*.
pub fn dynamic_yaml(obj: &kube::core::DynamicObject) -> String {
    let kind = obj.types.as_ref().map(|t| t.kind.as_str());
    // A kind literally named "Secret" (core v1 OR a `*.Secret` CRD/aggregated
    // API), or an object whose kind we can't determine (fail closed).
    let secret_like = matches!(kind, Some("Secret") | None);

    let mut o = obj.clone();
    if secret_like {
        for key in ["data", "stringData"] {
            if let Some(serde_json::Value::Object(m)) = o.data.get_mut(key) {
                for v in m.values_mut() {
                    let bytes = v.as_str().map(|s| s.len()).unwrap_or(0);
                    *v = redacted(bytes);
                }
            }
        }
        o.metadata.annotations = None;
    }
    // Inline-credential sweep over the body (spec/status/etc.) for every kind.
    mask_sensitive(&mut o.data);
    clean_yaml(&o)
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
    fn clean_yaml_strips_noise_and_renders() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry;
        use std::collections::BTreeMap;

        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));

        // A pod that actually carries the noise we claim to strip, plus a
        // benign annotation that must survive (so the `annotations:` block
        // stays).
        let mut p = fx::pod("demo", "web-1", Some("n1"));
        p.metadata.managed_fields = Some(vec![ManagedFieldsEntry::default()]);
        p.metadata.annotations = Some(BTreeMap::from([
            (
                "kubectl.kubernetes.io/last-applied-configuration".to_string(),
                "{\"big\":\"blob\"}".to_string(),
            ),
            ("keep.me/x".to_string(), "y".to_string()),
        ]));
        s.pod(p);

        let y = pod_yaml(&world, "demo", "web-1").expect("pod yaml");
        assert!(y.contains("kind: Pod") && y.contains("name: web-1"));
        assert!(y.contains("namespace: demo"));
        assert!(!y.contains("managedFields"), "managedFields stripped:\n{y}");
        assert!(
            !y.contains("last-applied-configuration"),
            "last-applied stripped:\n{y}"
        );
        assert!(y.contains("keep.me/x"), "benign annotation survives:\n{y}");
        assert!(y.contains("annotations:"), "block kept (has a survivor)");

        // A pod whose ONLY annotation is the stripped one → the whole
        // `annotations:` block is removed (the is_empty cleanup).
        let mut bare = fx::pod("demo", "bare", Some("n1"));
        bare.metadata.annotations = Some(BTreeMap::from([(
            "kubectl.kubernetes.io/last-applied-configuration".to_string(),
            "{}".to_string(),
        )]));
        s.pod(bare);
        let yb = pod_yaml(&world, "demo", "bare").expect("bare yaml");
        assert!(!yb.contains("annotations"), "empty block removed:\n{yb}");

        // Missing object → None (not a panic / empty string).
        assert!(pod_yaml(&world, "demo", "nope").is_none());
        assert!(node_yaml(&world, "n1").unwrap().contains("kind: Node"));
        assert!(node_yaml(&world, "ghost").is_none());
    }

    #[test]
    fn dynamic_yaml_redacts_only_secret_values() {
        // `types: Some(Secret)` is exactly what `browse::list_kind` stamps onto
        // a browsed Secret (the apiserver omits it on list items); the
        // None→stamp→redact pipeline is covered end-to-end in
        // `browse::tests::stamp_types_drives_secret_redaction`.
        use kube::core::{DynamicObject, ObjectMeta, TypeMeta};
        let secret = DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".into(),
                kind: "Secret".into(),
            }),
            metadata: ObjectMeta {
                name: Some("creds".into()),
                namespace: Some("demo".into()),
                ..Default::default()
            },
            data: serde_json::json!({ "type": "Opaque", "data": { "password": "c2VjcmV0" } }),
        };
        let y = dynamic_yaml(&secret);
        assert!(y.contains("kind: Secret"));
        assert!(y.contains("password:"), "key shown: {y}");
        assert!(!y.contains("c2VjcmV0"), "value redacted: {y}");
        assert!(y.contains("bytes)"), "placeholder shown: {y}");

        // A ConfigMap is NOT a secret — shown in full.
        let cm = DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".into(),
                kind: "ConfigMap".into(),
            }),
            metadata: ObjectMeta {
                name: Some("cfg".into()),
                ..Default::default()
            },
            data: serde_json::json!({ "data": { "key": "plainvalue" } }),
        };
        assert!(
            dynamic_yaml(&cm).contains("plainvalue"),
            "configmap in full"
        );
    }

    #[test]
    fn dynamic_yaml_redacts_non_core_and_failclosed_secrets() {
        use kube::core::{DynamicObject, ObjectMeta, TypeMeta};

        // A CRD literally named Secret in another group — still redacted.
        let crd_secret = DynamicObject {
            types: Some(TypeMeta {
                api_version: "vault.example.com/v1".into(),
                kind: "Secret".into(),
            }),
            metadata: ObjectMeta {
                name: Some("v".into()),
                ..Default::default()
            },
            data: serde_json::json!({ "data": { "k": "bm9wZQ==" } }),
        };
        assert!(
            !dynamic_yaml(&crd_secret).contains("bm9wZQ=="),
            "non-core Secret redacted"
        );

        // No TypeMeta (a mis-stamped list item) — fail CLOSED: data masked.
        let unstamped = DynamicObject {
            types: None,
            metadata: ObjectMeta {
                name: Some("x".into()),
                ..Default::default()
            },
            data: serde_json::json!({ "data": { "k": "bGVhaw==" } }),
        };
        assert!(
            !dynamic_yaml(&unstamped).contains("bGVhaw=="),
            "unknown-kind data masked (fail closed)"
        );
    }

    #[test]
    fn dynamic_yaml_masks_inline_credentials_but_not_references() {
        use kube::core::{DynamicObject, ObjectMeta, TypeMeta};
        // An operator CR with an inline password + a Secret *reference*.
        let cr = DynamicObject {
            types: Some(TypeMeta {
                api_version: "db.example.com/v1".into(),
                kind: "Database".into(),
            }),
            metadata: ObjectMeta {
                name: Some("pg".into()),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "password": "hunter2",
                    "secretName": "pg-tls",
                    "replicas": 3,
                }
            }),
        };
        let y = dynamic_yaml(&cr);
        assert!(!y.contains("hunter2"), "inline password masked: {y}");
        assert!(y.contains("pg-tls"), "secretName reference kept: {y}");
        assert!(y.contains("replicas"), "non-secret field kept: {y}");
    }

    #[test]
    fn dynamic_yaml_drops_secret_last_applied_annotation() {
        use kube::core::{DynamicObject, ObjectMeta, TypeMeta};
        use std::collections::BTreeMap;
        // kubectl apply leaves the full object (incl. base64 data) in the
        // last-applied annotation; for a Secret we drop annotations entirely.
        let secret = DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".into(),
                kind: "Secret".into(),
            }),
            metadata: ObjectMeta {
                name: Some("creds".into()),
                annotations: Some(BTreeMap::from([(
                    "kubectl.kubernetes.io/last-applied-configuration".to_string(),
                    "{\"data\":{\"password\":\"c2VjcmV0\"}}".to_string(),
                )])),
                ..Default::default()
            },
            data: serde_json::json!({ "data": { "password": "c2VjcmV0" } }),
        };
        let y = dynamic_yaml(&secret);
        assert!(!y.contains("c2VjcmV0"), "no base64 anywhere: {y}");
        assert!(!y.contains("last-applied"), "annotations dropped: {y}");
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
