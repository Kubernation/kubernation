# KuberNation — A2: Wire the Layout In

**Implementation guidance**
**Goal:** `build_world` consumes the layout instead of computing positions, and province extent derives from capacity rather than pod count.
**Gate:** watch a rolling refresh on the churn fleet. **Does the map hold still?**

This is the phase that can kill Workstream A. See §8 before starting.

Governing docs: `kubernation-workstream-a-decomposition.md` §4 (A2), `kubernation-enabling-plan.md` §3.

---

## 0. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `Models::build_filtered(world, filter) -> Self` is **stateless** — takes only `&ObservedWorld`, returns a fresh `Models` | `state/model.rs` ~1955 |
| 2 | `build_world(map, workloads, severity, customs, exposure, storage, batch)` — no layout parameter, no prior state | `state/world.rs` ~364 |
| 3 | Position is computed inline: `cx = zi * (PATCH_W + OCEAN_GAP)`, `y += h`, `h = (2 + 2*cities.len()).max(3)` | `state/world.rs` ~430–470 |
| 4 | `Layout` holds `BTreeMap<SlotKey, SlotState>`; `slots()` yields ghosts too; `changes_from` computes transitions | `state/layout.rs` ~105 |
| 5 | `NodeTile` carries `cpu_request_ratio` etc. as `Option<f64>` but **no absolute allocatable** | `state/model.rs` ~322 |
| 6 | `node_allocatable(node, key) -> Option<f64>` is `pub`, and callers must not fabricate | `state/model.rs` ~509 |
| 7 | `Province` is `{ tile, x, y, w, h, cities, infra }` — `w` is currently always `PATCH_W` | `state/world.rs` ~51 |

**Claim 1 is the phase-shaping one.** A1's `assign_layout(prior, observed)` needs a `prior`, and there is nowhere in the current pipeline for one to live. See §1.

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 8 | A rolling refresh on the churn fleet surges (100 → 115 → 100) | A-pre verified. The gate is meaningless without it |
| 9 | kwok backfills fake capacity unless the fixture supplies a Ready condition | A-pre §3. A node that *looks* allocatable-less may have 1Ti of phantom memory |
| 10 | Ghost slots exist immediately after any refresh — 100 occupied, 100 ghosts | A1's headline. A2 must render that state sanely from day one |

---

## 1. The state problem — solve this first

`Models::build_filtered` is a pure function of `&ObservedWorld`. `assign_layout` needs the previous frame's `Layout`. **These are incompatible as written**, and no amount of care inside `build_world` fixes it.

Three options:

**(a) Thread `prior` through the call chain.** `build_filtered(world, filter, prior: &Layout) -> Models`, and `Models` carries the new `Layout` as a field. The caller holds it between ticks.

- Preserves purity end to end — the property A1 was built for
- `Layout` becomes part of `Models`, which is already the aggregate everything renders from
- Cost: the signature change reaches every caller, including the ~33 `Models::build` sites A1's report flagged

**(b) Put the layout on `ObservedWorld`.** Fewer signature changes, but it makes the observed layer hold derived state, which inverts the model/view discipline the codebase is built on. **Reject.**

**(c) A module-level cache.** Rejected outright — the codebase documents global mutable state as architecturally wrong for anything participating in projection, and this is exactly that.

**Take (a).** It is more work and it is the only one that keeps the layout honest. Add `Models::build(world)` as a wrapper passing `&Layout::default()` so single-shot callers (tests, one-off renders) stay ergonomic — a fresh layout is correct for them, since with no prior there is nothing to hold still.

> This is the consumer sweep that broke the A0 and v1.6.0 estimates. **Budget it explicitly rather than discovering it.**

---

## 2. Position from slots

Replace the inline arithmetic. The mapping:

```
continent  ← zone            (already how continents are keyed)
region     ← pool ∩ zone     (new; may be visual grouping only in A2)
province   ← slot ordinal    (replaces `y += h`)
```

**Ordinal → y must not compact.** A pool with ordinals 0, 3, 7 occupies three *separated* positions; the gaps are ghosts and they are the point. A `y` computed by enumerating live provinces reintroduces exactly the reshuffle this phase removes.

**Zone ordering must be stable.** `cx = zi * (PATCH_W + OCEAN_GAP)` uses the index in `map.zones`. If that vector's order can change between builds, continents move. Verify how `build_map` orders zones; if it is not sorted by name, sort it — a cheap fix for a real instability source (§3.1a source 4).

---

## 3. Extent from capacity

`h = (2 + 2*cities.len()).max(3)` is instability source 1: workload churn resizes terrain.

Replace with the fallback chain, and **declare which rung fired**:

1. Node allocatable (memory preferred — incompressible, per plan §3.4.1)
2. `node.kubernetes.io/instance-type`, mapped to a size class
3. A declared default extent

`NodeTile` carries no absolutes, so this needs `node_allocatable` plumbed forward — a small carry-forward flagged in the decomposition §3 and in A1's report.

**Rung 3 is not a silent zero.** v1.6.0 established that unmeasurable must be visually distinct from empty; a node with no capacity gets the default extent *and* is marked as unmeasured. It must not read as a genuinely tiny node.

Quantise extent into a few size classes rather than mapping capacity linearly. Continuous sizing means a node type refresh nudges every province; classes are stable across small variation and easier to read.

**Cities keep their current row placement in A2.** Interior stability is A3. Do not fix it here — mixing them makes the gate ambiguous about which change did what.

---

## 4. Ghosts have to render

A1 produces ghosts on the very first refresh, so A2 cannot defer them entirely.

**Minimum for A2:** a ghost slot occupies its position and does not collapse. Anything else — ruins, reclaimed land, ageing — is A5.

The simplest honest treatment is empty terrain: the land is there, nothing is on it. Resist making it interesting; A5 owns that vocabulary, and a placeholder that looks deliberate is harder to replace than one that looks blank.

---

## 5. Tests

**Purity and stability (synthetic, no cluster):**
- [ ] Same observation twice → identical `WorldModel`, byte for byte
- [ ] A full surging refresh through `build_world`: every surviving province keeps its `(x, y)`
- [ ] Adding a workload to a node changes no province's `x`, `y`, or `w`/`h` — **the extent claim**
- [ ] A ghost slot leaves a gap; the province below it does not move up
- [ ] Zone ordering is stable across builds regardless of observation order

**Extent:**
- [ ] Same-capacity nodes get equal extent; a larger node gets more
- [ ] Instance-type fallback fires when allocatable is absent
- [ ] A node with neither gets the default extent **and** is marked unmeasured
- [ ] Small capacity variation within a size class does not change extent

**Regression:**
- [ ] Existing `map_layout_is_stable_under_insertion` still passes, or is replaced by something strictly stronger
- [ ] Hit-testing still resolves the right province — `cell_at`, `resolve_region`
- [ ] Minimap, coast markers, islands all still place correctly

**Mutation floor:** revert the carry so every node appends fresh, and confirm the refresh test fails.

---

## 6. The gate

Run the churn fleet's rolling refresh. Capture before, during, and after with all four flags pinned (`--center`, `--zoom`, `--overlay`, `--map-style` — the last two persist in prefs, per A-pre §4).

**Compare against the A-pre baseline captures of current `main`.** If those were not taken, take them from the v1.6.0 tag first — that comparison is the entire argument for this workstream and it cannot be reconstructed later.

The question is not *did the code do what it says*. It is: **watching this flipbook, does the map hold still?**

Answer in the report before analysing anything else.

---

## 7. Acceptance

- [ ] `build_world` computes no positions of its own
- [ ] `Layout` threaded through `Models`; no global state, no layout on `ObservedWorld`
- [ ] Ordinal gaps preserved — ghosts do not compact
- [ ] Extent is capacity-derived with a declared, marked fallback
- [ ] Zone order stable
- [ ] Ghosts render without collapsing
- [ ] The gate answered explicitly, with captures
- [ ] `cargo nextest` green

---

## 8. This phase can kill Workstream A

Plan §1 claims the map's advantage over K9s and Freelens is spatial memory. Spatial memory requires stability. **A2 is the first moment that claim is testable.**

So if the map holds still and is *still* not more useful, the failure is not A2's — it is §1's. The instinct at that gate will be to blame the implementation. Name the possibility now, before the result is known, so it can be reported honestly.

Salvage is thin. A1's layout engine has no value if A2 fails; it is machinery for a map nobody wants. What survives regardless: the churn harness, and the gate answer itself, which is information about the product thesis no amount of planning can produce.

---

## 9. Method notes

Standing questions, all three earned in this series:

1. **Where does a summing step precede a comparing step?** (Substrate, ghost nodes, QoS)
2. **Does every reducer over a now-optional input express unknown, or does it fabricate?** (v1.6.0's `worst_level`)
3. **Where do two sections constrain the same behaviour — and is there a fixture where they diverge?** (A1's lowest-ordinal versus own-slot contradiction)

Question 3 applies directly here: §2 says ordinals map to position without compacting, §4 says ghosts render. A fixture with **two adjacent ghosts** is where a naive implementation satisfies one and violates the other.

Review bar: this phase *has* consumers, so the ordinary "wrong today" bar applies — but keep the mutation floor, which has caught real defects in three consecutive rounds.

---

## 10. Estimate

**Two to three days.** The layout substitution is a day; the `Models` threading sweep (§1) is the rest, and it is the part that has broken every previous estimate in this series.
