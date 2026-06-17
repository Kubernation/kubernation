//! Hot/warm pair comparison: the sync-state of every workload across the
//! two continents. Presence, desired replicas, and pod-template images are
//! compared. DaemonSet replica counts are exempt — their "desired" tracks
//! node count, which legitimately differs between clusters.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use k8s_openapi::api::core::v1::PodTemplateSpec;

use super::attention::{Concern, Severity, Target};
use super::filter::NamespaceFilter;
use super::model::{WorkloadKind, WorkloadRef};
use super::observed::ObservedWorld;
use crate::events::ClusterId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    InSync,
    Drift {
        /// (hot desired, warm desired) when they differ.
        replicas: Option<(i32, i32)>,
        images: bool,
    },
    /// Present on hot, missing on warm — the dangerous direction.
    OnlyHot,
    /// Present only on warm.
    OnlyWarm,
}

impl SyncState {
    /// Short badge for table columns: `=`, `≠r`, `≠i`, `≠ri`, `−w`, `+w`.
    pub fn badge(&self) -> String {
        match self {
            SyncState::InSync => "=".into(),
            SyncState::Drift { replicas, images } => {
                let mut s = String::from("≠");
                if replicas.is_some() {
                    s.push('r');
                }
                if *images {
                    s.push('i');
                }
                s
            }
            SyncState::OnlyHot => "−w".into(),
            SyncState::OnlyWarm => "+w".into(),
        }
    }

    pub fn is_in_sync(&self) -> bool {
        *self == SyncState::InSync
    }

    /// One-line description for the city screen, oriented to the cluster
    /// the operator is currently viewing.
    pub fn describe(&self, viewer: ClusterId) -> String {
        let other = match viewer {
            ClusterId::Hot => "warm",
            ClusterId::Warm => "hot",
        };
        match self {
            SyncState::InSync => format!("{other}: in sync"),
            SyncState::Drift {
                replicas: Some((h, w)),
                images,
            } => {
                if *images {
                    format!("replicas hot {h} ≠ warm {w} · image drift")
                } else {
                    format!("replicas hot {h} ≠ warm {w}")
                }
            }
            SyncState::Drift { images: true, .. } => format!("{other}: image drift"),
            SyncState::Drift { .. } => format!("{other}: drift"),
            SyncState::OnlyHot => match viewer {
                ClusterId::Hot => "warm: MISSING".into(),
                ClusterId::Warm => "exists only on hot".into(),
            },
            SyncState::OnlyWarm => match viewer {
                ClusterId::Warm => "hot: MISSING".into(),
                ClusterId::Hot => "exists only on warm".into(),
            },
        }
    }
}

struct Snapshot {
    desired: i32,
    images: BTreeSet<String>,
}

fn images_of(template: Option<&PodTemplateSpec>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(spec) = template.and_then(|t| t.spec.as_ref()) {
        for c in &spec.containers {
            if let Some(img) = c.image.clone() {
                out.insert(img);
            }
        }
    }
    out
}

fn snapshot(world: &ObservedWorld) -> BTreeMap<WorkloadRef, Snapshot> {
    let mut out = BTreeMap::new();
    for d in world.deployments.state() {
        out.insert(
            WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: d.metadata.namespace.clone().unwrap_or_default(),
                name: d.metadata.name.clone().unwrap_or_default(),
            },
            Snapshot {
                desired: d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1),
                images: images_of(d.spec.as_ref().map(|s| &s.template)),
            },
        );
    }
    for s in world.statefulsets.state() {
        out.insert(
            WorkloadRef {
                kind: WorkloadKind::StatefulSet,
                namespace: s.metadata.namespace.clone().unwrap_or_default(),
                name: s.metadata.name.clone().unwrap_or_default(),
            },
            Snapshot {
                desired: s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1),
                images: images_of(s.spec.as_ref().map(|sp| &sp.template)),
            },
        );
    }
    for d in world.daemonsets.state() {
        out.insert(
            WorkloadRef {
                kind: WorkloadKind::DaemonSet,
                namespace: d.metadata.namespace.clone().unwrap_or_default(),
                name: d.metadata.name.clone().unwrap_or_default(),
            },
            Snapshot {
                desired: 0, // never compared for DaemonSets
                images: images_of(d.spec.as_ref().map(|sp| &sp.template)),
            },
        );
    }
    out
}

/// The cross-cluster comparison, rebuilt alongside the per-world models.
#[derive(Default)]
pub struct PairSync {
    pub by_workload: HashMap<WorkloadRef, SyncState>,
    pub drifted: usize,
    pub missing: usize,
}

impl PairSync {
    /// Compare the two worlds, scoped to `filter` so the pair-drift count
    /// matches the namespace-filtered attention queue (a `NamespaceFilter::All`
    /// compares everything).
    pub fn build(hot: &ObservedWorld, warm: &ObservedWorld, filter: &NamespaceFilter) -> Self {
        let keep = |s: BTreeMap<WorkloadRef, Snapshot>| -> BTreeMap<WorkloadRef, Snapshot> {
            s.into_iter()
                .filter(|(r, _)| filter.matches(&r.namespace))
                .collect()
        };
        let hot_snap = keep(snapshot(hot));
        let mut warm_snap = keep(snapshot(warm));
        let mut by_workload = HashMap::new();
        let (mut drifted, mut missing) = (0, 0);

        for (r, h) in hot_snap {
            let state = match warm_snap.remove(&r) {
                None => {
                    missing += 1;
                    SyncState::OnlyHot
                }
                Some(w) => {
                    let replicas = (r.kind != WorkloadKind::DaemonSet && h.desired != w.desired)
                        .then_some((h.desired, w.desired));
                    let images = h.images != w.images;
                    if replicas.is_none() && !images {
                        SyncState::InSync
                    } else {
                        drifted += 1;
                        SyncState::Drift { replicas, images }
                    }
                }
            };
            by_workload.insert(r, state);
        }
        for (r, _) in warm_snap {
            missing += 1;
            by_workload.insert(r, SyncState::OnlyWarm);
        }

        PairSync {
            by_workload,
            drifted,
            missing,
        }
    }

    pub fn state(&self, r: &WorkloadRef) -> Option<&SyncState> {
        self.by_workload.get(r)
    }

    /// One aggregate concern for the queue — per-workload drift entries
    /// would drown real incidents; the badges carry the detail.
    pub fn concern(&self) -> Option<Concern> {
        let total = self.drifted + self.missing;
        if total == 0 {
            return None;
        }
        let severity = if self.missing > 0 {
            Severity::Warning
        } else {
            Severity::Info
        };
        Some(Concern {
            severity,
            title: format!("pair drift: {total} workloads differ hot↔warm"),
            detail: format!("{} missing · {} drifting", self.missing, self.drifted),
            target: Target::WorkloadList,
            key: "pair:drift".into(),
            cluster: ClusterId::Hot,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    fn set_image(d: &mut k8s_openapi::api::apps::v1::Deployment, image: &str) {
        d.spec
            .as_mut()
            .unwrap()
            .template
            .spec
            .as_mut()
            .unwrap()
            .containers[0]
            .image = Some(image.into());
    }

    #[test]
    fn detects_presence_replica_and_image_drift() {
        let (hot, mut hs) = fx::world();
        let (warm, mut ws) = fx::world();

        // In sync.
        let mut a = fx::deployment("demo", "same", 3, 3);
        set_image(&mut a, "app:1");
        hs.deployment(a.clone());
        ws.deployment(a);

        // Replica drift.
        let mut h = fx::deployment("demo", "scaled", 3, 3);
        set_image(&mut h, "app:1");
        let mut w = fx::deployment("demo", "scaled", 1, 1);
        set_image(&mut w, "app:1");
        hs.deployment(h);
        ws.deployment(w);

        // Image drift.
        let mut h = fx::deployment("demo", "rolled", 2, 2);
        set_image(&mut h, "app:2");
        let mut w = fx::deployment("demo", "rolled", 2, 2);
        set_image(&mut w, "app:1");
        hs.deployment(h);
        ws.deployment(w);

        // Missing on warm / only on warm.
        hs.deployment(fx::deployment("demo", "hot-only", 1, 1));
        ws.deployment(fx::deployment("demo", "warm-only", 1, 1));

        let pair = PairSync::build(&hot, &warm, &NamespaceFilter::All);
        let get = |name: &str| {
            pair.state(&WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: "demo".into(),
                name: name.into(),
            })
            .unwrap()
            .clone()
        };
        assert_eq!(get("same"), SyncState::InSync);
        assert_eq!(
            get("scaled"),
            SyncState::Drift {
                replicas: Some((3, 1)),
                images: false
            }
        );
        assert_eq!(
            get("rolled"),
            SyncState::Drift {
                replicas: None,
                images: true
            }
        );
        assert_eq!(get("hot-only"), SyncState::OnlyHot);
        assert_eq!(get("warm-only"), SyncState::OnlyWarm);
        assert_eq!(pair.drifted, 2);
        assert_eq!(pair.missing, 2);

        let c = pair.concern().expect("aggregate concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(c.title.contains("4 workloads differ"));

        assert_eq!(get("scaled").badge(), "≠r");
        assert_eq!(get("rolled").badge(), "≠i");
        assert_eq!(get("hot-only").badge(), "−w");
        assert_eq!(
            get("scaled").describe(ClusterId::Hot),
            "replicas hot 3 ≠ warm 1"
        );
        assert_eq!(get("hot-only").describe(ClusterId::Hot), "warm: MISSING");
        assert_eq!(get("warm-only").describe(ClusterId::Warm), "hot: MISSING");
    }

    #[test]
    fn daemonset_replica_counts_are_exempt() {
        let (hot, mut hs) = fx::world();
        let (warm, mut ws) = fx::world();
        // Different "desired" (tracks node count) but same images → in sync.
        hs.daemonset(fx::daemonset("demo", "agent", 5, 5));
        ws.daemonset(fx::daemonset("demo", "agent", 2, 2));
        let pair = PairSync::build(&hot, &warm, &NamespaceFilter::All);
        let st = pair
            .state(&WorkloadRef {
                kind: WorkloadKind::DaemonSet,
                namespace: "demo".into(),
                name: "agent".into(),
            })
            .unwrap();
        assert!(st.is_in_sync(), "{st:?}");
        assert!(pair.concern().is_none());
    }

    #[test]
    fn identical_worlds_produce_no_concern() {
        let (hot, mut hs) = fx::world();
        let (warm, mut ws) = fx::world();
        hs.deployment(fx::deployment("demo", "web", 3, 3));
        ws.deployment(fx::deployment("demo", "web", 3, 3));
        let pair = PairSync::build(&hot, &warm, &NamespaceFilter::All);
        assert_eq!(pair.drifted + pair.missing, 0);
        assert!(pair.concern().is_none());
    }

    #[test]
    fn namespace_filter_scopes_pair_drift() {
        let (hot, mut hs) = fx::world();
        let (warm, mut ws) = fx::world();
        // demo/web is in sync; kube-system/coredns drifts (only on hot).
        hs.deployment(fx::deployment("demo", "web", 1, 1));
        ws.deployment(fx::deployment("demo", "web", 1, 1));
        hs.deployment(fx::deployment("kube-system", "coredns", 2, 2));

        // Unfiltered: coredns drift surfaces.
        let all = PairSync::build(&hot, &warm, &NamespaceFilter::All);
        assert_eq!(all.missing, 1);
        assert!(all.concern().is_some());

        // Scoped to demo: the kube-system drift is out of scope → no concern.
        let demo = PairSync::build(&hot, &warm, &NamespaceFilter::only("demo"));
        assert_eq!(demo.missing, 0);
        assert_eq!(demo.drifted, 0);
        assert!(demo.concern().is_none(), "out-of-ns drift leaked into pair");
    }
}
