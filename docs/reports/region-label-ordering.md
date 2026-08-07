# The region-label ordering defect

**Origin:** no guidance document — found while building a change the user asked
for directly ("let's have the region names follow the pan")
**Introduced:** v1.16.0 · **Fixed:** v1.17.0 · **Date:** 2026-08-07
**Status:** defect fixed and mutation-pinned. The audit it prompted found three
more instances of the same class; two are live, and none of them is fixed —
§6.4 lists what I would do and why I stopped short.

---

## 1. The defect, exactly

`Continent.provinces` is **not in map order**, and v1.16.0's region labelling
read it as though it were.

Two facts, each true in isolation and never put side by side:

| | |
|---|---|
| `zone.nodes` is sorted by `(fnv1a64(name), name)` | `model.rs:965` |
| a province's row is `slot_row(ordinal)` = `1 + ordinal * SLOT_STRIDE`, from the layout | `world.rs:584` |

The vector's sort key is a name hash. The row's is an allocation ordinal. They
are unrelated, so the index of a province in `cont.provinces` says nothing about
where its ground is.

v1.16.0's `pool_label_runs` walked that vector looking for maximal spans of
equal pool and called the longest one "the region's largest contiguous piece".
It was a run of **consecutive vector entries**: an arbitrary set of provinces
that happened to share a pool and land next to each other under a hash.

Three consequences, in ascending order of seriousness:

1. **The chosen piece was not the largest piece**, nor a piece at all.
2. **The span could be inverted.** The label was placed between `first.y` and
   `last.y + last.h`, and with the endpoints in hash order the first could be
   south of the last.
3. **A name could be drawn on another pool's ground** — the map asserting that
   ground belongs to a pool it does not. That is the same invariant the A2
   completeness audit stated after its third violation in one phase: *what is
   painted at a cell and what resolves there must be the same object.*

---

## 2. Why it shipped

Not haste. The premise was never examined, because it never surfaced as a
premise.

`Continent.provinces` is a `Vec` inside a struct whose other fields are all
geometry (`x`, `y`, `w`, `h`, `coast`, `ghosts`). Everything around it is
positional, so the vector reads as positional too. Writing `for (i, p) in
cont.provinces.iter().enumerate()` and thinking "down the column" takes no
deliberate step that could have been challenged — it is the absence of a step.

The tell I walked past: **A2's own code comments say this.** `province_y` is
introduced with *"Enumerating live provinces would reintroduce exactly the
reshuffle this removes"*, and `Coast::new` carries *"Those agreed while
`build_world` stacked provinces with `y += h`; now y comes from the slot ordinal
and the bands are sparse"*. Both are the same warning. I had read them — they
are the fix notes from a defect I wrote up myself — and did not connect them to
new code four months later in a different crate.

**The test could not catch it, because the test encoded the same assumption.**
`pool_label_runs_picks_the_largest_piece_of_each_region` passed a hand-written
`&["sys", "sys", "burst", …]` and asserted index ranges. The fixture *was* the
misunderstanding: a slice with no rows in it cannot express the difference
between vector order and map order. A test written from the same mental model as
the code is not a check on the code; it is a second copy of it.

---

## 3. How it was found, and what the guard did

By a crash, in unrelated new code:

```
panicked at core/src/num/f32.rs: min > max, or either was NaN. min = 334.0, max = 132.0
```

The pan-following work replaced a midpoint computation with
`view.clamp(first.y, last.y + last.h)`. `f32::clamp` panics when `min > max` —
so the first thing that ever *asserted* the ordering invariant found it violated
within one live frame.

The invariant had been violated all along. What changed is that the old code
absorbed it:

```rust
let run_px = (cam.to_land(0.0, bottom).y - cam.to_land(0.0, top).y).abs();
//                                                                  ^^^^^
```

That `.abs()` is the whole story. It was written to be safe. Its actual effect
was to take a number that could only be negative if the ordering assumption were
false, and make it look fine. **A defensive guard placed on the output of a
broken invariant does not protect the program; it removes the only evidence that
the invariant is broken.** The guard did not fail once in v1.16.0 — it silently
returned a plausible height for a meaningless span, every frame.

This is now written into the code at the point of use: the `.abs()` is gone, the
ordering is guaranteed by construction, and the comment says *"deliberately not
absolute-valued, because that is what hid the unsorted bug."*

---

## 4. The measurement was wrong too — and it chose the design

This is the part worth carrying forward, because it is worse than the bug.

v1.16.0 shipped a design decision — **name only the largest piece of a
region** — and justified it with a measurement: *"measured on the churn fleet,
1 of 8 regions is already in more than one piece."* At 1 in 8, one label per
region is obviously right; fragmentation is an edge case, and repeating a name
would be noise for no gain.

**That number was computed by the same wrong rule as the code.** I derived it by
walking the provinces vector, so it counted hash-order runs, not pieces of
ground.

Measured correctly, from `--dump-positions`:

```
z-a  37 provinces   burst 1 piece (100%)   sys 2 pieces (largest 60%)   t3.xlarge 1 piece (100%)
z-b  22 provinces   burst 1 piece (100%)   sys 3 pieces (largest 40%)
z-c  26 provinces   mem   1 piece (100%)   sys 2 pieces (largest 60%)
z-d  15 provinces   t3.xlarge 1 piece (100%)

3 of 8 regions are in more than one piece (8 regions in 12 pieces total);
a largest piece holds as little as 40%.
```

> **Corrected 2026-08-07.** This report first published *"4 of 8"*. The
> underlying per-zone data was right and is unchanged; the summary line was not.
> The instrument printed "8 regions in 12 pieces", and 12 − 8 = 4 counts *extra
> pieces*, not fragmented regions — three regions are split (into 2, 3 and 2).
> Re-derived by `hack/churn/pieces.py`, which now **emits** the fleet figure so
> it cannot be narrated from a breakdown again. The design conclusion is
> unaffected: 3 of 8 fragmented with a largest piece as low as 40% still says
> name every piece. See `docs/reports/t1-shape-rederivation.md` §5.

Three times as many as published, not one in eight — and on a fragmented region
the majority of its ground is *outside* the piece that gets the name (40% and
60% largest shares). The decision inverts on the real number: naming only the largest piece leaves most of a pool's territory
anonymous at any zoom close enough to fill the screen with a different piece,
which is precisely the situation the name exists for.

So the defect did not only misplace labels. It corrupted the evidence, and the
corrupted evidence selected the wrong design. Fixing the code without
re-measuring would have left a rule that is wrong for a reason the code no
longer contains.

The live confirmation was blunt: centred on `churn-sys-g2-002`, which is `sys`
at z-c slot 27, while that region's largest piece is slots 16–21. Standing on
`sys` ground, zoomed in, with `sys` written nowhere on the screen — the exact
complaint that prompted the pan-following request, arriving by a second route.

**The lesson is one step past the instrument lesson this project keeps
relearning.** The established form is *"an instrument can emit a plausible
number for a reason unrelated to what it claims to measure"* — nine-plus
occurrences, and the defence is the discrimination check. This is the sharper
case: **the instrument was not merely unrelated, it reimplemented the bug.** No
discrimination check would have caught it, because disabling the mechanism moves
both the code and the measurement together. The only defence is that a
measurement must not be derived by the same reasoning as the thing it measures —
here, from the dump, not from the model walk.

---

## 5. The fix: unrepresentable, not corrected

The obvious fix is to sort at the call site. I did that first; it was wrong, for
the reason the defect existed at all — it leaves the next caller free to make
exactly the same mistake, and there is nothing in the type or the name to stop
them.

So the signature moved instead:

```rust
pool_label_pieces(provinces: &[(&str, u16)]) -> Vec<(usize, usize)>
```

It takes `(pool, row)` **in the model's own order** and sorts internally. There
is no ordering for a caller to get wrong, because the caller no longer supplies
one. This is the same move as `draw::resolve_region` (one owner for the
coast→land→region probe order) and `world::slot_of_row` (one owner for row↔slot):
when a rule has been got wrong once, give it one home rather than a correction.

Two contract points settled while doing it:

- **Contiguous means consecutive slot ordinals**, not adjacent after sorting. A
  departed node's ghost ground sits *between* two same-pool provinces on screen,
  so it genuinely splits the region — and the resulting span then contains only
  that pool's ground, which is what makes "the name sits on its own pool" a
  guarantee rather than a hope.
- **Every piece is named**, per §4.

`region_label_row` — the pan-following rule — is also shared with the
graticule's `column_mark`, which had solved the same problem months earlier and
whose solution I had duplicated by hand before noticing. It is `total`: an
inverted span normalises rather than panicking, because a `clamp` in a draw path
is a crash, and the ordering is now pinned by tests rather than by a guard.

**Mutation floor, exercised.** Three reversions, each caught: drop the internal
sort (read stored order as map order — the shipped bug); drop the
consecutive-ordinal test (ghost ground stops splitting a region); flip the tie
rule. Plus two on `region_label_row`: return the midpoint (kills pan-following),
and return the view unclamped (name leaves its ground).

---

## 6. Audit — where else is container order read as world order?

52 references to `Continent.provinces` (19 in tests) and 7 to `ZoneColumn.nodes`.
Most are `find` / `flat_map` / `count`, which cannot care. Four sites carry an
order assumption. **Three of the four are the same defect; one is fine.**

### 6.1 `WorldModel::cities()` — live, documented contract false

Its doc said *"Cities in stable exploration order (west→east, north→south)."*
Both halves were true before A2 and are false now:

- **north→south** — provinces stacked with `y += h` down `zone.nodes`, so hash
  order *was* row order. A2 moved rows to slot ordinals; the sort key stayed.
- **west→east** — `cx = zi * (PATCH_W + OCEAN_GAP)` used the zone's index in an
  alphabetically sorted list (`v1.6.0 world.rs:435`), so alphabetical *was*
  west→east. A2 moved x to the durable first-observed ordinal; the sort key
  stayed.

Verified rather than argued. With `z-m` observed before `z-a`, the continents
vector comes out:

```
[("z-a", x = 30), ("z-m", x = 0)]
```

— the first entry is the *eastern* continent. Any zone added to a fleet after
the map exists takes the next ordinal and therefore sits east of every zone that
sorts after it. On the churn fleet the two orders coincide, which is why nothing
looked wrong: its zones were created alphabetically.

**Live consumer:** `]` / `[`, commented *"All cities across the scene, in
archipelago order."* It is still **deterministic**, so the sail visits every city
exactly once with no flicker or repeats. It simply is not a geographic sweep —
a degraded nicety, not a wrong number, which is why it is a finding and not an
incident. The doc comment now states what is true and points here.

### 6.2 `province_index_at` and `visible_provinces` — dead, contracts false

Both return a **vector index** documented as a **row**
(`"(zone col, node row)"`, `"first node row, node rows"`), and
`visible_provinces` goes further: it computes `first_row.min(i)` over the
enumeration index while testing the *y* extent, mixing the two coordinate
systems in one expression.

Both have **zero callers** — TUI-era helpers that outlived their frontend, and
`pub` in core so nothing warns. Harmless today; a trap for the next caller,
which is exactly how §1 happened. Docs corrected to say they are dead and why;
deletion left as a call (§6.4).

### 6.3 `Continent.ghosts` — same non-order, no consumer

`layout.ghosts()` iterates a `BTreeMap` keyed on `SlotKey { zone, pool, ordinal }`,
so ghosts come out ordered by **pool then ordinal** — also not map order, for a
third distinct reason. Nothing reads ghost adjacency (`draw_ghost_ground` is
per-item, `Coast::new` folds with `.max()`), so there is no defect. Recorded
because a future "ghost run" feature would walk straight into §1.

### 6.4 The terrain pass — a latent Relief-only occlusion risk

Not a current defect, and I want to be exact about that rather than inflate the
count.

`draw_world` paints `for prov in &cont.provinces` in vector order. Provinces
never overlap, so under `Plain` the order cannot matter. Under `Relief` it can:
`fill_prism` raises the top face by `land_lift` and fills the cliff down to the
sea-level footprint, so a province's painted region extends ~7px **north** of
its own ground. Correct back-to-front painting therefore requires ascending `y`,
and hash order does not provide it.

Today this is invisible, and the arithmetic says exactly why. Slots are
`1 + 9·n` and `SLOT_STRIDE == EXTENT_CLASSES.last() == 9`, so a province only
touches its neighbour at extent **9** — the largest class, which needs a node
above the top memory bound. The v1.7.1 completeness audit already found those
bounds are compared against *allocatable* and so never fire at the sizes they
name; no node on kind or on the churn fleet reaches class 9. Every province is
3–7 rows with empty slots between, the intrusion falls on water or ghost ground,
and ghosts are painted before terrain.

So: **reachable in principle, unreachable on any fleet we can currently build,
and unverifiable by test** (it is an occlusion, i.e. pixels). The fix is two
lines — sort the terrain pass by `y` — and is provably harmless. I did not make
it, because it is a rendering change with no way to demonstrate the before or
the after, and this was a report.

---

## 7. Standing questions

**1. Where does a summing step precede a comparing step?** Not present. The
neighbouring form did fire and is §1: *ordering* before comparing — the code
compared adjacent entries of a sequence whose adjacency meant nothing.

**2. Does every reducer over a possibly-empty input express unknown, or
fabricate?** `pool_label_pieces` returns an empty `Vec` for empty or all-unpooled
input, and the caller draws nothing. The unpooled sentinel is skipped rather
than named — an absence is not a region. `region_label_row` is total: `hh <= 0`
yields the band's first row, an inverted span normalises, no `NaN` escapes.

**3. Two sections constraining one behaviour?** Yes, and they conflicted. "Follow
the view" wants the name to track the pan; "never leave the ground it names"
wants it pinned. Clamping both axes satisfied the first and broke the second —
names went **offshore**, because a region spans every column it has and the
rectangle's edges are where the coast carving puts sea. Resolved by splitting the
axes: the row follows, the column is the continent's midline. That is
`column_mark`'s existing answer, which is why the rule is now shared.

**4. Old meanings?** This is the failure. A2 changed what `y` is derived from and
left every vector's sort key alone, so three "orders" quietly stopped meaning
what their names and docs said. §6 is the sweep that should have run then.

**5. Inherited claims?** The worst instance yet. The inherited claim was **my own
published measurement from the previous day** ("1 of 8"), and it was inherited
inside a single work session — short enough that re-deriving it never occurred to
me. Sharpened form: *a claim is not safer for being recent, or for being mine.*

**6. When a change moves one side of a comparison, does the other still mean the
same thing?** Directly: A2 moved rows to ordinals; the vectors' sort keys stayed
put; nothing compared the two afterwards.

---

## 8. A seventh standing question

Six questions did not catch this, and I do not think any of them could have —
they are about values, reductions and comparisons, and this was about *sequence*.
Proposed:

> **7. Where does the code treat neighbouring entries in a container as
> neighbouring things in the world — and what guarantees that?**

It has teeth here in a way the others do not: applied to `draw_world`, it names
§6.1–§6.4 in one pass without needing the crash. Its answer must be a mechanism,
not an observation — "they happen to line up on our fleet" is how §6.1 survived
A2 undetected for eleven minor versions.

---

## 9. What changed in the tree

| | |
|---|---|
| `pool_label_pieces` — sorts internally; contiguity = consecutive slots; every piece | `draw.rs` |
| `region_label_row` — pan-following, shared with `column_mark`, total | `draw.rs` |
| `.abs()` removed from the span, with the reason at the point of use | `draw.rs` |
| 5 mutations pinned across 2 tests | `draw.rs` |
| Three false doc contracts corrected (§6.1, §6.2) | `state/world.rs` |
| Almanac + CHANGELOG + decision log corrected, incl. the 1-of-8 figure | — |

450 core + 110 GUI tests; gui-smoke 55.

**Not done, deliberately:** delete the two dead helpers in §6.2; sort the terrain
pass by `y` (§6.4); make the `]` / `[` sail geographic (§6.1) — a design change,
not a fix, and it would want a decision about whether "exploration order" should
be by column then row, or by proximity to the camera.
