# KuberNation — A6: Graticule and Declared Frame

**Implementation guidance**
**Goal:** make a position on the map nameable, and state in the legend what the frame is anchored to.
**Gate:** one person names a position from the map; another finds it without further explanation.

The last phase of Workstream A — and the prerequisite for plan §7's time-series work.

---

## 0. Verify before building

**Claims are tagged.** `[V]` verified against source while writing this document. `[A]` asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | `WorldModel { width, height, continents, islands, city_count }` — `width`/`height` are `u16` world cells | `[V]` `world.rs:212` |
| 2 | `Layout::zone_ordinal(zone) -> Option<u16>` gives a zone's **durable** continent ordinal | `[V]` `layout.rs:174` |
| 3 | `zone_ordinals()` yields every zone **including those whose nodes have all departed** — their ground stays reserved | `[V]` `layout.rs:179` |
| 4 | `SlotKey { zone, pool, ordinal }`; ordinals are zone-wide since A2 | `[V]` `layout.rs` |
| 5 | Province `y` is slot ordinal × stride; ghost slots leave their stride empty | `[A]` A2 report §1 |
| 6 | The map is roughly two-thirds ocean — stride is the largest extent class while most provinces are 3–5 rows | `[A]` A2 report §6 |
| 7 | `Camera::to_screen` / `to_land` / `cell_at` are the projection, and `cell_at` is `to_screen`'s inverse | `[A]` relief work |
| 8 | Overlay and map-style settings persist in prefs; `--overlay` / `--map-style` are the flag-plus-prefs precedent | `[A]` A2, A5 |

**Claim 6 is the design problem** — see §2.1. Verify the current ratio before choosing a grid.

---

## 1. Why A6 is not the tail

Plan §3.3 made the graticule the **invariant against which change is read**:

> When the geography itself changes, comparison requires an invariant. Fix the frame to one durable entity, declare that you have done so, and let everything else read as motion relative to it.

That is Wegener's move — his 1915 drift plates state in the caption that the graticule is arbitrary, fixed to Africa's present position, and that declaration is what makes three epochs comparable.

Two things follow:

- **A6 completes A.** Stability that cannot be *named* cannot be exploited — "the node in C4" is what makes it usable in a handover, a ticket, or a screenshot annotation.
- **A6 gates plan §7.** Small multiples, change-since overlays and fault-line marking all require frames that can be laid against each other. Without a declared invariant they cannot be.

---

## 2. The graticule

### 2.1 The grid must not be uniform over an empty map

Claim 6 is the constraint. A uniform lattice over a world that is two-thirds ocean spends most of its labels on water, and a reference like "F7" would most often name nothing.

Three options, in preference order:

1. **Anchor the grid to the durable structure that already exists.** Columns are zones (claim 2 — `zone_ordinal` is already durable and reserved); rows are slot ordinals (claim 4 — zone-wide since A2). A reference is then `⟨zone-letter⟩⟨ordinal⟩`, and it names a **slot**, which is the thing that persists. This is the option that composes with everything A1–A5 built.
2. **A uniform cell grid** with plate labels. Familiar, and matches the reference specimens — but it labels ocean, and per claim 5 the ordinal-strided rows mean grid lines would not align with province boundaries.
3. **Label provinces directly**, no grid. Simplest, but gives no way to name empty ground, and empty ground is exactly what a ghost or a reserved stride is.

**Recommend (1).** It is the option where the coordinate means something in the model rather than being an artifact of the projection, and it survives everything the workstream made durable.

The consequence worth stating: a reference names a **slot, not a screen position**. That is the correct behaviour here — it is stable across restarts and refreshes, which a screen position is not.

### 2.2 It must recede

Both reference specimens draw their tessellation faintly — visible, never dominant. A graticule that competes with terrain has failed at being a reference.

It is **scenery, not instrumentation**: it encodes no cluster state, so it may vary by map style, and it does **not** route through the `cb_*` funnel. Distinguish this deliberately from A5's fresh ground, which is the opposite case.

Consider whether it should be toggleable. `--overlay` and `--map-style` are the precedent (claim 8) if so.

### 2.3 Zone letters need a durable assignment

Per claim 3, `zone_ordinals` retains departed zones so their ground stays reserved. **The letter must follow the ordinal, not the current zone list** — otherwise a zone vanishing re-letters its neighbours, which is precisely the instability A2 removed one level up.

A departed zone keeps its letter. That is the same discipline as a ghost keeping its ordinal.

---

## 3. The declared frame

§3.3's requirement is not the grid — it is the **declaration**.

The legend must state what the frame is anchored to, in the same voice the codebase already uses for inferred facts (`metric_source`, `CostBasis`, `PoolSource`, substrate's prevalence heuristic):

- Columns are zones, in the order first observed — **not** alphabetical, and not meaningful as adjacency
- Rows are slot ordinals within a zone
- **Position asserts nothing about the cluster.** Two adjacent provinces are not related; zone is the only real grouping

That last line matters most. Plan §3.3's honesty constraint says node adjacency means nothing real, and a grid makes positions look meaningful. Declaring the frame is what stops the map lying by implication.

Where the declaration lives: the Almanac is the established home for "how to read the map" (A5 documented ghost and fresh ground there). A compact on-map note may also be warranted, given a screenshot travels without the Almanac.

---

## 4. The gate

**One person names a position from the map; another finds it without further explanation.**

This is a usability gate, not a numeric one, and it is the first in the workstream that cannot be automated. Run it as stated: produce a reference from a capture, hand it to someone else with the app open, and see whether they land on the same slot.

### 4.1 The discrimination check

Six instruments in this workstream have emitted a plausible result for a reason unrelated to what they measured, and every phase since has been required to break the mechanism and confirm the measure moves.

Here: **hand over a reference with the graticule disabled.** If the second person finds the slot anyway — from the node name, from the shape of the map — the gate is measuring their familiarity with the fleet, not the graticule.

### 4.2 Failure criteria, stated before running

- The reference is ambiguous — two slots could match
- The reference is unreadable at the zoom where a fleet is viewed
- The grid competes with terrain rather than receding
- A reference names ocean, or names nothing

---

## 5. What A6 does not do

- **No small multiples, no change-since overlay, no fault-line marking.** Plan §7. A6 is their prerequisite, not their start.
- **No `region ← pool ∩ zone` grouping.** Still unclaimed; A2 gave up contiguity when ordinals went zone-wide, and plan §3.4.4 chose colour and label over it.
- **No change to projection or hit-testing.** The graticule is drawn over the existing coordinate space.

---

## 6. Tests

- [ ] A zone's letter is stable across a rolling refresh
- [ ] A departed zone keeps its letter; its neighbours do not re-letter — **the §2.3 requirement**
- [ ] A reference round-trips: slot → reference → slot
- [ ] A reference is unambiguous across the whole fleet
- [ ] The graticule reads in every map style, and toggles off cleanly if toggleable

**Mutation floor, exercised:** derive zone letters from the current zone list rather than the durable ordinal, and confirm the departed-zone test fails.

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims here were inherited rather than verified against the case at hand — and for each, **does the state it describes actually occur?**

Question 2 is live: a zone with **no ordinal** (never assigned, or a layout that failed to load) must render without a letter rather than fabricating `A`. A fabricated letter collides with a real zone's, which is the worst available failure.

Question 5's sharpened form comes from A5: verifying a type's shape is not verifying its inhabited states. Claim 3 says `zone_ordinals` retains departed zones — confirm a departed zone actually occurs in a run before designing §2.3 around it.

---

## 8. Acceptance

- [ ] References anchor to durable structure (zone ordinal + slot ordinal), not screen position
- [ ] A departed zone keeps its letter
- [ ] The graticule recedes; treated as scenery, not routed through `cb_*`
- [ ] The frame is declared, including that position asserts nothing about the cluster
- [ ] Gate run with a second person **and** its discrimination check
- [ ] Failure criteria stated before the run
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 9. Estimate

**Half a day to a day.** The drawing is small. The §2.1 choice and the §3 wording are the work — and the wording is load-bearing, because a grid that implies adjacency is meaningful would make the map assert something false.

This closes Workstream A.
