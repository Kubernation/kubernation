# Workstream A — open decisions

**For the planning session** · 2026-08-03 · state as of `v1.8.0`

A rolling consolidation of what needs deciding, gathered from the per-round
reports so they do not have to be read in sequence. Each item states the
evidence, what is blocked by it, and a recommendation. Items already answered by
subsequent rounds have been dropped rather than carried forward.

**One item blocks a phase. Four change scope. The rest are housekeeping, one of
which is overdue.**

---

## 1. Blocking: A4's §4 needs rewriting

**Status:** A4 stopped at §0. No code written.

Claim 10 — *"one refresh of N nodes produces N ghosts; accumulation rate is
refresh cadence"* — is false, and it is the claim §4's design rests on.
Ghosts reach a **steady state at the refresh batch size** and stay there: a
100-node fleet refreshed in waves of ten holds ten ghosts after the first
refresh and still ten after the fourth, because each wave's replacements reclaim
the previous wave's ghosts.

The consequences, in [a4-verification.md](a4-verification.md):

- §4.2's sizing is void — ~10 ghosts, not ~200, and it does not grow with cadence
- the automatic **age reap** is left addressing something that does not occur
- what *does* leave lasting ghosts is **shrinkage**, which is already
  compaction's job

**The decision:** what is the automatic reap for, now that accumulation is not
the answer? Three shapes, ranked:

1. **Compaction only** — build persistence, identity and the explicit verb;
   leave the age reap until something demonstrates a need. This is A3 §2.1's own
   principle applied one section over: *do not build it speculatively; the
   measurement exists to size the problem first.*
2. **Both, retention defaulting to `0`** (never) — machinery exists, opt-in,
   honest that the window is unsized.
3. **Both, keeping 14 days**, re-justified as bounding ghosts from *shrinkage* —
   which means a decommissioned pool's ground is reclaimed automatically after a
   fortnight, a different promise from the one §4.2 currently makes.

**A question worth settling first:** is a standing batch-size set of ghosts even
undesirable? Ten reserved slots on a 100-node fleet is the mechanism *working* —
ground held for nodes that may return — and A2's gate found that painting that
ground is what made a refresh read as stable rather than as the continent losing
pieces of itself. An eager reap would undo that.

**Two things for the rewrite regardless:** `Namespace` is not watched anywhere,
so §3's fingerprint needs a new one-shot read (`browse.rs` is the precedent, and
it is a small new read surface on a project whose privilege posture is
deliberate); and `vacated_at` is worth keeping in the format whichever way §4
goes, since A5's ageing wants it.

---

## 2. Scope and ownership

### 2.1 Two settled decisions naming A2 are owned by no phase

Raised in the A2 report, still open.

- **The migration cataclysm** — decomposition §6: *"first run after A2 remakes
  the world once. Declare it."* It has no in-session meaning until persistence
  exists. **Reassign to A4** — and note the A4 guidance already does this for the
  fingerprint-mismatch case (§3), so it may only need saying out loud.
- **`region ← pool ∩ zone`** — shipped in neither form. Pool has no renderer at
  all, so the row is *unimplemented*, not regressed. A2's zone-wide ordinals mean
  pools interleave positionally rather than banding, so contiguity would now need
  durable band ordinals — but plan §3.4.4 already chose colour and label over
  contiguity, which is the cheaper route.

**Ask:** assign both.

### 2.2 A3's charter is closed; the decomposition's A3 is not

Decomposition §4 gives A3 three things: instability sources 2, 3 and 5. Sources
2 and 3 are measured and closed (3 of 3 incumbents moved → 0). **Source 5 —
*"islands stop depending on continent height"* — was never measured and is
untouched.**

**Ask:** measure it with `--dump-positions` before deciding whether it needs
work? The dump already records provinces and ghosts; islands would be a small
addition, and the last three rounds have all found the measurement changed the
scope.

### 2.3 Where the kill point sits now

It has moved twice. The decomposition put it at A2; A2's gate passed for
provinces and moved it to A3; A3's gate passed. The A4 guidance §1 argues it is
now A4 — *"nobody builds spatial memory of a layout that resets on restart"* —
and that is the first version of plan §1's claim a user could actually
experience.

**Worth confirming explicitly**, because the framing decides how much weight A4's
gate carries and what a failure there would mean.

### 2.4 Instance-type as a pool fallback

Open since A1, restated in A2. A node whose instance type changes vacates its
slot, because its pool is then a hardware attribute rather than a declared
identity. Kept because the step only fires on clusters with no provider label at
all (bare metal, kind, kwok), where the alternative is one undifferentiated pool
per zone.

But decomposition §6's settled row reads *"override → standard labels → single
default"*, with **no instance type**. One line either way.

**Ask:** confirm the trade, or align to the literal row.

---

## 3. Deferred with evidence

Not defects; decisions taken with reasons, listed so they are not rediscovered
as surprises.

| Item | Evidence | Where |
|---|---|---|
| **`ExtentSource` has no consumer** — §7's "declared *and* marked" is declared only. An unmeasurable node draws at the default extent with nothing distinguishing it | zero hits outside `world.rs` | A2 |
| **Extent bounds never fire at the sizes they name** — `[32, 128, 512] GiB` compared against *allocatable*, always below nominal, so a nominal 32 GiB node takes the *smallest* class | why the smallest extent is the ordinary case | A2 |
| **The map is ~2/3 ocean** — the stride is the largest extent class (9) while most provinces are 3–5 rows, so about half the rows in a continent are unbuilt and render as sea | the same defect as unpainted ghosts, at smaller scale | A2 |
| **`city_dx` still hashes the bare name** while the row hashes the full ref | an asymmetry, not a defect; changing it moves every city once | A3 |
| **The placement residual** — a city still moves if a colliding newcomer sorts ahead of it | 11.4% on a crowded 4-row/6-city province, 0 on the ordinary case; test ceiling at 20% is the tripwire | A3 |

Two of these are worth pulling forward if anything else touches that code:
**the extent-bounds calibration** (it is a one-line constant change with a
visible effect) and **the stride question** — whether it should be per-zone-max
rather than global-max, and whether intra-slot reserved rows should be painted
like ghost ground. That last one also settles the long-deferred "chunkier
landmasses on multi-node zones" question, **negatively**: at 100 nodes over four
zones the provinces are still thin ribbons.

---

## 4. Overdue housekeeping: the release drift has restarted

**The last tag is `v1.6.0`. The workspace is at `v1.8.0`.** Four versions are
unreleased: 1.7.0, 1.7.1, 1.7.2, 1.8.0 — including the critical fix where
provinces of different pools were drawn on top of each other, which affects any
cluster with two nodepools in a zone.

This is the exact drift `CLAUDE.md` records the project already being burned by:
seventy versions piled up under Unreleased before v1.0.0 and had to be
aggregated. The convention added afterwards says *"actually roll it at tag time
— don't let that drift restart."*

A release has been asked for in **five** consecutive reports (substrate, A0,
A-pre, unmeasurable-capacity, and now) and not answered.

**Ask:** cut `v1.8.0`, or set a cadence. The `CHANGELOG` sections are already
rolled and dated; the signed-release pipeline needs a `v*` tag push and the five
Apple secrets, and pushing one would also prove the multi-platform CI green,
which is the last pre-1.0 item that still needs an operator.

---

## 5. Method — three practices that earned adoption

Each was proposed in a report and has now paid off more than once. They are
cheap, and the case for making them standing rather than per-document is that
every round they were *absent* is a round something got through.

### 5.1 The revised review bar

*"Would this be wrong when the consumer arrives?"* — for phases gated ahead of
their renderer. A0 established that the ordinary "wrong today?" bar cannot
evaluate a consumer-less phase; it returned 0 confirmed against four
demonstrable holes. v1.6.0 confirmed the fix (14 confirmed) and A1 again (10,
including the root).

**Ask:** promote from per-doc guidance to a standing rule.

### 5.2 The standing questions as a checked step

Both of A2's headline defects were **predicted by name** — decomposition §7's
*summing before comparing* is exactly what `province_y` got wrong, and guidance
§9's question 3 named the ghost divergence and even prescribed the fixture that
now tests it. The questions were not the failure; running them was.

**Ask:** make them an explicit pre-review checklist with a written answer per
phase, rather than a remembered list. A3 and A4 both did this and both caught
something.

### 5.3 Question 5 — inherited claims

Added to the A4 guidance because A3 found three requirements resting on a wrong
caution in A3-pre's report. **It caught the false claim on its first outing.**

The lesson is sharper than "prior reports can be wrong": A1's measurement was
correct and correctly reported — it describes a single-wave full-fleet surge,
where N nodes genuinely do leave N ghosts. What failed was the *generalisation*
to batched refreshes and to accumulation over time.

> **An inherited claim needs re-verification against the case at hand, not
> confirmation that its source said it.**

Two rounds running, the false claim in a guidance document was the inherited one.

**Ask:** keep question 5 permanently.

---

## 6. A pattern worth naming before the next phase

Three rounds, three instruments, and each caught something its predecessor
structurally could not see:

- `reshuffle.py` found the **27%** of provinces that moved where the pixel
  comparator reported **~1%**
- `--dump-positions` gave **7-of-7** city coverage where pixels reached **2-of-7**
- A3's collision test caught **its own name generator** faking a 0% result

And the failure mode is identical every time: **the instrument emits a plausible
number for a reason unrelated to what it claims to measure.** Six of A2's
instrument failures were silent; the measurement session added a seventh (component
ids compared across two independently-labelled frames); A3 an eighth.

The only defence that has worked is breaking the instrument deliberately and
checking that it notices — which is now committed as `compare-selftest.py` and
`positions-selftest.py`.

**Implication for A4:** the restart gate is a new instrument. It will need the
same treatment, and §6's mutation floor (*make the load path return
`Layout::default()` and confirm the restart test fails*) is the right shape —
but only if the gate is run before and after that mutation, not merely written.

---

## Appendix — where the workstream stands

| Phase | Status | Gate |
|---|---|---|
| A-pre | shipped, unversioned | six scenarios run reproducibly; a seventh added in A3-pre |
| A0 | shipped, unversioned | prerequisite, consumer-less by design |
| A1 | shipped, unversioned | 100-node surging refresh moves zero slots ✅ |
| A2 | **v1.7.0 / 1.7.1** | *does the map hold still?* — provinces yes, 0.41% silhouette ✅ |
| Measurement | unversioned | instruments committed; pixel method retired for assignment |
| A3-pre | **v1.7.2** | baseline taken: 3 of 3 incumbents moved |
| A3 | **v1.8.0** | *no incumbent moves* — 3 of 3 → **0** ✅ |
| **A4** | **stopped at §0** | *open, close, reopen — same map?* — not started |
| A5 | not started | a refresh reads as a *wave* |
| A6 | not started | two people name the same position |

407 core + 90 GUI tests; gui-smoke 51 states; CI green on every commit.
