//! The staged-intervention world — the planning turn.
//!
//! Operator changes are *staged* here as intents against
//! [`super::observed::ObservedWorld`], reviewed as a diff, and (in a future
//! slice) committed as a deliberate "end of turn". This is the design doc's
//! planning-turn model: intervention as deliberate staged changes rather
//! than imperative edits.
//!
//! **Preview-only:** staging never touches the cluster. `PlannedWorld` holds
//! intents and [`plan_diff`] derives a pure from→to diff against the observed
//! world; applying those intents is a separate, explicitly-gated step that
//! does not exist yet — so the codebase keeps its "no mutation paths"
//! guarantee while the planning *experience* is built.

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
        }
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
        })
        .collect()
}

fn cordon_word(on: bool) -> String {
    if on { "cordoned" } else { "schedulable" }.into()
}

/// The observed desired replicas of a scalable workload (DaemonSets aren't).
fn current_replicas(world: &ObservedWorld, r: &WorkloadRef) -> Option<i32> {
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
