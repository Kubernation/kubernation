//! Cluster **mutations** — the one place in the codebase that writes.
//!
//! Everything else is observe-only (reflectors, pure models, on-demand log
//! tails). This module is the deliberate, narrowly-scoped exception: a single
//! pod delete ("evict"), invoked only from the GUI behind an explicit confirm.
//! It is kept apart so the entire write surface is one small, auditable file.

use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, DeleteParams};

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
