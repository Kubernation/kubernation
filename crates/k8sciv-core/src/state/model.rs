//! Pure derivations from the observed world into render-ready view models.
//! Everything here is a function of `ObservedWorld` snapshots — no I/O, no
//! mutation — which is what makes the interesting logic unit-testable.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::{Container, Node, Pod, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

use super::attention::{self, Concern, Severity, Target};
use super::observed::{ObservedWorld, RecentEvent};
use super::world::{WorldModel, build_world};
use crate::k8s::quantity;
use crate::util::fnv1a64;

pub const ZONE_LABEL: &str = "topology.kubernetes.io/zone";
pub const ZONE_LABEL_LEGACY: &str = "failure-domain.beta.kubernetes.io/zone";
pub const UNZONED: &str = "unzoned";

/// Request-pressure buckets shared by tiles, gauges, and attention.
pub const PRESSURE_ELEVATED: f64 = 0.7;
pub const PRESSURE_HIGH: f64 = 0.9;

// ---------------------------------------------------------------------------
// Pods

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodState {
    Ok,
    Starting,
    Pending,
    Terminating,
    Failing,
    Succeeded,
}

/// Classify a pod and give the short reason shown in tables
/// ("CrashLoopBackOff", "ContainerCreating", "Running", ...).
pub fn pod_state(pod: &Pod) -> (PodState, String) {
    let status = pod.status.as_ref();
    let phase = status.and_then(|s| s.phase.as_deref()).unwrap_or("Unknown");
    if pod.metadata.deletion_timestamp.is_some() {
        return (PodState::Terminating, "Terminating".into());
    }
    match phase {
        "Succeeded" => return (PodState::Succeeded, "Succeeded".into()),
        "Failed" => {
            let reason = status
                .and_then(|s| s.reason.clone())
                .unwrap_or_else(|| "Failed".into());
            return (PodState::Failing, reason);
        }
        _ => {}
    }

    let container_statuses = status.and_then(|s| s.container_statuses.as_ref());
    if let Some(cs) = container_statuses {
        for c in cs {
            if let Some(w) = c.state.as_ref().and_then(|s| s.waiting.as_ref())
                && let Some(r) = w.reason.as_deref()
                && matches!(
                    r,
                    "CrashLoopBackOff"
                        | "ImagePullBackOff"
                        | "ErrImagePull"
                        | "InvalidImageName"
                        | "CreateContainerConfigError"
                        | "CreateContainerError"
                        | "RunContainerError"
                )
            {
                return (PodState::Failing, r.to_string());
            }
        }
    }

    match phase {
        "Running" => {
            let all_ready = container_statuses.is_none_or(|cs| cs.iter().all(|c| c.ready));
            if all_ready {
                (PodState::Ok, "Running".into())
            } else {
                (PodState::Starting, "NotReady".into())
            }
        }
        "Pending" => {
            if let Some(conds) = status.and_then(|s| s.conditions.as_ref())
                && conds.iter().any(|c| {
                    c.type_ == "PodScheduled"
                        && c.status == "False"
                        && c.reason.as_deref() == Some("Unschedulable")
                })
            {
                return (PodState::Pending, "Unschedulable".into());
            }
            if let Some(cs) = container_statuses {
                for c in cs {
                    if let Some(w) = c.state.as_ref().and_then(|s| s.waiting.as_ref())
                        && let Some(r) = w.reason.as_deref()
                    {
                        return (PodState::Pending, r.to_string());
                    }
                }
            }
            (PodState::Pending, "Pending".into())
        }
        other => (PodState::Pending, other.to_string()),
    }
}

pub fn pod_restarts(pod: &Pod) -> i32 {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

/// True when the pod's last container exit was an OOM kill.
pub fn pod_oom_killed(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .is_some_and(|cs| {
            cs.iter().any(|c| {
                c.last_state
                    .as_ref()
                    .and_then(|s| s.terminated.as_ref())
                    .and_then(|t| t.reason.as_deref())
                    == Some("OOMKilled")
            })
        })
}

// ---------------------------------------------------------------------------
// Workload identity & pod ownership

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkloadKind {
    Deployment,
    StatefulSet,
    DaemonSet,
}

impl fmt::Display for WorkloadKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WorkloadKind::Deployment => "deploy",
            WorkloadKind::StatefulSet => "sts",
            WorkloadKind::DaemonSet => "ds",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkloadRef {
    pub kind: WorkloadKind,
    pub namespace: String,
    pub name: String,
}

impl fmt::Display for WorkloadRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}/{}", self.kind, self.namespace, self.name)
    }
}

/// Resolves pod → owning workload. ReplicaSet hops to its Deployment via an
/// index built once per rebuild, so resolution is O(1) per pod.
#[derive(Default)]
pub struct OwnerIndex {
    rs_to_deploy: HashMap<(String, String), String>,
}

impl OwnerIndex {
    pub fn build(world: &ObservedWorld) -> Self {
        let mut rs_to_deploy = HashMap::new();
        for rs in world.replicasets.state() {
            let ns = rs.metadata.namespace.clone().unwrap_or_default();
            let name = rs.metadata.name.clone().unwrap_or_default();
            if let Some(owner) = controller_owner(rs.metadata.owner_references.as_deref())
                && owner.0 == "Deployment"
            {
                rs_to_deploy.insert((ns, name), owner.1.to_string());
            }
        }
        Self { rs_to_deploy }
    }

    pub fn workload_of(&self, pod: &Pod) -> Option<WorkloadRef> {
        let ns = pod.metadata.namespace.clone().unwrap_or_default();
        let (kind, name) = controller_owner(pod.metadata.owner_references.as_deref())?;
        match kind {
            "ReplicaSet" => {
                let deploy = self.rs_to_deploy.get(&(ns.clone(), name.to_string()))?;
                Some(WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: ns,
                    name: deploy.clone(),
                })
            }
            "StatefulSet" => Some(WorkloadRef {
                kind: WorkloadKind::StatefulSet,
                namespace: ns,
                name: name.to_string(),
            }),
            "DaemonSet" => Some(WorkloadRef {
                kind: WorkloadKind::DaemonSet,
                namespace: ns,
                name: name.to_string(),
            }),
            _ => None,
        }
    }
}

fn controller_owner(
    refs: Option<&[k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference]>,
) -> Option<(&str, &str)> {
    refs?
        .iter()
        .find(|o| o.controller == Some(true))
        .map(|o| (o.kind.as_str(), o.name.as_str()))
}

// ---------------------------------------------------------------------------
// Map model

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeHealth {
    Healthy,
    Cordoned,
    Pressure,
    NotReady,
}

#[derive(Debug, Clone)]
pub struct PodGlyph {
    pub namespace: String,
    pub name: String,
    pub state: PodState,
    /// Controller workload (through the RS chain), for city placement.
    pub owner: Option<WorkloadRef>,
}

#[derive(Debug, Clone)]
pub struct NodeTile {
    pub name: String,
    pub zone: String,
    pub health: NodeHealth,
    pub ready: bool,
    pub cordoned: bool,
    /// Abnormal condition short names: "Mem", "Disk", "PID", "Net".
    pub abnormal: Vec<&'static str>,
    /// Scheduling pressure: sum of pod requests / allocatable (NOT live
    /// usage — see CLAUDE.md "pressure semantics").
    pub cpu_ratio: f64,
    pub mem_ratio: f64,
    pub pods: Vec<PodGlyph>,
}

#[derive(Debug, Clone, Default)]
pub struct ZoneColumn {
    pub name: String,
    pub nodes: Vec<NodeTile>,
}

#[derive(Debug, Clone, Default)]
pub struct MapModel {
    pub zones: Vec<ZoneColumn>,
    pub total_nodes: usize,
    pub total_pods: usize,
}

impl MapModel {}

pub fn node_zone(node: &Node) -> String {
    node.metadata
        .labels
        .as_ref()
        .and_then(|l| l.get(ZONE_LABEL).or_else(|| l.get(ZONE_LABEL_LEGACY)))
        .cloned()
        .unwrap_or_else(|| UNZONED.into())
}

fn node_condition(node: &Node, type_: &str) -> Option<String> {
    node.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .and_then(|cs| cs.iter().find(|c| c.type_ == type_))
        .map(|c| c.status.clone())
}

pub fn node_ready(node: &Node) -> bool {
    node_condition(node, "Ready").as_deref() == Some("True")
}

/// Sum of CPU/memory *requests* of non-terminal pods on this node, divided
/// by allocatable. Missing allocatable yields 0 (gauge renders empty).
pub fn node_request_ratios(node: &Node, pods: &[&Pod]) -> (f64, f64) {
    let alloc = node.status.as_ref().and_then(|s| s.allocatable.as_ref());
    let alloc_cpu = alloc
        .and_then(|a| a.get("cpu"))
        .and_then(quantity::value)
        .unwrap_or(0.0);
    let alloc_mem = alloc
        .and_then(|a| a.get("memory"))
        .and_then(quantity::value)
        .unwrap_or(0.0);

    let (mut cpu, mut mem) = (0.0, 0.0);
    for pod in pods {
        let phase = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("");
        if phase == "Succeeded" || phase == "Failed" {
            continue;
        }
        if let Some(spec) = pod.spec.as_ref() {
            for c in &spec.containers {
                let req = c.resources.as_ref().and_then(|r| r.requests.as_ref());
                cpu += req
                    .and_then(|r| r.get("cpu"))
                    .and_then(quantity::value)
                    .unwrap_or(0.0);
                mem += req
                    .and_then(|r| r.get("memory"))
                    .and_then(quantity::value)
                    .unwrap_or(0.0);
            }
        }
    }
    let ratio = |used: f64, alloc: f64| if alloc > 0.0 { used / alloc } else { 0.0 };
    (ratio(cpu, alloc_cpu), ratio(mem, alloc_mem))
}

pub fn build_node_tile(node: &Node, pods_on_node: &[&Pod], idx: &OwnerIndex) -> NodeTile {
    let ready = node_ready(node);
    let cordoned = node
        .spec
        .as_ref()
        .and_then(|s| s.unschedulable)
        .unwrap_or(false);
    let mut abnormal = Vec::new();
    for (cond, short) in [
        ("MemoryPressure", "Mem"),
        ("DiskPressure", "Disk"),
        ("PIDPressure", "PID"),
        ("NetworkUnavailable", "Net"),
    ] {
        if node_condition(node, cond).as_deref() == Some("True") {
            abnormal.push(short);
        }
    }
    let (cpu_ratio, mem_ratio) = node_request_ratios(node, pods_on_node);

    let health = if !ready {
        NodeHealth::NotReady
    } else if cordoned {
        NodeHealth::Cordoned
    } else if !abnormal.is_empty() || cpu_ratio >= PRESSURE_HIGH || mem_ratio >= PRESSURE_HIGH {
        NodeHealth::Pressure
    } else {
        NodeHealth::Healthy
    };

    let mut pods: Vec<PodGlyph> = pods_on_node
        .iter()
        .map(|p| PodGlyph {
            namespace: p.metadata.namespace.clone().unwrap_or_default(),
            name: p.metadata.name.clone().unwrap_or_default(),
            state: pod_state(p).0,
            owner: idx.workload_of(p),
        })
        .collect();
    pods.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));

    NodeTile {
        name: node.metadata.name.clone().unwrap_or_default(),
        zone: node_zone(node),
        health,
        ready,
        cordoned,
        abnormal,
        cpu_ratio,
        mem_ratio,
        pods,
    }
}

/// Zone columns sorted by name; nodes within a zone ordered by stable hash
/// of the node name so the layout never reshuffles between reconciles.
pub fn build_map(world: &ObservedWorld) -> MapModel {
    let idx = OwnerIndex::build(world);
    let pods = world.pods.state();
    let mut by_node: HashMap<String, Vec<&Pod>> = HashMap::new();
    for p in &pods {
        if let Some(node) = p.spec.as_ref().and_then(|s| s.node_name.clone()) {
            by_node.entry(node).or_default().push(p.as_ref());
        }
    }

    let nodes = world.nodes.state();
    let mut zones: BTreeMap<String, Vec<NodeTile>> = BTreeMap::new();
    for node in &nodes {
        let name = node.metadata.name.clone().unwrap_or_default();
        let on_node = by_node.get(&name).map(Vec::as_slice).unwrap_or(&[]);
        let tile = build_node_tile(node, on_node, &idx);
        zones.entry(tile.zone.clone()).or_default().push(tile);
    }
    let mut zones: Vec<ZoneColumn> = zones
        .into_iter()
        .map(|(name, mut nodes)| {
            nodes.sort_by_key(|t| (fnv1a64(&t.name), t.name.clone()));
            ZoneColumn { name, nodes }
        })
        .collect();
    // "unzoned" sinks to the end rather than sorting alphabetically.
    zones.sort_by_key(|z| (z.name == UNZONED, z.name.clone()));

    MapModel {
        total_nodes: nodes.len(),
        total_pods: pods.len(),
        zones,
    }
}

// ---------------------------------------------------------------------------
// Workload rows & rollout status

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutStatus {
    Complete,
    Progressing,
    Stalled,
    Paused,
}

impl fmt::Display for RolloutStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RolloutStatus::Complete => "Complete",
            RolloutStatus::Progressing => "Progressing",
            RolloutStatus::Stalled => "Stalled",
            RolloutStatus::Paused => "Paused",
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkloadRow {
    pub r: WorkloadRef,
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
    pub updated: i32,
    pub status: RolloutStatus,
    pub note: String,
    pub age: Option<Time>,
}

pub fn deployment_status(d: &Deployment) -> (RolloutStatus, String) {
    if d.spec.as_ref().and_then(|s| s.paused) == Some(true) {
        return (RolloutStatus::Paused, "rollout paused".into());
    }
    let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
    let st = d.status.as_ref();
    let generation = d.metadata.generation.unwrap_or(0);
    let observed = st.and_then(|s| s.observed_generation).unwrap_or(0);
    if observed < generation {
        return (RolloutStatus::Progressing, "awaiting observation".into());
    }
    if let Some(conds) = st.and_then(|s| s.conditions.as_ref())
        && conds.iter().any(|c| {
            c.type_ == "Progressing" && c.reason.as_deref() == Some("ProgressDeadlineExceeded")
        })
    {
        return (RolloutStatus::Stalled, "progress deadline exceeded".into());
    }
    let updated = st.and_then(|s| s.updated_replicas).unwrap_or(0);
    let total = st.and_then(|s| s.replicas).unwrap_or(0);
    let available = st.and_then(|s| s.available_replicas).unwrap_or(0);
    if updated < desired {
        return (
            RolloutStatus::Progressing,
            format!("updating {updated}/{desired}"),
        );
    }
    if total > updated {
        return (
            RolloutStatus::Progressing,
            format!("terminating {} old", total - updated),
        );
    }
    if available < updated {
        return (
            RolloutStatus::Progressing,
            format!("available {available}/{updated}"),
        );
    }
    (RolloutStatus::Complete, String::new())
}

pub fn statefulset_status(s: &StatefulSet) -> (RolloutStatus, String) {
    let desired = s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1);
    let st = s.status.as_ref();
    let ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
    let current_rev = st.and_then(|s| s.current_revision.as_deref()).unwrap_or("");
    let update_rev = st.and_then(|s| s.update_revision.as_deref()).unwrap_or("");
    if !update_rev.is_empty() && current_rev != update_rev {
        return (RolloutStatus::Progressing, "rolling update".into());
    }
    if ready < desired {
        return (
            RolloutStatus::Progressing,
            format!("ready {ready}/{desired}"),
        );
    }
    (RolloutStatus::Complete, String::new())
}

pub fn daemonset_status(d: &DaemonSet) -> (RolloutStatus, String) {
    let st = d.status.as_ref();
    let desired = st.map(|s| s.desired_number_scheduled).unwrap_or(0);
    let ready = st.map(|s| s.number_ready).unwrap_or(0);
    let updated = st
        .and_then(|s| s.updated_number_scheduled)
        .unwrap_or(desired);
    if updated < desired {
        return (
            RolloutStatus::Progressing,
            format!("updating {updated}/{desired}"),
        );
    }
    if ready < desired {
        return (
            RolloutStatus::Progressing,
            format!("ready {ready}/{desired}"),
        );
    }
    (RolloutStatus::Complete, String::new())
}

pub fn build_workloads(world: &ObservedWorld) -> Vec<WorkloadRow> {
    let mut rows = Vec::new();
    for d in world.deployments.state() {
        let (status, note) = deployment_status(&d);
        let st = d.status.as_ref();
        rows.push(WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: d.metadata.namespace.clone().unwrap_or_default(),
                name: d.metadata.name.clone().unwrap_or_default(),
            },
            desired: d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1),
            ready: st.and_then(|s| s.ready_replicas).unwrap_or(0),
            available: st.and_then(|s| s.available_replicas).unwrap_or(0),
            updated: st.and_then(|s| s.updated_replicas).unwrap_or(0),
            status,
            note,
            age: d.metadata.creation_timestamp.clone(),
        });
    }
    for s in world.statefulsets.state() {
        let (status, note) = statefulset_status(&s);
        let st = s.status.as_ref();
        rows.push(WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::StatefulSet,
                namespace: s.metadata.namespace.clone().unwrap_or_default(),
                name: s.metadata.name.clone().unwrap_or_default(),
            },
            desired: s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1),
            ready: st.and_then(|s| s.ready_replicas).unwrap_or(0),
            available: st.and_then(|s| s.available_replicas).unwrap_or(0),
            updated: st.and_then(|s| s.updated_replicas).unwrap_or(0),
            status,
            note,
            age: s.metadata.creation_timestamp.clone(),
        });
    }
    for d in world.daemonsets.state() {
        let (status, note) = daemonset_status(&d);
        let st = d.status.as_ref();
        rows.push(WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::DaemonSet,
                namespace: d.metadata.namespace.clone().unwrap_or_default(),
                name: d.metadata.name.clone().unwrap_or_default(),
            },
            desired: st.map(|s| s.desired_number_scheduled).unwrap_or(0),
            ready: st.map(|s| s.number_ready).unwrap_or(0),
            available: st.and_then(|s| s.number_available).unwrap_or(0),
            updated: st.and_then(|s| s.updated_number_scheduled).unwrap_or(0),
            status,
            note,
            age: d.metadata.creation_timestamp.clone(),
        });
    }
    rows.sort_by(|a, b| a.r.cmp(&b.r));
    rows
}

// ---------------------------------------------------------------------------
// City screen model

#[derive(Debug, Clone)]
pub struct CityPod {
    pub name: String,
    pub state: PodState,
    pub reason: String,
    pub restarts: i32,
    pub age: Option<Time>,
    pub node: String,
}

#[derive(Debug, Clone)]
pub struct OwnedRes {
    pub kind: &'static str, // "svc" | "cm" | "secret" | "pvc"
    pub name: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct CityModel {
    pub r: WorkloadRef,
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
    pub updated: i32,
    pub status: RolloutStatus,
    pub note: String,
    pub strategy: String,
    pub age: Option<Time>,
    pub pods: Vec<CityPod>,
    pub owned: Vec<OwnedRes>,
    pub events: Vec<RecentEvent>, // newest first
}

fn template_labels(t: Option<&PodTemplateSpec>) -> BTreeMap<String, String> {
    t.and_then(|t| t.metadata.as_ref())
        .and_then(|m| m.labels.clone())
        .unwrap_or_default()
}

fn collect_refs(containers: &[Container], out: &mut BTreeSet<(&'static str, String)>) {
    for c in containers {
        for e in c.env.as_deref().unwrap_or(&[]) {
            if let Some(v) = e.value_from.as_ref() {
                if let Some(r) = v.config_map_key_ref.as_ref() {
                    out.insert(("cm", r.name.clone()));
                }
                if let Some(r) = v.secret_key_ref.as_ref() {
                    out.insert(("secret", r.name.clone()));
                }
            }
        }
        for e in c.env_from.as_deref().unwrap_or(&[]) {
            if let Some(r) = e.config_map_ref.as_ref() {
                out.insert(("cm", r.name.clone()));
            }
            if let Some(r) = e.secret_ref.as_ref() {
                out.insert(("secret", r.name.clone()));
            }
        }
    }
}

/// ConfigMap/Secret references straight from the pod template — we observe
/// the *shape* of dependencies without ever watching Secret contents.
fn template_refs(t: Option<&PodTemplateSpec>) -> BTreeSet<(&'static str, String)> {
    let mut out = BTreeSet::new();
    let Some(spec) = t.and_then(|t| t.spec.as_ref()) else {
        return out;
    };
    collect_refs(&spec.containers, &mut out);
    collect_refs(spec.init_containers.as_deref().unwrap_or(&[]), &mut out);
    for v in spec.volumes.as_deref().unwrap_or(&[]) {
        if let Some(cm) = v.config_map.as_ref() {
            out.insert(("cm", cm.name.clone()));
        }
        if let Some(s) = v.secret.as_ref()
            && let Some(n) = s.secret_name.clone()
        {
            out.insert(("secret", n));
        }
    }
    out
}

pub fn build_city(world: &ObservedWorld, r: &WorkloadRef) -> Option<CityModel> {
    let idx = OwnerIndex::build(world);

    // Header numbers + template, from whichever kind this is.
    let (desired, ready, available, updated, status, note, strategy, age, template);
    match r.kind {
        WorkloadKind::Deployment => {
            let d = world.deployments.state().into_iter().find(|d| {
                d.metadata.namespace.as_deref() == Some(&r.namespace)
                    && d.metadata.name.as_deref() == Some(&r.name)
            })?;
            let st = d.status.as_ref();
            desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
            ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
            available = st.and_then(|s| s.available_replicas).unwrap_or(0);
            updated = st.and_then(|s| s.updated_replicas).unwrap_or(0);
            (status, note) = deployment_status(&d);
            strategy = d
                .spec
                .as_ref()
                .and_then(|s| s.strategy.as_ref())
                .and_then(|s| s.type_.clone())
                .unwrap_or_else(|| "RollingUpdate".into());
            age = d.metadata.creation_timestamp.clone();
            template = d.spec.as_ref().map(|s| s.template.clone());
        }
        WorkloadKind::StatefulSet => {
            let s = world.statefulsets.state().into_iter().find(|s| {
                s.metadata.namespace.as_deref() == Some(&r.namespace)
                    && s.metadata.name.as_deref() == Some(&r.name)
            })?;
            let st = s.status.as_ref();
            desired = s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1);
            ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
            available = st.and_then(|s| s.available_replicas).unwrap_or(0);
            updated = st.and_then(|s| s.updated_replicas).unwrap_or(0);
            (status, note) = statefulset_status(&s);
            strategy = s
                .spec
                .as_ref()
                .and_then(|sp| sp.update_strategy.as_ref())
                .and_then(|u| u.type_.clone())
                .unwrap_or_else(|| "RollingUpdate".into());
            age = s.metadata.creation_timestamp.clone();
            template = s.spec.as_ref().map(|sp| sp.template.clone());
        }
        WorkloadKind::DaemonSet => {
            let d = world.daemonsets.state().into_iter().find(|d| {
                d.metadata.namespace.as_deref() == Some(&r.namespace)
                    && d.metadata.name.as_deref() == Some(&r.name)
            })?;
            let st = d.status.as_ref();
            desired = st.map(|s| s.desired_number_scheduled).unwrap_or(0);
            ready = st.map(|s| s.number_ready).unwrap_or(0);
            available = st.and_then(|s| s.number_available).unwrap_or(0);
            updated = st.and_then(|s| s.updated_number_scheduled).unwrap_or(0);
            (status, note) = daemonset_status(&d);
            strategy = d
                .spec
                .as_ref()
                .and_then(|sp| sp.update_strategy.as_ref())
                .and_then(|u| u.type_.clone())
                .unwrap_or_else(|| "RollingUpdate".into());
            age = d.metadata.creation_timestamp.clone();
            template = d.spec.as_ref().map(|sp| sp.template.clone());
        }
    }

    // Member pods via the ownership chain.
    let mut pods: Vec<CityPod> = Vec::new();
    let mut pod_names: BTreeSet<String> = BTreeSet::new();
    let mut pvc_names: BTreeSet<String> = BTreeSet::new();
    for p in world.pods.state() {
        if idx.workload_of(&p).as_ref() != Some(r) {
            continue;
        }
        let (state, reason) = pod_state(&p);
        let name = p.metadata.name.clone().unwrap_or_default();
        pod_names.insert(name.clone());
        for v in p
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_deref())
            .unwrap_or(&[])
        {
            if let Some(c) = v.persistent_volume_claim.as_ref() {
                pvc_names.insert(c.claim_name.clone());
            }
        }
        pods.push(CityPod {
            name,
            state,
            reason,
            restarts: pod_restarts(&p),
            age: p.metadata.creation_timestamp.clone(),
            node: p
                .spec
                .as_ref()
                .and_then(|s| s.node_name.clone())
                .unwrap_or_default(),
        });
    }
    pods.sort_by(|a, b| a.name.cmp(&b.name));

    // Owned resources.
    let labels = template_labels(template.as_ref());
    let mut owned: Vec<OwnedRes> = Vec::new();
    for svc in world.services.state() {
        if svc.metadata.namespace.as_deref() != Some(&r.namespace) {
            continue;
        }
        let Some(sel) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
            continue;
        };
        if !sel.is_empty() && sel.iter().all(|(k, v)| labels.get(k) == Some(v)) {
            owned.push(OwnedRes {
                kind: "svc",
                name: svc.metadata.name.clone().unwrap_or_default(),
                note: svc
                    .spec
                    .as_ref()
                    .and_then(|s| s.type_.clone())
                    .unwrap_or_default(),
            });
        }
    }
    // StatefulSet claims may exist before/after their pods do.
    if r.kind == WorkloadKind::StatefulSet
        && let Some(s) = world.statefulsets.state().into_iter().find(|s| {
            s.metadata.namespace.as_deref() == Some(&r.namespace)
                && s.metadata.name.as_deref() == Some(&r.name)
        })
    {
        for t in s
            .spec
            .as_ref()
            .and_then(|sp| sp.volume_claim_templates.as_deref())
            .unwrap_or(&[])
        {
            let prefix = format!(
                "{}-{}-",
                t.metadata.name.clone().unwrap_or_default(),
                r.name
            );
            for pvc in world.pvcs.state() {
                if pvc.metadata.namespace.as_deref() == Some(&r.namespace)
                    && pvc
                        .metadata
                        .name
                        .as_deref()
                        .is_some_and(|n| n.starts_with(&prefix))
                {
                    pvc_names.insert(pvc.metadata.name.clone().unwrap_or_default());
                }
            }
        }
    }
    for name in &pvc_names {
        let phase = world
            .pvcs
            .state()
            .into_iter()
            .find(|p| {
                p.metadata.namespace.as_deref() == Some(&r.namespace)
                    && p.metadata.name.as_deref() == Some(name)
            })
            .and_then(|p| p.status.as_ref().and_then(|s| s.phase.clone()))
            .unwrap_or_else(|| "?".into());
        owned.push(OwnedRes {
            kind: "pvc",
            name: name.clone(),
            note: phase,
        });
    }
    for (kind, name) in template_refs(template.as_ref()) {
        owned.push(OwnedRes {
            kind,
            name,
            note: String::new(),
        });
    }

    // Recent events touching the workload, its replicasets, or its pods.
    let prefix = format!("{}-", r.name);
    let mut events: Vec<RecentEvent> = world
        .recent_events()
        .into_iter()
        .filter(|e| {
            e.namespace == r.namespace
                && (e.name == r.name || e.name.starts_with(&prefix) || pod_names.contains(&e.name))
        })
        .collect();
    events.reverse(); // ring is oldest-first
    events.truncate(30);

    Some(CityModel {
        r: r.clone(),
        desired,
        ready,
        available,
        updated,
        status,
        note,
        strategy,
        age,
        pods,
        owned,
        events,
    })
}

// ---------------------------------------------------------------------------
// Node detail model

#[derive(Debug, Clone)]
pub struct NodePodRow {
    pub namespace: String,
    pub name: String,
    pub state: PodState,
    pub reason: String,
    pub restarts: i32,
    pub age: Option<Time>,
    pub owner: Option<WorkloadRef>,
}

#[derive(Debug, Clone)]
pub struct NodeDetailModel {
    pub tile: NodeTile,
    /// Terrain attributes: runtime, kubelet, OS, arch, kernel, provider.
    pub info: Vec<(&'static str, String)>,
    pub conditions: Vec<(String, String)>,
    pub cpu_alloc: f64,
    pub mem_alloc: f64,
    pub pods: Vec<NodePodRow>,
}

pub fn build_node_detail(world: &ObservedWorld, name: &str) -> Option<NodeDetailModel> {
    let node = world
        .nodes
        .state()
        .into_iter()
        .find(|n| n.metadata.name.as_deref() == Some(name))?;

    let pods_arc = world.pods.state();
    let on_node: Vec<&Pod> = pods_arc
        .iter()
        .map(|p| p.as_ref())
        .filter(|p| p.spec.as_ref().and_then(|s| s.node_name.as_deref()) == Some(name))
        .collect();
    let idx = OwnerIndex::build(world);
    let tile = build_node_tile(&node, &on_node, &idx);

    let mut info = Vec::new();
    if let Some(ni) = node.status.as_ref().and_then(|s| s.node_info.as_ref()) {
        info.push(("runtime", ni.container_runtime_version.clone()));
        info.push(("kubelet", ni.kubelet_version.clone()));
        info.push(("os", ni.os_image.clone()));
        info.push(("arch", ni.architecture.clone()));
        info.push(("kernel", ni.kernel_version.clone()));
    }
    if let Some(pid) = node.spec.as_ref().and_then(|s| s.provider_id.as_ref()) {
        info.push(("provider", pid.clone()));
    }
    if let Some(addr) = node.status.as_ref().and_then(|s| s.addresses.as_ref())
        && let Some(ip) = addr.iter().find(|a| a.type_ == "InternalIP")
    {
        info.push(("internal-ip", ip.address.clone()));
    }

    let conditions = node
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|cs| {
            cs.iter()
                .map(|c| (c.type_.clone(), c.status.clone()))
                .collect()
        })
        .unwrap_or_default();

    let alloc = node.status.as_ref().and_then(|s| s.allocatable.as_ref());
    let cpu_alloc = alloc
        .and_then(|a| a.get("cpu"))
        .and_then(quantity::value)
        .unwrap_or(0.0);
    let mem_alloc = alloc
        .and_then(|a| a.get("memory"))
        .and_then(quantity::value)
        .unwrap_or(0.0);

    let mut pods: Vec<NodePodRow> = on_node
        .iter()
        .map(|p| {
            let (state, reason) = pod_state(p);
            NodePodRow {
                namespace: p.metadata.namespace.clone().unwrap_or_default(),
                name: p.metadata.name.clone().unwrap_or_default(),
                state,
                reason,
                restarts: pod_restarts(p),
                age: p.metadata.creation_timestamp.clone(),
                owner: idx.workload_of(p),
            }
        })
        .collect();
    pods.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));

    Some(NodeDetailModel {
        tile,
        info,
        conditions,
        cpu_alloc,
        mem_alloc,
        pods,
    })
}

// ---------------------------------------------------------------------------
// Aggregate

/// Everything the UI renders from, rebuilt wholesale on world change (at
/// tick cadence). Detail views additionally re-derive their own models.
#[derive(Default)]
pub struct Models {
    pub map: MapModel,
    pub workloads: Vec<WorkloadRow>,
    pub attention: Vec<Concern>,
    pub workload_severity: HashMap<WorkloadRef, Severity>,
    /// The explorable world projection of all of the above.
    pub world: WorldModel,
}

impl Models {
    pub fn build(world: &ObservedWorld) -> Self {
        let map = build_map(world);
        let workloads = build_workloads(world);
        let attention = attention::build(world, &map, &workloads);
        let mut workload_severity: HashMap<WorkloadRef, Severity> = HashMap::new();
        for c in &attention {
            if let Target::Workload(r) = &c.target {
                workload_severity
                    .entry(r.clone())
                    .and_modify(|s| *s = (*s).max(c.severity))
                    .or_insert(c.severity);
            }
        }
        let world_model = build_world(
            &map,
            &workloads,
            &workload_severity,
            &world.custom_entries(),
        );
        Models {
            map,
            workloads,
            attention,
            workload_severity,
            world: world_model,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use k8s_openapi::jiff;

    #[test]
    fn map_zone_columns_sorted_unzoned_last() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n-bravo", Some("z-b")));
        s.node(fx::node("n-alpha", Some("z-a")));
        s.node(fx::node("n-loner", None));
        let m = build_map(&world);
        let names: Vec<&str> = m.zones.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(names, ["z-a", "z-b", UNZONED]);
        assert_eq!(m.total_nodes, 3);
    }

    #[test]
    fn map_layout_is_stable_under_insertion() {
        let (world, mut s) = fx::world();
        for n in ["n-a", "n-b", "n-c", "n-d"] {
            s.node(fx::node(n, Some("z-a")));
        }
        let before: Vec<String> = build_map(&world).zones[0]
            .nodes
            .iter()
            .map(|t| t.name.clone())
            .collect();
        // Two rebuilds agree (determinism)…
        let again: Vec<String> = build_map(&world).zones[0]
            .nodes
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert_eq!(before, again);
        // …and inserting a node never reorders the existing ones.
        s.node(fx::node("n-e", Some("z-a")));
        let after: Vec<String> = build_map(&world).zones[0]
            .nodes
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let filtered: Vec<String> = after.iter().filter(|n| *n != "n-e").cloned().collect();
        assert_eq!(before, filtered);
        assert_eq!(after.len(), 5);
    }

    #[test]
    fn request_pressure_from_requests_vs_allocatable() {
        // Node allocatable: 4 cores / 8Gi (fixture default).
        let n = fx::node("n1", Some("z-a"));
        let p1 = fx::pod_requests(fx::pod("d", "p1", Some("n1")), "1", "2Gi");
        let p2 = fx::pod_requests(fx::pod("d", "p2", Some("n1")), "1000m", "2Gi");
        // Succeeded pods do not count toward scheduling pressure.
        let done = fx::pod_phase(
            fx::pod_requests(fx::pod("d", "p3", Some("n1")), "4", "8Gi"),
            "Succeeded",
        );
        let tile = build_node_tile(&n, &[&p1, &p2, &done], &OwnerIndex::default());
        assert!(
            (tile.cpu_ratio - 0.5).abs() < 1e-9,
            "cpu {}",
            tile.cpu_ratio
        );
        assert!(
            (tile.mem_ratio - 0.5).abs() < 1e-9,
            "mem {}",
            tile.mem_ratio
        );
        assert_eq!(tile.pods.len(), 3); // glyphs still show all pods
        assert_eq!(tile.health, NodeHealth::Healthy);
    }

    #[test]
    fn node_health_precedence() {
        let not_ready = fx::node_with_condition(fx::node("n1", None), "Ready", "False");
        assert_eq!(
            build_node_tile(&not_ready, &[], &OwnerIndex::default()).health,
            NodeHealth::NotReady
        );

        // NotReady outranks cordon.
        let both = fx::cordoned(fx::node_with_condition(
            fx::node("n2", None),
            "Ready",
            "False",
        ));
        assert_eq!(
            build_node_tile(&both, &[], &OwnerIndex::default()).health,
            NodeHealth::NotReady
        );

        let cordoned = fx::cordoned(fx::node("n3", None));
        let t = build_node_tile(&cordoned, &[], &OwnerIndex::default());
        assert_eq!(t.health, NodeHealth::Cordoned);
        assert!(t.cordoned);

        let pressured = fx::node_with_condition(fx::node("n4", None), "MemoryPressure", "True");
        let t = build_node_tile(&pressured, &[], &OwnerIndex::default());
        assert_eq!(t.health, NodeHealth::Pressure);
        assert_eq!(t.abnormal, vec!["Mem"]);
    }

    #[test]
    fn pod_state_classification() {
        let crash = fx::pod_waiting(fx::pod("d", "p", Some("n")), "CrashLoopBackOff");
        assert_eq!(
            pod_state(&crash),
            (PodState::Failing, "CrashLoopBackOff".to_string())
        );

        let unsched = fx::pod_unschedulable(fx::pod("d", "p", None));
        assert_eq!(
            pod_state(&unsched),
            (PodState::Pending, "Unschedulable".to_string())
        );

        let mut terminating = fx::pod("d", "p", Some("n"));
        terminating.metadata.deletion_timestamp = Some(Time(jiff::Timestamp::now()));
        assert_eq!(pod_state(&terminating).0, PodState::Terminating);

        let ok = fx::pod("d", "p", Some("n"));
        assert_eq!(pod_state(&ok), (PodState::Ok, "Running".to_string()));
    }

    #[test]
    fn deployment_rollout_states() {
        let complete = fx::deployment("d", "web", 3, 3);
        assert_eq!(deployment_status(&complete).0, RolloutStatus::Complete);

        let mut updating = fx::deployment("d", "web", 3, 1);
        updating.status.as_mut().unwrap().updated_replicas = Some(1);
        let (st, note) = deployment_status(&updating);
        assert_eq!(st, RolloutStatus::Progressing);
        assert_eq!(note, "updating 1/3");

        let mut stalled = fx::deployment("d", "web", 3, 1);
        stalled.status.as_mut().unwrap().conditions =
            Some(vec![k8s_openapi::api::apps::v1::DeploymentCondition {
                type_: "Progressing".into(),
                status: "False".into(),
                reason: Some("ProgressDeadlineExceeded".into()),
                ..Default::default()
            }]);
        assert_eq!(deployment_status(&stalled).0, RolloutStatus::Stalled);
    }

    #[test]
    fn daemonset_rollout_states() {
        let complete = fx::daemonset("d", "agent", 3, 3);
        assert_eq!(daemonset_status(&complete).0, RolloutStatus::Complete);
        let lagging = fx::daemonset("d", "agent", 3, 1);
        let (st, note) = daemonset_status(&lagging);
        assert_eq!(st, RolloutStatus::Progressing);
        assert_eq!(note, "ready 1/3");
    }

    #[test]
    fn city_assembles_pods_services_and_ownership() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 2));
        s.replicaset(fx::replicaset("demo", "web-7d4b", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-1", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-2", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        // Decoy pod from another workload.
        s.pod(fx::pod("demo", "other", Some("n1")));
        s.service(fx::service("demo", "web", &[("app", "web")]));
        s.service(fx::service("demo", "unrelated", &[("app", "nope")]));

        let r = WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        };
        let city = build_city(&world, &r).expect("city");
        assert_eq!(city.desired, 2);
        assert_eq!(city.pods.len(), 2);
        assert!(city.pods.iter().all(|p| p.name.starts_with("web-7d4b-")));
        let svcs: Vec<&str> = city
            .owned
            .iter()
            .filter(|o| o.kind == "svc")
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(svcs, ["web"]);
        assert_eq!(city.status, RolloutStatus::Complete);
    }

    #[test]
    fn node_detail_resolves_pod_owners() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-7d4b", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-7d4b-1", Some("n1")),
            "ReplicaSet",
            "web-7d4b",
        ));
        let detail = build_node_detail(&world, "n1").expect("detail");
        assert_eq!(detail.pods.len(), 1);
        let owner = detail.pods[0].owner.as_ref().expect("owner");
        assert_eq!(owner.name, "web");
        assert_eq!(owner.kind, WorkloadKind::Deployment);
        assert!(detail.info.iter().any(|(k, _)| *k == "runtime"));
    }
}
