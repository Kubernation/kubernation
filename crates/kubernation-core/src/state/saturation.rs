//! Saturation — the **4th golden signal** (Latency / Traffic / Errors /
//! Saturation). PURE, UI-dep-free, unit-tested. Saturation = how full a node is
//! *toward a hard limit*, with queueing / eviction implied as it approaches —
//! a strict superset of the cpu/mem utilization the `Pressure` overlay shows.
//!
//! Per node we roll up the worst of several dimensions:
//! - **cpu / mem** — usage÷allocatable (live when metrics-server is up, else
//!   requests÷allocatable). A utilization-as-saturation proxy; cpu is
//!   compressible (CFS throttle), mem is not (the kubelet evicts at the limit).
//! - **pod-count** — non-terminal scheduled pods ÷ `allocatable["pods"]` (the
//!   kubelet max-pods, often 110). ALWAYS computable from the core API — no
//!   metrics-server — and the headline new signal: a node at max-pods silently
//!   refuses scheduling while cpu/mem look calm.
//! - **Disk / Mem / PID pressure conditions** — the kubelet's own authoritative
//!   "saturated NOW, evicting/refusing" booleans. These are the *only honest*
//!   representation of disk and PID exhaustion (metrics-server cannot quantify
//!   them), so they are pegged flags, **never** a fabricated percentage.
//!
//! HONESTY (load-bearing): a dimension with no honest source is OMITTED, never
//! assumed. There is deliberately **no numeric disk / ephemeral-storage or PID
//! dimension** — there is no node-usage source for them today; do not add a
//! fabricated ratio. `SatDim.ratio` is `Option<f64>` and stays `None` for the
//! boolean conditions (and is shaped for a future kubelet Summary-API graft).

use crate::state::model::{PRESSURE_ELEVATED, PRESSURE_HIGH};

/// Pod-count near-miss thresholds — tighter than cpu/mem's 0.7/0.9 because the
/// limit is a hard integer (105/110 is already a near-miss).
pub const SAT_PODS_ELEVATED: f64 = 0.85;
pub const SAT_PODS_HIGH: f64 = 0.95;

/// How close a single dimension is to its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SatLevel {
    #[default]
    Calm,
    Elevated,
    High,
}

impl SatLevel {
    /// Bucket a ratio against the documented cpu/mem pressure thresholds.
    fn from_ratio(ratio: f64) -> SatLevel {
        if ratio >= PRESSURE_HIGH {
            SatLevel::High
        } else if ratio >= PRESSURE_ELEVATED {
            SatLevel::Elevated
        } else {
            SatLevel::Calm
        }
    }

    /// Bucket a pod-count ratio against the tighter pod-slot thresholds.
    fn from_pod_ratio(ratio: f64) -> SatLevel {
        if ratio >= SAT_PODS_HIGH {
            SatLevel::High
        } else if ratio >= SAT_PODS_ELEVATED {
            SatLevel::Elevated
        } else {
            SatLevel::Calm
        }
    }
}

/// The saturation dimensions of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatDimKind {
    Cpu,
    Mem,
    Pods,
    DiskPressure,
    MemPressure,
    PidPressure,
}

impl SatDimKind {
    /// True for the boolean kubelet-condition dimensions (no ratio).
    pub fn is_condition(self) -> bool {
        matches!(
            self,
            SatDimKind::DiskPressure | SatDimKind::MemPressure | SatDimKind::PidPressure
        )
    }
}

/// One saturation dimension of a node: its kind, the ratio (None for boolean
/// conditions), the bucketed level, and a kubectl-greppable display label.
#[derive(Debug, Clone, PartialEq)]
pub struct SatDim {
    pub kind: SatDimKind,
    /// Utilization 0.0..=~1.0; `None` for the boolean conditions.
    pub ratio: Option<f64>,
    pub level: SatLevel,
    /// Display label, e.g. `cpu 93%`, `pods 105/110`, `DiskPressure (pegged)`.
    pub label: String,
}

impl SatDim {
    /// The effective ratio used for the worst-dimension comparison — a present
    /// condition counts as 1.0 (at the limit), since the kubelet says so.
    fn effective(&self) -> f64 {
        self.ratio.unwrap_or(1.0)
    }
}

/// A node's saturation: every present dimension + the worst level across them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeSaturation {
    pub dims: Vec<SatDim>,
    /// `None` when `dims` is empty — see [`NodeSaturation::worst_level`].
    pub worst: Option<SatLevel>,
}

impl NodeSaturation {
    /// The dimension that drove the verdict: among the dims AT the worst level
    /// (so it always agrees with `worst_level` / the overlay tint), the one
    /// closest to its limit. A pegged condition (effective 1.0) wins over a calm
    /// ratio. `None` when there are no dimensions at all (a bare/mid-sync node).
    ///
    /// Restricting to `level == worst` matters because the pod-count buckets
    /// (0.85/0.95) are tighter than cpu/mem's (0.7/0.9): a raw max-by-ratio could
    /// otherwise name an Elevated pod dim on a province the overlay paints High.
    pub fn worst_dim(&self) -> Option<(SatDimKind, f64)> {
        self.dims
            .iter()
            .filter(|d| Some(d.level) == self.worst)
            .max_by(|a, b| {
                a.effective()
                    .partial_cmp(&b.effective())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|d| (d.kind, d.effective()))
    }

    /// The worst level — what the overlay tints on. **`None` when there are no
    /// dimensions at all**, i.e. the node reports no allocatable and no pressure
    /// condition, so its strain is not computable.
    ///
    /// Optional rather than defaulting to `Calm`, because `Calm` is a MEASUREMENT
    /// and a node with zero measurements has not earned it. The old
    /// `unwrap_or(SatLevel::Calm)` was the same fabrication this module's inputs
    /// were fixed to refuse — one level up, on the output — and it leaked an
    /// unearned all-clear into every consumer that did not happen to guard on
    /// `dims.is_empty()` itself. The type is the guard now.
    pub fn worst_level(&self) -> Option<SatLevel> {
        self.worst
    }

    /// The pod-count dimension's ratio, if that dimension is present (used by the
    /// pod-slot-exhaustion attention detector).
    pub fn pod_ratio(&self) -> Option<f64> {
        self.dims
            .iter()
            .find(|d| d.kind == SatDimKind::Pods)
            .and_then(|d| d.ratio)
    }

    /// The pod-count dimension's display label (`pods 105/110`), if present.
    pub fn pod_label(&self) -> Option<&str> {
        self.dims
            .iter()
            .find(|d| d.kind == SatDimKind::Pods)
            .map(|d| d.label.as_str())
    }
}

/// PURE constructor. `cpu_ratio`/`mem_ratio` are the already-computed node ratios
/// (live-usage or requests — the caller knows which), each `None` when the node
/// reports no allocatable for that resource, in which case that dimension is
/// OMITTED rather than assumed calm; `nonterminal_pods` is the
/// count of scheduled non-terminal pods on the node; `alloc_pods` is
/// `allocatable["pods"]` (None ⇒ the pod-count dimension is OMITTED, never
/// assumed); `abnormal` is the node's pressure-condition short names ("Disk",
/// "Mem", "PID"; "Net" is ignored — not a saturation signal).
pub fn saturate_node(
    cpu_ratio: Option<f64>,
    mem_ratio: Option<f64>,
    nonterminal_pods: u32,
    alloc_pods: Option<f64>,
    abnormal: &[&str],
) -> NodeSaturation {
    let mut dims = Vec::new();

    // cpu/mem — OMITTED when the node reports no allocatable for that resource,
    // exactly as pod-count already is below. A fabricated 0% would make an
    // unmeasurable node read `strain: calm`, which is the one thing this
    // dimension must never say about a node it cannot measure.
    if let Some(r) = cpu_ratio {
        dims.push(SatDim {
            kind: SatDimKind::Cpu,
            ratio: Some(r),
            level: SatLevel::from_ratio(r),
            label: format!("cpu {}%", pct(r)),
        });
    }
    if let Some(r) = mem_ratio {
        dims.push(SatDim {
            kind: SatDimKind::Mem,
            ratio: Some(r),
            level: SatLevel::from_ratio(r),
            label: format!("mem {}%", pct(r)),
        });
    }

    // Pod-count — omitted entirely when we can't honestly compute it.
    if let Some(cap) = alloc_pods.filter(|c| *c > 0.0) {
        let ratio = nonterminal_pods as f64 / cap;
        dims.push(SatDim {
            kind: SatDimKind::Pods,
            ratio: Some(ratio),
            level: SatLevel::from_pod_ratio(ratio),
            label: format!("pods {}/{}", nonterminal_pods, cap.round() as i64),
        });
    }

    // Kubelet pressure conditions — pegged High booleans (never a percentage).
    for short in abnormal {
        let (kind, label) = match *short {
            "Disk" => (SatDimKind::DiskPressure, "DiskPressure (pegged)"),
            "Mem" => (SatDimKind::MemPressure, "MemoryPressure (pegged)"),
            "PID" => (SatDimKind::PidPressure, "PIDPressure (pegged)"),
            _ => continue, // "Net" etc. — not a saturation signal
        };
        dims.push(SatDim {
            kind,
            ratio: None,
            level: SatLevel::High,
            label: label.to_string(),
        });
    }

    // No `unwrap_or(Calm)`: zero dimensions means we measured nothing, and
    // "calm" is a measurement.
    let worst = dims.iter().map(|d| d.level).max();
    NodeSaturation { dims, worst }
}

/// Round a 0..1 ratio to a whole-percent for display.
fn pct(ratio: f64) -> i64 {
    (ratio * 100.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_bound_node_is_high_via_cpu() {
        let s = saturate_node(Some(0.95), Some(0.30), 10, Some(110.0), &[]);
        assert_eq!(s.worst, Some(SatLevel::High));
        assert_eq!(s.worst_dim().unwrap().0, SatDimKind::Cpu);
        // pod-count present but calm.
        assert_eq!(s.pod_ratio(), Some(10.0 / 110.0));
    }

    #[test]
    fn pod_bound_node_surfaces_pods_with_calm_cpu_mem() {
        let s = saturate_node(Some(0.20), Some(0.30), 108, Some(110.0), &[]);
        assert_eq!(
            s.worst,
            Some(SatLevel::High),
            "108/110 is past SAT_PODS_HIGH"
        );
        assert_eq!(s.worst_dim().unwrap().0, SatDimKind::Pods);
        let pods = s.dims.iter().find(|d| d.kind == SatDimKind::Pods).unwrap();
        assert_eq!(pods.label, "pods 108/110");
    }

    #[test]
    fn pod_dim_elevated_band() {
        // 95/110 = 0.863 → Elevated (>=0.85, <0.95).
        let s = saturate_node(Some(0.10), Some(0.10), 95, Some(110.0), &[]);
        let pods = s.dims.iter().find(|d| d.kind == SatDimKind::Pods).unwrap();
        assert_eq!(pods.level, SatLevel::Elevated);
        assert_eq!(s.worst, Some(SatLevel::Elevated));
    }

    #[test]
    fn disk_pressure_forces_high_with_no_ratio() {
        let s = saturate_node(Some(0.10), Some(0.10), 5, Some(110.0), &["Disk"]);
        assert_eq!(
            s.worst,
            Some(SatLevel::High),
            "the kubelet's own verdict pegs it"
        );
        let d = s
            .dims
            .iter()
            .find(|d| d.kind == SatDimKind::DiskPressure)
            .unwrap();
        assert_eq!(
            d.ratio, None,
            "a condition is never a fabricated percentage"
        );
        assert_eq!(d.level, SatLevel::High);
        // worst_dim treats the condition as effective 1.0 → it wins.
        assert_eq!(s.worst_dim().unwrap().0, SatDimKind::DiskPressure);
    }

    #[test]
    fn alloc_pods_absent_omits_the_pod_dimension() {
        let s = saturate_node(Some(0.50), Some(0.50), 200, None, &[]);
        assert!(s.dims.iter().all(|d| d.kind != SatDimKind::Pods));
        assert_eq!(s.pod_ratio(), None);
        // cpu/mem still tint.
        assert_eq!(s.dims.len(), 2);
        assert_eq!(s.worst, Some(SatLevel::Calm));
    }

    #[test]
    fn alloc_pods_zero_is_treated_as_absent() {
        let s = saturate_node(Some(0.1), Some(0.1), 3, Some(0.0), &[]);
        assert!(s.dims.iter().all(|d| d.kind != SatDimKind::Pods));
    }

    #[test]
    fn net_condition_is_not_a_saturation_signal() {
        let s = saturate_node(Some(0.1), Some(0.1), 3, Some(110.0), &["Net"]);
        assert!(s.dims.iter().all(|d| !d.kind.is_condition()));
        assert_eq!(s.worst, Some(SatLevel::Calm));
    }

    #[test]
    fn all_calm_node_reads_calm() {
        let s = saturate_node(Some(0.2), Some(0.3), 12, Some(110.0), &[]);
        assert_eq!(s.worst, Some(SatLevel::Calm));
        assert_eq!(s.worst_level(), Some(SatLevel::Calm));
    }

    #[test]
    fn worst_dim_agrees_with_worst_level() {
        // cpu 0.92 → High; pods 102/110 = 0.927 → Elevated (tighter buckets) yet
        // a HIGHER raw ratio. worst_dim must name the High dim (cpu), not pods,
        // so it can never disagree with the overlay tint / worst_level.
        let s = saturate_node(Some(0.92), Some(0.30), 102, Some(110.0), &[]);
        assert_eq!(s.worst, Some(SatLevel::High));
        let (kind, _) = s.worst_dim().unwrap();
        assert_eq!(kind, SatDimKind::Cpu);
        // Invariant: the named dim's level == worst across any inputs here.
        let named = s.dims.iter().find(|d| d.kind == kind).unwrap();
        assert_eq!(Some(named.level), s.worst);
    }

    #[test]
    fn a_saturation_with_no_dimensions_has_no_verdict() {
        // The default / bare case is NOT calm: calm is a measurement, and there
        // are no measurements here. Pinned because `unwrap_or(SatLevel::Calm)`
        // is exactly the fabrication this type refuses.
        let s = NodeSaturation::default();
        assert!(s.dims.is_empty());
        assert_eq!(s.worst_level(), None, "no dims ⇒ no verdict, not Calm");
        assert_eq!(s.worst_dim(), None);
        assert_eq!(s.pod_ratio(), None);

        // ...and a node reporting nothing at all goes through the same path.
        let bare = saturate_node(None, None, 0, None, &[]);
        assert_eq!(bare.worst_level(), None);
    }
}
