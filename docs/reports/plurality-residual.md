# §7 answered — what still asserts a workload's location

**Follows:** `docs/reports/plurality-false-claims.md` §7
**Date:** 2026-08-19 · **No product change.** An enumeration and a verdict.

**Answer: YES — two residuals remain**, both the same shape as the two that were
fixed (node facts folded into a workload's context with nothing saying how many
nodes there are), and both narrower. One of them **publishes off-laptop**.

Done by the method that found the original two: enumerate every surface, **read
the code**, and record what could not be exercised.

---

## 1. The enumeration

| Surface | Asserts a location? | Verdict |
|---|---|---|
| SELECTION / tooltip | yes | **fixed** in v1.26.0 |
| Almanac Legend + World | yes | **fixed** in v1.26.0 (§4) |
| **Oracle bundle — `WidenNode` lens** | **yes** | **RESIDUAL — §2** |
| **`--dump-positions` city record** | **yes** | **RESIDUAL — §3** |
| Attention concerns | one, `pool_confinement` | honest — §5.1 |
| Annals / timeline | no | node text is `TimelineScope::Node`'s own filter |
| Postmortem | no | `nodes` is a cluster census; the Scope caveat already says so |
| Workload table | no | kind/ns/name/ready/status/age + the `road` note; no node |
| Advisors (all six tabs) | no | `median_node_alloc_cpu` is a *unit* (node-equivalents), not a place |
| Chaos console | no | its node text is node-subject experiments and `node_protected` guards |
| Blast / IMPACT | no | resolves each workload to its own city (pre-check §3.3) |
| City window | no | renders no node at all |
| Node window / GARRISON | no | lists pods actually on that node |
| Inspector / resource browser | no | raw object YAML |
| Minimap | no | no text |

---

## 2. Residual 1 — the Oracle's "widen to node", and it leaves the laptop

`available_lenses` offers `DeepenLens::WidenNode` for a **Workload** scope
whenever a representative pod has a node. Clicking the chip folds
`node_sections(world, node)` — that node's health, conditions, strain, garrison —
into a bundle whose subject is the workload.

**The node is the alphabetically-first pod's.** `representative_pod(Workload)` is
`build_city(..).pods.into_iter().next()`, and `build_city` sorts its pods by name
(`model.rs:1752`). So it is not even the plurality node the city sits on — it is
an arbitrary one.

For `churn/api` that is one node of 65, running about 2 of 120 pods, and
**nothing in the bundle says so.** A model reading

```
workload churn/api …
node churn-…-013  (health, conditions, strain, garrison …)
```

would reasonably attribute that node's state to the workload — which is the same
inference the SELECTION box used to invite.

**Why it is the strongest residual:** on an armed remote endpoint the bundle is
**published off-laptop**. Every other surface here is read by one operator.

**Why it is narrower than the two that were fixed:** it is opt-in (a chip click),
the chip says *"widen to node"* singular and claims no totality, and the section
is titled `node {name}` rather than presented as the workload's. What is missing
is the **quantity** — one of how many — which `City.spread` now computes and the
bundle does not carry.

---

## 3. Residual 2 — the dev instrument, and it already misled this project

`--dump-positions` emits, per city:

```json
{"kind":"city","workload":"Deployment churn/api","node":"churn-edge-g1-013", …}
```

The field is called `node` and holds the **plurality province's** node, with no
qualifier. It is not user-facing — but this project's own measurements consume
it, and **I read that field as "where `api` runs" during the D3/D4 pre-check**,
which is how the plurality item was found at all. The instrument that surfaced
the problem carries it.

Renaming the field would break `hack/churn/positions.py` and its self-tests,
which match the literal `"node"`, so it is recorded rather than changed.

---

## 4. Extra B — is there a third almanac page describing siting?

**No.** Every sentence in the field guide mentioning siting or where something
runs:

| line | |
|---|---|
| `SITING_CLAIM` + the Legend's City entry | **corrected** — plurality, and "where the workload is DRAWN, not where it runs" |
| the World page | **corrected** — built from the same const |
| the Road entry: *"A DaemonSet — paved across every province its pods run on, never a city"* | **true, and complete** |

The remaining hits (pool identity, ground ageing) describe provinces, not
workload siting.

The Road entry is worth naming: it is the one mark that makes a **complete and
true** location claim, because a DaemonSet is drawn on *every* province its pods
run on. That is precisely the claim a city cannot make, and the contrast is the
clearest statement of why the two are rendered differently.

---

## 5. Extra A — does the concentrated case now under-claim?

**Yes, mildly — and deliberately.**

For a workload genuinely on one node the box reads:

```
3 pods on 1 node
on province kubernation-worker
```

The truth is stronger: *all* its pods run on `kubernation-worker`. "On province
X" is a statement about where the city is drawn, so it under-states a case where
the drawing and the running coincide.

**This is the accepted cost of §3.2's decision**, which explicitly refused
wording conditional on spread ("conditioning introduces a threshold that would
need defending"). The information needed to draw the stronger conclusion is on
the line directly above — `3 pods on 1 node` — so a reader can close the gap; the
panel simply does not close it for them.

Recorded as a deliberate tradeoff, not a defect. If it is ever revisited, the
change is to the *qualifier*, not to the footprint line.

### 5.1 The one concern that names a place, and why it is honest

`attention::pool_confinement` says *"all 29 placed on pool sys"*. It is a claim
about a **pool**, not a node, and it refuses four ways: fewer than two placed
pods, more than one pool, the `unpooled` sentinel, and a single-pool fleet. It
also says "placed", excluding unschedulable pods that have no node and so no
pool. The claim it makes is one the data supports completely.

---

## 6. Unmeasured

Recorded rather than reported as absent:

- **The Oracle bundle was read, not exercised.** Rendering a real consult needs
  an LLM endpoint. §2's finding is from `oracle.rs`'s composition, which is
  definitive about *what the bundle contains*, and says nothing about how a
  particular model reads it.
- **The `why` text on CONSULT NEXT links** is model output, display-only,
  `ascii()`-truncated and never folded back into a bundle. Not a KuberNation
  claim, so out of scope by construction.
- **A real cluster with resource pressure** — inherited from the pre-check, still
  true: both fixtures are synthetic in opposite directions.

---

## 7. Verdict

The pre-check's last row — *the city is a label, not a location claim* — is
**closer to true, but not yet true**. The map itself is honest; both
operator-facing false claims are fixed; and the two that remain are (a) an
opt-in Oracle lens that omits a quantity it could now state, and (b) a dev
instrument's field name.

**Neither is a map problem.** So the four map-shaped candidates the pre-check
recorded remain unchosen and, on this evidence, unneeded: nothing found here
would be fixed by changing where a city is drawn.

If a phase is scoped from this, it is small and specific: give the Oracle's node
section the workload's footprint, and rename one JSON field.
