# Item A — extent headroom

**Guidance:** `docs/kubernation-item-a-extent-headroom-guidance.md` (rev 2)
**Version:** v1.20.0 · **Date:** 2026-08-07

Closes the last open item from the consolidation round, which I had stopped at
§0. **Gate passed** — a synthetic boundary node classifies correctly, both
mutation directions fail as they should, and no fleet capture was attempted.

The round also produced something the record did not expect: **class 9 rendered
for the first time, and the Relief occlusion hazard was demonstrated to be real
and already defused.** §5.

453 core + 113 GUI tests; gui-smoke unchanged.

---

## 1. §0 — claims verified

| # | Claim | Verdict |
|---|---|---|
| 1 | bounds + class formula | **TRUE** — `world.rs:480,484,501` (guidance said 502/509; drift from v1.18.0's deletions) |
| 2 | the doc comment states the intent | **TRUE**, verbatim |
| 3 | classes `[3,5,7,9]`; both fallback rungs return `EXTENT_CLASSES[1]` | **TRUE** |
| 4 | three sites carry `Capacity` for a field none reads | **TRUE** |
| 5 | no capacity read path anywhere | **TRUE** — `status.capacity` appears nowhere in core |
| 5a | `extent_line(Capacity)` returns `None` | **TRUE** |
| 6 | kind reports `capacity == allocatable == 15.653 GiB` | **TRUE** (measured by me in the consolidation round) |
| 7 | kwok's round numbers make current and `[30,120,480]` identical | **TRUE** |
| 8 | distribution 30 / 53 / 16 | **CORRECTED → 30 / 54 / 16** |
| 9 | `SLOT_STRIDE` is the largest class, so no province moves | **TRUE** |

### 1.1 Claim 8 is mine, and it was wrong

Re-derived from a fresh `--dump-positions` rather than re-quoted:

```
extent class (h) x count: {3: 30, 5: 54, 7: 16}
by extent_source:         {Allocatable: 99, InstanceType: 1}
```

**30 + 53 + 16 = 99.** My consolidation report's figures did not sum to the
fleet, and neither the report nor the guidance that inherited them noticed. The
missing node is the one with no allocatable memory: it takes the `InstanceType`
fallback, which also returns `EXTENT_CLASSES[1]` — class 5.

The substance is unaffected and claim 8's purpose stands: **class 5 is the
ordinary case, not class 3**, so the retired symptom ("the smallest extent is
the ordinary case … mostly thin ribbons") stays retired and is not restated in
the changelog. Second small arithmetic slip of mine caught by re-deriving in as
many sessions; both were narrated numbers rather than emitted ones.

### 1.2 §3's precondition

`terrain_order` is in the tree and owns the paint sequence (`draw.rs:1260`), so
the ordering held and this could land alone — which is exactly what the
consolidation round split it for.

---

## 2. The change

`EXTENT_HEADROOM = 0.08`, and the **reported value is scaled, not the bounds
shifted**, so the bounds stay readable as machine sizes and the correction sits
where the two quantities differ, with a name on it.

```rust
let class = EXTENT_BOUNDS_GIB
    .iter()
    .filter(|b| gib * (1.0 + EXTENT_HEADROOM) >= **b)
    .count();
```

The constant's doc separates what is measured from what is not: the firmware
term is measured (kind, 2.2% short of nominal), the managed-cloud kubelet
reservation is **not measured here** and is stated as such. 8% is deliberately at
the small end — a genuine 24 GiB node would need 33% to be wrongly promoted, so
there is room but not unlimited room.

---

## 3. §2.1 — the rename, taken

`ExtentInput::Capacity` and `ExtentSource::Capacity` became **`Allocatable`**, at
all six enumerated sites plus the test name.

The argument that decided it is not tidiness: **this exact drift caused the
guidance's own first revision to recommend "compare against capacity" as the
fix**, on the strength of the variant name. A name asserting a quantity the value
is not, in the same class as `first_trouble` comparing an onset against a `when`.
Both enum variants now carry a doc comment saying what field they read and why
the name moved.

**§2.1's checklist item earned its keep.** `hack/churn/positions-selftest.py:39`
matched the literal string `"Capacity"` — `--dump-positions` emits the variant via
`{:?}`, so the rename changes the emitted JSON value. Updated in the same commit;
its self-tests pass. Nothing persisted is affected: the layout store carries
`zone/pool/ordinal/occupant/occupied_at/vacated_at/last_occupant` and contains no
`ExtentSource` at all (checked, not assumed).

---

## 4. §4 — the gate

**No fleet capture was attempted, and the reason is recorded**: kwok reports
exact round numbers, so every candidate bound set yields identical classes on all
100 nodes and a before/after would show zero changed pixels. kind is no better —
15.653 GiB is nominally 16 and correctly class 3 under every candidate. The check
was arithmetic, done before running anything.

`a_node_at_a_nominal_boundary_gets_the_class_its_size_implies` covers all of
§4.2: the boundary cases (30.9 → class 5, 123 → 7, 493 → 9), the promotion guards
(24 → 3, 96 → 5, 384 → 7), the exactly-nominal values unchanged, and totality at
`0`, `f64::MAX` and a negative.

**Mutation floor, both directions, as §4.2 requires:**

| Headroom | Result |
|---|---|
| `0.0` | fails "a nominal 32 GiB node" — the defect returns |
| `0.35` | fails "a genuine 24 GiB node" — over-promotion |

That is the point of the constant: it sits between two failure modes, and a test
suite that only caught one direction would license nudging it upward forever.

### 4.1 A pre-existing test failed, and it was right to

`extent_is_allocatable_derived_quantised_and_marked` asserted that 33 GiB and
120 GiB fall in the same class. They no longer do: 120 GiB is now promoted to the
128 class **on purpose**, because a nominal 128 GiB machine reports about that
after firmware and a reservation. The test's *property* ("small variation inside
a class does not resize anything") survives; its *example* had encoded the
boundary defect. Changed to 33/96 with a comment saying why.

This is standing question 4 landing on a test rather than on production code —
worth noting, because a test is the consumer most likely to be "fixed" by
changing the assertion without asking which side was wrong.

### 4.2 §6's city question, answered with a test

`a_class_change_keeps_a_city_on_its_own_province` builds the same workload on a
node either side of the boundary and asserts: the province gains height, the
province itself does not move (the stride claim, exercised from the consumer
side), and the city stays within its own province's rows and columns. A class
change *does* move a city within its province — `city_dy` is modulo `rows` — and
that is the design; the test pins that it is the only thing it does.

---

## 5. §5 — class 9 rendered, and the hazard was demonstrated

`hack/churn/bigmem.sh` adds two nominal-512 GiB nodes. Deliberately **not** part
of `up.sh`: the 100-node fleet is the reference state every recorded measurement
in `docs/reports/` is judged against, and growing it would silently invalidate
them. `MODE=down` restores, and the fleet is back at 100 nodes.

Two nodes, in a **new pool**, in **one zone** — because a node joining an
existing `(zone, pool)` would reclaim a ghost slot instead of appending, and the
occlusion case needs two class-9 provinces at **consecutive ordinals**:

```
churn-hpc-000  z-d ordinal=15  y=136 h=9  rows 136..144   Allocatable
churn-hpc-001  z-d ordinal=16  y=145 h=9  rows 145..153   Allocatable
==> gap between the bands: 0 rows  (TOUCHING)
```

Class 9 has never rendered before — `{3: 30, 5: 54, 7: 16, 9: 2}` — so this is a
new capability, not a before/after.

### 5.1 The discrimination check that was impossible until now

The consolidation round could only argue the Relief hazard was *reachable in
principle*. With class 9 on screen it is testable, so I ran it: rebuilt with
`terrain_order`'s sort removed, captured the same frame, and compared with the
committed `compare.py` decoder rather than a fourth ad-hoc comparator.

```
2760x1720;  differing pixels: 924 / 4747200 = 0.0195%
```

Non-zero, and confined to the seam between the two touching bands. So:

- the hazard was **real**, not hypothetical;
- **`terrain_order` defuses it** — v1.18.0's item B, landed while unobservable,
  on the argument that it would become observable exactly here;
- and the consolidation guidance's §1 causal link — *"fixing the calibration
  activates the occlusion risk"* — is confirmed, along with its instruction to
  land the sort first.

A fix shipped against a hazard nobody could see, and the hazard has now been
seen. That is the strongest available argument for §3's "do not skip it because
it is unobservable".

---

## 6. §6 — standing questions

**1. Summing before comparing?** Not present in the change. It *was* present in
claim 8's error: counts were summed into a narrated distribution that did not add
up to the fleet, and nothing compared the total against 100. §1.1.

**2. Unknown, or fabricated?** Untouched — the fallback rungs still return a
declared middle class and are still marked, and `extent_line` still speaks them
(v1.18.0). Nothing here makes an unmeasurable node look measured.

**3. Two sections constraining one behaviour, and a fixture where they diverge?**
Yes: §2's headroom and §4.2's promotion guards pull in opposite directions on the
same constant. The fixture where they diverge is the mutation floor, which is why
it has to be run in both directions rather than once.

**4. Consumers depending on the old meaning?** Three, all found:
`Province.h` → `rows` → city placement (tested, §4.2); the pre-existing 33/120
assertion (§4.1); and the dump's emitted `"Capacity"` string, matched by a
committed instrument (§3).

**5. Inherited claims — does the state each describes occur?** Claims 6–8 were
inherited from my own consolidation report. Claim 8 is wrong (§1.1). The question
has now caught an error of mine in three consecutive sessions, every one of them
a number I narrated rather than one an instrument emitted.

**6. One side of a comparison moved — does the other still mean the same?** This
*is* the defect: bounds in nominal sizes, values reported. After the change the
left side is *reported memory scaled to an estimated nominal* and the right side
is *nominal* — approximately the same thing, where before they were exactly
different. **The mismatch is reduced, not eliminated**, and the constant's doc
says so rather than implying it is gone. The residual is the unmeasured
managed-cloud reservation term.

**7. Container adjacency read as world adjacency?** Not present — this change
touches no ordered collection. `terrain_order` already owns the one that
mattered, and §5.1 is the evidence that it does.

---

## 7. Acceptance

- [x] `EXTENT_HEADROOM` named; firmware measured, reservation stated as unmeasured
- [x] The reported value is scaled; the bounds stay nominal
- [x] §2.1 decided — renamed to `Allocatable` at all six sites including the test name
- [x] The instrument matching the emitted `"Capacity"` updated in the same commit
- [x] §3's ordering confirmed — `terrain_order` was already in the tree
- [x] Boundary and promotion-guard tests; mutation floor in **both** directions
- [x] No fleet capture attempted; §4.1's reason recorded
- [x] A class-9 node added (as a reversible scenario) and captured under Relief
- [x] The city-placement question answered with a test
- [x] The changelog states the real symptom, not the retired one
- [x] Standing questions answered; claims tagged and one corrected
- [x] `cargo test` green — 453 core + 113 GUI

**Deviation, stated:** §5 says "add a node to the churn fixture". I added a
separate reversible script instead, because the fixture's size is load-bearing
for every recorded measurement. The capability is the same; the reference state
is preserved.

**Not done:** §4.3's managed-cloud check — no GKE/EKS/AKS node is available here.
The reservation term therefore remains unmeasured, which is stated on the
constant rather than papered over. If 8% turns out to be too small on a real
cloud node, the doc says to record the measured figure rather than nudge the
constant until it works.
