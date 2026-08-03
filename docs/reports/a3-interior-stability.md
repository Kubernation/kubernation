# A3 — Interior stability

**Report** · 2026-08-03 · **v1.8.0**
**Governing doc:** [`kubernation-a3-interior-stability-guidance.md`](../kubernation-a3-interior-stability-guidance.md)
**Baseline:** [A3-pre](a3-pre-instrument.md)

---

## The gate

> **On a province with several cities, add a workload sorting ahead of them.
> No incumbent moves.**
>
> **Passed. 0 of 3, where the baseline was 3 of 3.**

Same scenario, same instrument, same units:

| | pre-A3 | **A3** |
|---|---|---|
| add a workload sorting **ahead** of the incumbents | **3 of 3** incumbents moved | **0** |
| delete a workload sorting **ahead** | **2 of 2** moved | **0** |
| scale up · scale down · delete a later-sorting workload | 0 | 0 |
| whole scenario, every city in the realm | — | **0 of 13** |

Read per-province, per A3-pre: **100% of incumbents on the affected province
moved before; 0 now.** Every event boundary in the run reports 0 MOVED-WITHIN,
0 CARRIED, 0 FOLLOWED.

The fleet-wide rate is not the headline — it is a packing artefact that improves
by adding nodes without anything having been fixed — but for the record it went
27.3% → 0%.

---

## 1. The fix

Two lines, as §2 predicted. The row seed became name-derived, symmetric with
the column that already was:

```rust
let cell = city_cell(cx, y, rows, city_dx(&c.r.name), city_dy(&c.r, rows), &taken);
```

`city_dy` hashes the **full ref** through the `Display` impl `WorkloadRef`
already has (`"{kind} {namespace}/{name}"`) — §2 said to check what the type
provides rather than invent a format string, and it provides exactly this. Two
workloads sharing a name across namespaces therefore seed differently, which is
pinned by test.

`rows.max(1)` guards the modulo: a zero-row province must not panic, and must
not fabricate a row 0 that collides with a real cell.

---

## 2. Verification — all ten §0 claims TRUE

Seventh round running. Claim 1 — the positional row seed at world.rs:646 — was
the entire defect, and everything else followed from it.

### But claim 10 was inherited from an error of mine

§0 claim 10, §5's fixture instruction and §7's gate condition all rest on the
A3-pre report's closing caution, which said the scenario targeted the
allocatable-less node and therefore measured on a defaulted extent.

**It doesn't, and it didn't.** `lib.sh:131` puts the allocatable-less node in
the **`sys`** pool at index 0; the scenario targets the first `mem` node, which
reports `memory: 128Gi` and whose province record reads `"h": 7,
"extent_source": "Capacity"`. §7's requirement — *"a multi-city province whose
extent is measured, not defaulted"* — was already satisfied, and §5's "change
that too" had nothing to change.

The A3-pre report is corrected with a retraction rather than a quiet edit.
Caught by checking the claim instead of inheriting it, which is the only reason
§0 exists.

---

## 3. §2.1 — the residual, decided

**Decision: (a) accept.** Recorded with the measurement behind it.

Hashing removes the *index* dependency, not the *collision* dependency: two
cities can still hash to one cell, and the probe then resolves them in
`WorkloadRef` order, so a newcomer that both sorts ahead of an incumbent **and**
collides with it still displaces that one incumbent.

Measured on a crowded province (4 rows — the declared-default extent — with six
cities, 200 synthetic sets): **11.4% of cities do not get their hashed cell.**
On the common case of one or two cities per province it is 0.

So the residual is bounded to actual collisions and affects at most the colliding
city, where the defect being fixed moved **every** incumbent on **every**
insertion. Option (b) — reserving per-city slots in `Layout` with A1's
carry/reuse/ghost discipline — is not built, per §2.1's "do not build it
speculatively". The test carries a 20% ceiling and the reason to revisit if it
ever fires.

> **A near-miss worth recording.** The first version of that test generated names
> from a counter (`svc-0`, `svc-1`, …) and measured a collision rate of **0.0%**
> — which I nearly reported as the answer. FNV-1a's low bits advance by a
> constant for names differing in a trailing character, and `PRIME % CITY_COLS
> == 1`, so six sequential names take six *consecutive* columns and cannot
> collide at all. The number was a property of the name generator, not of the
> placement. Rewritten with a fixed LCG it reads 11.4%.
>
> This is the same shape as the last two rounds: an instrument that emits a
> plausible number for a reason unrelated to what it claims to measure.

---

## 4. §4 — the consumer audit

The guidance called this the phase's real risk rather than the two-line fix, and
it produced one genuine finding.

**Coast markers** moor on their city's row and take a free column in the ocean
strip, dropping when the row fills. Hashing does cluster more than round-robin
did — independent draws versus a perfect spread; measured, a 4-row province with
4 cities occupies 2.9 distinct rows on average where the index seed occupied 4.
But drops are **zero at every realistic density**: the strip holds `OCEAN_GAP`
markers per row, and only a 2-row province with 6+ exposed cities drops any —
where the index seed was already dropping them too (0.535 vs 0.450 per province
at 8 cities). Pinned by a test that gives every city a Service and asserts no
marker is lost.

**The forgiveness ring is clipped by the province boundary** — the finding. A
city on the province's edge row has a smaller clickable target, because
`region_at` locates the province by y-range first, so a ring cell past the last
row belongs to the next province's band. Extending the ring across that boundary
would let a city claim ground on a neighbouring node, which is worse than a
slightly smaller target, so the clip is correct and now asserted as an invariant.

It is pre-existing — but A3 makes it **ordinary**: under round-robin the first
city always took row 0, so edge rows only appeared on crowded provinces. The
existing test asserted the ring at `city.y + 1` and failed on the first run of
the fix, which is how it surfaced.

**Everything else reading `City.x/y` is position-agnostic**: the `CITY_MARGIN`
keep-out (whose window is the province's true span since A2, so any city row is
inside it), the painter's depth sort (`c.x + c.y`, still a valid back-to-front
order), label de-confliction (per-frame and position-driven), `city_pos`,
`--center`, blast `affected_cell`, the IMPACT rows and the Almanac locator. None
derives meaning from *which* row a city occupies.

---

## 5. §6 — the fixture

Every province carried exactly one city, so the sibling-order effect was
unreachable without a scenario constructing it — A3-pre had to build its own
conditions before it could measure anything.

`workloads.sh` now ships a co-located trio, so multi-city provinces exist by
default for the dev loop and every later scenario. Pinned by **(pool, index)**
rather than hostname: node names carry a generation token that a rolling refresh
rewrites, so a hostname pin would strand the trio as Pending the moment that
pool was refreshed. `lib.sh` gained a `churn.kubernation.io/index` label to make
that possible. The trio sits on `mem` index 001, deliberately *not* the node
scenario 7 targets, so the fixture's construction and the scenario's stay
independent.

---

## 6. Standing questions — written answers

**1. Where does a summing step precede a comparing step?**
In the collision measurement: displaced cities are summed over 200 sets and
compared to a ceiling. The denominator is cities *placed*, not sets, and the
assertion message states both — a rate over sets would have read six times
lower for the same behaviour.

**2. Does every reducer over a possibly-empty input express unknown, or
fabricate?**
`city_dy`'s `rows.max(1)` is the live case, and §8 named it: `% 0` is a panic,
and a fabricated row 0 would collide with a real cell rather than announce
itself. `rows` is `h - 1` and `h` is an extent class ≥ 3, so zero is unreachable
today — the guard is for the day it is not, and there is a test asserting
`city_dy(r, 0) == 0` rather than a crash.

**3. Where do two sections constrain the same behaviour, and is there a fixture
where they diverge?**
§2 asks for a name-derived row and §2.1 concedes collisions remain. They
diverge exactly where two cities hash alike — the case §3 measures rather than
assumes. And §5's fixture instruction diverged from reality itself (§2 above):
the fixture it told me to change was not the one in use.

**4. What existing consumers depend on the old meaning of a value this change
redefines?**
The live one this phase, and §4 exists because of it. Answered above: coast
markers (measured, no cost), the hit-test ring (clipped at the province
boundary — a real consequence, now asserted), and a list of position-agnostic
readers.

---

## 7. Acceptance

| §9 criterion | Status |
|---|---|
| Row seed name-derived; no index reaches `city_cell` | ✅ |
| `city_dy` hashes the full ref, not the bare name | ✅ via the existing `Display` |
| §2.1 decided and recorded, with the measurement behind it | ✅ (a) accept, 11.4% |
| Consumers of `City.y` audited; coast-marker drop rate checked | ✅ one finding, §4 |
| Fixture gives multi-city provinces by default | ✅ |
| Gate run and reported per-province against A3-pre's baseline | ✅ 3 of 3 → 0 |
| Standing questions answered in writing | ✅ §6 |
| `cargo nextest` green | ✅ 407 core + 90 GUI |

**Mutation floor:** reverting `city_dy` to `i % rows` fails both gate tests.

---

## 8. Decisions for the room

### A3's charter is closed; the decomposition's A3 is not

Decomposition §4 gives A3 three things: instability sources 2, 3 and 5 — city
slots, coast markers following, and *"islands stop depending on continent
height"*. This phase closed the measured mechanism behind source 2/3. **Source 5
was never measured and is untouched.**

**Ask:** is island placement worth measuring with the same instrument before
deciding whether it needs work? The dump already records ghosts and provinces;
islands would be a small addition.

### The residual has a trigger worth knowing

A city moves if a colliding newcomer sorts ahead of it — about 11% likely per
newcomer on a *crowded* province, 0 on a typical one. If that ever shows up in
practice, (b) is a follow-on with A1's engine as the template, and the test
ceiling is the tripwire.

### `city_dx` still hashes the bare name

§2 flagged it: the column seeds on `name` while the row now seeds on the full
ref, so two workloads with the same name in different namespaces share a column
and differ only by row. Not a defect — they still get distinct cells — but it is
an asymmetry, and changing it would move every city once.

**Ask:** leave it, or fold it into the next phase that already moves cities?

### The instrument keeps earning its keep

Three rounds, three instruments, and each caught something the previous method
could not see: `reshuffle.py` found the 27% the pixel diff reported as 1%;
`--dump-positions` gave 7-of-7 coverage where pixels reached 2-of-7; and this
round the collision test caught its own name generator faking a 0% result.

The pattern across all three: **the instrument's failure mode is emitting a
plausible number for the wrong reason**, and the only defence has been to break
it deliberately and check that it notices.
