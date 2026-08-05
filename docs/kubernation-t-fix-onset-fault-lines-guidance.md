# KuberNation — T-fix: Onset-Aware Fault Lines

**Implementation guidance**
**Goal:** the Annals' fault line marks when *this incident* began, not when the ring last saw the oldest chronic failure.
**Gate:** on a cluster with a chronic failure running, a deploy-then-fail incident still flags its suspects.

This fixes shipped behaviour and is T2's prerequisite. It is core work in `timeline.rs`, not a rendering pass.

---

## 0. Verify before building

**Claims are tagged.** `[V]` verified against source while writing this document. `[A]` asserted — here, from the T-pre measurement round, which established them live but which I have not re-checked against source.

| # | Claim | Tag |
|---|---|---|
| 1 | `first_trouble_is_earliest_in_window_warning` exists and names the minimum-over-window behaviour | `[V]` `timeline.rs:1070` |
| 2 | `first_trouble` = earliest entry at `Severity::Warning` or above, with a timestamp | `[A]` T-pre §2 |
| 3 | A suspect is an entry whose kind `is_change()` — `Deploy \| Scale \| Operator \| NodeChange` — falling `1..=600s` **strictly before** the fault line | `[A]` T-pre §2 |
| 4 | Deploy entries are `Severity::Info`, so a deploy can never *be* the fault line | `[A]` T-pre §2 |
| 5 | `Deploy \|\| Operator` escape the 15-minute recency window; everything else is windowed | `[A]` T-pre §2 |
| 6 | `RecentEvent` captures only `last_timestamp`; `from_event` discards `firstTimestamp` | `[A]` T-pre §5 |
| 7 | The event ring keeps only each key's **latest** occurrence | `[A]` T-pre §4, T0 |
| 8 | Subject-scope fault lines are already correct; only cluster scope is poisoned | `[A]` T-pre §4, verified live |
| 9 | `--postmortem` renders `build_timeline` + `row_decisions` as text | `[A]` T-pre §7 |

**Claim 8 bounds the whole change.** If subject scope is already right, the fix must not regress it — see §4.

**Claim 6 is the input.** If `firstTimestamp` turns out to be captured somewhere already, this phase shrinks to a policy change.

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 10 | CrashLoopBackOff's exponential backoff **caps at 5 minutes** | The collision with a 10-minute correlation window is the mechanism. Not a tuning accident |
| 11 | `firstTimestamp` is populated but **not universally** — 52/55 on kind | Absent onset is a real state, not a theoretical one. See §3.3 |
| 12 | Kubernetes may emit `eventTime` instead of `firstTimestamp` on newer events API objects | Check both before concluding onset is absent |

Claim 12 is mine and unverified — check it. Concluding "onset unavailable" when the field simply moved would silently disable the fix.

---

## 1. The defect

Shipped today, on any cluster with a chronic failure:

`first_trouble` is the **minimum** over Warning+ entries, and the ring keeps only each key's *latest* occurrence (claims 2, 7). A chronic failure therefore presents as **perpetually recent** — a mature crash-looper emits a BackOff every ~5 minutes (claim 10), the same order as the 10-minute correlation window, so it lands inside reliably and wins the anchor.

Measured, one controlled variable:

| | suspects flagged |
|---|---|
| chronic crash-looper running | **0** |
| same, scaled to zero | **2** |

The Annals draws *"── trouble begins here ──"* at a point meaning *"when the ring last saw the oldest chronic failure"*, and silently drops the correlation cue that is the section's main analytical claim.

At cluster scope, **one crash-looper anywhere poisons the anchor for every workload.**

---

## 2. The missing input

Kubernetes' Event carries `firstTimestamp` — the incident's **onset** — and the app discards it (claim 6). Onset separates chronic from acute trivially:

| object | reason | count | onset | last seen |
|---|---|---|---|---|
| crashy-…-j68lx | BackOff | ×4740 | **102.3 h ago** | 7.4 min ago |
| stuck-pvc | ProvisioningFailed | ×8386 | **44.0 h ago** | 1.7 min ago |

Capturing it is one field on `RecentEvent`, populated in `from_event`.

**Capturing it is not the design decision.** Anchoring naively on the minimum onset would drag the fault line 102 hours into the past and mark everything as after-the-trouble — worse than today.

---

## 3. The policy

### 3.1 Recommended: anchor on onset, windowed

> The fault line is the earliest Warning+ entry **whose onset falls inside the existing window**.

- Chronic failures **self-exclude** — their onset is days old, outside any window
- Acute failures qualify — onset ≈ last-seen for something that just started
- **No new threshold.** It reuses a constant that already exists and is already justified

That last point is the argument. The two alternatives both introduce something new:

| Option | Cost |
|---|---|
| Exclude chronic from the anchor | needs a *chronic* threshold — another unmeasured constant to defend |
| Anchor per-subject only | honest and cheap, but abandons cluster-scope fault lines entirely (claim 8 says subject scope is already right) |
| **Onset, windowed** | reuses the existing window; no new constant |

Record the reasoning where the policy lives, in the voice `metric_source` / `CostBasis` / `PoolSource` established — this is an inference about which failure started an incident, and it should say so.

### 3.2 What the window is

Check which window applies before choosing: claim 5 says the recency window is 15 minutes and the correlation window is 10. **They are different constants for different purposes**, and the fix needs the one that bounds "is this failure part of the current incident."

State which, and why, in the doc comment. Getting this wrong produces a fault line that is *nearly* right, which is the hardest kind to notice.

### 3.3 Absent onset is unknown

Claim 11: onset is populated 52/55 on kind. So absence is real.

**An entry with no onset must not be treated as infinitely old (silently excluded) or as now (silently anchoring).** Both fabricate. This is standing question 2 and it is live.

The safe reading, and the one consistent with this codebase: fall back to `last_timestamp` for that entry and **record that the onset was unavailable**, the way `metric_source` records which ratio you are looking at. An entry whose onset is unknown behaves as it does today — no worse, and honestly labelled.

---

## 4. Do not regress subject scope

Claim 8: a city's ANNALS already anchors correctly, because its entries are its own.

The fix is aimed at cluster scope. **Verify subject scope is unchanged**, not merely un-broken — a policy that also filters subject-scope entries could quietly drop a legitimate fault line on a workload whose own failure has been running for an hour.

Consider whether the policy should apply **only** at cluster scope. That is a smaller change with a narrower blast radius, and claim 8 is the evidence for it. If so, say why in the code rather than leaving it as an unexplained asymmetry.

---

## 5. Tests

**The mechanism:**
- [ ] A chronic Warning+ entry (onset days old, last-seen minutes old) does **not** anchor the fault line
- [ ] An acute failure (onset ≈ last-seen, both recent) **does**
- [ ] With both present, the acute one wins — **the T-pre scenario as a unit test**
- [ ] A deploy 1–10 minutes before an acute failure is flagged as a suspect, with the chronic entry present

**Onset handling:**
- [ ] Absent onset falls back to `last_timestamp` and is recorded as unavailable
- [ ] `eventTime`-only events are handled (claim 12) or explicitly out of scope with a stated reason
- [ ] Onset in the future — clock skew is real — does not produce a negative age or anchor everything

**No regression:**
- [ ] Subject scope produces the same fault line as before on every existing fixture
- [ ] `first_trouble_is_earliest_in_window_warning` still passes, or is replaced by something strictly stronger with the change explained

**Mutation floor, exercised not written:** make onset always equal `last_timestamp` — the current behaviour — and confirm the chronic-versus-acute test fails.

---

## 6. The gate

**On a cluster with a chronic failure running, does a deploy-then-fail incident flag its suspects?**

Reproduce T-pre's experiment: induce a bad rollout on kind with `crashy` running at 2 replicas, and read `--postmortem`.

**Expected: 2 suspects, with the chronic failure still emitting.** Today it is 0.

### 6.1 The instrument

Use `--postmortem` (claim 9), not a screenshot. T-pre's note is the reason: it renders the same pure functions as text, so it distinguishes *no suspect* from *cue truncated off the panel*. A screenshot cannot.

### 6.2 The discrimination check

Mandatory, per the standing requirement — eight instruments in this project have now emitted a plausible result for the wrong reason.

Run the same incident with `crashy` **scaled to zero**. Before the fix that yields 2; after the fix it should still yield 2. **The fix's signature is that the two runs now agree** — the chronic failure stops changing the answer.

That is a better check than "does it produce 2", because it isolates the variable the defect was sensitive to.

### 6.3 Where it must be measured

T0 §2.4 and T-pre §7 both hold: this must run on **kind**, not the churn fleet. kwok emits almost no events and would show nothing either way.

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and **does the state each describes actually occur?**

Question 1 is live and structural: `first_trouble` is a **minimum over a set**, and the defect is that the set includes entries it should not. That is the "aggregate then compare" shape in its purest form — the aggregation happens before the chronic/acute distinction is available.

Question 4: `first_trouble`'s meaning changes. Audit every consumer — `row_decisions`, the Annals rendering, the postmortem export, and anything computing "before the trouble."

---

## 8. Acceptance

- [ ] Onset captured on `RecentEvent`, populated in `from_event`, both field names checked
- [ ] Policy is onset-windowed with no new threshold constant, and the reasoning recorded at the definition
- [ ] Which window, and why, stated in the doc comment
- [ ] Absent onset falls back and is **recorded as unavailable**, never fabricated
- [ ] Subject scope verified unchanged, with the cluster-only decision explained if taken
- [ ] Gate run on kind with the discrimination check
- [ ] Standing questions answered, claims tagged `[V]`/`[A]`
- [ ] `cargo nextest` green

---

## 9. Estimate

**Half a day to a day.** The field capture is small; §3's policy and §4's non-regression are the work.

Independently valuable regardless of T2 — this is a defect in what ships today, and it improves the Annals immediately.
