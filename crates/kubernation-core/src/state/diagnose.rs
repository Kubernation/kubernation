//! Pod-not-Ready explainer — the "last mile" of the attention queue.
//!
//! The queue says a *city is in trouble*; this turns the raw Kubernetes reason a
//! pod carries (`CrashLoopBackOff`, `ImagePullBackOff`, `Unschedulable`, …) into
//! a plain-English **explanation** and a **next action** the operator can take —
//! ideally an in-app verb (tail the previous container, inspect the YAML, check
//! requests). Pure + unit-tested: it maps `(reason, restarts, oom)` to a
//! `Diagnosis`, no I/O.

/// What class of problem a not-ready pod has (for grouping / an icon later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagKind {
    /// Container keeps crashing and restarting (CrashLoopBackOff).
    Crash,
    /// Killed for exceeding its memory limit (OOMKilled).
    Oom,
    /// The image couldn't be pulled or is malformed.
    ImagePull,
    /// A referenced ConfigMap/Secret/key is missing, or the container can't start.
    Config,
    /// No node can fit the pod (resources / taints / affinity).
    Unschedulable,
    /// Running but failing its readiness probe (gets no traffic).
    NotReady,
    /// Waiting to start (scheduling / image pull / volume mount).
    Pending,
    /// The pod failed outright.
    Failed,
}

/// A plain-English diagnosis of why a pod isn't healthy, plus the next action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    pub kind: DiagKind,
    /// The raw Kubernetes reason (e.g. "CrashLoopBackOff") — kept greppable.
    pub reason: String,
    /// What it means, in one sentence.
    pub explain: String,
    /// The concrete next step (an in-app verb where one fits).
    pub hint: String,
}

/// Diagnose a pod from the fields the view models already carry. Returns `None`
/// for a healthy / succeeded / terminating pod (nothing to explain). `reason` is
/// `model::pod_state`'s short reason; `oom` is `model::pod_oom_killed`.
pub fn diagnose(reason: &str, restarts: i32, oom: bool) -> Option<Diagnosis> {
    let d = |kind, explain: &str, hint: &str| {
        Some(Diagnosis {
            kind,
            reason: reason.to_string(),
            explain: explain.into(),
            hint: hint.into(),
        })
    };
    // OOM dominates: a CrashLoopBackOff whose last exit was OOMKilled is really a
    // memory problem, and that's the more actionable story.
    if oom {
        return Some(Diagnosis {
            kind: DiagKind::Oom,
            reason: if reason == "Running" || reason.is_empty() {
                "OOMKilled".into()
            } else {
                reason.to_string()
            },
            explain: "a container was killed for exceeding its memory limit (OOMKilled)".into(),
            hint: "raise the memory limit or fix the leak; tail the previous container (p)".into(),
        });
    }
    match reason {
        "CrashLoopBackOff" => {
            let flap = if restarts >= 5 {
                format!(
                    "the container keeps crashing and restarting ({restarts} restarts); Kubernetes is backing off"
                )
            } else {
                "the container starts then exits, so Kubernetes is backing off restarts".into()
            };
            Some(Diagnosis {
                kind: DiagKind::Crash,
                reason: reason.to_string(),
                explain: flap,
                hint:
                    "tail the previous container (p) for the crash; check the command + exit code"
                        .into(),
            })
        }
        "ImagePullBackOff" | "ErrImagePull" => d(
            DiagKind::ImagePull,
            "the image couldn't be pulled — wrong tag/repo, or the registry needs credentials",
            "check the image ref and imagePullSecrets (y to inspect the spec)",
        ),
        "InvalidImageName" => d(
            DiagKind::ImagePull,
            "the image reference is malformed",
            "fix the image string in the workload spec (y to inspect)",
        ),
        "CreateContainerConfigError" => d(
            DiagKind::Config,
            "a referenced ConfigMap/Secret or key is missing",
            "check the env/volume references — the named ConfigMap/Secret may not exist",
        ),
        "CreateContainerError" | "RunContainerError" => d(
            DiagKind::Config,
            "the container couldn't be created or started (bad command, mount, or runtime error)",
            "check the command/args and volume mounts; see the chronicle/events",
        ),
        "Unschedulable" => d(
            DiagKind::Unschedulable,
            "no node can fit this pod — insufficient cpu/memory, or taints/affinity exclude every node",
            "lower requests, free capacity, or check node taints + the pod's affinity",
        ),
        "NotReady" => d(
            DiagKind::NotReady,
            "the container is running but failing its readiness probe, so it receives no traffic",
            "check the readiness probe + the app's health endpoint; tail its logs",
        ),
        "Running" | "Succeeded" | "Terminating" => None,
        "Pending" | "ContainerCreating" | "PodInitializing" => d(
            DiagKind::Pending,
            "the pod is waiting to start (scheduling, image pull, or volume mount)",
            "check the chronicle/events; if it lingers, inspect the cause",
        ),
        // Anything else (Failed / Error / a less-common waiting reason) — surface
        // it as a generic failure rather than hiding it.
        _ => d(
            DiagKind::Failed,
            "the pod is not healthy",
            "tail the previous container (p) and check the chronicle/events",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_pods_have_no_diagnosis() {
        assert!(diagnose("Running", 0, false).is_none());
        assert!(diagnose("Succeeded", 0, false).is_none());
        assert!(diagnose("Terminating", 0, false).is_none());
    }

    #[test]
    fn crashloop_points_at_the_previous_container() {
        let d = diagnose("CrashLoopBackOff", 2, false).unwrap();
        assert_eq!(d.kind, DiagKind::Crash);
        assert!(d.hint.contains("previous container"));
        // A high restart count is called out.
        let flap = diagnose("CrashLoopBackOff", 7, false).unwrap();
        assert!(flap.explain.contains("7 restarts"));
    }

    #[test]
    fn oom_overrides_a_crashloop_reason() {
        // A crash-looping pod whose last exit was OOM is diagnosed as a memory
        // problem, not a generic crash.
        let d = diagnose("CrashLoopBackOff", 3, true).unwrap();
        assert_eq!(d.kind, DiagKind::Oom);
        assert!(d.explain.contains("memory limit"));
    }

    #[test]
    fn image_config_and_schedule_reasons_map() {
        assert_eq!(
            diagnose("ImagePullBackOff", 0, false).unwrap().kind,
            DiagKind::ImagePull
        );
        assert_eq!(
            diagnose("ErrImagePull", 0, false).unwrap().kind,
            DiagKind::ImagePull
        );
        assert_eq!(
            diagnose("CreateContainerConfigError", 0, false)
                .unwrap()
                .kind,
            DiagKind::Config
        );
        assert_eq!(
            diagnose("Unschedulable", 0, false).unwrap().kind,
            DiagKind::Unschedulable
        );
        assert_eq!(
            diagnose("NotReady", 0, false).unwrap().kind,
            DiagKind::NotReady
        );
    }

    #[test]
    fn unknown_reasons_get_a_generic_failure_not_silence() {
        // An unrecognized non-healthy reason still produces a diagnosis.
        let d = diagnose("SomeNewReason", 0, false).unwrap();
        assert_eq!(d.kind, DiagKind::Failed);
        assert!(!d.hint.is_empty());
    }
}
