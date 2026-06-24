//! Security / hardening scan — the realm's fortifications.
//!
//! A PURE, read-only lint of each workload's pod *template* for security
//! misconfigurations, mapped to the standard each comes from: OWASP K8s K01
//! (insecure workload config), the Pod Security Standards (baseline / restricted),
//! and a Popeye-style sanitizer subset. It reports; it never writes (KuberNation's
//! gated write surface gains nothing here). Cluster-wide, metrics-free,
//! unit-tested without a cluster — `scan_template` takes a `PodTemplateSpec`
//! directly so every rule is testable in isolation.
//!
//! Deliberately a CURATED SUBSET, not full PSS compliance — and honest about it
//! (seccomp + default-ServiceAccount are *not* checked: a static template can't
//! tell whether the kubelet's `SeccompDefault` applied, and the SA object isn't
//! watched, so either would false-positive at the common default).

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};

use crate::state::model::{WorkloadRef, build_workloads, workload_template};
use crate::state::observed::ObservedWorld;

/// The standard a finding maps to (named so the operator can judge it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Standard {
    PssBaseline,
    PssRestricted,
    OwaspK01,
    Popeye,
}

impl Standard {
    pub fn label(self) -> &'static str {
        match self {
            Standard::PssBaseline => "PSS-baseline",
            Standard::PssRestricted => "PSS-restricted",
            Standard::OwaspK01 => "OWASP-K01",
            Standard::Popeye => "Popeye",
        }
    }
}

/// Hardening severity — local to this module (mapped to `attention::Severity`
/// only at the net.rs queue boundary, so core has no attention dependency here).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum HSeverity {
    Info,
    Warning,
    Critical,
}

/// One misconfiguration finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule_id: &'static str,
    pub standard: Standard,
    pub severity: HSeverity,
    /// The container the finding is about (None for pod-level findings).
    pub container: Option<String>,
    pub detail: String,
}

/// A workload's scan result.
#[derive(Clone, Debug)]
pub struct WorkloadFindings {
    pub r: WorkloadRef,
    pub worst: HSeverity,
    pub findings: Vec<Finding>,
    /// The pod template couldn't be resolved (mid-sync / mid-delete) — not clean,
    /// not flagged.
    pub unresolved: bool,
}

/// Cluster-wide hardening rollup.
#[derive(Clone, Debug, Default)]
pub struct HardeningReport {
    pub workloads_total: usize,
    pub workloads_clean: usize,
    pub unresolved: usize,
    pub critical: Vec<WorkloadFindings>,
    pub warning: Vec<WorkloadFindings>,
    pub info: Vec<WorkloadFindings>,
    pub counts_by_rule: BTreeMap<&'static str, usize>,
    pub counts_by_standard: BTreeMap<&'static str, usize>,
}

/// The ingredients for one aggregated attention concern (assembled in net.rs,
/// which owns `attention::Concern`). Only critical workloads produce one.
pub struct HardenConcern {
    pub r: WorkloadRef,
    pub title: String,
    pub detail: String,
    pub key: String,
}

/// PSS-baseline-allowed added capabilities (normalized, no `CAP_`). Anything
/// outside this set (notably `SYS_ADMIN`, `NET_ADMIN`, `SYS_PTRACE`, `ALL`) is a
/// baseline violation. `NET_RAW` is allowed at baseline.
const BASELINE_CAPS: &[&str] = &[
    "AUDIT_WRITE",
    "CHOWN",
    "DAC_OVERRIDE",
    "FOWNER",
    "FSETID",
    "KILL",
    "MKNOD",
    "NET_BIND_SERVICE",
    "SETFCAP",
    "SETGID",
    "SETPCAP",
    "SETUID",
    "SYS_CHROOT",
    "NET_RAW",
];

/// Uppercase + strip a leading `CAP_`, so `CAP_SYS_ADMIN` / `cap_sys_admin` /
/// `SYS_ADMIN` all compare equal.
fn norm_cap(cap: &str) -> String {
    let up = cap.to_uppercase();
    up.strip_prefix("CAP_").unwrap_or(&up).to_string()
}

fn baseline_allowed(cap: &str) -> bool {
    BASELINE_CAPS.contains(&norm_cap(cap).as_str())
}

/// Is an image unpinned (`:latest` or no tag)? A `@sha256:` digest is always
/// pinned. Only the segment after the final `/` is inspected, so a registry port
/// (`reg:5000/img`) isn't mistaken for a tag.
fn image_unpinned(image: &str) -> bool {
    let last = image.rsplit('/').next().unwrap_or(image);
    if last.contains('@') {
        return false; // digest-pinned
    }
    match last.split_once(':') {
        Some((_, tag)) => tag == "latest",
        None => true, // untagged
    }
}

/// Regular containers + native sidecars (`restartPolicy: Always` initContainers)
/// — the containers that run for the pod's whole life. Plain init / ephemeral
/// containers are excluded (legitimately elevated, short-lived).
fn scannable_containers(spec: &PodSpec) -> Vec<&Container> {
    let sidecars = spec
        .init_containers
        .iter()
        .flatten()
        .filter(|c| c.restart_policy.as_deref() == Some("Always"));
    spec.containers.iter().chain(sidecars).collect()
}

/// Scan one pod template for misconfigurations. PURE.
pub(crate) fn scan_template(t: &PodTemplateSpec) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    let Some(spec) = t.spec.as_ref() else {
        return out;
    };
    let p = |rule_id, standard, severity, container, detail: String| Finding {
        rule_id,
        standard,
        severity,
        container,
        detail,
    };

    // --- Pod-level ---------------------------------------------------------
    let mut hostns = Vec::new();
    if spec.host_network == Some(true) {
        hostns.push("hostNetwork");
    }
    if spec.host_pid == Some(true) {
        hostns.push("hostPID");
    }
    if spec.host_ipc == Some(true) {
        hostns.push("hostIPC");
    }
    if !hostns.is_empty() {
        out.push(p(
            "HARD02",
            Standard::PssBaseline,
            HSeverity::Critical,
            None,
            format!("{} enabled", hostns.join(" + ")),
        ));
    }
    let hostpaths: Vec<String> = spec
        .volumes
        .iter()
        .flatten()
        .filter_map(|v| v.host_path.as_ref().map(|h| h.path.clone()))
        .collect();
    if !hostpaths.is_empty() {
        let shown = hostpaths
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = if hostpaths.len() > 2 {
            format!(" +{}", hostpaths.len() - 2)
        } else {
            String::new()
        };
        out.push(p(
            "HARD04",
            Standard::PssBaseline,
            HSeverity::Critical,
            None,
            format!("hostPath: {shown}{more}"),
        ));
    }
    if spec.automount_service_account_token != Some(false) {
        out.push(p(
            "HARD22",
            Standard::Popeye,
            HSeverity::Info,
            None,
            "automountServiceAccountToken not disabled".into(),
        ));
    }

    // --- Container / effective-level --------------------------------------
    let pod_sc = spec.security_context.as_ref();
    let pod_non_root = pod_sc.and_then(|s| s.run_as_non_root);
    let pod_run_as_user = pod_sc.and_then(|s| s.run_as_user);

    for c in scannable_containers(spec) {
        let csc = c.security_context.as_ref();
        let name = Some(c.name.clone());
        let privileged = csc.and_then(|s| s.privileged) == Some(true);

        if privileged {
            out.push(p(
                "HARD01",
                Standard::PssBaseline,
                HSeverity::Critical,
                name.clone(),
                "privileged: true".into(),
            ));
        }
        // Dangerous added capabilities (outside the baseline allow-set).
        if let Some(add) = csc
            .and_then(|s| s.capabilities.as_ref())
            .and_then(|cp| cp.add.as_ref())
        {
            let bad: Vec<String> = add
                .iter()
                .filter(|cap| !baseline_allowed(cap))
                .map(|cap| norm_cap(cap))
                .collect();
            if !bad.is_empty() {
                out.push(p(
                    "HARD03",
                    Standard::PssBaseline,
                    HSeverity::Critical,
                    name.clone(),
                    format!("adds {}", bad.join(", ")),
                ));
            }
        }
        // Effective run-as-root.
        let eff_non_root = csc.and_then(|s| s.run_as_non_root).or(pod_non_root) == Some(true);
        let eff_user_nonzero = csc
            .and_then(|s| s.run_as_user)
            .or(pod_run_as_user)
            .is_some_and(|u| u != 0);
        if !eff_non_root && !eff_user_nonzero {
            out.push(p(
                "HARD10",
                Standard::PssRestricted,
                HSeverity::Warning,
                name.clone(),
                "may run as root (runAsNonRoot unset)".into(),
            ));
        }
        // allowPrivilegeEscalation + caps-drop-ALL are implied by privileged —
        // one root cause, one row.
        if !privileged {
            if csc.and_then(|s| s.allow_privilege_escalation) != Some(false) {
                out.push(p(
                    "HARD11",
                    Standard::PssRestricted,
                    HSeverity::Warning,
                    name.clone(),
                    "allowPrivilegeEscalation not false".into(),
                ));
            }
            let drops_all = csc
                .and_then(|s| s.capabilities.as_ref())
                .and_then(|cp| cp.drop.as_ref())
                .is_some_and(|d| d.iter().any(|c| norm_cap(c) == "ALL"));
            if !drops_all {
                out.push(p(
                    "HARD12",
                    Standard::PssRestricted,
                    HSeverity::Warning,
                    name.clone(),
                    "capabilities not dropped (ALL)".into(),
                ));
            }
        }
        if csc.and_then(|s| s.read_only_root_filesystem) != Some(true) {
            out.push(p(
                "HARD13",
                Standard::PssRestricted,
                HSeverity::Warning,
                name.clone(),
                "root filesystem writable".into(),
            ));
        }
        let limits = c.resources.as_ref().and_then(|r| r.limits.as_ref());
        let has_cpu = limits.is_some_and(|l| l.contains_key("cpu"));
        let has_mem = limits.is_some_and(|l| l.contains_key("memory"));
        // Fire on EITHER missing — a missing memory limit is the real OOM /
        // noisy-neighbour risk, so don't let a cpu-only limit hide it.
        if !has_cpu || !has_mem {
            let detail = match (has_cpu, has_mem) {
                (false, false) => "no cpu/memory limits",
                (true, false) => "no memory limit",
                (false, true) => "no cpu limit",
                (true, true) => unreachable!(),
            };
            out.push(p(
                "HARD20",
                Standard::Popeye,
                HSeverity::Info,
                name.clone(),
                detail.into(),
            ));
        }
        if let Some(img) = c.image.as_ref()
            && image_unpinned(img)
        {
            out.push(p(
                "HARD21",
                Standard::OwaspK01,
                HSeverity::Info,
                name.clone(),
                format!("unpinned image: {img}"),
            ));
        }
    }
    out
}

fn scan_workload(world: &ObservedWorld, r: &WorkloadRef) -> WorkloadFindings {
    match workload_template(world, r) {
        Some(t) => {
            let findings = scan_template(&t);
            let worst = findings
                .iter()
                .map(|f| f.severity)
                .max()
                .unwrap_or(HSeverity::Info);
            WorkloadFindings {
                r: r.clone(),
                worst,
                findings,
                unresolved: false,
            }
        }
        None => WorkloadFindings {
            r: r.clone(),
            worst: HSeverity::Info,
            findings: Vec::new(),
            unresolved: true,
        },
    }
}

/// Build the cluster-wide hardening report (NOT namespace-scoped, like the other
/// advisors).
pub fn hardening_report(world: &ObservedWorld) -> HardeningReport {
    let mut rep = HardeningReport::default();
    for row in build_workloads(world) {
        rep.workloads_total += 1;
        let wf = scan_workload(world, &row.r);
        if wf.unresolved {
            rep.unresolved += 1;
            continue;
        }
        if wf.findings.is_empty() {
            rep.workloads_clean += 1;
            continue;
        }
        for f in &wf.findings {
            *rep.counts_by_rule.entry(f.rule_id).or_default() += 1;
            *rep.counts_by_standard
                .entry(f.standard.label())
                .or_default() += 1;
        }
        match wf.worst {
            HSeverity::Critical => rep.critical.push(wf),
            HSeverity::Warning => rep.warning.push(wf),
            HSeverity::Info => rep.info.push(wf),
        }
    }
    let by_name = |a: &WorkloadFindings, b: &WorkloadFindings| {
        (&a.r.namespace, &a.r.name).cmp(&(&b.r.namespace, &b.r.name))
    };
    rep.critical.sort_by(by_name);
    rep.warning.sort_by(by_name);
    rep.info.sort_by(by_name);
    rep
}

/// One aggregated concern's ingredients — only for a workload whose worst finding
/// is Critical (privileged / host namespace / dangerous cap / hostPath). Warning
/// and Info findings are advisor-only (never the queue).
pub fn workload_concern(wf: &WorkloadFindings) -> Option<HardenConcern> {
    if wf.worst != HSeverity::Critical {
        return None;
    }
    let crits: Vec<&Finding> = wf
        .findings
        .iter()
        .filter(|f| f.severity == HSeverity::Critical)
        .collect();
    let std = standards_tag(&crits);
    let mut parts: Vec<String> = crits.iter().take(2).map(|f| f.detail.clone()).collect();
    if crits.len() > 2 {
        parts.push(format!("+{} more", crits.len() - 2));
    }
    Some(HardenConcern {
        r: wf.r.clone(),
        title: format!("insecure config: {}/{}", wf.r.namespace, wf.r.name),
        detail: format!("{} [{}]", parts.join(" + "), std),
        key: format!("harden:{}/{}", wf.r.namespace, wf.r.name),
    })
}

/// The distinct standards across a finding slice, as a tag — "PSS-baseline" or
/// "Popeye, OWASP-K01" when mixed (so the tag never mislabels a mixed bucket).
pub fn standards_tag(findings: &[&Finding]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for f in findings {
        let l = f.standard.label();
        if !seen.contains(&l) {
            seen.push(l);
        }
    }
    seen.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use k8s_openapi::api::core::v1::{
        Capabilities, Container, HostPathVolumeSource, PodSecurityContext, PodSpec,
        PodTemplateSpec, SecurityContext, Volume,
    };

    /// A template from one container with a custom security context.
    fn tmpl(c: Container) -> PodTemplateSpec {
        PodTemplateSpec {
            spec: Some(PodSpec {
                containers: vec![c],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn ctr() -> Container {
        Container {
            name: "main".into(),
            image: Some("nginx:1.27".into()),
            ..Default::default()
        }
    }

    fn has(findings: &[Finding], rule: &str) -> bool {
        findings.iter().any(|f| f.rule_id == rule)
    }

    #[test]
    fn privileged_is_critical_and_dedups_escalation_and_caps() {
        let mut c = ctr();
        c.security_context = Some(SecurityContext {
            privileged: Some(true),
            ..Default::default()
        });
        let f = scan_template(&tmpl(c));
        assert!(has(&f, "HARD01"));
        // privileged implies escalation + caps — those rows are suppressed.
        assert!(!has(&f, "HARD11"));
        assert!(!has(&f, "HARD12"));
        // but a writable root fs still fires.
        assert!(has(&f, "HARD13"));
    }

    #[test]
    fn dangerous_capability_with_normalization() {
        let mk = |cap: &str| {
            let mut c = ctr();
            c.security_context = Some(SecurityContext {
                capabilities: Some(Capabilities {
                    add: Some(vec![cap.to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            });
            scan_template(&tmpl(c))
        };
        assert!(has(&mk("SYS_ADMIN"), "HARD03"));
        assert!(has(&mk("CAP_SYS_ADMIN"), "HARD03")); // normalized
        assert!(!has(&mk("NET_BIND_SERVICE"), "HARD03")); // baseline-allowed
    }

    #[test]
    fn hard20_fires_on_a_missing_memory_limit_alone() {
        use k8s_openapi::api::core::v1::ResourceRequirements;
        let with_limits = |pairs: &[(&str, &str)]| {
            let mut c = ctr();
            c.resources = Some(ResourceRequirements {
                limits: Some(fx::quantities(pairs)),
                ..Default::default()
            });
            scan_template(&tmpl(c))
        };
        // cpu-only limit → still flags the missing memory limit (the OOM risk).
        let f = with_limits(&[("cpu", "500m")]);
        let h20 = f.iter().find(|x| x.rule_id == "HARD20").unwrap();
        assert!(h20.detail.contains("memory"), "{}", h20.detail);
        // both present → clean.
        assert!(!has(
            &with_limits(&[("cpu", "500m"), ("memory", "256Mi")]),
            "HARD20"
        ));
    }

    #[test]
    fn standards_tag_dedupes_distinct_standards() {
        let mk = |std| Finding {
            rule_id: "X",
            standard: std,
            severity: HSeverity::Info,
            container: None,
            detail: "d".into(),
        };
        let pop = mk(Standard::Popeye);
        let owasp = mk(Standard::OwaspK01);
        let refs = vec![&pop, &owasp, &pop];
        assert_eq!(standards_tag(&refs), "Popeye, OWASP-K01");
        assert_eq!(standards_tag(&[&pop]), "Popeye");
    }

    #[test]
    fn host_namespace_and_hostpath_are_critical() {
        let t = PodTemplateSpec {
            spec: Some(PodSpec {
                containers: vec![ctr()],
                host_network: Some(true),
                volumes: Some(vec![Volume {
                    name: "root".into(),
                    host_path: Some(HostPathVolumeSource {
                        path: "/".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let f = scan_template(&t);
        assert!(has(&f, "HARD02"));
        assert!(has(&f, "HARD04"));
        // host_network explicitly false is NOT flagged.
        let mut spec = t.spec.clone().unwrap();
        spec.host_network = Some(false);
        spec.volumes = None;
        let f2 = scan_template(&PodTemplateSpec {
            spec: Some(spec),
            ..Default::default()
        });
        assert!(!has(&f2, "HARD02"));
        assert!(!has(&f2, "HARD04"));
    }

    #[test]
    fn run_as_root_respects_effective_context() {
        // No securityContext → may run as root.
        assert!(has(&scan_template(&tmpl(ctr())), "HARD10"));
        // Pod-level runAsNonRoot suppresses it.
        let t = PodTemplateSpec {
            spec: Some(PodSpec {
                containers: vec![ctr()],
                security_context: Some(PodSecurityContext {
                    run_as_non_root: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!has(&scan_template(&t), "HARD10"));
        // A non-zero runAsUser on the container also satisfies it.
        let mut c = ctr();
        c.security_context = Some(SecurityContext {
            run_as_user: Some(5554),
            ..Default::default()
        });
        assert!(!has(&scan_template(&tmpl(c)), "HARD10"));
    }

    #[test]
    fn image_pin_rules() {
        let img = |s: &str| {
            let mut c = ctr();
            c.image = Some(s.into());
            scan_template(&tmpl(c))
        };
        assert!(has(&img("busybox:latest"), "HARD21"));
        assert!(has(&img("busybox"), "HARD21")); // untagged
        assert!(!has(&img("nginx:1.27"), "HARD21"));
        assert!(!has(&img("reg:5000/app:1.0"), "HARD21")); // port not tag
        assert!(!has(
            &img("nginx@sha256:abc123def4567890abc123def4567890abc123def4567890abc123def4567890"),
            "HARD21"
        )); // digest-pinned
    }

    #[test]
    fn no_limits_and_automount_are_info() {
        let f = scan_template(&tmpl(ctr()));
        assert!(has(&f, "HARD20")); // no limits
        assert!(has(&f, "HARD22")); // automount not disabled
        assert!(f.iter().find(|x| x.rule_id == "HARD20").unwrap().severity == HSeverity::Info);
    }

    #[test]
    fn native_sidecar_scanned_plain_init_not() {
        let priv_init = |restart: Option<&str>| {
            let mut init = ctr();
            init.name = "side".into();
            init.restart_policy = restart.map(Into::into);
            init.security_context = Some(SecurityContext {
                privileged: Some(true),
                ..Default::default()
            });
            PodTemplateSpec {
                spec: Some(PodSpec {
                    containers: vec![ctr()],
                    init_containers: Some(vec![init]),
                    ..Default::default()
                }),
                ..Default::default()
            }
        };
        // Native sidecar (restartPolicy: Always) → privileged fires.
        let f = scan_template(&priv_init(Some("Always")));
        assert!(
            f.iter()
                .any(|x| x.rule_id == "HARD01" && x.container.as_deref() == Some("side"))
        );
        // Plain init container → not scanned.
        let f2 = scan_template(&priv_init(None));
        assert!(!f2.iter().any(|x| x.container.as_deref() == Some("side")));
    }

    #[test]
    fn seccomp_nil_is_never_flagged() {
        // A template with no seccompProfile anywhere must not emit a seccomp rule
        // (none exists) — the deferred-by-design guard.
        let f = scan_template(&tmpl(ctr()));
        assert!(
            f.iter()
                .all(|x| !x.detail.to_lowercase().contains("seccomp"))
        );
    }

    #[test]
    fn report_buckets_and_concern() {
        let (world, mut s) = fx::world();
        // A clean-ish workload (web: pinned image, but still warnings) + a
        // privileged one (critical).
        s.deployment(fx::deployment("demo", "web", 1, 1));
        let mut bad = fx::deployment("demo", "bad", 1, 1);
        bad.spec
            .as_mut()
            .unwrap()
            .template
            .spec
            .as_mut()
            .unwrap()
            .containers[0]
            .security_context = Some(SecurityContext {
            privileged: Some(true),
            ..Default::default()
        });
        s.deployment(bad);

        let rep = hardening_report(&world);
        assert_eq!(rep.workloads_total, 2);
        assert_eq!(rep.critical.len(), 1);
        assert_eq!(rep.critical[0].r.name, "bad");
        // The critical workload yields exactly one aggregated concern.
        let hc = workload_concern(&rep.critical[0]).unwrap();
        assert!(hc.title.contains("insecure config"));
        assert!(hc.key.starts_with("harden:"));
        // A non-critical workload yields no concern.
        assert!(rep.warning.iter().all(|w| workload_concern(w).is_none()));
    }
}
