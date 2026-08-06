# KuberNation — T1: Change-Since

**Implementation guidance**
**Goal:** colour the map by *what changed since a chosen moment*, rather than by current level.
**Gate:** show a change the Annals does not already say more clearly.

**This is the kill point for Workstream T.** See §7 before starting.

Governing doc: `kubernation-workstream-t-planning.md` §3 (T1), §6 (where this dies).

---

## 0. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report. VOR was unavailable while this was written, so **everything is `[A]`** — verify all of it.

| # | Claim | Tag |
|---|---|---|
| 1 | `freshness(occupied_at, now, window) -> Option<…>` is the renderer-facing interface for succession | `[A]` A5-render |
| 2 | `theme::fresh_tier` is the single bucketing authority, used by both the colour and the words | `[A]` A5-render §3.2 |
| 3 | `Net.fresh_window` is an atomic read per tick, so a window change re-tints within one tick | `[A]` A5-render §3.1 |
| 4 | `SlotState.occupied_at` is stamped on a change of hands only, persisted, cleared on re-occupation | `[A]` A5 core |
| 5 | `Overlay` is an enum with eight variants; `overlay_pair` is the colour funnel; `overlay_flat` is the minimap fallback | `[A]` substrate round |
| 6 | Overlay selection persists in prefs and takes a CLI flag | `[A]` A2, A5 |
| 7 | `--dump-positions` records provinces, ghosts and city references per model rebuild | `[A]` A3-pre, A6 |
| 8 | `--postmortem` renders `build_timeline` + `row_decisions` as text | `[A]` T-pre |

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 9 | The event ring keeps only each key's **latest** occurrence | `[A]` T0. Bounds what "since" can mean for event-derived change |
| 10 | kwok emits almost no events; kind is where event-derived behaviour must be measured | `[A]` T0 §2.4, T-pre |
| 11 | A5's fresh ground already answers "what changed and where" for **one** kind of change over **one** rolling window | `[A]` T planning §2 |

**Claim 11 is the phase's starting position.** T1 is not built from nothing — it generalises a shipped feature along two axes: a *chosen* baseline instead of a rolling one, and a *delta* instead of a boolean.

---

## 1. Blocked on T0's answer

**T0 has not been reported.** The T-fix-2 report cites "T0's finding" about the ring keeping only latest occurrences, so some of it was done, but the substrate question the planning doc raised is unanswered:

> How much history does KuberNation keep, and where? In-session ring, persisted store, or re-derivation from the current snapshot?

This decides §2 entirely:

| If history is | Then the baseline can be | And T1 is |
|---|---|---|
| in-session only | no earlier than app launch | cheap, weakest |
| persisted summaries | any past poll, at a few numbers per province | cheap, sufficient |
| persisted world states | any past poll, at full fidelity | expensive, and T3's substrate |

**Answer this before §2.** If the answer is "in-session only", T1 still ships — the baseline is "when I opened the app", which is a real and useful question — but say so plainly rather than implying a durable one.

---

## 2. What "change" means

**Pick one axis. Ship it. Do not build a change-since framework.**

The planning doc lists health, saturation, cost, pod count and occupant as candidates. The recommendation is **occupant**, for three reasons:

- A5 already stamps it (claim 4) and already renders a version of it (claim 11), so the substrate cost is near zero
- It is the axis where the Annals is *weakest* — a list of thirty node replacements does not show that they were all in one zone
- It needs no history store at all beyond the layout, which A4 already persists

That last point matters: **occupant change-since works even if T0's answer is the weakest one.**

The cheap generalisation from A5 is therefore:

| | A5 fresh ground | T1 change-since |
|---|---|---|
| baseline | rolling — now minus a window | **chosen** — a fixed moment |
| answer | boolean, aged | **did this change since T** |
| window | a duration setting | a point in time |

### 2.1 Choosing the baseline

The interaction is the phase's real design content, not the colouring.

Candidates, in order of increasing cost:

1. **Session start** — zero UI, answers "what has changed while I've been watching"
2. **A fixed set of offsets** — 15 min / 1 hour / today, as a menu radio like the ageing window
3. **An arbitrary instant** — needs a picker; almost certainly out of scope

Recommend (2), mirroring **View ▸ AGEING WINDOW**'s pattern exactly (claim 3, claim 6). A5-render's finding applies directly: a setting that can only be changed by restarting means finding a workable value costs one restart per guess.

**`0` or "off" must be a real value**, per every prior setting in this project.

---

## 3. Rendering

A new `Overlay` variant, on an axis that already has eight (claim 5). That is the entire integration: `overlay_pair` gains an arm, `overlay_flat` gains a terrain fallback like walls, cost and substrate.

Reuse what exists rather than inventing:

- **Bucketing** through a single authority, the way `fresh_tier` is shared by colour and words (claim 2). If T1's buckets differ from A5's, they need their own authority — but check whether they can be the same one first.
- **Colour** through the `cb_*` funnel. This is instrumentation.
- **Discrete, not continuous.** Substrate's finding stands: gaps are small integers where zero dominates, and a ramp washes a healthy fleet into near-identical tints. "Changed / did not change" is the extreme case of that.

### 3.1 The panel half is not optional

Substrate's standard, restated by A5-render §3.3: *the overlay says which node, so something must say what changed, or the map raises a question it cannot answer and the operator leaves for `kubectl`.*

A province coloured "changed since 1 hour ago" must be able to say **what** changed, in SELECTION. For the occupant axis that is: from which node to which, and when.

### 3.2 Do not collide with fresh ground

A5's fresh ground is a *rolling* mark that appears under every overlay. T1 is a *chosen-baseline* colour that appears only under its own overlay.

Both will be on screen together, and both are about occupant change. **They must be distinguishable, and the distinction must be explainable in one sentence** — if it cannot be, that is evidence they should be one feature rather than two, and that is worth reporting rather than working around.

---

## 4. The gate

**Show a change that the Annals does not already say more clearly.**

This is the kill point. Run it deliberately and report the answer plainly.

The comparison is concrete: the Annals is a working feature, on the same data, in the same app. For a given incident, put them side by side and answer:

- What does the map show that the list does not?
- What does the list show that the map does not?
- Which would you reach for first?

**The strongest available case for the map**, and the one to construct: a rolling refresh that touched one zone, or one pool, disproportionately. The Annals reports thirty replacements as thirty lines; the map should show them as a shape. If it does not show a shape the list conceals, T1 has not earned its place.

### 4.1 The discrimination check

Standing requirement — nine instruments in this project have emitted a plausible result for a reason unrelated to what they measured.

Run the gate with the overlay set to its off value. If the same conclusion is reachable from the map without it — from fresh ground, from the terrain, from familiarity with the fleet — then the gate is measuring something else.

### 4.2 It may be a human gate

Per T planning §4, no existing instrument measures *"whether a person learns something faster."* The pixel comparator measures rendering; `--dump-positions` measures assignment.

**Decide up front whether this gate needs a second person**, as A6's did, rather than discovering it at the end. If it does, say so and run it that way — a self-assessed usability verdict is not evidence.

### 4.3 Where

**kind for event-derived change; the churn fleet for occupant change** (claim 10). The occupant axis is the recommended one, so the churn fleet is the primary venue — scenario 1, biased to one zone if the fixture allows.

---

## 5. Tests

- [ ] A province whose occupant changed after the baseline is marked; one that changed before is not
- [ ] The baseline setting round-trips through prefs; the flag overrides for one run
- [ ] "Off" marks nothing, anywhere in the draw path
- [ ] An unknown `occupied_at` is **unknown**, not "unchanged" and not "changed" — standing question 2
- [ ] Bucketing is deterministic for the same `(occupied_at, baseline)`
- [ ] T1's colours are distinguishable from A5's fresh ground under every palette, asserted not eyeballed
- [ ] The SELECTION line and the overlay agree about whether a province changed — one authority, per claim 2's pattern

**Mutation floor, exercised not written:** make the change test always return "unchanged" and confirm a gui-smoke state loses its marking.

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?

**Question 6 is live and this is exactly its shape.** T1 compares `occupied_at` against a chosen baseline. A5 compares the same field against `now`. Two comparisons, one field, different right-hand sides — and T-fix-2's negative result applies: the rule is not "make both sides match," it is **know which quantity each side is, and say so.**

**Question 2:** an absent `occupied_at` means the slot has never changed hands *or* the record predates the field. Those are different, and neither is "unchanged since the baseline."

---

## 7. Where Workstream T dies

Named before the result is known, per the discipline A2 established and A4 §8.1 settled.

**If a change-since overlay shows nothing the Annals does not already say more clearly, then "what changed and where" is a question the list answers adequately** — and the map's advantage is confined to shape and anomaly, which it already has.

That would not invalidate Workstream A: stability is what makes the current map trustworthy, and A's gates passed on their own terms. But it would settle plan §7 negatively, and the honest response is to stop rather than build T2 and T3 on a thesis the cheapest phase refuted.

**Salvage:** T0's measurement, and the finding itself — which is information about the product that no amount of planning produces.

The instinct at that gate will be to blame the implementation, or to widen the axis and try again. Resist both. If the occupant axis — the one where the Annals is weakest and the substrate is already built — cannot beat the list, a more expensive axis is unlikely to.

---

## 8. Acceptance

- [ ] T0's substrate question answered before §2 is designed
- [ ] One axis shipped, not a framework
- [ ] Baseline selectable at runtime, persisted, with "off" a real value
- [ ] Discrete buckets through one authority; `cb_*` funnelled; minimap falls back
- [ ] SELECTION says **what** changed, not only that something did
- [ ] T1 and A5's fresh ground distinguishable, and the distinction stated in one sentence
- [ ] Gate run, with its discrimination check, and §4.2 decided in advance
- [ ] The gate's answer reported plainly, including if it is negative
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 9. Estimate

**One day**, if the occupant axis is taken and T0's answer is "in-session" or "layout only" — the substrate exists and the work is an overlay variant plus a setting.

Longer if T0 says a history store is needed, in which case **stop and rescope**: a phase whose purpose is to test a thesis cheaply should not acquire a storage format first.
