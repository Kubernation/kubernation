# KuberNation — D2: Brushing (revision 2)

**Implementation guidance**
**Goal:** one selection, propagated across every view, so the map and the lists describe the same thing at the same time.
**Gate:** select in a list, and the map marks it. Select on the map, and the list marks it.

> **Supersedes revision 1.** Its §2 (the free version) is **refuted by
> measurement**, and its §1 claim 1 — the premise §3 was built on — is **false**.
> Both are recorded below rather than removed, so neither is re-proposed. Full
> working in `docs/reports/d2-brushing-precheck.md`.

---

## 0. What the pre-check changed

Two things, and together they make this a bigger phase than rev 1 thought while
also making its shape much clearer.

**The shortcut is gone.** Rev 1 §2 predicted that promoting `namespace_pair` to a
cross-view identity colour would deliver "most of brushing's correlation" for
free. It would not — §2 below has the numbers. Nothing here is cheap any more.

**The model is different from the one rev 1 described.** It claimed `Panel` *is*
the selection. There is a second, positional selection (`selected:
Option<(u16, u16)>`), and it is the one every map mark reads. So the hover/commit
split rev 1 proposed building **partly exists** — and the actual problem is one
rev 1 never states: **the selection is a position, and brushing needs an
identity.**

---

## 1. Verify before building

`[V]` verified against source during the pre-check (2026-08-07). `[A]` inherited.

| # | Claim | Tag |
|---|---|---|
| 1 | `Panel::{City(ClusterId, WorkloadRef), Node(ClusterId, String)}` — the *shape* rev 1 gave is right | `[V]` `panels.rs:638` |
| 2 | **A separate positional selection exists**: `selected: Option<(u16, u16)>` | `[V]` `main.rs:803` |
| 3 | It is a SCENE cell — warm-world cells carry `+ sw.off` | `[V]` `main.rs:2104`, `draw.rs:484` |
| 4 | `off = hot.models.world.width + WORLD_GAP` — it changes when the hot world grows | `[V]` `draw.rs:484` |
| 5 | `city_pos(&WorkloadRef)` and `province_pos(&str)` map identity → position | `[V]` `world.rs:355,359` |
| 6 | `namespace_pair` has exactly one draw site, `Overlay::Namespace` | `[V]` `draw.rs:394` |
| 7 | That arm tints by `dominant_namespace` — **plurality**, not membership | `[V]` `draw.rs:394` |
| 8 | `resolve_region` is the probe authority; `draw_hover` and `draw_blast` are the existing marks | `[V]` `draw.rs:549,588,1495` |
| 9 | `fly_to_within` / `aim_for_drilldown` are D1's "put a subject in view" primitive | `[V]` `draw.rs:815`, `main.rs:671` |
| 10 | A city sites at its pods' **plurality node**, so it moves when that shifts | `[A]` A3 |

**Claims 3, 4 and 10 are the phase.** Together they say a stored cell has two
independent ways to go stale, and §3 is what to do about it.

---

## 2. The free version, refuted — do not re-propose

Rev 1 §2's prediction was *"Córdoba is red in the list and red on the map"*. That
assumes a **1:1 region↔category mapping**, which this map does not have.

| | |
|---|---|
| **It is on one overlay of nine** (claim 6) | Under the default Terrain view, and under Pressure, Cost, Pool, Walls, Substrate, Saturation and Replicas, the map carries no namespace colour at all |
| **And there it is plurality-only** (claim 7) | A province is a *node*, and nodes host many namespaces |

Measured:

```
kind    4/4 nodes host >1 namespace;  30% of pods sit on a node
        whose plurality namespace is not their own
churn   0/99 mixed  <-- DEGENERATE: that fixture is single-namespace
```

**A swatch wrong ~30% of the time asserts a correspondence that does not hold**,
which this codebase refuses everywhere else (`pool_line`'s sentinel,
`extent_line`'s silence, `pool_confinement`'s single-pool refusal, every
`DEGENERATE` in the instruments).

What the colour would still buy is **within-list grouping** — always on, real, and
a legibility change to argue on its own merits. **It is not D2 and must not be
counted as part of this gate.**

---

## 3. The model: store identity, derive position

### 3.1 The evidence

Seven readers of `selected` were enumerated. **Five immediately convert the cell
back into an identity**, and two of them do it *verbatim identically*:

```rust
// oracle_scopes, main.rs:470          // blast subject, main.rs:2455
match world.region_at(x, y) {          match sw.world.region_at(local.0, local.1) {
  Region::City(_, c)    => c.r         Region::City(_, c)  => Subject::Workload(c.r)
  Region::Province(p)   => p.tile.name Region::Province(p) => Subject::Node(p.tile.name)
```

`panel_for` performs a third variant of the same conversion, and `sidebar_sel`
does `locate` then `region_lines`. **Only `draw_selection` genuinely wants a
cell.**

So the state is stored as a position and consumed as an identity by most of its
readers, through a conversion that has no home.

### 3.2 Two independent staleness sources

A stored cell is wrong, silently, when:

1. **the subject moves** — a city sites at its pods' plurality node (claim 10), so
   any reschedule that shifts the plurality moves it;
2. **the scene shifts** — a warm cell is `local + off`, and `off` is the hot
   world's width (claims 3, 4), so adding a zone to the hot cluster moves every
   stored warm cell.

The second is the more common and the less obvious. Neither produces an error:
the selection quietly starts pointing at a different province.

### 3.3 The shape

**Invert it.** The selection becomes an identity — the same identity `Panel`
already carries, including its `ClusterId`, which a bare cell does not carry at
all — and the position is derived per frame via `city_pos` / `province_pos`
(claim 5).

Both staleness sources dissolve: a moved city resolves to its new position, and a
shifted scene resolves through the current `off`.

- **Do the conversion collapse FIRST**, before changing the representation. One
  `fn subject_at(worlds, cell) -> Option<(ClusterId, Subject)>`, consumed by the
  Oracle scope, the blast subject and `panel_for`. That is D1 §2's move, and it
  is what makes the inversion a small change instead of a five-way edit.
- **`draw_selection` takes the derived position**, and must handle `None` — a
  selected workload that has left the cluster has no position. See §8 q2.

### 3.4 Hover and commit

Rev 1 §3.2 is right that these differ, and half of it exists: `hovered` is already
a separate value (`main.rs:2559`, `sidebar_sel = selected…or(hovered)`). Keep two
levels, no third. **Hover must not propagate as commit** — §7.2.

---

## 4. Question 4's list, enumerated

Rev 1 could not supply this, because it did not know the state existed. D1's
lesson was that the consumer which bites is the one not named, so:

**Readers (7)** — `oracle_scopes` (`o` key and the Oracle menu, two call sites) ·
`Enter` → `panel_for` · `draw_selection` · the blast subject · `sidebar_sel` (the
SELECTION box) · the concern-nav helper (`&mut`) · `AlmanacAction::Locate`.

**Writers (8)** — map click · `]`/`[` sail · `--inspect` (city and node arms) ·
concern nav · IMPACT-row focus · almanac locate · cleared on context switch and
on `Esc`.

**Every one of these changes meaning if `selected` stops being a cell.** The
IMPACT-row focus is the one to watch: D1's review found it must *not* re-root the
blast subject, and that constraint is expressed today in terms of not touching
`selected`.

---

## 5. What D2 does not do

- **No camera movement on selection.** Marking is not navigation; that is D4.
  D1's `aim_for_drilldown` fires once on *open* and must not be extended here.
- **No where-am-I marker during scroll** — D3.
- **No new views.**
- **No selection for rows without a map position**, and the refusal must be
  visible rather than a row that highlights nothing.
- **No namespace swatches** (§2). If they are wanted, they are a separate,
  separately-argued change.

---

## 6. Tests

- [ ] `subject_at` is the one conversion — the Oracle scope, the blast subject and `panel_for` all agree for the same cell, asserted by equality
- [ ] A selected workload whose city has **moved** resolves to the new position, not the old one — the staleness fix, and the reason the model changes at all
- [ ] A warm-world selection survives the hot world growing a zone (claim 4)
- [ ] Hover does not persist; commit does
- [ ] Closing the panel does not clear the selection
- [ ] A subject that has left the cluster resolves to `None` and is *said*, not drawn at a stale position
- [ ] The map's mark and the list's mark agree about which entity is current

**Mutation floor, and assert each applied** — four mutations this session were
first reported surviving because `cargo fmt` had reflowed the target and the
replacement matched nothing. Make the map ignore the selection; make a list
ignore it; and re-introduce a second copy of the cell→identity conversion.

---

## 7. The gate

**Select in a list, and the map marks it. Select on the map, and the list marks
it.** Run it on a **busy** map — Relief, an overlay active, fresh ground present,
graticule on — not a quiet one.

### 7.1 The discrimination check

Run the gate with brushing disabled and confirm the marks disappear. §2 having
been refuted, there is no "free version passes part of it" caveat left: if a
subject is still locatable with brushing off, the gate is measuring the open
panel or familiarity.

**Check the metric can discriminate before running it.** D1's occlusion figure
conflated covering the map with moving it, and the honest number turned out to be
geometric rather than a pixel diff.

### 7.2 Failure criteria, stated in advance

- The mark is invisible against a busy map
- The mark is indistinguishable from hover, blast or fresh ground
- Moving the pointer down a list strobes the map
- A row highlights nothing because its subject has no position
- **A selection points at the wrong province after a reschedule or a zone
  addition** — the two staleness sources §3.2 names, and the reason for the phase

---

## 8. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 4 is answered in advance for once** — §4. Verify the list is still
complete rather than trusting it; it was read on 2026-08-07.

**Question 2 is a design decision, not a check.** A selection whose subject has
left the cluster is **stale, not absent**. Decide: clear it, keep it as a
tombstone that says so, or refuse it — and make the SELECTION box say which. A
silent disappearance and a silent stale mark are both wrong.

**Question 6 is the inversion.** `selected` currently means "a scene cell". After
this it means "an entity". Every consumer in §4 must still mean the same thing
about the same subject, and the two that want a position must get it from the
same derivation.

---

## 9. Acceptance

- [ ] §2 recorded as refuted; no namespace swatches shipped as part of this
- [ ] The cell→identity conversion collapsed to one home **before** the representation changed
- [ ] Selection is an identity; position derived per frame
- [ ] Both staleness sources tested (§6)
- [ ] Hover and commit distinguished; no third level
- [ ] Rows without a map position cannot be selected, visibly
- [ ] No camera movement on selection
- [ ] Gate run on a busy map, with a discrimination check whose metric was shown to discriminate first
- [ ] Failure criteria stated before the run
- [ ] Mutations asserted to have applied
- [ ] §4's consumer list re-verified, and any consumer found beyond it recorded
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 10. Estimate

**One to one and a half days.** The conversion collapse (§3.3) is half a day and
is the part that makes the rest safe; the inversion touches fifteen sites (§4);
the marks themselves are small. Rev 1's "half a day for §2" is withdrawn.

**Do not start with the marks.** They are the visible part and the smallest; the
model is where this phase can go wrong quietly.
