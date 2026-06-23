//! "Investigate first" → clickable consult links. The Oracle's reply (especially
//! at realm scope) often ends with a prioritized list of OTHER objects worth a
//! look. The model emits that list as an OPTIONAL structured `investigate` block;
//! this module parses it tolerantly and VALIDATES each entry against the live
//! store, turning survivors into a `Scope` the GUI offers as a "CONSULT NEXT"
//! link. Clicking re-consults the Oracle scoped to that single object.
//!
//! SAFETY POSTURE (load-bearing — mirrors `oracle_suggest`):
//! - The model NEVER drives data access or actions. An `InvestigateJson` is a
//!   flat, stringly mirror of model output; serde only ever builds THAT.
//!   `validate_investigate` is the lone path to a `Scope`, and only after
//!   re-resolving the target against the live store — a hallucinated / garbage /
//!   injected target is dropped (the security boundary).
//! - A target is VERB-FREE: it is a read-only consult SCOPE, never an action
//!   (no verb field, so the model can't smuggle a write here).
//! - **No `chaos::ns_protected` / `node_protected` filter** — DELIBERATE, and the
//!   opposite of `oracle_suggest`. `oracle_suggest` refuses protected targets
//!   because it produces a WRITE (staged through the gate); an investigate target
//!   produces only a READ consult (`oracle::build_bundle`), which is fully
//!   redacted + fenced like any other. Blocking "check coredns" would cripple the
//!   realm→specific drill — the same read/write asymmetry the Charter (#6) and
//!   advisors already embody (they READ system namespaces the write paths refuse).
//!   Pinned by `protected_ns_is_not_filtered`.
//! - The model's `why` is untrusted OUTPUT: display-only (the GUI ascii()+truncates
//!   it), never republished, never folded into the next consult's bundle (the jump
//!   rebuilds fresh from the world).

use serde::Deserialize;

use crate::events::ClusterId;

use super::attention::{Concern, Target};
use super::blast;
use super::model::WorkloadRef;
use super::observed::ObservedWorld;
use super::oracle::Scope;
use super::oracle_suggest;

/// How many CONSULT NEXT links to offer (the app's attention queue + any
/// model-named extras, merged).
pub const CONSULT_NEXT_CAP: usize = 5;

/// Flat, stringly mirror of one investigate target the model emits. NEVER becomes
/// a `Scope` except through `validate_investigate`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct InvestigateJson {
    #[serde(default)]
    pub kind: String, // deployment|deploy|statefulset|sts|daemonset|ds|node
    #[serde(default)]
    pub namespace: String, // empty for a node
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub why: String, // model's one-liner; display-only, untrusted
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct InvestigateEnvelope {
    #[serde(default)]
    pub investigate: Vec<InvestigateJson>,
}

/// A validated survivor — a real consult scope + the model's (untrusted) why.
#[derive(Debug, Clone)]
pub struct InvestigateTarget {
    pub scope: Scope,
    pub why: String,
}

/// Why a proposed target was dropped (focused 4-state; deliberately NOT
/// `oracle_suggest::RejectReason`, whose verb/replica variants are meaningless for
/// a read-only scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvestigateReject {
    BadKind(String),
    Missing(&'static str),
    NotFound(String),
}

impl std::fmt::Display for InvestigateReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvestigateReject::BadKind(k) => write!(f, "not a drillable kind: {k}"),
            InvestigateReject::Missing(field) => write!(f, "missing {field}"),
            InvestigateReject::NotFound(s) => write!(f, "no such object: {s}"),
        }
    }
}

/// The optional prompt instruction (the single source of the `investigate` schema).
pub const INVESTIGATE_INSTRUCTION: &str = "If it would help, you MAY append ONE more fenced block listing up to 3 OTHER objects worth a focused, separate look, most-important-first, exactly like:\n```json\n{\"investigate\":[{\"kind\":\"deployment\",\"namespace\":\"ns\",\"name\":\"app\",\"why\":\"short reason\"}]}\n```\nkind is one of deployment|statefulset|daemonset|node (a node has no namespace). Name ONLY workloads or nodes shown in the data above. You cannot fetch anything: each entry becomes a button the operator clicks to RE-CONSULT you, scoped to that single object. This is separate from any FOLLOW-UP LENSES block.";

/// PURE: the instruction when `offer` (else "" — the empty-splice pattern). The
/// caller gates this to scopes where naming OTHER targets helps (Realm / Node).
pub fn investigate_instruction(offer: bool) -> String {
    if offer {
        INVESTIGATE_INSTRUCTION.to_string()
    } else {
        String::new()
    }
}

/// PURE + TOLERANT: pull the `investigate` block out of a model reply (reusing the
/// shared multi-fence scanner so it coexists with the suggestions + follow_up
/// blocks). `None` on absence/garbage; never panics.
pub fn parse_investigate(reply: &str) -> Option<InvestigateEnvelope> {
    for cand in super::oracle::json_blocks(reply) {
        if let Ok(env) = serde_json::from_str::<InvestigateEnvelope>(&cand)
            && !env.investigate.is_empty()
        {
            return Some(env);
        }
    }
    None
}

/// THE security boundary: a model-named target becomes a `Scope` ONLY after it
/// re-resolves against the live store. Workload (Deployment/StatefulSet/DaemonSet)
/// or Node; anything else / hallucinated / empty is rejected.
pub fn validate_investigate(
    i: &InvestigateJson,
    world: &ObservedWorld,
) -> Result<InvestigateTarget, InvestigateReject> {
    let why = i.why.trim().to_string();
    if i.kind.trim().eq_ignore_ascii_case("node") {
        let name = i.name.trim();
        if name.is_empty() {
            return Err(InvestigateReject::Missing("name"));
        }
        let exists = world
            .nodes
            .state()
            .iter()
            .any(|n| n.metadata.name.as_deref() == Some(name));
        if !exists {
            return Err(InvestigateReject::NotFound(format!("node {name}")));
        }
        // No node_protected check — a READ-ONLY consult on a control-plane node is fine.
        return Ok(InvestigateTarget {
            scope: Scope::Node(name.to_string()),
            why,
        });
    }
    let kind = oracle_suggest::parse_kind(&i.kind)
        .map_err(|_| InvestigateReject::BadKind(i.kind.clone()))?;
    if i.namespace.trim().is_empty() || i.name.trim().is_empty() {
        return Err(InvestigateReject::Missing("namespace/name"));
    }
    let wr = WorkloadRef {
        kind,
        namespace: i.namespace.trim().into(),
        name: i.name.trim().into(),
    };
    if !oracle_suggest::workload_exists(world, &wr) {
        return Err(InvestigateReject::NotFound(format!(
            "{} {}/{}",
            i.kind, i.namespace, i.name
        )));
    }
    Ok(InvestigateTarget {
        scope: Scope::Workload(wr),
        why,
    })
}

/// Validate an envelope → the surviving targets (deduped by `Scope::label()`,
/// since `Scope` has no `Eq`). Hallucinated / garbage entries are silently dropped.
pub fn validate_envelope(
    env: &InvestigateEnvelope,
    world: &ObservedWorld,
) -> Vec<InvestigateTarget> {
    let mut out: Vec<InvestigateTarget> = Vec::new();
    for i in &env.investigate {
        if let Ok(t) = validate_investigate(i, world)
            && !out.iter().any(|x| x.scope.label() == t.scope.label())
        {
            out.push(t);
        }
    }
    out
}

/// PURE draw-decision fn: the "CONSULT NEXT" link label — `{scope label} — {why}`,
/// the why truncated; an empty why omits the dash. The GUI maps it to a button.
pub fn investigate_label(t: &InvestigateTarget, max_why: usize) -> String {
    let label = t.scope.label();
    let why = t.why.trim();
    if why.is_empty() {
        return label;
    }
    let why = if why.chars().count() > max_why {
        format!("{}…", why.chars().take(max_why).collect::<String>())
    } else {
        why.to_string()
    };
    format!("{label} — {why}")
}

/// Build CONSULT NEXT targets from the app's OWN attention queue (already
/// severity-ordered) — so a clearly identified concern ALWAYS yields a drill-down
/// link, even when the model omits the structured `investigate` block (small local
/// models are unreliable at structured output; the app already knows what's wrong).
/// App-authored (the concern title is the `why` — trusted, never model output);
/// hot-cluster concerns only; `WorkloadList` (no specific destination) skipped;
/// deduped by scope label (one link per workload/node); capped. The model's
/// validated block is merged ON TOP of this by the caller — it can only ADD
/// targets the queue didn't flag, never bury the critical one.
pub fn concern_targets(concerns: &[Concern], cap: usize) -> Vec<InvestigateTarget> {
    let mut out: Vec<InvestigateTarget> = Vec::new();
    for c in concerns {
        if c.cluster != ClusterId::Hot {
            continue;
        }
        let scope = match &c.target {
            Target::Workload(wr) => Scope::Workload(wr.clone()),
            Target::Node(n) => Scope::Node(n.clone()),
            Target::WorkloadList => continue,
        };
        if out.iter().any(|x| x.scope.label() == scope.label()) {
            continue;
        }
        out.push(InvestigateTarget {
            scope,
            why: c.title.clone(),
        });
        if out.len() >= cap {
            break;
        }
    }
    out
}

/// Like [`concern_targets`] but SCOPED TO A NODE: the drill-downs from a node
/// consult are the troubled workloads STATIONED ON THAT NODE (via the shared
/// `blast::workloads_on_node` topology, so node seeding and blast highlighting can
/// never disagree) plus any concern targeting the node itself. Off-node workloads,
/// other nodes, and `WorkloadList` are skipped; hot-only; deduped; capped.
pub fn concern_targets_on_node(
    concerns: &[Concern],
    world: &ObservedWorld,
    node: &str,
    cap: usize,
) -> Vec<InvestigateTarget> {
    let on = blast::workloads_on_node(world, node);
    let mut out: Vec<InvestigateTarget> = Vec::new();
    for c in concerns {
        if c.cluster != ClusterId::Hot {
            continue;
        }
        let scope = match &c.target {
            Target::Node(n) if n == node => Scope::Node(n.clone()),
            Target::Workload(wr) if on.contains(wr) => Scope::Workload(wr.clone()),
            _ => continue,
        };
        if out.iter().any(|x| x.scope.label() == scope.label()) {
            continue;
        }
        out.push(InvestigateTarget {
            scope,
            why: c.title.clone(),
        });
        if out.len() >= cap {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    fn world() -> ObservedWorld {
        let (world, mut s) = fx::world();
        s.node(fx::node("worker", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 2));
        s.statefulset(fx::statefulset("demo", "db", 1, 1));
        s.daemonset(fx::daemonset("demo", "agent", 1, 1));
        world
    }

    fn t(kind: &str, ns: &str, name: &str) -> InvestigateJson {
        InvestigateJson {
            kind: kind.into(),
            namespace: ns.into(),
            name: name.into(),
            why: "because".into(),
        }
    }

    #[test]
    fn concern_targets_maps_dedups_skips_and_caps() {
        use crate::state::attention::{Severity, Target};
        use crate::state::model::WorkloadKind;
        let wr = WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "crashy".into(),
        };
        let mk = |sev, title: &str, target| Concern {
            severity: sev,
            title: title.into(),
            detail: String::new(),
            target,
            probe: None,
            key: "k".into(),
            cluster: ClusterId::Hot,
        };
        let concerns = vec![
            mk(
                Severity::Critical,
                "crashy crashing",
                Target::Workload(wr.clone()),
            ),
            mk(
                Severity::Warning,
                "crashy again",
                Target::Workload(wr.clone()),
            ), // dup → dropped
            mk(Severity::Warning, "node hot", Target::Node("worker".into())),
            mk(Severity::Info, "elsewhere", Target::WorkloadList), // no destination → skipped
        ];
        let out = concern_targets(&concerns, 5);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].scope.label(), "workload demo/crashy");
        assert_eq!(out[0].why, "crashy crashing"); // the FIRST (critical) title wins
        assert_eq!(out[1].scope.label(), "node worker");
        // A warm-tagged concern is excluded (the Oracle is hot-only).
        let warm = vec![Concern {
            cluster: ClusterId::Warm,
            ..mk(Severity::Critical, "warm", Target::Node("w".into()))
        }];
        assert!(concern_targets(&warm, 5).is_empty());
        // Cap.
        let many: Vec<Concern> = (0..10)
            .map(|i| mk(Severity::Warning, "x", Target::Node(format!("n{i}"))))
            .collect();
        assert_eq!(concern_targets(&many, 3).len(), 3);
    }

    #[test]
    fn concern_targets_on_node_scopes_to_the_node() {
        use crate::state::attention::Severity;
        use crate::state::model::WorkloadKind;
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.node(fx::node("n2", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        s.deployment(fx::deployment("demo", "api", 1, 1));
        s.replicaset(fx::replicaset("demo", "api-rs", "api"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "api-rs-1", Some("n2")),
            "ReplicaSet",
            "api-rs",
        ));
        let wref = |n: &str| WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: n.into(),
        };
        let mk = |title: &str, target| Concern {
            severity: Severity::Warning,
            title: title.into(),
            detail: String::new(),
            target,
            probe: None,
            key: "k".into(),
            cluster: ClusterId::Hot,
        };
        let concerns = vec![
            mk("web bad", Target::Workload(wref("web"))), // on n1 → included
            mk("api bad", Target::Workload(wref("api"))), // on n2 → EXCLUDED (load-bearing)
            mk("node hot", Target::Node("n1".into())),    // this node → included
            mk("other node", Target::Node("n2".into())),  // a different node → excluded
            mk("list", Target::WorkloadList),             // no destination → skipped
        ];
        let out = concern_targets_on_node(&concerns, &world, "n1", 5);
        let labels: Vec<String> = out.iter().map(|t| t.scope.label()).collect();
        assert!(labels.contains(&"workload demo/web".to_string()));
        // The off-node workload MUST be excluded — else node seeding == realm seeding.
        assert!(
            !labels.iter().any(|l| l.contains("api")),
            "off-node workload leaked: {labels:?}"
        );
        assert!(labels.contains(&"node n1".to_string()));
        assert!(!labels.iter().any(|l| l == "node n2"));
        assert_eq!(out.len(), 2);
        // Warm-tagged excluded; cap honored.
        let warm = vec![Concern {
            cluster: ClusterId::Warm,
            ..mk("w", Target::Workload(wref("web")))
        }];
        assert!(concern_targets_on_node(&warm, &world, "n1", 5).is_empty());
        assert_eq!(concern_targets_on_node(&concerns, &world, "n1", 1).len(), 1);
    }

    #[test]
    fn validates_each_kind() {
        let world = world();
        for (k, n) in [
            ("deployment", "web"),
            ("statefulset", "db"),
            ("daemonset", "agent"),
        ] {
            let r = validate_investigate(&t(k, "demo", n), &world);
            assert!(r.is_ok(), "{k}/{n} should validate: {r:?}");
        }
        // a deploy alias resolves the same.
        assert!(validate_investigate(&t("deploy", "demo", "web"), &world).is_ok());
        // a real node validates (no namespace).
        let r = validate_investigate(&t("node", "", "worker"), &world);
        assert_eq!(r.unwrap().scope.label(), "node worker");
    }

    #[test]
    fn rejects_hallucinations_and_garbage() {
        let world = world();
        assert!(matches!(
            validate_investigate(&t("deployment", "demo", "ghost"), &world),
            Err(InvestigateReject::NotFound(_))
        ));
        for k in ["pvc", "service", "pod", "ingress"] {
            assert!(matches!(
                validate_investigate(&t(k, "demo", "x"), &world),
                Err(InvestigateReject::BadKind(_))
            ));
        }
        assert!(matches!(
            validate_investigate(&t("deployment", "demo", ""), &world),
            Err(InvestigateReject::Missing(_))
        ));
        assert!(matches!(
            validate_investigate(&t("node", "", "no-such-node"), &world),
            Err(InvestigateReject::NotFound(_))
        ));
    }

    #[test]
    fn adversarial_envelope_yields_only_survivors() {
        let world = world();
        let env = InvestigateEnvelope {
            investigate: vec![
                t("deployment", "demo", "web"),   // valid
                t("pod", "demo", "web-xyz"),      // bad kind
                t("deployment", "demo", "ghost"), // hallucinated
                t("node", "", "no-such-node"),    // hallucinated node
            ],
        };
        let survivors = validate_envelope(&env, &world);
        assert_eq!(survivors.len(), 1);
        assert_eq!(survivors[0].scope.label(), "workload demo/web");
        let all_bad = InvestigateEnvelope {
            investigate: vec![t("pod", "x", "y"), t("deployment", "x", "ghost")],
        };
        assert!(validate_envelope(&all_bad, &world).is_empty());
    }

    #[test]
    fn protected_ns_is_not_filtered() {
        // A read-only consult on a system-namespace workload is ALLOWED (the
        // deliberate read/write asymmetry vs oracle_suggest). Pinned so it isn't
        // "fixed" into blocking the realm→system drill.
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("kube-system", "coredns", 2, 2));
        let r = validate_investigate(&t("deployment", "kube-system", "coredns"), &world);
        assert!(
            r.is_ok(),
            "a read consult on kube-system must validate: {r:?}"
        );
    }

    #[test]
    fn dedup_by_label() {
        let world = world();
        let env = InvestigateEnvelope {
            investigate: vec![
                t("deployment", "demo", "web"),
                t("deploy", "demo", "web"), // same target, alias
            ],
        };
        assert_eq!(validate_envelope(&env, &world).len(), 1);
    }

    #[test]
    fn parse_extracts_fenced_and_bare_and_rejects_prose() {
        let fenced = "look here\n```json\n{\"investigate\":[{\"kind\":\"node\",\"name\":\"n1\",\"why\":\"hot\"}]}\n```";
        assert_eq!(parse_investigate(fenced).unwrap().investigate.len(), 1);
        let bare =
            "{\"investigate\":[{\"kind\":\"deployment\",\"namespace\":\"x\",\"name\":\"y\"}]}";
        assert_eq!(parse_investigate(bare).unwrap().investigate.len(), 1);
        assert!(parse_investigate("just prose, no json").is_none());
        assert!(parse_investigate("```json\n{\"investigate\":[]}\n```").is_none());
    }

    #[test]
    fn investigate_label_formats_and_truncates() {
        let world = world();
        let tgt = validate_investigate(&t("deployment", "demo", "web"), &world).unwrap();
        let l = investigate_label(&tgt, 80);
        assert!(l.starts_with("workload demo/web"));
        assert!(l.contains(" — because"));
        let tgt2 = InvestigateTarget {
            scope: tgt.scope.clone(),
            why: String::new(),
        };
        assert!(!investigate_label(&tgt2, 80).contains(" — "));
        let tgt3 = InvestigateTarget {
            scope: tgt.scope.clone(),
            why: "x".repeat(200),
        };
        assert!(investigate_label(&tgt3, 20).contains("…"));
    }
}
