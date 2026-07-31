# KuberNation — Enabling Plan

**Beyond v1.0.0: stable geography, topology, and configurable cartographic expression**

---

## 1. What this is for

KuberNation is an **application**, not a map. It already has the Charter, the Annals, the workload table, the Oracle, the Almanac, the sidebar and the province window — and it should. Civilization does the same thing: the map is the spine, but the game is playable because of the city screens, the advisors and the demographics behind it.

So the competitive claim is not *map instead of list*. It is **coherence across modes**.

### 1.1 This is the researched answer, not a compromise

Wickens' *Engineering Psychology and Human Performance* reports a consensus in the visualization literature that multiple views should include a global, zoomed-out view **with a stable world frame of reference**, plus one or more local zoomed-in views — following Shneiderman and Plaisant's sequence of overview first, then zoom and filter, then details on demand.

Two claims from that passage matter enormously here:

1. The global overview supplies **spatial stability** and a context that prevents the user from getting lost in the data — the reason a large-scale map works at all.
2. That context should be **preserved through the later phases**, not replaced by them. Losing it produces the *keyhole phenomenon*, which the text notes is especially prevalent when scrolling a list.

Point 1 is independent validation of Workstream A: "stable world frame of reference" is precisely what `build_world` does not currently provide. Point 2 has a sharp, immediate consequence — see §1.4.

### 1.2 What each mode is for

| Question shape | Best mode |
|---|---|
| What is the state of X? | List / panel — K9s is faster and always will be |
| Enumerate, sort, filter, search | List — a map is bad at this and shouldn't try |
| What is the shape of the whole? | Map |
| What is *unusual* here? | Map — preattentive, seen without reading |
| What changed, and where? | Map — attempted nowhere else |

Losing the "what is X" race is fine. It is not our race, and we have lists for it anyway.

### 1.3 Redundancy is the mechanism, not waste

The same fact appearing in both modes is how a user learns to read the map. Substrate already works this way: the overlay says *which nodes*, the panel names *which DaemonSets*. Neither is redundant in the wasteful sense — the panel is what teaches you what the colour means.

The binding mechanism is **data brushing** (*Designing Interfaces*): selection in one view highlights the same entity in the others. Wickens calls the result **visual momentum**, preserved by highlighting the current location in the small-scale overview while exploration happens in the local view.

> **The test, revised:** is this fact in the mode that suits its shape — and is it linked to its counterpart in the other modes?

### 1.4 A defect this framing exposes

`panel_size` is `(sw - 80).clamp(900, 1100)` by `(sh - 80).clamp(560, 1000)`, centred — on the default window it occludes essentially the entire map.

By the standard above that is not a layout preference, it is **the keyhole phenomenon by construction**: the drill-down destroys the stable frame at exactly the moment the user most needs context for what they're reading. It is also why selection outlines were not worth building — the map isn't visible to draw on.

De-modalising the drill-down is cheap, unblocks brushing entirely, and `sidebar.rs` is already precedent for a non-occluding presentation reading the same `region_lines`. See Workstream D.

## 2. The aesthetic thesis: cartographic, not game-referential

**Beauty is a functional requirement here, not a finish.** A tool someone keeps on a second monitor all day has to be pleasant to look at or it does not stay open. More sharply: the entire differentiation in §1 is perceptual — *seen rather than read* — and an ugly perceptual instrument is a contradiction in terms. Sterile utility is not a safe fallback; it is a failure mode.

The scrutiny risk is nevertheless real. **If it reads as a game, it gets compared to games**, and that comparison is unwinnable on asset production.

The escape is *not* to lower the aesthetic bar. Cartography sets a **higher** one — Imhof's relief shading, Beck's Tube diagram, Bertin's semiology, Swiss topographic sheets are among the most beautiful printed artifacts of the last century *and* purely functional instruments. Nothing about that tradition is modest.

The escape is to change where the beauty **comes from**:

> Games buy beauty with art production. Maps earn it with craft — typography, colour discipline, visual hierarchy, deliberate generalisation, and restraint.

That is a bar this project can actually reach, because craft scales with judgment rather than budget. It is also the bar that defuses the comparison: craft-beauty reads as a serious instrument, whereas asset-beauty invites *"nice, but not as pretty as a real game."*

**This does not forbid high-fidelity rendering.** It disciplines what the fidelity is *for*. A richly rendered cartographic surface is squarely on-thesis; a richly rendered game surface is not. See §9 — an earlier draft of this plan closed that door on bad grounds.

It also resolves the naming of the style axis. Styles become **cartographic registers**, not skins: `Relief`, `Survey`, `Chart`, `Plan`. A vocabulary that says what each is *for*.

---

## 3. Workstream A — Stable geography

**This is the foundation, and it is not optional. Everything else depends on it.**

### 3.1 Why it is the precondition
`build_world` assigns position by iteration order and pod count: `y += h`, `x = cx` per continent, with `h` derived from what is running. Therefore:

- Adding or removing a node reshuffles everything after it
- A node's pod count changing changes its **shape**, moving its neighbours
- No correspondence across clusters, or across time on the same cluster

Spatial memory is the entire advantage a map has over a list. **A map that moves is just a slow list.** On an autoscaled fleet the layout churns constantly, which means the current map cannot accrue the one benefit it exists to provide.

### 3.1a Every source of instability in `build_world`

Read the builder rather than assuming; there are **five** independent sources of movement, and A1–A5 as originally written address only the first.

| # | Source | Code | Moves when |
|---|---|---|---|
| 1 | Province height | `let h = (2 + 2 * cities.len()).max(3)` then `y += h` | Any workload lands on or leaves a node |
| 2 | City row within a province | `c.y = y + 1 + 2 * i` | Any workload is added/removed on that node — every city below it shifts |
| 3 | Coast markers | `y: c.y` | Inherits (2) entirely |
| 4 | Continent x | `cx = zi * (PATCH_W + OCEAN_GAP)` | A zone appears, vanishes, or reorders in `map.zones` |
| 5 | Island block | `island_y = max_bottom + 2`, `h` from structure count | The *continents* get taller — islands move when unrelated nodes gain pods |

Two of these matter more than the plan first implied:

**(2) is arguably worse than (1).** Cities are workloads — the thing a user actually hunts for. `c.x` is already stable (`city_dx` hashes the name), but `c.y` is a bare ordinal, so adding one workload to a node reshuffles every city beneath it. **A must give cities a stable slot inside a province, on the same principle as nodes inside a pool**, or the most-looked-for objects on the map remain the least stable.

**(5) is a coupling nobody would predict.** Island position depends on the tallest continent, so a pod landing on an unrelated node in an unrelated zone moves every namespace island. Fixing (1) largely fixes this, but it should be verified rather than assumed.

### 3.2 The five changes

**A1 — Decouple extent from workload.** A province's size must not depend on pod count. Fixed extent per node (or extent derived from a durable attribute like instance type), with pod density expressed *thematically* — settlement density, colour, marks. This alone removes most of the churn: workloads move constantly, nodes do not.

**A2 — Anchor position to the slot, not the node.** Zone → continent stays. Within a zone, the durable identity is **not the node name.**

Immutable-infrastructure clusters replace nodes on upgrade, and the replacement carries a *different name*. Ordering by node identity or creation time therefore fails exactly when it matters most: a rolling refresh replaces the whole fleet, every node is "new," and the map reshuffles completely — the precise failure A exists to prevent.

What actually persists across a refresh is the **slot**: zone, plus nodepool, plus ordinal within that pool. The new node inherits the role, the instance type, the zone, and usually the workloads. Only the name changed, and the name is an artifact of immutable infrastructure rather than a fact about the cluster.

```
continent  ← zone            (topology.kubernetes.io/zone — already modelled)
region     ← nodepool        (GKE/EKS/AKS/Karpenter labels; fall back to
                              instance type, then to a single default pool)
province   ← ordinal in pool (the slot; survives its occupant)
```

**The terrain belongs to the slot. Nodes occupy it and are replaced.** That makes a rolling refresh a change of occupant rather than a change of world.

**A3 — Persist the assignment.** Derivation alone isn't enough: changing the derivation later would move everything. Assign slot coordinates on first sight and persist them per cluster, so the layout survives upgrades of KuberNation itself. `prefs.rs` establishes the storage pattern; this needs its own per-cluster store.

**A4 — Two tiers of change: succession and cataclysm.**

Discontinuity must be *named* rather than smoothed over — but the granularity decides whether that naming is signal or noise.

**Succession** is routine: a node replaced within a stable pool. The slot persists; the occupant changes. Mark the province as **fresh ground** — newly broken terrain that ages back to normal over some window.

This is not decoration. A rolling deployment then appears as a **wave of fresh terrain sweeping across zones**, which is "what changed and where" in its strongest form and something no list tool can show. The Annals can tell you thirty nodes were replaced; only the map can show you the *shape* of the rollout.

**Cataclysm** is rare and structural: a nodepool added or removed, a zone appearing or vanishing, a resize that changes the slot count. *This* is what scars the world — a recorded event with a date, permanently visible in the terrain.

> **Scars must be rare or they are noise.** If every rolling refresh scarred the map, the world would be nothing but scars within a month and the marking would stop carrying information. Reserving cataclysm for genuine topology change is what keeps it legible.

The pattern is **punctuated equilibrium** — long stable spans broken by disruptive events — and the geological analogy holds precisely: succession lays down new surface, tectonics changes the shape of the world. Only the second earns a boundary in the record.

Departure of a *slot* (not merely its occupant) keeps the ghost-town idiom: ruins, reclaimed land, a sunken province. **This gives the map memory, which no list tool has.**

**A5 — Make position referenceable.** Stability is only useful if a position can be *named*. Add a recessive graticule with plate coordinates, so "the node in C4" becomes shareable language — in a handover, a ticket, or a screenshot.

This is a genuine cartographic convention rather than a borrowed game one, and it is what converts A1–A4 from an internal property into something a user can actually exploit. Without it, the layout is stable but unspeakable.

### 3.3 The reference frame — and declaring it

Position will still be arbitrary. Node adjacency means nothing real, and the map must not imply otherwise.

But *arbitrary* is not the same as *unusable*, and cartography settled this a century ago. Wegener's 1915 continental-drift plates show a world whose geography is itself changing, across three epochs, in a way a reader can actually compare — and the caption states plainly that the graticule is arbitrary, fixed to Africa's present position.

That is the move:

> **When the geography itself changes, comparison requires an invariant. Fix the frame to one durable entity, declare that you have done so, and let everything else read as motion relative to it.**

Wegener does not pretend the grid is meaningful. He *names* it as a convention and thereby makes the frames comparable. Applied here:

- Anchor the coordinate system to something durable — oldest node, or the zone ordering — and **say so in the legend.**
- Node arrival and departure then read as motion against a declared reference, which is honest and legible at once.
- Zone remains the only genuine grouping the visual language asserts. Anything reading as "these nodes are related because they are near each other" is still a lie.

This also upgrades A5 from a convenience to a requirement: **the graticule is the invariant against which change is read.** Without it, two frames of a changing world cannot be compared at all — which is precisely what §7 depends on.

---

### 3.4 The occupation model

`city_home` sites a workload at the node holding the plurality of its pods and draws it there **once**. A Deployment across five nodes appears on one, and the other four read emptier than they are. **Node occupancy is currently false**, which is why resource-based terrain has nowhere to land.

The split that resolves it:

| | Is | Belongs to |
|---|---|---|
| **Workload** | An identity — named, connected to Services, the thing you search for | Topology: the schematic view, the lists |
| **Pods** | Occupation — they consume *this* node's memory | Space: the map |

So pods render on the node they are actually on. But **not as cities** — five settlements named `api` means "where is my api deployment" has five answers.

> **The province has one settlement. Workloads are districts within it.**

Occupancy is the built-up fraction; workload identity stays singular; D2's persistent namespace colour ties a district to its siblings elsewhere. `City` in the code becomes the node's built-up area rather than the workload — a real rename, squarely inside the 2.x revamp.

#### 3.4.1 Memory is the land, and it is a 2×2

Memory is the right constraint because it is **incompressible** — CPU throttles, memory OOM-kills. But two memory numbers tell opposite stories, and the interesting information is in their relationship:

| | Low usage | High usage |
|---|---|---|
| **Low requests** | Idle, schedulable | **Overcommitted — OOM risk** |
| **High requests** | **Waste — reserved, never used** | Healthy and full |

One diagonal is money, the other is danger. Neither is visible in any list tool. This is the strongest single instrument in the plan.

Expressed as: **land density** = node memory (requests and usage), **district density** = per-workload consumed ÷ requested. Their divergence is the finding.

#### 3.4.2 The rest of the dimensions

| Signal | Expression |
|---|---|
| Pod count | Settlement density — how much of the province is built |
| Containers per pod | Settlement tier (`draw_settlement` already tiers 0–3) |
| Requests ÷ allocatable | The choropleth — how much of the province is claimed |
| QoS class | **Building material** — tents (BestEffort), timber (Burstable), stone (Guaranteed) |

QoS is **eviction order**: the map would be showing which settlements burn first under pressure. Note Kubernetes has three QoS classes, not four — "fully specified but unequal" and "partially specified" both fall under Burstable. Sub-dividing Burstable is a genuine improvement, but must not be labelled QoS in the UI.

#### 3.4.3 Walls are misassigned

`Overlay::Coverage` / `WallData` currently claims "walls" for NetworkPolicy — the same name-collision class as `Flat` versus `overlay_flat`. And the assignment is backwards on the merits:

- **NetworkPolicy** governs who may cross *between territories* → a **frontier**, matching the dashed territory borders in the reference specimens
- **Resource limits** govern how far a thing may *grow* → a **city wall**

Swapping buys a real failure mode: a workload throttled or OOMKilled at its limit is **straining at the walls**.

#### 3.4.4 Hierarchy, with pools that span zones

A nodepool spanning three availability zones is normal, which breaks a strict zone-contains-pool nesting. **Zone stays primary, because zone is the failure domain.** A pool across three AZs *should* render as three regions — a zone outage takes exactly one of them, and that is the truth an operator needs. Pool identity travels by colour and label, not contiguity.

```
continent ← zone
region    ← pool ∩ this zone
province  ← slot
```

Heterogeneous hardware becomes **map resources**, matching Civ's strategic-resource idiom: GPUs as a resource deposit that gates what can be sited there. CPU architecture reads better as a terrain variant than a badge — an `arm64` pool as visibly different country, so a wrong-arch scheduling failure is something you *see* rather than read in an event.

---

## 3A. Workstream A0 — The resource data prerequisite

**Nothing in §3.4 is expressible today, and this must land before any of it.**

- `PodGlyph` carries namespace, name, state, owner. **No requests, limits, or usage.**
- `NodeTile` has `cpu_ratio` / `mem_ratio`, but they are **either** usage-based **or** request-based depending on `metric_source` — never both. The §3.4.1 2×2 is literally inexpressible.

The cost pipeline reads pod resources in order to price them, so the data is observable. It is collapsed into money and never carried to the map — the same class of loss as `Province.infra` being a count, but larger.

**A0 delivers:**

- [ ] Per-pod requests, limits, and (where metrics-server is present) usage on `PodGlyph`
- [ ] Per-pod QoS class, derived once in core rather than at each render site
- [ ] `NodeTile` carrying **both** request-ratio and usage-ratio, with `metric_source` saying whether usage is real or absent — rather than one polymorphic number
- [ ] The existing `cpu_ratio` / `mem_ratio` consumers migrated or kept as derived accessors

This is a pure model change with no rendering, which makes it independently testable and independently valuable — the province window and cost view both benefit immediately.

**Gate it separately.** If A0 is folded into the visual work, the session discovers the gap halfway through and improvises.

---

## 4. Workstream B — Topology

Per *Map Framework*'s decomposition, this is the construct the cluster genuinely has and the map currently omits. Two expressions, one model.

### 4.1 The model already exists

`blast_radius` computes the observed topology in core — pure, with `OwnerIndex`, `build_exposure`, `ExposureEntry`, and hop counts. It is already shared with the Oracle specifically so the two cannot disagree.

The hop structure *is* a transit line:

```
Ingress (hop 2) → Service (hop 1) → Workload (hop 0) → Node
```

An Ingress fronting several Services is an interchange. A Service fronting several workloads is a junction. Namespaces are the lines. NetworkPolicy is the fare zones.

**Both expressions below must read from this one model**, following the `resolve_region` / `SubstrateReport` precedent.

### 4.2 B1 — On-map flow, focus-bounded

`--blast` already marks affected *endpoints* without drawing the *edges* between them. Drawing the routes turns endpoint-marking into a flow map.

Bounded to one selected subject, so it never becomes spaghetti — which is precisely the failure mode that kills unbounded edge-drawing over tiled terrain.

### 4.3 B2 — The schematic view

A standalone topological view: no geography, orthogonal routing, deliberate distortion. Per *Spatial Computing*, transit maps sit outside GIS entirely — a different class of artifact, which is why it **coexists with the map rather than competing with it**.

**Correlation is by anchor, not position** — shared identity, shared selection, shared namespace colour, explicit node and zone labels. Selecting a workload in either view highlights it in the other.

**The hard part is layout stability, not drawing.** A schematic that reshuffles between polls is worse than none, because it destroys the same spatial memory Workstream A exists to build. Prototype layout stability first; it is the most likely thing to kill this.

---

## 5. Workstream D — Coordinated views

The cheapest workstream here, and it unblocks the value of every other one. Per §1.1, the overview must survive the phases that follow it.

**D1 — De-modalise the drill-down.** The province and workload windows must not occlude the map. `sidebar.rs` already reads the same `region_lines` in a non-occluding presentation, so the pattern exists in-tree. This is the single highest value-per-effort item in the plan.

**D2 — Brushing.** Selection propagates across every view: map, Annals, workload table, Charter, and later the schematic. One selection model, one identity, following the `resolve_region` / `SubstrateReport` discipline of a single shared authority rather than per-view reimplementation.

**D3 — Visual momentum.** While the user works in a list or panel, mark *where they are* on the map. This is the specific mechanism Wickens names for preserving context during local exploration, and it is what makes the map worth keeping on screen during a drill-down rather than merely possible to keep.

**D4 — Reverse indexing.** Lists point back at the map: selecting a row flies the camera and marks the province. Civ's advisors work exactly this way — the list is an index *into* the world, not a replacement for it.

D2 and D3 are also what make the schematic view (B2) correlate with the geographic one, so D is a prerequisite for B2 rather than a parallel nicety.

---

## 6. Workstream C — Cartographic registers

The configurable expression, with the discipline established earlier:

> **The pipeline owns structure. Vocabulary owns appearance. Instrumentation never varies.**

- **Pipeline** — plane order, culling, LOD, label de-confliction, hit-testing. Correctness, not taste.
- **Vocabulary** — terrain, settlements, vegetation, framing. Pluggable via trait.
- **Instrumentation** — blast rings, hover marks, severity, overlay tints. Must read identically in every register, for the reason the `cb_*` funnel exists.

### 6.1 A register has three dimensions, not one

An earlier draft treated a register as palette + shape. It should be:

| Dimension | Owns | Example variation |
|---|---|---|
| **Projection** | `Camera` — `to_screen`, `cell_at`, `to_plane` | isometric · top-down · orthographic |
| **Vocabulary** | The trait — what a cell, settlement, road looks like | prism · flat tile · contour |
| **Palette** | `theme.rs` — via the `cb_*` funnel | relief · schematic · nautical |

Projection is already a swappable concern in principle: `to_screen` is one function, `cell_at` is its inverse, and the relief work generalised both. A top-down projection is the same signature with different arithmetic. What made top-down look like a rewrite is that the *drawing* assumes diamonds — and under the vocabulary trait, shape is precisely what the vocabulary owns.

**So `Plan` (top-down) is architecturally the same shape of change as `Survey`.** Not a replacement of the map, a register of it.

Extract the trait **while writing the second register**, not before. `Survey` is the right second one — schematic, high-contrast, `land_lift() == 0.0`, immediately useful for documentation and projected screenshots — because it varies vocabulary and palette while holding projection fixed. `Plan` is the natural third, varying the dimension `Survey` held constant, which is what proves the three axes are genuinely independent.

### 6.2 Conventions worth adopting

Drawn from the Civilization lineage, filtered to **information design rather than art production** — the part that transfers to a project with no art budget:

| Convention | Application here |
|---|---|
| Edge channels separated by hue | Roads, routes and policy edges each take a hue outside the terrain palette. Topology needn't be rationed if it isn't competing with terrain for the same channel. |
| Persistent category colour | `namespace_pair` promoted from an overlay to a **cross-view identity colour**. Delivers most of D2's correlation with no selection interaction at all. |
| Recessive graticule | See A5 — faint, always present, never dominant. |
| Itemised attribution on hover | Decompose aggregates. Cost and Saturation both roll up today; the breakdown is what teaches a user to read the fill (§1.3). |
| Uniform mark chrome | Marks share one badge treatment and differ only by glyph, so the *system* is learned once rather than per-mark. |
| Chrome recedes | The franchise's own answer to accumulated clutter was less UI, not cleverer UI. |
| Typography as a channel | Italic for water features, roman for land, letterspacing for large regions, baselines following the feature. Costs judgment rather than budget — §2's thesis made concrete. |

**And one anti-pattern, from the same source.** Late-game Civ V degenerates into a field of near-identical markers distinguished only by tiny glyphs. It is survivable there because the player accreted that state over hundreds of turns and remembers building it. Our users arrive at a hundred nodes cold, with no such memory.

### 6.3 Density: stop drawing objects

The county-level choropleth answers what the game lineage could not. It renders roughly three thousand units simultaneously and stays readable, and it does so by **giving up per-unit identification entirely at that zoom**:

- One sequential ramp, no competing hues
- **No labels at all** — nothing is individually identified
- Stable boundaries, so the eye reads clusters rather than shapes
- A single categorical layer (the coastal fringe) over the sequential one, and no more

The pattern is read as a **field**, not as a set of objects. Nothing is legible individually and that is the point — the information is in the clustering.

> **At `Scale::World`, the map should be a field. Identification is what zooming is for.**

The LOD tiers already exist. What has to change is the ambition at the top tier: today it tries to keep objects legible, and it should instead stop trying and become a texture. That is also what makes a hundred-node fleet a strength rather than a scaling problem.

---

## 7. What A unlocks: time

This is the differentiator, and it is worth stating as the plan's real destination.

`timeline.rs` already computes the temporal story — `Annals`, fault lines, `annals_lines_flags_suspect_change_before_failure`. **And then renders it as lines.** That is exactly what K9s or Freelens would do. The analysis is done; the map has simply never been used for it.

Brewer's *Designing Better Maps* covers map series and time-series mapping, and stresses holding **data classing constant across a series** so frames are comparable.

That is only possible once geography is stable. Two frames cannot be compared if the layout moved between them — so **A is not merely first, it is what makes time meaningful at all.**

Candidate expressions, once A lands:

- **Small multiples** — the last N polls as a strip, identical classing, so change is seen rather than read
- **Change-since overlay** — colour by delta rather than level
- **Fault-line marking on the map** — `Annals` already identifies the suspect change before a failure; put it *where* it happened

### 7.1 What the drift plates specify

Wegener's three epochs, and the modern redrawings of them, agree on four rules for a series showing changing geography. They are the concrete spec for small multiples:

1. **Identical projection and graticule in every frame.** The invariant is what makes comparison possible (§3.3).
2. **Identical classing and palette.** Brewer's requirement, and the redrawings obey it strictly.
3. **Labels persist across frames.** North America is named in *every* panel, so the eye tracks one entity through the series rather than re-reading each frame. For us: node names carry through the strip even as their state changes.
4. **Direction is marked, not inferred.** The modern infographic adds motion arrows; the reader is told which way things went rather than being asked to diff two pictures.

Rule 3 is the one most likely to be dropped for space, and it is the one that makes the series legible.

### 7.2 Showing a projected or former state

National Geographic's sea-level projection is the cleanest model for rendering a state that is not the present one. Its whole method is a **three-value legend** — present urban area, flood-prone urban area, and land below the projected tide line — over present-day place labels that keep the reader oriented.

The structure transfers directly to the "map has memory" idea in A4: *present*, *departed*, and *newly arrived* as three restrained categories over a stable base. The changed state is the subject; the present remains the reference frame.

"What changed and where" is a question no list tool answers well and none of them attempt spatially. It is the strongest available claim to being genuinely more useful rather than merely different.

---

## 8. Dependency order

```
D1 (de-modalise) ───────── do first. Cheap, and every other view's value
                            depends on the map surviving a drill-down.

A0 (resource data) ─────→ blocks §3.4 entirely. Pure model change, no
                            rendering, independently testable.

A (stable geography)  ─┬─→ TIME (small multiples, change overlay, fault lines)
                       │
                       ├─→ B2 (schematic view — needs stable anchors)
                       │
D2/D3/D4 (brushing) ───┴─→ also required by B2, to correlate the two views

C (registers) ──────────── independent; can proceed in parallel

B1 (on-map flow) ───────── independent of A; cheap; do it early for signal
```

**D1 first** — it is small, it is grounded in §1.1 rather than taste, and until it lands every other view competes with the map instead of complementing it.

**A blocks the two most valuable things.** Do it in full before TIME or B2.

B1 is the cheap early probe: it tests whether on-map topology reads at all before B2's much larger layout investment.

C is genuinely parallel and can absorb spare capacity.

---

## 9. Risks and kill criteria

| Risk | Kill criterion |
|---|---|
| Stable geography still reads as arbitrary — users don't build spatial memory because position means nothing | After A, can a user find a named node faster on the map than in K9s? If no, the whole thesis fails |
| Schematic layout won't stabilise across polls | Prototype layout-under-churn before building the view. If it jumps, stop |
| Time-series frames are too dense to read at fleet scale | Test small multiples at 100 nodes before building the strip |
| Feature sprawl — three workstreams, none finished | One workstream in flight at a time, except C |
| Cartographic register reads as pretentious rather than serious | Show it to someone who runs clusters and doesn't know the project |

---

## 10. Kubernetes coverage review

A fresh pass over what the world models, what it omits, and what is assigned to the wrong feature. Absences below were verified against the codebase, not assumed.

### 10.1 Missing, ordered by operational weight

**1. PodDisruptionBudget — the standout omission.** Verified absent. PDB is the direct answer to the most common node-lifecycle question, *"can I drain this node?"*, and it is why drains hang.

It also fits the succession model exactly: **a PDB is what resists the cataclysm.** A workload with one is protected — it holds a charter the refresh must respect. Given A4 makes rolling replacement a first-class event, modelling the thing that *constrains* that event is not an addition to the theme, it is the missing half of it.

**2. Taints and tolerations.** Verified absent; only `cordoned` is modelled. A taint makes land **inhospitable** — desert, tundra, marsh — and a toleration is what lets a workload settle there. This is the strongest available answer to *"why won't this pod schedule?"*, and it answers it **geographically**, which is precisely the §1.2 claim.

**3. Zonal volumes.** A pod bound to a zonal PV **cannot leave its continent**. That is a geographic constraint, and geography is the map's entire business. Currently `CityStorage` counts claims and pending only.

**4. HorizontalPodAutoscaler.** Verified absent. A workload that scales is categorically different from one that does not — a settlement that grows and contracts with the season versus a static one.

**5. PriorityClass.** Complements QoS: QoS decides who is evicted under node pressure, priority decides who is preempted under scheduling pressure. Together they give a complete "who dies first" story; separately, each is half.

**6. The control plane.** Self-managed clusters have control-plane nodes and they currently render as ordinary provinces. **The map has no capital.** Every reference map in this thread has one.

**7. Gateway API.** The Ingress successor. Not urgent, but "we model Ingress" ages into "we model the deprecated one."

**8. ResourceQuota and LimitRange.** Namespace-level caps. Islands have no notion of capacity.

**9. Rollout state.** `ready`/`desired` is present, but *mid-rollout* is not — and it is the natural pairing for A4's fresh-ground succession marking.

### 10.2 Mis-assigned

**ClusterIP Services are moored in the ocean.** All Services become `CoastKind::Harbor` regardless of type. A ClusterIP is **internal-only** and reachable from nowhere outside — putting it on the coast asserts external reachability it does not have.

`CoastMarker.detail` already carries the Service type. **The data to distinguish is present and simply unused for placement.** Internal Services belong inland — a market, a crossroads — with only NodePort and LoadBalancer at the shore. This is the same honesty issue as the 1.x ingress note in §11, and the two should be fixed together.

**NetworkPolicy holds the "walls" name.** See §3.4.3 — it should be a frontier, with walls reassigned to resource limits.

**Islands are labelled by namespace but are really "the unlanded."** The code already knows this: islands hold custom resources, batch expeditions, and — per its own doc comment — a workload with no pods on any land. That is a coherent category, and it is *not* "namespace."

Namespace is a partition cutting across the whole cluster, not a place; rendering it as somewhere separate implies its pods live elsewhere, which is false. Namespace wants to be a **political dimension** — the persistent category colour of §6.2 — while the islands keep their real meaning: **things that exist without occupying capacity.** This is a relabelling, not a restructure.

**DaemonSets are double-booked.** They are drawn as roads *and* reported as SUBSTRATE. Once §3.4 makes districts carry occupation, roads are free to mean actual connectivity (Service → pod routing), with DaemonSets living entirely in the substrate.

### 10.3 Deliberately unmodelled

ConfigMaps and Secrets as map objects — too numerous, and their operational signal is *change*, which is the Annals' job rather than the terrain's. EndpointSlices — below the level the map operates at. Both belong in drill-downs if anywhere.

---

## 11. Metaphor roadmap — where the world's edge goes

The map's edge currently conflates two different kinds of "elsewhere." Everything external arrives across the same ocean, whether it comes from a cluster we could draw or from the public internet.

That distinction is the one worth building the vocabulary around, and it stages naturally across versions.

### 1.x — Harbours stay, but distinguish the destination

`CoastKind::Harbor` (Service) and `CoastKind::Gate` (Ingress) keep their present form. The cheap increment is **not** a new metaphor: it is marking whether an Ingress is genuinely externally reachable — an external LoadBalancer address, or an ingressClass owned by an off-cluster controller. That is a data question answerable now, expressible as a mark difference on the existing coast, and it makes the later split coherent rather than retrofitted.

### 2.x — Two categories of elsewhere

| Origin | Metaphor | Reads as |
|---|---|---|
| Another cluster on this map | Harbour + shipping lane | Traceable, on-map, a route with two ends |
| Off-world (internet, CDN, WAF) | **Uplink** — dish, mast, beacon | Leaves the map; the far end is not drawn |

The sea metaphor cannot make this distinction — everything arrives from the same water. A dish points *away from the world*, which is exactly the semantic needed: **this connects to something not represented here.**

This lands alongside the register work, since it is a vocabulary change and the vocabulary trait is what makes it expressible per register.

### 3.x — Model the far end

Off-cluster appliances as first-class entities: Traffic Manager, global load balancers, CDN edges. A satellite one can actually see, with the uplink pointing at it.

Deferred deliberately. It requires data KuberNation does not currently observe, and every observation source added is a new way to be confidently wrong about someone's infrastructure. The 2.x uplink is honest without it: it says *elsewhere* and declines to claim more.

---

## 12. Doors: which are open, which are shut, and why

An earlier draft of this section closed three doors on reasoning that does not survive inspection. Difficulty is not a reason to close a door; a *structural* argument is. Re-examined:

### Reopened

**Rich, high-fidelity rendering — including baked assets.** The stated objection was that it forfeits the single-binary property. It does not: `include_bytes!` compiles an atlas straight into the binary. The genuine costs are art production, WASM payload, and the game-comparison risk — and per §2 that last one is about the *referent*, not the fidelity. Imhof-grade relief is high fidelity and unmistakably a map.

The right question is not procedural versus baked. It is: **does this fidelity read as cartography or as a game?** On-thesis rendering can be as rich as we can make it.

**Top-down.** The objection — that it re-spends the spatial budget without adding topology — is correct as an argument against top-down *replacing* the map, but irrelevant to top-down as a **register** (§5.1). Projection is one dimension of three. `Plan` is a legitimate register and probably the one that best demonstrates the axes are independent.

**Per-node vertical encoding.** The cutaway fork killed this on tiling: provinces abut, so a cut face is always occluded by the province drawn after it. But that finding was **contingent on a layout Workstream A is about to replace.** The fork's own conclusion named gaps between provinces as the one thing that would change the answer, and dismissed it as a world-model geometry change — which is exactly what A is.

**A therefore reopens the whole vertical family for free.** Gated on A landing, and worth re-testing then rather than assuming either outcome.

### Still shut

**Competing with K9s on resource inspection.** Strategic focus, not difficulty. We lose that race and it is not ours.

**Reading DaemonSet specs for intent.** Held per the v1.5.0 decision pending evidence the prevalence heuristic actually fails. Revisitable on evidence, not on ambition.
