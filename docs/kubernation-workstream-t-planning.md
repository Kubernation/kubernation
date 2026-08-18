# KuberNation — Workstream T: Time on the Map

**Planning artifact, not implementation guidance.** Per-phase guidance follows, one at a time, after each gate.

Governing doc: `kubernation-enabling-plan.md` §7 (What A unlocks: time), §7.1 (the drift plates), §7.2 (showing a projected state).

---

## 0. Status — CLOSED, 2026-08-07

The workstream ran to its declared kill point and stopped there. §6 named T1's
gate as the point of death; **it fired**, and T2 was then refuted independently.

| Phase | Status | Evidence |
|---|---|---|
| **T0** history substrate | **done** 2026-08-04, no code | `docs/reports/t0-history-substrate.md` |
| **T-pre** fault-line frequency | **done** 2026-08-04 — and found a live defect | `docs/reports/t-pre-fault-line-frequency.md` |
| **T-fix / T-fix-2** that defect | **shipped** v1.11.2 | `docs/reports/t-fix-onset-fault-lines.md`, `t-fix-2-correlation-rule.md` |
| **T1** change-since | **gate mixed-to-negative**; merged into A5 as `NewGround`, v1.13.0 | `docs/reports/t1-change-since.md` |
| **T2** fault-line marking | **REFUTED** — §3 | `docs/reports/t2-pre-failure-clustering.md`, `t2-pre-pool-gap.md` |
| **T3** small multiples | **blocked — both preconditions failed** — §3 | — |

**The question in §1 is answered: no, on the evidence available.** Putting change
on the map did not beat the list that already exists — for the change axes that
could be tested. That is a real answer, it was reached cheaply, and it is what
the workstream was set up to find out.

**What shipped anyway**, which is more than §6's "salvage if it dies" expected:

- **T-fix + T-fix-2** — the Annals' fault line was anchoring on the wrong
  timestamp and its correlation rule compared incommensurable sides. Real defects
  in shipped behaviour, found by measuring for T2 and fixed independently of it.
- **T1, merged rather than shipped** — `NewGround{Off, Fading, Since}` gave A5's
  fresh ground a fixable baseline. One feature with two modes instead of a ninth
  overlay; the merge was T1's own recommendation after its gate.
- **The pool sentence** (v1.21.0) — what T2's measurement identified as actually
  missing, which was words in a concern rather than marks on a map.
- **The instruments** — `hack/churn/pieces.py` (three modes, self-tested),
  `--dump-positions`, and the reversible failure scripts for both clusters.

---

## 1. Why this is the plan's destination, and the one hard question

Plan §1's competitive claim rests on four question shapes. Three are settled:

| Question | Answered by |
|---|---|
| What is the state of X? | lists — K9s wins, and that is fine |
| What is the shape of the whole? | the map, since v1.0 |
| What is unusual here? | overlays — substrate is the proof |
| **What changed, and where?** | **the Annals, mostly** — and the map for exactly one axis: A5's `NewGround`, succession. Settled by this workstream; see §0 |

`timeline.rs` already computes the temporal analysis — `Annals`, fault lines, `annals_lines_flags_suspect_change_before_failure`. **And renders it as lines**, which is exactly what K9s or Freelens would do. The analysis is finished; the map has never been used for it.

**The hard question this workstream must answer, and should answer early:**

> Does putting change on the map beat the list that already exists?

Workstream A had a thesis nobody could falsify in a single gate. T does not have that excuse: the Annals is a working comparison, on the same data, in the same app. If a spatial expression is not better than it, that is a real answer and it is cheap to reach.

**Every phase below should be gated against the Annals, not against nothing.**

---

## 2. What A made possible, precisely

Three properties, each earned by a phase, and each strictly required here:

| Property | From | Why time needs it |
|---|---|---|
| Provinces and cities hold still | A2, A3 | Two frames cannot be compared if the layout moved between them |
| Layout survives restart | A4 | A change-since baseline older than one session is otherwise impossible |
| A declared, invariant frame | A6 | Wegener's requirement — frames laid against each other need a stated anchor |
| Succession already recorded | A5 | `occupied_at` is a change timestamp the map already reads |

**A5 is the one to notice.** Fresh ground is already a change-over-time expression — it answers *what changed, and where* for exactly one kind of change (a slot's occupant), over exactly one window. T generalises that rather than starting from nothing.

That suggests the cheapest first phase is not the most impressive one.

---

## 3. Candidate phases

### T1 — Change-since overlay · **GATED MIXED-TO-NEGATIVE — merged, not shipped**

**Colour provinces by what changed since a chosen moment**, rather than by current level.

The nearest thing to A5's fresh ground, and it reuses that machinery: a timestamp per province, a window, a tint. What differs is that the *baseline is chosen* rather than rolling, and the *change is a delta* rather than a boolean.

- Cheapest of the three — it is a new `Overlay` variant on an axis with eight
- Composes with the existing style and register axes for free
- The obvious question it must answer: **change in what?** Health, saturation, cost, pod count, and "occupant" are all candidates, and picking one is the phase's real content

**Gate:** show a change that the Annals reports, and one it does not. If everything T1 shows is already a line in the Annals, T1 is a prettier list.

---

#### Outcome — the kill point fired

(`docs/reports/t1-change-since.md`, v1.12.0.) Built as a ninth overlay, gated,
and **the discrimination check is what settled it**: T1 marked 288,027 px, while
A5's fresh ground — already shipped — marked 292,811 px on the same provinces,
within 2%. A control with fresh ground disabled confirmed T1 was rendering its
own answer rather than bleeding through. The answer simply coincided.

Against the **Annals** the verdict was mixed rather than negative: the map wins
on *what did not change* and on immunity to the list's 80-entry cap, while the
list wins on naming the thing that changed, because node names carry the pool.
Neither dominates.

T1's real contribution turned out to be **reach**, not a different fact — and
reach was already a setting on fresh ground's ageing window. Hence its own
recommendation, taken in v1.13.0: **merge, do not multiply.** `Overlay::Changed`
and its machinery were deleted and `NewGround{Off, Fading, Since}` took their
place, one feature with two modes. Verified complete in v1.19.0, which also
restored the one thing the merge had dropped — the panel saying *"no succession
on record"*, which is not the same claim as *"unchanged"*.

The shape evidence T1 published was re-derived in v1.18.1
(`docs/reports/t1-shape-rederivation.md`): the method was sound, one of four
columns was wrong, and the conclusion held.

### T2 — Fault-line marking · **REFUTED 2026-08-07 — do not build**

**Put the Annals' existing conclusion on the map.** `timeline.rs` already identifies the suspect change before a failure; T2 marks *where* it happened.

- Almost no new analysis — this is a rendering of a finished computation
- The strongest single claim in the workstream: a list can tell you *what* changed before a failure; only the map can show you that the changes cluster in one zone, one pool, one rack
- Risk: if fault lines are rare, the feature is invisible most of the time. Worth measuring frequency on a real cluster before building

**Gate:** a fault line whose *spatial* pattern says something the Annals line does not.

---

#### Outcome — the premise does not hold, and the salvage has shipped

Measured before scoping, in three rounds. The gate above was never reached,
because the claim it tests turned out to be false one level down: **the failures
do not cluster spatially in the first place.**

**Round 1 — which dimension do failures cluster in?**
(`docs/reports/t2-pre-failure-clustering.md`, v1.20.1.) Four failure shapes
induced separately on kind, each with its expectation stated in advance:

| Shape | node | workload |
|---|---|---|
| crash-looper | 3 groups, **P=0.2465** | 1 group, **P=0.0000** |
| bad rollout | 3 groups, **P=0.7240** | 1 group, **P=0.0020** |
| unbindable PVC | **not attributable** | 1 group, **P=0.0000** |
| node down | **no failing pods at all** | — |

Every constructible pod-level failure was workload-clustered and
node-**scattered**. The observed node distributions were 3/3/3 and 2/2/1 — as
even as the counts allow, so low power is not hiding a cluster. Two findings the
plan did not anticipate: **unschedulable pods have no `nodeName`**, so a whole
class of trouble has no position on any geography; and the one genuinely
node-shaped failure produced **zero failing pods**, at T+90s and past the
eviction timeout, because pods keep the status the dead kubelet last reported.
Its entire signal is the node's own condition — which the map already renders.

**Round 2 — the pool dimension, on a fleet that has pools.**
(`docs/reports/t2-pre-pool-gap.md`, v1.20.2.) kind cannot express `pool` at all,
so this was re-run on the 100-node churn fleet. A failure confined to **100% of
one nodepool** — `pool` P=**0.0000**, `node` P=1.0000 — renders as **8
disconnected pieces across 3 columns**, largest holding 40–67%. The
workload-shaped contrast arm gives 14 pieces across 4 zones, so pool-shaped is
2.7 pieces per zone against 3.5: modestly more contiguous, and both are scatter.
The cause is A2's zone-wide ordinals interleaving pools — the same fragmentation
T1 §3.1 found, confirmed from the failure side.

And worse than "not a shape": those 29 failures **drew no trouble mark at all**,
because `node-agent` is a DaemonSet and DaemonSets are roads, not cities. The
alternative is no better — a Deployment pinned to a pool sites its city at the
*plurality* node, so thirty spread failures would read as one troubled city.

**Round 3 — what was actually missing.** (v1.21.0.) Neither surface named the
pool: the map could not draw it, and the queue correctly aggregated to one
**workload**-grouped concern reading `×29`. So the gap was never a rendering
problem. `attention::pool_confinement` now appends the fact to the concern —
`ds churn/node-agent — CrashLoopBackOff ×29 · all 29 placed on pool sys` — pure,
unit-tested, and riding the three surfaces that already show `detail`.

**The risk named above was the wrong one.** The entry worried that fault lines
might be *rare*. Frequency was never the binding constraint; **shape** was. A
pool-confined failure is neither rare nor invisible — it is simply not a shape,
once laid on a zone-organised geography.

**What this does not settle:** whether real-world failures *tend* to be
pool-shaped. That is unanswerable on any test cluster, since every failure here
is induced. It does not change the conclusion, because the finding is about
**rendering**, which is fixture-independent: even granting the most favourable
possible frequency, the map still does not draw it as a shape.

**`region ← pool ∩ zone` is not the missing prerequisite.** It shipped in
v1.14.0–v1.17.0 (pool colour, region labels, the POOLS legend) and did not help,
because there is no mark on the provinces for a pool tint to group.

### T3 — Small multiples · **BLOCKED — both preconditions failed**

**The last N polls as a strip**, identical classing, so change is seen rather than read.

The plan's §7.1 gives the specification, drawn from the drift plates:

1. Identical projection and graticule in every frame — A6 supplies this
2. **Identical classing and palette** across the series — Brewer's requirement
3. **Labels persist across frames**, so the eye tracks one entity through the series
4. Direction is marked, not inferred

Rule 3 is flagged as the one most likely to be dropped for space and the one that makes the series legible.

- The most striking, and the most expensive
- Needs history: N retained world states, which is a storage question A4 did not answer
- Needs a small-frame rendering mode — at fleet scale a single frame is already dense, and six side by side may be unreadable

**Gate:** at 100 nodes, is a six-frame strip legible? Per A2's density finding, a frame that must stop drawing labels to fit is a frame that has stopped being comparable.

---

#### Outcome — not built, and its gate was never reached

§5 made T3 conditional: *"only if T0 says the substrate supports it and T1 says
spatial change reads."* **Both conditions failed.**

- **T0 says the substrate is not there.** Persisted world states "do not exist,
  in any form"; what exists is persisted *summaries*, for exactly one fact — the
  layout's `occupied_at`. T3 needs N retained states to redraw past frames, and
  none are kept: `snapshot` holds exactly one, the current.
  **Not because a store would be expensive** — T0 §3 costed it at ~23 KiB for a
  six-frame strip at 100 nodes, needing no persistence at all, since "the last N
  polls" is inherently a recent window. The blocker is *absence*, not cost, and
  the store is a phase the plan never listed. (Corrected 2026-08-07: this entry
  first read as though storage were the obstacle, which T0 explicitly refutes —
  see `docs/reports/t0-consolidation.md` §5.)
- **T1 did not say spatial change reads.** Its gate came back mixed-to-negative,
  and that — not the storage question — is what removed the reason to build T3.

So T3 would have meant building a history store first, to feed a strip whose
premise the cheapest phase had already failed to support. That is precisely the
sequencing §6 was written to prevent.

---

## 4. Sequencing, and the two prerequisites nobody has costed

```
T0  (history substrate)  ─────→ ANSWERED — no persisted world states exist
                                 
T1  (change-since)  ────────────→ GATED, merged into A5 as NewGround
T2  (fault lines)   ────────────→ REFUTED — failures are not spatial; §3
T3  (small multiples) ──────────→ BLOCKED — needs T0's store, and T1's answer
```

### T0 — the unasked question · **ANSWERED 2026-08-04**

**How much history does KuberNation keep, and where?**

`timeline.rs` computes over something, but whether that is an in-session ring, a persisted store, or a re-derivation from the current snapshot is unverified — and it decides whether T1 and T3 are cheap or foundational.

Three shapes, each with a different cost:

- **In-session only** — T1's baseline cannot precede app launch, T3's strip is bounded by uptime. Cheapest, and weakest.
- **Persisted world states** — full fidelity, real storage cost, and a format that outlives the process (A4's lesson: that surface deserves review attention out of proportion to its size).
- **Persisted summaries** — per-province, per-poll, a handful of numbers. Bounded growth, enough for a choropleth delta, not enough to redraw a past frame.

**The third is probably right for T1 and insufficient for T3**, which is a reason to sequence T1 first and let its experience size T3.

Recommend: **measure what exists before scoping anything.** This is the same move that shrank A3 from "city slots" to two lines.

**Answer** (`docs/reports/t0-history-substrate.md`): of the three shapes above,
**in-session only** describes metrics, SLO and operator actions; **persisted
world states do not exist, in any form**; and **persisted summaries already
exist, for exactly one fact** — the layout's `occupied_at`, which is the only
persisted per-entity change timestamp in the product.

That decided the workstream's shape. It is why T1 was scoped on the *occupant*
axis — the one axis whose baseline can precede app launch and survive restart —
and why T3 was left conditional. Measuring first was worth it, again.

### T-pre — the instrument · **RUN 2026-08-04, and it found a defect**

Every gate above is comparative and perceptual, which is the combination this project has repeatedly failed to measure honestly. Six instruments have emitted a plausible number for the wrong reason.

The gate here is *"is the spatial expression better than the Annals?"* — and there is no existing instrument for it. The pixel comparator measures rendering, `--dump-positions` measures assignment; neither measures **whether a person learns something faster.**

That may mean the gate is human, like A6's. If so, say it up front rather than discovering it at the end.

**What happened** (`docs/reports/t-pre-fault-line-frequency.md`): the frequency
question T2's risk note asked was **the wrong question**. Fault lines are common.
What is not common is the *correlation* T2 wanted to draw — it was suppressed on
any cluster carrying a chronic failure, which is most real clusters. A defect in
shipped behaviour, found while measuring for a feature that was never built, and
fixed in v1.11.2 (T-fix) and after (T-fix-2).

The human-gate warning also held: T1's decisive evidence came from a
**discrimination check against the map's own existing feature**, not from a
usability judgement, and T1's report declined to claim a usability verdict alone.

---

## 5. Recommended order

1. ~~**T0 — measure the history substrate.**~~ **Done 2026-08-04.** It determined everything else, exactly as expected — see the T0 outcome in §4.
2. ~~**T1 — change-since overlay.**~~ **Built and gated 2026-08-06; mixed against the Annals, negative against the map's own fresh ground.** Merged into A5 as `NewGround` rather than shipped as a ninth overlay. It cost about a day to find out, which is what the sequencing was for.
3. ~~**T2 — fault lines.** Reuses a finished computation; the strongest claim; measure fault-line frequency first.~~ **Measured and refuted, 2026-08-07 — see the T2 outcome in §3.** Failures cluster by workload, not by location; the one spatial signal that exists is node condition, which the map already shows. The salvage — naming the nodepool a workload's failures are confined to — shipped in v1.21.0 as a sentence in the concern, not a map feature.
4. ~~**T3 — small multiples.** Only if T0 says the substrate supports it and T1 says spatial change reads.~~ **Neither condition held** — see the T3 outcome in §3. Not built.

---

## 6. Where this workstream dies — **it did, and here is how**

Named up front, per the discipline A2's guidance established and A4 §8.1 settled.

**T1's gate is the kill point, and it is a real one.** If a change-since overlay shows nothing the Annals does not already say more clearly, then "what changed and where" is a question the list answers adequately, and the map's advantage is confined to shape and anomaly — which it already has.

That would not invalidate Workstream A: stability is what makes the *current* map trustworthy, and A's own gates passed on their own terms. But it would settle plan §7 negatively, and the honest response would be to stop rather than build T2 and T3 on a thesis the cheapest phase refuted.

**Salvage if it dies:** T0's measurement, and the finding itself — which is information about the product no amount of planning produces.

---

### How it actually went, 2026-08-07

**The kill point fired, and the prediction above was accurate but incomplete.**

T1's gate did settle it, and the honest response — *stop rather than build T2 and
T3 on a thesis the cheapest phase refuted* — is what happened. T2 was measured
before scoping rather than taken on trust, and was refuted on its own terms too,
so the stop rests on two independent findings rather than one.

**Where the prediction was incomplete:** it treated the salvage as consolation.
It was not. Measuring for a feature that was never built found and fixed **two
real defects in shipped behaviour** (T-fix, T-fix-2), produced a merge that made
an existing feature better (`NewGround`), and identified the one thing genuinely
missing — which turned out to be a sentence in a concern, not a mark on a map
(v1.21.0). Four shipped improvements, from a workstream whose headline feature
was never built.

**What is settled, and what is not.** Plan §7 is settled negatively **for the
change axes that could be tested**: occupant succession (T1) and failure location
(T2). It is not a claim that no temporal expression could ever beat a list — only
that these two did not, on this product, measured against the list that already
exists. Anyone reopening it should read T1 §3.3 and `t2-pre-pool-gap.md` §4
first, because both refutations are specific and both name what would have to
change.

**The map's advantage stands where it already was**, which §6 named correctly:
shape and anomaly. The overlays, the attention queue and A5's fresh ground are
that advantage; nothing here reduces it.

---

## 7. Method — carried forward

Standing questions, now **seven** — five here, plus two the T and post-T rounds earned:

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and **does the state each describes actually occur?**
6. When a change moves one side of a comparison, does the other side still mean the same thing? *(earned by T-fix-2: `first_trouble` moved to onset while the change side stayed on `when`.)*
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that? *(earned by the region-label defect: `Continent.provinces` is name-hash order, not map order.)*

Plus the rule A6 made explicit, now six instances deep:

> **Whenever a value has an inverse, the inverse gets a name and one home** — or the two will disagree. (`resolve_region`, `derive_qos`, `worst_level`, `changed_hands`, `fresh_tier`, `slot_of_row`.)

And the standing gate requirement:

> **Every gate needs a discrimination check** — run it against a build with the mechanism disabled and confirm the result moves. Seven instruments had failed this way when this was written, and at least two more have since: the "4 of 8" region figure, narrated from a breakdown rather than emitted, and `pieces.py` fabricating a shared `"?"` group for pods that have no node. It is still the only defence that has worked.
>
> **And its sharper form, from T1:** check whether the instrument *can* discriminate **before** running it, not after seeing the result. T1's gate was decided by a control against the map's own existing feature; the extent-headroom round declined to capture a fleet at all, having established arithmetically that the fleet could not tell the candidates apart.

Claims in every guidance doc are tagged `[V]` verified-this-round or `[A]` asserted-from-a-report.
