# A2 — Wire the layout in

**Implementation report** · 2026-08-02 · **v1.7.0**
**Commits:** `74d969d` (first pass) · this round (review fixes + the real gate)
**Governing docs:** [`kubernation-a2-wire-layout-guidance.md`](../kubernation-a2-wire-layout-guidance.md) · [`kubernation-workstream-a-decomposition.md`](../kubernation-workstream-a-decomposition.md) §4
**Gate evidence:** [`a2-gate/`](a2-gate/) — 6 frames from one session across a 30-node rolling refresh

> **This report supersedes the version written at `74d969d`, which was wrong.**
> That version answered the gate "yes, 1.3% → 0.0%, frames pixel-identical."
> The adversarial review found the flipbook could not see the mechanism it
> claimed to validate, the two numbers were not comparable, and the map it
> photographed had 42 of 100 nodes drawn invisibly on top of each other. The
> corrections are in [§1](#1-the-first-answer-was-wrong-three-ways) and the
> real answer is in [§2](#2-the-gate-answered).

---

## 1. The first answer was wrong, three ways

**The flipbook was blind to the mechanism.** Every frame was its own
`cargo run … --screenshot` process, so each one started from
`Layout::default()` and assigned from scratch. Assignment is deterministic in
the node set, so a process-per-frame flipbook renders identically **whether or
not the carry exists at all** — delete `prior_hot = hot_models.layout.clone()`
and the evidence is unchanged. It measured determinism, which was never in
doubt. And it erred flattering: from scratch a replacement inherits the
departed node's ordinal and the map looks perfect, while the long-lived session
the product actually runs in leaves the slot reserved and appends below it.

**The two numbers were not comparable.** A2 roughly tripled the vertical
stride, so the same `--center`/`--zoom` covered about a third as many
provinces. The before viewport contained five settlements; the after viewport
contained essentially none. Cities are the axis A2 deliberately does *not* fix
— the report said so two paragraphs later — so the unfixed axis was in the
numerator of one number and absent from the other.

**"Pixel-identical" was false.** 743 map pixels differ between the frames I
called identical, from forest re-rolls seeded on the node name.

> Third instance in one round of the failure the report's own second section
> was about. Writing "a gate whose instrument fails silently is worse than no
> gate" did not stop me publishing one.

---

## 2. The gate, answered

> **Does the map hold still?**
>
> **The provinces do. The cities do not, and that is A3's charter.**

One session, one 30-node rolling refresh on the 100-node churn fleet, surging,
from a freshly reset baseline. Same framing throughout, containing both
provinces and settlements.

| | share of map area |
|---|---|
| **Pixel-identical, start to end** | **92.66%** |
| land → ocean (ground vacated) | **0.03%** |
| ocean → land (ground taken) | 0.38% |
| changed in place (reserved ground greying) | 6.93% |

The two numbers that matter are the first two. **The land/sea silhouette moves
by 0.41% across a full third-of-the-fleet replacement** — the continents keep
their shape, and no province moved. Nearly all the remaining change is
*colour*: a slot whose node departed keeps its ground and goes grey, which is a
deliberate signal that it is reserved, not a settlement relocating.

Cities did move — settlement pixels jump at frame 10 as pods reschedule. That
is instability sources 2/3/5, which the guidance assigns to A3 and explicitly
tells A2 not to touch.

[`refresh-00`](a2-gate/refresh-00.png) → [`13`](a2-gate/refresh-13.png).

Per §8: the map now demonstrably holds still, so plan §1's spatial-memory claim
is testable for the first time. **Whether it is more useful is a different
question, and this result deliberately does not answer it.**

---

## 3. What the review found in the code

**23 confirmed findings, ~6 distinct defects.** One critical, and it was mine
twice over: I stared at the exact line the morning the review ran, called it
suspicious, and diagnosed the *minor* remote defect in it while the critical one
sat in the same expression.

### Provinces of different pools were drawn on top of each other

`SlotKey` is `(zone, pool, ordinal)` and A1 allocates ordinals **per (zone,
pool)**. `province_y` read `k.ordinal` alone. So the Nth node of every pool in a
zone landed on the same cell.

On the churn fleet — four pools, three of them spanning several zones — **42 of
100 nodes were drawn underneath another**: never visible, never clickable,
`region_at` returning a different node than the one under the cursor, and blast
radius computing the cascade of the wrong node. It fires on any GKE/EKS/AKS
cluster with two nodepools in one zone. The kind dev loop is single-pool, which
is why the dev loop could never surface it.

**Every A2 fixture was single-pool**, because `fx::node` sets no pool label. So
both behaviours were identical under test and the mutation floor could not
reach it. That is the finding behind the finding.

Fixed by making the ordinal **zone-wide**, so the collision is unrepresentable
rather than managed. The pool stays in the key as slot *identity* — it decides
which vacancies a node may reclaim — but it is no longer a private numbering
space that every consumer must remember to account for. Cost: pools interleave
positionally instead of occupying contiguous bands. Grouping them into visual
regions wants durable band ordinals of its own and should not be smuggled in as
a numbering convention.

### Four consumers still encoded the old meaning of `y`

The root pattern, and the one I should have caught: **A2 changed what province
`y` means** — from dense, accumulated, origin-at-row-1 to sparse,
ordinal-strided, arbitrary-origin — **and I did not audit what depended on the
old invariants.**

| Consumer | Assumed | Consequence |
|---|---|---|
| `Coast::new` | `h = Σ province heights` | Southern provinces clamped into the cape taper — several rendering as the same narrow sliver, their land hit-testing as ocean |
| the city keep-out | rows within that window | Settlements drawn in open water, still tooltipping as a workload |
| `Continent.y = 1` | first province at row 1 | Zone label and coastline origin adrift when the north slots are ghosts |
| `WorldModel.width` | `continents.last()` is eastmost | Real land outside `bounds`: painted, but un-hoverable, un-clickable, never framed by `F` |

### Cities were being stacked onto one cell

`h` used to grow as `2 + 2·cities.len()`, so rows could not run out. A2 fixed it
from capacity and **clamped** the overflow onto the last row, which collided
cities exactly. A stacked city is painted under its neighbour and `region_at`
returns the first match, so it has no clickable cell anywhere on the map. A
~32 GiB node takes the smallest extent, so this was the ordinary case.

Placement now *finds* a free cell instead of clamping. That turned out to be
necessary but not sufficient: the one-cell forgiveness ring from the hit-test
round meant a neighbour's ring could still swallow a city's own cell, so an
**exact cell match now outranks any ring** — the ring is a convenience for empty
ground, never a claim over occupied ground.

### `province_y` fabricated ordinal 0

`map_or(0, …)` invented a coordinate for a node `assign_layout` had
*deliberately* left unplaced rather than hand it ground a live node holds —
re-creating one level up exactly the collision the layout engine refused to
create. Standing question 2, third round running. It now expresses unknown, and
an unseated node gets no ground rather than someone else's.

---

## 4. Ghost ground: an acceptance criterion I claimed and had not met

The first report said "**Ghosts render** as empty terrain, per §4." They did
not. `ghosts()` had no consumer outside tests, `build_world` iterated live tiles
only, and a vacated slot rendered as **open sea**.

The gate is what made this more than a documentation error. With ghosts unpainted,
**7% of the map turned to ocean** across one refresh — by far the largest
reason the map did not look still, even though not one province had moved. A
rolling refresh read as the continent losing pieces of itself.

`Continent.ghosts` is deliberately not a `Province`: a ghost has no node, so it
has no health, no pressure, no cities and nothing to inspect, and inventing a
`NodeTile` to stand in would fabricate the very facts the slot no longer has. It
carries position only and is painted plain, in a colour outside the meaning
palette and outside the colour-blind funnel — it carries no severity, so it must
not borrow a colour that does. Ageing, ruins and succession stay A5's; §4's
"resist making it interesting" is the right instruction.

With it, `land → ocean` fell from 7.1% to **0.03%**.

### And one instability I introduced myself

Deriving `Continent.y` from the topmost *live* province made the coast noise —
keyed on the offset from that edge — re-roll **every province's shoreline in the
zone** whenever the northernmost node departed. The flipbook caught it as change
rippling across provinces that had not moved. The noise is now keyed on the
absolute world row, so a given row's coast is a fixed fact; only the province
that *inherits* the north edge legitimately gains the cape taper.

---

## 5. Tests

**399 core + 90 GUI** (was 393 + 87). Every fix above is mutation-verified:
revert it and a named test fails.

Three gaps the review found, all closed:

- **`node_extent_input`** — the whole capacity → instance-type → default
  selector — had no test. `province_extent` (input → size class) did, which is
  what disguised it.
- **The layout carry** — the mechanism A2 exists to deliver — was covered by no
  test in either crate. It is now `net::build_carrying`, named so it can be
  asserted; deleting the feed-forward moves a survivor from y=1 to y=19.
- **"Byte for byte"** compared four numbers for two nodes. It now compares the
  whole structure through `Debug`, over a fixture with cities in it.

---

## 6. Decisions for the room

### The review bar, again

A0 established that "does this produce a wrong result today?" cannot judge a
consumer-less phase. This round is the opposite case — A2 has consumers, the
ordinary bar applied — and it produced 23 confirmed findings including a
critical one that had been live in a shipped commit for a day.

The pattern worth extracting is narrower than the bar: **every defect this round
was a consumer that had not been re-examined after the thing it consumed changed
meaning.** Not one was in the new code's own logic.

**Ask:** add that to the standing questions? *"What changed meaning, and who
still reads it the old way?"*

### Instance-type as a pool fallback — still open from A1

Unchanged and still unanswered: a node whose instance type changes vacates its
slot, because its pool is then a hardware attribute. One-line change either way.

### Ghost ground is now a third of the map after a refresh

Honest, and the alternative was worse — but it is a lot of grey, and it does not
age. A5 owns ageing; A4 owns the declared compaction that reclaims it. Until one
of them lands, a long-lived session accumulates reserved ground indefinitely.

**Ask:** does that make A4 the next phase rather than A3?

### Review agents write into the working tree

Fourth round raising it. This round they were told to use `/tmp` worktrees and
did — `git status` stayed clean throughout.

**Resolved**, unless it recurs.
