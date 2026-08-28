# KuberNation — PodDisruptionBudget

**Implementation guidance**
**Goal:** observe PDBs, answer *"can I drain this node?"*, and make the app's own evict respect the budgets it reports.
**Gate:** a node with a blocking budget says so — and the evict button on a pod there fails for the same reason.

Follows: `docs/reports/pdb-precheck.md`

---

## 0. The decision, made

**Describe Kubernetes' constraint, and fix the eviction primitive so the app obeys it.**

The pre-check found `actions::evict_pod` is `api.delete(pod, ...)` — a plain DELETE, not the `pods/eviction` subresource. The apiserver enforces PDBs **only** on eviction, so the evict button ignores every budget and the chaos `NodeFailure` drill drains harder than `kubectl drain` would.

Reporting *"draining is blocked"* while the app's own button deletes the pod anyway would be the product contradicting itself. So both halves ship together, or neither does.

**This is also what the verb has always promised.** The v1.9-era decision named it "evict" because *"evict matches both k8s pod eviction and the 4X remove-an-inhabitant idea"* — the name was right and the implementation was not.

---

## 1. Verify before building

All `[A]`, from the pre-check (2026-08-19), which re-read its own eight inherited claims at source.

| # | Claim | Source |
|---|---|---|
| 1 | `PodDisruptionBudget`, `disruptionsAllowed` and `policy/v1` appear **nowhere** in the tree | §2 |
| 2 | The watch set is 12 `reflector::store` calls plus the Event ring | §1 |
| 3 | NetworkPolicy was the 13th reflector; the pattern is one `reflector::store::<T>()`, one `spawn_reflector`, a `WorldDelta` bit, a field on `ObservedWorld` | §2.1 |
| 4 | `k8s_openapi::api::policy::v1::PodDisruptionBudget` is in the pinned 0.27 / v1_33 — no dependency change | §2.1 |
| 5 | `netpol::selector_matches` implements k8s selector semantics — `matchLabels` **and** `matchExpressions`, In/NotIn/Exists/DoesNotExist, **fail-closed on an unknown operator** | §3.1 |
| 6 | `NodeTile.pods` is already built per node by `build_map` | §3.1 |
| 7 | `actions::evict_pod` is `api.delete(pod, &DeleteParams::default())` | §4.1 |
| 8 | The chaos `NodeFailure` drill evicts every pod on a node through that same primitive | §4.1 |
| 9 | `chaos::node_protected` is control-plane label policy, **not** availability — no drift with PDB | §4 |
| 10 | The Charter (#6) documents the app's required verbs | §2.1 |

**Claim 5 is the reuse that matters** — see §3.1.

---

## 2. Item 1 — the eviction primitive

**Do this first**, and separately from the watch. It is a behaviour change to a destructive action and deserves its own review attention.

### 2.1 The change

`pods/eviction` rather than `DELETE`. The apiserver then enforces PDBs and returns **429 Too Many Requests** when a budget would be violated, rather than succeeding.

### 2.2 The failure is the feature, and it must be legible

A DELETE that always succeeds becomes a call that can fail for a *good* reason. **That failure is the point**, and the surfacing has to say so:

- **429 with a PDB reason** → *"blocked by budget web-strict"*, not "eviction failed"
- Other failures (403, 404, 5xx) keep their existing handling
- The distinction must be visible; a generic error message would waste the whole change

This is the shape `pool_line`, `extent_line` and `GroundState::Unknown` established — a refusal that names its reason.

### 2.3 The chaos drill inherits it

Claim 8: `NodeFailure` evicts every pod on a node through the same primitive. After this change **the drill can partially fail**, which is a new state for it.

That is correct behaviour — it now drains the way `kubectl drain` does — but the drill's reporting must handle it. A drill that says "done" having evicted six of eight pods would be a new false claim in a codebase that just spent twelve rounds removing them.

**Decide and record:** does the drill stop, continue and report, or refuse to start when a budget would block it? `node_protected`'s refusal is the nearest precedent.

---

## 3. Item 2 — observe PDBs

### 3.1 The watch

Claim 3's pattern, exactly. NetworkPolicy is the template and it is recent enough to copy without archaeology.

**Carry the RBAC verb in the justification** (claim 10). This widens the read surface on a project whose privilege posture is deliberate, and the Charter is where it gets declared. A4's fingerprint round established the discipline: a new read is a decision, not a detail.

### 3.2 The derivation

The node-shaped claim: **which PDBs cover pods on this node, and do any allow zero disruptions?**

Both halves exist. Claim 6 gives node→pods. Claim 5 gives selector→pods, with correct semantics *including fail-closed on an unknown operator* — which matters, because a PDB whose selector cannot be evaluated must not silently cover nothing.

**Reuse `selector_matches`.** A second selector implementation would mean the walls overlay and the PDB feature could disagree about what a selector covers, which is the drift this project has paid for nine times.

**Cost is `O(pods × PDBs-in-namespace)`** — the pre-check calls this "almost certainly inside the 500-node rebuild budget, but that is a prediction and would need measuring, not asserting." **Measure it.** The model rebuild is 5.7ms at 500 nodes / 5000 pods; if this moves that materially, the derivation needs caching and that changes the phase.

### 3.3 Unprotected is not unknown

The pre-check's §6, and it constrains the whole feature:

> A workload with no PDB is **unprotected**, which is a real state. A workload the app has not looked at is **unknown**.

Today every workload is unknown. So:

- The claim **cannot ship ahead of the watch** — no inferring absence
- After the watch, the two states must stay **distinguishable**. A node with no covering budgets is drainable; a node whose PDBs could not be read is not known to be
- If the watch fails or is denied by RBAC, that is **unknown**, and every surface must say so rather than reporting "no budgets"

This is the unearned all-clear this codebase refuses everywhere — `SubstrateReport` falling back to terrain, `GroundState::Unknown` reaching the panel, `extent_line` speaking a guessed size.

---

## 4. Item 3 — surface it

Candidates from the pre-check §5, and the recommendation:

**Take the attention concern.** `pool_confinement` is the precedent: a pure fact appended to a concern's `detail`, riding the sidebar, the Oracle bundle and the postmortem for free. *"draining blocked by web-strict"* is that exact shape, and it needs no new geometry.

**And the node window / SELECTION**, which is the node-shaped claim's natural home — a province *is* its node, so this is the one recent item that escapes the plurality problem entirely (pre-check §3.2).

**Not a map mark, at least not yet.** *Can this node be drained* is a question you ask about one node, not a fleet-wide pattern, and the map is not short of ink. If a fleet-wide view turns out to be wanted, that is a later scoping with its own justification.

**Not the workload surfaces** in this phase. *Protected, and by how much* is honest and is a different feature.

---

## 5. Tests

**Eviction:**
- [ ] A 429 with a PDB reason surfaces as a named refusal, not a generic failure
- [ ] Other failure codes keep their existing handling
- [ ] The chaos drill's partial-failure behaviour matches §2.3's recorded decision

**Derivation:**
- [ ] A node with a blocking budget reports blocked, naming the budget
- [ ] A node covered only by permissive budgets reports drainable
- [ ] A node with no covering budgets reports drainable — **and is distinguishable from unknown** (§3.3)
- [ ] A PDB with an unevaluable selector fails closed, per claim 5
- [ ] A PDB in another namespace does not cover this namespace's pods

**Unknown:**
- [ ] With the watch unavailable, every surface says unknown rather than "no budgets"

**Mutation floor, asserted applied** — eight false survivals last stretch from `cargo fmt` reflowing targets:

- Make eviction a DELETE again → the 429 test fails
- Make an unread PDB set report "no budgets" → the unknown test fails
- Reimplement selector matching locally → an agreement test with `netpol` fails

---

## 6. The gate

**A node with a blocking budget says so — and the evict button on a pod there fails for the same reason.**

Both halves, in one session, on kind. The pre-check's fixture is the model: `web-strict` with `minAvailable` equal to the replica count gives `disruptionsAllowed: 0`; `db-loose` with `maxUnavailable: 1` gives a permissive contrast.

### 6.1 The discrimination check

**Run the eviction half against the old primitive.** A DELETE succeeds where an eviction is refused — if both behave the same, the change did not take.

And run the derivation against a cluster with **no** PDBs: every node should report drainable, not blocked. A derivation that reports blocked everywhere would pass a single-node test for the wrong reason.

### 6.2 Failure criteria, stated in advance

- A 429 surfaces as a generic error
- The drill reports success having partially drained
- A node with no PDBs is indistinguishable from a node whose PDBs were not read
- The derivation moves the model rebuild time materially (§3.2)

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 2 is §3.3** and it is the phase's central constraint.

**Question 4 is item 1:** `evict_pod`'s callers get a new failure mode. The pod row's button and the chaos drill (claim 8) are the two known; enumerate rather than assume, per D2-fix's finding that the consumer which bites is the one not named.

**Question 3:** `node_protected` and PDB both answer *"can this node be disturbed"* from different premises (claim 9). They are not drift today — check they do not become so, and that the chaos console does not end up with two refusal paths that disagree.

---

## 8. Acceptance

- [ ] Eviction uses `pods/eviction`; 429-with-a-PDB-reason surfaces as a named refusal
- [ ] The chaos drill's partial-failure behaviour decided and recorded (§2.3)
- [ ] PDB watched, following the NetworkPolicy pattern; the RBAC verb declared in the Charter
- [ ] `selector_matches` reused, not reimplemented
- [ ] Derivation cost measured against the rebuild budget, not predicted
- [ ] Unprotected and unknown distinguishable everywhere (§3.3)
- [ ] Gate run on kind, both halves, with both discrimination checks
- [ ] Failure criteria stated before the run
- [ ] Mutations asserted applied
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 9. Estimate

**One day.** The watch is a copied pattern and the derivation reuses two existing authorities; item 1's failure surfacing and §2.3's drill decision are the work.

**Land item 1 first.** It is a behaviour change to a destructive action, it is independently correct, and it means the derivation ships into an app that already obeys the constraint it reports.
