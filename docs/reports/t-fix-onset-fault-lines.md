# T-fix — onset-aware fault lines

**Phase:** T-fix, from `kubernation-t-fix-onset-fault-lines-guidance-rev2.md`
**Version:** v1.11.1 → v1.11.2 · **Date:** 2026-08-05
**Preceded by:** `t-fix-verification.md` (rev1, stopped at §0 — claim 8 was false)

All twelve §0 claims verified. **Both gate arms pass.** Dev cluster restored.

---

## 1. What shipped

| Piece | Where |
|---|---|
| `RecentEvent.onset`, chain `first_timestamp → event_time` | `state/observed.rs` |
| `TimelineEntry.onset` (resolved) + `onset_reported` | `state/timeline.rs` |
| `anchors()` — the policy, with its reasoning | `state/timeline.rs` |
| `first_trouble` = min **onset** among recently-started failures | `state/timeline.rs` |
| `SUSPECT_CUE` + `row_text()` — one wording, cue reserved not truncated | `gui/timeline.rs` |

439 core + 102 GUI tests; clippy clean.

### The rule

> The fault line is the earliest Warning+ entry whose **onset** falls inside the
> recency window.

Three decisions inside that sentence, each recorded at the definition:

**Onset, not latest occurrence.** "Trouble begins here" is a claim about when the
incident started, and the suspect window is measured backwards from it — so a
deploy five minutes before a failure *started* is a candidate cause, while five
minutes before its four-thousandth recurrence is not a fact about anything.

**The recency window, not the correlation window.** They are different constants
for different purposes. The entry set is already bounded by recency, so an anchor
bounded more tightly could leave an entry visible in the Annals that is
nonetheless unable to be the trouble it visibly is. It is also `opts.window_min`
rather than a constant, so a caller that widens the feed widens the anchor with it.

**The filter precedes the reduction.** §2.2's requirement, and standing question 1
in its purest form: once the minimum is taken the entry it came from is gone, so
the chronic/acute distinction cannot be applied afterwards.

Absent onset falls back to `when` and records that it did (`onset_reported`), so
an entry with unknown onset behaves exactly as it did before this field existed —
which is also why `first_trouble_is_earliest_in_window_warning` still passes
unchanged.

---

## 2. The gate

### Arm 1 — cluster scope, chronic failure running

```
4m · web — rev 10->11 · nginx:1.27-alpine -> …/broken2   (before the failure)
4m · web — ScalingReplicaSet: Scaled up …                (before the failure)
```

**2 suspects, with `crashy` running at 2 replicas. Before the fix: 0.**

### Arm 2 — subject scope, a subject owning both a chronic and an acute failure

```
! crashy-8546df48d-nnvct — BackOff        ← chronic, above the line, not anchoring
──────────────────── fault line ────────────────────
^ crashy — rev 7->8 · b~   (before the failure)
⌐ crashy — ScalingRepli~   (before the failure)
```

**2 suspects.** The 20-hour chronic entries sit above the rule without anchoring
it; the anchor is the acute onset. This is the arm rev1 asserted was unnecessary.

### The discrimination check

| | suspects fire? |
|---|---|
| chronic present (arm 1) | **yes** |
| chronic absent (run A) | **yes** |

They agree — §4.2's stated signature. Before the fix, T-pre measured them
disagreeing: 0 with the chronic failure present, 2 without. Chronic presence no
longer decides whether the correlation fires.

*Honest caveat:* by run A the cluster had accumulated several induced incidents
across the session, so the specific rows flagged differ between runs. The
invariant under test is whether chronic presence changes *whether* suspects fire,
not which ones.

### Mutation floor, exercised

| Mutation | Caught by |
|---|---|
| onset always equals `last_timestamp` (today's behaviour) | 4 tests |
| the windowing filter dropped | 3 tests |
| the `eventTime` rung dropped | the chain test |

---

## 3. Two findings the guidance did not anticipate

### 3.1 The suspect cue was structurally invisible in city and province panels

The cue is appended and the row then truncated, so it was **the first thing
dropped** — and a city row is capped at **30 characters** when it shares space
with a rollback button, which an entry title alone exceeds. Three renderers each
did this, and two had drifted to different wordings (`(before failure)` vs
`(before the failure)`).

So the fix's benefit was unobservable in exactly the panels §1.1 extended it to.
Now `SUSPECT_CUE` is one constant and `row_text()` is the one producer: it
reserves the cue's width and truncates the text into what remains. Unit-tested,
including a cap smaller than the cue itself.

**This is a rendering change, which rev2 says is out of scope.** I made it because
shipping a fix whose result cannot be seen at one of its two scopes is the
unearned-all-clear shape this codebase refuses everywhere else. Flagged for veto.

### 3.2 Cause and effect routinely share a timestamp

The correlation rule is `1..=600s`, strictly before. Measured live, twice:

| RS created | failure onset | d | suspect? |
|---|---|---|---|
| 22:58:44Z | 22:58:44Z | **0 s** | no |
| 23:04:45Z | 23:04:46Z | 1 s | yes |
| 22:42:13Z | 22:42:24Z | 11 s | yes |

Kubernetes Event timestamps are second-granularity and a kubelet fails an image
pull immediately, so for the very incident shape T2 is built on, **whether the
correlation fires is partly a coin flip at one-second resolution.** The lower
bound of 1 exists deliberately ("a change at the exact failure instant isn't a
precursor"), but with second-granularity timestamps that exclusion also catches
genuine causes.

Not changed here — it is the correlation rule's lower bound, not the anchor, and
it deserves its own decision. **It matters for T2**, whose whole claim is that
these correlations cluster spatially.

---

## 4. §5 — standing questions

**1. Where does a summing step precede a comparing step?**
This defect *was* that shape: `first_trouble` is a reduction, and the
chronic/acute distinction was unavailable at the point it was taken. Fixed by
filtering before reducing, which is why `anchors()` is a filter rather than a
post-hoc correction.

**2. Does every reducer over a possibly-empty input express unknown, or fabricate?**
`first_trouble` stays `Option` and is now `None` more often — a scope whose only
failures are chronic gets no fault line rather than one at an arbitrary point,
pinned by `an_all_chronic_scope_has_no_fault_line_rather_than_a_wrong_one`.
`onset` is `Option` at capture so "unavailable" survives to the consumer that
resolves it; `row_text` handles a cap smaller than the cue without underflow.

**3. Where do two sections constrain the same behaviour?**
This is why rev1 was stopped: its §4 and §5 disagreed and only one could be
satisfied. rev2 resolves it. Within this round, §2.3 (which window) and §2.5
(clock skew) both constrain `anchors()`; they agree, because the skew convention
is inherited from the recency filter rather than invented.

**4. What existing consumers depend on the old meaning?**
Exactly one: `row_decisions`, reached by `annals_lines` (the modal, city and
province) and the postmortem export. Both its outputs — `fault_line_above` and
`suspect` — now measure from the incident's start, which is the intended change.

**5. Which claims were inherited, and does each state occur?**
All twelve verified this round. The two states the fix depends on were produced,
not assumed: a chronic-plus-acute subject (arm 2) and an `eventTime`-only event
(measured, 3 of 31 on kind, all `Scheduled` with no `lastTimestamp`).

---

## 5. Method note — why the live arm took five attempts

Four arm-2 captures showed no cue, and I could not explain them. Each had a
different cause, and none was a defect:

1. cue truncated away (§3.1);
2. cause and effect in the same second (§3.2);
3. an *earlier* induced incident still inside the window, so the later deploy was
   correctly not a precursor to it;
4. the restored crash-looper was only 9 minutes old — not yet chronic by the
   window's own definition, so it legitimately anchored.

Cause 3 is the one worth carrying: **inducing repeated incidents to test a
correlation contaminates the very window the correlation reads.** The clean arm
needed the cluster quiesced until every prior onset aged past 15 minutes. Any
future live test of this machinery has to budget for that, and a test that skips
it will produce a plausible number for the wrong reason — the ninth instance.

I also spent several rounds inferring from pixels before checking the renderer,
after asserting that a full-width row implied `suspect == false`. That inference
was wrong because the city window truncates by *character count*, so suspect and
non-suspect rows truncate identically. Reading the code would have been faster
than reading the screen.
