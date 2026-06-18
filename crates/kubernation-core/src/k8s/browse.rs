//! The resource browser's data layer: discover the cluster's resource kinds,
//! and LIST any one of them on demand (fetch-not-watch, like `logs`/`metrics`).
//! Everything goes through `DynamicObject`, so a single path serves every kind
//! — built-in or CRD. Pure row/label helpers sit beside the async fetches and
//! are unit-tested without a cluster.

use kube::Client;
use kube::api::{Api, ListParams};
use kube::core::{ApiResource, DynamicObject, GroupVersionKind, TypeMeta};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::APIResource;

// Re-exported so frontends can name browsed objects without a direct `kube` dep.
pub use kube::core::DynamicObject as Object;

/// Max objects fetched per kind, so a busy kind can't flood the view. When the
/// server reports more, the `ListResult` is flagged `truncated`.
pub const LIST_LIMIT: u32 = 500;

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

/// The outcome of LISTing a kind: the objects and whether the server had more
/// than `LIST_LIMIT` (so the frontends can say "showing first N").
#[derive(Debug, Clone)]
pub struct ListResult {
    pub items: Vec<DynamicObject>,
    pub truncated: bool,
}

/// Discover every served resource kind (the preferred version of each), sorted
/// by label.
///
/// **Tolerant by design:** each API group/version is queried independently and a
/// failure for one is *skipped*, not fatal. A single broken aggregated
/// APIService (a down metrics-server, a webhook-backed API returning 503) is
/// extremely common, and `kube::discovery::Discovery::run` fails the *whole*
/// enumeration on the first such group — which would blank the entire browser.
/// Here the rest of the cluster's kinds still come through (this mirrors how
/// `kubectl api-resources` warns-but-continues).
pub async fn discover(client: &Client) -> Vec<KindEntry> {
    let mut out: Vec<KindEntry> = Vec::new();

    // Core group (/api). The first listed version is the server's preferred core
    // version (v1).
    if let Ok(versions) = client.list_core_api_versions().await
        && let Some(v) = versions.versions.first()
        && let Ok(list) = client.list_core_api_resources(v).await
    {
        collect(&mut out, &list.resources, "", v);
    }

    // Named groups (/apis). Each is queried on its own so one broken group can't
    // take down the rest.
    if let Ok(groups) = client.list_api_groups().await {
        for g in groups.groups {
            // Prefer the server's preferred version; fall back to the first.
            let gv = g
                .preferred_version
                .or_else(|| g.versions.into_iter().next());
            let Some(gv) = gv else { continue };
            if let Ok(list) = client.list_api_group_resources(&gv.group_version).await {
                collect(&mut out, &list.resources, &g.name, &gv.version);
            }
        }
    }

    out.sort_by_key(|k| k.label());
    out.dedup_by(|a, b| a.label() == b.label());
    out
}

/// Build `KindEntry` rows from one group/version's resource list, skipping
/// subresources (`name` carries a `/` — status/scale/…) and kinds the server
/// won't LIST (no `list` verb — e.g. tokenreviews, subjectaccessreviews,
/// bindings, componentstatuses), which would only fail when selected.
fn collect(out: &mut Vec<KindEntry>, resources: &[APIResource], group: &str, version: &str) {
    for ar in resources {
        if ar.name.contains('/') {
            continue; // subresource
        }
        if !ar.verbs.iter().any(|v| v == "list") {
            continue; // not listable
        }
        let api = ApiResource::from_gvk_with_plural(
            &GroupVersionKind {
                group: group.to_string(),
                version: version.to_string(),
                kind: ar.kind.clone(),
            },
            &ar.name,
        );
        out.push(KindEntry {
            api,
            namespaced: ar.namespaced,
        });
    }
}

/// LIST one kind across all namespaces (cluster-wide list endpoint; the table
/// shows the namespace column). Capped at `LIST_LIMIT`.
///
/// **Load-bearing:** the apiserver does NOT echo `apiVersion`/`kind` on the
/// individual items inside a collection response (only on the List envelope), so
/// each returned `DynamicObject` has `types == None`. We stamp the picked kind's
/// `TypeMeta` back onto every item — without it, `inspect::dynamic_yaml` cannot
/// recognise a Secret and would render its `data` values in full, silently
/// breaking the "never surface Secret contents" posture. (It also fixes the
/// inspector title, which reads `obj.types`.)
pub async fn list_kind(client: &Client, entry: &KindEntry) -> Result<ListResult, String> {
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &entry.api);
    let list = api
        .list(&ListParams::default().limit(LIST_LIMIT))
        .await
        .map_err(|e| e.to_string())?;
    let truncated = list
        .metadata
        .continue_
        .as_deref()
        .is_some_and(|c| !c.is_empty());
    let items = list
        .items
        .into_iter()
        .map(|mut o| {
            stamp_types(&mut o, &entry.api);
            o
        })
        .collect();
    Ok(ListResult { items, truncated })
}

/// Stamp a list item's `TypeMeta` from the kind it was LISTed as (the apiserver
/// omits it on collection items). This is what lets the inspector both title and
/// — critically — redact a Secret. See `list_kind`.
fn stamp_types(obj: &mut DynamicObject, api: &ApiResource) {
    obj.types = Some(TypeMeta {
        api_version: api.api_version.clone(),
        kind: api.kind.clone(),
    });
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

    fn apiresource(name: &str, kind: &str, listable: bool) -> APIResource {
        APIResource {
            name: name.into(),
            kind: kind.into(),
            namespaced: true,
            singular_name: String::new(),
            verbs: if listable {
                vec!["get".into(), "list".into(), "watch".into()]
            } else {
                vec!["get".into(), "create".into()]
            },
            ..Default::default()
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

    #[test]
    fn collect_skips_subresources_and_non_listable() {
        let resources = vec![
            apiresource("pods", "Pod", true),
            apiresource("pods/status", "Pod", true), // subresource → skipped
            apiresource("tokenreviews", "TokenReview", false), // no list verb → skipped
            apiresource("configmaps", "ConfigMap", true),
        ];
        let mut out = Vec::new();
        collect(&mut out, &resources, "", "v1");
        let labels: Vec<String> = out.iter().map(|k| k.label()).collect();
        assert_eq!(labels, vec!["pods".to_string(), "configmaps".to_string()]);
        // The kind/version are taken from the group context, not the item.
        assert_eq!(out[0].api.kind, "Pod");
        assert_eq!(out[0].api.api_version, "v1");
    }

    #[test]
    fn stamp_types_drives_secret_redaction() {
        use crate::state::inspect::dynamic_yaml;

        // A list item exactly as the apiserver returns it inside a collection:
        // NO apiVersion/kind, so `types` deserializes to None.
        let mut o = DynamicObject {
            types: None,
            metadata: kube::core::ObjectMeta {
                name: Some("creds".into()),
                namespace: Some("demo".into()),
                ..Default::default()
            },
            data: serde_json::json!({ "data": { "password": "c2VjcmV0" } }),
        };
        assert!(o.types.is_none(), "raw list item carries no TypeMeta");

        // Stamp from the picked Secret kind, exactly as `list_kind` does.
        let secret_api = ApiResource::from_gvk_with_plural(
            &GroupVersionKind {
                group: String::new(),
                version: "v1".into(),
                kind: "Secret".into(),
            },
            "secrets",
        );
        stamp_types(&mut o, &secret_api);
        assert_eq!(o.types.as_ref().unwrap().kind, "Secret");
        assert_eq!(o.types.as_ref().unwrap().api_version, "v1");

        // With the stamp in place, the inspector redacts the value (the leak the
        // adversarial review found is closed).
        let y = dynamic_yaml(&o);
        assert!(!y.contains("c2VjcmV0"), "secret value redacted: {y}");
        assert!(y.contains("bytes)"), "placeholder shown: {y}");
    }
}
