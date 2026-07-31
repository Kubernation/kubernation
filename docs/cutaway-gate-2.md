# Cutaway Fork — Gate 2

**Corrected bet built and evaluated: 2026-07-30, branch `fork/cutaway`.**
**Verdict: STOP the vertical-encoding family. The finding generalises.**

---

## The corrected bet

Gate 1 killed "lift a slab to reveal the floor" — an iso view reveals only a band
of height `lift` while a province's screen height grows as `(w + h) · hh`.

The correction looked sound: **stop putting the payload on the floor.** Encode it
in the plinth's own *height*, so the near cut face becomes the display surface —
fully visible at every zoom, on every node at once, nothing moved out of
position. Height = idle capacity, strata = saturation dims. "Tall == hollow", a
fleet's waste read as a skyline.

Built: `plinth_lift` + `draw_plinth` replacing the excavation pass, with
per-fleet range normalisation.

---

## What the build found

### 1. Absolute idle has no discriminating power (fixable)

Real clusters are over-provisioned. The dev cluster spans **0–19% used**, i.e.
81–100% idle. Mapped linearly onto height, every plinth is the same and the view
says nothing. Fixed by normalising to the fleet's observed range, at the cost of
heights no longer being comparable across clusters or across time.

Worth keeping in mind for any future "encode a ratio as a size" idea on this
map: the interesting variation is almost always in a narrow band near one end.

### 2. Provinces abut, so the cut face is occluded (fatal, structural)

On the dev cluster — one node per zone, so one province per continent — the
plinths worked. Faces visible, ballasted node visibly shorter than the empty
one, exactly as designed.

On the 100-node fleet it collapses. A zone's nodes are stacked as **adjacent
bands sharing `x` and `w`**, drawn back-to-front, so each province's south cut
face is immediately covered by the next province's slab. Verified at zoom 1.3:
twenty provinces visible by name (`perf-node-000` … `-035`), **not one cut face
between them**. The only faces that survive are at continent boundaries.

The scale at which a fleet overview would be *worth having* is exactly the scale
at which the display surface disappears.

---

## The general finding

Both Gate 1 and Gate 2 failed for one reason, and it is not about excavation:

> **The map's provinces tile the plane edge-to-edge. There is no free space
> around a province to render vertical information into.** Anything drawn
> vertically is either occluded by its neighbour (a cut face) or occludes it (a
> lift).

This also explains why `MapStyle::Relief` works: at 7px its cliffs only ever
overhang *water*, which is why `land_lift`'s doc can promise no pass-1 re-sort.
Relief is not a small version of the plinth idea — it is the only version the
geometry permits, and it survives precisely because it never tries to carry
per-node data.

**Corollary, worth recording as a design constraint:** on this map, vertical
encoding is available only where land meets sea. Per-node quantities must be
encoded in a province's **surface** — colour, texture, marks — which is what the
overlay axis already does, and why the Cost overlay's bronze choropleth was the
right shape for exactly this data all along.

---

## Verdict

**STOP.** Not "the execution needs work" — the geometry cannot carry per-node
vertical data at fleet scale, and no amount of lift tuning, lateral offset,
translucency or shrinking changes that, because they all still need free space
around a province that the tiling does not provide.

The cutaway thread is closed. Two rounds, roughly a day total, and it produced:

- a hard constraint on the map's information capacity (above), which will save
  time on the next idea that wants to grow something out of a province;
- **v1.4.0's SUBSTRATE section**, which shipped to `main` and delivers the
  content both rounds were actually reaching for;
- the `infra: Vec<String>` model fix, also shipped.

## What survives on `main`

Already merged, independent of the fork: the `Province.infra` name restoration
and the province window's SUBSTRATE section (DaemonSets by name + kubelet
pressure).

## What dies with the fork

`plinth_lift` / `draw_plinth` / `PLINTH` / `--strata`, and Gate 1's
`draw_excavation` / `EXCAVATION_LIFT` / `SUBSURFACE` / `EARTH` / `JUNCTION` /
`--excavate`. `Plane` / `to_plane` was never built — Gate 1 didn't need it and
Gate 2 removes the reason for it.

The branch is left intact for the record; nothing on it is proposed for merge.
