# KuberNation — D2-fix: Make the Gate Passable

**Implementation guidance**
**Goal:** move the selection decisions out of `main.rs`, so a re-mirror is detectable and §3.2's third hand-rolled conversion disappears.
**Gate:** re-run D2 §3.4 step 3. **The drifting re-mirror must now fail a test.**

D2's §3.4 gate failed and said stop. This is the half-day it protected, spent on foundation.

---

## 0. Why the gate could not pass

`main.rs` has **zero `#[cfg(test)]` modules**, by the v0.66.0 testability policy: macroquad is immediate-mode + GL and not unit-testable, so the prescription is *pure draw-decision functions tested elsewhere, plus `make gui-smoke` as a crash gate*.

Both collapsed consumers live in `main.rs`. So a re-mirror there — even one that **behaves differently**, as D2 §3's form 3b demonstrated with a `Region::Structure` arm the authority deliberately lacks — has nothing to go red.

That is not a missing test. **It is the policy working as designed on code that should not be in that file.**

`subject_at` was the right move and it is not enough on its own: the agreement test pins the *authority* against `panel_for`, and nothing pins that the *consumers* call it.

---

## 1. Verify before building

Everything is `[A]` — from the D2 gate report, 2026-08-19. VOR was unavailable. **Verify each.**

| # | Claim | Source |
|---|---|---|
| 1 | `draw::subject_at(worlds, cell) -> Option<(ClusterId, Subject)>` exists and is the collapsed authority | gate §1 |
| 2 | Its two consumers are the blast subject and `oracle_scopes`, both in `main.rs` | gate §1 |
| 3 | `main.rs` contains **zero** `#[cfg(test)]` modules | gate §3.1 |
| 4 | `main.rs:2628` hand-rolls a third conversion — the IMPACT-row click handler, `Region::City → Panel::City`, dropping the `Province` arm deliberately | gate §3.2 |
| 5 | It takes an already-`locate`d **local** cell, so it cannot call `panel_for` (which takes a `Hit`) | gate §3.2 |
| 6 | `oracle_scopes` now gates on `id == ClusterId::Hot` rather than on coordinate arithmetic | gate §1 |
| 7 | `panel_for` is **not** a variant of this conversion — different input, resolves coast markers and island structures, answers a different question | gate §1.1 |
| 8 | The project has precedent for structural CI guards: the `cargo tree -p miniquad` check and the license-drift guard | gate §4 |

**Claim 7 is a correction to my own earlier draft.** Do not re-fold `panel_for` in.

**Claim 5 is the constraint on §3.** The IMPACT handler's input shape is why it was hand-rolled; whatever replaces it must accept a local cell.

---

## 2. The move

Extract the decisions to a tested crate. `main.rs` keeps a call.

### 2.1 The blast subject

The chain `selected` → `raid_subject` → focused concern is a **decision**, not rendering. It belongs beside `subject_at`, as a pure function taking what it needs and returning a `Subject`.

After the move, a re-mirror means deliberately rewriting **one line into fifteen** — a different failure mode from drift, and not one anybody reaches by accident.

### 2.2 The Oracle scope

Same treatment. Claim 6 says it now gates on `ClusterId::Hot` explicitly, which makes it a pure predicate over `(worlds, cell)` with no GL dependency.

### 2.3 The IMPACT-row handler — fold it in (claim 4)

This is the site the gate found by forcing a caller enumeration, and folding it is what makes §4's guard simple enough to stay true.

It cannot call `panel_for` (claim 5), and it *should not* — claim 7. So it needs its own home in the same module, taking a local cell and returning the `Panel::City`-shaped answer.

**Its deliberate `Province`-arm omission must survive the move**, with the reason carried to the new site. A dropped constraint that was documented at the old site and not the new one is how this codebase has lost invariants before.

### 2.4 Do not move rendering

Only decisions. `draw_selection` genuinely wants a cell and genuinely draws; it stays. The policy's line is *pure draw-decision functions tested elsewhere* — the decision moves, the drawing does not.

---

## 3. Tests

The point of the move is that these can exist at all.

- [ ] The blast subject function agrees with `subject_at` for the same cell
- [ ] The Oracle scope refuses a warm cell, asserted on `ClusterId` rather than on coordinates (claim 6)
- [ ] The IMPACT-row conversion produces `Panel::City` for a city cell and **nothing** for a province cell — pinning the deliberate omission (§2.3)
- [ ] Each new function has at least one test that fails if the function is bypassed

**That last one is the gate's requirement**, restated as an ongoing property: a consumer that stops calling the authority must break something.

---

## 4. The structural backstop

Take it, and keep it small.

A check that `region_at` has exactly the sanctioned callers. It would have caught **both** gate forms — the verbatim copy and the drifting one — where no behavioural test could catch either.

Follow the precedent of the existing structural guards (claim 8): a CI check with a named list and a clear failure message.

**Its known weakness, and the reason §2 comes first:** it is a lint pinning code *shape*, so it goes stale differently from a behavioural test. Every legitimately-added caller needs the list updated, and **a lint that fires spuriously gets suppressed rather than fixed.** Folding §2.3's site first is what keeps the sanctioned list short enough that an addition is rare and obviously deliberate.

If the list cannot be kept short, prefer to drop the guard rather than ship one people learn to ignore.

---

## 5. The gate

**Re-run D2 §3.4 step 3.** Both forms:

| | Expected after this phase |
|---|---|
| **3a** verbatim re-mirror | caught by §4's structural guard |
| **3b** drifting re-mirror (`Region::Structure` arm) | **caught by a behavioural test** — this is the one that matters |

3b failing a real test is the pass condition. If only the structural guard catches it, §2 did not move enough of the decision out.

### 5.1 Assert the mutations applied

Per D2 §3.4.1, and it has now bitten five times this session: `cargo fmt` reflowing a target so the replacement matches nothing, and the suite reporting green for a mutation never in the tree.

*Applied* means the divergent arm is present and compiling. Verify before reading the result.

### 5.2 The discrimination check

The gate report notes 3a proves nothing alone — a verbatim copy is behaviourally identical, so no behavioural test *could* catch it. **Keep reporting both**, and keep saying which is which. A phase that only reported 3b passing would be hiding that half the hazard is covered by a lint rather than by a test.

---

## 6. What this does not do

- **No inversion.** D2 §3.3 remains unstarted; this is what makes it safe to start.
- **No behaviour change.** Moving a decision must not change it — including the carved-sea divergence in gate §2, which is pre-existing and which the inversion dissolves later.
- **No `panel_for` folding** (claim 7).
- **No new tests for `draw_selection`** — it stays where it is (§2.4).

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 4:** the moved functions have callers in `main.rs` whose behaviour must not change. The gate report's §3.2 is the precedent — a caller enumeration found a site nobody knew about, and this phase should enumerate again after moving, not before.

**Question 2:** `subject_at` returns `Option`. Each moved decision must preserve what `None` meant at its old site — *no subject here*, not *a default subject*. Check each independently; they may not have agreed before the move.

---

## 8. Acceptance

- [ ] Blast subject, Oracle scope and the IMPACT-row conversion all live in a tested crate
- [ ] The IMPACT handler's deliberate `Province` omission survives, with its reason
- [ ] Every moved function has a test that fails if a consumer bypasses it
- [ ] Structural guard added, with a **short** sanctioned-caller list
- [ ] D2 §3.4 step 3 re-run; **3b caught by a behavioural test**, not only the guard
- [ ] Both mutation forms reported, with which is covered by what
- [ ] Mutations asserted applied
- [ ] No behaviour changed — including the carved-sea divergence
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 9. Estimate

**Half a day** — the same half-day D2's gate protected, which is the trade §3.4 exists to surface.

After this, D2 §3.3's inversion becomes the phase it was costed as: fifteen sites, a model change, and a mutation floor that can actually detect a regression.
