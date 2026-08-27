# Plurality siting — the two false claims

**Guidance:** `docs/kubernation-plurality-false-claims-guidance.md`
**Follows:** `docs/reports/plurality-siting-precheck.md`
**Version:** 1.26.0 · **Date:** 2026-08-19

Both surfaces corrected. **The gate passes on `churn/api` — 120 pods across 65
nodes — and the discrimination check passes on a concentrated workload**, which
is what proves the fix is not conditional on being spread.

Not a map change: no new mark, no siting change, no road treatment.

---

## 1. §1 — claims verified

All eight TRUE, re-read at source rather than taken from the pre-check. Claims 1
and 3 were checked verbatim, since they are the exact strings being edited.

---

## 2. Item 1 — the Legend, and one authority instead of an anti-drift test

The two pages disagreed because **nothing compared them**. §5 asks for a test
that compares them; this codebase's stronger pattern is to make them unable to
disagree, so `almanac::SITING_CLAIM` is now the single phrase both pages are
built from, with `city_legend_text()` and `world_siting_text()` exposing them to
a test.

The test still exists, and it does more than compare: it names the specific
falsehood (`"most of its pods"`) so it cannot return by paraphrase.

### 2.1 §2's second question — stating the frame

§2 asks whether the Legend should also say that the chosen node is otherwise
meaningless, and to decide deliberately.

**Decided: say what the position IS, not that the tie-break is arbitrary.** The
entry now reads:

> *"…sited on the province holding the plurality of its pods — often a small
> minority of them, since the scheduler spreads. **The city marks where the
> workload is DRAWN, not where it runs**; its SELECTION box gives the real
> footprint."*

The tie-break's arbitrariness is a detail of *which* node wins among equals. The
thing an operator can actually be misled by is much bigger — that the position
means anything about location at all — and that sentence covers both. Naming the
hash tie-break as well would spend a line on the rarer case while stating the
common one less clearly.

---

## 3. Item 2 — the borrowed attributes

Qualified, not dropped, per §3.2 — the lines correctly describe the province, so
attributing them keeps them useful when a workload *is* concentrated.

```
api
deploy churn . pop 120/120
120 pods across 65 nodes          <- what the city stands for   (spread_line)
on province churn-edge-g1-013     <- whose the lines below are  (spread_qualifier)
grid D2
```

Both are pure draw-decision fns, unit-tested. The qualifier is **unconditional**:
the wording never depends on how spread a workload is, which §6.1 is the check
for.

### 3.1 §3.3's footprint line, and where its numbers come from

`CityPod.node` was the guidance's suggested source, but the SELECTION box holds
the map's `City`, not the city window's model — deriving it there would mean
running `build_city` inside a tooltip.

The right source was already in hand: `build_world`'s `pods_by_workload_node`,
**the very census that chooses the plurality**. `City.spread` is computed there,
so the city's position and the footprint it stands for cannot disagree.

§8's question 2: a city always has at least one placed pod (`city_home` needs one
to site it), but an empty spread still *says* `footprint not known` rather than
printing `0 pods across 0 nodes`, which would read as a measurement of nothing.

### 3.2 The budget, checked rather than assumed

§3.3 and §4 both warn that estimates about this panel have been wrong. The
footprint line is in the **SELECTION column**, not the pod list, so
`row_char_budget` is untouched — and the test asserts that (32 chars, unchanged)
rather than leaving it implied. The longest possible footprint line is also
asserted to fit the 264px column.

---

## 4. §6 — the gate

**Failure criteria, stated before the run** (§6.2): the box implies a host node
for a spread workload; the province's attributes are dropped entirely; the
footprint line pushes a row over budget; the two almanac pages still disagree.
None occurred.

**Gate — `churn/api`, 120 pods across 65 nodes, plurality node holding 5:** the
box is quoted in §3. Nothing in it is readable as *"this workload runs on this
node"*: the footprint is stated first, and every line after the qualifier is
visibly the province's.

**§6.1 discrimination — a concentrated workload on kind**, plurality 100%:

```
web
deploy kubernation-demo . pop 3/3
3 pods on 1 node
on province kubernation-worker
grid B0
```

Same shape, same attribution, singular grammar. A fix that only read correctly on
the churn fleet would have moved the problem rather than fixed it.

---

## 5. §5 — mutation floor, and one that survived

| | mutation | first run | after |
|---|---|---|---|
| M1 | the Legend says "most" again | caught | caught |
| M2 | the province attribution is dropped | caught | caught |
| M3 | the footprint line counts pods where it should count nodes | caught | caught |
| M4 | the spread is **not** taken from the census that sited the city | **SURVIVED** | caught |

**M4 is the round's process finding**, and it is the same shape as D2's M-D one
phase earlier: the GUI test pinned the line's *shape* against a fixture whose
city has one pod on one node, so a wrong source coincided with the truth.

Closed with a **core** test — the right home, since the computation is in
`world.rs` — using a workload with `desired 3, ready 1` and 3 pods across 2
nodes, so neither `ready` nor a hardcoded `1` can stand in. It carries
guard-the-guard assertions saying exactly that.

---

## 6. §8 — standing questions

**1. Summing before comparing?** `by_node.values().sum()` is a pod count and
`by_node.len()` a node count; they are reported separately and never compared.
M3 is the mutation that conflates them.

**2. Unknown, or fabricated?** §3.1 — an empty footprint is said, not printed as
zeroes.

**3. Two sections constraining one behaviour?** Item 1, and it is now one
section: both pages are built from `SITING_CLAIM`.

**4. Consumers depending on an old meaning?** `City` gained a field; the compiler
found both test literals that construct one. Nothing reads `City` expecting the
old shape, and no existing line's meaning changed — the province attributes still
say what they always said, they are now attributed.

**5. Inherited claims?** Eight, all from a pre-check written the same day, all
re-read. The two being edited were checked verbatim.

**6. One side of a comparison moved?** The footprint and the siting come from the
same `pods_by_workload_node`, so they cannot move apart — which is what M4 exists
to enforce.

**7. Container adjacency read as world adjacency?** None.

---

## 7. §7 — the question this leaves open, posed and not answered

> **With both false claims corrected, does anything still let a user conclude
> something false about where a workload runs?**

Deliberately not answered here. The pre-check's four map-shaped candidates remain
recorded and unchosen; if the answer is *no*, the pre-check's last row becomes
true — the city is a label, not a location claim — and the item closes. If *yes*,
the residual is a far smaller target than "cities are imprecise".

---

## 8. §9 — acceptance

- [x] Legend corrected, and the two pages verified to agree — by construction (§2)
- [x] §2's second question decided deliberately, with the reason (§2.1)
- [x] Province attributes attributed to the province, unconditionally (§3)
- [x] Footprint line added and costed against `row_char_budget` (§3.2)
- [x] Gate run on `churn/api`, with the concentrated-workload discrimination check (§4)
- [x] Failure criteria stated before the run
- [x] Mutations asserted applied; **one survived and was closed** (§5)
- [x] §7's question posed, not answered
- [x] Standing questions answered, claims tagged
- [x] `cargo nextest run --workspace` green — 589 tests

431 core + 133 GUI tests; gui-smoke 56 states.
