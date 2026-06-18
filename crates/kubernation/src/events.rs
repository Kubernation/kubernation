use ratatui_crossterm::crossterm::event::{self, Event as TermEvent};
use tokio::sync::mpsc;

pub use kubernation_core::events::{ClusterId, WorldDelta};
use kubernation_core::k8s::actions::CommitOutcome;

/// Everything the app's single event loop selects over (besides ticks):
/// terminal input and core world-delta notifications, merged into one
/// channel so a single `recv` drives the loop.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Keyboard / resize / mouse from the terminal.
    Term(TermEvent),
    /// A slice of one observed world changed (payload-free, coalescable).
    World(ClusterId, WorldDelta),
    /// A pod-log fetch finished. `gen` lets the app drop stale results
    /// (after the user moved to a different pod).
    Logs {
        generation: u64,
        result: Result<String, String>,
    },
    /// A pod eviction finished; the payload is the line to flash (Ok = a
    /// success message, Err = the failure).
    Evicted { result: Result<String, String> },
    /// An End-of-Turn commit finished; the per-row outcome feeds the review.
    Committed { outcome: CommitOutcome },
    /// Resource discovery finished — the browsable kinds.
    Kinds(Vec<kubernation_core::k8s::browse::KindEntry>),
    /// A resource-browser LIST finished (Ok = objects + truncation, Err = message).
    /// `generation` drops a stale result whose kind the user already moved off.
    BrowseRows {
        generation: u64,
        result: Result<kubernation_core::k8s::browse::ListResult, String>,
    },
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
