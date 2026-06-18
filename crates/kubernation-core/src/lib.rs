//! Kubernation core: the observed-world data layer (kube reflectors, custom
//! projections) and the pure derivation models (map, workloads, attention,
//! the explorable world geometry, pair sync). No UI dependencies — the
//! windowed `kubernation` client (and the headless `smoke` example) render
//! these models; any future frontend would too.

pub mod events;
pub mod k8s;
pub mod state;
pub mod util;
