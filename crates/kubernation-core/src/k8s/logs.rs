//! On-demand pod logs. Unlike the reflector-backed stores, logs are
//! fetched when a view asks for them: a one-shot tail of the last `TAIL`
//! lines. Frontends poll this every couple of seconds for a live tail
//! (the kube log *stream* is a fine future upgrade; polling the tail is
//! simpler and survives reconnects without stream lifecycle bookkeeping).

use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, LogParams};

/// How many trailing lines to pull per fetch.
pub const TAIL: i64 = 500;

/// Fetch the recent log tail for one pod. `container` is required only for
/// multi-container pods; `None` lets the server pick the sole container.
/// Errors are returned as display strings for the log view to show inline.
pub async fn tail(
    client: Client,
    namespace: &str,
    pod: &str,
    container: Option<String>,
) -> Result<String, String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let lp = LogParams {
        container,
        tail_lines: Some(TAIL),
        timestamps: false,
        ..Default::default()
    };
    api.logs(pod, &lp).await.map_err(|e| e.to_string())
}

/// First container name of a pod, so logs work on multi-container pods
/// without the caller guessing.
pub async fn first_container(client: Client, namespace: &str, pod: &str) -> Option<String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let p = api.get(pod).await.ok()?;
    p.spec?.containers.first().and_then(|c| {
        if c.name.is_empty() {
            None
        } else {
            Some(c.name.clone())
        }
    })
}
