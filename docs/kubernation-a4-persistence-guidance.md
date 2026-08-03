# KuberNation — A4: Persistence

**Implementation guidance**
**Goal:** the layout survives restart, so spatial memory can actually accrue.
**Gate:** open, close, reopen. **The map is the same map.**

Governing docs: `kubernation-workstream-a-decomposition.md` §4 (A4), §6 (settled decisions). Plan §3.2 (A3), §3.3 (declared frame).

---

## 0. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `Layout`'s fields are **private** (`slots`, `zone_ordinals`) with no serde derives | `state/layout.rs` ~120 |
| 2 | `SlotState` is `{ occupant: Option<Occupancy>, last_occupant: Option<String> }` | `layout.rs` ~126 |
| 3 | `SlotKey` is `{ zone, pool, ordinal }` | `layout.rs` |
| 4 | `Layout::ghosts()` yields slots whose `occupant` is `None` | `layout.rs` |
| 5 | `prefs.rs` writes atomically (temp + rename), is XDG-aware, versioned, and falls back on a corrupt file | `prefs.rs` |
| 6 | `PREFS_VERSION` exists and its doc says bump on **incompatible** schema change | `prefs.rs` |
| 7 | `Models::build_with(world, filter, prior)` takes the prior layout; `build_carrying` feeds it forward per world | `state/model.rs`, `net.rs` |

**Claim 1 shapes §2.** Private fields with no derives means the on-disk shape is a decision, not a `#[derive]`.

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 8 | Kubernetes has **no native cluster ID**; the `kube-system` namespace UID is a convention, not an identifier | The fingerprint (§3) rests on this. Do not describe it as a cluster ID |
| 9 | A kubeconfig context name can be re-pointed at a different cluster | The exact case the fingerprint guards |
| 10 | One refresh of N nodes produces N ghosts; accumulation rate is refresh cadence | Sizes the reap. A weekly refresh of 100 nodes is 100 ghosts/week |

---

## 1. Why A4 is not plumbing

Plan §1 claims the map's advantage over K9s and Freelens is **spatial memory**. A1–A3 made the map stable *within a session*. Nobody builds spatial memory of a layout that resets on restart.

**A4 is where that thesis actually gets tested.** The decomposition put the kill point at A2; A2 moved it to A3; the honest position now is that stability across sessions is the first version of the claim a user could actually experience. Frame the gate accordingly (§7).

---

## 2. On-disk shape

Use a **DTO with explicit conversion**, not serde derives on the internal types.

`Layout`'s encapsulation is deliberate — its doc comments explain why transitions are computed rather than stored. Deriving on private fields couples the file format to internal representation, so any future refactor of `slots` becomes a format break.

```rust
/// The persisted form. Deliberately flat and deliberately NOT `Layout` itself:
/// the internal `BTreeMap<SlotKey, SlotState>` is free to change without
/// breaking a file someone has on disk, which is the whole reason this type
/// exists separately.
struct StoredLayout {
    version: u32,
    fingerprint: Option<String>,
    zone_ordinals: Vec<(String, u16)>,
    slots: Vec<StoredSlot>,
}

struct StoredSlot {
    zone: String,
    pool: String,
    ordinal: u16,
    occupant: Option<String>,
    last_occupant: Option<String>,
    /// RFC3339. `Some` only while the slot is a ghost — see §4.
    vacated_at: Option<String>,
}
```

`Layout` needs a constructor from stored parts. Keep it explicit (`Layout::from_stored`) rather than making the fields public.

**Follow `prefs.rs`'s mechanics exactly**: atomic temp + rename, XDG-aware path, corrupt file falls back to empty rather than failing to start. A layout that cannot be read is a *fresh map*, not a crash — and it should say so, since the user will notice the world changed.

---

## 3. Store identity

Settled: **context name as the key, cluster-scoped UID as a fingerprint.**

- **Key** = kubeconfig context name. Human-meaningful — it is what `--context` takes and what appears on screen.
- **Fingerprint** = any stable cluster-scoped UID. The `kube-system` namespace UID is the usual choice; per claim 8, call it a fingerprint and not a cluster ID, because Kubernetes does not have one.

On load, if the stored fingerprint and the observed one disagree, **this layout belongs to a different world**. Discard it, start fresh, and **declare it** — a recorded event, not a silent reshuffle. This is the settled "first run remakes the world once, declare it — the first cataclysm" item, reassigned here from A2.

A missing fingerprint (older file, or a cluster where the UID is unreadable) is not a mismatch. Accept the layout and record that it is unverified. Refusing to load on an unreadable UID would break the map for the least-instrumented clusters, which is the population that benefits most from stability.

**Storage layout:** one file per context under a layouts directory, not a section inside `prefs.json`. Prefs are per-user and small; layouts are per-cluster and grow. Separate files also mean a corrupt layout for one cluster does not take out the others.

---

## 4. Ghost reaping

Two operations. **They are different verbs, not one verb with an override.**

| | Selects | Trigger | Declared |
|---|---|---|---|
| **Age reap** | ghosts vacant longer than the retention window | automatic, on load | yes |
| **Compaction** | **all** ghosts | explicit command | yes |

Age reaps *stale* ground. Compaction reaps *all* of it, because the reason to reach for it is "I decommissioned that pool and I want the map to show it now."

### 4.1 The clock must stay outside the pure function

`assign_layout` is pure and that is what makes it testable with synthetic fixtures. Do not give it an ambient clock.

- `SlotState` gains `vacated_at: Option<SystemTime>`, set when a slot's occupant departs
- The reap takes **now** as a parameter: `Layout::reap_ghosts(&mut self, now, retention)`
- Tests pass a fixed instant

Wall-clock, not tick counts: a tick is meaningless across restarts, and A4 is entirely about restarts.

### 4.2 Retention window

**Default 14 days**, user-overridable.

The reasoning, stated so it can be argued with: long enough that a rolling refresh's ghosts survive several weeks of ordinary operation and a node returning from maintenance still finds its own ground; short enough that a decommissioned pool does not haunt the map for a quarter.

**This default is a judgment, not a measurement.** Say so where a user will see it. Per claim 10 the accumulation rate is arithmetic — one refresh of N nodes yields N ghosts — so a weekly-refreshed 100-node fleet holds ~200 ghosts at 14 days. That is fine; if it turns out not to be, the number is a setting and the harness can measure it.

Override via prefs plus a CLI flag, mirroring how `--overlay` and `--map-style` work — persisted, settable from the UI, flag for one-off runs. `0` should mean *never reap*, and that must be a real supported value, not a footgun that reaps everything.

### 4.3 Reaping is a cataclysm

Per plan §3.2, structural change to the world gets recorded. A reap changes which ground exists. It must be:

- **Recorded** with a date and a count
- **Visible** — the user should be able to find out why the map has fewer slots than last week
- **Never silent**

A reap that quietly shrinks the world is exactly the silent reshuffle A exists to eliminate, arriving by a different door.

---

## 5. What A4 does not do

- **No succession or cataclysm rendering.** A5 owns the vocabulary; A4 records the events.
- **No compaction of live slot ordinals.** Reaping removes ghosts; it does **not** renumber the survivors. Renumbering moves live provinces, which is the thing being prevented. A reaped ghost leaves its ordinal unused.
- **No graticule.** A6.
- **No change to `assign_layout`'s algorithm** beyond carrying `vacated_at`.

> **§5's second bullet is the one to get right.** "Compaction" in the settled decisions means *reclaiming ghost ground*, not *closing ordinal gaps*. If compaction renumbers, A4 undoes A1.

---

## 6. Tests

**Round trip:**
- [ ] Save, load, and the layout is equal — including ghosts, `last_occupant`, and zone ordinals
- [ ] A corrupt file loads as empty and reports it, rather than failing to start
- [ ] A file from a future version is refused gracefully

**Identity:**
- [ ] Matching fingerprint → layout loads
- [ ] Mismatched fingerprint → discarded, fresh layout, event recorded
- [ ] Absent fingerprint → loads, marked unverified

**Reaping:**
- [ ] A ghost younger than the window survives; older is reaped — fixed clock, both directions
- [ ] Retention `0` reaps nothing
- [ ] Reaping does **not** renumber surviving ordinals — the §5 invariant
- [ ] Manual compaction removes all ghosts regardless of age
- [ ] Both record an event
- [ ] A slot that is re-occupied clears `vacated_at`

**Stability across a restart (the gate, as a unit test):**
- [ ] Build a layout, persist it, load into a fresh process, assign against the same node set → **every occupied slot keeps its coordinates**
- [ ] Same, with a rolling refresh between save and load → survivors hold, replacements reclaim their predecessors' ground

**Mutation floor:** make the load path return `Layout::default()` and confirm the restart test fails.

---

## 7. The gate

**Open, close, reopen. Is it the same map?**

Run it against the churn fleet with `--dump-positions` on both sides, and compare with `positions.py`. Every city HELD, every province in the same place. That is a positional comparison, not a pixel one — per the measurement session, pixels cannot see assignment.

Then the harder version, which is what the thesis actually claims:

- Refresh the fleet, close, reopen. Survivors hold; replacements sit on their predecessors' ground.
- Leave a ghost, reopen after the retention window with a manipulated clock or a short window, and confirm the reap is declared rather than silently applied.

Report the result against A3's numbers in the same units.

---

## 8. Standing questions — written answers required

1. **Where does a summing step precede a comparing step?**
2. **Does every reducer over a possibly-empty input express unknown, or fabricate?**
3. **Where do two sections constrain the same behaviour — and is there a fixture where they diverge?**
4. **What existing consumers depend on the old meaning of a value this change redefines?**
5. *(new)* **Which claims here were inherited from a prior report rather than verified against code?**

Question 5 is added because A3 found three requirements resting on a caution in A3-pre's report that was wrong — the allocatable-less node was in a different pool than stated. §0 checks claims against code; inherited claims got no such check. **Tag any claim in this document whose source is a prior report, and verify it independently.**

Question 2 applies directly to the retention window: an absent `vacated_at` on a ghost — from an older file, or a bug — must not be treated as "infinitely old" and reaped. Absent means *unknown*, and the safe reading is to keep it and stamp it now.

---

## 9. Acceptance

- [ ] Layout persists per context, keyed on context name with a fingerprint
- [ ] DTO with explicit conversion; `Layout`'s fields stay private
- [ ] Atomic write, XDG path, corrupt file degrades to empty and says so
- [ ] Fingerprint mismatch discards and **declares**; absent fingerprint loads as unverified
- [ ] Age reap with a 14-day default, overridable via prefs and a flag, `0` meaning never
- [ ] Manual compaction reaps all ghosts
- [ ] Neither renumbers live ordinals
- [ ] Both recorded as events
- [ ] Gate run positionally, both restart cases, reported against A3
- [ ] Standing questions answered, including question 5
- [ ] `cargo nextest` green

---

## 10. Estimate

**One to two days.** The DTO and round-trip are straightforward; the identity handling and the two reap paths carry the design risk, and the restart gate needs the harness.

The consistent overrun in this series has been consumer sweeps and review rounds, not the core change. A4 adds no new consumers — but it does add a **file format**, which is the first artifact in this workstream that outlives the process and cannot be silently changed later. Budget the review accordingly.
