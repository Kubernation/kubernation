# T0 — consolidation: the report existed, and one number was missing

**Guidance:** `docs/kubernation-t0-history-substrate-guidance.md`
**Version:** v1.21.1 · **Date:** 2026-08-07
**No product change** beyond one misplaced doc comment. No new instrument.

**§1's answer: the report exists**, at HEAD and at `62101a7`, unmodified since —
`docs/reports/t0-history-substrate.md`. It answers all four of §2's questions.
So per §9 this is a consolidation, and the deliverable is a **corrected, findable
record** plus the one thing T0 did not measure.

**The new number: the event ring's cap binds.** T0 measured occurrences; the cap
acts on *distinct keys*, which nobody had counted. Measured this round: **724
distinct keys against a cap of 500** on a 4-node dev cluster doing ordinary batch
work.

---

## 1. §1 — resolved

| Question | Answer |
|---|---|
| Exists at HEAD? | **yes** |
| Exists at `62101a7`? | **yes** — and byte-identical since; `git diff` is empty |
| Does it answer §2, or only the ring question? | **all four**, though not under those headings |

§0's premise — *"T0 has been cited three times and never written up"* — is
**false**. Every citation it lists is accurate:

- T2-pre's claim 7 attributes the kwok/kind event-volume finding to *"T0 §2.4"*.
  T0 §2.4 is titled *"kwok badly under-represents event volume"*. Correct.
- T1's §1 premise cites the report at `62101a7`. Correct.
- T-fix-2's *"T0's finding"* about latest-occurrence-only is T0 §2.1. Correct.

**Why it looked missing** is worth recording, because it is fixable and this is
the second time the record has cost something: the guidance's own §2 headings do
not correspond to T0's. Guidance §2.4 is *restart behaviour*; T0 §2.4 is *kwok
event volume*. A reader checking "§2.4" against the report finds an unrelated
section and reasonably concludes the phase was partly done. **§4 below is the
fix.**

### 1.1 Where T0 answers §2

| Guidance question | T0 |
|---|---|
| 2.1 what is retained, in what form | §1 table — 7 surfaces, each with form |
| 2.2 the bound | §1 table, "Bound" column |
| 2.3 fidelity | §1 (*"past world states — none"*), §2.1, and §3's byte costs |
| 2.4 restart behaviour | §1 table, "Survives restart" and "Survives context switch" columns |

Nothing in §2 needed re-measuring, and none of it was.

---

## 2. §3 — the seven inherited claims, verified

Every one was `[A]`. All **TRUE**, against source this round.

| # | Claim | Verdict |
|---|---|---|
| 1 | the ring keeps only each key's latest occurrence | **TRUE** — `watch.rs`: `g.retain(\|e\| e.key() != rec.key())` then `push_back`, `CAP = 500`, `pop_front` |
| 2 | `RecentEvent.onset` via `first_timestamp → event_time` | **TRUE** — `observed.rs:225,247` |
| 3 | `Deploy \|\| operator` escape the window; else `window_min * 60` | **TRUE** — `timeline.rs:437` |
| 4 | layout persists `occupied_at`/`vacated_at`/`last_occupant` | **TRUE** — `layout_store.rs:50,59,70` |
| 5 | operator actions are an in-session ring | **TRUE** — `Mutex<Arc<Vec<OperatorAction>>>`, `OP_LOG_CAP = 64`, not persisted |
| 6 | `set_layout_note` is a transient toast | **TRUE** — `take_layout_note`, *"read once and cleared"* |
| 7 | kwok emits almost no events | **TRUE** — T0 §2.4, and re-confirmed incidentally here: churn showed **1** event object while kind under load showed **743** |

**Claim 3 nearly went wrong, and the near-miss is the lesson.** `timeline.rs:72`
lists four kinds — `Deploy | Scale | Operator | NodeChange` — which reads like the
window predicate and is not: it is `is_change()`, the correlation "preceded by"
cue. The window predicate is thirty lines later and is genuinely two-armed
(`e.kind == Deploy || e.operator`). Confirming the claim against the first match
would have produced a confident, wrong answer about which entries survive
windowing.

**§3's own guess was right:** claims 5 and 6 together do give the shape of the
answer, and §2.4's striking statement (§3 below) is true.

---

## 3. §4 — the bound, confirmed empirically

T0 measured **occurrences**: 26,217 collapsing to 29 ring entries on kind. That
demonstrates dedup. It does not touch the cap, because the cap acts on the
**distinct-key population**, which nobody had counted.

Counted this round, using kubectl and arithmetic — no new instrument:

| State | event objects | distinct ring keys | vs `CAP` = 500 |
|---|---|---|---|
| kind, quiesced | 3 | **2** | far under |
| churn (kwok), quiesced | 1 | **1** | far under |
| kind, after a 120-pod Job | 493 | **483** | just under |
| kind, after 180 pods total | 743 | **724** | **224 beyond** |

**So the bound binds, and ordinary work reaches it.** 180 short-lived pods — a
trivial batch on any real cluster — put the key population 45% past the ring's
capacity on a four-node dev cluster.

This sharpens T0 §2.1 rather than contradicting it. The ring is not merely "a
latest-state set"; it is a **bounded** latest-state set whose bound is reachable,
and eviction is by **recency of last touch**, not by age of onset. A chronic
failure that stops being re-reported falls out while newer noise stays.

**What was not observed, and why.** The eviction itself is source-verified —
`while g.len() > CAP { pop_front() }`, three lines below the constant — not
watched happening. The ring is in-process and **no existing emitter reveals its
size**: `--postmortem` renders through `build_timeline`, which applies its own
recency window and cluster cap, so it cannot discriminate a 500-entry ring from a
larger one. Per §4's "do not build an instrument", it was not built. The risk §4
guards against — *a constant naming a bound the code does not enforce* — is
absent here: the enforcement is visible and adjacent.

---

## 4. §2.4, stated plainly, as the guidance asks

> **Succession is the only thing about which this product has a memory across
> sessions.**

`occupied_at` and `vacated_at` in the layout store are the only history
KuberNation itself persists. Everything else is either in-session (metrics rings,
SLO rings, operator actions, the snapshot) or rebuilt from etcd at launch (the
event ring) — and the one durable-looking exception, ReplicaSet revision history,
**is not ours**: it survives restart because it is the cluster's, bounded by the
cluster's own `revisionHistoryLimit`.

That is a striking fact about a product whose map makes temporal claims, and it
is why A5's fresh ground is the only map feature that can answer *"what changed"*
about a moment before the process started.

---

## 5. §5 — which row T3 is in

**Row four: "one state plus a latest-only ring."**

`snapshot: Mutex<Option<Arc<Snapshot>>>` holds exactly one — the current — and
`prior_hot` carried between ticks is a `Layout`, not a `Models`. Add the event
ring, now measurably bounded, and that is the whole substrate.

Row four's consequence: **T3 needs a storage phase first, which the plan never
costed.** True as written — though T0 §3 subsequently *did* cost it, and the
answer is that it is cheap: a six-frame strip is ~23 KiB at 100 nodes and needs
no persistence at all, since "the last N polls" is inherently a recent window.

**So the planning doc's guess is half right, and the half that is wrong matters.**
It supposed persisted summaries were *"probably right for T1 and insufficient for
T3"*. Insufficiency is not the problem — **absence** is. What exists is summaries
for exactly one fact; what T3 needs is a modest in-session ring that nobody has
built. Cost was never the blocker.

### 5.1 A correction to the workstream closeout

`kubernation-workstream-t-planning.md` §3's T3 outcome, written earlier today,
says *"T0 says the substrate is not there"* and that building the store *"is the
phase, not a prerequisite to it"*. Both are defensible, but the framing invites a
reader to infer that storage is expensive, which T0 §3 explicitly refutes.
Corrected there in this commit: T3's blocker is that **T1 removed the reason to
build it**, not that the store would be costly.

---

## 6. §6 — standing questions

**1. Summing before comparing?** Not present. The nearest miss is §2's claim 3,
which is a *matching* error of the same family: two predicates over the same enum,
thirty lines apart, and the wrong one matches first.

**2. Unknown, or fabricated?** §6's instruction was to report an undeterminable
bound as unknown rather than infer it. Not needed — every bound in T0's table is
determinable from source, and I read each. The one thing that *is* unknown is
whether eviction was observed; §3 says so explicitly rather than implying the
measurement covered it.

**3. Two sections constraining one behaviour?** Guidance §2.2 (the bound) and §4
(confirm it empirically) diverge here: the bound is knowable from source but its
*enforcement* is unobservable through existing surfaces. Resolved by separating
what was read from what was measured, and saying which is which.

**4. Consumers depending on an old meaning?** None — no behaviour changed.

**5. Inherited claims?** §6 called this the question's sharpest test, and it was:
seven claims, all inherited, several from a report the guidance suspected did not
exist. All seven true, the report real, and the near-miss on claim 3 is what the
question is for.

**6. One side of a comparison moved?** T0 counted occurrences; the cap counts
keys. Those are different quantities, and reading the first as evidence about the
second is precisely what left the bound unexamined for three days.

**7. Container adjacency read as world adjacency?** Not present. The ring is a
`VecDeque` whose order is recency, and it is consumed by key lookup and by
`build_timeline`'s own sort — never by position.

---

## 7. §7 — acceptance

- [x] §1 resolved — the report exists and covers §2; nothing re-measured
- [x] All four §2 questions answered, with the map from guidance headings to T0's
- [x] The bound confirmed empirically — and the part that could not be observed is named
- [x] §2.4 answered plainly — §4
- [x] The planning doc's guess confirmed/refuted — §5, half right
- [x] Every §3 claim verified
- [x] T3's row identified — row four, §5
- [x] Standing questions answered
- [x] No new instrument built

**Deviation, stated:** §8 says no product code changed. One doc comment moved —
the event ring's own description was attached to `spawn_dynamic`, the function
above it, so the ring was documented on the wrong thing. Given §0's complaint is
that this substrate is hard to find, leaving it misfiled seemed the worse trade.
No behaviour, no signature, no test.
