# KuberNation — A0: Pod Resource Data in the Map Model

**Implementation guidance**
**Goal:** carry per-pod resource facts into the map model so the occupation model (plan §3.4) becomes expressible.
**Shape:** pure model change. No rendering. Independently testable, independently useful.

---

## 0. Verify before building

I have been reliably wrong about one mechanism per round of this work. Rather than hope this round is different, here are the claims this document rests on, each with where to check it. **Verify these first; if one is false, stop and say so before writing code.**

| # | Claim | Check against |
|---|---|---|
| 1 | `sum_pod_requests` / `sum_pod_limits` / `sum_pod_reserved` exist and are `pub(crate)` | `state/model.rs` ~321–363 |
| 2 | `sum_pod_reserved` defaults request→limit and must NOT be used for QoS or over/under | its doc comment |
| 3 | `Metrics.pods` is keyed `(namespace, name)` and **summed across containers** | `k8s/metrics.rs` ~41–57 |
| 4 | `Metrics::pod_usage(ns, name) -> Option<NodeUsage>` is the accessor | same |
| 5 | `derive_qos` exists, returns three classes, uses relative tolerance | `state/advisor.rs` ~376–386 |
| 6 | `PodGlyph` currently carries only namespace / name / state / owner | `state/model.rs` ~256–262 |
| 7 | `NodeTile.cpu_ratio` / `mem_ratio` are usage-based **or** request-based per `metric_source`, never both | `state/model.rs` ~265–284 |

Claim 7 is the one that justifies the whole phase. If both ratios turn out to already be available separately, this document is much smaller than it looks.

---

## 1. Why this is its own phase

Nothing in the plan's §3.4 occupation model is expressible today:

- `PodGlyph` has no resources at all
- `NodeTile` has one memory ratio whose *meaning changes* depending on whether metrics-server is present

The 2×2 that §3.4.1 identifies as the strongest instrument in the plan — requests versus usage, waste on one diagonal and OOM risk on the other — is **literally unrepresentable** with one polymorphic number.

The observation path already exists. This is plumbing: carrying values that core already derives, correctly, into the model the map reads.

**Gate it separately.** Folded into visual work, the gap gets discovered mid-session and improvised around.

---

## 2. The distinction that must not be got wrong

Three request semantics exist in `model.rs` and are **deliberately non-interchangeable**. `sum_pod_reserved`'s doc comment says outright that it is not shared with the right-sizing advisor or `node_request_ratios`, because a request-defaults-to-limit rule "would corrupt the over/under comparison + QoS."

| Purpose | Use | Why |
|---|---|---|
| Occupancy, the 2×2, QoS | `sum_pod_requests` | The **literal** request — what the author actually declared |
| Cost allocation | `sum_pod_reserved` | The scheduler's effective reservation (request defaults to limit) |
| Wall-strain / throttle & OOM | `sum_pod_limits` | The ceiling |

> Using the wrong one produces **plausible numbers that are quietly wrong** — the same failure shape as the Substrate identity collapse, and equally invisible in review.

Put a comment at each new field saying which primitive fills it and why, so the next editor cannot pick the convenient one.

---

## 3. Scope limit: pod granularity, not container

`Metrics.pods` is keyed `(namespace, name)` and summed across containers. The request/limit helpers sum too.

**Consequences to state in the doc comments:**

- Request-versus-usage comparison works at **pod** level ✔
- **Per-container efficiency comparison is not available** — it would need a change to metrics extraction, and that is out of scope here
- Containers-per-pod remains available as a **count** from the spec, which is all the plan's settlement-tier mapping needs

Write this down where someone will find it, or it will get promised later.

---

## 4. The change

### 4.1 `PodGlyph`

```rust
pub struct PodGlyph {
    pub namespace: String,
    pub name: String,
    pub state: PodState,
    pub owner: Option<WorkloadRef>,

    /// LITERAL cpu (cores) / memory (bytes) requests — `sum_pod_requests`, NOT
    /// `sum_pod_reserved`. The reserved variant defaults request:=limit, which
    /// corrupts both QoS and the over/under comparison this feeds.
    pub requests: PodResources,
    /// Limits — `sum_pod_limits`. Zero means unset, which is meaningful
    /// (no ceiling) rather than missing.
    pub limits: PodResources,
    /// Live usage, summed across containers. `None` when metrics-server is
    /// absent or omitted this pod — NOT zero, which would read as idle.
    pub usage: Option<PodResources>,
    /// Standard three-class QoS via the shared `derive_qos`.
    pub qos: QosClass,
    /// Container count from the spec — a count only; see §3.
    pub containers: usize,
}
```

`None` versus zero for usage is the load-bearing detail. A pod with no metrics is **unknown**, not idle, and the map must be able to say so rather than paint an unearned all-clear — the same discipline `SubstrateReport` applies when no DaemonSet reaches the fleet bar.

### 4.2 `NodeTile` — two ratios, not one

```rust
/// Requests ÷ allocatable. ALWAYS available (scheduler-visible, needs no
/// metrics-server). This is what determines schedulability.
pub cpu_request_ratio: f64,
pub mem_request_ratio: f64,

/// Live usage ÷ allocatable. `None` without metrics-server. This is what
/// determines OOM risk.
pub cpu_usage_ratio: Option<f64>,
pub mem_usage_ratio: Option<f64>,
```

The existing `cpu_ratio` / `mem_ratio` become **derived accessors** returning usage when present and requests otherwise, preserving `metric_source` semantics exactly. That keeps every current consumer working unchanged and makes this a purely additive migration.

Do not delete them in this phase. The sweep is a separate, boring change and mixing it in obscures whether A0 itself is correct.

### 4.3 Promote `derive_qos`

It currently lives in `advisor.rs` and returns `RsQos`. The map must not depend on advisor types.

Promote both the function and a neutrally-named enum to a shared home in `state/`, and have the advisor consume it. This is the codebase's own convention — `workloads_on_node` is shared between blast and the Oracle explicitly so the two "can never disagree," and `resolve_region` exists for the same reason.

**Carry the standard three classes.** The plan notes a Burstable subdivision would be genuinely informative, but that is a derivation *on top of* QoS and must not be called QoS. Layer it at render time later; keep A0 pure.

---

## 5. Tests

Everything here is pure, so the interesting logic is directly testable with no GL context and no cluster.

**The distinction (§2) — the most important tests:**
- [ ] A pod with limits set and requests unset: `requests` reflects the **literal** (zero) request, while cost's reserved view still sees the limit. Pins the two apart.
- [ ] That same pod's QoS is Burstable, not Guaranteed

**Usage optionality:**
- [ ] No metrics-server → `usage` is `None` for every pod, and `mem_usage_ratio` is `None` — never `Some(0.0)`
- [ ] metrics-server present but omitting one pod → that pod alone is `None`, the rest unaffected

**QoS:**
- [ ] All three classes derive correctly through the promoted function
- [ ] Advisor and map agree on the class for the same pod — the anti-drift test, mirroring `the_tooltip_and_the_click_never_disagree`

**Migration safety:**
- [ ] Derived `cpu_ratio` / `mem_ratio` return exactly what they did before, under both metric_source values
- [ ] Native sidecar initContainers are still counted, run-to-completion ones still excluded

That last one guards behaviour `sum_pod_requests` already gets right; the risk is a reimplementation losing it.

---

## 6. Acceptance

- [ ] `PodGlyph` carries literal requests, limits, optional usage, QoS, container count
- [ ] `NodeTile` carries request-ratio and optional usage-ratio separately
- [ ] `cpu_ratio` / `mem_ratio` still exist as derived accessors; **no existing consumer changed**
- [ ] `derive_qos` promoted, advisor consumes the shared version, no second implementation
- [ ] Every new field's doc comment names the primitive that fills it
- [ ] Pod-granularity limit (§3) documented where a future reader will hit it
- [ ] `cargo nextest` green, including the advisor/map QoS agreement test

---

## 7. What this unlocks immediately

Worth stating so the phase justifies itself without waiting for the map work:

- **The province window** can show per-pod requests versus usage — the right-sizing story, currently only reachable through the advisor
- **The cost view** gains the requests-versus-usage split it currently collapses into one basis
- **A future Overlay** can render the §3.4.1 2×2 with no further model work

No rendering in this phase. But the phase after it becomes almost entirely presentation, which is the point.

---

## 8. Estimate

**~1 day.** Most of it is the migration sweep and the tests; the derivations already exist and are already correct. The review round should be budgeted as part of this rather than after it — per the v1.5.0 finding, a wrong-but-plausible number is the one unacceptable output, and §2 is exactly where one would come from.
