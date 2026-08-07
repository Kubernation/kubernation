# KuberNation — Consolidation

**Implementation guidance**
**Goal:** retire four deferred items that have accumulated across Workstream A and T, two of which are causally linked in a way nothing currently records.
**Shape:** small, independent fixes. No new features.

This unblocks nothing and blocks nothing. It exists because the deferrals have reached the point where one of them can surprise the next person.

---

## 0. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | `EXTENT_BOUNDS_GIB = [32.0, 128.0, 512.0]`, and the class is `EXTENT_BOUNDS_GIB.iter().filter(\|b\| gib >= **b).count()` | `[V]` `world.rs:537,558` |
| 2 | `EXTENT_CLASSES = [3, 5, 7, 9]`, and `SLOT_STRIDE` is the largest class | `[V]` `world.rs:534`, A2 |
| 3 | The comparison is against **allocatable**, which is always below nominal | `[A]` v1.7.1 audit |
| 4 | `province_index_at` and `visible_provinces` have **zero callers** and are `pub` in core | `[A]` v1.17.0 §6.2 |
| 5 | Both document a **vector index** as a **row** (`"(zone col, node row)"`) | `[A]` v1.17.0 §6.2 |
| 6 | `draw_world` paints `for prov in &cont.provinces` — vector order, which is hash order | `[A]` v1.17.0 §6.4 |
| 7 | `fill_prism` raises the top face by `land_lift` and fills the cliff to the sea-level footprint, so a province's painted region extends ~7px **north** of its ground | `[A]` v1.17.0 §6.4, relief work |
| 8 | `ExtentSource` has no consumer outside `world.rs` | `[A]` A2 §6 |

**Claim 4 needs a real check.** VOR's reference lookup for these symbols degraded to a text match this round and returned the whole codebase, so "zero callers" is currently unverified by me. Confirm before deleting.

---

## 1. The link nothing records

**Items A and B below are causally connected, and that connection appears in no open-decisions row.**

The Relief occlusion risk (§3) is unreachable today *only* because extent class 9 never fires. Class 9 never fires *only* because the bounds are compared against allocatable and so never fire at the sizes they name (§2).

> **Fixing the extent calibration activates the occlusion risk.**

Someone fixing the bounds alone — a one-line change with a visible, desirable effect — would get a Relief-only rendering artifact with no obvious cause, on the largest nodes only, at the moment they made the map *more* correct.

**So do them together, in this order: the terrain sort first, then the calibration.** The sort is provably harmless and unobservable today; landing it first means the calibration cannot surprise anyone.

---

## 2. Item A — extent bounds calibration

`gib >= 32.0` against allocatable means a nominal 32 GiB node reports ~30.9 and takes the class *below* the one it should. The same holds at every boundary, which is why the smallest extent is the ordinary case on real clusters — and why the map is mostly thin ribbons.

Two fixes, and the choice matters:

- **Move the bounds just under the nominal sizes** — e.g. `[30.0, 120.0, 480.0]`. Cheap, and it encodes a fudge factor with no principle behind it.
- **Compare against capacity rather than allocatable.** Principled — capacity *is* the nominal size — but check whether capacity is observed at all, since `node_allocatable` is the function that exists and v1.6.0 made its absence a first-class state.

Prefer the second **if capacity is available**. If it is not, take the first and say plainly in the comment that the bounds are offset because the comparison is against allocatable, so the next reader does not "correct" them back to round numbers.

Either way this changes what provinces look like on every real cluster. **Capture before and after** on the churn fleet — this is the one item here with a visible effect.

---

## 3. Item B — sort the terrain pass by `y`

Under `Plain`, province paint order cannot matter — provinces never overlap. Under `Relief` it can: `fill_prism` extends a province's painted region ~7px north of its own ground (claim 7), so correct back-to-front painting needs ascending `y`, and hash order does not provide it.

Two lines. Provably harmless. Unobservable today, for the reason in §1.

**Do not skip it because it is unobservable.** That is precisely the argument that left the `.abs()` in place for eleven minor versions — a guard that made a broken invariant look fine rather than protecting anything.

---

## 4. Item C — delete the dead helpers

`province_index_at` and `visible_provinces` return a vector index documented as a row, and `visible_provinces` mixes the two coordinate systems in one expression (`first_row.min(i)` over the enumeration index while testing the *y* extent).

**Delete rather than document.** v1.17.0 §1 is the proof of why: a false contract on an unused `pub` helper is a loaded trap, and a corrected doc comment is a weaker guard than nonexistence. They are TUI-era helpers that outlived their frontend.

Confirm claim 4 first. If a caller exists, that is a live defect and a different phase.

---

## 5. Item D — give `ExtentSource` a consumer, or say why not

A2's acceptance required the extent fallback to be **declared and marked**. It is declared only: an unmeasurable node draws at the default extent with nothing distinguishing it, and `InstanceType` and `Default` are visually identical to each other and to a measured mid-size node.

v1.6.0 established the treatment for exactly this: **hatching**, the no-data texture, chosen because it says *no data* rather than *a data value* and composes with any overlay.

The question is whether extent-by-fallback deserves it. Two readings:

- **It does** — a province whose size is a guess should not read as a measurement, same argument as the ratios
- **It does not** — extent is scenery, not instrumentation; the *size* carries no cluster state, so a fallback size misleads less than a fallback ratio

I lean the first, weakly. **Decide and record it either way** — an acceptance criterion that has been open since A2 without a decision is worse than either answer.

Note the interaction with §2: fixing the calibration changes how often `Capacity` produces each class, but not how often the *fallback* rungs fire. These are independent.

---

## 6. Tests

- [ ] Extent: a node at exactly nominal 32 GiB (whatever allocatable that implies) gets the class it should — **the calibration's discrimination test**
- [ ] Extent: class boundaries are exercised at each rung, not just the ends
- [ ] Terrain sort: provinces are painted in ascending `y` — assert the order, since the occlusion itself is pixels and unassertable
- [ ] Deletion: the tree builds and tests pass with the helpers gone (the check that claim 4 was right)
- [ ] `ExtentSource`: whichever way §5 goes, the decision has a test or a recorded rationale

**Mutation floor, exercised:** revert the terrain sort and confirm the order test fails. There is no mutation for the occlusion itself — that is the point of asserting the order instead.

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. **Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?**

**Question 7 is adopted as standing, and this is its first outing.** Item B is a direct hit: `draw_world` paints in vector order and relies on world adjacency. Its answer must be a **mechanism**, not an observation — "they happen to line up on our fleet" is how the region-label defect survived eleven minor versions.

**Question 4 is live for item A.** Changing extent bounds changes `province_extent`'s output, which feeds `SLOT_STRIDE`-relative layout. Verify nothing downstream assumed the old distribution — particularly anything that has only ever seen class 3.

---

## 8. Acceptance

- [ ] Terrain sort landed **before** the calibration change
- [ ] Calibration fixed, with the reason for the chosen approach in the comment
- [ ] Before/after captures on the churn fleet for the calibration
- [ ] Dead helpers deleted, after confirming zero callers
- [ ] §5 decided and recorded
- [ ] Question 7 answered with a mechanism
- [ ] Open-decisions rows retired, and the §1 link recorded wherever the two items are tracked
- [ ] `cargo nextest` green

---

## 9. Estimate

**Half a day.** Four small changes; §2's before/after capture and §5's decision are most of the time.
