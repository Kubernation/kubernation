# KuberNation — D1: De-modalise the Drill-Down

**Implementation guidance**
**Goal:** the map survives a drill-down, so the overview keeps supplying context while a local view is read.
**Gate:** open a city and a node window. **Can you still see where you are?**

Workstream D, phase 1. Sequenced first in the enabling plan and never started.

---

## 0. Why this was ranked first

Wickens' *Engineering Psychology and Human Performance* reports a consensus that multiple views should pair a global view **with a stable world frame of reference** against local views — and that the global context should be **preserved through** the later phases, not replaced by them. Losing it produces the *keyhole phenomenon*, which the text notes is worst when scrolling a list.

`panel_size` is `(sw - 80).clamp(900, 1100)` by `(sh - 80).clamp(560, 1000)`, centred. On a default window that occludes essentially the whole map.

So this is **a defect against a stated design standard**, not a layout preference. It also retroactively explains why selection outlines were never worth building: the map isn't visible to draw on.

Workstream A spent eleven versions making the map hold still. **A stable map you cannot see while working is worth very little**, which is the argument for doing this before any further map feature.

---

## 1. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report or earlier in this thread. VOR became unavailable mid-scan; verify all `[A]` entries.

| # | Claim | Tag |
|---|---|---|
| 1 | `panel_size` is at `panels.rs:893` | `[V]` |
| 2 | It returns `(sw - 80).clamp(900, 1100)` × `(sh - 80).clamp(560, 1000)` | `[A]` earlier in thread; **re-read, it may have moved** |
| 3 | Only `city::draw_city` and `node::draw_node` call `panel_size`; `panel_split_x` and `panel_frame` call it internally | `[A]` |
| 4 | `window::draw_window` is the single placement authority, called by **14** windows | `[A]` |
| 5 | `panel_size`, `panel_frame` and `panel_split_x` each independently reimplement `draw_window`'s centring, with doc comments saying they "mirror" it | `[A]` |
| 6 | `sidebar.rs` reads the same `region_lines` in a non-occluding presentation | `[A]` |
| 7 | macroquad has **no scissor** — `panels.rs` culls rows against a view rect because clipping is unavailable | `[A]` |
| 8 | `panel_frame` is called once, from `main.rs`, and gates scroll routing | `[A]` |

**Claim 5 is the structural problem**, and claim 4 bounds it. See §2.

**Claim 7 constrains the solution:** a docked panel cannot rely on clipping to keep its content inside its own rect. Whatever shape is chosen must cull, as the existing windows already do.

---

## 2. The structural half

Placement lives in **four** places held together by convention: `draw_window` and three functions whose doc comments say they mirror it. Change the geometry and all four must move in step, with nothing enforcing it.

This is the shape this codebase has paid for six times — `resolve_region`, `derive_qos`, `worst_level`, `changed_hands`, `fresh_tier`, `slot_of_row` — and the standing rule that came out of it:

> **Whenever a value has an inverse, the inverse gets a name and one home.** Here it is not an inverse but a *shared derivation*: hit-testing and scroll routing must agree with drawing about where the window is.

**Collapse placement to one authority before changing it.** One function returns the window's rect; drawing, hit-testing and scroll routing all consult it. That is the valuable half of this phase, and it is what makes the geometry change safe rather than a four-way edit.

---

## 3. Scope: two windows, not fourteen

Claim 4 says `draw_window` serves fourteen windows. **D1 is two of them.**

The keyhole argument applies where the map is *context for what you are reading*:

| Window | Drill-down? |
|---|---|
| `city::draw_city` — a workload | **yes** |
| `node::draw_node` — a province | **yes** |
| Almanac, About, Charter, Workloads, Oracle, Annals, Plan, Chaos… | no — not about a map location |

Occluding the map while reading the Almanac is fine. **Do not de-modalise all fourteen** — that is a much larger change with no supporting argument.

---

## 4. The shape

Not prescribed, because it is a design judgement that should be made against the live map. Three candidates:

| | |
|---|---|
| **Dock to one side** | `sidebar.rs` is the in-tree precedent (claim 6). Simple, predictable, costs horizontal map width permanently |
| **Shrink and offset** | Keep the window, make it smaller and push it to a corner. Smallest change; may still occlude the province you just clicked |
| **Dock, with the map panning to keep the subject visible** | Best context preservation, most work, and it interacts with A6's declared frame |

**Whatever is chosen, the subject must remain visible.** A drill-down that hides the very province it describes has moved the keyhole rather than removed it. That is the acceptance criterion, not the panel's position.

### 4.1 The content problem this creates

The current window is ~1100 × 780. Anything narrower has to drop or reflow content, and these windows are dense — the city window truncates row text at **30 characters** when a rollback button is present, which is already tight enough that T-fix found the suspect cue being dropped first.

**Check what falls off before committing to a width.** If a docked panel forces the city window to lose rows it currently shows, that is a real cost to weigh against the context gained — and it should be weighed explicitly, not discovered.

---

## 5. What D1 does not do

- **No brushing.** D2/D3/D4 — selection propagation, visual momentum, reverse indexing — are separate and depend on this landing first.
- **No selection outline on the map.** That was dropped as not worth building *because* the map was invisible. Reconsidering it is a later decision, not part of this.
- **No change to the other twelve windows** (§3).
- **No new content** in either window.

---

## 6. Tests

Placement is screen geometry, which the GUI testability policy concedes is unassertable. So test the **authority**, not the pixels:

- [ ] Drawing, hit-testing and scroll routing all derive the window rect from one function — assert they agree for several screen sizes
- [ ] A click inside the panel does not reach the map; a click outside it does
- [ ] Scroll inside the panel scrolls the panel; outside, it zooms the map
- [ ] Content culling still respects the panel's rect at the new size (claim 7 — no clipping to fall back on)
- [ ] The other twelve windows are unchanged — assert their rects against today's values

**Mutation floor, exercised:** change the placement in the one authority and confirm the hit-testing agreement test fails. If it passes, the four-way mirror is still there and §2 was not actually done.

---

## 7. The gate

**Open a city window and a node window. Can you still see where you are?**

Specifically: is the subject province visible, and is enough of the surrounding map visible to locate it?

### 7.1 The discrimination check

Standing requirement — ten instruments in this project have produced a plausible result for an unrelated reason, and the last one shipped a false positive.

**Capture the same frame with the panel open before and after.** Report the map area occluded, as a share of the play area, using the committed `compare.py` rather than a new comparator. Today's figure is near-total; if the new figure is not substantially lower, the change did not do what it claims.

### 7.2 Failure criteria, stated in advance

- The subject province is hidden by the panel
- The panel is so narrow the window loses content it currently shows (§4.1)
- Hit-testing and drawing disagree about the panel's edge at any screen size
- The map is visible but too small to locate anything in

---

## 8. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 6 is the phase.** The window's rect is used by drawing, hit-testing and scroll routing. Move it in one and the others must mean the same thing — which is exactly what four independent mirrors do not guarantee.

**Question 4:** `panel_size`'s return value is consumed by `panel_split_x` and `panel_frame` (claim 3). Both encode assumptions about the window's proportions, not just its origin. A narrower panel may break a split ratio that has always been computed against a ~1100px width.

**Question 5:** most of §1 is `[A]`. Claim 2 especially — `panel_size` has moved once already this session (it was at :700 earlier in this thread, now :893).

---

## 9. Acceptance

- [ ] Placement collapsed to one authority before the geometry changes (§2)
- [ ] Scope held to the two drill-down windows (§3)
- [ ] Subject province visible with the panel open
- [ ] Content loss at the new width assessed explicitly, not discovered (§4.1)
- [ ] Hit-testing, scroll routing and drawing verified to agree
- [ ] Occlusion measured before and after with `compare.py`
- [ ] Failure criteria stated before the gate was run
- [ ] The other twelve windows unchanged
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 10. Estimate

**One day.** The authority collapse (§2) is most of it; the geometry change is small and the judgement in §4 needs the live map in front of you.

This unblocks D2–D4, which is where the coordinated-views argument actually pays off — but those are separate phases and this one should be judged on its own gate.
