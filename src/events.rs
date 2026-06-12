use ratatui_crossterm::crossterm::event::{self, Event as TermEvent};
use tokio::sync::mpsc;

/// Which member of the (future-pair-ready) cluster set an event came from.
/// Single-cluster sessions only ever see `Hot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClusterId {
    Hot,
    Warm,
}

impl ClusterId {
    pub fn label(self) -> &'static str {
        match self {
            ClusterId::Hot => "HOT",
            ClusterId::Warm => "WARM",
        }
    }
}

/// Everything the app's single event loop selects over (besides ticks).
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Keyboard / resize / mouse from the terminal.
    Term(TermEvent),
    /// Coarse "something in this slice of the world changed" notification.
    /// The UI re-derives its view models from the store on the next tick;
    /// deltas carry no payload so they are trivially coalescable.
    World(ClusterId, WorldDelta),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldDelta {
    Nodes,
    Pods,
    Workloads,
    Storage,
    Services,
    Events,
    /// Node and pod stores have completed their initial list.
    Ready,
}

/// Terminal input arrives on a dedicated OS thread feeding the async loop.
/// (`ratatui-crossterm` does not expose crossterm's `event-stream` feature,
/// and a blocking reader thread avoids any crossterm version skew.)
pub fn spawn_input_thread(tx: mpsc::Sender<AppEvent>) {
    std::thread::Builder::new()
        .name("input".into())
        .spawn(move || {
            loop {
                match event::read() {
                    Ok(ev) => {
                        if tx.blocking_send(AppEvent::Term(ev)).is_err() {
                            break; // app gone
                        }
                    }
                    Err(err) => {
                        tracing::error!(%err, "terminal input thread exiting");
                        break;
                    }
                }
            }
        })
        .expect("spawn input thread");
}
