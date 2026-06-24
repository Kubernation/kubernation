use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::api::core::v1::{Event, Node, PersistentVolumeClaim, Pod, Service};
use k8s_openapi::api::networking::v1::{Ingress, NetworkPolicy};
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
    /// Batch workloads — projected as expedition structures on islands.
    pub jobs: Store<Job>,
    pub cronjobs: Store<CronJob>,
    pub pvcs: Store<PersistentVolumeClaim>,
    pub services: Store<Service>,
    /// Ingresses — the cluster's external gates, projected beside the
    /// Service harbors they route to.
    pub ingresses: Store<Ingress>,
    /// NetworkPolicies — the segmentation "walls" (read-only coverage analysis;
    /// `state/netpol.rs`). Empty when none observed / RBAC-denied → "unwalled".
    pub networkpolicies: Store<NetworkPolicy>,
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
    /// Every namespace that holds an observed object the map can show —
    /// workloads, pods, batch, storage, connectivity, and projected customs.
    /// Sorted; feeds the namespace-filter picker.
    pub fn namespaces(&self) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        let mut add = |ns: Option<String>| {
            if let Some(ns) = ns {
                out.insert(ns);
            }
        };
        for d in self.deployments.state() {
            add(d.metadata.namespace.clone());
        }
        for s in self.statefulsets.state() {
            add(s.metadata.namespace.clone());
        }
        for ds in self.daemonsets.state() {
            add(ds.metadata.namespace.clone());
        }
        for j in self.jobs.state() {
            add(j.metadata.namespace.clone());
        }
        for cj in self.cronjobs.state() {
            add(cj.metadata.namespace.clone());
        }
        for p in self.pods.state() {
            add(p.metadata.namespace.clone());
        }
        for pvc in self.pvcs.state() {
            add(pvc.metadata.namespace.clone());
        }
        for svc in self.services.state() {
            add(svc.metadata.namespace.clone());
        }
        for ing in self.ingresses.state() {
            add(ing.metadata.namespace.clone());
        }
        for c in self.custom_entries() {
            add(c.namespace);
        }
        out
    }

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

    /// Live usage for a pod, if metrics-server is available and reporting it.
    /// `None` means no per-pod metrics this poll (show requests / nothing).
    pub fn pod_usage(&self, namespace: &str, name: &str) -> Option<crate::k8s::metrics::NodeUsage> {
        let g = self.metrics.lock().ok()?;
        if !g.available {
            return None;
        }
        g.pod_usage(namespace, name)
    }

    /// The names of a pod's regular containers, read from the watched store (no
    /// fetch). Powers the in-overlay log container picker for multi-container pods;
    /// empty if the pod isn't in the store yet (one-container pods just return one).
    pub fn pod_containers(&self, namespace: &str, name: &str) -> Vec<String> {
        self.pods
            .find(|p| {
                p.metadata.name.as_deref() == Some(name)
                    && p.metadata.namespace.as_deref() == Some(namespace)
            })
            .and_then(|p| {
                p.spec.as_ref().map(|s| {
                    s.containers
                        .iter()
                        .map(|c| c.name.clone())
                        .filter(|n| !n.is_empty())
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default()
    }

    /// Is a pod with this (namespace, name) currently in the store? Used to
    /// reap a port-forward whose backing pod has disappeared (a forward targets
    /// a specific pod, so once it's gone the tunnel is dead).
    pub fn pod_exists(&self, namespace: &str, name: &str) -> bool {
        self.pods
            .find(|p| {
                p.metadata.name.as_deref() == Some(name)
                    && p.metadata.namespace.as_deref() == Some(namespace)
            })
            .is_some()
    }

    /// Recent usage samples for one node (oldest→newest) — the trend behind
    /// the node window's sparklines. Empty when metrics-server isn't reporting
    /// (the ring is retained across a blip but hidden while `available` is
    /// false, so the sparkline disappears and resumes with continuity).
    pub fn node_usage_history(&self, name: &str) -> Vec<crate::k8s::metrics::NodeUsage> {
        let g = match self.metrics.lock() {
            Ok(g) if g.available => g,
            _ => return Vec::new(),
        };
        g.node_history(name)
    }

    /// Recent cluster-aggregate usage samples (oldest→newest) — the STATUS
    /// overview sparkline. Empty when metrics-server isn't reporting.
    pub fn cluster_usage_history(&self) -> Vec<crate::k8s::metrics::NodeUsage> {
        let g = match self.metrics.lock() {
            Ok(g) if g.available => g,
            _ => return Vec::new(),
        };
        g.cluster_history()
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

#[cfg(test)]
mod tests {
    use crate::state::fixtures as fx;

    #[test]
    fn pod_exists_tracks_the_store() {
        let (world, mut s) = fx::world();
        // Absent before seeding.
        assert!(!world.pod_exists("demo", "web-1"));
        s.pod(fx::pod("demo", "web-1", Some("n1")));
        // Present once seeded — this is what the port-forward reaper checks.
        assert!(world.pod_exists("demo", "web-1"));
        // Namespace + name must both match (no cross-namespace false positive).
        assert!(!world.pod_exists("other", "web-1"));
        assert!(!world.pod_exists("demo", "web-2"));
    }
}
