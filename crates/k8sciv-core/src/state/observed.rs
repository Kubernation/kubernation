use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::core::v1::{Event, Node, PersistentVolumeClaim, Pod, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::runtime::reflector::Store;

use crate::k8s::client::ClusterMeta;

/// The read-only "observed world": reflector-backed caches the informers
/// keep current. This is the single rendering source of truth. One instance
/// per cluster context — the future hot/warm pair is two of these.
#[derive(Clone)]
pub struct ObservedWorld {
    pub meta: ClusterMeta,
    pub nodes: Store<Node>,
    pub pods: Store<Pod>,
    pub deployments: Store<Deployment>,
    pub replicasets: Store<ReplicaSet>,
    pub statefulsets: Store<StatefulSet>,
    pub daemonsets: Store<DaemonSet>,
    pub pvcs: Store<PersistentVolumeClaim>,
    pub services: Store<Service>,
    /// Ingresses — the cluster's external gates, projected beside the
    /// Service harbors they route to.
    pub ingresses: Store<Ingress>,
    /// Bounded ring of recent events (all types; Warning drives attention).
    pub events: Arc<Mutex<VecDeque<RecentEvent>>>,
    /// Dynamic custom-resource projections (configured via `projections` /
    /// `--project`): kind label + reflector store per projected CRD.
    pub customs: Arc<Vec<CustomWatch>>,
    /// Live node usage polled from metrics-server, when present. Gauges use
    /// it over request-based pressure whenever `available`.
    pub metrics: crate::k8s::metrics::MetricsStore,
}

/// One projected custom-resource type.
#[derive(Clone)]
pub struct CustomWatch {
    pub kind: String,
    pub store: kube::runtime::reflector::Store<kube::core::DynamicObject>,
}

impl ObservedWorld {
    /// Flatten projected custom-resource instances for the world model.
    pub fn custom_entries(&self) -> Vec<crate::state::world::CustomEntry> {
        let mut out = Vec::new();
        for cw in self.customs.iter() {
            for obj in cw.store.state() {
                out.push(crate::state::world::CustomEntry {
                    kind: cw.kind.clone(),
                    namespace: obj.metadata.namespace.clone(),
                    name: obj.metadata.name.clone().unwrap_or_default(),
                });
            }
        }
        out.sort_by(|a, b| (&a.kind, &a.namespace, &a.name).cmp(&(&b.kind, &b.namespace, &b.name)));
        out
    }

    /// Live usage for a node, if metrics-server is available and reporting
    /// it. `None` means fall back to scheduling pressure.
    pub fn node_usage(&self, name: &str) -> Option<crate::k8s::metrics::NodeUsage> {
        let g = self.metrics.lock().ok()?;
        if !g.available {
            return None;
        }
        g.nodes.get(name).copied()
    }

    /// Whether live metrics are currently driving the gauges.
    pub fn metrics_available(&self) -> bool {
        self.metrics.lock().map(|g| g.available).unwrap_or(false)
    }

    /// Snapshot of the recent-events ring, oldest first.
    pub fn recent_events(&self) -> Vec<RecentEvent> {
        self.events
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// A flattened, render-friendly Event. Kept tiny: the ring holds hundreds.
#[derive(Debug, Clone)]
pub struct RecentEvent {
    pub warning: bool,
    pub reason: String,
    pub message: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
    pub count: i32,
    pub when: Option<Time>,
}

impl RecentEvent {
    pub fn key(&self) -> (&str, &str, &str, &str) {
        (&self.kind, &self.namespace, &self.name, &self.reason)
    }

    pub fn from_event(ev: &Event) -> Self {
        let when = ev
            .last_timestamp
            .clone()
            .or_else(|| ev.event_time.clone().map(|mt| Time(mt.0)))
            .or_else(|| ev.metadata.creation_timestamp.clone());
        let mut message = ev.message.clone().unwrap_or_default();
        message = message.replace('\n', " ");
        if message.len() > 200 {
            message.truncate(200);
        }
        RecentEvent {
            warning: ev.type_.as_deref() == Some("Warning"),
            reason: ev.reason.clone().unwrap_or_default(),
            message,
            kind: ev.involved_object.kind.clone().unwrap_or_default(),
            namespace: ev
                .involved_object
                .namespace
                .clone()
                .or_else(|| ev.metadata.namespace.clone())
                .unwrap_or_default(),
            name: ev.involved_object.name.clone().unwrap_or_default(),
            count: ev.count.unwrap_or(1),
            when,
        }
    }
}
