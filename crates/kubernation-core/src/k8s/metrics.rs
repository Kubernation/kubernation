//! Live node usage from metrics.k8s.io (metrics-server). The metrics API
//! serves list/get but not watch, so this polls rather than reflects.
//! When metrics-server is absent the store stays `available = false` and
//! the gauges fall back to scheduling pressure (requests ÷ allocatable) —
//! exactly the behavior on a bare kind cluster.

use std::collections::{HashMap, VecDeque};
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

/// How many recent samples each history ring keeps. At `POLL` (15s) cadence,
/// 60 samples ≈ 15 minutes — enough trend for a small sparkline.
pub const HISTORY_CAP: usize = 60;

/// Consecutive polls a node may be absent from the metrics list before its
/// history ring is dropped. metrics-server occasionally omits a single node for
/// one poll (a kubelet scrape hiccup) while still reporting the rest — a grace
/// window keeps that node's trend instead of wiping it on a one-poll blip.
const RING_GRACE: u32 = 4;

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
    /// Recent per-node usage samples (oldest→newest) for trend sparklines.
    /// Kept across polls (only `nodes`/`pods` are replaced wholesale); a poll
    /// failure leaves these intact but `available` false, so the accessors
    /// hide them while metrics is down and the trend resumes on recovery.
    node_rings: HashMap<String, VecDeque<NodeUsage>>,
    /// Consecutive polls each ring's node has been absent (for `RING_GRACE`).
    ring_absences: HashMap<String, u32>,
    /// Recent per-POD usage samples, keyed like [`Self::pods`]. Same shape and
    /// same grace as the node rings, and one difference that decides the memory
    /// bound: **pod names churn**. A node name is stable for the node's life, so
    /// its ring is bounded by the fleet; a CronJob mints a new pod name every
    /// run, so a long grace would accumulate rings for pods that no longer
    /// exist. `RING_GRACE` (4 polls ≈ 1 min) drops a departed pod's ring
    /// promptly while still surviving the one-poll scrape hiccup the grace is
    /// there for, which bounds this at roughly the live pod count.
    ///
    /// Cost at the documented ceiling (5000 pods): 5000 × `HISTORY_CAP` × 16 B
    /// of samples ≈ 4.8 MB, plus keys and deque headers ≈ 5.5 MB total. Affordable
    /// for an operator-laptop tool, and the reason this is kept per POD rather
    /// than pre-aggregated per workload — a per-workload ring would be cheaper
    /// and could not answer "which replica is hot *over time*".
    pod_rings: HashMap<(String, String), VecDeque<NodeUsage>>,
    pod_ring_absences: HashMap<(String, String), u32>,
    /// Cluster-aggregate usage per sample (sum across nodes).
    cluster_ring: VecDeque<NodeUsage>,
}

impl Metrics {
    /// Live usage for one pod, if metrics-server reported it this poll.
    pub fn pod_usage(&self, namespace: &str, name: &str) -> Option<NodeUsage> {
        self.pods
            .get(&(namespace.to_string(), name.to_string()))
            .copied()
    }

    /// Append this poll's per-node usage to the history rings (capped), age out
    /// rings for nodes absent `RING_GRACE` consecutive polls, and record the
    /// cluster aggregate.
    pub fn record_sample(
        &mut self,
        nodes: &HashMap<String, NodeUsage>,
        pods: &HashMap<(String, String), NodeUsage>,
    ) {
        // Both ring sets, one retention rule (see `roll`): a key missing this
        // poll might be a one-poll scrape hiccup, so it is only dropped after
        // `RING_GRACE` consecutive absences.
        Self::roll(&mut self.node_rings, &mut self.ring_absences, nodes);
        Self::roll(&mut self.pod_rings, &mut self.pod_ring_absences, pods);
        let total = nodes.values().fold(NodeUsage::default(), |a, u| NodeUsage {
            cpu: a.cpu + u.cpu,
            mem: a.mem + u.mem,
        });
        self.cluster_ring.push_back(total);
        while self.cluster_ring.len() > HISTORY_CAP {
            self.cluster_ring.pop_front();
        }
    }

    /// Append one poll's samples to `rings`, cap each, and age out any key
    /// absent `RING_GRACE` consecutive polls. Shared by the node and pod rings
    /// so their retention cannot drift apart — the pod rings differ only in how
    /// fast their keys churn, not in how they are kept.
    fn roll<K: std::hash::Hash + Eq + Clone>(
        rings: &mut HashMap<K, VecDeque<NodeUsage>>,
        absences: &mut HashMap<K, u32>,
        sample: &HashMap<K, NodeUsage>,
    ) {
        for (k, &u) in sample {
            let ring = rings.entry(k.clone()).or_default();
            ring.push_back(u);
            while ring.len() > HISTORY_CAP {
                ring.pop_front();
            }
            absences.remove(k);
        }
        let absent: Vec<K> = rings
            .keys()
            .filter(|k| !sample.contains_key(*k))
            .cloned()
            .collect();
        for k in absent {
            let c = absences.entry(k.clone()).or_insert(0);
            *c += 1;
            if *c >= RING_GRACE {
                rings.remove(&k);
                absences.remove(&k);
            }
        }
    }

    /// Recent usage samples for one pod (oldest→newest) — the input to the
    /// right-sizing advisor's P90.
    pub fn pod_history(&self, namespace: &str, name: &str) -> Vec<NodeUsage> {
        self.pod_rings
            .get(&(namespace.to_string(), name.to_string()))
            .map(|r| r.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Recent usage samples for one node (oldest→newest).
    pub fn node_history(&self, name: &str) -> Vec<NodeUsage> {
        self.node_rings
            .get(name)
            .map(|r| r.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Recent cluster-aggregate usage samples (oldest→newest).
    pub fn cluster_history(&self) -> Vec<NodeUsage> {
        self.cluster_ring.iter().copied().collect()
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
                        g.record_sample(&nodes, &pods);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(cpu: f64, mem: f64) -> NodeUsage {
        NodeUsage { cpu, mem }
    }

    #[test]
    fn record_sample_rings_cap_prune_and_aggregate() {
        let mut m = Metrics::default();
        // Two nodes over two samples.
        let mut s1 = HashMap::new();
        s1.insert("a".to_string(), sample(1.0, 100.0));
        s1.insert("b".to_string(), sample(2.0, 200.0));
        m.record_sample(&s1, &HashMap::new());
        let mut s2 = HashMap::new();
        s2.insert("a".to_string(), sample(1.5, 150.0));
        s2.insert("b".to_string(), sample(2.5, 250.0));
        m.record_sample(&s2, &HashMap::new());

        assert_eq!(m.node_history("a").len(), 2);
        assert_eq!(m.node_history("a")[0].cpu, 1.0);
        assert_eq!(m.node_history("a")[1].cpu, 1.5);
        // Cluster aggregate is the per-sample sum across nodes.
        let cluster = m.cluster_history();
        assert_eq!(cluster.len(), 2);
        assert_eq!(cluster[0].cpu, 3.0); // 1.0 + 2.0
        assert_eq!(cluster[1].mem, 400.0); // 150 + 250

        // A node missing for a single poll keeps its ring (grace window) — a
        // one-poll scrape hiccup mustn't wipe the trend; survivors keep history.
        let mut s3 = HashMap::new();
        s3.insert("a".to_string(), sample(2.0, 200.0));
        m.record_sample(&s3, &HashMap::new());
        assert_eq!(m.node_history("a").len(), 3);
        assert_eq!(m.node_history("b").len(), 2, "one absence is within grace");
        // Absent for RING_GRACE consecutive polls → dropped.
        for _ in 1..RING_GRACE {
            m.record_sample(&s3, &HashMap::new());
        }
        assert!(
            m.node_history("b").is_empty(),
            "dropped after RING_GRACE absences"
        );
        // A node that returns within grace resets and keeps its ring.
        let mut s_both = HashMap::new();
        s_both.insert("a".to_string(), sample(3.0, 300.0));
        s_both.insert("c".to_string(), sample(1.0, 100.0));
        m.record_sample(&s_both, &HashMap::new()); // c appears
        let mut s_a = HashMap::new();
        s_a.insert("a".to_string(), sample(3.0, 300.0));
        m.record_sample(&s_a, &HashMap::new()); // c absent once
        m.record_sample(&s_both, &HashMap::new()); // c back within grace
        assert_eq!(m.node_history("c").len(), 2, "c's ring survived a blip");
    }

    #[test]
    fn record_sample_caps_at_history_cap() {
        let mut m = Metrics::default();
        for i in 0..(HISTORY_CAP + 25) {
            let mut s = HashMap::new();
            s.insert("a".to_string(), sample(i as f64, 0.0));
            m.record_sample(&s, &HashMap::new());
        }
        let h = m.node_history("a");
        assert_eq!(h.len(), HISTORY_CAP);
        // Oldest dropped: the window ends at the most recent sample.
        assert_eq!(h.last().unwrap().cpu, (HISTORY_CAP + 24) as f64);
        assert_eq!(m.cluster_history().len(), HISTORY_CAP);
    }

    /// A departed pod's ring is dropped, and a one-poll gap is not a departure.
    ///
    /// Pod names churn where node names do not — a CronJob mints a new one every
    /// run — so without this the ring map grows for the life of the process.
    /// `RING_GRACE` is what keeps it bounded at roughly the live pod count while
    /// still surviving the scrape hiccup the grace exists for.
    #[test]
    fn a_departed_pods_ring_ages_out_but_a_one_poll_gap_does_not() {
        let mut m = Metrics::default();
        let key = ("demo".to_string(), "job-abc".to_string());
        let with = HashMap::from([(key.clone(), NodeUsage { cpu: 1.0, mem: 1.0 })]);
        let without: HashMap<(String, String), NodeUsage> = HashMap::new();
        let nodes: HashMap<String, NodeUsage> = HashMap::new();

        m.record_sample(&nodes, &with);
        m.record_sample(&nodes, &without); // one-poll hiccup
        m.record_sample(&nodes, &with);
        assert_eq!(
            m.pod_history("demo", "job-abc").len(),
            2,
            "a single missed poll must not wipe the ring"
        );

        // Gone for good: dropped once the grace is spent.
        for _ in 0..RING_GRACE {
            m.record_sample(&nodes, &without);
        }
        assert!(
            m.pod_history("demo", "job-abc").is_empty(),
            "a departed pod's ring leaked — pod names churn, so this grows unbounded"
        );
    }
}
