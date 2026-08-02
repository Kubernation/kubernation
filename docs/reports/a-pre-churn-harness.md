# A-pre — The churn harness

**Implementation report** · 2026-08-02 · unversioned (test asset)
**Commit:** `f96f49b`
**Governing docs:** [`kubernation-a-pre-churn-harness-guidance.md`](../kubernation-a-pre-churn-harness-guidance.md), [`kubernation-enabling-plan.md`](../kubernation-enabling-plan.md) Workstream A

A reproducible 100-node fleet that can be made to churn on command, so
Workstream A's gates can be evaluated at all. Shell and YAML; no production code.

| | |
|---|---|
| Deliverable | `hack/churn/` — 10 scripts + README |
| Fleet stand-up | ~5s to 100 nodes, ~40s to 416 pods settled |
| Capture cost | 5.5s per frame |
| Scenarios | 6, all runnable in isolation and repeatable |
| Panics across a full run | **0** |

---

## 1. Verification: all six §0 claims TRUE

| # | Claim | Result |
|---|---|---|
| 1 | kwokctl installable, ~100 nodes | ✅ v0.7.0 |
| 2 | `--center` / `--zoom` / `--screenshot` | ✅ exact — the "quoted from memory" hedge was unnecessary |
| 3 | `--context` | ✅ |
| 4 | No instance-type label unless set | ✅ |
| 5 | Nodes report `status.allocatable` | ✅ (when the fixture sets it — see §3) |
| 6 | metrics-server absent | ✅ |

Second round running that §0 survived intact.

### Four things that could have blocked it, and don't

- **kwokctl runs a real `kube-scheduler` and `kube-controller-manager`.** Pods
  genuinely schedule and reschedule — scenario 1's hardest requirement is real,
  not faked.
- **A node can omit `allocatable`** and the app survives it.
- **Capture costs 5.5s**, so the guidance's conditional — *"if the capture flags
  support it cheaply"* — resolves to yes. A 20-frame flipbook is ~2 minutes.

---

## 2. Scenario 1 surges, and it is verified

The guidance singles this out: immutable-infrastructure refreshes *surge*, and a
delete-then-create script would quietly test an easier problem, because there
would never be more nodes than slots and slot assignment would never have to
choose.

Watching node count during a 15-node wave:

```
100  →  115  →  100
        ^^^ both generations Ready at once
```

Replacements take **new names** (a generation token), so nothing can be matched
by identity. Pods are drained onto them, so the scenario churns cities as well as
provinces — verified: 100 pods landed on the new generation.

---

## 3. Three kwok behaviours found the hard way

These are the "kwok will lie to you" class the guidance's §4 warns about, but
none of them is the one §4 names.

**kwok backfills a fake capacity.** A node that omits `status.allocatable` *and*
ships no Ready condition gets silently given **1k cpu / 1Ti / 1M pods** by kwok's
node-initialize stage. My first fixture did exactly that, so the
"missing allocatable" node wasn't missing anything — and the phantom capacity
polluted the heterogeneous-capacity spread. Supplying the Ready condition
ourselves opts out of that stage and the field stays genuinely absent.

*This also corrects my own pre-implementation review*, which reported the case as
representable on the strength of a probe I deleted after six seconds — before the
stage had run. The conclusion was right; the evidence was too thin.

**kwok cannot hold a node NotReady.** Its heartbeat rewrites the Ready condition
within ~1s, whether or not the node carries the `kwok.x-k8s.io/node` annotation.
A status patch applies and is reverted immediately; removing the annotation first,
with a settle, does not help. So scenario 6 deletes — which matches the guidance's
own wording ("continent vanishing") — and **refuses `MODE=notready` with exit 2**
rather than running and changing nothing. A silent no-op would let a reviewer
conclude the map survived an outage it never saw.

**Deleting a node strands its pods for 30–60s.** PodGC has a quarantine delay, so
immediately after a wave there are pods bound to nodes that no longer exist. Real
Kubernetes behaviour, not a kwok artefact — and the same ghost-pod window that bit
the substrate round's prevalence maths. The destructive scenarios now wait it out
before their final capture, or the "after" image shows workloads on departed nodes.

---

## 4. Guidance defects

§0 was clean; as with A0, the defects were elsewhere.

**§4 asserts `node_allocatable` "explicitly forbids fabricating a default".** Its
doc comment does say that — but **six of its seven callers do it anyway**
(`unwrap_or(0.0)`). Verified on the live fleet: the allocatable-less node renders
**cpu 0% / mem 0%**, visually identical to an idle node rather than flagged as
unmeasured. The fixture exercises the path; it does not exercise honest handling
of it, because there isn't any. Recorded in the README; left unfixed as
pre-existing and outside a test asset's scope.

**§5 pins the wrong set of flags.** It specifies fixed `--center` and `--zoom` so
that "apparent movement is just the camera moving". But `--overlay` and
`--map-style` **persist in `~/.config/kubernation/prefs.json`** and are restored
at launch — during development a capture came back tinted by the Namespace
overlay in Relief for exactly that reason. A before/after pair taken on different
days can differ for reasons having nothing to do with churn. `capture.sh` pins
all four.

**§4 cites `pool_source` and a "fallback cascade" as if they exist.** No pool
concept exists in the codebase at all. The fixture carries three provider
conventions plus an unlabelled pool so A1 has something real to build against,
but until A1 lands those acceptance items are checkable only *structurally*.

That last one is the A0 method finding recurring: **a prerequisite's artifacts
cannot be validated until their consumer arrives**, and §6's acceptance list does
not separate the criteria verifiable now from those that are not.

---

## 5. Acceptance

| §6 criterion | Status |
|---|---|
| ~100 nodes reproducibly from clean | ✅ ~5s |
| Six scenarios, isolated and repeatable | ✅ |
| Scenario 1 surges, configurable overlap | ✅ verified at 115 |
| Scenario 1 reschedules pods | ✅ |
| Fixture: multi-zone pool, unlabelled pool, no-allocatable node, no-instance-type pool, uneven zones | ✅ all present |
| Consistently-framed before/during/after captures | ✅ all four flags pinned |
| `main` survives every scenario without panicking | ✅ **0 panics** (a baseline, not a pass) |
| README states the metrics limitation | ✅ plus three more |

---

## 6. Decisions for the room

### The harness is built; the A2 gate is now runnable

Nothing further is needed to evaluate whether the map holds still under churn.
Today's map is expected to reshuffle wildly — that is the baseline, not a failure.

**Ask:** run the A2 gate now against current `main` to establish the "before", or
wait until A1's layout engine exists?

### `node_allocatable`'s contract is violated by six of its seven callers

A node with unknown capacity renders as 0% — indistinguishable from idle. This
predates A-pre and is out of a test asset's scope, but the harness now
reproduces it on demand, so it is cheap to fix and verify whenever it is
scheduled.

**Ask:** schedule the degrade-dark fix, or accept 0% as the documented behaviour?

### Five versions on main still carry no tag

Unchanged from the last two rounds: v1.2.0, v1.3.1, v1.4.0, v1.5.0 are pushed and
green but untagged, with notes under *Unreleased*.

**Ask:** cut a release, or set a cadence?
