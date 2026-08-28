# PodDisruptionBudget — pre-check

**Guidance:** `docs/kubernation-pdb-precheck.md`
**Date:** 2026-08-19 · **No product change, no instrument built.**

**PDB is not observed at all** — not watched, not read on demand, absent from the
tree. So this is an *acquisition* item, not a rendering one.

**The node-shaped claim is derivable, and it is the strongest version**: it
answers §0's question, names a culprit, and — uniquely among recent items — is a
claim about a **node**, which has a real map position.

**And the adjacent finding is the sharper one:** KuberNation's own "evict" is a
plain `DELETE`, not the Eviction API, so **the app already performs the operation
a PDB constrains while bypassing the constraint.** Its chaos "cordon + drain"
drill does the same. A feature that reports "draining this node is blocked" would
contradict the app's own buttons.

---

## 1. §1 — claims verified

All eight TRUE. Claim 1 was flagged for re-reading and is correct: **12
`reflector::store` calls plus the Event ring = thirteen watched kinds** (Node,
Pod, Deployment, ReplicaSet, StatefulSet, DaemonSet, Job, CronJob, PVC, Service,
Ingress, NetworkPolicy, Event).

---

## 2. §2 — is it observed? No.

`PodDisruptionBudget`, `disruptionsAllowed` and `policy/v1` appear **nowhere** in
the tree. Not a reflector, not a `browse.rs`-style on-demand read, not a mention.

### 2.1 What a watch would cost

Structurally it is the **NetworkPolicy precedent exactly** — that was the 13th
reflector and the pattern is one `reflector::store::<T>()`, one `spawn_reflector`,
a `WorldDelta` bit, and a field on `ObservedWorld`. Small and repeated.

The real costs are elsewhere:

| | |
|---|---|
| **Type** | `k8s_openapi::api::policy::v1::PodDisruptionBudget` is present in the pinned 0.27 / v1_33 — no dependency change |
| **RBAC** | a new `list`/`watch` on `policy/v1 poddisruptionbudgets`. The project's posture is read-by-default, and the Charter (#6) documents the app's required verbs — so widening the read surface has a place to be declared, and should be |
| **Memory** | negligible; PDBs are at most one per protected workload |
| **Staleness** | §2 is right that a one-shot read will not do: `disruptionsAllowed` moves as pods go ready/unready, so it is stale precisely mid-drain, which is when it matters |

**So: a watch, and the cost is ordinary.** The item is "acquire and surface", and
its justification has to carry the new read verb — not a large cost, but a real
one on this project's terms.

---

## 3. §3.1 — which claim, and is the node-shaped one derivable?

**Yes, and it was derived by hand on kind rather than argued.** Two budgets were
created deliberately (§5), giving both a blocking and a permissive state:

```
NS                 NAME         MIN   MAXU   ALLOWED  HEALTHY  DESIRED  EXPECTED
kubernation-demo   web-strict   3     -      0        3        3        3
kubernation-demo   db-loose     -     1      1        2        1        2
```

Intersecting pods-on-node with covering budgets:

```
node                       pods  budgets covering pods here (allowed)
kubernation-control-plane     9  none
kubernation-worker            8  db-loose(1), web-strict(0)   <-- DRAIN BLOCKED
kubernation-worker2           4  db-loose(1)
kubernation-worker3           6  none
```

**Exactly one of four nodes is blocked, and the reason is nameable.** That is the
third claim in §3.1's table — the operational one — and it is available.

### 3.1 The derivation has an existing authority

`netpol::selector_matches` already does label-selector matching with exact k8s
semantics — `matchLabels` **and** `matchExpressions`, In/NotIn/Exists/
DoesNotExist, and **fail-closed on an unknown operator**. That is precisely what
PDB→pod matching needs, and reusing it means the walls overlay and any PDB
feature cannot disagree about what a selector covers.

The node→pods half also exists: `NodeTile.pods` is already built per node by
`build_map`.

**Cost is `O(pods × PDBs-in-namespace)`** — small, since PDBs are few and each pod
only checks its own namespace. It is almost certainly inside the 500-node rebuild
budget, but that is a prediction and would need measuring, not asserting.

### 3.2 And this one escapes the plurality problem

§3.2 warns that a workload's map position is a label, not a location — the thing
v1.26.0 and v1.27.0 spent two rounds correcting.

**A node-shaped claim does not inherit it.** A province *is* its node. So PDB may
be the first item in this project whose natural rendering is a province rather
than a city — worth stating, because it is the opposite of every recent finding.

---

## 4. §7 — the drift check, and what it turned up instead

**`chaos::node_protected` is NOT the same concept.** It tests for
`node-role.kubernetes.io/control-plane` / `master` labels — a policy about which
nodes chaos refuses to touch. PDB is an availability floor. No drift, and no
partial home: the item does not shrink.

### 4.1 The finding: the app performs the operation and ignores the constraint

`actions::evict_pod` is:

```rust
api.delete(pod, &DeleteParams::default())
```

A plain `DELETE` on the pod resource — **not** the `pods/eviction` subresource.
The apiserver enforces PodDisruptionBudgets **only** on eviction, so:

- the pod row's **evict** button ignores every PDB;
- the chaos **`NodeFailure` ("cordon + drain")** drill, which evicts every pod on
  a node through the same primitive, drains harder than `kubectl drain` would.

The internal decision log is explicit that this is a delete, so it is not a
hidden behaviour — but the user-facing verb is "evict", named (per the v1.9-era
decision) because *"evict matches both k8s pod eviction and the 4X remove-an-
inhabitant idea"*.

**Why it matters here specifically:** a feature that says *"draining this node is
blocked by web-strict"* while the app's own evict button deletes that pod anyway
would be the product contradicting itself. Any PDB phase has to decide whether it
is describing **Kubernetes' constraint** or **KuberNation's own behaviour**, and
today those differ.

That is a scoping input, not a defect to fix here.

---

## 5. §4 — candidate surfaces, recorded, none chosen

| Surface | Fit |
|---|---|
| **Attention concern** | `pool_confinement` is the precedent — a pure fact appended to a concern's `detail`, riding the sidebar, the Oracle bundle and the postmortem for free. *"draining blocked by web-strict"* is that exact shape |
| **Node window / SELECTION** | The node-shaped claim's natural home; no new geometry, and the province is genuinely the node |
| **A map mark on the province** | Possible *because* the claim is node-shaped — but it must earn ink, and "can be drained" is a question you ask about one node, not a fleet-wide pattern |
| **City / workload surfaces** | The workload-shaped claims (protected, and by how much). Honest, but not §0's question |
| **Chaos console** | Not a home (§4) — but the place the contradiction in §4.1 is sharpest |

---

## 6. §7 — standing questions

**2. Unknown, or fabricated?** The decisive one, and it constrains the phase:
**a workload with no PDB is *unprotected*, which is a real state; a workload the
app has not looked at is *unknown*.** Today every workload is unknown. A surface
that said "no budget" before PDB is watched would be the unearned all-clear this
codebase refuses — so the claim cannot ship ahead of the watch, and the two
states must stay distinguishable after it.

**3. Two sections constraining one behaviour?** Checked, and clean: `node_protected`
is control-plane policy, not availability (§4). The genuine tension is §4.1 —
Kubernetes' constraint versus KuberNation's own delete — and it is between the
product and the API, not between two of the product's own mechanisms.

**5. Inherited claims?** All eight re-read. Claim 1 was the flagged one and held.

---

## 7. §8 — acceptance

- [x] §2 answered: **not observed**; a watch is the NetworkPolicy pattern, and the cost is a new RBAC read verb (§2.1)
- [x] §3.1's three claims distinguished; the node-shaped one **is** derivable, with an existing selector authority (§3)
- [x] The derivation's shape and cost stated — and flagged as a prediction needing measurement, not asserted
- [x] `node_protected` checked against PDB's concept — different (§4)
- [x] Candidate surfaces recorded; **none chosen** (§5)
- [x] The blocking case constructed deliberately and observed (§3)
- [x] Prevalence: **unmeasured** — see below
- [x] No product code changed, no instrument built; the two PDBs created for §3 were removed and the cluster left as found

### 7.1 Prevalence — unmeasured, and neither cluster can answer it

kind had **zero** PDBs before this pre-check created them; the churn fleet is kwok
and its PDB status would not be computed by a real controller anyway. PDBs are
opt-in and tend to exist on platform-managed production workloads and not on
ad-hoc ones — so "how many clusters would light this feature up" is a question
about production, which neither test cluster stands in for. Recorded as unmeasured
rather than guessed.

---

## 8. What this decides

Per §6's table, two rows fire together:

- **"Not observed, and a watch is cheap"** → scope it as *acquire and surface*,
  and carry the new read verb in the justification.
- **"The node-shaped claim is derivable and cheap"** → **the strongest version**,
  and the one that answers §0.

Plus one the guidance did not anticipate: **the app already performs the
constrained operation without the constraint** (§4.1). That does not block the
item, but a phase should decide up front whether it describes Kubernetes'
behaviour or KuberNation's, because they currently differ.
