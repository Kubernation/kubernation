use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::core::v1::{Event, Node, PersistentVolumeClaim, Pod, Service};
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
    /// Bounded ring of recent events (all types; Warning drives attention).
    pub events: Arc<Mutex<VecDeque<RecentEvent>>>,
}

impl ObservedWorld {
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
