# KuberNation — T-fix-2: The Correlation Rule

**Implementation guidance**
**Goal:** make the suspect rule as trustworthy as the anchor T-fix just fixed.
**Gate:** a deploy and its immediate failure, sharing a timestamp, are correlated.

T2's second prerequisite. Core work in `timeline.rs`.

---

## 0. Why this is a phase and not a note

T-fix moved the fault line to onset. The suspect rule that measures backwards from it was left untouched, and T-fix's own §3.2 found the first defect in it: **cause and effect routinely share a timestamp**, measured live at 0s / 1s / 11s, with the 0s case excluded.

Reading the rule for this document turned up a second, which nobody has reported:

```rust
let d = ftt.0.duration_since(w.0).as_secs();
(1..=CORRELATION_WINDOW_MIN * 60).contains(&d)
```

`ftt` is now **onset**. `w` is still the change entry's **`when`** — its latest occurrence. **T-fix moved one side of the comparison and not the other.**

Both defects sit in the same three lines, and both matter for the same reason: **T2's whole claim is that these correlations cluster spatially.** A correlation that fires or not for a temporally arbitrary reason produces a spatially random pattern — which will look like signal.

---

## 1. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | The suspect test is `(1..=CORRELATION_WINDOW_MIN * 60).contains(&d)` where `d = ftt.duration_since(w)` | `[V]` `timeline.rs:534–540` |
| 2 | `ftt` is `tl.first_trouble`, which T-fix made an **onset** | `[V]` `timeline.rs:512`, `anchors()` |
| 3 | `w` is `e.when` — the entry's latest occurrence, **not** its onset | `[V]` `timeline.rs:536` |
| 4 | `TimelineEntry` now carries `onset` and `onset_reported` | `[A]` T-fix §1 |
| 5 | Deploy entries take `when` from `rev.created` — an RS creation, so effectively an onset already | `[V]` `timeline.rs` deploy loop |
| 6 | Event-sourced entries take `when` from `ev.when`, the ring's latest occurrence | `[V]` same |
| 7 | `fault_line_above` picks the first shown row strictly older than `first_trouble` | `[V]` `timeline.rs:514–521` |
| 8 | `row_decisions` is shared by the GUI Annals and the postmortem export "so the screen and the exported doc can never disagree" | `[V]` doc comment |

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 9 | Kubernetes Event timestamps are **second-granularity** | The lower bound is a resolution artifact, not only a causality rule |
| 10 | A kubelet fails an image pull effectively immediately | Which is why the 0s case is common for the exact incident shape T2 is built on |
| 11 | Measured live: 0s excluded, 1s included, 11s included | T-fix §3.2 |

---

## 2. Defect A — the asymmetric comparison

`first_trouble` is an onset. The change side is a latest occurrence. So `d` is *"time from this change's most recent sighting to the incident's start"*, which is not the quantity the rule claims to measure.

**Impact depends on the change kind:**

| Kind | `when` is | Affected? |
|---|---|---|
| `Deploy` | `rev.created` — a creation, already an onset (claim 5) | No |
| `Scale` / `NodeChange` (event-sourced) | latest occurrence (claim 6) | **Yes** |
| `Operator` | the action instant, single-occurrence | No |

So a repeating `ScalingReplicaSet` event correlates from its most recent refresh rather than from when the scaling began — the same class of error T-fix removed from the anchor, surviving on the other side of the same comparison.

**Fix:** use the change's onset where available, falling back to `when` — the same resolution T-fix already built and recorded via `onset_reported`. This is a symmetry restoration, not a new policy.

**Check whether it changes the Deploy path at all.** Per claim 5 it should not, and a test asserting that is the cheapest guard that this fix does not disturb the case that already works.

---

## 3. Defect B — the lower bound is doing two jobs

`1..=600` excludes `d == 0`. The comment says why: *"a change at the exact failure instant isn't a precursor."*

That is a **causality** rule. But with second-granularity timestamps (claim 9) and an immediate kubelet failure (claim 10), it is also a **resolution** rule — and in that second role it excludes genuine causes. Measured: a deploy and its resulting `ImagePullBackOff` in the same second, not correlated.

These are different problems and the guidance must pick which one the bound is for.

### 3.1 The asymmetry that resolves it

A Deploy cannot be caused by a failure it precedes. The ambiguity at `d == 0` is **one-directional**: either the deploy caused the failure, or they are unrelated — never the reverse.

So the recommended policy:

> **Admit `d == 0` when the change is a `Deploy` or `Operator` action. Keep the exclusion for event-sourced changes.**

- Deploy and Operator are *acts*, with a known direction — a same-second failure after a deploy is the canonical incident, not a coincidence
- Event-sourced changes (`Scale`, `NodeChange`, `PodChurn`) can genuinely be *consequences* of the failure, so `d == 0` there stays ambiguous and stays excluded

This introduces no new constant. It uses a distinction the type system already carries.

### 3.2 The alternative, and why not

Widening to `0..=600` for everything is simpler and wrong in a specific way: a pod-churn event at the same instant as a failure is at least as likely to be the failure's *effect*, and flagging it as a suspect would put "preceded by" on something that followed. The rule's wording is `"preceded by", never "caused by"` — which is only honest if the ordering is real.

### 3.3 What must be said in the comment

The existing comment states a causality rationale for a bound that also does resolution work. Whatever policy is chosen, **the comment must name both jobs**, or the next reader re-derives the same confusion.

---

## 4. Tests

**Defect A:**
- [ ] A repeating `Scale` event correlates from its onset, not its latest occurrence
- [ ] A Deploy's correlation is **unchanged** — claim 5's guard
- [ ] A change with no onset falls back to `when` and behaves as today

**Defect B:**
- [ ] A Deploy at `d == 0` **is** a suspect — the T-fix §3.2 measurement as a unit test
- [ ] An event-sourced change at `d == 0` is **not**
- [ ] `d == 1` and `d == 600` remain suspects; `d == 601` does not — boundary guards
- [ ] An Operator action at `d == 0` is a suspect

**Shared authority (claim 8):**
- [ ] The Annals and the postmortem export agree on every row of a fixture containing all the above — the anti-drift test

**Mutation floor, exercised:** restore `1..=` for Deploy and confirm the `d == 0` test fails; restore `e.when` on the change side and confirm the repeating-Scale test fails.

---

## 5. The gate

**A deploy and its immediate failure, sharing a timestamp, are correlated.**

Reproduce T-fix's measurement on **kind**: induce a bad image rollout and capture `--postmortem`. T-fix measured this pair at `d == 0` and saw no suspect.

**Expected: the deploy is flagged.**

### 5.1 The contamination requirement

T-fix §5's cause 3, now a standing rule for this machinery:

> **Inducing repeated incidents to test a correlation contaminates the window the correlation reads.**

Quiesce the cluster until every prior onset has aged past the recency window before the clean run. A test that skips this produces a plausible number for the wrong reason — it would be the tenth instance in this project.

### 5.2 The discrimination check

Run the same incident with the `d == 0` admission reverted. Before: no suspect. After: suspect. The check is that **the two runs disagree** — the opposite signature from T-fix, where agreement was the pass condition, because here the fix is meant to change the outcome rather than stop something from changing it.

Because `d` is not controllable — it depends on how fast the kubelet reacts — capture several incidents and report the distribution, not one run.

### 5.3 Instrument

`--postmortem`, not a screenshot. T-fix §5's closing note is the reason: the city window truncates by character count, so suspect and non-suspect rows truncate identically. **The screen shows what was drawn, not what was decided.**

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?

**Question 4 is live.** `suspect`'s meaning changes, and claim 8 says `row_decisions` is shared by the GUI and the export specifically so they cannot disagree. Both move together; verify they do.

**A sixth question, earned this round and worth carrying:**

> **6. When a change moves one side of a comparison, does the other side still mean the same thing?**

T-fix moved `first_trouble` to onset and left `e.when` on the change side. The defect is invisible in review because the expression still typechecks and still reads sensibly — only the *quantity* changed. This is the same family as A6's re-derivation rule, one step over: an inverse needs one home; a comparison needs two sides in the same units.

---

## 7. Acceptance

- [ ] Both sides of the correlation comparison measure the same quantity
- [ ] Deploy correlation verified unchanged (claim 5)
- [ ] `d == 0` admitted for Deploy and Operator, excluded for event-sourced changes
- [ ] The comment names both jobs the lower bound does (§3.3)
- [ ] Annals and postmortem verified to agree
- [ ] Gate run on kind, with contamination quiesced and several incidents captured
- [ ] Discrimination check run — the two runs must **disagree**
- [ ] Standing questions answered, including question 6
- [ ] `cargo nextest` green

---

## 8. Estimate

**Half a day.** Three lines of policy, a test matrix, and a live gate that needs a quiesced cluster.

After this, T2's premise — *"put the Annals' existing conclusion on the map"* — is finally true. T1 remains independently available and unblocked; per the T planning doc it is still the cheaper thesis test and still the kill point.
