# KuberNation — Workstream A, Decomposed

**Planning artifact, not implementation guidance.** Per-phase guidance docs follow, one at a time, after each gate.

Governing plan: `kubernation-enabling-plan.md` §3 (Workstream A), §3.1a (instability inventory), §3.3 (reference frame).

---

## 1. Why this is decomposed

A is substantially larger than anything shipped in this series — A1–A5 in the plan, plus the slot-assignment algorithm, the persisted store, and the four additional instability sources found in §3.1a. Handed over as one unit it would contradict the pattern that has worked five times: a scoped doc, one session, a review, an explicit gate.

It is also the phase where **the plan's central thesis becomes testable for the first time**, which changes what the gates are for. See §5.

---

## 2. Prerequisite: the churn harness

Not a KuberNation change — a test asset, and every gate below depends on it.

A0's verification lesson was that correctness and perception need different instruments, and that distinction is sharper here:

| Question | Instrument |
|---|---|
| Does the layout function hold slots still? | **Synthetic fixtures.** Pure, deterministic, CI-runnable, no cluster |
| Does a stable map *read* as stable to a person? | **Scripted churn on a kwok fleet.** Cannot be faked |

So A's correctness needs no cluster at all, but A's *evaluation* does. The harness needs scripted scenarios for: rolling refresh, scale up, scale down, nodepool add, nodepool remove, and zone loss.

This is the standing decision from the v1.5.0 report, still unanswered, and A makes it blocking rather than merely advisable.

---

## 3. Small carry-forwards found

Cheap, but they will surprise a session mid-phase if not stated:

**Allocatable is not on the model.** `node_allocatable(node, key) -> Option<f64>` exists and is `pub`, but `NodeTile` carries only *ratios*. Capacity-derived extent (A2) needs the absolute carried forward. The function's doc comment says callers must not fabricate a default — so **a node without allocatable needs a declared fallback extent, not a silent zero.** The cost tests confirm this case occurs in practice.

**Instance type** is the extent fallback, via `node.kubernetes.io/instance-type`. Absent on bare metal and kind, so there is a third fallback: a single default extent, recorded as such.

---

## 4. The phases

### A-pre — Churn harness
**Deliverable:** kwok fleet with the six scripted scenarios above.
**Gate:** each scenario runs reproducibly and the current build survives it without panicking.
*Not a KuberNation commit. Do it first anyway.*

### A1 — The layout engine
**Deliverable:** a pure module. Slot identity `(zone, pool, ordinal)`; the pool-detection cascade with `pool_source` recorded; assignment by reuse-name → lowest-free-slot → append, retaining sparseness. Signature `(prior layout, observed nodes) → layout`.

No rendering, no persistence, no `build_world` changes. Persistence merely supplies `prior` across runs; in-session it is the previous frame.

**Gate:** a synthetic full-fleet rolling refresh moves **zero** slots. Surge (new node before old departs) behaves as decided — sparse, never silently compacted.

**This is a consumer-less phase**, so it needs A0's revised acceptance bar: contract review plus mutation survival as the objective floor.

### A2 — Wire it in
**Deliverable:** `build_world` consumes the layout instead of computing positions. Fixes instability sources 1 and 4. Extent becomes capacity-derived, with the fallback chain from §3.

**Gate: watch a rolling refresh on the churn fleet. Does the map hold still?**

**This is the phase that can kill A.** See §5.

### A3 — Interior stability
**Deliverable:** instability sources 2, 3 and 5. City slots within a province — the same principle one level down, and per §3.1a arguably more important than the province fix, since cities are the workloads people actually hunt for. Coast markers follow. Islands stop depending on continent height.

**Gate:** adding a workload to a node moves no existing city, anywhere.

### A4 — Persistence
**Deliverable:** per-cluster store keyed on context name with a fingerprint check. Ghost slots and their retention. Declared compaction as a recorded event.

**Gate:** layout survives restart unchanged. A rebuilt cluster behind the same context name is detected by fingerprint, regenerated, and **declared** rather than silently reshuffled.

### A5 — Succession and cataclysm
**Deliverable:** fresh-ground marking that ages out; cataclysm detection and recording; the two tiers kept distinct per plan §3.2.

**Gate:** a rolling refresh reads as a *wave* crossing the map. If it reads as noise, the tier boundary is wrong — most likely the ageing window.

### A6 — Graticule and declared frame
**Deliverable:** the recessive graticule with plate coordinates, and the reference frame stated in the legend per §3.3.

**Gate:** one person names a position from the map; another finds it without further explanation.

*Could move earlier — it is cheap and additive, and it is the invariant §7 (time) depends on. It is placed last only because A2's gate does not need it.*

---

## 5. Where A dies, and what that means

**A2 is the kill point,** and its gate is broader than it looks.

Plan §1 claims the map's advantage over K9s and Freelens is spatial memory. Spatial memory requires stability. A2 is the first moment that claim is testable — so if the map holds still and *still* isn't more useful, the failure is not A's. It is §1's.

That is worth naming plainly before starting, because the instinct at that gate will be to blame the implementation.

**Salvage is thinner than the cutaway fork's.** That fork left behind a real finding and two shipped features. A1's layout engine has no value if A2 fails — it is machinery for a map nobody wants. The honest position: A is a foundational bet with a genuine kill point, not an experiment with a salvage boundary.

What survives regardless: the churn harness, and the A2-gate answer itself, which is information about the product thesis that no amount of planning can produce.

---

## 6. Settled decisions — carried forward so sessions don't re-litigate

| Question | Decision |
|---|---|
| Surge handling | Sparseness, plus **declared** compaction. Never silent |
| Ghost retention | Same mechanism — compaction is the reclamation event |
| Pool detection | Fixed precedence list → `--pool-label` override → standard labels → single default. **Record which rule fired** (`pool_source`, mirroring `metric_source`) |
| Store identity | Context name as key, cluster-scoped UID as fingerprint |
| What sets extent | Node capacity, then instance type, then a declared default. Never pod count |
| Hierarchy | `continent ← zone`, `region ← pool ∩ zone`, `province ← slot`. Zone stays primary because zone is the failure domain |
| Migration | First run after A2 remakes the world once. Declare it — it is the first cataclysm |
| QoS classes | Standard three. The Burstable subdivision is a later render-time derivation and must not be called QoS |

---

## 7. Method notes for every phase

Carried from the A0 round, where the specification's threat model pointed at a hazard that could not fire while the real defect sat in what was called routine plumbing.

**Ask on the first pass: where does a summing step precede a comparing step?** That single question explains all three confirmed defects in this series — QoS summing containers before comparing request to limit, Substrate unioning node sets before applying the prevalence threshold, ghost-node inflation counting pods against a differently-scoped denominator. A's slot assignment aggregates nodes into pools and then compares ordinals; that is the same shape, and it is where to look first.

**Verify semantics, not only structure.** A0's seven structural claims all held; the defects were in what Kubernetes guarantees and what Rust syntax implies. Each phase's guidance will carry a second verification table for domain and language assumptions.

**A consumer-less phase needs the revised bar.** Contract review — *would this be wrong when the consumer arrives?* — plus mutation survival as an objective floor that needs no consumer at all. A1 is such a phase.
