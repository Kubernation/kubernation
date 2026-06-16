//! Frontend-agnostic event vocabulary: which cluster changed and which
//! slice of it. Frontends wrap these in their own input-event enums.

/// Which member of the cluster set an event came from. Single-cluster
/// sessions only ever see `Hot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum ClusterId {
    #[default]
    Hot,
    Warm,
}

impl ClusterId {
    pub fn label(self) -> &'static str {
        match self {
            ClusterId::Hot => "HOT",
            ClusterId::Warm => "WARM",
        }
    }
}

/// Coarse "something in this slice of the world changed" notification.
/// Payload-free, so frontends can coalesce them freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldDelta {
    Nodes,
    Pods,
    Workloads,
    Storage,
    Services,
    Events,
    /// Projected custom-resource instances changed.
    Custom,
    /// Live node usage from metrics-server was refreshed.
    Metrics,
    /// Node and pod stores have completed their initial list.
    Ready,
}
