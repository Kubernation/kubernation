//! Pod QoS class — the kubelet's **eviction order** under node pressure.
//!
//! Shared home so the map and the right-sizing advisor cannot disagree about
//! what a class means (the `workloads_on_node` / `resolve_region` convention:
//! one authority rather than a copy per consumer).
//!
//! **Two entry points, answering two different questions.** They are not
//! interchangeable and the difference is load-bearing:
//!
//! | Question | Use | Accuracy |
//! |---|---|---|
//! | What class is *this pod*? | [`pod_qos`] | Authoritative |
//! | What class do *these totals* look like? | [`qos_from_totals`] | Approximation |
//!
//! [`qos_from_totals`] exists only because the advisor asks its question at
//! *workload* granularity, where there is no single pod object to consult — it
//! has already summed per-replica requests and limits across containers. Summing
//! first and comparing after cannot reproduce the kubelet's rule, which is
//! per-container (see [`pod_qos`]). Anything holding an actual `Pod` must call
//! [`pod_qos`].

use k8s_openapi::api::core::v1::{Container, Pod};

use crate::k8s::quantity;

/// The three Kubernetes QoS classes. There are exactly three — "fully specified
/// but unequal" and "partially specified" both fall under Burstable. A finer
/// subdivision of Burstable is a useful *derivation on top of* QoS, but must
/// never be labelled QoS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QosClass {
    /// Every container has cpu and memory limits, and requests equal them.
    /// Evicted last.
    Guaranteed,
    /// Anything in between. Evicted after BestEffort.
    #[default]
    Burstable,
    /// No container sets any request or limit. Evicted first.
    BestEffort,
}

impl QosClass {
    pub fn label(self) -> &'static str {
        match self {
            QosClass::Guaranteed => "guaranteed",
            QosClass::Burstable => "burstable",
            QosClass::BestEffort => "besteffort",
        }
    }

    /// Parse the API server's `status.qosClass` string. `None` for an
    /// unrecognised value — we do not guess, because a wrong class misstates
    /// eviction order.
    fn parse(s: &str) -> Option<Self> {
        match s {
            "Guaranteed" => Some(QosClass::Guaranteed),
            "Burstable" => Some(QosClass::Burstable),
            "BestEffort" => Some(QosClass::BestEffort),
            _ => None,
        }
    }
}

/// PURE: the authoritative QoS class of one pod.
///
/// **Prefers `status.qosClass`**, which the API server assigns at admission and
/// which is what the kubelet actually acts on. Falls back to computing the same
/// rule from the spec only when that field is absent (hand-built objects and
/// fixtures; every pod from a live cluster carries it).
///
/// The rule is **per-container, not per-pod-total**, and that distinction is why
/// this function exists rather than reusing [`qos_from_totals`]. A pod with one
/// fully-specified container (`requests == limits`) beside a sidecar that
/// specifies nothing is **Burstable** — verified against a live API server —
/// whereas summing first makes the totals look equal and reports Guaranteed.
/// That shape (an unspecified logging or proxy sidecar) is common, so the
/// difference is reachable, not theoretical.
pub fn pod_qos(pod: &Pod) -> QosClass {
    if let Some(cls) = pod
        .status
        .as_ref()
        .and_then(|s| s.qos_class.as_deref())
        .and_then(QosClass::parse)
    {
        return cls;
    }
    qos_from_spec(pod)
}

/// The kubelet's own algorithm, reimplemented for the fallback path.
///
/// Mirrors upstream `GetPodQOS`: **all** containers count — regular and init
/// alike, not only the native sidecars that [`crate::state::model::sum_pod_requests`]
/// counts — and a container missing *either* a cpu or a memory limit disqualifies
/// the whole pod from Guaranteed.
fn qos_from_spec(pod: &Pod) -> QosClass {
    let Some(spec) = pod.spec.as_ref() else {
        return QosClass::BestEffort;
    };
    // Upstream visits regular and init containers alike for QoS purposes.
    let all = spec
        .containers
        .iter()
        .chain(spec.init_containers.iter().flatten());

    let (mut req_cpu, mut req_mem) = (0.0_f64, 0.0_f64);
    let (mut lim_cpu, mut lim_mem) = (0.0_f64, 0.0_f64);
    let (mut any_req, mut any_lim) = (false, false);
    let mut guaranteed = true;

    let get = |c: &Container, which: &str, key: &str| -> f64 {
        c.resources
            .as_ref()
            .and_then(|r| match which {
                "req" => r.requests.as_ref(),
                _ => r.limits.as_ref(),
            })
            .and_then(|m| m.get(key))
            .and_then(quantity::value)
            .unwrap_or(0.0)
    };

    for c in all {
        let (rc, rm) = (get(c, "req", "cpu"), get(c, "req", "memory"));
        let (lc, lm) = (get(c, "lim", "cpu"), get(c, "lim", "memory"));
        if rc > 0.0 {
            req_cpu += rc;
            any_req = true;
        }
        if rm > 0.0 {
            req_mem += rm;
            any_req = true;
        }
        if lc > 0.0 {
            lim_cpu += lc;
            any_lim = true;
        }
        if lm > 0.0 {
            lim_mem += lm;
            any_lim = true;
        }
        // THE per-container rule: this container must cap BOTH resources, or
        // no pod containing it can be Guaranteed however the totals look.
        if !(lc > 0.0 && lm > 0.0) {
            guaranteed = false;
        }
    }

    if !any_req && !any_lim {
        return QosClass::BestEffort;
    }
    if guaranteed && eq(req_cpu, lim_cpu) && eq(req_mem, lim_mem) {
        return QosClass::Guaranteed;
    }
    QosClass::Burstable
}

/// Relative tolerance — an absolute epsilon is meaningless at byte scale.
fn eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

/// PURE: the class a set of **already-summed** cpu/mem totals looks like.
///
/// Promoted verbatim from the right-sizing advisor, which is its only caller and
/// which asks at workload granularity — per-replica totals across every
/// container, with no pod object available. Behaviour is unchanged so the
/// advisor's output does not move.
///
/// **This is an approximation and must not be used where a `Pod` is in hand.**
/// Summing before comparing cannot see the per-container rule, so it over-reports
/// Guaranteed for a pod whose containers are unevenly specified — call
/// [`pod_qos`] instead. Making the advisor per-container-exact needs its pod
/// template rather than its totals, which changes advisor output and belongs in
/// its own change.
pub fn qos_from_totals(req_cpu: f64, lim_cpu: f64, req_mem: f64, lim_mem: f64) -> QosClass {
    if req_cpu == 0.0 && req_mem == 0.0 && lim_cpu == 0.0 && lim_mem == 0.0 {
        return QosClass::BestEffort;
    }
    if req_cpu > 0.0 && req_mem > 0.0 && eq(req_cpu, lim_cpu) && eq(req_mem, lim_mem) {
        return QosClass::Guaranteed;
    }
    QosClass::Burstable
}

#[cfg(all(test, feature = "fixtures"))]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use k8s_openapi::api::core::v1::{Container, ResourceRequirements};

    const MI: f64 = 1024.0 * 1024.0;

    fn container(name: &str, req: Option<(&str, &str)>, lim: Option<(&str, &str)>) -> Container {
        Container {
            name: name.into(),
            resources: Some(ResourceRequirements {
                requests: req.map(|(c, m)| fx::quantities(&[("cpu", c), ("memory", m)])),
                limits: lim.map(|(c, m)| fx::quantities(&[("cpu", c), ("memory", m)])),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn pod_with(containers: Vec<Container>) -> Pod {
        let mut p = fx::pod("ns", "p", Some("n1"));
        p.spec.as_mut().unwrap().containers = containers;
        p
    }

    #[test]
    fn all_three_classes_derive_from_the_spec() {
        assert_eq!(
            pod_qos(&pod_with(vec![container(
                "c",
                Some(("100m", "64Mi")),
                Some(("100m", "64Mi"))
            )])),
            QosClass::Guaranteed
        );
        assert_eq!(
            pod_qos(&pod_with(vec![container(
                "c",
                Some(("100m", "64Mi")),
                None
            )])),
            QosClass::Burstable,
            "requests without limits is Burstable"
        );
        assert_eq!(
            pod_qos(&pod_with(vec![container("c", None, None)])),
            QosClass::BestEffort
        );
    }

    /// The reason [`pod_qos`] exists. Verified against a live API server: one
    /// fully-specified container beside an unspecified sidecar is Burstable,
    /// because the sidecar caps neither resource — even though the pod's TOTALS
    /// have requests equal to limits.
    #[test]
    fn an_unspecified_sidecar_disqualifies_guaranteed() {
        let p = pod_with(vec![
            container("app", Some(("100m", "64Mi")), Some(("100m", "64Mi"))),
            container("sidecar", None, None),
        ]);
        assert_eq!(
            pod_qos(&p),
            QosClass::Burstable,
            "the kubelet's per-container rule"
        );
        // ...and this is precisely where the totals-based view goes wrong. Pinned
        // so nobody "simplifies" pod_qos into qos_from_totals.
        assert_eq!(
            qos_from_totals(0.1, 0.1, 64.0 * MI, 64.0 * MI),
            QosClass::Guaranteed,
            "summing first hides the per-container rule — the documented limit"
        );
    }

    /// Init containers count for QoS (upstream visits them), unlike the resource
    /// sums, which count only native sidecars.
    #[test]
    fn a_plain_init_container_also_disqualifies_guaranteed() {
        let mut p = pod_with(vec![container(
            "app",
            Some(("100m", "64Mi")),
            Some(("100m", "64Mi")),
        )]);
        p.spec.as_mut().unwrap().init_containers = Some(vec![container("setup", None, None)]);
        assert_eq!(pod_qos(&p), QosClass::Burstable);
    }

    /// `status.qosClass` is what the kubelet acts on, so it wins over any
    /// derivation. A live pod always carries it.
    #[test]
    fn the_api_servers_own_class_wins_over_derivation() {
        let mut p = pod_with(vec![container("c", None, None)]); // would derive BestEffort
        p.status.as_mut().unwrap().qos_class = Some("Guaranteed".into());
        assert_eq!(pod_qos(&p), QosClass::Guaranteed);

        // An unrecognised value is not guessed at — fall back to the spec.
        p.status.as_mut().unwrap().qos_class = Some("Sideways".into());
        assert_eq!(pod_qos(&p), QosClass::BestEffort);
    }

    /// The tolerance is LOAD-BEARING, and this pins it with operands that are
    /// genuinely unequal in f64 rather than bit-identical ones (which would
    /// reduce the comparison to `==` and test nothing).
    ///
    /// The shape is real and common: a container authored `cpu: "0.7"` against
    /// `cpu: "700m"` — one quantity, two spellings — parses to 0.7 and
    /// 0.7000000000000001. Kubernetes calls that pod Guaranteed; exact equality
    /// would call it Burstable, drawing timber where the kubelet evicts stone.
    #[test]
    fn guaranteed_tolerance_survives_two_spellings_of_one_quantity() {
        let a = quantity::parse("0.7").expect("0.7");
        let b = quantity::parse("700m").expect("700m");
        assert_ne!(a, b, "the premise: these differ in f64");
        assert_eq!(
            pod_qos(&pod_with(vec![container(
                "c",
                Some(("0.7", "1Gi")),
                Some(("700m", "1024Mi"))
            )])),
            QosClass::Guaranteed,
            "one quantity written two ways is still Guaranteed"
        );
        assert_eq!(
            qos_from_totals(a, b, 64.0 * MI, 64.0 * MI),
            QosClass::Guaranteed,
            "and the totals path agrees"
        );
    }

    #[test]
    fn the_three_classes_from_totals() {
        assert_eq!(
            qos_from_totals(0.1, 0.1, 64.0 * MI, 64.0 * MI),
            QosClass::Guaranteed
        );
        assert_eq!(
            qos_from_totals(0.1, 0.0, 64.0 * MI, 0.0),
            QosClass::Burstable
        );
        assert_eq!(qos_from_totals(0.0, 0.0, 0.0, 0.0), QosClass::BestEffort);
    }

    /// A pod that sets ONLY limits is Burstable, not BestEffort. The `any_lim`
    /// half of `qos_from_spec`'s BestEffort guard is otherwise uncovered, and
    /// getting it wrong evicts the pod FIRST instead of after BestEffort — the
    /// opposite end of the order.
    ///
    /// This is guidance §5's second prescribed test. I dropped it initially
    /// because the doc's framing is wrong about a *live* cluster (the API server
    /// defaults requests := limits, so such a pod is stored Guaranteed) — but the
    /// assertion about this fixture is correct and reaches a branch nothing else
    /// does. Dropping coverage because the surrounding prose was wrong was my
    /// error, caught by review.
    #[test]
    fn a_limits_only_pod_is_burstable_not_besteffort() {
        assert_eq!(
            pod_qos(&pod_with(vec![container(
                "c",
                None,
                Some(("250m", "64Mi"))
            )])),
            QosClass::Burstable
        );
    }
}
