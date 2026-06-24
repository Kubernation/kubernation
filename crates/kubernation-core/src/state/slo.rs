//! The treasury — per-workload availability SLOs and the **error budget** you
//! spend down, the central SRE reliability primitive (see the AIM SLO notes:
//! *Implementing Service Level Objectives*, *SLIs and SLOs Demystified*). In
//! the 4X framing the error budget is a treasury: a city that stays up hoards
//! its coins; one that flaps spends them, and an exhausted budget is a city
//! living beyond its means.
//!
//! **Derived, not configured.** The availability SLI is computed from observed
//! pod readiness — no metrics-server, no Prometheus — so it works on any
//! cluster. A workload is **up** at a sample if it has *at least one ready*
//! replica (the textbook "is the service serving" definition — a Ready pod is
//! in the Service's endpoints): this tracks true outages / crash-loops (0
//! ready) and ignores healthy rolling deploys (which keep ≥1 serving). We use
//! `ready`, not `available`, so a workload with a non-zero `minReadySeconds`
//! isn't counted down mid-rollout while pods are serving but not yet "available".
//! Partial capacity loss (3/3 → 1/3) is the attention queue's replica-gap job,
//! not the SLO's. Workloads scaled to 0 have no SLO.
//!
//! **In-session window.** We hold a rolling ring of recent samples (no
//! cross-restart persistence), so this is a *recent-availability* budget over
//! the observed window — a live tracker, not a 30-day compliance number. Honest
//! about what a laptop explorer can know.
//!
//! **Multi-burn-rate alerting.** Over that ring we run the SRE multiwindow burn
//! pattern with three windows: a SHORT window (~48s) gives the recent burn *rate*,
//! a LONG window (~2 min, a strict slice) confirms it's *sustained*, and a small
//! ACTIVE gate (~8s) confirms the incident is *current*. A *fast* burn (severe + still
//! down) **pages** (Critical); a *slow* burn (sustained-but-mild + still down)
//! **tickets** (Warning). The gates are the point — a one-sample blip (long window
//! cold) and a recovered incident (not active) both stay quiet, so the queue doesn't
//! churn on noise. The window sizes + thresholds are tuned to *this* ring/cadence
//! (recent-window rates, not a 30-day-budget burn rate) and are not portable to a
//! Prometheus deployment. The page/ticket split sharpens at looser targets (a bigger
//! budget = more dynamic range); at very tight targets (≳99.5%) the 8-min / 2s ring
//! is too coarse for a sub-breach burn distinction (one 2s down sample already
//! breaches) so it reads page-or-breach there — honest physics, not a missing case.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::events::ClusterId;
use crate::state::attention::{Concern, Severity, Target};
use crate::state::model::{WorkloadRef, WorkloadRow};

/// Default SLO target (availability fraction) when the caller doesn't override.
pub const DEFAULT_TARGET: f64 = 0.99;
/// Samples kept per workload (≈8 min at the frontend's 2s sample cadence) —
/// the rolling "recent availability" window.
pub const WINDOW: usize = 240;
/// Below this many samples there's no verdict yet ("warming up").
pub const MIN_SAMPLES: usize = 8;
// --- multi-burn-rate windows (the SRE multiwindow pattern, scaled to this ring) ---
// Three lookback windows over the availability ring classify a burn: an ACTIVE gate
// ("is the incident current?"), a SHORT window (the recent burn RATE), and a LONG
// window (is it SUSTAINED?). fast (page) + slow (ticket) both require the incident to
// be current, so a recovered/draining incident stays quiet. The window sizes +
// multipliers are tuned to the 240-sample / 2s ring above — RECENT-window rates, NOT
// a 30-day-budget burn rate, and not portable to a Prometheus deployment.
//
// RESOLUTION NOTE (load-bearing): the down-rate over a `w`-sample window quantizes in
// 1/w steps, so over a small window at a tight budget the burn JUMPS past the slow
// band — e.g. over 8 samples at a 1% budget (target 0.99) one down sample is already
// (1/8)/0.01 = 12.5× (past FAST), leaving the [HOT, FAST) ticket band empty. SHORT is
// therefore 24 samples (~48s) so a single down sample lands at 4.17× (inside the slow
// band) and the ticket tier is reachable at the DEFAULT 0.99 target. The split still
// sharpens at looser targets (a larger budget = more dynamic range); at very tight
// targets (≳99.5%) the 8-min / 2s ring is simply too coarse for a sub-breach burn
// distinction — a single 2s down sample already breaches — so it reads page-or-breach
// there, which is honest physics, not a missing case (see the reachability tests).
/// The ACTIVE gate: a fast/slow alert requires downtime within the last few samples,
/// so a recovered incident (its burst draining out of SHORT) doesn't keep alerting.
/// Small + stable against flapping (~8s), not the most-recent single sample (~2s).
const BURN_ACTIVE: usize = 4;
/// The SHORT window: the recent burn RATE (~48s). Wide enough that one down sample at
/// a 1% budget lands inside the ticket band, not past the page threshold (see above).
const BURN_SHORT: usize = 24;
/// The LONG window: "is the burn sustained?" — a strict SLICE of the ring (~2 min).
/// NOT the full ring: a full-ring burn equals `spent` (= 1−budget_remaining), which
/// would conflate the burn axis with the Breached axis.
const BURN_LONG: usize = 60;
/// The long window must be this populated before a slow-burn verdict, else a single
/// down sample in a thin early-session ring would falsely read as chronic erosion.
const BURN_LONG_MIN: usize = 24;
/// SHORT-window burn (× the sustainable spend) to PAGE (fast burn), with the long
/// window confirming it's no blip.
const BURN_FAST: f64 = 6.0;
/// LONG-window burn for a TICKET (slow burn — sustained moderate erosion) and the
/// "not a one-sample blip" confirmation a fast burn also requires.
const BURN_SLOW: f64 = 2.0;
/// SHORT-window floor a slow burn must clear (some real recent burn, not a single
/// stale sample). `BURN_FAST > BURN_SLOW > BURN_HOT > 1`.
const BURN_HOT: f64 = 1.5;

/// Where a workload's SLO target came from (precedence: manual > annotation >
/// default). Surfaced in the treasury band so an operator knows what they're
/// looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetSource {
    /// An in-session manual override (the city-window stepper).
    Manual,
    /// The workload's `kubernation.io/slo-target` annotation.
    Annotation,
    /// The global default (`--slo-target`, else `DEFAULT_TARGET`).
    #[default]
    Default,
}

/// How a workload's error budget reads right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    /// Not enough samples for a verdict.
    Warming,
    /// Budget intact, spending at or below the sustainable rate.
    Healthy,
    /// Sustained moderate burn (long window) that's still active (short window) —
    /// chronic erosion: a TICKET (Warning).
    SlowBurn,
    /// A severe burn confirmed by BOTH windows — imminent exhaustion: a PAGE
    /// (Critical).
    FastBurn,
    /// Budget exhausted (availability below the SLO over the window).
    Breached,
}

/// One workload's error-budget reading over the observed window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SloStatus {
    /// Observed availability (fraction of samples the workload was up).
    pub sli: f64,
    /// The SLO target this was measured against.
    pub target: f64,
    /// Fraction of the error budget still unspent (1.0 = full, 0.0 = exhausted).
    pub budget_remaining: f64,
    /// SHORT-window spend as a multiple of the sustainable rate (1.0 = on pace to
    /// exactly exhaust the budget over the window; >1 = faster; 0 = recovering) —
    /// "is it burning right now?".
    pub burn: f64,
    /// LONG-window spend multiple — "is the burn sustained?" (the slow-burn signal).
    pub burn_long: f64,
    /// How many samples back this reading (more = more trustworthy).
    pub samples: usize,
    pub state: BudgetState,
    /// Where `target` came from (the convenience builders report `Default`;
    /// `statuses_with` sets the real source).
    pub source: TargetSource,
}

impl SloStatus {
    fn from_ring(ring: &VecDeque<bool>, target: f64) -> Option<SloStatus> {
        let n = ring.len();
        if n == 0 {
            return None;
        }
        let up = ring.iter().filter(|&&a| a).count();
        let sli = up as f64 / n as f64;
        let budget = (1.0 - target).max(1e-9); // allowed unavailability
        let spent = (1.0 - sli) / budget; // fraction of budget consumed (may exceed 1)
        let budget_remaining = (1.0 - spent).clamp(0.0, 1.0);

        // Per-window burn = (down-rate over the last `w` samples) ÷ budget.
        let burn_over = |w: usize| -> f64 {
            let w = n.min(w);
            let down = ring.iter().rev().take(w).filter(|&&a| !a).count() as f64 / w as f64;
            down / budget
        };
        let burn = burn_over(BURN_SHORT); // short window — the recent burn RATE
        let burn_long = burn_over(BURN_LONG); // long window — "sustained?"
        // Is the incident CURRENT? A burst draining out of the (wide) SHORT window
        // would otherwise keep the short-window rate hot for ~48s after recovery and
        // false-alert — so both fast and slow require a recent down sample.
        let active = ring.iter().rev().take(BURN_ACTIVE).any(|&a| !a);

        // The multi-window gate. The long window is geometrically bounded — while the
        // budget is still positive (Breached takes precedence) a sustained burst over
        // the LONG window can't much exceed WINDOW/BURN_LONG (≈4×) before it breaches —
        // so the long window is the "this isn't a one-sample blip" confirmation at the
        // SLOW threshold, NOT a second FAST threshold.
        //   FAST (page): currently down AND the SHORT-window rate is severe AND the long
        //   window confirms it's no single-sample spike. A blip (long < SLOW) fails the
        //   long half. No long-min gate: a real outage early in a session should page.
        //   SLOW (ticket): currently down AND the long window shows sustained moderate
        //   burn AND the short-window rate is non-trivial but not severe AND the long
        //   window is genuinely populated. A recovered incident (not active) and a thin
        //   ring (n < BURN_LONG_MIN) both fail their gate.
        let fast = active && burn >= BURN_FAST && burn_long >= BURN_SLOW;
        let slow =
            active && n >= BURN_LONG_MIN && !fast && burn_long >= BURN_SLOW && burn >= BURN_HOT;
        let state = if n < MIN_SAMPLES {
            BudgetState::Warming
        } else if budget_remaining <= 0.0 {
            BudgetState::Breached
        } else if fast {
            BudgetState::FastBurn
        } else if slow {
            BudgetState::SlowBurn
        } else {
            BudgetState::Healthy
        };
        Some(SloStatus {
            sli,
            target,
            budget_remaining,
            burn,
            burn_long,
            samples: n,
            state,
            source: TargetSource::Default,
        })
    }
}

/// Accumulates per-workload availability samples into rolling rings. Driven by
/// the frontend (one `record` per sample cadence); the math (`status`) is pure.
#[derive(Default)]
pub struct SloTracker {
    rings: HashMap<WorkloadRef, VecDeque<bool>>,
}

impl SloTracker {
    /// Record one availability sample per workload from the *unfiltered* rows
    /// (SLOs track every workload regardless of the namespace view, like the
    /// reflectors watch all namespaces). Workloads scaled to 0 have no SLO;
    /// workloads that have disappeared are pruned.
    pub fn record(&mut self, rows: &[WorkloadRow]) {
        let mut seen: HashSet<&WorkloadRef> = HashSet::new();
        for row in rows {
            if row.desired <= 0 {
                continue;
            }
            seen.insert(&row.r);
            let up = row.ready >= 1;
            let ring = self.rings.entry(row.r.clone()).or_default();
            ring.push_back(up);
            while ring.len() > WINDOW {
                ring.pop_front();
            }
        }
        self.rings.retain(|k, _| seen.contains(k));
    }

    /// This workload's budget reading, if it's being tracked.
    pub fn status(&self, wr: &WorkloadRef, target: f64) -> Option<SloStatus> {
        SloStatus::from_ring(self.rings.get(wr)?, target)
    }

    /// Every tracked workload's reading at a single shared target.
    pub fn statuses(&self, target: f64) -> Vec<(WorkloadRef, SloStatus)> {
        self.statuses_with(|_| (target, TargetSource::Default))
    }

    /// Every tracked workload's reading at a *per-workload* target + source
    /// (the caller resolves config precedence: override > annotation > default).
    pub fn statuses_with(
        &self,
        target_of: impl Fn(&WorkloadRef) -> (f64, TargetSource),
    ) -> Vec<(WorkloadRef, SloStatus)> {
        self.rings
            .iter()
            .filter_map(|(wr, ring)| {
                let (target, source) = target_of(wr);
                SloStatus::from_ring(ring, target).map(|mut s| {
                    s.source = source;
                    (wr.clone(), s)
                })
            })
            .collect()
    }

    /// Drop all history (e.g. on a context switch — a new cluster's budgets).
    pub fn clear(&mut self) {
        self.rings.clear();
    }
}

/// The workload-annotation key for a per-workload SLO availability target.
pub const ANNOTATION: &str = "kubernation.io/slo-target";

/// Parse an SLO target string into an availability *fraction* in `(0, 1)`.
/// Accepts a percent (`"99"`, `"99.9"` — anything `>= 1`, treated as `n%`) or a
/// fraction (`"0.999"`). Rejects non-numbers, `<= 0`, and `>= 100%` (a 100%
/// target is a zero-budget singularity — any blip breaches). `Err` carries a
/// short reason for a log line.
pub fn parse_target(raw: &str) -> Result<f64, String> {
    let n: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("not a number: {raw:?}"))?;
    if !n.is_finite() {
        return Err(format!("not finite: {raw:?}"));
    }
    // >= 1 is a percent; < 1 is already a fraction.
    let frac = if n >= 1.0 { n / 100.0 } else { n };
    if frac <= 0.0 {
        return Err(format!("must be > 0: {raw:?}"));
    }
    if frac >= 1.0 {
        return Err(format!("must be < 100% (zero budget): {raw:?}"));
    }
    Ok(frac)
}

/// The per-workload SLO target declared on an object's annotations, if any and
/// valid (`build_workloads` calls this once per workload — cheaper than a
/// per-workload store walk). A malformed value resolves to `None` (→ default).
pub fn annotation_target(
    annotations: Option<&std::collections::BTreeMap<String, String>>,
) -> Option<f64> {
    annotations?
        .get(ANNOTATION)
        .and_then(|s| parse_target(s).ok())
}

/// Per-cluster SLO configuration: a global default plus in-session per-workload
/// manual overrides (the city-window stepper). Annotation targets are *not*
/// stored here — they're read from the live object each resolve so they stay
/// declarative; this only holds the ephemeral knobs.
#[derive(Debug, Clone)]
pub struct SloConfig {
    pub default: f64,
    overrides: HashMap<WorkloadRef, f64>,
}

impl Default for SloConfig {
    fn default() -> Self {
        SloConfig {
            default: DEFAULT_TARGET,
            overrides: HashMap::new(),
        }
    }
}

impl SloConfig {
    /// A config with the given global default and no overrides.
    pub fn new(default: f64) -> Self {
        SloConfig {
            default,
            overrides: HashMap::new(),
        }
    }

    /// The effective target + its source for a workload, given its annotation
    /// target (if any). Precedence: manual override > annotation > default.
    pub fn resolve(&self, wr: &WorkloadRef, annotation: Option<f64>) -> (f64, TargetSource) {
        if let Some(&t) = self.overrides.get(wr) {
            (t, TargetSource::Manual)
        } else if let Some(t) = annotation {
            (t, TargetSource::Annotation)
        } else {
            (self.default, TargetSource::Default)
        }
    }

    /// Set (or, with `None`, clear) a workload's manual override.
    pub fn set_override(&mut self, wr: WorkloadRef, target: Option<f64>) {
        match target {
            Some(t) => {
                self.overrides.insert(wr, t);
            }
            None => {
                self.overrides.remove(&wr);
            }
        }
    }

    /// Drop all manual overrides (e.g. on a context switch).
    pub fn clear_overrides(&mut self) {
        self.overrides.clear();
    }
}

/// Format the remaining budget as a short percent, for labels.
pub fn budget_pct(st: &SloStatus) -> String {
    format!("{:.0}%", st.budget_remaining * 100.0)
}

/// A queue concern for a workload whose budget is burning or exhausted, or
/// `None` when it's healthy/warming. `cluster` defaults to Hot; the frontend
/// re-tags for the warm world (like the rest of attention).
pub fn budget_concern(wr: &WorkloadRef, st: &SloStatus) -> Option<Concern> {
    // FastBurn pages (imminent exhaustion); SlowBurn tickets (chronic erosion).
    let (severity, label) = match st.state {
        BudgetState::Breached => (Severity::Critical, "error budget exhausted"),
        BudgetState::FastBurn => (Severity::Critical, "error budget burning fast"),
        BudgetState::SlowBurn => (Severity::Warning, "error budget eroding"),
        _ => return None,
    };
    Some(Concern {
        severity,
        title: format!("{label}: {}/{}", wr.namespace, wr.name),
        detail: format!(
            "availability {:.2}% vs {:.1}% SLO · {} budget left · {:.1}x/{:.1}x burn (short/long, recent window)",
            st.sli * 100.0,
            st.target * 100.0,
            budget_pct(st),
            st.burn,
            st.burn_long,
        ),
        target: Target::Workload(wr.clone()),
        probe: None,
        // Stable key: a workload escalating SlowBurn → FastBurn updates IN PLACE.
        key: format!("slo:{}/{}", wr.namespace, wr.name),
        cluster: ClusterId::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::WorkloadKind;

    fn row(name: &str, desired: i32, available: i32) -> WorkloadRow {
        WorkloadRow {
            r: WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: "demo".into(),
                name: name.into(),
            },
            desired,
            ready: available,
            available,
            updated: desired,
            status: crate::state::model::RolloutStatus::Complete,
            note: String::new(),
            age: None,
            slo_target: None,
        }
    }

    fn wr(name: &str) -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: name.into(),
        }
    }

    #[test]
    fn parse_target_accepts_percent_and_fraction_rejects_extremes() {
        let approx = |r: Result<f64, String>, want: f64| (r.unwrap() - want).abs() < 1e-9;
        assert!(approx(parse_target("99"), 0.99));
        assert!(approx(parse_target("99.9"), 0.999));
        assert!(approx(parse_target(" 0.995 "), 0.995));
        assert!(approx(parse_target("50"), 0.5)); // operator's call, accepted
        assert!(approx(parse_target("1.0"), 0.01)); // ">=1" is percent, so 1.0 = 1%
        assert!(parse_target("abc").is_err());
        assert!(parse_target("0").is_err()); // <= 0
        assert!(parse_target("100").is_err()); // 100% = zero budget
        assert!(parse_target("150").is_err()); // > 100%
    }

    #[test]
    fn config_resolve_precedence_override_annotation_default() {
        let mut cfg = SloConfig {
            default: 0.99,
            ..Default::default()
        };
        let w = wr("web");
        // default when nothing set
        assert_eq!(cfg.resolve(&w, None), (0.99, TargetSource::Default));
        // annotation beats default
        assert_eq!(
            cfg.resolve(&w, Some(0.999)),
            (0.999, TargetSource::Annotation)
        );
        // manual override beats both
        cfg.set_override(w.clone(), Some(0.95));
        assert_eq!(cfg.resolve(&w, Some(0.999)), (0.95, TargetSource::Manual));
        // clearing the override falls back to annotation
        cfg.set_override(w.clone(), None);
        assert_eq!(
            cfg.resolve(&w, Some(0.999)),
            (0.999, TargetSource::Annotation)
        );
    }

    #[test]
    fn statuses_with_applies_per_workload_target_and_source() {
        let mut t = SloTracker::default();
        for _ in 0..MIN_SAMPLES {
            t.record(&[row("web", 1, 1)]);
        }
        let got = t.statuses_with(|_| (0.999, TargetSource::Annotation));
        let (_, st) = got.first().unwrap();
        assert_eq!(st.target, 0.999);
        assert_eq!(st.source, TargetSource::Annotation);
    }

    #[test]
    fn always_up_keeps_a_full_budget() {
        let mut t = SloTracker::default();
        for _ in 0..MIN_SAMPLES {
            t.record(&[row("web", 3, 3)]);
        }
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert_eq!(st.sli, 1.0);
        assert_eq!(st.budget_remaining, 1.0);
        assert_eq!(st.state, BudgetState::Healthy);
        assert!(budget_concern(&wr("web"), &st).is_none());
    }

    #[test]
    fn always_down_exhausts_the_budget() {
        let mut t = SloTracker::default();
        for _ in 0..MIN_SAMPLES {
            t.record(&[row("crashy", 1, 0)]); // 0 available = down
        }
        let st = t.status(&wr("crashy"), 0.99).unwrap();
        assert_eq!(st.sli, 0.0);
        assert_eq!(st.budget_remaining, 0.0);
        assert_eq!(st.state, BudgetState::Breached);
        let c = budget_concern(&wr("crashy"), &st).unwrap();
        assert_eq!(c.severity, Severity::Critical);
    }

    #[test]
    fn one_ready_replica_counts_as_up() {
        // Partial capacity (1 of 3) is still "up" for the uptime SLI — the
        // replica-gap concern covers degradation; the SLO tracks outages.
        let mut t = SloTracker::default();
        for _ in 0..MIN_SAMPLES {
            t.record(&[row("web", 3, 1)]);
        }
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert_eq!(st.sli, 1.0);
        assert_eq!(st.state, BudgetState::Healthy);
    }

    #[test]
    fn ready_not_available_drives_the_sli() {
        // A pod serving (ready) but not yet "available" (minReadySeconds) counts
        // as up — so a fast rollout doesn't spend budget.
        let mut t = SloTracker::default();
        let mut r = row("web", 1, 1);
        r.ready = 1;
        r.available = 0; // serving, but inside minReadySeconds
        for _ in 0..MIN_SAMPLES {
            t.record(std::slice::from_ref(&r));
        }
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert_eq!(st.sli, 1.0, "ready≥1 is up even when available==0");
    }

    #[test]
    fn warming_until_min_samples() {
        let mut t = SloTracker::default();
        t.record(&[row("web", 1, 1)]);
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert_eq!(st.state, BudgetState::Warming);
        assert!(budget_concern(&wr("web"), &st).is_none());
    }

    #[test]
    fn scaled_to_zero_has_no_slo() {
        let mut t = SloTracker::default();
        t.record(&[row("idle", 0, 0)]);
        assert!(t.status(&wr("idle"), 0.99).is_none());
        assert!(t.statuses(0.99).is_empty());
    }

    #[test]
    fn ring_caps_and_prunes_vanished_workloads() {
        let mut t = SloTracker::default();
        for _ in 0..(WINDOW + 20) {
            t.record(&[row("web", 1, 1)]);
        }
        assert_eq!(t.status(&wr("web"), 0.99).unwrap().samples, WINDOW);
        // web drops out of the rows → its ring is pruned.
        t.record(&[row("other", 1, 1)]);
        assert!(t.status(&wr("web"), 0.99).is_none());
    }

    #[test]
    fn fast_burn_pages_on_a_recent_hot_burst() {
        // A long-healthy workload that's down right now with a severe recent rate
        // (budget still positive) → FastBurn (page / Critical). The short window runs
        // hot; the long window confirms it's no single-sample blip.
        let mut t = SloTracker::default();
        for _ in 0..WINDOW {
            t.record(&[row("api", 1, 1)]);
        }
        for _ in 0..2 {
            t.record(&[row("api", 1, 0)]); // currently down, 2-sample burst
        }
        let st = t.status(&wr("api"), 0.99).unwrap();
        assert!(
            st.budget_remaining > 0.0,
            "long history keeps budget positive"
        );
        assert!(st.burn >= BURN_FAST, "short window severe: {}", st.burn);
        assert!(
            st.burn_long >= BURN_SLOW,
            "long window confirms (not a blip)"
        );
        assert_eq!(st.state, BudgetState::FastBurn);
        assert_eq!(
            budget_concern(&wr("api"), &st).unwrap().severity,
            Severity::Critical
        );
    }

    #[test]
    fn slow_burn_tickets_at_the_default_target() {
        // The ticket tier must be REACHABLE at the DEFAULT 0.99 target (the review's
        // key finding: at a tight budget an 8-sample short window quantized the burn
        // past the slow band, leaving it dead). With the 24-sample short window, a
        // sustained-but-mild burn — one down in the short window plus an earlier one in
        // the long window, currently down — lands inside [HOT, FAST) → SlowBurn
        // (ticket / Warning), not a page.
        let mut t = SloTracker::default();
        for _ in 0..WINDOW {
            t.record(&[row("svc", 1, 1)]);
        }
        t.record(&[row("svc", 1, 0)]); // an earlier dip (in the long window only)
        for _ in 0..35 {
            t.record(&[row("svc", 1, 1)]);
        }
        t.record(&[row("svc", 1, 0)]); // currently down (in the short window)
        let st = t.status(&wr("svc"), 0.99).unwrap();
        assert!(
            st.budget_remaining > 0.0,
            "eroded, not exhausted: {}",
            st.budget_remaining
        );
        assert!(
            st.burn_long >= BURN_SLOW,
            "long window sustained: {}",
            st.burn_long
        );
        assert!(
            st.burn >= BURN_HOT && st.burn < BURN_FAST,
            "short rate mild, not severe: {}",
            st.burn
        );
        assert_eq!(st.state, BudgetState::SlowBurn);
        assert_eq!(
            budget_concern(&wr("svc"), &st).unwrap().severity,
            Severity::Warning
        );
    }

    #[test]
    fn a_one_sample_blip_does_not_alert() {
        // A single down sample: the long window stays cold (< SLOW) → no alert, even
        // though the workload is "down" at this instant. The long-window confirmation
        // is what rejects the blip.
        let mut t = SloTracker::default();
        for _ in 0..WINDOW {
            t.record(&[row("web", 1, 1)]);
        }
        t.record(&[row("web", 1, 0)]); // exactly one down sample
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert!(st.burn > BURN_HOT, "short window registers the dip");
        assert!(st.burn_long < BURN_SLOW, "long window stays cold");
        assert_eq!(st.state, BudgetState::Healthy);
        assert!(budget_concern(&wr("web"), &st).is_none());
    }

    #[test]
    fn a_recovered_incident_does_not_keep_alerting() {
        // A past burst still inside the long window, but fully recovered (the short
        // window cleared): long elevated, not active → no alert.
        let mut t = SloTracker::default();
        for _ in 0..WINDOW {
            t.record(&[row("web", 1, 1)]);
        }
        for _ in 0..2 {
            t.record(&[row("web", 1, 0)]); // the past incident (stays within budget)
        }
        for _ in 0..BURN_SHORT {
            t.record(&[row("web", 1, 1)]); // recovered: clears the short + active windows
        }
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert!(st.budget_remaining > 0.0);
        assert!(st.burn_long > BURN_SLOW, "long window still elevated");
        assert!(st.burn < BURN_HOT, "short window recovered");
        assert_ne!(st.state, BudgetState::SlowBurn);
        assert_ne!(st.state, BudgetState::FastBurn);
        assert!(budget_concern(&wr("web"), &st).is_none());
    }

    #[test]
    fn a_partial_recovery_does_not_page() {
        // A burst whose tail is still inside the (wide) short window but has stopped:
        // the short-window rate is still hot, yet no down sample in the active window.
        // Without the active gate this would FALSE-PAGE during the ~48s drain (the
        // review's third finding); the active gate keeps it quiet.
        let mut t = SloTracker::default();
        for _ in 0..WINDOW {
            t.record(&[row("web", 1, 1)]);
        }
        for _ in 0..2 {
            t.record(&[row("web", 1, 0)]); // the burst
        }
        for _ in 0..7 {
            t.record(&[row("web", 1, 1)]); // recovered 7 samples ago (still in short window)
        }
        let st = t.status(&wr("web"), 0.99).unwrap();
        assert!(
            st.burn >= BURN_FAST,
            "short window is still hot from the drain"
        );
        assert_ne!(st.state, BudgetState::FastBurn, "but not active → no page");
        assert_ne!(st.state, BudgetState::SlowBurn);
        assert!(budget_concern(&wr("web"), &st).is_none());
    }

    #[test]
    fn a_thin_ring_does_not_falsely_ticket() {
        // Between MIN_SAMPLES and BURN_LONG_MIN, a down sample must NOT read as chronic
        // erosion (the BURN_LONG_MIN populated-enough gate). At 0.90 the budget stays
        // positive, so the gate — not a breach — is what keeps it out of SlowBurn.
        let mut t = SloTracker::default();
        for _ in 0..(MIN_SAMPLES + 4) {
            t.record(&[row("web", 1, 1)]);
        }
        t.record(&[row("web", 1, 0)]);
        let st = t.status(&wr("web"), 0.90).unwrap();
        assert!(st.samples < BURN_LONG_MIN);
        assert!(st.budget_remaining > 0.0);
        assert_ne!(st.state, BudgetState::SlowBurn);
    }
}
