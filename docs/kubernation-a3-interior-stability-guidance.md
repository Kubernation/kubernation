# KuberNation — A3: Interior Stability

**Implementation guidance**
**Goal:** a city does not move because a sibling appeared or departed.
**Gate:** on a province with several cities, add and remove a workload sorting ahead of them. **No incumbent moves.**

Scoped against the mechanism A3-pre measured, not against decomposition §4's original description. See §1.

---

## 0. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | The row seed is positional: `city_cell(cx, y, rows, city_dx(&c.r.name), (i as u16) % rows, &taken)` where `i` is the enumerate index | `state/world.rs` ~641 |
| 2 | The column seed is already name-derived: `city_dx(name) = CITY_COL0 + fnv1a64(name) % CITY_COLS` | `world.rs` ~392 |
| 3 | `city_cell` probes rows of the preferred column first, then moves east; two spans (`CITY_COLS`, then `CITY_COLS_WIDE`) | `world.rs` ~417 |
| 4 | Past capacity `city_cell` returns the preferred cell and **collides** — documented, not silent | `world.rs` ~440 |
| 5 | `cities.sort_by(|a, b| a.r.cmp(&b.r))` runs before the placement loop | `world.rs` ~623 |
| 6 | `rows = h.saturating_sub(1).max(1)`, and `h` comes from `province_extent(&tile.extent)` | `world.rs` ~638 |
| 7 | Coast markers moor on `c.y` and take the first free column in the ocean strip | `world.rs` ~700 |

**Claim 1 is the entire defect.** Everything else in this document follows from it.

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 8 | A city legitimately moves when its own pod plurality moves (`city_home`) | That is FOLLOWED, correct, and must not be "fixed" |
| 9 | A city's province **is** its plurality node by construction, so MOVED-ACROSS is unrepresentable | A3-pre §2. Do not reintroduce the class |
| 10 | `rows` derives from extent, so a province on the declared-default extent has a fallback row count | A3-pre §7. Affects test fixture choice — see §5 |

---

## 1. The measured mechanism

A3-pre established this precisely, and it is smaller than "city slots":

> `row0 = i % rows`, where `i` is the city's index in the `WorkloadRef`-sorted sibling list.

Consequences, all measured:

| Event | Effect | Why |
|---|---|---|
| Insert a workload sorting **ahead** | **every** incumbent moves one row | all indices shift |
| Delete a workload sorting **ahead** | every incumbent moves back | same, inverted |
| Insert/delete sorting **after** | nothing moves | earlier indices untouched |
| Scale up or down | nothing moves | the set is unchanged |
| Node churn | nothing moves | siblings unchanged |

The column is already stable. **Only the row seed is positional.**

`city_cell`'s own doc comment anticipates this: *"Real city slots are A3's job — this only removes the collision."*

---

## 2. The fix

Make the row seed name-derived, symmetric with the column:

```rust
let cell = city_cell(cx, y, rows, city_dx(&c.r.name), city_dy(&c.r, rows), &taken);
```

```rust
/// City row inside a province, from a stable hash of the workload ref rather
/// than its index among siblings. The index made a city's row depend on how
/// many siblings sorted ahead of it, so ADDING an unrelated workload moved
/// every incumbent on the province (A3-pre measured 3 of 3, each by one row).
///
/// Hashed on the full ref, not the name: two workloads of different kinds or
/// namespaces sharing a name must not seed identically.
fn city_dy(r: &WorkloadRef, rows: u16) -> u16 {
    (fnv1a64(&r.to_string()) % rows.max(1) as u64) as u16
}
```

Use a ref rendering that already exists and is stable — check what `WorkloadRef` provides rather than inventing a format string. If `city_dx` hashes only the name, consider whether it should hash the ref too; **note it, but do not change it in this phase** unless a test shows a real collision, since changing the column moves every city once.

The probe loop, the `taken` set, the two spans and the overflow fallback all keep working unchanged.

### 2.1 The residual, stated honestly

Hashing removes the *index* dependency. It does not remove **collision** dependency.

Two cities hashing to the same cell still resolve by probe order, so a city can still move when a colliding sibling arrives. That is strictly better — bounded to actual collisions rather than every insertion ahead — but it is not zero.

**Decide explicitly, and record the decision:**

- **(a) Accept it.** With `CITY_COLS ≈ PATCH_W - 16` columns × `rows` rows, collisions are rare on realistic provinces. Cheap, and the failure is bounded.
- **(b) Reserve slots.** Persist a city's cell in `Layout` the way node slots are, with the same carry/reuse/ghost discipline. Fully stable, and it is the A1 pattern one level down — but it puts workload identity into the layout store, which A4 then has to persist and garbage-collect.

**Recommend (a) for A3**, with a test measuring collision frequency on a realistically packed province. If it turns out common, (b) is a follow-on with its own gate — and A1's engine is the template.

Do not build (b) speculatively. The measurement exists to size the problem before solving it.

---

## 3. What A3 does not do

- **No change to `city_home`.** Plurality siting is correct (claim 8).
- **No change to extent or `rows`.** That is A2's, and settled.
- **No slot reservation** unless §2.1 measurement demands it.
- **No coast-marker rework** beyond what falls out — but see §4.

---

## 4. The consumer question

Standing question 4: **what depends on the old meaning of `c.y`?**

A2's report found four consumers encoding the old meaning of province `y`, and none of the defects were in the new code's own logic. City `y` has at least one known dependant:

**Coast markers moor on `c.y`** and take the first free column in the ocean strip, dropping a marker when the row runs out. Changing which rows cities occupy changes which rows have markers, and how many share one. A hash seed can cluster cities onto fewer distinct rows than a round-robin index did — which would drop more markers.

- [ ] Check whether marker drops increase after the change, on a province with several exposed cities
- [ ] Audit anything else reading `City.y` — the keep-out region, label placement, hit-testing, the minimap

That audit is the phase's real risk, not the two-line fix.

---

## 5. Tests

**The mechanism:**
- [ ] Insert a workload sorting ahead of incumbents → **no incumbent's cell changes**. This is the gate condition, as a unit test.
- [ ] Delete a workload sorting ahead → no incumbent moves
- [ ] Insert sorting after → still nothing (regression guard; it already passes)
- [ ] Scale up and down → nothing (same)
- [ ] The same workload set in a different insertion order → identical cells

**Determinism:**
- [ ] Placement is a pure function of the city set and `rows` — no dependence on iteration or arrival order
- [ ] Two workloads with the same name in different namespaces get different seeds

**Boundaries:**
- [ ] `rows == 1` — every city hashes to row 0 and resolves by column probing
- [ ] A province packed past interior capacity still hits the documented fallback, not a panic
- [ ] Collision frequency on a realistically packed province — the §2.1 measurement

**Fixture note (claim 10):** do **not** use the allocatable-less node as the test province. Its `rows` comes from the declared default extent rather than a measurement, which is an avoidable confound in a row-based test. A3-pre's scenario used it; change that too.

**Mutation floor:** revert `city_dy` to `i % rows` and confirm the first test fails.

---

## 6. Fixture change

A3-pre found the standing fixture gives every province exactly one city, so the sibling-order effect is unreachable without a scenario constructing it. Every province is single-city, which under-represents real clusters where a node routinely hosts several workloads.

**Pin a few workloads together in `workloads.sh`**, so multi-city provinces exist by default — for A3's own tests, for the dev loop, and for every later scenario. Small change, and it removes a class of "the fixture cannot exercise its own gate" findings.

---

## 7. The gate

Run scenario 7 against the fix, on a **multi-city province whose extent is measured**, not defaulted.

Read the result **per-province**, per A3-pre: 100% of incumbents on an affected province moved before. The target is 0.

The fleet-wide rate (27.3%) is a packing artefact that would improve by adding nodes without anything having been fixed. Do not report it as the headline.

Record the post-fix number against A3-pre's baseline in the same units, from the same scenario.

---

## 8. Standing questions — written answers required

1. **Where does a summing step precede a comparing step?**
2. **Does every reducer over a possibly-empty input express unknown, or fabricate?**
3. **Where do two sections constrain the same behaviour — and is there a fixture where they diverge?**
4. **What existing consumers depend on the old meaning of a value this change redefines?**

Question 4 is the live one this phase — §4 exists because of it. Question 2 applies to `rows.max(1)`: a zero-row province must not produce a modulo by zero or a fabricated row 0 that collides with a real cell.

---

## 9. Acceptance

- [ ] Row seed is name-derived; no index reaches `city_cell`
- [ ] `city_dy` hashes the full ref, not the bare name
- [ ] §2.1 decided and recorded, with the collision measurement behind it
- [ ] Consumers of `City.y` audited (§4), coast-marker drop rate checked
- [ ] Fixture gives multi-city provinces by default
- [ ] Gate run and reported per-province against A3-pre's baseline
- [ ] Standing questions answered in writing
- [ ] `cargo nextest` green

---

## 10. Estimate

**Half a day to a day.** The fix is two lines. The consumer audit (§4) and the collision measurement (§2.1) are the work — which is the shape every phase in this series has taken, and the shape the estimates keep getting wrong in the same direction.
