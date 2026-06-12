pub mod attention_panel;
pub mod city;
pub mod context_picker;
pub mod help;
pub mod map;
pub mod node_detail;
pub mod status_bar;
pub mod symbols;
pub mod theme;
pub mod workloads;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_crossterm::crossterm::event::KeyEvent;

use crate::state::model::{Models, WorkloadRef};
use crate::state::observed::ObservedWorld;
use theme::Theme;

/// What a component asks the app to do in response to input. (Quit, back,
/// and view switching are global keys handled by the app itself.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenWorkloadList,
    OpenNode(String),
    OpenWorkload(WorkloadRef),
    SwitchContext(String),
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

/// Read-only context handed to components for update/render.
pub struct RenderCtx<'a> {
    pub models: &'a Models,
    pub world: &'a ObservedWorld,
    pub theme: &'a Theme,
    pub overlay: OverlayMode,
    pub ready: bool,
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
