use std::collections::VecDeque;
use std::pin::pin;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::core::v1::{Event, Node, PersistentVolumeClaim, Pod, Service};
use kube::api::Api;
use kube::runtime::reflector::store::Writer;
use kube::runtime::{WatchStreamExt, reflector, watcher};
use kube::{Resource, ResourceExt};
use tokio::task::JoinHandle;

use kube::core::{ApiResource, DynamicObject};

use super::client::Cluster;

/// How watchers tell a frontend "this slice changed". Frontends wrap their
/// own channel/flag; watchers call it from async tasks, so it must be
/// cheap and non-blocking.
pub trait DeltaSink: Fn(ClusterId, WorldDelta) + Send + Sync + Clone + 'static {}
impl<T: Fn(ClusterId, WorldDelta) + Send + Sync + Clone + 'static> DeltaSink for T {}
use crate::events::{ClusterId, WorldDelta};
use crate::state::observed::{CustomWatch, ObservedWorld, RecentEvent};

/// Owns the informer tasks for one cluster context. Dropping it (e.g. on
/// context switch) aborts every watcher.
pub struct WorldHandle {
    pub world: ObservedWorld,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for WorldHandle {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Spawn the full informer set for a cluster and return the observed world
/// backed by their stores. One call per context; multi-cluster later means
/// calling this once per member of the pair.
pub fn spawn(
    cluster: &Cluster,
    id: ClusterId,
    sink: impl DeltaSink,
    projections: &[(String, ApiResource)],
) -> WorldHandle {
    let c = &cluster.client;
    let mut tasks = Vec::new();

    // Dynamic custom-resource projections (resolved from CRDs at connect).
    let mut customs = Vec::new();
    for (kind, ar) in projections {
        let writer = Writer::<DynamicObject>::new(ar.clone());
        let store = writer.as_reader();
        let api = Api::<DynamicObject>::all_with(c.clone(), ar);
        tasks.push(spawn_dynamic(api, writer, id, sink.clone()));
        customs.push(CustomWatch {
            kind: kind.clone(),
            store,
        });
    }

    let (nodes, w) = reflector::store::<Node>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Nodes,
    ));
    let (pods, w) = reflector::store::<Pod>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Pods,
    ));
    let (deployments, w) = reflector::store::<Deployment>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Workloads,
    ));
    let (replicasets, w) = reflector::store::<ReplicaSet>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Workloads,
    ));
    let (statefulsets, w) = reflector::store::<StatefulSet>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Workloads,
    ));
    let (daemonsets, w) = reflector::store::<DaemonSet>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Workloads,
    ));
    let (pvcs, w) = reflector::store::<PersistentVolumeClaim>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Storage,
    ));
    let (services, w) = reflector::store::<Service>();
    tasks.push(spawn_reflector(
        Api::all(c.clone()),
        w,
        id,
        sink.clone(),
        WorldDelta::Services,
    ));

    let events = Arc::new(Mutex::new(VecDeque::new()));
    tasks.push(spawn_events(
        Api::all(c.clone()),
        id,
        events.clone(),
        sink.clone(),
    ));

    // Tell the frontend when the core stores have finished their initial
    // list.
    {
        let nodes = nodes.clone();
        let pods = pods.clone();
        let sink = sink.clone();
        tasks.push(tokio::spawn(async move {
            let _ = nodes.wait_until_ready().await;
            let _ = pods.wait_until_ready().await;
            sink(id, WorldDelta::Ready);
        }));
    }

    let world = ObservedWorld {
        meta: cluster.meta.clone(),
        customs: Arc::new(customs),
        nodes,
        pods,
        deployments,
        replicasets,
        statefulsets,
        daemonsets,
        pvcs,
        services,
        events,
    };
    WorldHandle { world, tasks }
}

fn spawn_reflector<K>(
    api: Api<K>,
    writer: Writer<K>,
    id: ClusterId,
    sink: impl DeltaSink,
    delta: WorldDelta,
) -> JoinHandle<()>
where
    K: Resource<DynamicType = ()>
        + Clone
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + Send
        + Sync
        + 'static,
{
    tokio::spawn(async move {
        let stream = watcher(api, watcher::Config::default())
            .default_backoff()
            .modify(|obj| {
                // Trim the heaviest metadata we never render.
                obj.managed_fields_mut().clear();
                obj.annotations_mut()
                    .remove("kubectl.kubernetes.io/last-applied-configuration");
            })
            .reflect(writer)
            .touched_objects(); // applied *and* deleted — deletions must repaint too
        let mut stream = pin!(stream);
        loop {
            match stream.next().await {
                // try_send: deltas are dirty-bits; dropping one under
                // backpressure is harmless because rebuilds are wholesale.
                Some(Ok(_)) => sink(id, delta),
                Some(Err(err)) => {
                    tracing::warn!(?delta, %err, "watcher error (backoff will retry)");
                }
                None => {
                    tracing::warn!(?delta, "watcher stream ended");
                    break;
                }
            }
        }
    })
}

/// Events are high-churn and mostly noise after the fact; rather than a full
/// reflector store we keep a bounded ring of the most recent ones, deduped
/// by (kind, ns, name, reason).
/// Reflector loop for a projected custom resource (DynamicObject needs an
/// explicit ApiResource dyntype, so it can't share `spawn_reflector`).
fn spawn_dynamic(
    api: Api<DynamicObject>,
    writer: Writer<DynamicObject>,
    id: ClusterId,
    sink: impl DeltaSink,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let stream = watcher(api, watcher::Config::default())
            .default_backoff()
            .modify(|obj| {
                obj.managed_fields_mut().clear();
            })
            .reflect(writer)
            .touched_objects();
        let mut stream = pin!(stream);
        loop {
            match stream.next().await {
                Some(Ok(_)) => sink(id, WorldDelta::Custom),
                Some(Err(err)) => {
                    tracing::warn!(%err, "custom watcher error (will retry)");
                }
                None => {
                    tracing::warn!("custom watcher stream ended");
                    break;
                }
            }
        }
    })
}

fn spawn_events(
    api: Api<Event>,
    id: ClusterId,
    ring: Arc<Mutex<VecDeque<RecentEvent>>>,
    sink: impl DeltaSink,
) -> JoinHandle<()> {
    const CAP: usize = 500;
    tokio::spawn(async move {
        let stream = watcher(api, watcher::Config::default())
            .default_backoff()
            .applied_objects();
        let mut stream = pin!(stream);
        loop {
            match stream.next().await {
                Some(Ok(ev)) => {
                    let rec = RecentEvent::from_event(&ev);
                    if let Ok(mut g) = ring.lock() {
                        g.retain(|e| e.key() != rec.key());
                        g.push_back(rec);
                        while g.len() > CAP {
                            g.pop_front();
                        }
                    }
                    sink(id, WorldDelta::Events);
                }
                Some(Err(err)) => {
                    tracing::warn!(%err, "event watcher error (backoff will retry)");
                }
                None => break,
            }
        }
    })
}
