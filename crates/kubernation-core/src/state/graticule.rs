//! Naming a position on the map.
//!
//! Workstream A made the map hold still. This makes a place on it *sayable* —
//! "the node in C4" — so the stability can be used in a handover, a ticket, or
//! an annotated screenshot. Stability that cannot be named cannot be exploited.
//!
//! # What a reference is anchored to
//!
//! A reference is `⟨column letter⟩⟨row number⟩`, and it names a **slot**, not a
//! screen position:
//!
//! - the **column** is a zone, lettered from its durable [`zone_ordinal`], which
//!   is assigned once and carried forward — so a zone keeps its letter across a
//!   refresh, a restart, and even its own disappearance;
//! - the **row** is the slot ordinal within that zone, which is zone-wide (A2)
//!   and likewise durable.
//!
//! Both halves come from the layout, so a reference survives everything the
//! workstream made durable. A screen position would not: it moves with the pan,
//! the zoom and the window size.
//!
//! # What a reference does NOT assert
//!
//! **Position means nothing about the cluster.** Columns are ordered by when a
//! zone was first observed — not alphabetically, and not by any real adjacency.
//! Two neighbouring columns are not related; two neighbouring rows are not
//! related. Zone is the only grouping that corresponds to something real (it is
//! the failure domain), and a row number is an allocation order, not a rank.
//!
//! Saying so is load-bearing rather than pedantic: a grid makes positions *look*
//! meaningful, so a map that draws one and stays silent asserts something false
//! by implication. [`FRAME_DECLARATION`] is that statement, and it is rendered
//! wherever the graticule is.
//!
//! # Uniqueness
//!
//! `(zone, ordinal)` identifies at most one slot even though [`SlotKey`] also
//! carries a pool: ordinals are allocated zone-wide (`next_ordinal` takes the
//! max over the whole zone), and reuse only ever re-enters a slot that already
//! exists. Pools therefore interleave within a column rather than each starting
//! their own numbering. `reference_is_unique_across_a_multi_pool_zone` pins it,
//! because the whole scheme collapses if two slots can answer to one name.

use crate::state::layout::{Layout, SlotKey};

/// What the frame is anchored to, in the voice the codebase uses elsewhere for
/// inferred or arbitrary facts (`metric_source`, `CostBasis`, `PoolSource`).
///
/// The last line is the one that matters: without it a grid implies adjacency is
/// meaningful, and node adjacency on this map is an artifact of allocation order.
pub const FRAME_DECLARATION: [&str; 3] = [
    "Columns are zones, in the order first observed - not alphabetical.",
    "Rows are slot ordinals within a zone.",
    "Position asserts nothing: neighbours are unrelated, zone is the only real grouping.",
];

/// A nameable position: a column letter and a row number.
///
/// Comparison is case-insensitive on the column, so an operator typing `c4` and
/// one typing `C4` mean the same slot — a reference is meant to be spoken and
/// retyped, and a case-sensitive one would fail the handover it exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridRef {
    pub column: String,
    pub row: u16,
}

impl std::fmt::Display for GridRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.column, self.row)
    }
}

impl std::str::FromStr for GridRef {
    type Err = ();

    /// Parse `C4`. Rejects anything that is not letters-then-digits, so a
    /// half-typed or mistranscribed reference fails rather than resolving to a
    /// plausible wrong slot.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let split = s.find(|c: char| c.is_ascii_digit()).ok_or(())?;
        let (letters, digits) = s.split_at(split);
        if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(());
        }
        Ok(GridRef {
            column: letters.to_ascii_uppercase(),
            row: digits.parse().map_err(|_| ())?,
        })
    }
}

/// A column of the graticule: one zone's reserved ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub zone: String,
    pub ordinal: u16,
    pub letter: String,
    /// No node is in this zone any more, but its ground stays reserved and its
    /// letter stays taken. The map draws the letter over the empty column so the
    /// reservation is visible — the same reason A2 draws ghost ground instead of
    /// letting a vacated slot read as ocean.
    pub departed: bool,
}

/// Bijective base-26: 0→A, 25→Z, 26→AA, 27→AB, 702→AAA.
///
/// Bijective rather than positional (which would emit A, B … Z, BA — skipping
/// the whole `A_` range and letting `AA` never occur) because the sequence is
/// read by people, and a numbering that silently omits names invites the reader
/// to think they mis-saw one.
pub fn column_letter(ordinal: u16) -> String {
    let mut n = u32::from(ordinal);
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Every column the layout reserves, west to east — **including zones whose
/// nodes have all departed**.
///
/// Departed zones are the reason this reads the layout rather than the observed
/// world: their ordinal is retained so surviving zones do not shift, and their
/// letter must stay taken for the same reason. Verified on the churn fleet —
/// losing z-b left z-c and z-d exactly where they were.
pub fn columns(layout: &Layout, observed_zones: &[&str]) -> Vec<Column> {
    let mut cols: Vec<Column> = layout
        .zone_ordinals()
        .map(|(zone, ordinal)| Column {
            zone: zone.to_string(),
            ordinal,
            letter: column_letter(ordinal),
            departed: !observed_zones.contains(&zone),
        })
        .collect();
    cols.sort_by_key(|c| c.ordinal);
    cols
}

/// The reference for a node, or `None` when it has no durable position.
///
/// `None` is a real answer, not a failure to be papered over: a node can be
/// unplaced (A1 leaves one unplaced rather than handing it an ordinal a live
/// node already holds), and a zone can lack an ordinal. Fabricating `A0` in
/// either case would collide with a real slot's name, which is the worst
/// available failure for a scheme whose entire purpose is unambiguous naming.
pub fn reference_for(layout: &Layout, node: &str) -> Option<GridRef> {
    let key = layout.slot_of(node)?;
    Some(GridRef {
        column: column_letter(layout.zone_ordinal(&key.zone)?),
        row: key.ordinal,
    })
}

/// The slot a reference names, or `None` if nothing answers to it.
///
/// The inverse of [`reference_for`], and the half that makes the gate runnable:
/// one person reads a reference off the map, another resolves it.
pub fn resolve(layout: &Layout, r: &GridRef) -> Option<SlotKey> {
    let zone = layout
        .zone_ordinals()
        .find(|(_, o)| column_letter(*o) == r.column)
        .map(|(z, _)| z.to_string())?;
    layout
        .entries()
        .map(|(k, _)| k)
        .find(|k| k.zone == zone && k.ordinal == r.row)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::layout::{ObservedNode, PoolSource, assign_layout};

    fn node(name: &str, zone: &str, pool: &str) -> ObservedNode {
        ObservedNode {
            name: name.into(),
            zone: zone.into(),
            pool: pool.into(),
            pool_source: PoolSource::Default,
        }
    }

    #[test]
    fn letters_are_bijective_base_26() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        // The positional-numbering bug this avoids: 26 must be AA, not BA.
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(27), "AB");
        assert_eq!(column_letter(51), "AZ");
        assert_eq!(column_letter(52), "BA");
        assert_eq!(column_letter(701), "ZZ");
        assert_eq!(column_letter(702), "AAA");
        // No two ordinals may share a letter, or two zones share a name.
        let mut seen = std::collections::HashSet::new();
        for o in 0..2000u16 {
            assert!(seen.insert(column_letter(o)), "letter collision at {o}");
        }
    }

    #[test]
    fn a_reference_round_trips_through_text_and_back_to_its_slot() {
        let observed = vec![
            node("n0", "z-a", "sys"),
            node("n1", "z-a", "sys"),
            node("n2", "z-b", "sys"),
        ];
        let layout = assign_layout(&Layout::default(), &observed);

        for n in ["n0", "n1", "n2"] {
            let r = reference_for(&layout, n).expect("a placed node has a reference");
            // slot -> reference -> text -> reference -> slot
            let reparsed: GridRef = r.to_string().parse().expect("its own text parses");
            assert_eq!(reparsed, r);
            let slot = resolve(&layout, &reparsed).expect("the reference resolves");
            assert_eq!(layout.slot_of(n), Some(&slot), "{n} round-tripped");
        }
    }

    /// The invariant the whole scheme rests on. Ordinals are zone-wide, so pools
    /// interleave within a column instead of each numbering from zero — if they
    /// did not, `sys/0` and `burst/0` would both be "A0".
    #[test]
    fn reference_is_unique_across_a_multi_pool_zone() {
        let observed: Vec<_> = (0..12)
            .map(|i| {
                let pool = ["sys", "burst", "mem"][i % 3];
                node(&format!("n{i}"), "z-a", pool)
            })
            .chain((0..5).map(|i| node(&format!("m{i}"), "z-b", "sys")))
            .collect();
        let layout = assign_layout(&Layout::default(), &observed);

        let mut seen = std::collections::HashMap::new();
        for (key, _) in layout.entries() {
            let r = GridRef {
                column: column_letter(layout.zone_ordinal(&key.zone).unwrap()),
                row: key.ordinal,
            };
            if let Some(prev) = seen.insert(r.to_string(), key.clone()) {
                panic!("'{r}' names both {prev:?} and {key:?}");
            }
        }
        assert_eq!(seen.len(), 17, "every slot got a distinct name");
    }

    /// §2.3 — the requirement A2 removed one level up, restated for letters.
    #[test]
    fn a_departed_zone_keeps_its_letter_and_neighbours_do_not_shift() {
        let all = vec![
            node("a0", "z-a", "sys"),
            node("b0", "z-b", "sys"),
            node("c0", "z-c", "sys"),
        ];
        let before = assign_layout(&Layout::default(), &all);
        let letters = |l: &Layout| -> Vec<(String, String)> {
            ["z-a", "z-b", "z-c"]
                .iter()
                .filter_map(|z| l.zone_ordinal(z).map(|o| (z.to_string(), column_letter(o))))
                .collect()
        };
        assert_eq!(
            letters(&before),
            vec![
                ("z-a".into(), "A".into()),
                ("z-b".into(), "B".into()),
                ("z-c".into(), "C".into())
            ]
        );

        // z-b loses every node. Verified on the churn fleet: the zone vanishes
        // from the observed world entirely — it does not even leave ghost ground,
        // because ghosts hang off a continent and there is no continent.
        let after = assign_layout(
            &before,
            &[node("a0", "z-a", "sys"), node("c0", "z-c", "sys")],
        );
        assert_eq!(
            letters(&after),
            letters(&before),
            "a departed zone keeps its letter and no neighbour re-letters",
        );

        // And it is reported as a column, so the map can show the reservation
        // rather than leaving an unexplained gap.
        let cols = columns(&after, &["z-a", "z-c"]);
        let b = cols
            .iter()
            .find(|c| c.zone == "z-b")
            .expect("still a column");
        assert!(b.departed);
        assert_eq!(b.letter, "B");
        assert!(cols.iter().filter(|c| !c.departed).count() == 2);
    }

    /// §7 question 2: a reducer over a possibly-absent input must express
    /// unknown, never fabricate. A fabricated `A0` collides with a real slot.
    #[test]
    fn an_unplaceable_node_has_no_reference_rather_than_a_fabricated_one() {
        let layout = assign_layout(&Layout::default(), &[node("n0", "z-a", "sys")]);
        assert!(reference_for(&layout, "n0").is_some());
        assert_eq!(reference_for(&layout, "not-a-node"), None);
        // And nothing answers to a name no slot holds.
        assert_eq!(
            resolve(&layout, &"Z9".parse().unwrap()),
            None,
            "an unheld reference resolves to nothing, not to the nearest slot",
        );
    }

    #[test]
    fn a_malformed_reference_is_refused() {
        for bad in ["", "4", "C", "C4C", "4C", "-1", "C-1", "C 4", "!4"] {
            assert!(
                bad.parse::<GridRef>().is_err(),
                "'{bad}' must not parse into a plausible wrong slot",
            );
        }
        assert_eq!(
            "c4".parse::<GridRef>().unwrap(),
            "C4".parse::<GridRef>().unwrap(),
            "a spoken reference is case-insensitive",
        );
    }
}
