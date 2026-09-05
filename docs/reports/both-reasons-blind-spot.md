# The both-reasons blind spot, closed

**Prompt:** "KuberNation — Close the Both-Reasons Blind Spot" (2026-09-05).
**Version:** none — test assets and docs only, no Rust changed (the A0 / A-pre /
measurement-session precedent).
**Gate:** a node that is both NotReady and allocatable-less renders the composed
tag, in every row, through the wrap/truncate path — **PASSED**, with the §4.1
discrimination.

The gap v1.38.0 §7 recorded is closed: the combination is no longer unit-tested
only. **Both** §3.1 and §3.2 turned out to work, and the round's finding is that
verifying claim 1 destroyed the fixture claim 4 describes.

---

## 1. Claims — all six re-read, and standing question 5 taken literally

| # | | |
|---|---|---|
| 1 | kwok's heartbeat rewrites Ready within ~1s | **TRUE**, re-verified: a patched `Ready=False` was back to `True` on the first read 2s later |
| 2 | kind can hold a node NotReady | **TRUE**, re-verified: `docker stop kubernation-worker2` → `Ready=Unknown` after ~50s, held across two reads 12s apart. Note **Unknown**, not `False` |
| 4 | the churn fleet's allocatable-less node is intact | **TRUE** at the start — and **false by the time I had finished checking claim 1** (§3) |
| 5 | `NodeTrouble{not_ready,no_capacity}`, composed | TRUE |
| 6 | `capacity_unreported()` reads the derived ratio pair | TRUE |
| 7 | kind reports real capacity | **TRUE**: `16424476Ki` allocatable = capacity, from a real kubelet |

Claims 1 and 2 are from rounds weeks apart on different clusters, and SQ5 says
re-verify both. Both were re-verified **on their own clusters**, not inferred.

**Why claim 1 holds, which A-pre did not establish.** The kwok controller runs
with **`--manage-all-nodes=true`**. There is no managed set to fall out of, so
A-pre's finding that removing the `kwok.x-k8s.io/node` annotation does not help
is true *and* could not have been otherwise. That also closes §3.2's first
sub-option ("delete the node from kwok's managed set") on inspection rather than
by experiment.

---

## 2. THE FINDING — verifying claim 1 broke the fixture claim 4 describes

Re-verifying claim 1 meant patching `Ready=False` on `churn-sys-g2-000`. Two
seconds later Ready was back to `True` — the claim confirmed. But the node's
`allocatable` had also become **`{"cpu":"1k","memory":"1Ti","pods":"1M"}`**:
kwok's node-initialize stage had backfilled it, and `node-agent` went from
100/99 to 100/100 because the node could suddenly schedule.

**The mechanism, and it is new.** A-pre §3 recorded that kwok backfills a
default capacity onto a node that ships **no Ready condition**, and that
supplying one opts out of the stage. What neither A-pre nor v1.38.0 recorded is
that the stage re-fires when Ready is *anything but True* — so marking the node
NotReady is itself enough to restore its capacity, destroying the very property
that makes it the fixture.

Two consequences, both now built in:

- `10-node-notready.sh` sets **Ready=True before restarting the controller**, in
  that order. The reverse would hand the running controller a not-Ready node and
  it would backfill capacity on the spot.
- The restore was run and verified (`allocatable` empty again, held) before any
  further work.

**One residual difference, stated.** The node now permanently carries kwok's
full condition set — MemoryPressure / DiskPressure / PIDPressure /
NetworkUnavailable — where the fixture originally gave it only Ready. All are
`False`, so `NodeTile.abnormal` is unchanged and nothing reads differently; but
the node is not byte-identical to how `up.sh` builds it.

---

## 3. Both paths work, and the reason to prefer §3.2 is not the one given

**§3.2 (churn, preferred by the prompt): works.** Stop the kwok controller, flip
Ready, and nothing rewrites it. Verified across two reads 12s apart.

**§3.1 (kind): also works — standing question 8, answered.** The prompt records
this as never asked and expects it may be closed: *"the apiserver may reject or
normalise a status patch that removes allocatable. If it does, this path is
closed."* It does not. With `kubernation-worker2` stopped, a status patch
emptying `allocatable` and `capacity` was **accepted and held** across two reads,
on a node already `Ready=Unknown`. So kind can express the both-reasons state
too, and with an advantage the churn path lacks: **no clock** — it holds for as
long as the container is stopped, where the kwok window is bounded (§4).

**But the capture still has to be on churn, for a different reason.** kind has
four nodes, and `floor_binds(4)` is true — the Substrate tab correctly refuses to
show a table at all there ("4 nodes: no gap is representable at this size",
v1.37.0's own gate). So kind can produce the node *state* and cannot render the
*tag*. That is a better reason than "the capacity half needs no construction",
and it means §3.1 is a live fallback for the state rather than a closed path.

---

## 4. Why this is not a gui-smoke state — recorded, not deferred

§5 allows for it, and it applies:

- gui-smoke runs against the **kind** dev cluster, where the arithmetic floor
  means the tab shows no table (§3).
- The churn path needs a **stopped cluster component** and holds only while the
  node lease has not lapsed: kwok renews with `--node-lease-duration-seconds=200`
  and kube-controller-manager runs `--node-monitor-grace-period=3m20s`, so after
  ~200s **every** node goes NotReady and the capture would show a dead fleet
  rather than one node that is both.

The scenario therefore asserts, before letting you capture, that the target is
the **only** NotReady node — the lapse check.

---

## 5. Two instrument bugs, both found by running the thing

**The one that matters: `[ cond ] && cmd` as a function's last statement.**
`assert_twice` ended with `[ "$i" = 1 ] && sleep "$SETTLE"`. On the second pass
the test is false, so the compound returns 1, so the function returns 1, so
`set -e` aborted the **caller** — *after* both "ok read" lines had printed. The
gate script died silently and its most important assertion (no other node
NotReady) never ran. I had verified that by hand, so the gate is sound, but the
script was not. Fixed with `if`, and added to `hack/README.md` as a corollary to
rule 2, because it is the same family as the three failures that motivated it:
**an instrument that prints success and then fails silently.**

**And an interaction between two scenarios.** `9-node-capacity.sh`'s restore
deletes the pods stranded by the capacity window; kwok finalizes fake pods, so
with its controller stopped (scenario 10) the delete **blocks forever**. It cost
a ten-minute timeout, during which the node lease lapsed, four nodes went
NotReady, and the node-lifecycle controller set `Ready=False` on every daemonset
pod — which kwok does not undo, so the fleet needed its `node-agent` pods
recreated before it was as found. Fixed with `--wait=false`.

---

## 6. The gate

**Failure criteria, stated in advance:** the tag missing or rendering as one
reason; the composed tag truncated where it loses the second reason; the row
losing its indent; the fixture reverting between assertion and capture. **None
occurred.**

**Both reasons** — `churn-sys-g2-000`, Ready=False, allocatable empty, the only
NotReady node:

```
churn/log-agent       on 98 / 100   missing from 2
    churn-sys-g2-000   (NotReady, reports no capacity — the node is the story)
    churn-sys-g2-001
churn/node-agent      on 99 / 100   missing from 1
    churn-sys-g2-000   (NotReady, reports no capacity — the node is the story)
churn/node-exporter   on 98 / 100   missing from 2
    churn-edge-g1-000
    churn-sys-g2-000   (NotReady, reports no capacity — the node is the story)
```

Composed, in **every** row, **indented**, and **uncut** — which is the clause the
unit test cannot reach. This is the longest of the three wordings, rendered
through the wrap/truncate path, in the exact case the v1.37.0 regression hid in.
The attention queue independently promoted `node churn-sys-g2-000 — NotReady` to
the top concern.

**§4.1 discrimination — restore one reason.** Giving the node capacity while it
stays NotReady:

| | tag |
|---|---|
| both | `(NotReady, reports no capacity — the node is the story)` |
| capacity restored | `(NotReady — the node is the story)` |

The tag dropped to the remaining reason, so the composition reads both flags —
had it not, the both-wording would have persisted. `node-agent` also went to
100/100 as its pod could finally schedule, while `log-agent`'s affinity gap
stayed, consistent with v1.38.0.

**Mutation, asserted applied:** composing from one flag only (`else if`) fails
`not_ready_and_no_capacity_are_distinguishable_and_both_carriable`, which reports
`a node that is both says both: (NotReady — the node is the story)`. The live
half of that mutation is what the §4.1 discrimination demonstrates.

---

## 7. Cluster left as found

**churn:** 100 nodes, 0 NotReady, `node-agent` 100 desired / 99 ready,
`churn-sys-g2-000` Ready=True with empty allocatable, scenario 8's daemonsets
removed. **kind:** 4 nodes Ready, `kubernation-worker2` reporting its real
16-core / 16424476Ki allocatable again.

`10-node-notready.sh` was re-run end to end after the fixes; both modes exit 0
and the fleet-wide assertion now runs.
