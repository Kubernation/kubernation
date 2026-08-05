# KuberNation — T-fix: Onset-Aware Fault Lines (revision 2)

**Implementation guidance**
**Goal:** the Annals' fault line marks when *this incident* began, not when the ring last saw the oldest chronic failure.
**Gate:** with a chronic failure running, a deploy-then-fail incident still flags its suspects — at **both** scopes.

> **Supersedes revision 1**, which was stopped at §0 by its own verification step. Revision 1's claim 8 — *"subject-scope fault lines are already correct; only cluster scope is poisoned"* — is **false**. It was my generalisation from one incidental observation, and two sections were built on it, including an acceptance criterion that would have required preserving the bug. See §1.1.

This fixes shipped behaviour and is T2's prerequisite. Core work in `timeline.rs`, not a rendering pass.

---

## 0. Verify before building

`[V]` = verified against source this round. `[A]` = asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | `first_trouble` is computed **inside `build_timeline`** (a local binding, not a free function) as `entries.iter().filter(severity >= Warning && when.is_some()).filter_map(when).min_by(...)` | `[V]` `timeline.rs:397–401` |
| 2 | That expression **never references `opts.scope`** — scope changes the entry set, not the rule | `[V]` same |
| 3 | Suspect = `is_change()` kind, `1..=600s` strictly before the fault line | `[A]` T-fix verification |
| 4 | Deploy entries are `Severity::Info`, so a deploy can never *be* the fault line | `[A]` verification, `timeline.rs:246` |
| 5 | `Deploy \|\| operator` escape the recency window; everything else is windowed by `opts.window_min * 60` | `[V]` `timeline.rs:363–376` |
| 6 | `RecentEvent` keeps only `last_timestamp`; onset discarded in `from_event` | `[A]` verification, `observed.rs:204–225` |
| 7 | The ring keeps only each key's latest occurrence | `[A]` verification, `watch.rs:307–330` |
| 8 | `--postmortem` renders `build_timeline` + `row_decisions` as text | `[A]` T-pre |
| 9 | `firstTimestamp` is populated but not universally — 28/31 measured this round | `[A]` verification |
| 10 | The 3 events lacking it are `Scheduled` on the newer path, carrying `eventTime` and **no `lastTimestamp` at all** | `[A]` verification §3 |
| 11 | Windowing already handles clock skew deliberately: `now.duration_since(t.0).as_secs() <= cutoff`, with a comment noting a future timestamp is kept | `[V]` `timeline.rs:373` |

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 12 | CrashLoopBackOff's backoff caps at 5 min — **but observed cadence was 6.6 and 7.4 min**, because the kubelet's event correlator also aggregates and throttles | The collision with the correlation window is real and measured; the clean 5-minute story is not quite what the data shows. State it accurately |

---

## 1. The defect

`first_trouble` is a **minimum over a filtered set**, taken after scope filtering (claims 1–2). The ring keeps only each key's latest occurrence (claim 7), so a chronic failure presents as **perpetually recent** — a mature crash-looper emits every ~5–7 minutes, lands inside the window reliably, and wins the anchor.

Measured, one controlled variable:

| | suspects flagged |
|---|---|
| chronic crash-looper running | **0** |
| same, scaled to zero | **2** |

The Annals draws *"── trouble begins here ──"* at a point meaning *"when the ring last saw the oldest chronic failure"*, and silently drops the correlation cue that is the section's main analytical claim.

### 1.1 It affects both scopes

Revision 1 claimed subject scope was already correct. **It is not.** The source shows the rule is scope-blind (claim 2), and a probe at `TimelineScope::Workload` with one subject owning a chronic entry, an acute entry, and a change confirms it:

```
first_trouble = the CHRONIC entry (6 min ago)
suspects      = 0
```

The change sits *after* the anchor, so it cannot be a suspect. T-pre's live observation was incidental — `web` had no chronic failure of its own at that moment.

**This makes the work simpler, not harder:** §2's policy is scope-neutral and fixes both. There is no cluster-only variant worth considering.

---

## 2. The policy

### 2.1 The input

Kubernetes' Event carries `firstTimestamp` — the incident's **onset** — and the app discards it (claim 6). Onset separates chronic from acute trivially:

| object | reason | count | onset | last seen |
|---|---|---|---|---|
| crashy-…-j68lx | BackOff | ×4740 | **102.3 h ago** | 7.4 min ago |
| stuck-pvc | ProvisioningFailed | ×8386 | **44.0 h ago** | 1.7 min ago |

Capture it as one field on `RecentEvent`, populated in `from_event`.

**The fallback chain mirrors the one `from_event` already uses for `when`:**

```
first_timestamp → event_time → fall back to `when`
```

Per claim 10, a single-occurrence `Scheduled` event's `eventTime` *is* simultaneously onset and last-seen, so that rung is **exact, not approximate**. Genuinely-absent onset is rarer than 3/31 suggests.

### 2.2 The rule: onset, windowed

> The fault line is the earliest Warning+ entry **whose onset falls inside the existing window**.

- Chronic failures **self-exclude** — onset days old, outside any window
- Acute failures qualify — onset ≈ last-seen
- **No new threshold constant.** It reuses a window that already exists and is already justified

The alternatives both introduce something new: excluding chronic needs a *chronic* threshold to defend; anchoring per-subject only was revision 1's §4 and is now known to half-fix the problem, leaving it exactly where an operator most often looks.

**Add the discriminating filter before the reduction, not after.** `first_trouble` is `min_by` over a filtered set — standing question 1 in its purest form. Correcting the result after the minimum is taken cannot work, because the information needed is gone by then.

### 2.3 Which window

Claim 5: the recency window is `opts.window_min * 60`, and `Deploy || operator` escape it. The correlation window (claim 3) is a separate `1..=600s`.

**They are different constants for different purposes.** Choose the one that bounds *"is this failure part of the current incident"*, and state which and why in the doc comment. Getting this wrong produces a fault line that is *nearly* right — the hardest kind to notice.

### 2.4 Absent onset is unknown

Fall back to `last_timestamp` for that entry and **record that onset was unavailable** — the way `metric_source` records which ratio you are looking at. An entry with unknown onset then behaves as it does today: no worse, and honestly labelled. Never fabricate infinitely-old (silently excluded) or now (silently anchoring).

### 2.5 Clock skew — follow the existing convention

Claim 11: the windowing already handles a future timestamp deliberately and says so in a comment. **Use the same convention for onset rather than inventing a second skew policy.** An onset in the future must not produce a negative age or anchor everything.

---

## 3. Tests

**The mechanism:**
- [ ] A chronic Warning+ entry (onset days old, last-seen minutes old) does **not** anchor the fault line
- [ ] An acute failure (onset ≈ last-seen, both recent) **does**
- [ ] With both present, the acute one wins — the T-pre scenario as a unit test
- [ ] A deploy 1–10 min before an acute failure is flagged as a suspect **with the chronic entry present**

**Both scopes — this is the amendment:**
- [ ] Subject scope **changes** where the subject owns a chronic failure — the §1.1 probe, which currently fails and is the mutation floor already satisfied from the correct direction
- [ ] Subject scope **does not change** where the subject owns no chronic failure — the T-pre case
- [ ] Cluster scope changes as above

> Revision 1's no-regression item read *"subject scope produces the same fault line as before on every existing fixture."* As written that **required preserving the bug**. It is replaced by the two tests above.

**Onset handling:**
- [ ] Absent onset falls back to `last_timestamp` and is recorded as unavailable
- [ ] `eventTime`-only events resolve through the chain (claim 10)
- [ ] Onset in the future is handled per claim 11's convention

**Existing:**
- [ ] `first_trouble_is_earliest_in_window_warning` (`timeline.rs:1070`) still passes, or is replaced by something strictly stronger with the change explained

**Mutation floor, exercised not written:** make onset always equal `last_timestamp` — today's behaviour — and confirm the chronic-versus-acute test fails.

---

## 4. The gate

**With a chronic failure running, does a deploy-then-fail incident flag its suspects?**

Reproduce T-pre's experiment on **kind**: induce a bad rollout with `crashy` running at 2 replicas, read `--postmortem`.

**Expected: 2 suspects. Today it is 0.**

**Both arms are required:**

| Arm | Subject |
|---|---|
| cluster scope | the realm ANNALS |
| **subject scope** | a city's ANNALS for a workload with both a chronic and an acute failure **of its own** |

The second arm exists because of §1.1 and was absent from revision 1.

### 4.1 The instrument

`--postmortem` (claim 8), not a screenshot. It renders the same pure functions as text, so it distinguishes *no suspect* from *cue truncated off the panel*. A screenshot cannot.

### 4.2 The discrimination check

Run the same incident with `crashy` **scaled to zero**. Before the fix that yields 2; after the fix it should still yield 2.

**The fix's signature is that the two runs now agree** — the chronic failure stops changing the answer. Better than checking for "2", which could be right for other reasons.

### 4.3 Where

**kind, not the churn fleet.** kwok emits almost no events and would show nothing either way.

---

## 5. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and **does the state each describes actually occur?**

**Question 1 is the defect itself.** `first_trouble` aggregates before the chronic/acute distinction is available. See §2.2.

**Question 3 is why revision 1 was stopped.** Its §4 (policy) and §5 (acceptance) disagreed, and only one could be satisfied — an implementation faithful to the document would have shipped a test asserting the defect. When a §0 claim is tagged as bounding the change, check whether any acceptance criterion depends on it; if one does, they must fail together.

**Question 4:** `first_trouble`'s meaning changes. Audit `row_decisions`, the Annals rendering, the postmortem export, and anything computing "before the trouble."

---

## 6. Acceptance

- [ ] Onset captured on `RecentEvent`, populated in `from_event`, with the `first_timestamp → event_time → when` chain
- [ ] Policy is onset-windowed, scope-neutral, with no new threshold constant; reasoning recorded at the definition
- [ ] The discriminating filter precedes the reduction
- [ ] Which window, and why, stated in the doc comment
- [ ] Absent onset falls back and is **recorded as unavailable**
- [ ] Clock skew follows the existing convention (claim 11)
- [ ] Both scopes tested — changes where a chronic failure is owned, unchanged where not
- [ ] Gate run on kind, **both arms**, with the discrimination check
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 7. Estimate

**Half a day to a day.** The field capture is small; §2's policy and §3's two-scope test matrix are the work.

Independently valuable regardless of T2 — this is a defect in what ships today, and it improves the Annals immediately.
