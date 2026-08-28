# KuberNation — PodDisruptionBudget: Pre-check

**Measurement guidance**
**Goal:** establish whether PDB is observable, what it would let the product claim, and about which entity — before anything is scoped.
**No product change.** The output is an inventory, a data question answered, and a decision about whether this is a map item at all.

---

## 0. Why this is the highest-value remaining item, and why it is a pre-check

The enabling plan's §10 coverage review named PDB the standout omission:

> PDB is the direct answer to the most common node-lifecycle question, *"can I drain this node?"*, and it is why drains hang.

And it composes with what Workstream A built. A5 made rolling replacement a first-class event — succession marks fresh ground, cataclysm marks structural change. **A PDB is what constrains that event.** Modelling the thing that resists succession is the missing half of a theme already half-built.

**But two questions have to be answered before it is scoped**, and both have killed or reshaped phases in this project:

1. **Is PDB observed at all?** If not, this needs a new watch — a new read surface on a project whose privilege posture is deliberate
2. **Which entity does it describe?** Its operational meaning is about a *node* you want to drain; its data is per-*workload*. The plurality round just established a workload's map position is a label, not a location claim — so a PDB rendered on a city inherits that whole question

The last five pre-checks each changed the scope of what followed. This one has a specific reason to.

---

## 1. Verify before building

All `[A]`. VOR was unavailable when this was written.

| # | Claim | Source |
|---|---|---|
| 1 | The watch set is **thirteen kinds** | T0 |
| 2 | `k8s/browse.rs` is the on-demand read precedent, and reports what it could not enumerate rather than omitting silently | A4 §4.1 |
| 3 | `logs::first_container` is a single named `api.get` degrading to `None` — the one-shot read shape | A4 |
| 4 | The event ring's cap is 500 **distinct keys**, and ordinary batch work reaches it (724 measured) | T0 consolidation |
| 5 | A city's province is its plurality node; a workload at fleet scale is typically at 4–12% plurality | plurality pre-check |
| 6 | `workloads_on_node` is the node→workload authority; `workload_pods_by_node` is its inverse, pinned equal to the siting census | v1.27.0 |
| 7 | `attention::build` is where operator-facing concerns are produced, and it reads `now` internally — the one clock exception in `state/` | v1.9.1 |
| 8 | `pool_confinement` is the precedent for a concern that appends a fact and refuses when the data does not support it | v1.21.0 |

**Claims 6 and 7 are the ones that decide §3 and §4.**

---

## 2. Is it observed?

**Answer this first. It bounds everything else.**

- [ ] Is `PodDisruptionBudget` in the watch set (claim 1)?
- [ ] If not, what would adding it cost — RBAC, a new informer, memory?
- [ ] Is a one-shot read viable instead (claims 2, 3), or does the answer need to be live?

**PDB status is not static.** `disruptionsAllowed` changes as pods become ready or unready, so a one-shot read at launch would go stale in exactly the situation the operator cares about — mid-drain. That argues for a watch, which argues for costing it properly.

**If it is not observed and a watch is expensive, that is the finding**, and it changes the item from "render a fact" to "acquire a fact", which is a different phase with a different justification.

---

## 3. What would it actually claim, and about what?

The crux, and the reason this is a pre-check.

### 3.1 The data is per-workload; the question is per-node

A PDB selects pods by label and states a floor — `minAvailable` or `maxUnavailable`. Its status carries `disruptionsAllowed`, `currentHealthy`, `desiredHealthy`, `expectedPods`.

Those are facts about a **workload's** availability. But *"can I drain this node?"* is a question about a **node**, and it is answered by the intersection: **which PDBs cover pods on this node, and do any of them currently allow zero disruptions?**

That intersection is computable — claim 6 gives node→workload and its inverse — but it is a *derived* fact, not a field on either object.

**Establish which of these the product should claim**, because they are different features:

| Claim | Entity | Needs |
|---|---|---|
| *this workload is protected, and by how much* | workload | the PDB alone |
| *this workload is currently blocking disruption* | workload | PDB status |
| **"draining this node would be blocked"** | **node** | the intersection over pods on that node |

The third is the one §0 quotes as the operational question. **Check whether it is derivable from what would be observed**, and whether the derivation is cheap enough to run per tick.

### 3.2 And a workload's map position is a label

Claim 5. If the answer is rendered on a city, it sits on a province holding 4–12% of the workload's pods — which is the exact confusion v1.26.0 and v1.27.0 spent two rounds correcting.

**So a PDB mark on a city would be a location claim about a non-location.** That does not rule it out — the SELECTION box already carries workload facts honestly — but it rules out *the map* as the surface, unless the claim is the node-shaped one.

**A node-shaped claim has a real map position.** That is worth noticing: PDB may be the first item in this project whose natural rendering is a province rather than a city.

---

## 4. Where would it go?

Record candidates, do not choose.

| Surface | Fit |
|---|---|
| **Attention concern** | Claim 8's `pool_confinement` is the precedent — a fact appended to a concern, pure and unit-tested, riding surfaces that already show `detail`. *"draining is blocked by 2 budgets"* is exactly that shape |
| **Node window / SELECTION** | The node-shaped claim's natural home; no new geometry |
| **A map mark on the province** | Only if §3.1's third claim is what is built, and only if it earns ink on a map that is not short of it |
| **The city / workload surfaces** | The workload-shaped claims; honest, but not the operational question |
| **The Chaos console** | It already guards node-subject experiments with `node_protected` — check whether that is the same concept under another name |

**That last row is worth checking early.** If Chaos already reasons about whether a node can be disturbed, PDB may have a partial home already, and the item is smaller than it looks.

---

## 5. Method

**Read the code, do not infer it.** Four wrong diagnoses in recent rounds came from reasoning about a path rather than reading it.

**Do not build an instrument.** Fourteen instrument failures in this workstream. `kubectl` and the running app cover this.

**Measure on kind.** kwok emits almost no events and its pods are fake; PDB status is computed by a real controller. A PDB on the churn fleet may report nothing meaningful — **check that before trusting any number from it.**

**Construct the blocking case deliberately.** A PDB that allows disruptions is the uninteresting state. `minAvailable` equal to the replica count, or a workload with an unready pod, produces `disruptionsAllowed: 0` — which is the state the feature exists to show, and the one a quiet cluster will not produce on its own.

---

## 6. What this decides

| Finding | Consequence |
|---|---|
| Not observed, and a watch is cheap | Scope it; the item is "observe and surface" |
| Not observed, and a watch is expensive | A different phase, justified on acquisition cost, not on rendering |
| The node-shaped claim is derivable and cheap | **The strongest version.** It answers §0's question and has a real map position |
| Only workload-shaped claims are available | Smaller and panel-shaped; the map stays out of it |
| Chaos already has the concept | The item shrinks to connecting what exists |
| PDBs are rare on real clusters | Worth knowing — a feature that is invisible on most clusters is a different proposition |

---

## 7. Standing questions

**2. Unknown, or fabricated?** A workload with no PDB is **unprotected**, which is a real state and different from *unknown*. If PDB is not observed at all, every workload is unknown and none is unprotected — and a surface that says "no budget" when it has not looked would be the unearned all-clear this codebase refuses.

**3. Two sections constraining one behaviour?** Check `node_protected` in the Chaos console against whatever PDB would introduce. Two mechanisms answering *"can this node be disturbed"* would be the drift this project has paid for eight times.

**5. Inherited claims?** All of §1 comes from reports, several weeks old in code terms. Claim 1's thirteen kinds is the one to re-read — the watch set may have changed.

---

## 8. Acceptance

- [ ] §2 answered: observed or not, and the cost if not
- [ ] §3.1's three claims distinguished, and which are derivable stated
- [ ] The node-shaped derivation costed per tick, or ruled out
- [ ] `node_protected` checked against PDB's concept (§7)
- [ ] Candidate surfaces recorded with their fit; **none chosen**
- [ ] The blocking case constructed deliberately and observed (§5)
- [ ] PDB prevalence on a real-ish cluster noted, or recorded as unmeasured
- [ ] No product code changed, no instrument built

---

## 9. Estimate

**Two to three hours.** §2 is a grep and a cost estimate; §3 is reading; §5's blocking case is a small manifest.
