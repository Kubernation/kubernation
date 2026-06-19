//! The advisor screens — classic-4X "advisors" (Civ's F1 Berater) over the
//! pure core reports. Five read-only summary tabs: Health (state of the realm),
//! Storage (granaries), Network (harbors & gates), Right-sizing (requests vs
//! metrics-server usage — waste / risk / scheduler-blind), and Hardening
//! (security misconfigurations — OWASP-K01 / Pod Security Standards / Popeye).
//! Opened from the Advisors menu; a modal window like the Almanac, sharing its
//! window/tab/scroll machinery. Cluster-wide (hot).

use kubernation_core::state::advisor::{
    HealthReport, NetworkReport, RightSizingReport, RsRow, RsVerdict, StorageReport, health_report,
    network_report, rightsizing_report, storage_report,
};
use kubernation_core::state::harden::{self, HardeningReport, WorkloadFindings};
use kubernation_core::util::human_bytes;
use macroquad::prelude::*;

use crate::net::Snapshot;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AdvisorTab {
    Health,
    Storage,
    Network,
    RightSizing,
    Hardening,
}

impl AdvisorTab {
    pub const ALL: [AdvisorTab; 5] = [
        AdvisorTab::Health,
        AdvisorTab::Storage,
        AdvisorTab::Network,
        AdvisorTab::RightSizing,
        AdvisorTab::Hardening,
    ];
    fn idx(self) -> usize {
        match self {
            AdvisorTab::Health => 0,
            AdvisorTab::Storage => 1,
            AdvisorTab::Network => 2,
            AdvisorTab::RightSizing => 3,
            AdvisorTab::Hardening => 4,
        }
    }
}

pub enum AdvisorAction {
    None,
    Close,
}

pub struct Advisor {
    tab: AdvisorTab,
    scroll: f32,
    max_scroll: f32,
}

impl Advisor {
    pub fn new(tab: AdvisorTab) -> Self {
        Advisor {
            tab,
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    pub fn go(&mut self, tab: AdvisorTab) {
        self.tab = tab;
        self.scroll = 0.0;
    }

    pub fn cycle(&mut self, delta: i32) {
        let i = (self.tab.idx() as i32 + delta).rem_euclid(AdvisorTab::ALL.len() as i32);
        self.go(AdvisorTab::ALL[i as usize]);
    }

    /// Wheel scroll (positive = up).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(&mut self, snap: Option<&Snapshot>, mouse: Vec2, click: bool) -> AdvisorAction {
        let labels = [
            "Health",
            "Storage",
            "Network",
            "Right-sizing",
            "Hardening",
            "Close",
        ];
        let win = draw_window(
            "Advisors — state of the realm",
            vec2(760.0, 540.0),
            &labels,
            self.tab.idx(),
        );

        let mut cx = Ctx {
            body: win.body,
            y: win.body.y - self.scroll,
        };
        if let Some(s) = snap {
            let obs = &s.hot.observed;
            match self.tab {
                AdvisorTab::Health => page_health(&mut cx, &health_report(obs)),
                AdvisorTab::Storage => page_storage(&mut cx, &storage_report(obs)),
                AdvisorTab::Network => page_network(&mut cx, &network_report(obs)),
                AdvisorTab::RightSizing => page_rightsizing(&mut cx, &rightsizing_report(obs)),
                AdvisorTab::Hardening => page_hardening(&mut cx, &harden::hardening_report(obs)),
            }
        } else {
            cx.note("the world is not yet explored", DIM);
        }

        let content_h = cx.y - (win.body.y - self.scroll);
        self.max_scroll = (content_h - win.body.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        if self.max_scroll > 0.0 {
            let b = win.body;
            let frac = (b.h / content_h).clamp(0.05, 1.0);
            let thumb_h = b.h * frac;
            let t = self.scroll / self.max_scroll;
            let ty = b.y + t * (b.h - thumb_h);
            draw_rectangle(b.x + b.w + 2.0, b.y, 3.0, b.h, darker(PANEL, 0.6));
            draw_rectangle(b.x + b.w + 2.0, ty, 3.0, thumb_h, PARCHMENT);
        }

        if click {
            if win.close.contains(mouse) {
                return AdvisorAction::Close;
            }
            if let Some(i) = win.button_at(mouse) {
                if let Some(t) = AdvisorTab::ALL.get(i) {
                    self.go(*t);
                } else {
                    return AdvisorAction::Close; // the trailing "Close" tab
                }
            } else if !win.frame.contains(mouse) {
                return AdvisorAction::Close;
            }
        }
        AdvisorAction::None
    }
}

// --- content rendering ------------------------------------------------------

struct Ctx {
    body: Rect,
    y: f32,
}

impl Ctx {
    fn visible(&self) -> bool {
        self.y > self.body.y - 18.0 && self.y < self.body.y + self.body.h
    }
    fn heading(&mut self, s: &str) {
        // A clear gap above the heading (separates it from the prior section),
        // then it groups tightly with its own rows below.
        self.y += 24.0;
        if self.visible() {
            text_bold(s, self.body.x + 4.0, self.y, 15.0, PARCHMENT);
        }
        self.y += 6.0;
    }
    /// A "label ........ value" row, the value right-aligned and colored.
    fn stat(&mut self, label: &str, value: &str, color: Color) {
        self.y += 19.0;
        if self.visible() {
            text(label, self.body.x + 14.0, self.y, 14.0, INK);
            let vw = text_size(value, 14.0).width;
            text(
                value,
                self.body.x + self.body.w - vw - 6.0,
                self.y,
                14.0,
                color,
            );
        }
    }
    fn note(&mut self, s: &str, color: Color) {
        self.y += 18.0;
        if self.visible() {
            text(s, self.body.x + 14.0, self.y, 13.0, color);
        }
    }
    /// A free-form colored line (used by the right-sizing page, which renders a
    /// pure list of (text, role) lines). `bold` headings sit flush-left + larger.
    fn row(&mut self, s: &str, color: Color, bold: bool) {
        self.y += if bold { 23.0 } else { 18.0 };
        if self.visible() {
            if bold {
                text_bold(s, self.body.x + 4.0, self.y, 15.0, color);
            } else {
                text(s, self.body.x + 14.0, self.y, 13.0, color);
            }
        }
    }
}

// --- right-sizing page (pure line builder + renderer) -----------------------

/// The severity role of a right-sizing line (mapped to a theme colour at draw
/// time). Keeps `rightsizing_lines` pure + unit-testable (the `region_lines`
/// pattern from the GUI testability policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsRole {
    Headline,
    Heading,
    Good,
    Warn,
    Crit,
    Dim,
}

fn cpu_s(cores: f64) -> String {
    format!("{}m", (cores * 1000.0).round() as i64)
}

fn rs_row_line(row: &RsRow, bucket: RsVerdict) -> String {
    let mut s = format!(
        "{} {}/{} [{}]",
        row.kind,
        row.namespace,
        row.name,
        row.qos.label()
    );
    let mut clause = |res: &kubernation_core::state::advisor::RsResource, name: &str, mem: bool| {
        if res.verdict != bucket {
            return;
        }
        let fmt = |v: f64| if mem { human_bytes(v) } else { cpu_s(v) };
        match bucket {
            RsVerdict::Over => {
                let sug = res
                    .suggested
                    .map(|v| format!(" ~{}", fmt(v)))
                    .unwrap_or_default();
                s.push_str(&format!(
                    "  {name} {}->{}{}",
                    fmt(res.request),
                    fmt(res.usage),
                    sug
                ));
            }
            RsVerdict::Under => {
                // Show the value that DROVE the verdict — the peak pod for memory
                // (incompressible: the hottest replica OOMs), mean usage for cpu.
                // `suggested` is guaranteed a genuine raise above the request.
                let driver = if mem { res.peak } else { res.usage };
                let label = if mem { "peak" } else { "use" };
                let sug = res
                    .suggested
                    .map(|v| format!(" ~raise {}", fmt(v)))
                    .unwrap_or_default();
                s.push_str(&format!(
                    "  {name} req {} {label} {}{}",
                    fmt(res.request),
                    fmt(driver),
                    sug
                ));
            }
            RsVerdict::Unrequested => {
                let sug = res
                    .suggested
                    .map(|v| format!(" ~start {}", fmt(v)))
                    .unwrap_or_default();
                s.push_str(&format!("  {name} unset{sug}"));
            }
            _ => {}
        }
        if let Some(n) = res.note {
            s.push_str(&format!("  ({n})"));
        }
    };
    clause(&row.cpu, "cpu", false);
    clause(&row.mem, "mem", true);
    if row.measured_pods < row.running_pods {
        s.push_str(&format!(
            "  ({}/{} sampled)",
            row.measured_pods, row.running_pods
        ));
    }
    s
}

/// Cap a section of rows to `CAP`, appending a "+N more" overflow line.
const RS_CAP: usize = 12;

fn push_section(
    out: &mut Vec<(String, RsRole)>,
    heading: &str,
    rows: &[RsRow],
    bucket: RsVerdict,
    row_role: RsRole,
    empty: &str,
) {
    out.push((heading.to_string(), RsRole::Heading));
    if rows.is_empty() {
        out.push((empty.to_string(), RsRole::Dim));
        return;
    }
    for row in rows.iter().take(RS_CAP) {
        // BestEffort scheduler-blind rows are the most urgent — already CRIT.
        out.push((rs_row_line(row, bucket), row_role));
    }
    if rows.len() > RS_CAP {
        out.push((format!("+{} more", rows.len() - RS_CAP), RsRole::Dim));
    }
}

/// PURE: the right-sizing advisor's lines as (text, role). Unit-tested.
pub fn rightsizing_lines(r: &RightSizingReport) -> Vec<(String, RsRole)> {
    let mut out: Vec<(String, RsRole)> = Vec::new();
    let footer = "advice only — Kubernation can't edit container requests; apply via kubectl/manifest, then observe over time.";

    if !r.metrics_available {
        out.push((
            "right-sizing needs per-pod metrics (metrics-server). showing only scheduler-blind workloads.".to_string(),
            RsRole::Warn,
        ));
        push_section(
            &mut out,
            "SCHEDULER-BLIND (NO REQUESTS)",
            &r.unrequested,
            RsVerdict::Unrequested,
            RsRole::Crit,
            "every workload declares requests",
        );
        out.push((footer.to_string(), RsRole::Dim));
        return out;
    }

    // Headline: reclaimable reserved request (never invented dollars).
    let mut headline = format!(
        "RECLAIMABLE  {} cpu · {} mem",
        cpu_s(r.reclaimable_cpu),
        human_bytes(r.reclaimable_mem)
    );
    if r.node_equiv >= 0.05 {
        // Only when it rounds to a non-zero "{:.1}" — never "≈ 0.0 nodes".
        headline.push_str(&format!("  ≈ {:.1} nodes", r.node_equiv));
    }
    out.push((headline, RsRole::Headline));
    out.push((
        "from 1 metrics-server sample — directional, not a multi-day VPA fit".to_string(),
        RsRole::Dim,
    ));

    // Count strip.
    let count = |n: usize, on: RsRole| if n > 0 { on } else { RsRole::Dim };
    out.push((
        format!("over-provisioned: {}", r.over.len()),
        count(r.over.len(), RsRole::Warn),
    ));
    out.push((
        format!("under-provisioned: {}", r.under.len()),
        count(r.under.len(), RsRole::Crit),
    ));
    out.push((
        format!("scheduler-blind: {}", r.unrequested.len()),
        count(r.unrequested.len(), RsRole::Crit),
    ));
    out.push((
        format!("right-sized: {}", r.right_sized_count),
        RsRole::Good,
    ));
    if r.unmeasured > 0 {
        // Parts now sum to workloads_total (no misleading "X / Y" ratio).
        out.push((
            format!("not measured: {} (no usage / scaled to zero)", r.unmeasured),
            RsRole::Dim,
        ));
    }

    push_section(
        &mut out,
        "OVER-PROVISIONED (WASTE)",
        &r.over,
        RsVerdict::Over,
        RsRole::Warn,
        "every city is well-sized",
    );
    push_section(
        &mut out,
        "UNDER-PROVISIONED (THROTTLE / OOM RISK)",
        &r.under,
        RsVerdict::Under,
        RsRole::Crit,
        "no workload is starved",
    );
    push_section(
        &mut out,
        "SCHEDULER-BLIND (NO REQUESTS)",
        &r.unrequested,
        RsVerdict::Unrequested,
        RsRole::Crit,
        "every workload declares requests",
    );
    out.push((footer.to_string(), RsRole::Dim));
    out
}

fn page_rightsizing(cx: &mut Ctx, r: &RightSizingReport) {
    for (line, role) in rightsizing_lines(r) {
        let (color, bold) = match role {
            RsRole::Headline => (PARCHMENT, true),
            RsRole::Heading => (PARCHMENT, true),
            RsRole::Good => (GOOD, false),
            RsRole::Warn => (WARN, false),
            RsRole::Crit => (CRIT, false),
            RsRole::Dim => (DIM, false),
        };
        // Truncate to the body width so a long row never overflows the window.
        let size = if bold { 15.0 } else { 13.0 };
        let avail = cx.body.w - if bold { 10.0 } else { 22.0 };
        let shown = crate::panels::fit_width(&ascii(&line), size, avail);
        cx.row(&shown, color, bold);
    }
}

// --- hardening page (pure line builder + renderer) --------------------------

/// One workload's worst-severity findings as a compact "summary [standard]".
fn hf_summary(wf: &WorkloadFindings) -> String {
    let top: Vec<_> = wf
        .findings
        .iter()
        .filter(|f| f.severity == wf.worst)
        .collect();
    // The distinct standards across the shown findings — never mislabel a mixed
    // bucket (e.g. an Info row with both a Popeye + an OWASP-K01 finding).
    let std = harden::standards_tag(&top);
    let mut parts: Vec<String> = top
        .iter()
        .take(2)
        .map(|f| match &f.container {
            Some(c) => format!("{} ({c})", f.detail),
            None => f.detail.clone(),
        })
        .collect();
    if top.len() > 2 {
        parts.push(format!("+{} more", top.len() - 2));
    }
    format!("{} [{std}]", parts.join("; "))
}

fn hardening_section(
    out: &mut Vec<(String, RsRole)>,
    heading: &str,
    rows: &[WorkloadFindings],
    role: RsRole,
) {
    if rows.is_empty() {
        return;
    }
    out.push((heading.to_string(), RsRole::Heading));
    for wf in rows.iter().take(RS_CAP) {
        out.push((
            format!(
                "{} {}/{} — {}",
                wf.r.kind,
                wf.r.namespace,
                wf.r.name,
                hf_summary(wf)
            ),
            role,
        ));
    }
    if rows.len() > RS_CAP {
        out.push((format!("+{} more", rows.len() - RS_CAP), RsRole::Dim));
    }
}

/// PURE: the hardening advisor's lines as (text, role). Unit-tested.
pub fn hardening_lines(r: &HardeningReport) -> Vec<(String, RsRole)> {
    let mut out: Vec<(String, RsRole)> = Vec::new();
    // Separate the axes rather than a single "clean/total fortified" fraction —
    // Info-level hygiene nits (no limits / automount) trip almost every default
    // workload, so a fraction would read ~0/N and overstate the danger.
    out.push((
        format!(
            "DEFENSE — {} critical · {} warning · {} info · {} clean of {} workloads",
            r.critical.len(),
            r.warning.len(),
            r.info.len(),
            r.workloads_clean,
            r.workloads_total
        ),
        RsRole::Headline,
    ));
    out.push((
        "curated subset: PSS-baseline + PSS-restricted + OWASP-K01 + Popeye — not full PSS compliance".to_string(),
        RsRole::Dim,
    ));
    let by_std = |s: &str| r.counts_by_standard.get(s).copied().unwrap_or(0);
    out.push((
        format!(
            "findings: PSS-baseline {} · PSS-restricted {} · OWASP-K01 {} · Popeye {}",
            by_std("PSS-baseline"),
            by_std("PSS-restricted"),
            by_std("OWASP-K01"),
            by_std("Popeye")
        ),
        RsRole::Dim,
    ));
    if r.unresolved > 0 {
        out.push((
            format!("{} workload(s) not yet resolved", r.unresolved),
            RsRole::Dim,
        ));
    }

    // The all-clear is GREEN only when something was actually scanned clean —
    // never when the cluster is empty or every template is still unresolved
    // (a reassuring green there would be a false all-clear).
    let nothing_found = r.critical.is_empty() && r.warning.is_empty() && r.info.is_empty();
    if r.workloads_total == 0 {
        out.push(("no workloads to scan".to_string(), RsRole::Dim));
    } else if nothing_found && r.unresolved == 0 && r.workloads_clean > 0 {
        out.push((
            "every workload is fortified against the checked controls".to_string(),
            RsRole::Good,
        ));
    } else if nothing_found && r.unresolved > 0 {
        out.push((
            "scan pending — templates not yet resolved".to_string(),
            RsRole::Dim,
        ));
    }
    hardening_section(
        &mut out,
        "CRITICAL (escalation / breakout)",
        &r.critical,
        RsRole::Crit,
    );
    hardening_section(
        &mut out,
        "WARNING (PSS-restricted gaps)",
        &r.warning,
        RsRole::Warn,
    );
    hardening_section(&mut out, "INFO (hygiene)", &r.info, RsRole::Dim);

    out.push((
        "read-only — fix in the manifest/Helm chart and redeploy. Bare pods & Jobs not scanned; seccomp & default-SA deferred (often set at the namespace default).".to_string(),
        RsRole::Dim,
    ));
    out
}

fn page_hardening(cx: &mut Ctx, r: &HardeningReport) {
    for (line, role) in hardening_lines(r) {
        let (color, bold) = match role {
            RsRole::Headline | RsRole::Heading => (PARCHMENT, true),
            RsRole::Good => (GOOD, false),
            RsRole::Warn => (WARN, false),
            RsRole::Crit => (CRIT, false),
            RsRole::Dim => (DIM, false),
        };
        let size = if bold { 15.0 } else { 13.0 };
        let avail = cx.body.w - if bold { 10.0 } else { 22.0 };
        let shown = crate::panels::fit_width(&ascii(&line), size, avail);
        cx.row(&shown, color, bold);
    }
}

/// Token color for a count that's bad when non-zero (else dim).
fn warn_if(n: usize, col: Color) -> Color {
    if n > 0 { col } else { DIM }
}

fn page_health(cx: &mut Ctx, r: &HealthReport) {
    cx.heading("PROVINCES (NODES)");
    cx.stat("total", &r.nodes_total.to_string(), INK);
    cx.stat("healthy", &r.nodes_healthy.to_string(), GOOD);
    cx.stat(
        "cordoned",
        &r.nodes_cordoned.to_string(),
        warn_if(r.nodes_cordoned, WARN),
    );
    cx.stat(
        "under pressure",
        &r.nodes_pressure.to_string(),
        warn_if(r.nodes_pressure, WARN),
    );
    cx.stat(
        "NotReady",
        &r.nodes_notready.to_string(),
        warn_if(r.nodes_notready, CRIT),
    );

    cx.heading("CITIZENS (PODS)");
    cx.stat("total", &r.pods_total.to_string(), INK);
    cx.stat("running", &r.pods_running.to_string(), GOOD);
    cx.stat(
        "starting",
        &r.pods_starting.to_string(),
        warn_if(r.pods_starting, STRUCT),
    );
    cx.stat(
        "pending",
        &r.pods_pending.to_string(),
        warn_if(r.pods_pending, WARN),
    );
    cx.stat("terminating", &r.pods_terminating.to_string(), DIM);
    cx.stat(
        "failing",
        &r.pods_failing.to_string(),
        warn_if(r.pods_failing, CRIT),
    );
    cx.stat("succeeded", &r.pods_succeeded.to_string(), DIM);

    cx.heading("CITIES (WORKLOADS)");
    cx.stat("total", &r.workloads_total.to_string(), INK);
    cx.stat(
        "understrength",
        &r.workloads_degraded.to_string(),
        warn_if(r.workloads_degraded, WARN),
    );
    cx.note(
        if r.metrics_live {
            "node gauges: live usage (metrics-server)"
        } else {
            "node gauges: scheduling pressure (requests)"
        },
        DIM,
    );
}

fn page_storage(cx: &mut Ctx, r: &StorageReport) {
    cx.heading("GRANARIES (PVCs)");
    cx.stat("total", &r.total.to_string(), INK);
    cx.stat("bound", &r.bound.to_string(), GOOD);
    cx.stat("pending", &r.pending.to_string(), warn_if(r.pending, WARN));

    cx.heading("PENDING CLAIMS");
    if r.pending_claims.is_empty() {
        cx.note("all claims bound — granaries full", DIM);
    } else {
        for c in &r.pending_claims {
            cx.stat(&format!("{}/{}", c.namespace, c.name), &c.phase, WARN);
        }
    }
}

fn page_network(cx: &mut Ctx, r: &NetworkReport) {
    cx.heading("CONNECTIVITY");
    cx.stat("services (harbors)", &r.services.to_string(), INK);
    cx.stat("ingresses (gates)", &r.ingresses.to_string(), INK);

    cx.heading("ORPHAN GATES (INGRESS)");
    if r.orphan_ingresses.is_empty() {
        cx.note("every gate reaches a service", DIM);
    } else {
        for o in &r.orphan_ingresses {
            cx.stat(&format!("{}/{}", o.namespace, o.name), &o.detail, WARN);
        }
    }

    cx.heading("IDLE HARBORS (SERVICE)");
    if r.idle_services.is_empty() {
        cx.note("every harbor serves a city", DIM);
    } else {
        for s in &r.idle_services {
            cx.stat(&format!("{}/{}", s.namespace, s.name), &s.detail, STRUCT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubernation_core::state::advisor::{RsQos, RsResource};
    use kubernation_core::state::model::WorkloadKind;

    fn over_row(name: &str) -> RsRow {
        RsRow {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: name.into(),
            qos: RsQos::Burstable,
            measured_pods: 1,
            running_pods: 1,
            cpu: RsResource {
                request: 0.5,
                usage: 0.05,
                suggested: Some(0.08),
                verdict: RsVerdict::Over,
                ..Default::default()
            },
            mem: RsResource::default(),
            worst: RsVerdict::Over,
        }
    }

    #[test]
    fn rightsizing_lines_degrade_dark_shows_only_scheduler_blind() {
        let mut blind = over_row("blind");
        blind.qos = RsQos::BestEffort;
        blind.cpu.verdict = RsVerdict::Unrequested;
        blind.cpu.suggested = None;
        blind.worst = RsVerdict::Unrequested;
        let r = RightSizingReport {
            metrics_available: false,
            unrequested: vec![blind],
            ..Default::default()
        };
        let lines = rightsizing_lines(&r);
        assert!(lines[0].0.contains("needs per-pod metrics"));
        assert!(!lines.iter().any(|(s, _)| s.starts_with("RECLAIMABLE")));
        assert!(
            lines
                .iter()
                .any(|(s, role)| s.contains("blind") && *role == RsRole::Crit)
        );
    }

    #[test]
    fn rightsizing_lines_headline_counts_and_caps() {
        let over: Vec<RsRow> = (0..15).map(|i| over_row(&format!("w{i}"))).collect();
        let r = RightSizingReport {
            metrics_available: true,
            workloads_total: 20,
            over,
            right_sized_count: 5,
            reclaimable_cpu: 1.5,
            node_equiv: 0.0,
            ..Default::default()
        };
        let lines = rightsizing_lines(&r);
        assert!(lines[0].0.starts_with("RECLAIMABLE") && lines[0].1 == RsRole::Headline);
        assert!(!lines[0].0.contains("nodes")); // node_equiv 0 → no nodes clause
        assert!(lines.iter().any(|(s, _)| s == "+3 more")); // 15 over → cap 12 + overflow
        assert!(
            lines
                .iter()
                .any(|(s, role)| s.starts_with("over-provisioned: 15") && *role == RsRole::Warn)
        );
    }

    #[test]
    fn hardening_lines_headline_sections_and_honesty() {
        use kubernation_core::state::harden::{Finding, HSeverity, Standard, WorkloadFindings};
        use kubernation_core::state::model::{WorkloadKind, WorkloadRef};

        let wr = |n: &str| WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: n.into(),
        };
        let crit = WorkloadFindings {
            r: wr("bad"),
            worst: HSeverity::Critical,
            findings: vec![Finding {
                rule_id: "HARD01",
                standard: Standard::PssBaseline,
                severity: HSeverity::Critical,
                container: Some("c".into()),
                detail: "privileged: true".into(),
            }],
            unresolved: false,
        };
        let mut report = HardeningReport {
            workloads_total: 3,
            workloads_clean: 2,
            ..Default::default()
        };
        report.critical.push(crit);
        *report.counts_by_standard.entry("PSS-baseline").or_default() += 1;

        let lines = hardening_lines(&report);
        // Headline separates the axes (no misleading clean/total fraction).
        assert!(lines[0].1 == RsRole::Headline);
        assert!(lines[0].0.contains("1 critical") && lines[0].0.contains("2 clean of 3"));
        // Honesty line present.
        assert!(
            lines
                .iter()
                .any(|(s, _)| s.contains("not full PSS compliance"))
        );
        // The critical workload appears under CRITICAL with its standard tag.
        assert!(lines.iter().any(|(s, role)| s.contains("demo/bad")
            && s.contains("[PSS-baseline]")
            && *role == RsRole::Crit));
        // Footer honesty.
        assert!(lines.last().unwrap().0.contains("read-only"));

        // A fully-clean report shows the fortified line.
        let clean = HardeningReport {
            workloads_total: 2,
            workloads_clean: 2,
            ..Default::default()
        };
        assert!(
            hardening_lines(&clean)
                .iter()
                .any(|(s, r)| s.contains("every workload is fortified") && *r == RsRole::Good)
        );
    }
}
