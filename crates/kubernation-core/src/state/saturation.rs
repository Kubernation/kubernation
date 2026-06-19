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
    pub worst: SatLevel,
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
            .filter(|d| d.level == self.worst)
            .max_by(|a, b| {
                a.effective()
                    .partial_cmp(&b.effective())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|d| (d.kind, d.effective()))
    }

    /// The worst level — what the overlay tints on.
    pub fn worst_level(&self) -> SatLevel {
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
/// (live-usage or requests — the caller knows which); `nonterminal_pods` is the
/// count of scheduled non-terminal pods on the node; `alloc_pods` is
/// `allocatable["pods"]` (None ⇒ the pod-count dimension is OMITTED, never
/// assumed); `abnormal` is the node's pressure-condition short names ("Disk",
/// "Mem", "PID"; "Net" is ignored — not a saturation signal).
pub fn saturate_node(
    cpu_ratio: f64,
    mem_ratio: f64,
    nonterminal_pods: u32,
    alloc_pods: Option<f64>,
    abnormal: &[&str],
) -> NodeSaturation {
    let mut dims = Vec::new();

    dims.push(SatDim {
        kind: SatDimKind::Cpu,
        ratio: Some(cpu_ratio),
        level: SatLevel::from_ratio(cpu_ratio),
        label: format!("cpu {}%", pct(cpu_ratio)),
    });
    dims.push(SatDim {
        kind: SatDimKind::Mem,
        ratio: Some(mem_ratio),
        level: SatLevel::from_ratio(mem_ratio),
        label: format!("mem {}%", pct(mem_ratio)),
    });

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

    let worst = dims.iter().map(|d| d.level).max().unwrap_or(SatLevel::Calm);
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
        let s = saturate_node(0.95, 0.30, 10, Some(110.0), &[]);
        assert_eq!(s.worst, SatLevel::High);
        assert_eq!(s.worst_dim().unwrap().0, SatDimKind::Cpu);
        // pod-count present but calm.
        assert_eq!(s.pod_ratio(), Some(10.0 / 110.0));
    }

    #[test]
    fn pod_bound_node_surfaces_pods_with_calm_cpu_mem() {
        let s = saturate_node(0.20, 0.30, 108, Some(110.0), &[]);
        assert_eq!(s.worst, SatLevel::High, "108/110 is past SAT_PODS_HIGH");
        assert_eq!(s.worst_dim().unwrap().0, SatDimKind::Pods);
        let pods = s.dims.iter().find(|d| d.kind == SatDimKind::Pods).unwrap();
        assert_eq!(pods.label, "pods 108/110");
    }

    #[test]
    fn pod_dim_elevated_band() {
        // 95/110 = 0.863 → Elevated (>=0.85, <0.95).
        let s = saturate_node(0.10, 0.10, 95, Some(110.0), &[]);
        let pods = s.dims.iter().find(|d| d.kind == SatDimKind::Pods).unwrap();
        assert_eq!(pods.level, SatLevel::Elevated);
        assert_eq!(s.worst, SatLevel::Elevated);
    }

    #[test]
    fn disk_pressure_forces_high_with_no_ratio() {
        let s = saturate_node(0.10, 0.10, 5, Some(110.0), &["Disk"]);
        assert_eq!(s.worst, SatLevel::High, "the kubelet's own verdict pegs it");
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
        let s = saturate_node(0.50, 0.50, 200, None, &[]);
        assert!(s.dims.iter().all(|d| d.kind != SatDimKind::Pods));
        assert_eq!(s.pod_ratio(), None);
        // cpu/mem still tint.
        assert_eq!(s.dims.len(), 2);
        assert_eq!(s.worst, SatLevel::Calm);
    }

    #[test]
    fn alloc_pods_zero_is_treated_as_absent() {
        let s = saturate_node(0.1, 0.1, 3, Some(0.0), &[]);
        assert!(s.dims.iter().all(|d| d.kind != SatDimKind::Pods));
    }

    #[test]
    fn net_condition_is_not_a_saturation_signal() {
        let s = saturate_node(0.1, 0.1, 3, Some(110.0), &["Net"]);
        assert!(s.dims.iter().all(|d| !d.kind.is_condition()));
        assert_eq!(s.worst, SatLevel::Calm);
    }

    #[test]
    fn all_calm_node_reads_calm() {
        let s = saturate_node(0.2, 0.3, 12, Some(110.0), &[]);
        assert_eq!(s.worst, SatLevel::Calm);
        assert_eq!(s.worst_level(), SatLevel::Calm);
    }

    #[test]
    fn worst_dim_agrees_with_worst_level() {
        // cpu 0.92 → High; pods 102/110 = 0.927 → Elevated (tighter buckets) yet
        // a HIGHER raw ratio. worst_dim must name the High dim (cpu), not pods,
        // so it can never disagree with the overlay tint / worst_level.
        let s = saturate_node(0.92, 0.30, 102, Some(110.0), &[]);
        assert_eq!(s.worst, SatLevel::High);
        let (kind, _) = s.worst_dim().unwrap();
        assert_eq!(kind, SatDimKind::Cpu);
        // Invariant: the named dim's level == worst across any inputs here.
        let named = s.dims.iter().find(|d| d.kind == kind).unwrap();
        assert_eq!(named.level, s.worst);
    }

    #[test]
    fn default_is_calm_empty() {
        let s = NodeSaturation::default();
        assert_eq!(s.worst, SatLevel::Calm);
        assert!(s.dims.is_empty());
        assert_eq!(s.worst_dim(), None);
    }
}
