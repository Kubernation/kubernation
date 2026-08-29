// Each module's documentation lives in the module's own file. A `///` here as
// well would be a SECOND home for the same claim — and rustdoc then resolves the
// file's own `//!` links in THIS module's scope, where the file's items are not
// visible, which is what silently broke seven doc links.
pub mod actions;
pub mod adapter;
pub mod browse;
pub mod client;
pub mod fingerprint;
pub mod logs;
pub mod metrics;
pub mod opencost;
#[cfg(feature = "oracle")]
pub mod oracle_client;
pub mod portforward;
pub mod quantity;
pub mod rbac;
pub mod watch;
