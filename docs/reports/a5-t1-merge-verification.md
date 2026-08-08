# A5/T1 merge — verification pass

**Guidance:** `docs/kubernation-a5-t1-merge-verification-guidance.md`
**Version:** v1.19.0 · **Date:** 2026-08-07

**Verdict:** the removal (§2) is **complete** — no partial state. The regression
(§1) is **real but half of it was correct to drop**, and the guidance's implied
fix is refuted by measurement: restoring the third state at the *fill* would
hatch 82% of the churn fleet under every overlay. Restored where it was genuinely
lost — the panel. §3 narrowed three `pub` items; §4 and §5 verified.

451 core + 113 GUI tests; gui-smoke 55.

---

## 1. §0 — the guidance's own claims

All verified against source. All TRUE: `NewGround::{Off, Fading, Since}` with
`mark()`; `net::marking_by_node` wiring it; `mark` returning `Option<f64>` with
`Unchanged` and `Unknown` both collapsing to `None`; `ChangeSince`'s three
variants and its doc comment; and the test
`the_two_modes_diverge_only_in_when_they_let_go`, whose doc states the design
conclusion verbatim.

---

## 2. §1 — the regression, and the half of it that was right

### 2.1 What was actually lost

Traced end to end, as §1.1 asks:

| | Before (T1's `Overlay::Changed`) | After the merge |
|---|---|---|
| `Changed` | its own colour | tinted |
| `Unchanged` | its own colour | untinted, panel silent |
| `Unknown` | **hatched**, panel said "no succession on record" | untinted, panel silent |

- **The hatch does not fire.** `province_unmeasured` is gated to `Pressure` and
  `Saturation`; there is no second path.
- **The panel had gone silent.** `fresh_line` took `Option<f64>` and returned
  `None` for anything unmarked.
- **No test would have caught it**, exactly as §1.1 predicted:
  `changed_since_separates_unknown_from_unchanged` pins the core function, and
  it passed throughout. The feature was never asserted.

### 2.2 But the fill-level loss was correct, and the record already said so

CLAUDE.md's v1.13.0 entry:

> The merge also removed the need for T1's three-state honesty: a single channel
> makes one positive claim, so "no succession on record" needs no colour of its
> own — where the separate overlay painted "unchanged" and therefore had to
> distinguish it.

That argument is sound and I checked it rather than accepting it. T1's variant
was an **overlay**: it replaced the fill for every province, so *every* province
carried a positive claim, and rendering `Unknown` as `Unchanged` would have been
the map asserting "nothing happened here". The merged mark is **not** an
overlay — it is an override drawn beneath all of them, and unmarked ground shows
whatever the active overlay says. Absence stopped being a claim.

### 2.3 And restoring it at the fill is refuted by measurement

`occupied_at` is stamped only on a change of hands, so most slots never have one:

```
churn fleet:  82 / 100 live provinces have no occupied_at
kind:         has never churned — 100% would qualify
```

Under `Since`, hatching `Unknown` would therefore paint **82% of the map** with a
no-data texture — and because this mark draws beneath *every* overlay, it would
do so under Terrain, Pressure, Cost, Pool, Walls, Substrate and Saturation
alike, the moment a baseline is set. T1 tolerated this because it was confined to
one overlay, and its own report conceded it was "visually loud".

Worse, it would collide with `province_unmeasured`, which hatches to mean *this
reading has no denominator*. Two unrelated meanings on one texture,
indistinguishable on a node that has both — the same argument that decided
v1.18.0's item D against hatching extent.

So §1.2's table is right that `Option<f64>` cannot carry three states, and right
to prefer a richer type — but the richer type's job is to reach the **panel**,
not to repaint the terrain.

### 2.4 What shipped

`GroundState { New(f64), Settled, Unknown, Unasked }` in core, with
`NewGround::state()` as the full answer and **`mark()` reimplemented as a view
over it**, so the fill and the panel cannot disagree about which state a slot is
in. Each consumer then makes a *documented* choice:

- **the fill** matches `New` only, with a comment giving the two reasons above —
  it flattens deliberately, not accidentally;
- **the panel** speaks all three under `Since`: `changed since the baseline`,
  `unchanged since the baseline`, `no succession on record`.

`marking_by_node` publishes the richer value in **one** map, not two — its own
doc comment argues against two maps for one fact, and that still holds.

**The asymmetry is documented on `NewGround::state`**, per §7: under `Fading` an
absent record means "not recently new", full stop, because succession has been
stamped since the field existed and anything inside a minutes-to-hours window
would carry a time. Under `Since` the baseline can reach back past the point this
map began keeping records, so an absent record genuinely means *we do not know*.
As the guidance puts it, both modes returning an `Option` is what made the
collapse look symmetric and safe.

**Mutation floor, three, all caught:** collapse `Unknown` into `Settled` (the
regression itself); silence the panel for `Unknown`; make `mark()` disagree with
`state()`.

---

## 3. §2 — removal completeness

**Complete. No partial state.** Zero occurrences of `Overlay::Changed`,
`theme::changed_land_pair`, `panels::changed_line`, `Net.change_baseline`. The
fresh-ground suppression is gone with the variant it was conditioned on.

### 3.1 §2.1 — the stale pref and the stale flag, checked live

- **`--changed-since` was not removed; it is aliased**, and correctly — it
  produces `NewGround::Since` plus the matching menu radio, with a comment
  giving its precedence over `--fresh-minutes` ("it names a moment, where
  `--fresh-minutes` names a duration").
- **A persisted `"changed"` overlay** was seeded into a real prefs file and the
  app launched: no panic, `overlay_from_str` fell back to the default, and
  **no unrelated setting was disturbed** — `map_style`, `graticule` and
  `fresh_minutes` all survived intact. On a normal exit the file is rewritten
  with the live overlay's label, so it self-heals; it persisted through *this*
  check only because `--screenshot` deliberately skips the exit save.

This was the item flagged as having "a user-visible failure mode that no test
will catch". It is benign, and now checked rather than assumed.

---

## 4. §3 — dead-`pub` audit

| | Callers outside `layout.rs` | Action |
|---|---|---|
| `changed_since` | none (`args.changed_since` is a field name, not this) | **private** |
| `ChangeSince` | none | **private** |
| `freshness` | `layout_store.rs` tests only | **`pub(crate)`** |
| `is_off` | `net.rs` | kept `pub` |

The honesty `ChangeSince`'s doc comment carried is not lost by making it
internal — `GroundState::Unknown` now carries it in the public type, which is
the one a consumer actually receives.

---

## 5. §4 and §5 — control surface and panel

- **Mode and parameter switch at runtime** via View ▸ NEW GROUND (nine choices
  under two mode labels), through `net.set_new_ground`.
- **The `Since` baseline is captured at click time and not persisted** — the
  comment at the menu handler says why: "storing the duration would make it the
  fading mode wearing the other mode's name."
- **The mode does not persist; the fading window does.** Reopening therefore
  lands in `Fading` (or `Off` when the saved window is 0) — a defined behaviour,
  and the one §4 suggests.
- **`Off` is a real value**, reachable from the menu and from
  `--fresh-minutes 0`.
- **The panel wording is true in both modes**: `Since` never claims an age (a
  fixed baseline does not decay, and a test asserts no `Since` wording contains
  "just" or "recently"), and `Fading` keeps its relative tiers.

---

## 6. §6 — standing questions

**1. Summing before comparing?** Not present.

**2. Unknown, or fabricated?** The whole of §1, and the guidance's framing is the
sharpest yet: *a type that can say "unknown" does not help if every consumer
flattens it before the answer is used.* The fix is not to stop flattening — the
fill legitimately wants one bit — but to make the flattening a **documented view
over the full answer** rather than the only answer available. `state()` is that
answer; `mark()` is the view; both consumers now choose knowingly.

**3. Two sections constraining one behaviour, and a fixture where they diverge?**
§1 (restore the third state) and the unmeasured hatch (already owns that texture)
constrain the same pixels, and the churn fleet is the fixture: 82 of 100
provinces would carry both meanings at once. Resolved by splitting the channel —
texture keeps its one meaning, words carry the other.

**4. Consumers depending on an old meaning?** `Overlay`'s persisted string, §3.1.
Checked live rather than reasoned about.

**5. Inherited claims?** The guidance's §0 claims are all verified. The one claim
I did *not* inherit is the important one: v1.13.0's recorded rationale for
dropping the third state, which I re-derived (§2.2) and then bounded — it is
right about the fill and silent about the panel, which is exactly the half that
was wrong.

**6. One side of a comparison moved?** This is what the merge did: the same
field, `occupied_at`, read against a rolling window in one mode and a fixed
instant in the other. `changed_since` and `freshness` still answer different
questions, `freshness_and_changed_since_answer_different_questions` still pins
it, and `state()` now routes both without unifying them.

**7. Container adjacency read as world adjacency?** Not present — this change
touches no ordered collection. `marking_by_node` is keyed by name and read by
lookup, never by position.

---

## 7. §7 — acceptance

- [x] §1 verified; the lost half restored through a richer return type
- [x] The `Fading`/`Since` asymmetry documented, on `NewGround::state`
- [x] `Overlay::Changed` and its machinery fully removed — verified zero, no partial state
- [x] Fresh-ground suppression gone
- [x] Stale-pref and stale-flag behaviour confirmed live
- [x] Dead `pub` helpers made private (3) or justified (1)
- [x] Mode and parameter switchable at runtime; baseline not persisted; restore behaviour defined
- [x] Panel wording true in both modes and speaking the unknown state
- [x] Standing questions answered
- [x] `cargo test` green — 451 core + 113 GUI; gui-smoke 55

**Deviation from §1.2, stated:** the guidance prefers restoring the third state
through the draw path. Only the panel half was restored; the fill half is
recorded as correctly dropped, with the measurement behind it (§2.3). Doing both
would have been the more literal reading and the worse map.
