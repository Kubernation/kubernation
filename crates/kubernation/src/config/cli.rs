use std::path::PathBuf;

use clap::Parser;

/// Kubernation — the cluster as a living map. Observe-only MVP.
#[derive(Debug, Parser)]
#[command(
    name = "kubernation",
    version,
    about,
    after_help = "Kubernation is an independent, unaffiliated homage — not associated with, \
endorsed by, or sponsored by Take-Two Interactive Software, Inc., Firaxis Games, or the \
Civilization franchise. Sid Meier's Civilization and Civ are trademarks of Take-Two Interactive."
)]
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

    /// Log-file filter, e.g. "info" or "kubernation=debug" ($RUST_LOG overrides)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Project a CRD's instances onto the world map (repeatable; CRD name
    /// like "gizmos.example.com"; merged with config `projections`)
    #[arg(long = "project", value_name = "CRD")]
    pub project: Vec<String>,

    /// Connect, wait for the initial sync, print a one-line world summary,
    /// and exit without starting the TUI. Used by CI and the Makefile.
    #[arg(long)]
    pub smoke: bool,
}
