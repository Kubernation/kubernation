//! KuberNation core: the observed-world data layer (kube reflectors, custom
//! projections) and the pure derivation models (map, workloads, attention,
//! the explorable world geometry, pair sync). No UI dependencies — the
//! windowed `kubernation` client (and the headless `smoke` example) render
//! these models; any future frontend would too.

// Prose has no compiler — except here. A doc link naming an item that was
// renamed, made private, or never existed is the one class of stale claim a
// machine can catch, and the prose audit found eight of them by hand (two
// pointing at a function D1 renamed, in the comment explaining that placement
// has ONE home). Denied rather than warned so it fails `cargo doc` in CI.
//
// Escape notational brackets — `\[0,1\]`, `\[default\]` — which are values,
// not links.
#![deny(rustdoc::broken_intra_doc_links)]

pub mod events;
pub mod k8s;
pub mod state;
pub mod util;

/// Re-exported so UI crates (which don't depend on `k8s-openapi` directly) can
/// name the time types the pure models hand back — e.g. the timeline's `now`
/// parameter and `TimelineEntry::when`.
pub use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
pub use k8s_openapi::jiff;
/// Re-exported so frontends (which don't depend on `kube` directly) can name the
/// client type — e.g. the GUI's connection liveness probe.
pub use kube::Client;
