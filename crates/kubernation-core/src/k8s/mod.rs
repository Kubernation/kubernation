pub mod actions;
pub mod browse;
pub mod client;
pub mod logs;
pub mod metrics;
/// The Oracle (BYO-LLM) HTTP client — non-mutating, beside the write file but
/// not in it; gated behind the `oracle` feature so the headless core smoke
/// example never links an HTTP egress.
#[cfg(feature = "oracle")]
pub mod oracle_client;
pub mod portforward;
pub mod quantity;
pub mod rbac;
pub mod watch;
