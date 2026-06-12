mod app;
mod config;
mod events;
mod k8s;
mod logging;
mod state;
mod ui;
mod util;

use clap::Parser;
use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::app::App;
use crate::config::Config;
use crate::config::cli::Args;
use crate::k8s::watch::WorldHandle;
use crate::state::model::Models;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    color_eyre::install()?;
    let log_path = logging::init(&args.log_level)?;
    let cfg = Config::load()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "k8sciv starting");

    let cluster = k8s::client::connect(args.kubeconfig.as_deref(), args.context.as_deref()).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(1024);
    let handle = k8s::watch::spawn(&cluster, tx.clone());

    if args.smoke {
        return smoke(handle).await;
    }

    // Terminal up only after a client exists, so connection errors print
    // like a normal CLI instead of corrupting an alternate screen.
    let terminal = ratatui::init();
    events::spawn_input_thread(tx.clone());
    let result = App::new(cfg, args.kubeconfig.clone(), handle, tx, rx)
        .run(terminal)
        .await;
    ratatui::restore();
    if result.is_err() {
        eprintln!("diagnostic log: {}", log_path.display());
    }
    result
}

/// --smoke: wait for the initial sync, print one summary line + top
/// concerns, exit. CI's way of asking "does the world assemble?".
async fn smoke(handle: WorldHandle) -> Result<()> {
    let w = &handle.world;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        w.nodes
            .wait_until_ready()
            .await
            .map_err(|e| eyre!("nodes store: {e}"))?;
        w.pods
            .wait_until_ready()
            .await
            .map_err(|e| eyre!("pods store: {e}"))?;
        w.deployments
            .wait_until_ready()
            .await
            .map_err(|e| eyre!("deployments store: {e}"))?;
        Ok::<_, color_eyre::Report>(())
    })
    .await
    .map_err(|_| eyre!("timed out waiting for initial sync (20s)"))??;

    // Give secondary stores a beat to fill.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let m = Models::build(w);
    println!(
        "context={} platform={} nodes={} pods={} workloads={} concerns={}",
        w.meta.context,
        w.meta.platform.label(),
        m.map.total_nodes,
        m.map.total_pods,
        m.workloads.len(),
        m.attention.len()
    );
    for c in m.attention.iter().take(8) {
        println!("  {} {} — {}", c.severity.glyph(), c.title, c.detail);
    }
    Ok(())
}
