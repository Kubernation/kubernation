# Two-thirds ocean — the reserved rows

**Guidance:** `docs/kubernation-two-thirds-ocean-guidance.md`
**Version:** 1.28.0 · **Date:** 2026-08-19

**Lever B only.** Ground the layout has reserved is now painted as reserved
ground instead of open sea. **Ground rose 33.1% → 55.0% of the play area
(+65.9% relative)**, no live land was lost, and provinces are still readable as
different sizes — legible now by height as well as colour.

Lever A (the stride) was **not taken**, per §2.3: measure B first, and pay A's
cross-zone cost only if B is insufficient. It is not.

---

## 1. §1 — claims verified, and claim 3 was not stale

All eight verified at source. Claim 3 was flagged stale and **re-measured
first**, as §7's opening acceptance item requires.

```
extent class distribution, post-v1.20.0:   h=3: 30   h=5: 54   h=7: 16   h=9: 0
```

**Identical to A6's pre-v1.20.0 figures.** The reason is one the
extent-headroom round already established: `EXTENT_HEADROOM` promotes *boundary*
machines — nodes reporting slightly under a nominal size — and kwok reports exact
round numbers, so no node on this fleet is near a boundary. A code change having
landed does not mean a measurement moved.

So the gap was exactly as described: **47.6% of the slots' own rows are
reserved**, and no province reaches the stride.

### 1.1 The geometry, in cells

Computed from `--dump-positions`, and it reproduces A6's bounding box and both
of its land figures exactly — which is what makes it trustworthy as a decomposition
of the third:

| | cells | share |
|---|---|---|
| land | 12,272 | 28.9% |
| ghost ground | 1,560 | 3.7% |
| **reserved (in-slot)** | **11,128** | **26.2%** |
| ocean | 17,496 | 41.2% |

A6's 67.4% "ocean" is **26.2 points of reserved ground plus 41.2 points of real
sea.** That is the size of the item, and it is what lever B reclaims.

---

## 2. §2 — the decision, made by looking

**Lever B, with a distinct treatment. Lever A not taken.**

`Coast::insets` is pure noise over the continent's whole row span — it already
treats reserved rows as land-shaped, and nothing was painting them. So B is a
band paint of exactly the shape `draw_ghost_ground` already uses.

**Three things were decided in front of the map, not in advance:**

1. **A distinct material, not land.** §2.2's risk is real: if reserved rows
   looked like land, a class-3 and a class-9 province would occupy the same
   visible ground and capacity would stop being legible. A separate treatment
   keeps the green thickness meaning what it meant.
2. **The tone was tuned by looking, twice.** The first attempt
   (`0.22, 0.26, 0.27`) was structurally right and **too heavy** — it framed
   every band in near-black and competed for attention it has not earned. The
   shipped value recedes.
3. **Not lifted under `Relief`.** A ghost is ground in its own right and rises
   with the land; this is the part of a plot its node does not occupy, so leaving
   it at sea level makes the province stand *proud* of it. Extent is then legible
   as height as well as colour — which is half the gate, bought for free.

**Distinct from ghost ground because it is a different fact:** a ghost says *a
node was here*; this says *this plot is bigger than its node*. Ghost ground was
not touched (§3).

---

## 3. §5 — the gate

**Failure criteria, stated before the run** (§5.3): provinces of different
extents no longer distinguishable; reserved ground reads as live land; the map
busier without being more informative; reserved rows interacting badly with the
lift under `Relief`. **None occurred.**

### 3.1 The measurement, and the trap §5.2 named — which fired twice

```
class = ground (land ∪ reserved)      before → after
  ground footprint    33.139%  →  54.987%    +21.8 points, +65.9% relative
  ground lost                0     0.000%    no live land was taken
class = reserved
  reserved footprint   0.000%  →  21.597%    absent before, present after
```

**§5.2's discrimination check is the `reserved` row itself**: the class is
*exactly zero* in the pre-change build, so the metric is measuring the change and
not something incidental to it.

And the trap fired **twice**, both times as §5.2 warned:

1. `compare.py`'s land class is `g > b`. Reserved ground is a cool grey whose
   **blue exceeds its green**, so without its own class it would have counted as
   **sea** — and the gate would have reported no improvement at all.
2. The first classifier listed only the pair's two shades and caught **a fifth**
   of the ground it was measuring (reserved read 4.2% where it is 21.6%).
   `land_diamond` does not paint the pair flat: it picks a shade by
   checkerboard and adds `cell_jitter` as `(d, 1.3d, d)`. `cell_jitter` returns
   one of exactly **five** values, so the class is 2 × 5 = **ten exact colours,
   enumerated from the same constants the renderer uses** — not sampled from a
   frame, which would drift with the palette, and not a range, which would start
   catching ghost grey.

Caught by decomposing the transitions rather than trusting the total: a
`before → after` histogram showed sea becoming a *family* of greys, only two of
which the classifier knew.

### 3.2 `Relief` with a class-9 province (§5.3's fourth criterion)

`hack/churn/bigmem.sh` (reversible, additive) put two nominal-512 GiB nodes on
the fleet, giving `h=9` for the first time:

```
extent classes with bigmem:  h=3: 30   h=5: 54   h=7: 16   h=9: 2
churn-hpc-000  h=9  ordinal=15     class 9 fills its slot → no reserved band
```

Verified on screen: the two class-9 provinces render as thick land with **no
shelf at all** — `reserved_band` returning `None`, visible — while their
neighbours' lifted land stands above flat shelves with cliffs intact. The fleet
was restored to its 100-node reference state afterwards.

---

## 4. §4 — tests and the mutation floor

- reserved ground is ≥ 0.20 from live land, ghost ground **and** sea in RGB,
  with its own dither pair asserted *close* so the separation cannot be faked by
  making the fill noisy
- a reserved band covers exactly the slot rows the node does not fill, is `None`
  when the band fills its slot, and is clipped at the continent's southern edge
- **§2.2's risk as a test**: every class leaves `SLOT_STRIDE - h` reserved rows,
  so land-row counts stay distinct per class — capacity stays legible
- reserved bands join `terrain_order`'s single back-to-front sequence (claim 5),
  with the band count asserted so one cannot be silently dropped

| | mutation | |
|---|---|---|
| M1 | reserved rows paint as sea (§4's named mutation) | caught |
| M2 | reserved ground takes the sea's colour | caught |
| M3 | reserved bands dropped from the paint order | caught |
| M4 | the band runs past its own slot into the next | caught |

Each asserted applied — present **and compiling**.

---

## 5. §6 — standing questions

**1. Summing before comparing?** The A6 figure summed land and ghost and called
the remainder ocean, which is how 26 points of reserved ground hid inside it for
two rounds. §1.1 separates them before comparing.

**2. Unknown, or fabricated?** `reserved_band` returns `Option` and says nothing
when a band fills its slot or lies past the coast — rather than a zero-row band
that would paint an empty run.

**3. Two sections constraining one behaviour?** §2.1 (pack tighter) and §2.2
(keep sizes legible) pull opposite ways over the same pixels. The fixture where
they diverge is a zone holding both a class-3 and a class-9 province, which
§3.2's bigmem run produced — and the shipped answer satisfies both because the
reserved ground is a *different material*, not more land.

**4. Consumers depending on an old meaning?** `terrain_order` gained a parameter,
so the compiler found all four call sites. `Band` gained a variant, which made
one exhaustive match fail loudly rather than silently ignoring reserved bands.
The stride was not touched, so question 4's consumer list (`slot_row`,
`slot_of_row`, the graticule, the minimap, `province_ring`) is untouched — that
list is lever A's cost, and lever A was not taken.

**5. Inherited claims?** Claim 3 was flagged stale and **is not** (§1) — the
opposite of the expected failure, and worth recording as such: a code change
landing between two measurements does not imply the measurement moved.

**6. One side of a comparison moved?** Yes, and it is §3.1: adding painted
ground changed what the classifier had to recognise, and the classifier had to
move with it. It did not, twice, and both times the number was wrong in the
direction that would have understated the change.

**7. Container adjacency read as world adjacency?** `terrain_order` sorts by row
rather than by container position, and reserved bands are keyed on their own top
row — the same discipline the consolidation round established for ghosts.

---

## 6. §7 — acceptance

- [x] Claim 3 re-measured post-v1.20.0 before anything was decided (§1)
- [x] §2's decision recorded, with what was looked at (§2)
- [x] Reserved ground visibly distinct from land, ghost and sea — asserted (§4)
- [x] Extent still legible — asserted, and doubly so under `Relief` (§2, §3.2)
- [x] Gate run with `compare.py`, before and after, with a reserved classifier
- [x] Discrimination check run — the class is absent from the "before" build
- [x] Failure criteria stated before the run (§3)
- [x] `Relief` checked with a class-9 province present (§3.2)
- [x] Mutations asserted applied (§4)
- [x] Standing questions answered, claims tagged
- [x] `cargo nextest run --workspace` green — 595 tests

431 core + 136 GUI tests; gui-smoke 57 states; `compare-selftest.py` green.

**Lever A remains available and unneeded.** If the complaint ever becomes the
map's *height* rather than its holes (§3 of the guidance is explicit that those
are different problems), that is the lever — and question 4's consumer list is
its real cost.
