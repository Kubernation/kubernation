# D2 — the free version, checked first; and a false premise

**Guidance:** `docs/kubernation-d2-brushing-guidance.md`
**Date:** 2026-08-07 · **No product change**, no version bump.

**Two findings, and together they say stop before §3.**

1. **§2's free version is refuted.** Persistent namespace colour does not deliver
   "most of brushing's correlation" on this map. It correlates under **one of
   nine overlays**, and even there it is **plurality-only** — measured on the one
   cluster where the question is non-degenerate, **30% of rows would carry a
   swatch that contradicts the province they sit on.**
2. **§1 claim 1 is FALSE**, and it is the premise §3's selection model is built
   on. `Panel` is *not* the selection; a separate positional selection already
   exists.

Per §9's first acceptance item — *"§2's free version tried first, and its result
reported before selection work was scoped"* — that is this report.

---

## 1. §1 — claims verified

All eight were `[A]`, with the guidance warning that the codebase had moved.

| # | Verdict |
|---|---|
| 1 | **FALSE** — §2 below |
| 2, 3 | TRUE — `resolve_region` at `draw.rs:549`; `panel_for` and `region_lines` both route through it |
| 4 | TRUE — `draw_hover` `:588`, `draw_blast` `:1495` |
| 5 | TRUE — `namespace_pair` has exactly one draw site, the `Overlay::Namespace` arm |
| 6 | TRUE — `city_pos` `world.rs:355`, `province_pos` `:359` |
| 7 | TRUE |
| 8 | TRUE — `fly_to_within` `draw.rs:815`, `aim_for_drilldown` `main.rs:671` |

### 1.1 Claim 1, precisely

`Panel`'s *shape* is right. Its second half — *"there is no separate selected-cell
state"* — is not:

```rust
main.rs:803   let mut selected: Option<(u16, u16)>
```

Set by a map click, by the `]`/`[` city sail, and by `--inspect`; cleared on
context switch; and per D1 it is both the map highlight and the blast subject.

**So selection is already two pieces of state:**

| | | |
|---|---|---|
| `selected` | `Option<(u16, u16)>` | a map **cell** — highlight, blast subject |
| `panel` | `Option<Panel>` | an **identity** — the open drill-down |

§3.1 prescribes introducing exactly that split. It exists. **The real problem is
different, and harder:** `selected` is *positional*. A list row carries an
identity, so brushing from a row needs `city_pos`/`province_pos` to convert — and
a position goes stale when a city moves, which A3 established happens whenever a
workload's pod plurality shifts. A selection that silently points at the wrong
province after a reschedule is worse than none.

That is a design question §3 does not address, because it did not know the state
existed. **Question 4's warning landed exactly where it predicted:** D1's lesson
was that the consumer which bites is the one not named, and here the whole
premise turns on state the claim said was absent.

---

## 2. §2 — the free version, measured

The guidance's prediction, from plan §6.2:

> Córdoba is red in the list *and* red on the map. Correlation with nothing clicked.

That assumes a **1:1 region↔category mapping**. This map does not have one, for
two independent reasons.

### 2.1 The colour is on one overlay of nine

`namespace_pair` is consumed at a single draw site — `overlay_pair`'s
`Overlay::Namespace` arm. Under the default Terrain view, and under Pressure,
Cost, Pool, Walls, Substrate, Saturation and Replicas, the map carries **no**
namespace colour at all. A swatch in a list would match nothing on screen unless
the operator had already switched to the one overlay that shows it.

### 2.2 And there it is plurality-only

`dominant_namespace(&prov.cities)` tints a province by its **plurality**
namespace. A province is a node, and nodes host pods from many namespaces, so a
minority-namespace row's swatch would contradict the province it sits on.

Measured:

```
kind    nodes hosting >1 namespace:  4/4  = 100%
        pods whose namespace is NOT their province's tint:  8/27 = 30%

churn   nodes hosting >1 namespace:  0/99 = 0%
        pods whose namespace is NOT their province's tint:  0/421 = 0%
```

**The churn figure is degenerate and must not be read as support.** That fixture
puts every workload in one namespace, so the correlation is trivially perfect and
measures the fixture. kind is the only available cluster where the question has
an answer, and there **roughly one row in three would be wrong**.

### 2.3 So it should not be built as brushing

A swatch that contradicts the map 30% of the time asserts a correspondence that
does not hold. This codebase refuses that shape everywhere else — `pool_line`
will not name the unpooled sentinel, `extent_line` stays silent for a measured
size, `pool_confinement` refuses a single-pool fleet, and the measuring
instruments refuse a degenerate dimension. Shipping a decorative-but-wrong
correlation here would contradict all of it.

**What the colour would still buy is within-list grouping** — rows of one
namespace sharing a swatch, which is real and always-on. That is a legibility
improvement, not brushing, and it should be argued on its own merits rather than
as D2's cheap path.

---

## 3. What this decides

**§2 does not shrink D2. It removes the shortcut.** The remaining scope is §3's
selection propagation in full — and §3 is written on a premise (claim 1) that is
false, so it needs revising before it is built, not adapted around.

Specifically, §3 should be re-scoped knowing that:

- the hover/commit split it proposes **partly exists**, as `selected` vs `panel`
- `selected` is **positional**, and the central question is whether brushing
  makes it identity-based — which touches every consumer of `selected`
  (highlight, blast subject, `]`/`[`, `--inspect`, IMPACT-row focus)
- §2 covers none of it

**Estimate impact:** §10 offered "half a day for §2, one day for selection if
still needed". §2's half-day should not be spent, and the selection day is now
the whole phase, with a model change §3 has not costed.

---

## 4. §8 — standing questions

**2. Unknown, or fabricated?** The question this round turns on. A namespace
swatch that matches the map only under one overlay, and only for a plurality, is
a *fabricated* correspondence — and the refusal to ship it is the same judgement
`pool_confinement` makes about a single-pool fleet.

**4. Consumers depending on an old meaning?** Flagged by the guidance as "the live
one", and it is: `selected` has at least five consumers, and §3 would redefine
what it holds. None are enumerated in the guidance because it believed the state
did not exist.

**5. Inherited claims — does the state each describes occur?** Eight inherited,
one false, and the false one is load-bearing. The guidance's own warning ("claim 1
especially — `Panel`'s shape was read many versions ago") was correct about the
risk and wrong about which half would fail: the shape was fine, the absence claim
was not.

**1, 3, 6, 7** — not engaged; no code changed.

---

## 5. Acceptance

- [x] §2's free version tried first and its result reported before selection work was scoped
- [x] Claims tagged and verified; the false one reported rather than adapted around
- [x] Failure criteria for the free version stated and measured (§2.2)
- [ ] Everything else — **not started**, pending a revised §3
