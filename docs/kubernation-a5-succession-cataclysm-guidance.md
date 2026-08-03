# KuberNation — A5: Succession and Cataclysm

**Implementation guidance**
**Goal:** make change legible on the map — routine replacement as fresh ground that ages, structural change as a recorded scar.
**Gate:** a rolling refresh reads as a **wave** crossing the map.

Governing docs: `kubernation-workstream-a-decomposition.md` §4 (A5), plan §3.2 (succession vs cataclysm), §7 (time).

---

## 0. Verify before building

### Structural — **verified against source while writing this document**

| # | Claim | Verified |
|---|---|---|
| 1 | `changes_from(prior) -> Vec<SlotChange>` over the **union** of both key sets; doc says *"Pure comparison; A5 decides what is a cataclysm"* | ✅ `layout.rs:275` |
| 2 | `stamp_vacancies(now)` is caller-clocked, and `vacated_at` is read by nothing today | ✅ `layout.rs:224` |
| 3 | It stamps **only unstamped** vacancies, so the value means "how long has this vacancy stood" | ✅ same |
| 4 | `home_of(node)` finds the slot a node last held via `last_occupant` — the ground a returning node reclaims | ✅ `layout.rs:265` |

Re-verify anyway: this document was written against `v1.9.0` and the session may be starting from a later tree.
| 5 | `attention::build` and `build_timeline` take `now` as a parameter — the established clock convention | `state/attention.rs`, `timeline.rs` |
| 6 | `Continent.ghosts` is deliberately **not** a `Province` — a ghost has no node, so no health, no cities | A2 report §5 |
| 7 | Ghost ground renders in a colour outside the meaning palette and outside the `cb_*` funnel | A2 report §5 |
| 8 | `--dump-positions` records ghost slots and province records per model rebuild | A3-pre report §3 |

**Claim 2 is the phase's hinge.** A4 carried `vacated_at` specifically for A5. If it turns out not to be stamped as described, A5's ageing has no input.

### Semantic

| # | Assumption | Source | Why it matters |
|---|---|---|---|
| 9 | Ghosts steady-state at refresh batch size, not cumulative | A4 verification, re-measured | Sizes how much fresh ground is on screen at once |
| 10 | A rolling refresh surges — replacement Ready before predecessor drains | A-pre, verified at 115 | Fresh ground and ghost ground coexist during a wave |
| 11 | Scenario 1 replaces in waves, with configurable overlap | A-pre §2 | The gate's instrument |

### Inherited claims (standing question 5)

Claims 2, 6, 7, 9, 10 and 11 come from prior reports. **Verify each against the case at hand.** Claim 2 especially: A4 states it was pinned by test precisely because nothing read it, and A5 is the first reader.

---

## 1. The two tiers

Settled in plan §3.2, and the distinction is what keeps the marking legible:

| | Trigger | Marking | Lifetime |
|---|---|---|---|
| **Succession** | a slot changes occupant — the routine case | **fresh ground** on the province | ages back to normal |
| **Cataclysm** | structural change: a pool or zone appears or vanishes, a compaction | a **scar**, dated | permanent in the record |

> **Scars must be rare or they are noise.** If every rolling refresh scarred the map, the world would be nothing but scars within a month. Reserving cataclysm for genuine topology change is what keeps it carrying information.

Per A4's finding, succession is common and bounded — batch-size ghosts at steady state — so fresh ground is a recurring, self-clearing state, not an accumulating one.

---

## 2. Succession: fresh ground

### 2.1 Detection

A slot whose occupant changed. `changes_from` already computes transitions between two layouts (claim 1) — **use it rather than inventing a parallel detector.**

`SlotChange { slot, from, to }` distinguishes all three transitions directly: `from: None` is an arrival, `to: None` is a departure, both `Some` is a replacement. **Succession is the both-`Some` case.**

The signal A5 needs is per-province: *this slot's occupant changed at time T*. Two candidate sources:

- **`changes_from` between consecutive rebuilds** — transient, and a change occurring while the app is closed would be missed
- **A stamp on the slot**, symmetric with `vacated_at`

Prefer the stamp. A refresh that happened overnight should still read as fresh when the operator opens the map in the morning — that is the whole point of A4 having made layouts persist. A transient-only detector would make succession invisible in exactly the case where it is most useful.

So: `SlotState` gains `occupied_at`, stamped when a slot takes a **new** occupant (not on every rebuild), persisted alongside `vacated_at`, and driven by the same caller-supplied `now` per claim 5.

**Format change:** adding a field to `StoredSlot`. Check whether A4's stored `version` needs bumping — an added optional field is normally backward compatible, and `prefs.rs`'s convention is to bump only on incompatible change.

### 2.2 The ageing window

Fresh ground fades back to normal over a window. **Make it a setting with a declared default**, following A4's precedent for the retention window that was ultimately dropped, and `--map-style`/`--overlay`'s pattern for persisted, flag-overridable settings.

Default: something on the order of an hour. The reasoning to state so it can be argued with — the marking should survive an operator stepping away from the screen during a rollout, and should be gone by the next working day so a morning map is not covered in marks from yesterday's routine churn. **This is a judgment, not a measurement**, and it should say so where a user can see it.

`0` must mean *never mark*, and be a real supported value.

### 2.3 Rendering

Fresh ground is **instrumentation, not scenery** — it encodes cluster state, so per the plan's discipline it must read identically in every map style and route through the `cb_*` funnel.

Note what already exists to avoid colliding with: ghost ground is a colour deliberately outside the meaning palette (claim 7). Fresh ground needs to be distinguishable from ghost ground at a glance, since during a surge both are on screen simultaneously (claim 10) and adjacent.

The obvious treatment is a tint that decays with age. Consider instead whether the ageing should be **quantised into two or three steps** rather than continuous: a continuous fade is hard to read as a wave, and quantisation would make the leading edge visible as an edge. This is the phase's main aesthetic decision and it should be made against the live map, not in advance.

---

## 3. Cataclysm: scars

### 3.1 What qualifies

Structural change only:

- A pool appears or vanishes — derivable from `SlotKey.pool` across `changes_from`
- A zone appears or vanishes — **`zone_ordinals()` already yields zones whose nodes have all departed**, so a vanished zone is directly observable rather than needing inference
- A compaction (A4's explicit verb — it already records an event)
- A fingerprint mismatch discarding a layout (A4's migration cataclysm)

**Not** a node replaced, not a node added or removed within an existing pool.

### 3.2 Recording

A4 established that compaction and fingerprint-mismatch already record events. A5 should read those rather than build a second event path — a parallel record would drift, which is the failure `resolve_region` and `derive_qos` were both promoted to prevent.

Check what A4's event record actually contains before designing on top of it. If it is a log line rather than a structured record, that is a small extension, and doing it now is cheaper than after A6.

### 3.3 Rendering, and the honest limit

A scar is permanent in the record. **Whether it is permanently visible on the map is a separate question**, and worth deciding deliberately rather than by default:

- A vanished zone **keeps its reserved ordinal** (`zone_ordinals` retains it deliberately, so neighbours do not slide over) — so there *is* a position, but no provinces on it
- A vanished pool's slots become ghosts, which already render
- A compaction removes ghost ground

So there may be **nothing left to draw on**. If that is the case, say so: cataclysm may be a *record* rather than a *rendering*, surfaced in the Annals rather than on the terrain, and the honest outcome is to report that rather than invent a mark with nowhere to sit.

This is worth checking early — it could substantially shrink §3.

---

## 4. The gate

**Does a rolling refresh read as a wave?**

Run scenario 1 on the churn fleet with `--shot-seq`, capturing across a multi-wave refresh with all four flags pinned.

### 4.1 The discrimination check is mandatory

Per A4's finding — **three phases where its absence would have published a meaningless result** — run the gate against a build with the marking disabled (ageing window `0`) and confirm the result differs.

A wave is a *perceptual* judgment, so state up front what would count as failure:

- Marks appear but do not form a visible leading edge
- The wave is invisible at the zoom where a fleet is actually viewed
- Fresh ground is indistinguishable from ghost ground during a surge
- The map reads as damaged rather than as changing

If the tier boundary is wrong, the most likely cause is the ageing window — too long and everything is marked, too short and the wave has no body.

### 4.2 What the gate does not settle

Same discipline as A4 §8.1. This gate answers whether the marking reads. It does not answer whether change-over-time makes the map more useful than the Annals' list — that is plan §7's territory and needs the time-series work, not this phase.

---

## 5. What A5 does not do

- **No small multiples, no change-since overlay, no fault-line marking.** Those are plan §7 and depend on A6's graticule as the invariant frame.
- **No automatic reap.** Settled in A4.
- **No change to `assign_layout`'s algorithm** beyond carrying `occupied_at`.
- **No new observation.** Everything needed is in the layout.

---

## 6. Tests

**Detection:**
- [ ] A slot taking a new occupant stamps `occupied_at`; a slot rebuilt with the same occupant does not re-stamp
- [ ] The stamp survives save and load
- [ ] A slot that goes ghost and is reclaimed stamps afresh
- [ ] Succession is detected for a change that occurred while the app was closed — **the case that motivates the stamp over a transient detector**

**Ageing:**
- [ ] Fresh at T, normal at T + window, with a fixed clock
- [ ] Window `0` marks nothing
- [ ] An absent `occupied_at` is unknown, not infinitely old — do not mark, do not crash

**Cataclysm:**
- [ ] Pool appearance and disappearance are detected; a node replacement is not
- [ ] Compaction and fingerprint mismatch surface through the same record, not a parallel one

**Discrimination (per A4 §5):**
- [ ] Every test that could pass without the feature carries a guard-the-guard assertion declaring itself non-discriminating rather than passing

**Mutation floor:** make the ageing always return "not fresh" and confirm the succession tests fail. Exercise it, do not merely write it.

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims here were inherited from a prior report rather than verified against the case at hand?

Question 2 is live: an absent `occupied_at` — older file, or a slot never restamped — is **unknown**. Marking it fresh would paint the whole map on first load after an upgrade; treating it as infinitely old is the safe reading and should be explicit.

Question 4 applies to `StoredSlot`: A4 froze a format that now gains a field, and it is the first artifact in this workstream that outlives the process.

---

## 8. Acceptance

- [ ] `occupied_at` stamped on new occupancy only, persisted, cleared appropriately
- [ ] Format version decision made and justified
- [ ] Ageing window is a setting with a declared default; `0` means never
- [ ] Fresh ground routes through the `cb_*` funnel and reads in every map style
- [ ] Fresh ground is distinguishable from ghost ground during a surge
- [ ] Cataclysm reads A4's existing event record, not a parallel one
- [ ] §3.3 decided: whether cataclysm is a rendering or only a record
- [ ] Gate run **with its discrimination check**
- [ ] Standing questions answered, question 5 with sources tagged
- [ ] `cargo nextest` green

---

## 9. Estimate

**One to two days.** Detection and ageing are small; the rendering decision in §2.3 and the §3.3 question are where the time and the judgment go.

The consistent overrun in this series has been consumer sweeps and review rounds. A5 adds a persisted field to a format A4 just froze, which is the surface most deserving of review attention.
