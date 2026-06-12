use std::path::Path;

use color_eyre::eyre::{Result, eyre};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{Api, ListParams};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::core::ApiResource;
use kube::{Client, Config};

/// Cluster identity shown in the status bar; the "politics of the world".
#[derive(Debug, Clone)]
pub struct ClusterMeta {
    pub context: String,
    pub server: String,
    pub platform: Platform,
    /// Every context in the kubeconfig, for the context picker.
    pub all_contexts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Eks,
    Gke,
    Aks,
    OpenShift,
    Kind,
    K3s,
    Minikube,
    DockerDesktop,
    Unknown,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Eks => "EKS",
            Platform::Gke => "GKE",
            Platform::Aks => "AKS",
            Platform::OpenShift => "OpenShift",
            Platform::Kind => "kind",
            Platform::K3s => "k3s",
            Platform::Minikube => "minikube",
            Platform::DockerDesktop => "docker",
            Platform::Unknown => "k8s",
        }
    }

    /// Heuristic from kubeconfig alone; refined later from node providerIDs.
    pub fn detect(context: &str, server: &str) -> Self {
        let c = context.to_ascii_lowercase();
        let s = server.to_ascii_lowercase();
        if c.starts_with("kind-") {
            Platform::Kind
        } else if c.starts_with("k3d-") || c.contains("k3s") {
            Platform::K3s
        } else if c == "minikube" {
            Platform::Minikube
        } else if c == "docker-desktop" {
            Platform::DockerDesktop
        } else if s.contains("eks.amazonaws.com") || c.starts_with("arn:aws:eks") {
            Platform::Eks
        } else if c.starts_with("gke_") {
            Platform::Gke
        } else if s.contains("azmk8s.io") {
            Platform::Aks
        } else if s.contains("openshift") || c.contains("openshift") {
            Platform::OpenShift
        } else {
            Platform::Unknown
        }
    }

    /// `node.spec.providerID` prefix is the most reliable signal we observe.
    pub fn from_provider_id(pid: &str) -> Option<Self> {
        let scheme = pid.split("://").next()?;
        match scheme {
            "aws" => Some(Platform::Eks),
            "gce" => Some(Platform::Gke),
            "azure" => Some(Platform::Aks),
            "kind" => Some(Platform::Kind),
            "k3s" => Some(Platform::K3s),
            _ => None,
        }
    }
}

pub struct Cluster {
    pub client: Client,
    pub meta: ClusterMeta,
}

/// Build a client for `context` (or the kubeconfig's current-context) from
/// the standard kubeconfig locations or an explicit path.
pub async fn connect(kubeconfig: Option<&Path>, context: Option<&str>) -> Result<Cluster> {
    let kc = match kubeconfig {
        Some(p) => Kubeconfig::read_from(p)?,
        None => Kubeconfig::read()?,
    };
    let all_contexts: Vec<String> = kc.contexts.iter().map(|c| c.name.clone()).collect();
    let ctx = context
        .map(String::from)
        .or_else(|| kc.current_context.clone())
        .ok_or_else(|| eyre!("kubeconfig has no current-context; pass --context"))?;
    if !all_contexts.iter().any(|c| c == &ctx) {
        return Err(eyre!(
            "context {ctx:?} not found in kubeconfig (have: {})",
            all_contexts.join(", ")
        ));
    }
    let opts = KubeConfigOptions {
        context: Some(ctx.clone()),
        ..Default::default()
    };
    let config = Config::from_custom_kubeconfig(kc, &opts).await?;
    let server = config.cluster_url.to_string();
    let client = Client::try_from(config)?;
    let platform = Platform::detect(&ctx, &server);
    Ok(Cluster {
        client,
        meta: ClusterMeta {
            context: ctx,
            server,
            platform,
            all_contexts,
        },
    })
}

/// Resolve requested CRD projections ("plural.group" names, e.g.
/// "gizmos.example.com") into dynamic ApiResources. Resolution happens once
/// at connect; CRDs that don't exist on this cluster are skipped with a log
/// line, so a pair can project asymmetrically.
pub async fn resolve_projections(client: &Client, wanted: &[String]) -> Vec<(String, ApiResource)> {
    if wanted.is_empty() {
        return Vec::new();
    }
    let api = Api::<CustomResourceDefinition>::all(client.clone());
    let crds = match api.list(&ListParams::default()).await {
        Ok(list) => list.items,
        Err(err) => {
            tracing::warn!(%err, "could not list CRDs; no projections active");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for name in wanted {
        let Some(crd) = crds
            .iter()
            .find(|c| c.metadata.name.as_deref() == Some(name))
        else {
            tracing::warn!(%name, "projection CRD not found on this cluster; skipping");
            continue;
        };
        let spec = &crd.spec;
        let Some(version) = spec
            .versions
            .iter()
            .find(|v| v.served && v.storage)
            .or_else(|| spec.versions.iter().find(|v| v.served))
        else {
            tracing::warn!(%name, "projection CRD has no served version; skipping");
            continue;
        };
        let kind_label = spec
            .names
            .singular
            .clone()
            .unwrap_or_else(|| spec.names.kind.to_lowercase());
        out.push((
            kind_label,
            ApiResource {
                group: spec.group.clone(),
                version: version.name.clone(),
                api_version: format!("{}/{}", spec.group, version.name),
                kind: spec.names.kind.clone(),
                plural: spec.names.plural.clone(),
            },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::Platform;

    #[test]
    fn platform_heuristics() {
        assert_eq!(
            Platform::detect("kind-dev", "https://127.0.0.1:6443"),
            Platform::Kind
        );
        assert_eq!(
            Platform::detect(
                "arn:aws:eks:us-east-1:1:cluster/x",
                "https://x.eks.amazonaws.com"
            ),
            Platform::Eks
        );
        assert_eq!(
            Platform::detect("gke_proj_zone_name", "https://1.2.3.4"),
            Platform::Gke
        );
        assert_eq!(
            Platform::detect("prod", "https://x.azmk8s.io:443"),
            Platform::Aks
        );
        assert_eq!(
            Platform::detect("prod", "https://10.0.0.1:6443"),
            Platform::Unknown
        );
    }

    #[test]
    fn provider_id_refinement() {
        assert_eq!(
            Platform::from_provider_id("kind://docker/k8sciv/k8sciv-worker"),
            Some(Platform::Kind)
        );
        assert_eq!(
            Platform::from_provider_id("aws:///us-east-1a/i-abc"),
            Some(Platform::Eks)
        );
        assert_eq!(Platform::from_provider_id("weird"), None);
    }
}
