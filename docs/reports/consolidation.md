# Consolidation — three of four items, and a stop

**Guidance:** `docs/kubernation-consolidation-guidance.md`
**Version:** v1.18.0 · **Date:** 2026-08-07
**Status:** items B, C and D landed. **Item A stopped at §0** — its preferred fix
rests on a premise that measurement refutes, and its acceptance criterion is
unsatisfiable on any cluster we can build. §5 has the finding and a recommendation.

450 core + 112 GUI tests; gui-smoke 55.

---

## 1. §0 — claims verified

Six of eight were `[A]`. All eight checked against source this round; the two
`[A]` claims that carry the most weight (3 and 7) were re-derived rather than
accepted.

| # | Claim | Verdict |
|---|---|---|
| 1 | `EXTENT_BOUNDS_GIB = [32,128,512]`, class = `filter(gib >= b).count()` | **TRUE** — `world.rs:538,555` (guidance said 537,558; ~3 lines of drift) |
| 2 | `EXTENT_CLASSES = [3,5,7,9]`, `SLOT_STRIDE` = largest | **TRUE** — `world.rs:534,588` |
| 3 | The comparison is against **allocatable** | **TRUE** — `node_extent_input` → `node_allocatable` → `status.allocatable`. But see §5: the *consequence* the guidance draws does not occur on either cluster. |
| 4 | Both helpers have zero callers, `pub` in core | **TRUE** — verified over the whole tree; the only hits are the two definitions, my report, and the guidance itself |
| 5 | Both document a vector index as a row (`"(zone col, node row)"`) | **FALSE at HEAD** — I rewrote both doc comments in v1.17.1 (`eb95e6e`); that string no longer exists. True of the tree the guidance was written against. Does not change item C: deleting is strictly stronger than documenting. |
| 6 | `draw_world` paints `for prov in &cont.provinces` | **TRUE** — `draw.rs:1249` |
| 7 | `fill_prism` extends paint ~7px **north** of the ground | **TRUE, re-derived** — see below |
| 8 | `ExtentSource` has no consumer outside `world.rs` | **FALSE** — `main.rs:519,530` emits it in `--dump-positions`, added in A3-pre (v1.7.2), *after* the A2 audit the claim cites. Sharpens item D rather than blocking it: the consumer is a dev instrument, and the **user-facing** marking A2 asked for is what is missing. |

### Claim 7, re-derived rather than inherited

Item B rests entirely on this, and the *direction* decides the sort order, so I
read the geometry instead of quoting my own report. `to_land` subtracts
`lift_px` from screen y — the land plane is the sea plane **raised**. Inside
`fill_prism`, `drop(v) = v + (0, lift)` walks *down* to the sea-level footprint
and the cliff quads fill between. So a band's painted region spans
`[c.y − hh, c.y + hh + lift]` against a sea footprint of
`[c.y + lift − hh, c.y + lift + hh]`: **`lift` px further north, flush south.**
A southern band must therefore paint over its northern neighbour — ascending
`y`. Confirmed.

### One thing the guidance did not know: ghosts are lifted too

`draw_ghost_ground` uses `cam.to_land` and a `TILE_H + lift` margin, and the
loop order is *all ghosts, then all provinces*. So item B is not "sort the
province loop" (its stated two lines): ghost ground and land **interleave by
slot ordinal**, and two separate passes put every ghost either in front of or
behind every province regardless of where it actually is. They have to sort
together. §3.

---

## 2. What shipped

| Item | Piece | Where |
|---|---|---|
| B | `Band`, `terrain_order` — one back-to-front pass over land + ghosts | `draw.rs` |
| C | `province_index_at`, `visible_provinces` deleted (54 lines) | `state/world.rs` |
| D | `extent_line` — the marking A2's acceptance asked for | `panels.rs`, `node.rs` |
| A | **not done** — §5 | — |

---

## 3. Item B — the terrain sort

`terrain_order(province_y, ghost_y) -> Vec<Band>` is pure and takes both `y`
lists **in the model's own order**, returning the paint sequence. As with
`pool_label_pieces`, the caller does not compute an order, so it cannot compute
a wrong one — the same discipline, for the same reason: neither input is in map
order (`Continent.provinces` is `fnv1a64(name)`; `Continent.ghosts` comes out of
a `BTreeMap` keyed `(zone, pool, ordinal)`).

**Mutation floor, exercised — three, all caught:**

1. Drop the sort (paint in model order) — the shipped behaviour.
2. Two separate passes, each sorted (ghosts then land) — the shape the old code
   had, and the one a "sort the province loop" reading would have produced.
3. Flip the tie-break.

Mutation 2 is worth recording because my **first attempt at it was neutralised
by the code it left behind** — I replaced the band construction but left the
`sort_by_key` in place, which re-sorted correctly, and the suite reported `ok`.
A mutation that does not actually change behaviour is a false negative about
test quality, which is the same failure mode as an instrument that measures
something else. Re-run properly, it fails.

The occlusion itself stays unassertable (it is pixels), which is exactly why the
*order* is asserted instead — the guidance is right about that, and §6's "there
is no mutation for the occlusion itself" is the correct framing.

---

## 4. Items C and D

**C.** Deleted. Claim 4 verified first, and the build is the confirmation: 54
lines out, 450 core tests green, clippy clean. Nothing referenced them.

**D — decided: a panel line, not the hatch.** Three reasons, in weight order.

1. **The hatch would carry two meanings.** `province_unmeasured` means *this
   reading has no denominator* and is gated to the ratio overlays. Extent is
   drawn under **every** overlay, so hatching it would put an unrelated second
   meaning on one texture — and on a node with no allocatable at all (which is
   both) they would be indistinguishable.
2. **It is the precedent the type itself names.** `ExtentSource`'s doc says it
   travels "exactly as `metric_source` and `pool_source` do". Both of those are
   named in the panel, not drawn on the terrain.
3. **Extent carries no cluster state.** It is scenery; a fallback size misleads
   about the machine, not about the cluster — a panel-sized claim.

`extent_line` is silent for `Capacity` (a measured size needs no caveat, the
`pool_line` rule) and distinguishes the two fallback rungs in words, which was
the actual gap: they produce the *same* extent, so nothing separated them from
each other or from a genuinely mid-sized node.

**The hatch would not have covered this anyway** — a fact found while deciding,
not assumed. `province_unmeasured` fires on `worst_known(cpu, mem).is_none()`,
and `worst_known` returns `Some` when *either* is known. A node reporting
allocatable cpu but not memory therefore gets a fallback extent and **no hatch**.

Verified live on the churn fleet's allocatable-less node: the province window
reads `province of z-a . pool sys . size from instance type - not measured`
beside hatched "unknown" gauges — pool, size and ratios each marked by their own
mechanism, none overloading another.

---

## 5. Item A — stopped, with the measurement

### 5.1 The guidance's preferred fix rests on a false premise

> *"Compare against capacity rather than allocatable. Principled — capacity **is**
> the nominal size"*

**Measured: it is not.** kind reports

```
capacity = allocatable = 15.653 GiB   on a nominally 16 GiB VM   (2.2% short)
```

Capacity is below nominal for the same reason allocatable is — firmware and
reserved RAM — so switching the comparison would change nothing and fix nothing.
On both clusters we can build, `capacity == allocatable` exactly; the kubelet
reserves nothing by default in either kind or kwok, so the allocatable-vs-capacity
gap the item is named for is **zero** here.

### 5.2 The prescribed verification cannot discriminate

§2 and §8 require before/after captures on the churn fleet. kwok reports exact
round numbers, so:

| reported | current `[32,128,512]` | guidance `[30,120,480]` |
|---|---|---|
| 16 GiB ×30 | 3 | 3 |
| 32 GiB ×29 | 5 | 5 |
| 64 GiB ×24 | 5 | 5 |
| 128 GiB ×16 | 7 | 7 |

**Identical.** The capture would show zero changed pixels. Had I run it without
checking, I would have produced a blank before/after and had to interpret it —
the trap this project has hit nine-plus times, avoided here only by arithmetic
first. kind is no better: 15.653 GiB is nominally 16, correctly class 3 under
every candidate.

So the item's own symptom — *"the smallest extent is the ordinary case … the map
is mostly thin ribbons"* — is **not what either cluster shows**. The churn
fleet's distribution is 30 × class 3, 53 × class 5, 16 × class 7: class 5 is the
ordinary case. The thin-ribbon appearance has a different cause, already on
record from the v1.7.1 audit: the **stride is 9 while extents are 3–7**, so
roughly half of every continent's rows are unbuilt. Recalibrating bounds cannot
touch that.

### 5.3 The defect is nonetheless real — just not here

A real cloud node whose nominal size sits *at* a bound reports below it and
falls a class, which contradicts the documented intent ("a node at or above the
Nth bound gets the (N+1)th height"):

| | current | `[30,120,480]` | `[24,96,384]` |
|---|---|---|---|
| ~32 GiB reporting 30.9 | **3** ✗ | 5 ✓ | 5 ✓ |
| ~128 GiB reporting 123 | **5** ✗ | 7 ✓ | 7 ✓ |
| genuinely 24 GiB | 3 ✓ | 3 ✓ | **5** ✗ |
| genuinely 96 GiB | 5 ✓ | 5 ✓ | **7** ✗ |

The mechanism is directly measured (kind, 2.2%); the magnitude on managed clouds
is **not** measured here and the ~30.9 figure is inherited from the v1.7.1 audit.

### 5.4 Recommendation

Neither of the guidance's two options is right: capacity is refuted, and a
midpoint scheme promotes genuine in-between machines (24, 96 and 384 GiB are all
real instance sizes). Prefer an explicit, named headroom:

```rust
/// Reported memory runs below the machine's nominal size — firmware/reserved RAM
/// (measured: kind reports 15.653 GiB on a nominal 16 GiB VM) plus any kubelet
/// reservation (0 on kind and kwok, larger on managed clouds). Classify with that
/// headroom so a node AT a nominal boundary does not fall to the class below,
/// while a genuinely in-between machine still classifies honestly.
const EXTENT_HEADROOM: f64 = 0.08;
let class = EXTENT_BOUNDS_GIB.iter().filter(|b| gib * (1.0 + EXTENT_HEADROOM) >= **b).count();
```

It fixes every row of the table above without breaking one. **Verify with a
synthetic boundary node, not a fleet capture** — the fleets cannot show it.

I did not implement it: the guidance's rationale is refuted, its acceptance is
unsatisfiable as written, and the headroom value is a judgement about real
hardware that should be made deliberately rather than folded into a stop report.

### 5.5 §1's causal link survives, and is now defused

*"Fixing the extent calibration activates the occlusion risk"* is correct — class 9
needs a node above the top bound, which is what the calibration would enable. §1's
prescribed ordering (sort first) was right, and since **item B has landed while A
has not**, the link is already broken in the safe direction. Whenever A is done,
it can be done alone.

---

## 6. Standing questions

**1. Summing before comparing?** Not present.

**2. Unknown or fabricated?** Central to item D and answered by shipping the
marking: a fallback extent now says so instead of passing as a measurement. The
gap found while deciding — a cpu-only node escaping the hatch — is closed by the
panel line, which is ungated.

**3. Two sections constraining one behaviour, and a fixture where they diverge?**
Yes: §3's "sort the terrain pass" and §5's hatch/scenery question both bear on
how a province is drawn. They diverge exactly at ghost ground, which §3 does not
mention and which §2's own reasoning requires to be sorted with the land.

**4. What depends on the old meaning of a value this change redefines?** Item A
is the live one, and it is stopped. For B, `terrain_order` redefines nothing —
`Province.y` is unchanged; only the order of consumption is fixed. For D,
`ExtentSource` gains a second consumer; the first (`--dump-positions`) is
unaffected.

**5. Inherited claims — and does the state each describes actually occur?** This
is the round's finding. Claim 3 is *true* and its consequence **does not occur on
any cluster we can build**. Claim 8 was true when written and was falsified by a
later phase (A3-pre) that its author had no reason to revisit. Both are inherited
from my own reports. The question earned its keep twice in one sitting.

**6. When a change moves one side of a comparison, does the other still mean the
same thing?** Item A is exactly this in miniature: `EXTENT_BOUNDS_GIB` is written
in nominal machine sizes while the left side is a reported figure. They have
never meant the same thing, which is the defect.

**7. Where does the code treat neighbouring entries in a container as neighbouring
things in the world — and what guarantees that?** *First outing, and it hit.*

- `draw_world`'s terrain pass — **guaranteed now**, by `terrain_order` owning the
  sort so no caller supplies one. Mechanism, not observation.
- **The question found something the guidance missed**: ghost ground. Asking
  "which containers does this pass treat as ordered?" surfaces `cont.ghosts`
  immediately, where "sort the province loop" does not. That is the difference
  between a mechanism-shaped question and an observation-shaped one, and it is
  the case for keeping it standing.
- Remaining honest answer: pass 2's `draw_province_features` still iterates in
  vector order. Its marks are small and sparse and do not tile, so nothing
  currently depends on the order — an *observation*, not a mechanism, and
  recorded as such rather than claimed as safe.

---

## 7. Acceptance

- [x] Terrain sort landed **before** the calibration change — and the calibration did not land
- [ ] **Calibration fixed** — stopped, §5
- [ ] **Before/after captures** — shown to be non-discriminating, §5.2
- [x] Dead helpers deleted, after confirming zero callers
- [x] §5 decided and recorded — panel line, not hatch, §4
- [x] Question 7 answered with a mechanism, §6
- [x] `cargo test` green (450 core + 112 GUI); gui-smoke 55
- [ ] Open-decisions rows: A stays open with a sharper description; B, C, D retire
