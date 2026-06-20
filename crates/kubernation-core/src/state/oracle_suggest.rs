//! Oracle suggest-to-gate — the model PROPOSES an intervention; this PURE module
//! turns its UNTRUSTED output into a real `planned::Intervention` ONLY after
//! re-resolving the target against the live store.
//!
//! Load-bearing invariant: **model output never deserializes straight into an
//! `Intervention`.** It deserializes into a flat, stringly `SuggestionJson` (a
//! mirror, never the enum), and only [`validate`] emits an `Intervention` —
//! after checking the target exists, isn't a chaos-protected namespace/node,
//! and the verb/fields are in range. A hallucinated workload, an unknown verb, a
//! DaemonSet scale, or a kube-system target is REJECTED with a reason, never
//! staged. The model can at worst PROPOSE a reversible, reviewed, gated change.
//! Kept in its own module so this boundary is visible + independently testable.

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::Deserialize;

use crate::state::model::{self, WorkloadKind, WorkloadRef};
use crate::state::observed::ObservedWorld;
use crate::state::planned::Intervention;
use crate::state::{chaos, rollout};

/// A sane upper bound on a suggested replica count (a hallucinated 1e9 is a
/// rejected out-of-range, not a cluster-melting apply).
const MAX_REPLICAS: i64 = 1000;

/// The compact suggestion schema embedded in the system prompt so the model can
/// emit a valid block. The field names here are the SINGLE source shared with
/// `SuggestionJson` + [`validate`] — keep them in sync (the drift guard below
/// fails to compile if the `Intervention` verb set changes).
pub const SUGGEST_INSTRUCTION: &str = "\
If — and only if — you recommend a concrete change, you MAY append ONE fenced \
block exactly like:\n\
```json\n\
{\"suggestions\":[{\"verb\":\"scale\",\"kind\":\"deployment\",\"namespace\":\"ns\",\"name\":\"app\",\"replicas\":3,\"rationale\":\"why\"}]}\n\
```\n\
Allowed verbs and their fields: scale {kind,namespace,name,replicas}; restart \
{kind,namespace,name}; set-image {kind,namespace,name,container,image}; rollback \
{kind,namespace,name,to_revision} (Deployment only); cordon/uncordon {node}. \
kind is one of deployment|statefulset|daemonset. Suggest only changes to objects \
shown in the data. You do NOT apply anything — the operator reviews each \
suggestion and applies it through a confirmed, RBAC-checked, server-side-dry-run \
gate; a wrong suggestion is simply rejected.";

/// The flat, stringly mirror of an intervention the model emits. Deserialized
/// from untrusted output — NEVER converted to an `Intervention` by serde; only
/// [`validate`] does that, after live-store checks.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SuggestionJson {
    #[serde(default)]
    pub verb: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub replicas: Option<i64>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub to_revision: Option<i64>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SuggestionEnvelope {
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub suggestions: Vec<SuggestionJson>,
}

/// Why a suggestion was rejected (shown to the operator — the model is fallible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    UnknownVerb(String),
    BadKind(String),
    /// Target not in the live store (hallucinated, or already gone).
    NotFound(String),
    /// Scale on a DaemonSet (its replica count tracks node count).
    NotScalable,
    OutOfRange(String),
    /// A chaos-protected namespace / control-plane node — never auto-suggested.
    Protected(String),
    /// A required field for the verb is absent/empty.
    Missing(&'static str),
    NoContainer(String),
    NoRevision(i64),
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::UnknownVerb(v) => write!(f, "unknown verb '{v}'"),
            RejectReason::BadKind(k) => write!(f, "bad kind '{k}'"),
            RejectReason::NotFound(t) => write!(f, "{t} not found in the cluster"),
            RejectReason::NotScalable => write!(f, "a DaemonSet can't be scaled"),
            RejectReason::OutOfRange(s) => write!(f, "out of range: {s}"),
            RejectReason::Protected(n) => {
                write!(f, "'{n}' is protected (system namespace / control-plane)")
            }
            RejectReason::Missing(field) => write!(f, "missing '{field}'"),
            RejectReason::NoContainer(c) => write!(f, "no container '{c}' on that workload"),
            RejectReason::NoRevision(r) => write!(f, "no revision {r} to roll back to"),
        }
    }
}

/// A model suggestion that passed validation — a real intervention + a one-line
/// human summary (incl. the model's rationale, if any) for the stage list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSuggestion {
    pub intervention: Intervention,
    pub summary: String,
}

/// PURE + TOLERANT: pull a `SuggestionEnvelope` out of the model's (untrusted)
/// reply text. Never panics; returns `None` when there is no parseable
/// non-empty suggestion block. Tries a fenced ```json block first, then the
/// first `{`..last `}` slice.
pub fn parse_suggestions(reply: &str) -> Option<SuggestionEnvelope> {
    for cand in json_candidates(reply) {
        if let Ok(env) = serde_json::from_str::<SuggestionEnvelope>(&cand)
            && !env.suggestions.is_empty()
        {
            return Some(env);
        }
    }
    None
}

fn json_candidates(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(block) = fenced_block(s) {
        out.push(block);
    }
    if let (Some(a), Some(b)) = (s.find('{'), s.rfind('}'))
        && a < b
    {
        out.push(s[a..=b].to_string());
    }
    out
}

/// The content of the first ``` ... ``` fence (dropping an optional `json`
/// language tag on the opening line).
fn fenced_block(s: &str) -> Option<String> {
    let start = s.find("```")?;
    let after = &s[start + 3..];
    let end = after.find("```")?;
    let mut block = &after[..end];
    // Drop a leading language tag line (```json\n…).
    if let Some(nl) = block.find('\n') {
        let first = block[..nl].trim();
        if !first.contains('{') && first.len() <= 8 {
            block = &block[nl + 1..];
        }
    }
    Some(block.trim().to_string())
}

/// Validate every suggestion in an envelope against the live world. Returns the
/// accepted (real) interventions + a human reason for each rejected one.
pub fn validate_envelope(
    env: &SuggestionEnvelope,
    world: &ObservedWorld,
) -> (Vec<ValidatedSuggestion>, Vec<String>) {
    let mut ok = Vec::new();
    let mut rejected = Vec::new();
    for s in &env.suggestions {
        match validate(s, world) {
            Ok(intervention) => {
                let mut summary = summarize(&intervention);
                if let Some(r) = s.rationale.as_deref().filter(|r| !r.trim().is_empty()) {
                    summary.push_str(&format!(" — {r}"));
                }
                ok.push(ValidatedSuggestion {
                    intervention,
                    summary,
                });
            }
            Err(reason) => rejected.push(format!("{}: {reason}", describe(s))),
        }
    }
    (ok, rejected)
}

/// THE boundary: an untrusted `SuggestionJson` → a real `Intervention`, or a
/// reason. Every target is re-resolved against the live store here.
pub fn validate(s: &SuggestionJson, world: &ObservedWorld) -> Result<Intervention, RejectReason> {
    match s.verb.trim().to_ascii_lowercase().as_str() {
        "cordon" | "uncordon" => {
            let on = s.verb.trim().eq_ignore_ascii_case("cordon");
            let node = s
                .node
                .clone()
                .filter(|n| !n.is_empty())
                .or_else(|| (!s.name.is_empty()).then(|| s.name.clone()))
                .ok_or(RejectReason::Missing("node"))?;
            let nodes = world.nodes.state();
            let nd = nodes
                .iter()
                .find(|n| n.metadata.name.as_deref() == Some(node.as_str()))
                .ok_or_else(|| RejectReason::NotFound(format!("node {node}")))?;
            if chaos::node_protected(nd) {
                return Err(RejectReason::Protected(node));
            }
            Ok(Intervention::Cordon { node, on })
        }
        "scale" => {
            let wr = resolve_workload(s, world)?;
            if wr.kind == WorkloadKind::DaemonSet {
                return Err(RejectReason::NotScalable);
            }
            let r = s.replicas.ok_or(RejectReason::Missing("replicas"))?;
            if !(0..=MAX_REPLICAS).contains(&r) {
                return Err(RejectReason::OutOfRange(format!("replicas {r}")));
            }
            Ok(Intervention::Scale {
                workload: wr,
                replicas: r as i32,
            })
        }
        "restart" => Ok(Intervention::Restart {
            workload: resolve_workload(s, world)?,
        }),
        "set-image" | "setimage" | "set_image" | "image" => {
            let wr = resolve_workload(s, world)?;
            let container = s
                .container
                .clone()
                .filter(|c| !c.is_empty())
                .ok_or(RejectReason::Missing("container"))?;
            let image = s
                .image
                .clone()
                .filter(|i| !i.is_empty())
                .ok_or(RejectReason::Missing("image"))?;
            let has = model::workload_template(world, &wr)
                .and_then(|t| t.spec)
                .map(|sp| sp.containers.iter().any(|c| c.name == container))
                .unwrap_or(false);
            if !has {
                return Err(RejectReason::NoContainer(container));
            }
            Ok(Intervention::SetImage {
                workload: wr,
                container,
                image,
            })
        }
        "rollback" => {
            let wr = resolve_workload(s, world)?;
            if wr.kind != WorkloadKind::Deployment {
                return Err(RejectReason::BadKind("rollback is Deployment-only".into()));
            }
            let rev = s.to_revision.ok_or(RejectReason::Missing("to_revision"))?;
            if !rollout::revisions(world, &wr)
                .iter()
                .any(|r| r.number == rev)
            {
                return Err(RejectReason::NoRevision(rev));
            }
            Ok(Intervention::Rollback {
                workload: wr,
                to_revision: rev,
            })
        }
        other => Err(RejectReason::UnknownVerb(other.to_string())),
    }
}

/// Resolve + verify a workload target: a known kind, a non-protected namespace,
/// and an object that actually exists in the store (else hallucinated).
fn resolve_workload(
    s: &SuggestionJson,
    world: &ObservedWorld,
) -> Result<WorkloadRef, RejectReason> {
    let kind = parse_kind(&s.kind)?;
    if s.namespace.is_empty() || s.name.is_empty() {
        return Err(RejectReason::Missing("namespace/name"));
    }
    if chaos::ns_protected(&s.namespace) {
        return Err(RejectReason::Protected(s.namespace.clone()));
    }
    let wr = WorkloadRef {
        kind,
        namespace: s.namespace.clone(),
        name: s.name.clone(),
    };
    // Existence = the OBJECT is in the store (not "has a resolvable template" —
    // scale/restart/cordon/rollback don't need the template; only set-image does,
    // and it checks the container separately).
    if !workload_exists(world, &wr) {
        return Err(RejectReason::NotFound(format!(
            "{} {}/{}",
            s.kind, s.namespace, s.name
        )));
    }
    Ok(wr)
}

fn workload_exists(world: &ObservedWorld, wr: &WorkloadRef) -> bool {
    let m = |meta: &ObjectMeta| {
        meta.namespace.as_deref() == Some(wr.namespace.as_str())
            && meta.name.as_deref() == Some(wr.name.as_str())
    };
    match wr.kind {
        WorkloadKind::Deployment => world.deployments.state().iter().any(|d| m(&d.metadata)),
        WorkloadKind::StatefulSet => world.statefulsets.state().iter().any(|s| m(&s.metadata)),
        WorkloadKind::DaemonSet => world.daemonsets.state().iter().any(|d| m(&d.metadata)),
    }
}

fn parse_kind(s: &str) -> Result<WorkloadKind, RejectReason> {
    match s.trim().to_ascii_lowercase().as_str() {
        "deployment" | "deploy" => Ok(WorkloadKind::Deployment),
        "statefulset" | "sts" => Ok(WorkloadKind::StatefulSet),
        "daemonset" | "ds" => Ok(WorkloadKind::DaemonSet),
        other => Err(RejectReason::BadKind(other.to_string())),
    }
}

/// A short label for the (rejected) raw suggestion, for the reason list.
fn describe(s: &SuggestionJson) -> String {
    if s.node.is_some()
        || s.verb.eq_ignore_ascii_case("cordon")
        || s.verb.eq_ignore_ascii_case("uncordon")
    {
        format!(
            "{} {}",
            s.verb,
            s.node.clone().unwrap_or_else(|| s.name.clone())
        )
    } else {
        format!("{} {} {}/{}", s.verb, s.kind, s.namespace, s.name)
    }
}

/// A one-line human summary of a validated intervention (the stage-list label).
fn summarize(iv: &Intervention) -> String {
    match iv {
        Intervention::Scale { workload, replicas } => {
            format!(
                "scale {} {}/{} -> {replicas} replicas",
                workload.kind, workload.namespace, workload.name
            )
        }
        Intervention::Cordon { node, on } => {
            format!("{} node {node}", if *on { "cordon" } else { "uncordon" })
        }
        Intervention::Restart { workload } => {
            format!(
                "restart {} {}/{}",
                workload.kind, workload.namespace, workload.name
            )
        }
        Intervention::SetImage {
            workload,
            container,
            image,
        } => {
            format!(
                "set {}/{} [{container}] image -> {image}",
                workload.namespace, workload.name
            )
        }
        Intervention::Rollback {
            workload,
            to_revision,
        } => {
            format!(
                "rollback {} {}/{} -> rev {to_revision}",
                workload.kind, workload.namespace, workload.name
            )
        }
    }
}

/// Verb-set drift guard: if a 6th `Intervention` variant is added, this match
/// stops compiling — a reminder to extend [`validate`], [`summarize`], and the
/// schema in [`SUGGEST_INSTRUCTION`] (a validator that silently can't emit the
/// new verb is the failure mode this prevents).
#[allow(dead_code)]
fn _verb_drift_guard(iv: &Intervention) {
    match iv {
        Intervention::Scale { .. } => {}
        Intervention::Cordon { .. } => {}
        Intervention::Restart { .. } => {}
        Intervention::SetImage { .. } => {}
        Intervention::Rollback { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    fn world() -> ObservedWorld {
        let (world, mut s) = fx::world();
        s.node(fx::node("worker", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 2));
        world
    }

    fn dref() -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        }
    }

    fn sug(verb: &str) -> SuggestionJson {
        SuggestionJson {
            verb: verb.into(),
            ..Default::default()
        }
    }

    #[test]
    fn validates_each_verb_against_the_live_store() {
        let w = world();
        // scale
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            replicas: Some(3),
            ..sug("scale")
        };
        assert_eq!(
            validate(&s, &w),
            Ok(Intervention::Scale {
                workload: dref(),
                replicas: 3
            })
        );
        // restart
        let s = SuggestionJson {
            kind: "deploy".into(),
            namespace: "demo".into(),
            name: "web".into(),
            ..sug("restart")
        };
        assert_eq!(
            validate(&s, &w),
            Ok(Intervention::Restart { workload: dref() })
        );
        // set-image (container "main" exists in the fixture template)
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            container: Some("main".into()),
            image: Some("nginx:1.29".into()),
            ..sug("set-image")
        };
        assert_eq!(
            validate(&s, &w),
            Ok(Intervention::SetImage {
                workload: dref(),
                container: "main".into(),
                image: "nginx:1.29".into()
            })
        );
        // cordon a real, non-protected node
        let s = SuggestionJson {
            node: Some("worker".into()),
            ..sug("cordon")
        };
        assert_eq!(
            validate(&s, &w),
            Ok(Intervention::Cordon {
                node: "worker".into(),
                on: true
            })
        );
    }

    #[test]
    fn rejects_hallucinations_and_unsafe_targets() {
        let w = world();
        // hallucinated workload
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "ghost".into(),
            replicas: Some(1),
            ..sug("scale")
        };
        assert!(matches!(validate(&s, &w), Err(RejectReason::NotFound(_))));
        // unknown verb
        assert!(matches!(
            validate(&sug("delete-namespace"), &w),
            Err(RejectReason::UnknownVerb(_))
        ));
        // protected namespace
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "kube-system".into(),
            name: "coredns".into(),
            replicas: Some(0),
            ..sug("scale")
        };
        assert!(matches!(validate(&s, &w), Err(RejectReason::Protected(_))));
        // out-of-range replicas
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            replicas: Some(-5),
            ..sug("scale")
        };
        assert!(matches!(validate(&s, &w), Err(RejectReason::OutOfRange(_))));
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            replicas: Some(9_000),
            ..sug("scale")
        };
        assert!(matches!(validate(&s, &w), Err(RejectReason::OutOfRange(_))));
        // set-image to a container that doesn't exist
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            container: Some("nope".into()),
            image: Some("x".into()),
            ..sug("set-image")
        };
        assert!(matches!(
            validate(&s, &w),
            Err(RejectReason::NoContainer(_))
        ));
        // rollback to a non-existent revision
        let s = SuggestionJson {
            kind: "deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            to_revision: Some(99),
            ..sug("rollback")
        };
        assert!(matches!(
            validate(&s, &w),
            Err(RejectReason::NoRevision(99))
        ));
        // cordon a control-plane / absent node
        let s = SuggestionJson {
            node: Some("ghost-node".into()),
            ..sug("cordon")
        };
        assert!(matches!(validate(&s, &w), Err(RejectReason::NotFound(_))));
    }

    #[test]
    fn daemonset_scale_is_rejected() {
        let (world, mut s) = fx::world();
        s.daemonset(fx::daemonset("demo", "agent", 3, 3));
        let sug = SuggestionJson {
            kind: "daemonset".into(),
            namespace: "demo".into(),
            name: "agent".into(),
            replicas: Some(3),
            ..sug("scale")
        };
        assert_eq!(validate(&sug, &world), Err(RejectReason::NotScalable));
    }

    #[test]
    fn adversarial_envelope_stages_nothing() {
        // The end-to-end invariant: a malicious model output yields ZERO accepted
        // interventions and a visible reject list.
        let w = world();
        let env = SuggestionEnvelope {
            rationale: "trust me".into(),
            suggestions: vec![
                SuggestionJson {
                    verb: "delete-namespace".into(),
                    namespace: "demo".into(),
                    ..Default::default()
                },
                SuggestionJson {
                    kind: "deployment".into(),
                    namespace: "kube-system".into(),
                    name: "coredns".into(),
                    replicas: Some(0),
                    ..sug("scale")
                },
                SuggestionJson {
                    kind: "deployment".into(),
                    namespace: "demo".into(),
                    name: "web".into(),
                    replicas: Some(-1),
                    ..sug("scale")
                },
                SuggestionJson {
                    kind: "deployment".into(),
                    namespace: "demo".into(),
                    name: "ghost".into(),
                    replicas: Some(1),
                    ..sug("scale")
                },
            ],
        };
        let (ok, rejected) = validate_envelope(&env, &w);
        assert!(ok.is_empty(), "no malicious suggestion may be accepted");
        assert_eq!(rejected.len(), 4);
    }

    #[test]
    fn parse_extracts_fenced_and_bare_json() {
        let fenced = "Here is my analysis.\n\n```json\n{\"suggestions\":[{\"verb\":\"restart\",\"kind\":\"deploy\",\"namespace\":\"demo\",\"name\":\"web\"}]}\n```\nDone.";
        let env = parse_suggestions(fenced).expect("fenced parse");
        assert_eq!(env.suggestions.len(), 1);
        assert_eq!(env.suggestions[0].verb, "restart");
        // Bare object, no fence.
        let bare = "prose {\"suggestions\":[{\"verb\":\"scale\",\"replicas\":2}]} more prose";
        assert!(parse_suggestions(bare).is_some());
        // No suggestions → None (never panics on junk).
        assert!(parse_suggestions("just prose, no json").is_none());
        assert!(parse_suggestions("{not valid json").is_none());
        assert!(parse_suggestions("{\"suggestions\":[]}").is_none());
    }

    #[test]
    fn validate_envelope_accepts_the_good_rejects_the_bad() {
        let w = world();
        let env = SuggestionEnvelope {
            rationale: String::new(),
            suggestions: vec![
                SuggestionJson {
                    kind: "deployment".into(),
                    namespace: "demo".into(),
                    name: "web".into(),
                    replicas: Some(4),
                    rationale: Some("under load".into()),
                    ..sug("scale")
                },
                SuggestionJson {
                    verb: "nuke".into(),
                    ..Default::default()
                },
            ],
        };
        let (ok, rejected) = validate_envelope(&env, &w);
        assert_eq!(ok.len(), 1);
        assert!(ok[0].summary.contains("scale") && ok[0].summary.contains("under load"));
        assert_eq!(rejected.len(), 1);
    }
}
