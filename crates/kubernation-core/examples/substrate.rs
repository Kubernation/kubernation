//! Headless substrate check — the instrument for `state::substrate`'s live gate.
//!
//! Connects, waits for the initial sync, and prints the fleet's DaemonSet
//! coverage exactly as `coverage_report` derives it: which DaemonSets clear the
//! prevalence bar, and which nodes are missing which of them. The Advisors >
//! Substrate tab and the `substrate` map overlay both read this one report, so
//! this is the set to check a capture against.
//!
//! Read the FIRST line: at 4 nodes or fewer no gap is representable at all, and
//! the instrument says so rather than printing an empty table that reads as
//! "all covered".
//!
//!   cargo run -p kubernation-core --example substrate -- --context kind-kubernation

use std::time::Duration;

use color_eyre::eyre::{Result, eyre};

use kubernation_core::events::{ClusterId, WorldDelta};
use kubernation_core::k8s::{client, watch};
use kubernation_core::state::substrate::{coverage_report, floor_binds, prevalence_note};

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

    let r = coverage_report(&hot.world);
    if floor_binds(r.nodes_total) {
        println!(
            "{} nodes: no gap is representable at this size (the floor binds)",
            r.nodes_total
        );
    } else if !r.has_data() {
        println!(
            "{} nodes: no daemonset reaches the fleet bar — not 'all covered'",
            r.nodes_total
        );
    } else {
        println!(
            "{} fleet-wide daemonsets · {} of {} nodes with gaps",
            r.expected.len(),
            r.nodes_with_gaps,
            r.nodes_total
        );
    }
    println!("({})", prevalence_note());
    for ds in &r.expected {
        println!("expected: {ds}");
    }
    let mut nodes: Vec<&String> = r.missing_by_node.keys().collect();
    nodes.sort();
    for n in nodes {
        println!("gap: {n} missing {}", r.missing(n).join(", "));
    }
    Ok(())
}
