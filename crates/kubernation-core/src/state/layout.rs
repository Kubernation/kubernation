//! The layout engine — durable map slots that outlive their occupants.
//!
//! **The terrain belongs to the SLOT. Nodes occupy it and are replaced.** A node
//! swapped for a differently-named successor inherits the same coordinates, so
//! an immutable-infrastructure refresh does not move the world.
//!
//! PURE, and deliberately consumer-less at this phase: nothing renders from it
//! yet. The correctness claim is *given this sequence of cluster states, the
//! layout does not move except where declared* — a property of a function over a
//! sequence, testable with synthetic fixtures in CI with no cluster and no GL
//! context. That is why [`assign_layout`] takes plain data rather than
//! `k8s_openapi` types (the same reason `SubstrateReport::from_world` and
//! `cost_report` are shaped as they are).
//!
//! **What lives elsewhere:** compaction and persistence are A4's; extent and
//! capacity are A2's; cataclysm marking is A5's, which reads
//! [`Layout::changes_from`].

use std::collections::{BTreeMap, BTreeSet};

use crate::util::fnv1a64;

/// How a node's pool was determined. Mirrors `MetricSource` / `CostBasis`: the
/// reading is only as trustworthy as the rule that produced it, so the rule
/// travels with the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PoolSource {
    /// An operator-supplied label key.
    Override,
    /// A known provider key — the `&str` is the key that matched.
    Provider(&'static str),
    /// `node.kubernetes.io/instance-type`. Coarser: merges same-type pools that
    /// the provider considers distinct, but it is portable.
    InstanceType,
    /// Nothing matched. The node is in the single default pool — honestly, not
    /// silently folded into a real one.
    Default,
}

impl PoolSource {
    pub fn label(self) -> &'static str {
        match self {
            PoolSource::Override => "override",
            PoolSource::Provider(k) => k,
            PoolSource::InstanceType => "instance-type",
            PoolSource::Default => "default",
        }
    }
}

/// A node as the layout engine sees it: identity plus the two axes that decide
/// where it belongs. Deliberately NOT a `k8s_openapi::Node` — fixtures stay
/// cheap, and the engine cannot accidentally depend on cluster detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedNode {
    pub name: String,
    /// From `model::node_zone` — the `UNZONED` sentinel is an ordinary zone here,
    /// never a missing key.
    pub zone: String,
    pub pool: String,
    pub pool_source: PoolSource,
}

/// A position in the world. Terrain hangs off this, not off the node.
///
/// Ordered by (zone, pool, ordinal) so iteration is deterministic — the engine
/// must never depend on `HashMap` order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotKey {
    pub zone: String,
    pub pool: String,
    pub ordinal: u16,
}

/// Who is in a slot, and how their pool was inferred. `PoolSource` rides here so
/// it is reachable from the layout without a second lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occupancy {
    pub node: String,
    pub pool_source: PoolSource,
}

/// The assignment for one frame. A slot with `None` is a **ghost** — its
/// occupant departed and the position is retained, vacant.
///
/// Equality is over the slot map alone, which is what makes idempotence a
/// meaningful assertion. Per-frame *transitions* are deliberately not stored on
/// it: a change is a relation between two layouts, not a property of one, so it
/// is computed by [`Layout::changes_from`] rather than baked into the state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    slots: BTreeMap<SlotKey, Option<Occupancy>>,
}

/// One slot's transition between two layouts — A5's raw material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotChange {
    pub slot: SlotKey,
    /// `None` when the slot did not exist before, or was a ghost.
    pub from: Option<String>,
    /// `None` when the occupant departed (the slot is now a ghost).
    pub to: Option<String>,
}

impl Layout {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    /// Every slot, ghosts included, in deterministic order.
    pub fn slots(&self) -> impl Iterator<Item = (&SlotKey, Option<&Occupancy>)> {
        self.slots.iter().map(|(k, v)| (k, v.as_ref()))
    }
    /// Slots with an occupant.
    pub fn occupied(&self) -> impl Iterator<Item = (&SlotKey, &Occupancy)> {
        self.slots
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|o| (k, o)))
    }
    /// Vacant slots — positions whose occupant departed and which are held open
    /// rather than closed up. See [`assign_layout`] on why.
    pub fn ghosts(&self) -> impl Iterator<Item = &SlotKey> {
        self.slots
            .iter()
            .filter_map(|(k, v)| v.is_none().then_some(k))
    }
    /// Where this node currently sits, if anywhere.
    pub fn slot_of(&self, node: &str) -> Option<&SlotKey> {
        self.slots
            .iter()
            .find(|(_, v)| v.as_ref().is_some_and(|o| o.node == node))
            .map(|(k, _)| k)
    }

    /// Every slot whose occupant differs from `prior`'s — arrivals, departures
    /// and replacements. Pure comparison; A5 decides what is a cataclysm.
    pub fn changes_from(&self, prior: &Layout) -> Vec<SlotChange> {
        let keys: BTreeSet<&SlotKey> = self.slots.keys().chain(prior.slots.keys()).collect();
        keys.into_iter()
            .filter_map(|k| {
                let from = prior
                    .slots
                    .get(k)
                    .and_then(|o| o.as_ref())
                    .map(|o| o.node.clone());
                let to = self
                    .slots
                    .get(k)
                    .and_then(|o| o.as_ref())
                    .map(|o| o.node.clone());
                (from != to).then(|| SlotChange {
                    slot: k.clone(),
                    from,
                    to,
                })
            })
            .collect()
    }
}

/// PURE: given the previous layout and the nodes observed now, produce this
/// frame's layout.
///
/// ```text
/// 1. CARRY   a prior slot whose occupant is still observed keeps it, unchanged
/// 2. REUSE   an observed node with no slot claims the LOWEST-ordinal vacant
///            slot in its own (zone, pool)
/// 3. APPEND  otherwise, a new slot at the next ordinal in (zone, pool)
/// 4. GHOST   a prior slot whose occupant is gone is RETAINED as vacant
/// ```
///
/// **Sparseness is the decision, and it is deliberate.** Under surge the
/// replacement is Ready *before* its predecessor drains (verified on the churn
/// fleet: 100 → 115 → 100), so at step 2 there is no vacancy and it appends;
/// when the old node goes, its slot becomes a ghost. The pool is now sparse, and
/// compacting it would move existing slots — precisely what this engine exists
/// to prevent. Reclamation is a *declared* event, and it belongs to A4.
///
/// `prior` is just the previous frame's output. This function does not know
/// whether that came from memory or from disk, and must not.
pub fn assign_layout(prior: &Layout, observed: &[ObservedNode]) -> Layout {
    let mut slots: BTreeMap<SlotKey, Option<Occupancy>> = BTreeMap::new();
    // Every prior slot survives as at least a ghost — nothing is ever removed,
    // so ordinals never shift to close a gap.
    for k in prior.slots.keys() {
        slots.insert(k.clone(), None);
    }

    // 1. CARRY. A node keeps its slot only while it still belongs to that
    //    (zone, pool): a node that MOVES zone vacates the old slot and takes a
    //    new one, rather than dragging coordinates across a continent.
    let mut unplaced: Vec<&ObservedNode> = Vec::new();
    for n in observed {
        match prior.slot_of(&n.name) {
            Some(k) if k.zone == n.zone && k.pool == n.pool => {
                slots.insert(k.clone(), Some(occupancy(n)));
            }
            _ => unplaced.push(n),
        }
    }

    // Deterministic order for everything that follows. Two nodes may contend for
    // one vacancy in the same frame; the winner must not depend on iteration
    // order, which `HashMap` does not guarantee. Stable hash first, then name as
    // a collision guard — matching `city_home`'s tie-break.
    unplaced.sort_by_key(|n| (fnv1a64(&n.name), n.name.clone()));

    for n in unplaced {
        // 2. REUSE the lowest-ordinal vacancy in this node's own (zone, pool).
        let vacancy = slots
            .iter()
            .find(|(k, v)| v.is_none() && k.zone == n.zone && k.pool == n.pool)
            .map(|(k, _)| k.clone());
        let key = match vacancy {
            Some(k) => k,
            // 3. APPEND at the next ordinal.
            None => SlotKey {
                zone: n.zone.clone(),
                pool: n.pool.clone(),
                ordinal: next_ordinal(&slots, &n.zone, &n.pool),
            },
        };
        slots.insert(key, Some(occupancy(n)));
    }

    Layout { slots }
}

fn occupancy(n: &ObservedNode) -> Occupancy {
    Occupancy {
        node: n.name.clone(),
        pool_source: n.pool_source,
    }
}

/// The next free ordinal in a (zone, pool).
///
/// `map_or(0, m + 1)`, NOT `max().unwrap_or(0) + 1`: the latter starts an empty
/// pool at ordinal 1 and leaves 0 permanently unused, and worse, a
/// `max().unwrap_or(0)` on an empty set would hand back 0 — an ordinal a real
/// slot may already hold. Empty means *no ordinals yet*, which is a different
/// statement from *the highest ordinal is zero*.
fn next_ordinal(slots: &BTreeMap<SlotKey, Option<Occupancy>>, zone: &str, pool: &str) -> u16 {
    slots
        .keys()
        .filter(|k| k.zone == zone && k.pool == pool)
        .map(|k| k.ordinal)
        .max()
        .map_or(0, |m| m.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, zone: &str, pool: &str) -> ObservedNode {
        ObservedNode {
            name: name.into(),
            zone: zone.into(),
            pool: pool.into(),
            pool_source: PoolSource::Default,
        }
    }

    fn fleet(names: &[&str], zone: &str, pool: &str) -> Vec<ObservedNode> {
        names.iter().map(|n| node(n, zone, pool)).collect()
    }

    /// Slot -> occupant name, for readable assertions.
    fn placed(l: &Layout) -> BTreeMap<(String, String, u16), String> {
        l.occupied()
            .map(|(k, o)| ((k.zone.clone(), k.pool.clone(), k.ordinal), o.node.clone()))
            .collect()
    }

    // --- THE HEADLINE ------------------------------------------------------

    /// A full-fleet rolling refresh WITH SURGE: zero occupied slots move.
    ///
    /// This is the phase's reason to exist. The surge ordering is what makes it
    /// hard — replacements are Ready before their predecessors drain, so there
    /// is never a vacancy to reuse and every replacement appends. A
    /// delete-then-create sequence would test a strictly easier problem.
    #[test]
    fn a_surging_full_fleet_refresh_moves_no_occupied_slot() {
        let old: Vec<&str> = (0..100)
            .map(|i| Box::leak(format!("old-{i:03}").into_boxed_str()) as &str)
            .collect();
        let new: Vec<&str> = (0..100)
            .map(|i| Box::leak(format!("new-{i:03}").into_boxed_str()) as &str)
            .collect();

        let l0 = assign_layout(&Layout::default(), &fleet(&old, "z-a", "sys"));
        assert_eq!(l0.occupied().count(), 100);
        let before = placed(&l0);

        // SURGE: both generations observed at once.
        let mut both = fleet(&old, "z-a", "sys");
        both.extend(fleet(&new, "z-a", "sys"));
        let l1 = assign_layout(&l0, &both);
        assert_eq!(l1.occupied().count(), 200, "both generations are placed");
        for (slot, who) in &before {
            assert_eq!(
                placed(&l1).get(slot),
                Some(who),
                "an occupied slot moved during the surge"
            );
        }

        // DRAIN: the old generation departs.
        let l2 = assign_layout(&l1, &fleet(&new, "z-a", "sys"));
        assert_eq!(l2.occupied().count(), 100);
        assert_eq!(
            l2.ghosts().count(),
            100,
            "the old slots are ghosts, not gone"
        );
        assert_eq!(l2.slots().count(), 200, "200 slots, 100 occupied");

        // THE CLAIM: every surviving occupant is exactly where it was.
        let after = placed(&l2);
        for (slot, who) in placed(&l1) {
            if who.starts_with("new-") {
                assert_eq!(after.get(&slot), Some(&who), "occupant {who} moved");
            }
        }
    }

    // --- Assignment --------------------------------------------------------

    #[test]
    fn scale_up_appends_and_moves_nothing() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        let before = placed(&l0);
        let l1 = assign_layout(&l0, &fleet(&["a", "b", "c"], "z", "p"));
        for (slot, who) in &before {
            assert_eq!(placed(&l1).get(slot), Some(who));
        }
        assert_eq!(l1.occupied().count(), 3);
    }

    #[test]
    fn scale_down_leaves_a_ghost_and_shifts_no_ordinal() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b", "c"], "z", "p"));
        let before = placed(&l0);
        let l1 = assign_layout(&l0, &fleet(&["a", "c"], "z", "p"));
        assert_eq!(l1.ghosts().count(), 1);
        for who in ["a", "c"] {
            let k = |l: &Layout| l.slot_of(who).cloned();
            assert_eq!(k(&l1), k(&l0), "{who} moved to close a gap");
        }
        assert_eq!(before.len(), 3);
    }

    #[test]
    fn a_returning_node_reclaims_its_own_slot_while_it_is_still_vacant() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        let home = l0.slot_of("b").cloned().expect("b has a slot");
        let l1 = assign_layout(&l0, &fleet(&["a"], "z", "p")); // b departs
        assert_eq!(l1.ghosts().count(), 1);
        let l2 = assign_layout(&l1, &fleet(&["a", "b"], "z", "p")); // b returns
        assert_eq!(l2.slot_of("b"), Some(&home), "b did not get its slot back");
        assert_eq!(l2.ghosts().count(), 0);
    }

    /// A node that changes zone must NOT drag its coordinates across a
    /// continent: it vacates the old slot and takes one in the new zone.
    #[test]
    fn a_node_changing_zone_vacates_its_old_slot() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a"], "z-a", "p"));
        let l1 = assign_layout(&l0, &fleet(&["a"], "z-b", "p"));
        let k = l1.slot_of("a").expect("a is placed");
        assert_eq!(k.zone, "z-b");
        assert_eq!(l1.ghosts().count(), 1, "the z-a slot is left behind");
        assert!(l1.ghosts().all(|g| g.zone == "z-a"));
    }

    /// The same applies to a pool change — (zone, pool) is the slot's identity.
    #[test]
    fn a_node_changing_pool_vacates_its_old_slot() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a"], "z", "old-pool"));
        let l1 = assign_layout(&l0, &fleet(&["a"], "z", "new-pool"));
        assert_eq!(l1.slot_of("a").map(|k| k.pool.as_str()), Some("new-pool"));
        assert_eq!(l1.ghosts().count(), 1);
    }

    // --- Determinism -------------------------------------------------------

    #[test]
    fn assignment_is_idempotent() {
        let obs = fleet(&["a", "b", "c"], "z", "p");
        let once = assign_layout(&Layout::default(), &obs);
        let twice = assign_layout(&once, &obs);
        assert_eq!(
            once, twice,
            "re-applying the same observation moved something"
        );
    }

    #[test]
    fn assignment_is_independent_of_input_order() {
        let mut a = fleet(&["alpha", "beta", "gamma", "delta"], "z", "p");
        let base = assign_layout(&Layout::default(), &a);
        for _ in 0..4 {
            a.rotate_left(1);
            assert_eq!(
                assign_layout(&Layout::default(), &a),
                base,
                "a shuffled observation produced a different layout"
            );
        }
        a.reverse();
        assert_eq!(assign_layout(&Layout::default(), &a), base);
    }

    /// Two nodes contending for ONE vacancy must resolve identically every run.
    #[test]
    fn contention_for_one_vacancy_resolves_stably() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        let l1 = assign_layout(&l0, &fleet(&["a"], "z", "p")); // one vacancy
        let mut winners = BTreeSet::new();
        for order in [["x", "y"], ["y", "x"]] {
            let mut obs = fleet(&["a"], "z", "p");
            obs.extend(fleet(&order, "z", "p"));
            let l2 = assign_layout(&l1, &obs);
            let vacant = l1.ghosts().next().expect("a vacancy").clone();
            winners.insert(
                l2.occupied()
                    .find(|(k, _)| **k == vacant)
                    .map(|(_, o)| o.node.clone())
                    .expect("the vacancy was filled"),
            );
        }
        assert_eq!(
            winners.len(),
            1,
            "the winner depended on input order: {winners:?}"
        );
    }

    // --- Boundaries --------------------------------------------------------

    #[test]
    fn empty_and_single_node_clusters() {
        let empty = assign_layout(&Layout::default(), &[]);
        assert!(empty.is_empty());
        // Idempotent on nothing, too.
        assert_eq!(assign_layout(&empty, &[]), empty);

        let one = assign_layout(&Layout::default(), &fleet(&["solo"], "z", "p"));
        assert_eq!(one.occupied().count(), 1);
        // The FIRST ordinal is 0, not 1 — an empty pool has no ordinals yet,
        // which is a different statement from "the highest ordinal is zero".
        assert_eq!(one.slot_of("solo").map(|k| k.ordinal), Some(0));
    }

    #[test]
    fn every_node_replaced_at_once_without_surge() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        // No overlap: the replacements find both slots vacant and REUSE them.
        let l1 = assign_layout(&l0, &fleet(&["x", "y"], "z", "p"));
        assert_eq!(l1.occupied().count(), 2);
        assert_eq!(
            l1.ghosts().count(),
            0,
            "vacancies were reused, not appended"
        );
        assert_eq!(l1.slots().count(), 2, "no new slots were needed");
    }

    /// The `UNZONED` sentinel is an ordinary zone, not a missing key.
    #[test]
    fn the_unzoned_sentinel_is_an_ordinary_zone() {
        let obs = vec![
            node("a", crate::state::model::UNZONED, "p"),
            node("b", "z-a", "p"),
        ];
        let l = assign_layout(&Layout::default(), &obs);
        assert_eq!(l.occupied().count(), 2);
        let za = l.slot_of("a").expect("a placed");
        assert_eq!(za.zone, crate::state::model::UNZONED);
        assert_eq!(za.ordinal, 0, "it gets its own ordinal space");
        assert_eq!(l.slot_of("b").map(|k| k.ordinal), Some(0));
    }

    /// THE HIERARCHY CLAIM: a pool spanning three zones is three groups, because
    /// zone is the failure domain. A zone outage takes exactly one of them.
    #[test]
    fn a_pool_spanning_three_zones_is_three_groups() {
        let obs = vec![
            node("a", "z-a", "sys"),
            node("b", "z-b", "sys"),
            node("c", "z-c", "sys"),
        ];
        let l = assign_layout(&Layout::default(), &obs);
        let zones: BTreeSet<&str> = l.occupied().map(|(k, _)| k.zone.as_str()).collect();
        assert_eq!(zones.len(), 3, "the pool did not split by zone");
        // Each group has its own ordinal space starting at 0.
        for (k, _) in l.occupied() {
            assert_eq!(k.ordinal, 0, "{} should start its own group", k.zone);
        }
    }

    // --- Changes (A5's raw material) ---------------------------------------

    #[test]
    fn changes_from_reports_arrivals_departures_and_replacements() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        let l1 = assign_layout(&l0, &fleet(&["a", "c"], "z", "p"));
        let ch = l1.changes_from(&l0);
        assert_eq!(ch.len(), 1, "only b's slot changed: {ch:?}");
        assert_eq!(ch[0].from.as_deref(), Some("b"));
        assert_eq!(ch[0].to.as_deref(), Some("c"), "c reused b's vacancy");

        // A layout compared with itself has no changes — which is what makes
        // "changed this frame" a relation rather than stored state.
        assert!(l1.changes_from(&l1).is_empty());
    }

    #[test]
    fn pool_source_is_reachable_from_the_layout() {
        let obs = vec![ObservedNode {
            name: "a".into(),
            zone: "z".into(),
            pool: "gke-1".into(),
            pool_source: PoolSource::Provider("cloud.google.com/gke-nodepool"),
        }];
        let l = assign_layout(&Layout::default(), &obs);
        let (_, occ) = l.occupied().next().expect("placed");
        assert_eq!(
            occ.pool_source,
            PoolSource::Provider("cloud.google.com/gke-nodepool")
        );
        assert_eq!(occ.pool_source.label(), "cloud.google.com/gke-nodepool");
    }
}
