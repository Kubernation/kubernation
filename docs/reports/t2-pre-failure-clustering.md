# T2-pre — which dimension do failures cluster in?

**Guidance:** `docs/kubernation-t2-pre-failure-clustering-guidance.md`
**Version:** v1.20.1 · **Date:** 2026-08-07
**No product change.** One instrument extension, one reversible fixture script,
and a number.

**Answer: workload, in every shape that could be constructed.** Not one
pod-level failure clustered by node beyond chance; one whole class has no map
position at all; and the single genuinely node-shaped failure produced **no
failing pods whatsoever** — its signal is the node's own condition, which the map
already renders.

Per §6 this is the "**T2 shrinks substantially**" branch, and §2.1's warned-of
outcome — three of four shapes workload-shaped by construction — did occur and is
discussed rather than waved at.

---

## 1. §1 — claims verified

Claims 1, 2 and 3 checked against source (`PodGlyph`'s fields, `NodeTile`'s
identity fields, `workloads_on_node` as the shared authority). All TRUE. Claims
4–8 inherited and consistent with what this session observed.

**The trap is real and is the instrument's central design point.** `PodGlyph`'s
doc, verbatim: `PodState::Failing` "covers both a terminal `Failed` pod and a
live CrashLoopBackOff one". The instrument therefore never uses it — it buckets
`succeeded` / `failed` / `unhealthy` / `healthy` from the raw pod status and runs
the analysis on **live unhealthy** only, reporting the terminal counts beside it.
Three succeeded pods (the demo Job's history) sat in every capture and would have
been swept in by a naive "not Running" filter.

### 1.1 What kind can and cannot express — established before measuring

| Dimension | On kind |
|---|---|
| workload | measurable |
| node | measurable, 4 nodes |
| zone | **≡ node** — one worker per zone, so the two are indistinguishable by construction |
| pool | **unmeasurable** — no pool label and no instance-type, so every node resolves to `unpooled` |

§5 anticipated "may be unmeasurable for want of pools and zones". It is sharper
than that: pool is not merely underpowered, it is absent, and zone carries no
information that node does not. The instrument refuses both rather than
reporting them — see §3.1.

---

## 2. The shapes

Each induced separately on a quiesced cluster via `hack/kind-failures.sh`
(reversible, adds no nodes), with the expectation stated in the script before the
run. Baseline between every shape: 24 healthy, 3 succeeded, **0 unhealthy**.

| Shape | Expected | node | workload | verdict |
|---|---|---|---|---|
| 1 crash-looper | workload | 3 groups, largest 33%, **P=0.2465** | 1 group, 100%, **P=0.0000** | as expected |
| 2 bad rollout | workload | 3 groups, largest 40%, **P=0.7240** | 1 group, 100%, **P=0.0020** | as expected |
| 3 unbindable PVC | workload | **not attributable** (9/9) | 1 group, 100%, **P=0.0000** | as expected, and more so |
| 4 node down | node | **no failing pods at all** | — | expectation refuted, informatively |

`zone` tracked `node` exactly in shapes 1 and 2, as §1.1 predicted. `pool` was
refused as degenerate in both.

### 2.1 Shape 3 — a failure with no position

Nine pods blocked on an unbindable PVC are `Pending`, so they carry **no
`nodeName`**. They therefore have no node, no zone and no pool — they are not
merely scattered on the map, they are **absent from it**. The instrument reports
`not attributable (9/9)` and `contiguity: no failing pod is on a node the map
places`.

This is a stronger result than "workload-shaped". A geography-based view cannot
show this class of trouble at all, because the scheduler has not yet given it a
location.

### 2.2 Shape 4 — the node failure that produced no failing pods

`docker stop kubernation-worker2`. The node went `NotReady`, and pod status was
captured twice:

```
T+90s   node NotReady   27 pods: healthy=24, succeeded=3   0 unhealthy
T+390s  node NotReady   27 pods: healthy=24, succeeded=3   0 unhealthy
```

Zero, at both time points, including past the 300s eviction timeout. The pods on
the dead node keep the status its kubelet last reported — nothing marks them
failed — and the Deployment's replicas were rescheduled elsewhere healthy while
the DaemonSet's tolerate `unreachable` indefinitely.

**So the canonical node-shaped failure is invisible to a pod-clustering
measurement**, and would have been invisible to a T2 built on one. Its signal
lives entirely in the node's own condition, where the app already reports it —
verified from the app's own output while the node was down:

```
‼ node kubernation-worker2 — NotReady — zone z-b · 4 pods · cpu 1% mem 1%
```

### 2.3 Memory-pressure eviction was not constructible — §2.1's "gap that matters"

All four kind nodes are containers inside one Docker VM sharing its ~15.6 GiB, so
filling "a node's" memory fills every node's and the host's. The pressure would
not be node-scoped even if it were safe to induce, and inducing it risks the
cluster and the host. `nodedown` was substituted, and §2.2 is what it found.

Recorded as the guidance asks: *we could not construct a memory-pressure failure
here*, and that is a fact about what this project can measure.

---

## 3. §3 — the discrimination check

Per dimension, 2000 trials, shuffling the same number of **attributable** pods
across the population of non-terminal pods. Same-count on both sides, so observed
and chance are counting the same thing.

The control discriminates in both directions, pinned by self-tests on synthetic
data: a workload-shaped fixture reports `workload 1 group / node 3 groups`, and a
node-shaped one reports `node 1 group / workload 3 groups`, from the same code
path.

**Low power is not hiding a cluster.** With 4 nodes the node dimension is
underpowered in principle — but the observed distributions were 3/3/3 and 2/2/1,
i.e. as close to *even* as the counts allow. That is the signature of scatter,
not of a cluster the test failed to detect.

### 3.1 A defect in my own instrument, found by real data

Shape 3's first run reported:

```
zone   1 group(s) [9]  largest 9/9 = 100%   P(groups<=obs)=0.0000  <-- clusters beyond chance
pool   1 group(s) [9]  largest 9/9 = 100%   P(groups<=obs)=0.0400  <-- clusters beyond chance
```

Both false. The `node` lookup correctly said "not attributable", but the zone and
pool lookups defaulted a missing node to `"?"` — so nine unschedulable pods
shared a fabricated group, and the control duly found that "cluster" beyond
chance. A measurement of the placeholder, not of the fleet.

This is standing question 2 landing on the instrument, and it is exactly the
failure the product code is written to avoid (`Option` for unknown, never a
sentinel). Fixed: a pod with no node has no zone and no pool, missing values are
excluded from the statistic and counted separately, and the control draws from
the attributable population only. Pinned by five self-tests using the shape that
found it.

Worth stating plainly: **had shape 3 not been run, the instrument would have
reported a spatial cluster that does not exist**, and it would have argued *for*
T2.

---

## 4. §6 — what the answer decides

**The "failures cluster by workload" branch, with two findings the guidance did
not anticipate.**

1. **Every constructible pod-level failure was workload-clustered and
   node-scattered.** P ≤ 0.002 on workload in all three; P ≥ 0.24 on node in both
   where node was attributable.
2. **One class has no map position at all** (§2.1). Unschedulable pods cannot be
   placed, so no geography shows them.
3. **The one genuinely node-shaped failure produced no failing pods** (§2.2). The
   spatial signal that does exist is *node condition*, not pod failure — and the
   map already renders it, as terrain health and a Critical node concern.

Taken together, T2's premise — *"failures cluster spatially, and only the map
shows that"* — is **not supported by this measurement**. The failures that
cluster spatially are node-condition failures, which the map already shows; the
failures that are pod-level cluster by workload, which the map's zone-organised
geography scatters by construction.

What might survive, per §6, is the smaller feature: **marking where a workload's
failures landed** — which is a different claim from "failures cluster spatially",
and needs no new clustering machinery.

### 4.1 Honest limits on that conclusion

- **Three of four shapes were workload-shaped by construction**, as §2.1 warned.
  The taxonomy is partly tautological: a workload-caused failure is
  workload-shaped. What the measurement adds is that the *dominant* everyday
  incident classes — bad rollout, crash loop, unschedulable — are all
  workload-caused, and all three scatter or vanish on the map.
- **This is not a sample of real incidents.** Which causes dominate in practice
  is an empirical question about production clusters that kind cannot answer.
  The generalisation in §4 is an argument, not a measurement, and is labelled so.
- **Pool was never measured** (§1.1) and zone carried no independent information.
  A fleet with real pools might show pool-shaped failures — the churn fleet has
  them, but kwok emits no real failures (claim 7). **No environment available
  here has both.** That is the single biggest gap in this result.

  > **Closed 2026-08-07** — `docs/reports/t2-pre-pool-gap.md`. The gap was two
  > questions: whether real failures *tend* to be pool-shaped (not answerable on
  > any test cluster, since every failure here is induced) and whether a
  > pool-shaped failure *renders as a shape* (answerable, and the one that
  > decides T2). The second was measured on the churn fleet: a failure confined
  > to 100% of one nodepool, `P=0.0000` on the pool dimension, renders as **8
  > disconnected pieces across 3 columns** — and produced no trouble marking on
  > the map at all. T2's premise fails in its own best case.

---

## 5. §7 — standing questions

**1. Summing before comparing?** The guidance flags this as having bitten three
times running. The instrument now `assert`s its bucket counts against the pod
total on every run, so a distribution that does not add up fails loudly instead
of being narrated. Every figure in §2 is emitted by the instrument.

**2. Unknown, or fabricated?** §3.1 — the round's finding, and it was in my own
code. Empty sets report "no failures" rather than "1 group, 0%"; missing
attribution reports "not attributable (n/k)" rather than a shared placeholder;
a single-valued dimension is refused as DEGENERATE rather than reported as a
100% cluster.

**3. Two sections constraining one behaviour, and a fixture where they diverge?**
§2.2 (categorical grouping) and §2.3 (spatial contiguity) both claim the word
"cluster", and shape 3 is where they diverge hardest: perfect workload grouping
alongside *no spatial existence at all*. The instrument reports both and never
substitutes one for the other.

**4. Consumers depending on an old meaning?** None — no product code changed. The
instrument's existing region and changed-set modes still pass their self-tests
unaltered.

**5. Inherited claims?** Claims 4–8 inherited; each describes a state observed
this session. Claim 7 (kwok emits almost no events) is why kind was used, and
§4.1 records the cost of that constraint.

**6. One side of a comparison moved?** The control draws the same number of
*attributable* pods as the observed set, after §3.1's fix. Before it, the
observed set counted fabricated placeholders and the control did not — the two
sides of the comparison had stopped meaning the same thing, which is how the
false cluster survived its first printing.

**7. Container adjacency read as world adjacency?** Applies to the script, and is
handled: contiguity is computed from **slot ordinals**, never from record order
in either input file. Grouping dimensions are categorical and order-free.

---

## 6. §8 — acceptance

- [x] Four shapes induced separately on a quiesced cluster, expectations stated in advance
- [x] Terminal and live failures separated; both counts reported every run
- [x] All four dimensions reported for every shape, including the refused ones
- [x] Contiguity by slot ordinal computed alongside grouping
- [x] Shuffle control per dimension, 2000 trials, `P` reported
- [x] Every figure emitted; totals asserted against the population
- [x] `pieces-selftest.py` extended in the same commit (12 new checks)
- [x] The shape that could not be constructed is reported as such (§2.3)
- [x] Standing questions answered
- [x] No product code changed

**Deviation:** §2.1's "node pressure" shape was substituted with a node stop,
because the former is not constructible here (§2.3). The substitute is genuinely
node-shaped — victims chosen by location, not identity — and what it found is
§2.2, which is more informative than the shape it replaced would have been.
