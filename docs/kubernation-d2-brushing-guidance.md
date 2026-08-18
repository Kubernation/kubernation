# KuberNation — D2: Brushing

**Implementation guidance**
**Goal:** one selection, propagated across every view, so the map and the lists describe the same thing at the same time.
**Gate:** select in a list, and the map marks it. Select on the map, and the list marks it.

Workstream D, phase 2. D1 landed the precondition — the map now survives a drill-down.

---

## 0. Why this is next

Wickens' finding, cited in the enabling plan §1.1: a global overview should be **preserved through** the local phases, and **visual momentum** is preserved by highlighting the current location in the small-scale worldview while exploration happens elsewhere.

D1 made the map visible during a drill-down. **D2 is what makes that visibility useful** — otherwise the map is present but says nothing about what you are reading.

The mechanism is *data brushing* (from *Designing Interfaces*): selection in one view highlights the same entity in the others.

**The plan also predicted a free version of this**, and it should be checked before any selection plumbing is built — see §2.

---

## 1. Verify before building

Everything below is `[A]`. VOR was unavailable when this was written, and the codebase has moved substantially since these were last read. **Verify each, and note that §2 may make most of this moot.**

| # | Claim | Source |
|---|---|---|
| 1 | `Panel::{City(ClusterId, WorkloadRef), Node(ClusterId, String)}` is the selection — there is no separate selected-cell state | `panel_for`, earlier in this thread |
| 2 | `panel_for` and `region_lines` both live in `panels.rs` and both route through `draw::resolve_region` | v1.x hit-test work |
| 3 | `resolve_region` is the single probe authority, so the map and the tooltip cannot disagree about what is under the pointer | same |
| 4 | `draw_hover` marks the hovered region; `draw_blast` marks a subject's dependency fan-out with a pulsing ring | hover work, `--blast` |
| 5 | `namespace_pair` computes a stable per-namespace colour, currently used only by `Overlay::Namespace` | substrate/plan §6.2 |
| 6 | `city_pos` / `province_pos` map an identity back to a map position; `draw_blast` already uses them | A-series |
| 7 | The Annals, Charter, Workloads table, Oracle and Almanac are separate windows, each with their own row model | plan §1 |
| 8 | `fly_to_within` aims at a screen rect; `aim_for_drilldown` is the one home both open paths use | D1 §4 |

**Claim 8 matters:** D1 built the "put a subject in view" primitive. D2 should consume it rather than build a second one.

---

## 2. Check the free version first

Plan §6.2 predicted that **persistent category colour** delivers most of brushing's correlation with no interaction at all:

> Córdoba is red in the list *and* red on the map. Correlation with nothing clicked.

`namespace_pair` already computes a stable per-namespace colour (claim 5). Promoting it from a single overlay to a **cross-view identity colour** — used in the Annals rows, the workload table, the province window's lists — would be a much smaller change than selection propagation, and it works without a selection existing.

**Do this first and look at it.** If a row's namespace colour matching its province's tint is enough to locate things, D2's remaining scope shrinks to the cases colour cannot cover: *which* workload, not which namespace.

This is the same move that shrank A3 to two lines and killed T2 before it was built. **The cheap thing first, then measure what is left.**

---

## 3. The selection model

If §2 leaves real work, this is its shape.

### 3.1 One selection, one authority

Claim 1 says `Panel` *is* the selection. That is adequate for a modal drill-down and inadequate for brushing, because brushing needs a selection that:

- outlives the panel being closed
- can be set from a list row, not only from a map click
- can be *hovered* as well as *committed* — a distinction the current model does not carry

**One type, one home**, per the rule this codebase has paid for seven times (`resolve_region`, `derive_qos`, `worst_level`, `changed_hands`, `fresh_tier`, `slot_of_row`, `window_rect_at`). Every view reads it; no view keeps its own copy.

### 3.2 Hover and commit are different

A list of rows the mouse passes over would strobe the map if hover propagated as commit. Two levels:

| | Set by | Cleared by | Marks |
|---|---|---|---|
| **hovered** | pointer over a row or region | pointer leaving | lightly, transiently |
| **selected** | a click | another click, or explicit dismissal | persistently |

The map already distinguishes these (claim 4: `draw_hover` versus `draw_blast`), so the vocabulary exists. **Do not invent a third level.**

### 3.3 What can be selected

Start with the two things `Panel` already carries: a **workload** and a **node**. Both have a map position (claim 6).

**Do not generalise to every row type.** A Charter RBAC row, an Almanac entry and an Annals line are not all map-locatable, and a selection model that pretends they are will produce rows that highlight nothing. Rows whose subject has no map position should not offer selection — and that refusal should be visible, not silent.

---

## 4. What the map does with it

**Reuse existing marks.** The map has `draw_hover` and `draw_blast` (claim 4), and A5's fresh ground, and A6's graticule. A fourth mark competing with those is a cost, not a feature.

Preferred: the **hover mark** for hovered, and something distinct-but-quiet for selected — the selected subject is usually also the open panel's subject, so a loud mark would be redundant with the panel beside it.

**Check what is already on screen.** Under `Relief`, with an overlay active, with fresh ground marked, with the graticule on, the map is not short of ink. §7's gate should be run in that state, not on a quiet map.

### 4.1 Do not re-aim the camera on selection

D1 §4 established that `aim_for_drilldown` fires **once, on open**, because re-aiming per frame takes the camera away from the operator.

Selection changing as a pointer moves down a list must not pan the map. **Marking is not navigation.** D4 (reverse indexing — click a row, fly there) is a separate, explicit action and a separate phase.

---

## 5. What D2 does not do

- **No camera movement on selection** (§4.1) — that is D4
- **No where-am-I marker during scroll** — that is D3
- **No new views.** D2 connects what exists
- **No selection for rows without a map position** (§3.3)

---

## 6. Tests

- [ ] One authority: setting the selection from a list and from a map click produces the same state, asserted by equality not by both-render
- [ ] Hover does not persist; commit does
- [ ] Closing the panel does not clear the selection (the reason the model changes at all)
- [ ] A row whose subject has no map position cannot be selected
- [ ] The map's mark and the list's mark agree about which entity is current — the anti-drift test, mirroring `the_tooltip_and_the_click_never_disagree`

**Mutation floor, exercised, and assert it applied.** Three times this session a mutation was reported surviving because `cargo fmt` had reflowed the target and the replacement matched nothing. Make the map ignore the selection and confirm a test fails; make a list ignore it and confirm a different one does.

---

## 7. The gate

**Select in a list, and the map marks it. Select on the map, and the list marks it.**

Run it on a **busy** map — Relief, an overlay active, fresh ground present, graticule on (§4).

### 7.1 The discrimination check

Standing requirement — eleven instruments in this project have produced a plausible result for an unrelated reason, and D1's own occlusion figure was the most recent.

**Run the gate with brushing disabled** and confirm the marks disappear. If a subject is still locatable — from the open panel, from the namespace colour §2 may have already shipped, from familiarity — then the gate is measuring something else.

Note that if §2 shipped, **the free version will pass part of this gate on its own.** That is the finding, not a failure: report which part colour covered and which part needed selection.

### 7.2 Failure criteria, stated in advance

- The mark is invisible against a busy map
- The mark is indistinguishable from hover, blast or fresh ground
- Moving the pointer down a list strobes the map
- A row highlights nothing because its subject has no position

---

## 8. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 4 is the live one.** If `Panel` stops being the selection, everything reading `Panel` to mean "what is selected" changes meaning. D1's lesson applies directly: I named two consumers of `panel_size` and the one that bit was a third I had not named.

**Question 2:** a selection whose subject has left the cluster is **stale**, not absent. Decide whether it clears, persists as a tombstone, or is refused — and make it say which.

**Question 5:** every §1 claim is inherited and the codebase has moved a lot. Claim 1 especially — `Panel`'s shape was read many versions ago.

---

## 9. Acceptance

- [ ] §2's free version tried first, and its result reported before selection work was scoped
- [ ] One selection authority; no view keeps a copy
- [ ] Hover and commit distinguished; no third level
- [ ] Rows without a map position cannot be selected, visibly
- [ ] No camera movement on selection
- [ ] Gate run on a busy map, with its discrimination check
- [ ] Failure criteria stated before the run
- [ ] Mutations asserted to have applied
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 10. Estimate

**Half a day for §2's colour promotion. One day for selection propagation if it is still needed after looking at §2.**

The honest possibility is that §2 covers most of it and D2 ends up much smaller than its name suggests — which has been the outcome of measuring first in every phase of this project that did so.
