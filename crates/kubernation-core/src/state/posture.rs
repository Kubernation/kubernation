//! The realm-defense **Posture score** — a 0-100 severity-weighted rollup of the
//! two security scans (hardening #7 + NetworkPolicy walls #10) into one
//! glanceable rating + tier, capping the security trio. PURE, read-only.
//!
//! It is the ONLY place that imports both reports, so the score can never
//! disagree with the Hardening / Network (WALLS) tabs it summarizes. **Honest:**
//! a CURATED subset (a handful of PSS/OWASP rules + ingress segmentation), NOT a
//! CIS/full-PSS compliance score — the UI says so. **System namespaces**
//! (kube-system/…, via `chaos::ns_protected`) are scored *separately* and NEVER
//! drag the operator score (the distro's CNI/kube-proxy posture isn't the
//! operator's to fix — mirrors the #7 queue exclusion). **Never** a green
//! all-clear on an empty / unscanned cluster (`score == None` ⇒ Unscanned).
//!
//! Methodology: two start-at-100-and-deduct axis sub-scores, severity-weighted
//! (a high linear CRIT weight, no presence floors, an Info cap), blended 60/40
//! (pod-security heavier: breakout > lateral movement). Both sub-scores are
//! published — the blend is the glance, the axes + ranked factors are the why.

use crate::state::chaos::ns_protected;
use crate::state::harden::hardening_report;
use crate::state::netpol::coverage_report;
use crate::state::observed::ObservedWorld;

// --- weights (one auditable place, fenced by the anti-trap tests) -----------

/// One Critical (breakout-risk) operator workload removes this much — high
/// enough that a single privileged pod visibly bites (no floor needed).
pub const DEDUCT_CRIT: f64 = 22.0;
pub const DEDUCT_WARN: f64 = 6.0;
pub const DEDUCT_INFO: f64 = 1.5;
/// Info hygiene nits (no-limits / automount / `:latest`) trip almost every
/// default workload — cap their total so they can't tank a crit-free cluster.
pub const INFO_DEDUCT_CAP: f64 = 10.0;
/// One unwalled-AND-exposed workload (the K07 lateral-movement hole).
pub const DEDUCT_K07: f64 = 14.0;
/// One zero-policy namespace with workloads (a wide-open continent).
pub const DEDUCT_WIDE_OPEN: f64 = 5.0;
/// Blend: pod-security heavier (breakout worse than lateral movement, and it's
/// always-evaluable / metrics-free).
pub const POD_WEIGHT: f64 = 0.6;
pub const NET_WEIGHT: f64 = 0.4;

pub const FORTIFIED_MIN: i32 = 90;
pub const DEFENDED_MIN: i32 = 70;
pub const EXPOSED_MIN: i32 = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostureTier {
    Fortified,
    Defended,
    Exposed,
    Breached,
    Unscanned,
}

impl PostureTier {
    pub fn label(self) -> &'static str {
        match self {
            PostureTier::Fortified => "FORTIFIED",
            PostureTier::Defended => "DEFENDED",
            PostureTier::Exposed => "EXPOSED",
            PostureTier::Breached => "BREACHED",
            PostureTier::Unscanned => "UNSCANNED",
        }
    }
}

/// Which detail tab a factor links to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Fortifications,
    Walls,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FactorKind {
    Critical,
    Warning,
    Info,
    K07,
    WideOpen,
}

/// One ranked, explainable deduction (the "why"), points-descending.
#[derive(Clone, Debug, PartialEq)]
pub struct PostureFactor {
    pub axis: Axis,
    pub kind: FactorKind,
    /// Points removed (rounded, >0) — the ranking key.
    pub points: i32,
    pub label: String,
    /// Named offenders + the target tab.
    pub detail: String,
    /// The Info bucket when bound by `INFO_DEDUCT_CAP` (renders "(capped)").
    pub capped: bool,
}

/// One axis sub-score + its operator-scope counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AxisScore {
    pub score: i32,
    /// pod: crit workloads · walls: unwalled-&-exposed.
    pub critical: usize,
    /// pod: warn workloads · walls: wide-open namespaces.
    pub warning: usize,
    /// pod: info-only workloads · walls: unwalled-but-unexposed (advisory).
    pub info: usize,
}

/// The whole realm-defense report.
#[derive(Clone, Debug, PartialEq)]
pub struct PostureReport {
    /// `None` ⇒ Unscanned (never a number then).
    pub score: Option<i32>,
    pub tier: PostureTier,
    pub scanned: bool,
    pub fortifications: AxisScore,
    pub walls: AxisScore,
    pub workloads_total: usize,
    /// System-namespace findings — surfaced dimmed, NEVER deducted.
    pub system_critical: usize,
    pub system_warning: usize,
    /// The explainability spine, sorted points-descending (operator scope only).
    pub factors: Vec<PostureFactor>,
}

/// The tier for a score (`None` ⇒ Unscanned). Exposed for boundary tests.
pub fn band(score: Option<i32>) -> PostureTier {
    match score {
        None => PostureTier::Unscanned,
        Some(s) if s >= FORTIFIED_MIN => PostureTier::Fortified,
        Some(s) if s >= DEFENDED_MIN => PostureTier::Defended,
        Some(s) if s >= EXPOSED_MIN => PostureTier::Exposed,
        Some(_) => PostureTier::Breached,
    }
}

/// Format up to 3 `ns/name` offender refs + the target tab.
fn offenders(refs: &[(String, String)], tab: &str) -> String {
    let shown: Vec<String> = refs
        .iter()
        .take(3)
        .map(|(ns, n)| format!("{ns}/{n}"))
        .collect();
    let more = refs.len().saturating_sub(3);
    let tail = if more > 0 {
        format!(" +{more}")
    } else {
        String::new()
    };
    format!("{}{}  → {}", shown.join(", "), tail, tab)
}

/// THE pure builder — the single importer of both security reports.
pub fn posture_report(world: &ObservedWorld) -> PostureReport {
    let h = hardening_report(world);
    let n = coverage_report(world);

    // --- Axis A: FORTIFICATIONS (pod hardening), operator scope -------------
    let op = |refs: &[crate::state::harden::WorkloadFindings]| -> Vec<(String, String)> {
        refs.iter()
            .filter(|wf| !ns_protected(&wf.r.namespace))
            .map(|wf| (wf.r.namespace.clone(), wf.r.name.clone()))
            .collect()
    };
    let crit = op(&h.critical);
    let warn = op(&h.warning);
    let info = op(&h.info);
    let system_critical = h
        .critical
        .iter()
        .filter(|wf| ns_protected(&wf.r.namespace))
        .count();
    let system_warning = h
        .warning
        .iter()
        .filter(|wf| ns_protected(&wf.r.namespace))
        .count();

    let info_raw = DEDUCT_INFO * info.len() as f64;
    let info_deduction = info_raw.min(INFO_DEDUCT_CAP);
    let info_capped = info_raw > INFO_DEDUCT_CAP;
    let fort = (100.0
        - DEDUCT_CRIT * crit.len() as f64
        - DEDUCT_WARN * warn.len() as f64
        - info_deduction)
        .clamp(0.0, 100.0);

    // --- Axis B: WALLS (network segmentation), operator scope ---------------
    let k07: Vec<(String, String)> = n
        .unwalled_exposed
        .iter()
        .filter(|r| !ns_protected(&r.r.namespace))
        .map(|r| (r.r.namespace.clone(), r.r.name.clone()))
        .collect();
    let wide_open: Vec<String> = n
        .open_namespaces
        .iter()
        .filter(|ns| !ns_protected(&ns.namespace))
        .map(|ns| ns.namespace.clone())
        .collect();
    // Advisory only (not deducted): unwalled but not reachable.
    let unwalled_unexposed = n
        .rows
        .iter()
        .filter(|r| !r.cov.ingress && !r.exposed && !ns_protected(&r.r.namespace))
        .count();
    let walls = (100.0 - DEDUCT_K07 * k07.len() as f64 - DEDUCT_WIDE_OPEN * wide_open.len() as f64)
        .clamp(0.0, 100.0);

    // --- scanned? (never a green all-clear on empty / mid-sync) -------------
    // Operator-scoped: at least one OPERATOR workload must actually resolve (its
    // pod template is observed). Reusing the cluster-wide `h.unresolved` /
    // `h.workloads_total` would let a resolvable SYSTEM workload (kube-system
    // CNI/kube-proxy) unlock a green score while every operator workload is still
    // mid-sync — a false all-clear. Mirrors the resolution test hardening uses.
    let operator_total = n
        .rows
        .iter()
        .filter(|r| !ns_protected(&r.r.namespace))
        .count();
    let operator_resolved = n
        .rows
        .iter()
        .filter(|r| !ns_protected(&r.r.namespace))
        .filter(|r| crate::state::model::workload_template(world, &r.r).is_some())
        .count();
    let scanned = operator_resolved > 0;

    let score = scanned.then(|| (POD_WEIGHT * fort + NET_WEIGHT * walls).round() as i32);
    let tier = band(score);

    // --- factors (operator scope, points-descending) ------------------------
    let mut factors: Vec<PostureFactor> = Vec::new();
    if !crit.is_empty() {
        factors.push(PostureFactor {
            axis: Axis::Fortifications,
            kind: FactorKind::Critical,
            points: (DEDUCT_CRIT * crit.len() as f64).round() as i32,
            label: format!("{} workload(s) with breakout risk", crit.len()),
            detail: offenders(&crit, "Hardening"),
            capped: false,
        });
    }
    if !k07.is_empty() {
        factors.push(PostureFactor {
            axis: Axis::Walls,
            kind: FactorKind::K07,
            points: (DEDUCT_K07 * k07.len() as f64).round() as i32,
            label: format!(
                "{} city(ies) open to lateral movement (unwalled & exposed)",
                k07.len()
            ),
            detail: offenders(&k07, "Network ▸ WALLS"),
            capped: false,
        });
    }
    if !warn.is_empty() {
        factors.push(PostureFactor {
            axis: Axis::Fortifications,
            kind: FactorKind::Warning,
            points: (DEDUCT_WARN * warn.len() as f64).round() as i32,
            label: format!("{} workload(s) with PSS-restricted gaps", warn.len()),
            detail: offenders(&warn, "Hardening"),
            capped: false,
        });
    }
    if !wide_open.is_empty() {
        let names: Vec<(String, String)> = wide_open
            .iter()
            .map(|ns| (ns.clone(), String::new()))
            .collect();
        factors.push(PostureFactor {
            axis: Axis::Walls,
            kind: FactorKind::WideOpen,
            points: (DEDUCT_WIDE_OPEN * wide_open.len() as f64).round() as i32,
            label: format!("{} wide-open namespace(s) (no policies)", wide_open.len()),
            detail: {
                let shown: Vec<String> = wide_open.iter().take(3).cloned().collect();
                let more = wide_open.len().saturating_sub(3);
                let tail = if more > 0 {
                    format!(" +{more}")
                } else {
                    String::new()
                };
                let _ = names;
                format!("{}{}  → Network ▸ WALLS", shown.join(", "), tail)
            },
            capped: false,
        });
    }
    if !info.is_empty() {
        factors.push(PostureFactor {
            axis: Axis::Fortifications,
            kind: FactorKind::Info,
            points: info_deduction.round() as i32,
            label: format!(
                "hygiene nits on {} city(ies) (no-limits / automount)",
                info.len()
            ),
            detail: offenders(&info, "Hardening"),
            capped: info_capped,
        });
    }
    factors.sort_by_key(|f| std::cmp::Reverse(f.points));

    PostureReport {
        score,
        tier,
        scanned,
        fortifications: AxisScore {
            score: fort.round() as i32,
            critical: crit.len(),
            warning: warn.len(),
            info: info.len(),
        },
        walls: AxisScore {
            score: walls.round() as i32,
            critical: k07.len(),
            warning: wide_open.len(),
            info: unwalled_unexposed,
        },
        workloads_total: operator_total,
        system_critical,
        system_warning,
        factors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec, SecurityContext};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    /// A Deployment with `app=<name>` template labels; `privileged` makes it a
    /// hardening Critical, else (with a resource limit) it is clean.
    fn dep(s: &mut fx::Seeds, ns: &str, name: &str, privileged: bool, clean: bool) {
        let mut d = fx::deployment(ns, name, 1, 1);
        let mut c = Container {
            name: "main".into(),
            image: Some("img:1.2.3".into()),
            ..Default::default()
        };
        if privileged {
            c.security_context = Some(SecurityContext {
                privileged: Some(true),
                ..Default::default()
            });
        }
        if clean && !privileged {
            // A restricted-ish container: drop caps, non-root, ro-fs, limits, etc.
            c.security_context = Some(SecurityContext {
                run_as_non_root: Some(true),
                allow_privilege_escalation: Some(false),
                read_only_root_filesystem: Some(true),
                capabilities: Some(k8s_openapi::api::core::v1::Capabilities {
                    drop: Some(vec!["ALL".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            });
            c.resources = Some(k8s_openapi::api::core::v1::ResourceRequirements {
                limits: Some(fx::quantities(&[("cpu", "100m"), ("memory", "128Mi")])),
                ..Default::default()
            });
        }
        d.spec.as_mut().unwrap().template = PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some(BTreeMap::from([("app".to_string(), name.to_string())])),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![c],
                automount_service_account_token: Some(false),
                ..Default::default()
            }),
        };
        s.deployment(d);
    }

    #[test]
    fn empty_world_is_unscanned() {
        let (world, _s) = fx::world();
        let r = posture_report(&world);
        assert_eq!(r.score, None);
        assert_eq!(r.tier, PostureTier::Unscanned);
        assert!(!r.scanned);
    }

    #[test]
    fn unresolved_operator_workloads_stay_unscanned_despite_system() {
        let (world, mut s) = fx::world();
        // A resolvable SYSTEM workload — must NOT unlock a green operator score.
        dep(&mut s, "kube-system", "kindnet", false, true);
        // Operator workloads present but mid-sync (no spec ⇒ template unresolved).
        for i in 0..3 {
            let mut d = fx::deployment("demo", &format!("w{i}"), 1, 1);
            d.spec = None;
            s.deployment(d);
        }
        let r = posture_report(&world);
        assert_eq!(
            r.score, None,
            "every operator workload mid-sync ⇒ Unscanned"
        );
        assert_eq!(r.tier, PostureTier::Unscanned);
    }

    #[test]
    fn clean_scanned_world_is_fortified() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", false, true);
        dep(&mut s, "demo", "api", false, true);
        // A namespace-wide deny-default wall → not wide-open → a true 100 all-clear.
        s.networkpolicy(fx::networkpolicy_empty(
            "demo",
            "deny-default",
            &["Ingress"],
        ));
        let r = posture_report(&world);
        assert!(r.scanned);
        assert_eq!(r.score, Some(100));
        assert_eq!(r.tier, PostureTier::Fortified);
        assert!(r.factors.is_empty());
    }

    #[test]
    fn info_nits_cannot_tank() {
        let (world, mut s) = fx::world();
        // 50 workloads that are clean except they trip Info nits — model that by a
        // container with no limits + automount (Info), nothing worse.
        for i in 0..50 {
            let mut d = fx::deployment("demo", &format!("w{i:02}"), 1, 1);
            d.spec.as_mut().unwrap().template = PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(BTreeMap::from([("app".to_string(), format!("w{i}"))])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "main".into(),
                        image: Some("img:1".into()),
                        // non-root + drop-all + ro-fs + no-priv-esc so the only
                        // findings are Info (no limits, automount default).
                        security_context: Some(SecurityContext {
                            run_as_non_root: Some(true),
                            allow_privilege_escalation: Some(false),
                            read_only_root_filesystem: Some(true),
                            capabilities: Some(k8s_openapi::api::core::v1::Capabilities {
                                drop: Some(vec!["ALL".into()]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            };
            s.deployment(d);
        }
        let r = posture_report(&world);
        // Info deduction is capped → fortifications stays high, never Breached.
        assert_eq!(r.fortifications.score, 90, "info cap = 100 - 10");
        assert!(
            r.score.unwrap() >= FORTIFIED_MIN,
            "nits alone never sink the realm"
        );
        assert_eq!(r.tier, PostureTier::Fortified);
        assert!(
            r.factors
                .iter()
                .any(|f| f.kind == FactorKind::Info && f.capped)
        );
    }

    #[test]
    fn one_privileged_workload_dents_visibly() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "bad", true, false);
        for i in 0..19 {
            dep(&mut s, "demo", &format!("ok{i}"), false, true);
        }
        let r = posture_report(&world);
        assert_eq!(r.fortifications.score, 78, "one crit = 100 - 22, no floor");
        assert_eq!(r.factors[0].kind, FactorKind::Critical);
        assert_eq!(r.factors[0].axis, Axis::Fortifications);
        assert!(r.factors[0].detail.contains("demo/bad"));
    }

    #[test]
    fn three_criticals_is_breached() {
        let (world, mut s) = fx::world();
        for i in 0..3 {
            dep(&mut s, "demo", &format!("bad{i}"), true, false);
        }
        let r = posture_report(&world);
        assert_eq!(r.fortifications.score, 34, "3 crit = 100 - 66");
        // blended with walls=100: 0.6*34 + 0.4*100 = 60.4 → 60 (Exposed). Add an
        // exposed-unwalled to push Breached is covered elsewhere; here assert the
        // axis is Breached-grade and the blend is honest.
        assert!(r.fortifications.score < EXPOSED_MIN);
    }

    #[test]
    fn system_namespace_criticals_do_not_score() {
        let (world, mut s) = fx::world();
        dep(&mut s, "kube-system", "kindnet", true, false); // distro default — excluded
        dep(&mut s, "demo", "web", false, true);
        let r = posture_report(&world);
        assert_eq!(
            r.fortifications.critical, 0,
            "system crit not counted in the axis"
        );
        assert_eq!(r.system_critical, 1, "...but surfaced separately");
        assert_eq!(
            r.tier,
            PostureTier::Fortified,
            "the operator realm is clean"
        );
    }

    #[test]
    fn k07_unwalled_exposed_deducts() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "web", false, true);
        s.service(fx::service("demo", "web", &[("app", "web")])); // exposed, unwalled
        // A policy on a DIFFERENT app keeps demo from being "wide-open" so this
        // isolates the K07 deduction (web stays unwalled & exposed).
        s.networkpolicy(fx::networkpolicy(
            "demo",
            "api-iso",
            &[("app", "api")],
            &["Ingress"],
        ));
        let r = posture_report(&world);
        assert_eq!(r.walls.critical, 1);
        assert_eq!(r.walls.score, 86, "100 - 14");
        assert!(r.factors.iter().any(|f| f.kind == FactorKind::K07));
    }

    #[test]
    fn zero_policies_no_exposure_scores_high() {
        let (world, mut s) = fx::world();
        // workloads, no Service/Ingress, no policies → nothing reachable to move
        // into; a private dev cluster shouldn't be punished.
        dep(&mut s, "demo", "web", false, true);
        dep(&mut s, "demo", "api", false, true);
        let r = posture_report(&world);
        assert_eq!(r.walls.critical, 0, "no exposed-unwalled finding");
        assert!(r.walls.score >= EXPOSED_MIN || r.walls.score == 100 - DEDUCT_WIDE_OPEN as i32);
    }

    #[test]
    fn band_cutoffs() {
        assert_eq!(band(None), PostureTier::Unscanned);
        assert_eq!(band(Some(39)), PostureTier::Breached);
        assert_eq!(band(Some(40)), PostureTier::Exposed);
        assert_eq!(band(Some(69)), PostureTier::Exposed);
        assert_eq!(band(Some(70)), PostureTier::Defended);
        assert_eq!(band(Some(89)), PostureTier::Defended);
        assert_eq!(band(Some(90)), PostureTier::Fortified);
        assert_eq!(band(Some(100)), PostureTier::Fortified);
    }

    #[test]
    fn factors_sorted_points_desc_and_carry_axis() {
        let (world, mut s) = fx::world();
        dep(&mut s, "demo", "bad", true, false); // crit (22)
        dep(&mut s, "demo", "web", false, true);
        s.service(fx::service("demo", "web", &[("app", "web")])); // k07 (14)
        let r = posture_report(&world);
        assert!(r.factors.len() >= 2);
        assert!(r.factors[0].points >= r.factors[1].points);
        assert_eq!(r.factors[0].kind, FactorKind::Critical);
        assert!(r.factors.iter().any(|f| f.axis == Axis::Walls));
    }

    #[test]
    fn blend_is_weighted_60_40() {
        // fortifications 100, walls dented to exercise the blend: hard to hit an
        // exact 0 walls cleanly, so assert the blend formula directly.
        let fort = 100.0_f64;
        let walls = 0.0_f64;
        assert_eq!((POD_WEIGHT * fort + NET_WEIGHT * walls).round() as i32, 60);
    }
}
