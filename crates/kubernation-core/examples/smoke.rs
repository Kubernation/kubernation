//! Headless smoke check — the CI / sanity gate. Connects, waits for the initial
//! sync, prints one summary line per world (plus pair drift when `--warm` is
//! given), and exits non-zero on failure. No UI: this is the (GUI-only) product's
//! stand-in for the former TUI `--smoke` flag, living in the pure core so it
//! needs no display.
//!
//!   cargo run -p kubernation-core --example smoke -- \
//!       --context kind-kubernation [--warm <ctx>] [--project <name>]...

use std::time::Duration;

use color_eyre::eyre::{Result, eyre};

use kubernation_core::events::{ClusterId, WorldDelta};
use kubernation_core::k8s::watch::WorldHandle;
use kubernation_core::k8s::{client, watch};
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::Models;
use kubernation_core::state::pair::PairSync;

#[tokio::main]
async fn main() -> Result<()> {
    // Minimal arg parse: --context <ctx> · --warm <ctx> · --project <name> (xN).
    let (mut context, mut warm, mut projects) = (None, None, Vec::new());
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--context" => context = it.next(),
            "--warm" => warm = it.next(),
            "--project" => {
                if let Some(p) = it.next() {
                    projects.push(p);
                }
            }
            other => return Err(eyre!("unknown arg: {other}")),
        }
    }

    let hot_cluster = client::connect(None, context.as_deref()).await?;
    let warm_cluster = match &warm {
        Some(w) => Some(client::connect(None, Some(w)).await?),
        None => None,
    };

    // One channel, a payload-free sink — exactly what the frontends subscribe.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(ClusterId, WorldDelta)>(1024);
    let sink = move |id, delta| {
        let _ = tx.try_send((id, delta));
    };
    let hot_proj = client::resolve_projections(&hot_cluster.client, &projects).await;
    let hot = watch::spawn(&hot_cluster, ClusterId::Hot, sink.clone(), &hot_proj);
    let warm = match &warm_cluster {
        Some(c) => {
            let proj = client::resolve_projections(&c.client, &projects).await;
            Some(watch::spawn(c, ClusterId::Warm, sink, &proj))
        }
        None => None,
    };

    // Wait for the initial sync (Ready per world). We listen for the delta
    // rather than calling `Store::wait_until_ready` ourselves — kube's readiness
    // holds a single waker slot, so the watch's readiness task must be the only
    // concurrent waiter (see CLAUDE.md decisions log).
    let want_warm = warm.is_some();
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut hot_ready, mut warm_ready) = (false, !want_warm);
        loop {
            match rx.recv().await {
                Some((id, WorldDelta::Ready)) => {
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
    tokio::time::sleep(Duration::from_millis(500)).await;

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
            },
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
            let pair = PairSync::build(&hot.world, &w.world, &NamespaceFilter::All);
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
