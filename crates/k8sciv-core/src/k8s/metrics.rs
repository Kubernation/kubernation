//! Live node usage from metrics.k8s.io (metrics-server). The metrics API
//! serves list/get but not watch, so this polls rather than reflects.
//! When metrics-server is absent the store stays `available = false` and
//! the gauges fall back to scheduling pressure (requests ÷ allocatable) —
//! exactly the behavior on a bare kind cluster.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kube::Client;
use kube::api::{Api, ListParams};
use kube::core::{ApiResource, DynamicObject};
use tokio::task::JoinHandle;

use crate::events::{ClusterId, WorldDelta};
use crate::k8s::quantity;
use crate::k8s::watch::DeltaSink;

/// Live usage for one node: CPU in cores, memory in bytes (both canonical,
/// matching `node_request_ratios`).
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeUsage {
    pub cpu: f64,
    pub mem: f64,
}

/// Shared, pollable metrics state. `available` flips false the moment a
/// poll fails, so a metrics-server that goes away degrades cleanly.
#[derive(Default)]
pub struct Metrics {
    pub available: bool,
    pub nodes: HashMap<String, NodeUsage>,
}

pub type MetricsStore = Arc<Mutex<Metrics>>;

pub fn store() -> MetricsStore {
    Arc::new(Mutex::new(Metrics::default()))
}

const POLL: Duration = Duration::from_secs(15);

/// metrics.k8s.io/v1beta1 NodeMetrics. The plural is `nodes` (not the
/// auto-derived `nodemetrics`), so the ApiResource is built by hand.
fn node_metrics_resource() -> ApiResource {
    ApiResource {
        group: "metrics.k8s.io".into(),
        version: "v1beta1".into(),
        api_version: "metrics.k8s.io/v1beta1".into(),
        kind: "NodeMetrics".into(),
        plural: "nodes".into(),
    }
}

/// Poll node usage every `POLL` seconds into `store`, nudging the frontend
/// with `WorldDelta::Metrics` whenever availability or values change.
pub fn spawn(
    client: Client,
    id: ClusterId,
    store: MetricsStore,
    sink: impl DeltaSink,
) -> JoinHandle<()> {
    let ar = node_metrics_resource();
    tokio::spawn(async move {
        let api: Api<DynamicObject> = Api::all_with(client, &ar);
        loop {
            match api.list(&ListParams::default()).await {
                Ok(list) => {
                    let mut nodes = HashMap::with_capacity(list.items.len());
                    for item in list {
                        let Some(name) = item.metadata.name.clone() else {
                            continue;
                        };
                        let usage = &item.data["usage"];
                        let cpu = usage
                            .get("cpu")
                            .and_then(|v| v.as_str())
                            .and_then(quantity::parse)
                            .unwrap_or(0.0);
                        let mem = usage
                            .get("memory")
                            .and_then(|v| v.as_str())
                            .and_then(quantity::parse)
                            .unwrap_or(0.0);
                        nodes.insert(name, NodeUsage { cpu, mem });
                    }
                    if let Ok(mut g) = store.lock() {
                        g.available = true;
                        g.nodes = nodes;
                    }
                    sink(id, WorldDelta::Metrics);
                }
                Err(err) => {
                    // First failure flips availability and notifies once; we
                    // keep polling so a later-installed metrics-server is
                    // picked up without a restart.
                    let was_available = store
                        .lock()
                        .map(|mut g| {
                            let was = g.available;
                            g.available = false;
                            g.nodes.clear();
                            was
                        })
                        .unwrap_or(false);
                    if was_available {
                        sink(id, WorldDelta::Metrics);
                    }
                    tracing::debug!(%err, "metrics-server unavailable; gauges show scheduling pressure");
                }
            }
            tokio::time::sleep(POLL).await;
        }
    })
}
