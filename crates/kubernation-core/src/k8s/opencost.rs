//! OpenCost adapter — the **impure** half: poll the in-cluster OpenCost
//! `/allocation` API through the kube API-server **service proxy**.
//!
//! This is the first reusable "in-cluster HTTP source" substrate. It reaches any
//! in-cluster Service via `GET /api/v1/namespaces/{ns}/services/{svc}:{port}/
//! proxy/{path}` on the SAME authenticated kube connection as the reflectors — no
//! port-forward, no new off-laptop egress, gated by `get services/proxy` RBAC.
//! Fetch-not-watch, like [`metrics`](super::metrics): a poll loop fills a shared
//! store; the pure parse lives in [`state::opencost`](crate::state::opencost).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::AsyncReadExt;
use kube::Client;
use tokio::task::JoinHandle;

use crate::state::opencost::{OpenCostData, parse_allocation};

/// Poll cadence (OpenCost recomputes from Prometheus; ~1 min is plenty).
const POLL: Duration = Duration::from_secs(60);
/// Per-request timeout — a hung OpenCost must not wedge the poller.
const TIMEOUT: Duration = Duration::from_secs(20);
/// Response body cap (8 MiB, matching the Oracle client) — a misconfigured or
/// hostile in-cluster Service reached through the proxy can't OOM the process.
const MAX_RESP_BYTES: u64 = 8 * 1024 * 1024;

/// Where OpenCost lives in-cluster + the query window. Defaults match a stock
/// `helm install opencost` (Service `opencost` in namespace `opencost`, port 9003).
#[derive(Debug, Clone)]
pub struct OpenCostSource {
    pub namespace: String,
    pub service: String,
    pub port: u16,
    /// The allocation window (e.g. "1d", "today", "1h").
    pub window: String,
}

impl Default for OpenCostSource {
    fn default() -> Self {
        OpenCostSource {
            namespace: "opencost".into(),
            service: "opencost".into(),
            port: 9003,
            window: "1d".into(),
        }
    }
}

impl OpenCostSource {
    /// Parse a `ns/service:port` ref; any part may be omitted → the default.
    pub fn parse(s: &str) -> Self {
        let mut src = OpenCostSource::default();
        let s = s.trim();
        if s.is_empty() {
            return src;
        }
        let (ns_svc, port) = s.split_once(':').unwrap_or((s, ""));
        if let Ok(p) = port.trim().parse::<u16>()
            && p > 0
        {
            src.port = p;
        }
        if let Some((ns, svc)) = ns_svc.split_once('/') {
            if !ns.is_empty() {
                src.namespace = ns.to_string();
            }
            if !svc.is_empty() {
                src.service = svc.to_string();
            }
        } else if !ns_svc.is_empty() {
            src.service = ns_svc.to_string();
        }
        src
    }
}

/// Shared, pollable OpenCost state. `available` flips false the moment a poll
/// fails, so the frontend degrades cleanly (it falls back to the request-based
/// cost). The last good `data` is kept so a transient blip doesn't blank it.
#[derive(Default)]
pub struct OpenCostStore {
    pub data: Option<OpenCostData>,
    pub available: bool,
    pub error: Option<String>,
}
pub type OpenCostArc = Arc<Mutex<OpenCostStore>>;

/// GET an in-cluster Service path through the API-server proxy (reusable for any
/// in-cluster HTTP tool). Returns the response body text. Needs `get
/// services/proxy` RBAC for the namespace.
pub async fn fetch_service_proxy(
    client: &Client,
    namespace: &str,
    service: &str,
    port: u16,
    path_and_query: &str,
) -> Result<String, String> {
    let p = path_and_query.trim_start_matches('/');
    let uri = format!("/api/v1/namespaces/{namespace}/services/{service}:{port}/proxy/{p}");
    let req = http::Request::get(&uri)
        .body(Vec::new())
        .map_err(|e| format!("request: {e}"))?;
    // Stream + cap the body (request_text would buffer it unbounded) so a huge or
    // hostile response can't OOM the process; the timeout bounds wall-clock.
    let fut = async {
        let stream = client
            .request_stream(req)
            .await
            .map_err(|e| classify(&e.to_string()))?;
        let mut reader = Box::pin(stream).take(MAX_RESP_BYTES + 1);
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("read: {e}"))?;
        if buf.len() as u64 > MAX_RESP_BYTES {
            return Err(format!(
                "response exceeds {} MiB cap",
                MAX_RESP_BYTES / (1024 * 1024)
            ));
        }
        Ok::<_, String>(String::from_utf8_lossy(&buf).into_owned())
    };
    match tokio::time::timeout(TIMEOUT, fut).await {
        Ok(r) => r,
        Err(_) => Err("timed out".into()),
    }
}

/// A DNS-1123 label (k8s namespace/service names) — lowercase alnum + `-`, edges
/// alphanumeric, ≤63 chars. Rejecting a non-label keeps a typo from producing a
/// silently-wrong "from OpenCost" number (it errors → honest fallback instead).
fn valid_dns_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.bytes()
            .next_back()
            .is_some_and(|b| b.is_ascii_alphanumeric())
}

/// A safe OpenCost window token — no URL-control chars that could truncate or
/// inject query params (`#`, `&`, `?`, `/`, whitespace), non-empty, ≤32.
fn valid_window(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && !s
            .bytes()
            .any(|b| matches!(b, b'#' | b'&' | b'?' | b'/' | b'%') || b.is_ascii_whitespace())
}

fn classify(e: &str) -> String {
    let lo = e.to_lowercase();
    if e.contains("403") || lo.contains("forbidden") {
        "forbidden — need RBAC get services/proxy".into()
    } else if e.contains("404") || lo.contains("not found") {
        "not found — is OpenCost installed at that service/port?".into()
    } else {
        e.chars().take(160).collect()
    }
}

/// Fetch + parse one OpenCost allocation snapshot.
pub async fn fetch_once(client: &Client, source: &OpenCostSource) -> Result<OpenCostData, String> {
    // Validate up front: a bad ns/svc/window must error (→ honest fallback to the
    // estimate) rather than build a wrong-but-authoritative "from OpenCost" number.
    if !valid_dns_label(&source.namespace) || !valid_dns_label(&source.service) {
        return Err("invalid namespace/service (must be DNS-1123 labels)".into());
    }
    if !valid_window(&source.window) {
        return Err("invalid window (use e.g. 1d, today, 1h)".into());
    }
    // accumulate=true collapses any step-sets into one; aggregate=namespace,controller
    // gives the per-workload + per-namespace rollup, disambiguated across namespaces.
    // OpenCost bills by workload/namespace, not node — per-node breakdown is
    // deliberately NOT requested here (it's unreliable under controller aggregation:
    // a multi-node controller has no single node), so `by_node` stays empty and the
    // map overlay recedes to idle-land under OpenCost (the $ rollup is the value; a
    // per-node OpenCost overlay via an aggregate=node query is a future increment).
    // includeIdle adds the cluster __idle__ line.
    let q = format!(
        "allocation?window={}&aggregate=namespace,controller&includeIdle=true&accumulate=true",
        source.window
    );
    let body =
        fetch_service_proxy(client, &source.namespace, &source.service, source.port, &q).await?;
    parse_allocation(&body)
}

/// Spawn a poll loop filling `store` every `POLL`. Abort the returned handle to stop.
pub fn spawn(client: Client, source: OpenCostSource, store: OpenCostArc) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match fetch_once(&client, &source).await {
                Ok(data) => {
                    if let Ok(mut g) = store.lock() {
                        g.data = Some(data);
                        g.available = true;
                        g.error = None;
                    }
                }
                Err(e) => {
                    tracing::debug!(%e, "OpenCost poll failed");
                    if let Ok(mut g) = store.lock() {
                        g.available = false;
                        g.error = Some(e);
                    }
                }
            }
            tokio::time::sleep(POLL).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_parse_handles_partial_refs() {
        let d = OpenCostSource::default();
        assert_eq!(
            (d.namespace.as_str(), d.service.as_str(), d.port),
            ("opencost", "opencost", 9003)
        );
        let s = OpenCostSource::parse("kubecost/kubecost-cost-analyzer:9090");
        assert_eq!(
            (s.namespace.as_str(), s.service.as_str(), s.port),
            ("kubecost", "kubecost-cost-analyzer", 9090)
        );
        // service only.
        let s = OpenCostSource::parse("mycost");
        assert_eq!(
            (s.namespace.as_str(), s.service.as_str()),
            ("opencost", "mycost")
        );
        // empty → default; bad port ignored.
        assert_eq!(OpenCostSource::parse("").port, 9003);
        assert_eq!(OpenCostSource::parse("a/b:nope").port, 9003);
    }

    #[test]
    fn input_validation_rejects_unsafe_refs() {
        assert!(valid_dns_label("opencost"));
        assert!(valid_dns_label("kubecost-cost-analyzer"));
        assert!(!valid_dns_label("")); // empty
        assert!(!valid_dns_label("-bad")); // edge non-alnum
        assert!(!valid_dns_label("Bad")); // uppercase
        assert!(!valid_dns_label("a/b")); // path injection
        assert!(valid_window("1d"));
        assert!(valid_window("today"));
        assert!(valid_window("7d"));
        assert!(!valid_window("")); // empty
        assert!(!valid_window("1d&aggregate=node")); // param injection
        assert!(!valid_window("1d#x")); // fragment truncation
        assert!(!valid_window("a b")); // whitespace
    }
}
