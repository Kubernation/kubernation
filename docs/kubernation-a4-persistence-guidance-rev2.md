# KuberNation — A4: Persistence (revised)

**Implementation guidance · revision 2**
**Goal:** the layout survives restart, so spatial memory can accrue across sessions.
**Gate:** open, close, reopen. **The map is the same map.**

> **Supersedes revision 1**, which was stopped at §0 by its own verification step. Revision 1's claim 10 — *"one refresh of N nodes produces N ghosts; accumulation rate is refresh cadence"* — is **false for batched refreshes**, and it was the claim the automatic age reap rested on. See §1.

Governing docs: `kubernation-workstream-a-decomposition.md` §4, §6. Plan §3.2, §3.3.

---

## 1. What changed, and why

A1's measurement was correct and correctly reported: a **single-wave full-fleet surge** does leave N ghosts. Revision 1 generalised that to batched refreshes without checking. It does not hold.

**Ghosts reach a steady state at the refresh batch size and stay there.** A 100-node fleet refreshed in waves of ten holds ten ghosts after the first refresh and still ten after the fourth — each wave's replacements reclaim the previous wave's ghosts. Accumulation does not grow with cadence.

Two consequences:

- Revision 1 §4.2's sizing was void: ~10 ghosts, not ~200
- The **automatic age reap addressed something that does not occur**

And a third, which is the actual design conclusion:

> **A standing batch-size set of ghosts is the mechanism working, not debt.** That ground is reserved for nodes that may return. A2's gate found that *painting* that ground is what made a refresh read as stable rather than as the continent losing pieces of itself. An automatic reap would be the map quietly deciding those nodes are not coming back — a judgment it has no basis for and the user never asked it to make.

The real source of lasting ghosts is **shrinkage**, which is deliberate: you decommissioned a pool and you know you did. The honest response is an explicit verb.

**Decision: compaction only.** No automatic age reap in A4. Per A3 §2.1's principle, applied one section over: do not build it speculatively — the measurement exists to size the problem first.

`vacated_at` **stays in the format regardless.** A5's succession ageing wants it, and adding a field later costs a format bump.

---

## 2. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `Layout`'s fields (`slots`, `zone_ordinals`) are **private**, no serde derives | `state/layout.rs` ~120 |
| 2 | `SlotState` is `{ occupant: Option<Occupancy>, last_occupant: Option<String> }` | `layout.rs` ~126 |
| 3 | `SlotKey` is `{ zone, pool, ordinal }` | `layout.rs` |
| 4 | `Layout::ghosts()` yields slots whose `occupant` is `None` | `layout.rs` |
| 5 | `prefs.rs` writes atomically (temp + rename), is XDG-aware, versioned, degrades on a corrupt file | `prefs.rs` |
| 6 | `Namespace` is **not watched anywhere** — the fingerprint needs a new one-shot read | search the watch set |
| 7 | `k8s/browse.rs` is the on-demand read precedent, and reports what it could not enumerate rather than omitting silently | `browse.rs` ~70 |
| 8 | `Models::build_with(world, filter, prior)` takes the prior layout; `build_carrying` feeds it forward per world | `state/model.rs`, `net.rs` |

### Semantic

| # | Assumption | Source | Why it matters |
|---|---|---|---|
| 9 | Kubernetes has **no native cluster ID**; the `kube-system` UID is a convention | domain | Do not call the fingerprint a cluster ID |
| 10 | A kubeconfig context name can be re-pointed at a different cluster | domain | The case the fingerprint guards |
| 11 | Ghosts steady-state at batch size, not cumulative | **A4 verification round** | §1. Verified this round — do not re-inherit revision 1's version |

### Inherited claims (standing question 5)

Claims 6, 7 and 11 come from prior reports rather than from this document's own reading. **Verify each against the case at hand, not merely that its source said it** — that is exactly how revision 1's claim 10 got through.

---

## 3. On-disk shape

Use a **DTO with explicit conversion**, not serde derives on the internal types.

`Layout`'s encapsulation is deliberate — its doc comments explain why transitions are computed rather than stored. Deriving on private fields couples the file format to internal representation, so a later refactor of `slots` becomes a format break. This is the first artifact in the workstream that **outlives the process and cannot be silently changed later.**

```rust
/// The persisted form. Deliberately flat and deliberately NOT `Layout` itself:
/// the internal `BTreeMap<SlotKey, SlotState>` stays free to change without
/// breaking a file already on disk.
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
    /// RFC3339, set when the slot vacates. Not used for reaping in A4 — see §1
    /// — but carried because A5's succession ageing wants it and adding it
    /// later would cost a format bump.
    vacated_at: Option<String>,
}
```

`Layout` needs an explicit `from_stored` constructor. Keep the fields private.

Mechanics follow `prefs.rs` exactly: atomic temp + rename, XDG path, and **a corrupt file degrades to an empty layout rather than failing to start.** A layout that cannot be read is a *fresh map*, not a crash — and it must say so, because the user will notice the world changed and deserves to know why.

**Storage:** one file per context under a layouts directory, not a section in `prefs.json`. Prefs are per-user and small; layouts are per-cluster and grow. Separate files also mean one corrupt layout does not take out the others.

---

## 4. Store identity, and a new read surface

**Key** = kubeconfig context name — human-meaningful, what `--context` takes and what appears on screen.
**Fingerprint** = a stable cluster-scoped UID, conventionally `kube-system`'s namespace UID. Per claim 9, call it a fingerprint; Kubernetes has no cluster ID.

### 4.1 The read surface is new, and this project's privilege posture is deliberate

`Namespace` is not in the watch set, so the fingerprint needs a one-shot read of a single named object. **Model it on `browse.rs`**, which is the existing on-demand read precedent.

Two requirements that follow from the posture rather than from convenience:

- **One object, by name, once per connection.** Not a namespace list, not a watch.
- **Failure is not fatal.** A cluster where the read is denied is common — and `browse.rs`'s own convention is to *report* what it could not enumerate rather than silently omitting it. Same here: an unreadable fingerprint means **unverified**, not broken.

### 4.2 The three cases

| Stored | Observed | Action |
|---|---|---|
| matches | matches | load |
| differs | — | **discard, fresh layout, declare it** |
| absent, or unreadable | either | load, record as **unverified** |

Refusing to load on an unreadable UID would break stability for the least-privileged clusters — which is the population that benefits most from it.

The mismatch case is the **migration cataclysm**, reassigned here from A2 per the decomposition's settled row: *"first run after A2 remakes the world once. Declare it."* It has no in-session meaning without persistence, and this is where it lands.

---

## 5. Compaction

One operation. Explicit, user-triggered, declared.

```
Reclaim all ghost ground. The map keeps the slots that are occupied.
```

- **Selects all ghosts**, regardless of age — the reason to reach for it is "I decommissioned that pool and I want the map to show it"
- **Recorded** with a date and a count
- **Never automatic**, and never silent

### 5.1 The invariant that could undo A1

> **Compaction reclaims ghost ground. It does NOT renumber surviving ordinals.**

"Compaction" invites the wrong reading. A reaped ghost leaves its ordinal unused; the survivors keep theirs. Renumbering would move live provinces, which is precisely what Workstream A exists to prevent — A4 would undo A1 by closing gaps.

Pin this with a test, not just a comment.

### 5.2 What is deferred, and the tripwire

No automatic reap, no retention setting, no window. If shrinkage ever produces a ghost population that bothers someone in practice, the age machinery is a follow-on — and `vacated_at` is already in the format, so it needs no migration.

The signal to revisit: ghosts substantially exceeding the refresh batch size on a fleet nobody shrank. That is measurable with `--dump-positions`, which already records ghost slots.

---

## 6. What A4 does not do

- No succession or cataclysm **rendering** — A5 owns the vocabulary; A4 records events
- No automatic reap (§1)
- No renumbering (§5.1)
- No graticule — A6
- No change to `assign_layout`'s algorithm beyond carrying `vacated_at`

---

## 7. Tests

**Round trip:**
- [ ] Save, load, equal — ghosts, `last_occupant`, `vacated_at`, zone ordinals all preserved
- [ ] Corrupt file loads as empty and reports it
- [ ] A future `version` is refused gracefully

**Identity:**
- [ ] Matching fingerprint loads
- [ ] Mismatch discards, starts fresh, records the event
- [ ] Absent fingerprint loads as unverified
- [ ] **Unreadable** (RBAC-denied) fingerprint loads as unverified, not as a mismatch — the distinction §4.2 rests on

**Compaction:**
- [ ] Removes all ghosts
- [ ] **Does not renumber surviving ordinals** — the §5.1 invariant
- [ ] Records an event
- [ ] Compacting a layout with no ghosts is a no-op that says so, rather than a silent success

**Restart (the gate, as a unit test):**
- [ ] Persist, load into a fresh `Layout`, assign against the same nodes → every occupied slot keeps its coordinates
- [ ] Same with a rolling refresh between save and load → survivors hold, replacements reclaim their predecessors' ground

**Ghost steady state — pins §1's corrected claim:**
- [ ] Four successive batched refreshes of ten nodes on a hundred-node fleet leave **ten** ghosts, not forty

**Mutation floor:** make the load path return `Layout::default()` and confirm the restart test fails. **Run the gate before and after the mutation**, not merely write it — per the instrument pattern in §9.

---

## 8. The gate

**Open, close, reopen. Is it the same map?**

Positional, not pixel — the measurement session settled that pixels cannot see assignment. Dump with `--dump-positions` on both sides and compare with `positions.py`: every city HELD, every province in place.

Then the harder case: refresh the fleet, close, reopen. Survivors hold; replacements sit on their predecessors' ground.

### 8.1 What this gate does and does not settle

The kill point has moved twice already — decomposition put it at A2, A2's report moved it to A3, revision 1 argued for A4. **Stop moving it.**

The honest framing: **plan §1's spatial-memory thesis is not testable by any single gate.** A1–A3 proved the map *can* hold still; A4 proves it holds still across sessions. Whether that produces spatial memory in a user is answerable only by someone living with it for weeks.

A4's gate is real and binary — same map after restart, pass or fail. Report it as that, and do not promise a verdict on the thesis it cannot deliver.

---

## 9. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. **Which claims here were inherited from a prior report rather than verified against the case at hand?**

Question 5 stopped revision 1 at §0 on its first outing. The sharpened form, from that round: *an inherited claim needs re-verification against the case at hand, not confirmation that its source said it.* Revision 1's claim 10 was a true measurement wrongly generalised.

Question 2 applies to `vacated_at`: an absent timestamp on a ghost — from an older file or a bug — is **unknown**, not infinitely old. Since A4 does not reap, nothing depends on it yet; the guard matters for whoever adds ageing later.

---

## 10. Acceptance

- [ ] Layout persists per context; DTO with explicit conversion; `Layout`'s fields stay private
- [ ] Atomic write, XDG path, corrupt file degrades to empty **and says so**
- [ ] Fingerprint read is one object by name, once per connection, modelled on `browse.rs`
- [ ] Mismatch discards and declares; absent **or unreadable** loads as unverified
- [ ] Compaction reclaims all ghosts, records an event, **never renumbers**
- [ ] No automatic reap; `vacated_at` carried in the format
- [ ] Ghost steady-state test present
- [ ] Gate run positionally, both restart cases; mutation floor exercised, not just written
- [ ] Standing questions answered, including question 5 with sources tagged
- [ ] `cargo nextest` green

---

## 11. Estimate

**One to two days.** The DTO and round trip are straightforward; identity handling carries the design risk, and the fingerprint read is a new surface on a project whose privilege posture is deliberate — that deserves review attention out of proportion to its size.

Dropping the automatic reap removes roughly a third of revision 1's scope.
