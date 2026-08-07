# KuberNation — T1 §3.1 Re-derivation (revision 2)

**Measurement guidance**
**Goal:** establish whether T1's "the map showed it as a shape" was derived from the model's own order or from map order — and, if the former, what the correct answer is.
**No product change.** The output is a number and a corrected record.

> **Supersedes revision 1.** Its claim 5 implied `pool_label_pieces` was reusable; it is `pub(crate)` in the GUI crate and cannot be called from a script. Its §3 also conflated two different break rules. Both corrected below, against source.

---

## 0. Why this is worth a session

T1's gate produced one piece of positive evidence for the map: 18 successions concentrated in the `sys` pool, rendered as *"one contiguous run per column, and one column clean."* A diagram of per-column runs was published.

**Two rounds later, `Continent.provinces` was found not to be in map order** (v1.17.0 §1) — it is sorted by `fnv1a64(name)` while a province's row comes from its slot ordinal, and the two are unrelated. The same round found that a measurement derived by walking that vector produced a "1 of 8" figure that was really "4 of 8", and that the wrong figure had **chosen a design**.

T1's report does not state which method produced its diagram.

The record also contains a direct tension, now confirmed at source rather than inherited. `pool_label_pieces`' own doc comment states:

> measured on the churn fleet, **4 of 8** are [in several pieces], and a largest piece holds as little as **40%** of its region

If `sys` is that fragmented, T1's clean per-column runs are surprising. Either the 18 successions genuinely fell inside single pieces — plausible if a refresh wave targets consecutive ordinals — or T1's diagram was a hash-order artifact.

**Both outcomes are useful. Only one of them is currently assumed.**

---

## 1. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | `Continent.provinces` is ordered by `fnv1a64(name)`; `Continent.ghosts` comes from a `BTreeMap` keyed `(zone, pool, ordinal)` — **neither is map order** | `[V]` `draw.rs:1568`, `terrain_order` doc |
| 2 | `slot_row(ordinal) = 1 + ordinal * SLOT_STRIDE`; `slot_of_row(y) = (y-1) / SLOT_STRIDE` is its **`pub`** inverse in core | `[V]` `world.rs:551,561` |
| 3 | `slot_of_row` is exposed specifically so callers do not re-derive it — *"a label that disagreed with the reference on the same province would send someone to the wrong node"* | `[V]` `world.rs:545–550` |
| 4 | `pool_label_pieces` is **`pub(crate)` in the `kubernation` GUI crate** — readable as a specification, **not callable from a script** | `[V]` `draw.rs:1617` |
| 5 | Its rule: contiguous means **consecutive slots**; a region is broken by *another pool's province* **and** by *a departed node's ghost ground* | `[V]` `draw.rs:1604–1606` |
| 6 | It skips `DEFAULT_POOL` — *"an absence is not a region and must not be given a name"* | `[V]` `draw.rs:1618,1636` |
| 7 | `--dump-positions` emits per-province records including node, zone, pool, ordinal and extent, per model rebuild | `[A]` A3-pre, A6 |
| 8 | T1's `changed_since` marks a province whose `occupied_at` is after the baseline | `[A]` T1 |

---

## 2. The first question: how was the diagram produced?

Read T1's implementation and its report's method **before measuring anything**.

| If the diagram came from | Then |
|---|---|
| `--dump-positions`, by slot ordinal | The evidence is sound. §3 becomes a confirmation rather than a correction |
| a walk over `cont.provinces` | It is the v1.17.0 §4 error again. The diagram is a hash-order artifact and must be withdrawn |
| reading the rendered map by eye | Neither wrong nor rigorous — the map *draws* by ordinal, so the **shape** was real, but the per-column run counts were never measured |

**The third is the likeliest and the most awkward**, because a visual reading is genuinely informative about shape while being unquotable as a run count. If that is the answer, say so plainly: the observation stands, the diagram does not.

---

## 3. The measurement

From `--dump-positions`, on the same scenario T1 ran — a 30-node rolling refresh biased to one pool.

### 3.1 Two different piece definitions — say which you are computing

Revision 1 blurred these. They are related and they give different numbers.

| | Set | A run is broken by |
|---|---|---|
| **Region pieces** (`pool_label_pieces`) | all provinces of one pool | another pool's province · ghost ground |
| **Changed-set pieces** (what T1 claimed) | provinces that changed since the baseline | **any slot that did not change** — ghosts, other pools, *and* unchanged same-pool nodes |

T1's claim was about the second. **Report it as the second, and state that it is not comparable to `pool_label_pieces`' 4-of-8 figure**, which describes the first.

Computing both is cheap and worth it: the gap between them *is* the answer to "did the pool's shape survive the refresh, or only part of it."

### 3.2 Use `slot_of_row`, do not reimplement it

Claim 3 is the reason. Its doc comment exists because a second derivation of the same inverse is how a label ends up disagreeing with a reference.

Reimplementing `(y-1) / SLOT_STRIDE` inline in a script written to check for exactly this class of error would be the sixth instance of the pattern. If the dump emits ordinals directly, use those and skip the conversion entirely.

### 3.3 Exclude the unpooled sentinel

Per claim 6. If the churn fleet carries unpooled nodes, they must be excluded the same way, or the numbers are not comparable to anything else in the record.

### 3.4 Do not derive this from the model walk

The whole point. v1.17.0 §4's lesson is operative:

> A measurement must not be derived by the same reasoning as the thing it measures.

The dump is the independent source. Adjacency comes from **ordinals**, never from record order in the file, which inherits whatever order the emitter used.

### 3.5 Report the largest-piece share, not only the count

A piece count alone can look tidy while describing speckle. Three runs of two provinces each, separated by unchanged ground, share a pool but are not a shape.

Report per zone and for the fleet: number of pieces, largest piece as a share of the changed set, and the size of each piece.

---

## 4. The discrimination check

Standing requirement, with a specific form here.

**Compare against a pool-blind control.** Take the same number of changed slots, distributed at random across the zone's ordinals, and compute the same statistics.

If the real `sys` refresh produces piece counts indistinguishable from random placement, then the change did not read as pool-shaped **whatever the map drew** — the shape was in the reader, not the data.

This is the check that decides the downstream question, and it is cheap: one shuffle and the same arithmetic. Run it several times and report the distribution, not a single draw.

---

## 5. What the answer decides

**If T1's diagram was sound and the pieces are genuinely contiguous:** the shape was real on this fleet, and the open item stays as recorded — pool-shaped change reads as a shape *when allocation order happens to make it contiguous*, which is a fixture property rather than a map guarantee.

**If the diagram was a hash-order artifact, or was never measured:** T1's one piece of positive evidence for the map is withdrawn, and its gate verdict becomes purely negative rather than mixed. That does not change T1's recommendation — merge into A5 — but it removes the only evidence that pool-shaped change is legible at all, which makes `region ← pool ∩ zone` a firm T2 blocker rather than a strong suspicion.

**Either way**, the record must carry the method, not just the number. T1's report is currently the only evidence on this question and it does not say how it was derived.

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 7 is why this session exists**, and it applies to the **measurement script**, not to product code — which is where the previous instance of this error lived. The 1-of-8 figure was a model walk, not a rendering bug.

**Question 3 is live:** §3.1's two definitions constrain the same word, "piece", and diverge on any fleet where a pool's provinces are not all changed. That divergence is the interesting number, not a nuisance.

**Question 2:** a zone with no changed slots reports "no changed slots" — not "1 piece", not "0% largest". An empty set has no largest piece.

---

## 7. Acceptance

- [ ] T1's derivation method identified and recorded
- [ ] Changed-set pieces re-derived from `--dump-positions`, by ordinal, using `slot_of_row` or the dumped ordinal directly
- [ ] Region pieces computed alongside, and the two explicitly distinguished
- [ ] Unpooled sentinel excluded
- [ ] Largest-piece share reported, not only counts
- [ ] Pool-blind control run repeatedly; distribution reported
- [ ] T1's published diagram corrected or confirmed **in T1's report**, not only here
- [ ] The open-decisions row for pool-shaped change updated with the evidence and its method
- [ ] Standing questions answered
- [ ] No product code changed

---

## 8. What this session must not do

**No T2 work, and no `region ← pool ∩ zone` work.** This measurement decides how strongly that item blocks; doing it here presumes the answer.

**No re-running T1's gate.** The verdict stands regardless — T1 was killed by the discrimination check against fresh ground, which this does not touch.

**No tuning toward a tidy number.** If the pieces come back fragmented, that is the finding, and it is the more useful one.

---

## 9. Estimate

**Two to three hours.** The dump exists, `slot_of_row` is `pub`, and the control is a shuffle. Most of the time is reading T1's implementation to answer §2 — and `pool_label_pieces` must be re-implemented rather than called (claim 4), which is small but is not zero.
