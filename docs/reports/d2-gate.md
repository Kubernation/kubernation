# D2 §3.4 — the staged gate, run and FAILED

**Guidance:** `docs/kubernation-d2-brushing-guidance-rev2.md` §3.4 (added by the
author to this revision)
**Date:** 2026-08-19 · **Version:** 1.22.1 · **The inversion was not started.**

§3.4 says: collapse the conversion, write the agreement test, **then run the
re-mirror mutation — and if the suite stays green, stop.** It stayed green. This
is that report, delivered at the half-day mark the estimate note anticipated
rather than at a day and a half.

---

## 1. What was built (steps 1–2)

`draw::subject_at(worlds, cell) -> Option<(ClusterId, Subject)>` is the one home
for the cell→identity conversion, and the two verbatim-identical sites now route
through it: the blast subject (`main.rs`) and `oracle_scopes`.

`oracle_scopes` changed slightly in the process and the change is an improvement
worth naming. It ran `region_at` on the **hot** world using a **scene-global**
cell, so warm selections were excluded only by coordinate arithmetic — a warm
cell's `x` lies past the hot world's width, so no continent matched and it fell
through. Same outcome; now stated, as `id == ClusterId::Hot`.

### 1.1 A correction to my own draft

Draft §3.1 called `panel_for` "a third variant of the same conversion". **False,
and I wrote it.** `panel_for` uses `locate_hit` + `resolve_region`, resolves
coast markers and island structures, and answers a different question (*what
panel does this open?*). It is deliberately not folded in. Corrected in the
guidance; the collapse folds two sites, not three.

---

## 2. The agreement test found two divergences, one unanticipated

`subject_at_is_the_one_conversion_and_panel_for_is_not_it` sweeps every cell of a
fixture world and asserts each of three outcomes actually occurs.

| | `subject_at` | `panel_for` | why |
|---|---|---|---|
| land under a city / province | same entity | same entity | agreement |
| a moored coast marker | `None` | `City` | `panel_for` resolves harbours; documented, intended |
| **carved-away sea inside a province rect** | **`Node`** | **`None`** | **not anticipated** |

The third is the v1.3.0 finding seen from the other side: `region_at` tests a
province's **rectangle**, while `resolve_region` applies the coast carving —
which it can, because `Coast` is view-side noise that core cannot consult.

**It has a live consequence today.** `selected` is `hit.land`, a raw cell, so a
click on water the shoreline carved out of a province's rectangle yields a
selection the **tooltip calls ocean** and the **blast subject calls a node**.
Pre-existing and minor; preserved here, because steps 1–2 change no behaviour.
Worth recording that **§3.3's inversion dissolves it for free** — an identity is
a node or it is not, and no ambiguous cell survives to disagree about.

The fixture needed a Service added to moor a harbour: without one the sweep
contained no coast marker and the divergence the test exists to pin went
unexercised. The guard-the-guard assertion caught that, which is the only reason
it is not still passing vacuously.

---

## 3. The gate (step 3)

Run in two forms, each checked for application per §3.4.1 — *present and
compiling*, not *a string was replaced*.

**3a — verbatim re-mirror.** The exact copy the collapse removed, restored into
the blast consumer. Compiles. **116 GUI + 430 core tests green.**

Proves nothing on its own, and is reported for that reason: a verbatim copy is
behaviourally identical, so no behavioural test *could* catch it. It establishes
the baseline the real mutation is measured against.

**3b — a drifting re-mirror.** The same duplicate plus the drift a mirror
actually acquires: a `Region::Structure` arm copied from `panel_for`, which the
authority deliberately lacks. This genuinely changes behaviour — clicking an
island structure now produces a blast subject where it produced none. Asserted
applied by compilation of the divergent arm. **116 GUI + 430 core tests green.**

**So the gate fails, and it fails on the strong form.** Not merely "an identical
copy goes unnoticed" but "a copy that behaves differently goes unnoticed".

### 3.1 Why — and it is structural, not a flaw in the collapse

`main.rs` contains **zero `#[cfg(test)]` modules**, by policy: the GUI
testability decision (v0.66.0) records that macroquad is immediate-mode + GL and
not unit-testable, and prescribes pure draw-decision functions tested elsewhere
plus `make gui-smoke` as a crash gate.

Both collapsed consumers live in `main.rs`. **No behavioural test can catch a
re-mirror in a file that has no tests to go red.** The agreement test pins the
*authority* against `panel_for`; nothing pins that the *consumers* call it.

### 3.2 What enumerating callers turned up: the collapse folded two of three

`main.rs:2628` still hand-rolls a cell→identity conversion — the IMPACT-row click
handler, `Region::City → Panel::City`. It is `panel_for`-shaped rather than
`subject_at`-shaped, and it takes an already-`locate`d local cell so it cannot
call `panel_for` (which takes a `Hit`). Dropping the `Province` arm is deliberate
and documented there.

It is nonetheless a hand-rolled conversion outside the authority, in the untested
file, and it is precisely the kind of site that drifts. I did not know it was
there until the gate made me enumerate callers.

---

## 4. What would make the gate pass

Stated, not decided — §3.4 says stop.

**(a) A structural caller assertion.** A test or CI check that `region_at` has
exactly the sanctioned callers. It would have caught **both** 3a and 3b. Cheap,
and the project has precedent for structural guards (the `cargo tree -p miniquad`
CI check; the license-drift guard). It is a lint, not a behavioural test — it
pins the shape of the code rather than what it does, and goes stale differently.

**(b) Move the decision out of `main.rs`, so there is nothing left to mirror.**
The blast-subject chain — `selected` → `raid_subject` → focused concern — is
itself a decision and could be a pure, tested `blast_subject(..)`. `main.rs` would
hold a call, and a re-mirror would mean deliberately rewriting one line into
fifteen: a different risk from drift. This is what the project's own testability
policy prescribes, and it would fold §3.2's third site in passing.

**(b) is the recommendation, with (a) as a cheap backstop.** Both are the *same*
half-day the gate was meant to protect, spent on foundation instead of inversion —
which is the choice §3.4 exists to surface, and it is the author's to make.

---

## 5. Standing questions

**1. Summing before comparing?** Not applicable; no aggregate.

**2. Unknown, or fabricated?** The agreement test's three outcomes are each
asserted to have *occurred* (`saw_city`, `saw_province`, `saw_coast_divergence`,
`saw_carved_divergence`). One fired immediately and was right to: the fixture had
no harbour, so the coast divergence was documented but unexercised.

**3. Two sections constraining one behaviour?** §3.1 (the collapse) and §3.4 (the
gate) constrain it, and they diverge exactly here: §3.1 is satisfied, §3.4 is not.
The guidance was right to separate them.

**4. Consumers depending on an old meaning?** The whole question of §3.2 — and
one does, unfolded.

**5. Inherited claims?** The false claim was **mine**, in the draft I wrote (§1.1).
Fifth consecutive session in which re-examining one of my own statements changed
the work.

**6. One side of a comparison moved?** 3a and 3b use the same suite, the same
fixture and the same authority; only the consumer changed.

**7. Container adjacency read as world adjacency?** The sweep iterates cells by
coordinate, never by container order.

---

## 6. Acceptance against §3.4

- [x] Step 1 — the conversion collapsed into `subject_at`
- [x] Step 2 — the agreement test written, and it found a divergence nobody had named
- [x] **Step 3 — the re-mirror mutation run, asserted applied, BEFORE the inversion was started**
- [x] Both forms reported, including the one that proves nothing alone
- [ ] Step 4 — the inversion. **Not started.** The gate said stop.
