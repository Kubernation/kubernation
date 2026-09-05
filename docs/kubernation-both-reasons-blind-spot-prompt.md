# KuberNation — Close the Both-Reasons Blind Spot

**Implementation prompt.** No new capability. A known gap where a regression already hid once, and a fixture to close it.

---

## 1. The gap

v1.38.0 §7 recorded it:

> The both-reasons tag has no live capture: kwok cannot hold a node NotReady, so that combination is unit-tested only — the same limit that hid §2's regression.

And §2 is why it matters. A v1.37.0 regression — `wrap` stripping a dimmed row's indent — sat unobserved for a version because the only dimmed row was a NotReady node, and no fixture could produce one. It surfaced only when a second reason to dim a row was added.

**A code path no fixture can reach is where regressions accumulate unobserved.** The both-reasons tag is the one such path this project has named. Close it.

---

## 2. Verify before building

All `[A]`. Re-read.

| # | Claim | Source |
|---|---|---|
| 1 | kwok's heartbeat rewrites the Ready condition within ~1s; a status patch is reverted immediately, and removing the `kwok.x-k8s.io/node` annotation does not help | A-pre §3 |
| 2 | kind **can** hold a node NotReady — `docker stop kubernation-worker2` did it in T2-pre, and the node stayed NotReady past the 300s eviction timeout | T2-pre §2.2 |
| 3 | On kind, pods on the dead node keep the status its kubelet last reported; the Deployment's replicas reschedule elsewhere, DaemonSets tolerate `unreachable` | T2-pre §2.2 |
| 4 | The churn fleet's allocatable-less node is `churn-sys-g2-000`, `sys` pool index 0, and its fixture is intact — `allocatable: {}` and `capacity: {}` | v1.38.0 §1.1, re-checked at gate time |
| 5 | `NodeTrouble { not_ready, no_capacity }` — two flags, composed into the tag, with the both-case wording `(NotReady, reports no capacity — the node is the story)` | v1.38.0 §1.4 |
| 6 | `NodeTile::capacity_unreported()` reads the derived ratio pair; `None` means the allocatable key is absent, never that metrics-server is down | v1.38.0 §1.2 |
| 7 | kind's nodes report `capacity == allocatable == 15.653 GiB` — real values, from a real kubelet | consolidation §5.1 |

**Claims 2 and 4 are on different clusters**, and that is the whole problem — see §3.

---

## 3. The constraint, and the two ways through it

The two reasons live on two clusters that each can express only one:

| | NotReady | no capacity |
|---|---|---|
| **kind** | yes — `docker stop` (claim 2) | **no** — real kubelet, real allocatable (claim 7) |
| **churn (kwok)** | **no** — heartbeat rewrites it (claim 1) | yes — `churn-sys-g2-000` (claim 4) |

No existing fixture can produce a node that is both. Two ways to construct one:

### 3.1 Make a kind node report no capacity, then stop it

A kind node's allocatable comes from its kubelet. To make it absent you would have to patch `status.allocatable` on the Node object — and the kubelet's own status update will overwrite it on the next heartbeat, exactly as kwok's does for Ready.

**Unless the kubelet is already stopped.** Order matters: `docker stop` the node first, *then* patch its status. With no kubelet running, nothing overwrites the patch, and the node is both NotReady and — as far as the API server is concerned — allocatable-less.

That is the same mechanism claim 3 relies on (pods keep their last-reported status because nothing updates it), applied to the node's own status.

**Check whether the API server preserves the patch.** Node status is a subresource and the apiserver may reject or normalise a status patch that removes allocatable. If it does, this path is closed and §3.2 is the one.

### 3.2 Make a kwok node NotReady by removing its heartbeat

Claim 1 says the annotation removal does not help. But A-pre tested that against a *running* kwok controller. Two things it may not have tried:

- **Deleting the node from kwok's managed set** — if the controller only heartbeats nodes it knows about, a node it has forgotten stays in whatever state it was left
- **Stopping the kwok controller entirely** for the capture window — then *no* node is heartbeated, and a status patch to NotReady sticks

The second is crude but it is a scenario script, not a fixture change, and `9-node-capacity.sh` already establishes the `MODE=give|restore` shape for a reversible manipulation of `churn-sys-g2-000`.

**Prefer §3.2 if it works** — it produces the state on the node that is *already* allocatable-less, so the capacity half needs no construction and the fixture stays the reference fleet.

### 3.3 Either way, verify the state before capturing

Rule 3 of the `hack/` convention, and the one that fired on its author in v1.38.0: **assert the fixture changed before photographing it.** Specifically:

- the node's `Ready` condition is `False` or `Unknown`, and **stays so** across two consecutive reads at least 10s apart — a state that reverts within a second (claim 1) would pass a single read
- the node's `allocatable` is empty, by the same two-read check
- `capacity_unreported()` is `true` for that node in the model, not only in `kubectl`

Without the two-read check, a heartbeat that reverted between the assertion and the capture would photograph the wrong state under the right caption.

---

## 4. The gate

**A node that is both NotReady and allocatable-less renders the both-reasons tag, in every row it appears in, and the rendered line survives the wrap/truncate path.**

The last clause is the point. v1.38.0's unit test pins the wording; what it cannot pin is that the composed tag — the longest of the three — fits the row, keeps its indent, and is not cut where the earlier regression cut. That needs a capture.

### 4.1 The discrimination check

Restore one reason and confirm the tag drops to the other. Restore both and confirm it vanishes. If the both-case tag persists after one reason is restored, the composition is not reading both flags.

### 4.2 Failure criteria, stated in advance

- The both-reasons tag does not render, or renders as one of the single-reason wordings
- The composed tag is truncated where it would lose the second reason
- The row loses its indent — the v1.37.0 regression, in the case it was hidden by
- The fixture reverted between assertion and capture (§3.3)

---

## 5. Tests

- [ ] `gui-smoke` gains the both-reasons state, **if** §3 produces a reproducible fixture. If it needs a stopped controller or a stopped container, it may not be a gui-smoke state — and that is recorded, not deferred
- [ ] The two-read assertion is in the scenario script and fails loudly on reversion
- [ ] The existing unit test for the both-case wording still passes and is now corroborated by a capture

**Mutation floor, asserted applied:** compose the tag from one flag only → the both-case unit test fails; and the scenario's discrimination (§4.1) fails, since the tag would not change when the second reason is restored.

---

## 6. If neither §3.1 nor §3.2 works

Then the blind spot is **structural**, not a fixture gap — no environment this project can build produces the state — and the honest output is:

- record it as such, with what was tried
- note that the both-case is unit-tested only, and *why* that is the limit
- and, per T0's lesson, do not re-narrate it as "captured" in a later handoff

A recorded structural gap is a known place to look when the next regression appears. An unrecorded one is v1.37.0 again.

---

## 7. Standing questions

**5** — every §2 claim is inherited; claims 1 and 2 are from rounds weeks apart on different clusters, and the whole design rests on both still holding. Re-verify both, not just one.

**8** — *"kind can hold a node NotReady"* is true. Is it sufficient? T2-pre stopped a worker to observe *pods*; whether the node's own status can be patched while stopped is a different question (§3.1) and has not been asked.

**2** — a node whose condition could not be verified across two reads is in an **unknown** state, not a NotReady one. The script must say so rather than capture.

---

## 8. Acceptance

- [ ] Claims 1 and 2 re-verified on their respective clusters
- [ ] §3.1 or §3.2 produces the both-reasons state, verified across two reads
- [ ] The tag renders composed, indented, uncut — captured
- [ ] Discrimination run: one reason restored, then both
- [ ] gui-smoke state added, or recorded as unrepresentable with the reason
- [ ] The scenario committed under the `hack/` convention, `MODE=…|restore`
- [ ] If neither path works: §6, recorded
- [ ] Cluster left as found

---

## 9. Estimate

**Two to three hours.** Most of it is §3 — finding which cluster will hold the state. The capture itself is minutes once the fixture exists.
