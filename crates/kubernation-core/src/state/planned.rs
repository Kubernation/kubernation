//! The staged-intervention world — the planning turn.
//!
//! Operator changes are *staged* here as intents against
//! [`super::observed::ObservedWorld`], reviewed as a diff, and committed as a
//! deliberate "end of turn". This is the design doc's planning-turn model:
//! intervention as deliberate staged changes rather than imperative edits.
//!
//! This module is **pure**: staging and [`plan_diff`] never touch the cluster.
//! Committing — applying the staged intents — is a separate, explicitly-gated
//! step in the GUI: `k8s::actions::apply_intervention` patches the cluster
//! behind a confirm, after a server-side dry-run validates every change (which
//! also enforces RBAC). So the diff stays a pure function of the observed
//! world; only Commit writes.

use super::model::{WorkloadKind, WorkloadRef};
use super::observed::ObservedWorld;

/// One staged change the operator intends. Latest-wins per target inside a
/// [`PlannedWorld`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intervention {
    /// Set a workload's desired replicas (Deployment / StatefulSet).
    Scale {
        workload: WorkloadRef,
        replicas: i32,
    },
    /// Cordon (`on = true`) or uncordon a node.
    Cordon { node: String, on: bool },
    /// Rolling-restart a workload (Deployment / StatefulSet / DaemonSet) — a
    /// one-shot trigger, not a state, so it has no from→to and never no-ops.
    Restart { workload: WorkloadRef },
    /// Set a container's image on a workload (Deployment / StatefulSet /
    /// DaemonSet). Keyed by (workload, container) so multi-container pods can
    /// each be set; rolls the workload like `kubectl set image`.
    SetImage {
        workload: WorkloadRef,
        container: String,
        image: String,
    },
}

/// The staged plan: a set of interventions, at most one per target.
#[derive(Debug, Default, Clone)]
pub struct PlannedWorld {
    interventions: Vec<Intervention>,
}

impl PlannedWorld {
    /// Stage (or replace) a scale intent for a workload. Replicas floor at 0.
    pub fn stage_scale(&mut self, workload: WorkloadRef, replicas: i32) {
        self.interventions
            .retain(|i| !matches!(i, Intervention::Scale { workload: w, .. } if *w == workload));
        self.interventions.push(Intervention::Scale {
            workload,
            replicas: replicas.max(0),
        });
    }

    /// Stage an intervention by value (frontends build the intent, the model
    /// routes it to the right latest-wins slot).
    pub fn stage(&mut self, iv: Intervention) {
        match iv {
            Intervention::Scale { workload, replicas } => self.stage_scale(workload, replicas),
            Intervention::Cordon { node, on } => self.stage_cordon(node, on),
            Intervention::Restart { workload } => self.stage_restart(workload),
            Intervention::SetImage {
                workload,
                container,
                image,
            } => self.stage_set_image(workload, container, image),
        }
    }

    /// Stage (or replace) a container-image change for a workload, latest-wins
    /// per (workload, container).
    pub fn stage_set_image(&mut self, workload: WorkloadRef, container: String, image: String) {
        self.interventions.retain(|i| {
            !matches!(i, Intervention::SetImage { workload: w, container: c, .. }
                if *w == workload && *c == container)
        });
        self.interventions.push(Intervention::SetImage {
            workload,
            container,
            image,
        });
    }

    /// The staged image for a workload's container, if any.
    pub fn image_set(&self, workload: &WorkloadRef, container: &str) -> Option<&str> {
        self.interventions.iter().find_map(|i| match i {
            Intervention::SetImage {
                workload: w,
                container: c,
                image,
            } if w == workload && c == container => Some(image.as_str()),
            _ => None,
        })
    }

    /// Stage (or replace) a rolling-restart intent for a workload. A workload
    /// can carry a restart *and* a scale at once — they're different fields.
    pub fn stage_restart(&mut self, workload: WorkloadRef) {
        self.interventions
            .retain(|i| !matches!(i, Intervention::Restart { workload: w } if *w == workload));
        self.interventions.push(Intervention::Restart { workload });
    }

    /// Drop a staged restart for a workload (the city's restart toggle).
    pub fn unstage_restart(&mut self, workload: &WorkloadRef) {
        self.interventions
            .retain(|i| !matches!(i, Intervention::Restart { workload: w } if w == workload));
    }

    /// Is a rolling restart staged for this workload?
    pub fn restarting(&self, workload: &WorkloadRef) -> bool {
        self.interventions
            .iter()
            .any(|i| matches!(i, Intervention::Restart { workload: w } if w == workload))
    }

    /// Stage (or replace) a cordon intent for a node.
    pub fn stage_cordon(&mut self, node: String, on: bool) {
        self.interventions
            .retain(|i| !matches!(i, Intervention::Cordon { node: n, .. } if *n == node));
        self.interventions.push(Intervention::Cordon { node, on });
    }

    /// Remove the intervention at `idx` (review-screen unstage).
    pub fn unstage(&mut self, idx: usize) {
        if idx < self.interventions.len() {
            self.interventions.remove(idx);
        }
    }

    /// Drop every staged intent ("discard the turn").
    pub fn clear(&mut self) {
        self.interventions.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.interventions.is_empty()
    }
    pub fn len(&self) -> usize {
        self.interventions.len()
    }
    pub fn interventions(&self) -> &[Intervention] {
        &self.interventions
    }

    /// Staged replicas for a workload, if any.
    pub fn scaled(&self, workload: &WorkloadRef) -> Option<i32> {
        self.interventions.iter().find_map(|i| match i {
            Intervention::Scale {
                workload: w,
                replicas,
            } if w == workload => Some(*replicas),
            _ => None,
        })
    }

    /// Staged cordon state for a node, if any.
    pub fn cordoned(&self, node: &str) -> Option<bool> {
        self.interventions.iter().find_map(|i| match i {
            Intervention::Cordon { node: n, on } if n == node => Some(*on),
            _ => None,
        })
    }
}

/// One reviewable change: where, what field, and from→to. `noop` is set when
/// the staged value already matches the observed one (nothing would change).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanChange {
    pub target: String,
    pub field: &'static str,
    pub from: String,
    pub to: String,
    pub noop: bool,
}

/// Pure diff of a plan against the observed world — the End-of-Turn review.
pub fn plan_diff(observed: &ObservedWorld, planned: &PlannedWorld) -> Vec<PlanChange> {
    planned
        .interventions()
        .iter()
        .map(|iv| match iv {
            Intervention::Scale { workload, replicas } => {
                let current = current_replicas(observed, workload);
                PlanChange {
                    target: workload.to_string(),
                    field: "replicas",
                    from: current.map_or_else(|| "?".into(), |c| c.to_string()),
                    to: replicas.to_string(),
                    noop: current == Some(*replicas),
                }
            }
            Intervention::Cordon { node, on } => {
                let current = current_cordon(observed, node);
                PlanChange {
                    target: format!("node {node}"),
                    field: "cordon",
                    from: current.map_or_else(|| "?".into(), cordon_word),
                    to: cordon_word(*on),
                    noop: current == Some(*on),
                }
            }
            Intervention::Restart { workload } => PlanChange {
                target: workload.to_string(),
                field: "restart",
                from: "running".into(),
                to: "rolling restart".into(),
                noop: false,
            },
            Intervention::SetImage {
                workload,
                container,
                image,
            } => {
                let current = current_image(observed, workload, container);
                PlanChange {
                    target: format!("{workload} [{container}]"),
                    field: "image",
                    from: current.clone().unwrap_or_else(|| "?".into()),
                    to: image.clone(),
                    noop: current.as_deref() == Some(image.as_str()),
                }
            }
        })
        .collect()
}

/// The observed image of a named container in a workload's pod template.
pub(crate) fn current_image(
    world: &ObservedWorld,
    r: &WorkloadRef,
    container: &str,
) -> Option<String> {
    let ns = r.namespace.as_str();
    let name = r.name.as_str();
    let from_template = |tmpl: Option<&k8s_openapi::api::core::v1::PodTemplateSpec>| {
        tmpl.and_then(|t| t.spec.as_ref())
            .and_then(|s| s.containers.iter().find(|c| c.name == container))
            .and_then(|c| c.image.clone())
    };
    match r.kind {
        WorkloadKind::Deployment => world.deployments.state().into_iter().find_map(|d| {
            (d.metadata.namespace.as_deref() == Some(ns)
                && d.metadata.name.as_deref() == Some(name))
            .then(|| from_template(d.spec.as_ref().map(|s| &s.template)))
            .flatten()
        }),
        WorkloadKind::StatefulSet => world.statefulsets.state().into_iter().find_map(|s| {
            (s.metadata.namespace.as_deref() == Some(ns)
                && s.metadata.name.as_deref() == Some(name))
            .then(|| from_template(s.spec.as_ref().map(|sp| &sp.template)))
            .flatten()
        }),
        WorkloadKind::DaemonSet => world.daemonsets.state().into_iter().find_map(|ds| {
            (ds.metadata.namespace.as_deref() == Some(ns)
                && ds.metadata.name.as_deref() == Some(name))
            .then(|| from_template(ds.spec.as_ref().map(|sp| &sp.template)))
            .flatten()
        }),
    }
}

fn cordon_word(on: bool) -> String {
    if on { "cordoned" } else { "schedulable" }.into()
}

/// The observed desired replicas of a scalable workload (DaemonSets aren't).
pub(crate) fn current_replicas(world: &ObservedWorld, r: &WorkloadRef) -> Option<i32> {
    let ns = r.namespace.as_str();
    let name = r.name.as_str();
    match r.kind {
        WorkloadKind::Deployment => world
            .deployments
            .state()
            .into_iter()
            .find(|d| {
                d.metadata.namespace.as_deref() == Some(ns)
                    && d.metadata.name.as_deref() == Some(name)
            })
            .map(|d| d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1)),
        WorkloadKind::StatefulSet => world
            .statefulsets
            .state()
            .into_iter()
            .find(|s| {
                s.metadata.namespace.as_deref() == Some(ns)
                    && s.metadata.name.as_deref() == Some(name)
            })
            .map(|s| s.spec.as_ref().and_then(|sp| sp.replicas).unwrap_or(1)),
        WorkloadKind::DaemonSet => None,
    }
}

/// Whether a node is currently cordoned (`spec.unschedulable`).
fn current_cordon(world: &ObservedWorld, node: &str) -> Option<bool> {
    world
        .nodes
        .state()
        .into_iter()
        .find(|n| n.metadata.name.as_deref() == Some(node))
        .map(|n| {
            n.spec
                .as_ref()
                .and_then(|s| s.unschedulable)
                .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    fn wref(name: &str) -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: name.into(),
        }
    }

    #[test]
    fn staging_is_latest_wins_per_target() {
        let mut p = PlannedWorld::default();
        p.stage_scale(wref("web"), 5);
        p.stage_scale(wref("web"), 7); // replaces
        p.stage_scale(wref("api"), 2);
        assert_eq!(p.len(), 2);
        assert_eq!(p.scaled(&wref("web")), Some(7));
        assert_eq!(p.scaled(&wref("api")), Some(2));
        // Replicas floor at 0.
        p.stage_scale(wref("web"), -3);
        assert_eq!(p.scaled(&wref("web")), Some(0));
    }

    #[test]
    fn set_image_diffs_from_current_and_is_latest_wins() {
        let (world, mut s) = fx::world();
        let mut d = fx::deployment("demo", "web", 1, 1);
        d.spec
            .as_mut()
            .unwrap()
            .template
            .spec
            .as_mut()
            .unwrap()
            .containers[0]
            .image = Some("nginx:1.25".into());
        s.deployment(d);

        let mut p = PlannedWorld::default();
        p.stage_set_image(wref("web"), "main".into(), "nginx:1.26".into());
        assert_eq!(p.image_set(&wref("web"), "main"), Some("nginx:1.26"));
        // Latest-wins per (workload, container).
        p.stage_set_image(wref("web"), "main".into(), "nginx:1.27".into());
        assert_eq!(p.len(), 1);
        assert_eq!(p.image_set(&wref("web"), "main"), Some("nginx:1.27"));

        let diff = plan_diff(&world, &p);
        let img = diff.iter().find(|c| c.field == "image").unwrap();
        assert_eq!(
            (img.from.as_str(), img.to.as_str()),
            ("nginx:1.25", "nginx:1.27")
        );
        assert!(!img.noop);
        assert!(img.target.contains("[main]"), "{}", img.target);

        // Staging the current image is a no-op change.
        p.stage_set_image(wref("web"), "main".into(), "nginx:1.25".into());
        assert!(
            plan_diff(&world, &p)
                .iter()
                .find(|c| c.field == "image")
                .unwrap()
                .noop
        );
    }

    #[test]
    fn unstage_and_clear() {
        let mut p = PlannedWorld::default();
        p.stage_scale(wref("web"), 5);
        p.stage_cordon("n1".into(), true);
        p.unstage(0);
        assert_eq!(p.len(), 1);
        assert_eq!(p.cordoned("n1"), Some(true));
        p.clear();
        assert!(p.is_empty());
    }

    #[test]
    fn restart_coexists_with_scale_and_diffs() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 2, 2));
        let mut p = PlannedWorld::default();
        p.stage_scale(wref("web"), 4);
        p.stage_restart(wref("web")); // a workload can have both
        assert_eq!(p.len(), 2);
        assert!(p.restarting(&wref("web")));
        assert_eq!(p.scaled(&wref("web")), Some(4));

        let diff = plan_diff(&world, &p);
        let restart = diff.iter().find(|c| c.field == "restart").unwrap();
        assert_eq!(restart.to.as_str(), "rolling restart");
        assert!(!restart.noop); // a restart is never a no-op

        p.unstage_restart(&wref("web"));
        assert!(!p.restarting(&wref("web")));
        assert_eq!(p.len(), 1); // scale survives
    }

    #[test]
    fn diff_reports_from_to_and_noop() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 2));

        let mut p = PlannedWorld::default();
        p.stage_scale(wref("web"), 5);
        p.stage_cordon("n1".into(), true);
        let diff = plan_diff(&world, &p);

        let scale = diff.iter().find(|c| c.field == "replicas").unwrap();
        assert_eq!((scale.from.as_str(), scale.to.as_str()), ("2", "5"));
        assert!(!scale.noop);

        let cordon = diff.iter().find(|c| c.field == "cordon").unwrap();
        assert_eq!(
            (cordon.from.as_str(), cordon.to.as_str()),
            ("schedulable", "cordoned")
        );
        assert!(!cordon.noop);

        // Staging the current value is a no-op change.
        p.stage_scale(wref("web"), 2);
        let again = plan_diff(&world, &p);
        assert!(again.iter().find(|c| c.field == "replicas").unwrap().noop);
    }
}
