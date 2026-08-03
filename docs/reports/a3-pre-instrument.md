# A3-pre — an instrument that can see assignment

**Report** · 2026-08-03 · **v1.7.2** (a dev flag; no behaviour change)
**Governing doc:** [`kubernation-a3-pre-instrument-guidance.md`](../kubernation-a3-pre-instrument-guidance.md)
**Instruments:** `--dump-positions` · `hack/churn/positions.py` · `positions-selftest.py` · `positions-run.sh` · `scenarios/7-workload-churn.sh`

---

## The pre-A3 number

> **Adding one workload moved every incumbent city on that province — 3 of 3.**
> **Deleting it moved them all back — 2 of 2.**
> **Nothing else moved a city at all.**

| event | MOVED-WITHIN | of cities in both ticks |
|---|---|---|
| add a workload sorting **ahead** of the incumbents | **3** | 11 → **27.3%** |
| delete a workload sorting **ahead** of them | **2** | 10 → **20.0%** |
| scale a workload up (3 → 9) | 0 | 12 |
| scale it back down (9 → 3) | 0 | 12 |
| delete a workload sorting **after** them | 0 | 11 |
| settle, no change | 0 | 11 |

The movement is **deterministic, not probabilistic**: every incumbent on the
province moved, each by exactly one row, each keeping its column —
`(6,1)→(6,2)`, `(5,2)→(5,3)`, `(4,3)→(4,4)` — and the deletion inverted it
exactly.

**So A3's target is not "cities drift". It is one specific dependency:** a
city's row is `i % rows` where `i` is its index in the province's
`WorkloadRef`-sorted city list, so anything that changes how many siblings sort
ahead of it moves it. Scaling does not (the set is unchanged). Deleting a
later-sorting sibling does not (earlier indices are untouched). Only insertion
or removal *ahead* of a city moves it.

That is a smaller, sharper problem than the phase was scoped against, and it has
an obvious shape of fix: a stable per-city key instead of a positional index.

### The denominator, stated

The rates above are over **cities present in both ticks**, which is what the
script prints. Two other denominators are worth naming because they say
different things:

- **Of incumbents on the affected province: 100%.** Every one moved, both times.
- **Of all cities in the realm, across the whole scenario: 0%.** The seven
  original workloads never shared a province with a churned sibling, so they
  held throughout (0 of 7, ticks 0 → 90).

The fleet-wide figure is therefore a property of *packing*, not of stability:
on this 100-node fleet seven workloads land on seven distinct nodes, so almost
nothing shares a province. On a smaller or denser cluster the same defect
touches far more cities. **A3's gate should be read against the per-province
figure, which is 100% and does not depend on fleet size.**

---

## 1. Verification — all nine §1 claims TRUE

Sixth round running that §1 survived intact.

`city_dx` is `CITY_COL0 + fnv1a64(name) % CITY_COLS` (world.rs:392); `city_cell`
finds a free cell and falls back to the preferred one past capacity (:440);
placement is order-dependent through an accumulating `taken` set; `City.x/y` are
`u16`; `Models` carries `world: WorldModel`. The three semantic claims were
established in A-pre and A1.

**Claim 4 held and its implication matters**, as the guidance said it would:
`cities.sort_by(|a, b| a.r.cmp(&b.r))` runs at world.rs:623, *before* the
placement loop at :641. So the ordering is a function of the city *set*, not of
iteration order — order-dependence is **between frames**, and the measured
mechanism above is exactly that.

---

## 2. Where the guidance was wrong

**§2.4's `MOVED-ACROSS` class cannot occur, and its suggested extra column
would have been the same column twice.**

A city is emitted only on the province whose node is its pod plurality:
`city_home` takes `max_by_key` over pods-per-node (world.rs:579-587) and the
render loop skips any province that is not that node (:605-607). So **a city's
province *is* its plurality node, by construction.** A cross-province move
cannot mean anything except that the plurality moved — which is `FOLLOWED`,
correct behaviour. There is no state in which a city sits on a province that is
not its plurality, so "different province, plurality did not move" is
unrepresentable.

§2.4 asks to decide the FOLLOWED-versus-MOVED-ACROSS distinction explicitly
rather than let it emerge. Decided: **the distinction does not exist**, the dump
carries no pod-plurality column, and the reason is recorded in
`positions.py`'s own docstring so the next reader does not go looking for it.

### And one class the guidance did not ask for

`CARRIED` — same node, same offset, but the **province itself** moved. On
absolute coordinates that is indistinguishable from a city moving, so a layout
relocation would be charged to A3. The dump therefore records each city's cell
twice: absolutely, and as an offset from its province origin. Classification is
on the offset.

It read 0 throughout this scenario, which is the expected result when no node
churns — but it is the difference between measuring placement and measuring
placement-plus-layout, and it costs one column.

---

## 3. The instrument

**`--dump-positions <PATH>`** appends JSON-lines per tick: a record per city
(workload, node, zone, absolute cell, offset within its province), per province
(node, zone, pool, ordinal, extent, extent source), and per ghost slot.

Per §2.2 it is a **pure read of the finished `WorldModel`** — no new model
fields, no new observation path, no trace of the placement algorithm. It walks
`world.continents` and reads `models.layout.slot_of` for the ordinal.

Two details that are not incidental:

- **It dumps per model REBUILD, not per frame** — keyed on the snapshot's `Arc`
  identity. The world rebuilds at tick cadence while the GUI redraws at ~60fps,
  so a per-frame dump would emit roughly fifteen identical copies of every tick
  and make a diff meaningless.
- **Coverage is total.** The dump carried all 7 workloads on the first probe.
  The pixel instrument it replaces topped out at 2 of 7, because no
  label-drawing viewport holds more than about three cities.

### `positions.py`

Classifies each city HELD / CARRIED / MOVED-WITHIN / FOLLOWED / ARRIVED /
DEPARTED, and prints the rate **with its denominator** — the metric that
inverted last session did so for want of one.

### The self-tests (§2.5) — all pass

```
ok   identical ticks are all HELD
ok   a shifted cell is exactly one MOVED-WITHIN
ok   a city following its pods is FOLLOWED, not a defect
ok   an empty dump refuses rather than reporting 0%
ok   no shared city means no rate, not a 0% rate
ok   a city gained and one lost are counted, not dropped
ok   a moving province CARRIES its cities rather than moving them
```

Seven, not five: the two extra are the no-shared-city case (a real input that
would otherwise divide by zero) and the CARRIED case above. Committed as a
script — a comparator always emits a plausible percentage, which is the shape of
failure this workstream keeps meeting.

---

## 4. The seventh scenario

`scenarios/7-workload-churn.sh`. Touches no nodes; the fleet ends with the same
100 it started with.

**Coverage was the hard part, and the stock fixture could not provide it.** On
the settled fleet every province carries exactly **one** city — seven workloads
across a hundred nodes — and a single-city province cannot exhibit a
sibling-order effect at all. Run against the fixture as it stands, the scenario
would have reported perfect stability while testing nothing.

So it builds the conditions §3.2 requires: three workloads pinned to one node
via `nodeSelector` (not `nodeName` — the real scheduler stays in the loop, per
claim 7) to make a genuinely multi-city province, plus a bystander on a
different province in the same zone. The added workload is named to sort
**first**, because a name sorting last would change no index and the scenario
would quietly test nothing.

**The no-op guard** counts Running incumbent pods on the target and exits 2 if
there are fewer than three, with the reason. `gate.sh` learned the same lesson
the hard way.

---

## 5. Standing questions — written answers

**1. Where does a summing step precede a comparing step?**
In the rate: `MOVED-WITHIN` is counted over the intersection of two ticks, then
divided. The denominator is the intersection, not either tick's total — using
one tick's count would have made a run that gains cities report a lower rate for
the same movement. The script prints which it used.

**2. Does every reducer over a possibly-empty input express unknown, or
fabricate?**
Three empty inputs are real here and all three refuse: a dump with no records, a
dump with only one tick, and two ticks sharing no city. The last is the
interesting one — the natural code prints `0.0%`, which reads as perfect
stability. It prints "no city is present in BOTH ticks" and exits 2, and there
is a self-test for it.

**3. Where do two sections constrain the same behaviour, and is there a fixture
where they diverge?**
§2.4 defines MOVED-ACROSS as a defect class while §1 claim 8 describes a city
legitimately following its pods. They diverge on any cross-province move — and
the code (§2 above) settles it: the two are the same event, so one of the
classes had to go.

**4. What existing consumers depend on the old meaning of a value this change
redefines?**
Nothing is redefined — the flag reads an existing structure and writes a new
file. The nearest hazard is the *offset* convention: `ox`/`oy` are relative to
the province origin, and a later phase that changes what `Province.x/y` means
would silently change what the instrument reports. Stated in the dump's doc
comment.

---

## 6. Acceptance

| §5 criterion | Status |
|---|---|
| `--dump-positions` emits per-tick city and province records, reading `WorldModel` only | ✅ |
| No new model fields, no new observation path | ✅ |
| Comparison script committed beside `compare.py`, method in the file | ✅ |
| FOLLOWED distinguishable from MOVED-ACROSS, and how is stated | ✅ **the class cannot occur** — §2 |
| Rate prints its denominator | ✅ |
| Instrument self-tests committed and passing | ✅ 7 |
| Seventh scenario committed; touches no nodes; guards a no-op | ✅ |
| Coverage includes a multi-city province and a same-zone bystander | ✅ built by the scenario, since the fixture has none |
| A baseline run recorded against current `main` | ✅ above |
| Standing questions answered in writing | ✅ §5 |
| No product behaviour changed | ✅ dev flag only |

No A3 work, no tuning, no pixel work.

---

## 7. Decisions for the room

### A3 is smaller than it was scoped to be

§7 of the guidance asked for this to be said plainly if it turned out that way.
It did. The instability is **one index dependency**, not a family of placement
problems: `row0 = i % rows` over a sorted sibling list. Scaling, deletion of a
later sibling, and node churn all move nothing.

A city slot keyed on the workload rather than on its position in a list would
close it. That is a smaller change than "city slots within a province" implies,
and A3 should probably be specified against the measured mechanism rather than
against §3.1a's original description.

**Ask:** re-scope A3 to the index dependency before writing its guidance?

### The gate target, and which denominator it uses

**100% of incumbents on an affected province.** Not the fleet-wide rate, which
is a packing artefact — 27.3% here, and it would be near zero on a fleet with
more nodes or fewer workloads, without anything having improved.

**Ask:** confirm the gate reads per-province.

### The fixture cannot exercise its own gate

Every province carries one city, so the sibling-order effect is unreachable
without the scenario building the conditions itself. That is fine for a scenario
but it means **the standing fixture under-represents realistic clusters**, where
a node routinely hosts several workloads. Worth deciding whether
`workloads.sh` should pin a few workloads together permanently, so other
scenarios and the dev loop see multi-city provinces too.

### ~~A caution about the target node~~ — **this was wrong; retracted**

> The original text said the scenario targets `churn-mem-g1-000`, "the
> deliberately allocatable-less node from A-pre", and therefore measured on a
> province sized by the *declared default* extent.
>
> **That is false.** `lib.sh:131` puts the allocatable-less node in the **`sys`**
> pool at index 0 — `churn-sys-g1-000`, which is also what the attention queue
> names in the gate frames. The scenario's target reports
> `memory: 128Gi`, and its province record in the dump reads
> `"h": 7, "extent_source": "Capacity"` — a **measured** extent.
>
> The error propagated: the A3 guidance took it up as claim 10, as a §5 fixture
> instruction ("do not use the allocatable-less node as the test province.
> A3-pre's scenario used it; change that too"), and as a §7 gate condition ("a
> multi-city province whose extent is measured, not defaulted"). That condition
> was already satisfied, and nothing needed changing. Caught in A3's §0
> verification, by checking the claim instead of inheriting it.
