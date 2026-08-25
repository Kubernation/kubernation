# D2 — brushing: the list and the map agree

**Guidance:** `docs/kubernation-d2-brushing-guidance-rev2.md` §5–§6 — the half its
test list and mutation floor still named after §3.3's inversion landed
**Version:** 1.24.0 · **Date:** 2026-08-19

The inversion (v1.23.0) made the selection a *shareable* identity. Nothing
shared it: `workloads.rs`, `charter.rs` and `timeline.rs` contained **zero**
references to it. Now the workload table marks what the map has selected,
selecting a row marks it on the map, and a row the map cannot mark **says so**.

---

## 1. Scope, decided from the data rather than the prose

The enabling plan names "map, Annals, workload table, Charter". Checked against
what each list actually holds:

| view | rows are | brushed? |
|---|---|---|
| **workload table** | `WlRow { r: WorkloadRef, … }` — map entities, already row-hit-tested | **yes, both directions** |
| ATTENTION (sidebar) | concerns; already *writes* the selection via `focus_concern` | already connected |
| Annals | `TimelineEntry.subject: (String, String, String)` over pods, events and RS revisions | **deferred** — §1.1 |
| Charter | capability probes (verb × resource) | **not applicable** — a row is not an entity |
| Advisors | text lines; `AdvisorAction` has no row variant | **not applicable** |
| IMPACT (sidebar) | a subject's *dependents* — the subject is never in the list | **not applicable** |

### 1.1 Why the Annals is deferred rather than skipped

Its subjects are a stringly `(namespace, name, kind)` triple, and the kinds are
mixed: Deploy entries hardcode `"Deployment"`, event entries carry the involved
object's kind (Pod, ReplicaSet, Node…), operator entries carry their own. Turning
one into a selection needs a validator that parses the kind and re-resolves it
against the live store — the `oracle_investigate::validate` pattern — and would
cover only the Deploy subset cleanly.

That is a phase, not a paragraph. Recorded as deferred with its reason rather
than half-done.

---

## 2. What brushing means here, and what it deliberately isn't

**Read direction** — the row that IS the map's selection carries a parchment
wash and a left bar (`theme::SEL_ROW`). Same argument as `HOVER`: a state cue,
not a severity, so deliberately neither `CRIT`/`WARN` nor `good()` — a row must
not appear to carry a health meaning it does not have. Outside the colour-blind
funnel, because it encodes no cluster state.

**Write direction** — clicking a row makes that workload the selection, and
**does not move the camera**. That is §5's line: marking is not navigation (D4),
and this path does not set `panel_just_opened`, so D1's aim-on-open does not
fire either. Verified: `panel_just_opened` has exactly one assignment site, the
map click.

---

## 3. The refusal, which is the ordinary case and not a corner

§5 requires that a row without a map position refuse **visibly**, "rather than a
row that highlights nothing."

**A DaemonSet has no map position by design.** `world.rs:656` excludes it from
city siting — it is drawn as a road across every province it touches — and the
island-encampment fallback at `world.rs:875` is for *zero-pod* workloads only.
So a DaemonSet gets neither. Every cluster has several; on the dev cluster three
of nine rows are DaemonSets.

The row says the honest reason rather than a generic one:

```
ds   kube-system/kindnet     road - not a settlement    4/4   Complete   69d
ds   kube-system/kube-proxy  road - not a settlement    4/4   Complete   69d
ds   kubernation-demo/agent  road - not a settlement    3/3   Complete   69d
```

Clicking such a row still opens its window; it just does not claim a position.
It also **leaves any existing selection alone** — silently clearing it would be
a side effect the row's own note does not describe.

---

## 4. One source for "is this on the map"

`WlRow.placed` is `world.city_pos(r).or_else(|| world.structure_pos(r)).is_some()`
— the *same two lookups* `draw::selection_pos` derives a position from. A test
sweeps every row and asserts the row's answer and the map's answer agree, in
both directions, with guard-the-guard flags so it cannot pass on a fixture that
happens to contain only one kind.

A mutation asking a *different* question (city only, dropping the encampment)
fails that test — which is the drift it exists to catch.

## 4.1 The decision that a click carries

`WorkloadsAction::Open` carries `select: Option<Selection>`, already decided by
the pure `row_selection`, rather than a `placed` flag for `main.rs` to interpret.
`main.rs` has no test module by the v0.66.0 policy, so a decision it had to make
would sit in the one file the mutation floor cannot reach. D2-fix's finding,
applied before it bit.

---

## 5. Mutation floor — five, all caught

| | mutation | |
|---|---|---|
| M1 | **the list ignores the selection** (§6's named mutation) | caught |
| M2 | the refusal is silent (`row_note` says nothing) | caught |
| M3 | every row claims a place on the map | caught |
| M4 | `placed` asks a different question than the selection does | caught |
| M5 | a click always claims a selection, placed or not | caught |

M3 and M4 were first reported **NOT APPLIED**: `cargo fmt` had reflowed the
target across four lines, so the replacement matched nothing. The assertion
caught it — the sixth time this session, and the reason §4.1 of the D2-fix
guidance exists.

---

## 6. A dev-flag finding

`--inspect X --workloads` rendered neither the table nor a selection, and the
reason was not what I assumed twice.

The dev-flag block that opens the table sits inside
`if !had_snap && inspect.is_none()` — **skipped entirely whenever `--inspect` is
given**. My first fix added `|| inspected` to a condition *inside* that block,
which is dead code. And `--inspect` itself lives inside the nav-suspend
`else` branch, which an open table suspends — so the table cannot simply be
opened first either.

The table is now armed like `--blast` is (`workloads_armed`), fired **outside
both gates** once `inspected` says the selection is set. New gui-smoke state
`workloads-brushed`.

Both wrong diagnoses came from reasoning about the control flow instead of
reading it — the same shape as the previous round's dead-container-runtime call.

---

## 7. Standing questions

**1. Summing before comparing?** None.

**2. Unknown, or fabricated?** `placed` is a fact about the world, not a
default: an empty `WorldModel` reports nothing placed, which the sort/filter
tests use deliberately so they stay about order. `row_note` returns `None` for a
placed row rather than an empty string.

**3. Two sections constraining one behaviour?** The row's "can this be marked"
and the map's "where is this" — the divergence fixture is M4, which makes them
ask different questions and fails.

**4. Consumers depending on an old meaning?** `table_rows` gained a parameter
and `WorkloadsAction::Open` changed shape, so the compiler enumerated both
callers. `WlRow` is not public outside the crate.

**5. Inherited claims?** The scope claim — "the plan names four views" — was
checked against each view's actual row type rather than taken from the prose,
and two of the four turned out not to have entity rows at all (§1).

**6. One side of a comparison moved?** The agreement test compares the row's
answer with `selection_pos`'s; neither is derived from the other, and both are
asserted over a real `Models::build` world.

**7. Container adjacency read as world adjacency?** Row order is a sort, never
an adjacency; the mark is keyed on identity, not index.

---

## 8. Acceptance against §6

- [x] The map's mark and the list's mark agree about which entity is current
- [x] Hover does not propagate as commit — hover only highlights; only a click sets the action
- [x] No camera movement on selection (§5) — verified at the single `panel_just_opened` site
- [x] No selection for rows without a map position, **and the refusal is visible**
- [x] Mutation floor including §6's "make a list ignore it", each asserted applied
- [x] No new views, no namespace swatches
- [x] `cargo nextest run --workspace` green — 581 tests

430 core + 126 GUI tests; gui-smoke 56 states.

**Still open in the D workstream:** D3 (visual momentum — mark where you are on
the map while working in a list) and D4 (reverse indexing — a row click flies the
camera), both deferred by name here.
