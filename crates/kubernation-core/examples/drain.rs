//! Headless drain check — the instrument for `state::pdb`'s live gate.
//!
//! Connects, waits for the initial sync, and prints the per-node drain
//! constraint. It exists because item 2 has no user-facing surface yet (that is
//! item 3), and a derivation about a destructive operation should not ship on
//! unit tests alone.
//!
//! Read the FIRST line before any node line: it says whether the budgets were
//! read at all. "not read" and "no budgets" are different answers, and the whole
//! point of the derivation is that they stay different.
//!
//!   cargo run -p kubernation-core --example drain -- --context kind-kubernation

use std::time::Duration;

use color_eyre::eyre::{Result, eyre};

use kubernation_core::events::{ClusterId, WorldDelta};
use kubernation_core::k8s::{client, watch};
use kubernation_core::state::pdb::{Drain, drain_report};

#[tokio::main]
async fn main() -> Result<()> {
    let mut context = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--context" => context = it.next(),
            other => return Err(eyre!("unknown arg: {other}")),
        }
    }

    let cluster = client::connect(None, context.as_deref()).await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(ClusterId, WorldDelta)>(1024);
    let sink = move |id, delta| {
        let _ = tx.try_send((id, delta));
    };
    let hot = watch::spawn(&cluster, ClusterId::Hot, sink, &[]);

    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match rx.recv().await {
                Some((_, WorldDelta::Ready)) => break Ok(()),
                Some(_) => continue,
                None => break Err(eyre!("event channel closed before initial sync")),
            }
        }
    })
    .await
    .map_err(|_| eyre!("timed out waiting for initial sync (20s)"))??;

    // The PDB store syncs alongside the core ones; give it a beat. If RBAC denies
    // the LIST it will never sync, which is exactly what we want to observe.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Draining IS evicting, so the instrument reports the permission the evict
    // path actually needs — `create pods/eviction`, which is separately grantable
    // from `delete pods`.
    match kubernation_core::k8s::actions::can_evict_pod(cluster.client.clone(), "kubernation-demo")
        .await
    {
        Ok(v) => println!("may evict in kubernation-demo: {v}"),
        Err(e) => println!("may evict in kubernation-demo: unknown ({e})"),
    }

    let r = drain_report(&hot.world);
    println!(
        "budgets: {}",
        if r.observed {
            format!("{} read", r.budgets)
        } else {
            "NOT READ — every node below is unknown, not drainable".into()
        }
    );
    let mut names: Vec<String> = hot
        .world
        .nodes
        .state()
        .iter()
        .filter_map(|n| n.metadata.name.clone())
        .collect();
    names.sort();
    for name in names {
        match r.node(&name) {
            Some(n) => {
                let glyph = match n.state {
                    Drain::Allowed => "ok  ",
                    Drain::Blocked => "STOP",
                    Drain::Unknown => "?   ",
                };
                println!("  {glyph} {name} — {}", n.detail());
            }
            None => println!("  ---- {name} — not examined"),
        }
    }
    Ok(())
}
