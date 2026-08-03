//! The persisted form of a [`Layout`], and the conversions to and from it.
//!
//! PURE — this module knows nothing about the filesystem. The disk half lives in
//! the GUI crate, mirroring the split between `state/oracle_config.rs` (pure
//! types with serde) and `gui/oracle_config_io.rs` (the only file that touches
//! disk). It means the round trip is testable without a temp directory, and the
//! format cannot quietly acquire an I/O dependency.
//!
//! **This is the first artifact in the workstream that outlives the process.**
//! Everything else can be changed by changing the code; a file already on
//! someone's disk cannot. Hence a DTO with explicit conversion rather than serde
//! derives on [`Layout`]: `Layout`'s fields are private on purpose, and deriving
//! on them would couple the format to the internal `BTreeMap<SlotKey,
//! SlotState>`, so a later refactor of the engine would become a format break.

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::layout::{Layout, Occupancy, PoolSource, SlotKey, SlotState};

/// Bump on an **incompatible** schema change. A file whose version is newer than
/// this is refused rather than guessed at — see [`from_stored`].
pub const LAYOUT_VERSION: u32 = 1;

/// The persisted form. Deliberately flat, and deliberately not [`Layout`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLayout {
    pub version: u32,
    /// A stable cluster-scoped UID observed when this was written.
    ///
    /// `None` is a real and supported state — a cluster where the UID could not
    /// be read is common, and it means *unverified*, never *mismatched*.
    #[serde(default)]
    pub fingerprint: Option<String>,
    pub zone_ordinals: Vec<(String, u16)>,
    pub slots: Vec<StoredSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSlot {
    pub zone: String,
    pub pool: String,
    pub ordinal: u16,
    /// The node in this slot. `None` is a ghost.
    #[serde(default)]
    pub occupant: Option<String>,
    /// Who held it last, which is what lets a returning node reclaim its own
    /// ground instead of whichever vacancy sorts lowest.
    #[serde(default)]
    pub last_occupant: Option<String>,
    /// Seconds since the Unix epoch, set when the slot vacated.
    ///
    /// Nothing reads it in A4 — there is no automatic reap — but it is carried
    /// because A5's succession ageing wants it, and adding a field to a format
    /// already on disk costs a version bump.
    ///
    /// Absent means **unknown**, not infinitely old.
    #[serde(default)]
    pub vacated_at: Option<u64>,
    /// Seconds since the Unix epoch, set when the slot last CHANGED HANDS.
    ///
    /// Added after the format shipped. **`LAYOUT_VERSION` is deliberately not
    /// bumped**: an optional field with `#[serde(default)]` reads an older file
    /// as `None` (which means *unknown*, and unknown means not fresh), and a
    /// newer file read by an older build is ignored rather than rejected —
    /// `StoredSlot` does not deny unknown fields. Compatible both directions, so
    /// per `prefs.rs`'s convention of bumping only on an INCOMPATIBLE change,
    /// this is not one.
    #[serde(default)]
    pub occupied_at: Option<u64>,
}

/// Why a stored layout could not be used as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadRefusal {
    /// Written by a newer KuberNation. Refused rather than guessed at: a partial
    /// read of a format we do not know could place nodes on wrong ground, which
    /// is worse than starting fresh.
    FutureVersion { found: u32, supported: u32 },
    /// The stored fingerprint and the observed one disagree, so this layout
    /// describes a different cluster that happens to share a context name.
    DifferentCluster,
}

impl LoadRefusal {
    /// Phrased for a user who is about to notice their map changed.
    pub fn describe(&self) -> String {
        match self {
            LoadRefusal::FutureVersion { found, supported } => format!(
                "the saved map was written by a newer version (format {found}, this build reads \
                 {supported}) — starting a fresh map rather than guessing at it"
            ),
            LoadRefusal::DifferentCluster => {
                "this context now points at a different cluster than the saved map was drawn \
                 for — starting a fresh map"
                    .into()
            }
        }
    }
}

/// How much to trust the identity check that let a layout load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Stored and observed fingerprints matched.
    Verified,
    /// One or both fingerprints were absent. The layout loads — refusing here
    /// would break stability for exactly the least-privileged clusters, which
    /// are the ones that benefit from it most — but it is not proof of identity
    /// and must not be reported as such.
    Unverified,
}

/// Serialise a layout, tagging it with the fingerprint observed now.
pub fn to_stored(layout: &Layout, fingerprint: Option<&str>) -> StoredLayout {
    StoredLayout {
        version: LAYOUT_VERSION,
        fingerprint: fingerprint.map(str::to_owned),
        zone_ordinals: layout
            .zone_ordinals()
            .map(|(z, o)| (z.to_string(), o))
            .collect(),
        slots: layout
            .entries()
            .map(|(k, v)| StoredSlot {
                zone: k.zone.clone(),
                pool: k.pool.clone(),
                ordinal: k.ordinal,
                occupant: v.occupant.as_ref().map(|o| o.node.clone()),
                last_occupant: v.last_occupant.clone(),
                vacated_at: v.vacated_at.and_then(to_unix),
                occupied_at: v.occupied_at.and_then(to_unix),
            })
            .collect(),
    }
}

/// Rebuild a layout, checking version and identity first.
///
/// `observed` is the fingerprint read from the live cluster, or `None` when it
/// could not be read. The three cases are deliberately distinct:
///
/// | stored | observed | result |
/// |---|---|---|
/// | matches | matches | `Verified` |
/// | differs | — | `DifferentCluster` |
/// | either absent | — | `Unverified` |
///
/// **Absent is not mismatched.** Conflating them would discard a working map
/// every time an RBAC-restricted cluster could not read its own namespace.
pub fn from_stored(
    stored: StoredLayout,
    observed: Option<&str>,
) -> Result<(Layout, Trust), LoadRefusal> {
    if stored.version > LAYOUT_VERSION {
        return Err(LoadRefusal::FutureVersion {
            found: stored.version,
            supported: LAYOUT_VERSION,
        });
    }
    let trust = match (stored.fingerprint.as_deref(), observed) {
        (Some(a), Some(b)) if a == b => Trust::Verified,
        (Some(_), Some(_)) => return Err(LoadRefusal::DifferentCluster),
        _ => Trust::Unverified,
    };

    let slots = stored.slots.into_iter().map(|s| {
        let key = SlotKey {
            zone: s.zone,
            pool: s.pool,
            ordinal: s.ordinal,
        };
        let state = SlotState {
            occupant: s.occupant.map(|node| Occupancy {
                node,
                // The pool source is a fact about how a LIVE node was read, not
                // about the slot, so it is re-derived on the next observation
                // rather than persisted. Storing it would let a stale answer
                // outlive the labels it came from.
                pool_source: PoolSource::Default,
            }),
            last_occupant: s.last_occupant,
            vacated_at: s.vacated_at.map(from_unix),
            occupied_at: s.occupied_at.map(from_unix),
        };
        (key, state)
    });
    Ok((Layout::from_stored(slots, stored.zone_ordinals), trust))
}

fn to_unix(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

fn from_unix(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::layout::{ObservedNode, assign_layout};

    fn node(name: &str) -> ObservedNode {
        ObservedNode {
            name: name.into(),
            zone: "z-a".into(),
            pool: "p".into(),
            pool_source: PoolSource::Default,
        }
    }

    /// A layout with an occupant, a ghost, and a zone ordinal — the three things
    /// that have to survive a restart for the map to be the same map.
    fn sample() -> Layout {
        let full = assign_layout(&Layout::default(), &[node("a"), node("b")]);
        let mut fewer = assign_layout(&full, &[node("a")]);
        fewer.stamp_vacancies(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        fewer
    }

    #[test]
    fn a_layout_survives_the_round_trip_intact() {
        let before = sample();
        assert_eq!(before.ghosts().count(), 1, "the fixture needs a ghost");

        let json = serde_json::to_string(&to_stored(&before, Some("fp"))).expect("serialise");
        let back: StoredLayout = serde_json::from_str(&json).expect("deserialise");
        let (after, trust) = from_stored(back, Some("fp")).expect("loads");

        assert_eq!(trust, Trust::Verified);
        assert_eq!(before, after, "the layout is not the layout it was");
        // Equality is over the slot map, so check the parts it does not cover.
        assert_eq!(
            before.zone_ordinals().collect::<Vec<_>>(),
            after.zone_ordinals().collect::<Vec<_>>()
        );
        let ghost = after.ghosts().next().expect("ghost survived").clone();
        assert_eq!(
            after.vacated_at(&ghost),
            Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            "the vacancy timestamp did not survive"
        );
    }

    #[test]
    fn a_file_from_a_future_version_is_refused_not_guessed_at() {
        let mut s = to_stored(&sample(), None);
        s.version = LAYOUT_VERSION + 1;
        let err = from_stored(s, None).expect_err("must refuse");
        assert!(matches!(err, LoadRefusal::FutureVersion { .. }));
        assert!(err.describe().contains("newer version"));
    }

    #[test]
    fn a_different_cluster_behind_the_same_context_is_discarded() {
        let s = to_stored(&sample(), Some("cluster-one"));
        let err = from_stored(s, Some("cluster-two")).expect_err("must refuse");
        assert_eq!(err, LoadRefusal::DifferentCluster);
    }

    /// ABSENT IS NOT MISMATCHED. Refusing to load when the fingerprint cannot be
    /// read would break the map for exactly the RBAC-restricted clusters that
    /// benefit most from stability — so it loads, and says it is unverified.
    #[test]
    fn an_unreadable_fingerprint_loads_as_unverified_rather_than_as_a_mismatch() {
        // Written with a fingerprint, read on a cluster that will not give one.
        let (_, trust) = from_stored(to_stored(&sample(), Some("fp")), None).expect("loads");
        assert_eq!(trust, Trust::Unverified);
        // And an older file that never had one.
        let (_, trust) = from_stored(to_stored(&sample(), None), Some("fp")).expect("loads");
        assert_eq!(trust, Trust::Unverified);
        // Neither side has one.
        let (_, trust) = from_stored(to_stored(&sample(), None), None).expect("loads");
        assert_eq!(trust, Trust::Unverified);
    }

    /// Occupied slots as a comparable list.
    fn placement(l: &Layout) -> Vec<(SlotKey, String)> {
        l.occupied()
            .map(|(k, o)| (k.clone(), o.node.clone()))
            .collect()
    }

    /// A layout carrying HISTORY — a node arrived, took ground, and left.
    ///
    /// This matters more than it looks. A restart test whose fixture is a
    /// single-shot assignment of the live set is **insensitive to persistence**:
    /// assignment from scratch is deterministic in the node set, so it
    /// reproduces the same map with the load path entirely broken. The first
    /// version of these tests had exactly that, and the mutation floor passed
    /// straight through them — the same blind spot as a process-per-frame
    /// screenshot flipbook, one layer down.
    ///
    /// Growing the fleet is not enough either: whether arrival order differs
    /// from hash order is luck. A departure leaving an **interior** gap is not
    /// luck — a rebuild has no reason to skip an ordinal — but *which* ordinal a
    /// departing node held is itself hash order, so the first attempt at this
    /// picked one that sorted last and left no gap at all. Both guards fired.
    ///
    /// So the candidate is searched for rather than assumed: try names until one
    /// lands in the interior, and fail loudly if none does.
    fn with_history(live: &[ObservedNode]) -> Layout {
        for i in 0..64 {
            let departed = node(&format!("departed-{i}"));
            let mut all: Vec<ObservedNode> = live.to_vec();
            all.push(departed.clone());
            let full = assign_layout(&Layout::default(), &all);
            let ordinal = full.slot_of(&departed.name).expect("placed").ordinal;
            // Interior: something still sits below it, so its departure leaves a
            // hole rather than just shortening the fleet.
            if (ordinal as usize) + 1 < all.len() {
                return assign_layout(&full, live);
            }
        }
        panic!("no interior departure candidate — the fixture cannot discriminate");
    }

    /// **THE GATE, as a unit test: open, close, reopen — the same map.**
    #[test]
    fn a_layout_reloaded_after_a_restart_keeps_every_occupied_slot() {
        let nodes = [node("alpha"), node("bravo"), node("charlie")];
        let session_one = with_history(&nodes);

        // GUARD THE GUARD. If a from-scratch assignment already produces this
        // placement, the test cannot tell a restored layout from a rebuilt one
        // and proves nothing.
        assert_ne!(
            placement(&session_one),
            placement(&assign_layout(&Layout::default(), &nodes)),
            "fixture is not discriminating: a fresh assignment already matches, \
             so this test would pass with persistence broken"
        );

        // Close: serialise, drop everything.
        let json = serde_json::to_string(&to_stored(&session_one, Some("fp"))).expect("write");
        let before = placement(&session_one);
        let ghosts_before = session_one.ghosts().count();
        drop(session_one);

        // Reopen: load, then assign against the same cluster.
        let (restored, _) =
            from_stored(serde_json::from_str(&json).expect("read"), Some("fp")).expect("loads");
        let session_two = assign_layout(&restored, &nodes);

        assert_eq!(
            before,
            placement(&session_two),
            "the map is not the same map after a restart"
        );
        assert_eq!(
            session_two.ghosts().count(),
            ghosts_before,
            "reserved ground did not survive the restart"
        );
    }

    /// The harder case, and the one the thesis actually rests on: a rolling
    /// refresh happens **while the app is closed**. Survivors must hold, and the
    /// replacements must land on their predecessors' ground rather than being
    /// appended past the end of the world.
    #[test]
    fn survivors_hold_and_replacements_reclaim_across_a_restart() {
        let old = [
            node("keep-1"),
            node("keep-2"),
            node("gone-1"),
            node("gone-2"),
        ];
        let session_one = with_history(&old);
        let kept: Vec<(SlotKey, String)> = session_one
            .occupied()
            .filter(|(_, o)| o.node.starts_with("keep-"))
            .map(|(k, o)| (k.clone(), o.node.clone()))
            .collect();
        let vacated: Vec<u16> = session_one
            .occupied()
            .filter(|(_, o)| o.node.starts_with("gone-"))
            .map(|(k, _)| k.ordinal)
            .collect();

        let json = serde_json::to_string(&to_stored(&session_one, None)).expect("write");
        drop(session_one);

        // Closed. The two `gone-` nodes are replaced while nobody is watching.
        let now = [node("keep-1"), node("keep-2"), node("new-1"), node("new-2")];
        let (restored, _) =
            from_stored(serde_json::from_str(&json).expect("read"), None).expect("loads");
        let restored_slots = restored.slots().count();
        let session_two = assign_layout(&restored, &now);

        // Guard the guard, as above: without the stored ghosts a fresh
        // assignment must NOT already satisfy the assertions below.
        let scratch = assign_layout(&Layout::default(), &now);
        assert_ne!(
            placement(&session_two),
            placement(&scratch),
            "fixture is not discriminating: rebuilding from nothing already \
             matches, so this would pass with persistence broken"
        );

        for (slot, name) in &kept {
            assert_eq!(
                session_two.slot_of(name),
                Some(slot),
                "{name} did not hold its ground across the restart"
            );
        }
        // The replacements must land on ground that ALREADY EXISTED — reclaiming
        // reserved slots rather than extending the world past its old edge.
        //
        // Not "exactly their predecessors' ordinals": REUSE takes the lowest
        // vacancy in the (zone, pool), and this fixture deliberately carries an
        // older ghost too, so a replacement may honestly land on that one first.
        // Asserting the stricter thing would be asserting the fixture, and it
        // failed here for exactly that reason.
        let widest_before = vacated.iter().copied().max().expect("two departed");
        let ceiling = session_two
            .slots()
            .map(|(k, _)| k.ordinal)
            .max()
            .expect("slots");
        for (slot, occ) in session_two.occupied() {
            if occ.node.starts_with("new-") {
                assert!(
                    slot.ordinal <= ceiling.max(widest_before),
                    "{} was appended past the world's edge instead of reclaiming \
                     reserved ground",
                    occ.node
                );
            }
        }
        assert_eq!(
            session_two.slots().count(),
            restored_slots,
            "the world grew — replacements took new ground rather than reserved ground"
        );
    }

    /// **A CHANGE THAT HAPPENED WHILE THE APP WAS CLOSED MUST STILL READ AS
    /// FRESH.** This is the case that motivates a stamp over a transient
    /// detector, and the reason the field is persisted at all: a refresh
    /// overnight should be visible on the map in the morning.
    #[test]
    fn a_succession_that_happened_while_closed_survives_the_restart() {
        use crate::state::layout::freshness;
        use std::time::Duration;
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        // Session one: `b` holds ground, then is replaced by `b2`.
        let both = assign_layout(&Layout::default(), &[node("a"), node("b")]);
        let drained = assign_layout(&both, &[node("a")]);
        let mut refreshed = assign_layout(&drained, &[node("a"), node("b2")]);
        refreshed.stamp_successions(&drained, t);
        let slot = refreshed.slot_of("b2").expect("placed").clone();
        assert_eq!(refreshed.occupied_at(&slot), Some(t));

        // Close, reopen. The stamp has to come back, or the wave is invisible
        // in exactly the case the operator was not watching it happen.
        let json = serde_json::to_string(&to_stored(&refreshed, None)).expect("write");
        drop(refreshed);
        let (restored, _) =
            from_stored(serde_json::from_str(&json).expect("read"), None).expect("loads");
        assert_eq!(
            restored.occupied_at(&slot),
            Some(t),
            "the succession was forgotten across the restart"
        );

        // And it is still fresh a few minutes later, then not an hour on.
        let hour = Duration::from_secs(3600);
        assert!(
            freshness(
                restored.occupied_at(&slot),
                t + Duration::from_secs(300),
                hour
            )
            .is_some()
        );
        assert!(freshness(restored.occupied_at(&slot), t + hour, hour).is_none());
    }

    /// A file written before the field existed loads with the map intact and
    /// **nothing marked** — an upgrade must not paint the whole world as
    /// freshly changed.
    #[test]
    fn a_file_from_before_the_field_existed_marks_nothing() {
        let mut stored = to_stored(&sample(), None);
        // Exactly what an older writer produced: no `occupied_at` key at all.
        for s in &mut stored.slots {
            s.occupied_at = None;
        }
        let json = serde_json::to_string(&stored).expect("write");
        let stripped = json.replace(",\"occupied_at\":null", "");
        assert!(
            !stripped.contains("occupied_at"),
            "the fixture still has the field"
        );

        let (layout, _) =
            from_stored(serde_json::from_str(&stripped).expect("read"), None).expect("loads");
        assert!(
            layout.occupied().count() > 0,
            "the fixture must have occupied slots or this proves nothing"
        );
        assert!(
            layout
                .occupied()
                .all(|(k, _)| layout.occupied_at(k).is_none()),
            "an older file marked the map as freshly changed"
        );
    }

    /// A slot re-occupied after a restart must not look like it is still vacant.
    #[test]
    fn a_reoccupied_slot_clears_its_vacancy_timestamp() {
        let (loaded, _) = from_stored(to_stored(&sample(), None), None).expect("loads");
        let ghost = loaded.ghosts().next().expect("a ghost").clone();
        assert!(loaded.vacated_at(&ghost).is_some());

        // `b` returns and reclaims its own ground.
        let back = assign_layout(&loaded, &[node("a"), node("b")]);
        assert_eq!(back.ghosts().count(), 0);
        assert_eq!(back.vacated_at(&ghost), None, "stale vacancy timestamp");
    }
}
