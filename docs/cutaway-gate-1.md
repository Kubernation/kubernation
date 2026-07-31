# Cutaway Fork — Gate 1

**Phase 1 built and evaluated: 2026-07-30, branch `fork/cutaway`.**
**Verdict: PIVOT.**

---

## What was built

`--excavate <SUBSTR>` lifts one node's slab out of the world and exposes what
runs beneath it. One extra draw pass after everything else, entirely inside
`crates/kubernation`, no core changes — as §2.2 specified, so it stays cheap to
delete.

Stack, bottom to top: subsurface plane (filled per row, exact against the ragged
coast) → DaemonSet junctions → earth curtain → the province's terrain and
settlements re-drawn lifted.

Two things the guidance didn't anticipate, both found by looking at it:

**The near-side curtain seals the hole shut.** §2.5 warns about *far*-side
curtain artifacts — walls that should be hidden behind the slab. The real
problem is the opposite. Lifting a slab straight up vacates a strip along the
province's south edge, and that strip is the *only* place the floor is visible.
Drawing the full ring's curtain covers exactly that strip, so the subsurface and
its junctions vanish. A cutaway is the inverse of `fill_prism`: a raised block
shows its NEAR faces, a hole shows its FAR ones. Fixed by drawing only the west
edge and north cap.

**Junctions had to move to the south edge.** At mid-province they render
correctly and are then hidden behind the slab. They have no real position — a
DaemonSet is on the node, not at a spot on it — so this costs no honesty.

---

## Evaluation (§2.6)

| Question | Answer |
|---|---|
| Does the excavation tell you something the node panel doesn't? | **No — it currently tells you less.** The node window's GARRISON lists pods by name with live usage. The excavation shows three anonymous hexagons. |
| Do the junctions read as infrastructure, or as decoration? | **Decoration.** Nothing about a featureless hexagon says "DaemonSet". |
| Is "which node is excavated" obvious without a label? | **Yes — unambiguously, at every zoom tested.** The strongest result of the build. |
| At what zoom does it stop being legible? | The *slab* survives to ~0.6. The *junctions* stop being readable below ~1.0 — they're 2–7px dots. |
| Does it survive a busy cluster, or only a demo one? | **Structurally, no.** See below. |
| Would you reach for this while actually debugging? | **No, not as built.** |

### The geometric constraint is inherent, not a polish problem

The revealed band is a fixed `lift` px tall, while a province's screen height
grows as `(w + h) · hh`. On the dev cluster's ~20×5 province at zoom 1.6, the
band shows roughly a quarter of the floor. To reveal the *whole* floor you would
need `lift ≈ (w + h) · hh` — about 320px there — which throws the slab clear off
the top of the screen. On a real 50-node zone it is worse in proportion.

So "see the subsurface" is **not reachable by tuning the lift**. It needs a
different form: lateral offset (a true exploded view), a translucent slab, or a
shrunk slab. That is a much bigger bet than the one this fork was scoped to test.

---

## Why PIVOT, not Stop or Proceed

§3 anticipated this outcome precisely: *"the excavation is not useful but the
subsurface content is — the substrate conditions and DaemonSet inventory turn
out to be what's missing, and they don't need a cutaway to deliver."*

The most informative moment in the whole build was not on screen. It was
`kubectl` reporting that the node runs `agent`, `kindnet`, `kube-proxy` — three
**names**. The visualization rendered three anonymous shapes, because
`Province.infra` is a `usize`. Even with Phase 2's model fix restoring the names,
the result is a labelled list of three items, drawn via a lifted slab, a curtain,
and a subsurface plane.

The information gap is **names and substrate conditions, not geometry**. The
cutaway is an expensive delivery vehicle for content that a panel section
delivers better, at any zoom, on any cluster size.

**Not Stop**, because the content question is real and now sharply defined.
**Not Proceed**, because Phase 2's 1–1.5 weeks would buy correctness for a form
that the evaluation says is the wrong form.

---

## What to salvage

Per §1, `Plane` and the `infra` model fix were the salvageable pieces — and
neither was built, because Phase 1 correctly didn't need them. What Phase 1
leaves behind is the finding itself, plus two concrete follow-ons:

1. **The `infra` model fix is worth doing on its own** (§4.2).
   `build_world` computes `HashMap<&str, BTreeSet<&str>>` of DaemonSet names per
   node at world.rs:393 and reduces it to `.len()` at :467. Restoring
   `infra: Vec<String>` is a straight improvement whatever renders it —
   `draw_road_iso` keeps working via `.len()`.

2. **A "substrate" section in the province window** — the DaemonSet inventory by
   name plus `tile.abnormal` (Mem / Disk / PID / Net). This is the pivot target:
   the content the cutaway was reaching for, in the place the operator already
   looks, with no new geometry, no plane system, and no zoom floor.

`Plane` / `to_plane` remains a good idea independently, but nothing in this
result argues for it now — it was justified by making the excavated slab
clickable, and there is no excavated slab.

---

## Cost

About half a day against the 2–3 estimated, because `province_ring` — built for
the hover marker — did the geometric heavy lifting exactly as §2.4 predicted.

The branch `fork/cutaway` is left intact and unmerged. It is reproducible with:

```
--excavate <node-substr> --center <node-substr> --zoom 1.6
```
