use std::path::PathBuf;

use clap::Parser;

/// K8sCiv — the cluster as a living map. Observe-only MVP.
#[derive(Debug, Parser)]
#[command(name = "k8sciv", version, about)]
pub struct Args {
    /// Path to kubeconfig (defaults to $KUBECONFIG, then ~/.kube/config)
    #[arg(long)]
    pub kubeconfig: Option<PathBuf>,

    /// Kubeconfig context to use (defaults to current-context)
    #[arg(long)]
    pub context: Option<String>,

    /// Warm-standby context: observe a second cluster side-by-side with
    /// sync-state badges (overrides config `warm_context`)
    #[arg(long)]
    pub warm: Option<String>,

    /// Log-file filter, e.g. "info" or "k8sciv=debug" ($RUST_LOG overrides)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Connect, wait for the initial sync, print a one-line world summary,
    /// and exit without starting the TUI. Used by CI and the Makefile.
    #[arg(long)]
    pub smoke: bool,
}
