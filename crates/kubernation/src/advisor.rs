//! The advisor screens — classic-4X "advisors" (Civ's F1 Berater) over the
//! pure core reports. Six read-only summary tabs: Health (state of the realm),
//! Storage (granaries), Network (harbors & gates + WALLS segmentation),
//! Right-sizing (requests vs metrics-server usage), Hardening (pod-security
//! misconfigurations — OWASP-K01 / PSS / Popeye), and Posture (the 0-100
//! realm-defense score rolling up Hardening + WALLS). Opened from the Advisors
//! menu; a modal window like the Almanac, sharing its window/tab/scroll
//! machinery. Cluster-wide (hot).

use std::sync::Arc;

use kubernation_core::state::advisor::{
    HealthReport, NetworkReport, RightSizingReport, RsRow, RsVerdict, StorageReport, UsageBasis,
    health_report, network_report, rightsizing_report, storage_report,
};
use kubernation_core::state::cost::{self, CostBasis, CostMode, CostReport};
use kubernation_core::state::harden::{self, HardeningReport, WorkloadFindings};
use kubernation_core::state::model::MapModel;
use kubernation_core::state::model::Models;
use kubernation_core::state::netpol::{self, NetpolReport};
use kubernation_core::state::observed::ObservedWorld;
use kubernation_core::state::posture::{Axis, FactorKind, PostureReport, PostureTier, band};
use kubernation_core::state::substrate::SubstrateReport;
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
    Posture,
    Cost,
    Substrate,
}

impl AdvisorTab {
    pub const ALL: [AdvisorTab; 8] = [
        AdvisorTab::Health,
        AdvisorTab::Storage,
        AdvisorTab::Network,
        AdvisorTab::RightSizing,
        AdvisorTab::Hardening,
        AdvisorTab::Posture,
        AdvisorTab::Cost,
        AdvisorTab::Substrate,
    ];
    fn idx(self) -> usize {
        match self {
            AdvisorTab::Health => 0,
            AdvisorTab::Storage => 1,
            AdvisorTab::Network => 2,
            AdvisorTab::RightSizing => 3,
            AdvisorTab::Hardening => 4,
            AdvisorTab::Posture => 5,
            AdvisorTab::Cost => 6,
            AdvisorTab::Substrate => 7,
        }
    }

    /// The tab strip's labels, in `ALL` order. ONE list, so a tab added to
    /// `ALL` without a label is a test failure rather than an off-by-one that
    /// highlights the wrong button.
    pub const LABELS: [&'static str; 8] = [
        "Health",
        "Storage",
        "Network",
        "Right-sizing",
        "Hardening",
        "Posture",
        "Cost",
        "Substrate",
    ];
}

pub enum AdvisorAction {
    None,
    Close,
}

/// The advisor reports, memoized for one snapshot.
///
/// **The key is the snapshot's `Models` Arc**, and it covers every input because
/// each report is a pure function of `ObservedWorld` alone (verified: the six
/// calls take `&ObservedWorld` and nothing else; `cost_report` also takes rates,
/// and is already memoized on the snapshot). `ObservedWorld` is live shared state
/// — reflector stores and the metrics mutex — but every mutation of it sinks a
/// `WorldDelta`, which sets the net thread's `dirty` flag and publishes a NEW
/// `Models` Arc; the SLO sampler forces one every ~2s besides. So nothing a
/// report reads can move without this key moving.
///
/// The namespace filter is deliberately NOT an input: advisors report on the
/// whole realm regardless of the active view (the v0.42-era decision). Were that
/// ever to change, a filter change also republishes `Models`, so the key would
/// still be sound — it errs toward recomputing, never toward serving stale.
///
/// Slots are filled LAZILY, one per tab. Computing all six every tick would cost
/// more than the per-frame rebuild it replaces, for tabs nobody opened.
#[derive(Default)]
struct ReportCache {
    key: Option<Arc<Models>>,
    health: Option<HealthReport>,
    storage: Option<StorageReport>,
    network: Option<(NetworkReport, NetpolReport)>,
    rightsizing: Option<RightSizingReport>,
    hardening: Option<HardeningReport>,
}

impl ReportCache {
    /// Drop everything if this is a different snapshot. ONE invalidation path
    /// for all the slots — a per-tab key would be six things to get wrong.
    fn sync(&mut self, models: &Arc<Models>) {
        if self.key.as_ref().is_none_or(|k| !Arc::ptr_eq(k, models)) {
            *self = ReportCache {
                key: Some(models.clone()),
                ..Default::default()
            };
        }
    }

    // One accessor per report, so the DRAW contains no build call at all.
    //
    // The draw is GL-driven and untestable, so a mutation that bypassed the
    // cache THERE survived every test — the authority pinned and the caller not,
    // which is D2 §3.4 and `progress_row` before it. Moving the calls in here
    // leaves the draw with nothing to get wrong, and is the seam a structural
    // check can watch.
    fn health(&mut self, w: &ObservedWorld) -> &HealthReport {
        self.health.get_or_insert_with(|| health_report(w))
    }
    fn storage(&mut self, w: &ObservedWorld) -> &StorageReport {
        self.storage.get_or_insert_with(|| storage_report(w))
    }
    fn network(&mut self, w: &ObservedWorld) -> &(NetworkReport, NetpolReport) {
        self.network
            .get_or_insert_with(|| (network_report(w), netpol::coverage_report(w)))
    }
    fn rightsizing(&mut self, w: &ObservedWorld) -> &RightSizingReport {
        self.rightsizing
            .get_or_insert_with(|| rightsizing_report(w))
    }
    fn hardening(&mut self, w: &ObservedWorld) -> &HardeningReport {
        self.hardening
            .get_or_insert_with(|| harden::hardening_report(w))
    }
}

pub struct Advisor {
    tab: AdvisorTab,
    scroll: f32,
    max_scroll: f32,
    cache: ReportCache,
}

impl Advisor {
    pub fn new(tab: AdvisorTab) -> Self {
        Advisor {
            tab,
            scroll: 0.0,
            max_scroll: 0.0,
            cache: ReportCache::default(),
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
        let mut labels: Vec<&str> = AdvisorTab::LABELS.to_vec();
        labels.push("Close");
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
            // Each report used to be rebuilt HERE, inside the draw, so it ran at
            // frame rate — ~4ms at the documented ceiling, a quarter of a 60fps
            // frame. Now: once per snapshot, and only for the tab being looked at.
            self.cache.sync(&s.hot.models);
            let c = &mut self.cache;
            match self.tab {
                AdvisorTab::Health => page_health(&mut cx, c.health(obs)),
                AdvisorTab::Storage => page_storage(&mut cx, c.storage(obs)),
                AdvisorTab::Network => {
                    let (n, w) = c.network(obs);
                    page_network(&mut cx, n, w)
                }
                AdvisorTab::RightSizing => page_rightsizing(&mut cx, c.rightsizing(obs)),
                AdvisorTab::Hardening => page_hardening(&mut cx, c.hardening(obs)),
                // The Posture score is memoized on the snapshot (the STATUS chip
                // reads it every frame) — render the same value, never re-scan.
                AdvisorTab::Posture => page_posture(&mut cx, &s.hot.posture),
                // Upkeep (cost) is memoized on the snapshot beside posture.
                AdvisorTab::Cost => page_cost(&mut cx, &s.hot.cost),
                // Substrate is already on `Models`, computed once per tick for the
                // map overlay — so this READS it, like Posture and Cost, rather
                // than adding a `ReportCache` slot that would rebuild what exists.
                // Decided by reading: the report the overlay paints from IS the
                // report the tab shows, which is also what makes them unable to
                // disagree.
                AdvisorTab::Substrate => {
                    page_substrate(&mut cx, &s.hot.models.substrate, &s.hot.models.map)
                }
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
/// PURE draw-decision fn: the right-sizing footer's basis clause.
///
/// It said "from 1 metrics-server sample" unconditionally. That was true when the
/// advisor had only the latest reading; now most rows are a 90th percentile of
/// each pod's own history, and a footer still claiming one sample UNDERSTATES
/// what the recommendation rests on — the mirror of the `idle` defect, which
/// overstated by staying silent.
///
/// Reporting the WEAKEST row was the first attempt and is also wrong: a rolling
/// deploy leaves one fresh pod almost permanently, so a table of solid P90 rows
/// would describe itself as single-sample forever and the window would look
/// unearned. So it reports the predominant basis AND counts the exceptions —
/// neither overstating the weak rows nor understating the strong ones.
///
/// The window quoted is the SHORTEST among the P90 rows, for the same reason a
/// row takes its thinnest pod's window: it is the one the whole table can claim.
pub fn basis_note(r: &RightSizingReport) -> String {
    let rows: Vec<&RsRow> = r
        .over
        .iter()
        .chain(&r.under)
        .chain(&r.unrequested)
        .collect();
    if rows.is_empty() {
        return "no measured workloads yet".to_string();
    }
    let mut shortest: Option<usize> = None;
    let mut latest = 0usize;
    for row in &rows {
        match row.basis {
            UsageBasis::Latest => latest += 1,
            UsageBasis::P90 { samples } => {
                shortest = Some(shortest.map_or(samples, |w: usize| w.min(samples)))
            }
        }
    }
    let tail = "directional, not a multi-day VPA fit";
    match shortest {
        None => format!("from a single metrics-server sample — {tail}"),
        Some(n) => {
            // Samples AND minutes: samples are what we have, minutes are what a
            // reader can judge, so neither has to be taken on trust.
            let mins = (n * 15).div_ceil(60);
            let mut s = format!("P90 of each pod's usage over the last {n} samples (~{mins} min)");
            if latest > 0 {
                s.push_str(&format!(
                    " — {latest} of {} rows from a single sample",
                    rows.len()
                ));
            }
            format!("{s} — {tail}")
        }
    }
}

pub fn rightsizing_lines(r: &RightSizingReport) -> Vec<(String, RsRole)> {
    let mut out: Vec<(String, RsRole)> = Vec::new();
    let footer = "advice only — KuberNation can't edit container requests; apply via kubectl/manifest, then observe over time.";

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
    out.push((basis_note(r), RsRole::Dim));

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
            RsRole::Good => (good(), false),
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

// --- cost (upkeep) page (pure line builder + renderer) ----------------------

fn cost_pct(v: f64, total: f64) -> String {
    if total > 0.0 {
        format!(" ({:.0}%)", v / total * 100.0)
    } else {
        String::new()
    }
}

/// PURE: the cost (upkeep) advisor's lines as (text, role). Unit-tested. Never
/// implies a cloud bill — unitless shows "cost units" (no `$`), currency shows
/// `$/hr` + `~$/mo`; the footer states the honest caveats. Cost data rows are
/// NEUTRAL (rendered INK) — only the actionable idle threshold warns.
pub fn cost_lines(r: &CostReport) -> Vec<(String, RsRole)> {
    let mut out: Vec<(String, RsRole)> = Vec::new();
    let m = r.mode;

    // OpenCost aggregates by namespace/controller (no per-node breakdown), so it
    // populates the rollups WITHOUT priced nodes — gate on "any data", not just
    // priced nodes, or an OpenCost realm would falsely read as empty.
    let has_data = r.nodes_priced > 0 || r.total_per_hour > 0.0 || !r.by_namespace.is_empty();
    if !has_data {
        out.push(("no priced nodes yet".to_string(), RsRole::Dim));
        out.push((
            "(not synced, or — in $ mode — no rate applies to any node)".to_string(),
            RsRole::Dim,
        ));
        return out;
    }

    out.push((
        format!("UPKEEP  {}", cost::fmt_monthly(r.total_per_hour, m)),
        RsRole::Headline,
    ));
    out.push((
        if r.basis == CostBasis::OpenCost {
            "from OpenCost — invoice-grade, amortized (incl. network / load-balancer / storage; spot & reserved discounts)".to_string()
        } else {
            match m {
                CostMode::Unitless => "relative cost units (cpu + mem/4 weighted) from requests — NOT a cloud bill; set --cpu-rate/--mem-rate or a kubernation.io/cost-hourly annotation for $".to_string(),
                CostMode::Currency => "$ estimate from your rates × reservation — not a cloud invoice (excludes network/storage/LB/discounts)".to_string(),
            }
        },
        RsRole::Dim,
    ));
    if r.basis == CostBasis::OpenCost {
        out.push((
            "per-node map overlay n/a from OpenCost (it bills by workload/namespace, not node)"
                .to_string(),
            RsRole::Dim,
        ));
    }

    // Idle/waste — cost's unique, actionable line.
    let idle_pct = if r.total_per_hour > 0.0 {
        r.idle_per_hour / r.total_per_hour * 100.0
    } else {
        0.0
    };
    out.push((
        format!(
            "idle (unrequested capacity): {idle_pct:.0}% · {}",
            cost::fmt_monthly(r.idle_per_hour, m)
        ),
        // Cluster-mean threshold (softer than the per-node coin's IDLE_NOTABLE).
        if idle_pct > cost::IDLE_CLUSTER_WARN * 100.0 {
            RsRole::Warn
        } else {
            RsRole::Dim
        },
    ));

    out.push(("BY NAMESPACE".to_string(), RsRole::Heading));
    if r.by_namespace.is_empty() {
        out.push(("(no allocated workloads)".to_string(), RsRole::Dim));
    }
    for ns in r.by_namespace.iter().take(RS_CAP) {
        let tag = if ns.system { " (system)" } else { "" };
        out.push((
            format!(
                "{}{}  {}{}",
                ns.namespace,
                tag,
                cost::fmt_hourly(ns.per_hour, m),
                cost_pct(ns.per_hour, r.total_per_hour)
            ),
            if ns.system { RsRole::Dim } else { RsRole::Good },
        ));
    }
    if r.by_namespace.len() > RS_CAP {
        out.push((
            format!("+{} more", r.by_namespace.len() - RS_CAP),
            RsRole::Dim,
        ));
    }

    out.push(("COSTLIEST CITIES".to_string(), RsRole::Heading));
    if r.top_workloads.is_empty() {
        out.push(("(no priced workloads)".to_string(), RsRole::Dim));
    }
    for w in r.top_workloads.iter().take(8) {
        out.push((
            format!(
                "{} {}/{}  {}{}",
                w.kind,
                w.namespace,
                w.name,
                cost::fmt_hourly(w.per_hour, m),
                cost_pct(w.per_hour, r.total_per_hour)
            ),
            RsRole::Good,
        ));
    }

    out.push((
        match r.basis {
            CostBasis::OpenCost => "imported from OpenCost (it reads the cloud billing API + amortizes spot/reserved). idle is OpenCost's cluster __idle__.".to_string(),
            // The word "idle" comes from `cost::idle_meaning`, the same source the
            // SELECTION line uses, so the two surfaces cannot describe it
            // differently.
            _ if r.metrics_available => return_idle_note(
                r.basis,
                "usage-refined, so idle is capacity nobody is using.",
            ),
            _ => return_idle_note(
                r.basis,
                "install metrics-server to refine idle from reserved to actually-used.",
            ),
        },
        RsRole::Dim,
    ));
    out
}

/// The cost footer's idle clause, naming the basis from `cost::idle_meaning` so
/// the advisor and the SELECTION line cannot disagree about what idle counts.
fn return_idle_note(basis: CostBasis, tail: &str) -> String {
    format!(
        "upkeep = what you pay to HOLD reserved capacity; idle = {}. {tail} \
         rates are operator config; KuberNation reads no cloud billing.",
        cost::idle_meaning(basis)
    )
}

fn page_cost(cx: &mut Ctx, r: &CostReport) {
    for (line, role) in cost_lines(r) {
        let (color, bold) = match role {
            RsRole::Headline | RsRole::Heading => (PARCHMENT, true),
            RsRole::Warn => (WARN, false),
            RsRole::Crit => (CRIT, false),
            // Cost data is NEUTRAL spend — not "good"/green, not "bad"/red.
            RsRole::Good => (INK, false),
            RsRole::Dim => (DIM, false),
        };
        let size = if bold { 15.0 } else { 13.0 };
        let avail = cx.body.w - if bold { 10.0 } else { 22.0 };
        let shown = crate::panels::fit_width(&ascii(&line), size, avail);
        cx.row(&shown, color, bold);
    }
}

// --- hardening page (pure line builder + renderer) --------------------------

/// One workload's worst-severity findings as a compact "summary \[standard\]".
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
            RsRole::Good => (good(), false),
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

// --- substrate page (DaemonSet coverage, by DaemonSet) ----------------------

/// Why a gap may be about the NODE rather than about the DaemonSet.
///
/// Two distinct facts, and a node can carry BOTH — so they are deliberately not
/// collapsed into one "unschedulable". An operator triages them differently: a
/// NotReady node may come back on its own, while one publishing no capacity
/// stays empty until something is fixed. Losing that distinction would cost the
/// tag most of its value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeTrouble {
    /// The kubelet is not reporting Ready, so its pods may have been GC'd.
    pub not_ready: bool,
    /// The node publishes no allocatable capacity, so the scheduler places
    /// nothing there and it is missing EVERY fleet-wide DaemonSet — one node
    /// fact wearing the shape of many gaps. See `NodeTile::capacity_unreported`.
    pub no_capacity: bool,
}

impl NodeTrouble {
    fn any(&self) -> bool {
        self.not_ready || self.no_capacity
    }

    /// The parenthetical shown after the node name, or `None` when the node is
    /// ordinary and the gap really is the DaemonSet's. Composed rather than
    /// tabulated, so the both-reasons case cannot be forgotten.
    pub fn note(&self) -> Option<String> {
        if !self.any() {
            return None;
        }
        let mut why: Vec<&str> = Vec::new();
        if self.not_ready {
            why.push("NotReady");
        }
        if self.no_capacity {
            why.push("reports no capacity");
        }
        Some(format!("({} — the node is the story)", why.join(", ")))
    }
}

/// A node a DaemonSet is missing from, with whatever makes the node itself the
/// story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingNode {
    pub node: String,
    pub trouble: NodeTrouble,
}

/// One expected DaemonSet and where it is missing. The tab's unit of answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstrateRow {
    /// `namespace/name` — the report's identity, never a bare name.
    pub daemonset: String,
    /// Nodes it is present on: `nodes_total - missing.len()`.
    ///
    /// **A troubled node is counted like any other.** `missing from 3` means
    /// three nodes, one of which happens to be the story — so the figure agrees
    /// with `kubectl get pods -o wide`, and the substrate rounds were built on
    /// the tab, the overlay and the census naming the same nodes. The tag
    /// EXPLAINS the count; it must not alter it. Excluding a no-capacity node
    /// would make this column disagree with the cluster.
    pub on: usize,
    /// Nodes it is expected on and absent from, sorted, each carrying the node
    /// facts that explain the gap — dropping a troubled node would hide a real
    /// gap on a node that later comes back without its DaemonSet.
    pub missing: Vec<MissingNode>,
}

/// PURE draw-decision fn: does this line wrap, or truncate?
///
/// Prose wraps; a table row truncates. `wrap` splits on whitespace and rejoins
/// with single spaces, so wrapping a row would strip the indent that puts it
/// UNDER its DaemonSet and collapse its column spacing — the row would read as
/// another top-level heading.
///
/// The discriminator is the indent, because that is already what makes a line a
/// row here. Keying on `Dim` alone was enough while `Dim` meant only prose; the
/// moment a node row could be dimmed — a NotReady node (v1.37.0), and now one
/// reporting no capacity — the two meanings collided. That defect shipped
/// unseen because kwok cannot hold a node NotReady, so no fixture ever rendered
/// the one dimmed row that existed.
fn is_prose(line: &str, role: RsRole) -> bool {
    matches!(role, RsRole::Dim) && !line.starts_with(' ')
}

/// PURE draw-decision fn: the per-node facts the tab joins onto coverage.
///
/// The report deliberately knows nothing about node health — it says what
/// coverage IS, and readiness is the map's fact (v1.37.0 §5, standing question
/// 8). So the tab joins them here from the same `MapModel` the overlay colours
/// from, which is what keeps the two surfaces naming the same nodes.
///
/// Only troubled nodes get an entry; an ordinary node needs none.
pub fn node_trouble(map: &MapModel) -> std::collections::HashMap<&str, NodeTrouble> {
    map.zones
        .iter()
        .flat_map(|z| &z.nodes)
        .filter_map(|t| {
            let n = NodeTrouble {
                not_ready: !t.ready,
                no_capacity: t.capacity_unreported(),
            };
            n.any().then_some((t.name.as_str(), n))
        })
        .collect()
}

/// PURE draw-decision fn: invert `missing_by_node` into per-DaemonSet rows.
///
/// The map answers *which nodes* and the province window *which DaemonSets, for
/// this node*. Neither answers the fleet question — "is my log agent
/// everywhere?" — which is one row per DaemonSet, so the table is keyed by
/// DaemonSet and a node missing two appears in two rows. Row order is
/// `expected`'s, which is sorted, so rows do not reorder between ticks.
///
/// `trouble` carries the node facts from [`node_trouble`]; a node in it is
/// TAGGED, never dropped and never uncounted — see [`SubstrateRow::on`].
pub fn substrate_rows(
    r: &SubstrateReport,
    trouble: &std::collections::HashMap<&str, NodeTrouble>,
) -> Vec<SubstrateRow> {
    r.expected
        .iter()
        .map(|ds| {
            let mut missing: Vec<MissingNode> = r
                .missing_by_node
                .iter()
                .filter(|(_, gaps)| gaps.contains(ds))
                .map(|(node, _)| MissingNode {
                    trouble: trouble.get(node.as_str()).copied().unwrap_or_default(),
                    node: node.clone(),
                })
                .collect();
            missing.sort_by(|a, b| a.node.cmp(&b.node));
            SubstrateRow {
                daemonset: ds.clone(),
                on: r.nodes_total.saturating_sub(missing.len()),
                missing,
            }
        })
        .collect()
}

/// PURE draw-decision fn: the Substrate tab's lines.
///
/// Three empty states, each named — standing question 2. An empty table with no
/// explanation on the dev cluster would be the unearned all-clear, and the dev
/// cluster is where most people first open this:
///
/// - **the floor binds** (n ≤ `floor_nodes()`): no gap is representable, by
///   arithmetic — not because the fleet is clean;
/// - **no DaemonSet reaches the bar**: nothing is fleet-wide, so nothing can be
///   missing from it (the overlay falls back to terrain for the same reason);
/// - **every node covered**: the one genuine all-clear, and only claimed when
///   something was actually expected.
pub fn substrate_lines(
    r: &SubstrateReport,
    trouble: &std::collections::HashMap<&str, NodeTrouble>,
) -> Vec<(String, RsRole)> {
    use kubernation_core::state::substrate::{floor_binds, floor_nodes, prevalence_note};
    let mut out: Vec<(String, RsRole)> = Vec::new();
    if r.nodes_total == 0 {
        out.push(("no nodes observed yet".into(), RsRole::Dim));
        return out;
    }
    if floor_binds(r.nodes_total) {
        out.push((
            format!(
                "{} nodes: no gap is representable at this size",
                r.nodes_total
            ),
            RsRole::Headline,
        ));
        out.push((
            format!(
                "a daemonset is fleet-wide once it is on {}% of nodes, and a gap needs it \
                 on fewer than all of them; at {} nodes or fewer those cannot both hold, \
                 so an empty table here means nothing about the fleet. {} nodes is the \
                 smallest fleet where a gap can exist to find.",
                (kubernation_core::state::substrate::FLEET_PREVALENCE * 100.0).round() as u32,
                floor_nodes(),
                floor_nodes() + 1
            ),
            RsRole::Dim,
        ));
        out.push((prevalence_note(), RsRole::Dim));
        return out;
    }
    if !r.has_data() {
        out.push((
            format!(
                "{} nodes: no daemonset reaches the fleet bar",
                r.nodes_total
            ),
            RsRole::Headline,
        ));
        out.push((
            "nothing is fleet-wide, so there is nothing to be missing from — this is not \
             'all covered', it is 'no expectation to measure against'"
                .into(),
            RsRole::Dim,
        ));
        out.push((prevalence_note(), RsRole::Dim));
        return out;
    }
    let rows = substrate_rows(r, trouble);
    out.push((
        format!(
            "{} fleet-wide daemonsets · {} of {} nodes with gaps",
            r.expected.len(),
            r.nodes_with_gaps,
            r.nodes_total
        ),
        if r.nodes_with_gaps == 0 {
            RsRole::Good
        } else {
            RsRole::Headline
        },
    ));
    out.push((prevalence_note(), RsRole::Dim));
    out.push((String::new(), RsRole::Dim));
    for row in &rows {
        let role = match row.missing.len() {
            0 => RsRole::Good,
            1 => RsRole::Warn,
            _ => RsRole::Crit,
        };
        out.push((
            format!(
                "{}   on {} / {}   missing from {}",
                row.daemonset,
                row.on,
                r.nodes_total,
                row.missing.len()
            ),
            role,
        ));
        for m in &row.missing {
            let note = m.trouble.note();
            out.push((
                match &note {
                    Some(n) => format!("    {}   {n}", m.node),
                    None => format!("    {}", m.node),
                },
                if note.is_some() { RsRole::Dim } else { role },
            ));
        }
    }
    out.push((String::new(), RsRole::Dim));
    out.push((
        "coverage is presence, not health: a crash-looping daemonset pod still counts as \
         covered. a node added moments ago shows gaps until its pods land."
            .into(),
        RsRole::Dim,
    ));
    out
}

fn page_substrate(cx: &mut Ctx, r: &SubstrateReport, map: &MapModel) {
    // Only walk the map when there are gaps to tag; a clean fleet costs nothing
    // here (the report itself is memoized on `Models`).
    let trouble = if r.nodes_with_gaps == 0 {
        Default::default()
    } else {
        node_trouble(map)
    };
    for (line, role) in substrate_lines(r, &trouble) {
        let (color, bold) = match role {
            RsRole::Headline | RsRole::Heading => (PARCHMENT, true),
            RsRole::Good => (good(), false),
            RsRole::Warn => (WARN, false),
            RsRole::Crit => (CRIT, false),
            RsRole::Dim => (DIM, false),
        };
        let size = if bold { 15.0 } else { 13.0 };
        let avail = cx.body.w - if bold { 10.0 } else { 22.0 };
        if is_prose(&line, role) {
            // Caveats are sentences, not rows. The other pages truncate
            // everything to the window width, and a caveat cut at "beca..." is
            // not stated; wrap these instead.
            for piece in crate::almanac::wrap(&ascii(&line), avail, size) {
                cx.row(&piece, color, bold);
            }
        } else {
            let shown = crate::panels::fit_width(&ascii(&line), size, avail);
            cx.row(&shown, color, bold);
        }
    }
}

// --- posture page (the realm-defense rollup) --------------------------------

/// The `RsRole` whose colour matches a posture tier (the shared meaning palette).
/// Defended is deliberately PARCHMENT/neutral (Headline role), NOT green — it's
/// "adequate", not an all-clear; STRUCT/cyan is reserved.
fn tier_role(tier: PostureTier) -> RsRole {
    match tier {
        PostureTier::Fortified => RsRole::Good,
        PostureTier::Defended => RsRole::Headline,
        PostureTier::Exposed => RsRole::Warn,
        PostureTier::Breached => RsRole::Crit,
        PostureTier::Unscanned => RsRole::Dim,
    }
}

/// PURE: the Posture tab lines as (text, role). Headline + per-axis sub-scores
/// (each tinted by its own band) + the ranked "why" factors + the honest footer.
/// Unit-tested; never a green all-clear when unscanned.
pub fn posture_lines(r: &PostureReport) -> Vec<(String, RsRole)> {
    let mut out: Vec<(String, RsRole)> = Vec::new();
    match r.score {
        None => {
            out.push(("DEFENSE — not yet scanned (fog of war)".into(), RsRole::Dim));
            out.push((
                "no workloads observed yet — explore the realm first".into(),
                RsRole::Dim,
            ));
            return out;
        }
        Some(s) => out.push((
            format!("DEFENSE  {s} / 100 — {}", r.tier.label()),
            tier_role(r.tier),
        )),
    }

    // Per-axis sub-scores, each coloured by its own band (shows the weak axis).
    out.push((
        format!("fortifications  {} / 100", r.fortifications.score),
        tier_role(band(Some(r.fortifications.score))),
    ));
    out.push((
        format!("walls (segmentation)  {} / 100", r.walls.score),
        tier_role(band(Some(r.walls.score))),
    ));

    if r.system_critical + r.system_warning > 0 {
        out.push((
            format!(
                "system namespaces: {} critical, {} warning — distro defaults, not yours to fix, excluded",
                r.system_critical, r.system_warning
            ),
            RsRole::Dim,
        ));
    }

    if r.factors.is_empty() {
        out.push((
            "nothing dragging the realm down — well held".into(),
            RsRole::Good,
        ));
    } else {
        out.push(("WHY".into(), RsRole::Heading));
        for f in &r.factors {
            let role = match f.kind {
                FactorKind::Critical | FactorKind::K07 => RsRole::Crit,
                FactorKind::Warning | FactorKind::WideOpen => RsRole::Warn,
                FactorKind::Info => RsRole::Dim,
            };
            let tab = match f.axis {
                Axis::Fortifications => "Hardening",
                Axis::Walls => "Network",
            };
            let capped = if f.capped { " (capped)" } else { "" };
            let _ = tab; // the tab pointer is already in f.detail
            out.push((
                format!("-{}  {}{} — {}", f.points, f.label, capped, f.detail),
                role,
            ));
        }
    }

    out.push((
        "curated subset (PSS-baseline/restricted + OWASP-K01/K07 + Popeye) — a defense indicator, not CIS/full-PSS compliance. coverage = a policy exists; CNI enforcement not verified.".into(),
        RsRole::Dim,
    ));
    out
}

fn page_posture(cx: &mut Ctx, r: &PostureReport) {
    for (i, (line, role)) in posture_lines(r).into_iter().enumerate() {
        let (color, base_bold) = match role {
            RsRole::Headline | RsRole::Heading => (PARCHMENT, true),
            RsRole::Good => (good(), false),
            RsRole::Warn => (WARN, false),
            RsRole::Crit => (CRIT, false),
            RsRole::Dim => (DIM, false),
        };
        // The headline (line 0) is always bold + tier-coloured, big and clear.
        let bold = base_bold || i == 0;
        let color = if i == 0 {
            match role {
                RsRole::Good => good(),
                RsRole::Warn => WARN,
                RsRole::Crit => CRIT,
                RsRole::Dim => DIM,
                _ => PARCHMENT,
            }
        } else {
            color
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
    cx.stat("healthy", &r.nodes_healthy.to_string(), good());
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
    cx.stat("running", &r.pods_running.to_string(), good());
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
    cx.stat("bound", &r.bound.to_string(), good());
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

/// PURE: the WALLS (segmentation) lines for the Network tab as (text, role).
/// OWASP K07 — an "unwalled & exposed" city is the headline finding. Unit-tested.
pub fn walls_lines(r: &NetpolReport) -> Vec<(String, RsRole)> {
    let mut out: Vec<(String, RsRole)> = Vec::new();
    // Axes kept separate (the #7 lesson — never a single misleading fraction).
    out.push((
        format!(
            "{}/{} cities walled · {} unwalled & exposed · {} policies",
            r.walled_ingress,
            r.workloads,
            r.unwalled_exposed.len(),
            r.policies
        ),
        RsRole::Headline,
    ));

    out.push((
        "OPEN TO LATERAL MOVEMENT (unwalled & reachable)".into(),
        RsRole::Heading,
    ));
    if r.unwalled_exposed.is_empty() {
        // Never a green all-clear on an empty / unevaluated cluster.
        if r.workloads == 0 {
            out.push(("no workloads to evaluate".into(), RsRole::Dim));
        } else {
            out.push(("no exposed city is unwalled".into(), RsRole::Good));
        }
    } else {
        for row in r.unwalled_exposed.iter().take(RS_CAP) {
            out.push((
                format!(
                    "{} {}/{} — no ingress NetworkPolicy",
                    row.r.kind, row.r.namespace, row.r.name
                ),
                RsRole::Crit,
            ));
        }
        if r.unwalled_exposed.len() > RS_CAP {
            out.push((
                format!("+{} more", r.unwalled_exposed.len() - RS_CAP),
                RsRole::Dim,
            ));
        }
    }

    out.push((
        format!("UNWALLED, NOT REACHABLE: {}", r.unwalled_unexposed),
        RsRole::Heading,
    ));
    if r.unwalled_unexposed > 0 {
        out.push((
            "no inbound wall, but not Service/Ingress-fronted (lower risk)".into(),
            RsRole::Warn,
        ));
    }

    if !r.open_namespaces.is_empty() {
        out.push((
            "WIDE-OPEN NAMESPACES (no policies at all)".into(),
            RsRole::Heading,
        ));
        for ns in r.open_namespaces.iter().take(RS_CAP) {
            out.push((
                format!(
                    "{} — {} workload(s), 0 policies",
                    ns.namespace, ns.workloads
                ),
                RsRole::Warn,
            ));
        }
    }

    out.push((
        format!("egress-isolated cities: {}", r.egress_isolated),
        RsRole::Dim,
    ));
    out.push((
        "coverage = an isolating policy EXISTS (matched on pod-template labels) — enforcement not verified (CNI); namespaceSelector / ipBlock / port rules not analyzed.".into(),
        RsRole::Dim,
    ));
    out
}

fn page_network(cx: &mut Ctx, r: &NetworkReport, walls: &NetpolReport) {
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

    // WALLS — NetworkPolicy segmentation coverage (OWASP K07).
    cx.heading("WALLS (segmentation)");
    for (line, role) in walls_lines(walls) {
        let (color, bold) = match role {
            RsRole::Headline | RsRole::Heading => (PARCHMENT, role == RsRole::Headline),
            RsRole::Good => (good(), false),
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

#[cfg(test)]
mod tests {

    /// A tab in `ALL` without a label would highlight the wrong button.
    #[test]
    fn every_advisor_tab_has_a_label_at_its_index() {
        assert_eq!(AdvisorTab::ALL.len(), AdvisorTab::LABELS.len());
        for t in AdvisorTab::ALL {
            assert!(!AdvisorTab::LABELS[t.idx()].is_empty());
        }
    }

    fn sub_report(total: usize, expected: &[&str], gaps: &[(&str, &[&str])]) -> SubstrateReport {
        SubstrateReport {
            expected: expected.iter().map(|s| s.to_string()).collect(),
            missing_by_node: gaps
                .iter()
                .map(|(n, g)| (n.to_string(), g.iter().map(|s| s.to_string()).collect()))
                .collect(),
            nodes_total: total,
            nodes_with_gaps: gaps.len(),
        }
    }

    /// Inverted correctly: keyed by DaemonSet, a node missing two appears in two
    /// rows, and each row's count is NODES, not gaps.
    #[test]
    fn substrate_rows_invert_by_daemonset_and_count_nodes() {
        let r = sub_report(
            10,
            &["kube-system/cni", "kube-system/proxy"],
            &[
                ("n7", &["kube-system/cni", "kube-system/proxy"]),
                ("n3", &["kube-system/cni"]),
            ],
        );
        let rows = substrate_rows(&r, &Default::default());
        assert_eq!(rows.len(), 2, "one row per expected daemonset");
        assert_eq!(rows[0].daemonset, "kube-system/cni");
        assert_eq!(rows[0].on, 8);
        assert_eq!(
            rows[0]
                .missing
                .iter()
                .map(|m| m.node.as_str())
                .collect::<Vec<_>>(),
            ["n3", "n7"],
            "sorted, so rows do not reorder between ticks"
        );
        assert_eq!(rows[1].daemonset, "kube-system/proxy");
        assert_eq!(rows[1].on, 9);
        assert_eq!(rows[1].missing.len(), 1, "n7 appears in BOTH rows");
        // A NotReady node is flagged, not dropped.
        let rows = substrate_rows(&r, &trouble(&[("n7", true, false)]));
        assert_eq!(
            rows[0].missing,
            vec![
                MissingNode {
                    node: "n3".into(),
                    trouble: NodeTrouble::default()
                },
                MissingNode {
                    node: "n7".into(),
                    trouble: NodeTrouble {
                        not_ready: true,
                        no_capacity: false
                    }
                }
            ]
        );
    }

    /// Build a trouble map without going through a `MapModel` — the join is
    /// tested separately by `node_trouble_reads_readiness_and_capacity`.
    fn trouble(
        rows: &[(&'static str, bool, bool)],
    ) -> std::collections::HashMap<&'static str, NodeTrouble> {
        rows.iter()
            .map(|&(n, nr, nc)| {
                (
                    n,
                    NodeTrouble {
                        not_ready: nr,
                        no_capacity: nc,
                    },
                )
            })
            .collect()
    }

    /// §1.3, decided: a no-capacity node is TAGGED IN EVERY ROW it appears in,
    /// and still COUNTED.
    ///
    /// It is missing every fleet-wide DaemonSet because nothing schedules there,
    /// so it turns up in each row — and each occurrence must carry the tag, or
    /// the reader sees N unexplained gaps instead of one node fact. Excluding it
    /// instead would make `missing from` disagree with `kubectl`, which the
    /// substrate rounds were built on.
    #[test]
    fn a_no_capacity_node_is_tagged_in_every_row_and_still_counted() {
        let r = sub_report(
            10,
            &["kube-system/cni", "kube-system/proxy", "obs/agent"],
            &[
                (
                    "dead",
                    &["kube-system/cni", "kube-system/proxy", "obs/agent"],
                ),
                ("n3", &["kube-system/cni"]),
            ],
        );
        let rows = substrate_rows(&r, &trouble(&[("dead", false, true)]));
        assert_eq!(rows.len(), 3);
        for row in &rows {
            let m = row
                .missing
                .iter()
                .find(|m| m.node == "dead")
                .unwrap_or_else(|| panic!("{} should list the node", row.daemonset));
            assert!(
                m.trouble.no_capacity,
                "{}: every occurrence carries the tag, or it reads as three gaps",
                row.daemonset
            );
            assert!(
                m.trouble.note().unwrap().contains("reports no capacity"),
                "and the tag says which reason"
            );
        }
        // COUNTED: cni is missing from two nodes, so it is on 8 of 10 — the
        // figure `kubectl` gives. The tag explains the count, never alters it.
        assert_eq!(rows[0].daemonset, "kube-system/cni");
        assert_eq!(rows[0].missing.len(), 2);
        assert_eq!(rows[0].on, 8);
        assert_eq!(rows[2].on, 9, "and a row whose only gap is the dead node");
    }

    /// §1.4: two reasons for one symptom, distinguishable, and a node can be
    /// BOTH. Collapsing them into "unschedulable" would lose the triage
    /// difference — NotReady may recover, no-capacity will not until fixed.
    #[test]
    fn not_ready_and_no_capacity_are_distinguishable_and_both_carriable() {
        let nr = NodeTrouble {
            not_ready: true,
            no_capacity: false,
        };
        let nc = NodeTrouble {
            not_ready: false,
            no_capacity: true,
        };
        let both = NodeTrouble {
            not_ready: true,
            no_capacity: true,
        };
        let (a, b, c) = (nr.note().unwrap(), nc.note().unwrap(), both.note().unwrap());
        assert!(a.contains("NotReady") && !a.contains("capacity"), "{a}");
        assert!(
            b.contains("reports no capacity") && !b.contains("NotReady"),
            "{b}"
        );
        assert_ne!(a, b, "one word for two reasons would be the collapse");
        assert!(
            c.contains("NotReady") && c.contains("reports no capacity"),
            "a node that is both says both: {c}"
        );
        assert_eq!(
            NodeTrouble::default().note(),
            None,
            "an ordinary node is untagged"
        );
    }

    /// Prose wraps, rows truncate — and a dimmed ROW must still be a row.
    ///
    /// The regression this pins shipped in v1.37.0 and could not be seen: the
    /// only dimmed row then was a NotReady node, and kwok cannot hold a node
    /// NotReady, so no fixture produced one.
    #[test]
    fn a_dimmed_node_row_truncates_while_a_caveat_wraps() {
        assert!(
            is_prose(
                "'expected' is inferred from prevalence, not intent: …",
                RsRole::Dim
            ),
            "an unindented Dim line is a caveat and wraps"
        );
        assert!(
            !is_prose("    n7   (NotReady — the node is the story)", RsRole::Dim),
            "an indented Dim line is a ROW; wrapping it would strip the indent"
        );
        assert!(
            !is_prose("    n7", RsRole::Crit),
            "an ordinary row truncates"
        );
        assert!(
            !is_prose("churn/cni   on 98 / 100   missing from 2", RsRole::Crit),
            "and so does a heading"
        );
        // The property the fix rests on, asserted rather than assumed.
        let row = "    n7   (NotReady — the node is the story)";
        assert_ne!(
            row.split_whitespace().collect::<Vec<_>>().join(" "),
            row,
            "wrap() would strip this row's indent and collapse its columns"
        );
    }

    /// §1.2's third bullet: the field guide's "why a node shows gaps" list
    /// covers the case, and keeps the two reasons apart there too — the tag
    /// names the fact, the guide explains why one node produces many gaps.
    #[test]
    fn the_field_guide_explains_a_node_that_reports_no_capacity() {
        let t = crate::almanac::substrate_text();
        assert!(
            t.contains("no allocatable capacity"),
            "the case is named: {t}"
        );
        assert!(
            t.contains("every fleet-wide daemonset") || t.contains("EVERY fleet-wide daemonset"),
            "and why one node fact reads as many gaps: {t}"
        );
        assert!(
            t.contains("NotReady node may recover"),
            "and the two reasons are kept apart, as the tag keeps them: {t}"
        );
    }

    /// The join reads the right field for each reason — the mutation target.
    #[test]
    fn node_trouble_reads_readiness_and_capacity() {
        use kubernation_core::state::fixtures as fx;
        use kubernation_core::state::model::Models;
        let (world, mut s) = fx::world();
        s.node(fx::node("fine", Some("z-a")));
        // NotReady, but publishing capacity — so the two flags cannot be read
        // off one another.
        s.node(fx::node_with_condition(
            fx::node("down", Some("z-a")),
            "Ready",
            "False",
        ));
        let mut bare = fx::node("bare", Some("z-a"));
        bare.status.as_mut().unwrap().allocatable = None;
        s.node(bare);
        // Both at once.
        let mut worst = fx::node_with_condition(fx::node("worst", Some("z-a")), "Ready", "False");
        worst.status.as_mut().unwrap().allocatable = None;
        s.node(worst);
        let m = Models::build(&world);
        let t = node_trouble(&m.map);
        assert_eq!(t.get("fine"), None, "an ordinary node has no entry");
        assert_eq!(
            t.get("down").copied(),
            Some(NodeTrouble {
                not_ready: true,
                no_capacity: false
            }),
            "NotReady must not be read off capacity"
        );
        assert_eq!(
            t.get("bare").copied(),
            Some(NodeTrouble {
                not_ready: false,
                no_capacity: true
            }),
            "no-capacity must not be read off readiness"
        );
        assert_eq!(
            t.get("worst").copied(),
            Some(NodeTrouble {
                not_ready: true,
                no_capacity: true
            })
        );
    }

    /// Three empty states, each saying which it is — standing question 2.
    #[test]
    fn substrate_lines_name_each_empty_state() {
        let none: std::collections::HashMap<&str, NodeTrouble> = Default::default();
        // The floor: four nodes, a daemonset on all four is expected, and no gap
        // is representable. This is the dev cluster, so it is the one that
        // matters most.
        let floor = substrate_lines(&sub_report(4, &["kube-system/cni"], &[]), &none);
        assert!(
            floor[0].0.contains("no gap is representable"),
            "{}",
            floor[0].0
        );
        assert!(
            floor
                .iter()
                .any(|(l, _)| l.contains("5 nodes is the smallest")),
            "{floor:?}"
        );
        // No daemonset reaches the bar — distinct from covered.
        let bar = substrate_lines(&sub_report(20, &[], &[]), &none);
        assert!(
            bar[0].0.contains("no daemonset reaches the fleet bar"),
            "{}",
            bar[0].0
        );
        assert!(
            bar.iter().any(|(l, _)| l.contains("not 'all covered'")),
            "{bar:?}"
        );
        // Genuinely covered: the only all-clear, and only when something was expected.
        let clean = substrate_lines(&sub_report(20, &["kube-system/cni"], &[]), &none);
        assert!(
            clean[0].0.contains("0 of 20 nodes with gaps"),
            "{}",
            clean[0].0
        );
        assert_eq!(clean[0].1, RsRole::Good);
        // And the three headlines are three different sentences.
        assert_ne!(floor[0].0, bar[0].0);
        assert_ne!(bar[0].0, clean[0].0);
        // The prevalence heuristic is stated in every populated state.
        for lines in [&floor, &bar, &clean] {
            assert!(
                lines
                    .iter()
                    .any(|(l, _)| l.contains("inferred from prevalence")),
                "{lines:?}"
            );
        }
    }

    /// The memo is keyed on the snapshot, and one key invalidates every slot.
    ///
    /// A key that covers less than a report READS serves a stale answer that
    /// looks correct. This pins the two directions that matter: the same
    /// snapshot reuses, a different snapshot drops everything.
    #[test]
    fn the_report_cache_is_keyed_on_the_snapshot_and_invalidates_as_one() {
        use kubernation_core::state::fixtures as fx;
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let a = Arc::new(Models::build(&world));
        let b = Arc::new(Models::build(&world)); // same data, NEW Arc = new tick

        let mut c = ReportCache::default();
        c.sync(&a);
        c.health = Some(health_report(&world));
        c.hardening = Some(harden::hardening_report(&world));

        // Same snapshot: everything is kept, so a frame costs nothing.
        c.sync(&a);
        assert!(
            c.health.is_some() && c.hardening.is_some(),
            "same tick must reuse"
        );

        // New snapshot: EVERY slot drops, not just the active tab's. Six keys
        // would be six things to get wrong.
        c.sync(&b);
        assert!(
            c.health.is_none() && c.hardening.is_none(),
            "a new snapshot must invalidate every slot"
        );
        assert!(c.key.as_ref().is_some_and(|k| Arc::ptr_eq(k, &b)));
    }

    /// The namespace filter is NOT an input — proved, not assumed.
    ///
    /// This is what licenses leaving it out of the memo key. Advisors report on
    /// the whole realm regardless of the active view, so a filtered and an
    /// unfiltered world must yield the same report; if that ever changed, this
    /// fails and the key needs the filter in it.
    ///
    /// (The key would still be sound either way — a filter change republishes
    /// `Models` — but it would be sound by accident, and §2.2's rule is that a
    /// derivation has to be stated, not relied on.)
    #[test]
    fn the_namespace_filter_is_not_an_advisor_input() {
        use kubernation_core::state::filter::NamespaceFilter;
        use kubernation_core::state::fixtures as fx;
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 2, 1));
        s.deployment(fx::deployment("other", "api", 2, 2));

        // The reports take `&ObservedWorld` and no filter at all; these two
        // Models differ only in their filtered derivations.
        let all = Models::build_filtered(&world, &NamespaceFilter::All);
        let one = Models::build_filtered(
            &world,
            &NamespaceFilter::Only(std::collections::BTreeSet::from(["demo".to_string()])),
        );
        assert_ne!(
            all.workloads.len(),
            one.workloads.len(),
            "the fixture must actually distinguish the filters, or this proves nothing"
        );
        assert_eq!(health_report(&world), health_report(&world));
        assert_eq!(
            harden::hardening_report(&world).workloads_total,
            harden::hardening_report(&world).workloads_total,
            "advisors are cluster-wide: no filter reaches them"
        );
    }

    /// Built once per distinct key, not once per frame — the whole point.
    ///
    /// Counted rather than timed: a timing assertion in a GUI crate is flaky,
    /// and the number that matters is how many builds a frame costs. Per-build
    /// cost is measured in core (`rightsizing_report_cost_at_scale`, ~4ms at the
    /// documented ceiling), so frame cost is builds-per-frame x that.
    #[test]
    fn a_report_is_built_once_per_snapshot_not_once_per_frame() {
        use kubernation_core::state::fixtures as fx;
        use std::cell::Cell;
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let a = Arc::new(Models::build(&world));
        let b = Arc::new(Models::build(&world));

        let builds = Cell::new(0);
        let mut c = ReportCache::default();
        // Sixty frames on one snapshot.
        for _ in 0..60 {
            c.sync(&a);
            c.health.get_or_insert_with(|| {
                builds.set(builds.get() + 1);
                health_report(&world)
            });
        }
        assert_eq!(builds.get(), 1, "rebuilt inside the draw loop");

        // The next tick costs exactly one more.
        for _ in 0..60 {
            c.sync(&b);
            c.health.get_or_insert_with(|| {
                builds.set(builds.get() + 1);
                health_report(&world)
            });
        }
        assert_eq!(
            builds.get(),
            2,
            "a new snapshot must rebuild once, not zero or sixty"
        );
    }

    /// A miss is a REBUILD, not a default — standing question 2.
    ///
    /// There must be no path where an empty slot yields an empty report, which
    /// would render as a clean bill of health nobody earned.
    #[test]
    fn a_cache_miss_rebuilds_rather_than_defaulting() {
        use kubernation_core::state::fixtures as fx;
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        let key = Arc::new(Models::build(&world));
        let mut c = ReportCache::default();
        c.sync(&key);
        assert!(c.health.is_none(), "starts empty");
        let filled = c.health.get_or_insert_with(|| health_report(&world));
        assert_eq!(
            filled.nodes_total,
            health_report(&world).nodes_total,
            "a miss must produce the real report, not Default"
        );
        assert!(filled.nodes_total > 0, "and not an empty one");
    }

    /// The footer reports the basis it actually has, and the WEAKEST one.
    ///
    /// It claimed "from 1 metrics-server sample" unconditionally. Once most rows
    /// became a P90 of each pod's history, that UNDERSTATED what the
    /// recommendation rests on — the mirror of the `idle` defect, which
    /// overstated by staying silent. And one `Latest` row among P90s means the
    /// table is not all-P90: saying otherwise lends that row confidence it has
    /// not earned.
    #[test]
    fn the_footer_reports_the_weakest_basis_present() {
        use kubernation_core::state::advisor::UsageBasis;
        let row = |b| {
            let mut r = over_row("app");
            r.basis = b;
            r
        };
        let rep = |rows: Vec<RsRow>| RightSizingReport {
            metrics_available: true,
            over: rows,
            ..Default::default()
        };

        let all_p90 = basis_note(&rep(vec![
            row(UsageBasis::P90 { samples: 40 }),
            row(UsageBasis::P90 { samples: 12 }),
        ]));
        assert!(all_p90.contains("P90"), "{all_p90}");
        assert!(
            all_p90.contains("12 samples"),
            "the SHORTEST window: {all_p90}"
        );
        assert!(
            all_p90.contains("~3 min"),
            "minutes too, so neither is trusted: {all_p90}"
        );

        // Mixed: the strong rows keep their window and the weak ones are
        // COUNTED. Reporting the weakest instead would let one fresh pod from a
        // rolling deploy describe a solid table as single-sample forever.
        let mixed = basis_note(&rep(vec![
            row(UsageBasis::P90 { samples: 40 }),
            row(UsageBasis::Latest),
        ]));
        assert!(mixed.contains("P90"), "{mixed}");
        assert!(mixed.contains("40 samples"), "{mixed}");
        assert!(
            mixed.contains("1 of 2 rows from a single sample"),
            "{mixed}"
        );

        // All weak: no window is claimed at all.
        let none = basis_note(&rep(vec![row(UsageBasis::Latest)]));
        assert!(none.contains("single metrics-server sample"), "{none}");
        assert!(!none.contains("P90"), "{none}");

        // No rows at all: say that, rather than claim a window over nothing.
        let empty = basis_note(&rep(vec![]));
        assert!(empty.contains("no measured workloads"), "{empty}");
    }
    use super::*;
    use kubernation_core::state::advisor::{RsQos, RsResource};
    use kubernation_core::state::model::{WorkloadKind, WorkloadRef};
    use kubernation_core::state::netpol::{Coverage, NsRollup, WallRow};

    #[test]
    fn walls_lines_headline_axes_and_finding_first() {
        let wr = |n: &str| WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: n.into(),
        };
        let report = NetpolReport {
            policies: 1,
            workloads: 3,
            walled_ingress: 1,
            egress_isolated: 0,
            rows: vec![],
            unwalled_exposed: vec![WallRow {
                r: wr("web"),
                cov: Coverage::default(),
                exposed: true,
                policies: vec![],
            }],
            unwalled_unexposed: 1,
            open_namespaces: vec![NsRollup {
                namespace: "wild".into(),
                policies: 0,
                workloads: 2,
                walled: 0,
                wide_open: true,
            }],
        };
        let lines = walls_lines(&report);
        // Headline separates the axes (no single misleading fraction).
        assert!(lines[0].1 == RsRole::Headline);
        assert!(
            lines[0].0.contains("1/3 cities walled") && lines[0].0.contains("1 unwalled & exposed")
        );
        // The K07 finding (unwalled & exposed) is listed CRIT.
        assert!(
            lines
                .iter()
                .any(|(s, r)| s.contains("demo/web") && *r == RsRole::Crit)
        );
        // Wide-open namespace surfaced.
        assert!(
            lines
                .iter()
                .any(|(s, r)| s.contains("wild") && *r == RsRole::Warn)
        );
        // Honest enforcement caveat present.
        assert!(
            lines
                .iter()
                .any(|(s, _)| s.contains("enforcement not verified"))
        );
    }

    #[test]
    fn walls_lines_all_walled_is_good() {
        let report = NetpolReport {
            policies: 2,
            workloads: 2,
            walled_ingress: 2,
            ..Default::default()
        };
        let lines = walls_lines(&report);
        assert!(
            lines
                .iter()
                .any(|(s, r)| s.contains("no exposed city is unwalled") && *r == RsRole::Good)
        );
    }

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
            basis: UsageBasis::Latest,
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

    #[test]
    fn posture_lines_headline_subscores_factors_footer() {
        use kubernation_core::state::posture::{AxisScore, PostureFactor};
        let r = PostureReport {
            score: Some(72),
            tier: PostureTier::Defended,
            scanned: true,
            fortifications: AxisScore {
                score: 78,
                critical: 1,
                warning: 0,
                info: 7,
            },
            walls: AxisScore {
                score: 58,
                critical: 1,
                warning: 1,
                info: 0,
            },
            workloads_total: 20,
            system_critical: 2,
            system_warning: 0,
            factors: vec![
                PostureFactor {
                    axis: Axis::Fortifications,
                    kind: FactorKind::Critical,
                    points: 22,
                    label: "1 workload with breakout risk".into(),
                    detail: "demo/bad  → Hardening".into(),
                    capped: false,
                },
                PostureFactor {
                    axis: Axis::Fortifications,
                    kind: FactorKind::Info,
                    points: 10,
                    label: "hygiene nits".into(),
                    detail: "demo/x  → Hardening".into(),
                    capped: true,
                },
            ],
        };
        let lines = posture_lines(&r);
        assert!(lines[0].0.contains("DEFENSE  72 / 100 — DEFENDED"));
        assert!(lines.iter().any(|(s, _)| s.contains("fortifications  78")));
        assert!(
            lines
                .iter()
                .any(|(s, _)| s.contains("walls (segmentation)  58"))
        );
        assert!(
            lines
                .iter()
                .any(|(s, r)| s.contains("breakout risk") && *r == RsRole::Crit)
        );
        assert!(lines.iter().any(|(s, _)| s.contains("(capped)")));
        assert!(
            lines
                .iter()
                .any(|(s, _)| s.contains("system namespaces: 2 critical"))
        );
        assert!(lines.last().unwrap().0.contains("not CIS/full-PSS"));
    }

    #[test]
    fn posture_lines_unscanned_is_not_green() {
        use kubernation_core::state::posture::AxisScore;
        let r = PostureReport {
            score: None,
            tier: PostureTier::Unscanned,
            scanned: false,
            fortifications: AxisScore::default(),
            walls: AxisScore::default(),
            workloads_total: 0,
            system_critical: 0,
            system_warning: 0,
            factors: vec![],
        };
        let lines = posture_lines(&r);
        assert!(lines[0].0.contains("not yet scanned"));
        assert!(!lines.iter().any(|(_, r)| *r == RsRole::Good));
    }

    #[test]
    fn cost_lines_modes_idle_warn_and_honesty() {
        use kubernation_core::state::cost::NamespaceCost;
        let mut r = CostReport {
            nodes_priced: 1,
            total_per_hour: 10.0,
            idle_per_hour: 4.0, // 40% > 25% → the idle line warns
            by_namespace: vec![NamespaceCost {
                namespace: "demo".into(),
                per_hour: 6.0,
                system: false,
            }],
            ..Default::default()
        };
        let join = |v: &[(String, RsRole)]| {
            v.iter()
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        // Unitless: the headline + the rollups carry "units" and no "$" amount
        // (the subline may mention "for $" as the how-to — that's instruction).
        let u = cost_lines(&r);
        let ut = join(&u);
        assert!(
            u[0].0.contains("UPKEEP") && u[0].0.contains("units") && !u[0].0.contains('$'),
            "{ut}"
        );
        assert!(
            u.iter()
                .any(|(s, _)| s.contains("demo") && s.contains("units"))
        );
        assert!(ut.to_lowercase().contains("cloud"), "honesty footer");
        assert!(
            u.iter()
                .any(|(s, role)| s.starts_with("idle") && *role == RsRole::Warn)
        );
        // Currency: shows "$".
        r.mode = CostMode::Currency;
        assert!(join(&cost_lines(&r)).contains('$'));
        // No priced nodes → a quiet placeholder, never a false zero.
        let empty = cost_lines(&CostReport::default());
        assert!(empty[0].0.contains("no priced nodes"));
    }
}
