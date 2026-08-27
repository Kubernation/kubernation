# KuberNation — D4: Reverse Indexing

**Implementation brief** — short by design. The pre-check reduced D4 from a phase to three items.
**Follows:** `docs/reports/d3-d4-precheck.md`

---

## 0. The decision, already made

**Surfaces keep their own answer to "should acting here move the camera?" Do not force conformity.**

The pre-check found the two connected surfaces made opposite calls, each for a stated reason:

| | Flies | Marks | Why |
|---|---|---|---|
| IMPACT (sidebar) | yes | **no** | the blast subject is re-derived from `selected` each frame, so marking a dependent would silently re-root the radius — D1's review constraint |
| Workloads table | **no** | yes | marking is not navigation — D2-brushing §5 |

These are not inconsistent. **IMPACT is a cascade you walk without changing your subject; the table is a directory of things you might select.** Forcing uniformity would break one of them.

**So there is no camera-move sweep in this phase.** Whatever each new wiring does applies to that wiring alone. Changing IMPACT or the table is a separate call with D1's blast-radius constraint attached, and it is not on the table here.

---

## 1. Verify before building

All `[A]`, from the pre-check (2026-08-19). Line numbers drift; re-read.

| # | Claim | Source |
|---|---|---|
| 1 | Oracle CONSULT NEXT carries a validated `Scope::{Workload, Node}` — the same identity the selection holds | pre-check §1.2 |
| 2 | `oracle.rs:774 jump_to_scope` moves the Oracle's own cursor, re-seeds deepen, requests a consult — and never touches `selected` or the camera | same |
| 3 | An almanac cross-reference resolves through `draw::selection_at`, which returns `None` for a coast marker, gate or island structure | pre-check §1.3 |
| 4 | City and Node cross-references mark correctly since v1.23.1 | same |
| 5 | `sidebar.rs:56` documents `focus_impact` as "fly to + **select**"; D2's inversion removed the select | pre-check §1.4 |
| 6 | A selection is a **workload or a node**; coast markers and structures are not selectable by design | D2-inversion §5.1 |

**Claim 6 constrains item 2** — see §3.

---

## 2. Item 1 — wire the Oracle's CONSULT NEXT

The only surface with genuine entity rows that neither flies nor marks, and the app's most explicitly identity-carrying list.

The scope is already the identity the selection wants (claim 1), so this is a small wiring change: `jump_to_scope` should also set the selection.

**Should it fly?** Per §0, that is this surface's own call. The Oracle is closer to the table than to IMPACT — a consult link is a thing you might want to look at, not a cascade you are walking — so **mark without flying** is the consistent choice, and the one I would take. Decide and record it; do not inherit it from either neighbour.

---

## 3. Item 2 — the almanac's unmarkable references

Claim 3: coast, gate and structure cross-references fly and mark nothing.

**This may not be fixable as stated.** Claim 6 says those are not selectable by design — a selection is a workload or a node. So the honest options are:

| | |
|---|---|
| **Mark without selecting** | A transient map mark that is not a selection. New concept; check whether `draw_hover`'s mark can serve rather than inventing a third |
| **Declare it unmarkable, and say so** | The almanac tells the reader the reference has no marker, in the shape D2-brushing's `road - not a settlement` established |

**Prefer the second unless the first is genuinely cheap.** D2-brushing established that a visible, specific refusal is better than a silent nothing, and the wording pattern already exists. Inventing a third map mark to cover three reference kinds is a poor trade against a sentence.

Either way, **it must not become a third selection level.** D2 kept hover and commit distinct deliberately.

---

## 4. Item 3 — the stale doc

`sidebar.rs:56` says `focus_impact` selects. It does not, and has not since D2's inversion. The comment currently reads as evidence that IMPACT marks — which is exactly what would mislead the next reader.

Correct it, and state *why* it does not select (the blast-radius constraint from §0), so the omission reads as deliberate rather than as an oversight someone should fix.

---

## 5. Tests

- [ ] An Oracle CONSULT NEXT sets the selection to the scope's identity
- [ ] It does — or does not — move the camera, per §2's recorded decision, asserted either way
- [ ] An almanac coast/gate/structure reference produces the chosen outcome (§3), asserted, not silent
- [ ] City and Node cross-references still mark correctly (claim 4) — regression guard
- [ ] IMPACT still does **not** set `selected`, and the table still does **not** fly — §0's non-conformity, pinned so a later tidy-up does not quietly unify them

**That last one matters.** Two surfaces deliberately behaving differently is the kind of thing a future refactor "fixes". Pin it.

**Mutation floor, asserted applied.** Six false survivals this session from `cargo fmt` reflowing targets — *applied* means present and compiling, not that a string was replaced.

---

## 6. What this does not do

- **No camera-move conformity sweep** (§0)
- **No third selection level** (§3)
- **No change to IMPACT or the workload table**
- **No D3.** Closed by the pre-check, with §2 of that report as the reason
- **Nothing about plurality siting.** The pre-check's §3 finding — a city representing ~4% of a spread workload's pods — is a separate and larger item, and it is not this phase

---

## 7. Standing questions

Answer all seven; two are live.

**Question 4:** setting the selection from the Oracle adds a writer. D2-inversion enumerated twelve; this makes thirteen. Check nothing assumed the previous set.

**Question 2:** the almanac's unmarkable case is the phase's `None`. Whichever option §3 takes, it must **say** what happened rather than silently doing nothing — the standard `SubstrateReport`, `GroundState::Unknown` and `extent_line` all meet.

---

## 8. Acceptance

- [ ] Oracle CONSULT NEXT sets the selection; its fly/no-fly decision recorded
- [ ] Almanac unmarkable references resolved per §3, and the choice is visible to the user
- [ ] `sidebar.rs:56` corrected, with the reason for the omission
- [ ] §0's non-conformity pinned by test
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 9. Estimate

**Two to three hours.** Three small items; §3's choice is the only judgement, and it should be settled by looking at the almanac rather than in advance.
