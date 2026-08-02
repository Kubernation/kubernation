# A1 — The layout engine

**Implementation report** · 2026-08-02 · unversioned (consumer-less by design)
**Commits:** `b9391c4` (engine) · `c61bdb0` (review fixes)
**Governing docs:** [`kubernation-a1-layout-engine-guidance.md`](../kubernation-a1-layout-engine-guidance.md) · [`kubernation-workstream-a-decomposition.md`](../kubernation-workstream-a-decomposition.md) §4

A pure `assign_layout(prior, observed) -> Layout` giving each node a durable map
**slot**, so replacing a node does not move the world. No rendering, no
persistence, no `build_world` changes — those are A2 and A4.

| | |
|---|---|
| New module | `state/layout.rs` |
| Tests | 384 core · 87 GUI |
| Mutations verified | 14 (8 in the first pass, 6 more the review exposed) |
| GUI crate diff | **empty** |
| Perf | no measurable change |

---

## 1. Verification: all ten §0 claims TRUE

Fourth round running that §0 survived intact. Claim 3 — `NodeTile` carries no
pool — is the one that shaped the phase; it held, so pool resolution went beside
`node_zone`, per §2.1's preferred option.

The referenced governing doc `kubernation-workstream-a-decomposition.md` **did
not exist** when the round started. It was added mid-round, and A1 was then
checked against it: **deliverable, slot identity, cascade, signature and gate all
match §4.** Nothing had to change.

---

## 2. Where the guidance underspecified

**§2.2 makes `--pool-label` precedence 1 without costing it.** Wiring an override
reaches `Models::build`'s ~33 call sites. `node_pool` takes it as a parameter, so
the cascade is complete and tested including `PoolSource::Override`, and the sole
caller passes `None` until a phase needs the flag.

**§7 named the trap, and it was real.** *"The layout is a reduction over nodes —
ask whether an empty input produces a fabricated answer."* That is `next_ordinal`
exactly: `max().unwrap_or(0) + 1` starts an empty pool at 1, and a bare
`max().unwrap_or(0)` returns **0 — an ordinal a real slot may already hold**.

---

## 3. The review found the root, and three lenses agreed

**18 raised, 10 confirmed, ~4 distinct defects.**

### Slots did not remember who held them

`insert(k, None)` discarded the departed occupant's identity, so REUSE could only
choose *positionally*. The decomposition's §4 acceptance item — *"a node returning
after departure claims its own slot back if still vacant"* — was therefore
**unimplementable from stored state.**

What that produced, reproduced by the reviewers:

- Two nodes draining and returning together **swap coordinates**.
- A node returning after a one-frame blip lands on a **stranger's ground** and
  leaves a permanent ghost at its own.

And the test named for the property could not detect any of it: its fixture
departs exactly one node, so *"the lowest vacancy"* and *"its own slot"* are the
same slot. **Inverting REUSE to take the highest vacancy left all 15 tests
green** — the "lowest ordinal first" rule, stated in three places, was pinned by
nothing.

> The underlying cause is that **the guidance is internally inconsistent**: §3
> mandates *lowest ordinal first*, §4 mandates *own slot back*. Those differ the
> moment there is more than one vacancy. I implemented §3 faithfully and wrote a
> test named for §4 — which is how a contradiction becomes a passing suite.

Slots now carry `last_occupant`; RECLAIM precedes lowest-ordinal REUSE, so both
rules hold.

### The ordinal ceiling evicted a live node

`saturating_add` handed back `u16::MAX` a second time. The newcomer's key then
*equalled* the incumbent's, and `insert` overwrote it — **two nodes in, one slot
out, and the incumbent gone from the layout entirely.** Reproduced directly.

Now `checked_add`, and a node with no honest coordinate is left **unplaced**
rather than given one a live node holds. Notably the review refuted this three
times and confirmed it once; the reproduction settled it.

### `DEFAULT_POOL` collided with real pools

It was the literal `"default"` — and providers ship pools by that name. An
unresolvable node would silently join a real pool and share its slot space, which
is the exact failure the cascade exists to prevent. It is now the `unpooled`
sentinel, which no provider emits.

### Unpinned contracts

`changes_from`'s departure direction, the lowest-ordinal fallback, and
`POOL_LABELS` precedence — reversing the list left every test green, and that
order decides the slot key.

**Six previously-surviving mutations now fail.**

---

## 4. A decision the decomposition doesn't settle

§6's settled pool-detection row reads *"override → standard labels → single
default"*, with **no instance-type**. The A1 guidance adds it as step 3. §6
separately settles instance type as the **extent** fallback.

The consequence, now pinned by test:

> **A node whose instance type changes vacates its slot and leaves a ghost.**

Because the pool is then a hardware *attribute* rather than a declared identity —
the milder form of the "inferred pools re-split when attributes shift" hazard
that `node_pool`'s own documentation warns against. A node carrying a provider
key is immune.

Kept, because the step only fires on clusters with no provider label at all
(bare metal, kind, kwok), where the alternative is one undifferentiated pool per
zone — real structure lost for every such cluster to protect a rarer event. But
**flagged rather than treated as agreed**, since §6 says settled decisions should
not be re-litigated mid-session and this one is not listed.

---

## 5. Carry-forward for A2

The decomposition's §3 is still outstanding and lands squarely on A2:
**`NodeTile` carries only ratios, never absolute allocatable** — and since
v1.6.0 those ratios are `Option`. Capacity-derived extent needs the absolutes
plumbed through, with a *declared* fallback rather than a silent zero.

---

## 6. Acceptance

| §5 criterion | Status |
|---|---|
| `assign_layout` pure — no I/O, clock or globals | ✅ |
| Input is a plain data struct, not a `k8s_openapi` type | ✅ |
| `PoolSource` recorded per node, reachable from the layout | ✅ |
| Sparseness retained; no compaction | ✅ |
| Idempotent and order-independent, both tested | ✅ |
| Zero rendering changes; GUI crate diff empty | ✅ |
| Tests green | ✅ 384 + 87 |

**§4's headline:** a 100-node surging refresh → 200 slots, 100 occupied, 100
ghosts, **zero occupied slots moved**. **§4's mutation floor:** reverting CARRY
to always-append fails it.

---

## 7. Decisions for the room

### Instance-type as a pool fallback — keep or drop?

Dropping it is a one-line change, and the test pinning the consequence is what
will fail. Keeping it trades a rare instability for real structure on
label-less clusters.

**Ask:** confirm the trade, or align to §6's literal row?

### The revised review bar keeps paying

A0 established that *"does this produce a wrong result today?"* cannot be met by
a consumer-less phase. v1.6.0 confirmed the fix. This round the doc specified the
revised bar itself (§7) — and it produced 10 confirmed findings including the
root, on a phase where the old bar would have found nothing at all.

**Ask:** promote it from per-doc guidance to a standing rule?

### Review agents write into the working tree

Twice now — an oracle probe last round, two probe files this round, one using
`gen` (reserved in edition 2024), which broke the build mid-session. Their
findings were good; the mechanism is hazardous.

**Ask:** constrain reviewers to a scratch directory or a worktree?
