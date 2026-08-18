# D1 — de-modalise the drill-down

**Guidance:** `docs/kubernation-d1-demodalise-guidance.md`
**Version:** v1.22.0 · **Date:** 2026-08-07

**Gate: PASSED.** Opening a city or a node leaves the subject visible, with its
neighbours, its zone label and the graticule beside it. Occlusion of the play
area falls from **~100%** (panel 93% plus a scrim over the remainder) to **66.6%**
(panel only, no scrim).

Shape chosen by the user from §4's three candidates: **dock right of the map, and
pan so the subject stays visible** — the only one that satisfies the acceptance
criterion by construction rather than by luck.

455 core + 115 GUI tests; gui-smoke 55.

---

## 1. §1 — claims verified

Seven of eight TRUE. Claim 2's warning was warranted in spirit — `panel_size` had
moved — but it is at `:893` as claim 1 says and returns exactly what claim 2 says.

**Claim 5 is FALSE, in a bounded way.** It says three functions reimplement
`draw_window`'s centring. `panel_size` does not: it returns a *size*, centres
nothing, and its doc references `draw_window`'s cap rather than claiming to
mirror it. Placement lived in **three** places — `draw_window`, `panel_frame`,
`panel_split_x` — and `panel_size` was already the shared sizing helper both
mirrors called.

§2's prescription is unchanged by that; the work was simply smaller. Reported
rather than adapted around, per the standing rule.

---

## 2. §2 — the structural half, landed first

`window::window_rect_at(size, sw, sh, Place)` is the one home for placement.
`draw_window`, `panel_frame` and `panel_split_x` all consult it, and the twelve
non-drill-down windows are untouched (two of them pinned by concrete rect).

**The test had to change shape to be worth anything.** `panel_frame ==
window_rect_at(..)` became a tautology the moment `panel_frame` delegated — it
proves delegation and cannot detect the geometry moving. The test pins the actual
rect instead. (It immediately caught my own arithmetic: About's `y` is 30, not 40.)

**§6's stated mutation is aimed backwards.** It says to change the placement in
the authority and expect the agreement test to fail — but if placement really has
one home, changing it moves everything *consistently*, which is the point. The
mutation that detects a re-introduced mirror is changing a **consumer**. Both are
now run: moving the authority fails the concrete rects; re-mirroring in
`panel_split_x` fails with `split stopped tracking the authority`.

Both were first reported as *surviving*, because `cargo fmt` had reflowed the
target and the replacement matched nothing. They now assert they applied — the
third time this session that an unasserted mutation looked like a test gap.

---

## 3. §4 — the shape, and what it cost

Docked flush with the play area's right edge, so the docked column stays visible;
it is the other context surface, and hiding it to show the map would move the
problem rather than solve it.

**The scrim is gone for docked windows.** A 50% black wash over the map would
have given back most of what docking bought. It stays for the twelve centred
windows, which are modal and should look it.

| | width | map strip |
|---|---|---|
| before | 1100 centred + full-screen scrim | none |
| after | 758 docked | **358px** |

### 3.1 §4.1's content cost — assessed, and it was real

The guidance asked for this to be weighed, not discovered. It was nearly
discovered: at 758px the left column is 402px, and the fwd/yaml/evict cluster is
a **fixed 156px**, leaving 246px of clear space against a full pod row of ~329px.
**The row text ran under the hover buttons** — §7.2's second failure criterion.

Not a pre-existing problem: at the old 1100px panel the column was 590px and had
434px clear. The dock created it.

Fixed by deriving the budget from the column instead of a constant
(`panels::row_char_budget`, pure and unit-tested), and spending it on the
*name* — the hash half of `web-f56f55fb4-j78nf` is the least useful part of the
row, so the state/restarts/age/usage tail survives intact. Applied to the city's
CITIZENS and the node's GARRISON, which had the same shape.

**No content is lost**, then — the rows keep every field and shorten a name.
Mutation: setting the reserved strip to zero fails the budget test.

---

## 4. The pan, and the defect it exposed

`Camera::fly_to_within(cell, view)` aims at a screen rect rather than the screen
centre, with `fly_to` reimplemented as its whole-screen case so the two cannot
disagree. It fires **once, on open** — re-aiming per frame would take the camera
away from the operator, which is worse than the occlusion it fixes.

**The defect, caught before the gate rather than by it.** `--inspect` — the dev
flag used to *photograph* the gate — called `cam.jump_to`, which centres on the
whole screen and therefore parked the subject **underneath the docked panel**.
That is §7.2's first failure criterion, produced by the instrument that captures
the gate: it would have recorded a failure as a pass.

Both open paths now go through one `aim_for_drilldown`. This is the same lesson
as `resolve_region` and `terrain_order` — when two callers must agree about a
derived thing, it gets a name and one home — arriving this time through an
instrument rather than through the product.

`map_strip` returns `None` below 220px, so a small window declines to pan rather
than aiming at a sliver (§7.2's fourth criterion, refused rather than rendered).

---

## 5. §7.1 — the discrimination check, and why the pixel figure misleads

Measured with the committed `compare.py`, no new comparator:

```
before, panel open vs closed:  99.4% of the play area changed
after,  panel open vs closed:  88.1%
```

**88.1% is not the occlusion, and reporting it as such would be wrong.**
Decomposing it:

```
visible map strip:  72.3% changed   <-- the camera PANNED; this is the feature
panel area:         95.5% changed   <-- this is the occlusion
```

The pixel metric conflates covering the map with moving it. The honest occlusion
figure is geometric: panel 758×812 against a play area of 1116×828 = **66.6%**,
down from 93% panel **plus a scrim over everything else**.

That decomposition is the check doing its job. Had I quoted 88.1% as "occlusion
after", it would have been a plausible number measuring something else — the
failure this project has now catalogued eleven times.

---

## 6. §8 — standing questions

**1. Summing before comparing?** Not present.

**2. Unknown, or fabricated?** `map_strip` returns `None` when the strip is too
narrow to aim at, rather than a rect nobody should use; `row_char_budget` returns
0 for a column narrower than the buttons, not a negative.

**3. Two sections constraining one behaviour, with a fixture where they diverge?**
§4 (dock narrow, for map) and §4.1 (keep the content) pull against each other,
and 758px is exactly where they diverged — see §3.1.

**4. Consumers depending on an old meaning?** The question named this precisely:
`panel_size`'s value feeds `panel_split_x` and `panel_frame`, which encode
proportions computed against ~1100px. Both were re-derived from the frame. The
third consumer the question did *not* name is the one that bit — the pod rows'
fixed character caps, §3.1.

**5. Inherited claims?** Seven of eight held; claim 5 did not, §1.

**6. One side of a comparison moved?** This is the phase, and it is why §2 came
first: drawing, hit-testing and scroll routing all had to keep meaning the same
thing while the geometry moved. They now derive from one function.

**7. Container adjacency as world adjacency?** Not present.

---

## 7. §9 — acceptance

- [x] Placement collapsed to one authority **before** the geometry changed
- [x] Scope held to the two drill-downs; the other twelve pinned unchanged
- [x] Subject province visible with the panel open — both gates captured
- [x] Content loss at the new width assessed explicitly (§3.1) — it was real, and fixed
- [x] Hit-testing, scroll routing and drawing verified to agree
- [x] Occlusion measured before and after with `compare.py` — and decomposed, §5
- [x] Failure criteria stated in advance (the guidance's §7.2)
- [x] The other twelve windows unchanged
- [x] Standing questions answered, claims tagged
- [x] `cargo test` green

**Deviation:** §6's mutation-floor instruction was run as written *and* in the
form that actually detects the failure it describes (§2).

**Not done, deliberately:** keyboard shortcuts still treat an open drill-down as
modal. Which keys should belong to a docked panel is a real question and D1 §5
says this phase changes no behaviour beyond the geometry it must. Pointer gates —
click, hover, tooltip — now key on *being over the panel*, which is what §6's
test list asks for.

**Unblocks D2–D4** (brushing, visual momentum, reverse indexing), none of which
were touched.
