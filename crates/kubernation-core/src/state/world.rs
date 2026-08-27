//! The world: a 2D geography that Kubernetes resources project onto.
//!
//! Zones are continents of solid land separated by ocean; each node is a
//! province (a patch of that land) whose terrain reflects its health.
//! Workloads are cities sited on the province hosting most of their pods,
//! with population badges and name labels. DaemonSets are infrastructure
//! (roads on every province), never cities. Things with no place on the
//! land — custom-resource instances and zero-pod workloads — live on
//! namespace islands in the southern archipelago: abstract resources get
//! abstract geography.
//!
//! Everything here is pure geometry derived from the observed world, so
//! placement stability is unit-testable.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::attention::Severity;
use super::model::{MapModel, NodeTile, WorkloadKind, WorkloadRef, WorkloadRow};
use crate::util::fnv1a64;

pub const PATCH_W: u16 = 26;
const OCEAN_GAP: u16 = 4;
const ISLAND_W: u16 = 22;
const ISLAND_GAP: u16 = 3;
/// Structures shown per island before "+N more".
const ISLAND_CAP: usize = 4;

#[derive(Debug, Clone)]
pub struct City {
    pub r: WorkloadRef,
    pub ready: i32,
    pub desired: i32,
    pub severity: Option<Severity>,
    /// Persistent storage the workload mounts, shown as a granary inland of
    /// the city. `None` when it mounts no PVCs.
    pub storage: Option<CityStorage>,
    /// How many placed pods the workload has, and across how many nodes.
    ///
    /// The city is drawn on the province holding the PLURALITY of those pods,
    /// which at fleet scale is a small minority: measured, a 120-pod workload
    /// spread over 65 nodes has its city on a node holding 5 of them. Carried
    /// here so a surface describing the city can say what it actually stands
    /// for, from the very census that sited it.
    pub spread: CitySpread,
    /// Absolute world cell of the city glyph (label sits on the row below).
    pub x: u16,
    pub y: u16,
}

/// A workload's placed-pod footprint: the fact that makes a city's position
/// interpretable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CitySpread {
    pub pods: usize,
    pub nodes: usize,
}

/// A city's persistent storage at a glance: how many PVCs it mounts and how
/// many of those are not yet Bound (a pending granary flags trouble).
#[derive(Debug, Clone, Copy)]
pub struct CityStorage {
    pub claims: usize,
    pub pending: usize,
}

#[derive(Debug, Clone)]
pub struct Province {
    pub tile: NodeTile,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub cities: Vec<City>,
    /// Where this province's SIZE came from — see [`province_extent`]. A
    /// `Default` province is sized but unmeasured, and must not read as small.
    pub extent_source: crate::state::model::ExtentSource,
    /// This province's graticule reference, e.g. `C4` — how an operator names
    /// it out loud.
    ///
    /// Stored rather than recomputed by each consumer: it comes from
    /// `graticule::reference_for`, the single authority, so the map, the panel
    /// and a positional dump cannot disagree about what a province is called.
    /// `None` when the node has no durable position (A1 leaves a node unplaced
    /// rather than giving it an ordinal a live node holds) — never fabricated,
    /// because a made-up reference names some other node's ground.
    pub reference: Option<crate::state::graticule::GridRef>,
    /// Distinct DaemonSets with pods here — the node's *infrastructure*,
    /// rendered as roads rather than cities. Sorted, so the order is stable.
    ///
    /// Names, not a count: `build_world` computes them anyway, and a count
    /// forces every consumer that wants to SAY what runs on a node to go back
    /// to the store for it. `.len()` covers the render sites that only need a
    /// quantity.
    pub infra: Vec<String>,
}

/// Ground a departed node still holds. The land is there; nothing is on it.
///
/// Deliberately not a `Province`: there is no node, so there is no terrain
/// health, no pressure, no cities and nothing to inspect — and inventing a
/// `NodeTile` to stand in would fabricate exactly the facts the slot no longer
/// has. It carries position only, and the renderer paints it plain. Ageing,
/// ruins and succession are a later phase's vocabulary; a placeholder that
/// looks deliberate is harder to replace than one that looks blank.
#[derive(Debug, Clone)]
pub struct GhostGround {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

#[derive(Debug, Clone)]
pub struct Continent {
    pub zone: String,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub provinces: Vec<Province>,
    /// Slots in this zone whose occupant has departed, still holding their
    /// ground. Without these the reserved ordinal renders as open sea, so a
    /// rolling refresh reads as the continent losing chunks of itself — which
    /// is what the churn flipbook measured before they were drawn.
    pub ghosts: Vec<GhostGround>,
    /// Connectivity markers moored on the east coast: Service harbors and
    /// Ingress gates, each on the row of the city it serves.
    pub coast: Vec<CoastMarker>,
    /// This zone's graticule column letter, from its DURABLE ordinal.
    ///
    /// `None` when the zone has no ordinal — deliberately not the `unwrap_or`
    /// fallback `x` uses. A fabricated letter would collide with a real zone's,
    /// and for a scheme whose only job is unambiguous naming that is the worst
    /// available failure; an unlabelled column merely says so.
    pub column: Option<String>,
}

/// Which connectivity kind a coast marker represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoastKind {
    /// A Service fronting the city — a `⚓` harbor.
    Harbor,
    /// An Ingress routing to the city from outside — a gate.
    Gate,
}

/// A connectivity marker on a continent's east coast (in the ocean strip,
/// on the latitude of the city it serves). Render-only — not a `Region`
/// hit-test variant; the city screen carries the authoritative routing.
#[derive(Debug, Clone)]
pub struct CoastMarker {
    pub kind: CoastKind,
    /// Service or Ingress name.
    pub name: String,
    /// Service type, or the Ingress host.
    pub detail: String,
    /// The city this marker serves.
    pub workload: WorkloadRef,
    pub x: u16,
    pub y: u16,
}

/// A connectivity object (Service or Ingress) tied to the workload it
/// exposes — the input that `build_world` moors as a `CoastMarker`.
#[derive(Debug, Clone)]
pub struct ExposureEntry {
    pub workload: WorkloadRef,
    pub kind: CoastKind,
    pub name: String,
    pub detail: String,
}

/// Per-workload storage tally — the input `build_world` hangs on a city as
/// its `CityStorage` granary.
#[derive(Debug, Clone)]
pub struct StorageEntry {
    pub workload: WorkloadRef,
    pub claims: usize,
    pub pending: usize,
}

/// Something standing on an island: a custom-resource instance (`✦`), an
/// encampment for a workload with no pods on any land (`◌`), or a batch
/// expedition — a Job (`◈`) or CronJob (`◷`).
#[derive(Debug, Clone)]
pub struct Structure {
    pub glyph: char,
    pub kind: String,
    pub name: String,
    /// Status / schedule suffix (e.g. "3/3 ✓", "1 active", a cron schedule).
    /// Empty for customs and encampments.
    pub detail: String,
    /// Trouble (a failed Job) — frontends paint it in the warning colour.
    pub alert: bool,
    /// Set when the structure has a city screen behind it.
    pub workload: Option<WorkloadRef>,
    pub y: u16,
}

/// A batch workload to project as an expedition structure on its namespace
/// island.
#[derive(Debug, Clone)]
pub struct BatchEntry {
    pub kind: BatchKind,
    pub namespace: String,
    pub name: String,
    /// Status (Job) or schedule (CronJob), shown after the name.
    pub detail: String,
    pub alert: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Job,
    CronJob,
}

impl BatchKind {
    /// The island glyph (TUI-safe, single-width).
    pub fn glyph(self) -> char {
        match self {
            BatchKind::Job => '◈',
            BatchKind::CronJob => '◷',
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            BatchKind::Job => "Job",
            BatchKind::CronJob => "CronJob",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Island {
    pub label: String,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub structures: Vec<Structure>,
    pub more: usize,
}

#[derive(Debug, Clone, Default)]
pub struct WorldModel {
    pub width: u16,
    pub height: u16,
    pub continents: Vec<Continent>,
    pub islands: Vec<Island>,
    pub city_count: usize,
    /// Columns whose zone has no nodes left, but whose ground and letter stay
    /// reserved so surviving zones neither move nor re-letter.
    ///
    /// These have no `Continent` at all — verified on the churn fleet, a fully
    /// departed zone leaves not even ghost ground, because ghosts hang off a
    /// continent and there is none. Without this the reservation is an
    /// unexplained gap in the sea; with it the map can say "B is taken".
    pub reserved: Vec<ReservedColumn>,
}

/// A graticule column standing empty: every node in the zone has departed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedColumn {
    pub zone: String,
    pub letter: String,
    /// West edge, in the same world cells a continent's `x` uses.
    pub x: u16,
}

/// A custom-resource instance to project onto the map.
#[derive(Debug, Clone)]
pub struct CustomEntry {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Region<'a> {
    Ocean,
    Province(&'a Province),
    City(&'a Province, &'a City),
    Island(&'a Island),
    Structure(&'a Island, &'a Structure),
}

impl WorldModel {
    /// What stands at a world cell — the explorer's hit test.
    /// Is `(x, y)` within a city's clickable region?
    ///
    /// The settlement is DRAWN centred on its own cell, so the region is that
    /// cell plus a one-cell forgiveness ring — a 3x3 that matches the drawing
    /// (roughly one cell, ~1.5 at the largest tier) and is easy to hit.
    ///
    /// It deliberately does NOT derive from the workload's name. It used to:
    /// the region was 2 rows tall and `name.len() + 2` columns wide, an
    /// ASCII-map leftover from when a city literally WAS its label text. That
    /// made the target up to ~22 cells wide for a long name, so clicking empty
    /// terrain well east of a settlement opened the workload instead of the
    /// node — and the size of the mistake scaled with how long you'd named
    /// things. Honest regression: the target is now much smaller.
    fn city_hit_region(cx: u16, cy: u16, x: u16, y: u16) -> bool {
        x + 1 >= cx && x <= cx + 1 && y + 1 >= cy && y <= cy + 1
    }

    pub fn region_at(&self, x: u16, y: u16) -> Region<'_> {
        for cont in &self.continents {
            if x < cont.x || x >= cont.x + cont.w {
                continue;
            }
            for p in &cont.provinces {
                if y < p.y || y >= p.y + p.h {
                    continue;
                }
                // A settlement's OWN cell outranks a neighbour's forgiveness
                // ring. Without this pass, two cities a cell apart make the
                // second unreachable: the click lands inside the first's ring
                // and the first match wins, so the workload has no cell
                // anywhere on the map that opens it. The ring is a convenience
                // for empty ground, never a claim over occupied ground.
                for c in &p.cities {
                    if c.x == x && c.y == y {
                        return Region::City(p, c);
                    }
                }
                for c in &p.cities {
                    if Self::city_hit_region(c.x, c.y, x, y) {
                        return Region::City(p, c);
                    }
                }
                return Region::Province(p);
            }
        }
        for isl in &self.islands {
            if x < isl.x || x >= isl.x + isl.w || y < isl.y || y >= isl.y + isl.h {
                continue;
            }
            for s in &isl.structures {
                if y == s.y {
                    return Region::Structure(isl, s);
                }
            }
            return Region::Island(isl);
        }
        Region::Ocean
    }

    /// Every city, in a deterministic but **non-geographic** order.
    ///
    /// This read "west→east, north→south", and was true until A2: continents
    /// were laid out at `zone_index * stride` over an alphabetically sorted zone
    /// list, and provinces stacked with `y += h` down `zone.nodes`. A2 moved
    /// both to durable ordinals — first-observed for a zone, layout slot for a
    /// node — while the vectors kept their old sort keys (alphabetical, and
    /// `fnv1a64(name)`). Neither now tracks position: a zone added to an
    /// existing fleet takes the next ordinal, so it sits east of every zone that
    /// sorts after it. Verified — with `z-m` observed before `z-a`, the vector
    /// is `[(z-a, x=30), (z-m, x=0)]`.
    ///
    /// It is still stable, so `]` / `[` cycles every city exactly once with no
    /// flicker; it just is not a geographic sweep. Do not use it to reason about
    /// adjacency. Making the sail genuinely geographic is a design change, not a
    /// fix — see `docs/reports/region-label-ordering.md`.
    pub fn cities(&self) -> impl Iterator<Item = &City> {
        self.continents
            .iter()
            .flat_map(|c| c.provinces.iter())
            .flat_map(|p| p.cities.iter())
    }

    pub fn city_pos(&self, r: &WorkloadRef) -> Option<(u16, u16)> {
        self.cities().find(|c| &c.r == r).map(|c| (c.x, c.y))
    }

    /// A province's model coordinate, nudged two columns in from its western
    /// edge.
    ///
    /// **Not a hit-testable cell.** The view carves a shoreline this cannot see
    /// (core does not consult `Coast` — the v1.3.0 decision), and the inset
    /// routinely exceeds two columns: measured, *every* province on the probe
    /// fixture resolves to open water here. A caller that needs a cell the
    /// resolver agrees is this province wants `draw::province_land_cell`, which
    /// runs the same land test the tooltip does.
    pub fn province_pos(&self, node: &str) -> Option<(u16, u16)> {
        self.continents
            .iter()
            .flat_map(|c| c.provinces.iter())
            .find(|p| p.tile.name == node)
            .map(|p| (p.x + 2, p.y))
    }

    /// The connectivity marker at a world cell, if any — for the GUI hover
    /// tooltip. Coast markers are render-only, so they live outside
    /// `region_at`'s land/island sweep.
    pub fn coast_at(&self, x: u16, y: u16) -> Option<(&Continent, &CoastMarker)> {
        for cont in &self.continents {
            for m in &cont.coast {
                if m.x == x && m.y == y {
                    return Some((cont, m));
                }
            }
        }
        None
    }

    /// Island position of a workload's encampment (a city with no land).
    pub fn structure_pos(&self, r: &WorkloadRef) -> Option<(u16, u16)> {
        for isl in &self.islands {
            for s in &isl.structures {
                if s.workload.as_ref() == Some(r) {
                    return Some((isl.x + 2, s.y));
                }
            }
        }
        None
    }
}

/// City label column inside a province, jittered by a stable hash so the
/// land feels settled rather than gridded.
fn city_dx(name: &str) -> u16 {
    CITY_COL0 + (fnv1a64(name) % CITY_COLS as u64) as u16
}

/// A city's row inside its province, from a stable hash of the workload rather
/// than from its index among siblings.
///
/// The index was the whole of A3's defect. A city's row was `i % rows` over the
/// `WorkloadRef`-sorted sibling list, so anything that changed how many siblings
/// sorted *ahead* of it moved it: adding one unrelated workload to a province
/// moved every incumbent on it by a row (measured 3 of 3, and the inverse on
/// delete), while scaling, node churn, and adding a later-sorting workload moved
/// nothing. Hashing makes the row a property of the workload's identity, so a
/// stranger's arrival cannot shift it.
///
/// Hashed on the FULL ref rather than the bare name: two workloads of different
/// kinds or in different namespaces may share a name, and seeding them
/// identically would put them in the same row for no reason.
///
/// `rows` is `h - 1` for the province's extent class, so it is at least 2 in
/// practice; `max(1)` is here because a modulo by zero is a panic, and a
/// fabricated row would collide with a real cell rather than announce itself.
fn city_dy(r: &WorkloadRef, rows: u16) -> u16 {
    (fnv1a64(&r.to_string()) % u64::from(rows.max(1))) as u16
}

/// Westmost column a settlement may occupy, and how many the preferred band
/// spans. The overflow band is wider but still short of the east shore, where
/// the coast markers moor.
const CITY_COL0: u16 = 2;
const CITY_COLS: u16 = PATCH_W - 16;
const CITY_COLS_WIDE: u16 = PATCH_W - 7;

/// The cell a settlement occupies, guaranteed not to be one already taken.
///
/// The province no longer grows to fit its cities (that was instability source
/// 1 — workload churn resizing terrain), so placement has to FIND a free cell
/// rather than clamp onto the last row. Clamping collided them exactly: two
/// settlements on one cell means the second is painted underneath the first and
/// `region_at`, which returns the first match, can never resolve it — a
/// workload with no clickable cell anywhere on the map. That was the common
/// case, not an edge one: a ~32 GiB node gets the smallest extent, leaving two
/// interior rows for every city on it.
///
/// The preferred cell is tried first, so ordinary provinces look exactly as
/// they did; probing walks the rows of the hash-derived column before moving
/// east, keeping the column as the stable per-name cue. Real city slots are
/// A3's job — this only removes the collision.
fn city_cell(
    cx: u16,
    y: u16,
    rows: u16,
    col0: u16,
    row0: u16,
    taken: &BTreeSet<(u16, u16)>,
) -> (u16, u16) {
    for span in [CITY_COLS, CITY_COLS_WIDE] {
        for dc in 0..span {
            for dr in 0..rows {
                let col = CITY_COL0 + (col0 - CITY_COL0 + dc) % span;
                let cell = (cx + col, y + 1 + (row0 + dr) % rows);
                if !taken.contains(&cell) {
                    return cell;
                }
            }
        }
    }
    // Every cell in the province interior is occupied — more settlements than
    // the smallest extent class can seat (40 on a 32 GiB node). Returning the
    // preferred cell collides, which is why this is documented rather than
    // silent; seating them honestly needs A3's city slots.
    (cx + col0, y + 1 + row0)
}

/// Connectivity markers shown per city before the rest spill (they share
/// the narrow ocean strip east of the continent).
const COAST_CAP: usize = 3;

/// Province extent height by SIZE CLASS, smallest first.
///
/// Quantised rather than mapped linearly from capacity: continuous sizing means
/// a node-type refresh nudges every province by a row or two, while classes are
/// stable across small variation and easier to read at a glance. Odd heights so
/// a city row sits clear of both edges; the smallest matches the old floor of 3.
const EXTENT_CLASSES: [u16; 4] = [3, 5, 7, 9];

/// Memory thresholds (bytes) for the classes above — a node at or above the Nth
/// bound gets the (N+1)th height.
const EXTENT_BOUNDS_GIB: [f64; 3] = [32.0, 128.0, 512.0];

/// How far a node's REPORTED memory runs below its nominal machine size.
///
/// `EXTENT_BOUNDS_GIB` is written in nominal sizes; `node_allocatable` reports
/// what the kubelet publishes, which is always lower — firmware and reserved
/// RAM, plus any kubelet reservation. So the reported figure is scaled up by
/// this headroom before comparison. Otherwise a node sold as 32 GiB reports
/// ~30.9, fails `>= 32.0`, and takes the class BELOW the one the bounds' own
/// doc comment promises it.
///
/// Scaling the value rather than shifting the bounds is deliberate: the bounds
/// stay readable as machine sizes, and the correction sits where the two
/// quantities actually differ, with a name on it. Bounds of `[30, 120, 480]`
/// would encode the same fudge somewhere a later reader rounds back.
///
/// **The firmware term is measured; the reservation term is not.** kind reports
/// 15.653 GiB on a nominal 16 GiB VM (2.2% short) and reserves nothing, as does
/// kwok; managed clouds reserve a tiered fraction on top of that, and no such
/// node has been measured here. 8% is deliberately at the small end of
/// plausible: too small leaves the original defect, too large promotes genuine
/// in-between machines — 24, 96 and 384 GiB are all real instance sizes, and a
/// 24 GiB node would need 33% to be wrongly promoted. The tripwire is a node
/// whose nominal size is known and whose class is wrong.
const EXTENT_HEADROOM: f64 = 0.08;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// The extent a province gets, and which rung of the fallback chain produced it.
///
/// PURE. Allocatable memory (incompressible) → instance type → a DECLARED
/// default. The default is not a silent zero: an unmeasurable node gets a
/// middle-of-the-road extent and `ExtentSource::Default`, because a node that
/// cannot be measured must not render as a genuinely tiny one (v1.6.0).
pub fn province_extent(
    input: &crate::state::model::ExtentInput,
) -> (u16, crate::state::model::ExtentSource) {
    use crate::state::model::{ExtentInput, ExtentSource};
    match input {
        ExtentInput::Allocatable(mem) => {
            let gib = mem / GIB;
            let class = EXTENT_BOUNDS_GIB
                .iter()
                .filter(|b| gib * (1.0 + EXTENT_HEADROOM) >= **b)
                .count();
            (EXTENT_CLASSES[class], ExtentSource::Allocatable)
        }
        // No allocatable memory, but a declared machine size. Coarse — the type string is
        // not parsed into a size — so every instance type shares one class until
        // a table earns its keep. Still better than the default: it is at least
        // evidence the node exists as a real machine.
        ExtentInput::InstanceType(_) => (EXTENT_CLASSES[1], ExtentSource::InstanceType),
        // DECLARED default, and marked. Deliberately not the smallest class.
        ExtentInput::Unknown => (EXTENT_CLASSES[1], ExtentSource::Default),
    }
}

/// A province's y, from its slot ORDINAL rather than from enumerating live
/// provinces.
///
/// Ordinal is multiplied by the LARGEST extent class, so a slot's ground never
/// depends on how big its neighbours are: a node replaced by a bigger one grows
/// into reserved space instead of pushing everything below it down. That costs
/// vertical density and buys the property the whole workstream exists for.
/// Ghost ordinals simply leave their stride empty.
///
/// `None` when the layout holds no slot for this node in this zone. That is not
/// a default to paper over: `assign_layout` deliberately leaves a node UNPLACED
/// when a zone has exhausted its ordinal space, rather than handing it ground a
/// live node already holds — so fabricating an ordinal here (this returned 0,
/// which is a real province's row) would re-introduce one level up exactly the
/// collision the layout engine refused to create.
/// The vertical pitch of one slot, in world rows.
///
/// The largest extent class, so a province of any size fits its own slot without
/// depending on its neighbours' sizes — that independence is what lets a node be
/// replaced by a larger or smaller one without moving anything.
pub const SLOT_STRIDE: u16 = EXTENT_CLASSES[EXTENT_CLASSES.len() - 1];

/// The north edge of a slot's band.
///
/// The ONE place a slot ordinal becomes a world row. The graticule draws its
/// rules here and `province_y` puts land here, so a rule cannot drift off the
/// province tops it is supposed to delimit.
pub fn slot_row(ordinal: u16) -> u16 {
    1 + ordinal * SLOT_STRIDE
}

/// Which slot a world row falls in — the inverse of [`slot_row`].
///
/// Exposed rather than left to each caller to re-derive: the graticule labels
/// bands with this, and a label that disagreed with the reference on the same
/// province would send someone to the wrong node, which is the one failure a
/// naming scheme cannot have.
pub fn slot_of_row(y: u16) -> u16 {
    y.saturating_sub(1) / SLOT_STRIDE
}

fn province_y(layout: &crate::state::layout::Layout, zone: &str, tile: &NodeTile) -> Option<u16> {
    let ordinal = layout
        .slot_of(&tile.name)
        .filter(|k| k.zone == zone)?
        .ordinal;
    Some(slot_row(ordinal))
}

/// The non-node things placed on the map, bundled because they arrive together
/// from the same build and adding a fifth should not be a signature change at
/// every call site (the `OverlayData` precedent).
pub struct Placements<'a> {
    pub customs: &'a [CustomEntry],
    pub exposure: &'a [ExposureEntry],
    pub storage: &'a [StorageEntry],
    pub batch: &'a [BatchEntry],
}

pub fn build_world(
    layout: &crate::state::layout::Layout,
    map: &MapModel,
    workloads: &[WorkloadRow],
    severity: &HashMap<WorkloadRef, Severity>,
    p: Placements<'_>,
) -> WorldModel {
    let Placements {
        customs,
        exposure,
        storage,
        batch,
    } = p;
    // Connectivity grouped by the city it exposes.
    let mut exp_by: HashMap<&WorkloadRef, Vec<&ExposureEntry>> = HashMap::new();
    for e in exposure {
        exp_by.entry(&e.workload).or_default().push(e);
    }
    // Storage tally per city.
    let mut storage_by: HashMap<&WorkloadRef, CityStorage> = HashMap::new();
    for e in storage {
        storage_by.insert(
            &e.workload,
            CityStorage {
                claims: e.claims,
                pending: e.pending,
            },
        );
    }

    // --- Site each city: the province hosting the plurality of its pods.
    // Ties break on stable hash, so the city only migrates when its pods
    // genuinely move. DaemonSets become per-province infrastructure.
    // The (possibly namespace-filtered) workload list is the source of truth
    // for which cities/roads exist — terrain pod census is physical (all
    // namespaces), so siting must be gated on a workload actually being listed,
    // else a filtered-out workload still gets a 0-pop city sited from the map.
    let row_of: HashMap<&WorkloadRef, &WorkloadRow> = workloads.iter().map(|w| (&w.r, w)).collect();
    let mut pods_by_workload_node: HashMap<&WorkloadRef, BTreeMap<&str, usize>> = HashMap::new();
    let mut infra: HashMap<&str, std::collections::BTreeSet<&str>> = HashMap::new();
    for zone in &map.zones {
        for tile in &zone.nodes {
            for pod in &tile.pods {
                let Some(owner) = &pod.owner else { continue };
                if !row_of.contains_key(owner) {
                    continue; // not in the (filtered) workload list
                }
                if owner.kind == WorkloadKind::DaemonSet {
                    infra.entry(&tile.name).or_default().insert(&owner.name);
                } else {
                    *pods_by_workload_node
                        .entry(owner)
                        .or_default()
                        .entry(&tile.name)
                        .or_default() += 1;
                }
            }
        }
    }
    let mut city_home: HashMap<&WorkloadRef, &str> = HashMap::new();
    for (r, by_node) in &pods_by_workload_node {
        let home = by_node
            .iter()
            .max_by_key(|(node, n)| (**n, u64::MAX - fnv1a64(node)))
            .map(|(node, _)| *node);
        if let Some(h) = home {
            city_home.insert(r, h);
        }
    }

    // --- Continents and provinces -------------------------------------
    let mut continents = Vec::new();
    let mut city_count = 0usize;
    let mut max_bottom = 1u16;
    for (zi, zone) in map.zones.iter().enumerate() {
        // Continent x from the zone's DURABLE ordinal, not its index in this
        // build's zone list. Enumeration order meant a zone appearing earlier in
        // the sort shifted every continent east of it (instability source 4);
        // the ordinal is assigned once and carried, so a new zone appends into
        // fresh ocean and a departed one leaves its ground reserved.
        let cx = layout.zone_ordinal(&zone.name).unwrap_or(zi as u16) * (PATCH_W + OCEAN_GAP);
        let mut provinces = Vec::new();
        for tile in &zone.nodes {
            // Cities on this province, stable order.
            let mut cities: Vec<City> = Vec::new();
            for (r, home) in &city_home {
                if *home != tile.name {
                    continue;
                }
                let (ready, desired) = row_of
                    .get(*r)
                    .map(|w| (w.ready, w.desired))
                    .unwrap_or((0, 0));
                // From the SAME census that chose `home`, so the city's
                // position and the footprint it stands for cannot disagree.
                let spread = pods_by_workload_node
                    .get(*r)
                    .map(|by_node| CitySpread {
                        pods: by_node.values().sum(),
                        nodes: by_node.len(),
                    })
                    .unwrap_or_default();
                cities.push(City {
                    r: (*r).clone(),
                    ready,
                    desired,
                    severity: severity.get(r).copied(),
                    storage: storage_by.get(*r).copied(),
                    spread,
                    x: 0,
                    y: 0,
                });
            }
            cities.sort_by(|a, b| a.r.cmp(&b.r));
            // EXTENT FROM CAPACITY, not from workload count. `h` used to be
            // `(2 + 2*cities.len()).max(3)`, so any workload landing on or
            // leaving a node resized its terrain and shifted every province
            // below it — instability source 1.
            let (h, extent_source) = province_extent(&tile.extent);
            // Province y from the slot's ORDINAL, so ghosts leave gaps rather
            // than letting the provinces below slide up. Enumerating live
            // provinces would reintroduce exactly the reshuffle this removes.
            // A node the layout could not seat gets no ground rather than a
            // fabricated row. It stays reachable through the attention queue,
            // the workload table and the advisors — all keyed by name, not by
            // position — which is the honest degrade for a map that has run out
            // of coordinates.
            let Some(y) = province_y(layout, &zone.name, tile) else {
                continue;
            };
            // Both of a city's seeds are now derived from the workload itself —
            // column from its name, row from its full ref — so its cell depends
            // on WHO it is, not on how many siblings happen to sort ahead of it.
            // The probe still resolves an actual collision, and that residual is
            // deliberate: see `city_cell`.
            let rows = h.saturating_sub(1).max(1);
            let mut taken: BTreeSet<(u16, u16)> = BTreeSet::new();
            for c in cities.iter_mut() {
                let cell = city_cell(cx, y, rows, city_dx(&c.r.name), city_dy(&c.r, rows), &taken);
                taken.insert(cell);
                c.x = cell.0;
                c.y = cell.1;
            }
            city_count += cities.len();
            provinces.push(Province {
                tile: tile.clone(),
                x: cx,
                y,
                w: PATCH_W,
                h,
                cities,
                extent_source,
                reference: crate::state::graticule::reference_for(layout, &tile.name),
                infra: infra
                    .get(tile.name.as_str())
                    .map_or_else(Vec::new, |s| s.iter().map(|n| (*n).to_string()).collect()),
            });
            max_bottom = max_bottom.max(y + h);
        }

        // Ground held by this zone's departed nodes. Sized at the DECLARED
        // default extent: the slot remembers who held it, not how big they
        // were, so painting a measured-looking size would be a fabrication —
        // the same reason an unmeasurable node is not drawn as a small one.
        let mut ghosts = Vec::new();
        for k in layout.ghosts().filter(|k| k.zone == zone.name) {
            let gy = 1 + k.ordinal * EXTENT_CLASSES[EXTENT_CLASSES.len() - 1];
            let gh = EXTENT_CLASSES[1];
            ghosts.push(GhostGround {
                x: cx,
                y: gy,
                w: PATCH_W,
                h: gh,
            });
            max_bottom = max_bottom.max(gy + gh);
        }

        // Moor connectivity markers in the ocean strip east of this
        // continent, each on its city's row. Gates sort ahead of harbors so
        // external exposure is never the marker dropped to the cap.
        //
        // Each marker takes a cell no other marker holds. The column used to be
        // the index within ONE city's markers, which was collision-free only
        // while every city had its own row — true when `h` grew to fit them,
        // false since extent came from capacity and cities began sharing rows.
        // Two markers on one cell is worse than two cities on one cell: the
        // painters draw them in order so the LAST one is what you see, while
        // `coast_at` returns the FIRST, and a coast hit opens `m.workload` — so
        // the anchor on screen belongs to one workload and clicking it opens
        // another.
        let mut coast = Vec::new();
        let mut moored: BTreeSet<(u16, u16)> = BTreeSet::new();
        for p in &provinces {
            for c in &p.cities {
                let Some(entries) = exp_by.get(&c.r) else {
                    continue;
                };
                let mut ordered = entries.clone();
                ordered.sort_by(|a, b| {
                    let rank = |k: CoastKind| match k {
                        CoastKind::Gate => 0,
                        CoastKind::Harbor => 1,
                    };
                    rank(a.kind)
                        .cmp(&rank(b.kind))
                        .then_with(|| a.name.cmp(&b.name))
                });
                for e in ordered.into_iter().take(COAST_CAP) {
                    // First free column in the ocean strip on this row. The
                    // strip is OCEAN_GAP wide — beyond it is the next
                    // continent's land — so a row shared by several exposed
                    // cities can run out, and the marker is dropped rather than
                    // stacked or moored on someone else's ground. Gates are
                    // ordered first, so external exposure is never what falls.
                    let Some(x) = (0..OCEAN_GAP)
                        .map(|i| cx + PATCH_W + i)
                        .find(|x| !moored.contains(&(*x, c.y)))
                    else {
                        continue;
                    };
                    moored.insert((x, c.y));
                    coast.push(CoastMarker {
                        kind: e.kind,
                        name: e.name.clone(),
                        detail: e.detail.clone(),
                        workload: c.r.clone(),
                        x,
                        y: c.y,
                    });
                }
            }
        }

        // The continent's northern edge is its TOPMOST PROVINCE, not row 1.
        // While `y` accumulated from 1 those were the same number; now that y
        // comes from the slot ordinal, a zone whose low ordinals are all ghosts
        // has no land anywhere near row 1 — and `Continent.y` is the anchor for
        // the zone label and for the coastline's row zero, so a stale 1 paints
        // the label over open ocean and shapes the shore against the wrong rows.
        let y = provinces
            .iter()
            .map(|p| p.y)
            .chain(ghosts.iter().map(|g| g.y))
            .min()
            .unwrap_or(1);
        continents.push(Continent {
            zone: zone.name.clone(),
            x: cx,
            y,
            w: PATCH_W,
            provinces,
            ghosts,
            coast,
            column: layout
                .zone_ordinal(&zone.name)
                .map(crate::state::graticule::column_letter),
        });
    }

    // --- The southern archipelago: namespace islands -------------------
    // Custom-resource instances plus encampments for workloads that have
    // no pods on any land right now.
    let mut by_island: BTreeMap<String, Vec<Structure>> = BTreeMap::new();
    for c in customs {
        let key = c.namespace.clone().unwrap_or_else(|| "cluster".into());
        by_island.entry(key).or_default().push(Structure {
            glyph: '✦',
            kind: c.kind.clone(),
            name: c.name.clone(),
            detail: String::new(),
            alert: false,
            workload: None,
            y: 0,
        });
    }
    for w in workloads {
        if w.r.kind != WorkloadKind::DaemonSet && !city_home.contains_key(&w.r) {
            by_island
                .entry(w.r.namespace.clone())
                .or_default()
                .push(Structure {
                    glyph: '◌',
                    kind: w.r.kind.to_string(),
                    name: w.r.name.clone(),
                    detail: String::new(),
                    alert: false,
                    workload: Some(w.r.clone()),
                    y: 0,
                });
        }
    }
    // Batch expeditions: Jobs (◈) and CronJobs (◷) on their namespace island.
    for b in batch {
        by_island
            .entry(b.namespace.clone())
            .or_default()
            .push(Structure {
                glyph: b.kind.glyph(),
                kind: b.kind.label().to_string(),
                name: b.name.clone(),
                detail: b.detail.clone(),
                alert: b.alert,
                workload: None,
                y: 0,
            });
    }
    let mut islands = Vec::new();
    let island_y = max_bottom + 2;
    let mut ix = 1u16;
    let mut island_bottom = island_y;
    for (label, mut structures) in by_island {
        structures.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
        let more = structures.len().saturating_sub(ISLAND_CAP);
        structures.truncate(ISLAND_CAP);
        let h = 2 + structures.len() as u16 + u16::from(more > 0);
        for (i, s) in structures.iter_mut().enumerate() {
            s.y = island_y + 1 + i as u16;
        }
        islands.push(Island {
            label,
            x: ix,
            y: island_y,
            w: ISLAND_W,
            h,
            structures,
            more,
        });
        island_bottom = island_bottom.max(island_y + h);
        ix += ISLAND_W + ISLAND_GAP;
    }

    let coast_right = continents
        .iter()
        .flat_map(|c| c.coast.iter())
        .map(|m| m.x + 1)
        .max()
        .unwrap_or(0);
    // The widest continent, not the LAST one. `continents` is pushed in
    // name-sorted zone order (with UNZONED sunk to the end) while `x` now comes
    // from the durable zone ordinal, assigned in first-observed order — so the
    // vector's last entry is no longer the eastmost. A short width puts real
    // land outside `bounds`, where it is painted but cannot be hovered,
    // clicked or framed by `F`.
    let width = continents
        .iter()
        .map(|c| c.x + c.w)
        .max()
        .unwrap_or(PATCH_W)
        .max(islands.last().map(|i| i.x + i.w).unwrap_or(0))
        .max(coast_right)
        + 2;
    let height = if islands.is_empty() {
        max_bottom + 2
    } else {
        island_bottom + 2
    };

    // Zones the layout still reserves but that no longer appear on the map.
    let live: Vec<&str> = map.zones.iter().map(|z| z.name.as_str()).collect();
    let reserved: Vec<ReservedColumn> = crate::state::graticule::columns(layout, &live)
        .into_iter()
        .filter(|c| c.departed)
        .map(|c| ReservedColumn {
            zone: c.zone,
            letter: c.letter,
            x: c.ordinal * (PATCH_W + OCEAN_GAP),
        })
        .collect();

    WorldModel {
        width,
        height,
        continents,
        islands,
        city_count,
        reserved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;

    /// `slot_of_row` inverts `slot_row` across the band, not just at its edge.
    ///
    /// Every row WITHIN a slot's band must report that slot: the graticule
    /// labels a band by the row it draws the label at, and if the two disagreed
    /// the number beside a province would name a different province's slot.
    #[test]
    fn slot_row_and_its_inverse_agree_across_the_whole_band() {
        for ordinal in 0..40u16 {
            let top = slot_row(ordinal);
            assert_eq!(slot_of_row(top), ordinal, "at the top of band {ordinal}");
            for within in 0..SLOT_STRIDE {
                assert_eq!(
                    slot_of_row(top + within),
                    ordinal,
                    "row {} is inside band {ordinal}",
                    top + within,
                );
            }
            // And the row one past the band belongs to the next one.
            assert_eq!(slot_of_row(top + SLOT_STRIDE), ordinal + 1);
        }
    }
    use crate::state::model::Models;

    fn world_with(f: impl FnOnce(&mut fx::Seeds)) -> Models {
        let (world, mut s) = fx::world();
        s.node(fx::node("n-alpha", Some("z-a")));
        s.node(fx::node("n-bravo", Some("z-b")));
        f(&mut s);
        Models::build(&world)
    }

    #[test]
    fn workload_becomes_a_city_on_its_plurality_province() {
        let m = world_with(|s| {
            s.deployment(fx::deployment("demo", "web", 3, 3));
            s.replicaset(fx::replicaset("demo", "web-abc", "web"));
            for (i, node) in ["n-alpha", "n-alpha", "n-bravo"].iter().enumerate() {
                s.pod(fx::pod_owned(
                    fx::pod("demo", &format!("web-abc-{i}"), Some(node)),
                    "ReplicaSet",
                    "web-abc",
                ));
            }
        });
        let w = &m.world;
        assert_eq!(w.city_count, 1);
        let city = w.cities().next().unwrap();
        assert_eq!(city.r.name, "web");
        assert_eq!((city.ready, city.desired), (3, 3));
        assert!(city.storage.is_none(), "web mounts no PVCs → no granary");
        // Plurality is on n-alpha (zone z-a, first continent).
        let (x, _) = w.city_pos(&city.r).unwrap();
        let cont = &w.continents[0];
        assert_eq!(cont.zone, "z-a");
        assert!(x >= cont.x && x < cont.x + cont.w, "city not on z-a");
        // Hit-testing finds the city at its glyph cell and the label row.
        assert!(matches!(
            w.region_at(city.x, city.y),
            Region::City(_, c) if c.r.name == "web"
        ));
        assert!(matches!(w.region_at(city.x, city.y + 1), Region::City(..)));
        // Elsewhere on the patch is the province; far off is ocean.
        assert!(matches!(w.region_at(cont.x, cont.y), Region::Province(_)));
        assert!(matches!(w.region_at(w.width - 1, 0), Region::Ocean));
    }

    /// A city carries the footprint it actually stands for, from the SAME
    /// census that sited it.
    ///
    /// The city is drawn on the plurality node, which at fleet scale holds a
    /// small minority — measured, 5 of 120 pods across 65 nodes. The panel says
    /// so, and it can only say so honestly if this number comes from
    /// `pods_by_workload_node` rather than from anything that merely correlates
    /// with it on a small fixture.
    #[test]
    fn a_city_knows_how_many_pods_and_nodes_it_stands_for() {
        let m = world_with(|s| {
            // desired 3, ready 1: so `ready` cannot stand in for the pod count.
            s.deployment(fx::deployment("demo", "web", 3, 1));
            s.replicaset(fx::replicaset("demo", "web-abc", "web"));
            for (i, node) in ["n-alpha", "n-alpha", "n-bravo"].iter().enumerate() {
                s.pod(fx::pod_owned(
                    fx::pod("demo", &format!("web-abc-{i}"), Some(node)),
                    "ReplicaSet",
                    "web-abc",
                ));
            }
        });
        let city = m.world.cities().next().unwrap();
        assert_eq!(city.spread, CitySpread { pods: 3, nodes: 2 });

        // Guard the guard: the fixture must be able to tell the census apart
        // from the things that would otherwise coincide with it.
        assert_ne!(
            city.spread.pods, city.ready as usize,
            "ready would stand in for the pod count on this fixture"
        );
        assert!(
            city.spread.nodes > 1,
            "a single-node fixture cannot detect a spread that is not measured"
        );
        // And the city sits on the plurality of THAT census, not elsewhere.
        assert!(city.spread.nodes < city.spread.pods, "not actually spread");
    }

    /// The two computations of a workload's footprint agree.
    ///
    /// `build_world` groups pods by node in one pass over the map's tiles (for
    /// siting); `model::workload_pods_by_node` does it per workload from the
    /// observed pods (for the Oracle, which has no `Models`). Merging them would
    /// make siting O(workloads x pods) and blow the 500-node rebuild budget, so
    /// they are separate — and therefore pinned equal here, or the panel and the
    /// bundle could report different footprints for the same workload.
    #[test]
    fn the_map_and_the_observed_world_agree_about_a_workloads_footprint() {
        let (obs, mut st) = fx::world();
        for n in ["n-alpha", "n-bravo", "n-charlie"] {
            st.node(fx::node(n, Some("z-a")));
        }
        st.deployment(fx::deployment("demo", "web", 4, 4));
        st.replicaset(fx::replicaset("demo", "web-abc", "web"));
        for (i, node) in ["n-alpha", "n-alpha", "n-bravo", "n-charlie"]
            .iter()
            .enumerate()
        {
            st.pod(fx::pod_owned(
                fx::pod("demo", &format!("web-abc-{i}"), Some(node)),
                "ReplicaSet",
                "web-abc",
            ));
        }
        // An unschedulable pod: it belongs to the workload but is nowhere, so
        // neither computation may count it.
        st.pod(fx::pod_owned(
            fx::pod("demo", "web-abc-pending", None),
            "ReplicaSet",
            "web-abc",
        ));

        let m = Models::build(&obs);
        let city = m.world.cities().next().expect("a city");
        let by_node = crate::state::model::workload_pods_by_node(&obs, &city.r);

        assert_eq!(city.spread.pods, by_node.values().sum::<usize>());
        assert_eq!(city.spread.nodes, by_node.len());
        // Guard the guard: the fixture must be spread, or agreement is trivial.
        assert_eq!(city.spread, CitySpread { pods: 4, nodes: 3 });
        assert!(
            !by_node.contains_key(""),
            "an unplaced pod was counted onto an empty node name"
        );
    }

    /// The city's clickable region must match what is DRAWN — its own cell plus
    /// a one-cell forgiveness ring — and must NOT scale with the workload's
    /// name. It used to be `name.len() + 2` cells wide, so a long name silently
    /// stole terrain from the node underneath it.
    #[test]
    fn city_hit_region_matches_the_settlement_not_its_name() {
        // A deliberately long name: under the old rule this reserved 22+ cells.
        let long = "a-very-long-workload-name";
        let m = world_with(|s| {
            s.deployment(fx::deployment("demo", long, 1, 1));
            s.replicaset(fx::replicaset("demo", "rs", long));
            s.pod(fx::pod_owned(
                fx::pod("demo", "rs-1", Some("n-alpha")),
                "ReplicaSet",
                "rs",
            ));
        });
        let w = &m.world;
        let city = w.cities().next().expect("a city").clone();
        // The settlement's own cell resolves to the city…
        assert!(matches!(w.region_at(city.x, city.y), Region::City(..)));
        // …and so does every cell of the forgiveness ring THAT LIES INSIDE THE
        // PROVINCE. The clip is not incidental: `region_at` finds the province
        // by y-range first, so a ring cell below a city on the province's last
        // row belongs to the next province's band — extending the ring across
        // that boundary would let a city claim ground on a neighbouring node,
        // which is worse than a slightly smaller target. Cities sit on hashed
        // rows now, so the edge rows are ordinary rather than rare, and this
        // asserts the invariant rather than one fixture's happening row.
        let prov = w
            .continents
            .iter()
            .flat_map(|c| &c.provinces)
            .find(|p| p.cities.iter().any(|c| c.r == city.r))
            .expect("the city's province");
        let mut ring_hits = 0;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (rx, ry) = (city.x as i32 + dx, city.y as i32 + dy);
                if ry < prov.y as i32 || ry >= (prov.y + prov.h) as i32 {
                    continue; // outside the province — not this city's to claim
                }
                assert!(
                    matches!(w.region_at(rx as u16, ry as u16), Region::City(..)),
                    "ring cell ({rx}, {ry}) inside the province does not resolve to the city"
                );
                ring_hits += 1;
            }
        }
        assert!(ring_hits >= 6, "the ring should cover at least two rows");
        // But a cell well east on the same row is the PROVINCE (the node), not
        // the city — the whole point of the fix.
        assert!(
            matches!(w.region_at(city.x + 10, city.y), Region::Province(_)),
            "10 cells east of a {}-char name must be the node, not the city",
            long.len()
        );
        // And the name's length must not move that boundary: two cells out is
        // already province, however long the name is.
        assert!(matches!(
            w.region_at(city.x + 2, city.y),
            Region::Province(_)
        ));
    }

    #[test]
    fn placement_is_stable_across_rebuilds() {
        let build = || {
            let m = world_with(|s| {
                s.deployment(fx::deployment("demo", "web", 2, 2));
                s.replicaset(fx::replicaset("demo", "web-abc", "web"));
                s.pod(fx::pod_owned(
                    fx::pod("demo", "web-abc-1", Some("n-alpha")),
                    "ReplicaSet",
                    "web-abc",
                ));
                s.pod(fx::pod_owned(
                    fx::pod("demo", "web-abc-2", Some("n-bravo")),
                    "ReplicaSet",
                    "web-abc",
                ));
            });
            m.world
                .city_pos(&m.world.cities().next().unwrap().r.clone())
        };
        assert_eq!(build(), build(), "tie-broken placement must not wander");
    }

    #[test]
    fn daemonsets_are_infrastructure_not_cities() {
        let m = world_with(|s| {
            s.daemonset(fx::daemonset("demo", "agent", 2, 2));
            for (i, node) in ["n-alpha", "n-bravo"].iter().enumerate() {
                s.pod(fx::pod_owned(
                    fx::pod("demo", &format!("agent-{i}"), Some(node)),
                    "DaemonSet",
                    "agent",
                ));
            }
        });
        assert_eq!(m.world.city_count, 0);
        let p = &m.world.continents[0].provinces[0];
        assert_eq!(
            p.infra.len(),
            1,
            "daemonset should pave roads on the province"
        );
        assert_eq!(
            p.infra,
            vec!["agent"],
            "and it should be NAMED, not counted"
        );
    }

    #[test]
    fn mounted_pvcs_give_a_city_a_granary() {
        let m = world_with(|s| {
            s.deployment(fx::deployment("demo", "web", 1, 1));
            s.replicaset(fx::replicaset("demo", "web-abc", "web"));
            let mut pod =
                fx::pod_with_pvc(fx::pod("demo", "web-abc-1", Some("n-alpha")), "web-data");
            pod = fx::pod_with_pvc(pod, "web-cache");
            s.pod(fx::pod_owned(pod, "ReplicaSet", "web-abc"));
            s.pvc(fx::pvc("demo", "web-data", "Bound"));
            s.pvc(fx::pvc("demo", "web-cache", "Pending"));
        });
        let city = m.world.cities().next().expect("web city");
        let st = city.storage.expect("web mounts PVCs → a granary");
        assert_eq!(st.claims, 2);
        assert_eq!(st.pending, 1, "web-cache is unbound");
    }

    #[test]
    fn services_and_ingresses_moor_on_the_city_coast() {
        let m = world_with(|s| {
            s.deployment(fx::deployment("demo", "web", 2, 2));
            s.replicaset(fx::replicaset("demo", "web-abc", "web"));
            s.pod(fx::pod_owned(
                fx::pod("demo", "web-abc-1", Some("n-alpha")),
                "ReplicaSet",
                "web-abc",
            ));
            s.service(fx::service("demo", "web", &[("app", "web")]));
            s.ingress(fx::ingress("demo", "web-ing", "web.example.com", "web"));
        });
        let w = &m.world;
        let city = w.cities().next().expect("web city");
        let cont = &w.continents[0];
        assert_eq!(cont.zone, "z-a");
        let on_row: Vec<&CoastMarker> = cont.coast.iter().filter(|m| m.y == city.y).collect();
        assert!(
            on_row
                .iter()
                .any(|m| m.kind == CoastKind::Harbor && m.name == "web"),
            "missing Service harbor: {:?}",
            cont.coast
        );
        assert!(
            on_row
                .iter()
                .any(|m| m.kind == CoastKind::Gate && m.name == "web-ing"),
            "missing Ingress gate: {:?}",
            cont.coast
        );
        // Markers float in the ocean strip east of the land, on the city's
        // latitude — and are discoverable by the hover hit-test.
        for m in &on_row {
            assert!(m.x >= cont.x + PATCH_W, "marker not offshore: {m:?}");
            assert!(w.coast_at(m.x, m.y).is_some(), "coast_at misses {m:?}");
        }
    }

    #[test]
    fn batch_workloads_become_island_expeditions() {
        let m = world_with(|s| {
            s.job(fx::job("demo", "migrate", 3, 3, 0, 0)); // completed
            s.job(fx::job("demo", "backup", 1, 0, 0, 2)); // failed
            s.cronjob(fx::cronjob("demo", "nightly", "0 2 * * *", false));
        });
        let island = m
            .world
            .islands
            .iter()
            .find(|i| i.label == "demo")
            .expect("demo island");
        let job = island
            .structures
            .iter()
            .find(|s| s.name == "migrate")
            .expect("migrate job");
        assert_eq!(job.glyph, '◈');
        assert!(job.detail.contains("3/3"), "detail: {}", job.detail);
        assert!(!job.alert);
        let failed = island
            .structures
            .iter()
            .find(|s| s.name == "backup")
            .expect("backup job");
        assert!(failed.alert, "a failed job raises alert");
        let cron = island
            .structures
            .iter()
            .find(|s| s.name == "nightly")
            .expect("nightly cronjob");
        assert_eq!(cron.glyph, '◷');
        assert!(cron.detail.contains("0 2 * * *"), "detail: {}", cron.detail);
    }

    #[test]
    fn placeless_things_live_on_namespace_islands() {
        let m = world_with(|s| {
            // A workload with desired replicas but no pods anywhere.
            s.deployment(fx::deployment("demo", "ghost", 2, 0));
        });
        // The fixture world has no customs; build_world is exercised via
        // Models with an extra custom entry here.
        let customs = vec![CustomEntry {
            kind: "gizmo".into(),
            namespace: Some("demo".into()),
            name: "frobnicator".into(),
        }];
        let w = build_world(
            &m.layout,
            &m.map,
            &m.workloads,
            &m.workload_severity,
            Placements {
                customs: &customs,
                exposure: &[],
                storage: &[],
                batch: &[],
            },
        );
        let island = w
            .islands
            .iter()
            .find(|i| i.label == "demo")
            .expect("demo island");
        let glyphs: Vec<char> = island.structures.iter().map(|s| s.glyph).collect();
        assert!(glyphs.contains(&'✦'), "custom resource missing: {glyphs:?}");
        assert!(
            glyphs.contains(&'◌'),
            "ghost encampment missing: {glyphs:?}"
        );
        // The encampment opens the workload's city screen.
        let ghost = island.structures.iter().find(|s| s.glyph == '◌').unwrap();
        assert_eq!(ghost.workload.as_ref().unwrap().name, "ghost");
        // Structures are hit-testable rows.
        assert!(matches!(
            w.region_at(island.x + 1, ghost.y),
            Region::Structure(..)
        ));
    }
    use crate::state::filter::NamespaceFilter;
    use crate::state::observed::ObservedWorld;

    // --- A2: positions come from the layout --------------------------------

    fn a2_world(nodes: &[(&str, &str)], cities: &[(&str, &str)]) -> ObservedWorld {
        let (world, mut s) = fx::world();
        for (n, z) in nodes {
            s.node(fx::node(n, Some(z)));
        }
        for (ns, name) in cities {
            s.deployment(fx::deployment(ns, name, 1, 1));
            s.replicaset(fx::replicaset(ns, &format!("{name}-rs"), name));
            s.pod(fx::pod_owned(
                fx::pod(ns, &format!("{name}-rs-1"), Some(nodes[0].0)),
                "ReplicaSet",
                &format!("{name}-rs"),
            ));
        }
        world
    }

    fn pos(w: &WorldModel, node: &str) -> Option<(u16, u16, u16, u16)> {
        w.continents
            .iter()
            .flat_map(|c| &c.provinces)
            .find(|p| p.tile.name == node)
            .map(|p| (p.x, p.y, p.w, p.h))
    }

    /// A node gaining an extent class must not move a city to another province.
    ///
    /// `EXTENT_HEADROOM` promotes nodes across class boundaries, and `Province.h`
    /// feeds `rows`, which A3's `city_dy` hashes into modulo. So a class change
    /// DOES move a city within its province — that is the design, and this pins
    /// that it is the only thing it does. `SLOT_STRIDE` is the largest class, so
    /// no province moves either (v1.7.0's stride claim, exercised from the
    /// consumer side rather than restated).
    #[test]
    fn a_class_change_keeps_a_city_on_its_own_province() {
        use crate::state::fixtures as fx;
        let build = |mem: &str| {
            let (world, mut s) = fx::world();
            let mut n = fx::node("big", Some("z-a"));
            n.status.as_mut().unwrap().allocatable =
                Some(fx::quantities(&[("cpu", "8"), ("memory", mem)]));
            s.node(n);
            s.deployment(fx::deployment("demo", "app", 1, 1));
            s.replicaset(fx::replicaset("demo", "app-rs", "app"));
            s.pod(fx::pod_owned(
                fx::pod("demo", "app-rs-1", Some("big")),
                "ReplicaSet",
                "app-rs",
            ));
            Models::build(&world)
        };

        // Either side of the 32 GiB bound, as the headroom now draws it.
        let small = build("24Gi");
        let large = build("30900Mi");

        let prov = |m: &Models| {
            m.world
                .continents
                .iter()
                .flat_map(|c| &c.provinces)
                .find(|p| p.tile.name == "big")
                .expect("the province")
                .clone()
        };
        let (a, b) = (prov(&small), prov(&large));
        assert!(b.h > a.h, "the larger node should have gained a class");
        assert_eq!((a.x, a.y), (b.x, b.y), "the province itself must not move");

        // The city stays on THIS province, on both sides of the boundary.
        for p in [&a, &b] {
            let c = p.cities.first().expect("app is sited here");
            assert!(
                c.y >= p.y && c.y < p.y + p.h,
                "city at {} escaped province rows {}..{}",
                c.y,
                p.y,
                p.y + p.h
            );
            assert!(
                c.x >= p.x && c.x < p.x + p.w,
                "city escaped the province columns"
            );
        }
    }

    /// THE EXTENT CLAIM: adding a workload to a node must not resize its terrain
    /// or move anything. `h` used to be `(2 + 2*cities.len()).max(3)`, so every
    /// workload landing on a node shifted every province below it.
    #[test]
    fn adding_a_workload_moves_no_province_and_resizes_none() {
        let before = Models::build(&a2_world(
            &[("n1", "z-a"), ("n2", "z-a"), ("n3", "z-b")],
            &[("demo", "one")],
        ));
        let after = Models::build(&a2_world(
            &[("n1", "z-a"), ("n2", "z-a"), ("n3", "z-b")],
            &[("demo", "one"), ("demo", "two"), ("demo", "three")],
        ));
        for n in ["n1", "n2", "n3"] {
            assert_eq!(
                pos(&before.world, n),
                pos(&after.world, n),
                "{n} moved or resized when a workload was added"
            );
        }
    }

    /// NO TWO PROVINCES SHARE GROUND — the multi-pool case.
    ///
    /// The ordinal is a ZONE-wide row index precisely so this holds. When it was
    /// per-(zone, pool), each pool restarted at 0 and `province_y` — which reads
    /// the ordinal alone — drew the Nth node of every pool on the same cell. On
    /// a four-pool 100-node fleet that hid 42 nodes underneath each other:
    /// invisible, unclickable, and silent, because the map still looked like a
    /// map. Every fixture before this one was single-pool, so both behaviours
    /// were identical under test and the mutation floor could not reach it.
    #[test]
    fn provinces_of_different_pools_in_one_zone_never_share_ground() {
        let (world, mut s) = fx::world();
        for (n, pool) in [
            ("sys-a", "sys"),
            ("sys-b", "sys"),
            ("app-a", "app"),
            ("app-b", "app"),
            ("edge-a", "edge"),
        ] {
            s.node(fx::node_in_pool(fx::node(n, Some("z-a")), pool));
        }
        let m = Models::build(&world);

        let mut seen: BTreeMap<(u16, u16), &str> = BTreeMap::new();
        for p in m.world.continents.iter().flat_map(|c| &c.provinces) {
            if let Some(other) = seen.insert((p.x, p.y), p.tile.name.as_str()) {
                panic!(
                    "{} and {} are drawn on the same ground at ({}, {})",
                    other, p.tile.name, p.x, p.y
                );
            }
        }
        assert_eq!(seen.len(), 5, "every node got its own ground");

        // And the ground is genuinely reachable, not merely distinct: the y
        // stride must exceed each province's own height or they overlap.
        for p in m.world.continents.iter().flat_map(|c| &c.provinces) {
            let overlaps = m
                .world
                .continents
                .iter()
                .flat_map(|c| &c.provinces)
                .any(|q| {
                    q.tile.name != p.tile.name && q.x == p.x && q.y < p.y + p.h && p.y < q.y + q.h
                });
            assert!(!overlaps, "{} overlaps a neighbour vertically", p.tile.name);
        }
    }

    /// A NODE WITH NO SLOT GETS NO GROUND — it does not get row 1.
    ///
    /// `assign_layout` deliberately leaves a node unplaced when a zone has
    /// exhausted its ordinals, rather than handing it ground a live node holds.
    /// `province_y` used to answer that with `map_or(0, ..)`, fabricating one
    /// level up the very coordinate the layout engine had refused to invent —
    /// and ordinal 0 is a real province's row, so the two would be stacked.
    #[test]
    fn a_node_the_layout_never_seated_is_not_stacked_on_ordinal_zero() {
        let observed = a2_world(&[("a", "z-a"), ("b", "z-a"), ("c", "z-a")], &[]);
        let m = Models::build(&observed);

        // An EMPTY layout stands in for the exhausted one: every node is
        // unplaced, which is the same question asked of every tile at once.
        let empty = crate::state::layout::Layout::default();
        let w = build_world(
            &empty,
            &m.map,
            &[],
            &HashMap::new(),
            Placements {
                customs: &[],
                exposure: &[],
                storage: &[],
                batch: &[],
            },
        );
        let placed: Vec<&str> = w
            .continents
            .iter()
            .flat_map(|c| &c.provinces)
            .map(|p| p.tile.name.as_str())
            .collect();
        assert!(
            placed.is_empty(),
            "unseated nodes were given ground anyway: {placed:?}"
        );
    }

    /// NO TWO CITIES SHARE A CELL.
    ///
    /// `h` used to grow as `2 + 2*cities.len()`, so the rows could never run
    /// out; A2 fixed it from capacity and clamped the overflow onto the last
    /// row, which stacked settlements exactly. A stacked city is painted
    /// underneath its neighbour and `region_at` returns the first match, so it
    /// has no clickable cell anywhere on the map — and its coast markers moor on
    /// the shared row too. A ~32 GiB node takes the smallest extent, so this is
    /// the ordinary case rather than an edge one.
    #[test]
    fn cities_on_one_small_province_never_share_a_cell() {
        let (world, mut s) = fx::world();
        s.node(fx::node("only", Some("z-a"))); // 8Gi fixture → smallest extent
        for name in ["a-app", "b-app", "c-app", "d-app", "e-app", "f-app"] {
            s.deployment(fx::deployment("demo", name, 1, 1));
            s.replicaset(fx::replicaset("demo", &format!("{name}-rs"), name));
            s.pod(fx::pod_owned(
                fx::pod("demo", &format!("{name}-rs-1"), Some("only")),
                "ReplicaSet",
                &format!("{name}-rs"),
            ));
        }
        let m = Models::build(&world);
        let prov = &m.world.continents[0].provinces[0];
        assert_eq!(prov.cities.len(), 6, "all six settled");

        let mut seen: BTreeMap<(u16, u16), &str> = BTreeMap::new();
        for c in &prov.cities {
            if let Some(other) = seen.insert((c.x, c.y), c.r.name.as_str()) {
                panic!("{} and {} share cell ({}, {})", other, c.r.name, c.x, c.y);
            }
        }
        // Every one is reachable through the model's own hit-test.
        for c in &prov.cities {
            match m.world.region_at(c.x, c.y) {
                Region::City(_, hit) => assert_eq!(
                    hit.r.name, c.r.name,
                    "clicking {}'s cell resolves to {}",
                    c.r.name, hit.r.name
                ),
                other => panic!("{} does not hit-test as a city: {other:?}", c.r.name),
            }
        }
    }

    /// A province with several cities on one node, for the sibling-order tests.
    /// `names` are Deployments in `demo`, all pinned to the one node.
    fn crowded(names: &[&str]) -> ObservedWorld {
        let (world, mut s) = fx::world();
        s.node(fx::node("only", Some("z-a")));
        for name in names {
            s.deployment(fx::deployment("demo", name, 1, 1));
            s.replicaset(fx::replicaset("demo", &format!("{name}-rs"), name));
            s.pod(fx::pod_owned(
                fx::pod("demo", &format!("{name}-rs-1"), Some("only")),
                "ReplicaSet",
                &format!("{name}-rs"),
            ));
        }
        world
    }

    /// Every city's cell, keyed by workload name.
    fn cells(w: &WorldModel) -> BTreeMap<String, (u16, u16)> {
        w.continents
            .iter()
            .flat_map(|c| &c.provinces)
            .flat_map(|p| &p.cities)
            .map(|c| (c.r.name.clone(), (c.x, c.y)))
            .collect()
    }

    /// **A3's GATE, as a unit test.** Adding a workload that sorts AHEAD of the
    /// incumbents on a province must move none of them.
    ///
    /// The row used to be `i % rows` over the `WorkloadRef`-sorted sibling list,
    /// so an insertion shifted every later index and every incumbent with it —
    /// measured on the churn fleet as 3 of 3, each by exactly one row.
    #[test]
    fn adding_a_workload_that_sorts_first_moves_no_incumbent() {
        let before = cells(&Models::build(&crowded(&["m-one", "m-two", "m-three"])).world);
        let after =
            cells(&Models::build(&crowded(&["a-newcomer", "m-one", "m-two", "m-three"])).world);
        assert_eq!(before.len(), 3);
        assert_eq!(after.len(), 4);
        for (name, cell) in &before {
            assert_eq!(
                after.get(name),
                Some(cell),
                "{name} moved when an unrelated workload was added"
            );
        }
    }

    /// The inverse: removing a workload that sorts ahead must move no survivor.
    #[test]
    fn removing_a_workload_that_sorts_first_moves_no_survivor() {
        let before = cells(&Models::build(&crowded(&["a-newcomer", "m-one", "m-two"])).world);
        let after = cells(&Models::build(&crowded(&["m-one", "m-two"])).world);
        assert_eq!(after.len(), 2);
        for (name, cell) in &after {
            assert_eq!(
                before.get(name),
                Some(cell),
                "{name} moved when a sibling left"
            );
        }
    }

    /// PLACEMENT IS A PURE FUNCTION OF THE CITY SET — not of arrival order.
    ///
    /// The seeds are hashes of the workload, and the probe walks a `taken` set
    /// in `WorkloadRef` order, which `build_world` sorts. So the same set
    /// presented in a different order must produce identical cells.
    #[test]
    fn the_same_workloads_land_identically_whatever_order_they_arrive_in() {
        let a = cells(&Models::build(&crowded(&["alpha", "beta", "gamma", "delta"])).world);
        let b = cells(&Models::build(&crowded(&["delta", "gamma", "beta", "alpha"])).world);
        assert_eq!(a.len(), 4);
        assert_eq!(a, b);
    }

    /// The row seed hashes the FULL ref, so a name shared across namespaces does
    /// not force two workloads onto the same row for no reason.
    #[test]
    fn the_same_name_in_two_namespaces_seeds_differently() {
        let r = |ns: &str| WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: ns.into(),
            name: "web".into(),
        };
        let rows = 4;
        assert_ne!(
            city_dy(&r("alpha"), rows),
            city_dy(&r("beta"), rows),
            "a bare-name seed would have collided here"
        );
    }

    /// A one-row province is a real input: every city seeds to row 0 and the
    /// column probe has to do the separating. It must not divide by zero.
    #[test]
    fn a_single_row_province_places_every_city_by_column() {
        let r = |n: &str| WorkloadRef {
            kind: WorkloadKind::Deployment,
            namespace: "demo".into(),
            name: n.into(),
        };
        for n in ["a", "b", "c"] {
            assert_eq!(city_dy(&r(n), 1), 0);
        }
        assert_eq!(city_dy(&r("a"), 0), 0, "rows=0 must not panic");
    }

    /// THE RESIDUAL, SIZED. Hashing removes the *index* dependency; it does not
    /// remove the *collision* dependency, and this pins how much is left.
    ///
    /// Two cities can still hash to one cell, and the probe then resolves them
    /// in `WorkloadRef` order — so a newcomer that both sorts ahead of an
    /// incumbent AND collides with it still displaces it. That is bounded to
    /// actual collisions rather than to every insertion, which is the whole
    /// improvement: before, ONE arrival moved EVERY incumbent.
    ///
    /// The decision (A3 §2.1) is to accept that residual rather than reserve
    /// per-city slots in the layout store, and this test is the evidence behind
    /// it. If a future change makes collisions common, this fires.
    #[test]
    fn cell_collisions_stay_rare_enough_to_accept() {
        // A 4-row province (the declared-default extent) with six cities is a
        // crowded but realistic one. Names come from a fixed LCG rather than a
        // counter, and that is load-bearing: FNV-1a's low bits advance by a
        // constant for names differing only in a trailing character
        // (`PRIME % CITY_COLS == 1`), so `svc-0`..`svc-5` take six CONSECUTIVE
        // columns and cannot collide at all. Written that way this test
        // measured a 0% collision rate — a property of the name generator, not
        // of the placement.
        const ROWS: u16 = 4;
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut displaced = 0usize;
        let mut total = 0usize;
        for _ in 0..200u32 {
            let names: Vec<String> = (0..6).map(|_| format!("w{:x}", next())).collect();
            let mut taken: BTreeSet<(u16, u16)> = BTreeSet::new();
            let mut refs: Vec<WorkloadRef> = names
                .iter()
                .map(|n| WorkloadRef {
                    kind: WorkloadKind::Deployment,
                    namespace: "demo".into(),
                    name: n.clone(),
                })
                .collect();
            refs.sort();
            for r in &refs {
                let want = (city_dx(&r.name), city_dy(r, ROWS));
                let got = city_cell(0, 0, ROWS, want.0, want.1, &taken);
                taken.insert(got);
                total += 1;
                if got != (want.0, 1 + want.1) {
                    displaced += 1;
                }
            }
        }
        let pct = 100.0 * displaced as f64 / total as f64;
        assert!(
            pct < 20.0,
            "{displaced} of {total} cities ({pct:.1}%) were displaced from their \
             hashed cell — collisions are no longer rare, so §2.1's decision to \
             accept the residual instead of reserving slots needs revisiting"
        );
    }

    /// COAST MARKERS SURVIVE THE HASHED ROWS.
    ///
    /// The §4 consumer question. Markers moor on their city's row and take a
    /// free column in the ocean strip, dropping when the row fills — so a row
    /// seed that CLUSTERS cities onto fewer distinct rows would drop more of
    /// them. Hashing does cluster more than the old round-robin index did
    /// (independent draws versus a perfect spread), so this checks the
    /// consequence rather than assuming it away.
    #[test]
    fn hashed_rows_do_not_cost_coast_markers() {
        let (world, mut s) = fx::world();
        s.node(fx::node("only", Some("z-a"))); // smallest extent → two city rows
        let names = ["a-app", "b-app", "c-app", "d-app"];
        for name in names {
            s.deployment(fx::deployment("demo", name, 1, 1));
            s.replicaset(fx::replicaset("demo", &format!("{name}-rs"), name));
            let mut pod = fx::pod_owned(
                fx::pod("demo", &format!("{name}-rs-1"), Some("only")),
                "ReplicaSet",
                &format!("{name}-rs"),
            );
            pod.metadata
                .labels
                .get_or_insert_with(Default::default)
                .insert("app".into(), name.into());
            s.pod(pod);
            s.service(fx::service(
                "demo",
                &format!("{name}-svc"),
                &[("app", name)],
            ));
        }
        let m = Models::build(&world);
        let cont = &m.world.continents[0];
        // Every exposed city keeps a marker: four cities, four harbours. The
        // strip is OCEAN_GAP wide, so this only fails if the rows cluster hard
        // enough to put more than OCEAN_GAP cities on one row.
        assert_eq!(
            cont.coast.len(),
            names.len(),
            "a marker was dropped — the hashed rows crowded the ocean strip"
        );
        let mut seen: BTreeSet<(u16, u16)> = BTreeSet::new();
        for mk in &cont.coast {
            assert!(seen.insert((mk.x, mk.y)), "two markers share a cell");
        }
    }

    /// NO TWO COAST MARKERS SHARE A CELL EITHER.
    ///
    /// Distinct city cells are not enough. A marker's column used to be its
    /// index within ONE city's markers, which never collided while every city
    /// had its own row — guaranteed when `h` grew to fit them, and no longer
    /// true since extent came from capacity. Two markers on a cell is the worse
    /// half of the same defect: painters draw in order so the LAST is visible,
    /// `coast_at` returns the FIRST, and a coast hit opens `m.workload` — so the
    /// anchor you see belongs to one workload and clicking it opens another.
    #[test]
    fn coast_markers_of_cities_sharing_a_row_never_share_a_cell() {
        let (world, mut s) = fx::world();
        s.node(fx::node("only", Some("z-a"))); // smallest extent → 2 city rows
        for name in ["a-app", "b-app", "c-app", "d-app"] {
            s.deployment(fx::deployment("demo", name, 1, 1));
            s.replicaset(fx::replicaset("demo", &format!("{name}-rs"), name));
            let mut pod = fx::pod_owned(
                fx::pod("demo", &format!("{name}-rs-1"), Some("only")),
                "ReplicaSet",
                &format!("{name}-rs"),
            );
            pod.metadata
                .labels
                .get_or_insert_with(Default::default)
                .insert("app".into(), name.into());
            s.pod(pod);
            // Each workload is fronted by a Service, so each wants a harbour.
            s.service(fx::service(
                "demo",
                &format!("{name}-svc"),
                &[("app", name)],
            ));
        }
        let m = Models::build(&world);
        let cont = &m.world.continents[0];
        assert!(cont.coast.len() >= 3, "need several moored markers");

        let mut seen: BTreeMap<(u16, u16), &str> = BTreeMap::new();
        for mk in &cont.coast {
            if let Some(other) = seen.insert((mk.x, mk.y), mk.name.as_str()) {
                panic!(
                    "{} and {} moor on cell ({}, {})",
                    other, mk.name, mk.x, mk.y
                );
            }
        }
        // And what is drawn at the cell is what resolves there.
        for mk in &cont.coast {
            let (_, hit) = m.world.coast_at(mk.x, mk.y).expect("a marker");
            assert_eq!(
                hit.workload, mk.workload,
                "the anchor at ({}, {}) is {}'s but resolves to {}'s",
                mk.x, mk.y, mk.name, hit.name
            );
        }
    }

    /// EVERY CONTINENT IS INSIDE THE DECLARED WIDTH.
    ///
    /// `continents` is name-sorted with UNZONED sunk last, while `x` comes from
    /// the durable zone ordinal in first-observed order — so the vector's last
    /// entry is not the eastmost. Land outside `width` is painted but sits
    /// outside `bounds`, so it cannot be hovered, clicked or framed by `F`.
    #[test]
    fn the_world_is_wide_enough_for_its_eastmost_continent() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.node(fx::node("n0", None)); // sorts LAST as a zone, but takes a low ordinal
        let m = Models::build(&world);

        assert!(m.world.continents.len() >= 2, "need two continents");
        for c in &m.world.continents {
            assert!(
                c.x + c.w <= m.world.width,
                "continent {} spans to {} but width is {}",
                c.zone,
                c.x + c.w,
                m.world.width
            );
        }
    }

    /// A CONTINENT'S NORTH EDGE COUNTS ITS RESERVED GROUND.
    ///
    /// `Continent.y` anchors the zone label and the coastline's row zero, so it
    /// has to be the northmost thing the continent actually holds — and once a
    /// vacated slot keeps its ground, that can be a GHOST sitting above every
    /// live province. Deriving the edge from `provinces` alone puts the coast's
    /// row window below the reserved land, which clamps the ghost's shoreline
    /// into the cape taper and paints the label south of the continent's tip.
    ///
    /// (`y` was previously hardcoded to 1, which was true only while provinces
    /// accumulated from row 1. It happens to be 1 again in today's pipeline,
    /// because ordinal 0's slot is never released — but that is a consequence
    /// to be derived, not an assumption to be baked in, and a later declared
    /// compaction is exactly what would break it.)
    #[test]
    fn a_continents_north_edge_counts_ghost_ground_not_just_provinces() {
        let all = a2_world(&[("a", "z-a"), ("b", "z-a"), ("c", "z-a")], &[]);
        let m0 = Models::build(&all);

        // The two lowest-ordinal nodes depart; only the southernmost survives.
        let survivor = m0.world.continents[0]
            .provinces
            .iter()
            .max_by_key(|p| p.y)
            .expect("a province")
            .tile
            .name
            .clone();
        let fewer = a2_world(&[(survivor.as_str(), "z-a")], &[]);
        let m1 = Models::build_with(&fewer, &NamespaceFilter::All, &m0.layout);

        let cont = &m1.world.continents[0];
        let live = cont
            .provinces
            .iter()
            .map(|p| p.y)
            .min()
            .expect("a province");
        let ghost = cont.ghosts.iter().map(|g| g.y).min().expect("ghosts");
        assert!(
            ghost < live,
            "fixture must leave reserved ground NORTH of the survivor"
        );
        assert_eq!(cont.y, ghost, "the north edge ignores its reserved ground");
    }

    /// Byte-for-byte determinism: the same observation twice is the same world.
    ///
    /// Compared through `Debug`, which walks the WHOLE structure — continents,
    /// provinces, tiles, cities, coast markers, islands, extents and sources.
    /// This test previously compared four numbers for two nodes while claiming
    /// "byte for byte", so most of the world could have varied between builds
    /// and it would still have passed.
    #[test]
    fn the_same_observation_builds_the_same_world_twice() {
        let w = a2_world(
            &[("n1", "z-a"), ("n2", "z-b"), ("n3", "z-a")],
            &[("demo", "app"), ("demo", "other"), ("infra", "thing")],
        );
        let a = Models::build(&w).world;
        let b = Models::build(&w).world;
        assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "the same observation built two different worlds"
        );
        // Guard the guard: an empty world would make the comparison vacuous.
        assert!(a.continents.iter().any(|c| !c.provinces.is_empty()));
        assert!(a.cities().count() >= 3);
    }

    /// A GHOST LEAVES A GAP. The province below a departed node must not slide
    /// up — that is the reshuffle this phase removes, and §9's question 3 warns
    /// this is exactly where "ordinals map to position" and "ghosts render" can
    /// diverge. Two ADJACENT ghosts is the fixture that separates them.
    #[test]
    fn adjacent_ghosts_leave_their_ground_empty() {
        let all = a2_world(
            &[("n1", "z-a"), ("n2", "z-a"), ("n3", "z-a"), ("n4", "z-a")],
            &[],
        );
        let m0 = Models::build(&all);
        let survivors: Vec<(u16, u16)> = ["n1", "n4"]
            .iter()
            .map(|n| {
                let p = pos(&m0.world, n).expect("placed");
                (p.0, p.1)
            })
            .collect();

        // n2 and n3 depart together — two adjacent ghosts.
        let fewer = a2_world(&[("n1", "z-a"), ("n4", "z-a")], &[]);
        let m1 = Models::build_with(&fewer, &NamespaceFilter::All, &m0.layout);

        for (i, n) in ["n1", "n4"].iter().enumerate() {
            let p = pos(&m1.world, n).expect("still placed");
            assert_eq!((p.0, p.1), survivors[i], "{n} slid over the ghosts' ground");
        }
        assert_eq!(m1.layout.ghosts().count(), 2, "two ghosts retained");

        // The reserved ground REACHES THE MAP. A retained slot that emits no
        // geometry renders as open sea, so a departure reads as the continent
        // losing a piece of itself rather than as land standing empty — the
        // dominant visual effect measured on the churn fleet, and an acceptance
        // criterion this previously claimed to meet while emitting nothing.
        let cont = &m1.world.continents[0];
        assert_eq!(cont.ghosts.len(), 2, "the vacated ground was not emitted");
        for g in &cont.ghosts {
            assert_eq!(g.x, cont.x, "ghost ground drifted off its continent");
            assert!(
                cont.provinces
                    .iter()
                    .all(|p| g.y >= p.y + p.h || p.y >= g.y + g.h),
                "ghost ground overlaps a live province"
            );
        }
    }

    /// A surging refresh through the FULL builder: every surviving province keeps
    /// its coordinates, which is A2's reason to exist.
    #[test]
    fn a_surging_refresh_keeps_every_surviving_province_in_place() {
        let old = a2_world(&[("old-1", "z-a"), ("old-2", "z-a")], &[]);
        let m0 = Models::build(&old);

        // SURGE: replacements Ready before the predecessors drain.
        let both = a2_world(
            &[
                ("old-1", "z-a"),
                ("old-2", "z-a"),
                ("new-1", "z-a"),
                ("new-2", "z-a"),
            ],
            &[],
        );
        let m1 = Models::build_with(&both, &NamespaceFilter::All, &m0.layout);
        for n in ["old-1", "old-2"] {
            assert_eq!(
                pos(&m0.world, n),
                pos(&m1.world, n),
                "{n} moved during surge"
            );
        }
        let newly: Vec<_> = ["new-1", "new-2"]
            .iter()
            .map(|n| pos(&m1.world, n))
            .collect();

        // DRAIN.
        let after = a2_world(&[("new-1", "z-a"), ("new-2", "z-a")], &[]);
        let m2 = Models::build_with(&after, &NamespaceFilter::All, &m1.layout);
        for (i, n) in ["new-1", "new-2"].iter().enumerate() {
            assert_eq!(
                pos(&m2.world, n),
                newly[i],
                "{n} moved when its predecessor drained"
            );
        }
    }

    /// A new zone appends into fresh ocean instead of shifting every continent
    /// east — instability source 4's "appears" half, end to end.
    #[test]
    fn a_new_zone_moves_no_existing_continent() {
        let a = a2_world(&[("n1", "z-b"), ("n2", "z-c")], &[]);
        let m0 = Models::build(&a);
        let xs: Vec<u16> = ["n1", "n2"]
            .iter()
            .map(|n| pos(&m0.world, n).unwrap().0)
            .collect();

        // z-a sorts FIRST — the case that used to move everything.
        let b = a2_world(&[("n1", "z-b"), ("n2", "z-c"), ("n3", "z-a")], &[]);
        let m1 = Models::build_with(&b, &NamespaceFilter::All, &m0.layout);
        for (i, n) in ["n1", "n2"].iter().enumerate() {
            assert_eq!(
                pos(&m1.world, n).unwrap().0,
                xs[i],
                "{n}'s continent moved when a new zone appeared"
            );
        }
    }

    /// A node AT a nominal boundary gets the class its size implies.
    ///
    /// The defect, as a test: the bounds are nominal machine sizes and the value
    /// compared against them is a reported figure that is always lower, so a
    /// node sold as 32 GiB reported ~30.9, failed `>= 32.0`, and took the class
    /// below — the opposite of what the bounds' doc comment promises.
    ///
    /// Both directions matter, which is why the promotion guards are here too:
    /// the headroom's job is to sit between two failure modes, and a constant
    /// large enough to fix the first would start promoting genuine in-between
    /// machines. 24, 96 and 384 GiB are all real instance sizes.
    #[test]
    fn a_node_at_a_nominal_boundary_gets_the_class_its_size_implies() {
        use crate::state::model::ExtentInput;
        let gib = |g: f64| ExtentInput::Allocatable(g * 1024.0 * 1024.0 * 1024.0);
        let class_of = |g: f64| province_extent(&gib(g)).0;

        // The boundary cases the fix exists for — reported below a nominal bound.
        assert_eq!(class_of(30.9), EXTENT_CLASSES[1], "a nominal 32 GiB node");
        assert_eq!(class_of(123.0), EXTENT_CLASSES[2], "a nominal 128 GiB node");
        assert_eq!(class_of(493.0), EXTENT_CLASSES[3], "a nominal 512 GiB node");

        // The promotion guards — genuinely in-between machines stay put.
        assert_eq!(class_of(24.0), EXTENT_CLASSES[0], "a genuine 24 GiB node");
        assert_eq!(class_of(96.0), EXTENT_CLASSES[1], "a genuine 96 GiB node");
        assert_eq!(class_of(384.0), EXTENT_CLASSES[2], "a genuine 384 GiB node");

        // Exactly-nominal values are unchanged from before the headroom existed.
        assert_eq!(class_of(32.0), EXTENT_CLASSES[1]);
        assert_eq!(class_of(128.0), EXTENT_CLASSES[2]);
        assert_eq!(class_of(512.0), EXTENT_CLASSES[3]);
        assert_eq!(class_of(16.0), EXTENT_CLASSES[0]);

        // Totality: no panic, no out-of-range index at either extreme.
        assert_eq!(class_of(0.0), EXTENT_CLASSES[0]);
        assert_eq!(class_of(f64::MAX), EXTENT_CLASSES[3]);
        assert_eq!(class_of(-1.0), EXTENT_CLASSES[0]);
    }

    /// Extent comes from allocatable memory, in classes, with a marked fallback.
    #[test]
    fn extent_is_allocatable_derived_quantised_and_marked() {
        use crate::state::model::{ExtentInput, ExtentSource};
        let gib = |g: f64| ExtentInput::Allocatable(g * 1024.0 * 1024.0 * 1024.0);

        // Same class → same extent; a much larger node → more.
        assert_eq!(province_extent(&gib(8.0)).0, province_extent(&gib(16.0)).0);
        assert!(province_extent(&gib(256.0)).0 > province_extent(&gib(8.0)).0);
        // Small variation inside a class does not resize anything. The upper
        // example used to be 120 GiB, which `EXTENT_HEADROOM` now promotes to
        // the 128 class ON PURPOSE — a nominal 128 GiB machine reports about
        // that after firmware and a kubelet reservation. The property still
        // holds; the example had encoded the boundary defect.
        assert_eq!(province_extent(&gib(33.0)).0, province_extent(&gib(96.0)).0);

        // The fallbacks are DECLARED, and neither is the smallest class — an
        // unmeasurable node must not read as a genuinely tiny one.
        let (h_it, s_it) = province_extent(&ExtentInput::InstanceType("m5.large".into()));
        assert_eq!(s_it, ExtentSource::InstanceType);
        let (h_un, s_un) = province_extent(&ExtentInput::Unknown);
        assert_eq!(s_un, ExtentSource::Default);
        assert!(
            h_un > province_extent(&gib(1.0)).0,
            "unmeasured is not the smallest"
        );
        assert_eq!(h_it, h_un);
        assert_eq!(province_extent(&gib(8.0)).1, ExtentSource::Allocatable);
    }
}
