//! Chaos / game-day — resilience drills that inject a *real* failure with
//! standard Kubernetes resources and let you watch the cluster respond (the
//! blast radius spreads, the attention queue lights up, the treasury spends).
//! The 4X framing is a "raid" on a city; the k8s nouns stay greppable.
//!
//! **Pass 1 reuses the existing write primitives** — `evict_pod` (delete) and
//! `apply_intervention(Scale)` (patch `spec.replicas`) — so chaos adds **no new
//! verb and no new resource type**; the RBAC surface is exactly `delete pods` +
//! `patch scale`, already gated. Three reversible experiments: kill one pod,
//! kill all pods (the controller recreates), and an outage (scale to 0) with an
//! explicit restore. Node-failure (cordon+drain), NetworkPolicy partition, and
//! mesh fault-injection are deferred to later passes.
//!
//! This module is **pure** (no client / no I/O): it *plans* a drill against the
//! observed world — enumerating the concrete steps, capturing the restore
//! value, computing the blast size, and **refusing protected targets**
//! (control-plane / system namespaces) so a UI bug can't aim chaos at the
//! cluster's own plumbing. Execution lives in the one write file
//! (`k8s::actions::run_chaos`), sequencing these steps through the existing gate.

use k8s_openapi::api::core::v1::Node;

use crate::state::blast::{Subject, blast_radius};
use crate::state::model::{OwnerIndex, WorkloadKind, WorkloadRef};
use crate::state::observed::ObservedWorld;
use crate::state::planned::{Intervention, current_replicas};
use crate::state::slo::SloStatus;

/// Namespaces a drill must never target — the cluster's own control plane.
pub const PROTECTED_NS: &[&str] = &["kube-system", "kube-node-lease", "kube-public"];

/// Is this a protected (system) namespace?
pub fn ns_protected(ns: &str) -> bool {
    PROTECTED_NS.contains(&ns)
}

/// Is this a control-plane node (chaos must not cordon/drain it)? Used by the
/// node-failure experiment deferred to a later pass; kept here with the guards.
pub fn node_protected(node: &Node) -> bool {
    node.metadata.labels.as_ref().is_some_and(|l| {
        l.contains_key("node-role.kubernetes.io/control-plane")
            || l.contains_key("node-role.kubernetes.io/master")
    })
}

/// A chaos experiment the operator can run against a workload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Experiment {
    /// Delete one representative pod (the controller recreates it).
    KillOne { workload: WorkloadRef },
    /// Delete every pod of the workload (a full raid).
    KillAll { workload: WorkloadRef },
    /// Scale the workload to 0 (a real outage), restorable to its current count.
    Outage { workload: WorkloadRef },
}

impl Experiment {
    pub fn workload(&self) -> &WorkloadRef {
        match self {
            Experiment::KillOne { workload }
            | Experiment::KillAll { workload }
            | Experiment::Outage { workload } => workload,
        }
    }

    /// Short operator-facing label.
    pub fn label(&self) -> &'static str {
        match self {
            Experiment::KillOne { .. } => "kill one pod",
            Experiment::KillAll { .. } => "kill all pods",
            Experiment::Outage { .. } => "outage (scale to 0)",
        }
    }
}

/// One concrete cluster step — always an *existing* primitive, never a new verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChaosStep {
    /// Delete a pod (`actions::evict_pod`).
    Evict { namespace: String, pod: String },
    /// Apply a Scale intervention (`actions::apply_intervention`).
    Scale(Intervention),
}

/// A planned drill: the inject steps, the (optional) restore, why it was
/// refused (if so), and the blast size at plan time.
#[derive(Debug, Clone)]
pub struct ChaosPlan {
    pub steps: Vec<ChaosStep>,
    /// Interventions that undo the drill (Outage → scale back); empty for kills
    /// (the controller recreates pods on its own).
    pub restore: Vec<Intervention>,
    /// Set when a guard blocks the drill (protected target, DaemonSet, no pods).
    pub refused: Option<String>,
    /// `blast_radius(...).len()` for the target — the predicted reach.
    pub blast: usize,
}

impl ChaosPlan {
    pub fn is_refused(&self) -> bool {
        self.refused.is_some()
    }
    fn refuse(blast: usize, why: impl Into<String>) -> Self {
        ChaosPlan {
            steps: Vec::new(),
            restore: Vec::new(),
            refused: Some(why.into()),
            blast,
        }
    }
}

/// The pods owned by a workload, sorted (so "one pod" is deterministic).
fn workload_pods(world: &ObservedWorld, wr: &WorkloadRef) -> Vec<(String, String)> {
    let idx = OwnerIndex::build(world);
    let mut pods: Vec<(String, String)> = world
        .pods
        .state()
        .iter()
        .filter(|p| idx.workload_of(p).as_ref() == Some(wr))
        .filter_map(|p| Some((p.metadata.namespace.clone()?, p.metadata.name.clone()?)))
        .collect();
    pods.sort();
    pods
}

/// Resolve an experiment against the observed world into a concrete plan —
/// PURE (no client). Refuses protected targets fail-closed, captures the
/// restore value, and computes the blast size.
pub fn plan_chaos(world: &ObservedWorld, exp: &Experiment) -> ChaosPlan {
    let wr = exp.workload();
    let blast = blast_radius(world, &Subject::Workload(wr.clone())).len();

    if ns_protected(&wr.namespace) {
        return ChaosPlan::refuse(blast, format!("{} is a protected namespace", wr.namespace));
    }

    match exp {
        Experiment::KillOne { workload } => match workload_pods(world, workload).into_iter().next()
        {
            Some((namespace, pod)) => ChaosPlan {
                steps: vec![ChaosStep::Evict { namespace, pod }],
                restore: Vec::new(),
                refused: None,
                blast,
            },
            None => ChaosPlan::refuse(blast, "no pods to kill"),
        },
        Experiment::KillAll { workload } => {
            let pods = workload_pods(world, workload);
            if pods.is_empty() {
                return ChaosPlan::refuse(blast, "no pods to kill");
            }
            ChaosPlan {
                steps: pods
                    .into_iter()
                    .map(|(namespace, pod)| ChaosStep::Evict { namespace, pod })
                    .collect(),
                restore: Vec::new(),
                refused: None,
                blast,
            }
        }
        Experiment::Outage { workload } => {
            if workload.kind == WorkloadKind::DaemonSet {
                return ChaosPlan::refuse(blast, "DaemonSets scale with node count, not replicas");
            }
            match current_replicas(world, workload) {
                Some(n) if n > 0 => ChaosPlan {
                    steps: vec![ChaosStep::Scale(Intervention::Scale {
                        workload: workload.clone(),
                        replicas: 0,
                    })],
                    restore: vec![Intervention::Scale {
                        workload: workload.clone(),
                        replicas: n,
                    }],
                    refused: None,
                    blast,
                },
                Some(_) => ChaosPlan::refuse(blast, "already scaled to 0"),
                None => ChaosPlan::refuse(blast, "replicas unknown"),
            }
        }
    }
}

/// A game-day scorecard: what the drill did and how the cluster responded.
#[derive(Debug, Clone)]
pub struct ChaosScorecard {
    pub experiment: String,
    pub target: String,
    pub blast: usize,
    pub budget_before: Option<SloStatus>,
    pub budget_after: Option<SloStatus>,
    /// The target was actually observed down (`ready == 0`) after the drill —
    /// distinguishes a real outage from a kill the workload shrugged off.
    pub dipped: bool,
    /// The target's pods came back (`ready >= 1`) after dipping.
    pub recovered: bool,
    /// Seconds from run to recovery (None if not yet recovered).
    pub recover_secs: Option<f64>,
}

/// A colour role for a scorecard line — the GUI maps it to a theme colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreRole {
    Good,
    Warn,
    Bad,
    Info,
}

/// The scorecard rendered as text lines + roles — pure draw-decision logic,
/// testable without a GL context.
pub fn scorecard_lines(s: &ChaosScorecard) -> Vec<(String, ScoreRole)> {
    let mut out = vec![
        (format!("{} on {}", s.experiment, s.target), ScoreRole::Info),
        (
            format!("blast radius: {} affected", s.blast),
            ScoreRole::Info,
        ),
    ];
    // Recovery — only meaningful once the target actually went down. A kill the
    // workload shrugged off (other replicas stayed up) reads as "stayed up", not
    // a phantom "self-healed in 0s" before any outage even registered.
    if !s.dipped {
        out.push(("stayed up — no outage".into(), ScoreRole::Good));
    } else {
        match (s.recovered, s.recover_secs) {
            (true, Some(secs)) => out.push((format!("self-healed in {secs:.0}s"), ScoreRole::Good)),
            (true, None) => out.push(("self-healed".into(), ScoreRole::Good)),
            (false, _) => out.push(("not recovered yet…".into(), ScoreRole::Warn)),
        }
    }
    // Budget spent.
    if let (Some(before), Some(after)) = (&s.budget_before, &s.budget_after) {
        let spent = (before.budget_remaining - after.budget_remaining).max(0.0);
        let role = if spent > 0.001 {
            ScoreRole::Warn
        } else {
            ScoreRole::Good
        };
        out.push((
            format!(
                "budget {:.0}% -> {:.0}% (spent {:.0}%)",
                before.budget_remaining * 100.0,
                after.budget_remaining * 100.0,
                spent * 100.0
            ),
            role,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    fn web() -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        }
    }

    fn seed_web(s: &mut fx::Seeds) {
        s.deployment(fx::deployment("demo", "web", 3, 3));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        for i in 0..3 {
            s.pod(fx::pod_owned(
                fx::pod("demo", &format!("web-rs-{i}"), Some("n1")),
                "ReplicaSet",
                "web-rs",
            ));
        }
    }

    #[test]
    fn outage_captures_restore_replicas() {
        let (world, mut s) = fx::world();
        seed_web(&mut s);
        let plan = plan_chaos(&world, &Experiment::Outage { workload: web() });
        assert!(!plan.is_refused());
        assert_eq!(plan.steps.len(), 1);
        assert!(matches!(
            &plan.steps[0],
            ChaosStep::Scale(Intervention::Scale { replicas: 0, .. })
        ));
        // Restore scales back to the captured count (3).
        assert!(matches!(
            &plan.restore[0],
            Intervention::Scale { replicas: 3, .. }
        ));
    }

    #[test]
    fn kill_all_enumerates_every_pod() {
        let (world, mut s) = fx::world();
        seed_web(&mut s);
        let plan = plan_chaos(&world, &Experiment::KillAll { workload: web() });
        assert_eq!(plan.steps.len(), 3);
        assert!(plan.restore.is_empty()); // controller recreates
        // Kill one picks exactly one (the first, deterministically).
        let one = plan_chaos(&world, &Experiment::KillOne { workload: web() });
        assert_eq!(one.steps.len(), 1);
        assert!(matches!(&one.steps[0], ChaosStep::Evict { pod, .. } if pod == "web-rs-0"));
    }

    #[test]
    fn protected_namespace_is_refused() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("kube-system", "coredns", 2, 2));
        let plan = plan_chaos(
            &world,
            &Experiment::Outage {
                workload: WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: "kube-system".into(),
                    name: "coredns".into(),
                },
            },
        );
        assert!(plan.is_refused());
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn daemonset_outage_is_refused() {
        let (world, mut s) = fx::world();
        s.daemonset(fx::daemonset("demo", "agent", 3, 3));
        let plan = plan_chaos(
            &world,
            &Experiment::Outage {
                workload: WorkloadRef {
                    kind: WorkloadKind::DaemonSet,
                    namespace: "demo".into(),
                    name: "agent".into(),
                },
            },
        );
        assert!(plan.is_refused());
    }

    #[test]
    fn no_pods_kill_is_refused() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 0, 0)); // no pods
        let plan = plan_chaos(&world, &Experiment::KillAll { workload: web() });
        assert!(plan.is_refused());
    }

    #[test]
    fn control_plane_node_is_protected() {
        use k8s_openapi::api::core::v1::Node;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        use std::collections::BTreeMap;
        let cp = Node {
            metadata: ObjectMeta {
                labels: Some(BTreeMap::from([(
                    "node-role.kubernetes.io/control-plane".to_string(),
                    String::new(),
                )])),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(node_protected(&cp));
        assert!(!node_protected(&Node::default()));
    }

    #[test]
    fn scorecard_lines_report_recovery_and_spend() {
        let base = ChaosScorecard {
            experiment: "outage (scale to 0)".into(),
            target: "demo/web".into(),
            blast: 2,
            budget_before: None,
            budget_after: None,
            dipped: true,
            recovered: false,
            recover_secs: None,
        };
        // Dipped but not back → "not recovered yet".
        assert!(
            scorecard_lines(&base)
                .iter()
                .any(|(t, r)| t.contains("not recovered") && *r == ScoreRole::Warn)
        );
        // Never dipped (a kill it shrugged off) → "stayed up", not a false heal.
        let shrugged = ChaosScorecard {
            dipped: false,
            ..base.clone()
        };
        assert!(
            scorecard_lines(&shrugged)
                .iter()
                .any(|(t, _)| t.contains("stayed up"))
        );
        // Dipped + recovered → self-healed with the time.
        let healed = ChaosScorecard {
            dipped: true,
            recovered: true,
            recover_secs: Some(3.0),
            ..base
        };
        assert!(
            scorecard_lines(&healed)
                .iter()
                .any(|(t, _)| t.contains("self-healed in 3s"))
        );
    }
}
