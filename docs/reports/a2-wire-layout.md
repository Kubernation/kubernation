# A2 — Wire the layout in

**Implementation report** · 2026-08-02 · **v1.7.0** (+ a follow-up patch)
**Governing docs:** [`kubernation-a2-wire-layout-guidance.md`](../kubernation-a2-wire-layout-guidance.md) · [`kubernation-workstream-a-decomposition.md`](../kubernation-workstream-a-decomposition.md) §4
**Gate evidence:** [`a2-gate/`](a2-gate/) — 6 frames from one session across a 30-node rolling refresh

> **This supersedes the report written at `74d969d`.** That version answered the
> gate "yes, 1.3% → 0.0%, frames pixel-identical." All three numbers were
> unsound — see [§4](#4-the-first-gate-answer-was-wrong-three-ways) — and the map
> it photographed had 42 of 100 nodes drawn invisibly on top of each other.

---

## 1. What A2 built

`build_world` computes no positions of its own.

- **Province y** = slot ordinal × the largest extent class. The stride is
  uniform so a slot's ground never depends on its neighbours' size; a ghost
  leaves its stride empty.
- **Continent x** = a **durable zone ordinal**, carried/appended/reserved
  exactly like slots. This is instability source 4 and half of A2's chartered
  scope. §2 prescribed only "sort the zones", which fixes *reorders* but not
  arrivals or departures: verified before the change, adding an
  alphabetically-first zone moved `z-b` from x=0 to x=30 and `z-c` from 30 to 60
  — **every continent shifts.**
- **Extent** from node memory capacity, quantised into four classes
  (`EXTENT_CLASSES = [3,5,7,9]`), with a declared fallback chain
  capacity → instance type → default, recorded in `ExtentSource`. The default is
  deliberately not the smallest class.
- **Ghost ground** renders (added late — see §5).
- **State threading:** `Models::build_with(world, filter, prior)`; `Models`
  carries the resulting `Layout`. The net thread feeds last tick's layout
  forward per world (`build_carrying`), dropped on a context switch.

---

## 2. The gate, answered

> **Does the map hold still?**
>
> **The provinces do. The cities do not, and that is A3's charter.**

One session, one 30-node rolling refresh on the 100-node churn fleet, surging,
from a freshly reset and settled baseline. Fixed framing containing both
provinces and settlements.

| | share of map area |
|---|---|
| **Pixel-identical, start to end** | **92.66%** |
| land → ocean (ground vacated) | **0.03%** |
| ocean → land (ground taken) | 0.38% |
| changed in place (reserved ground greying) | 6.93% |

**The land/sea silhouette moves by 0.41% across a full third-of-the-fleet
replacement.** No province moved. Nearly all the remaining change is *colour*: a
slot whose node departed keeps its ground and goes grey.

Cities did move — settlement pixels change during the run, and one workload
crossed the map when its pods rescheduled. That is instability sources 2/3/5,
which the guidance assigns to A3 and tells A2 not to touch.

### Method — so the next gate is comparable

Frames converted to BMP with `sips`, compared byte-wise over the **play area
only** (x < `width − 528`, y > 60 — the docked column's counters change every
frame and would swamp the map). Exact match, no tolerance. Land classified as
`green > blue`, which covers terrain, sand and ghost grey but not sea. Frame 00
against frame 13.

**The comparator is not committed** — it was an ad-hoc script. Committing it
beside `gate.sh` is a prerequisite for A3's gate producing a number of the same
kind.

### Operational caveats

- **Reset and settle first.** Runs chain: reserved ground accumulates, so a
  second run starts from a partly-grey map and its numbers are not comparable.
- **`gate.sh` now refuses a no-op run.** Re-running it without a reset finds no
  `OLD_GEN` nodes, scenario 1 breaks at wave 0, and the flipbook would photograph
  a static fleet and report a perfectly still map. It counts first and exits 2.
- **Not CI-able** — needs the kwok fleet and a display.

---

## 3. Verification and acceptance

### §0 — all ten claims TRUE

Fifth round running that §0 survived intact; the defects keep landing in what
the docs describe as routine. Re-verified against the pre-A2 tree
(`74d969d~1`): stateless `build_filtered` (model.rs:1955); `build_world` with no
layout parameter (world.rs:364); position inline as
`h = (2 + 2*cities.len()).max(3)` / `y += h` / `cx = zi * (PATCH_W + OCEAN_GAP)`
(world.rs:460/477/435); `Layout` over `BTreeMap<SlotKey, SlotState>` with
`slots()` yielding ghosts and `changes_from` (layout.rs:106/124/158); `NodeTile`
ratios `Option<f64>` with no absolute allocatable (model.rs:373); pub
`node_allocatable` (model.rs:632); `Province` as specified (world.rs:51). The
three semantic claims were established in A-pre and A1.

### §7 — acceptance

| Criterion | Status |
|---|---|
| `build_world` computes no positions of its own | ✅ |
| `Layout` threaded through `Models`; no global, none on `ObservedWorld` | ✅ |
| Ordinal gaps preserved — ghosts do not compact | ✅ |
| Extent capacity-derived with a declared fallback | ✅ declared |
| …and **marked** | ❌ **not met** — see §6 |
| Zone order stable | ✅ (durable ordinals, beyond what §2 asked) |
| Ghosts render without collapsing | ✅ *after* the review — falsely claimed before |
| The gate answered explicitly, with captures | ✅ on the second attempt |
| `cargo nextest` green | ✅ 399 core + 90 GUI |

---

## 4. The first gate answer was wrong, three ways

**The flipbook was blind to the mechanism.** Every frame was its own
`--screenshot` process, so each started from `Layout::default()` and assigned
from scratch. Assignment is deterministic in the node set, so the flipbook
renders identically **whether or not the carry exists at all**. It measured
determinism, which was never in doubt — and it erred flattering, because from
scratch a replacement inherits the departed node's ordinal.

**The two numbers were not comparable.** A2 roughly tripled the vertical stride,
so the same framing covered a third as many provinces: the before viewport held
five settlements, the after essentially none. Cities are the axis A2 deliberately
does *not* fix, so the unfixed axis was in one number and absent from the other.

**"Pixel-identical" was false** — 743 map pixels differed, from forest re-rolls
seeded on the node name.

### The instrument had already lied three times before that

Carried forward because they are the harness's durable failure modes:

1. **The camera anchor sat inside the pool under refresh.** When it drained the
   anchor vanished, capture silently fell back to fit-the-world, and the
   "movement" was the camera. Fixed: anchor outside the pool, and the fallback
   is loud.
2. **`reset.sh` deleted the namespace**, which under kwok takes minutes with
   ~400 pods; re-applying into a `Terminating` namespace silently applies
   nothing, producing a zero-pod fleet that renders as a plausible map. Fixed:
   delete the workload objects; `up.sh` verifies pods are running.
3. **I ran `reset.sh >/dev/null 2>&1`**, silencing the evidence.

> Six instrument failures across one phase, four of them silent. That is the
> transferable finding, not any individual defect.

---

## 5. What the review found

**23 confirmed, ~6 distinct.** The critical one was in a line I had flagged as
suspicious that morning and diagnosed the *minor* defect in.

### Provinces of different pools were drawn on top of each other — CRITICAL

`SlotKey` is `(zone, pool, ordinal)` and A1 allocates ordinals **per (zone,
pool)**; `province_y` read `k.ordinal` alone. The Nth node of every pool in a
zone landed on one cell — **42 of 100 churn-fleet nodes drawn underneath
another**, never visible, never clickable, `region_at` naming the wrong node,
blast radius cascading the wrong node. Fires on any cluster with two nodepools
in a zone; kind is single-pool, so the dev loop could never surface it, and
**every A2 fixture was single-pool** (`fx::node` sets no pool label) so the
mutation floor could not reach it either.

Fixed by making the ordinal **zone-wide** — the collision is unrepresentable
rather than managed. Pool stays in the key as slot *identity* (which vacancies a
node may reclaim), not a private numbering space.

### Four consumers still encoded the old meaning of `y` — THE PATTERN

A2 changed what province `y` means — dense/accumulated/origin-at-1 → sparse,
ordinal-strided, arbitrary-origin — and I did not audit what depended on the old
invariants.

| Consumer | Assumed | Consequence |
|---|---|---|
| `Coast::new` | `h = Σ province heights` | Southern provinces clamped into the cape taper; their land hit-tests as ocean |
| the city keep-out | rows within that window | Settlements drawn in open water, still tooltipping as a workload |
| `Continent.y = 1` | first province at row 1 | Zone label and coastline origin adrift |
| `WorldModel.width` | `continents.last()` is eastmost | Real land outside `bounds`: painted, un-hoverable, un-clickable, never framed by `F` |

**Not one defect was in the new code's own logic.**

### Cities stacked onto one cell — and the half I missed

`h` used to grow as `2 + 2·cities.len()`; A2 fixed it from capacity and
**clamped** the overflow onto the last row. A stacked city is painted under its
neighbour and `region_at` returns the first match, so it had no clickable cell
anywhere. Placement now *finds* a free cell, and an **exact cell match outranks
a neighbour's forgiveness ring**.

The completeness audit then found I had closed only half the class: **coast
markers moor at the city's row with a column that ignores the city's column**,
so two cities now legitimately sharing a row emitted their harbour and gate on
the *same cell*. Worse than the city case — painters draw in order so the last
is visible, `coast_at` returns the first, and a coast hit opens `m.workload`, so
the anchor on screen belonged to one workload and clicking it opened another.
Fixed in the follow-up patch; markers now take a free column in the ocean strip.

> The invariant worth stating outright, since it has now been violated three
> times in one phase: **what is painted at a cell and what resolves there must
> be the same object.** Painters take the last entry; `region_at` / `coast_at`
> take the first.

### `province_y` fabricated ordinal 0

`map_or(0, …)` invented a coordinate for a node `assign_layout` had
*deliberately* left unplaced — re-creating one level up the collision the layout
engine refused to create. Standing question 2, third round running.

### Ghost ground: an acceptance criterion claimed and unmet

The first report said "**Ghosts render** as empty terrain, per §4." They did
not: `ghosts()` had no consumer outside tests and a vacated slot rendered as
**open sea**. The gate made it material — **7% of the map turned to ocean** per
refresh, the largest single reason it did not look still though no province had
moved.

`Continent.ghosts` is deliberately not a `Province`: a ghost has no node, so no
health, no pressure, no cities, and a stand-in `NodeTile` would fabricate the
facts the slot no longer has. Painted plain, in a colour outside the meaning
palette and outside the colour-blind funnel. `land → ocean` fell 7.1% → 0.03%.

### One instability I introduced myself

Deriving `Continent.y` from the topmost *live* province made the coast noise —
keyed on the offset from that edge — re-roll **every shoreline in the zone**
when the northernmost node departed. Now keyed on the absolute world row.

### Two of the six came out of the REFUTED pile

Ghost-as-ocean and the fabricated ordinal 0 were both refuted by the verify
pass — and the first was the largest defect of the round by measured impact. So
the ordinary "wrong today?" bar missed the biggest one here too. That is a
second data point for the standing review-bar question, not a counter-example
to A0's.

---

## 6. Still open in the code

Not defects with a fix pending — decisions that need making.

**`ExtentSource` has no consumer.** Zero hits outside `world.rs`. §3 and §7
require the fallback to be declared *and* marked; it is declared only. An
unmeasurable node draws at the default extent with nothing distinguishing it,
and the `InstanceType` and `Default` rungs are visually identical to each other
and to a measured mid-size node. The CHANGELOG overstated this and has been
corrected.

**The extent bounds never fire at the sizes they name.**
`EXTENT_BOUNDS_GIB = [32, 128, 512]` is compared against **allocatable**, which
is always below nominal — a nominal 32 GiB node reports ~30.9 and takes the
*smallest* class, and the same holds at every boundary. This is why the smallest
extent is the ordinary case on real clusters. Either move the bounds just under
the nominal sizes, or compare capacity.

**The world is roughly two-thirds ocean.** The stride is the largest extent
class (9) while most provinces are 3 or 5 rows, so around half the rows inside a
continent are unbuilt — and those reserved rows render as ocean, *exactly as
whole ghost slots did before they were painted*. Same defect, smaller scale, and
I did not see it. Open: should the stride be per-zone-max rather than
global-max? Should intra-slot reserved rows be painted like ghost ground — or
does that erase the size differences extent-from-capacity exists to show? This
also settles the long-deferred "chunkier landmasses on multi-node zones"
question **negatively**: at 100 nodes over four zones the provinces are still
thin ribbons.

**City placement is order-dependent**, since a city's cell now depends on which
siblings were placed first; and past the interior's capacity (~40 cities on the
smallest extent) it still falls back to the preferred cell and stacks. Both are
A3's to remove, and both belong in A3's gate.

---

## 7. Tests

**399 core + 90 GUI** (was 393 + 87). Every fix is mutation-verified — revert it
and a named test fails.

Three gaps the review found, all closed:

- **`node_extent_input`** — the whole capacity → instance-type → default
  selector — had no test. `province_extent` (input → class) did, which disguised
  it.
- **The layout carry** — the mechanism A2 exists to deliver — was covered by no
  test in either crate. It is now `net::build_carrying`, named so it can be
  asserted; deleting the feed-forward moves a survivor from y=1 to y=19.
- **"Byte for byte"** compared four numbers for two nodes. It now compares the
  whole structure through `Debug`.

---

## 8. What this round leaves behind

- **`--shot-seq N` / `--shot-interval S`** — numbered captures from one session.
  The only regime in which the carry is observable.
- **`hack/churn/gate.sh`** — the flipbook, with the no-op guard.
- **`fx::node_in_pool`** — makes the entire multi-pool defect class testable.
- The gate method above, and the six instrument failures.

---

## 9. Decisions for the room

### The kill point has moved from A2 to A3

§5 places it at A2 because A2 is the first moment §1's spatial-memory claim is
testable. It is only **half** testable while cities move — and §3.1a calls
cities the *more* important half, since cities are what people hunt for. So A3's
gate is now the kill point, and putting A4 first defers the decision the
workstream exists to force.

**Ask:** A3 next, to reach the real gate? Or A4 first, accepting the deferral?

The honest input is missing either way: **cities were not quantified.** Provinces
got four measurements and 0.41%; cities got one sentence. The settlement delta
should be measured from the same committed frames, as a share of map area
against the 0.41%, so the ordering is decided on a number.

### A3's gate has no instrument

All six churn scenarios are node-level; none churns workloads. A3's gate — *"adding
a workload to a node moves no existing city, anywhere"* — needs a seventh
scenario (add or scale a deployment on a settled fleet). That is a prerequisite
for A3, not part of it.

### The baseline comparison is outstanding

§6 called the before/after against pre-A2 *"the entire argument for this
workstream"* and warned it cannot be reconstructed later. The first pair was
withdrawn as non-comparable and not re-taken, so 92.66% is an absolute with
nothing to contrast. It is still reconstructable — cherry-pick `--shot-seq` onto
the v1.6.0 tag and re-run `gate.sh` — but only while the tag and harness are to
hand, and it should happen before A3's gate needs something to be read against.

### Two settled decisions naming A2 are now owned by no phase

- **The migration cataclysm** (§6: "first run after A2 remakes the world once.
  Declare it") has no in-session meaning until A4 supplies persistence. Reassign
  it explicitly to A4.
- **`region ← pool ∩ zone`** shipped in neither form: pool has no renderer at
  all, so the row is *unimplemented*, not regressed. Zone-wide ordinals mean
  pools interleave positionally rather than banding, so contiguity would now
  need durable band ordinals — but plan §3.4.4 already chose colour and label
  over contiguity, which is the cheaper route to the row.

**Ask:** assign both.

### The standing questions were not the failure — running them was

Both headline defects were **predicted by name and location**. Decomposition §7
says to ask *"where does a summing step precede a comparing step?"* —
aggregate-into-pools then compare-ordinals is precisely what `province_y` got
wrong. Guidance §9's question 3 named the ghost divergence and even prescribed
the two-adjacent-ghosts fixture that is now
`adjacent_ghosts_leave_their_ground_empty`.

So a fourth standing question is probably the wrong response. The question is
how the existing three become a **checked step** rather than a remembered one.

**Ask:** make the standing questions an explicit pre-review checklist with a
written answer per phase?

### Instance-type as a pool fallback — still open from A1

Unchanged: a node whose instance type changes vacates its slot, because its pool
is then a hardware attribute. One line either way.

### On the estimate

§1 and §10 costed the `Models` threading sweep as the bulk of the phase — "the
part that has broken every previous estimate in this series". It cost **two call
sites**: of ~33 `Models::build` sites exactly one is production, and the
guidance's own suggested wrapper covered the rest unchanged. The overrun was
instead a full second pass for the review fixes and the re-taken gate.

Transferable: **a grep count is not an estimate input until production and test
callers are separated.**

### Review agents writing into the working tree

Fourth round raising it; this round they were told to use `/tmp` worktrees and
did. `git status` stayed clean throughout. **Resolved** unless it recurs.
