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

use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use k8s_openapi::api::core::v1::{Node, Pod};
use k8s_openapi::api::networking::v1::{NetworkPolicy, NetworkPolicySpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::Client;
use kube::api::{Api, DeleteParams, Patch, PatchParams, PostParams};

use crate::state::chaos::NetpolSpec;
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
        Intervention::SetImage {
            workload,
            container,
            image,
        } => {
            // A *strategic* merge so the containers list is merged by `name`
            // (preserving the container's other fields and sibling containers).
            // A plain JSON merge patch would replace the whole containers array.
            let patch = Patch::Strategic(serde_json::json!({
                "spec": { "template": { "spec": { "containers": [
                    { "name": container, "image": image }
                ]}}}
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

/// The label every chaos-created NetworkPolicy carries, so a partition is
/// recognizable (and findable) as ours.
pub fn partition_label() -> (&'static str, &'static str) {
    ("app.kubernetes.io/managed-by", "kubernation-chaos")
}

/// Apply (create-or-update) a deny-all NetworkPolicy scoped to a workload's
/// pods — the partition experiment's one new write. A no-rule policy with
/// `policyTypes: [Ingress, Egress]` denies all traffic in both directions for
/// pods matching the selector. Uses server-side apply (idempotent), so a repeat
/// run is harmless. `dry_run` gates it (validation + RBAC) without persisting.
pub async fn apply_partition(
    client: Client,
    spec: &NetpolSpec,
    dry_run: bool,
) -> Result<(), String> {
    // Fail closed: an empty `podSelector` matches EVERY pod in the namespace, so
    // a deny-all with no selector would isolate the whole namespace. The pure
    // planner already refuses this, but the failsafe also lives here in the one
    // write file so the primitive is safe in isolation.
    if spec.pod_selector.is_empty() {
        return Err(
            "refusing a chaos partition with an empty podSelector (it would deny the whole namespace)"
                .into(),
        );
    }
    let (lk, lv) = partition_label();
    let np = NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(spec.name.clone()),
            namespace: Some(spec.namespace.clone()),
            labels: Some(BTreeMap::from([(lk.to_string(), lv.to_string())])),
            ..Default::default()
        },
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(LabelSelector {
                match_labels: Some(spec.pod_selector.clone()),
                ..Default::default()
            }),
            // No ingress/egress rules + the chosen policy types = deny that
            // direction (Both = full deny-all; Ingress = out of rotation;
            // Egress = cut off from dependencies).
            policy_types: Some(
                spec.direction
                    .policy_types()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ..Default::default()
        }),
    };
    let pp = PatchParams {
        dry_run,
        field_manager: Some("kubernation-chaos".into()),
        ..Default::default()
    };
    let api: Api<NetworkPolicy> = Api::namespaced(client, &spec.namespace);
    api.patch(&spec.name, &pp, &Patch::Apply(&np))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Delete a chaos NetworkPolicy by name. Idempotent: a `404` (already gone) is
/// success, so a restore is safe to retry.
pub async fn delete_partition(client: Client, namespace: &str, name: &str) -> Result<(), String> {
    let api: Api<NetworkPolicy> = Api::namespaced(client, namespace);
    match api.delete(name, &DeleteParams::default()).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 404 => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Execute a confirmed chaos drill: a sequence of existing primitives (pod
/// deletes and Scale/Cordon/SetImage patches) plus the one partition verb chaos
/// adds (create/delete a deny-all NetworkPolicy). **All-or-nothing at the gate:**
/// the dry-run-able steps (Apply patches + Partition creates) are server-side
/// dry-run-gated (which enforces RBAC), and pod-delete permission is pre-flighted
/// with a `SelfSubjectAccessReview` per evicting namespace (evicts can't be
/// dry-run). If any gate fails, **nothing is written** and the forbidden rows are
/// returned — so a cordon+drain whose drain is forbidden can't half-apply (cordon
/// then stop). Only if every gate passes does each step run for real. Reuses
/// `CommitRow`/`CommitOutcome` so the UI shows it like a commit.
pub async fn run_chaos(client: Client, steps: &[crate::state::chaos::ChaosStep]) -> CommitOutcome {
    use crate::state::chaos::ChaosStep;
    // Gate part 1: dry-run the patchable steps (netpol deletes aren't gated; a
    // forbidden one fails its own row, and a 404 is success anyway).
    let mut dry_fail = Vec::new();
    for step in steps {
        let dry = match step {
            ChaosStep::Apply(iv) => apply_intervention(client.clone(), iv, true).await,
            ChaosStep::Partition(spec) => apply_partition(client.clone(), spec, true).await,
            ChaosStep::Evict { .. } | ChaosStep::Unpartition { .. } => continue,
        };
        if let Err(detail) = dry {
            dry_fail.push(CommitRow {
                label: chaos_step_label(step),
                ok: false,
                detail,
            });
        }
    }
    // Gate part 2: pre-flight `delete pods` RBAC for every namespace we'd evict
    // in (evicts aren't dry-runnable). This is what makes a drain all-or-nothing.
    let mut evict_ns: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for step in steps {
        if let ChaosStep::Evict { namespace, .. } = step {
            evict_ns.insert(namespace.as_str());
        }
    }
    for ns in evict_ns {
        let allowed = can_evict_pod(client.clone(), ns).await;
        let row = |detail: String| CommitRow {
            label: format!("delete pods in {ns}"),
            ok: false,
            detail,
        };
        match allowed {
            Ok(true) => {}
            Ok(false) => dry_fail.push(row("forbidden — no `delete pods` permission".into())),
            Err(e) => dry_fail.push(row(e)),
        }
    }
    if !dry_fail.is_empty() {
        return CommitOutcome {
            applied: false,
            rows: dry_fail,
        };
    }
    // Run every step for real.
    let mut rows = Vec::new();
    for step in steps {
        let res = match step {
            ChaosStep::Evict { namespace, pod } => evict_pod(client.clone(), namespace, pod).await,
            ChaosStep::Apply(iv) => apply_intervention(client.clone(), iv, false).await,
            ChaosStep::Partition(spec) => apply_partition(client.clone(), spec, false).await,
            ChaosStep::Unpartition { namespace, name } => {
                delete_partition(client.clone(), namespace, name).await
            }
        };
        rows.push(CommitRow {
            label: chaos_step_label(step),
            ok: res.is_ok(),
            detail: res.err().unwrap_or_default(),
        });
    }
    CommitOutcome {
        applied: true,
        rows,
    }
}

/// Short human label for a chaos step (drill-result rows).
fn chaos_step_label(step: &crate::state::chaos::ChaosStep) -> String {
    use crate::state::chaos::ChaosStep;
    match step {
        ChaosStep::Evict { namespace, pod } => format!("kill pod {namespace}/{pod}"),
        ChaosStep::Apply(iv) => iv_label(iv),
        ChaosStep::Partition(spec) => {
            format!("partition {}/{} (deny-all)", spec.namespace, spec.name)
        }
        ChaosStep::Unpartition { namespace, name } => {
            format!("remove partition {namespace}/{name}")
        }
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
        Intervention::SetImage {
            workload,
            container,
            image,
        } => format!(
            "set image {} {}/{} [{container}] → {image}",
            workload.kind, workload.namespace, workload.name
        ),
    }
}
