# KuberNation — A1: The Layout Engine

**Implementation guidance**
**Goal:** a pure function that assigns each node a durable map slot, so that replacing a node does not move the world.
**Shape:** new core module. No rendering, no persistence, no `build_world` changes.
**Consumer-less by design** — see §7 for what that means for review.

Governing docs: `kubernation-enabling-plan.md` §3, `kubernation-workstream-a-decomposition.md` §4 (phase A1).

---

## 0. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `build_world` computes position inline: `cx = zi * (PATCH_W + OCEAN_GAP)`, `y += h`, `h = (2 + 2*cities.len()).max(3)` | `state/world.rs` ~430–470 |
| 2 | `MapModel` is `{ zones: Vec<ZoneColumn>, … }`, `ZoneColumn` is `{ name, nodes: Vec<NodeTile> }` | `state/model.rs` ~393–406 |
| 3 | `NodeTile` carries `name` and `zone` as `String`, but **no pool and no labels** | `state/model.rs` ~322 |
| 4 | `node_zone` resolves `ZONE_LABEL` → `ZONE_LABEL_LEGACY` → `UNZONED` sentinel | `state/model.rs` ~408 |
| 5 | `fnv1a64` exists and is used for stable tie-breaks | `state/world.rs` (used in `city_home`) |
| 6 | `Province` is `{ tile, x, y, w, h, cities, infra }` — position is on the province, not the tile | `state/world.rs` ~51 |

**Claim 3 is the one that shapes the phase.** If `NodeTile` carries no pool label, A1 either takes raw `Node` objects or `NodeTile` gains a pool field. Decide which in the first ten minutes — see §2.1.

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 7 | No standard nodepool label exists; each provider uses its own key | The cascade in §2.2 is a heuristic, and must be honest about it |
| 8 | Immutable-infrastructure refreshes **surge** — the replacement is Ready before the predecessor drains | Verified in A-pre (100 → 115 → 100). This is why reuse-by-name cannot be the whole algorithm |
| 9 | A node can lack `instance-type` (bare metal, kind, kwok default) | Verified in A-pre. Affects A2's extent, not A1 — but the fixture exercises it |
| 10 | `node_zone` returns a sentinel rather than `Option`, so "unzoned" is a real zone | A1 must not treat the sentinel as a missing key |

Claim 8 is the phase's hardest requirement. **If the harness scenario does not actually surge, A1 will pass a test that proves nothing.**

---

## 1. What A1 is, precisely

```rust
/// PURE. Given the previous layout and the nodes observed now, produce the
/// layout for this frame. Slots persist across their occupants: a node
/// replaced by a differently-named successor inherits the same coordinates.
pub fn assign_layout(prior: &Layout, observed: &[ObservedNode]) -> Layout;
```

That is the whole deliverable. Not `build_world`, not persistence, not rendering.

`prior` is simply the previous frame's output in-session; A4 will supply it from disk. **A1 must not know which.**

**Why pure matters here:** A's correctness claim is *given this sequence of cluster states, the layout does not move except where declared*. That is a property of a function over a sequence — testable with synthetic fixtures, no cluster, in CI. The churn fleet is for A2's perception gate, not A1's correctness. Keeping `assign_layout` pure is what makes that split real.

Precedents for the shape: `SubstrateReport::from_world`, `province_ring`, `cost_report` — all pure, all testable without a GL context.

---

## 2. The slot

```rust
pub struct SlotKey {
    pub zone: String,   // node_zone(), sentinel included
    pub pool: String,   // §2.2
    pub ordinal: u16,   // position within (zone, pool)
}
```

**The terrain belongs to the slot. Nodes occupy it and are replaced.**

### 2.1 Input shape — decide first

`NodeTile` has `name` and `zone` but no pool. Two options:

- **A1 takes `&[ObservedNode]`**, its own minimal input struct built from `Node` objects. Keeps `NodeTile` untouched; needs a small adapter at the call site.
- **`NodeTile` gains `pool: String`** (and `pool_source`), filled in `build_node_tile` beside `zone`.

Prefer the **second**. It puts pool resolution next to `node_zone`, which is exactly the analogous concern, and it means A2 does not need a parallel plumbing path. `MapModel` already carries everything else the world builder needs; pool belongs with it.

Either way, A1's own input must be a plain data struct, not a `k8s_openapi::Node` — that is what keeps fixtures cheap.

### 2.2 Pool resolution — cascade, and record which rule fired

Model this on `node_zone`, which already does label-with-legacy-fallback-to-sentinel.

Precedence:

1. Explicit `--pool-label <key>` override, if set
2. Known provider keys, in a fixed list:
   - `cloud.google.com/gke-nodepool`
   - `eks.amazonaws.com/nodegroup`
   - `kubernetes.azure.com/agentpool` (and legacy `agentpool`)
   - `karpenter.sh/nodepool` (and legacy `karpenter.sh/provisioner-name`)
   - `cluster.x-k8s.io/deployment-name`
   - `machine.openshift.io/cluster-api-machineset`
3. `node.kubernetes.io/instance-type` — coarser, merges same-type pools, but portable
4. A single default pool

**Record the rule that fired**, mirroring `metric_source`:

```rust
pub enum PoolSource { Override, Provider(&'static str), InstanceType, Default }
```

This is the honesty discipline the codebase already applies — `metric_source` says which ratio you are looking at, `CostBasis` says which basis, `PoolSource` says how the pool was inferred. It is also what lets §3.3 of the plan declare the reference frame.

**Do not infer pools by clustering** on shared attributes. An inferred pool that re-splits when a node's attributes shift is precisely the instability A exists to remove.

---

## 3. Assignment

Order matters, and each step exists for a reason.

```
1. CARRY   — a prior slot whose occupant is still observed keeps it, unchanged.
2. REUSE   — an observed node with no slot claims a VACANT slot in its own
             (zone, pool). Lowest ordinal first.
3. APPEND  — otherwise, a new slot at the next ordinal in (zone, pool).
4. GHOST   — a prior slot whose occupant is gone is retained as vacant,
             not removed. Ordinals never shift to close a gap.
```

### 3.1 Sparseness is the decision, and it is deliberate

Under surge the replacement arrives **before** the predecessor departs, so at step 2 there is no vacancy and it appends. When the old node drains, its slot becomes a ghost. The pool is now sparse.

**That is correct and must not be silently compacted.** Compaction moves existing slots, which is exactly what A prevents. Reclamation happens only as a *declared* event — A4's compaction, recorded like a cataclysm.

Expected shape after a full 100-node refresh: 200 slots, 100 occupied, 100 ghosts. **Zero occupied slots moved.** That is A1's headline test.

### 3.2 Determinism

Two nodes may claim the same vacancy in the same frame. Break ties on a stable hash (`fnv1a64` on node name), matching `city_home`'s existing tie-break — never on iteration order, which `HashMap` does not guarantee.

`assign_layout` must be **idempotent**: applying it twice to the same observation yields the same layout. Worth an explicit test; it is the cheapest guard against accidental order-dependence.

### 3.3 What A1 does *not* do

- No compaction (A4)
- No fresh-ground or cataclysm marking (A5) — though the layout must carry enough for A5 to detect them: at minimum, which slots changed occupant this frame
- No extent, no capacity (A2)
- No persistence (A4)
- No `build_world` integration (A2)

---

## 4. Tests

The interesting half. All pure, all synthetic — no cluster.

**The headline:**
- [ ] A full-fleet rolling refresh with surge: **zero occupied slots move**. This is the phase's reason to exist.

**Assignment:**
- [ ] Scale up appends; nothing existing moves
- [ ] Scale down leaves a vacant slot; no ordinals shift
- [ ] A node returning after departure claims its own slot back if still vacant
- [ ] Surge produces sparseness, not compaction
- [ ] A node moving between zones gets a new slot in the new zone and vacates the old — it does **not** carry coordinates across a continent

**Determinism:**
- [ ] Idempotent: `assign(assign(prior, obs), obs) == assign(prior, obs)`
- [ ] Two nodes contending for one vacancy resolve identically across runs
- [ ] Result is independent of input ordering — shuffle `observed`, same layout

**Pool cascade:**
- [ ] Each provider key resolves, and `PoolSource` names it
- [ ] Override beats provider keys
- [ ] Instance-type fallback fires when no provider key is present
- [ ] A node with no labels at all lands in the default pool with `PoolSource::Default`
- [ ] A pool spanning three zones yields three distinct `(zone, pool)` groups — **the hierarchy claim**

**Boundaries:**
- [ ] Empty cluster; single node; every node replaced at once
- [ ] The `UNZONED` sentinel behaves as an ordinary zone

**Mutation floor** (per v1.6.0): revert the carry step to always-append and confirm the headline test fails. If it does not, the test is not testing what it claims.

---

## 5. Acceptance

- [ ] `assign_layout` is pure — no I/O, no clock, no globals
- [ ] Input is a plain data struct, not a `k8s_openapi` type
- [ ] `PoolSource` recorded per node and reachable from the layout
- [ ] Sparseness retained; no compaction anywhere in this phase
- [ ] Idempotent and order-independent, both tested
- [ ] Zero rendering changes; GUI crate diff empty
- [ ] `cargo nextest` green

---

## 6. Acceptance criteria this phase cannot yet verify

Per the A-pre finding — a prerequisite's acceptance list must separate what is checkable now from what needs its consumer.

**Verifiable now:** everything in §5.

**Not verifiable until A2:** that the assigned slots produce a map which visibly holds still. A1 can prove coordinates are stable; only A2 can show that stability *reads*. Do not claim it here.

**Not verifiable until A4:** that layouts survive restart. `prior` is in-memory this phase.

---

## 7. Review bar for a consumer-less phase

A0 established that the ordinary bar — *does this produce a wrong result today?* — cannot be met by a phase with no consumers, and that applying it anyway yields a false all-clear. v1.6.0 confirmed the fix: verifiers told that a path which could reintroduce the defect counts, even with no consumer exercising it, found 14 real issues where the old bar found none.

Use the revised bar: **would this be wrong when the consumer arrives?** Plus mutation survival as the objective floor.

### The specific thing to look for

From the v1.6.0 round: *inputs were fixed to express unknown, and the output that summarised them kept fabricating.*

A1's analogue: **the layout is a reduction over nodes.** Ask at every reduction — does an empty or unknown input produce a *fabricated* answer here?

- An empty pool: does ordinal assignment start sensibly, or does some `unwrap_or(0)` collide with a real slot?
- A node whose pool cannot be resolved: does it land in `Default` *honestly*, or silently join a real pool?
- A `max()` over existing ordinals on an empty set: `unwrap_or(0)` versus `None` — the same shape as `worst_level`.

Also ask the standing question: **where does a summing step precede a comparing step?** A1 groups nodes into pools and then compares ordinals — the same shape as all three prior confirmed defects.

---

## 8. Estimate

**One day**, most of it tests. The algorithm is short; the fixture matrix is not.

The v1.6.0 round taught that the consumer sweep is where estimates break. A1 has no consumers by construction, so that risk is absent here — it arrives in full at A2.
