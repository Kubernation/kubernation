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
/// Recent samples the burn rate looks at.
const BURN_RECENT: usize = 8;
/// Burn rate (× the sustainable spend) above which a budget reads as "burning".
const BURN_HOT: f64 = 1.5;

/// How a workload's error budget reads right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    /// Not enough samples for a verdict.
    Warming,
    /// Budget intact, spending at or below the sustainable rate.
    Healthy,
    /// Budget left, but spending faster than sustainable.
    Burning,
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
    /// Recent spend as a multiple of the sustainable rate (1.0 = on pace to
    /// exactly exhaust the budget over the window; >1 = faster; 0 = recovering).
    pub burn: f64,
    /// How many samples back this reading (more = more trustworthy).
    pub samples: usize,
    pub state: BudgetState,
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

        let recent_n = n.min(BURN_RECENT);
        let recent_down =
            ring.iter().rev().take(recent_n).filter(|&&a| !a).count() as f64 / recent_n as f64;
        let burn = recent_down / budget;

        let state = if n < MIN_SAMPLES {
            BudgetState::Warming
        } else if budget_remaining <= 0.0 {
            BudgetState::Breached
        } else if burn > BURN_HOT {
            BudgetState::Burning
        } else {
            BudgetState::Healthy
        };
        Some(SloStatus {
            sli,
            target,
            budget_remaining,
            burn,
            samples: n,
            state,
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

    /// Every tracked workload's reading.
    pub fn statuses(&self, target: f64) -> Vec<(WorkloadRef, SloStatus)> {
        self.rings
            .iter()
            .filter_map(|(wr, ring)| SloStatus::from_ring(ring, target).map(|s| (wr.clone(), s)))
            .collect()
    }

    /// Drop all history (e.g. on a context switch — a new cluster's budgets).
    pub fn clear(&mut self) {
        self.rings.clear();
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
    let (severity, label) = match st.state {
        BudgetState::Breached => (Severity::Critical, "error budget exhausted"),
        BudgetState::Burning => (Severity::Warning, "error budget burning"),
        _ => return None,
    };
    Some(Concern {
        severity,
        title: format!("{label}: {}/{}", wr.namespace, wr.name),
        detail: format!(
            "availability {:.2}% vs {:.1}% SLO · {} budget left · {:.1}x burn",
            st.sli * 100.0,
            st.target * 100.0,
            budget_pct(st),
            st.burn,
        ),
        target: Target::Workload(wr.clone()),
        probe: None,
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
    fn recent_outages_burn_a_mostly_full_budget() {
        // A long-healthy workload that just went down reads as Burning (budget
        // still positive over the window, but the recent rate is unsustainable).
        let mut t = SloTracker::default();
        // A full window of uptime, then a 2-sample dip — over the whole window
        // that's 2/240 ≈ 0.83% < the 1% budget (still positive), but the recent
        // rate is unsustainable.
        for _ in 0..WINDOW {
            t.record(&[row("flaky", 1, 1)]);
        }
        for _ in 0..2 {
            t.record(&[row("flaky", 1, 0)]); // recent downtime
        }
        let st = t.status(&wr("flaky"), 0.99).unwrap();
        assert!(
            st.budget_remaining > 0.0,
            "long history keeps budget positive"
        );
        assert!(st.burn > BURN_HOT, "recent downtime burns hot: {}", st.burn);
        assert_eq!(st.state, BudgetState::Burning);
    }
}
