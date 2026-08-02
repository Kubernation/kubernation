# KuberNation — Unmeasurable Capacity Must Not Read as Idle

**Implementation guidance**
**Goal:** a node whose `status.allocatable` is absent must be distinguishable from a node that is genuinely empty.
**Shape:** small core change plus a render decision. Prerequisite for A2.

---

## 0. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `node_allocatable` returns `Option<f64>` and its doc says callers must not fabricate a default | `state/model.rs` ~509 |
| 2 | `node_request_ratios` calls it with `.unwrap_or(0.0)` **twice** | `model.rs` ~524–525 |
| 3 | `node_usage_ratios` does the same | `model.rs` ~542–543 |
| 4 | Both then guard `if alloc > 0.0 { used / alloc } else { 0.0 }` — so a missing denominator and a zero-usage node produce **the same 0.0** | same |
| 5 | `cost_report` does **not** have this bug — it branches on `cap_w <= 0.0` and records an honest all-idle node | `state/cost.rs` ~281–315 |
| 6 | `build_node_tile` calls `node_allocatable(node, "pods")` for pod-slot saturation | `model.rs` ~555 |

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 7 | A node can genuinely lack `status.allocatable` in a live cluster | The whole fix is pointless otherwise. A-pre confirmed it, and `override_node_without_allocatable_is_priced_all_idle` already pins the cost path |
| 8 | kwok **backfills** fake capacity (1k cpu / 1Ti / 1M pods) unless the fixture supplies a Ready condition | A-pre §3. A harness node that looks allocatable-less may not be — verify the fixture actually produces the case before trusting a green run |

Claim 5 is the useful one: **the correct pattern already exists in this codebase.** This is not a new design, it is propagating one.

---

## 1. The defect

`node_allocatable`'s doc comment says callers must not fabricate a default. Two of its callers do, and `node_request_ratios`' own doc comment *documents* the fabrication — "Missing allocatable yields 0 (gauge renders empty)". **Two doc comments in the same file contradict each other.**

The observable result, verified on the churn fleet: a node with no allocatable renders **cpu 0% / mem 0%** — pixel-identical to an idle node.

That is an unearned all-clear, and this codebase has repeatedly refused exactly that shape:

- `SubstrateReport` falls back to terrain when no DaemonSet reaches the fleet bar, rather than colouring everything clean
- A0's `usage: Option<PodResources>` — absent means *unknown*, never idle
- `CostBasis` degrades to `Requests` rather than claiming a false "usage-refined"
- `cost_report` records an all-idle node explicitly "so it isn't silently dropped"

The ratio helpers are the one place the discipline was never applied.

**It is now a blocker.** Plan §3.2 makes A2's province extent capacity-derived. A node reporting 0.0 capacity would get zero extent — no province, or a silent fallback indistinguishable from a genuinely small node. A2 cannot be honest on top of a dishonest denominator.

---

## 2. The fix

### 2.1 Make unmeasurable representable

```rust
/// Sum of CPU/memory *requests* of non-terminal pods, divided by allocatable.
/// `None` when the node does not report allocatable — the ratio is UNKNOWN,
/// not zero. A zero would be indistinguishable from an idle node, which is the
/// unearned all-clear `cost_report` already refuses (see its `cap_w <= 0.0`
/// branch).
pub fn node_request_ratios(node: &Node, pods: &[&Pod]) -> (Option<f64>, Option<f64>);
```

Same for `node_usage_ratios`. **Per-resource, not per-node** — cpu and memory are separate keys and one can be present without the other.

Do **not** invent a sentinel (`-1.0`, `f64::NAN`). The type is the mechanism; A0 established this with `Option<PodResources>`.

### 2.2 Carry it to `NodeTile`

A0 gave `NodeTile` separate request- and usage-ratios. This makes the request side optional too. Note the resulting asymmetry, and document it — the two `None`s mean different things:

| Field | `None` means |
|---|---|
| `*_usage_ratio` | No metrics-server, or no sample for this node |
| `*_request_ratio` | **The node does not report its capacity** |

The second is rarer and more serious: usage is best-effort telemetry, capacity is something a healthy node always reports. A node missing it is malfunctioning or mid-registration, and that is worth seeing.

### 2.3 Pod-slot saturation

`build_node_tile` also reads `node_allocatable(node, "pods")`. Same treatment — an unknown pod ceiling is not an empty one. Check what the existing `pod_slot_concern_absent_without_allocatable_pods` test already pins before changing behaviour here; the attention path may already degrade correctly.

---

## 3. Rendering: what unmeasurable looks like

The core change is mechanical. **This is the part that needs a decision.**

`None` must not render as 0. The requirement is that an operator can tell "I don't know" from "nothing here" at a glance.

Options, in preference order:

1. **Hatching.** The established cartographic idiom for *no data* — and it composes with any overlay, because it is texture rather than hue. `Coast` already generates terrain texture, so the machinery is nearby.
2. **A distinct desaturated fill**, clearly outside the overlay ramps. Simpler; costs a colour in every palette and must survive the `cb_*` funnel.
3. **A mark on an otherwise-normal province.** Weakest — it reads as an annotation on a valid reading rather than an absence of one.

Recommend **hatching**. It says *no data* rather than *a data value*, which is exactly the distinction, and it is register-independent — a `Survey` or `Plan` style inherits it without a palette decision.

Gauges elsewhere (province window, sidebar) should show a dash or "unknown", not `0%`.

---

## 4. Tests

- [ ] A node with no allocatable yields `None`, not `Some(0.0)` — both request and usage helpers
- [ ] A node with cpu allocatable but no memory allocatable yields `(Some(_), None)` — the per-resource case
- [ ] A genuinely idle node with allocatable present still yields `Some(0.0)` — **the discrimination test, and the point of the whole change**
- [ ] Existing consumers of `cpu_ratio` / `mem_ratio` behave unchanged for nodes that do report allocatable
- [ ] `cost_report`'s all-idle branch is untouched — regression guard, since it was already right
- [ ] Pod-slot saturation does not treat an unknown ceiling as an empty one

**Mutation check** (per the A0 acceptance bar): replace `None` with `Some(0.0)` in each helper and confirm a test fails. If none does, the discrimination test is not actually discriminating.

---

## 5. Acceptance

- [ ] `node_request_ratios` and `node_usage_ratios` return `Option` per resource
- [ ] `NodeTile` carries the optionality; both `None` meanings documented at the fields
- [ ] No sentinel values anywhere in the change
- [ ] An unmeasurable province is visually distinct from an idle one at a glance
- [ ] Numeric gauges show unknown rather than `0%`
- [ ] `node_allocatable`'s doc comment is now true of every caller — **or** the callers that legitimately default say why inline
- [ ] `node_request_ratios`' contradicting doc comment is corrected
- [ ] Verified on the churn fleet's allocatable-less node, after confirming per §0 claim 8 that the fixture really produces the case
- [ ] `cargo nextest` green

---

## 6. Why now, and not folded into A1

A1 is the layout engine and is consumer-less by design; this is a live user-visible correctness fix with an existing reproduction. Keeping them separate means A1's review is not entangled with a rendering decision, and this lands its own regression guard.

It also lets A2's guidance state the extent fallback chain — capacity, then instance type, then a declared default — as something that *works*, rather than something carrying a known-dishonest denominator.

---

## 7. Estimate

**Half a day.** The core change is mechanical; the render treatment and its verification on the churn fleet are most of the time.
