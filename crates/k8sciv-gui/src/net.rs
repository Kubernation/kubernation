//! The network side: tokio + the core watchers on a background thread,
//! publishing snapshots the render loop reads without ever blocking on
//! the cluster. `ObservedWorld` rides along (its stores are cheap Arc
//! clones) so detail panels can run the pure city/node builders on demand.
//! With `--warm`, a second world is watched and compared — the GUI shows
//! it as a second archipelago east of the hot one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use k8sciv_core::events::{ClusterId, WorldDelta};
use k8sciv_core::k8s::{client, watch};
use k8sciv_core::state::attention::Concern;
use k8sciv_core::state::model::Models;
use k8sciv_core::state::observed::ObservedWorld;
use k8sciv_core::state::pair::PairSync;

pub struct WorldSnap {
    pub models: Arc<Models>,
    pub observed: ObservedWorld,
}

pub struct Snapshot {
    pub hot: WorldSnap,
    pub warm: Option<WorldSnap>,
    pub pair: Option<Arc<PairSync>>,
    /// Merged severity-ordered concerns across both worlds (tagged with
    /// their cluster), plus the single aggregate pair-drift concern.
    pub attention: Arc<Vec<Concern>>,
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
    pub warm: Option<String>,
    pub projections: Vec<String>,
}

pub fn spawn(args: NetArgs, net: Arc<Net>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            *net.status.lock().unwrap() = "connecting…".into();
            let hot_cluster =
                match client::connect(args.kubeconfig.as_deref(), args.context.as_deref()).await {
                    Ok(c) => c,
                    Err(err) => {
                        *net.status.lock().unwrap() = format!("connect failed: {err}");
                        return;
                    }
                };
            // A warm failure degrades to single-world rather than aborting.
            let warm_cluster = match &args.warm {
                Some(w) if *w == hot_cluster.meta.context => {
                    *net.status.lock().unwrap() =
                        "warm context equals hot; running single-world".into();
                    None
                }
                Some(w) => match client::connect(args.kubeconfig.as_deref(), Some(w)).await {
                    Ok(c) => Some(c),
                    Err(err) => {
                        *net.status.lock().unwrap() = format!("warm connect failed: {err}");
                        None
                    }
                },

                None => None,
            };
            let label = match &warm_cluster {
                Some(w) => format!("HOT {} / WARM {}", hot_cluster.meta.context, w.meta.context),
                None => format!(
                    "{} · {}",
                    hot_cluster.meta.context,
                    hot_cluster.meta.platform.label()
                ),
            };
            *net.status.lock().unwrap() = format!("{label} · exploring…");

            let dirty = Arc::new(AtomicBool::new(false));
            let ready_hot = Arc::new(AtomicBool::new(false));
            let ready_warm = Arc::new(AtomicBool::new(false));
            let sink = {
                let dirty = dirty.clone();
                let ready_hot = ready_hot.clone();
                let ready_warm = ready_warm.clone();
                move |id: ClusterId, delta: WorldDelta| {
                    if delta == WorldDelta::Ready {
                        match id {
                            ClusterId::Hot => ready_hot.store(true, Ordering::Relaxed),
                            ClusterId::Warm => ready_warm.store(true, Ordering::Relaxed),
                        }
                    }
                    dirty.store(true, Ordering::Relaxed);
                }
            };

            let hot_proj =
                client::resolve_projections(&hot_cluster.client, &args.projections).await;
            let hot_handle = watch::spawn(&hot_cluster, ClusterId::Hot, sink.clone(), &hot_proj);
            let warm_handle = match &warm_cluster {
                Some(c) => {
                    let proj = client::resolve_projections(&c.client, &args.projections).await;
                    Some(watch::spawn(c, ClusterId::Warm, sink, &proj))
                }
                None => None,
            };

            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                if !ready_hot.load(Ordering::Relaxed) || !dirty.swap(false, Ordering::Relaxed) {
                    continue;
                }
                let hot_models = Arc::new(Models::build(&hot_handle.world));
                let warm = warm_handle
                    .as_ref()
                    .filter(|_| ready_warm.load(Ordering::Relaxed))
                    .map(|h| WorldSnap {
                        models: Arc::new(Models::build(&h.world)),
                        observed: h.world.clone(),
                    });
                let pair = warm
                    .as_ref()
                    .map(|w| Arc::new(PairSync::build(&hot_handle.world, &w.observed)));

                let mut merged = hot_models.attention.clone();
                if let Some(w) = &warm {
                    merged.extend(w.models.attention.iter().cloned().map(|mut c| {
                        c.cluster = ClusterId::Warm;
                        c
                    }));
                }
                if let Some(c) = pair.as_ref().and_then(|p| p.concern()) {
                    merged.push(c);
                }
                merged.sort_by(|a, b| {
                    b.severity
                        .cmp(&a.severity)
                        .then_with(|| a.key.cmp(&b.key))
                        .then_with(|| a.cluster.cmp(&b.cluster))
                });

                *net.status.lock().unwrap() = label.clone();
                *net.snapshot.lock().unwrap() = Some(Arc::new(Snapshot {
                    hot: WorldSnap {
                        models: hot_models,
                        observed: hot_handle.world.clone(),
                    },
                    warm,
                    pair,
                    attention: Arc::new(merged),
                }));
            }
        });
    });
}
