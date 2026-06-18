//! Hand-built cluster objects + store seeding for tests. No cluster needed:
//! we drive reflector writers with synthetic watcher events.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use k8s_openapi::api::apps::v1::{
    DaemonSet, DaemonSetStatus, Deployment, DeploymentSpec, DeploymentStatus, ReplicaSet,
    StatefulSet, StatefulSetSpec, StatefulSetStatus,
};
use k8s_openapi::api::batch::v1::{
    CronJob, CronJobSpec, CronJobStatus, Job, JobSpec, JobStatus, JobTemplateSpec,
};
use k8s_openapi::api::core::v1::{
    Container, ContainerState, ContainerStateWaiting, ContainerStatus, Node, NodeCondition,
    NodeSpec, NodeStatus, NodeSystemInfo, PersistentVolumeClaim, PersistentVolumeClaimStatus,
    PersistentVolumeClaimVolumeSource, Pod, PodCondition, PodSpec, PodStatus, PodTemplateSpec,
    ResourceRequirements, Service, ServiceSpec, Volume,
};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, ServiceBackendPort,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
use kube::runtime::reflector::store::Writer;
use kube::runtime::{reflector, watcher};

use super::model::ZONE_LABEL;
use super::observed::ObservedWorld;
use crate::k8s::client::{ClusterMeta, Platform};

/// Holds the store writers so the readers in `ObservedWorld` stay live.
pub struct Seeds {
    pub nodes: Writer<Node>,
    pub pods: Writer<Pod>,
    pub deployments: Writer<Deployment>,
    pub replicasets: Writer<ReplicaSet>,
    pub statefulsets: Writer<StatefulSet>,
    pub daemonsets: Writer<DaemonSet>,
    pub jobs: Writer<Job>,
    pub cronjobs: Writer<CronJob>,
    pub pvcs: Writer<PersistentVolumeClaim>,
    pub services: Writer<Service>,
    pub ingresses: Writer<Ingress>,
}

macro_rules! seed_fn {
    ($fn_name:ident, $field:ident, $ty:ty) => {
        pub fn $fn_name(&mut self, obj: $ty) {
            self.$field.apply_watcher_event(&watcher::Event::Apply(obj));
        }
    };
}

impl Seeds {
    seed_fn!(node, nodes, Node);
    seed_fn!(pod, pods, Pod);
    seed_fn!(deployment, deployments, Deployment);
    seed_fn!(replicaset, replicasets, ReplicaSet);
    seed_fn!(statefulset, statefulsets, StatefulSet);
    seed_fn!(daemonset, daemonsets, DaemonSet);
    seed_fn!(pvc, pvcs, PersistentVolumeClaim);
    seed_fn!(service, services, Service);
    seed_fn!(ingress, ingresses, Ingress);
    seed_fn!(job, jobs, Job);
    seed_fn!(cronjob, cronjobs, CronJob);
}

pub fn world() -> (ObservedWorld, Seeds) {
    let (nodes, nodes_w) = reflector::store();
    let (pods, pods_w) = reflector::store();
    let (deployments, deployments_w) = reflector::store();
    let (replicasets, replicasets_w) = reflector::store();
    let (statefulsets, statefulsets_w) = reflector::store();
    let (daemonsets, daemonsets_w) = reflector::store();
    let (jobs, jobs_w) = reflector::store();
    let (cronjobs, cronjobs_w) = reflector::store();
    let (pvcs, pvcs_w) = reflector::store();
    let (services, services_w) = reflector::store();
    let (ingresses, ingresses_w) = reflector::store();
    let world = ObservedWorld {
        meta: ClusterMeta {
            context: "test".into(),
            server: "https://test:6443".into(),
            platform: Platform::Kind,
            all_contexts: vec!["test".into()],
        },
        nodes,
        pods,
        deployments,
        replicasets,
        statefulsets,
        daemonsets,
        jobs,
        cronjobs,
        pvcs,
        services,
        ingresses,
        events: Arc::new(Mutex::new(VecDeque::new())),
        customs: Arc::new(Vec::new()),
        metrics: crate::k8s::metrics::store(),
    };
    let seeds = Seeds {
        nodes: nodes_w,
        pods: pods_w,
        deployments: deployments_w,
        replicasets: replicasets_w,
        statefulsets: statefulsets_w,
        daemonsets: daemonsets_w,
        jobs: jobs_w,
        cronjobs: cronjobs_w,
        pvcs: pvcs_w,
        services: services_w,
        ingresses: ingresses_w,
    };
    (world, seeds)
}

fn meta(ns: Option<&str>, name: &str) -> ObjectMeta {
    ObjectMeta {
        name: Some(name.into()),
        namespace: ns.map(Into::into),
        ..Default::default()
    }
}

pub fn quantities(pairs: &[(&str, &str)]) -> BTreeMap<String, Quantity> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Quantity(v.to_string())))
        .collect()
}

fn cond(type_: &str, status: &str) -> NodeCondition {
    NodeCondition {
        type_: type_.into(),
        status: status.into(),
        ..Default::default()
    }
}

pub fn node(name: &str, zone: Option<&str>) -> Node {
    let mut labels = BTreeMap::new();
    if let Some(z) = zone {
        labels.insert(ZONE_LABEL.to_string(), z.to_string());
    }
    Node {
        metadata: ObjectMeta {
            name: Some(name.into()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(NodeSpec {
            provider_id: Some(format!("kind://docker/test/{name}")),
            ..Default::default()
        }),
        status: Some(NodeStatus {
            conditions: Some(vec![cond("Ready", "True")]),
            allocatable: Some(quantities(&[("cpu", "4"), ("memory", "8Gi")])),
            node_info: Some(NodeSystemInfo {
                container_runtime_version: "containerd://2.0.0".into(),
                kubelet_version: "v1.33.0".into(),
                os_image: "Fixture Linux".into(),
                architecture: "arm64".into(),
                kernel_version: "6.6.0".into(),
                ..Default::default()
            }),
            ..Default::default()
        }),
    }
}

pub fn node_with_condition(mut n: Node, type_: &str, status: &str) -> Node {
    let conds = n
        .status
        .get_or_insert_default()
        .conditions
        .get_or_insert_default();
    conds.retain(|c| c.type_ != type_);
    conds.push(cond(type_, status));
    n
}

pub fn cordoned(mut n: Node) -> Node {
    n.spec.get_or_insert_default().unschedulable = Some(true);
    n
}

pub fn pod(ns: &str, name: &str, node: Option<&str>) -> Pod {
    Pod {
        metadata: meta(Some(ns), name),
        spec: Some(PodSpec {
            node_name: node.map(Into::into),
            containers: vec![Container {
                name: "main".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        status: Some(PodStatus {
            phase: Some("Running".into()),
            container_statuses: Some(vec![ContainerStatus {
                name: "main".into(),
                ready: true,
                ..Default::default()
            }]),
            ..Default::default()
        }),
    }
}

pub fn pod_phase(mut p: Pod, phase: &str) -> Pod {
    p.status.get_or_insert_default().phase = Some(phase.into());
    p
}

/// A pod being deleted (a deletion timestamp → `PodState::Terminating`).
pub fn pod_terminating(mut p: Pod) -> Pod {
    p.metadata.deletion_timestamp = Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
        k8s_openapi::jiff::Timestamp::UNIX_EPOCH,
    ));
    p
}

/// First container waits with `reason` and is not ready.
pub fn pod_waiting(mut p: Pod, reason: &str) -> Pod {
    let cs = p
        .status
        .get_or_insert_default()
        .container_statuses
        .get_or_insert_default();
    if cs.is_empty() {
        cs.push(ContainerStatus {
            name: "main".into(),
            ..Default::default()
        });
    }
    cs[0].ready = false;
    cs[0].state = Some(ContainerState {
        waiting: Some(ContainerStateWaiting {
            reason: Some(reason.into()),
            ..Default::default()
        }),
        ..Default::default()
    });
    p
}

pub fn pod_restarting(mut p: Pod, count: i32) -> Pod {
    let cs = p
        .status
        .get_or_insert_default()
        .container_statuses
        .get_or_insert_default();
    if let Some(c) = cs.first_mut() {
        c.restart_count = count;
    }
    p
}

pub fn pod_requests(mut p: Pod, cpu: &str, mem: &str) -> Pod {
    if let Some(c) = p.spec.as_mut().and_then(|s| s.containers.first_mut()) {
        c.resources = Some(ResourceRequirements {
            requests: Some(quantities(&[("cpu", cpu), ("memory", mem)])),
            ..Default::default()
        });
    }
    p
}

pub fn pod_owned(mut p: Pod, kind: &str, owner: &str) -> Pod {
    p.metadata.owner_references = Some(vec![OwnerReference {
        api_version: "apps/v1".into(),
        kind: kind.into(),
        name: owner.into(),
        controller: Some(true),
        uid: "fixture-uid".into(),
        ..Default::default()
    }]);
    p
}

pub fn pod_unschedulable(mut p: Pod) -> Pod {
    p = pod_phase(p, "Pending");
    let st = p.status.get_or_insert_default();
    st.container_statuses = None;
    st.conditions = Some(vec![PodCondition {
        type_: "PodScheduled".into(),
        status: "False".into(),
        reason: Some("Unschedulable".into()),
        ..Default::default()
    }]);
    p
}

fn template(app: &str) -> PodTemplateSpec {
    PodTemplateSpec {
        metadata: Some(ObjectMeta {
            labels: Some(BTreeMap::from([("app".to_string(), app.to_string())])),
            ..Default::default()
        }),
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "main".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
    }
}

pub fn deployment(ns: &str, name: &str, desired: i32, ready: i32) -> Deployment {
    Deployment {
        metadata: ObjectMeta {
            generation: Some(1),
            ..meta(Some(ns), name)
        },
        spec: Some(DeploymentSpec {
            replicas: Some(desired),
            selector: LabelSelector {
                match_labels: Some(BTreeMap::from([("app".to_string(), name.to_string())])),
                ..Default::default()
            },
            template: template(name),
            ..Default::default()
        }),
        status: Some(DeploymentStatus {
            observed_generation: Some(1),
            replicas: Some(desired),
            ready_replicas: Some(ready),
            available_replicas: Some(ready),
            updated_replicas: Some(desired),
            ..Default::default()
        }),
    }
}

pub fn replicaset(ns: &str, name: &str, deploy: &str) -> ReplicaSet {
    let mut rs = ReplicaSet {
        metadata: meta(Some(ns), name),
        ..Default::default()
    };
    rs.metadata.owner_references = Some(vec![OwnerReference {
        api_version: "apps/v1".into(),
        kind: "Deployment".into(),
        name: deploy.into(),
        controller: Some(true),
        uid: "fixture-uid".into(),
        ..Default::default()
    }]);
    rs
}

pub fn statefulset(ns: &str, name: &str, desired: i32, ready: i32) -> StatefulSet {
    StatefulSet {
        metadata: meta(Some(ns), name),
        spec: Some(StatefulSetSpec {
            replicas: Some(desired),
            template: template(name),
            ..Default::default()
        }),
        status: Some(StatefulSetStatus {
            replicas: desired,
            ready_replicas: Some(ready),
            available_replicas: Some(ready),
            updated_replicas: Some(desired),
            ..Default::default()
        }),
    }
}

pub fn daemonset(ns: &str, name: &str, desired: i32, ready: i32) -> DaemonSet {
    DaemonSet {
        metadata: meta(Some(ns), name),
        status: Some(DaemonSetStatus {
            desired_number_scheduled: desired,
            number_ready: ready,
            updated_number_scheduled: Some(desired),
            number_available: Some(ready),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Attach a PVC volume to a pod, so storage resolution sees the claim.
pub fn pod_with_pvc(mut pod: Pod, claim: &str) -> Pod {
    let vol = Volume {
        name: format!("vol-{claim}"),
        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
            claim_name: claim.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    pod.spec
        .get_or_insert_with(Default::default)
        .volumes
        .get_or_insert_with(Vec::new)
        .push(vol);
    pod
}

pub fn pvc(ns: &str, name: &str, phase: &str) -> PersistentVolumeClaim {
    PersistentVolumeClaim {
        metadata: meta(Some(ns), name),
        status: Some(PersistentVolumeClaimStatus {
            phase: Some(phase.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn service(ns: &str, name: &str, selector: &[(&str, &str)]) -> Service {
    Service {
        metadata: meta(Some(ns), name),
        spec: Some(ServiceSpec {
            selector: Some(
                selector
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            type_: Some("ClusterIP".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn job(
    ns: &str,
    name: &str,
    completions: i32,
    succeeded: i32,
    active: i32,
    failed: i32,
) -> Job {
    Job {
        metadata: meta(Some(ns), name),
        spec: Some(JobSpec {
            completions: Some(completions),
            template: template(name),
            ..Default::default()
        }),
        status: Some(JobStatus {
            succeeded: Some(succeeded),
            active: Some(active),
            failed: Some(failed),
            ..Default::default()
        }),
    }
}

pub fn cronjob(ns: &str, name: &str, schedule: &str, suspend: bool) -> CronJob {
    CronJob {
        metadata: meta(Some(ns), name),
        spec: Some(CronJobSpec {
            schedule: schedule.into(),
            suspend: Some(suspend),
            job_template: JobTemplateSpec {
                spec: Some(JobSpec {
                    template: template(name),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        }),
        status: Some(CronJobStatus::default()),
    }
}

/// An Ingress with a single host→service rule (the common shape).
pub fn ingress(ns: &str, name: &str, host: &str, service: &str) -> Ingress {
    Ingress {
        metadata: meta(Some(ns), name),
        spec: Some(IngressSpec {
            rules: Some(vec![IngressRule {
                host: Some(host.into()),
                http: Some(HTTPIngressRuleValue {
                    paths: vec![HTTPIngressPath {
                        path: Some("/".into()),
                        path_type: "Prefix".into(),
                        backend: IngressBackend {
                            service: Some(IngressServiceBackend {
                                name: service.into(),
                                port: Some(ServiceBackendPort {
                                    number: Some(80),
                                    ..Default::default()
                                }),
                            }),
                            ..Default::default()
                        },
                    }],
                }),
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}
