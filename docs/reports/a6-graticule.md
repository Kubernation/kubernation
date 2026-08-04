# A6 — the graticule and the declared frame

**Phase:** A6, from `kubernation-a6-graticule-guidance.md`
**Version:** v1.11.0 · **Date:** 2026-08-04
**Status:** built and verified except §4's gate, which needs a second person — see §6.

This closes Workstream A.

---

## 1. §0 — claims verified

All eight, against source, this round. Nothing was accepted on a prior report's word.

| # | Claim | Result |
|---|---|---|
| 1 | `WorldModel { width, height, … }`, `u16` cells | TRUE (`world.rs:212`) |
| 2 | `zone_ordinal(zone) -> Option<u16>`, durable | TRUE (`layout.rs:174`) |
| 3 | `zone_ordinals()` retains departed zones | TRUE — only ever inserted into (`layout.rs:397–407`), never removed, and round-tripped by `layout_store` |
| 4 | `SlotKey{zone,pool,ordinal}`, ordinals zone-wide | TRUE — `next_ordinal` filters by zone only |
| 5 | Province y = ordinal × stride | TRUE — `1 + ordinal * EXTENT_CLASSES.last()` |
| 6 | ~two-thirds ocean; stride is the largest class | **MEASURED, not inherited** — see below |
| 7 | `to_screen`/`to_land`/`cell_at` are the projection | TRUE |
| 8 | Overlay + map style persist in prefs | TRUE |

### Claim 6, measured

The guidance calls this "the design problem" and says to verify the ratio before
choosing a grid, so I measured rather than quoted A2:

```
world bounding box 116 x 366 = 42,456 cells
  land           12,272   28.9%
  ghost ground    1,560    3.7%
  OCEAN          28,624   67.4%
```

"Roughly two-thirds" is right. Sharper than the claim states: the stride is 9
while the extent classes actually present are 3 (30 nodes), 5 (54) and 7 (16) —
**no province on this fleet reaches the stride**, so every slot has at least two
empty rows under it. That is decisive for §2.1: a uniform lattice would spend
two-thirds of its labels on water.

### The invariant the scheme rests on

`SlotKey` carries a pool, so `(zone, ordinal)` being unique is not free — if it
were not, `sys/0` and `burst/0` would both be "A0". Verified live: **100
provinces, 4 pools, zero collisions**, with pools interleaving within a zone
rather than each numbering from zero. Pinned by
`reference_is_unique_across_a_multi_pool_zone`.

---

## 2. What shipped

| Piece | Where |
|---|---|
| `GridRef`, `column_letter`, `columns`, `reference_for`, `resolve` | `state/graticule.rs` (pure, 6 tests) |
| `FRAME_DECLARATION` — what the frame is anchored to | `state/graticule.rs` |
| `slot_row` / `slot_of_row` — one authority for row↔slot | `state/world.rs` |
| `Province.reference`, `Continent.column`, `WorldModel.reserved` | `state/world.rs` |
| Rules, row numbers, column letters, reserved columns | `draw::draw_graticule` |
| The on-map declaration | `panels::draw_frame_note` |
| The SELECTION line | `panels::grid_ref_line` |
| Toggle | View ▸ Reference frame · `--graticule` · pref |

432 core + 101 GUI tests; gui-smoke 53; clippy clean.

---

## 3. The one real defect — and where it was

**Row numbers were off by one.** The map showed `3` beside the node the
positional dump called **C4**. A reference read off that map sends someone to the
wrong node, which is the single failure a naming scheme cannot have.

The cause was a placement choice that looked right in isolation: each label was
drawn at its band's south-west corner, because in iso that is the band's
west-most point. But that corner sits on the boundary *between* two bands, so it
reads as labelling the one below. Fixed by placing labels mid-band on the west
edge.

Two things about this are worth carrying forward.

**No test could have caught it.** It is a screen position — precisely what the
GUI testability policy concedes is unassertable, and why `make gui-smoke` is a
crash gate rather than a correctness one. It was caught by rendering the live map
and reading it against `--dump-positions`. The dump now emits each province's
reference specifically so that comparison can be made routinely; that pairing is
the instrument this class of defect needs.

**It came from a re-derivation.** `province_y` turns an ordinal into a row, and
`draw_graticule` was turning the row back into an ordinal with its own inline
expression. The fix routes both through `world::slot_of_row`, the tested inverse
of `slot_row`, so a label cannot disagree with the reference on the same
province. Same shape as every other drift this workstream has paid for.

---

## 4. Decisions the guidance did not settle

### 4.1 A departed zone has nothing to draw a letter on

§2.3 requires that a departed zone keep its letter, "the same discipline as a
ghost keeping its ordinal". But the two are not analogous. A ghost keeps its
ordinal **and is drawn** (A2 made ghost ground visible). A fully departed zone
leaves **no `Continent` at all** — verified on the churn fleet: it does not even
leave ghost ground, because ghosts hang off a continent and there is none.

So "keeps its letter" is unobservable unless the map labels reserved columns.
Hence `WorldModel.reserved`, and the letter drawn over the empty sea with
"departed, ground reserved" — the same argument A2 used for painting ghost ground
rather than letting a vacated slot read as ocean.

Verified live (scenario 6, `ZONE=z-b`): z-c stayed at x=60 and z-d at x=90, and
the lettering reads **A → C → D** with B held over open water.

### 4.2 §2.2 and §4.2 conflict

"It must recede" against "unreadable at the zoom where a fleet is viewed is a
failure". These are not reconcilable at one alpha. Resolved by splitting the ink:
hairlines stay ambient (0.20) because a tessellation is texture; labels meant to
be *read* get 0.55, because a numeral at a hairline's alpha is not subtle, it is
absent.

The frame is **scenery**: drawn between the terrain pass and the feature pass, so
it lies on the ground but can never compete with a settlement, and deliberately
outside the `cb_*` colour-blind funnel since it encodes no cluster state — the
explicit contrast §2.2 draws with A5's fresh ground.

### 4.3 Column letters in an isometric projection

The first attempt centred each letter in its column's screen-space bounding box.
In iso a column is a diagonal band whose AABB is enormous and overlaps its
neighbours', so all four letters stacked within a few pixels of screen centre,
labelling nothing. Placement now rides each column's own centre line, which
spreads the letters exactly as the bands are spread and follows the pan — the way
an atlas repeats edge labels down a long sheet.

---

## 5. §7 — standing questions

**1. Where does a summing step precede a comparing step?**
Nowhere new: the graticule aggregates nothing. The related failure did occur in a
neighbouring form — *deriving, then re-deriving* — and produced §3's defect. Now
one authority (`slot_of_row`).

**2. Does every reducer over a possibly-empty input express unknown, or fabricate?**
Yes, throughout: `reference_for` → `Option`; `resolve` → `Option`;
`Continent.column` → `Option`; the letter is drawn only when the column has rows;
`column_mark` draws nothing when off-screen rather than clamping to an edge where
it would appear to label something else; and `grid_ref_line(None, on)` *says* "no
durable position" rather than omitting the line, because a blank where a
reference belongs reads as "not loaded yet".

Audited and found: `world.rs:622` does fabricate — `zone_ordinal(…).unwrap_or(zi)`
for a continent's *x*. It is unreachable today (`build_with` always runs
`assign_layout` over the same node set first, so every drawn zone has an ordinal),
and I deliberately did not copy it for the letter: a fabricated letter collides
with a real zone's, which is strictly worse than an unlabelled column.

**3. Where do two sections constrain the same behaviour, and is there a fixture
where they diverge?**
§2.3 and §6 both constrain the departed zone; they diverge on what is
*observable* — see §4.1. §2.2 and §4.2 constrain the same ink and genuinely
conflict — see §4.2.

**4. What existing consumers depend on the old meaning of a value this redefines?**
Nothing is redefined. `province_y` now calls the named `slot_row` instead of an
inline expression — identical value, newly pinned. `Province`, `Continent` and
`WorldModel` gain fields; `region_lines` / `draw_tooltip` / `draw_sidebar` gain a
parameter at four call sites. No consumer's reading of an existing value changes.

**5. Which claims were inherited rather than verified, and does each state occur?**
All eight re-verified this round, including the four tagged `[A]`. The two states
the design depends on were produced, not assumed: a **departed zone** occurs via
scenario 6 (verified), and multi-pool zones occur on the standard fleet (verified,
zero reference collisions).

---

## 6. The gate — what is done and what is not

§4: *one person names a position from the map; another finds it without further
explanation.* This is the first gate in the workstream that cannot be automated,
and **I cannot run it alone.** Reporting it as passed would be reporting a
usability result I have no evidence for.

**Done — the mechanical halves:**

- slot → reference → text → reference → slot round-trips (unit).
- Every province on a 78-node fleet has a unique, non-empty reference; the
  lettering skips the departed zone (live).
- Map labels agree with the dump's references, after §3's fix (live).
- **Discrimination check:** with `--graticule` off the frame accounts for **0**
  drawn elements and the app offers **no naming aid at all** — no letters, no row
  numbers, no SELECTION reference (gated and tested). Turning it on changes
  **46,265 pixels, 1.25% of the play area** — present, and a light touch.

**Not done — the human half.** To run it:

```
cargo run -p kubernation -- --context <ctx> --graticule --zoom 1.0
```

Screenshot it, pick a province, read its reference off the map, and hand the
screenshot plus the reference to someone else with the app open. They should land
on the same node. Then repeat with `--graticule` omitted: if they still find it,
the gate is measuring their familiarity with the fleet, not the frame.

Failure criteria, stated in advance (§4.2): the reference is ambiguous; it is
unreadable at fleet zoom; the grid competes with terrain; or a reference names
ocean or nothing.

---

## 7. Found in passing — a pre-existing test flake

Four theme tests mutate one process-global palette atomic and run in parallel, so
a test sampling a colour ramp could see another test flip the palette halfway
through. It passed by scheduling luck until this round's new tests perturbed the
ordering, then reported "4 distinct colours, expected 3".

Not caused by this change, and now serialised behind a lock. Worth noting because
of its shape: a random CI failure that passes on retry is worse than a wrong
colour, since the natural response is to re-run rather than to look.

---

## 8. Open for planning

1. **The gate's human half** (§6) is the only outstanding acceptance item.
2. **`resolve` has no caller.** A "go to reference" jump — type `C4`, fly there —
   is built and tested in core but unwired. It is the obvious complement to
   naming, and the keyboard-ownership question is the same one the free-text
   Oracle question ran into.
3. **`region ← pool ∩ zone` grouping** remains unclaimed, as it has since A2 gave
   up contiguity when ordinals went zone-wide. The graticule makes the cost
   visible: pools interleave within a column, so a reference tells you the zone
   but not the pool.
4. **The SELECTION reference is not capturable headlessly** — no dev flag drives
   a hover — so it is covered by a test through the real `region_lines` rather
   than by a screenshot. A `--select <node>` flag would close it.
5. **Workstream A is complete.** A6 was its prerequisite for the plan's §7
   time-series work: small multiples, change-since overlays and fault-line
   marking all need frames that can be laid against each other, and there is now
   a declared one.
