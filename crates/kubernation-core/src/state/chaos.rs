//! Chaos / game-day — resilience drills that inject a *real* failure with
//! standard Kubernetes resources and let you watch the cluster respond (the
//! blast radius spreads, the attention queue lights up, the treasury spends).
//! The 4X framing is a "raid" on a city; the k8s nouns stay greppable.
//!
//! **Pass 1** reused the existing write primitives — `evict_pod` (delete) and
//! `apply_intervention(Scale)` (patch `spec.replicas`): kill one pod, kill all
//! pods (the controller recreates), and an outage (scale to 0) with an explicit
//! restore. **Pass 2** adds three more, reusing the same primitives where it can
//! and adding exactly one new write surface: **node failure** (Cordon + drain
//! the node's pods, restore = uncordon — all existing verbs), **broken image**
//! (SetImage onto an unresolvable ref → ImagePullBackOff, restore = the captured
//! original — existing verb), and **partition** (a deny-all NetworkPolicy scoped
//! to the workload's pods, restore = delete it — the one new verb/resource type
//! chaos adds, in `actions::apply_partition`/`delete_partition`). Mesh
//! fault-injection (Istio/Linkerd) is deferred to a later pass.
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

/// The image a broken-image drill rolls onto — a reserved-TLD ref that can
/// never resolve (RFC-6761 `.invalid`), so the workload goes ImagePullBackOff
/// and the cause is self-announcing in events. Restored to the captured original.
pub const BAD_IMAGE: &str = "kubernation.invalid/chaos/broken:does-not-exist";

/// A chaos experiment. Most target a workload; node-failure targets a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Experiment {
    /// Delete one representative pod (the controller recreates it).
    KillOne { workload: WorkloadRef },
    /// Delete every pod of the workload (a full raid).
    KillAll { workload: WorkloadRef },
    /// Scale the workload to 0 (a real outage), restorable to its current count.
    Outage { workload: WorkloadRef },
    /// Cordon a node and drain (evict) every pod on it; restore = uncordon.
    NodeFailure { node: String },
    /// Roll the workload onto a broken image (ImagePullBackOff); restore = the
    /// captured original image.
    BrokenImage { workload: WorkloadRef },
    /// Isolate the workload with a deny-all NetworkPolicy; restore = delete it.
    Partition { workload: WorkloadRef },
}

impl Experiment {
    /// What the drill targets (for the blast radius + scorecard).
    pub fn subject(&self) -> Subject {
        match self {
            Experiment::NodeFailure { node } => Subject::Node(node.clone()),
            Experiment::KillOne { workload }
            | Experiment::KillAll { workload }
            | Experiment::Outage { workload }
            | Experiment::BrokenImage { workload }
            | Experiment::Partition { workload } => Subject::Workload(workload.clone()),
        }
    }

    /// The target workload, or `None` for a node-scoped experiment.
    pub fn workload(&self) -> Option<&WorkloadRef> {
        match self {
            Experiment::KillOne { workload }
            | Experiment::KillAll { workload }
            | Experiment::Outage { workload }
            | Experiment::BrokenImage { workload }
            | Experiment::Partition { workload } => Some(workload),
            Experiment::NodeFailure { .. } => None,
        }
    }

    /// Short operator-facing label.
    pub fn label(&self) -> &'static str {
        match self {
            Experiment::KillOne { .. } => "kill one pod",
            Experiment::KillAll { .. } => "kill all pods",
            Experiment::Outage { .. } => "outage (scale to 0)",
            Experiment::NodeFailure { .. } => "node failure (cordon + drain)",
            Experiment::BrokenImage { .. } => "broken image",
            Experiment::Partition { .. } => "partition (deny-all)",
        }
    }
}

/// A deny-all NetworkPolicy descriptor (the k8s object is built in `actions.rs`
/// — `chaos.rs` stays client-free).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetpolSpec {
    pub namespace: String,
    pub name: String,
    /// `matchLabels` for the policy's `podSelector` (the workload's pods).
    pub pod_selector: std::collections::BTreeMap<String, String>,
}

/// One concrete cluster step. Every variant is an existing primitive *except*
/// the NetworkPolicy create/delete the partition experiment adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChaosStep {
    /// Delete a pod (`actions::evict_pod`).
    Evict { namespace: String, pod: String },
    /// Apply any planning intervention — Scale / Cordon / SetImage
    /// (`actions::apply_intervention`).
    Apply(Intervention),
    /// Create a deny-all NetworkPolicy (`actions::apply_partition`) — the one
    /// new write verb/resource type chaos adds.
    Partition(NetpolSpec),
    /// Delete a chaos NetworkPolicy by name (`actions::delete_partition`),
    /// idempotent (a 404 is success).
    Unpartition { namespace: String, name: String },
}

/// A planned drill: the inject steps, the restore steps (undo), why it was
/// refused (if so), and the blast size at plan time.
#[derive(Debug, Clone)]
pub struct ChaosPlan {
    pub steps: Vec<ChaosStep>,
    /// Steps that undo the drill (Outage → scale back, partition → unpartition,
    /// node → uncordon); empty for kills (the controller recreates pods).
    pub restore: Vec<ChaosStep>,
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

/// Pods scheduled on a node, sorted, *excluding protected namespaces* (a
/// node-failure drill must never drain the cluster's own system pods).
fn pods_on_node(world: &ObservedWorld, node: &str) -> Vec<(String, String)> {
    let mut pods: Vec<(String, String)> = world
        .pods
        .state()
        .iter()
        .filter(|p| p.spec.as_ref().and_then(|s| s.node_name.as_deref()) == Some(node))
        .filter_map(|p| Some((p.metadata.namespace.clone()?, p.metadata.name.clone()?)))
        .filter(|(ns, _)| !ns_protected(ns))
        .collect();
    pods.sort();
    pods
}

/// The chaos NetworkPolicy name for a workload (DNS-1123-safe).
fn netpol_name(wr: &WorkloadRef) -> String {
    format!("kubernation-chaos-{}", wr.name)
}

/// The namespace an intervention writes to (`None` for node-scoped Cordon) —
/// used by the frontend's fail-closed protected-namespace re-check.
pub fn iv_namespace(iv: &Intervention) -> Option<&str> {
    match iv {
        Intervention::Scale { workload, .. }
        | Intervention::Restart { workload }
        | Intervention::SetImage { workload, .. } => Some(&workload.namespace),
        Intervention::Cordon { .. } => None,
    }
}

/// Resolve an experiment against the observed world into a concrete plan —
/// PURE (no client). Refuses protected targets fail-closed, captures the
/// restore steps, and computes the blast size.
pub fn plan_chaos(world: &ObservedWorld, exp: &Experiment) -> ChaosPlan {
    let blast = blast_radius(world, &exp.subject()).len();

    // Workload experiments refuse protected namespaces up front; node-failure
    // has its own control-plane guard below.
    if let Some(wr) = exp.workload()
        && ns_protected(&wr.namespace)
    {
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
                    steps: vec![ChaosStep::Apply(Intervention::Scale {
                        workload: workload.clone(),
                        replicas: 0,
                    })],
                    restore: vec![ChaosStep::Apply(Intervention::Scale {
                        workload: workload.clone(),
                        replicas: n,
                    })],
                    refused: None,
                    blast,
                },
                Some(_) => ChaosPlan::refuse(blast, "already scaled to 0"),
                None => ChaosPlan::refuse(blast, "replicas unknown"),
            }
        }
        Experiment::NodeFailure { node } => {
            let Some(n) = world
                .nodes
                .state()
                .into_iter()
                .find(|n| n.metadata.name.as_deref() == Some(node))
            else {
                return ChaosPlan::refuse(blast, "node not found");
            };
            if node_protected(&n) {
                return ChaosPlan::refuse(blast, "control-plane node — refused");
            }
            let pods = pods_on_node(world, node);
            if pods.is_empty() {
                return ChaosPlan::refuse(blast, "no drainable pods on this node");
            }
            // Cordon first, then drain every (non-system) pod.
            let mut steps = vec![ChaosStep::Apply(Intervention::Cordon {
                node: node.clone(),
                on: true,
            })];
            steps.extend(
                pods.into_iter()
                    .map(|(namespace, pod)| ChaosStep::Evict { namespace, pod }),
            );
            ChaosPlan {
                steps,
                restore: vec![ChaosStep::Apply(Intervention::Cordon {
                    node: node.clone(),
                    on: false,
                })],
                refused: None,
                blast,
            }
        }
        Experiment::BrokenImage { workload } => {
            let Some(container) = crate::state::model::workload_primary_container(world, workload)
            else {
                return ChaosPlan::refuse(blast, "no container to break");
            };
            // The original image must be captured or restore is impossible.
            let Some(original) = crate::state::planned::current_image(world, workload, &container)
            else {
                return ChaosPlan::refuse(blast, "cannot read the current image (no restore)");
            };
            ChaosPlan {
                steps: vec![ChaosStep::Apply(Intervention::SetImage {
                    workload: workload.clone(),
                    container: container.clone(),
                    image: BAD_IMAGE.to_string(),
                })],
                restore: vec![ChaosStep::Apply(Intervention::SetImage {
                    workload: workload.clone(),
                    container,
                    image: original,
                })],
                refused: None,
                blast,
            }
        }
        Experiment::Partition { workload } => {
            let labels = crate::state::model::workload_template_labels(world, workload);
            if labels.is_empty() {
                // An empty podSelector denies the WHOLE namespace — never do that.
                return ChaosPlan::refuse(
                    blast,
                    "no pod labels — a deny-all would hit the whole namespace",
                );
            }
            let name = netpol_name(workload);
            ChaosPlan {
                steps: vec![ChaosStep::Partition(NetpolSpec {
                    namespace: workload.namespace.clone(),
                    name: name.clone(),
                    pod_selector: labels,
                })],
                restore: vec![ChaosStep::Unpartition {
                    namespace: workload.namespace.clone(),
                    name,
                }],
                refused: None,
                blast,
            }
        }
    }
}

/// What class of scorecard to render — the experiments have different shapes
/// (a workload's budget/recovery, a node's multi-workload drain, an isolation
/// with no readiness signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreKind {
    /// Outage / KillOne / KillAll / BrokenImage — the dip/recover + budget model.
    Workload,
    /// Node-failure — pods drained + cordon state.
    Node { pods_drained: usize, cordoned: bool },
    /// Partition — a deny-all NetworkPolicy; readiness doesn't dip.
    Isolation,
}

/// A game-day scorecard: what the drill did and how the cluster responded.
#[derive(Debug, Clone)]
pub struct ChaosScorecard {
    pub kind: ScoreKind,
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

/// The recovery line for a dip/recover-model scorecard.
fn recovery_line(s: &ChaosScorecard) -> (String, ScoreRole) {
    if !s.dipped {
        ("stayed up — no outage".into(), ScoreRole::Good)
    } else {
        match (s.recovered, s.recover_secs) {
            (true, Some(secs)) => (format!("self-healed in {secs:.0}s"), ScoreRole::Good),
            (true, None) => ("self-healed".into(), ScoreRole::Good),
            (false, _) => ("not recovered yet…".into(), ScoreRole::Warn),
        }
    }
}

/// The scorecard rendered as text lines + roles — pure draw-decision logic,
/// testable without a GL context. Branches on `kind` so each experiment class
/// tells an honest story.
pub fn scorecard_lines(s: &ChaosScorecard) -> Vec<(String, ScoreRole)> {
    let mut out = vec![
        (format!("{} on {}", s.experiment, s.target), ScoreRole::Info),
        (
            format!("blast radius: {} affected", s.blast),
            ScoreRole::Info,
        ),
    ];
    match s.kind {
        ScoreKind::Workload => {
            out.push(recovery_line(s));
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
        }
        ScoreKind::Node {
            pods_drained,
            cordoned,
        } => {
            out.push((format!("{pods_drained} pod(s) drained"), ScoreRole::Info));
            out.push(recovery_line(s)); // "workloads back to full strength?"
            if cordoned {
                out.push((
                    "node still cordoned — Restore to uncordon".into(),
                    ScoreRole::Warn,
                ));
            }
        }
        ScoreKind::Isolation => {
            // A partition doesn't drop readiness — suppress the dip/recover model.
            out.push(("deny-all NetworkPolicy applied".into(), ScoreRole::Info));
            out.push((
                "isolates traffic — readiness won't dip; Restore removes it".into(),
                ScoreRole::Info,
            ));
            out.push((
                "effect depends on the CNI enforcing NetworkPolicy".into(),
                ScoreRole::Warn,
            ));
        }
    }
    out
}

/// Extra preview lines for the chaos window, per experiment — pure, testable.
/// (The common lines — steps/blast/budget — the GUI renders directly.)
pub fn preview_lines(exp: &Experiment, plan: &ChaosPlan) -> Vec<(String, ScoreRole)> {
    if plan.is_refused() {
        return Vec::new();
    }
    match exp {
        Experiment::BrokenImage { .. } => vec![
            (format!("roll onto {BAD_IMAGE}"), ScoreRole::Warn),
            (
                "restore re-applies the current image".into(),
                ScoreRole::Info,
            ),
        ],
        Experiment::Partition { .. } => vec![
            (
                "deny-all NetworkPolicy (a new resource)".into(),
                ScoreRole::Info,
            ),
            (
                "effect depends on the CNI enforcing NetworkPolicy".into(),
                ScoreRole::Warn,
            ),
        ],
        Experiment::NodeFailure { .. } => {
            vec![(
                "graceful drain (evict), then Restore to uncordon".into(),
                ScoreRole::Info,
            )]
        }
        _ => Vec::new(),
    }
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
            ChaosStep::Apply(Intervention::Scale { replicas: 0, .. })
        ));
        // Restore scales back to the captured count (3).
        assert!(matches!(
            &plan.restore[0],
            ChaosStep::Apply(Intervention::Scale { replicas: 3, .. })
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
            kind: ScoreKind::Workload,
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

    #[test]
    fn node_failure_cordons_then_drains_and_restores() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        seed_web(&mut s); // 3 web pods, all on n1
        let plan = plan_chaos(&world, &Experiment::NodeFailure { node: "n1".into() });
        assert!(!plan.is_refused(), "{:?}", plan.refused);
        // Cordon first, then one evict per drainable pod.
        assert_eq!(plan.steps.len(), 4);
        assert!(matches!(
            &plan.steps[0],
            ChaosStep::Apply(Intervention::Cordon { on: true, .. })
        ));
        assert!(matches!(&plan.steps[1], ChaosStep::Evict { .. }));
        // Restore uncordons.
        assert!(matches!(
            &plan.restore[0],
            ChaosStep::Apply(Intervention::Cordon { on: false, .. })
        ));
    }

    #[test]
    fn node_failure_refuses_missing_node_and_empty_node() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        // Node exists but has no (non-system) pods → refused.
        assert!(plan_chaos(&world, &Experiment::NodeFailure { node: "n1".into() }).is_refused());
        // Node doesn't exist → refused.
        assert!(
            plan_chaos(
                &world,
                &Experiment::NodeFailure {
                    node: "ghost".into()
                }
            )
            .is_refused()
        );
    }

    #[test]
    fn broken_image_captures_original_and_refuses_without() {
        // Fixture deployments have no container image → can't restore → refused.
        let (world, mut s) = fx::world();
        seed_web(&mut s);
        assert!(plan_chaos(&world, &Experiment::BrokenImage { workload: web() }).is_refused());

        // A deployment whose container carries an image → captured + restorable.
        let (world2, mut s2) = fx::world();
        let mut d = fx::deployment("demo", "web", 3, 3);
        if let Some(c) = d
            .spec
            .as_mut()
            .and_then(|sp| sp.template.spec.as_mut())
            .and_then(|ps| ps.containers.first_mut())
        {
            c.image = Some("nginx:1.25".into());
        }
        s2.deployment(d);
        let plan = plan_chaos(&world2, &Experiment::BrokenImage { workload: web() });
        assert!(!plan.is_refused(), "{:?}", plan.refused);
        assert!(matches!(
            &plan.steps[0],
            ChaosStep::Apply(Intervention::SetImage { image, .. }) if image == BAD_IMAGE
        ));
        assert!(matches!(
            &plan.restore[0],
            ChaosStep::Apply(Intervention::SetImage { image, .. }) if image == "nginx:1.25"
        ));
    }

    #[test]
    fn partition_uses_pod_labels_and_restores() {
        let (world, mut s) = fx::world();
        seed_web(&mut s); // template labels {app: web}
        let plan = plan_chaos(&world, &Experiment::Partition { workload: web() });
        assert!(!plan.is_refused(), "{:?}", plan.refused);
        match &plan.steps[0] {
            ChaosStep::Partition(spec) => {
                assert_eq!(spec.namespace, "demo");
                assert_eq!(spec.name, "kubernation-chaos-web");
                assert_eq!(
                    spec.pod_selector.get("app").map(String::as_str),
                    Some("web")
                );
            }
            other => panic!("expected Partition step, got {other:?}"),
        }
        assert!(matches!(
            &plan.restore[0],
            ChaosStep::Unpartition { name, .. } if name == "kubernation-chaos-web"
        ));
    }

    #[test]
    fn partition_refused_without_pod_labels() {
        // A deployment whose pod template carries no labels → a deny-all would
        // hit the whole namespace, so it's refused.
        let (world, mut s) = fx::world();
        let mut d = fx::deployment("demo", "web", 3, 3);
        if let Some(t) = d.spec.as_mut().map(|sp| &mut sp.template) {
            t.metadata = Some(Default::default());
        }
        s.deployment(d);
        assert!(plan_chaos(&world, &Experiment::Partition { workload: web() }).is_refused());
    }

    #[test]
    fn node_and_isolation_scorecards_tell_their_own_story() {
        let node_card = ChaosScorecard {
            kind: ScoreKind::Node {
                pods_drained: 3,
                cordoned: true,
            },
            experiment: "node failure (cordon + drain)".into(),
            target: "node n1".into(),
            blast: 3,
            budget_before: None,
            budget_after: None,
            dipped: false,
            recovered: false,
            recover_secs: None,
        };
        let lines = scorecard_lines(&node_card);
        assert!(lines.iter().any(|(t, _)| t.contains("3 pod(s) drained")));
        assert!(lines.iter().any(|(t, _)| t.contains("still cordoned")));

        let iso_card = ChaosScorecard {
            kind: ScoreKind::Isolation,
            experiment: "partition (deny-all)".into(),
            ..node_card
        };
        let lines = scorecard_lines(&iso_card);
        assert!(lines.iter().any(|(t, _)| t.contains("NetworkPolicy")));
        // An isolation never claims a recovery/dip.
        assert!(!lines.iter().any(|(t, _)| t.contains("self-healed")));
    }
}
