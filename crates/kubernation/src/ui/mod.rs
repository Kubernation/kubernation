pub mod attention_panel;
pub mod city;
pub mod context_picker;
pub mod help;
pub mod logs;
pub mod map;
pub mod namespace_picker;
pub mod node_detail;
pub mod plan;
pub mod sidebar;
pub mod status_bar;
pub mod symbols;
pub mod theme;
pub mod workloads;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_crossterm::crossterm::event::KeyEvent;

use crate::events::ClusterId;
use kubernation_core::state::attention::Concern;
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::{Models, WorkloadRef};
use kubernation_core::state::observed::ObservedWorld;
use kubernation_core::state::pair::PairSync;
use kubernation_core::state::planned::{Intervention, PlannedWorld};
use theme::Theme;

/// What a component asks the app to do in response to input. (Quit, back,
/// and view switching are global keys handled by the app itself.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenWorkloadList,
    OpenNode(String),
    OpenWorkload(WorkloadRef),
    /// Tail a pod's logs (namespace, pod); cluster is the active view's.
    OpenLogs {
        namespace: String,
        pod: String,
    },
    /// Evict (delete) a pod (namespace, pod); the app confirms + checks RBAC
    /// before writing. Cluster is the active view's.
    EvictPod {
        namespace: String,
        pod: String,
    },
    /// Re-fetch the open log tail now (e.g. the `--previous` toggle flipped).
    RefetchLogs,
    SwitchContext(String),
    /// Scope the whole world to these namespaces (the namespace-filter picker).
    SetNamespaceFilter(NamespaceFilter),
    /// Stage a planning-turn intervention (scale / cordon). Preview-only until
    /// committed from the End-of-Turn review.
    Stage(Intervention),
    /// Toggle a staged rolling-restart for a workload (stage if absent,
    /// unstage if already staged — the app holds the planned world to decide).
    ToggleRestart(WorkloadRef),
    /// Cursor pushed against a map edge — in pair mode the app crosses to
    /// the other continent.
    EdgeReached(Edge),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

/// Map color overlays — one primary signal at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Pressure,
    ReplicaHealth,
    Namespace,
}

impl OverlayMode {
    pub fn label(self) -> &'static str {
        match self {
            OverlayMode::Pressure => "PRESSURE",
            OverlayMode::ReplicaHealth => "REPLICAS",
            OverlayMode::Namespace => "NAMESPACE",
        }
    }
}

/// Read-only context handed to components for update/render. One of these
/// exists per world; the merged attention queue and the pair comparison are
/// shared across both.
pub struct RenderCtx<'a> {
    pub models: &'a Models,
    pub world: &'a ObservedWorld,
    pub theme: &'a Theme,
    pub overlay: OverlayMode,
    pub ready: bool,
    /// Which world this ctx describes.
    pub cluster: ClusterId,
    /// False for the inactive continent in pair mode (mutes selection).
    pub focused: bool,
    /// Present only in pair mode.
    pub pair: Option<&'a PairSync>,
    /// "HOT"/"WARM" in pair mode, None single-cluster (no clutter).
    pub cluster_label: Option<&'static str>,
    /// Merged severity-ordered concerns across both worlds.
    pub attention: &'a [Concern],
    /// The staged planning turn (hot-cluster only). Views read it to show
    /// staged deltas; staging is gated to the hot world in the app.
    pub planned: &'a PlannedWorld,
}

/// Common shape for the major views (component pattern per the ratatui
/// async template): input → optional Action, update syncs derived state,
/// render is pure drawing.
pub trait Component {
    fn handle_key(&mut self, key: KeyEvent, ctx: &RenderCtx) -> Option<Action>;
    fn update(&mut self, ctx: &RenderCtx);
    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &RenderCtx);
}

/// Centered popup rect clamped to the parent area.
pub fn centered(parent: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(parent.width);
    let h = h.min(parent.height);
    Rect {
        x: parent.x + (parent.width - w) / 2,
        y: parent.y + (parent.height - h) / 2,
        width: w,
        height: h,
    }
}
