# Unmeasurable capacity must not read as idle

**Implementation report** · 2026-08-02 · shipped as **v1.6.0**
**Commits:** `8f45e8d` (fix) · `1f33ac2` (review fixes) · `0480612` (docs)
**Governing doc:** [`kubernation-unmeasurable-capacity-guidance.md`](../kubernation-unmeasurable-capacity-guidance.md)

A node that does not report `status.allocatable` rendered **cpu 0% / mem 0%** —
pixel-identical to a node that is genuinely empty. This makes unknown
representable, and hatches it on the map.

The guidance was written against a finding from the
[A-pre round](a-pre-churn-harness.md), and the churn harness built there
reproduces the case on demand. The loop closed.

| | |
|---|---|
| Tests | 355 core · 87 GUI |
| Mutations caught | 6 (3 before the review) |
| Review | 19 raised → 14 confirmed → ~4 distinct defects |
| CI | green — Linux, macOS, Windows |

---

## 1. Verification: all eight §0 claims TRUE

Third round running that §0 survived intact.

| # | Claim | Result |
|---|---|---|
| 1 | `node_allocatable` → `Option`, doc forbids defaults | ✅ |
| 2 | `node_request_ratios` defaults twice | ✅ |
| 3 | `node_usage_ratios` same | ✅ |
| 4 | Both guard so missing-denominator == zero-usage | ✅ |
| 5 | **`cost_report` already does it right** | ✅ comment verbatim |
| 6 | `build_node_tile` reads `allocatable["pods"]` | ✅ |
| 7 | A node can genuinely lack allocatable | ✅ + the named test exists |
| 8 | kwok backfills unless a Ready condition is supplied | ✅ (found in A-pre) |

Claim 5 was the useful one: **the correct pattern already existed**, so this
propagates a design rather than inventing one. `saturate_node` turned out to be a
third precedent — it already omitted pod-count when `allocatable["pods"]` is
absent, so unknown cpu/mem dims got identical treatment.

---

## 2. The fix

Both ratio helpers now return `(Option<f64>, Option<f64>)` **per resource** — cpu
and memory are separate allocatable keys and one can be present without the
other. No sentinel: the type is the mechanism.

Making `NodeTile`'s request pair *and* the derived `cpu_ratio`/`mem_ratio`
optional let the compiler find every consumer, which is the point.

- `saturate_node` omits the cpu/mem dims it cannot compute.
- Node health uses `is_some_and`: unknown is not pressure, and equally not
  health-because-zero, which the old `0.0` quietly asserted.
- A node with no ratio raises an **Info** concern: *"capacity not reported — load
  unknown"*.
- The map **hatches** such a province — texture says *no data* where any hue is
  read as a value on the ramp — gated to the ratio-derived overlays only.
- Gauges read "unknown", not `0%`.

### What §2 doesn't say, and it's load-bearing

The guidance covers the two helpers and `NodeTile`'s request pair, but never
mentions **`cpu_ratio`/`mem_ratio`** — the derived legacy pair that every current
consumer actually reads (the Pressure overlay, the province-window gauges,
attention's pressure concerns, the Oracle bundle). Leaving those `f64` would have
reintroduced the fabrication at precisely the field the acceptance criteria
target.

So §7's *"half a day, the core change is mechanical"* understates: two helpers
are mechanical, and the consumer sweep — the one A0 deliberately deferred — is
the work.

---

## 3. The review found my own defect, one level up

**19 raised, 14 confirmed, ~4 distinct defects.** The root was converged on by
**three independent lenses**, and it is the sharpest available lesson:

> I fixed `saturate_node`'s **inputs** so they could express unknown, and left its
> **output** fabricating.

`worst_level()` returned `SatLevel::Calm` from an `unwrap_or` on an *empty*
dimension set. Three of its four consumers happened to guard on
`dims.is_empty()`. The two that did not:

- **The Oracle bundle** printed `saturation: Calm` one line below the "cpu
  unknown" this very change added — answering the node scope's own default
  question (*"what is straining node X?"*) with a fabrication, and publishing it
  off-laptop on an armed remote endpoint.
- **The minimap** rendered an unmeasurable province byte-identical to a calm one:
  `overlay_flat` draws no hatch, so the fill was the only distinguisher and it was
  `idle_land_pair` either way.

Fixed at the type — `worst_level() -> Option<SatLevel>` — which forced all four
consumers, and let the minimap carry the distinction in its fill since a hatch is
unavailable there.

### Three more

**The hatch floated.** `hatch_diamond` applied the relief lift a *second* time —
`c` is already the lifted top-face centre, because `fill_prism` fills the top at
`c`. Under `MapStyle::Relief` the hatch sat above the tile it marked. Verified
fixed on the churn fleet.

**My own inline justification was false.** The "sanctioned exception" comment on
`cost.rs` claimed the defaulted zeros only feed the `cap_w <= 0.0` guard — but
that guard fires only when *both* keys are absent. A node reporting cpu and not
memory was priced as though it genuinely had no memory, skewing every pod share
and the idle fraction. Both keys are now required, which makes the comment true.

**A test-gap cluster.** The `saturation_lines` unknown branch, the new attention
concern and `pct_or_unknown`, `worst_known`'s two arms, the `> 0.0` denominator
guard, and the province-window strain line — which had no pure fn at all, and is
now `node::strain_line` reading the same `worst_level()` authority as its
SELECTION twin.

### The mutation pass that caught the mutation pass

Six mutations are caught where three were before. Notably, the oracle and cost
fixes were **themselves untested** until a second mutation run caught that — so
both now ship with a test that fails without them. A fix is not done when it
compiles; it is done when reverting it fails something.

---

## 4. A correction to my own decision-log entry

The entry claimed the map, tooltip and panel "cannot disagree". True of those
three — but there was a **fourth** consumer of the same verdict, and it did
disagree. Corrected in `CLAUDE.md` rather than quietly softened.

---

## 5. Acceptance

| §5 criterion | Status |
|---|---|
| Helpers return `Option` per resource | ✅ |
| `NodeTile` carries it; both `None` meanings documented | ✅ |
| No sentinel values anywhere | ✅ |
| Unmeasurable province visually distinct at a glance | ✅ verified live, Plain and Relief |
| Gauges show unknown rather than `0%` | ✅ |
| `node_allocatable`'s doc true of every caller, or the exception says why | ✅ and the exception is now actually true |
| The contradicting doc comment corrected | ✅ |
| Verified on the churn fleet, after confirming the fixture produces the case | ✅ |
| Tests green | ✅ 355 + 87, gui-smoke 51 |

---

## 6. Decisions for the room

### Was minor the right bump?

Shipped as **1.6.0**. It is framed as a fix, but it introduces a new visual idiom
(hatching) and a new attention concern that users will notice — calling that a
patch would undersell it. Flagged because it is a judgment call, not a rule.

### The review bar change worked, and is worth keeping

The A0 round found our verify pass systematically refuted real defects in
prevention-shaped work, because it demanded a wrong number *today*. This round's
verifiers were told explicitly that a path which could reintroduce the confusion
counts even if no consumer exercises it yet. Result: 14 confirmed instead of 0,
including the Oracle leak.

**Ask:** adopt that wording as the standing bar for correctness-hardening rounds?

### Five versions on main still carry no tag

v1.2.0, v1.3.1, v1.4.0, v1.5.0 — and now v1.6.0 — are pushed and green but
untagged, notes under *Unreleased*. Third round raising this.

**Ask:** cut a release, or set a cadence?
