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

use std::collections::{BTreeMap, HashMap};

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
    /// Absolute world cell of the city glyph (label sits on the row below).
    pub x: u16,
    pub y: u16,
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
    /// Distinct DaemonSets with pods here — the node's *infrastructure*,
    /// rendered as roads rather than cities. Sorted, so the order is stable.
    ///
    /// Names, not a count: `build_world` computes them anyway, and a count
    /// forces every consumer that wants to SAY what runs on a node to go back
    /// to the store for it. `.len()` covers the render sites that only need a
    /// quantity.
    pub infra: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Continent {
    pub zone: String,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub provinces: Vec<Province>,
    /// Connectivity markers moored on the east coast: Service harbors and
    /// Ingress gates, each on the row of the city it serves.
    pub coast: Vec<CoastMarker>,
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

    /// Cities in stable exploration order (west→east, north→south).
    pub fn cities(&self) -> impl Iterator<Item = &City> {
        self.continents
            .iter()
            .flat_map(|c| c.provinces.iter())
            .flat_map(|p| p.cities.iter())
    }

    pub fn city_pos(&self, r: &WorkloadRef) -> Option<(u16, u16)> {
        self.cities().find(|c| &c.r == r).map(|c| (c.x, c.y))
    }

    /// (zone col, node row) of the province containing a world cell, for
    /// the minimap cursor.
    pub fn province_index_at(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        for (ci, cont) in self.continents.iter().enumerate() {
            if x < cont.x || x >= cont.x + cont.w {
                continue;
            }
            for (pi, p) in cont.provinces.iter().enumerate() {
                if y >= p.y && y < p.y + p.h {
                    return Some((ci, pi));
                }
            }
        }
        None
    }

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

    /// Which provinces a camera window can see, for the minimap's viewport
    /// frame: (first zone col, zone cols, first node row, node rows).
    pub fn visible_provinces(
        &self,
        cam: (u16, u16),
        view: (u16, u16),
    ) -> (usize, usize, usize, usize) {
        let (cx, cy) = cam;
        let (vw, vh) = view;
        let stride = (PATCH_W + OCEAN_GAP) as usize;
        let first_col = (cx as usize) / stride;
        let cols = ((cx + vw) as usize).div_ceil(stride).max(first_col + 1) - first_col;
        let (mut first_row, mut last_row) = (usize::MAX, 0usize);
        for cont in &self.continents {
            for (i, p) in cont.provinces.iter().enumerate() {
                if p.y < cy + vh && p.y + p.h > cy {
                    first_row = first_row.min(i);
                    last_row = last_row.max(i);
                }
            }
        }
        if first_row == usize::MAX {
            (first_col, cols, 0, 1)
        } else {
            (first_col, cols, first_row, last_row - first_row + 1)
        }
    }
}

/// City label column inside a province, jittered by a stable hash so the
/// land feels settled rather than gridded.
fn city_dx(name: &str) -> u16 {
    2 + (fnv1a64(name) % (PATCH_W as u64 - 16)) as u16
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

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// The extent a province gets, and which rung of the fallback chain produced it.
///
/// PURE. Capacity (memory — incompressible) → instance type → a DECLARED
/// default. The default is not a silent zero: an unmeasurable node gets a
/// middle-of-the-road extent and `ExtentSource::Default`, because a node that
/// cannot be measured must not render as a genuinely tiny one (v1.6.0).
pub fn province_extent(
    input: &crate::state::model::ExtentInput,
) -> (u16, crate::state::model::ExtentSource) {
    use crate::state::model::{ExtentInput, ExtentSource};
    match input {
        ExtentInput::Capacity(mem) => {
            let gib = mem / GIB;
            let class = EXTENT_BOUNDS_GIB.iter().filter(|b| gib >= **b).count();
            (EXTENT_CLASSES[class], ExtentSource::Capacity)
        }
        // No capacity, but a declared machine size. Coarse — the type string is
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
fn province_y(layout: &crate::state::layout::Layout, zone: &str, tile: &NodeTile) -> u16 {
    let ordinal = layout
        .slot_of(&tile.name)
        .filter(|k| k.zone == zone)
        .map_or(0, |k| k.ordinal);
    1 + ordinal * EXTENT_CLASSES[EXTENT_CLASSES.len() - 1]
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
                cities.push(City {
                    r: (*r).clone(),
                    ready,
                    desired,
                    severity: severity.get(r).copied(),
                    storage: storage_by.get(*r).copied(),
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
            let y = province_y(layout, &zone.name, tile);
            for (i, c) in cities.iter_mut().enumerate() {
                c.x = cx + city_dx(&c.r.name);
                // Cities keep their A2 row placement, clamped into the province
                // now that `h` no longer grows to fit them. Real slots for
                // cities are A3's job; until then the last rows can share.
                c.y = (y + 1 + 2 * i as u16).min(y + h.saturating_sub(1));
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
                infra: infra
                    .get(tile.name.as_str())
                    .map_or_else(Vec::new, |s| s.iter().map(|n| (*n).to_string()).collect()),
            });
            max_bottom = max_bottom.max(y + h);
        }

        // Moor connectivity markers in the ocean strip east of this
        // continent, each on its city's row. Gates sort ahead of harbors so
        // external exposure is never the marker dropped to the cap.
        let mut coast = Vec::new();
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
                for (i, e) in ordered.into_iter().take(COAST_CAP).enumerate() {
                    coast.push(CoastMarker {
                        kind: e.kind,
                        name: e.name.clone(),
                        detail: e.detail.clone(),
                        workload: c.r.clone(),
                        x: cx + PATCH_W + i as u16,
                        y: c.y,
                    });
                }
            }
        }

        continents.push(Continent {
            zone: zone.name.clone(),
            x: cx,
            y: 1,
            w: PATCH_W,
            provinces,
            coast,
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
    let width = continents
        .last()
        .map(|c| c.x + c.w)
        .unwrap_or(PATCH_W)
        .max(islands.last().map(|i| i.x + i.w).unwrap_or(0))
        .max(coast_right)
        + 2;
    let height = if islands.is_empty() {
        max_bottom + 2
    } else {
        island_bottom + 2
    };

    WorldModel {
        width,
        height,
        continents,
        islands,
        city_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures as fx;
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
        // …and so does the forgiveness ring around it.
        assert!(matches!(w.region_at(city.x + 1, city.y), Region::City(..)));
        assert!(matches!(w.region_at(city.x, city.y + 1), Region::City(..)));
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

    /// Byte-for-byte determinism: the same observation twice is the same world.
    #[test]
    fn the_same_observation_builds_the_same_world_twice() {
        let w = a2_world(&[("n1", "z-a"), ("n2", "z-b")], &[("demo", "app")]);
        let a = Models::build(&w).world;
        let b = Models::build(&w).world;
        for n in ["n1", "n2"] {
            assert_eq!(pos(&a, n), pos(&b, n));
        }
        assert_eq!(a.continents.len(), b.continents.len());
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

    /// Extent comes from capacity, in classes, with a marked fallback.
    #[test]
    fn extent_is_capacity_derived_quantised_and_marked() {
        use crate::state::model::{ExtentInput, ExtentSource};
        let gib = |g: f64| ExtentInput::Capacity(g * 1024.0 * 1024.0 * 1024.0);

        // Same class → same extent; a much larger node → more.
        assert_eq!(province_extent(&gib(8.0)).0, province_extent(&gib(16.0)).0);
        assert!(province_extent(&gib(256.0)).0 > province_extent(&gib(8.0)).0);
        // Small variation inside a class does not resize anything.
        assert_eq!(
            province_extent(&gib(33.0)).0,
            province_extent(&gib(120.0)).0
        );

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
        assert_eq!(province_extent(&gib(8.0)).1, ExtentSource::Capacity);
    }
}
