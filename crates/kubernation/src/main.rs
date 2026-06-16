mod app;
mod config;
mod events;
mod logging;
mod ui;

use clap::Parser;
use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::app::App;
use crate::config::Config;
use crate::config::cli::Args;
use crate::events::ClusterId;
use kubernation_core::k8s::watch::WorldHandle;
use kubernation_core::state::model::Models;
use kubernation_core::state::pair::PairSync;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    color_eyre::install()?;
    let log_path = logging::init(&args.log_level)?;
    let cfg = Config::load()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "kubernation starting");

    let hot_cluster =
        kubernation_core::k8s::client::connect(args.kubeconfig.as_deref(), args.context.as_deref())
            .await?;
    let warm_name = args.warm.clone().or_else(|| cfg.warm_context.clone());
    if warm_name.as_deref() == Some(hot_cluster.meta.context.as_str()) {
        return Err(eyre!(
            "--warm must name a different context than the hot cluster ({})",
            hot_cluster.meta.context
        ));
    }
    let warm_cluster = match &warm_name {
        Some(w) => {
            Some(kubernation_core::k8s::client::connect(args.kubeconfig.as_deref(), Some(w)).await?)
        }
        None => None,
    };

    let mut wanted = args.project.clone();
    for p in &cfg.projections {
        if !wanted.contains(p) {
            wanted.push(p.clone());
        }
    }

    let (tx, rx) = tokio::sync::mpsc::channel(1024);
    let sink = {
        let tx = tx.clone();
        move |id, delta| {
            let _ = tx.try_send(events::AppEvent::World(id, delta));
        }
    };
    let hot_proj =
        kubernation_core::k8s::client::resolve_projections(&hot_cluster.client, &wanted).await;
    let hot =
        kubernation_core::k8s::watch::spawn(&hot_cluster, ClusterId::Hot, sink.clone(), &hot_proj);
    let warm = match &warm_cluster {
        Some(c) => {
            let proj = kubernation_core::k8s::client::resolve_projections(&c.client, &wanted).await;
            Some(kubernation_core::k8s::watch::spawn(
                c,
                ClusterId::Warm,
                sink,
                &proj,
            ))
        }
        None => None,
    };

    if args.smoke {
        return smoke(hot, warm, rx).await;
    }

    // Terminal up only after clients exist, so connection errors print
    // like a normal CLI instead of corrupting an alternate screen.
    let terminal = ratatui::init();
    events::spawn_input_thread(tx.clone());
    let result = App::new(
        cfg,
        args.kubeconfig.clone(),
        wanted,
        hot,
        warm,
        hot_cluster,
        warm_cluster,
        tx,
        rx,
    )
    .run(terminal)
    .await;
    ratatui::restore();
    if result.is_err() {
        eprintln!("diagnostic log: {}", log_path.display());
    }
    result
}

/// --smoke: wait for the initial sync, print one summary line per world
/// (plus pair drift when a warm cluster is attached), exit.
///
/// Listens for `WorldDelta::Ready` exactly like the TUI does, rather than
/// calling `Store::wait_until_ready` itself: kube's readiness signal holds a
/// single waker slot, so the readiness task in `k8s::watch` must be the only
/// concurrent waiter per store (see CLAUDE.md decisions log).
async fn smoke(
    hot: WorldHandle,
    warm: Option<WorldHandle>,
    mut rx: tokio::sync::mpsc::Receiver<events::AppEvent>,
) -> Result<()> {
    let want_warm = warm.is_some();
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let (mut hot_ready, mut warm_ready) = (false, !want_warm);
        loop {
            match rx.recv().await {
                Some(events::AppEvent::World(id, events::WorldDelta::Ready)) => {
                    match id {
                        ClusterId::Hot => hot_ready = true,
                        ClusterId::Warm => warm_ready = true,
                    }
                    if hot_ready && warm_ready {
                        break Ok(());
                    }
                }
                Some(_) => continue,
                None => break Err(eyre!("event channel closed before initial sync")),
            }
        }
    })
    .await
    .map_err(|_| eyre!("timed out waiting for initial sync (20s)"))??;

    // Give secondary stores a beat to fill.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let summarize = |label: &str, h: &WorldHandle, m: &Models| {
        println!(
            "{label}context={} platform={} nodes={} pods={} workloads={} customs={} concerns={} gauges={}",
            h.world.meta.context,
            h.world.meta.platform.label(),
            m.map.total_nodes,
            m.map.total_pods,
            m.workloads.len(),
            h.world.custom_entries().len(),
            m.attention.len(),
            if m.map.metrics_live {
                "live"
            } else {
                "requests"
            }
        );
    };

    let m_hot = Models::build(&hot.world);
    match &warm {
        None => {
            summarize("", &hot, &m_hot);
            for c in m_hot.attention.iter().take(8) {
                println!("  {} {} — {}", c.severity.glyph(), c.title, c.detail);
            }
        }
        Some(w) => {
            let m_warm = Models::build(&w.world);
            summarize("hot:  ", &hot, &m_hot);
            summarize("warm: ", w, &m_warm);
            let pair = PairSync::build(&hot.world, &w.world);
            println!("pair: {} drifting · {} missing", pair.drifted, pair.missing);
            let mut drifted: Vec<_> = pair
                .by_workload
                .iter()
                .filter(|(_, s)| !s.is_in_sync())
                .collect();
            drifted.sort_by(|a, b| a.0.cmp(b.0));
            for (r, st) in drifted.into_iter().take(10) {
                println!("  {} {} — {}", st.badge(), r, st.describe(ClusterId::Hot));
            }
        }
    }
    Ok(())
}
