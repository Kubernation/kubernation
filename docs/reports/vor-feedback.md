# VOR — field notes from first use

> **Re-checked twice on 2026-08-29 after two VOR updates. §1 and every item in
> §4 that was real are addressed — and two of my §4 nits were not real. See §0
> and §0.2.**

**Date:** 2026-08-29 · **Project:** Kubernation (Rust, 2-crate workspace)
**Context:** first session using VOR, during the panel-wording pass. rust-analyzer
`running`, call edges at 0.95.

Written for the VOR session. Findings are ordered by how much they would change
what VOR does, not by severity to me.

---

## 0. Re-check after the update — the headline finding is fixed

Same query, same symbol, verified line by line against the file.

```json
{ "call_sites": [446, 478, 524],
  "caller_start_line": 277,
  "reference_kind": "calls",
  "qualified": "…attention::build" }
```

Both asks from §1 are in:

- `call_line` → **`caller_start_line`**, so the number can no longer be misread
  as a call location.
- **`call_sites`** lists every call inside that caller — and it includes **478**,
  which is `and_then(Agg::primary)`, the path reference that has no call node,
  that my grep missed, and that tree-sitter alone cannot see. One query now
  returns exactly the three lines a signature change has to edit.

**Checked, not taken on trust.** All five call-site claims across three files
were read back against the source and every one is exact: `attention.rs` 446 /
478 / 524, `panels.rs` 438, `advisor.rs` 585, and the test's three uses at 2234 /
2243 / 2244. The instrument now says something specific, and the specific thing
is right.

**§4's file-row is explained rather than removed**, which is the better fix. It
now carries `reference_kind: "imports"` with a path as its `qualified` — it is
the `use` statement, correctly attributed, and a reader can tell it from a call.
`reference_kind` is new and does that work generally.

**Residual nit, minor:** that import row's `caller_start_line` is `1`, while the
`use` is at line 8. Defensible — the node IS the file, and files start at line 1
— but a reader who has just learned `call_sites` are exact may read `1` as a
claim about the import's position.

**Unaddressed, and it was explicitly minor:** `find_symbol` still has no `kind`
filter (§4).

### 0.2 Second re-check — the remaining items, and two nits that were wrong

**The import row is fixed, and my nit about it was mistaken.** It now carries
`call_sites: [2217]`. I had written that the `use` was at line 8 and that `1` was
therefore imprecise. Line 8 is `use …cost::{self, CostBasis, NodeCost}` — it
imports the `cost` **module**, not `idle_meaning`. The only line that imports
`idle_meaning` by name is 2217, a function-local `use` inside a test, and that is
exactly where VOR points. It was right when I flagged it and it is more precise
now; the error was mine, and it was an assumption I had not checked.

**`find_symbol`'s `kind` filter works and always existed.** `name=primary,
kind=function` returns 2 rows instead of 3, dropping the test whose *name*
merely contains "primary". My §4 said a kind filter "would have saved a step",
implying absence. It is in the schema and I did not use it.

**Regression check:** `Agg::primary` still returns `call_sites: [446, 478, 524]`
with `caller_start_line: 277` and `reference_kind: "calls"`.

**`vor_impact` does NOT carry `call_sites`, and should not.** Its rows are
transitive dependents, not callers — only a depth-1 row could have a call site at
all. Noting it so nobody "fixes" it into a field that would be empty or
misleading for most rows.

**Spot-checked `vor_impact` for invention, and found none.** On `Agg::primary` it
reports 37 dependents at `risk: HIGH`, reaching the GUI crate at depth 3. I read
one far row back: `net::build_carrying` → `Models::build_with` →
`attention::build` (model.rs:2162) → `Agg::primary`. Three hops, two crates,
correct. The `provenance` split (29 corroborated / 7 cross-file-only / 1 local at
0.6 from tree-sitter) matched: that low-confidence row is a real test.

### 0.3 What this says about the report, not the tool

Of my four §4 items: one was real and is fixed (the unlabelled file row), one
was already addressed before I wrote it (`reference_kind`), and **two were simply
wrong** — the import line and the `kind` filter. §1's practical complaint was
real, but its framing was unfair (§0.1).

So a majority of my smaller criticisms did not survive being checked. I would
weight this report accordingly: **§1 and the file-row are worth acting on and
were; the nits should be read as things I got wrong, kept here because deleting
them would hide the correction.**

### 0.1 One correction to this report's own framing

When I first ran these queries the tool's description **already documented**
`caller_start_line`, `call_sites`, `reference_kind`, and even
`and_then(Foo::bar)` as its worked example — while the responses carried none of
them. So the description was ahead of the server.

That cuts both ways, and both halves are worth saying. My §1 complained that VOR
"answers a different question", when the description said plainly it answered at
symbol granularity and named the field that would close the gap — I should have
read it more carefully before writing. And the practical complaint still stood,
because the fields genuinely were not in the responses.

It is also, exactly, the defect class this project spent the day on: **prose
describing behaviour the code does not have.** Nothing compiles a tool
description either. Worth a check on VOR's side that the documented response
shape and the emitted one are tested against each other — that is the one place a
machine could catch it.

---

## 1. The headline: `find_references` answers a different question than a
## signature change asks

**What happened.** Earlier the same day, without VOR, I changed the signature of
`Agg::primary` in `attention.rs`. I enumerated callers with `grep 'primary()'`,
found two, and missed a third — `and_then(Agg::primary)`, a path reference with
no parentheses. The compiler caught it. I then told the user *"vor_find_references
is exactly the tool that doesn't miss that."*

**I was wrong, and VOR says so.** Run against the same symbol:

```
result_count: 1
results: [{ qualified: "…attention::build", call_line: 277 }]
coverage: { bounds: [], exhaustive: true }
```

All three call sites (446, 478, 524) are inside `attention::build`, which starts
at line 277. So VOR reported **one calling symbol**, correctly, and
`exhaustive: true` is true *of calling symbols*.

**Why that is the wrong shape for the commonest use.** The tool's own
description says "USE WHEN … scoping a refactor". Scoping a signature change
needs **call sites**, not callers: I would have opened `build`, fixed the call I
was thinking of, and still missed two. VOR would not have saved me here, and its
answer would have read as complete while doing so.

**Two concrete asks:**

1. `call_line: 277` points at the callee-caller's **declaration**, not at a call.
   A reader jumps there and finds `pub fn build(`. Either name the field for what
   it is (`caller_start_line`) or make it the first call site.
2. Consider `call_sites: [446, 478, 524]` per caller, or a `granularity`
   parameter. One caller with three calls and one caller with one call are very
   different refactors, and the current answer cannot tell them apart.

**The epistemics are otherwise excellent** — `coverage.bounds`, `exhaustive`, the
open-world framing. This is a *granularity* gap, not an honesty one, and I want
to be precise about that because the honesty is the tool's best feature.

---

## 2. What worked well

**Cross-file, cross-crate references — the case grep is worst at.**
`cost::idle_meaning` (defined in `kubernation-core`, consumed in `kubernation`)
returned all three real consumers *by enclosing symbol*: `panels::cost_lines`,
`advisor::return_idle_note`, and the test. Enclosing symbol is more useful than
grep's line hit, because it names the thing I have to reason about.

**Incremental indexing is fast and live.** `idle_meaning` and its test were
written minutes before that query and were already indexed. I did not have to
think about staleness once.

**`vor_impact` is the strongest tool here**, and the one I would keep. On
`consent_preview` it walked cross-crate to depth 3 — `freeze` → `draw_consult` →
`draw` — with per-row `layer` (`cross-file-corroborated` vs `cross-file`),
`provenance.at_risk_if_sweep_stale: 1`, an explicit `frontier_open` bound, and
`risk: UNKNOWN` rather than a false LOW. That last choice is exactly right: a
floor that reports a confident level would be worse than useless.

**`vor_get_source` with `context_lines` earned its place.** It gave me
`consent_preview`'s body *and its doc comment* without my knowing the file or
line, and the doc was where the answer lived twice today (§3).

---

## 3. Where a doc comment beat the graph — and VOR could surface it

Both defects I found today were caught by reading a **doc comment that stated a
contract the code did not meet**:

- `stream_status_line`: *"Used once tokens start arriving"* — the caller branched
  on a buffer existing, not a token arriving.
- `k8s/mod.rs` vs `adapter.rs`: two doc comments on one module, which is what
  silently broke seven rustdoc links.

`vor_symbol_summary` already returns the doc comment. A speculative but cheap
idea: surface **doc-vs-edge tension** — e.g. a doc saying "used once X" on a
symbol whose only caller is unconditional, or two doc comments attached to one
module node. I would not expect precision here; even a low-confidence flag would
have pointed me at both.

---

## 4. Smaller things

- **A file appeared as a reference.** `find_references` on `idle_meaning`
  returned a 4th row whose `qualified` is `crates/kubernation/src/panels.rs`
  (a path, not a symbol) with `call_line: 1`. Looks like a file-level node
  leaking into a symbol-level answer.
- **`find_symbol` on a common short name is noisy in a useful way.** `primary`
  returned the function, an unrelated test whose *name contains* "primary", and
  `workload_primary_container`. Documented behaviour (substring, short-name), and
  the `response_state` said so. No complaint — noting that a `kind` filter would
  have saved a step.
- **The MCP instruction says "run `vor_impact` before modifying any function".**
  Its own tool description scopes it to "function signature, struct field or
  trait definition". Most of my edits today were bodies and doc comments, where
  impact adds nothing. The two disagree, and the broader wording would train
  people to run it constantly and then ignore it.

---

## 5. How I plan to use it

Symbol-shaped questions — callers before a signature change, blast radius,
finding a definition, reading one body — go to VOR. Prose, config, workflows and
"where does this string appear" stay with grep, because a false claim in a doc
comment has no symbol to resolve. Today's prose audit found nine defects that VOR
could not have found, and one (§1) that it could have found *if* it answered at
call-site granularity.

**One honest caveat about this report.** Every finding above comes from a single
session on one Rust workspace. The `find_references` granularity gap is the only
one I would bet on generalising; the rest may be artifacts of this project's
shape — a 2-crate workspace with one very large `main.rs` that has no tests.
