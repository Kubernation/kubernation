# Closing the pools-and-failures gap

**Follows:** `docs/reports/t2-pre-failure-clustering.md` §4.1, which named this
the biggest gap in that result
**Version:** v1.20.2 · **Date:** 2026-08-07
**No product change.** One instrument-adjacent script, and a number.

**Answer: a perfectly pool-shaped failure does not read as a shape on the map.**
100% of one nodepool, `P=0.0000` on the pool dimension, renders as **8
disconnected pieces across 3 columns**. That is §6's third branch of the T2-pre
guidance, measured rather than suspected.

And a second finding the design did not anticipate: **those 29 failures produced
no trouble marking on the map at all.**

---

## 1. The gap was two questions wearing one name

T2-pre said *"no environment available here has both real pools and real
failures."* That is true and, examined, it splits:

| | Answerable? |
|---|---|
| **(a) Do real-world failures TEND to be pool-shaped?** | **No — on any test cluster.** Every failure here is induced, so the frequency is whatever the fixture picks. A bigger fleet does not help; this is a production question |
| **(b) IF failures are pool-shaped, does the map render them as a shape?** | **Yes** — and it is the one that decides T2 |

Only (b) was ever closable, and it is what §6's third branch turns on. It needs
real **placement**, not real **causes** — and placement is exactly what the churn
fleet has that kind does not.

Conflating the two is what made the gap look unclosable. Recorded so the
distinction survives.

---

## 2. Mechanism, and its honest limit

kwok has no failure stage, and adding one needs `kwokctl --enable-crds=Stage`,
which means **recreating the cluster** — discarding the layout store that carries
T1's 18-succession record and every measurement judged against it. Not worth it.

Instead: a patch to the pod **status subresource** sticks. Verified before
relying on it — a probe pod read `Running ready=false reason=CrashLoopBackOff`
twenty seconds later, unmolested by kwok's reconcile.

So `hack/churn/failures.sh` writes an API state shaped exactly like a real
crash-loop onto pods **the real kube-scheduler placed**. Nothing touches nodes,
so slots, ordinals and the succession record are untouched — confirmed after
restore: 100 nodes, 18 changed slots, 3 of 8 regions fragmented, identical to
before.

**The limit, stated plainly:** the *cause* is synthetic. That is fine for (b),
which is a question about geometry and rendering, and useless for (a), which this
does not claim to answer.

---

## 3. The two arms

Same fleet, same instrument, same 2000-trial control.

**Pool-shaped** — every `node-agent` pod on the `sys` pool's 30 nodes. The
canonical pool-shaped incident: a bad node image rolled to one nodepool, breaking
its per-node agent.

```
node      29 group(s), largest 3%    P=1.0000   indistinguishable from chance
zone       3 group(s), largest 34%   P=0.0010   clusters beyond chance
pool       1 group(s), largest 100%  P=0.0000   clusters beyond chance
workload   1 group(s), largest 100%  P=0.0000   clusters beyond chance

contiguity z-a: 3 pieces [6, 2, 1]  largest 6/9  = 67%
contiguity z-b: 3 pieces [4, 3, 3]  largest 4/10 = 40%
contiguity z-c: 2 pieces [6, 4]     largest 6/10 = 60%
```

**Workload-shaped** — every pod of one Deployment, wherever the scheduler put
them.

```
node      23 group(s), largest 8%    P=0.9325   indistinguishable from chance
zone       4 group(s), largest 25%   P=1.0000   indistinguishable from chance
pool       4 group(s), largest 33%   P=1.0000   indistinguishable from chance
workload   2 group(s), largest 96%   P=0.0000   clusters beyond chance

contiguity: 3 + 4 + 4 + 3 = 14 pieces across 4 zones
```

### 3.1 The discrimination check — pool is genuinely measurable here

The same dimension gives **P=0.0000** in one arm and **P=1.0000** in the other.
The instrument can tell pool-shaped from not, on this fleet, which is precisely
what kind could not do (every node there is `unpooled`). **The gap is closed as a
measurement capability**, not only as a result.

---

## 4. The answer

**A pool-shaped failure clusters perfectly and still does not draw a shape.**

Eight disconnected pieces across three columns, the largest holding 40–67% of its
zone's affected nodes. Set against the workload-shaped arm's 14 pieces across
four zones, pool-shaped is **modestly** more contiguous — 2.7 pieces per zone
versus 3.5 — and that is the entire difference. Both are scatter.

The cause is the one T1 §3.1 identified and this now confirms from the failure
side: A2's zone-wide ordinals interleave pools, so `sys` is in 2–3 region pieces
per zone. A failure confined to `sys` inherits that fragmentation exactly.

### 4.1 It is worse than "not a shape" — it is not drawn at all

The 29 failing pods produced **no trouble marking on the map**. `node-agent` is a
DaemonSet, and DaemonSets are roads, not cities — there is no settlement to flag,
and the terrain shows node health, which was fine (the nodes were Ready; only the
agent pods were failing).

The alternative is no better. A *Deployment* pinned to one pool sites its city at
the **plurality** node — one province — so thirty failures spread across thirty
nodes would render as a single troubled city, which understates the shape in the
opposite direction.

### 4.2 And the list does not name the pool either

The app's own queue aggregated all 29 into one concern:

```
‼ ds churn/node-agent — CrashLoopBackOff ×29 — 99/100 ready · rollout Progressing
```

Correct behaviour — "city in trouble, not 40 pod alarms" — and **workload-grouped
with no mention of the pool**. T1 found the Annals beating the map because node
names carry the pool; here nothing carries it, because the concern names the
workload.

So for the incident class most favourable to T2, *neither* surface says "this is
confined to one nodepool".

---

## 5. What this decides

**T2's premise fails in its own best case.** The measurement was built to give
T2 its strongest shot — a failure that is 100% pool-confined, on a fleet with
four real pools across four zones — and the map still does not show it as a
shape, does not mark it at all, and the queue does not name the dimension.

§6's third branch said this would make `region ← pool ∩ zone` "a real blocker
rather than a strong suspicion". That item **already shipped** (v1.14.0–v1.17.0:
pool colour, region labels, the POOLS legend), and shipping it did not help —
because the failures are not rendered on the provinces at all, so there is no
mark for the pool tint to group.

**What survives is a sentence, not a map feature.** The gap is that nothing says
*"these failures are confined to one pool"*. That is a property the app can
already compute — it has every failing pod's node, and `NodeTile.pool` beside
it — and it belongs in the concern, next to the existing `×29`. It needs no
geography, no clustering machinery, and no T2.

---

## 6. Standing questions

**1. Summing before comparing?** The instrument asserts its bucket counts against
the pod total on every run; all figures here are emitted, none narrated.

**2. Unknown, or fabricated?** The `1/30 not attributable` in the pool arm is
real and is reported: one `sys` node has no `node-agent` pod, because it is the
no-allocatable node whose agent is genuinely unschedulable — the fleet's
long-standing `ds churn/node-agent — unschedulable` concern. It is excluded from
the statistic and counted, not silently dropped or defaulted.

**3. Two sections constraining one behaviour?** Grouping and contiguity, again,
and this round is the sharpest divergence yet: `pool` grouping is **perfect**
(P=0.0000, 100%) while pool contiguity is **8 pieces**. Reporting only the first
would have said "failures cluster by pool" and licensed T2.

**4. Consumers depending on an old meaning?** None — no product code changed.

**5. Inherited claims?** The claim inherited here is my own §4.1 from the T2-pre
report: *"no environment has both real pools and real failures."* Re-examined, it
is true of *causes* and false of *placement*, and the distinction is what made
the measurement possible. Fourth session running in which re-examining one of my
own statements changed the work.

**6. One side of a comparison moved?** The two arms use the same control, the
same population and the same k-selection, so the P values are comparable across
them. That comparability is what §3.1 rests on.

**7. Container adjacency read as world adjacency?** Contiguity is computed from
slot ordinals, never from record order in either input.

---

## 7. Restoration

`MODE=down` deletes the marked pods; their controllers recreate them clean.
Verified after: 100 nodes, 422 pods, 1 unhealthy (the pre-existing unschedulable
agent), and the layout reference state unchanged — 18 changed slots in 4 pieces,
3 of 8 regions fragmented, `P` values identical to this morning's.
