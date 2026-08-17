# KuberNation — Item A: Extent Headroom (revision 2)

**Implementation guidance**
**Goal:** a node at a nominal size boundary gets the class its size implies.
**Gate:** a synthetic boundary node classifies correctly. **Not a fleet capture** — see §4.

Closes the last open item from the consolidation round.

> **Revision 2.** VOR became available after revision 1 was written. Claims 4 and 5 are now `[V]`, and the naming drift they describe turned out to sit at **three** sites rather than one — with a decision attached rather than only a comment. See §2.1.

---

## 0. Verify before building

`[V]` verified against source this round. `[A]` asserted from a prior report.

| # | Claim | Tag |
|---|---|---|
| 1 | `EXTENT_BOUNDS_GIB = [32.0, 128.0, 512.0]`, class = `EXTENT_BOUNDS_GIB.iter().filter(\|b\| gib >= **b).count()` | `[V]` `world.rs:502,509` |
| 2 | The doc comment states the intent: *"a node at or above the Nth bound gets the (N+1)th height"* | `[V]` `world.rs:500–501` |
| 3 | `EXTENT_CLASSES = [3, 5, 7, 9]`; the fallback rungs both return `EXTENT_CLASSES[1]` | `[V]` `world.rs:498,511–512` |
| 4 | `node_extent_input` wraps `node_allocatable(node, "memory")` in **`ExtentInput::Capacity`**; `province_extent` matches that arm and returns **`ExtentSource::Capacity`** — three sites carrying a word for a field none of them reads | `[V]` `model.rs:455–469`, `world.rs:499–503` |
| 5 | There is **no capacity read path** anywhere — `status.capacity` is never consulted | `[V]` same; `node_extent_input`'s own doc says "allocatable memory, else instance type, else nothing" |
| 5a | `extent_line(ExtentSource::Capacity)` returns `None` — the measured case is deliberately silent, so the word never reaches the panel as text | `[V]` `panels.rs:417–426` |
| 6 | kind reports `capacity == allocatable == 15.653 GiB` on a nominal 16 GiB VM (2.2% short) | `[A]` consolidation §5.1, measured |
| 7 | kwok reports exact round numbers, so both the current and the `[30,120,480]` bounds give identical classes on every churn-fleet node | `[A]` consolidation §5.2, measured |
| 8 | The churn fleet's distribution is 30 × class 3, 53 × class 5, 16 × class 7 — **class 5 is the ordinary case**, not class 3 | `[A]` consolidation §5.2 |
| 9 | `SLOT_STRIDE` is the largest extent class, so a class change does not move any province | `[V]` `world.rs`, `slot_row` doc |

**Claim 4 is now a decision, not just a comment.** See §2.1.

**Claim 8 kills a symptom that has been repeated in the record.** The open item has been described as *"why the smallest extent is the ordinary case … the map is mostly thin ribbons."* It is not — class 5 is ordinary, and the thin ribbons come from the stride being 9 while extents are 3–7. **Do not restate the old symptom in the changelog.**

---

## 1. The defect

`EXTENT_BOUNDS_GIB` is written in **nominal machine sizes**. The value compared against it is a **reported** figure, which is always lower — firmware and reserved RAM, plus any kubelet reservation.

So a node sold as 32 GiB reports ~30.9, fails `gib >= 32.0`, and takes the class below. The same holds at every bound. That contradicts claim 2's stated intent, in the doc comment directly above the constant.

**This is standing question 6 in miniature:** two sides of a comparison that have never meant the same thing.

### 1.1 Why the obvious fixes were refuted

Recorded so they are not re-proposed:

| Fix | Refuted by |
|---|---|
| Compare against capacity | Capacity is not nominal either (claim 6), and there is no capacity read path (claim 5). It would change nothing and cost new plumbing |
| Move bounds to `[30, 120, 480]` | Fixes the boundary case, but encodes an unexplained fudge in constants that look like machine sizes — the next reader rounds them back |
| Midpoints `[24, 96, 384]` | Promotes genuine 24, 96 and 384 GiB machines, which are real instance sizes |

---

## 2. The change

```rust
/// Reported memory runs below the machine's nominal size: firmware and reserved
/// RAM (measured — kind reports 15.653 GiB on a nominal 16 GiB VM, 2.2% short),
/// plus any kubelet reservation (zero on kind and kwok, larger on managed clouds
/// where the reserve is a tiered fraction of total memory).
///
/// The bounds above are written in NOMINAL sizes, so the reported figure is
/// scaled by this headroom before comparison — otherwise a node sold as 32 GiB
/// reports ~30.9, fails `>= 32.0`, and takes the class below, which is the
/// opposite of what the bounds' own doc comment promises.
///
/// UNMEASURED on managed clouds; 8% covers firmware plus a modest reservation
/// and is deliberately at the small end of plausible. Too small leaves the
/// original defect; too large promotes genuine in-between machines — a 24 GiB
/// node would need 33% to be wrongly promoted, so there is room, but not
/// unlimited room. The tripwire is a node whose nominal size is known and whose
/// class is wrong.
const EXTENT_HEADROOM: f64 = 0.08;
```

```rust
let class = EXTENT_BOUNDS_GIB
    .iter()
    .filter(|b| gib * (1.0 + EXTENT_HEADROOM) >= **b)
    .count();
```

**Scale the reported value, do not shift the bounds.** The bounds stay readable as machine sizes, and the correction sits where the two quantities differ, named. That is the whole argument for this shape over `[30, 120, 480]`.

### 2.1 The naming drift — rename or comment, decide explicitly

`node_extent_input` reads **allocatable** and wraps it in `ExtentInput::Capacity`. `province_extent` matches that arm and returns `ExtentSource::Capacity`. Three sites carry a word that, in Kubernetes, names a specific field (`status.capacity`) none of them reads — and which reports a *different number*.

This is the same class as `first_trouble` comparing an onset against a `when`: a name asserting a quantity the value is not. It is also how revision 1 of this document came to recommend "compare against capacity" — I read the variant name and inferred the source.

**The consumers are now enumerated**, so this is a bounded decision rather than a judgement:

| Site | What it is | `[V]` |
|---|---|---|
| `ExtentInput::Capacity` | the input variant | `model.rs:455–469` |
| `ExtentSource::Capacity` | the source variant | `model.rs` |
| `province_extent`'s match arm | the classifier | `world.rs:499–503` |
| `extent_input_prefers_capacity_then_instance_type_then_nothing` | a **test name** | `model.rs:2908` |
| `extent_line(ExtentSource::Capacity) => None` | the panel — silent for this arm | `panels.rs:417–426` |
| `dump_positions` | emits `"extent_source":"{:?}"` — **`Debug`**, so the JSON value is literally `"Capacity"` | `main.rs:506–569` |

Callers of `node_extent_input`: `build_node_tile` and one test. Nothing else.

**Recommend the rename to `Allocatable`.** Six sites, all in-tree, and the panel is silent for this arm so no user-facing text changes.

**The one real consequence: the dump's JSON value changes**, because `{:?}` prints the variant name. That is a dev artifact rather than a persisted user file — unlike A4's layout store, nothing degrades — but it is an output format, so:

- [ ] Check `hack/churn/pieces.py` and any other committed instrument for a literal `"Capacity"` match, and update it in the same commit
- [ ] A script silently matching a string that no longer appears is the shape that produced the "1 of 8" figure

Do not overlook the **test name**. A test called `..._prefers_capacity_...` asserting allocatable behaviour is the same drift one layer out, and it is the artifact a future reader is most likely to trust.

It is cheap now and gets more expensive the moment anyone scripts against the dump's field values.

**If the comment is taken instead**, it must say plainly: this is allocatable, `status.capacity` is never consulted, and the two report different numbers. Leaving it inferable is what caused this document's own first revision to be wrong.

---

### 2.2 Check the arithmetic before writing tests

At 8%, the effective threshold is `bound / 1.08`:

| Bound | Admits from | A genuine machine at | Promoted? |
|---|---|---|---|
| 32 | 29.63 | 24 GiB | no |
| 128 | 118.5 | 96 GiB | no |
| 512 | 474.1 | 384 GiB | no |

And the boundary cases the fix exists for — 30.9 and ~123 — both clear. Confirm these against the real constants rather than trusting the table; if `EXTENT_CLASSES` or the bounds have moved, the margins move with them.

---

## 3. What this activates

**Class 9 becomes reachable.** It is currently unreachable, which is the only reason the Relief occlusion risk is dormant.

Consolidation landed the terrain sort (`terrain_order`) **first**, specifically so this could later be done alone. Confirm that ordering held — if `terrain_order` is in the tree and owns the paint sequence, the hazard is already defused and this change is safe in isolation.

If it is not, **stop**: land the sort first.

---

## 4. The gate

**A synthetic boundary node classifies correctly.**

### 4.1 The fleet cannot discriminate — do not capture one

Consolidation §5.2 measured this: kwok reports exact round numbers, so both the current and any candidate bounds give identical classes on every churn-fleet node. A before/after capture would show **zero changed pixels**, and then someone has to interpret a blank.

That is the trap this project has hit repeatedly, and it is avoidable here by arithmetic rather than by running the instrument and squinting.

> **Check whether the instrument can discriminate before running it, not after seeing the result.**

kind is no better: 15.653 GiB is nominally 16 and correctly class 3 under every candidate.

### 4.2 What to run instead

Unit tests over `province_extent` with synthetic `ExtentInput::Capacity` values:

- [ ] `30.9 GiB` → class index 1 (was 0) — **the defect, as a test**
- [ ] `123 GiB` → class index 2 (was 1)
- [ ] `24 GiB` → class index 0 — the promotion guard
- [ ] `96 GiB` → class index 1 — same
- [ ] `384 GiB` → class index 2 — same
- [ ] `32.0` and `128.0` exactly → unchanged from today
- [ ] `0` and an absurdly large value → no panic, sensible class

**Mutation floor, exercised:** set `EXTENT_HEADROOM` to `0.0` and confirm the 30.9 and 123 tests fail; set it to `0.35` and confirm the 24 GiB guard fails. Both directions, because the constant's job is to sit between two failure modes.

### 4.3 If a managed-cloud node is available

Optional, and worth taking if it is cheap. A single GKE/EKS/AKS node with a known nominal size, checked against its class, converts claim 6's kind measurement into something that covers the reservation term too.

If it turns out 8% is too small there, that is a finding — record the measured figure rather than nudging the constant until it works.

---

## 5. The Relief capture

Per §3, class 9 becomes reachable. No current fleet produces it, so the occlusion risk stays untested unless one is constructed.

- [ ] Add a node to the churn fixture above the top bound (nominally ≥ 512 GiB — kwok will report whatever is declared)
- [ ] Capture it under `MapStyle::Relief` and confirm the top-class province paints correctly against its northern neighbour

This is the one visual check worth doing, and it is a *new* capability rather than a before/after — there is no "before" because the class has never rendered.

---

## 6. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 6 is the defect.** Answer it about the fixed code too: after the change, the left side is *reported memory scaled to an estimated nominal*, and the right side is *nominal*. Those now mean the same thing approximately, which is better than exactly wrong — but say so, rather than implying the mismatch is gone.

**Question 4:** extent feeds `province_extent` → `Province.h` → the terrain pass and `rows` for city placement. Per claim 9 the stride is the largest class, so **no province moves** — but `rows` changes for any node that gains a class, and A3's city placement hashes into `rows`. Verify that a class change relocates cities *within* their province (expected, since `city_dy` is modulo `rows`) and does not do anything worse.

That last point deserves a test: **a node gaining an extent class must not move a city to another province.**

---

## 7. Acceptance

- [ ] `EXTENT_HEADROOM` named, with firmware measured and reservation stated as unmeasured
- [ ] The reported value is scaled; the bounds stay in nominal sizes
- [ ] §2.1 decided: renamed to `Allocatable` at all **six** sites (including the test name), **or** commented with the reason for keeping it
- [ ] If renamed: `dump_positions`' emitted JSON value changes — committed instruments matching `"Capacity"` updated in the same commit
- [ ] §3's ordering confirmed — `terrain_order` in the tree before this lands
- [ ] Boundary and promotion-guard tests present; mutation floor exercised in **both** directions
- [ ] No fleet before/after capture attempted, and §4.1's reason recorded
- [ ] A class-9 node added to the fixture and captured under Relief
- [ ] The city-placement question in §6 answered with a test
- [ ] The changelog states the real symptom, not the retired "smallest extent is ordinary" one (claim 8)
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 8. Estimate

**Two to three hours.** The change is two lines and a comment; the tests and the class-9 Relief capture are the work.
