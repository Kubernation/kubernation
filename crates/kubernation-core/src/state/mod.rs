pub mod advisor;
pub mod attention;
pub mod blast;
pub mod chaos;
pub mod charter;
pub mod diagnose;
pub mod filter;
pub mod harden;
pub mod inspect;
pub mod logline;
pub mod model;
pub mod netpol;
pub mod observed;
pub mod oracle;
// Endpoint profiles + the precedence resolver reference the feature-gated
// `oracle_client` types (LlmConfig/Endpoint), so this module rides the feature.
#[cfg(feature = "oracle")]
pub mod oracle_config;
pub mod oracle_suggest;
pub mod pair;
pub mod planned;
pub mod postmortem;
pub mod posture;
pub mod rollout;
pub mod saturation;
pub mod slo;
pub mod timeline;
pub mod world;

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
