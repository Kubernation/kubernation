//! Headless right-sizing check — the live instrument for the P90 basis.
//!
//! The advisor's recommendation is a number an operator edits a manifest from,
//! and it is only as good as the window behind it. Unit tests pin the maths; this
//! watches a REAL metrics-server fill the rings and the basis flip from
//! `Latest` to `P90` on a live cluster, which no test can do — the ring needs
//! `P90_MIN_SAMPLES` polls at 15s, about two minutes.
//!
//! Read the `basis` column: `latest` means the ring is still short, and every
//! suggestion on that line rests on ONE reading.
//!
//!   cargo run -p kubernation-core --example rightsize -- --context kind-kubernation

use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use kubernation_core::events::{ClusterId, WorldDelta};
use kubernation_core::k8s::{client, watch};
use kubernation_core::state::advisor::{RsRow, UsageBasis, rightsizing_report};

fn basis_of(r: &RsRow) -> String {
    match r.basis {
        UsageBasis::Latest => "latest (1 sample)".into(),
        UsageBasis::P90 { samples } => format!("P90 over {samples}"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (mut context, mut minutes) = (None, 3u64);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--context" => context = it.next(),
            "--minutes" => minutes = it.next().and_then(|v| v.parse().ok()).unwrap_or(3),
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

    // Poll until the rings fill. Reported each interval so a run that never
    // reaches P90 shows WHY — a cluster without metrics-server stays at
    // "no metrics", not at a plausible-looking `latest`.
    let deadline = Instant::now() + Duration::from_secs(minutes * 60);
    loop {
        let r = rightsizing_report(&hot.world);
        let elapsed = minutes * 60 - deadline.saturating_duration_since(Instant::now()).as_secs();
        if !r.metrics_available {
            println!(
                "[{elapsed:>3}s] no metrics-server — right-sizing degrades to the unrequested list"
            );
        } else {
            let rows: Vec<&RsRow> = r.over.iter().chain(&r.under).collect();
            let p90 = rows
                .iter()
                .filter(|x| matches!(x.basis, UsageBasis::P90 { .. }))
                .count();
            println!(
                "[{elapsed:>3}s] {} measured rows, {p90} on a P90 window",
                rows.len()
            );
            for row in rows.iter().take(4) {
                println!(
                    "         {:<28} cpu req {:.3} use {:.3} → {:<7} basis {}",
                    format!("{}/{}", row.namespace, row.name),
                    row.cpu.request,
                    row.cpu.usage,
                    row.cpu
                        .suggested
                        .map_or("—".to_string(), |s| format!("{s:.3}")),
                    basis_of(row)
                );
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    Ok(())
}
