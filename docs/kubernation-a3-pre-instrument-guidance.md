# KuberNation — A3-pre: An Instrument That Can See Assignment

**Implementation guidance**
**Goal:** make city placement measurable, then make it churnable.
**Produces no product behaviour change.** Its output is an instrument and a scenario.

Governing docs: measurement session report §3, §4, §7. Decomposition §4 (A3).

---

## 0. Why this exists, and why the order matters

The measurement session established that **the gate method used through A2 is blind to the thing Workstream A removes**:

> The map renders a projection, and a projection can be stable in appearance while unstable in assignment.

Pre-A2, 27% of untouched provinces moved while the pixel comparator reported ~1% of land area. The comparator was not wrong about pixels; pixels were the wrong measurement. `reshuffle.py` only rescued that case because the pre-A2 ordering was recomputable from node names alone. **Nothing about A3's city placement is recomputable that way**, so there is no equivalent rescue available.

The same session also found that pixels cannot see cities at realm scale at all: at the zoom where name plates render, no viewport holds more than about three cities; zoom out far enough to hold them all and the GUI stops drawing plates. Coverage on the committed frames was 2 of 7 workloads, and both were structurally insulated from the churn.

**So: instrument first, scenario second.** A workload-churn scenario with no way to read the result would produce another number of unknown meaning.

---

## 1. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `city_dx(name)` is hash-stable — `CITY_COL0 + fnv1a64(name) % CITY_COLS` | `state/world.rs` ~391 |
| 2 | `city_cell` *finds* a free cell rather than clamping, and past capacity falls back to the preferred cell and stacks | `state/world.rs` ~417 |
| 3 | City placement is order-dependent — a city's cell depends on which siblings were placed first | A2 report §6; confirm in `city_cell`'s caller |
| 4 | Cities are sorted by `WorkloadRef` before placement (`cities.sort_by(|a, b| a.r.cmp(&b.r))`) | `state/world.rs` ~455 |
| 5 | `City` carries `x` and `y` as `u16` | `state/world.rs` ~51 |
| 6 | `Models::build_with(world, filter, prior)` returns `Models` carrying `world: WorldModel` | `state/model.rs` |

**Claim 4 matters more than it looks.** If cities are already sorted by a stable key before placement, then order-dependence is *between* frames (which siblings exist), not *within* a frame. Those are different problems and A3 needs to know which it is solving.

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 7 | kwok schedules real pods via a real `kube-scheduler` | A-pre §1. Workload churn will genuinely reschedule, not just mutate objects |
| 8 | `city_home` sites a workload at the node holding the plurality of its pods, tie-broken on stable hash | A city legitimately moves when its pods move. That is **not** the instability A3 fixes |
| 9 | Deleting a workload strands its pods for 30–60s (PodGC quarantine) | A-pre §3. A scenario that measures immediately after a delete reads a transitional state |

Claim 8 is the one that will muddy the gate. **A3's charter is that a city does not move when *something else* changes.** A city following its own pods across provinces is correct behaviour, and the instrument must be able to tell the two apart.

---

## 2. The positional instrument

A dev-only dump of what the model *assigned*, not what was rendered.

### 2.1 Shape

Per tick, emit one record per city:

```
tick, workload_ref, province_node, slot_key, city_x, city_y
```

Plus, for the province context A3's gate needs:

```
tick, node, zone, pool, ordinal, extent_class, ghost?
```

Line-oriented and diffable — CSV or JSON-lines. **Not** a pretty-printed report: the consumer is a comparison script, and the measurement session's lesson is that the instrument should carry its own method rather than needing prose to interpret it.

### 2.2 Where it hooks

`Models` already carries `world: WorldModel`, which holds every `Province` and its `City` list with final coordinates. **The dump is a pure read of an existing structure** — walk `world.continents`, emit a row per city.

Prefer this over instrumenting placement internals. A dump of the output is what the gate is actually about; a trace of the algorithm would couple the instrument to an implementation A3 is about to change.

### 2.3 Driver

Follow the house dev-flag convention (`--blast`, `--inspect`, `--shot-seq`):

```
--dump-positions <PATH>    Append a positional record per tick. Dev instrument.
```

**No new observation path, no new model fields.** If this needs anything beyond reading `WorldModel`, stop and report — that would mean the coordinates are not where the gate assumes they are.

### 2.4 The comparison script

Beside `compare.py` in `hack/churn/`. Given two ticks, classify each city:

| Class | Meaning |
|---|---|
| **HELD** | same province, same cell |
| **MOVED-WITHIN** | same province, different cell — **A3's target** |
| **FOLLOWED** | different province, *and* its pod plurality moved — correct behaviour (claim 8) |
| **MOVED-ACROSS** | different province, pod plurality did **not** move — a defect |
| **ARRIVED** / **DEPARTED** | not present in one tick |

Report counts and a rate. **Print the denominator** — the measurement session's headline was a metric that inverted because a per-class delta was divided by whole-map area. State what the rate is over: cities present in both ticks.

Distinguishing FOLLOWED from MOVED-ACROSS needs pod placement in the dump. Either add a per-city pod-plurality-node column, or derive it — but decide explicitly, because without it every cross-province move looks like a defect.

### 2.5 Test the instrument

Per A2's six instrument failures and the measurement session's self-test:

- [ ] Dump twice with no cluster change → every city HELD, zero deltas
- [ ] Force a known city move (hand-edit a fixture, or shift one city's `x` in a test) → the script reports exactly that one MOVED-WITHIN
- [ ] A workload whose pods move to another node → FOLLOWED, not MOVED-ACROSS
- [ ] An empty world → the script says so and exits non-zero, rather than printing a confident 0%
- [ ] Ticks of differing city counts → ARRIVED/DEPARTED counted, not silently dropped

Commit these as a self-test script, not a one-off run. **A comparator always emits a plausible percentage** — that is the shape of failure this workstream keeps meeting.

---

## 3. The seventh scenario

`hack/churn/` currently has six, all node-level. A3's gate condition — *"adding a workload to a node moves no existing city, anywhere"* — needs workload churn on a **settled, un-refreshed** fleet.

### 3.1 What it must do

- Add a Deployment to a settled fleet; scale an existing one up and down; delete one
- **Touch no nodes.** Node churn is the six existing scenarios' job, and mixing them makes the gate ambiguous about which change moved what
- Settle between steps — per claim 9, measuring straight after a delete reads a PodGC transient

### 3.2 Coverage is the requirement, not the step count

The measurement session's finding was that coverage was 2 of 7 workloads and both were insulated. The scenario must place workloads on provinces that are **actually observed**:

- Cities on the same province as the churned workload — the direct displacement case
- Cities on a *different* province in the same zone — the case that catches ordinal-style knock-on
- At least one province carrying several cities, since single-city provinces cannot exhibit sibling-order effects at all

A scenario that adds one workload to an empty province would pass a broken A3.

### 3.3 The no-op guard

`gate.sh` learned to refuse a run that finds no `OLD_GEN` nodes. This scenario needs the same: **if the fleet is not settled, or the target province has no incumbent cities, exit non-zero.** A silent no-op that reports perfect stability is precisely how the first A2 gate answer went wrong.

---

## 4. Standing questions — written answers required

1. **Where does a summing step precede a comparing step?**
2. **Does every reducer over a possibly-empty input express unknown, or fabricate?**
3. **Where do two sections constrain the same behaviour — and is there a fixture where they diverge?**
4. **What existing consumers depend on the old meaning of a value this change redefines?**

Question 1 applies directly to §2.4's rate, which is the metric that inverted last session. Question 2 applies to the empty-world and no-common-cities cases.

---

## 5. Acceptance

- [ ] `--dump-positions` emits per-tick city and province records, reading `WorldModel` only
- [ ] No new model fields, no new observation path
- [ ] Comparison script committed beside `compare.py`, method documented in the file
- [ ] FOLLOWED is distinguishable from MOVED-ACROSS, and how is stated
- [ ] Rate prints its denominator
- [ ] Instrument self-tests committed and passing
- [ ] Seventh scenario committed; touches no nodes; guards against a no-op run
- [ ] Scenario coverage includes a multi-city province and a same-zone bystander
- [ ] A baseline run recorded against current `main` — **the pre-A3 number**
- [ ] Standing questions answered in writing
- [ ] No product behaviour changed

---

## 6. What this session must not do

**No A3 work.** No city slots, no placement changes. This session measures the current behaviour; changing it here would erase the baseline the gate needs.

**No tuning toward a desired number.** If current `main` turns out to be more stable than expected, that is a finding and it may shrink A3.

**No pixel work.** The measurement session settled that pixels cannot see this. Corroboration only, if at all.

---

## 7. The baseline is the point

Record the pre-A3 number before touching anything. A2's report notes the before/after was withdrawn once and nearly lost; this is the same debt one phase later, and it is cheapest now.

Concretely: run the seventh scenario against current `main`, dump positions, and report the MOVED-WITHIN count and rate. **That number is A3's gate target.**

If it comes back at or near zero, say so plainly — it would mean A2's placement change already fixed more than expected, and A3's scope should shrink before it is specified rather than after.

---

## 8. Estimate

**Half a day to a day.** The dump is a walk over an existing structure; the scenario is YAML and shell. The comparison script's FOLLOWED-versus-MOVED-ACROSS distinction is the only real design work, and §2.4 flags it as the thing to decide explicitly rather than let emerge.
