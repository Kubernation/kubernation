# T-fix — §0 verification: claim 8 is false

**Stopped at §0 per the standing method.** No code written.
**Date:** 2026-08-04 · Tree clean, 407 core tests green.

11 of 12 claims verified TRUE. **Claim 8 is false**, and it is the one §0 says
"bounds the whole change" — so this is a stop-and-report rather than an adapt.

---

## 1. Verification

| # | Claim | Result |
|---|---|---|
| 1 | `first_trouble_is_earliest_in_window_warning` exists at `timeline.rs:1070` | TRUE |
| 2 | `first_trouble` = earliest Warning+ with a timestamp | TRUE (`:397–401`) |
| 3 | Suspect = `is_change()` kind, `1..=600 s` strictly before the fault line | TRUE (`:450–458`) |
| 4 | Deploy entries are `Severity::Info` | TRUE (`:246`) |
| 5 | `Deploy \|\| operator` escape the window; the rest are windowed | TRUE (`:363–376`) |
| 6 | `RecentEvent` keeps only `last_timestamp`; onset discarded | TRUE (`observed.rs:204–225`) |
| 7 | The ring keeps only each key's latest occurrence | TRUE (`watch.rs:307–330`) |
| **8** | **Subject-scope fault lines are already correct; only cluster scope is poisoned** | **FALSE — see §2** |
| 9 | `--postmortem` renders `build_timeline` + `row_decisions` as text | TRUE (used as the T-pre instrument) |
| 10 | CrashLoopBackOff backoff caps at 5 min | TRUE as a Kubernetes fact; see §4 for a caveat on the observed cadence |
| 11 | `firstTimestamp` populated but not universally (52/55) | TRUE — re-measured 28/31 this round |
| 12 | Kubernetes may emit `eventTime` instead — check both | **TRUE, and useful** — see §3 |

---

## 2. Claim 8, and why it matters

`first_trouble` is computed at `timeline.rs:397` as a minimum over `entries`
*after* scope filtering. **Nothing about it is scope-aware.** Subject scope has a
smaller entry set, not a different rule — so it is poisoned exactly when the
subject has a chronic failure **of its own**.

Probed against current code, at `TimelineScope::Workload`, with one subject
owning all three entries:

| entry | age | kind |
|---|---|---|
| `web-old-abc` BackOff | 6 min | chronic (a mature crash-looper's cadence) |
| `web-new-xyz` Failed | 30 s | acute |
| `web` ScalingReplicaSet | 2.5 min | the change — a textbook suspect |

```
first_trouble = 11:54:00Z   (the CHRONIC entry, 6 min ago)
suspects      = 0
```

The change sits *after* the anchor, so it cannot be a suspect. Same defect, same
mechanism, subject scope.

T-pre's live observation was not wrong, it was **incidental**: `web` had no
chronic failure of its own at that moment. I generalised one case into a property
of the scope, and the guidance inherited it. That is the third time in this
project an inherited claim has carried an error forward, and the second time the
error was mine.

### What it invalidates

- **§4** — "Consider whether the policy should apply **only** at cluster scope.
  That is a smaller change with a narrower blast radius, and claim 8 is the
  evidence for it." The evidence does not exist. Taking that path would leave the
  defect exactly where an operator most often looks: a city's ANNALS.
- **§5's no-regression item** — "Subject scope produces the same fault line as
  before on every existing fixture." As written this **requires preserving the
  bug**. An implementation faithful to the guidance would ship a test asserting
  the defect.

That second point is why this is a stop rather than a silent correction: the
guidance's acceptance criteria and its recommended policy now disagree with each
other, and only one of them can be satisfied.

---

## 3. Claim 12 is true, and it makes §3.3 easier

The 3 events lacking `firstTimestamp` are `Scheduled` events on the newer events
path. They carry `eventTime` and **no `lastTimestamp` at all**:

```
apiVersion=v1  reason='Scheduled'  eventTime=2026-08-05T02:33:14Z  series=false  lastTimestamp=None
```

For a single-occurrence event, `eventTime` is *simultaneously* onset and
last-seen. So the onset chain should mirror the one `from_event` already uses for
`when`: `first_timestamp` → `event_time` → fall back to `when`. That is not a
fabrication, which softens §3.3 — genuinely-absent onset is rarer than 3/31
suggests, because two of the three fallbacks are exact.

§3.3's requirement still stands for the residual case, and its reasoning is right.

---

## 4. A caveat on claim 10's mechanism

The 5-minute backoff cap is real, but the *observed* cadence was 6.6 and 7.4
minutes between last-seen timestamps — longer than the cap. The kubelet's event
correlator also aggregates and throttles repeated events, so the emission rate is
not purely the backoff cap.

This does not affect the defect, which is measured, or the fix. It affects only
the story about *why* the collision is reliable. Worth stating accurately rather
than asserting a clean 5-minute mechanism that the data does not quite show.

---

## 5. What survives, and what I would do

Everything else in the guidance holds, and the correction makes the work
**simpler and broader**, not harder:

- **§3.1's policy — onset, windowed — is scope-neutral and fixes both scopes.**
  It needs no cluster-scope special case, so §4's "narrower blast radius"
  alternative was never the cheaper option; it was the option that half-fixed it.
- §2's field capture is unchanged, and §3 gains the `eventTime` fallback.
- §5's mechanism and onset-handling tests all stand. Only the no-regression item
  needs rewriting: subject scope must **change** on a fixture where the subject
  owns a chronic failure, and must **not** change where it does not.
- The gate (§6) should gain a subject-scope arm, since that is now known to be
  affected: the city ANNALS for a workload with both a chronic and an acute
  failure.

The probe in §2 is the regression test §5 wants, and it currently fails — which
is the mutation floor already satisfied, from the correct direction.
