# KuberNation — Two-Thirds Ocean

**Implementation guidance**
**Goal:** stop reserved ground inside a continent rendering as sea.
**Gate:** the same fleet, with a substantially smaller ocean share — and provinces still readable as different sizes.

**One decision has to be made before a session starts.** See §2.

---

## 0. The finding, and why it is the same defect twice

Measured during the A6 round:

```
world bounding box 116 x 366 = 42,456 cells
  land           12,272   28.9%
  ghost ground    1,560    3.7%
  OCEAN          28,624   67.4%
```

And the cause, from A2's report §6:

> The stride is the largest extent class (9) while most provinces are 3–5 rows, so about half the rows in a continent are unbuilt and render as sea.

The A6 measurement sharpened it: the classes actually present were 3 (30 nodes), 5 (54) and 7 (16). **No province on that fleet reached the stride**, so every slot had at least two empty rows beneath it.

**This is the same defect A2 already fixed once, at a different scale.** Ghost ground — a whole vacated slot — used to render as ocean, and A2's report called that *"the largest single reason a refresh did not look still though no province had moved"*, dropping land→ocean from 7.1% to 0.03% once ghosts were painted.

Intra-slot reserved rows are the same thing one level down: **ground the layout has reserved, drawn as though it were not there.**

---

## 1. Verify before building

All `[A]`. VOR was unavailable. The extent numbers changed in v1.20.0, so the distribution below is **stale by construction** — re-measure it first.

| # | Claim | Source |
|---|---|---|
| 1 | `SLOT_STRIDE` is the largest extent class; province `y` is `1 + ordinal * SLOT_STRIDE` | A2, A6 |
| 2 | `EXTENT_CLASSES = [3, 5, 7, 9]` | consolidation |
| 3 | Pre-v1.20.0 distribution: 3 → 30 nodes, 5 → 54, 7 → 16, **9 → none** | A6 §1 |
| 4 | v1.20.0 added `EXTENT_HEADROOM = 0.08`, which **promotes boundary machines** and made class 9 reachable | item A |
| 5 | `terrain_order` owns the paint sequence over land **and** ghosts, sorted back-to-front | consolidation item B |
| 6 | Ghost ground is painted in a colour outside the meaning palette and outside the `cb_*` funnel | A2 §5 |
| 7 | `Continent.ghosts` is deliberately not a `Province` — a ghost has no node, so no health, no cities | A2 §5 |
| 8 | `compare.py` is the committed comparator, with a land classifier and a play-area crop | measurement session |

**Claim 3 is stale.** v1.20.0's headroom promotes nodes at nominal boundaries, so the distribution has shifted upward and class 9 now fires. **Re-measure before assuming the gap is the same size.** It may be smaller — or the ocean share may be unchanged because the stride moved with it.

---

## 2. The decision, and it is yours

Two independent levers, and they interact.

### 2.1 Lever A — the stride

**Global-max (today):** every slot is `EXTENT_CLASSES.last()` rows tall, so a 3-row province wastes 6.

**Per-zone-max:** each continent's stride is its own tallest province. A zone of uniformly small nodes packs tightly; a zone with one large node still spaces to fit it.

The cost is that **provinces are no longer comparable across zones by position** — the same row offset means different things in different columns. A6's graticule labels rows by *slot ordinal*, not by screen row, so references survive. But anything reading screen geometry across zones does not.

**Per-slot (stride = that province's own extent):** tightest packing, and it breaks the property A2 built deliberately — *"the stride is uniform so a slot's ground never depends on its neighbours' size."* A province changing extent would move every province below it. **That is the instability Workstream A spent eleven versions removing. Do not take this option.**

### 2.2 Lever B — the reserved rows

**Paint them like ghost ground.** They are reserved, not absent — exactly the argument A2 used for ghosts.

The risk, from A2's own report: **it may erase the size differences extent-from-capacity exists to show.** If a 3-row province and a 9-row province both occupy 9 rows of visible ground, capacity is no longer legible from the map.

Unless the reserved ground is *visibly* reserved — a different treatment from live land, as ghost ground already is — in which case the province's own extent stays readable and the continent stops being holed.

### 2.3 What I would take, and why it is not my call

**Lever B, alone, first.** It is the smaller change, it reuses a treatment that already exists and was already argued, and it addresses the visual complaint directly — a continent full of holes — without touching the layout invariant.

Then re-measure. If the ocean share is acceptable, lever A is unnecessary and its cross-zone cost is not paid.

**But this is an aesthetic judgement about the live map**, and A5-render established that this class of decision has to be made in front of the map rather than in advance. §2.2's risk in particular — does painting reserved rows erase the size signal? — is not answerable from a description.

**So: decide B's treatment by looking, and decide A only if B is insufficient.**

---

## 3. What this is not

- **Not a fix for the map being tall.** The world is 366 rows because 100 slots × a stride of 9 is 900-odd; lever A shortens it, lever B does not. If the complaint is height rather than holes, that is lever A and it should be said
- **Not a change to extent.** v1.20.0 settled the calibration
- **Not a change to ghost ground.** It already works and is the model here

---

## 4. Tests

- [ ] Reserved rows are distinguishable from live land, from ghost ground, and from sea — asserted on colours, not eyeballed
- [ ] A province's own extent is still readable when its reserved rows are painted — the §2.2 risk, as a test
- [ ] If lever A is taken: a province's `y` still derives from `slot_row`, and A6's references still resolve
- [ ] The paint order still holds — `terrain_order` sorts land and ghosts together (claim 5); reserved rows join that ordering or state why not

**Mutation floor, asserted applied** — six false survivals this session from `cargo fmt` reflowing targets. Make reserved rows paint as sea and confirm a test fails.

---

## 5. The gate

**Same fleet, substantially smaller ocean share — and provinces still readable as different sizes.**

Both halves. A change that fills the ocean by making every province look the same has traded one problem for a worse one.

### 5.1 Measure with the committed comparator

`compare.py` (claim 8), not a new one. Fourteen instrument failures in this workstream.

Report the same three figures A6 did — land, ghost ground, ocean — as shares of the world bounding box, plus a new row for reserved ground. **Before and after, on the same fleet, same framing.**

### 5.2 The discrimination check

Run it against a build with the change disabled. If the ocean share does not move, the change is not doing what it claims.

And check the metric can discriminate first, per D1: `compare.py`'s land classifier is `green > blue`, which covers terrain, sand and ghost grey. **Reserved ground needs its own classification** or it will be counted as one of those and the number will be wrong in an unknown direction.

### 5.3 Failure criteria, stated in advance

- Provinces of different extents are no longer distinguishable
- Reserved ground reads as live land
- The map is busier without being more informative
- Under `Relief`, reserved rows interact badly with the lift — class 9 now fires (claim 4), which is the case the occlusion work was verified against

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 5 is live and specific:** claim 3's distribution predates v1.20.0's headroom. Re-measure rather than inherit — the item's size may have changed.

**Question 4, if lever A is taken:** the stride is read by `slot_row`, `slot_of_row`, A6's graticule, the minimap and `province_ring`. A per-zone stride makes it a function of the continent rather than a constant, and every one of those must still agree.

**Question 3:** §2.1 (pack tighter) and §2.2 (keep sizes legible) constrain the same pixels and pull opposite ways. The fixture where they diverge is a zone containing both a class-3 and a class-9 province.

---

## 7. Acceptance

- [ ] Claim 3 re-measured post-v1.20.0 before anything is decided
- [ ] §2's decision recorded, with what was looked at
- [ ] Reserved ground visibly distinct from land, ghost and sea
- [ ] Extent still legible — asserted, not assumed
- [ ] Gate run with `compare.py`, before and after, with a reserved-ground classifier (§5.2)
- [ ] Discrimination check run
- [ ] Failure criteria stated before the run
- [ ] `Relief` checked with a class-9 province present (§5.3)
- [ ] Mutations asserted applied
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 8. Estimate

**Half a day for lever B.** One to two days if lever A is taken as well, because the stride stops being a constant and question 4's consumer list is real.

Take B, measure, and only then decide about A.
