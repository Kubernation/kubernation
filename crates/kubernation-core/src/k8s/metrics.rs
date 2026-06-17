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

/// Live usage for one node or pod: CPU in cores, memory in bytes (both
/// canonical, matching `node_request_ratios`).
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
    /// Per-pod usage keyed by (namespace, name); summed across containers.
    /// Best-effort — empty if the PodMetrics list fails (nodes drive
    /// `available`).
    pub pods: HashMap<(String, String), NodeUsage>,
}

impl Metrics {
    /// Live usage for one pod, if metrics-server reported it this poll.
    pub fn pod_usage(&self, namespace: &str, name: &str) -> Option<NodeUsage> {
        self.pods
            .get(&(namespace.to_string(), name.to_string()))
            .copied()
    }
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

/// metrics.k8s.io/v1beta1 PodMetrics. Plural `pods`, built by hand for the
/// same reason as nodes.
fn pod_metrics_resource() -> ApiResource {
    ApiResource {
        group: "metrics.k8s.io".into(),
        version: "v1beta1".into(),
        api_version: "metrics.k8s.io/v1beta1".into(),
        kind: "PodMetrics".into(),
        plural: "pods".into(),
    }
}

/// Sum a PodMetrics item's per-container usage into one (cpu, mem) total.
fn pod_total(item: &DynamicObject) -> NodeUsage {
    let mut total = NodeUsage::default();
    if let Some(containers) = item.data.get("containers").and_then(|c| c.as_array()) {
        for c in containers {
            let usage = &c["usage"];
            total.cpu += usage
                .get("cpu")
                .and_then(|v| v.as_str())
                .and_then(quantity::parse)
                .unwrap_or(0.0);
            total.mem += usage
                .get("memory")
                .and_then(|v| v.as_str())
                .and_then(quantity::parse)
                .unwrap_or(0.0);
        }
    }
    total
}

/// Poll node usage every `POLL` seconds into `store`, nudging the frontend
/// with `WorldDelta::Metrics` whenever availability or values change.
pub fn spawn(
    client: Client,
    id: ClusterId,
    store: MetricsStore,
    sink: impl DeltaSink,
) -> JoinHandle<()> {
    let node_ar = node_metrics_resource();
    let pod_ar = pod_metrics_resource();
    tokio::spawn(async move {
        let nodes_api: Api<DynamicObject> = Api::all_with(client.clone(), &node_ar);
        let pods_api: Api<DynamicObject> = Api::all_with(client, &pod_ar);
        loop {
            match nodes_api.list(&ListParams::default()).await {
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
                    // Pod usage is best-effort: a failure here leaves pods
                    // empty but keeps `available` true (nodes are the signal).
                    let pods = match pods_api.list(&ListParams::default()).await {
                        Ok(plist) => {
                            let mut m = HashMap::with_capacity(plist.items.len());
                            for item in plist {
                                if let (Some(name), Some(ns)) =
                                    (item.metadata.name.clone(), item.metadata.namespace.clone())
                                {
                                    m.insert((ns, name), pod_total(&item));
                                }
                            }
                            m
                        }
                        Err(err) => {
                            tracing::debug!(%err, "pod metrics unavailable this poll");
                            HashMap::new()
                        }
                    };
                    if let Ok(mut g) = store.lock() {
                        g.available = true;
                        g.nodes = nodes;
                        g.pods = pods;
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
                            g.pods.clear();
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
