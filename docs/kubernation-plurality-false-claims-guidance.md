# KuberNation — Plurality Siting: The Two False Claims

**Implementation guidance**
**Goal:** stop the two surfaces that let a user conclude something false about where a workload runs.
**Gate:** neither surface asserts a host node for a spread workload.

**Explicitly not a map change.** The pre-check found the map is honest and the dishonesty is elsewhere. Whether this is *sufficient* is §7's question, answered after.

Follows: `docs/reports/plurality-siting-precheck.md`

---

## 0. What the pre-check established

**The problem is arithmetic, not a fixture.** A workload lands on `k ≈ min(replicas, nodes)` nodes, so the plurality share is about `1/k`. Any cluster with more nodes than a workload's replica count produces this — the ordinary shape of a production fleet. On the churn fleet, 7 of 7 eligible workloads are below 20%.

**But most of the map is honest.** Province surfaces report the physical census from `tile.pods`. Blast walks `workloads_on_node` and resolves each affected workload to its own city. The city drill-down asserts nothing. `city_home` and `workloads_on_node` disagree by construction and nothing depends on them agreeing.

**Two surfaces are not honest**, and both are cheap to fix:

| | |
|---|---|
| **§3.1** — the Legend says a city is *"sited on the province holding **most** of its pods"* | False whenever no node holds a majority. The World page two pages later says *"plurality"*, correctly |
| **§3.2** — selecting a city appends the **province's** attributes: grid reference, pool, extent, freshness, strain, upkeep, substrate gaps | For `churn/api` that is the strain and cost of a node running 2 of its 120 pods, presented as the workload's context |

**And the cheap escape hatch is measured shut.** A node column in CITIZENS does not fit — the row is already over budget and only `.clamp(8, 22)`'s floor keeps the pod name legible.

---

## 1. Verify before building

All `[A]`, from the pre-check (2026-08-19). It re-read all eight of its own inherited claims at source, so these are second-hand but recently checked.

| # | Claim | Source |
|---|---|---|
| 1 | `almanac.rs:393` (Legend, City) says *"most"*; `almanac.rs:502` (World) says *"plurality"* | pre-check §3.1 |
| 2 | The code implements plurality: `max_by_key(|(node, n)| (**n, u64::MAX - fnv1a64(node)))` | §1 |
| 3 | Selecting a city appends the host province's attributes, with a comment saying so: *"The city sits on the tinted province — show its host node's strain / upkeep too"* | §3.2 |
| 4 | Province SELECTION is `"{health} . {N} pods"` from `tile.pods` — the physical census, **honest** | §3.3 |
| 5 | Blast resolves each affected workload to its own city via `affected_cell` — **honest** | §3.3 |
| 6 | `city.rs` renders no node at all | §3.3 |
| 7 | The docked column's budget is 32 chars; today's tail is 28; a node name adds 14–18 over | §4 |
| 8 | Only Deployment and StatefulSet are sited as cities, and they behave identically here — no per-kind split | §2.2 |

**Claim 3 is the phase.** Claim 8 means there is no kind-specific handling to write.

---

## 2. Item 1 — the Legend sentence

One word. `almanac.rs:393` should say what the code does and what the World page already says.

But **do not simply change "most" to "plurality"** without deciding what else the entry should carry. The pre-check's §1 found a second, related silence:

> `city_home`'s tie-break is deterministic and stable by design, and **no user-facing surface says the chosen node is otherwise meaningless.**

So the Legend entry is the natural place to state the frame, in the shape A6's declared graticule already uses — *this position is a stable convention, not a claim*. Whether to say that here or leave it to item 2's wording is a judgement; make it deliberately.

**Check the two pages agree afterwards.** They disagreed for long enough that nothing was comparing them.

---

## 3. Item 2 — the borrowed attributes

The harder half, and the one that matters.

### 3.1 The error is structural, not wording

The pre-check is precise about this and it should drive the fix:

> The panel is correctly describing the province; the error is that the *city* is presented as having a host node at all.

So the fix is not to reword the lines. It is to decide **which of a province's attributes are true of a workload sited there** — and the answer for a spread workload is *none of them*.

| Attribute | True of the workload? |
|---|---|
| grid reference, pool, extent, freshness | No — these describe the node |
| strain, upkeep, substrate gaps | No — and actively misleading; they are the node's, for a node running a fraction of the pods |

### 3.2 Three shapes

| | |
|---|---|
| **Drop them** | The city's SELECTION says only what is true of the workload. Simplest; loses context that is genuinely useful when a workload *is* concentrated |
| **Qualify them** | Keep the lines, but say whose they are — *"host province: …"* — so the attribution is explicit |
| **Condition on spread** | Show them when the plurality is high, drop or qualify them when it is not |

**Prefer qualify.** Dropping loses real information for the concentrated case, and conditioning introduces a threshold that would need defending — the pre-check's §0 threshold was chosen for measurement, not for behaviour, and reusing it here would give it a weight it has not earned.

Qualifying is also the shape this codebase already uses when a value's provenance matters: `metric_source`, `CostBasis`, `PoolSource`, `ExtentSource`, `pool_line`'s refusal to name the unpooled sentinel.

### 3.3 And say what the city actually is

If the province's attributes are qualified as the province's, the workload's own footprint is still unsaid — 65 nodes, none of which the box mentions.

`CityPod.node` is on every pod (D3/D4 pre-check §3), so a **count** is available without the column that does not fit: *"120 pods across 65 nodes"*. One line, no per-pod detail, and it is the fact that makes the qualified province line interpretable.

**Check it against `row_char_budget`** before committing — §4's lesson is that this panel has no spare room and estimates about it have been wrong.

---

## 4. What this does not do

- **No map change.** No new mark, no siting change, no road treatment
- **No node column in CITIZENS** — measured shut (claim 7). A narrower variant is uncosted and belongs to a later scoping if wanted
- **No threshold-conditioned behaviour** (§3.2)
- **No change to province surfaces or blast** — both honest (claims 4, 5)

---

## 5. Tests

- [ ] The Legend and World pages make the same claim about siting — the anti-drift test, and the reason they diverged is that nothing compared them
- [ ] A city's SELECTION attributes are attributed to the province, not to the workload
- [ ] The spread line (§3.3) reports pods and distinct nodes, and is correct for a single-node workload too
- [ ] Panel rows still fit — `row_char_budget` asserted, not eyeballed

**Mutation floor, asserted applied** — six false survivals this session from `cargo fmt` reflowing targets:

- Make the Legend say "most" again → the anti-drift test fails
- Drop the province attribution → the SELECTION test fails
- Make the spread line count pods instead of nodes → its test fails

---

## 6. The gate

**Select `churn/api` — 120 pods across 65 nodes, plurality node holding 2 — and read the SELECTION box.**

Nothing in it should be readable as *"this workload runs on this node."*

### 6.1 The discrimination check

Select a **concentrated** workload on kind, where the plurality node genuinely holds 100%. The box should still attribute the province's attributes to the province — the wording must not be conditional on spread (§3.2), and a fix that only reads correctly on the churn fleet has moved the problem rather than fixed it.

### 6.2 Failure criteria, stated in advance

- The box implies a host node for a spread workload
- The province's attributes are dropped entirely, losing the concentrated case
- The spread line pushes a row over budget
- The two almanac pages still disagree

---

## 7. What this leaves open, and the question to answer after

The pre-check's §5 recorded four map-shaped candidates and chose none. This phase does not touch them.

**After it lands, answer:** *with both false claims corrected, does anything still let a user conclude something false about where a workload runs?*

If no — the pre-check's §5 last row becomes true, the city is a label rather than a location claim, and the item closes with a recorded reason.

If yes — that residual is what a map change should be scoped against, and it will be a much smaller and more specific target than "cities are imprecise."

**Do not decide this in advance.** It is the same shape as every pre-check in this project: the cheap fix first, then measure what is left.

---

## 8. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 3 is item 1.** Two almanac pages constrained the same behaviour and one was wrong. The fixture where they diverge is any spread workload — which is most of them at fleet scale.

**Question 2:** a workload with zero placed pods has no spread to report. §3.3's line must say so rather than printing *"0 pods across 0 nodes"*, which reads as a measurement.

---

## 9. Acceptance

- [ ] Legend corrected, and the two pages verified to agree
- [ ] The arbitrariness of the tie-break stated somewhere the operator can see, or a recorded decision not to (§2)
- [ ] Province attributes attributed to the province, unconditionally (§3.2)
- [ ] Spread line added and costed against `row_char_budget` (§3.3)
- [ ] Gate run on `churn/api`, with the concentrated-workload discrimination check
- [ ] Failure criteria stated before the run
- [ ] Mutations asserted applied
- [ ] §7's question posed, not answered
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 10. Estimate

**Two to three hours.** One sentence, one panel behaviour, one new line. §3.3's budget check is the only place it can go over.
