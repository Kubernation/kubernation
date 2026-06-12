//! The attention queue: pure detectors over the observed world, aggregated
//! per workload/node so the operator sees "city in trouble", not a hundred
//! identical pod alarms. This is Civ's "next unit needs orders" loop.

use std::collections::{BTreeMap, HashMap, HashSet};

use k8s_openapi::jiff;

use super::model::{
    MapModel, OwnerIndex, PRESSURE_HIGH, PodState, RolloutStatus, WorkloadRef, WorkloadRow,
    pod_oom_killed, pod_restarts, pod_state,
};
use super::observed::ObservedWorld;

/// How long ago a Warning event may have fired and still surface here.
const EVENT_WINDOW_MIN: i64 = 15;
/// Restart count at which a pod is "flapping" even without a waiting reason.
const RESTART_THRESHOLD: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn glyph(self) -> &'static str {
        match self {
            Severity::Critical => "‼",
            Severity::Warning => "!",
            Severity::Info => "·",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Node(String),
    Workload(WorkloadRef),
    /// No better destination known — land on the workload list.
    WorkloadList,
}

#[derive(Debug, Clone)]
pub struct Concern {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub target: Target,
    /// Stable identity for cycling; also the sort tiebreaker.
    pub key: String,
}

#[derive(Default)]
struct Agg {
    crash: u32,
    image: u32,
    config: u32,
    failed: u32,
    unsched: u32,
    oom: u32,
    flapping: u32,
}

impl Agg {
    fn any(&self) -> bool {
        self.crash
            + self.image
            + self.config
            + self.failed
            + self.unsched
            + self.oom
            + self.flapping
            > 0
    }

    fn classify(&mut self, reason: &str, state: PodState) {
        match reason {
            "CrashLoopBackOff" => self.crash += 1,
            "ImagePullBackOff" | "ErrImagePull" | "InvalidImageName" => self.image += 1,
            "CreateContainerConfigError" | "CreateContainerError" | "RunContainerError" => {
                self.config += 1
            }
            "Unschedulable" => self.unsched += 1,
            _ if state == PodState::Failing => self.failed += 1,
            _ => {}
        }
    }

    /// The single most important thing to say about this group of pods.
    fn primary(&self) -> Option<(Severity, String)> {
        let crit = [
            (self.crash, "CrashLoopBackOff"),
            (self.image, "image pull failing"),
            (self.config, "container create failing"),
            (self.failed, "pods Failed"),
        ];
        for (n, label) in crit {
            if n > 0 {
                return Some((Severity::Critical, format!("{label} ×{n}")));
            }
        }
        let warn = [
            (self.unsched, "unschedulable"),
            (self.oom, "OOM-killed recently"),
            (self.flapping, "restarting repeatedly"),
        ];
        for (n, label) in warn {
            if n > 0 {
                return Some((Severity::Warning, format!("{label} ×{n}")));
            }
        }
        None
    }
}

pub fn build(world: &ObservedWorld, map: &MapModel, workloads: &[WorkloadRow]) -> Vec<Concern> {
    let idx = OwnerIndex::build(world);
    let mut concerns: Vec<Concern> = Vec::new();

    // --- Pod-level signals, aggregated per owning workload -----------------
    let mut by_workload: BTreeMap<WorkloadRef, Agg> = BTreeMap::new();
    for pod in world.pods.state() {
        let (state, reason) = pod_state(&pod);
        let mut agg = Agg::default();
        agg.classify(&reason, state);
        if pod_oom_killed(&pod) {
            agg.oom += 1;
        }
        if pod_restarts(&pod) >= RESTART_THRESHOLD {
            agg.flapping += 1;
        }
        if !agg.any() {
            continue;
        }
        match idx.workload_of(&pod) {
            Some(r) => {
                let e = by_workload.entry(r).or_default();
                e.crash += agg.crash;
                e.image += agg.image;
                e.config += agg.config;
                e.failed += agg.failed;
                e.unsched += agg.unsched;
                e.oom += agg.oom;
                e.flapping += agg.flapping;
            }
            None => {
                // Bare pod (or Job-owned — Jobs have no city screen yet).
                let ns = pod.metadata.namespace.clone().unwrap_or_default();
                let name = pod.metadata.name.clone().unwrap_or_default();
                let (severity, msg) = agg.primary().expect("agg.any() checked");
                let target = pod
                    .spec
                    .as_ref()
                    .and_then(|s| s.node_name.clone())
                    .map_or(Target::WorkloadList, Target::Node);
                concerns.push(Concern {
                    severity,
                    title: format!("pod {ns}/{name} — {msg}"),
                    detail: reason.clone(),
                    target,
                    key: format!("b:{ns}/{name}"),
                });
            }
        }
    }

    // --- Workload rows: merge pod aggregates with rollout/replica state ----
    let mut covered_workloads: HashSet<(String, String)> = HashSet::new();
    for row in workloads {
        let agg = by_workload.remove(&row.r);
        let gap = row.ready < row.desired;
        let stalled = row.status == RolloutStatus::Stalled;
        let pod_issue = agg.as_ref().and_then(Agg::primary);
        if !gap && !stalled && pod_issue.is_none() {
            continue;
        }
        let (severity, headline) = if let Some((sev, msg)) = pod_issue {
            (if stalled { Severity::Critical } else { sev }, msg)
        } else if stalled {
            (Severity::Critical, "rollout stalled".into())
        } else {
            (
                Severity::Warning,
                format!("{}/{} ready", row.ready, row.desired),
            )
        };
        let mut detail = format!(
            "{}/{} ready · rollout {}",
            row.ready, row.desired, row.status
        );
        if !row.note.is_empty() {
            detail.push_str(&format!(" ({})", row.note));
        }
        covered_workloads.insert((row.r.namespace.clone(), row.r.name.clone()));
        concerns.push(Concern {
            severity,
            title: format!("{} — {headline}", row.r),
            detail,
            target: Target::Workload(row.r.clone()),
            key: format!("w:{}/{}/{}", row.r.kind, row.r.namespace, row.r.name),
        });
    }
    // Aggregates whose workload row vanished (e.g. workload deleted while
    // pods linger) still deserve a line.
    for (r, agg) in by_workload {
        if let Some((severity, msg)) = agg.primary() {
            covered_workloads.insert((r.namespace.clone(), r.name.clone()));
            concerns.push(Concern {
                severity,
                title: format!("{r} — {msg}"),
                detail: String::new(),
                key: format!("w:{}/{}/{}", r.kind, r.namespace, r.name),
                target: Target::Workload(r),
            });
        }
    }

    // --- Nodes --------------------------------------------------------------
    let mut covered_nodes: HashSet<String> = HashSet::new();
    for zone in &map.zones {
        for tile in &zone.nodes {
            let (severity, headline) = if !tile.ready {
                (Severity::Critical, "NotReady".to_string())
            } else if !tile.abnormal.is_empty() {
                (
                    Severity::Warning,
                    format!("{} pressure", tile.abnormal.join("/")),
                )
            } else if tile.cpu_ratio >= PRESSURE_HIGH || tile.mem_ratio >= PRESSURE_HIGH {
                (
                    Severity::Warning,
                    format!(
                        "requests cpu {:.0}% mem {:.0}%",
                        tile.cpu_ratio * 100.0,
                        tile.mem_ratio * 100.0
                    ),
                )
            } else if tile.cordoned {
                (Severity::Info, "cordoned".to_string())
            } else {
                continue;
            };
            covered_nodes.insert(tile.name.clone());
            concerns.push(Concern {
                severity,
                title: format!("node {} — {headline}", tile.name),
                detail: format!(
                    "zone {} · {} pods · cpu {:.0}% mem {:.0}%",
                    tile.zone,
                    tile.pods.len(),
                    tile.cpu_ratio * 100.0,
                    tile.mem_ratio * 100.0
                ),
                target: Target::Node(tile.name.clone()),
                key: format!("n:{}", tile.name),
            });
        }
    }

    // --- PVCs ----------------------------------------------------------------
    for pvc in world.pvcs.state() {
        let phase = pvc
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("");
        if phase != "Pending" && phase != "Lost" {
            continue;
        }
        let ns = pvc.metadata.namespace.clone().unwrap_or_default();
        let name = pvc.metadata.name.clone().unwrap_or_default();
        let owner = pvc_owner(world, &idx, &ns, &name);
        let sc = pvc
            .spec
            .as_ref()
            .and_then(|s| s.storage_class_name.clone())
            .unwrap_or_else(|| "default".into());
        concerns.push(Concern {
            severity: Severity::Warning,
            title: format!("pvc {ns}/{name} — {phase}"),
            detail: format!("storageClass {sc}"),
            target: owner.map_or(Target::WorkloadList, Target::Workload),
            key: format!("p:{ns}/{name}"),
        });
    }

    // --- Recent Warning events not already covered above ---------------------
    let now = jiff::Timestamp::now();
    let mut event_groups: BTreeMap<(String, String, String), (u32, String)> = BTreeMap::new();
    for ev in world.recent_events() {
        if !ev.warning {
            continue;
        }
        let stale = ev
            .when
            .as_ref()
            .is_none_or(|t| now.duration_since(t.0).as_secs() > EVENT_WINDOW_MIN * 60);
        if stale {
            continue;
        }
        if ev.kind == "Node" && covered_nodes.contains(&ev.name) {
            continue;
        }
        if covered_workloads.contains(&(ev.namespace.clone(), ev.name.clone())) {
            continue;
        }
        if ev.kind == "Pod" {
            // Skip if the pod's workload already has a concern.
            let owned = world.pods.state().iter().any(|p| {
                p.metadata.name.as_deref() == Some(&ev.name)
                    && p.metadata.namespace.as_deref() == Some(&ev.namespace)
                    && idx.workload_of(p).is_some_and(|r| {
                        covered_workloads.contains(&(r.namespace.clone(), r.name.clone()))
                    })
            });
            if owned {
                continue;
            }
        }
        let entry = event_groups
            .entry((ev.kind.clone(), ev.namespace.clone(), ev.name.clone()))
            .or_insert((0, ev.reason.clone()));
        entry.0 += ev.count.max(1) as u32;
        entry.1 = ev.reason.clone();
    }
    for ((kind, ns, name), (count, reason)) in event_groups.into_iter().take(20) {
        let target = event_target(world, &idx, &kind, &ns, &name);
        let place = if ns.is_empty() {
            name.clone()
        } else {
            format!("{ns}/{name}")
        };
        concerns.push(Concern {
            severity: Severity::Info,
            title: format!(
                "events: {reason} ×{count} on {} {place}",
                kind.to_lowercase()
            ),
            detail: String::new(),
            target,
            key: format!("e:{kind}/{ns}/{name}"),
        });
    }

    concerns.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.key.cmp(&b.key)));
    concerns
}

/// Find the StatefulSet a PVC belongs to (claim-template naming), or the
/// workload of any pod mounting it.
fn pvc_owner(world: &ObservedWorld, idx: &OwnerIndex, ns: &str, name: &str) -> Option<WorkloadRef> {
    for s in world.statefulsets.state() {
        if s.metadata.namespace.as_deref() != Some(ns) {
            continue;
        }
        let sts_name = s.metadata.name.as_deref().unwrap_or_default();
        for t in s
            .spec
            .as_ref()
            .and_then(|sp| sp.volume_claim_templates.as_deref())
            .unwrap_or(&[])
        {
            let tmpl = t.metadata.name.as_deref().unwrap_or_default();
            if name.starts_with(&format!("{tmpl}-{sts_name}-")) {
                return Some(WorkloadRef {
                    kind: super::model::WorkloadKind::StatefulSet,
                    namespace: ns.to_string(),
                    name: sts_name.to_string(),
                });
            }
        }
    }
    for p in world.pods.state() {
        if p.metadata.namespace.as_deref() != Some(ns) {
            continue;
        }
        let mounts = p
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_deref())
            .unwrap_or(&[])
            .iter()
            .any(|v| {
                v.persistent_volume_claim
                    .as_ref()
                    .is_some_and(|c| c.claim_name == name)
            });
        if mounts && let Some(r) = idx.workload_of(&p) {
            return Some(r);
        }
    }
    None
}

fn event_target(
    world: &ObservedWorld,
    idx: &OwnerIndex,
    kind: &str,
    ns: &str,
    name: &str,
) -> Target {
    match kind {
        "Node" => Target::Node(name.to_string()),
        "Deployment" => Target::Workload(WorkloadRef {
            kind: super::model::WorkloadKind::Deployment,
            namespace: ns.into(),
            name: name.into(),
        }),
        "StatefulSet" => Target::Workload(WorkloadRef {
            kind: super::model::WorkloadKind::StatefulSet,
            namespace: ns.into(),
            name: name.into(),
        }),
        "DaemonSet" => Target::Workload(WorkloadRef {
            kind: super::model::WorkloadKind::DaemonSet,
            namespace: ns.into(),
            name: name.into(),
        }),
        "Pod" => world
            .pods
            .state()
            .iter()
            .find(|p| {
                p.metadata.name.as_deref() == Some(name)
                    && p.metadata.namespace.as_deref() == Some(ns)
            })
            .and_then(|p| idx.workload_of(p))
            .map_or(Target::WorkloadList, Target::Workload),
        _ => Target::WorkloadList,
    }
}

/// Counts per severity, for the collapsed panel summary.
pub fn severity_counts(concerns: &[Concern]) -> HashMap<Severity, usize> {
    let mut out = HashMap::new();
    for c in concerns {
        *out.entry(c.severity).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use crate::state::model::{WorkloadKind, build_map, build_workloads};
    use crate::state::observed::ObservedWorld;

    fn concerns(world: &ObservedWorld) -> Vec<Concern> {
        let map = build_map(world);
        let rows = build_workloads(world);
        build(world, &map, &rows)
    }

    #[test]
    fn crashloop_pods_aggregate_into_one_workload_concern() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "crashy", 3, 1));
        s.replicaset(fx::replicaset("demo", "crashy-abc", "crashy"));
        for i in 0..2 {
            s.pod(fx::pod_owned(
                fx::pod_waiting(
                    fx::pod("demo", &format!("crashy-abc-{i}"), Some("n1")),
                    "CrashLoopBackOff",
                ),
                "ReplicaSet",
                "crashy-abc",
            ));
        }
        let cs = concerns(&world);
        let workload: Vec<&Concern> = cs.iter().filter(|c| c.key.starts_with("w:")).collect();
        assert_eq!(workload.len(), 1, "one aggregated concern, got {cs:?}");
        let c = workload[0];
        assert_eq!(c.severity, Severity::Critical);
        assert!(c.title.contains("deploy demo/crashy"), "{}", c.title);
        assert!(c.title.contains("CrashLoopBackOff ×2"), "{}", c.title);
        assert!(matches!(&c.target, Target::Workload(r) if r.name == "crashy"));
        // No per-pod entries for owned pods.
        assert!(cs.iter().all(|c| !c.key.starts_with("b:")));
    }

    #[test]
    fn bare_pod_concern_targets_its_node() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod_waiting(
            fx::pod("demo", "loner", Some("n1")),
            "ImagePullBackOff",
        ));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "b:demo/loner")
            .expect("bare pod concern");
        assert_eq!(c.severity, Severity::Critical);
        assert_eq!(c.target, Target::Node("n1".into()));
    }

    #[test]
    fn pending_pvc_targets_owning_statefulset() {
        let (world, mut s) = fx::world();
        let mut sts = fx::statefulset("demo", "db", 1, 1);
        sts.spec.as_mut().unwrap().volume_claim_templates =
            Some(vec![k8s_openapi::api::core::v1::PersistentVolumeClaim {
                metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                    name: Some("data".into()),
                    ..Default::default()
                },
                ..Default::default()
            }]);
        s.statefulset(sts);
        s.pvc(fx::pvc("demo", "data-db-0", "Pending"));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key == "p:demo/data-db-0")
            .expect("pvc concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(
            matches!(&c.target, Target::Workload(r) if r.kind == WorkloadKind::StatefulSet && r.name == "db"),
            "{:?}",
            c.target
        );
    }

    #[test]
    fn flapping_daemonset_pod_is_a_warning() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.daemonset(fx::daemonset("demo", "agent", 1, 1));
        s.pod(fx::pod_owned(
            fx::pod_restarting(fx::pod("demo", "agent-x1", Some("n1")), 7),
            "DaemonSet",
            "agent",
        ));
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key.contains("agent"))
            .expect("flapping concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(c.title.contains("restarting repeatedly ×1"), "{}", c.title);
        assert!(matches!(&c.target, Target::Workload(r) if r.kind == WorkloadKind::DaemonSet));
    }

    #[test]
    fn replica_gap_is_warning_and_stall_is_critical() {
        let (world, mut s) = fx::world();
        let mut gap = fx::deployment("demo", "gappy", 3, 1);
        gap.status.as_mut().unwrap().updated_replicas = Some(3);
        s.deployment(gap);
        let cs = concerns(&world);
        let c = cs
            .iter()
            .find(|c| c.key.contains("gappy"))
            .expect("gap concern");
        assert_eq!(c.severity, Severity::Warning);
        assert!(c.title.contains("1/3 ready"), "{}", c.title);
    }

    #[test]
    fn node_states_and_global_ordering() {
        let (world, mut s) = fx::world();
        s.node(fx::node_with_condition(
            fx::node("n-bad", Some("z-a")),
            "Ready",
            "False",
        ));
        s.node(fx::cordoned(fx::node("n-cord", Some("z-a"))));
        s.node(fx::node("n-ok", Some("z-a")));
        let cs = concerns(&world);
        assert_eq!(cs.len(), 2);
        // Critical (NotReady) sorts before Info (cordoned).
        assert_eq!(cs[0].severity, Severity::Critical);
        assert!(cs[0].title.contains("n-bad"));
        assert!(cs[0].title.contains("NotReady"));
        assert_eq!(cs[1].severity, Severity::Info);
        assert!(cs[1].title.contains("cordoned"));
        // Healthy node contributes nothing.
        assert!(!cs.iter().any(|c| c.title.contains("n-ok")));
    }

    #[test]
    fn severity_counts_tally() {
        let (world, mut s) = fx::world();
        s.node(fx::node_with_condition(
            fx::node("n-bad", None),
            "Ready",
            "False",
        ));
        s.node(fx::cordoned(fx::node("n-cord", None)));
        let cs = concerns(&world);
        let counts = severity_counts(&cs);
        assert_eq!(counts.get(&Severity::Critical), Some(&1));
        assert_eq!(counts.get(&Severity::Info), Some(&1));
    }
}
