//! The Annals — a recent, classified change-feed answering "what changed?".
//!
//! The third triage axis beside the attention queue (what's wrong NOW) and the
//! blast/impact panel (what else is affected): **what changed**. PURE — merges
//! three sources into one newest-first, classified timeline:
//!
//!   (a) the recent-events ring (`recent_events()`, ~500, deduped by
//!       (kind,ns,name,reason) — so it is RECENT, not an audit log),
//!   (b) ReplicaSet **revisions** (the *authoritative* deploy record — the event
//!       ring dedups `ScalingReplicaSet` by reason and would hide intermediate
//!       rollouts, so deploys come from the RS store, not the ring), and
//!   (d) an injected slice of **in-session operator actions** (commits / evicts /
//!       chaos drills — the GUI owns these facts and passes them in, so core
//!       stays pure + persistence-free).
//!
//! NOT a full audit log: the event ring is bounded (~15 min) and revisions are
//! Deployment-only (STS/DS track theirs in unwatched ControllerRevisions). The
//! UI states this. `now` is passed in (clockless core; deterministic tests) — the
//! project's accepted windowed-recency exception, exactly like `attention::build`.

use std::collections::HashSet;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::jiff::Timestamp;

use super::attention::Severity;
use super::filter::NamespaceFilter;
use super::model::{WorkloadKind, WorkloadRef, build_workloads};
use super::observed::ObservedWorld;
use super::rollout::{image_changes, revisions};

/// How far back event-sourced entries may be and still surface. Aligned with
/// `attention::EVENT_WINDOW_MIN` — and honest about the ring's ~horizon.
pub const TIMELINE_WINDOW_MIN: i64 = 15;
/// A change within this window *before* the first failure is flagged "preceded
/// by" (a suspect). `<=` the data horizon — we never claim older correlation.
pub const CORRELATION_WINDOW_MIN: i64 = 10;
/// Cap for the cluster-wide feed (the modal).
pub const CLUSTER_CAP: usize = 80;
/// Cap for a per-subject feed (the city/node window section).
pub const SUBJECT_CAP: usize = 30;

/// What class of change an entry is. Drives the glyph + colour in the GUI; also
/// gates which entries the cluster feed shows (PodChurn is per-subject only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// A new ReplicaSet revision — a rollout (carries the rev + image delta).
    Deploy,
    /// A replica-count change (`ScalingReplicaSet`/`Scaled`, operator Scale).
    Scale,
    /// Benign pod lifecycle (Started/Created/Pulled/Killing/…). Per-subject only.
    PodChurn,
    /// Scheduling (Scheduled / FailedScheduling).
    Schedule,
    /// Node lifecycle (NotReady / Ready / cordon / register).
    NodeChange,
    /// Something KuberNation itself did this session (injected).
    Operator,
    /// A failure event (crash / OOM / probe / image-pull / config).
    Failure,
    /// Any other event — never dropped (honest fallback).
    Event,
}

impl ChangeKind {
    /// Whether this is a *change* (a candidate cause), as opposed to a symptom
    /// (Failure), passive churn (PodChurn), or noise (Event/Schedule). Used by
    /// the correlation "preceded by" cue.
    pub fn is_change(self) -> bool {
        matches!(
            self,
            ChangeKind::Deploy | ChangeKind::Scale | ChangeKind::Operator | ChangeKind::NodeChange
        )
    }
}

/// The operator verb behind an injected entry (drives the glyph + phrasing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpVerb {
    Scale,
    Restart,
    SetImage,
    Rollback,
    Cordon,
    Evict,
    Chaos,
    /// Ground reclaimed — the slots of nodes that have gone for good released
    /// back to the map.
    ///
    /// A **cataclysm** in the plan's sense: structural change to the world
    /// rather than a routine replacement. It reaches the operator as a record
    /// here rather than as a mark on the terrain, because after a compaction
    /// there is nothing left to mark — the ground it describes is precisely the
    /// ground that stopped existing.
    Compact,
}

/// A change KuberNation itself made this session — injected by the GUI (which
/// owns these facts), keeping core pure + persistence-free. The GUI stamps
/// `when` at action time with `util::now()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorAction {
    pub when: Timestamp,
    pub verb: OpVerb,
    pub namespace: String,
    pub name: String,
    /// "Deployment" / "Pod" / "Node" — the involved object's kind.
    pub kind: String,
    /// What happened, e.g. "scaled 3→5" / "evicted web-7j8fp" / "chaos KillOne".
    pub detail: String,
    pub severity: Severity,
}

/// One row of the feed.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    /// When it happened. `None` sinks to a flagged, deterministic tail (never
    /// epoch-0, never a correlation suspect or the fault-line anchor).
    pub when: Option<Time>,
    pub kind: ChangeKind,
    pub severity: Severity,
    /// (namespace, name, kind) — for scope filtering + the correlation subject.
    pub subject: (String, String, String),
    pub title: String,
    pub detail: String,
    /// Set on Deploy entries — the city window's rollback button reads this.
    pub revision: Option<i64>,
    /// Collapsed-repeat multiplicity (>=1) — the ring's `count`.
    pub count: i32,
    /// True for injected operator entries — drives the "(you)" attribution.
    pub operator: bool,
    /// Stable identity (cycling / dedup tiebreak).
    pub key: String,
    /// When this entry's subject FIRST went wrong — the incident's onset, as
    /// opposed to `when`, its latest occurrence. Already resolved: falls back to
    /// `when` when nothing reported it, so an entry with unknown onset behaves
    /// exactly as it did before this field existed.
    pub onset: Option<Time>,
    /// Whether `onset` was READ or ASSUMED from `when`.
    ///
    /// Recorded rather than silently collapsed, in the voice `metric_source` and
    /// `CostBasis` use: a fault line anchored on an assumed onset is as good as
    /// the old behaviour and no better, and a caller that cares can tell.
    pub onset_reported: bool,
}

/// What the feed is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineScope {
    Cluster,
    Workload(WorkloadRef),
    Node(String),
}

/// Build inputs.
pub struct TimelineOpts<'a> {
    pub scope: TimelineScope,
    /// Applied only in `Cluster` scope (node/cluster-scoped entries always kept).
    pub filter: &'a NamespaceFilter,
    /// Event-sourced recency window (minutes). Deploy + operator entries are
    /// always kept (full rollout history / sparse in-session actions).
    pub window_min: i64,
    pub cap: usize,
}

/// The built feed.
#[derive(Debug, Clone)]
pub struct Timeline {
    /// Newest first; untimed entries trail at the end.
    pub entries: Vec<TimelineEntry>,
    /// When the current trouble BEGAN: the earliest onset among in-scope
    /// failures (>= Warning) that started recently enough to be part of it.
    ///
    /// Onset, not latest occurrence — see [`anchors`]. `None` means nothing
    /// started recently, which is a real answer: a scope whose only failures are
    /// chronic gets no fault line rather than one drawn at an arbitrary point.
    pub first_trouble: Option<Time>,
    /// The cap clipped older entries.
    pub truncated: bool,
    /// A non-Deployment subject has no RS-tracked rollout history (honest note).
    pub deployment_only_note: bool,
    /// Echoed in the honesty footer.
    pub window_min: i64,
}

/// The onset at which `e` could anchor a fault line, or `None` if it cannot.
///
/// # Onset, not latest occurrence
///
/// The anchor is the entry's ONSET. "Trouble begins here" is a claim about when
/// the incident started, and the suspect window is measured backwards from it —
/// so a deploy five minutes before a failure *started* is a candidate cause,
/// while five minutes before that failure's four-thousandth recurrence is not a
/// fact about anything. Anchoring on the latest occurrence made the second
/// reading the only one available.
///
/// # Why the recency window, and not the correlation window
///
/// Two windows exist and they are different constants for different purposes:
/// `opts.window_min` bounds what is recent enough to appear in the feed at all,
/// and `CORRELATION_WINDOW_MIN` bounds how long before a failure a change may be
/// called a precursor.
///
/// The anchor uses the **recency** window, because the entry set has already
/// been narrowed by it — so an anchor bounded any more tightly could leave an
/// entry visible in the Annals that is nonetheless unable to be the trouble it
/// visibly is. Using the correlation window would do exactly that: a failure
/// that began twelve minutes ago would be shown and simultaneously ineligible.
/// It is also the caller's option rather than a constant, so a caller that
/// widens the feed widens the anchor with it.
///
/// # What this excludes
///
/// A chronic failure self-excludes: its onset is hours or days old, outside any
/// window, so it stops winning the minimum by virtue of having been refreshed a
/// moment ago. No chronic *threshold* is introduced — that would be another
/// unmeasured constant to defend, and the window already earns its keep.
///
/// If every failure in scope is chronic, there is no anchor and no fault line.
/// That is the honest answer: nothing started recently.
///
/// Clock skew follows the convention the recency filter already set — the
/// signed comparison keeps a future timestamp rather than treating it as
/// impossibly old, so onset does not get a second, contradictory skew policy.
fn anchors(e: &TimelineEntry, now: Timestamp, cutoff: i64) -> Option<Time> {
    let onset = e.onset.clone()?;
    // An entry with no `when` sinks to the untimed tail and never anchors, the
    // same rule as before this change.
    e.when.as_ref()?;
    // Signed, like the recency filter above: a future onset (skew) yields a
    // negative age and is kept.
    (now.duration_since(onset.0).as_secs() <= cutoff).then_some(onset)
}

/// THE pure builder. `now` is passed in.
pub fn build_timeline(
    world: &ObservedWorld,
    opts: &TimelineOpts,
    ops: &[OperatorAction],
    now: Timestamp,
) -> Timeline {
    // For a Node-scoped feed, the pods stationed on it as precise (ns, name) pairs
    // — a same-named pod in another namespace must not leak in. Workload scopes
    // match their pods by an RS-name prefix instead (assembled below), which also
    // catches a now-deleted pod whose events still linger in the ring.
    let node_pods: Vec<(String, String)> = match &opts.scope {
        TimelineScope::Node(node) => world
            .pods
            .state()
            .iter()
            .filter(|p| p.spec.as_ref().and_then(|s| s.node_name.as_deref()) == Some(node.as_str()))
            .filter_map(|p| {
                Some((
                    p.metadata.namespace.clone().unwrap_or_default(),
                    p.metadata.name.clone()?,
                ))
            })
            .collect(),
        _ => Vec::new(),
    };

    let mut entries: Vec<TimelineEntry> = Vec::new();

    // (b) RS revisions → Deploy entries (the authoritative deploy record).
    // `covered_rs` lets us suppress the redundant per-pod create/delete events
    // those revisions already imply.
    let mut covered_rs: HashSet<(String, String)> = HashSet::new();
    let deploy_refs: Vec<WorkloadRef> = match &opts.scope {
        TimelineScope::Cluster => build_workloads(world)
            .into_iter()
            .filter(|w| w.r.kind == WorkloadKind::Deployment)
            .map(|w| w.r)
            .collect(),
        TimelineScope::Workload(wr) if wr.kind == WorkloadKind::Deployment => vec![wr.clone()],
        _ => Vec::new(),
    };
    for wr in &deploy_refs {
        let revs = revisions(world, wr);
        for (i, rev) in revs.iter().enumerate() {
            covered_rs.insert((wr.namespace.clone(), rev.rs_name.clone()));
            // `revs` is newest-first, so the next index is the older revision.
            let prev = revs.get(i + 1);
            let detail = match prev {
                Some(p) => {
                    let deltas = image_changes(p, rev);
                    // ASCII arrow + "(none)" — render-safe in the GUI's `ascii()`
                    // (which maps fancier glyphs to '?'); matches the old HISTORY.
                    if deltas.is_empty() {
                        format!("rev {} -> {}", p.number, rev.number)
                    } else {
                        let d = deltas
                            .iter()
                            .map(|d| {
                                format!(
                                    "{}: {} -> {}",
                                    d.container,
                                    d.from.as_deref().unwrap_or("(none)"),
                                    d.to.as_deref().unwrap_or("(none)")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("rev {}->{} · {}", p.number, rev.number, d)
                    }
                }
                None => format!("rev {} (first observed)", rev.number),
            };
            entries.push(TimelineEntry {
                onset: rev.created.clone(),
                onset_reported: true,
                when: rev.created.clone(),
                kind: ChangeKind::Deploy,
                severity: Severity::Info,
                subject: (wr.namespace.clone(), wr.name.clone(), "Deployment".into()),
                title: wr.name.clone(),
                detail,
                revision: Some(rev.number),
                count: 1,
                operator: false,
                key: format!("deploy:{}:{}:{}", wr.namespace, wr.name, rev.number),
            });
        }
    }

    // The membership of a scoped feed. A Workload matches: its own object + its
    // ReplicaSets (exact ns/name), plus any object whose name carries one of its
    // RS-name prefixes — its pods, current AND now-deleted ones still in the ring;
    // the RS pod-template-hash disambiguates a sibling like `web` vs `web-api`. A
    // Deployment with no RS observed yet, and STS/DS (no RS), fall back to the
    // workload-name prefix. A Node matches its own events + the pods on it.
    let mut member_exact: Vec<(String, String)> = node_pods;
    let mut member_prefixes: Vec<String> = Vec::new();
    if let TimelineScope::Workload(wr) = &opts.scope {
        member_exact.push((wr.namespace.clone(), wr.name.clone()));
        member_exact.extend(covered_rs.iter().cloned());
        if wr.kind == WorkloadKind::Deployment && !covered_rs.is_empty() {
            member_prefixes.extend(covered_rs.iter().map(|(_, rs)| format!("{rs}-")));
        } else {
            member_prefixes.push(format!("{}-", wr.name));
        }
    }

    // (a) Events.
    for ev in world.recent_events() {
        if !touches(
            &opts.scope,
            &ev.namespace,
            &ev.name,
            &member_exact,
            &member_prefixes,
        ) {
            continue;
        }
        // Drop the per-pod create/delete churn a Deploy entry already implies.
        if matches!(ev.reason.as_str(), "SuccessfulCreate" | "SuccessfulDelete")
            && ev.kind == "ReplicaSet"
            && covered_rs.contains(&(ev.namespace.clone(), ev.name.clone()))
        {
            continue;
        }
        let (kind, severity) = classify_reason(&ev.reason, ev.warning);
        // PodChurn floods the realm view — keep it only in a scoped feed.
        if kind == ChangeKind::PodChurn && matches!(opts.scope, TimelineScope::Cluster) {
            continue;
        }
        let detail = if ev.message.is_empty() {
            ev.reason.clone()
        } else {
            format!("{}: {}", ev.reason, ev.message)
        };
        entries.push(TimelineEntry {
            onset: ev.onset.clone().or_else(|| ev.when.clone()),
            onset_reported: ev.onset.is_some(),
            when: ev.when.clone(),
            kind,
            severity,
            subject: (ev.namespace.clone(), ev.name.clone(), ev.kind.clone()),
            title: if ev.name.is_empty() {
                ev.kind.clone()
            } else {
                ev.name.clone()
            },
            detail,
            revision: None,
            count: ev.count.max(1),
            operator: false,
            key: format!("ev:{}:{}:{}", ev.namespace, ev.name, ev.reason),
        });
    }

    // (d) In-session operator actions.
    for op in ops {
        if !touches(
            &opts.scope,
            &op.namespace,
            &op.name,
            &member_exact,
            &member_prefixes,
        ) {
            continue;
        }
        entries.push(TimelineEntry {
            onset: Some(Time(op.when)),
            onset_reported: true,
            when: Some(Time(op.when)),
            kind: ChangeKind::Operator,
            severity: op.severity,
            subject: (op.namespace.clone(), op.name.clone(), op.kind.clone()),
            title: if op.name.is_empty() {
                op.kind.clone()
            } else {
                op.name.clone()
            },
            detail: op.detail.clone(),
            revision: None,
            count: 1,
            operator: true,
            key: format!(
                "op:{}:{}:{:?}:{}",
                op.namespace,
                op.name,
                op.verb,
                op.when.as_millisecond()
            ),
        });
    }

    // Namespace filter (cluster scope only); node/cluster-scoped entries (no
    // namespace) always stay.
    if matches!(opts.scope, TimelineScope::Cluster) {
        entries.retain(|e| e.subject.0.is_empty() || opts.filter.matches(&e.subject.0));
    }

    // Recency: window event-sourced entries; Deploy (full rollout history) +
    // operator (in-session, sparse) entries are always kept. Untimed entries are
    // kept (can't be windowed) and trail at the end.
    let cutoff = opts.window_min * 60;
    entries.retain(|e| {
        if e.kind == ChangeKind::Deploy || e.operator {
            return true;
        }
        match &e.when {
            None => true,
            // Signed: a future timestamp (clock skew) yields <= cutoff → kept.
            Some(t) => now.duration_since(t.0).as_secs() <= cutoff,
        }
    });

    // Newest first; untimed sink to a deterministic tail; ties: severity then key.
    entries.sort_by(|a, b| match (&a.when, &b.when) {
        (Some(x), Some(y)) => {
            y.0.cmp(&x.0)
                .then(b.severity.cmp(&a.severity))
                .then(a.key.cmp(&b.key))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        // Undated tail: still order Deploy revisions newest-first (a real RS
        // always has a creation_timestamp, but be robust if one is missing).
        (None, None) => match (a.revision, b.revision) {
            (Some(ra), Some(rb)) => rb.cmp(&ra).then(a.key.cmp(&b.key)),
            _ => a.key.cmp(&b.key),
        },
    });

    // The fault line: the earliest ONSET among failures that STARTED recently
    // (computed over the full windowed set, before the cap, so it's correct even
    // if the failure is past the cap).
    //
    // The `anchors` filter runs BEFORE the minimum, and has to. `first_trouble`
    // is a reduction, and the chronic/acute distinction is not recoverable from
    // its result — once the minimum is taken, the entry it came from is gone.
    // Filtering after would mean deciding whether to keep an answer without
    // being able to see what produced it.
    let cutoff = opts.window_min * 60;
    let first_trouble = entries
        .iter()
        .filter(|e| e.severity >= Severity::Warning)
        .filter_map(|e| anchors(e, now, cutoff))
        .min_by(|a, b| a.0.cmp(&b.0));

    let truncated = entries.len() > opts.cap;
    entries.truncate(opts.cap);

    let deployment_only_note =
        matches!(&opts.scope, TimelineScope::Workload(wr) if wr.kind != WorkloadKind::Deployment);

    Timeline {
        entries,
        first_trouble,
        truncated,
        deployment_only_note,
        window_min: opts.window_min,
    }
}

/// Per-entry render decisions for a timeline, shared by the GUI Annals and the
/// postmortem export so the screen and the exported doc can never disagree about
/// the fault line or which change is a suspect. One per entry, capped, in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowDecision {
    /// The "— trouble begins here —" rule lands ABOVE this row (the first row
    /// strictly older than `first_trouble`).
    pub fault_line_above: bool,
    /// A *change* within `CORRELATION_WINDOW_MIN` strictly before the first
    /// failure — "preceded by", never "caused by".
    pub suspect: bool,
}

/// The fault-line + suspect decisions for the first `cap` entries. PURE.
pub fn row_decisions(tl: &Timeline, cap: usize) -> Vec<RowDecision> {
    let ft = tl.first_trouble.as_ref();
    // The fault line: the first shown row strictly older than the first trouble.
    //
    // This comparison deliberately keeps `when` while the suspect test below
    // moved to `onset`. They are different questions: the rule is a POSITION in
    // a list sorted by `when`, so "is this row placed before the incident began"
    // is answered by the sort key. The suspect test is a DURATION, so both its
    // sides must be the same quantity.
    let mut fault_idx: Option<usize> = None;
    if let Some(ftt) = ft {
        for (i, e) in tl.entries.iter().take(cap).enumerate() {
            if matches!(&e.when, Some(t) if t.0 < ftt.0) {
                fault_idx = Some(i);
                break;
            }
        }
    }
    tl.entries
        .iter()
        .take(cap)
        .enumerate()
        .map(|(i, e)| {
            let suspect = e.kind.is_change()
                && ft.is_some_and(|ftt| {
                    // ONSET on this side too. `first_trouble` became an onset in
                    // T-fix and this side was left on `when`, so `d` measured
                    // "from this change's most recent sighting to the incident's
                    // start" — not the quantity the rule claims. Deploy and
                    // operator entries are unaffected (their onset IS their
                    // `when`); a repeating `ScalingReplicaSet` was not, and
                    // correlated from its latest refresh rather than from when
                    // the scaling began.
                    e.onset.as_ref().is_some_and(|w| {
                        let d = ftt.0.duration_since(w.0).as_secs();
                        (correlation_floor(e)..=CORRELATION_WINDOW_MIN * 60).contains(&d)
                    })
                });
            RowDecision {
                fault_line_above: Some(i) == fault_idx,
                suspect,
            }
        })
        .collect()
}

/// The smallest gap at which a change of this kind can be called a precursor.
///
/// The lower bound does **two jobs**, and they want different answers.
///
/// As a *causality* rule it excludes `d == 0`: something simultaneous with a
/// failure did not precede it. As a *resolution* rule it is an artifact —
/// Kubernetes Event timestamps are second-granularity and a kubelet fails an
/// image pull effectively immediately, so a deploy and the `ImagePullBackOff` it
/// caused routinely land in the same second. Measured live across three
/// incidents: 0 s, 1 s, 11 s. Under a flat `1..` the first was silently dropped,
/// which is the canonical incident this whole feature exists to show.
///
/// The asymmetry that resolves it: **a deploy cannot be caused by a failure it
/// precedes.** At `d == 0` an *act* — a rollout, or something the operator did
/// here — is either the cause or unrelated, never the consequence. An
/// event-sourced change has no such direction: a `ScalingReplicaSet` in the same
/// second as a failure is at least as likely to be the failure's effect (a
/// controller reacting), and flagging it would print "preceded by" on something
/// that followed. That phrase is only honest when the ordering is real.
///
/// No new constant: the distinction is one the type already carries.
fn correlation_floor(e: &TimelineEntry) -> i64 {
    match e.kind {
        // Acts, with a known direction.
        ChangeKind::Deploy | ChangeKind::Operator => 0,
        // Observations, which may be consequences.
        _ => 1,
    }
}

/// Whether an event/action about `(ns, name)` belongs to `scope`. `members` is
/// the precomputed precise (namespace, name) roster of the scope's objects (the
/// workload + its ReplicaSets + its pods, or a node's pods) — matched exactly, so
/// a sibling workload sharing a name prefix (`web` vs `web-api`) or a same-named
/// pod in another namespace never leaks in. A node's own events (which carry an
/// empty namespace) match the node name directly.
pub(crate) fn touches(
    scope: &TimelineScope,
    ns: &str,
    name: &str,
    members: &[(String, String)],
    prefixes: &[String],
) -> bool {
    match scope {
        TimelineScope::Cluster => true,
        TimelineScope::Workload(wr) => {
            members.iter().any(|(mns, mn)| mns == ns && mn == name)
                || (ns == wr.namespace && prefixes.iter().any(|p| name.starts_with(p)))
        }
        TimelineScope::Node(node) => {
            name == node.as_str() || members.iter().any(|(mns, mn)| mns == ns && mn == name)
        }
    }
}

/// Map an event reason → (category, severity). Pinned by a regression test
/// against `attention.rs`'s reason vocabulary so the two cannot silently drift.
pub(crate) fn classify_reason(reason: &str, warning: bool) -> (ChangeKind, Severity) {
    use ChangeKind::*;
    use Severity::*;
    match reason {
        "ScalingReplicaSet" | "Scaled" => (Scale, Info),
        "SuccessfulCreate" | "Created" | "Started" | "Pulled" | "Pulling" | "SuccessfulDelete"
        | "Killing" | "Preempting" | "SandboxChanged" => (PodChurn, Info),
        "Scheduled" => (Schedule, Info),
        "FailedScheduling" | "FailedCreate" => (Schedule, Warning),
        // Critical container failures (mirrors attention::Agg::classify).
        "CrashLoopBackOff"
        | "ErrImagePull"
        | "ImagePullBackOff"
        | "InvalidImageName"
        | "CreateContainerConfigError"
        | "CreateContainerError"
        | "RunContainerError"
        | "FailedCreatePodSandBox" => (Failure, Critical),
        // Warning-level failures.
        "BackOff" | "Unhealthy" | "ProbeWarning" | "FailedMount" | "FailedAttachVolume"
        | "NetworkNotReady" | "OOMKilling" | "Evicted" | "Failed" => (Failure, Warning),
        // Node lifecycle.
        "NodeNotReady" | "NodeNotSchedulable" | "Rebooted" => (NodeChange, Warning),
        "NodeReady"
        | "NodeSchedulable"
        | "RegisteredNode"
        | "Starting"
        | "NodeAllocatableEnforced" => (NodeChange, Info),
        // Honest fallback: trust the Warning flag, never drop.
        _ => {
            if warning {
                (Failure, Warning)
            } else {
                (Event, Info)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use crate::state::observed::RecentEvent;
    use crate::state::rollout::REVISION_ANNOTATION;
    use k8s_openapi::api::apps::v1::{ReplicaSetSpec, ReplicaSetStatus};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use std::collections::BTreeMap;

    const ALL: NamespaceFilter = NamespaceFilter::All;

    fn opts(scope: TimelineScope) -> TimelineOpts<'static> {
        TimelineOpts {
            scope,
            filter: &ALL,
            window_min: TIMELINE_WINDOW_MIN,
            cap: CLUSTER_CAP,
        }
    }

    fn now() -> Timestamp {
        // A fixed reference instant for deterministic windowing.
        "2026-06-19T12:00:00Z".parse().unwrap()
    }

    fn ago(now: Timestamp, secs: i64) -> Time {
        Time(now - k8s_openapi::jiff::SignedDuration::from_secs(secs))
    }

    fn push_event(world: &ObservedWorld, ev: RecentEvent) {
        world.events.lock().unwrap().push_back(ev);
    }

    fn ev(
        kind: &str,
        ns: &str,
        name: &str,
        reason: &str,
        warning: bool,
        when: Time,
    ) -> RecentEvent {
        RecentEvent {
            warning,
            reason: reason.into(),
            message: format!("{reason} happened"),
            kind: kind.into(),
            namespace: ns.into(),
            name: name.into(),
            count: 1,
            when: Some(when),
            // Unknown onset: falls back to `when`, i.e. exactly the behaviour
            // that existed before onset was captured. Every test written against
            // the old anchor therefore keeps its meaning.
            onset: None,
        }
    }

    /// An event whose onset is KNOWN and distinct from its latest occurrence —
    /// the shape of a chronic failure: started long ago, seen a moment ago.
    fn ev_onset(
        kind: &str,
        ns: &str,
        name: &str,
        reason: &str,
        warning: bool,
        when: Time,
        onset: Time,
    ) -> RecentEvent {
        RecentEvent {
            onset: Some(onset),
            ..ev(kind, ns, name, reason, warning, when)
        }
    }

    fn rs(
        ns: &str,
        name: &str,
        deploy: &str,
        rev: &str,
        image: &str,
        replicas: i32,
    ) -> k8s_openapi::api::apps::v1::ReplicaSet {
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
    fn classify_reason_table_matches_attention_vocabulary() {
        // The attention queue's Critical reasons must classify Critical here.
        for r in [
            "CrashLoopBackOff",
            "ErrImagePull",
            "ImagePullBackOff",
            "InvalidImageName",
            "CreateContainerConfigError",
            "CreateContainerError",
            "RunContainerError",
        ] {
            assert_eq!(
                classify_reason(r, true),
                (ChangeKind::Failure, Severity::Critical),
                "{r}"
            );
        }
        assert_eq!(
            classify_reason("ScalingReplicaSet", false),
            (ChangeKind::Scale, Severity::Info)
        );
        assert_eq!(
            classify_reason("Started", false),
            (ChangeKind::PodChurn, Severity::Info)
        );
        assert_eq!(
            classify_reason("FailedScheduling", true),
            (ChangeKind::Schedule, Severity::Warning)
        );
        assert_eq!(
            classify_reason("NodeNotReady", true),
            (ChangeKind::NodeChange, Severity::Warning)
        );
        // Unknown reasons: trust the Warning flag, never drop.
        assert_eq!(
            classify_reason("SomethingNew", true),
            (ChangeKind::Failure, Severity::Warning)
        );
        assert_eq!(
            classify_reason("SomethingNew", false),
            (ChangeKind::Event, Severity::Info)
        );
    }

    #[test]
    fn revisions_become_deploy_entries_newest_first_with_delta() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 3, 3));
        s.replicaset(rs("demo", "web-old", "web", "1", "web:1.0", 0));
        s.replicaset(rs("demo", "web-new", "web", "2", "web:1.1", 3));
        let tl = build_timeline(
            &world,
            &opts(TimelineScope::Workload(wr("demo", "web"))),
            &[],
            now(),
        );
        let deploys: Vec<_> = tl
            .entries
            .iter()
            .filter(|e| e.kind == ChangeKind::Deploy)
            .collect();
        assert_eq!(deploys.len(), 2);
        // newest first
        assert_eq!(deploys[0].revision, Some(2));
        assert!(
            deploys[0].detail.contains("web:1.0 -> web:1.1"),
            "{}",
            deploys[0].detail
        );
        assert!(!tl.deployment_only_note);
    }

    #[test]
    fn deploy_suppresses_redundant_rs_pod_churn() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 3, 3));
        s.replicaset(rs("demo", "web-new", "web", "2", "web:1.1", 3));
        // A SuccessfulCreate event on the covered RS must be suppressed...
        push_event(
            &world,
            ev(
                "ReplicaSet",
                "demo",
                "web-new",
                "SuccessfulCreate",
                false,
                ago(now(), 60),
            ),
        );
        // ...but a pod-level Started event survives (different object).
        push_event(
            &world,
            ev(
                "Pod",
                "demo",
                "web-new-abc",
                "Started",
                false,
                ago(now(), 50),
            ),
        );
        let tl = build_timeline(
            &world,
            &opts(TimelineScope::Workload(wr("demo", "web"))),
            &[],
            now(),
        );
        assert!(
            !tl.entries
                .iter()
                .any(|e| e.key == "ev:demo:web-new:SuccessfulCreate"),
            "RS SuccessfulCreate should be folded into the Deploy entry"
        );
        assert!(tl.entries.iter().any(|e| e.title == "web-new-abc"));
    }

    #[test]
    fn noisy_event_carries_count() {
        let (world, _s) = fx::world();
        let mut e = ev("Pod", "demo", "crashy-1", "BackOff", true, ago(now(), 30));
        e.count = 40;
        push_event(&world, e);
        let tl = build_timeline(&world, &opts(TimelineScope::Cluster), &[], now());
        let row = tl.entries.iter().find(|e| e.title == "crashy-1").unwrap();
        assert_eq!(row.count, 40);
        assert_eq!(row.severity, Severity::Warning);
    }

    #[test]
    fn recency_drops_old_events_keeps_operator_and_deploy() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 1, 1));
        // An ancient Deploy revision (created long ago) is still kept.
        s.replicaset(rs("demo", "web-1", "web", "1", "web:1.0", 1));
        // An event older than the window is dropped.
        push_event(
            &world,
            ev(
                "Pod",
                "demo",
                "web-1-x",
                "Unhealthy",
                true,
                ago(now(), 60 * 60),
            ),
        );
        // An in-window event is kept.
        push_event(
            &world,
            ev("Pod", "demo", "web-1-y", "Unhealthy", true, ago(now(), 60)),
        );
        let op = OperatorAction {
            when: now() - k8s_openapi::jiff::SignedDuration::from_secs(60 * 60),
            verb: OpVerb::Scale,
            namespace: "demo".into(),
            name: "web".into(),
            kind: "Deployment".into(),
            detail: "scaled 1→3".into(),
            severity: Severity::Info,
        };
        let tl = build_timeline(
            &world,
            &opts(TimelineScope::Workload(wr("demo", "web"))),
            &[op],
            now(),
        );
        assert!(
            tl.entries.iter().any(|e| e.title == "web-1-y"),
            "in-window event kept"
        );
        assert!(
            !tl.entries.iter().any(|e| e.title == "web-1-x"),
            "old event dropped"
        );
        assert!(
            tl.entries.iter().any(|e| e.kind == ChangeKind::Deploy),
            "old deploy kept"
        );
        assert!(
            tl.entries
                .iter()
                .any(|e| e.operator && e.detail.contains("1→3")),
            "old operator action kept"
        );
    }

    #[test]
    fn none_timestamp_sinks_to_tail_deterministically() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "timed", "Unhealthy", true, ago(now(), 30)),
        );
        let mut undated = ev("Pod", "demo", "undated", "Unhealthy", true, ago(now(), 10));
        undated.when = None;
        push_event(&world, undated);
        let tl = build_timeline(&world, &opts(TimelineScope::Cluster), &[], now());
        // The undated entry trails the timed one regardless of severity.
        let pos_timed = tl.entries.iter().position(|e| e.title == "timed").unwrap();
        let pos_undated = tl
            .entries
            .iter()
            .position(|e| e.title == "undated")
            .unwrap();
        assert!(pos_timed < pos_undated);
        // It is never the fault-line anchor (which requires a timestamp).
        assert_eq!(tl.first_trouble, Some(ago(now(), 30)));
    }

    #[test]
    fn cap_keeps_newest_and_flags_truncated() {
        let (world, _s) = fx::world();
        for i in 0..10 {
            push_event(
                &world,
                ev(
                    "Pod",
                    "demo",
                    &format!("p{i:02}"),
                    "Unhealthy",
                    true,
                    ago(now(), 60 + i),
                ),
            );
        }
        let mut o = opts(TimelineScope::Cluster);
        o.cap = 4;
        let tl = build_timeline(&world, &o, &[], now());
        assert_eq!(tl.entries.len(), 4);
        assert!(tl.truncated);
        // Newest (smallest age) survive: p00..p03.
        assert!(tl.entries.iter().all(|e| e.title.as_str() < "p04"));
    }

    #[test]
    fn workload_scope_uses_touches_and_drops_siblings() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(rs("demo", "web-rs", "web", "1", "web:1", 1));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        push_event(
            &world,
            ev("Pod", "demo", "web-rs-1", "Unhealthy", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev("Pod", "demo", "other-9", "Unhealthy", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev(
                "Pod",
                "elsewhere",
                "web-z",
                "Unhealthy",
                true,
                ago(now(), 30),
            ),
        );
        // A name-PREFIX sibling: a different deployment "web-api" with its own pod
        // — its event must NOT leak into "web" (the old prefix match would have).
        s.deployment(fx::deployment("demo", "web-api", 1, 1));
        s.replicaset(rs("demo", "web-api-rs", "web-api", "1", "x:1", 1));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-api-rs-1", Some("n1")),
            "ReplicaSet",
            "web-api-rs",
        ));
        push_event(
            &world,
            ev(
                "Pod",
                "demo",
                "web-api-rs-1",
                "Unhealthy",
                true,
                ago(now(), 30),
            ),
        );
        let tl = build_timeline(
            &world,
            &opts(TimelineScope::Workload(wr("demo", "web"))),
            &[],
            now(),
        );
        assert!(
            tl.entries.iter().any(|e| e.title == "web-rs-1"),
            "own pod kept"
        );
        assert!(
            !tl.entries.iter().any(|e| e.title == "other-9"),
            "sibling dropped"
        );
        assert!(
            !tl.entries.iter().any(|e| e.title == "web-z"),
            "other-ns dropped"
        );
        assert!(
            !tl.entries.iter().any(|e| e.title == "web-api-rs-1"),
            "name-prefix sibling deployment's pod must not leak in"
        );
    }

    #[test]
    fn node_scope_keeps_node_event_and_stationed_pods() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.pod(fx::pod("demo", "podA", Some("n1")));
        s.pod(fx::pod("demo", "podB", Some("n2")));
        push_event(
            &world,
            ev("Node", "", "n1", "NodeNotReady", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev("Pod", "demo", "podA", "Unhealthy", true, ago(now(), 20)),
        );
        push_event(
            &world,
            ev("Pod", "demo", "podB", "Unhealthy", true, ago(now(), 20)),
        );
        // A same-NAMED pod in another namespace (not on n1) must NOT match — the
        // touches roster keys on (namespace, name), not the bare name.
        push_event(
            &world,
            ev("Pod", "other", "podA", "Unhealthy", true, ago(now(), 20)),
        );
        let tl = build_timeline(&world, &opts(TimelineScope::Node("n1".into())), &[], now());
        assert!(
            tl.entries
                .iter()
                .any(|e| e.title == "n1" && e.kind == ChangeKind::NodeChange)
        );
        assert!(
            tl.entries
                .iter()
                .any(|e| e.title == "podA" && e.subject.0 == "demo"),
            "pod on n1 kept"
        );
        assert!(
            !tl.entries.iter().any(|e| e.title == "podB"),
            "pod on n2 dropped"
        );
        assert!(
            !tl.entries
                .iter()
                .any(|e| e.title == "podA" && e.subject.0 == "other"),
            "same-named pod in another namespace dropped"
        );
    }

    #[test]
    fn namespace_filter_scopes_cluster_feed_but_keeps_node_entries() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "keep", "Unhealthy", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev("Pod", "other", "drop", "Unhealthy", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev("Node", "", "n1", "NodeNotReady", true, ago(now(), 30)),
        );
        let only: NamespaceFilter =
            NamespaceFilter::Only(["demo".to_string()].into_iter().collect());
        let o = TimelineOpts {
            scope: TimelineScope::Cluster,
            filter: &only,
            window_min: TIMELINE_WINDOW_MIN,
            cap: CLUSTER_CAP,
        };
        let tl = build_timeline(&world, &o, &[], now());
        assert!(tl.entries.iter().any(|e| e.title == "keep"));
        assert!(!tl.entries.iter().any(|e| e.title == "drop"));
        assert!(
            tl.entries.iter().any(|e| e.title == "n1"),
            "node entry (no ns) kept"
        );
    }

    #[test]
    fn cluster_scope_drops_podchurn_subject_keeps_it() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "web-1-x", "Started", false, ago(now(), 30)),
        );
        let cluster = build_timeline(&world, &opts(TimelineScope::Cluster), &[], now());
        assert!(
            !cluster
                .entries
                .iter()
                .any(|e| e.kind == ChangeKind::PodChurn),
            "realm view hides churn"
        );
        // In a scope that touches it, churn is shown.
        let scoped = build_timeline(
            &world,
            &opts(TimelineScope::Workload(wr("demo", "web"))),
            &[],
            now(),
        );
        assert!(
            scoped
                .entries
                .iter()
                .any(|e| e.kind == ChangeKind::PodChurn && e.title == "web-1-x")
        );
    }

    #[test]
    fn operator_actions_merge_sort_and_attribute() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "web-1", "Unhealthy", true, ago(now(), 120)),
        );
        let ops = vec![OperatorAction {
            when: now() - k8s_openapi::jiff::SignedDuration::from_secs(30),
            verb: OpVerb::Evict,
            namespace: "demo".into(),
            name: "web-1".into(),
            kind: "Pod".into(),
            detail: "evicted web-1".into(),
            severity: Severity::Warning,
        }];
        let tl = build_timeline(&world, &opts(TimelineScope::Cluster), &ops, now());
        // The newer operator action sorts above the older event.
        assert!(tl.entries[0].operator && tl.entries[0].detail.contains("evicted"));
        assert_eq!(tl.entries[0].kind, ChangeKind::Operator);
    }

    #[test]
    fn first_trouble_is_earliest_in_window_warning() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "later", "Unhealthy", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev(
                "Pod",
                "demo",
                "earliest",
                "CrashLoopBackOff",
                true,
                ago(now(), 120),
            ),
        );
        push_event(
            &world,
            ev("Pod", "demo", "benign", "Started", false, ago(now(), 300)),
        );
        let tl = build_timeline(&world, &opts(TimelineScope::Cluster), &[], now());
        assert_eq!(tl.first_trouble, Some(ago(now(), 120)));
    }

    #[test]
    fn non_deployment_subject_sets_note_no_deploys() {
        let (world, mut s) = fx::world();
        s.statefulset(fx::statefulset("demo", "db", 1, 1));
        push_event(
            &world,
            ev("Pod", "demo", "db-0", "Unhealthy", true, ago(now(), 30)),
        );
        let sts = WorkloadRef {
            kind: WorkloadKind::StatefulSet,
            namespace: "demo".into(),
            name: "db".into(),
        };
        let tl = build_timeline(&world, &opts(TimelineScope::Workload(sts)), &[], now());
        assert!(tl.deployment_only_note);
        assert!(!tl.entries.iter().any(|e| e.kind == ChangeKind::Deploy));
    }

    #[test]
    fn empty_world_is_clean() {
        let (world, _s) = fx::world();
        let tl = build_timeline(&world, &opts(TimelineScope::Cluster), &[], now());
        assert!(tl.entries.is_empty());
        assert!(tl.first_trouble.is_none());
        assert!(!tl.truncated);
    }
    // --- onset-aware fault lines (T-fix) ---------------------------------
    //
    // The defect these pin: `first_trouble` was a minimum over each entry's
    // LATEST occurrence, and the event ring refreshes that, so a failure running
    // for days looked like it had just started and won the anchor. Measured on
    // kind, one controlled variable: chronic crash-looper running -> 0 suspects;
    // scaled to zero -> 2.

    const DAYS_4: i64 = 4 * 24 * 3600;

    /// The T-pre scenario, as a unit test: both present, the acute one wins.
    #[test]
    fn a_chronic_failure_does_not_anchor_and_an_acute_one_does() {
        let (world, _s) = fx::world();
        // Started four days ago, last seen 6 minutes ago — a mature
        // crash-looper's cadence. Under the old rule this anchored.
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "old",
                "BackOff",
                true,
                ago(now(), 360),
                ago(now(), DAYS_4),
            ),
        );
        // Started 45 s ago.
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "new",
                "Failed",
                true,
                ago(now(), 30),
                ago(now(), 45),
            ),
        );
        let tl = cluster_tl(&world);
        assert_eq!(
            tl.first_trouble.as_ref().map(|t| t.0),
            Some(ago(now(), 45).0),
            "the acute onset anchors, not the chronic entry's refreshed sighting",
        );
    }

    /// With ONLY a chronic failure there is no anchor at all — the honest
    /// answer. Nothing started recently, so nothing marks where it began.
    #[test]
    fn an_all_chronic_scope_has_no_fault_line_rather_than_a_wrong_one() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "old",
                "BackOff",
                true,
                ago(now(), 60),
                ago(now(), DAYS_4),
            ),
        );
        let tl = cluster_tl(&world);
        assert_eq!(tl.first_trouble, None);
        assert!(
            row_decisions(&tl, CLUSTER_CAP)
                .iter()
                .all(|r| !r.fault_line_above),
            "no anchor means no rule drawn, not a rule drawn somewhere arbitrary",
        );
    }

    /// THE GATE, as a unit test — at BOTH scopes.
    ///
    /// A deploy 1–10 min before an acute failure is a suspect even while a
    /// chronic failure is present. The subject-scope arm is the §1.1 probe that
    /// revision 1 of the guidance asserted could not fail; it failed, and is why
    /// revision 1 was stopped.
    #[test]
    fn a_change_before_an_acute_failure_is_a_suspect_despite_chronic_noise() {
        for subject_scope in [false, true] {
            let (world, _s) = fx::world();
            push_event(
                &world,
                ev_onset(
                    "Pod",
                    "demo",
                    "web-old-abc",
                    "BackOff",
                    true,
                    ago(now(), 360),
                    ago(now(), DAYS_4),
                ),
            );
            push_event(
                &world,
                ev_onset(
                    "Pod",
                    "demo",
                    "web-new-xyz",
                    "Failed",
                    true,
                    ago(now(), 30),
                    ago(now(), 45),
                ),
            );
            // 105 s before the acute onset — inside the correlation window.
            push_event(
                &world,
                ev(
                    "Deployment",
                    "demo",
                    "web",
                    "ScalingReplicaSet",
                    false,
                    ago(now(), 150),
                ),
            );
            let scope = if subject_scope {
                TimelineScope::Workload(WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: "demo".into(),
                    name: "web".into(),
                })
            } else {
                TimelineScope::Cluster
            };
            let tl = build_timeline(
                &world,
                &TimelineOpts {
                    scope,
                    filter: &NamespaceFilter::All,
                    window_min: TIMELINE_WINDOW_MIN,
                    cap: CLUSTER_CAP,
                },
                &[],
                now(),
            );
            let suspects = row_decisions(&tl, CLUSTER_CAP)
                .iter()
                .filter(|r| r.suspect)
                .count();
            assert!(
                suspects > 0,
                "subject_scope={subject_scope}: the change before the acute \
                 failure must be flagged even with a chronic entry present",
            );
        }
    }

    /// Subject scope with no chronic failure of its own is UNCHANGED — the case
    /// T-pre observed live and mistook for a property of the scope.
    #[test]
    fn a_subject_without_chronic_noise_anchors_exactly_as_before() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "web-new-xyz", "Failed", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev(
                "Deployment",
                "demo",
                "web",
                "ScalingReplicaSet",
                false,
                ago(now(), 150),
            ),
        );
        let tl = build_timeline(
            &world,
            &TimelineOpts {
                scope: TimelineScope::Workload(WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: "demo".into(),
                    name: "web".into(),
                }),
                filter: &NamespaceFilter::All,
                window_min: TIMELINE_WINDOW_MIN,
                cap: CLUSTER_CAP,
            },
            &[],
            now(),
        );
        // Unknown onset falls back to `when`, so this is the old anchor exactly.
        assert_eq!(
            tl.first_trouble.as_ref().map(|t| t.0),
            Some(ago(now(), 30).0)
        );
        assert!(row_decisions(&tl, CLUSTER_CAP).iter().any(|r| r.suspect));
    }

    /// Unknown onset is recorded, not silently collapsed — and behaves as it did
    /// before the field existed.
    #[test]
    fn absent_onset_falls_back_to_when_and_says_that_it_did() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev("Pod", "demo", "quiet", "Failed", true, ago(now(), 30)),
        );
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "loud",
                "BackOff",
                true,
                ago(now(), 60),
                ago(now(), 90),
            ),
        );
        let tl = cluster_tl(&world);
        let quiet = tl.entries.iter().find(|e| e.title == "quiet").unwrap();
        let loud = tl.entries.iter().find(|e| e.title == "loud").unwrap();
        assert!(!quiet.onset_reported, "nothing reported this one's onset");
        assert_eq!(quiet.onset, quiet.when, "so it stands in for itself");
        assert!(loud.onset_reported);
        assert_ne!(
            loud.onset, loud.when,
            "a real onset is distinct from last-seen"
        );
    }

    /// Clock skew: an onset in the future must not be treated as impossibly old
    /// and dropped. Follows the convention the recency filter already set.
    #[test]
    fn an_onset_in_the_future_still_anchors() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "skewed",
                "Failed",
                true,
                ago(now(), 30),
                ago(now(), -120),
            ),
        );
        let tl = cluster_tl(&world);
        assert!(
            tl.first_trouble.is_some(),
            "a future onset is kept, like a future `when` in the recency filter",
        );
    }

    fn cluster_tl(world: &ObservedWorld) -> Timeline {
        build_timeline(
            world,
            &TimelineOpts {
                scope: TimelineScope::Cluster,
                filter: &NamespaceFilter::All,
                window_min: TIMELINE_WINDOW_MIN,
                cap: CLUSTER_CAP,
            },
            &[],
            now(),
        )
    }
    // --- the correlation rule (T-fix-2) ----------------------------------
    //
    // Two defects in three lines. (A) T-fix moved `first_trouble` to onset and
    // left the change side on `when`, so the comparison measured two different
    // quantities. (B) the `1..` floor did causality AND resolution work, and in
    // the second role it dropped the canonical incident: a deploy and the
    // ImagePullBackOff it caused, in the same second (measured live).

    /// Build a one-failure/one-change timeline and report whether the change is
    /// a suspect. `gap` is seconds from the change to the failure's ONSET.
    fn correlates(change: RecentEvent, gap_from: Time, ft_onset: Time) -> bool {
        let (world, _s) = fx::world();
        push_event(&world, change);
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "victim",
                "Failed",
                true,
                ago(now(), 5),
                ft_onset,
            ),
        );
        let _ = gap_from;
        let tl = build_timeline(
            &world,
            &TimelineOpts {
                scope: TimelineScope::Cluster,
                filter: &NamespaceFilter::All,
                window_min: TIMELINE_WINDOW_MIN,
                cap: CLUSTER_CAP,
            },
            &[],
            now(),
        );
        row_decisions(&tl, CLUSTER_CAP).iter().any(|r| r.suspect)
    }

    /// DEFECT A: a repeating change correlates from when it STARTED, not from
    /// its most recent sighting.
    ///
    /// A `ScalingReplicaSet` that began 5 minutes before the failure but was
    /// last seen a minute *after* it is a precursor. Under the old comparison
    /// its latest sighting was used, which put it on the wrong side entirely.
    #[test]
    fn a_repeating_change_correlates_from_its_onset() {
        let failure_onset = ago(now(), 300);
        let repeating = ev_onset(
            "Deployment",
            "demo",
            "web",
            "ScalingReplicaSet",
            false,
            ago(now(), 60),  // last seen AFTER the failure began
            ago(now(), 420), // but it started 2 min before it
        );
        assert!(
            correlates(repeating, ago(now(), 420), failure_onset),
            "the scaling began before the failure, so it is a precursor",
        );
    }

    /// DEFECT A's guard: the Deploy path must be untouched. A revision's `when`
    /// IS its onset (an RS is created once), so this change cannot move it.
    #[test]
    fn a_deploys_correlation_is_unchanged_because_its_onset_is_its_when() {
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "victim",
                "Failed",
                true,
                ago(now(), 5),
                ago(now(), 120),
            ),
        );
        let tl = build_timeline(
            &world,
            &TimelineOpts {
                scope: TimelineScope::Cluster,
                filter: &NamespaceFilter::All,
                window_min: TIMELINE_WINDOW_MIN,
                cap: CLUSTER_CAP,
            },
            &[],
            now(),
        );
        for e in tl.entries.iter().filter(|e| e.kind == ChangeKind::Deploy) {
            assert_eq!(
                e.onset, e.when,
                "a Deploy's onset and `when` are the same instant, so switching \
                 the comparison to onset cannot change its correlation",
            );
        }
    }

    /// DEFECT B: an act at `d == 0` IS a precursor; an observation is not.
    ///
    /// The live measurement as a unit test — an RS created at 22:58:44Z and its
    /// ImagePullBackOff onset at 22:58:44Z, which a flat `1..` floor dropped.
    #[test]
    fn a_same_second_act_correlates_but_a_same_second_observation_does_not() {
        let at = ago(now(), 120);

        // An operator action in the same second as the failure's onset.
        let (world, _s) = fx::world();
        push_event(
            &world,
            ev_onset(
                "Pod",
                "demo",
                "victim",
                "Failed",
                true,
                ago(now(), 5),
                at.clone(),
            ),
        );
        let op = OperatorAction {
            when: at.0,
            verb: OpVerb::SetImage,
            kind: "Deployment".into(),
            namespace: "demo".into(),
            name: "web".into(),
            detail: "set image".into(),
            severity: Severity::Info,
        };
        let tl = build_timeline(
            &world,
            &TimelineOpts {
                scope: TimelineScope::Cluster,
                filter: &NamespaceFilter::All,
                window_min: TIMELINE_WINDOW_MIN,
                cap: CLUSTER_CAP,
            },
            &[op],
            now(),
        );
        assert!(
            row_decisions(&tl, CLUSTER_CAP)
                .iter()
                .zip(&tl.entries)
                .any(|(r, e)| r.suspect && e.kind == ChangeKind::Operator),
            "an act cannot be the consequence of the failure it coincides with",
        );

        // An event-sourced change in the same second stays ambiguous.
        let churn = ev_onset(
            "Deployment",
            "demo",
            "web",
            "ScalingReplicaSet",
            false,
            at.clone(),
            at.clone(),
        );
        assert!(
            !correlates(churn, at.clone(), at),
            "a controller reacting in the same second may be the EFFECT",
        );
    }

    /// The window's edges, which no test pinned before.
    #[test]
    fn the_correlation_window_boundaries_hold() {
        let ft = ago(now(), 60);
        let at = |secs_before: i64| {
            ev_onset(
                "Deployment",
                "demo",
                "web",
                "ScalingReplicaSet",
                false,
                ago(now(), 60 + secs_before),
                ago(now(), 60 + secs_before),
            )
        };
        assert!(correlates(at(1), ago(now(), 61), ft.clone()), "d = 1");
        assert!(correlates(at(600), ago(now(), 660), ft.clone()), "d = 600");
        assert!(
            !correlates(at(601), ago(now(), 661), ft),
            "d = 601 is too old"
        );
    }
}
