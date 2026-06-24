//! In-cluster HTTP-source **substrate** — reach any in-cluster Service read-only
//! through the kube API-server **service proxy**, on the SAME authenticated
//! connection as the reflectors: no port-forward, no new off-laptop egress, gated
//! by `get services/proxy` RBAC. The OpenCost adapter ([`super::opencost`]) is the
//! first consumer; a Prometheus/PromQL, kube-state-metrics, or Hubble source would
//! ride this same fetch + validation + body-cap + timeout rather than re-deriving
//! it.
//!
//! **Pattern for a new adapter:**
//!  1. a `*Source { namespace, service, port, … }` config — validate the parts with
//!     [`valid_dns_label`] (ns/service) and [`valid_query_token`] (query values), so
//!     a typo errors honestly instead of building a wrong-but-authoritative result;
//!  2. `adapter::`[`fetch_service_proxy`]`(client, ns, svc, port, "path?query",
//!     timeout)` → the response body text (streamed + capped at [`MAX_RESP_BYTES`],
//!     bounded by `timeout`, errors classified by [`classify`]);
//!  3. a **PURE** parser in `state/` turning the body into a typed model
//!     (tolerant, never panics);
//!  4. a fetch-not-watch poller (like `opencost::spawn`) filling a shared store the
//!     net thread reads at snapshot time.

use std::time::Duration;

use futures::AsyncReadExt;
use kube::Client;

/// Default per-request timeout — a hung in-cluster service must not wedge a poller.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Response body cap (8 MiB) — a misconfigured or hostile in-cluster Service reached
/// through the proxy can't OOM the process.
pub const MAX_RESP_BYTES: u64 = 8 * 1024 * 1024;

/// GET an in-cluster Service path through the API-server proxy. Streams + caps the
/// body (so it can't OOM the process) and bounds it by `timeout`. Returns the
/// response body text. Needs `get services/proxy` RBAC for the namespace.
pub async fn fetch_service_proxy(
    client: &Client,
    namespace: &str,
    service: &str,
    port: u16,
    path_and_query: &str,
    timeout: Duration,
) -> Result<String, String> {
    let p = path_and_query.trim_start_matches('/');
    let uri = format!("/api/v1/namespaces/{namespace}/services/{service}:{port}/proxy/{p}");
    let req = http::Request::get(&uri)
        .body(Vec::new())
        .map_err(|e| format!("request: {e}"))?;
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
    match tokio::time::timeout(timeout, fut).await {
        Ok(r) => r,
        Err(_) => Err("timed out".into()),
    }
}

/// Collapse a kube/transport error from the proxy fetch into a short, legible line.
pub fn classify(e: &str) -> String {
    let lo = e.to_lowercase();
    if e.contains("403") || lo.contains("forbidden") {
        "forbidden — need RBAC get services/proxy".into()
    } else if e.contains("404") || lo.contains("not found") {
        "not found — is the service installed at that namespace/port?".into()
    } else {
        e.chars().take(160).collect()
    }
}

/// A DNS-1123 label (k8s namespace/service names) — lowercase alnum + `-`, edges
/// alphanumeric, ≤63 chars. Rejecting a non-label keeps a typo from producing a
/// silently-wrong result (the caller errors → an honest fallback instead).
pub fn valid_dns_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.bytes()
            .next_back()
            .is_some_and(|b| b.is_ascii_alphanumeric())
}

/// A safe query-param value — no URL-control chars that could truncate or inject
/// params (`#`, `&`, `?`, `/`, `%`, whitespace), non-empty, ≤32.
pub fn valid_query_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && !s
            .bytes()
            .any(|b| matches!(b, b'#' | b'&' | b'?' | b'/' | b'%') || b.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_validation_rejects_unsafe_refs() {
        assert!(valid_dns_label("opencost"));
        assert!(valid_dns_label("kubecost-cost-analyzer"));
        assert!(!valid_dns_label("")); // empty
        assert!(!valid_dns_label("-bad")); // edge non-alnum
        assert!(!valid_dns_label("Bad")); // uppercase
        assert!(!valid_dns_label("a/b")); // path injection
        assert!(valid_query_token("1d"));
        assert!(valid_query_token("today"));
        assert!(!valid_query_token("")); // empty
        assert!(!valid_query_token("1d&aggregate=node")); // param injection
        assert!(!valid_query_token("1d#x")); // fragment truncation
        assert!(!valid_query_token("a b")); // whitespace
    }
}
