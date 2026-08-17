# KuberNation — T2-pre: Do Failures Cluster Spatially?

**Measurement guidance**
**Goal:** establish which dimension failures cluster in, before T2 is scoped against an assumption.
**No product change.** The output is a number, an instrument, and possibly a smaller T2.

---

## 0. Why this comes first

T2's claim is *"failures cluster spatially, and only the map shows that."* Nothing in the record tests it.

What T1 measured was **node-replacement** change: 18 successions, pool-shaped at `P ≤ 0.005` against a pool-blind control. Strong, and about a different axis.

T1 also found the map **losing** to the Annals on one point: node names carry the pool, so a reader scanning `churn-sys-g1-0NN` ten times learns *"the sys pool was replaced"* directly. For failures the list has no such crutch — which is T2's strongest remaining argument, and it is entirely unmeasured.

**The answer decides T2's size:**

| If failures cluster by | Then |
|---|---|
| node, pool, or zone | The map shows a shape a list conceals. T2 proceeds as planned |
| **workload** | The clustering is in a dimension the map's zone-organised geography scatters. T2 shrinks, or dies |

The second is plausible and cheap to check. Every incident the T-fix rounds induced was a bad rollout, whose failures follow the Deployment's pods wherever they land — no node property involved.

This is the move that shrank A3 from "city slots" to two lines and reversed T2's ordering after T-pre. **Measure before scoping.**

---

## 1. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | `PodGlyph` carries `namespace`, `name`, `state`, `owner: Option<WorkloadRef>` | `[V]` `model.rs:268` |
| 2 | `NodeTile` carries `name`, `zone`, `pool` and `pool_source` | `[V]` `model.rs:322` |
| 3 | `workloads_on_node` is the shared authority for node→workload, used by blast and the Oracle so they cannot disagree | `[V]` `blast.rs:86` |
| 4 | `--dump-positions` records per-province node, zone, pool, ordinal, extent | `[A]` A3-pre, A6 |
| 5 | `--postmortem` renders `build_timeline` + `row_decisions` as text | `[A]` T-pre |
| 6 | `first_trouble` anchors on **onset**, windowed, so chronic failures self-exclude | `[A]` T-fix |
| 7 | kwok emits almost no events — event-derived behaviour must be measured on kind | `[A]` T0 §2.4, T-pre |
| 8 | The pool-blind control in T1 §4 shuffles the same `k` across the same zone's live ordinals | `[A]` T1 re-derivation |

### The trap, from `PodGlyph`'s own doc

> `PodState::Failing` **cannot stand in for** `pod_terminal` — it covers both a terminal `Failed` pod and a live `CrashLoopBackOff` one.

A measurement counting "failing pods" that does not separate these will mix a pod that died once and stopped with one that is failing right now, repeatedly. Those cluster differently: the first follows whatever ran there, the second follows a live cause.

**Decide which you are counting and say so.** T2 is about live trouble, so `pod_terminal` should filter — but the count of terminal pods is itself worth reporting, because a large one means the fleet carries history that a naive measurement would have swept in.

---

## 2. The measurement

### 2.1 Induce distinct failure shapes

Four, chosen because their *expected* clustering dimension differs. Run each separately, on a quiesced cluster.

| Shape | How | Expected to cluster by |
|---|---|---|
| **Bad rollout** | Deployment → invalid image (T-fix-2's `INVALID_IMAGE_NAME` makes it deterministic) | workload |
| **Node pressure** | Fill a node's memory until eviction | node |
| **Storage failure** | A PVC with an unsatisfiable StorageClass | workload, possibly zone if the class is zonal |
| **Crash-looper** | A container that exits nonzero on a loop | workload |

**State the expectation before running each.** If three of four are workload-shaped by construction, that is itself the finding — and it means the fixture, not the world, is deciding the answer.

**The gap that matters:** if you can only produce workload-shaped failures on kind, say so plainly. *"We could not construct a node-shaped failure"* is a real result about what this project can measure, and it is the honest input to T2's scoping.

### 2.2 What to record

Per failure shape, for each failing pod:

- its node, that node's zone and pool (claim 2)
- its owning workload (claim 1)
- whether it is terminal or live (the trap above)

Then, per dimension, the same statistics T1 used so the numbers are comparable:

| Dimension | Pieces | Largest share |
|---|---|---|
| node | | |
| pool | | |
| zone | | |
| workload | | |

**Report all four for every shape**, not just the one that looks best. A shape that clusters strongly in two dimensions at once is more informative than one that clusters in the expected dimension only.

### 2.3 Spatial contiguity, not just grouping

"Clusters by pool" and "reads as a shape on the map" are different claims, and T1 §3.1 is the proof: `sys` is in 2–3 region pieces per zone, so a pool-shaped change only looked contiguous because allocation order happened to make it so.

So for any dimension that clusters, also compute **contiguity by slot ordinal**, using T1's changed-set definition: a run is broken by any slot not in the set.

This is the step that distinguishes *"the failures share a pool"* from *"the map draws them as a run."* Only the second is T2's claim.

---

## 3. The discrimination check

Per T1 §4, and it is what makes the numbers mean anything.

**Shuffle control:** place the same number of failing pods at random across the fleet's live slots, compute the same statistics, repeat 2000 times, and report `P(observed ≤ chance)` per dimension.

Run it **per dimension separately**. A result that beats chance on workload and not on node is the answer to this session's question, and a single combined figure would hide it.

---

## 4. Instrument

Extend `hack/churn/pieces.py` rather than writing a fifth comparator. It already computes both piece definitions by ordinal, runs the shuffle control, and cross-checks against `--dump-positions`.

- Add failure data as an input dimension; keep the existing region and changed-set modes working
- **Emit every figure it reports** — three consecutive sessions have caught an arithmetic error that came from a number narrated rather than emitted, and the fix that worked was making the instrument print it
- **Check totals against the population.** The last such error was a distribution summing to 99 of 100. Assert the total, do not eyeball it
- Extend `pieces-selftest.py` in the same commit

---

## 5. Where to run it

**kind, not the churn fleet** (claim 7). kwok emits almost no events, and three of the four shapes in §2.1 depend on real kubelet behaviour.

That is a constraint worth stating in the result: kind is a handful of nodes, so *pool* and *zone* clustering may be unmeasurable there simply for want of pools and zones. If so, the honest report is that the workload dimension was measurable and the others were not — which still answers the question that matters, since a workload-shaped result does not need a large fleet to be credible.

**Quiesce first.** T-fix §5's cause 3 is a standing rule for this machinery: inducing repeated incidents contaminates the window the correlation reads. Wait until every prior onset has aged past the recency window between shapes.

---

## 6. What the answer decides

**Failures cluster by node, pool or zone, contiguously:** T2 proceeds. The map shows a shape the Annals conceals, and the argument that the list has no node-name crutch for failures holds.

**Failures cluster by workload:** T2 shrinks substantially. The map's geography is zone-organised, so a workload-shaped failure scatters across it — and the Annals already groups by workload for free. What might survive is marking *where* a workload's failures landed, which is a smaller feature than "failures cluster spatially".

**Failures cluster by pool but not contiguously:** T2 inherits T1 §3.1's gap directly. A pool-shaped pattern would render as scatter with matching labels, and `region ← pool ∩ zone` becomes a real blocker rather than a strong suspicion.

**Nothing clusters:** T2's premise is refuted and the phase should not be built. That is a cheap answer to reach and it is the point of measuring first.

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 1 is live and has bitten three times running**, always on a narrated distribution. §4's totality assertion is the mechanical guard.

**Question 2:** a shape producing no failures reports "no failures", not "1 piece" or "0%". An empty set has no largest piece — `pieces.py` already handles this; confirm the new dimensions do too.

**Question 7** applies to the script, not to product code — the previous instance of this error lived in a measurement, not a renderer.

---

## 8. Acceptance

- [ ] Four failure shapes induced separately, each on a quiesced cluster, expectations stated in advance
- [ ] Terminal and live failures separated, per §1's trap; both counts reported
- [ ] All four dimensions reported for every shape, not only the expected one
- [ ] Contiguity by slot ordinal computed for any dimension that clusters (§2.3)
- [ ] Shuffle control run per dimension, 2000 trials, `P` reported
- [ ] Every figure emitted by the instrument; totals asserted against the population
- [ ] `pieces-selftest.py` extended in the same commit
- [ ] Shapes that could not be constructed on kind reported as such
- [ ] Standing questions answered
- [ ] No product code changed

---

## 9. What this session must not do

**No T2 work.** The measurement decides T2's scope; building any of it presumes the answer.

**No tuning toward a clustering result.** A workload-shaped answer is the more useful finding, because it is the one that changes the plan.

**No fixture growth.** The kind cluster is a reference state like the churn fleet is; if a shape needs new nodes, add them via a reversible script, as `bigmem.sh` does.

---

## 10. Estimate

**Two to three hours.** Inducing the shapes and quiescing between them is most of it; the analysis reuses `pieces.py` and the shuffle control already exists.
