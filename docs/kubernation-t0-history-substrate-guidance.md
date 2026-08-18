# KuberNation — T0: The History Substrate

**Measurement guidance**
**Goal:** establish what history this product retains, where, and for how long — so T3 can be scoped against a fact rather than an assumption.
**No product change.** The output is a report and, if the answer is thin, a smaller T3.

---

## 0. Why T0 has no report

T0 has been cited three times and never written up:

- T-fix-2 cites *"T0's finding"* that the event ring keeps only each key's latest occurrence
- T2-pre's claim 7 — *kwok emits almost no events; kind is where event-derived behaviour must be measured* — is attributed to *"T0 §2.4"*
- T1's §1 premise cites `docs/reports/t0-history-substrate.md` at commit `62101a7`, and the T1 report says the document **does** exist

So either a report exists and is not in the working set, or fragments have been attributed to a phase that was partly done. **Resolve that first** (§1) — the answer changes whether this is a measurement or a consolidation.

Either way the question the planning doc asked has not been answered in a form T3 can use:

> How much history does KuberNation keep, and where? In-session ring, persisted store, or re-derivation from the current snapshot?

---

## 1. Step one: find out whether T0 exists

- [ ] Does `docs/reports/t0-history-substrate.md` exist at HEAD? At `62101a7`?
- [ ] If it exists, does it answer §2's questions, or only the ring question the later rounds cite?
- [ ] If it partly answers them, this session **completes** it rather than duplicating it — and says so

**Do not re-measure what is already recorded.** The record has already cost two arithmetic errors from re-narrating rather than re-deriving; the opposite error — measuring something already measured and reporting a second, slightly different number — is equally corrosive.

---

## 2. What has to be established

Four questions. T3 needs all four; T1's retrospective needs the first two.

### 2.1 What is retained, and in what form

| Candidate | Where to look |
|---|---|
| The event ring | `watch.rs` — cited as keeping each key's **latest** occurrence only |
| Snapshot history | Whatever `Net` holds between ticks |
| `NodeDetailModel`'s `cpu_history` / `mem_history` | Per-node series, if they persist beyond a tick |
| The layout store | `occupied_at` / `vacated_at` — the only **persisted** per-entity change timestamps |
| `Annals` / `build_timeline` inputs | Whether it reads retained state or re-derives from the current snapshot |

For each: **what is kept, keyed how, bounded how, and does it survive restart.**

### 2.2 The bound

Ring size, retention window, or uptime? This is the number T3 lives or dies on.

A strip of six frames needs six retained states. If the answer is "one, plus a ring of latest-only events," then T3 as the plan describes it — *the last N polls as a strip* — **cannot be built without new storage**, and that is a different phase from the one the plan costed.

### 2.3 Fidelity

Can a past state be **redrawn**, or only summarised?

- **Redrawable** — a full `WorldModel` per retained tick. T3's drift-plate specification (identical projection, persistent labels) is satisfiable
- **Summarisable only** — a handful of numbers per province per tick. Enough for a choropleth delta, **not** enough to redraw a frame with labels

The planning doc guessed "persisted summaries" is right for T1 and insufficient for T3. Confirm or refute that guess; it is currently an assumption carrying a phase.

### 2.4 Restart behaviour

A4 established that the layout survives restart. Does anything else?

If the only durable history is `occupied_at` and `vacated_at`, then **succession is the only thing about which this product has a memory across sessions** — which is a striking fact about the map's temporal claims, and it should be stated plainly if true.

---

## 3. Verify before building

Every claim below is `[A]` — VOR was unavailable when this was written, and all of these come from prior reports rather than from source read this round. **Verify each.**

| # | Claim | Source |
|---|---|---|
| 1 | The event ring keeps only each key's **latest** occurrence | T-fix-2, citing T0 |
| 2 | `RecentEvent` now carries `onset` via `first_timestamp → event_time` | T-fix |
| 3 | `Deploy \|\| operator` escape the recency window; everything else is windowed by `opts.window_min * 60` | T-fix |
| 4 | The layout store persists `occupied_at`, `vacated_at`, `last_occupant` per slot | A4, A5 |
| 5 | `OperatorAction` / `net.operator_actions()` is an **in-session ring**, rendered by the Annals | A5 core |
| 6 | `set_layout_note` is a **transient toast** cleared on first read — nothing accumulates | A5 core §2 |
| 7 | kwok emits almost no events; kind is required for event-derived measurement | T0 §2.4, via T2-pre |

**Claim 5 and 6 together are the shape of the answer.** If the operator ring is in-session and layout notes are transient, then the durable surface is narrow — and §2.4's striking statement is probably true.

---

## 4. Method

This is a **source-and-configuration** measurement, not a live one. Most of it is reading declarations and bounds.

**One live component, and only one:** confirm the bounds empirically rather than from the constant. Run the app against kind, generate more than the bound's worth of events, and confirm the oldest fall off as declared. A constant naming a bound the code does not enforce is exactly the class this project keeps finding.

**Do not build an instrument.** If a figure needs measuring, take it from `--dump-positions` or `--postmortem`, both of which already emit. A new comparator for a one-off inventory would be the eleventh instrument in this project, and the last one shipped a false positive.

---

## 5. What the answer decides

| If history is | Then T3 is |
|---|---|
| Six-plus redrawable states retained | The phase the plan describes. Proceed to the legibility gate |
| Bounded by uptime, redrawable | The same phase with a stated limit: no strip spans a restart |
| Summaries only | **Not small multiples.** A change-since choropleth over time — which T1 already refuted as a separate overlay |
| One state plus a latest-only ring | T3 needs a storage phase first, which the plan never costed |

**The third row is the one to watch.** If the substrate only supports summaries, T3 collapses into something T1 already tested and killed — and the honest conclusion is that Workstream T is finished, not that T3 needs building differently.

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 5 is the whole of §3** — every claim here is inherited, several from a report that may not exist in the working set. This is the sharpest test the question has had.

**Question 2 applies to the inventory itself:** a retention bound that cannot be determined from source is **unknown**, not unbounded and not zero. Report it as unknown rather than inferring it from observed behaviour on one run.

---

## 7. Acceptance

- [ ] §1 resolved: whether a T0 report exists, and what it already covers
- [ ] All four §2 questions answered, per surface, with source references
- [ ] The bound confirmed empirically, not only from a constant (§4)
- [ ] §2.4 answered plainly — if succession is the only cross-session memory, say so
- [ ] The planning doc's "summaries are right for T1, insufficient for T3" guess confirmed or refuted
- [ ] Every §3 claim verified or marked as unverifiable
- [ ] T3's row in §5 identified
- [ ] Standing questions answered
- [ ] No product code changed, and no new instrument built

---

## 8. What this session must not do

**No T3 work.** This decides whether T3 is the phase the plan describes.

**No storage work.** If the answer is "T3 needs a storage phase", that is a finding to report, not to start.

**No re-measuring what a prior T0 already established** (§1).

---

## 9. Estimate

**Two to three hours**, most of it reading. Less if a T0 report already covers §2 — in which case this is a consolidation and the deliverable is a corrected, findable record rather than new work.
