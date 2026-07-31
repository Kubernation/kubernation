# A0 — Pod resource data in the map model

**Implementation report** · 2026-07-31 · unversioned by design
**Commits:** `7b072ce` (A0) · `440a0f6` (guardrails) · `2fdd560` (coverage) · `fc4e724` (method)
**Governing docs:** [`kubernation-a0-resource-data-guidance.md`](../kubernation-a0-resource-data-guidance.md) (the work), [`kubernation-enabling-plan.md`](../kubernation-enabling-plan.md) §3.4 / §3A (why the phase exists)

A gated prerequisite: carry per-pod requests, limits, usage and QoS into the model
the map reads, so the occupation model becomes expressible. No rendering. One wrong
number was caught before it shipped — and the round says something uncomfortable
about how we specify and review this work.

| | |
|---|---|
| Core lines | +879 / −55 |
| New module (`state/qos.rs`) | 351 loc |
| New tests | 19 |
| Existing consumers changed | **0** — the GUI crate diff across the phase is empty |
| Model rebuild @ 500 nodes / 5000 pods | 5.7 ms (unchanged) |
| Wrong numbers caught | 1 |

Tests: 345 core, 83 GUI. Lint clean.

---

## 1. Verification came back clean

The guidance opened with seven claims and an instruction to stop if any were false.
**All seven hold.** That is the first time in this series a specification has
survived verification intact — worth noting, because the defects this round were
somewhere else entirely.

| # | Claim | Verdict |
|---|---|---|
| 1 | The three `sum_pod_*` primitives exist, `pub(crate)` | true (line drift only: 341/347/357 vs "~321–363") |
| 2 | `sum_pod_reserved` must not feed QoS or over/under | true — verbatim in its doc comment |
| 3 | Pod metrics keyed `(ns, name)`, summed across containers | true |
| 4 | `pod_usage` is the accessor | true |
| 5 | `derive_qos`: three classes, relative tolerance | true |
| 6 | `PodGlyph` carries only ns / name / state / owner | true — exactly four fields |
| **7** | **Node ratios are request- *or* usage-based, never both** | **true** |

Claim 7 is the one that justifies the phase existing, so it got the closest look.
It holds at the strongest possible level: `build_node_tile` computes
`(cpu_ratio, mem_ratio, metric_source)` from a `match usage`, so when metrics-server
is present the request ratio is not merely hidden — **the function that computes it
is never called**. Nothing here was redundant work.

---

## 2. What the model can now say

One polymorphic number cannot express a relationship between two. With requests and
usage carried separately, the matrix the plan calls its strongest instrument becomes
representable — one diagonal is money, the other is danger, and neither was visible
before.

| | Low usage | High usage |
|---|---|---|
| **Low requests** | Idle — schedulable, nothing to do | **Overcommitted — OOM risk.** Memory is incompressible: it kills rather than throttles |
| **High requests** | **Waste** — reserved and never used. This is money | Healthy — full and correctly sized |

Also carried: per-pod limits, an **optional** usage (absent means *unknown*, never
idle), the QoS class, and a container count. Node tiles keep their old numbers
unchanged, now derived from the new pair in one place.

### The migration mechanism the guidance got wrong

§4.2 asked for derived **accessors** so no consumer would change. That does not work
in Rust — field access and method calls differ syntactically, so accessors would have
touched every call site, the opposite of the stated goal. Keeping them as fields
derived at construction achieves the purpose exactly, with a single derivation point
so they cannot drift:

```
git diff --stat 817f565..HEAD -- crates/kubernation/     # empty
```

---

## 3. The wrong number, and where it came from

The specification named §2 — three deliberately different request semantics — as the
place a plausible-but-wrong number would originate, and treated the QoS promotion as
routine plumbing. **It was exactly backwards.**

| Flagged as dangerous | Where it actually was |
|---|---|
| ~~`sum_pod_requests` vs `sum_pod_reserved`~~ | `derive_qos` — "just promote it" |
| Kubernetes defaults `requests := limits` at admission, so no stored pod has a limit without a request. Verified against a live API server on the exact mixed-container case: **the two functions return identical values for every pod that exists.** The hazard cannot fire. | Real QoS is **per-container**: one container missing either a cpu or a memory limit disqualifies the whole pod. That function sums first and compares after, so it cannot see the rule. |

Verified live — a fully-specified container beside an unspecified sidecar, an
ordinary logging or proxy shape:

```
container a: requests={cpu:100m, memory:64Mi}  limits={cpu:100m, memory:64Mi}
container b: requests=None                     limits=None

apiserver qosClass : Burstable
summed totals      : req == lim  ->  Guaranteed   <-- wrong
```

Since the plan (§3.4.2) renders QoS as building material — tents / timber / stone by
eviction order — the map would have drawn **stone where the kubelet evicts as
timber**.

**Fixed** by making the pod-level path authoritative: `qos::pod_qos` prefers the API
server's own `status.qosClass` and falls back to the upstream per-container rule. The
totals-based function is kept, renamed `qos_from_totals`, and documented as an
approximation used only where the advisor genuinely has no pod object to consult.

> ### The generalizable rule
>
> **The hazard sits wherever a summing step precedes a comparing step.**
>
> That single question explains the QoS defect, both converged review findings, and
> why the flagged distinction was harmless — it involves no comparison at all. Asked
> on the first pass, it would have pointed straight at the right section.

---

## 4. Review, and a problem with the review

Five lenses raised nine findings. The verification pass **refuted all nine**. Four of
them were real, and I fixed them anyway.

The four were coverage holes, each demonstrated by a mutation that survived the
entire suite — I reproduced every one before acting:

1. **A limits-only pod could be classified BestEffort** — evicted *first* instead of
   after. Dropping the `any_lim` half of the guard left all tests green. This hole was
   self-inflicted: I dropped the guidance's prescribed QoS test because its *prose* is
   wrong about a live cluster, when the *assertion about the fixture* was correct and
   reached a branch nothing else did. Wrong framing is not a reason to delete coverage.
2. **The tolerance test exercised no tolerance** — every operand pair was
   bit-identical, so it reduced to `==`. The tolerance is load-bearing:
   `cpu: "0.7"` and `cpu: "700m"` are one quantity written two ways and parse to
   `0.7` and `0.7000000000000001`. Kubernetes calls that pod Guaranteed; exact
   equality would say Burstable.
3. **The census-vs-load distinction had no test** — summing glyph requests gives 5
   cores on a 4-core node whose request ratio reads 0.25. Both correct, not comparable.
4. **`build_node_detail`'s pod-usage closure was untested** — replacing it with
   `&|_,_| None` left the suite green, and that is the exact field the guidance names
   as this phase's first unlocked consumer.

The verifiers conceded every factual claim and refuted regardless:

> "The coverage claim is factually accurate but describes no defect."
>
> "The guidance's prescribed assertion would indeed have passed."
>
> "`eq` degenerates to `==` and the assertion is tautological with respect to the tolerance."

One verifier reproduced the surviving mutation itself, watched the suite stay green,
reverted, and refuted it.

### The cause is our bar, not the findings

Verifiers were told to refute unless they could trace a path to a wrong result
*today*. For a pure model change whose consumers do not exist **by design**, no
finding can ever meet that bar — every criticism of A0 is necessarily about a future
consumer. The gate made the phase unfalsifiable, and "zero confirmed" is an artifact
of it rather than evidence of correctness.

Note the shape: **this is the same error the specification made.** Both aimed a threat
model at a failure that cannot fire. For a gated prerequisite the bar has to be
*"would this be wrong when the consumer arrives?"*

### A correction to last round's lesson

The v1.5.0 report concluded that cross-lens convergence was the reliable signal. This
round produced two converged clusters and **both were refuted on solid evidence** —
including a pre-existing test, predating this work (`d84f41d`), that already pins the
behaviour one of them attacked. Convergence marked *"this area is confusing"*, not
*"this is broken"*. Both are now documented at the point of use rather than dismissed.
One round is not a law.

---

## 5. What this unlocks, and what it does not

| Now available | Still out of reach |
|---|---|
| Per-pod requests versus usage in the province window | Per-*container* efficiency — pod metrics are summed across containers upstream |
| The requests/usage split the cost view currently collapses | A Burstable subdivision — a derivation *on top of* QoS, and must not be labelled QoS |
| The 2×2 overlay, with no further model work | Per-container-exact advisor QoS — needs its pod template, and moves advisor output |

The pod-granularity limit is stated on the fields themselves, where the next reader
will hit it, rather than in a document they may not open.

---

## 6. Decisions for the room

### Adopt a different acceptance bar for gated prerequisites

This phase was deliberately built with no consumers, and our review gate cannot
evaluate anything built that way. Every future prerequisite — and the plan front-loads
several — will hit the same wall.

**Ask:** adopt *"wrong when the consumer arrives?"* for phases gated ahead of their renderer?

### The advisor still classifies QoS by summing

The map is now authoritative; the advisor is not, because it asks at workload
granularity where no pod object exists. The two can legitimately disagree, which is
documented and tested. Making it exact needs the pod template and would move advisor
output, so it wants its own gate.

**Ask:** schedule it, or accept the documented approximation?

### Five versions on main now carry no tag

A0 is deliberately unversioned — no user-visible surface, following the established
precedent for a prerequisite phase. But v1.2.0, v1.3.1, v1.4.0 and v1.5.0 remain
untagged with their notes under *Unreleased*, and a tag fires the signed release
pipeline.

**Ask:** cut a release, or set a cadence? (Unchanged from last round.)
