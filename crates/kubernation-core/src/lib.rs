//! KuberNation core: the observed-world data layer (kube reflectors, custom
//! projections) and the pure derivation models (map, workloads, attention,
//! the explorable world geometry, pair sync). No UI dependencies — the
//! windowed `kubernation` client (and the headless `smoke` example) render
//! these models; any future frontend would too.

pub mod events;
pub mod k8s;
pub mod state;
pub mod util;

/// Re-exported so UI crates (which don't depend on `k8s-openapi` directly) can
/// name the time types the pure models hand back — e.g. the timeline's `now`
/// parameter and `TimelineEntry::when`.
pub use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
pub use k8s_openapi::jiff;
