# Per-pod usage history, and P90 right-sizing

**Follows:** the v1.32.0 handoff §4.3 deferred graft ("per-pod metrics history for
true P90 sizing"), recorded at v0.41.0 as *"a per-pod usage history ring for true
P90/P95 sizing"*
**Version:** 1.35.0 · **Date:** 2026-08-30

The right-sizing advisor emitted concrete resource requests — advice an operator
acts on by editing a manifest — from **one instantaneous reading**, with "peak"
meaning the hottest *replica at that instant* rather than the hottest *moment*.
It now uses the 90th percentile of each pod's own history, and says which basis
it used.

---

## 1. What was actually there, verified before designing

- `rightsizing_report` read `world.pod_usage(...)`: the latest sample only.
- `upeak_cpu/mem` was `max` across replicas **at one instant**, not over time.
- `metrics.rs` already kept `node_rings` and `cluster_ring` at `HISTORY_CAP` 60.
  **No pod rings.**

So the pattern was built and tested one level up, which is why this was recorded
as a graft rather than a feature.

---

## 2. The design decision: per-pod rings, not per-workload

Per-workload aggregation at record time would be far cheaper — workloads are
~50× fewer than pods — and it was the obvious first thought. Rejected for two
reasons, recorded so it is not re-proposed:

1. **Layering.** `metrics.rs` is the poller; a per-workload ring needs the
   pod→workload mapping, which lives in `state/` behind `OwnerIndex`. Recording
   per pod keeps the poller ignorant of workloads and leaves `rightsizing_report`
   a pure function of `ObservedWorld`.
2. **It cannot answer "which replica is hot over time."** Aggregating at record
   time destroys that permanently; aggregating at read time keeps the option.

**Cost, computed rather than feared:** `NodeUsage` is two `f64`s. At the
documented 5000-pod ceiling, 5000 × 60 × 16 B ≈ 4.8 MB of samples, ~5.5 MB with
keys and deque headers. Affordable for an operator-laptop tool.

**The one real difference from node rings is churn.** A node name is stable for
the node's life; a CronJob mints a new pod name every run, so a long grace
accumulates rings for pods that no longer exist. `RING_GRACE` (4 polls ≈ 1 min)
drops a departed pod promptly while still surviving the one-poll scrape hiccup
the grace exists for, which bounds the map at roughly the live pod count.

**One retention rule, not two.** Extracting `Metrics::roll` for the pod rings
left the node path inlining the same logic — two homes for one rule, in the same
function, minutes after a whole session spent removing exactly that. Both now
call `roll`.

---

## 3. The honesty rules

**A short ring does not get to call itself a P90.** Below `P90_MIN_SAMPLES` (8,
≈2 min at the 15s poll) the percentile of three readings is the maximum wearing a
statistical name — worse than useless for a number an operator edits a manifest
from. Those rows fall back to the latest reading.

**`percentile` returns `None` for an empty input, not 0.0.** A fabricated zero
would read as a workload using nothing and be recommended down to the floor.

**A row is only as well-founded as its thinnest input.** `RsRow.basis` takes the
*shortest* history among the row's measured pods — one replica with two minutes
and one just started does not make a two-minute row.

**`UsageBasis::Latest` is the `#[default]`**, deliberately: a row built without
setting it has not earned the stronger claim.

**And the surface says which** — the `metric_source` / `CostBasis` /
`idle_meaning` discipline, applied for the third time this stretch.

### 3.1 The footer had to be got right twice

It said *"from 1 metrics-server sample"* unconditionally. Once most rows became
P90s, that **understated** what the recommendation rests on — the mirror of this
morning's `idle` defect, which overstated by staying silent.

My first fix reported the **weakest** row, so any `Latest` row collapsed the whole
footer. That is also wrong, and worse in practice: a rolling deploy leaves one
fresh pod almost permanently, so a table of solid P90 rows would describe itself
as single-sample forever and the new window would look unearned.

It now reports the predominant basis **and counts the exceptions** —
*"P90 of each pod's usage over the last 40 samples (~10 min) — 1 of 9 rows from a
single sample"* — overstating neither side. Samples **and** minutes, so neither
figure has to be taken on trust.

---

## 4. Mutation floor — six, and two fixtures that could not see their own rule

| | mutation | |
|---|---|---|
| V1 | a short ring claims a P90 anyway | caught |
| V2 | a row claims its LONGEST input, not its shortest | **survived twice** |
| V3 | the advisor ignores the history (pre-phase behaviour) | caught |
| V4 | `percentile` fabricates 0.0 for an empty ring | caught |
| V5 | the footer hides the weakest row | caught |
| V6 | pod rings are never pruned (the churn leak) | **survived** |

**V6** survived because nothing tested pod-ring churn at all — the node tests
cover the shared logic, but a pod's *name* is the thing that churns, and that is
the whole reason the grace matters here. Closed with a test that a one-poll gap
is not a departure and a real departure ages out.

**V2 survived twice, and the second time exposed a defect in my own fixture.**
`set_pod_history` seeded one pod per call and *replaced* the metrics map each
time, so seeding a second pod erased the first and aged out its ring — a two-pod
test silently collapsed to one measured pod, where longest and shortest are the
same number. Replaced by `set_pod_histories`, which drives all the pods along one
timeline the way the poller does, and aligns shorter series to the END (a pod
with fewer samples is one that started later, which is what the poller sees).

That is the same shape as this stretch's other survivals — the fixture could
express the positive case and not the negative one — but with a sharper edge: the
helper's *design* made the negative case unrepresentable.

---

## 5. Performance, measured — and a pre-existing cost I did not introduce

`rightsizing_report` is called inside the advisor's **draw**, so it runs at frame
rate while that tab is open. The P90 work adds a ring clone, two allocations and
two sorts per measured pod, so this needed measuring rather than assuming.

At the documented ceiling (500 nodes / 5000 pods, full 60-sample rings):

| | |
|---|---|
| with the P90 path | **4.20 ms/call** |
| with it disabled | **4.76 ms/call** |

Indistinguishable — the cost is dominated by walking 5000 pods, not by the
percentile. **The ~4ms per frame is pre-existing**, applies to every advisor tab
(all of which build their report in the draw), and is ~25% of a 60fps frame.

Left alone deliberately rather than folded in: memoizing the report on the
snapshot `Arc` (the `browse.rs` / posture-chip pattern) would fix it for all the
tabs at once and belongs in its own change. `rightsizing_report_cost_at_scale` is
the guard that keeps the tab inside a frame meanwhile.

The full model rebuild is unchanged at 7.0 ms (budget 100 ms) — the rings are
filled by the metrics poll, not by `Models::build`.

---

## 6. The gate

`examples/rightsize.rs`, headless, on the live kind cluster with metrics-server:
a real ring cannot be filled in a unit test, since it needs `P90_MIN_SAMPLES`
polls at 15s. The `examples/drain.rs` precedent — a derivation an operator acts
on should not ship on unit tests alone.

**Expectation, stated before the run:** rows start on `latest`, and after ~2
minutes flip to a P90 window.

**Result — the flip lands exactly on the boundary.** `P90_MIN_SAMPLES` is 8 and
the poll is 15s, so the window fills at 120s:

```
[  1s] 5 measured rows, 0 on a P90 window
[ 31s] 5 measured rows, 0 on a P90 window
[ 61s] 5 measured rows, 0 on a P90 window
[ 91s] 5 measured rows, 0 on a P90 window
[121s] 5 measured rows, 5 on a P90 window
         kube-system/coredns          cpu req 0.100 use 0.002 -> 0.030   basis P90 over 8
         kube-system/kindnet          cpu req 0.100 use 0.002 -> 0.030   basis P90 over 8
         kube-system/metrics-server   cpu req 0.100 use 0.005 -> 0.030   basis P90 over 8
         kubernation-demo/db          cpu req 0.050 use 0.000 -> 0.030   basis P90 over 8
```

The four polls of `latest` before it are the discrimination: the instrument
distinguishes a short ring from a filled one on live data, rather than reporting
P90 for everything from the start.

**An honest limit of this cluster.** kind is idle, so the suggestions are
identical either way — every workload sits at the VPA floor (0.030 cores)
regardless of basis. The gate proves the *mechanism* (rings fill, the basis
flips, the window is reported); it does not demonstrate a *changed
recommendation*, which needs a workload with a spiky profile. The unit test
`the_p90_differs_from_the_latest_sample` covers that case, and it is the
synthetic half of the evidence rather than the live half.

---

## 7. Acceptance

- [x] Per-pod rings, keyed and pruned like the node rings, with the churn difference documented and tested
- [x] One retention rule shared by both ring sets
- [x] P90 used only where the window supports it; `Latest` otherwise
- [x] The basis is on the row, and the surface says it (§3.1)
- [x] Cost measured, not predicted — including the A/B that shows what I did *not* cause (§5)
- [x] Mutations asserted applied; both survivals closed, with the fixture defect recorded
- [x] Live gate on a real metrics-server (§6)
- [x] `cargo nextest` green; clippy clean with and without features; 0 broken doc links

**Deferred, with reasons:** per-container P90 (metrics-server sums containers per
pod — the data does not exist); P95/P99 (the ring is 60 samples, so the top
percentiles are one or two readings — `percentile` takes `p` so this is a
constant, not a rewrite); a longer window (bounded by `HISTORY_CAP`, and a
multi-hour window wants persistence, which this tool does not do); and memoizing
the advisor reports (§5).
