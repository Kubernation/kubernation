# Measurement session

**Report** · 2026-08-03 · no product change
**Governing doc:** [`kubernation-measurement-session-guidance.md`](../kubernation-measurement-session-guidance.md)
**Instruments:** `hack/churn/compare.py` · `compare-selftest.py` · `reshuffle.py`
**Evidence:** [`a2-gate/`](a2-gate/) (A2) · [`a2-gate/baseline-v1.6.0/`](a2-gate/baseline-v1.6.0/) (pre-A2)

---

## The headline

**The metric the guidance prescribes inverts the answer.**

§3 asks for the settlement delta as a share of map area, against the province
figure. Measured:

| | share of map area | share of the class's own footprint |
|---|---|---|
| provinces (land silhouette) | 0.413% | **1.26%** |
| settlements | 0.164% | **120.5%** |

By share of map area cities look **two and a half times more stable** than
terrain. By share of their own footprint they are **ninety-five times less**
stable. Land covers 33% of this map; settlements cover 0.14%. Same data,
opposite conclusion — so the comparator now prints both, and the README says
which one to read.

And a second, larger inversion, which is the real result of the session:

> **The before/after pair is not commensurable, for a deeper reason than
> framing** — and when measured with an instrument that *can* see it, the pre-A2
> map moved 27% of its untouched provinces where A2 moves none.

---

## 1. Verification

All seven §1 claims TRUE. `a2-gate/` holds six frames; `gate.sh` refuses a no-op
run with exit 2; `--shot-seq`/`--shot-interval` produce numbered captures from
one process; the `v1.6.0` tag exists at `8bd0d34`, is genuinely pre-A1/pre-A2
(A1, A2 and its fixes are the only commits after it), builds, and has **zero**
occurrences of `shot_seq`.

**Claim 6 — the session-invalidating one — verified on a real frame before any
classifier was written.** `POP_CALM` (0.88, 0.83, 0.66) has exactly two uses in
the whole GUI, both a settlement's name banner or population chip. On the
committed frames it is 5041 px confined to x 1427–1850, y 237–935, while `CRIT`
and `WARN` are **0 px** anywhere in the crop — so no severity-tinted chip and no
attention chrome is being counted, and sand is a separate colour entirely.

One qualification: `draw_name_banner` uses `POP_CALM` whatever the severity, so a
flagged city loses its chip from the count but keeps its banner. The class
undercounts such a city rather than dropping it.

*Found in passing:* the GUI's f32 colours reach the framebuffer by **truncation**,
not rounding — `0.83 × 255` is 211, not 212. Getting that backwards matches
nothing at all, silently, which is how the first probe returned zero hits for
every constant including sand.

---

## 2. The comparator

`hack/churn/compare.py`, beside `gate.sh`. Crop and classifier are flags; the
frame pair is explicit; no tolerance; the method is documented at the top of the
file rather than in prose somewhere else.

It reproduces the A2 gate's published figures **exactly** — 92.661% identical,
0.031% lost, 0.382% gained, 6.925% changed in place — which is the cheapest
possible check that the extraction did not change what was being measured.

Beyond §2's list it also reports each class's **footprint** in both frames, and
the delta as a share of that footprint. That is not decoration: without it the
map-area share is the only figure available and, per the headline, it points the
wrong way.

### The instrument tests (§6) — all pass

```
ok   self-compare is total: 100.000% identical, 0 px otherwise
ok   known-different frames differ: 92.661% identical, class delta 0.413%
ok   a 4px shift is a large delta: 12.809% of the crop changed
ok   the crop excludes the docked column: overpainting the column and menu bar moved 0 px
```

Committed as `compare-selftest.py` rather than run once by hand — a comparator
always emits a plausible percentage, which is exactly the shape of failure A2
spent six instrument failures learning about.

---

## 3. Cities

### What was measured

From the committed A2 frames, one 30-node rolling refresh:

- **Frames 00 → 09** — the entire refresh: surge, drain, delete.
  **Settlement delta 0.000%.** Byte-identical. Node churn did not move a city.
- **Frames 09 → 13** — a new workload appears. **One incumbent city moves 20.1px
  WITHIN its own province**, and a new settlement lands on that same province.

The whole settlement delta comes from the second window. The trigger is a
**workload appearing**, not node churn — which is instability sources 2/3, A3's
charter, and a direct measured violation of A3's gate condition (*"adding a
workload to a node moves no existing city, anywhere"*).

Intra- versus inter-province came out of a land-component analysis: contiguous
land is a province, since provinces are separated by open sea. Settlement pixels
are excluded from that mask, because a banner overhanging a shore can otherwise
bridge two provinces across a strait.

> **A wrong answer I nearly published.** The first implementation compared
> component *ids* between two independently-labelled frames — different
> namespaces — and confidently reported the move as ACROSS provinces. It is
> WITHIN. Both frames must now agree, or the row reads INDETERMINATE.

### What could not be measured, and why

**Coverage is 2 of 7 workloads, and the two are structurally insulated.** `store`
and `batch` each keep their pod plurality in `burst`, a pool the refresh never
touches — so "node churn did not move them" is weaker evidence than it sounds.

There is no framing that fixes this. The world is ~120 cells wide and ~900 rows
tall; at the zoom where name plates are drawn, no viewport holds more than about
three cities, and zooming out far enough to hold them all crosses into World
scale where the GUI stops drawing the plates the classifier detects. A fit-view
capture across a second full refresh contained **zero** settlements in frame 00.

So, in claim 6's own terms: **city stability is not measurable this way at realm
scale.** What would be needed is a positional dump from the model rather than a
pixel classifier — which is a product-side change this session was scoped to
exclude.

---

## 4. The baseline

Retaken and committed. The `--shot-seq` backport onto v1.6.0 **applied cleanly**
— the insertion points are structurally identical, so it was three small edits,
not the fight §9 warned might stop the session.

| across one 30-node refresh | v1.6.0 (pre-A2) | A2 |
|---|---|---|
| land delta / footprint | 1.09% | 1.26% · 1.42% |
| settlements | 3 held, 1 new | 1 held, 1 moved within, 1 new |

Read naively that says A2 made the map *less* stable. **That reading is wrong,
and demonstrating why is the session's main finding.**

### Why the pair is not commensurable

A pixel comparison measures the **rendered** map. The instability A2 removes is a
**permutation of which node occupies which ground**. On a fleet of uniformly
healthy nodes those are not the same thing: before A2 the provinces stack
contiguously into solid landmasses, so permuting green provinces inside one
changes almost no pixels. After A2 they sit in a sparse grid where a move
vacates ground and takes other ground — so the same amount of movement registers
*more*. The metric flatters the pre-A2 layout in exactly the dimension under test.

### Measured directly instead — `reshuffle.py`

Node position before A2 was rank in `(fnv1a64(name), name)` order within the
zone. That is computable from the node names alone, so the question can be
answered without pixels at all:

```
z-a:  37 nodes,  27 untouched —  15 ( 55%) would MOVE under the pre-A2 ordering
z-b:  22 nodes,  12 untouched —   0 (  0%)
z-c:  26 nodes,  16 untouched —   0 (  0%)
fleet: 15 of 55 untouched provinces (27%) would move.
```

**15 of z-a's 27 untouched provinces moved — every one by exactly ten slots —
while the comparator reported ~1% of land area.** Under A2 the answer is 0 by
construction.

The mechanism, since the uneven result is the interesting part: **FNV-1a mixes
trailing bytes mainly into the low bits, so names sharing a prefix share their
high bits and the ordering clusters by name prefix** — with conventional naming,
by pool. A refresh rewrites a generation token mid-name and moves that pool's
whole cluster. On this fleet `sys` hashed `0xd8…` as `g1` and `0xfd…` as `g2`:
in z-b (clusters `burst`, `sys`) it was already last and displaced nobody; in
z-a (`burst`, `sys`, `edge`) it jumped from the middle to the end and pushed
every `edge` province up ten slots.

In one sentence: **renaming one pool's nodes moved a different pool's
provinces.** That is the failure A2 exists to prevent, it is invisible to a pixel
diff, and it is now measurable in one command.

---

## 5. Standing questions — written answers

**1. Where does a summing step precede a comparing step?**
In the comparator itself, twice. It sums matching pixels and divides by a crop
area — so two frames of different sizes would be summed against different
denominators; `compare()` refuses a size mismatch rather than producing a
number. And the delta is summed over a class then compared to the *map* area,
which is precisely the error in §3's prescribed metric: a small class's delta
against the whole map's area flatters it. Both figures are now printed.

**2. Does every reducer over a possibly-empty input express unknown, or
fabricate?**
An empty crop returns `None` and the CLI prints "crop is empty" and exits 2,
rather than dividing by zero or reporting a confident 0%. `DELTA / FOOTPRINT`
prints "unknown (the class is absent from frame A)" — and that fired for real:
the fit-view capture had no settlements in frame 00.

**3. Where do two sections constrain the same behaviour, and is there a fixture
where they diverge?**
§3 says measure cities from the existing committed frames; §0 claim 6 says
verify separability first and refuse a number of unknown meaning. They diverge
on the frames actually committed — the classifier separates cleanly, but the
frames hold 2 of 7 cities and both are insulated from the churn. Followed claim
6 over §3's convenience: the number is reported with its coverage stated, and
the limitation is the finding.

**4. What existing consumers depend on the old meaning of a value this change
redefines?**
Nearly vacuous as predicted, but not entirely. Nothing product-facing changed —
but the *reported metric* now has two meanings, share-of-map and
share-of-footprint, and A2's published 0.41% is the former. Printing both, and
labelling which to compare across classes, is what stops the next phase reading
one for the other.

---

## 6. Acceptance

| §7 criterion | Status |
|---|---|
| Comparator committed, method documented at the point of use | ✅ |
| Crop and classifier are parameters, not constants | ✅ |
| Instrument tests pass and are recorded | ✅ committed as a script |
| City delta reported as a share of map area, against the 0.41% | ✅ 0.164% — **and the footprint-relative figure, which inverts it** |
| Intra- versus inter-province movement separated | ✅ 1 held, 1 within, 1 new |
| v1.6.0 baseline retaken and committed, **or** a stated finding that it is not commensurable | ✅ **both** — retaken, and shown not commensurable |
| Standing questions answered in writing | ✅ §5 |
| No product code changed | ✅ |

No A3 work, no classifier tuning, no new scenario.

---

## 7. Decisions for the room

### A3 is next, and the measurement says so

Node churn did not move a single city. A workload appearing moved one 20px and
displaced it within its own province. So A4's motivation (ghost accumulation
under node churn) does not touch the axis that is actually unstable, while A3's
gate condition already has a measured violation from the frames in this repo.

**Ask:** confirm A3 next.

### A3-pre needs a positional instrument, not a pixel one

Two blockers, both now evidenced:

1. **No workload-churn scenario exists** — all six are node-level. A3's gate
   needs "add or scale a deployment on a settled fleet".
2. **Pixels cannot see city placement at realm scale** (§3 above), and — the
   sharper version — pixels cannot see *province* placement either when the
   fleet is uniformly healthy (§4). `reshuffle.py` works only because the pre-A2
   ordering was recomputable from names; A3's city placement is not.

The general form: **the map renders a projection, and a projection can be stable
in appearance while unstable in assignment.** Any further gate in this workstream
should measure the assignment.

**Ask:** approve a dev-only positional dump (a flag or a core example that prints
each city's cell per tick) as A3-pre's first deliverable.

### The published A2 gate figure should be read with this caveat

A2's 0.41% silhouette change is sound as *what it measures*, and the report says
so. But this session shows the same metric reports ~1% for a layout that moved
27% of its provinces. The figure is evidence that the rendered map holds still —
not that the assignment does. The assignment evidence is the core tests and now
`reshuffle.py`.

**Ask:** should the A2 report carry a pointer to this session's §4?

### An incidental finding worth keeping

The pre-A2 hash ordering **clustered nodes by name prefix**, so provinces were
already grouped by pool on any conventionally-named cluster — for free, and
invisibly. A2's zone-wide ordinals deliberately gave that up to make collisions
unrepresentable, and the decomposition's `region ← pool ∩ zone` row is still
unowned. Worth knowing that the property existed by accident before it is
rebuilt on purpose.
