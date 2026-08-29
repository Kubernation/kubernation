# PDB item 2 — observing the budgets

**Guidance:** `docs/kubernation-pdb-guidance.md` §3
**Follows:** `docs/reports/pdb-item1-eviction.md`
**Version:** 1.31.0 · **Date:** 2026-08-28

PodDisruptionBudget becomes the 14th reflector, and `state/pdb.rs` answers the
node-shaped question: *can this node be drained, and if not, which budget says
no?* Read-only — no new write verb.

**Item 3 (the surfaces) is not started.** The derivation is on `Models` and has
a headless instrument; the attention concern and the province panel are next.

---

## 1. §1 — claims verified

All ten re-read at source. Two were true and **insufficient**, which is the
phase's first finding.

**Claim 5** (`selector_matches` implements k8s selector semantics) is true about
matchLabels, matchExpressions and the fail-closed unknown operator — and its
`None` case is **the opposite of what PDB needs**. `policy/v1`, in the pinned
tree's own doc comment:

> *"A null selector will match no pods, while an empty ({}) selector will select
> all pods within the namespace."*

`selector_matches(None, ..)` returns `true`, because a NetworkPolicy's absent
`podSelector` **is** namespace-wide. So "reuse `selector_matches`" is right about
the expressions and wrong if applied to the whole selector: a null-selector PDB
would have covered every pod in its namespace and blocked every node it touched.
`pdb::covers` handles the null case and delegates the rest, with an agreement
test asserting the two never diverge on the expression semantics.

**Claim 6** (`NodeTile.pods` is already built per node) is true and unusable:
`PodGlyph` carries no labels, and a PDB selector matches on pod labels. The
derivation therefore reads `ObservedWorld` directly, like
`netpol::coverage_report` and `substrate::coverage_report` — which is the
precedent the guidance cites anyway, and avoids hanging a label map off 5000
glyphs rebuilt every tick.

---

## 2. §3.3 — unprotected is not unknown

The constraint the phase turns on, and **the existing reflector pattern cannot
express it**. A denied LIST makes `spawn_reflector` log a warning and back off
forever; the store stays empty and reads exactly like a cluster with no budgets.
`ObservedWorld`'s own doc for `networkpolicies` records that conflation as
accepted ("Empty when none observed / RBAC-denied → unwalled") because there the
fail-safe direction is the same. Here it is not: reporting a node drainable on
the strength of a denied LIST is the unearned all-clear.

So the pattern is extended by one flag, `pdbs_synced`, set by a task awaiting
`Store::wait_until_ready`.

**The signal has to come from the watcher, not from the objects.** An
`AtomicBool` flipped on the first observed *object* would never flip on a
cluster with no budgets — the case that most needs to be distinguishable — and
would have read "unknown" forever on a healthy realm. `wait_until_ready`
resolves on the first `InitDone`, which kube's writer emits even when the initial
list was empty (`reflector/store.rs`: `ready_tx.init(())` in the `InitDone` arm,
after the buffer swap, unconditionally). Verified at source before use.

**The single-waker rule is respected, not waived.** CLAUDE.md forbids new
`wait_until_ready` call sites; the recorded *reason* is that kube's `DelayedInit`
holds one waker slot, so two tasks awaiting the **same** store race and the loser
is never woken. Re-read in kube 3.1 (`utils/delayed_init.rs` — still a
`oneshot::Receiver` under a mutex): the hazard is per-store, and nothing else
awaits the PDB store. One waiter, one store.

It is never cleared. A watch error after a successful list leaves the last-known
set, which is still an answer — the same reasoning as the metrics ring surviving
a failed poll.

---

## 3. Rule 2 — a stale `disruptionsAllowed` is not a number

Also from the API's own text:

> *"DisruptionsAllowed and other status information is valid only if
> observedGeneration equals to PDB's object generation."*

So there are three per-budget states, not two: refusing, permissive, and
**unreadable** — a budget the disruption controller has not caught up with, or
has never reconciled (`status: None`). An unreadable budget covering a node makes
that node `Unknown`, never `Allowed`. A definite refusal outranks it: we know a
blocked node is blocked.

---

## 4. §3.2 — cost, measured rather than predicted

The guidance says measure it, and warns that a material change to the rebuild
would change the phase. Measured on a 500-node / 5000-pod fixture:

| case | drain_report | note |
|---|---|---|
| healthy realm (no budget could refuse) | **0ms** | short-circuited before the pod walk |
| every workload at its limit (100 refusing budgets) | 9.6ms | first version |
| same, memoized by label set | 3.9ms | |
| same, no allocation in the hot loop | **3.4ms** | shipped |

Three things worth stating.

**The measurement is of the case that costs.** A fixture with permissive budgets
would have reported ~0ms and told me nothing — every workload is deliberately at
`disruptionsAllowed: 0`, which is a cluster in serious trouble and the honest
worst case.

**Only budgets that could refuse are matched against pods**, because a permissive
budget cannot change any node's verdict. That is what makes the healthy case
free, and it is a real filter rather than a heuristic — pinned by a mutation that
inverts it.

**The memoization is keyed on (namespace, labels)**, and the namespace half is
load-bearing: see §6.

The A/B on the whole rebuild is inside the noise (~8.2/7.9/7.4ms with, ~8.7/8.4/8.7ms
without), so §6.2's fourth failure criterion is not met.

---

## 5. The defect item 2 found in item 1

**`can_evict_pod` was probing the wrong verb.** It ran a `SelfSubjectAccessReview`
for `delete pods` — correct while `evict_pod` was a DELETE, and wrong the moment
item 1 moved it to `pods/eviction`, which RBAC authorizes as **`create` on the
`pods/eviction` subresource**. Exactly the defect the Charter round fixed once
already for `patch` versus `update` on deployments, and I introduced it yesterday
without noticing.

It gave a false verdict in **both** directions, proven on the cluster with two
throwaway roles:

| identity | `delete pods` | `create pods/eviction` |
|---|---|---|
| admin | yes | yes |
| `pdb-blind` | no | no |
| **`evict-only`** | **no** | **yes** |

And end-to-end on one pod, seconds apart, as `evict-only`:

```
POST   /pods/agent-db5sp/eviction   -> 201 Success
DELETE /pods/agent-db5sp            -> 403 Forbidden
```

That is the **exact inverse** of item 1's check (`DELETE` 200 where eviction
429), so the two verbs are now shown independent in both directions — which is
why probing one for the other cannot work. Fixed at the probe, at the chaos
pre-flight's label and message, and in the Charter grid; pinned by a regression
test named after the failure.

**An instrument trap, caught in flight.** `kubectl auth can-i create
pods/eviction` reads `eviction` as a **resource name**, not a subresource — it
answers *"may I create a pod called eviction"*. As admin it says `yes`, which is
the right answer to the wrong question, and I nearly validated the fix against
it. The correct form is `--subresource=eviction`, which is what exposed
`evict-only` as `no`→`yes`. Sixteenth catalogued case of an instrument emitting a
plausible number for an unrelated reason.

**The Charter also declares the new read.** `list poddisruptionbudgets` is a verb
the app did not previously require; §3.1's discipline is that a new read is a
decision, and the Charter is where it is declared — with a test, because a denied
read must be visible as denied for the "unknown" story to hold.

---

## 6. §5 — mutation floor, asserted applied

| | mutation | |
|---|---|---|
| M2 | an unread PDB set reports "no budgets" | caught |
| M3 | selector matching reimplemented locally | caught |
| M4 | a null selector treated as namespace-wide | caught |
| M5 | a stale `disruptionsAllowed` read as a number | caught |
| M6 | a terminal pod counted as a disruption | caught |
| M7 | the Charter drops the eviction verb | caught |
| M8 | a refusal stops naming the budget | caught |
| M9 | `could_refuse` filters out a budget that would refuse | caught |
| M10 | the healthy-path short-circuit removed | **survived — correctly** |
| M11 | the memo key drops the namespace | **survived, then closed** |

**M10 is a true survival and is reported as one.** The short-circuit changes no
answer, only cost, and a perf assertion is a budget rather than a regression
guard. Nothing here can catch it, and nothing should pretend to.

**M11 was the real one.** The coverage memo holds indices into *that namespace's*
candidate list, so a key without the namespace hands one namespace's answer to
another — and `app=web` is the most common label in Kubernetes, so the collision
is the ordinary case, not a corner. Every existing test had one namespace with
candidate budgets, where all indices mean the same thing: the fixture could not
express the failure. Closed with two namespaces running identically-labelled pods
under differently-targeted budgets, where the collision names a budget that does
not cover the pod. Same shape as D2's M-D and plurality's M4 — an optimisation
outrunning what the fixture could see.

M1 from §5 ("make eviction a DELETE again") remains **not unit-catchable**, for
the reason item 1 recorded: the primitive is one line in an async fn needing a
cluster. The same now applies to `can_evict_pod`'s verb — the Charter grid is
pinned, the probe itself is covered only by §7's live run.

---

## 7. §6 — the gate

**Failure criteria, stated before the run:** a node reporting Blocked with no
PDBs present; covered nodes not reporting Blocked or not naming the budget; a
denied LIST reading as "drainable" or "0 budgets"; the derivation moving the
rebuild materially. None occurred.

Instrument: `cargo run -p kubernation-core --example drain`, headless (item 2
has no user-facing surface, and a derivation about a destructive operation should
not ship on unit tests alone).

**§6.1, first — no PDBs at all**, which had to be run before the positive case so
a blocked-everywhere derivation could not pass for the wrong reason:

```
budgets: 0 read
  ok   kubernation-control-plane — no budget blocks a drain
  ok   kubernation-worker  ...  ok   kubernation-worker2  ...  ok   kubernation-worker3
```

**The positive case.** Expectation written down first, from where the pods
actually run (`web`: worker ×2, worker2 ×1): `web-strict` (`minAvailable: 3` on 3
replicas → `disruptionsAllowed: 0`) blocks worker and worker2 only; `db-loose`
(`maxUnavailable: 1`) is permissive and must appear nowhere.

```
budgets: 2 read
  ok   kubernation-control-plane — no budget blocks a drain
  STOP kubernation-worker  — draining blocked by kubernation-demo/web-strict
  STOP kubernation-worker2 — draining blocked by kubernation-demo/web-strict
  ok   kubernation-worker3 — no budget blocks a drain
```

**§3.3 live**, through a kubeconfig for a ServiceAccount denied `list
poddisruptionbudgets` — same cluster, same nodes, **the budgets still in place**:

```
budgets: NOT READ — every node below is unknown, not drainable
  ?    kubernation-worker — disruption budgets not read - drain cost unknown
```

The pair of runs is itself the discrimination: *no budgets* reads `0 read` /
drainable and *budgets unread* reads `NOT READ` / unknown, where a naive
implementation gives one answer to both.

Cluster left as found: PDBs, roles, bindings and ServiceAccounts removed; the
evicted DaemonSet pod recreated itself (`agent-695xt Running`), which is the
"a managed pod comes back" property the evict decision documents.

---

## 8. §7 — standing questions

**1. Summing before comparing?** None. `disruptionsAllowed` is compared per
budget, never totalled across budgets — a sum would be meaningless, since two
budgets each allowing one disruption do not allow two of the same pod.

**2. Unknown, or fabricated?** This is the phase (§2, §3). Three reducers could
have fabricated and do not: an unread store (`Drain::Unknown`, not `Allowed`), a
stale status (`headroom` → `None`), and a node absent from the report
(`node()` → `None`, not an invented `Allowed`). A node with **no pods** is
seeded from the node store as genuinely `Allowed`, because that is a fact rather
than an absence — and seeding from pods alone would have made an idle node
silent, which a surface cannot tell from "not examined".

**3. Two sections constraining one behaviour?** `chaos::node_protected` (control
plane) and PDB (availability) still answer *"can this node be disturbed"* from
different premises, and still do not overlap: `node_protected` gates which nodes
a drill may target, a budget refuses per pod at execution. Checked deliberately
per the guidance; the chaos console has one refusal path per stage, not two that
could disagree.

**4. Consumers depending on the old meaning?** Enumerated, not assumed. The verb
change had two known consumers and a **third I had missed**: the Charter grid
(§5), which is a consumer of the *meaning* of `evict` rather than of the
function. That is the D2-fix finding again — the consumer that bites is the one
not named.

**5. Inherited claims?** Ten, all re-read; two true-but-insufficient (§1), and
the sufficiency is what mattered both times. My own claim from yesterday — that
item 1 was complete — was **wrong**, and re-reading it is what found the RBAC
defect. Eighth consecutive round in which re-examining one of my own statements
changed the work.

**6. One side of a comparison moved?** Yes, and this is §5: item 1 moved the
write from DELETE to eviction while the permission probe stayed on the old side.
The comparison was "may I do X" against "I am about to do Y".

**7. Container adjacency read as world adjacency?** The memo (§6, M11) is the
inverse case — indices into a per-namespace container, keyed as if they were
namespace-independent. Caught by mutation, closed by a fixture that can express
it.

---

## 9. §8 — acceptance

- [x] Eviction uses `pods/eviction`; a PDB refusal is a named refusal — item 1
- [x] The chaos drill's partial-failure behaviour decided and recorded — item 1
- [x] PDB watched on the NetworkPolicy pattern; the RBAC verb declared in the Charter
- [x] `selector_matches` reused, not reimplemented — with the null case handled by the caller (§1)
- [x] Derivation cost measured against the rebuild budget (§4)
- [x] Unprotected and unknown distinguishable everywhere (§2, §7)
- [x] Gate run on kind, with both discrimination checks (§7)
- [x] Failure criteria stated before the run
- [x] Mutations asserted applied; the two survivals reported with their reasons (§6)
- [x] Standing questions answered, claims tagged
- [x] `cargo test --workspace` green (and `cargo nextest run --workspace`,
      2026-08-29 — 621 tests, plus the `--all-features` and
      `--no-default-features` core runs)

449 core (474 with the `oracle` feature) + 139 GUI tests; gui-smoke 57; clippy
clean with and without features.

**Next:** item 3 — the attention concern (`pool_confinement`'s shape) and the
province window / SELECTION, plus the drill-verdict gap item 1 recorded.
