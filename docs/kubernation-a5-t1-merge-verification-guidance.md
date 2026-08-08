# KuberNation — A5/T1 Merge: Verification Pass

**Verification guidance, not implementation guidance.**
**The merge has already landed.** `NewGround::{Off, Fading, Since}` exists in core with `mark()`, and `net::marking_by_node` wires it to the draw path.

This session establishes what state it is in, closes the record, and fixes one regression the merge appears to have introduced.

---

## 0. Why this is a verification pass

T1's recommendation 1 was *"merge, don't multiply — give A5's fresh ground a fixable baseline alongside its rolling window, one feature two modes."* That is implemented:

```rust
pub enum NewGround { Off, Fading(Duration), Since(SystemTime) }
```

with a test named `the_two_modes_diverge_only_in_when_they_let_go` whose doc comment states the design conclusion verbatim — *"which is why it is a mode rather than a second overlay with its own colour and setting."*

So there is no merge to specify. What is unverified is whether T1's **removal** is complete and whether anything was lost in the transition. A merge of this shape leaves loose ends in predictable places, and §2 lists them.

---

## 1. The regression — verify first, then decide

`NewGround::mark` returns `Option<f64>`, and `marking_by_node` collects into `HashMap<String, f64>` where **absent means unmarked**.

```rust
NewGround::Since(base) =>
    matches!(changed_since(occupied_at, base), ChangeSince::Changed).then_some(1.0)
```

`ChangeSince` has **three** variants. `Changed` maps to `Some(1.0)`; `Unchanged` and `Unknown` both map to `None`.

**T1 shipped three states deliberately.** Its report:

> Absent `occupied_at` means either never-changed *or* record-predates-the-field, and neither is "unchanged". It renders as the hatch — the established no-data texture from v1.6.0 — and the panel says "no succession on record", not silence.

And `ChangeSince`'s own doc comment, still in source, says collapsing them *"would let the map report 'nothing happened here' about ground it has no record for, which is the unearned all-clear this codebase refuses."*

**The doc comment now describes an honesty the shipped path does not deliver.** The type still distinguishes three states; the renderer sees two.

### 1.1 What to verify

- [ ] Does the hatch still fire for `ChangeSince::Unknown` under `Since` mode? Trace from `marking_by_node` to the draw site — a second, separate path may still carry it
- [ ] Does the panel still say "no succession on record", or has it gone silent?
- [ ] Is there a test that would have caught this? If `changed_since_separates_unknown_from_unchanged` passes while the rendered result does not, the test is pinning the core function and not the feature

### 1.2 If it is genuinely lost

`Option<f64>` cannot carry three states. Two shapes, and the choice matters:

| | Cost |
|---|---|
| **Return a richer type** — e.g. `Option<Mark>` where `Mark` distinguishes strength from unknown | Touches `mark`, `marking_by_node`'s map value, and the draw site. Honest by construction |
| **A second map for unknown** | Cheaper, but it is two maps for one fact — exactly what `marking_by_node`'s own doc comment argues against |

Prefer the first. And note the asymmetry: **`Fading` has no unknown state to lose** — its `None` genuinely means "do not mark" for three reasons that share one honest answer. `Since` is different, because *unknown* and *unchanged* are different claims about the ground.

That asymmetry is worth a doc comment wherever it lands, because "both modes return `Option`" is what made the collapse look symmetric and safe.

---

## 2. Removal completeness

T1 shipped a ninth `Overlay` variant plus its supporting machinery. If it is now a mode, that variant should be gone. Verify each, and note that a partial removal is worse than either state.

- [ ] `Overlay::Changed` — removed from the enum, `overlay_pair`, `overlay_flat`, `Overlay::label`, `overlay_from_str`, the View menu, and the Almanac
- [ ] `theme::changed_land_pair` — removed, or retained deliberately as the `Since` mode's colour with a comment saying so
- [ ] **The fresh-ground suppression.** T1 §2 describes suppressing fresh ground under `Overlay::Changed`, because `overlay_pair` returns fresh ground *before* the overlay match. With one feature there is nothing to suppress; that code is now either dead or a contradiction
- [ ] `Net.change_baseline` — folded into `NewGround::Since`, or still a parallel field
- [ ] `panels::changed_line` — merged with the fresh-ground panel line, or duplicating it

### 2.1 The stale-pref case

`overlay_from_str` falls back on an unrecognised value. A user who selected the Changed overlay before this merge has `"changed"` persisted.

- [ ] Confirm a stale value lands somewhere sensible, and does not silently reset an unrelated setting
- [ ] Same question for `--changed-since` as a CLI flag: removed, or aliased to the new control?

This is the one item with a user-visible failure mode that no test will catch, because the test fixtures do not carry last month's prefs file.

---

## 3. Dead-`pub` audit

v1.18.0 deleted two `pub` core helpers with zero callers, on the grounds that a false contract on an unused helper is a loaded trap. The merge creates candidates for the same treatment:

- [ ] Are `freshness` and `changed_since` still called from outside `layout.rs`? If `NewGround::mark` is now their only caller, they should be private
- [ ] Is `ChangeSince` still used outside the module? If §1 resolves by enriching `mark`'s return, it may become internal
- [ ] Does `is_off` have callers beyond `marking_by_node`?

Keep them `pub` only if something outside calls them, or if a doc comment says why the contract is worth publishing.

---

## 4. The control surface

A5-render found that the ageing window had to be an **in-app** setting, because finding a workable value by restarting once per guess is not a workflow. The same reasoning applies to the merged control, and more so — it now has a mode as well as a value.

- [ ] Can a user switch between Fading and Since at runtime, and set each mode's parameter?
- [ ] Is the `Since` baseline captured at click time and **not persisted**? T1's reasoning stands: a baseline is a moment in *this* investigation, and restoring a stale one silently answers a question about a different afternoon
- [ ] Does the *mode* persist even though the baseline does not? Those are different, and a user reopening into `Since` with no baseline needs a defined behaviour — probably `Off`, or a baseline of "now"
- [ ] Is `Off` reachable and a real value, per every prior setting in this project

---

## 5. The panel half

A5-render §3.3's standard: *the overlay says which node, so something must say what changed, or the map raises a question it cannot answer.*

Under `Since`, ground stays marked indefinitely — so the panel must distinguish "changed 30 seconds ago" from "changed at some point since your baseline three hours ago." A wording that only works for `Fading` will mislead in `Since`.

- [ ] The panel line names the active mode, or is worded to be true in both
- [ ] Under `Since` it can say *when*, or says plainly that it is reporting against a fixed baseline
- [ ] The `Unknown` state is spoken, not silent (§1)

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 2 is the whole of §1**, and it is the sharpest instance yet: the type expresses unknown, the doc comment defends expressing unknown, and the value that reaches the renderer fabricates. **A type that can say "unknown" does not help if every consumer flattens it before the answer is used.**

**Question 4** applies to `Overlay`: removing a variant changes the meaning of a persisted string. §2.1.

**Question 6** is the one this merge was built to answer well — same field, two right-hand sides — and `changed_since`'s doc comment answers it explicitly. Verify that answer survived the merge, since the merge is exactly the kind of change that quietly unifies two things that were deliberately different.

---

## 7. Acceptance

- [ ] §1 verified; if the three-state honesty is lost, restored with a richer return type
- [ ] The `Fading`/`Since` asymmetry about *unknown* documented where it lands
- [ ] `Overlay::Changed` and its machinery fully removed, or retained with a stated reason — no partial state
- [ ] Fresh-ground suppression removed as dead code
- [ ] Stale-pref and stale-flag behaviour confirmed
- [ ] Dead `pub` helpers made private or justified
- [ ] Mode and parameter switchable at runtime; baseline not persisted; mode's restore behaviour defined
- [ ] Panel wording true in both modes and speaking the unknown state
- [ ] Standing questions answered
- [ ] `cargo nextest` green

---

## 8. What this session must not do

**No new features.** This closes a merge that already happened.

**No re-litigating the merge decision.** It was T1's recommendation, accepted, and `the_two_modes_diverge_only_in_when_they_let_go` documents why the second mode earns its place.

**No T2 work.**

---

## 9. Estimate

**Two to four hours**, most of it §1 and §2's removal audit. Longer if §1's three-state honesty has to be rebuilt through the draw path — which is the one item here that is a real change rather than a check.
