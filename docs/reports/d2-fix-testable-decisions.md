# D2-fix — making the gate passable

**Guidance:** `docs/kubernation-d2-fix-testable-decisions-guidance.md`
**Follows:** `docs/reports/d2-gate.md` (the gate that failed and said stop)
**Version:** 1.22.2 · **Date:** 2026-08-19 · **No behaviour change.**

The selection decisions are out of `main.rs`. Five mutations against them now
fail a behavioural test, including **exactly the drift D2's gate used** and which
nothing could catch before. §3.2's third hand-rolled conversion is folded, and a
structural guard keeps the conversion in the one file that has tests.

**One finding is a correction to the guidance's framing**, and it is §4 below:
the hazard has two halves, and only one of them was ever behaviourally
reachable. §5's pass condition is met for that half and is **unmeetable** for the
other — for a reason that is a property of test coverage, not a shortfall in §2.

---

## 1. §1 — claims verified

All eight verified against source. Seven TRUE as written.

| # | Verdict |
|---|---|
| 1 | TRUE — `draw.rs:513` |
| 2 | TRUE for product consumers (a third caller is the D2 agreement test) |
| 3 | TRUE — **zero** `#[cfg(test)]` in `main.rs` |
| 4 | TRUE in substance, and the line number was exact. **Its stated reason was wrong** — §1.1 |
| 5 | TRUE — `panel_for(worlds, hit: Hit)` vs an already-`locate`d local cell |
| 6 | TRUE — `main.rs:477` |
| 7 | TRUE — `panel_for` goes through `resolve_region`; coast markers and structures |
| 8 | TRUE — `ci.yml:80` (miniquad) and the `license-notices` job |

### 1.1 Claim 4's *reason* was mine, and it was an inference

I wrote that the IMPACT handler "drops the `Province` arm **deliberately**". The
code's comment never says that — it explains only why *coast* rows open nothing.
"Deliberate" was my reading of a silence.

Chased down, the omission is correct for a better reason: **an unreachability
argument**, and it has three distinct cases, only one of which the old comment
covered.

| region | reachable from an IMPACT row? | why |
|---|---|---|
| Province | **no** | `affected_cell` yields `city_pos`, `structure_pos` or `coast_cell` — never bare land |
| Structure | **no** | `Affected::Workload` comes only from `workloads_on_node`, which reads pods' `node_name`; an affected workload therefore has a pod and is sited as a **city** |
| Coast | **yes** | and correctly silent: a harbour has no window, the SELECTION box describes it |

So `affected_cell`'s `.or_else(|| w.structure_pos(wr))` is **defensive and
currently unreachable**. That is reported, not changed — but it is now *pinned*,
so if the blast core ever does reach a structure, the test fails instead of a row
flying to an island and opening nothing.

This is what §2.3 asks for when it says the reason must survive the move. Copying
the word "deliberate" would have carried a rationale I had inferred; the three
cases are what the code actually relies on.

---

## 2. The move

| moved to | function | replaces |
|---|---|---|
| `draw.rs` | `blast_subject` | 14 lines of precedence chain in `main.rs` |
| `draw.rs` | `selected_scope` | 12 lines in `oracle_scopes` |
| `draw.rs` | `city_at` | the §3.2 hand-rolled conversion |
| `panels.rs` | `impact_panel` | its `Panel`-shaped wrapper, beside `panel_for` |

**The compiler supplied the evidence that the decision left:** `main.rs` no
longer imports `kubernation_core::state::world::Region`. The file that used to
convert map cells into entities no longer knows what a region *is*.

Each has exactly one production caller, all in `main.rs`, all thin
(§7 question 4's enumeration, run after the move as instructed).

Two rules got sharper in passing, without changing behaviour:

- `selected_scope`'s hot-only rule is now an **exhaustive match on `ClusterId`**.
  It used to hold by arithmetic and would not have survived two worlds of equal
  width.
- `blast_subject`'s empty-queue case is `checked_sub(1)?` rather than
  `attention[idx.min(len - 1)]`. Same outcome, but it *expresses* "no focused
  concern" instead of relying on a guard elsewhere (§7 question 2).

`draw_selection` was not touched (§2.4): it wants a cell and it draws.

---

## 3. The tests, and the fixture change that made them possible

Three new tests (§3's checklist), plus a fixture change that is the load-bearing
part of this phase:

**The fixture had no island structure**, so adding a `Region::Structure` arm to
the conversion — *the exact drift D2's gate 3b used* — changed nothing
observable and no test could see it. `probe_fixture` now carries a zero-pod
workload, which is sited as an encampment.

That turns the structure case into a second asserted divergence between
`subject_at` and `panel_for`, counted **separately from the coast case** — a
single flag covering both would let either vanish from the fixture unnoticed.

### 3.1 Mutation floor — all five caught

Each asserted applied per §5.1: the divergent arm present **and compiling**, not
a string replaced.

| | mutation | result |
|---|---|---|
| M1 | `subject_at` gains the `Structure` arm — **gate form 3b, at the authority** | **caught** |
| M2 | `city_at` gains the `Structure` arm | **caught** |
| M3 | `blast_subject`: a running drill outranks the selection | **caught** |
| M4 | `blast_subject`: empty-queue guard removed | **caught** |
| M5 | `selected_scope`: a warm selection is consultable | **caught** |

Before this phase, **none** of that logic had a test of any kind.

---

## 4. §5 — the gate re-run, and a correction to its framing

Both forms re-applied in `main.rs`, both asserted applied by compilation.

| | behavioural suite | structural guard |
|---|---|---|
| **3a** verbatim re-mirror | green — did not catch it | **caught** |
| **3b** drifting re-mirror (`Structure` arm) | green — did not catch it | **caught** |

§5 says 3b must be caught by a behavioural test, and that if only the guard
catches it, "§2 did not move enough of the decision out."

**That diagnosis does not hold here, and the reason matters.** The hazard is two
different things wearing one name:

- **Drift** — someone edits the conversion. Now caught behaviourally: M1 and M2
  are the *same* `Structure` arm as 3b, at the place the decision now lives.
- **Re-mirror** — someone writes a *second copy* in `main.rs`. This cannot be
  caught behaviourally **by any arrangement**: a test cannot observe code in a
  file that has no tests, and `main.rs` has none by policy.

There is no further move available. The *call* has to live in the render loop,
and any re-mirror is by definition new code in that file. Moving more would only
relocate which line gets re-mirrored.

So: **§5's pass condition is met for drift and unmeetable for re-mirror.** I am
reporting both readings rather than picking the flattering one — the phase's
honest result is that the drift half moved from *uncovered* to *covered by five
tests*, and the re-mirror half moved from *uncovered* to *covered by a lint*.

### 4.1 The discrimination check (§5.2)

Reported both forms and which is covered by what, per §5.2. And the guard was
checked in both directions — it **passes** on the clean tree and **fails** on
each mutation. A guard only ever observed passing is not evidence.

3a is still reported despite proving nothing on its own: a verbatim copy is
behaviourally identical, so no behavioural test *could* catch it, and omitting it
would hide that half the hazard rests on a lint.

---

## 5. §4 — the structural backstop

`hack/check-conversion-authorities.sh`, wired into `make lint` and CI beside the
miniquad and license-drift guards (claim 8).

**The sanctioned list is one file, not a list of functions.** After folding
§3.2's site, every production `region_at` call is in `draw.rs` — `subject_at`,
`city_at`, `resolve_region`, all three authorities, all tested. The guard reads
production code only (everything above a file's `#[cfg(test)]`).

§4 warns that a lint which fires spuriously gets suppressed rather than fixed,
and that if the list cannot be kept short the guard should be dropped. One entry
is as short as it goes, and it does not churn per-function. The script says so,
and says to delete it rather than grow it.

Framed as **confinement to files that are under test**, the two halves of §4
compose: drift inside `draw.rs` is behaviourally catchable, and a copy outside it
is what the guard catches.

---

## 6. §7 — standing questions

**1. Summing before comparing?** No aggregate in this change.

**2. Unknown, or fabricated?** Checked independently per the guidance, and they
had **not** all agreed before. `blast_subject`'s `None` (no subject — the banner
says so) and `selected_scope`'s `None` (no scope pushed; Realm remains) were both
faithful. `impact_panel`'s `None` leaves `panel` untouched, as the old `if let`
did — the camera still flies. The one that improved is the empty queue: `len() - 1`
was safe only because a separate `is_empty()` check stood in front of it; it is
now expressed in the same expression that uses it.

**3. Two sections constraining one behaviour, and a fixture where they diverge?**
§2.3 (fold the site into a shared home) and §6 (no behaviour change) both
constrain `impact_panel`, and they diverge on the **structure** cell: folding it
next to `panel_for` invites giving it `panel_for`'s `Structure` arm — which would
arguably be an improvement — and §6 forbids it. Old behaviour kept and pinned, so
the question is visible rather than silently resolved.

**4. Consumers depending on an old meaning?** Enumerated after the move, per the
guidance's instruction. One production caller each, all in `main.rs`. Nothing
depends on an old meaning because no meaning changed — the compiler's dropped
`Region` import is the check that the old code is gone rather than merely
bypassed.

**5. Inherited claims?** All eight were mine, from a report written the same day.
Seven held; claim 4's *reason* did not (§1.1). **Sixth consecutive session in
which re-examining one of my own statements changed the work** — and this time
the claim was hours old, which is the useful part: recency is not verification.

**6. One side of a comparison moved?** The agreement test compares `subject_at`
against `panel_for` and neither changed. The **fixture** grew, which is a change
to what the comparison covers rather than to either side — and it is the reason
the comparison now discriminates M1. Stated plainly: the earlier run of that test
covered less than its name implied.

**7. Container adjacency read as world adjacency?** The cell sweeps iterate by
coordinate. `attention.get(concern_idx.min(last))` is genuinely positional, but
that index is the app's own queue cursor, not an inferred adjacency.

---

## 7. §8 — acceptance

- [x] Blast subject, Oracle scope and the IMPACT-row conversion all live in a tested crate
- [x] The IMPACT handler's `Province` omission survives — **with a corrected reason** (§1.1)
- [x] Every moved function has a test that fails when it is mutated (M1–M5)
- [x] Structural guard added, sanctioned list of **one file**
- [x] D2 §3.4 step 3 re-run; **3b's drift caught behaviourally** at the authority (M1/M2), and 3b as a `main.rs` re-mirror caught by the guard — with §4's correction to the framing stated, not elided
- [x] Both mutation forms reported, with which is covered by what
- [x] Mutations asserted applied (present **and** compiling)
- [x] No behaviour changed — the carved-sea divergence is preserved and still asserted
- [x] Standing questions answered, claims tagged
- [x] `cargo nextest run --workspace` green — 574 tests

430 core + 119 GUI tests; gui-smoke 55 states; fmt, clippy, actionlint and
shellcheck clean.

**D2 §3.3's inversion is now safe to start**, which is what this phase was for.
