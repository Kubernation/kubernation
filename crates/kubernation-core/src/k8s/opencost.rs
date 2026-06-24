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

use kube::Client;
use tokio::task::JoinHandle;

use super::adapter;
use crate::state::opencost::{OpenCostData, parse_allocation};

/// Poll cadence (OpenCost recomputes from Prometheus; ~1 min is plenty).
const POLL: Duration = Duration::from_secs(60);

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

/// Fetch + parse one OpenCost allocation snapshot (over the reusable
/// [`adapter::fetch_service_proxy`] substrate).
pub async fn fetch_once(client: &Client, source: &OpenCostSource) -> Result<OpenCostData, String> {
    // Validate up front: a bad ns/svc/window must error (→ honest fallback to the
    // estimate) rather than build a wrong-but-authoritative "from OpenCost" number.
    if !adapter::valid_dns_label(&source.namespace) || !adapter::valid_dns_label(&source.service) {
        return Err("invalid namespace/service (must be DNS-1123 labels)".into());
    }
    if !adapter::valid_query_token(&source.window) {
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
    let body = adapter::fetch_service_proxy(
        client,
        &source.namespace,
        &source.service,
        source.port,
        &q,
        adapter::DEFAULT_TIMEOUT,
    )
    .await?;
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
    // The input-validation tests moved to `k8s::adapter` with the helpers.
}
