# The Cutaway Fork

**Experimental fork report** · 2026-07-30 · **not merged**
**Branch:** `fork/cutaway` — committed, lint-clean, unmerged, nothing proposed for merge
**Gate detail:** [`cutaway-gate-1.md`](../cutaway-gate-1.md) · [`cutaway-gate-2.md`](../cutaway-gate-2.md)

Two rounds testing whether opening a node to show what runs beneath it beats the panel
we already have. **It doesn't** — and the reason turns out to be a hard limit on the
map itself, not on the idea.

| Gate 1 | Gate 2 | Cost | Shipped |
|---|---|---|---|
| Pivot | Stop | ≈1 day | v1.4.0 |

---

## The finding that transfers

The map's provinces **tile the plane edge-to-edge**. There is no free space around a
province to render vertical information into — anything drawn vertically is either
occluded by its neighbour, or occludes it.

> **Corollary: on this map, vertical encoding is available only where land meets sea.**
>
> Per-node quantities must live on a province's *surface* — colour, texture, marks —
> which is exactly what the overlay axis already does.

This is worth more than the feature would have been. It explains something already
shipped: `MapStyle::Relief` works because at 7 px its cliffs **only ever overhang
water**. Relief isn't a small version of the cutaway idea — it's the only version the
geometry permits, and it survives precisely because it never tries to carry per-node
data.

```
AT THE COAST — face visible        IN THE INTERIOR — face hidden
  open sea, nothing to occlude       next province drawn after → covers it
```

Verified at zoom 1.3 on a 100-node fleet: twenty provinces visible by name, not one cut
face between them.

---

## How we got there

### Round 1 — Pivot · "lift a slab, reveal the floor"

Excavating one node worked on screen and answered "which node is this?" unambiguously
at every zoom. But it showed **less** than the node panel: three anonymous shapes where
the panel lists pods by name.

The blocker was geometric. Lifting straight up reveals a band of fixed height, while a
province's on-screen height grows with its size — so you see a sliver, and no amount of
extra lift fixes it without throwing the slab off-screen.

The most informative moment wasn't on screen: `kubectl` naming the node's three
DaemonSets. **The gap was names, not geometry.**

### Round 2 — Stop · "encode height instead"

The correction was sound: make the plinth's height the payload, so the cut face becomes
the display surface. Tall = hollow; a fleet's waste read as a skyline.

On a four-node cluster it worked exactly as designed. At a hundred nodes it collapsed —
provinces in a zone abut, so each cut face is covered by the next province drawn.

> **The scale at which a fleet overview would be worth having is the scale at which the
> display surface disappears.**

---

## Secondary finding — for any "encode a ratio as a size" idea

| Cluster | Range of node utilisation | Implication |
|---|---|---|
| Dev (4 nodes) | 0 – 19 % used | Idle bunches at 81–100 %; a linear size map is flat |
| Fleet (100 nodes) | 9 – 11 pods/node | Near-uniform; range-normalising is mandatory, and costs cross-cluster comparability |

Real clusters are over-provisioned. The interesting variation always sits in a narrow
band near one end, so anything sized by a ratio has to be normalised against the fleet —
which means the sizes stop meaning anything across clusters or over time.

---

## What shipped anyway

- **SUBSTRATE in the province window** — the DaemonSets on a node *by name*, paired with
  kubelet Memory/Disk/PID pressure. The content both rounds were reaching for, readable
  at any zoom, on any cluster size. Released as v1.4.0.
- **The `infra` model fix** — the world model computed the DaemonSet names and threw them
  away, keeping only a count. Restored; a straight improvement whatever renders them.

Both went to `main` independently of the fork and survive it.

---

## What this rules out

- **Lateral offset** — an exploded view still needs free space beside a province.
- **Translucency** — layering two textured surfaces reads as mud, and fights the flat art
  direction.
- **Shrinking the slab** — breaks the isometric grid, so it stops reading as the same map.
- **Any per-node vertical encoding** — same constraint, whatever shape it takes.

These are ruled out by the **tiling**, not by execution quality, so none is worth a second
attempt without first changing how provinces are laid out.

---

## What would change the answer

One thing, and it's structural: **gaps between provinces.** If the layout gave each node
breathing room rather than tiling edge-to-edge, the whole vertical family reopens at once.
That's a change to the world model's geometry, not to a renderer — worth naming as a fork
in the road rather than a feature.

Short of that, per-node quantities belong on the overlay axis, where the Cost view's
choropleth already demonstrates the right shape for this exact data.
