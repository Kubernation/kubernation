//! On-demand pod logs. Unlike the reflector-backed stores, logs are
//! fetched when a view asks for them: a one-shot tail of the last `TAIL`
//! lines. Frontends poll this every couple of seconds for a live tail
//! (the kube log *stream* is a fine future upgrade; polling the tail is
//! simpler and survives reconnects without stream lifecycle bookkeeping).

use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, LogParams};

/// Default trailing lines per fetch (the `LogWindow::Tail` window).
pub const TAIL: i64 = 500;
/// Larger line window (`LogWindow::More`).
const TAIL_MORE: i64 = 2000;
/// Safety ceiling on the time-windowed fetch so a chatty pod can't return an
/// unbounded dump.
const SINCE_CAP_LINES: i64 = 5000;

/// How much history a fetch pulls. Cycled by the view; maps to `LogParams`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogWindow {
    /// The last `TAIL` (500) lines — the default.
    #[default]
    Tail,
    /// The last `TAIL_MORE` (2000) lines.
    More,
    /// Everything from the last hour (capped at `SINCE_CAP_LINES`).
    Hour,
}

impl LogWindow {
    /// Cycle to the next window (for a single toggle key).
    pub fn next(self) -> Self {
        match self {
            LogWindow::Tail => LogWindow::More,
            LogWindow::More => LogWindow::Hour,
            LogWindow::Hour => LogWindow::Tail,
        }
    }

    /// Short label for the view title.
    pub fn label(self) -> &'static str {
        match self {
            LogWindow::Tail => "500",
            LogWindow::More => "2k",
            LogWindow::Hour => "1h",
        }
    }

    fn apply(self, lp: &mut LogParams) {
        match self {
            LogWindow::Tail => lp.tail_lines = Some(TAIL),
            LogWindow::More => lp.tail_lines = Some(TAIL_MORE),
            LogWindow::Hour => {
                lp.since_seconds = Some(3600);
                lp.tail_lines = Some(SINCE_CAP_LINES);
            }
        }
    }
}

/// How a view wants its tail fetched. A small struct (vs. a pile of bool args)
/// so new fetch knobs ride along without churning every call site; it is also
/// the change-detection key the GUI poll compares (`PartialEq`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogOpts {
    /// Tail the *previously terminated* container (`kubectl logs --previous`).
    pub previous: bool,
    /// Prefix each line with the server's RFC3339 timestamp.
    pub timestamps: bool,
    /// How much history to pull.
    pub window: LogWindow,
}

/// Fetch the recent log tail for one pod. `container` is required only for
/// multi-container pods; `None` lets the server pick the sole container.
/// `opts` selects the previous-container tail (the crash-loop's last words —
/// the server errors if no previous instance exists, surfaced inline),
/// timestamps, and the history window. Errors are returned as display strings.
pub async fn tail(
    client: Client,
    namespace: &str,
    pod: &str,
    container: Option<String>,
    opts: &LogOpts,
) -> Result<String, String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let mut lp = LogParams {
        container,
        timestamps: opts.timestamps,
        previous: opts.previous,
        ..Default::default()
    };
    opts.window.apply(&mut lp);
    api.logs(pod, &lp)
        .await
        .map_err(|e| classify_log_err(e, pod, opts.previous))
}

/// Turn a kube error into a short, operator-legible line for the overlay.
/// The common one is asking for the *previous* container on a pod that hasn't
/// restarted — the server returns a verbose 400 (`previous terminated container
/// "x" in pod "y" not found: BadRequest (Status { … })`) that both reads badly
/// and (in the GUI) overran the window; collapse it to a one-liner.
fn classify_log_err(e: kube::Error, pod: &str, previous: bool) -> String {
    if let kube::Error::Api(status) = &e {
        let msg = status.message.to_lowercase();
        if previous && (msg.contains("previous terminated container") || msg.contains("not found"))
        {
            return format!(
                "{pod} has no previous container (it hasn't restarted) — press p for the live tail"
            );
        }
        match status.code {
            403 => return format!("forbidden — you can't read logs for {pod}"),
            404 => return format!("{pod} not found (it may have been deleted)"),
            _ => {}
        }
    }
    e.to_string()
}

/// First container name of a pod, so logs work on multi-container pods
/// without the caller guessing.
pub async fn first_container(client: Client, namespace: &str, pod: &str) -> Option<String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let p = api.get(pod).await.ok()?;
    p.spec?.containers.first().and_then(|c| {
        if c.name.is_empty() {
            None
        } else {
            Some(c.name.clone())
        }
    })
}
