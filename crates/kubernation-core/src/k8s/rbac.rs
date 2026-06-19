//! Self-scoped RBAC probes — "what can *I* do here?" (the Charter screen).
//!
//! A **read-only self-query**: the operator asks the apiserver about their own
//! access via `SelfSubjectAccessReview` (the exact mechanism `kubectl auth can-i`
//! uses). It is NOT privilege escalation and surfaces nothing secret — only
//! allowed/denied booleans for a curated set of `(verb, group, resource,
//! subresource)` tuples. So it lives here in the read/data layer (beside
//! `browse.rs`/`logs.rs`/`metrics.rs`), NOT in the one write file `actions.rs`.
//!
//! **SSAR-per-cell, authoritative.** Each cell is a real authorizer decision —
//! we never re-implement RBAC rule/wildcard/apiGroup matching client-side (which
//! `SelfSubjectRulesReview` would force and can get subtly wrong, and which
//! misses Node/Webhook authorizers). For a "kills surprise 403s" feature a false
//! ✓/✗ is the one unacceptable failure, so the apiserver decides every cell.

use futures::future::join_all;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use kube::Client;
use kube::api::{Api, PostParams};

/// How alarming an *allowed* capability is (drives the audit highlight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Escalation primitive (exec, secrets-list, rbac-write, SA-token, node proxy).
    Critical,
    /// Powerful but routine for an operator (delete pods, patch nodes, write deploys).
    High,
    /// Benign / read-ish.
    Normal,
}

/// One cell's question — a single `can-i` probe. Static: the curated grid is a
/// const table. `subresource` is a DISTINCT field (SSAR uses
/// `resource:"pods" + subresource:Some("log")`, never `resource:"pods/log"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessProbe {
    pub verb: &'static str,
    pub group: &'static str,               // "" = core group (NOT "core")
    pub resource: &'static str,            // plural, e.g. "pods"
    pub subresource: Option<&'static str>, // e.g. Some("log"); resource stays the parent
    pub namespaced: bool,                  // true → probe the active ns; false → namespace=None
    pub risk: Risk,
}

/// One cell's answer. `Unknown` carries the error — it is NEVER fabricated into
/// an allowed/denied (a missing answer must read as "?", not a guess).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Denied,
    Unknown(String),
}

/// Build the `ResourceAttributes` for a probe. Pure (no client) so the encoding
/// — especially the core-group `""` and the subresource split — is unit-testable.
pub fn resource_attributes(probe: &AccessProbe, namespace: Option<&str>) -> ResourceAttributes {
    ResourceAttributes {
        verb: Some(probe.verb.into()),
        group: Some(probe.group.into()),
        resource: Some(probe.resource.into()),
        subresource: probe.subresource.map(Into::into),
        namespace: namespace.map(Into::into),
        ..Default::default()
    }
}

/// Run one `SelfSubjectAccessReview`. Mirrors `actions::can_evict_pod`'s shape;
/// `None` status or a transport error → `Unknown` (never silently `Allowed`).
pub async fn can_i(client: Client, probe: &AccessProbe, namespace: Option<&str>) -> Verdict {
    let api: Api<SelfSubjectAccessReview> = Api::all(client);
    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(resource_attributes(probe, namespace)),
            ..Default::default()
        },
        ..Default::default()
    };
    match api.create(&PostParams::default(), &review).await {
        Ok(res) => match res.status {
            Some(s) if s.allowed => Verdict::Allowed,
            Some(_) => Verdict::Denied,
            None => Verdict::Unknown("apiserver returned no status".into()),
        },
        Err(e) => Verdict::Unknown(e.to_string()),
    }
}

/// Probe every cell concurrently (one round-trip wall-clock — the
/// `browse::discover` `join_all` precedent). Namespaced probes are scoped to
/// `namespace`; cluster-scoped probes pass `None`. Result is positional —
/// `verdicts[i]` answers `probes[i]`.
pub async fn matrix(client: Client, probes: &[AccessProbe], namespace: &str) -> Vec<Verdict> {
    let futs = probes.iter().map(|p| {
        let p = *p; // AccessProbe: Copy
        let client = client.clone();
        let ns = if p.namespaced {
            Some(namespace.to_string())
        } else {
            None
        };
        async move { can_i(client, &p, ns.as_deref()).await }
    });
    join_all(futs).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_encode_core_group_and_subresource() {
        // Core group is the empty string, not "core"; subresource is its own field.
        let log = AccessProbe {
            verb: "get",
            group: "",
            resource: "pods",
            subresource: Some("log"),
            namespaced: true,
            risk: Risk::Normal,
        };
        let a = resource_attributes(&log, Some("demo"));
        assert_eq!(a.group.as_deref(), Some(""));
        assert_eq!(a.resource.as_deref(), Some("pods"));
        assert_eq!(a.subresource.as_deref(), Some("log"));
        assert_eq!(a.verb.as_deref(), Some("get"));
        assert_eq!(a.namespace.as_deref(), Some("demo"));

        // A cluster-scoped probe carries no namespace.
        let nodes = AccessProbe {
            verb: "patch",
            group: "",
            resource: "nodes",
            subresource: None,
            namespaced: false,
            risk: Risk::High,
        };
        let a = resource_attributes(&nodes, None);
        assert_eq!(a.namespace, None);
        assert_eq!(a.subresource, None);
    }
}
