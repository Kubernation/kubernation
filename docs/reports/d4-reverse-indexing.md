# D4 — reverse indexing

**Brief:** `docs/kubernation-d4-brief.md`
**Follows:** `docs/reports/d3-d4-precheck.md`
**Version:** 1.25.0 · **Date:** 2026-08-19

Three items, all shipped. The Oracle's CONSULT NEXT links now mark the map; the
almanac's unmarkable cross-references **say** that they are unmarkable; and the
stale doc that claimed IMPACT selects now says why it deliberately does not.

**No camera-move conformity sweep** (§0). Each surface keeps its own answer.

---

## 1. §1 — claims verified

All six TRUE against source, re-read rather than taken from the pre-check.

| # | Verdict |
|---|---|
| 1 | TRUE — `InvestigateTarget { scope: Scope, why: String }`, validated by `validate_investigate` |
| 2 | TRUE — `oracle.rs:774`; the click site returned `OracleAction::None` |
| 3 | TRUE — `main.rs:3275` resolves through `selection_at` |
| 4 | TRUE — City/Node references mark, and land on land since v1.23.1 |
| 5 | TRUE — `sidebar.rs:56` said "fly to + select" |
| 6 | TRUE — `Selection` is `Workload | Node`; §3 turns on it |

---

## 2. Item 1 — the Oracle's CONSULT NEXT marks, and does not fly

`OracleAction::Select(Selection)`, produced by the pure `oracle::scope_selection`
and applied by `main.rs`. `Realm` and `Concern` name no single entity and yield
`None` — a concern's *target* does, but that is the attention queue's own jump
(`N`), not this one.

### 2.1 The fly/no-fly decision, recorded

**Mark without flying.** Reached independently of the brief's lean, and for a
reason the brief does not give:

A consult jump is **speculative**. You drill from the realm to a suspect and come
back — which is precisely what the reply carousel exists for. Flying on each jump
would leave the camera wherever the last guess landed, with nothing to restore
it. **A mark is one identity and is replaced by the next; a camera position is
not recoverable.**

The surface-shape argument agrees (a consult link is a directory entry, like the
workload table, not a cascade step like IMPACT), but the recoverability argument
is the one that decides it.

---

## 3. Item 2 — the almanac, settled by looking

§9 said to settle this at the almanac rather than in advance. Rendering it
settles it immediately, because **the almanac's own header already states the
contract**:

> *Entries marked > have a live example — click to fly there.*

It promises a **fly**. Harbour, gate and structure references fly. So they meet
the contract as written, and D2 §5.1's "regression" is measured against a
capability the almanac never advertised — and one that, before the inversion,
worked by storing a *sea cell*, which is the thing the inversion existed to
remove.

**So: option 2, declare it.** Those entries now read:

```
Harbor (Service)              flies, no marker  >
Gate (Ingress)                flies, no marker  >
Structure (custom resource)   flies, no marker  >
```

while City, Road, Province and Granary keep the plain chevron. Verified on the
running app.

The real cost of silence was not the missing marker but the SELECTION box: fly to
a harbour and the box goes on describing whatever was selected before. The note
is what makes that predictable.

**No third selection level** was created (§3's constraint), and no new map mark.

### 3.1 One authority, not a second rule

The split is **not** a table keyed on the legend entry's kind. `draw::markable_in`
is a *view over* `selection_at` — it builds the one-world scene the almanac
implies and asks the same function the click will ask. So the note the almanac
prints and the mark the click produces cannot disagree, and if structures ever
become selectable the note disappears on its own.

A kind-keyed table would have been three lines shorter and is exactly the drift
this project keeps paying for.

---

## 4. Item 3 — the stale doc

`sidebar.rs` documented `focus_impact` as "fly to + **select**". It now states
that it deliberately does **not** select, and why: the blast subject is
re-derived from `selected` every frame, so marking a dependent would silently
re-root the radius and you would stop walking the cascade. The omission now reads
as the constraint it is rather than as an oversight to be tidied away.

---

## 5. §5 — tests, and the non-conformity pinned

Four new tests. The one that matters most is the last:

- a consult scope becomes a selection **only** when it names one entity
- the almanac can only mark what the selection can name (swept over a real
  fixture, with guard-the-guard flags for both coast and structure)
- **IMPACT and the table answer the camera question differently** — pinned by
  asserting the *shapes* that make each possible: IMPACT's payload is a `Panel`
  with no selection in it, the table's is a `Selection` with no camera target in
  it. Both call sites live in `main.rs`, which has no test module, so the
  payloads are what can be asserted

That last test exists because two surfaces deliberately behaving differently is
exactly what a future refactor "fixes".

### 5.1 Mutation floor — four, all caught

| | mutation | |
|---|---|---|
| M1 | a consult scope never becomes a selection | caught |
| M2 | the **realm** is treated as a selectable entity | caught |
| M3 | the almanac claims every reference can be marked | caught |
| M4 | the table's click stops carrying a selection (the non-conformity collapses) | caught |

Each asserted applied — present **and compiling**.

---

## 6. §7 — standing questions

**1. Summing before comparing?** None.

**2. Unknown, or fabricated?** §3 is the phase's `None`, and it now *says*
"flies, no marker" instead of silently doing nothing — the standard
`SubstrateReport`, `GroundState::Unknown` and `extent_line` set.

**3. Two sections constraining one behaviour?** §0's non-conformity: IMPACT and
the table constrain "should acting here move the camera" in opposite directions.
The fixture where they diverge is §5's third test, which asserts the divergence
rather than resolving it.

**4. Consumers depending on an old meaning?** Re-enumerated as instructed: the
selection now has **fourteen** write sites, not twelve — the Oracle is thirteen
and D2-brushing's table click was already the twelfth. Nothing reads the writer
set; every reader consumes `selected` itself, so a new writer changes no
consumer's meaning.

**5. Inherited claims?** Six from the pre-check, all re-read at source. Claim 5's
line number had drifted, which is why the brief said to re-read.

**6. One side of a comparison moved?** `markable_in` and `selection_at` are the
same function by construction (§3.1), so the two sides cannot move apart.

**7. Container adjacency read as world adjacency?** None — legend entries are a
fixed authored list, and the sweep iterates cells by coordinate.

---

## 7. §8 — acceptance

- [x] Oracle CONSULT NEXT sets the selection; the fly/no-fly decision recorded (§2.1)
- [x] Almanac unmarkable references resolved per §3, and the choice is **visible to the user**
- [x] `sidebar.rs` corrected, with the reason for the omission
- [x] §0's non-conformity pinned by test
- [x] Standing questions answered, claims tagged
- [x] `cargo nextest run --workspace` green — 584 tests

430 core + 129 GUI tests; gui-smoke 56 states.

**Not done, deliberately** (§6): no camera-move sweep, no third selection level,
no change to IMPACT or the workload table, no D3, and nothing about plurality
siting — the pre-check's larger finding, which remains its own item.
