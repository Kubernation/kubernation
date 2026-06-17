//! Cluster **mutations** — the one place in the codebase that writes.
//!
//! Everything else is observe-only (reflectors, pure models, on-demand log
//! tails). This module is the deliberate, narrowly-scoped exception: pod
//! eviction (a delete) and applying a planning-turn intervention (scale a
//! workload, cordon a node). Each write is invoked only behind an explicit
//! confirm, and every staged intervention is validated with a **server-side
//! dry-run** (which also enforces RBAC) before any real apply. Kept apart so
//! the entire write surface is one small, auditable file.
//!
//! Committing a planning turn goes through [`commit_interventions`]: it
//! dry-runs every staged change first and only writes for real if *all* pass,
//! so a turn the cluster would reject never half-applies. Both frontends call
//! it, keeping the "decide to write for real" step inside this one file.

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::Client;
use kube::api::{Api, DeleteParams, Patch, PatchParams, PostParams};

use crate::state::model::WorkloadKind;
use crate::state::planned::Intervention;

/// Evict (delete) a single pod. A pod owned by a controller (Deployment,
/// StatefulSet, DaemonSet, …) is recreated by it; a bare pod is gone. Errors
/// are returned as display strings for the UI to surface.
pub async fn evict_pod(client: Client, namespace: &str, pod: &str) -> Result<(), String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    api.delete(pod, &DeleteParams::default())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Can the current user `delete pods` in `namespace`? A read-only RBAC probe
/// (a `SelfSubjectAccessReview`) the frontends use to disable the evict
/// control when permission is lacking. Errs to the UI as a display string.
pub async fn can_evict_pod(client: Client, namespace: &str) -> Result<bool, String> {
    let api: Api<SelfSubjectAccessReview> = Api::all(client);
    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(ResourceAttributes {
                verb: Some("delete".into()),
                resource: Some("pods".into()),
                namespace: Some(namespace.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let res = api
        .create(&PostParams::default(), &review)
        .await
        .map_err(|e| e.to_string())?;
    Ok(res.status.map(|s| s.allowed).unwrap_or(false))
}

/// Apply one staged planning-turn intervention with a strategic-merge patch.
/// `dry_run` runs it server-side without persisting — the validation +
/// authorization gate the End-of-Turn review uses before a real commit.
/// Errors (validation failures, RBAC `Forbidden`, …) come back as strings.
pub async fn apply_intervention(
    client: Client,
    iv: &Intervention,
    dry_run: bool,
) -> Result<(), String> {
    let pp = PatchParams {
        dry_run,
        ..Default::default()
    };
    match iv {
        Intervention::Scale { workload, replicas } => {
            let patch = Patch::Merge(serde_json::json!({ "spec": { "replicas": replicas } }));
            let ns = workload.namespace.as_str();
            let name = workload.name.as_str();
            match workload.kind {
                WorkloadKind::Deployment => Api::<Deployment>::namespaced(client, ns)
                    .patch(name, &pp, &patch)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                WorkloadKind::StatefulSet => Api::<StatefulSet>::namespaced(client, ns)
                    .patch(name, &pp, &patch)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                WorkloadKind::DaemonSet => {
                    Err("DaemonSets scale with node count, not a replica field".into())
                }
            }
        }
        Intervention::Cordon { node, on } => {
            let patch = Patch::Merge(serde_json::json!({ "spec": { "unschedulable": on } }));
            Api::<Node>::all(client)
                .patch(node, &pp, &patch)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        Intervention::Restart { workload } => {
            // The kubectl convention: stamp the pod template with a fresh
            // `restartedAt` annotation, which rolls the workload's pods.
            let ts = k8s_openapi::jiff::Timestamp::now().to_string();
            let patch = Patch::Merge(serde_json::json!({
                "spec": { "template": { "metadata": { "annotations": {
                    "kubectl.kubernetes.io/restartedAt": ts
                }}}}
            }));
            let ns = workload.namespace.as_str();
            let name = workload.name.as_str();
            let to_err = |e: kube::Error| e.to_string();
            match workload.kind {
                WorkloadKind::Deployment => Api::<Deployment>::namespaced(client, ns)
                    .patch(name, &pp, &patch)
                    .await
                    .map(|_| ())
                    .map_err(to_err),
                WorkloadKind::StatefulSet => Api::<StatefulSet>::namespaced(client, ns)
                    .patch(name, &pp, &patch)
                    .await
                    .map(|_| ())
                    .map_err(to_err),
                WorkloadKind::DaemonSet => Api::<DaemonSet>::namespaced(client, ns)
                    .patch(name, &pp, &patch)
                    .await
                    .map(|_| ())
                    .map_err(to_err),
            }
        }
    }
}

/// One change's commit result, for the End-of-Turn review to display.
#[derive(Debug, Clone)]
pub struct CommitRow {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

/// The result of committing a planning turn. `applied` is false when the
/// server-side dry-run gate blocked the turn — then `rows` carries only the
/// dry-run failures and *nothing was written*.
#[derive(Debug, Clone)]
pub struct CommitOutcome {
    pub applied: bool,
    pub rows: Vec<CommitRow>,
}

/// Commit a planning turn: dry-run every staged intervention first (which also
/// enforces RBAC), and only if *all* pass apply them for real. All-or-nothing
/// at the gate — a turn the cluster would reject never half-applies. Returns
/// per-row outcomes for the review to show.
pub async fn commit_interventions(client: Client, ivs: &[Intervention]) -> CommitOutcome {
    let mut dry_fail = Vec::new();
    for iv in ivs {
        if let Err(detail) = apply_intervention(client.clone(), iv, true).await {
            dry_fail.push(CommitRow {
                label: iv_label(iv),
                ok: false,
                detail,
            });
        }
    }
    if !dry_fail.is_empty() {
        return CommitOutcome {
            applied: false,
            rows: dry_fail,
        };
    }
    let mut rows = Vec::new();
    for iv in ivs {
        let r = apply_intervention(client.clone(), iv, false).await;
        rows.push(CommitRow {
            label: iv_label(iv),
            ok: r.is_ok(),
            detail: r.err().unwrap_or_default(),
        });
    }
    CommitOutcome {
        applied: true,
        rows,
    }
}

/// Short human label for a staged intervention (commit-result rows).
pub fn iv_label(iv: &Intervention) -> String {
    match iv {
        Intervention::Scale { workload, replicas } => format!(
            "scale {} {}/{} → {replicas}",
            workload.kind, workload.namespace, workload.name
        ),
        Intervention::Cordon { node, on } => {
            format!("{} node {node}", if *on { "cordon" } else { "uncordon" })
        }
        Intervention::Restart { workload } => format!(
            "restart {} {}/{}",
            workload.kind, workload.namespace, workload.name
        ),
    }
}
