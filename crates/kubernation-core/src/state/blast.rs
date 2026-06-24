//! Blast radius — the dependency fan-out of a troubled subject, derived purely
//! from the observed topology (no traffic data needed). The SRE practice: given
//! the solution topology, determine which components an anomaly affects so you
//! can isolate the impact (see the AIM observability/AIOps notes on
//! topology-driven blast-radius isolation).
//!
//! KuberNation already *owns* that topology — workloads, the Services that
//! select them, the Ingresses that route to those Services, and which node each
//! pod runs on — so the fan-out is a pure graph walk over `ObservedWorld`:
//!
//!   * a **node** cascades node → hosted workloads → their Services → Ingresses
//!     ("if this province falls, these cities lose citizens, and their harbors
//!     and gates go dark"),
//!   * a **workload** walks workload → its Services → Ingresses (the external
//!     routes that lose their backing).
//!
//! We deliberately do NOT invent app-level "who calls whom" edges — those
//! aren't derivable from core API objects without a service mesh / eBPF, and a
//! wrong dependency is worse than a missing one. So a workload with no Service
//! has an (honestly) empty blast radius.

use crate::state::model::{OwnerIndex, WorkloadRef, build_exposure};
use crate::state::observed::ObservedWorld;
use crate::state::world::CoastKind;

/// The thing whose impact we trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Workload(WorkloadRef),
    Node(String),
}

/// A resource reached from the subject through the topology. `via` records the
/// affected workload the route hangs off, so the renderer highlights only that
/// workload's harbor/gate mark — a Service fronting several workloads must not
/// light up the marks of healthy ones on other nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Affected {
    /// A workload (city) hosted on a failing node.
    Workload(WorkloadRef),
    /// A Service (harbor) fronting an affected workload.
    Service {
        namespace: String,
        name: String,
        via: WorkloadRef,
    },
    /// An Ingress (gate) routing to an affected Service.
    Ingress {
        namespace: String,
        name: String,
        via: WorkloadRef,
    },
}

/// One reached resource and its hop distance from the subject (1 = directly
/// affected; larger = further down the cascade).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastItem {
    pub item: Affected,
    pub hop: u8,
}

/// The computed fan-out, sorted nearest-first (hop, then identity). Deduped so
/// a resource reached two ways keeps its *smallest* hop.
#[derive(Debug, Clone)]
pub struct BlastRadius {
    pub subject: Subject,
    pub items: Vec<BlastItem>,
}

impl BlastRadius {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many distinct resources are affected.
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// PURE: the distinct workloads with ≥1 pod stationed on `node` (pod `node_name` →
/// `OwnerIndex`). Shared by `blast_radius`'s node cascade and the Oracle's
/// node-scope CONSULT NEXT seeding, so the two can never disagree on "who lives
/// here" (the `draw::affected_cell` DRY precedent).
pub(crate) fn workloads_on_node(world: &ObservedWorld, node: &str) -> Vec<WorkloadRef> {
    let idx = OwnerIndex::build(world);
    let mut owners: Vec<WorkloadRef> = Vec::new();
    for p in world.pods.state() {
        if p.spec.as_ref().and_then(|s| s.node_name.as_deref()) == Some(node)
            && let Some(wr) = idx.workload_of(&p)
            && !owners.contains(&wr)
        {
            owners.push(wr);
        }
    }
    owners
}

/// Compute the blast radius of `subject` over the observed topology.
pub fn blast_radius(world: &ObservedWorld, subject: &Subject) -> BlastRadius {
    let exposure = build_exposure(world);
    let mut acc: Vec<BlastItem> = Vec::new();

    match subject {
        Subject::Workload(wr) => {
            // The workload itself is the source; trace only its routes.
            add_routes(wr, 0, &exposure, &mut acc);
        }
        Subject::Node(node) => {
            // Hosted workloads are directly at risk; then their routes cascade.
            for wr in &workloads_on_node(world, node) {
                add(&mut acc, Affected::Workload(wr.clone()), 1);
                add_routes(wr, 1, &exposure, &mut acc);
            }
        }
    }

    acc.sort_by(|a, b| {
        a.hop
            .cmp(&b.hop)
            .then_with(|| affected_key(&a.item).cmp(&affected_key(&b.item)))
    });
    BlastRadius {
        subject: subject.clone(),
        items: acc,
    }
}

/// Add a workload's Services (at `base+1`) and Ingresses (at `base+2`) — the
/// Ingress depends on the Service, so it's one hop further.
fn add_routes(
    wr: &WorkloadRef,
    base: u8,
    exposure: &[crate::state::world::ExposureEntry],
    acc: &mut Vec<BlastItem>,
) {
    for e in exposure.iter().filter(|e| &e.workload == wr) {
        let ns = wr.namespace.clone();
        match e.kind {
            CoastKind::Harbor => add(
                acc,
                Affected::Service {
                    namespace: ns,
                    name: e.name.clone(),
                    via: wr.clone(),
                },
                base + 1,
            ),
            CoastKind::Gate => add(
                acc,
                Affected::Ingress {
                    namespace: ns,
                    name: e.name.clone(),
                    via: wr.clone(),
                },
                base + 2,
            ),
        }
    }
}

/// Insert `item` at `hop`, or lower an existing entry's hop if this path is
/// shorter (the small-set dedup; blast radii are a handful of items).
fn add(acc: &mut Vec<BlastItem>, item: Affected, hop: u8) {
    if let Some(existing) = acc.iter_mut().find(|b| b.item == item) {
        if hop < existing.hop {
            existing.hop = hop;
        }
    } else {
        acc.push(BlastItem { item, hop });
    }
}

/// A stable sort key for an `Affected` (the hop tiebreaker); `via` keeps the
/// per-workload routes ordered deterministically.
fn affected_key(a: &Affected) -> (u8, String, String, String) {
    match a {
        Affected::Workload(wr) => (0, wr.namespace.clone(), wr.name.clone(), String::new()),
        Affected::Service {
            namespace,
            name,
            via,
        } => (1, namespace.clone(), name.clone(), via.name.clone()),
        Affected::Ingress {
            namespace,
            name,
            via,
        } => (2, namespace.clone(), name.clone(), via.name.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
    use crate::state::model::WorkloadKind;

    fn web_ref() -> WorkloadRef {
        WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: "web".into(),
        }
    }

    /// A workload with a Service + Ingress reports both as its blast radius,
    /// the Ingress one hop further than the Service.
    #[test]
    fn workload_radius_walks_service_then_ingress() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "web", 1, 1));
        // web's pods carry app=web (the fixture's pod-template labels).
        s.service(fx::service("demo", "web", &[("app", "web")]));
        s.ingress(fx::ingress("demo", "web-ing", "web.example", "web"));

        let r = blast_radius(&world, &Subject::Workload(web_ref()));
        let svc = r
            .items
            .iter()
            .find(|b| matches!(&b.item, Affected::Service { name, .. } if name == "web"))
            .expect("the Service is in the radius");
        let ing = r
            .items
            .iter()
            .find(|b| matches!(&b.item, Affected::Ingress { name, .. } if name == "web-ing"))
            .expect("the Ingress is in the radius");
        assert_eq!(svc.hop, 1, "service is directly affected");
        assert_eq!(ing.hop, 2, "ingress is one hop past the service");
    }

    /// A workload with no Service has an (honestly) empty blast radius — we
    /// don't fabricate consumer edges.
    #[test]
    fn workload_with_no_routes_is_empty() {
        let (world, mut s) = fx::world();
        s.deployment(fx::deployment("demo", "lonely", 1, 1));
        let r = blast_radius(
            &world,
            &Subject::Workload(WorkloadRef {
                kind: WorkloadKind::Deployment,
                namespace: "demo".into(),
                name: "lonely".into(),
            }),
        );
        assert!(r.is_empty());
    }

    /// A node cascades to the workloads hosted on it and then their routes:
    /// node → web (hop 1) → web's Service (hop 2) → its Ingress (hop 3).
    #[test]
    fn workloads_on_node_lists_only_the_hosted_workloads() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.node(fx::node("n2", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        s.deployment(fx::deployment("demo", "api", 1, 1));
        s.replicaset(fx::replicaset("demo", "api-rs", "api"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "api-rs-1", Some("n2")),
            "ReplicaSet",
            "api-rs",
        ));
        let on1 = workloads_on_node(&world, "n1");
        assert_eq!(on1.len(), 1);
        assert_eq!(on1[0].name, "web");
        // The api workload lives on n2 → not listed for n1.
        assert!(!on1.iter().any(|w| w.name == "api"));
        assert!(workloads_on_node(&world, "nope").is_empty());
    }

    #[test]
    fn node_radius_cascades_to_hosted_workloads_and_their_routes() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        s.service(fx::service("demo", "web", &[("app", "web")]));
        s.ingress(fx::ingress("demo", "web-ing", "web.example", "web"));

        let r = blast_radius(&world, &Subject::Node("n1".into()));
        let wl = r
            .items
            .iter()
            .find(|b| matches!(&b.item, Affected::Workload(w) if w.name == "web"))
            .expect("hosted workload");
        assert_eq!(wl.hop, 1);
        // The Service/Ingress carry `via` = the affected workload, so the
        // renderer highlights only that workload's harbor/gate (not a sibling's).
        assert!(r.items.iter().any(|b| matches!(
            &b.item,
            Affected::Service { name, via, .. } if name == "web" && via.name == "web"
        ) && b.hop == 2));
        assert!(r.items.iter().any(|b| matches!(
            &b.item,
            Affected::Ingress { name, via, .. } if name == "web-ing" && via.name == "web"
        ) && b.hop == 3));
    }

    /// An empty node (no pods) has an empty radius.
    #[test]
    fn empty_node_is_empty() {
        let (world, mut s) = fx::world();
        s.node(fx::node("idle", Some("z-a")));
        let r = blast_radius(&world, &Subject::Node("idle".into()));
        assert!(r.is_empty());
    }
}
