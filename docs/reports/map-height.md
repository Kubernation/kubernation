# The map's height — analysed, and lever A closed

**Follows:** `docs/reports/two-thirds-ocean.md`, which left lever A "available"
**Version:** 1.29.0 · **Date:** 2026-08-19

**Lever A is closed.** The goal it served — a guaranteed view of the whole world
at a scale that shows detail — **is not achievable at arbitrary cluster size**,
and pursuing it would have cost the stability Workstream A exists to protect.

What the analysis found instead was a real bug in `fit`, independent of height
and wrong at every cluster size. That is fixed.

---

## 1. The height, measured

**370 rows against 116 columns**, for 100 nodes. In the isometric projection the
on-screen bounding box scales with `W + H`, so height costs both screen axes:
span 486, of which 370 is height.

It is tall because `SLOT_STRIDE` is the largest extent class (9) while the
fleet's classes are 3/5/7 — **47.6% of every slot's rows are reserved**. That is
Workstream A's invariant working: a uniform stride is what stops a slot's ground
depending on its neighbours' size.

---

## 2. Two issues that looked like one

**The fit bug.** `Camera::fit` framed against `screen_width()`/`screen_height()`,
but the docked column (264) and top chrome (32) are drawn *over* the map. The
fitted world was 2698px wide in a 2232px play area — roughly a sixth behind the
sidebar, with the northern continents under the menu bar.

**The zoom floor.** Framed correctly, fit wants **0.279** on this fleet, below
the `clamp(0.30, 2.0)` floor. So even corrected, a 100-node world cannot fit.

The two are separable, and only the first is a defect.

---

## 3. The options, with their real costs

| | span | fit (play area) | cost |
|---|---|---|---|
| today | 486 | 0.279 → floored at 0.30 | world overflows |
| per-zone stride (lever A) | 327 | 0.415 | **reintroduces instability source 1** |
| shrink `EXTENT_CLASSES` to `[3,4,5]` | 322 | 0.422 | capacity signal compresses 3-vs-9 → 3-vs-5 |
| fully packed (per-slot) | 279 | 0.487 | rejected by A2 outright |

**Lever A carries a cost the guidance did not name.** Per-zone stride is *the
zone's tallest extent*, so a slot's ground depends on the biggest machine in its
zone: adding one class-9 node to `z-d` (tallest 3) moves all 15 of its provinces.
That is instability source 1 at zone granularity — the class of movement
Workstream A spent eleven versions removing. `bigmem.sh` would have triggered it.

**Shrinking the classes gets the same height without that cost** — H 206 versus
per-zone's 211, with the stride still a global constant, so A2's invariant and
the `slot_row` / `slot_of_row` / graticule / minimap / `province_ring` consumer
set are untouched. It is recorded here because it is the strictly better of the
two, should the question ever return. It was **not** taken: the
two-thirds-ocean guidance §3 rules extent out of scope ("v1.20.0 settled the
calibration"), and its motivation was the goal §4 retires.

**None of them reaches the Regional LOD tier at fit** on a 100-node fleet — that
needs span ≤ 272, below even fully-packed. So no option delivers "names visible
with the whole fleet on screen".

---

## 4. Why the goal is retired

The operator's reasoning, recorded because it is the decision:

> A guaranteed view of the entire world at a scale that shows all detail is not
> possible — Civilization, which this models, does not have one either. And with
> cluster sizes from one node to thousands, the guarantee could not be made even
> if it happened to work on a test fleet.

That is right, and it reframes the height from a defect into a property. A map of
an unbounded world has a zoom range, not a guarantee. **Lever A is closed rather
than deferred**, so it is not re-proposed from the earlier report's wording.

---

## 5. What was fixed

`panels::play_rect(sw, sh)` is the pure authority for the area the map is
visible in, and **`fit` now takes its view rect** rather than reading the screen.

The signature change is the point: `fit` previously read `screen_width()` itself,
so **no test could call it** — which is why it framed into the sidebar unnoticed.
It is now assertable, and asserted: the iso AABB's four projected corners must
land inside the rect it was given.

Verified live on the 100-node fleet — all four zone letters visible, zone D
inside the play area instead of under the column. The remaining edge overflow is
the zoom floor (§2), which `fit`'s doc states and the test explicitly skips
checking when clamped, rather than pretending a large fleet fits.

### 5.1 Two process notes

**Mutation M4 survived at first.** An iso scene is always twice as wide as tall
in screen space, so on any ordinary window the *width* constraint binds and a fit
that ignored the view's height passes. Closed by adding a wide, short view — an
ultrawide monitor — as the only fixture where height binds.

**A test silently stopped running.** My insertion duplicated a `#[test]` and
orphaned the neighbouring one; the suite still reported green with one fewer test
and nothing said so. Only clippy's dead-code lint under `-D warnings` caught it —
worth remembering that a test count is not self-verifying.
