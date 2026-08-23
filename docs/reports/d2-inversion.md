# D2 §3.3 — the inversion

**Guidance:** `docs/kubernation-d2-inversion-guidance.md`
**Follows:** `docs/reports/d2-gate.md` (the gate that stopped the phase) and
`docs/reports/d2-fix-testable-decisions.md` (the half-day that made it safe)
**Version:** 1.23.0 · **Date:** 2026-08-19

The map selection is an identity. Its position is derived from the current
world every frame.

**Both gates pass live, with discrimination** (§8) — and the round's finding is
that **§0's third dissolution was not free**, while a defect nobody was looking
for was: `WorldModel::province_pos` returns a cell that resolves to **open
water** on every province measured, which has been putting the selection marker
into the sea since the coast carving shipped.

---

## 1. §1 — claims verified

All nine TRUE against source. Claim 5's inventory was re-run rather than
inherited (§2), as §1 instructed, and was accurate.

**§3's prose is not.** It says *"the map click is the only writer whose natural
input is a position."* The **almanac's cross-reference** is also positional —
`AlmanacAction::Locate(cell)`, derived from a `Locator` — and worse, it can
point at a harbour, a gate or an island structure, none of which the conversion
can name at all. That changed the design: see §5.

---

## 2. §3 — the site inventory, re-enumerated

Recorded rather than inherited. **12 writers, 10 readers** — more than §3's
expected shape.

| writer | already knew an identity? |
|---|---|
| `focus_concern` (`N`, sidebar, IMPACT) | **yes** — `concern.target` |
| `]` / `[` sail | **yes** — `c.r` |
| **map click** | **no** — the genuinely positional one |
| `--inspect` city / node | yes |
| `--evict`, `--forward` | yes |
| `--blast` node / city | yes |
| **almanac Locate** | **no** — §1 |
| context switch ×2 | clears |

Ten of twelve were already holding the identity and throwing it away to store a
cell. `focus_concern` is the sharpest case: it took the concern's `Target`,
converted it to a cell **through `province_pos`**, and stored that — which is
§4's defect, on the app's most-used verb.

**The compiler did the enumeration.** Changing the type surfaced every site;
none had to be found by reading.

---

## 3. §0's third dissolution was NOT free

§0 claims the inversion dissolves the carved-sea divergence "for free", because
"an identity is a node or it is not."

**It does not.** The ambiguity was never in what the selection *stored* — it was
in the *conversion*. `subject_at` called `region_at`, which tests a province's
**rectangle**; the tooltip calls `resolve_region`, which applies the shoreline
carving. Inverting the storage leaves both exactly as they were.

The test written for §4's third bullet failed on the first run and said so.

So this is a **deliberate change**, not a consequence: `subject_at` now resolves
through `resolve_region`, sharing the one land test. `city_at` follows it — a
city sits on land by construction so the answer cannot differ, which is exactly
why uniformity is cheap there and why keeping a second land test on the strength
of "it provably cannot matter" is the trade that goes wrong later.

The D2-fix agreement test's carved-divergence arm is now **deleted**, and its
absence is enforced by that test's catch-all: if the two ever disagree that way
again, it panics.

---

## 4. THE FINDING: the derivation pointed into the sea

`WorldModel::province_pos` returns `(p.x + 2, p.y)`. Measured on the probe
fixture, **every province's `province_pos` cell resolves to `Resolved::Ocean`**,
and nudging the row does not help — the west inset simply exceeds two cells.

It cannot be fixed in core: `Coast` is procedural noise generated in the VIEW,
and the v1.3.0 decision deliberately keeps it out of the world model.

**Three sites had hand-rolled `+2` variants of the same idea** — `province_pos`
itself, the `--inspect` node arm (`+2/+1`, so someone already knew the top row
was wrong and fixed it *there*), and `almanac::locate`. Standing question 3, with
three sections constraining one behaviour and no fixture where they were ever
compared.

**This was live, not theoretical.** `focus_concern` stored that cell, so pressing
**`N`** onto a node concern — the app's spine, "park the cursor on what needs
orders" — selected open water: the marker pulsed on the sea and the SELECTION
box, which resolves through `region_lines`, came back **empty**. `draw_blast`
put its crisis ring there too.

`draw::province_land_cell` is now the one authority, running the **same
`land_span` test `resolve_region` applies**, from the province's middle row —
which is what makes a derived position resolve back to the province it came
from. All four consumers route through it.

Confirmed on the live kind cluster, not only on the fixture. Selecting node
`kubernation-worker2`, whose province occupies `x=60..86, y=1..4`:

```
province_pos would give   (62, 1)     two cells inside the west edge -- open water
the land test gives       (72, 2)     ten cells further east, one row south
```

---

## 5. §5 — decided: TOMBSTONE

A selection whose subject has left the cluster keeps the box, says what
happened, and says how to dismiss it:

```
workload gate/wanderer
departed - nothing left to mark
click elsewhere to dismiss
```

Nothing is drawn on the map, because there is nowhere to draw. The two silent
options — vanishing, or marking a position that is no longer its own — are wrong
in the same way, and this codebase has refused that shape repeatedly
(`SubstrateReport` falling back to terrain, `GroundState::Unknown` reaching the
panel, `extent_line` speaking a guessed size). It is a pure `departed_lines`,
unit-tested.

A departed selection outranks the hover fallback, exactly as a live one does; a
rule that changed with liveness would be harder to predict than a box that
occasionally holds a tombstone.

In a paired session it leads with `HOT` / `WARM`, as the live box does — without
it a departed workload never says which side lost it, which is most of what an
operator wants to know.

### 5.1 What the identity cannot hold, and what that costs

A selection is a workload or a node. A **coast marker** and an **island
structure** have places on the map and are not selectable, so:

- clicking a harbour opens the city it serves (unchanged) but no longer stores
  the sea cell underneath it;
- an almanac cross-reference to a harbour, gate or structure now **flies without
  marking**.

Both follow from routing the click through `subject_at` as §3 specifies, rather
than through `panel_for` — which D2-fix recorded as deliberately richer, and
which would give the Oracle and the blast radius a resolution they have never
had. Reported as a consequence, not smuggled in as an improvement.

---

## 6. §4 — the mutation floor, and one that survived

| | mutation | first run | after |
|---|---|---|---|
| M-A | the derivation **caches** its result (§2.2's hazard) | caught | caught |
| M-B | the click bypasses the shared land test | caught | caught |
| M-C | a departed subject handled silently | caught | caught |
| M-D | a node's position from `province_pos`, not the land test | **SURVIVED** | caught |

**M-D is the round's process finding.** The round-trip test pinned
`province_land_cell` — the *authority* — and nothing said `selection_pos` *used*
it. That is precisely D2-fix's own lesson recurring one level down, and it was
invisible until the mutation ran. The assertion now goes through the consumer.

Each mutation was asserted applied — present **and compiling** — per §4.1.

---

## 7. §8 — the gate, live

### 7.1 On throwaway clusters, and a misdiagnosis on the way there

`hack/d2-selection-gate.sh` stands up two throwaway kwok clusters, measures, and
tears them down. Committed, because a measurement that cannot be re-run is a
claim rather than evidence.

**Two clusters is the requirement, not a convenience.** Gate B needs a *paired*
session in which only the HOT world grows, so the warm offset moves and the warm
world does not — which a single fleet cannot express, whatever its size. Gate A
needs a reschedule across zones, which is a three-line fixture. Neither wants a
hundred nodes.

**And I first gave the wrong reason for not using the churn fleet.** `kwokctl
start` failed with *"component etcd does not exist"* alongside a warning that
the cluster had been created by an older kwokctl, and I recorded that as a
version incompatibility whose fix would destroy the layout store carrying T1's
succession record. It was not. **The container runtime was simply not running.**
Starting it brought the churn fleet back at its full 100 nodes, untouched,
along with the kind cluster.

Diagnosed from an error message instead of from the substrate underneath it —
the same shape as every other finding in this report, and the reason §9's
question 5 keeps earning its place. The fleet was available after all; the gate
still belongs on purpose-built clusters, for the reason above.

### 7.2 Gate A — a reschedule

The selected workload's city was re-pinned from `a1` (zone z-a) to `b2` (zone
z-b) — a different continent.

```
the city actually moved                              2 distinct positions, nodes ['a1','b2']
the selection never lost its place                   yes
the selection is AT the city's position every tick   yes
a stored cell would now name something else          (11,2) -> province a1; identity resolves to (41,20)
```

### 7.3 Gate B — a zone addition, with a warm selection

A **warm** city selected; zone `z-c` then added to the **hot** cluster.

```
the hot world actually grew a zone     extent 56->86, zones [z-a,z-b] -> [z-a,z-b,z-c]
the selection is in the WARM cluster   yes
it moved by exactly the hot growth     selection dx=30, hot growth=30
only the offset moved, not the row     yes
a stored warm cell would now fall...   (70,12) -> HOT province c1 (zone z-c)
```

That last line is the phase in one sentence: **a pre-inversion warm selection
would now be pointing at a hot node, in a zone that did not exist when it was
made.** No error, no clue.

### 7.4 §8.1 — the metric, checked before being trusted

Not a before/after image. The mark is *supposed* to move — the subject moved —
so a pixel diff would confirm the wrong thing. Both gates compare what the
selection **resolves to** against the identity's current position, from
`--dump-positions`, which now emits the selection beside the world.

### 7.5 §8.2 — and the discrimination check caught a false pass

Each gate asserts its own precondition. That earned its keep twice on the way in:

1. The first run of the committed script **re-used a leftover cluster** that
   already had zone `z-c`, so gate B's precondition never happened — extent
   86 → 86 — while *every other assertion still passed*. Without the
   precondition check it would have reported green on a no-op. The script now
   always starts clean.
2. Gate A then failed on a fresh cluster with "the city actually moved: 1
   distinct position". The assertion was right and the **wait** was wrong: a
   fixed `sleep 30` outran the reschedule on a busier cluster. It now waits on
   the condition — pods actually landing on `b2` — not on the clock.

Twelfth instance this workstream of an instrument reporting a plausible number
for a reason unrelated to what it claimed to measure. It is the only class of
error here that has never yet been caught by anything except explicitly checking
for it.

---

## 8. §6 — what this did not do

- **No camera movement on selection.** The writers that flew before still fly
  (`N`, `]`/`[`, the dev flags, the almanac); the map click still does not.
  `aim_for_drilldown` is unchanged and still fires once, on open.
- **No where-am-I marker** (D3), **no third selection level** (hover and commit,
  unchanged), **no namespace swatches**.
- IMPACT rows still do not set the selection — the D1 review's invariant that
  you walk a cascade without re-rooting the blast subject.

---

## 9. §7 — standing questions

**1. Summing before comparing?** None.

**2. Unknown, or fabricated?** The phase's centre. `selection_pos` returns
`Option`, and its `None` is *said* (§5) rather than defaulted. Each old site's
`None` was checked independently, as instructed, and they had not all agreed:
`focus_concern` returned early on an unplaced target, the map click overwrote
unconditionally, `--blast` fell through to a second loop. All preserved.

**3. Two sections constraining one behaviour?** §4 — the derivation and the
resolver, with three hand-rolled variants and no fixture that compared them.
Now one authority, and a test that asserts the round trip **through the
consumer**.

**4. Consumers depending on an old meaning?** Enumerated after the move, and the
type system enumerated it *for* me: every site failed to compile until it was
converted. That is the strongest form this question has taken.

**5. Inherited claims?** Nine tabled claims verified TRUE; the false statement
was in §3's **prose** (the almanac writer), and §0's "for free" was an
expectation the code did not meet (§3). Both were mine in origin — §0's
dissolution claim came from my own gate report.

**6. One side of a comparison moved?** §7 declares this *is* the phase.
`selected` meant a scene cell and now means an entity. The check that it still
means the same thing about the same subject is gate A's "the selection is at the
city's current position at every tick" — 23 ticks, across a continent change.

**7. Container adjacency read as world adjacency?** `]`/`[` walks a `Vec` of
cities and that order is arbitrary; it was arbitrary before and is a cursor, not
an inferred adjacency. The cell sweeps iterate by coordinate.

---

## 10. §9 — acceptance

- [x] §3's inventory re-enumerated and recorded (§2)
- [x] Selection is an identity carrying `ClusterId`; position derived per frame
- [x] No cached derived position — and the cache mutation is pinned (M-A)
- [x] Both staleness sources tested, and both discrimination-checked live (§7)
- [x] The carved-sea divergence dissolves — **deliberately, not for free** (§3)
- [x] §5 decided (tombstone), and the SELECTION box says which
- [x] `draw_selection` handles `None` audibly
- [x] Mutations asserted applied; **one survived and was closed** (§6)
- [x] Standing questions answered, claims tagged
- [x] `cargo nextest run --workspace` green — 579 tests

430 core + 124 GUI tests; gui-smoke 55 states; fmt, clippy, shellcheck and the
conversion-authority guard clean.
