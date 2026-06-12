//! K8sCiv core: the observed-world data layer (kube reflectors, custom
//! projections) and the pure derivation models (map, workloads, attention,
//! the explorable world geometry, pair sync). No UI dependencies — the TUI
//! and any other frontend (GUI spike, future web view) render these models.

pub mod events;
pub mod k8s;
pub mod state;
pub mod util;
