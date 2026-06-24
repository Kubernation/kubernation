//! Rollout history + revision diff — "which change broke it?"
//!
//! A pure resolver over the watched **ReplicaSet** store: a Deployment's
//! revisions (newest first; the newest is *current*, the one before it the
//! *last-known-good*), each with the container images it ran, plus the image
//! **delta** between two revisions. This is the read half; `state/planned.rs`'s
//! Rollback intervention uses [`revision_template`] to stage a roll-back.
//!
//! StatefulSets and DaemonSets track their revisions in `ControllerRevision`
//! objects, which KuberNation deliberately doesn't watch — so history here is
//! **Deployment-only** (an empty list for the others, surfaced honestly).

use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::PodTemplateSpec;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

use super::model::{WorkloadKind, WorkloadRef, controller_owner};
use super::observed::ObservedWorld;

/// The annotation the Deployment controller stamps on each ReplicaSet with its
/// monotonic revision number (what `kubectl rollout history` reads).
pub const REVISION_ANNOTATION: &str = "deployment.kubernetes.io/revision";

/// One rollout revision (one ReplicaSet of a Deployment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// The revision number (`deployment.kubernetes.io/revision`), 0 if unset.
    pub number: i64,
    pub rs_name: String,
    /// Desired replicas on this RS (0 for a wound-down old revision).
    pub replicas: i32,
    /// Ready replicas reported on this RS.
    pub ready: i32,
    pub created: Option<Time>,
    /// `(container, image)` pairs from the RS pod template, sorted by container.
    pub images: Vec<(String, String)>,
    /// True for the highest-numbered revision (what's running now).
    pub current: bool,
}

/// An image change for one container going from one revision to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageDelta {
    pub container: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// All revisions of a Deployment, **newest first**. Empty for non-Deployments
/// (no RS-tracked history) or a Deployment with no ReplicaSets observed yet.
pub fn revisions(world: &ObservedWorld, wr: &WorkloadRef) -> Vec<Revision> {
    if wr.kind != WorkloadKind::Deployment {
        return Vec::new();
    }
    let mut revs: Vec<Revision> = world
        .replicasets
        .state()
        .iter()
        .filter(|rs| owns(rs.as_ref(), wr))
        .map(|rs| rs_revision(rs.as_ref()))
        .collect();
    // Newest first; ties (e.g. a missing annotation) fall back to name so the
    // order is stable across rebuilds.
    revs.sort_by(|a, b| b.number.cmp(&a.number).then(a.rs_name.cmp(&b.rs_name)));
    if let Some(max) = revs.iter().map(|r| r.number).max() {
        for r in &mut revs {
            r.current = r.number == max;
        }
    }
    revs
}

/// The previous (last-known-good) revision — the highest-numbered one that
/// isn't current. `None` when there's only one revision (nothing to roll back
/// to). Expects the `revisions()` ordering (newest first).
pub fn previous(revs: &[Revision]) -> Option<&Revision> {
    revs.iter().find(|r| !r.current)
}

/// The pod template of a specific revision, for staging a roll-back. `None` if
/// no such revision (or it carries no template).
pub fn revision_template(
    world: &ObservedWorld,
    wr: &WorkloadRef,
    number: i64,
) -> Option<PodTemplateSpec> {
    if wr.kind != WorkloadKind::Deployment {
        return None;
    }
    world
        .replicasets
        .state()
        .iter()
        .filter(|rs| owns(rs.as_ref(), wr))
        .find(|rs| rs_number(rs.as_ref()) == number)
        .and_then(|rs| rs.spec.as_ref())
        .and_then(|s| s.template.clone())
}

/// The image changes going `from`→`to`, per container (union of both sides),
/// dropping unchanged containers. The "what changed" between two revisions.
pub fn image_changes(from: &Revision, to: &Revision) -> Vec<ImageDelta> {
    use std::collections::BTreeSet;
    let names: BTreeSet<&String> = from
        .images
        .iter()
        .map(|(n, _)| n)
        .chain(to.images.iter().map(|(n, _)| n))
        .collect();
    let look = |imgs: &[(String, String)], name: &str| {
        imgs.iter().find(|(n, _)| n == name).map(|(_, i)| i.clone())
    };
    names
        .into_iter()
        .filter_map(|name| {
            let f = look(&from.images, name);
            let t = look(&to.images, name);
            (f != t).then(|| ImageDelta {
                container: name.clone(),
                from: f,
                to: t,
            })
        })
        .collect()
}

fn owns(rs: &ReplicaSet, wr: &WorkloadRef) -> bool {
    rs.metadata.namespace.as_deref() == Some(wr.namespace.as_str())
        && controller_owner(rs.metadata.owner_references.as_deref())
            == Some(("Deployment", wr.name.as_str()))
}

fn rs_number(rs: &ReplicaSet) -> i64 {
    rs.metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get(REVISION_ANNOTATION))
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
}

fn rs_revision(rs: &ReplicaSet) -> Revision {
    let mut images: Vec<(String, String)> = rs
        .spec
        .as_ref()
        .and_then(|s| s.template.as_ref())
        .and_then(|t| t.spec.as_ref())
        .map(|ps| {
            ps.containers
                .iter()
                .map(|c| (c.name.clone(), c.image.clone().unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();
    images.sort();
    Revision {
        number: rs_number(rs),
        rs_name: rs.metadata.name.clone().unwrap_or_default(),
        replicas: rs.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
        ready: rs
            .status
            .as_ref()
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0),
        created: rs.metadata.creation_timestamp.clone(),
        images,
        current: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use k8s_openapi::api::apps::v1::{ReplicaSetSpec, ReplicaSetStatus};
    use k8s_openapi::api::core::v1::{Container, PodSpec};
    use std::collections::BTreeMap;

    fn rs(ns: &str, name: &str, deploy: &str, rev: &str, image: &str, replicas: i32) -> ReplicaSet {
        let mut r = fx::replicaset(ns, name, deploy);
        r.metadata.annotations = Some(BTreeMap::from([(
            REVISION_ANNOTATION.to_string(),
            rev.to_string(),
        )]));
        r.spec = Some(ReplicaSetSpec {
            replicas: Some(replicas),
            template: Some(PodTemplateSpec {
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "main".into(),
                        image: Some(image.into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        r.status = Some(ReplicaSetStatus {
            ready_replicas: Some(replicas),
            ..Default::default()
        });
        r
    }

    fn wr(ns: &str, name: &str) -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: ns.into(),
            name: name.into(),
        }
    }

    #[test]
    fn revisions_are_newest_first_with_current_marked() {
        let (world, mut seeds) = fx::world();
        // Two revisions of web: rev 1 (wound down) and rev 2 (current), plus an
        // unrelated RS that must not leak in.
        seeds.replicaset(rs("demo", "web-old", "web", "1", "web:1.0", 0));
        seeds.replicaset(rs("demo", "web-new", "web", "2", "web:1.1", 3));
        seeds.replicaset(rs("demo", "other-x", "other", "1", "x:1", 1));

        let revs = revisions(&world, &wr("demo", "web"));
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].number, 2);
        assert!(revs[0].current);
        assert!(!revs[1].current);
        assert_eq!(previous(&revs).unwrap().number, 1);
    }

    #[test]
    fn image_delta_between_revisions() {
        let (world, mut seeds) = fx::world();
        seeds.replicaset(rs("demo", "web-old", "web", "1", "web:1.0", 0));
        seeds.replicaset(rs("demo", "web-new", "web", "2", "web:1.1", 3));
        let revs = revisions(&world, &wr("demo", "web"));
        let prev = previous(&revs).unwrap();
        let changes = image_changes(prev, &revs[0]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].container, "main");
        assert_eq!(changes[0].from.as_deref(), Some("web:1.0"));
        assert_eq!(changes[0].to.as_deref(), Some("web:1.1"));
    }

    #[test]
    fn revision_template_returns_the_target_revisions_pods() {
        let (world, mut seeds) = fx::world();
        seeds.replicaset(rs("demo", "web-old", "web", "1", "web:1.0", 0));
        seeds.replicaset(rs("demo", "web-new", "web", "2", "web:1.1", 3));
        let tmpl = revision_template(&world, &wr("demo", "web"), 1).unwrap();
        let img = tmpl.spec.unwrap().containers[0].image.clone();
        assert_eq!(img.as_deref(), Some("web:1.0"));
    }

    #[test]
    fn non_deployments_have_no_history() {
        let (world, _seeds) = fx::world();
        let sts = WorkloadRef {
            kind: WorkloadKind::StatefulSet,
            namespace: "demo".into(),
            name: "db".into(),
        };
        assert!(revisions(&world, &sts).is_empty());
    }
}
