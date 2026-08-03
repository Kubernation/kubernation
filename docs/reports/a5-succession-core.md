# A5 — Succession (core), and cataclysm resolved

**Report** · 2026-08-03 · **v1.9.1**
**Governing doc:** [`kubernation-a5-succession-cataclysm-guidance.md`](../kubernation-a5-succession-cataclysm-guidance.md)
**Status: half the phase.** The core landed; §2.3's rendering and §4's gate did
not, deliberately — see [§6](#6-what-is-not-done-and-why).

---

## The headline

Two results changed the phase's shape before any code was written, and one of
them shrank it.

> **Cataclysm is a record, not a rendering** — because after a structural change
> there is nothing left to draw on.
>
> **§3.2's premise is wrong**: A4 does not record these events. But its
> *instruction* survives, because a structured record the Annals already renders
> does exist.

And the succession half turned out to hinge on a detail the obvious
implementation gets wrong: **a rolling refresh is never a replacement in any
single tick.**

---

## 1. Verification — 9 of 11 hold, 1 false, 1 refined

### Claim 5 is FALSE

> *"`attention::build` and `build_timeline` take `now` as a parameter — the
> established clock convention."*

`build_timeline` does. **`attention::build(world, map, workloads, filter)` does
not** — it reads `jiff::Timestamp::now()` internally at `attention.rs:597`. It is
a *counter-example* to the convention, not an example of it.

Not design-changing: the convention is real (`build_timeline`, `postmortem`,
`chaos`, and A4's own `stamp_vacancies`), so §2.1's instruction to pass `now`
stands on its other citation. Recorded rather than quietly corrected, because
the next document to cite "the established convention" should cite something
that is one.

### Claim 9 is refined, not false

Measured live across a g2→g3 refresh on the churn fleet, ghost counts ran:

```
0 … 10 → 1 → 11 → 2 → 12 → (settles at 12)
```

Each wave's surge reclaims the previous wave's vacancies. The **not-cumulative**
half of the claim is solid. But it settles at **12, not the batch size of 10**,
because reclaim is per-(zone, pool) and the `sys` pool spans three zones — a
vacancy in one zone cannot be reclaimed by a newcomer in another.

My own A4 measurement used a single partition, which is exactly why it read a
clean 10. **The real fleet's steady state is *on the order of* the batch,
partitioned.**

### Claim 8's near-miss

The first `--dump-positions` file I checked contained **zero** ghost records —
which reads as a refutation of "the dump records ghost slots". It was not: that
dump simply had no ghosts, because the refresh had completed and every vacancy
had been reclaimed. Driving a live refresh shows them in **138 of 166 ticks**.

Checking a claim against a stored artifact is not the same as checking it
against the case at hand, and the difference nearly produced a false negative.

Claims 1–4, 6, 7, 10 and 11 all hold as stated.

---

## 2. §3.2 and §3.3 — the cataclysm question, resolved

### There is no record to read

> §3.2: *"A4 established that compaction and fingerprint-mismatch already record
> events. A5 should read those rather than build a second event path."*

They do not. Both call `set_layout_note(Some(String))` — a **transient toast**
that `take_layout_note()` clears on first read. Nothing persists, nothing
accumulates, nothing is queryable.

But the instruction survives its premise. A structured, bounded, already-rendered
record does exist: `OperatorAction` / `net.operator_actions()`, the in-session
ring the **Annals** shows. So compaction now pushes an `OpVerb::Compact` action
and reaches the operator through the path that exists — which is what §3.2 was
protecting against a second one for.

### There is nothing left to draw on

§3.3 asked whether cataclysm is a rendering at all, and told me to say so if not.
It is not:

| Structural change | What remains on the map |
|---|---|
| a pool vanishes | its slots become ghosts — **already rendered** as ghost ground |
| a compaction | the ground it describes **stops existing** |
| a zone vanishes | a reserved ordinal with **no provinces on it** |

A scar has nowhere to sit. **Cataclysm is a record.** That is the honest outcome
§3.3 predicted would "substantially shrink §3", and it did.

---

## 3. Succession: the detail that decides the design

A slot now records when it **changed hands**, and three cases have to be
separated or the marking is worthless:

| | | |
|---|---|---|
| **first sighting** | a slot that never had an occupant | not marked — or a first run paints the entire map |
| **carry / return** | the same node, including one back from a blip | not marked — nothing changed hands |
| **succession** | different node takes ground its predecessor held | **marked** |

### Why `changes_from` cannot detect it

§2.1 suggests `changes_from`, and notes `from: Some` + `to: Some` is the
replacement case. On the real sequence that case **never occurs**:

```
tick N     node drains          from Some → to None    a departure
tick N+1   replacement reclaims from None → to Some    an arrival
```

A rolling refresh drains in one tick and reclaims in another, so no single tick
ever sees a replacement. Detection therefore keys on `last_occupant`, which spans
the gap — which is precisely what A1 added it for, and the reason §2.1's own
preference for a stamp over a transient detector is right for a second reason it
does not mention.

### The predicate existed twice, and disagreed immediately

The placer cleared the stamp on a change of hands; the stamper stamped anything
unstamped. Those are not the same rule, and two tests caught it on the first run:
a carry and a node returning to its own ground were both marked as replacements.

Now one `changed_hands` predicate with two callers — the same DRY discipline
`resolve_region` and `affected_cell` were promoted under, applied before the
drift rather than after it.

---

## 4. The format decision

`occupied_at` joins `StoredSlot`, and **`LAYOUT_VERSION` is deliberately not
bumped.**

An optional field with `#[serde(default)]` reads an older file as `None`, and a
newer file read by an older build is ignored rather than rejected — `StoredSlot`
does not deny unknown fields. Compatible in both directions, so per `prefs.rs`'s
convention of bumping only on an *incompatible* change, this is not one.

The case that decides it is the upgrade: a file written before the field existed
must load with **nothing marked**, or the first launch after an upgrade paints
the whole world as freshly changed. There is a test that strips the key from the
JSON entirely and asserts exactly that.

---

## 5. Tests and the mutation floor

**425 core + 94 GUI** (was 419 + 94). §6's mutation floor was **exercised, not
merely written**, in both directions:

| Mutation | Fails |
|---|---|
| ageing always returns "not fresh" | 2 tests |
| succession never detected | 3 tests |

`freshness` returns `None` for every "do not mark" state — never changed hands,
timestamp unknown, or window zero — and those are three genuinely different
things with the same honest answer. Zero is checked *before* the division rather
than producing an infinity that would mark everything forever, and a clock that
stepped backwards loses precision rather than losing the fact that ground changed.

---

## 6. What is not done, and why

**§2.3's rendering and §4's gate.** The guidance calls the fresh-ground treatment
*"the phase's main aesthetic decision"* and says it *"should be made against the
live map, not in advance"* — specifically whether ageing should be **quantised
into two or three steps** rather than a continuous fade, since a continuous fade
is hard to read as a wave.

That is a judgment that needs the churn fleet in front of it, and it is the half
of the phase where the gate lives. Splitting here leaves a clean seam: everything
a renderer needs is

```rust
freshness(layout.occupied_at(slot), now, window)
```

Also outstanding with it: the ageing window as a persisted, flag-overridable
setting (§2.2), and the `cb_*` funnel routing plus the ghost-versus-fresh
distinguishability requirement (§2.3, §4.1).

### Acceptance, honestly partial

| §8 criterion | |
|---|---|
| `occupied_at` stamped on new occupancy only, persisted, cleared appropriately | ✅ |
| Format version decision made and justified | ✅ not bumped, §4 |
| Cataclysm reads A4's existing record, not a parallel one | ✅ — after finding A4 has none, §2 |
| §3.3 decided: rendering or only a record | ✅ **record** |
| Standing questions answered | ✅ §7 |
| `cargo nextest` green | ✅ |
| Ageing window is a setting with a declared default; `0` means never | ⏳ `0` works; the setting is not wired |
| Fresh ground routes through `cb_*` and reads in every map style | ⏳ |
| Fresh ground distinguishable from ghost ground during a surge | ⏳ |
| Gate run with its discrimination check | ⏳ |

---

## 7. Standing questions

**1. Summing before comparing.** The ghost-count measurement sums slots per tick
and compares across ticks; the trap avoided was comparing a *global* batch size
against a *per-partition* steady state, which is exactly the discrepancy §1
records — 12 against an expected 10.

**2. Reducers over possibly-empty input.** `freshness` is the live one and
returns `None` in three distinct empty cases rather than picking an extreme.
Absent `occupied_at` is **unknown**: marking it fresh paints the map on upgrade,
treating it as infinitely old is a different fabrication, and neither is what an
absent value means.

**3. Two sections constraining the same behaviour.** §2.1 says use
`changes_from`; §2.1 also says prefer a stamp because a transient detector misses
changes made while closed. They diverge on the drain-then-reclaim sequence — and
there the stronger reason is that `changes_from` cannot see it *at all*, not
merely that it would miss it when closed (§3).

**4. Consumers of a redefined value.** `SlotState` gained a field, so every
literal construction had to be revisited; the compiler found them. The real
consumer question is `StoredSlot` — A4 froze a format one day ago and this is the
first change to it — answered in §4.

**5. Inherited claims.** Six were tagged (2, 6, 7, 9, 10, 11). All verified
against the case at hand; claim 9 came back **refined**, and claim 8 — not tagged
as inherited but sourced from a prior report — came back needing a live check
rather than a stored artifact.

---

## 8. Decisions for the room

### The next session is small and well-bounded

Rendering plus the gate. The seam is one function call, and the decision to make
against the live map is quantised-versus-continuous.

### Claim 9's refinement changes a number the guidance uses

§1 says fresh ground is "a recurring, self-clearing state, not an accumulating
one", sized by claim 9. That still holds — but the standing quantity is
*per-partition*, so on a fleet with many (zone, pool) partitions there is more
fresh and ghost ground on screen simultaneously than a single-partition estimate
suggests. Worth knowing before the ageing window's default is chosen, since the
window and the standing quantity together decide how much of the map is marked.

### The `attention::build` clock is a genuine inconsistency

Not A5's to fix, but worth recording: it is the only clock-reading function in
`state/` that does not take `now`, and `state/`'s own module doc calls that out
as the deliberate windowed-recency exception. So it is documented, not accidental
— but it means "the established convention" has one standing exception, and
documents citing the convention should not cite that one.

**Ask:** leave it, or make it symmetric while A6 is still ahead of us?
