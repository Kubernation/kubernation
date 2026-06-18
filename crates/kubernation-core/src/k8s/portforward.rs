//! On-demand **port-forward**: a local TCP listener that tunnels each accepted
//! connection to a pod port through the apiserver's `portforward` subresource
//! (the same mechanism as `kubectl port-forward`).
//!
//! This is an *active* capability, not a cluster mutation — it writes nothing
//! to the cluster, so it lives here in the data layer rather than in the one
//! write file (`k8s::actions`). It is still **deliberate and gated**, in the
//! same spirit: the apiserver enforces `create pods/portforward` RBAC, the
//! frontends pre-check it with [`can_forward`] (a read-only probe) before
//! offering the control, and a forward only runs after an explicit action and
//! is visible + individually stoppable.
//!
//! Fetch-not-watch like [`super::logs`] and [`super::browse`]: there is no
//! reflector lifecycle. [`start`] binds `127.0.0.1:<os-assigned>` and returns a
//! [`Forward`] handle whose `Drop` aborts the accept loop *and* every in-flight
//! tunnel (the per-connection tasks live in a `JoinSet` the loop owns, and a
//! dropped `JoinSet` aborts its tasks) — so "stop" means stop.

use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use k8s_openapi::api::core::v1::{Pod, Service};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::Client;
use kube::api::{Api, ListParams, PostParams};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{debug, warn};

/// A live local→pod port-forward. Holds the accept-loop task; dropping it (or
/// calling [`Forward::stop`]) aborts the loop and, with it, the `JoinSet` of
/// in-flight tunnels — tearing the forward down cleanly.
pub struct Forward {
    pub namespace: String,
    pub pod: String,
    /// The pod-side container port being forwarded.
    pub pod_port: u16,
    /// The OS-assigned local port now listening on `127.0.0.1`.
    pub local_port: u16,
    task: JoinHandle<()>,
}

impl Forward {
    /// Tear the forward down (same as dropping it).
    pub fn stop(self) {
        // Drop runs the abort.
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Start forwarding `127.0.0.1:<os-assigned>` → `pod:pod_port`. Binds the
/// listener up front (so the local port is known immediately and a bind error
/// surfaces synchronously), then spawns the accept loop. Per-connection
/// failures (a wrong port, a refused pod socket) are logged, not fatal — the
/// listener stays up, mirroring `kubectl port-forward`.
pub async fn start(
    client: Client,
    namespace: &str,
    pod: &str,
    pod_port: u16,
) -> Result<Forward, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not bind a local port: {e}"))?;
    let local_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let ns = namespace.to_string();
    let pod_name = pod.to_string();
    let task = {
        let ns = ns.clone();
        let pod_name = pod_name.clone();
        tokio::spawn(async move {
            // The in-flight tunnels. Owning them here means aborting this task
            // (Forward::drop) drops the set, which aborts them all.
            let mut conns: JoinSet<()> = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => match accepted {
                        Ok((socket, _)) => {
                            conns.spawn(forward_conn(
                                client.clone(),
                                ns.clone(),
                                pod_name.clone(),
                                pod_port,
                                socket,
                            ));
                        }
                        Err(e) => {
                            warn!("port-forward listener {ns}/{pod_name}: {e}");
                            break;
                        }
                    },
                    // Reap finished tunnels so the set can't grow without bound.
                    Some(_) = conns.join_next(), if !conns.is_empty() => {}
                }
            }
        })
    };

    Ok(Forward {
        namespace: ns,
        pod: pod_name,
        pod_port,
        local_port,
        task,
    })
}

/// Pump bytes both directions between one accepted local socket and a fresh
/// per-connection tunnel to the pod port. A new `portforward` upgrade per
/// connection mirrors how `kubectl` multiplexes.
async fn forward_conn(client: Client, ns: String, pod: String, port: u16, mut socket: TcpStream) {
    let api: Api<Pod> = Api::namespaced(client, &ns);
    let mut pf = match api.portforward(&pod, &[port]).await {
        Ok(pf) => pf,
        Err(e) => {
            warn!("port-forward {ns}/{pod}:{port} could not open: {e}");
            return;
        }
    };
    let Some(mut upstream) = pf.take_stream(port) else {
        warn!("port-forward {ns}/{pod}:{port}: no stream for port");
        return;
    };
    // Pump until either side closes. We deliberately do NOT await
    // `pf.take_error(port)` afterward: that future isn't guaranteed to resolve
    // once the stream is done, and awaiting it could leave this per-connection
    // task parked (and unreaped in the accept loop's JoinSet) until the whole
    // Forward is dropped. The copy result + the listener-level logging cover
    // the failure modes a user can act on.
    if let Err(e) = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await {
        debug!("port-forward {ns}/{pod}:{port} closed: {e}");
    }
}

/// Best-effort default forward target for a pod, so the caller need not guess
/// (mirrors [`super::logs::first_container`]):
///
/// 1. the pod's first declared `containerPort`, else
/// 2. a numeric `targetPort` of a Service that selects this pod (many real
///    workloads — and all the dev samples — declare the port only on the
///    Service, not the pod), falling back to the Service's own `port` when the
///    `targetPort` is a *named* one we can't resolve.
///
/// `None` if nothing resolves (then the frontend disables the control; a manual
/// port-entry field is the natural future upgrade). The Service LIST failing —
/// e.g. no `list services` permission — also degrades to `None`.
pub async fn default_port(client: Client, namespace: &str, pod: &str) -> Option<u16> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let p = pods.get(pod).await.ok()?;

    // 1. A declared containerPort wins (no Service LIST needed — so a pod that
    //    declares its port still resolves even without `list services`).
    if let Some(n) = container_port(&p) {
        return Some(n);
    }

    // 2. Fall back to a Service that selects this pod.
    let svcs: Api<Service> = Api::namespaced(client, namespace);
    let list = svcs.list(&ListParams::default()).await.ok()?;
    service_port(&p, &list.items)
}

/// First usable declared `containerPort` of a pod. Pure (unit-tested).
fn container_port(pod: &Pod) -> Option<u16> {
    pod.spec
        .as_ref()?
        .containers
        .iter()
        .filter_map(|c| c.ports.as_ref())
        .flatten()
        .map(|cp| cp.container_port)
        .find(|&n| n > 0 && n <= u16::MAX as i32)
        .map(|n| n as u16)
}

/// A numeric `targetPort` (else the front `port`) of the first Service whose
/// selector matches this pod's labels. Pure (unit-tested). A named, unresolved
/// `targetPort` falls back to the Service's own `port`; selector-less Services
/// (headless / ExternalName) and empty selectors never match.
fn service_port(pod: &Pod, services: &[Service]) -> Option<u16> {
    let labels = pod.metadata.labels.clone().unwrap_or_default();
    for s in services {
        let Some(spec) = s.spec.as_ref() else {
            continue;
        };
        let Some(sel) = spec.selector.as_ref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if !sel.iter().all(|(k, v)| labels.get(k) == Some(v)) {
            continue;
        }
        for port in spec.ports.iter().flatten() {
            let n = match &port.target_port {
                Some(IntOrString::Int(n)) => *n,
                _ => port.port,
            };
            if n > 0 && n <= u16::MAX as i32 {
                return Some(n as u16);
            }
        }
    }
    None
}

/// Can the current user `create pods/portforward` in `namespace`? A read-only
/// RBAC probe (a `SelfSubjectAccessReview`) the frontends use to enable/disable
/// the forward control — the same shape as `actions::can_evict_pod`. Errs to
/// the UI as a display string; the frontends treat any error as "not allowed".
pub async fn can_forward(client: Client, namespace: &str) -> Result<bool, String> {
    let api: Api<SelfSubjectAccessReview> = Api::all(client);
    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(ResourceAttributes {
                verb: Some("create".into()),
                resource: Some("pods".into()),
                subresource: Some("portforward".into()),
                namespace: Some(namespace.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let res = api
        .create(&PostParams::default(), &review)
        .await
        .map_err(|e| e.to_string())?;
    Ok(res.status.map(|s| s.allowed).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{Container, ContainerPort, PodSpec, ServicePort, ServiceSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn pod(container_port: Option<i32>, labels: &[(&str, &str)]) -> Pod {
        Pod {
            metadata: ObjectMeta {
                labels: Some(
                    labels
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "c".into(),
                    ports: container_port.map(|p| {
                        vec![ContainerPort {
                            container_port: p,
                            ..Default::default()
                        }]
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn svc(selector: &[(&str, &str)], port: i32, target: Option<IntOrString>) -> Service {
        Service {
            spec: Some(ServiceSpec {
                selector: Some(
                    selector
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                ports: Some(vec![ServicePort {
                    port,
                    target_port: target,
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn container_port_prefers_a_declared_port() {
        assert_eq!(container_port(&pod(Some(8080), &[])), Some(8080));
        assert_eq!(container_port(&pod(None, &[])), None);
        // A zero/invalid port is skipped.
        assert_eq!(container_port(&pod(Some(0), &[])), None);
    }

    #[test]
    fn service_port_resolves_a_selecting_service() {
        let pod = pod(None, &[("app", "web")]);
        // Numeric targetPort wins.
        let svcs = vec![svc(&[("app", "web")], 80, Some(IntOrString::Int(8080)))];
        assert_eq!(service_port(&pod, &svcs), Some(8080));
        // A non-matching selector contributes nothing.
        let other = vec![svc(&[("app", "db")], 5432, Some(IntOrString::Int(5432)))];
        assert_eq!(service_port(&pod, &other), None);
    }

    #[test]
    fn service_port_handles_named_and_absent_target_ports() {
        let pod = pod(None, &[("app", "web")]);
        // A named targetPort we can't resolve → fall back to the front port.
        let named = vec![svc(
            &[("app", "web")],
            80,
            Some(IntOrString::String("http".into())),
        )];
        assert_eq!(service_port(&pod, &named), Some(80));
        // No targetPort at all → the front port.
        let bare = vec![svc(&[("app", "web")], 8443, None)];
        assert_eq!(service_port(&pod, &bare), Some(8443));
    }

    #[test]
    fn service_port_skips_selectorless_services() {
        // A headless / ExternalName Service (empty selector) never matches —
        // mirrors the attention-queue "idle harbor" skip.
        let pod = pod(None, &[("app", "web")]);
        let headless = vec![svc(&[], 80, Some(IntOrString::Int(80)))];
        assert_eq!(service_port(&pod, &headless), None);
    }
}
