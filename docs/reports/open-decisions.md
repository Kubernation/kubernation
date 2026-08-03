# Workstream A — open decisions

**For the planning session** · 2026-08-03 · state as of `v1.9.1` (released: `v1.9.0`)

A rolling consolidation of what needs deciding, gathered from the per-round
reports so they do not have to be read in sequence. Each item states the
evidence, what is blocked by it, and a recommendation. Items already answered by
subsequent rounds have been dropped rather than carried forward.

**Nothing blocks a phase.** A4 shipped and A5's core landed; the immediate work
is A5's second half. Four items change scope, and the overdue release is done.

---

## 1. Next: A5's second half

**Not blocked — scoped and seamed.** A5's core landed in v1.9.1: ground records
when it changed hands, it persists, and cataclysm was resolved as a *record*
rather than a rendering (there is nothing left to draw on after a structural
change — see [a5-succession-core.md](a5-succession-core.md) §2).

What remains is §2.3's **fresh-ground rendering** and §4's **gate**, held back
deliberately because the guidance calls the treatment *"the phase's main
aesthetic decision"* to be made *"against the live map, not in advance"* —
specifically whether ageing should be quantised into two or three steps rather
than a continuous fade, since a continuous fade is hard to read as a wave.

Everything a renderer needs is one call:

```rust
freshness(layout.occupied_at(slot), now, window)
```

Outstanding with it: the ageing window as a persisted, flag-overridable setting;
`cb_*` funnel routing; fresh-versus-ghost distinguishability during a surge; and
the gate with its mandatory discrimination check.

**One input worth having first.** A5's claim-9 refinement matters here: ghosts
settle at **12 on the churn fleet, not the batch size of 10**, because reclaim is
per-(zone, pool). The standing quantity of marked ground is therefore
*per-partition*, so a fleet with many partitions carries more fresh and ghost
ground on screen at once than a single-partition estimate suggests — and the
ageing window's default and that quantity together decide how much of the map is
marked at any moment.

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

### 2.3 The kill point — settled, and worth leaving settled

It moved twice (decomposition said A2; A2's report said A3; A4's guidance said
A4), and A4 §8.1 said **stop moving it**, which its report accepted. The position
now on record:

> **Plan §1's spatial-memory thesis is not testable by any single gate.** A1–A3
> proved the map *can* hold still; A4 proved it holds still across sessions.
> Whether that produces spatial memory in a user is answerable only by someone
> living with it for weeks.

Every gate since has been reported as what it is — real, binary, and silent on
the thesis. Recorded here so it is not relitigated a fourth time.

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

## 4. ~~Overdue housekeeping: the release drift~~ — **done**

**v1.9.0 is released**, carrying 1.7.0, 1.7.1, 1.7.2 and 1.8.0. Signed and
notarized `.dmg`, Linux tarball, Windows zip, `SHA256SUMS`.

The five versions were **not** backfilled as tags, because `gh release list`
against the CHANGELOG showed that is not drift but the project's convention —
v1.6.0's own entry records aggregating 1.2.0 through 1.5.0 the same way.
Backfilling would also have fired five full release builds to publish four
releases nobody would download.

Two things fixed in passing: the release body was a fixed template that never
said what changed (it now links the CHANGELOG at the tag), and the
pool-collision fix is called out explicitly, since it is the one a user might
need to act on.

**Also closed:** this was the last pre-1.0-hardening item needing an operator.
A real tag push has now proven the multi-platform release pipeline green end to
end, which local dry runs structurally could not.

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
| A4 | **v1.9.0** | *open, close, reopen* — 70/70 hold, vs 55/70 without ✅ |
| **A5** | **v1.9.1, core only** | *a refresh reads as a wave* — **not yet run** |
| A6 | not started | two people name the same position |

425 core + 94 GUI tests; gui-smoke 51 states; CI green on every commit.
Released at **v1.9.0**; working version **v1.9.1**.
