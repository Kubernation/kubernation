# KuberNation — A-pre: The Churn Harness

**Implementation guidance**
**Goal:** a reproducible fleet-scale cluster that can be made to churn on command, so Workstream A's gates can actually be evaluated.
**Shape:** test asset. Little or no production code.

---

## 0. Verify before building

This document is mostly outside the Rust codebase, so the usual structural table is thin. What matters here is **environment and CLI facts**, which is where the A0 round's errors actually lived.

| # | Claim | Check |
|---|---|---|
| 1 | `kwok`/`kwokctl` is installable and can host ~100 nodes on the dev machine | run it |
| 2 | KuberNation has CLI flags for framing and capture (`--center`, `--zoom`, `--screenshot` or similar) | `Args` in `main.rs` — **verify the exact names**, they are quoted from memory in this thread |
| 3 | `--context` (or equivalent) selects the cluster | same |
| 4 | kwok nodes carry no `node.kubernetes.io/instance-type` unless the fixture sets it | inspect a created node |
| 5 | kwok nodes report `status.allocatable` | inspect a created node |
| 6 | metrics-server is **absent** under kwok, so `Metrics.pods` is empty | run against it |

**Claims 4–6 are the ones that will bite.** They decide whether the harness can exercise the fallback chains at all — see §4.

---

## 1. Why this exists

The v1.5.0 report established that the four-node dev cluster is **arithmetically incapable** of exercising fleet-shaped features, and that live verification only happened because a kwok fleet was seeded by hand. That was raised as a standing decision and never resolved.

Workstream A makes it blocking. A's entire claim is *behaviour under churn at scale*, and there is currently no way to produce churn at scale.

A0 also taught that correctness and perception need different instruments:

| Question | Instrument | Needs a cluster? |
|---|---|---|
| Does the layout function hold slots still? | Synthetic fixtures | **No** |
| Does a stable map *read* as stable? | This harness | **Yes** |

So the harness is not for correctness. It is for the A2 gate — the point where a human looks at a rolling refresh and judges whether the map holds. That judgment cannot be faked, and it is the gate that can kill A.

---

## 2. Deliverable

A directory (suggested `hack/churn/` or `tools/churn/`, follow existing repo convention) containing:

- Cluster bring-up and teardown
- A fleet fixture: ~100 nodes, multiple pools, multiple zones, a realistic workload spread
- Six scripted scenarios (§3)
- A capture helper that drives KuberNation's existing framing/screenshot flags

Plain shell plus YAML is sufficient and preferable. **Resist building a Rust tool for this** — it is a fixture, and a fixture that needs its own test suite has failed at being one.

---

## 3. The six scenarios

These are exactly the events Workstream A claims to handle. Each must be runnable on demand, in isolation, and be repeatable.

| # | Scenario | What it proves | A-phase gate |
|---|---|---|---|
| 1 | **Rolling refresh** — replace every node, new names, surge before drain | Slot survives its occupant | **A2 — the kill gate** |
| 2 | **Scale up** | New slots append; nothing existing moves | A2 |
| 3 | **Scale down** | Departure leaves a ghost, not a reshuffle | A4 |
| 4 | **Nodepool add** | A cataclysm is detected and recorded | A5 |
| 5 | **Nodepool remove** | Structural loss, distinct from routine churn | A5 |
| 6 | **Zone loss** | Continent vanishing; the failure-domain claim in the hierarchy | A5 |

**Scenario 1 is the one to get exactly right.** Immutable-infrastructure refreshes *surge*: the replacement node is created and Ready before the old one drains. That ordering is what makes slot assignment hard, and a script that deletes-then-creates would quietly test an easier problem than the real one. Make surge the default and expose the overlap as a parameter.

Scenario 1 also needs its workloads to move with it — pods rescheduling onto the new node — or it tests node churn without the city churn that A3 has to survive.

---

## 4. Fixture realism, and where kwok will lie to you

kwok fakes the kubelet. Nodes are whatever the YAML says, which is a strength for reproducibility and a hazard for realism.

Set deliberately, because A depends on all of them:

- **Zone labels** (`topology.kubernetes.io/zone`) — at least three, unevenly filled. Even zones hide layout bugs.
- **Pool labels** — use at least two *different* providers' conventions across pools (e.g. a GKE-style key on one, a Karpenter-style key on another) plus **one pool with no pool label at all**. That last one is what exercises the fallback cascade and the `pool_source` recording.
- **A pool spanning multiple zones** — this is the case that broke the naive hierarchy, and the fixture must contain it or the fix goes untested.
- **`status.allocatable`** on every node, with **at least one node deliberately missing it** — the cost tests confirm that case is real, and `node_allocatable` explicitly forbids fabricating a default.
- **Instance type** present on most nodes, absent on one pool — exercising extent fallback.
- **Heterogeneous capacity** — pools of different sizes, so capacity-derived extent produces visible variation rather than a uniform grid.

**What kwok cannot give you:** real metrics. Expect `Metrics.pods` empty and every `usage_ratio` to be `None`. That is fine for A — extent and slots are request- and capacity-derived — but it means **the harness cannot exercise A0's usage path**, and nobody should conclude from a green harness run that usage handling works.

Say this in the README. It is exactly the kind of unstated limitation that becomes a wrong claim three rounds later.

---

## 5. Capture

The A2 gate is a human judgment, so the harness must make the comparison easy and repeatable:

- Fixed camera framing across captures — same `--center`, same `--zoom` — or apparent movement is just the camera moving
- A capture before churn, one or more during, one after
- Deterministic output filenames, so a before/after pair can be flipped between

**A flipbook beats prose.** "Does the map hold still?" is answered by alternating two images, not by describing them. If the capture flags support it cheaply, a frame sequence across a rolling refresh is the single most valuable artifact this harness produces — it is also what A5's *wave* gate needs.

---

## 6. Acceptance

- [ ] Fleet of ~100 nodes stands up reproducibly from a clean machine
- [ ] All six scenarios run in isolation and repeat identically
- [ ] Scenario 1 **surges** — replacement Ready before predecessor drains — with configurable overlap
- [ ] Scenario 1 reschedules pods onto replacement nodes
- [ ] Fixture includes: multi-zone pool, unlabelled pool, node without allocatable, pool without instance type, uneven zones
- [ ] Capture helper produces consistently-framed before/during/after images
- [ ] Current `main` survives every scenario without panicking *(this is a baseline, not a pass — today's map is expected to reshuffle wildly)*
- [ ] README states the metrics limitation from §4

---

## 7. What this is not

**Not a correctness test for A.** A1's layout engine is verified by synthetic fixtures with no cluster at all. If the harness starts growing assertions about slot coordinates, that logic belongs in Rust unit tests instead.

**Not CI.** A hundred-node cluster per PR is not a reasonable ask. This is a local instrument for gate evaluation.

**Not a Rust project.** See §2.

---

## 8. Estimate

**Half a day to a day**, most of it in the fixture rather than the scripts. Scenario 1's surge ordering is the only fiddly part.

It is also the smallest phase in Workstream A and the one every later gate depends on, which is the argument for doing it first even though it ships nothing.
