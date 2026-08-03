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
use std::time::SystemTime;

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
///
/// **`ordinal` is unique within the ZONE, not within the (zone, pool).** The
/// pool is slot *identity* — it decides which vacancies a node may reclaim or
/// reuse — but it is deliberately NOT a private numbering space, because a
/// consumer that renders position from the ordinal alone would then draw the
/// Nth node of every pool on the same ground. That is not hypothetical: A2's
/// `province_y` did exactly this, and on a four-pool fleet it hid 42 of 100
/// nodes underneath each other. Per-pool numbering makes the collision
/// *representable* and pushes the burden onto every consumer to remember the
/// pool; zone-wide numbering makes it unrepresentable.
///
/// The cost is that pools interleave positionally as they grow rather than
/// occupying contiguous bands. Grouping pools into visual regions (the plan's
/// `region ← pool ∩ zone`) is a later phase's job and wants durable band
/// ordinals of its own; it must not be smuggled in as a numbering convention.
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

/// One position's state. `occupant` is `None` for a **ghost**.
///
/// `last_occupant` is what makes reclaim possible: a vacated slot remembers who
/// held it, so a node returning after a drain gets ITS OWN ground back rather
/// than whichever vacancy happens to sort lowest. Without it the guidance's
/// "a node returning after departure claims its own slot back" is unimplementable
/// from stored state — two nodes that drain and return together simply swap
/// coordinates, which is the move this engine exists to prevent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotState {
    pub occupant: Option<Occupancy>,
    pub last_occupant: Option<String>,
    /// When this slot last became a ghost. `Some` only while vacant — a slot
    /// that is re-occupied clears it, so the timestamp always answers "how long
    /// has THIS vacancy stood", never "when was it ever empty".
    ///
    /// **Nothing reads it yet.** A4 deliberately has no automatic reap: ghosts
    /// hold at the refresh batch size rather than accumulating, so reaping by
    /// age would be the map quietly deciding those nodes are not coming back.
    /// It is carried because A5's succession ageing wants it and adding a field
    /// to a format already on disk costs a version bump.
    ///
    /// Absent is **unknown**, not infinitely old — an older file or a bug must
    /// never be read as "reap me". Whoever adds ageing inherits that guard.
    pub vacated_at: Option<SystemTime>,
    /// When this slot last **changed hands** — a different node took ground its
    /// predecessor held. Succession, in the plan's sense.
    ///
    /// Not set when a slot is first created, and not set when a node returns to
    /// its own ground: neither is a succession. The first distinction is what
    /// stops a first run painting the entire map as freshly changed; the second
    /// is what stops a node coming back from a blip looking like a replacement.
    ///
    /// Absent is **unknown**, not "changed at the dawn of time" — a slot from an
    /// older file, or one never restamped, must read as *not fresh* rather than
    /// as either extreme.
    pub occupied_at: Option<SystemTime>,
}

/// The assignment for one frame. A slot whose `occupant` is `None` is a
/// **ghost** — its occupant departed and the position is retained, vacant.
///
/// Equality is over the slot map alone, which is what makes idempotence a
/// meaningful assertion. Per-frame *transitions* are deliberately not stored on
/// it: a change is a relation between two layouts, not a property of one, so it
/// is computed by [`Layout::changes_from`] rather than baked into the state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    slots: BTreeMap<SlotKey, SlotState>,
    /// zone → continent ordinal. Durable for the same reason slots are.
    ///
    /// Continent x was `zone_index * stride`, so a zone that sorts before an
    /// existing one shifted EVERY continent east — verified: adding `z-a` moved
    /// `z-b` from x=0 to x=30. Sorting by name fixes only the "reorders" third of
    /// instability source 4; "appears" and "vanishes" need a durable ordinal, the
    /// same principle the slots apply one level down. A departed zone keeps its
    /// ordinal reserved, so its neighbours do not slide over to fill the gap.
    zone_ordinals: BTreeMap<String, u16>,
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
    /// This zone's continent ordinal, if it has one.
    pub fn zone_ordinal(&self, zone: &str) -> Option<u16> {
        self.zone_ordinals.get(zone).copied()
    }
    /// Every zone ordinal, including zones whose nodes have all departed —
    /// their ground stays reserved.
    pub fn zone_ordinals(&self) -> impl Iterator<Item = (&str, u16)> {
        self.zone_ordinals.iter().map(|(z, o)| (z.as_str(), *o))
    }
    /// Every slot, ghosts included, in deterministic order.
    pub fn slots(&self) -> impl Iterator<Item = (&SlotKey, Option<&Occupancy>)> {
        self.slots.iter().map(|(k, v)| (k, v.occupant.as_ref()))
    }
    /// Every slot with its full state — what persistence needs to write.
    ///
    /// Deliberately separate from [`Layout::slots`]: that one exposes only what
    /// a renderer needs, and widening it would hand every consumer the internal
    /// state so it could grow a dependency on the representation.
    pub fn entries(&self) -> impl Iterator<Item = (&SlotKey, &SlotState)> {
        self.slots.iter()
    }

    /// Rebuild a layout from persisted parts.
    ///
    /// Explicit rather than public fields: the fields stay private so the
    /// on-disk shape is a decision rather than a `#[derive]`, and a later
    /// refactor of `slots` cannot silently become a format break.
    ///
    /// Takes whatever it is given. Validation belongs to the loader, which knows
    /// about versions and fingerprints; this only assembles.
    pub fn from_stored(
        slots: impl IntoIterator<Item = (SlotKey, SlotState)>,
        zone_ordinals: impl IntoIterator<Item = (String, u16)>,
    ) -> Self {
        Layout {
            slots: slots.into_iter().collect(),
            zone_ordinals: zone_ordinals.into_iter().collect(),
        }
    }
    /// Slots with an occupant.
    pub fn occupied(&self) -> impl Iterator<Item = (&SlotKey, &Occupancy)> {
        self.slots
            .iter()
            .filter_map(|(k, v)| v.occupant.as_ref().map(|o| (k, o)))
    }
    /// Vacant slots — positions whose occupant departed and which are held open
    /// rather than closed up. See [`assign_layout`] on why.
    pub fn ghosts(&self) -> impl Iterator<Item = &SlotKey> {
        self.slots
            .iter()
            .filter_map(|(k, v)| v.occupant.is_none().then_some(k))
    }
    /// Stamp any vacancy that does not yet carry a time, using a clock the
    /// caller supplies.
    ///
    /// Separate from [`assign_layout`] on purpose: that function is pure, and
    /// purity is what lets the whole engine be tested against synthetic
    /// fixtures without a cluster or a clock. Passing `now` in explicitly is the
    /// house pattern — `attention::build` and `build_timeline` do the same.
    ///
    /// Only *unstamped* ghosts are touched. A vacancy that already has a time
    /// keeps it, so the timestamp answers how long this vacancy has stood
    /// rather than restarting on every tick.
    pub fn stamp_vacancies(&mut self, now: SystemTime) {
        for state in self.slots.values_mut() {
            if state.occupant.is_none() && state.vacated_at.is_none() {
                state.vacated_at = Some(now);
            }
        }
    }

    /// Stamp any slot that just changed hands, using a clock the caller supplies.
    ///
    /// `prior` is the layout this one was assigned from, and it is what makes
    /// the three cases separable: ground that changed hands, ground merely
    /// carried, and ground seen for the first time. All three leave
    /// `occupied_at` unset, so the field alone cannot tell them apart —
    /// [`changed_hands`] against the prior occupant can, and is the same
    /// predicate `place` uses.
    pub fn stamp_successions(&mut self, prior: &Layout, now: SystemTime) {
        let changed: Vec<SlotKey> = self
            .slots
            .iter()
            .filter(|(k, v)| {
                let Some(taking) = v.occupant.as_ref() else {
                    return false;
                };
                let held = prior.slots.get(*k).and_then(|s| s.last_occupant.as_deref());
                changed_hands(held, &taking.node)
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in changed {
            if let Some(s) = self.slots.get_mut(&k) {
                s.occupied_at = Some(now);
            }
        }
    }

    /// When this slot last changed hands, if it has and was stamped.
    pub fn occupied_at(&self, key: &SlotKey) -> Option<SystemTime> {
        self.slots.get(key).and_then(|s| s.occupied_at)
    }

    /// **Reclaim all ghost ground.** Returns how many slots were released.
    ///
    /// Explicit and user-triggered — never automatic. Ghosts hold at the
    /// refresh batch size rather than accumulating, so an automatic reap would
    /// be the map quietly deciding some nodes are not coming back, which is a
    /// judgment it has no basis for. The reason to reach for this is the one
    /// case the map cannot infer: *"I decommissioned that pool and I want the
    /// map to show it."*
    ///
    /// **It does NOT renumber the survivors**, and that is the invariant that
    /// keeps A4 from undoing A1. "Compaction" invites the opposite reading —
    /// closing the gaps — but closing a gap moves every live slot below it,
    /// which is exactly the reshuffle this workstream exists to eliminate. A
    /// reclaimed ordinal is simply left unused.
    pub fn compact(&mut self) -> usize {
        let before = self.slots.len();
        self.slots.retain(|_, v| v.occupant.is_some());
        before - self.slots.len()
    }

    /// When a slot became vacant, if it is vacant and has been stamped.
    pub fn vacated_at(&self, key: &SlotKey) -> Option<SystemTime> {
        self.slots.get(key).and_then(|s| s.vacated_at)
    }

    /// Where this node currently sits, if anywhere.
    pub fn slot_of(&self, node: &str) -> Option<&SlotKey> {
        self.slots
            .iter()
            .find(|(_, v)| v.occupant.as_ref().is_some_and(|o| o.node == node))
            .map(|(k, _)| k)
    }
    /// The slot this node last held, occupied or not — the ground a returning
    /// node reclaims.
    pub fn home_of(&self, node: &str) -> Option<&SlotKey> {
        self.slots
            .iter()
            .find(|(_, v)| v.last_occupant.as_deref() == Some(node))
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
                    .and_then(|s| s.occupant.as_ref())
                    .map(|o| o.node.clone());
                let to = self
                    .slots
                    .get(k)
                    .and_then(|s| s.occupant.as_ref())
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
    // Every prior slot survives as at least a ghost — nothing is ever removed,
    // so ordinals never shift to close a gap. `last_occupant` is carried so a
    // returning node can find its own ground again.
    let mut slots: BTreeMap<SlotKey, SlotState> = prior
        .slots
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                SlotState {
                    occupant: None,
                    last_occupant: v.last_occupant.clone(),
                    // Carried, never stamped here: `assign_layout` is pure and
                    // an ambient clock is what would end that. A slot that is
                    // vacant now and was vacant before keeps its original
                    // timestamp — the answer to "how long has THIS vacancy
                    // stood" must not restart every tick. `place` clears it on
                    // re-occupation, and `stamp_vacancies` fills in the new
                    // ones with a clock the caller supplies.
                    vacated_at: v.vacated_at,
                    // Carried the same way. `place` decides whether the slot
                    // changed hands and clears it if so.
                    occupied_at: v.occupied_at,
                },
            )
        })
        .collect();

    // Zone ordinals first: carried where the zone is already known, appended in
    // a deterministic order otherwise, and NEVER reassigned — a zone keeps its
    // continent position for the life of the layout even after its last node
    // leaves, so neighbours do not slide over.
    let mut zone_ordinals = prior.zone_ordinals.clone();
    let mut fresh_zones: Vec<&str> = observed
        .iter()
        .map(|n| n.zone.as_str())
        .filter(|z| !zone_ordinals.contains_key(*z))
        .collect();
    fresh_zones.sort_unstable();
    fresh_zones.dedup();
    for z in fresh_zones {
        let next = zone_ordinals.values().copied().max().map_or(0, |m| m + 1);
        zone_ordinals.insert(z.to_string(), next);
    }

    // 1. CARRY. A node keeps its slot only while it still belongs to that
    //    (zone, pool): a node that MOVES zone or pool vacates the old slot and
    //    takes a new one, rather than dragging coordinates across a continent.
    let mut unplaced: Vec<&ObservedNode> = Vec::new();
    for n in observed {
        match prior.slot_of(&n.name) {
            Some(k) if k.zone == n.zone && k.pool == n.pool => {
                place(&mut slots, k.clone(), n);
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
        // 2a. RECLAIM — the node's OWN former slot, if it is still vacant and
        //     still in the right (zone, pool). This is what stops two nodes that
        //     drained together from swapping coordinates when they return, and
        //     what stops a returning node landing on a stranger's ground.
        let own = prior.home_of(&n.name).filter(|k| {
            k.zone == n.zone
                && k.pool == n.pool
                && slots.get(*k).is_some_and(|s| s.occupant.is_none())
        });
        // 2b. REUSE — otherwise the LOWEST-ordinal vacancy in this (zone, pool).
        //     BTreeMap orders by (zone, pool, ordinal), so the first match in
        //     iteration order is the lowest ordinal; asserted by test rather than
        //     left to the map's ordering to imply.
        let key = match own.cloned().or_else(|| {
            slots
                .iter()
                .filter(|(k, v)| v.occupant.is_none() && k.zone == n.zone && k.pool == n.pool)
                .map(|(k, _)| k.ordinal)
                .min()
                .map(|ordinal| SlotKey {
                    zone: n.zone.clone(),
                    pool: n.pool.clone(),
                    ordinal,
                })
        }) {
            Some(k) => k,
            // 3. APPEND at the next ordinal free anywhere in the zone.
            None => match next_ordinal(&slots, &n.zone) {
                Some(ordinal) => SlotKey {
                    zone: n.zone.clone(),
                    pool: n.pool.clone(),
                    ordinal,
                },
                // The zone has used every ordinal AND every slot is
                // occupied — 65_536 live nodes in one zone. There is
                // no honest coordinate to give, so the node is reported unplaced
                // rather than handed one a live node already holds. The previous
                // `saturating_add` did exactly that: it returned u16::MAX again
                // and the insert silently EVICTED the incumbent, losing a node
                // from the layout entirely.
                None => continue,
            },
        };
        place(&mut slots, key, n);
    }

    Layout {
        slots,
        zone_ordinals,
    }
}

/// How recently ground changed hands, as `1.0` at the moment of succession
/// decaying to `0.0` at the end of the window.
///
/// `None` means **do not mark**, and it covers three genuinely different states
/// that all have the same honest answer: the slot never changed hands, its
/// timestamp is unknown (an older file, a slot never restamped), or the window
/// is `0` and marking is off.
///
/// A `window` of zero is a real supported value meaning *never mark*, not a
/// degenerate one — so it is checked before the division rather than producing
/// an infinity that would mark everything forever.
///
/// A timestamp in the future is treated as "just now" rather than discarded: a
/// clock that stepped backwards is a reason to be slightly wrong about age, not
/// a reason to lose the fact that ground changed.
pub fn freshness(
    occupied_at: Option<SystemTime>,
    now: SystemTime,
    window: std::time::Duration,
) -> Option<f64> {
    if window.is_zero() {
        return None;
    }
    let at = occupied_at?;
    let age = now.duration_since(at).unwrap_or_default();
    if age >= window {
        return None;
    }
    Some(1.0 - age.as_secs_f64() / window.as_secs_f64())
}

/// Did ground change hands? `held` is who the slot last belonged to.
///
/// The single definition of succession, consulted by both `place` (which clears
/// the stamp) and [`Layout::stamp_successions`] (which sets it). They ran as two
/// copies of the rule at first and disagreed immediately — the placer cleared on
/// a change of hands while the stamper stamped anything unstamped, so a carry
/// and a node returning to its own ground were both marked as replacements.
///
/// `None` — the slot never had an occupant — is a **first sighting**, not a
/// succession. That is what stops a first run painting the whole map.
fn changed_hands(held: Option<&str>, taking: &str) -> bool {
    held.is_some_and(|h| h != taking)
}

/// Seat a node, recording it as the slot's most recent occupant so it can
/// reclaim the ground later.
fn place(slots: &mut BTreeMap<SlotKey, SlotState>, key: SlotKey, n: &ObservedNode) {
    // Did this ground CHANGE HANDS? Compared against who last held it, not
    // against who held it on the previous tick — a rolling refresh drains a node
    // in one tick and its replacement reclaims the slot in another, so a
    // tick-to-tick comparison sees a departure and then an arrival and never a
    // succession at all. `last_occupant` spans that gap, which is exactly what
    // it was added for.
    let prior = slots.get(&key);
    let succeeded = changed_hands(prior.and_then(|s| s.last_occupant.as_deref()), &n.name);
    slots.insert(
        key,
        SlotState {
            occupant: Some(occupancy(n)),
            last_occupant: Some(n.name.clone()),
            // Cleared on a change of hands so `stamp_successions` fills it in
            // with the caller's clock; carried otherwise, so a slot that merely
            // persists keeps ageing from when it actually changed rather than
            // resetting every tick.
            occupied_at: if succeeded {
                None
            } else {
                prior.and_then(|s| s.occupied_at)
            },
            // Occupied ground is not vacant ground: clearing here is what keeps
            // `vacated_at` meaning "how long this CURRENT vacancy has stood"
            // rather than "when this slot was ever last empty".
            vacated_at: None,
        },
    );
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
/// The next free ordinal in a ZONE — across every pool in it, not within one.
///
/// See `SlotKey`: the ordinal is a zone-wide row index, so a fresh pool appends
/// below whatever is already there instead of restarting at 0 on top of it.
fn next_ordinal(slots: &BTreeMap<SlotKey, SlotState>, zone: &str) -> Option<u16> {
    match slots
        .keys()
        .filter(|k| k.zone == zone)
        .map(|k| k.ordinal)
        .max()
    {
        None => Some(0),
        // `checked_add`, NOT `saturating_add`: saturating hands back u16::MAX a
        // second time, and since the key is then equal to the incumbent's, the
        // insert OVERWRITES a live node — it vanishes from the layout. Verified.
        Some(m) => m.checked_add(1),
    }
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

    /// THE MULTI-VACANCY CASE, which the single-vacancy reclaim test could not
    /// discriminate: with more than one ghost, a returning node must land on ITS
    /// OWN ground, not on whichever vacancy sorts lowest.
    ///
    /// Without slot memory, `b` below takes `a`'s ordinal and leaves a permanent
    /// ghost at its own — and if `a` and `b` drain and return together they
    /// simply SWAP coordinates, which is precisely the move this engine exists
    /// to prevent.
    #[test]
    fn a_returning_node_reclaims_its_own_ground_when_several_slots_are_vacant() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b", "c"], "z", "p"));
        let (home_a, home_b) = (
            l0.slot_of("a").cloned().expect("a"),
            l0.slot_of("b").cloned().expect("b"),
        );
        assert_ne!(home_a.ordinal, home_b.ordinal);

        // a and b drain together: two vacancies, so "lowest" and "own" differ.
        let l1 = assign_layout(&l0, &fleet(&["c"], "z", "p"));
        assert_eq!(l1.ghosts().count(), 2);

        // b alone returns — it must NOT take a's ground just because it sorts lower.
        let l2 = assign_layout(&l1, &fleet(&["b", "c"], "z", "p"));
        assert_eq!(
            l2.slot_of("b"),
            Some(&home_b),
            "b landed on a stranger's slot"
        );

        // Both return: they must not swap.
        let l3 = assign_layout(&l1, &fleet(&["a", "b", "c"], "z", "p"));
        assert_eq!(
            l3.slot_of("a"),
            Some(&home_a),
            "a and b swapped coordinates"
        );
        assert_eq!(
            l3.slot_of("b"),
            Some(&home_b),
            "a and b swapped coordinates"
        );
        assert_eq!(l3.ghosts().count(), 0);
    }

    /// The FALLBACK when a node has no ground of its own: the LOWEST-ordinal
    /// vacancy. Stated in three places in the docs and previously pinned by
    /// nothing — inverting it to `.max()` left every test green.
    #[test]
    fn a_newcomer_takes_the_lowest_ordinal_vacancy() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a", "b", "c", "d"], "z", "p"));
        let lowest = l0
            .occupied()
            .map(|(k, _)| k.ordinal)
            .min()
            .expect("some ordinal");
        let gone: Vec<&str> = l0
            .occupied()
            .filter(|(k, _)| k.ordinal != 1)
            .map(|(_, o)| o.node.as_str())
            .collect();
        // Drain everything except ordinal 1, leaving several vacancies.
        let keep = fleet(
            &l0.occupied()
                .filter(|(k, _)| k.ordinal == 1)
                .map(|(_, o)| o.node.as_str())
                .collect::<Vec<_>>(),
            "z",
            "p",
        );
        let l1 = assign_layout(&l0, &keep);
        assert!(l1.ghosts().count() >= 2, "need multiple vacancies");
        assert!(!gone.is_empty());

        // A NEWCOMER (no history) must take the lowest vacancy.
        let mut obs = keep.clone();
        obs.push(node("newcomer", "z", "p"));
        let l2 = assign_layout(&l1, &obs);
        assert_eq!(
            l2.slot_of("newcomer").map(|k| k.ordinal),
            Some(lowest),
            "a newcomer did not take the lowest-ordinal vacancy"
        );
    }

    /// The ordinal ceiling must not fabricate a coordinate. `saturating_add`
    /// returned `u16::MAX` a second time, making the newcomer's key equal the
    /// incumbent's — and the insert then EVICTED a live node from the layout.
    /// **SUCCESSION IS A CHANGE OF HANDS — not an arrival, not a return.**
    ///
    /// The three cases have to be distinguished or the marking is useless: a
    /// first run would paint every slot, and a node returning from a blip would
    /// look like it had been replaced.
    #[test]
    fn only_ground_that_changed_hands_is_stamped() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);

        // FIRST SIGHTING — every slot is new. Nothing changed hands.
        let empty = Layout::default();
        let mut first = assign_layout(&empty, &fleet(&["a", "b"], "z", "p"));
        first.stamp_successions(&empty, t);
        assert!(
            first
                .occupied()
                .all(|(k, _)| first.occupied_at(k).is_none()),
            "a first run marked the map as freshly changed"
        );

        // CARRY — the same nodes again. Still nothing.
        let mut same = assign_layout(&first, &fleet(&["a", "b"], "z", "p"));
        same.stamp_successions(&first, t);
        assert!(same.occupied().all(|(k, _)| same.occupied_at(k).is_none()));

        // SUCCESSION — `b` drains, and a differently-named replacement reclaims
        // its ground. Note this spans TWO rebuilds, which is why the detection
        // keys on `last_occupant` rather than on a tick-to-tick comparison:
        // between them the slot is a ghost, so a transient detector sees a
        // departure and then an arrival and never a succession at all.
        let drained = assign_layout(&same, &fleet(&["a"], "z", "p"));
        let mut refreshed = assign_layout(&drained, &fleet(&["a", "b2"], "z", "p"));
        refreshed.stamp_successions(&drained, t);

        let b2 = refreshed.slot_of("b2").expect("placed").clone();
        assert_eq!(
            refreshed.occupied_at(&b2),
            Some(t),
            "the wave was not marked"
        );
        let a = refreshed.slot_of("a").expect("placed").clone();
        assert_eq!(
            refreshed.occupied_at(&a),
            None,
            "an untouched slot was marked"
        );
    }

    /// A node that leaves and comes back has not been succeeded — the ground
    /// never changed hands, so marking it would report a replacement that did
    /// not happen.
    #[test]
    fn a_node_returning_to_its_own_ground_is_not_a_succession() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let both = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        let gone = assign_layout(&both, &fleet(&["a"], "z", "p"));
        let mut back = assign_layout(&gone, &fleet(&["a", "b"], "z", "p"));
        back.stamp_successions(&gone, t);

        let b = back.slot_of("b").expect("placed").clone();
        assert_eq!(
            back.occupied_at(&b),
            None,
            "a return was marked as a replacement"
        );
    }

    /// The stamp is set once and then ages; a later rebuild must not reset it,
    /// or the marking would never fade and the wave would have no trailing edge.
    #[test]
    fn a_succession_stamp_ages_rather_than_resetting_each_tick() {
        let t0 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let t1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5_000);
        let both = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        let gone = assign_layout(&both, &fleet(&["a"], "z", "p"));
        let mut refreshed = assign_layout(&gone, &fleet(&["a", "b2"], "z", "p"));
        refreshed.stamp_successions(&gone, t0);
        let slot = refreshed.slot_of("b2").expect("placed").clone();

        let mut later = assign_layout(&refreshed, &fleet(&["a", "b2"], "z", "p"));
        later.stamp_successions(&refreshed, t1);
        assert_eq!(later.occupied_at(&slot), Some(t0), "the stamp restarted");
    }

    /// Ageing: fresh at the moment, gone by the end of the window, and every
    /// "do not mark" case answers `None` rather than a fabricated extreme.
    #[test]
    fn freshness_fades_and_refuses_to_guess() {
        use std::time::Duration;
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let hour = Duration::from_secs(3600);

        assert_eq!(freshness(Some(t), t, hour), Some(1.0));
        let half = freshness(Some(t), t + Duration::from_secs(1800), hour).expect("mid-window");
        assert!((half - 0.5).abs() < 1e-9, "{half}");
        assert_eq!(
            freshness(Some(t), t + hour, hour),
            None,
            "still fresh at the window's end"
        );
        assert_eq!(freshness(Some(t), t + hour * 2, hour), None);

        // Unknown is not "infinitely old" and not "brand new" — it is unmarked.
        assert_eq!(freshness(None, t, hour), None);
        // A window of zero means never mark, and must not divide by it.
        assert_eq!(freshness(Some(t), t, Duration::ZERO), None);
        // A clock that stepped backwards loses precision, not the fact.
        assert_eq!(
            freshness(Some(t), t - Duration::from_secs(60), hour),
            Some(1.0)
        );
    }

    /// **COMPACTION RECLAIMS GROUND. IT DOES NOT RENUMBER.**
    ///
    /// The invariant that keeps A4 from undoing A1. The word invites the
    /// opposite reading — closing the gaps — but closing a gap moves every live
    /// slot below it, which is the exact reshuffle this workstream exists to
    /// eliminate. A reclaimed ordinal is left unused.
    #[test]
    fn compaction_reclaims_ghost_ground_without_moving_a_live_slot() {
        let all = assign_layout(
            &Layout::default(),
            &fleet(&["a", "b", "c", "d", "e"], "z", "p"),
        );
        // Keep the FIRST and LAST ordinals, so the three ghosts are interior —
        // the case where renumbering is tempting and would drag the last
        // survivor two slots north. Chosen by ordinal rather than by name:
        // ordinals come from hash order, so naming them would be asserting a
        // fixture accident instead of the invariant.
        let ends: Vec<String> = {
            let mut occ: Vec<(u16, String)> = all
                .occupied()
                .map(|(k, o)| (k.ordinal, o.node.clone()))
                .collect();
            occ.sort();
            vec![
                occ.first().unwrap().1.clone(),
                occ.last().unwrap().1.clone(),
            ]
        };
        let survivors: Vec<&str> = ends.iter().map(String::as_str).collect();
        let mut layout = assign_layout(&all, &fleet(&survivors, "z", "p"));
        let before: Vec<(SlotKey, String)> = layout
            .occupied()
            .map(|(k, o)| (k.clone(), o.node.clone()))
            .collect();
        assert_eq!(layout.ghosts().count(), 3, "three interior ghosts");

        let reclaimed = layout.compact();

        assert_eq!(reclaimed, 3);
        assert_eq!(layout.ghosts().count(), 0, "all ghost ground reclaimed");
        let after: Vec<(SlotKey, String)> = layout
            .occupied()
            .map(|(k, o)| (k.clone(), o.node.clone()))
            .collect();
        assert_eq!(before, after, "compaction moved a live slot");
        // Said plainly: the reclaimed ordinals stay unused rather than closing
        // up. Renumbering two survivors would produce 0 and 1; the gap between
        // the ends must still be there.
        let mut ordinals: Vec<u16> = layout.occupied().map(|(k, _)| k.ordinal).collect();
        ordinals.sort();
        assert_eq!(ordinals, vec![0, 4], "the ordinal gap was closed up");
    }

    /// Compacting a layout with nothing to reclaim reports zero rather than
    /// succeeding silently — the caller has to be able to tell "done" from
    /// "there was nothing to do", because it will say so to a user.
    #[test]
    fn compacting_with_no_ghosts_reclaims_nothing_and_says_so() {
        let mut layout = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));
        assert_eq!(layout.compact(), 0);
        assert_eq!(layout.occupied().count(), 2);
    }

    /// A vacancy is stamped once and keeps that time; re-occupying clears it.
    /// Without the first half, "how long has this stood empty" would restart on
    /// every tick and any future ageing would never fire.
    #[test]
    fn a_vacancy_is_stamped_once_and_cleared_on_return() {
        let t0 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let t1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(9_999);
        let all = assign_layout(&Layout::default(), &fleet(&["a", "b"], "z", "p"));

        let mut gone = assign_layout(&all, &fleet(&["a"], "z", "p"));
        gone.stamp_vacancies(t0);
        let ghost = gone.ghosts().next().expect("a ghost").clone();
        assert_eq!(gone.vacated_at(&ghost), Some(t0));

        // A later tick with the slot still vacant must not restamp it.
        let mut still = assign_layout(&gone, &fleet(&["a"], "z", "p"));
        still.stamp_vacancies(t1);
        assert_eq!(still.vacated_at(&ghost), Some(t0), "the vacancy restarted");

        // `b` returns to its own ground.
        let back = assign_layout(&still, &fleet(&["a", "b"], "z", "p"));
        assert_eq!(back.vacated_at(&ghost), None);
    }

    /// GHOSTS REACH A STEADY STATE AT THE REFRESH BATCH SIZE — they do not
    /// accumulate with cadence.
    ///
    /// This pins the claim A4's first revision got wrong, and the reason it was
    /// wrong is worth keeping: a *single-wave full-fleet surge* really does
    /// leave N ghosts for N nodes, which is what A1 measured and reported
    /// correctly. Generalising that to a **rolling** refresh does not hold,
    /// because each wave's replacements REUSE the vacancies the previous wave
    /// left — so the standing count is the batch size, forever, however often
    /// the fleet is refreshed.
    ///
    /// The design consequence, recorded because it is easy to lose: a standing
    /// batch-size set of ghosts is the mechanism working, not debt to be
    /// reclaimed. That ground is reserved for nodes that may return.
    #[test]
    fn batched_refreshes_hold_ghosts_at_the_batch_size() {
        const BATCH: usize = 10;
        let mut live: Vec<String> = (0..100).map(|i| format!("g0-{i:03}")).collect();
        let as_nodes =
            |v: &[String]| -> Vec<ObservedNode> { v.iter().map(|n| node(n, "z", "p")).collect() };
        let mut layout = assign_layout(&Layout::default(), &as_nodes(&live));

        for round in 1..=4 {
            let prefix = format!("g{round}-");
            let stale: Vec<String> = live
                .iter()
                .filter(|n| !n.starts_with(&prefix))
                .cloned()
                .collect();
            for wave in stale.chunks(BATCH) {
                // SURGE: replacements are Ready before the predecessors drain,
                // which is what makes this a rolling refresh rather than a
                // delete-then-create.
                let mut surged = live.clone();
                for (i, old) in wave.iter().enumerate() {
                    surged.push(format!("{prefix}{i}-{old}"));
                }
                layout = assign_layout(&layout, &as_nodes(&surged));
                // DRAIN
                live = surged.into_iter().filter(|n| !wave.contains(n)).collect();
                layout = assign_layout(&layout, &as_nodes(&live));
            }
            assert_eq!(layout.occupied().count(), 100, "round {round}: fleet size");
            assert_eq!(
                layout.ghosts().count(),
                BATCH,
                "round {round}: ghosts should hold at the batch size, not accumulate"
            );
        }

        // Shrinkage is the case that DOES leave lasting ground: twenty nodes
        // leave and are not replaced, so their slots stay reserved on top of
        // the standing batch. That is what compaction is for.
        live.truncate(80);
        let after = assign_layout(&layout, &as_nodes(&live));
        assert_eq!(after.occupied().count(), 80);
        assert_eq!(
            after.ghosts().count(),
            BATCH + 20,
            "a genuine scale-down retains its ground"
        );
    }

    #[test]
    fn a_full_ordinal_space_never_evicts_a_live_node() {
        let mut prior = Layout::default();
        prior.slots.insert(
            SlotKey {
                zone: "z".into(),
                pool: "p".into(),
                ordinal: u16::MAX,
            },
            SlotState {
                occupant: Some(Occupancy {
                    node: "incumbent".into(),
                    pool_source: PoolSource::Default,
                }),
                last_occupant: Some("incumbent".into()),
                vacated_at: None,
                occupied_at: None,
            },
        );
        let l = assign_layout(&prior, &fleet(&["incumbent", "newcomer"], "z", "p"));
        assert_eq!(
            l.slot_of("incumbent").map(|k| k.ordinal),
            Some(u16::MAX),
            "the incumbent was evicted from its own slot"
        );
        assert_eq!(
            l.slot_of("newcomer"),
            None,
            "a node with no honest coordinate must be left unplaced, not given one \
             a live node holds"
        );
        assert_eq!(l.occupied().count(), 1);
    }

    /// INSTABILITY SOURCE 4, the "appears" half. Continent x was
    /// `zone_index * stride`, so a zone sorting before an existing one shifted
    /// every continent east. Sorting by name only fixes "reorders".
    #[test]
    fn a_new_zone_appends_and_moves_no_existing_continent() {
        let l0 = assign_layout(
            &Layout::default(),
            &[node("a", "z-b", "p"), node("b", "z-c", "p")],
        );
        let (b0, c0) = (l0.zone_ordinal("z-b"), l0.zone_ordinal("z-c"));
        assert_eq!((b0, c0), (Some(0), Some(1)));

        // z-a sorts FIRST alphabetically — the case that used to shift everything.
        let l1 = assign_layout(
            &l0,
            &[
                node("a", "z-b", "p"),
                node("b", "z-c", "p"),
                node("c", "z-a", "p"),
            ],
        );
        assert_eq!(l1.zone_ordinal("z-b"), b0, "z-b's continent moved");
        assert_eq!(l1.zone_ordinal("z-c"), c0, "z-c's continent moved");
        assert_eq!(l1.zone_ordinal("z-a"), Some(2), "the newcomer appends");
    }

    /// The "vanishes" half: a zone losing every node keeps its ground reserved,
    /// so its neighbours do not slide over to close the gap.
    #[test]
    fn a_departed_zone_keeps_its_continent_ordinal_reserved() {
        let l0 = assign_layout(
            &Layout::default(),
            &[
                node("a", "z-a", "p"),
                node("b", "z-b", "p"),
                node("c", "z-c", "p"),
            ],
        );
        let c0 = l0.zone_ordinal("z-c");
        // z-b loses every node (a zone outage).
        let l1 = assign_layout(&l0, &[node("a", "z-a", "p"), node("c", "z-c", "p")]);
        assert_eq!(
            l1.zone_ordinal("z-c"),
            c0,
            "z-c slid over z-b's empty ground"
        );
        assert_eq!(
            l1.zone_ordinal("z-b"),
            Some(1),
            "z-b's ground stays reserved"
        );

        // And it reclaims that ground when it comes back.
        let l2 = assign_layout(&l1, &[node("b2", "z-b", "p")]);
        assert_eq!(l2.zone_ordinal("z-b"), Some(1));
    }

    /// Zone ordinals must not depend on the order nodes are observed in.
    #[test]
    fn zone_ordinals_are_independent_of_observation_order() {
        let mut obs = vec![
            node("a", "z-c", "p"),
            node("b", "z-a", "p"),
            node("c", "z-b", "p"),
        ];
        let base = assign_layout(&Layout::default(), &obs);
        for _ in 0..3 {
            obs.rotate_left(1);
            let l = assign_layout(&Layout::default(), &obs);
            for z in ["z-a", "z-b", "z-c"] {
                assert_eq!(l.zone_ordinal(z), base.zone_ordinal(z), "{z} moved");
            }
        }
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

    /// `changes_from` is A5's declared raw material, and it was tested only on
    /// the replacement case — dropping either direction left every test green.
    /// Both are pinned here: a slot present only in the NEW layout (an arrival at
    /// fresh ground) and one present only in the PRIOR (a departure).
    #[test]
    fn changes_from_reports_both_directions_of_the_key_union() {
        let l0 = assign_layout(&Layout::default(), &fleet(&["a"], "z", "p"));

        // ARRIVAL at a slot the prior layout did not contain at all.
        let l1 = assign_layout(&l0, &fleet(&["a", "b"], "z", "p"));
        let arrivals = l1.changes_from(&l0);
        assert_eq!(arrivals.len(), 1, "{arrivals:?}");
        assert_eq!(arrivals[0].from, None, "the slot did not exist before");
        assert_eq!(arrivals[0].to.as_deref(), Some("b"));

        // DEPARTURE: the slot exists in both, occupied only in the prior.
        let l2 = assign_layout(&l1, &fleet(&["a"], "z", "p"));
        let departures = l2.changes_from(&l1);
        assert_eq!(departures.len(), 1, "{departures:?}");
        assert_eq!(departures[0].from.as_deref(), Some("b"));
        assert_eq!(departures[0].to, None, "b left a ghost");

        // And the reverse comparison sees the mirror image, so neither direction
        // is silently dropped.
        let mirrored = l1.changes_from(&l2);
        assert_eq!(mirrored.len(), 1);
        assert_eq!(mirrored[0].from, None);
        assert_eq!(mirrored[0].to.as_deref(), Some("b"));
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
