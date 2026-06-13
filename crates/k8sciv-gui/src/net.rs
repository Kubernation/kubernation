//! The network side: tokio + the core watchers on a background thread,
//! publishing snapshots the render loop reads without ever blocking on
//! the cluster. `ObservedWorld` rides along (its stores are cheap Arc
//! clones) so detail panels can run the pure city/node builders on demand.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use k8sciv_core::events::{ClusterId, WorldDelta};
use k8sciv_core::k8s::{client, watch};
use k8sciv_core::state::model::Models;
use k8sciv_core::state::observed::ObservedWorld;

pub struct Snapshot {
    pub models: Arc<Models>,
    pub observed: ObservedWorld,
}

pub struct Net {
    pub snapshot: Mutex<Option<Arc<Snapshot>>>,
    pub status: Mutex<String>,
}

impl Net {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(None),
            status: Mutex::new("starting…".into()),
        })
    }

    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }
}

pub struct NetArgs {
    pub context: Option<String>,
    pub kubeconfig: Option<PathBuf>,
    pub projections: Vec<String>,
}

pub fn spawn(args: NetArgs, net: Arc<Net>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            *net.status.lock().unwrap() = "connecting…".into();
            let cluster =
                match client::connect(args.kubeconfig.as_deref(), args.context.as_deref()).await {
                    Ok(c) => c,
                    Err(err) => {
                        *net.status.lock().unwrap() = format!("connect failed: {err}");
                        return;
                    }
                };
            let label = format!(
                "{} · {}",
                cluster.meta.context,
                cluster.meta.platform.label()
            );
            *net.status.lock().unwrap() = format!("{label} · exploring…");
            let proj = client::resolve_projections(&cluster.client, &args.projections).await;

            let dirty = Arc::new(AtomicBool::new(false));
            let ready = Arc::new(AtomicBool::new(false));
            let sink = {
                let dirty = dirty.clone();
                let ready = ready.clone();
                move |_id: ClusterId, delta: WorldDelta| {
                    if delta == WorldDelta::Ready {
                        ready.store(true, Ordering::Relaxed);
                    }
                    dirty.store(true, Ordering::Relaxed);
                }
            };
            let handle = watch::spawn(&cluster, ClusterId::Hot, sink, &proj);

            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                if ready.load(Ordering::Relaxed) && dirty.swap(false, Ordering::Relaxed) {
                    let models = Models::build(&handle.world);
                    *net.status.lock().unwrap() = label.clone();
                    *net.snapshot.lock().unwrap() = Some(Arc::new(Snapshot {
                        models: Arc::new(models),
                        observed: handle.world.clone(),
                    }));
                }
            }
        });
    });
}
