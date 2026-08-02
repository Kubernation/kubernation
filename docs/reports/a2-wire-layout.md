# A2 — Wire the layout in

**Implementation report** · 2026-08-02 · unversioned (no user-visible surface yet)
**Commit:** `74d969d`
**Governing docs:** [`kubernation-a2-wire-layout-guidance.md`](../kubernation-a2-wire-layout-guidance.md) · [`kubernation-workstream-a-decomposition.md`](../kubernation-workstream-a-decomposition.md) §4
**Gate evidence:** [`a2-gate/`](a2-gate/) — 6 frames before, 6 after

---

## The gate

> **Does the map hold still?**
>
> **Yes.**

Same rolling refresh both times: all 30 `sys` nodes replaced on a 100-node fleet,
surge ordering, identical scenario, identical flags, identical camera anchor.

| | Map-pixel change across the refresh |
|---|---|
| Before A2 | **1.3%** — cities move, coastlines change (`ingest` vanishes, `store` shifts) |
| After A2 | **0.0%** — frames 00 and 05 are pixel-identical |

The only thing that changes on screen after A2 is the attention line, which
correctly renames itself from `churn-sys-g1-000` to `churn-sys-g2-000` as the
allocatable-less node is itself replaced.

[`a2-gate/before/refresh-00.png`](a2-gate/before/refresh-00.png) →
[`…-05.png`](a2-gate/before/refresh-05.png) against
[`a2-gate/after/refresh-00.png`](a2-gate/after/refresh-00.png) →
[`…-05.png`](a2-gate/after/refresh-05.png).

Per §8: the map now demonstrably holds still, so plan §1's spatial-memory claim
is testable for the first time. **Whether it is more useful is a different
question, and this result deliberately does not answer it.**

---

## The gate lied three times before it told the truth

This is the most transferable finding of the round. Three flipbooks looked like
answers and none was one.

**1 — The camera moved, not the map.** `capture.sh` defaulted to anchoring on
`churn-sys-g1-000`: a node *in the pool scenario 1 refreshes*. When it drained,
the anchor vanished and capture silently fell back to a fit-the-world view. The
"movement" in the flipbook was the camera reframing. §6 warns about exactly this
— "apparent movement is just the camera moving" — and the harness I built in
A-pre had the trap wired in as its default.

**2 and 3 — Two runs on a fleet with no pods.** `reset.sh` deleted `ns/churn`,
which under kwok takes minutes to terminate with ~400 pods. Re-applying into a
namespace still `Terminating` silently applies *nothing*, producing a 100-node
zero-pod fleet — which renders as a perfectly plausible map. I compounded it by
running `reset.sh >/dev/null 2>&1`, silencing the evidence.

Both fixed:

- The capture anchor sits outside the pool under test, and the fallback is
  **loud** — a reframed capture announces that it is not comparable.
- `reset.sh` deletes the workload objects rather than the namespace, and
  `up.sh` **verifies** pods are running instead of assuming, failing loudly if
  not.

> A gate whose instrument fails silently is worse than no gate: it produces a
> confident answer to a question it never asked.

---

## §2 under-specified the fix for a source A2 is chartered to close

§2 says *"verify how `build_map` orders zones; if it is not sorted by name, sort
it."* They were already sorted — and sorting only addresses the **reorders**
third of instability source 4, which is *"a zone appears, vanishes, or
reorders."*

Verified before touching anything: adding a zone that sorts first moved `z-b`
from x=0 to x=30 and `z-c` from 30 to 60. **Every continent shifts.**

So zones now carry **durable ordinals** with the same carry / append / reserve
discipline as slots: a new zone appends into fresh ocean, a departed one keeps
its ground reserved and reclaims it if it returns. Both halves are test-pinned.

The threading sweep §10 feared went the other way. §1's own suggested wrapper —
keep `Models::build` passing a fresh layout — absorbed 32 of 34 call sites. Only
`net.rs` changed, and it is where the mechanism actually lives: last tick's
layout fed back as `prior`, per world, dropped on a context switch.

---

## What changed

`build_world` computes no positions of its own.

- **Province y** from the slot ordinal × the largest extent class, so a slot's
  ground never depends on its neighbours' size and a ghost leaves its ground
  empty rather than letting the provinces below slide up.
- **Continent x** from the durable zone ordinal.
- **Extent from capacity**, quantised into four size classes — continuous sizing
  means a node-type refresh nudges every province, while classes are stable
  across small variation. Memory, because it is incompressible.
- **The fallback chain is declared**: capacity → instance type → default, with
  `ExtentSource` travelling on the province. The default is deliberately *not*
  the smallest class, so an unmeasurable node cannot read as a genuinely tiny
  one — the v1.6.0 discipline.
- **Ghosts render** as empty terrain, per §4. Nothing decorative; A5 owns that
  vocabulary.

Cities keep their A2 row placement, now clamped into the province since `h` no
longer grows to fit them. Real city slots are A3's job.

---

## Tests

393 core + 87 GUI. §5's list, plus the source-4 pair.

**Mutation floor** — each fails a test:

| Mutation | Restores |
|---|---|
| Revert carry, every node appends fresh | the reshuffle A1 removed |
| Compact province y by enumerating live provinces | instability source 1's symptom |
| Restore `h = (2 + 2*cities.len()).max(3)` | instability source 1 |
| Restore `cx = zone_index * stride` | instability source 4 |

---

## Decisions for the room

### A2 passed. What does that license?

The gate is answered and the thesis is now testable. §8's caution applies from
here on: if the map holds still and still isn't more useful, the failure is
§1's, not the implementation's.

**Ask:** is the next step A3 (interior stability — cities are what people
actually hunt for, and §3.1a argues that matters more than the province fix), or
a pause to test §1's claim now that it finally can be?

### Instance-type as a pool fallback — still open from A1

Unchanged and still unanswered: a node whose instance type changes vacates its
slot, because its pool is then a hardware attribute. One-line change either way.

### Review agents write into the working tree

Third round raising it. This round it was a `gen` identifier (reserved in
edition 2024) breaking the build mid-session.

**Ask:** constrain reviewers to a scratch directory or a worktree?
