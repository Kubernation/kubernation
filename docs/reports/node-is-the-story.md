# The node is the story — and a shell convention

**Prompt:** "KuberNation — The Node Is the Story" (2026-09-03).
**Version:** 1.38.0.
**Gate:** on the churn fleet, the allocatable-less node is shown **tagged in
every row**, not as three unexplained entries — **PASSED**, with the
discrimination check and no failure criterion fired.

Two items: the fix the Substrate gate found, and a convention for the class of
instrument that keeps failing. The round's finding is a **v1.37.0 regression
that could not render**, exposed by the fix.

---

## 1.1 — the claims, checked before building

All six re-read; all six **TRUE**. Claim 4 was checked first, because a
backfilled fixture would have made the whole gate vacuous.

| # | Claim | |
|---|---|---|
| 1 | The tab tags NotReady nodes per-node, from `NodeTile` | TRUE — `substrate_rows` takes the set, the row renders `(NotReady — the node is the story)` |
| 2 | `NodeTile` carries allocatable as `Option` per resource; absent ≠ zero | TRUE — `node_allocatable` returns `Option`, and the request pair is `None` per resource |
| 3 | One allocatable-less node, `sys` pool index 0 | TRUE — `churn-sys-g2-000`, and `lib.sh:131` still builds it that way |
| 4 | **kwok backfills fake capacity unless the fixture supplies a Ready condition** | TRUE, and **the fixture is intact**: the node reports `allocatable: {}` *and* `capacity: {}` after 28 days and several regenerations. Re-checked again at gate time, as step 0 of the run |
| 5 | `missing_by_node` is keyed by node; the tab inverts by DaemonSet | TRUE |
| 6 | The Almanac has a "why a node shows gaps" list | TRUE — new nodes, drained/GC'd nodes, NotReady |

**Claim 8 — true, and NOT sufficient.** "The tab already tags NotReady this
way" is true, and the *row* has room for a second tag (it is a trailing
parenthetical). The **type** did not: `missing: Vec<(String, bool)>` is one
bool, so it can carry one reason and cannot express a node that is both. That
is the shape the sufficiency check was for.

---

## 1.2 — read, don't add: the predicate already existed **twice**

The fact needs no new field. It needs a name, because it was already being
asked in two places and the tab would have been a third:

| where | how it was written |
|---|---|
| `attention.rs:579` | `tile.cpu_ratio.is_none() && tile.mem_ratio.is_none()` → the "capacity not reported — load unknown" concern |
| `draw.rs:355` | `worst_known(cpu_ratio, mem_ratio).is_none()` → the Pressure overlay's hatch |

Identical predicates: `worst_known` returns `Some` when *either* is known, so
its `is_none()` is "neither". Both now call **`NodeTile::capacity_unreported()`**,
which is the read the prompt asked for, named once so the three surfaces cannot
drift. `panels.rs`'s prose, which named the old spelling, follows.

It reads the **derived** pair deliberately: `cpu_ratio` falls back to requests
when there is no metrics sample, so `None` means *the allocatable key is
absent*, never *metrics-server is down*. **Partial reporting is not this** — a
node publishing cpu but not memory is `false`, because it has a denominator on
one axis and the scheduler can still place on it, which is exactly why
`worst_known` calls it measured. That boundary is the one thing about the
predicate that could be got wrong silently, so it is its own test.

---

## 1.3 — the decision, stated: **counted and tagged**

A tagged node is counted like any other. `missing from 3` means three nodes,
one of which happens to be the story.

Taken as the prompt prefers, and for its reason: excluding it would make the
`on N / total` column disagree with `kubectl`, and the substrate rounds were
built on the tab, the overlay and the census naming the same nodes. **The tag
explains the count; it must not alter it.** Recorded on `SubstrateRow::on`, and
pinned by a test asserting both halves — tagged in every row, and still counted.

---

## 1.4 — two reasons, one symptom

`NodeTrouble { not_ready, no_capacity }` — two flags, not an enum, because a
node can be **both**. The note is composed rather than tabulated, so the
both-case cannot be forgotten:

- `(NotReady — the node is the story)`
- `(reports no capacity — the node is the story)`
- `(NotReady, reports no capacity — the node is the story)`

Deliberately not collapsed into "unschedulable": an operator triages them
differently — a NotReady node may come back on its own, one publishing no
capacity stays empty until something is fixed. The Almanac says so; the tag
names the fact and the field guide explains it.

---

## 2 — THE FINDING: a v1.37.0 regression that could not render

The first gate capture showed the tagged rows **losing their indentation** and
reading as top-level headings, while untagged rows kept theirs.

Cause, confirmed by running the expression rather than reasoning about it:
v1.37.0 made `Dim` advisor lines **wrap** so a caveat would not be cut at
"…beca…". `almanac::wrap` splits on `split_whitespace()` and rejoins with single
spaces, so it strips a leading indent and collapses column spacing. Proof:

```
in : "    churn-sys-g2-000   (reports no capacity)"
out: "churn-sys-g2-000 (reports no capacity)"
```

**The regression shipped in v1.37.0 and was unobservable there.** `Dim` meant
"prose" *and* "a dimmed node row"; the only dimmed row that existed was a
NotReady node — and **kwok cannot hold a node NotReady** (the A-pre finding), so
no fixture on either cluster could render one. Adding a second reason to dim a
row is what made it appear.

Fixed with `is_prose(line, role) = Dim && !line.starts_with(' ')`: prose wraps,
a row truncates, and the indent is the discriminator because it is already what
makes a line a row here. Pinned by a test that also asserts the underlying
property — that `wrap` would strip that row's indent — so the fix cannot be
undone by changing `wrap` either.

---

## 3 — tests and the mutation floor

Six new tests. `make lint` was run **before** any count was quoted, per the
v1.37.0 finding that a green suite does not say which tests ran.

| | mutation | |
|---|---|---|
| N1 | the join reads readiness for the capacity flag (**the prompt's stated one**) | CAUGHT |
| N2 | both reasons collapse to one word | CAUGHT |
| N3 | a troubled node is excluded from the count (§1.3's rejected option) | CAUGHT |
| N4 | only one row carries the tag | CAUGHT |
| N5 | the predicate fires on partial reporting | CAUGHT |
| N6 | the field guide drops the case | CAUGHT |

Each asserted applied (single occurrence, replacement present and compiling).

---

## 4 — the gate

### Failure criteria, stated before the run

1. The node appears untagged in any row.
2. The tag collapses NotReady and no-capacity into one word.
3. The `missing from` counts disagree with `kubectl`.
4. The fixture turns out backfilled and the gate passed on nothing.

**None fired.** A fifth was added *at* the run, having fired: a row rendered
without its indent (§2) — found by looking at the capture, not the data.

### Baseline

`churn-sys-g2-000` appears in **all three** rows, each tagged and indented
under its DaemonSet; `on` reads 98 / 99 / 98, matching the headless report and
`kubectl` (log-agent ready 98, node-agent 99, node-exporter 98).

### Discrimination — the tag is reading capacity

`MODE=give` patches capacity onto the node. The DaemonSet pods land, and:

| | baseline | given |
|---|---|---|
| `log-agent` | 98/100, missing `sys-g2-000` **tagged** + `sys-g2-001` | 98/100, missing `sys-g2-000` **untagged** + `sys-g2-001` |
| `node-agent` | 99/100, missing `sys-g2-000` tagged | **100/100, missing from 0** |
| `node-exporter` | 98/100, missing `edge-g1-000` + `sys-g2-000` tagged | 99/100, missing `edge-g1-000` |

So the tag disappeared, the gaps *caused by* the missing capacity became
non-gaps, and `log-agent`'s gap — caused by a hostname affinity, not by
capacity — **remained, now as a real untagged gap**. If the tag were reading
anything else it would have survived. `MODE=restore` returns the exact baseline
set, verified in the same run.

---

## 5 — the shell convention (§2)

`hack/README.md` now carries three rules, with the three v1.37.0 failures as
their evidence: **functions not command variables**; **`set -euo pipefail`**;
and **assert the fixture changed before you photograph it**.

**Applied to this round's instrument, and it fired on its author.** The gate
script's first draft waited on `desiredNumberScheduled` after giving the node
capacity. That field counts nodes matching a DaemonSet's *affinity* and never
moves when capacity changes — so it timed out and **refused to capture**, rather
than photographing a world that had not reached the state the caption would
claim. The right signal is `numberReady`. Rule 3 caught a wrong instrument on
its first outing, which is the argument for the rule.

Two more things the convention caught while writing, both by reading:

- `restore` resolved its node by name-sort ("first `sys` node alphabetically").
  Node names carry a generation token a rolling refresh rewrites, so that is not
  stably index 0. Now resolved by the fixture's own labels.
- kwok has no kubelet, so nothing evicts the pods that land during the capacity
  window; `restore` deletes them, or the fixture would look restored while the
  node stayed covered.

Committed as `hack/churn/scenarios/9-node-capacity.sh` (`MODE=give|restore`), so
the discrimination is re-runnable rather than ad hoc. Not swept across `hack/`,
per §2.2.

---

## 6 — standing questions

**2 — unknown, or fabricated?** §1.4. Two reasons, distinguishable, both
carriable, and neither collapsed into a word that would lose the triage
difference. The predicate itself refuses the fabrication in the other direction:
partial reporting is not "no capacity".

**5 — inherited claims?** All six from §1.1, checked; claim 4 was the one whose
staleness would have made the gate meaningless, and it is checked twice — once
before building, once as step 0 of the gate run.

**8 — true and sufficient?** The NotReady-tag pattern: true that it exists, and
the row had room. The *type* did not (§1.1), which is why this round changed a
`bool` into two flags rather than adding a second set.

---

## 7 — what was not done

- The `hack/` sweep (§2.2 says not to).
- The both-reasons tag has no live capture: kwok cannot hold a node NotReady, so
  that combination is unit-tested only — the same limit that hid §2's regression.
- `wrap`'s whitespace behaviour is unchanged; only who is wrapped changed.

**Counts:** 648 workspace tests (642 at v1.37.0), `make lint` green before
quoting them, gui-smoke 59, clippy clean with and without features, 0 broken doc
links. The churn fleet is as found: `node-agent` only, `churn-sys-g2-000`
allocatable-less.
