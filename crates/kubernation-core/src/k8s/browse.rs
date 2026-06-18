//! The resource browser's data layer: discover the cluster's resource kinds,
//! and LIST any one of them on demand (fetch-not-watch, like `logs`/`metrics`).
//! Everything goes through `DynamicObject`, so a single path serves every kind
//! — built-in or CRD. Pure row/label helpers sit beside the async fetches and
//! are unit-tested without a cluster.

use kube::Client;
use kube::api::{Api, ListParams};
use kube::core::{ApiResource, DynamicObject};
use kube::discovery::{Discovery, Scope};

// Re-exported so frontends can name browsed objects without a direct `kube` dep.
pub use kube::core::DynamicObject as Object;

/// One browsable resource kind (the preferred version of a discovered kind).
#[derive(Debug, Clone)]
pub struct KindEntry {
    pub api: ApiResource,
    pub namespaced: bool,
}

impl KindEntry {
    /// Picker label: `plural` for core kinds, `plural.group` otherwise — so
    /// `pods`, `deployments.apps`, `gizmos.example.com`.
    pub fn label(&self) -> String {
        if self.api.group.is_empty() {
            self.api.plural.clone()
        } else {
            format!("{}.{}", self.api.plural, self.api.group)
        }
    }
}

/// One row in the browser table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseRow {
    pub namespace: String,
    pub name: String,
    pub age: String,
}

/// Discover every served resource kind (the recommended version of each),
/// sorted by label. Best-effort: a discovery error yields an empty list.
pub async fn discover(client: &Client) -> Vec<KindEntry> {
    let disc = match Discovery::new(client.clone()).run().await {
        Ok(d) => d,
        Err(err) => {
            tracing::warn!(%err, "resource discovery failed");
            return Vec::new();
        }
    };
    let mut out: Vec<KindEntry> = Vec::new();
    for group in disc.groups() {
        for (api, caps) in group.recommended_resources() {
            // Skip subresources (status/scale/…) — their plural carries a `/`.
            if api.plural.contains('/') {
                continue;
            }
            out.push(KindEntry {
                namespaced: caps.scope == Scope::Namespaced,
                api,
            });
        }
    }
    out.sort_by_key(|k| k.label());
    out.dedup_by(|a, b| a.label() == b.label());
    out
}

/// LIST one kind across all namespaces (cluster-wide list endpoint; the table
/// shows the namespace column). Capped so a huge kind can't flood the view.
pub async fn list_kind(client: &Client, entry: &KindEntry) -> Result<Vec<DynamicObject>, String> {
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &entry.api);
    api.list(&ListParams::default().limit(500))
        .await
        .map(|l| l.items)
        .map_err(|e| e.to_string())
}

/// A table row from an object's metadata.
pub fn row(obj: &DynamicObject) -> BrowseRow {
    BrowseRow {
        namespace: obj.metadata.namespace.clone().unwrap_or_default(),
        name: obj.metadata.name.clone().unwrap_or_default(),
        age: crate::util::format_age_opt(obj.metadata.creation_timestamp.as_ref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::core::TypeMeta;

    fn obj(kind: &str, ns: Option<&str>, name: &str) -> DynamicObject {
        DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".into(),
                kind: kind.into(),
            }),
            metadata: kube::core::ObjectMeta {
                name: Some(name.into()),
                namespace: ns.map(Into::into),
                ..Default::default()
            },
            data: serde_json::json!({}),
        }
    }

    #[test]
    fn label_uses_group_for_non_core() {
        let core = KindEntry {
            api: ApiResource {
                group: String::new(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
            },
            namespaced: true,
        };
        assert_eq!(core.label(), "pods");
        let crd = KindEntry {
            api: ApiResource {
                group: "example.com".into(),
                version: "v1".into(),
                api_version: "example.com/v1".into(),
                kind: "Gizmo".into(),
                plural: "gizmos".into(),
            },
            namespaced: true,
        };
        assert_eq!(crd.label(), "gizmos.example.com");
    }

    #[test]
    fn row_reads_metadata() {
        let r = row(&obj("ConfigMap", Some("demo"), "cfg"));
        assert_eq!(r.namespace, "demo");
        assert_eq!(r.name, "cfg");
        // No creationTimestamp → age is the "unknown" marker, not a panic.
        assert!(!r.age.is_empty());
    }
}
