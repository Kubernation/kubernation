# PDB item 1 — the eviction primitive

**Guidance:** `docs/kubernation-pdb-guidance.md` §2
**Follows:** `docs/reports/pdb-precheck.md` §4.1
**Version:** 1.30.0 · **Date:** 2026-08-19

**The app now obeys the budgets it is about to report on.** `evict_pod` goes
through `pods/eviction` rather than `DELETE`, so the apiserver enforces
PodDisruptionBudgets — and a refusal is surfaced as a refusal, naming the budget
in the apiserver's own words.

**Items 2 and 3 are not done.** §9 says land this first, and it is independently
correct: the app stops bypassing a constraint whether or not it ever renders it.

---

## 1. §1 — claims verified

All ten TRUE. Claim 4 confirmed against the pinned tree
(`k8s-openapi-0.27.1/src/v1_33/api/policy/v1/`), and `Api::<Pod>::evict` exists
in kube 3.1 (`subresource.rs`), hitting `/pods/{name}/eviction`.

---

## 2. The change, and where the budget's name comes from

The refusal was captured live before the classifier was written, rather than
guessed:

```json
{ "code": 429, "reason": "TooManyRequests",
  "message": "Cannot evict pod as it would violate the pod's disruption budget.",
  "details": { "causes": [ { "reason": "DisruptionBudget",
    "message": "The disruption budget web-strict needs 3 healthy pods and has 3 currently" } ] } }
```

**The budget's name is not a field** — it is inside the cause's human-readable
message. So the message is passed through **verbatim**: the match is on
`causes[].reason == "DisruptionBudget"`, which *is* machine-readable, and nothing
is parsed out of English prose. The app says what the apiserver said.

`classify_evict(code, causes, message)` is pure and takes plain values rather
than a `kube::Error`, so the distinction can be pinned without a cluster —
otherwise it would be testable only by evicting something.

### 2.1 §2.2 — the refusal is legible

```
kubernation-demo/web-f56f55fb4-scp2w is protected -
The disruption budget web-strict needs 3 healthy pods and has 3 currently
```

Not "evict failed". `EvictRefusal::{Budget, Other}` keeps the two apart at the
type level, so the GUI cannot accidentally collapse them, and other codes (403,
404, 409, 5xx) keep their existing handling.

### 2.2 §2.3 — the drill's decision: continue and report

Recorded as the guidance asks. **This is already the drill's behaviour and it is
the right one:** `run_chaos` runs every step and returns a `CommitRow` per step,
and the chaos window already renders every `!ok` row as `! {label}: {detail}`. So
a blocked eviction appears as a named per-step refusal.

Stopping midway would leave a half-drained node with no record of why; refusing
to start would need a pre-flight PDB read, which item 2 has not landed. What the
change adds is that the row's `detail` now names the budget instead of being an
apiserver string.

**One honest gap, recorded not fixed:** the drill's *verdict* line
(`chaos_outcome_summary`) describes the cluster's recovery, not the drill's
execution. A drill whose evictions were all blocked would report "stayed up — no
outage", which is true but could be read as the workload resisting rather than
the drill never landing. The per-step rows say otherwise, and item 3 — which
knows which budgets cover a node — is the natural place to fix it properly.

---

## 3. §6 — the gate, both halves

**Failure criteria, stated before the run** (§6.2): a 429 surfacing as a generic
error; the drill reporting success having partially drained; unprotected
indistinguishable from unknown; the derivation moving the rebuild time. The first
is the one item 1 can violate, and it does not.

**The app half** — the evict button on a pod covered by a blocking budget
(`minAvailable: 3` on a 3-replica workload, `disruptionsAllowed: 0`) produced the
toast in §2.1, and the pod count stayed at 3.

**§6.1's discrimination check** — the old primitive against the new, on the same
workload, seconds apart:

```
DELETE /pods/web-f56f55fb4-j78nf            -> HTTP 200   (budget ignored)
POST   /pods/web-f56f55fb4-scp2w/eviction   -> HTTP 429   (budget enforced)
```

That is the change, measured rather than asserted: the delete succeeds where the
eviction is refused.

---

## 4. §5 — mutation floor, and what it cannot reach

| | mutation | |
|---|---|---|
| M1 | a 429 is classified as an ordinary failure | caught |
| M2 | the cause is dropped, so the budget is never named | caught |
| M3 | a refusal reads as "eviction failed" | caught |

**§5's first named mutation — "make eviction a DELETE again" — is NOT caught by
any unit test**, and cannot be: the primitive is one line in an async fn that
needs a cluster, so no test can call it. It is caught by §3's discrimination
check, live.

That is the same structural limit D2-fix established for `main.rs`, and it is
reported rather than papered over: the classification is covered by tests, the
*choice of primitive* is covered only by the live gate.

---

## 5. §7 — standing questions

**1. Summing before comparing?** None.

**2. Unknown, or fabricated?** Item 1 does not yet make any claim about budgets,
so §3.3's constraint is not yet live — but the type already respects it:
`EvictRefusal::Budget` is only ever constructed from an apiserver refusal, never
inferred. Nothing in this change says a pod is unprotected.

**3. Two sections constraining one behaviour?** `chaos::node_protected` (control
plane) and PDB (availability) both answer *"can this node be disturbed"* from
different premises. Still not drift: `node_protected` gates which nodes a drill
may target; a budget refusal happens per pod at execution. They do not overlap,
and the chaos console does not now have two refusal paths that could disagree —
one refuses to start, the other reports per step.

**4. Consumers depending on the old meaning?** Enumerated before changing the
signature, per D2-fix: exactly two — `net.rs`'s evict button and `run_chaos`'s
`ChaosStep::Evict`. Both updated; the chaos arm maps to `String` so
`CommitRow`'s shape is unchanged.

**5. Inherited claims?** Ten, all from a pre-check written the same day, all
re-read. Claim 7 (`evict_pod` is a delete) is the one this phase exists to
falsify, and it was true.

**6. One side of a comparison moved?** `evict_pod`'s success case is unchanged —
a managed pod is still recreated, a bare pod still gone. Only the failure side
gained a distinction.

**7. Container adjacency?** None.

---

## 6. §8 — acceptance, so far

- [x] Eviction uses `pods/eviction`; 429-with-a-PDB-reason surfaces as a named refusal
- [x] The chaos drill's partial-failure behaviour decided and recorded (§2.2)
- [ ] PDB watched — **item 2, not started**
- [ ] `selector_matches` reused — item 3
- [ ] Derivation cost measured — item 3
- [ ] Unprotected/unknown distinguishable — item 3 (the type already respects it)
- [x] Gate run on kind, with the discrimination check (§3)
- [x] Failure criteria stated before the run
- [x] Mutations asserted applied; the one that cannot be unit-caught is named (§4)
- [x] Standing questions answered, claims tagged
- [x] Test suite green — 599 tests (`cargo test --workspace`; re-run under
      `cargo nextest run --workspace` on 2026-08-29, 621 tests, all passing)

435 core + 139 GUI tests; gui-smoke 57. The PDBs created for the gate were
removed and the cluster left as found.

**Next:** item 2 (watch PDBs, RBAC verb in the Charter) and item 3 (the
node-shaped derivation and its surfaces).
