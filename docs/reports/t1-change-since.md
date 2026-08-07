# T1 — change-since, and the gate's answer

**Phase:** T1, from `kubernation-t1-change-since-guidance.md` — **the kill point**
**Version:** v1.12.0, superseded by **v1.13.0** (the merge) · **Date:** 2026-08-06
**Gate verdict: MIXED against the Annals, and NEGATIVE against the map's own
existing feature.** §4.1's discrimination check fails. Recommendation in §5.

443 core + 106 GUI tests; gui-smoke 54; dev fleet unchanged.

---

## 1. §0 — all eleven claims verified

Every claim was tagged `[A]`, so all were checked against source. All TRUE.

**§1's premise is false: T0 *has* been reported** —
`docs/reports/t0-history-substrate.md`, commit `62101a7`. The phase's blocking
condition was already satisfied, and its answer shaped the design:

> The layout persists `occupied_at` per slot — the only persisted per-entity
> change timestamp in the product.

So the occupant axis sits in §1's **middle** row, not its weakest: the baseline
can precede app launch, and survives restart. That is why this axis was chosen.

---

## 2. What shipped

| Piece | Where |
|---|---|
| `ChangeSince` + `changed_since(occupied_at, baseline)` | `state/layout.rs` (pure) |
| `Net.change_baseline` — a fixed **instant** | `net.rs` |
| `changed_by_node` per tick | `net.rs` |
| `Overlay::Changed`, hatch for Unknown, minimap fallback | `draw.rs` |
| `theme::changed_land_pair` (violet; teal under `cb_*`) | `theme.rs` |
| `panels::changed_line` — the panel half | `panels.rs` |
| **View ▸ CHANGED SINCE** radio · `--changed-since` | `menu.rs`, `main.rs` |

**The baseline is an instant, not a duration.** A stored duration would slide
with the clock and make this A5's rolling window under another name. Captured
once at click time; the marked set then only grows until the operator moves it.
Deliberately **not persisted** — a baseline is a moment in *this* investigation,
and restoring a stale one would silently answer a question about a different
afternoon. The overlay *choice* persists, like every other.

**§3.2's collision was real and structural.** `overlay_pair` returns fresh ground
*before* the overlay match, so a new overlay about the same fact would have been
overwritten on exactly the provinces it exists to describe. Resolved by
suppressing fresh ground under `Overlay::Changed` only — the one place the two
would contradict each other.

**Three states, and the third is not a shrug.** Absent `occupied_at` means either
never-changed *or* record-predates-the-field, and neither is "unchanged". It
renders as the hatch — the established no-data texture from v1.6.0 — and the
panel says "no succession on record", not silence.

---

## 3. The gate

### 3.1 It was run on the strongest available case

The configuration arose naturally: 18 successions from a 30-node rolling refresh,
**100% concentrated in the `sys` pool** (0 in the other three), spread 6/6/6
across three zones with the fourth untouched.

The map showed it as a shape — one contiguous run per column, and one column
clean:

```
z-a  ............XXXXXX...................
z-b  ............XXXXXX....
z-c  ................XXXXXX....
z-d  ...............
```

**But the contiguity is incidental, not structural.** It holds because the `sys`
pool was allocated as a batch so its ordinals are consecutive. A2 deliberately
gave up pool contiguity when ordinals went zone-wide, and A6 recorded
`region ← pool ∩ zone` as still unclaimed. On a fleet that has churned enough to
interleave pools, the same change would scatter. **The map showed a shape here
because the fixture preserved one, not because the map guarantees one.**

### 3.2 Against the Annals — mixed

The list, on the same event, reports `RemovingNode` for `churn-sys-g1-020` …
`-029` and so on.

**What the list does better than expected:** the node names carry the pool. A
reader scanning `churn-sys-g1-0NN` repeated ten times learns *"the sys pool was
replaced"* directly — which is the operationally meaningful grouping, and the one
the map **cannot express**, because its geography is zone-organised.

**What the map does better:** z-d being untouched is an immediate visual fact,
where in the list it is an *absence* — and absences are hard to notice. The map
is also immune to the list's 80-entry cap and to pod-churn noise; there were 970
events on the fleet, and the node lines compete with hundreds of `Scheduled`
lines.

Honest summary: the map wins on *what did not change* and on noise immunity; the
list wins on *naming the thing that changed*. Neither dominates.

### 3.3 Against the map's own existing feature — negative

§4.1's discrimination check, run as specified:

| | marked pixels |
|---|---|
| T1 overlay **on** — violet | 288,027 |
| T1 overlay **off** — A5's fresh ground | 292,811 |

The same provinces, to within 2%. **With T1 off, the identical conclusion is
reachable from the map, via a feature that shipped in v1.10.0.**

That is §4.1's stated failure condition verbatim: *"If the same conclusion is
reachable from the map without it — from fresh ground, from the terrain, from
familiarity with the fleet — then the gate is measuring something else."*

A control confirms T1 is not merely fresh ground bleeding through: with fresh
ground disabled entirely, T1 still marks 288,027 px. It renders its own answer.
The answer simply coincides.

**Where T1 genuinely differs** (pinned by
`freshness_and_changed_since_answer_different_questions`): fresh ground decays,
change-since does not. So T1's real contribution is **reach**, not a different
fact — and T0 §2.2 already flagged that shape as a trap in another guise. Worse,
reach is *already a setting on fresh ground*: the ageing window goes to 4 hours.
Distinguishing the two live requires setting one short and the other long, which
is a configuration difference, not a conceptual one.

### 3.4 §4.2, decided in advance

I decided before running that this would be an **information-content**
comparison — what each artefact does and does not convey — and that I would
**not** claim a usability verdict about learning speed, which I cannot assess
alone. §3.2 and §3.3 are stated on that basis. If planning wants the usability
question answered, it needs a second person, as A6's gate did.

### 3.5 Mutation floor

Forcing the change test to always answer "unchanged" fails two core tests, and
the gui-smoke state's marking disappears.

---

## 4. §6 — standing questions

**1. Summing before comparing?** No aggregation here; `changed_since` is a
three-way comparison on one field.

**2. Unknown or fabricated?** This is the phase's central honesty point, and §6
called it correctly. Absent `occupied_at` is `Unknown`, hatched, and *spoken* in
SELECTION. On a quiet fleet most of the map is therefore hatched — visually loud,
and true: there is no succession record for that ground. A future
"recording since" timestamp on the layout would let most of those resolve to
`Unchanged`; without one, the honest answer is that we do not know.

**3. Two sections constraining one behaviour?** §3.2 (do not collide with fresh
ground) and §3's "reuse the existing funnel" pull opposite ways: reuse would have
let fresh ground win the fill. Resolved by scoping the suppression to the one
overlay, so every other view keeps A5's mark.

**4. Old meanings?** Nothing redefined. `Overlay` gains a ninth variant;
`province_unmeasured` gains a `data` parameter so the hatch can consult the
change map.

**5. Inherited claims?** All eleven verified; §1's process claim about T0 was the
only falsity, and it was a release rather than a block.

**6. One side of a comparison moved?** Directly applicable, and answered
deliberately: `freshness` compares `occupied_at` against **`now`**;
`changed_since` compares it against a **fixed baseline**. Same field, different
right-hand sides, on purpose — and the divergence is the unit test in §3.3.
T-fix-2's rule holds: not "make both sides match", but know which quantity each
side is and say so.

---

## 5. The verdict, and what I recommend

Per §7, stated plainly rather than softened.

**T1 does not earn its place as a separate overlay.** Not because the
implementation is weak — it works, it is tested, and its three-state honesty is
better than fresh ground's boolean — but because the map already answers this
question, and the only axis on which T1 improves is a duration that fresh ground
also exposes as a setting.

**The workstream thesis is not refuted.** The map *does* show something the list
does not: what did **not** change, and a picture immune to the list's cap. That
is a real advantage and it is the one the planning doc predicted. What is refuted
is the assumption that a *new overlay* was the way to get it.

**Recommendation, for planning to decide:**

1. **Merge, don't multiply.** Give A5's fresh ground a *fixable* baseline
   alongside its rolling window — one feature, two modes — rather than two
   overlays about one fact. T1's `changed_since`, the three-state honesty, and
   the panel line all transplant directly.
2. Before T2 or T3, settle `region ← pool ∩ zone`. §3.1 is the evidence: the
   change was pool-shaped, the map is zone-shaped, and it showed a shape only by
   luck of allocation order. Every later phase that claims change "clusters
   spatially" inherits that gap.
3. If T2 proceeds regardless, note that its claim is about *failures* clustering,
   not node replacements — a different axis, and one where the list has no node
   names to lean on.

**What I did not do:** widen the axis or blame the implementation, both of which
§7 names as the instinct to resist.

The code is committed and off by default, so the decision is reversible in either
direction — keeping it costs nothing, and merging it into A5 is a smaller change
from here than from scratch.

---

## 6. Outcome: merged (v1.13.0)

Recommendation 1 was taken. A5's fresh ground gained a fixable baseline, and the
separate overlay was removed.

`NewGround { Off, Fading(Duration), Since(SystemTime) }` has one `mark()` entry
point composing the two predicates that already existed. One net slot, one
per-tick map, one colour channel, one menu radio, one panel line. Deleted:
`Overlay::Changed`, `changed_land_pair`, `OverlayData.changed`,
`WorldSnap.changed`, `changed_line`. The phase is net removal.

Two things the merge bought that the separate feature could not:

**The three-state honesty became unnecessary.** T1 painted "unchanged" as its own
colour, which forced a third state to keep "no succession on record" from reading
as a positive all-clear. A single channel makes one positive claim, so unmarked
ground needs no explanation — it means the map is not claiming anything. Pinned by
`unknown_ground_is_never_marked_in_either_mode`.

**The divergence is now visible in one setting.** Verified live on identical
ground: *fading 5 min* marks 0 px (it let go — the successions are 23 h old),
*since 24 h ago* marks 287,806 px (it holds). Opposite answers on the same data,
which is exactly what justifies keeping the second mode and nothing more.

A `Since` baseline is captured as an **instant** at click time and is not
persisted — a stored duration would slide with the clock and silently become the
fading mode, and a restored baseline would answer a question about a different
afternoon.
