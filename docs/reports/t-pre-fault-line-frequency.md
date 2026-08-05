# T-pre — fault-line frequency, and a live defect it found

**Measurement** for `kubernation-workstream-t-planning.md` §3 (T2's risk note:
"if fault lines are rare, the feature is invisible most of the time. Worth
measuring frequency on a real cluster before building").
**Date:** 2026-08-04 · No code changed. Dev cluster restored.

**Headline: the frequency question is the wrong question. Fault lines are
common; the *correlation* T2 wants to draw is suppressed on any cluster with a
chronic failure — which is most real clusters. That is a defect in shipped
behaviour, and it makes T2 more expensive than my previous recommendation said.**

---

## 1. What "frequency" can honestly mean here

Frequency in the wild is not measurable in an afternoon on a dev cluster, and
attempting it would have produced instrument failure number eight: kind's standing
failures (`crashy`, `stuck-pvc`) are *permanent*, so nothing arises and nothing
precedes it. I would have measured "fault lines are rare" for a reason unrelated
to how often they occur anywhere real.

What is measurable, and what gates T2: **does the machinery fire, on the incident
shape T2 exists to show?** That shape is deploy → failure — the classic bad
rollout. So: induce one, and look.

---

## 2. The structural bound, from source

- **Fault line** (`first_trouble`) = the *earliest* entry at `Severity::Warning`
  or above, with a timestamp.
- **Suspect** = an entry whose kind `is_change()` — `Deploy | Scale | Operator |
  NodeChange` — falling `1..=600` seconds **strictly before** the fault line.
- **Deploy entries are `Severity::Info`**, so a deploy can never *be* the fault
  line, only a suspect.
- `Deploy || operator` escape the 15-minute recency window; everything else is
  windowed.

So the durable, catchable correlation is exactly: *a deploy, up to 10 minutes
before a failure that is itself still inside the 15-minute event window.* Good
news on its face — that is the common real incident.

---

## 3. What actually happened

Induced a real bad rollout on kind: `web` → `kubernation.invalid/faultline/broken`.
Result within a minute: a new Deployment revision, and `ImagePullBackOff`.

The cluster-scope timeline showed the textbook pair one minute apart:

```
1s   ! web-7db478547c-rgxs6 — Failed: Error: ImagePullBackOff  ×4
1m   · web — rev 8->9 · nginx:1.27-alpine -> kubernation.invalid/faultline/broken
```

**Suspects flagged: 0.**

Re-checked at 4 minutes, when the deploy sat comfortably before the failures.
**Still 0.** The fault line had anchored on `5m crashy … BackOff`, an unrelated
chronic failure.

### The discrimination check

Same incident, same deploy, same failure. Only variable: whether the chronic
crash-looper was still emitting.

| | suspects flagged |
|---|---|
| `crashy` running (2 replicas) | **0** |
| `crashy` scaled to 0 | **2** |

With it stopped, the correlation appears exactly as designed:

```
10m  · web — rev 8->9 · nginx:1.27-alpine -> …/faultline/broken   (before the failure)
10m  · web — ScalingReplicaSet: Scaled up replica set web-7db478547c   (before the failure)
```

The mechanism works. An unrelated chronic failure suppresses it.

---

## 4. Why — and it is not bad luck

`first_trouble` is the **minimum** over Warning+ entries, and the event ring keeps
only each key's **latest** occurrence (T0's finding). A chronic failure therefore
presents as *perpetually recent*.

The cadence is the trap. **CrashLoopBackOff's exponential backoff caps at 5
minutes**, so a mature crash-looper emits a BackOff event every ~5 min — the same
order as `CORRELATION_WINDOW_MIN` (10 min). Measured on kind: crashy's BackOff
last fired 6.6 and 7.4 minutes before the capture. It lands inside the window
reliably, wins the anchor, and whether a real correlation survives becomes a coin
flip on which chronic failure refreshed least recently.

At **cluster scope** one crash-looper anywhere poisons the anchor for every
workload. At **subject scope** (a city's ANNALS) it is clean — verified: `web`'s
own fault line sat correctly beneath its own failures and above its deploy.

---

## 5. This is a live defect, not only a planning input

Shipped behaviour, today, on any cluster with a chronic failure: the Annals draws
"── trouble begins here ──" at a point that means *"when the ring last saw the
oldest chronic failure"*, not *"when this incident began"* — and silently drops
the correlation cue that is the section's main analytical claim.

The missing input is **onset**. `RecentEvent` captures only `last_timestamp`;
Kubernetes' Event carries `firstTimestamp`, and it is populated (52/55 on kind)
and cleanly discriminating:

| object | reason | count | onset | last seen |
|---|---|---|---|---|
| crashy-…-j68lx | BackOff | ×4740 | **102.3 h ago** | 7.4 min ago |
| stuck-pvc | ProvisioningFailed | ×8386 | **44.0 h ago** | 1.7 min ago |

Chronic and acute are trivially separable by onset. The app throws the field away.

**Capturing it is small** — one field on `RecentEvent`, populated in `from_event`.
**Using it is a design decision**, and not simply "anchor on onset": crashy's onset
is 102 hours back, so a naive minimum-over-onset would drag the fault line four
days into the past and mark everything as after-the-trouble. The policy — exclude
chronic failures from the anchor, or anchor per-subject only, or both — is what
T2's guidance has to settle. Onset supplies the evidence; it does not decide.

---

## 6. This overturns my own recommendation

Last turn I said T2 was the phase that "needs nothing new" and floated running it
before T1. **The measurement falsifies that.** T2's premise is "put the Annals'
existing conclusion on the map" — and on a realistic cluster that conclusion is
unreliable. Rendering it spatially would render a coin flip.

T2 now has a prerequisite: fix the correlation first. Which is core work in
`timeline.rs`, not a rendering pass.

That prerequisite is worth doing regardless of T2, because §5 above is a defect in
what ships today. It improves the Annals immediately, and it is the kind of small,
pure-core, testable change this codebase is good at.

**Recommended order, revised:**

1. **T-fix — onset-aware fault lines.** Small, pure, independently valuable,
   fixes shipped behaviour, and is T2's prerequisite. Needs a policy decision on
   chronic-vs-acute that its guidance must state explicitly.
2. **T1 — change-since, occupant axis only**, gated against the Annals *with the
   window held constant* (T0 §2.2). Still the honest thesis test, still cheap
   because A5 shipped the substrate, and it keeps §6's kill point early.
3. **T2 — fault lines on the map**, once step 1 makes the conclusion trustworthy.
4. **T3 — small multiples**, if T1 says spatial change reads. T0 sized the strip
   at 23 KiB for six frames at 100 nodes, needing no persistence.

---

## 7. Method notes

- The instrument was the **postmortem export** (`--postmortem`), because it
  renders `build_timeline` + `row_decisions` — the same pure functions the Annals
  uses — as text rather than pixels. Reading a screenshot would not have
  distinguished "no suspect" from "cue truncated off the panel".
- **T0 §2.4 held:** this had to be measured on kind. The churn fleet emits almost
  no events and would have shown nothing either way.
- The discrimination check was the whole experiment, not a footnote. "0 suspects"
  alone would have been a plausible number with several possible explanations;
  0-versus-2 across one controlled variable identifies the cause.
- Dev cluster restored: `web` back on `nginx:1.27-alpine` at 3/3, `crashy` back to
  2 replicas.
