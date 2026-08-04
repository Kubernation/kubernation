# KuberNation — A5-render: Fresh Ground and the Wave Gate

**Implementation guidance**
**Goal:** make succession visible on the map, and find out whether a rolling refresh reads as a wave.
**Gate:** run scenario 1. **Does it read as a wave?** — with a mandatory discrimination check.

Second half of A5. The core shipped in v1.9.1; cataclysm was resolved as a record, not a rendering.

---

## 0. Verify before building

**Claims are tagged.** `[V]` was verified against source while writing this document. `[A]` is asserted from a prior report and carries only that report's authority.

The distinction exists because A5's claim 5 was false — I stated `attention::build` takes `now` as an example of the house convention when it is the sole *exception*, and I stated it in the same unmarked table as four claims I had actually checked. Verify both kinds; treat `[A]` as the likelier to be wrong.

| # | Claim | Tag |
|---|---|---|
| 1 | `freshness(occupied_at, now, window) -> Option<f32>` (or similar) is the whole renderer-facing interface | `[A]` A5 report §6 |
| 2 | `freshness` returns `None` for all three do-not-mark states: never changed hands, timestamp unknown, window zero | `[A]` A5 report §5 |
| 3 | `layout.occupied_at(slot)` exists and is reachable from the draw path | `[A]` A5 report §6 |
| 4 | Ghost ground renders in a colour deliberately **outside** the meaning palette and outside the `cb_*` funnel | `[A]` A2 report §5 |
| 5 | `Continent.ghosts` is not a `Province` — a ghost has no node, health, or cities | `[A]` A2 report §5 |
| 6 | Ghosts settle at **~12** on the churn fleet, not the batch size of 10, because reclaim is per-(zone, pool) | `[A]` A5 report §1, measured live |
| 7 | `--shot-seq N` / `--shot-interval S` capture numbered frames from **one** process | `[A]` A2 report §8 |
| 8 | Scenario 1 surges and refreshes in waves with configurable overlap | `[A]` A-pre |
| 9 | `prefs.rs` persists settings; `--overlay` / `--map-style` are the flag-plus-prefs precedent | `[A]` A4, A2 |

**Claim 6 is the one that shapes the design** — see §2.3. Verify the number on the current fleet rather than trusting it; it was measured once.

**Claim 1 is the seam.** If the interface differs from what A5's report describes, stop and report rather than adapting around it — the whole phase is built on it being a single call.

---

## 1. What this phase decides

The core made succession *detectable*. This makes it *legible*, and the guidance for the core deliberately deferred one judgment:

> Whether ageing should be **quantised into two or three steps** rather than a continuous fade, since a continuous fade is hard to read as a wave.

That decision has to be made against the live map. **It is the phase.** Everything else here is wiring around it.

---

## 2. Fresh ground

### 2.1 It is instrumentation

Fresh ground encodes cluster state, so per the plan's standing discipline it is **instrumentation, not scenery**:

- Routes through the `cb_*` funnel
- Reads identically in every map style — `Plain` and `Relief` today, any register later
- Does not vary by aesthetic choice

### 2.2 It must be distinguishable from ghost ground

During a surge, fresh ground and ghost ground are on screen **simultaneously and adjacent** — a wave leaves vacancies behind it and new occupancy ahead of it.

Ghost ground already occupies a colour deliberately outside the meaning palette (claim 4). Fresh ground needs to be separable from it at a glance, not merely different on inspection.

Worth stating what the two mean, since the visual should follow:

| | Means |
|---|---|
| ghost | ground held for a node that is **gone** |
| fresh | ground whose occupant **just changed** |

One is absence, the other is recent change. If the treatment makes them look like two shades of the same thing, the wave will read as damage rather than as motion — which §4.2 lists as a failure mode.

### 2.3 Quantised or continuous — the decision

Try quantised first. Two or three steps, not a smooth ramp.

The argument for quantisation: a wave is perceived by its **leading edge**, and a continuous fade has no edge — it has a gradient, which the eye reads as a smear. Discrete steps put a boundary where the front is.

The argument against: steps flicker at the boundary as ground crosses between them, and on a fleet with many partitions (claim 6) there may be enough marked ground at once that steps look like noise.

**Decide against the live map.** Build both if the cost is small — the difference is a quantise step in one function — and look at them.

Note the interaction with claim 6: the standing quantity of marked ground is *per-partition*, so a many-partition fleet carries more marked ground simultaneously than a single-partition estimate suggests. That argues for **fewer, more distinct steps** rather than more subtle ones.

### 2.4 The ageing window

A persisted, flag-overridable setting, following the `--overlay` / `--map-style` pattern (claim 9): settable in the UI, persisted in prefs, with a flag for one-off runs.

**Declared default: on the order of an hour.** The reasoning, stated so it can be argued with — the marking should survive an operator stepping away during a rollout, and be gone by the next working day so a morning map is not covered in marks from yesterday's routine churn.

**This is a judgment, not a measurement.** Say so where a user can see it, the way the substrate prevalence heuristic does.

`0` means *never mark* and must be a real supported value. Per claim 2 the core already returns `None` for it.

---

## 3. What this phase does not do

- **No cataclysm rendering.** Resolved as a record in v1.9.1 — there is nothing left to draw on.
- **No small multiples, no change-since overlay, no fault-line marking.** Plan §7, and they depend on A6's graticule as the invariant frame.
- **No new detection.** The core owns it; this reads `freshness`.
- **No change to ghost rendering** beyond whatever §2.2 requires for separability.

---

## 4. The gate

**Does a rolling refresh read as a wave?**

Run scenario 1 with `--shot-seq` across a multi-wave refresh, all four flags pinned (`--center`, `--zoom`, `--overlay`, `--map-style` — the last two persist in prefs).

### 4.1 The discrimination check is mandatory

Three phases now where its absence would have published a meaningless result — A2's flipbook was blind to the layout carry, A4's first restart tests passed the mutation, and A4's gate as specified passed with the layout file deleted.

**Run the gate with the window set to `0`** and confirm the captures differ. A gate that looks the same with the mechanism disabled is measuring something else.

### 4.2 State the failure criteria before running

A wave is a perceptual judgment, and the temptation to grade generously after the fact is real. Failure looks like:

- Marks appear but form no visible leading edge
- The wave is invisible at the zoom where a fleet is actually viewed
- Fresh ground is not separable from ghost ground during the surge
- The map reads as **damaged** rather than as **changing**

If the tier boundary is wrong, the most likely cause is the window: too long and everything is marked, too short and the wave has no body. Second most likely is the quantisation choice from §2.3.

### 4.3 What it does not settle

Per A4 §8.1 and the settled position in the open-decisions doc: this gate answers whether the marking reads. It does not answer whether change-over-time makes the map more useful than the Annals' list — that is plan §7, and needs the time-series work.

Report it as what it is.

---

## 5. Tests

Rendering is not unit-testable, and pretending otherwise wastes the session. Test what is testable and gate the rest.

- [ ] The window setting round-trips through prefs; the flag overrides for one run
- [ ] Window `0` produces no marking anywhere in the draw path
- [ ] Fresh and ghost resolve to different colours under **every** style and under colour-blind mode — the §2.2 requirement, as an assertion rather than an eyeball
- [ ] Quantisation boundaries are deterministic: the same `(occupied_at, now, window)` yields the same step

**Mutation floor, exercised not written:** make `freshness` always return `None` and confirm the marking disappears from a gui-smoke state. If nothing fails, nothing was testing it.

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims here were inherited from a prior report rather than verified against the case at hand?

Question 2 is live at the draw site: `freshness` returning `None` means *do not mark*, and it must not fall through to a default that marks. Three distinct causes share that answer (claim 2) and all three must land in the same place.

Question 5 has a sharpened form from A5: **verifying a type's shape is not verifying its inhabited states.** A5's core found that `SlotChange`'s replacement case — which I had verified existed as a variant — never occurs in practice, because a refresh drains in one tick and reclaims in another. Ask of each claim here not only "is this true of the type" but "does this state occur."

---

## 7. Acceptance

- [ ] Fresh ground rendered from `freshness`, routed through `cb_*`, reading in every map style
- [ ] Fresh separable from ghost at a glance during a surge, asserted by test
- [ ] §2.3 decided against the live map, with the reasoning recorded
- [ ] Window is a persisted, flag-overridable setting with a declared default; `0` means never
- [ ] The default is labelled a judgment, visibly to the user
- [ ] Gate run **with its discrimination check**, failure criteria stated beforehand
- [ ] Mutation floor exercised
- [ ] Standing questions answered, claims tagged `[V]`/`[A]`
- [ ] `cargo nextest` green

---

## 8. Estimate

**Half a day to a day.** The wiring is small — one call, one setting, one colour decision. The gate and the §2.3 judgment are the work, and they need the churn fleet in front of you.

This closes A5. A6 (graticule and declared frame) is the last phase in Workstream A.
