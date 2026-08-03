# A4 — verification, stopped at §0

**Verification report** · 2026-08-03 · **no code written**
**Governing doc:** [`kubernation-a4-persistence-guidance.md`](../kubernation-a4-persistence-guidance.md)

Stopped before implementation: §0 claim 10 is false, and it is the claim §4's
design rests on. Per the standing rule — *if a claim is false, stop and report
rather than adapting around it* — §4 needs revising before anything is built.

---

## The finding

> **Claim 10:** *"One refresh of N nodes produces N ghosts; accumulation rate is
> refresh cadence. A weekly refresh of 100 nodes is 100 ghosts/week."*

A 100-node fleet, refreshed in waves of ten — which is what a *rolling* refresh
is — measured through `assign_layout`:

```
after refresh 1: 100 occupied, 10 ghosts
after refresh 2: 100 occupied, 10 ghosts
after refresh 3: 100 occupied, 10 ghosts
after refresh 4: 100 occupied, 10 ghosts
after scale-down to 80: 80 occupied, 30 ghosts
```

**Ghosts reach a steady state at the batch size and stay there, regardless of
cadence.** The mechanism is A1's own REUSE rule: an unplaced node takes the
lowest-ordinal vacancy in its (zone, pool), so wave N+1's replacements reclaim
the ghosts wave N left. Nothing accumulates.

Batch size is what sets the standing count — 12 nodes refreshed in waves of 4
leaves 4 ghosts, in waves of 3 leaves 3, and in one wave leaves 12.

### What is affected

- **§0 claim 10.** True only for a single-wave full-fleet surge, where batch
  equals fleet. False for any batched refresh.
- **§4.2's sizing.** *"A weekly-refreshed 100-node fleet holds ~200 ghosts at 14
  days"* — it holds about ten, permanently. The 14-day default may still be a
  reasonable number, but the arithmetic offered as its justification is void,
  and §4.2 explicitly invites the reasoning to be argued with.
- **§4's age reap, in its motivation.** Refresh churn does not accumulate
  ghosts. What leaves lasting ghosts is **shrinkage** — the scale-down above, a
  decommissioned pool, a lost zone — and that is precisely the case §4 assigns
  to *compaction*, the explicit verb. The automatic verb is left addressing a
  problem the measurement says does not occur.
- **The acceptance list**, items 5–8, which are all reap behaviour.

### What is not affected

§2's DTO decision (verified: `Layout`'s fields are private with no serde
derives, so the on-disk shape is a real decision), §3's identity handling, the
round-trip and restart tests, and §7's gate. That is the bulk of A4 and none of
it depends on claim 10.

---

## §0 verification — claims 1 to 9 hold

| # | Claim | |
|---|---|---|
| 1 | `Layout`'s fields private, no serde derives | ✅ `slots`, `zone_ordinals`; derives are `Debug, Clone, Default, PartialEq, Eq` |
| 2 | `SlotState { occupant, last_occupant }` | ✅ |
| 3 | `SlotKey { zone, pool, ordinal }` | ✅ |
| 4 | `ghosts()` yields `occupant == None` | ✅ |
| 5 | `prefs.rs` atomic, XDG-aware, versioned, corrupt→fallback | ✅ temp + rename; `XDG_CONFIG_HOME` else `$HOME/.config`; corrupt renamed aside, never deleted |
| 6 | `PREFS_VERSION` with a bump-on-incompatible doc | ✅ |
| 7 | `build_with(world, filter, prior)`; `build_carrying` per world | ✅ |
| 8 | No native cluster ID; `kube-system` UID is a convention | ✅ and readable on both dev clusters |
| 9 | A context can be re-pointed at another cluster | ✅ |
| **10** | **One refresh of N nodes → N ghosts** | ❌ **see above** |

### Incidental, worth knowing before §3 is built

**`Namespace` is not watched anywhere.** The fingerprint therefore needs a new
one-shot read rather than a store lookup. `browse.rs` is the precedent — a
fetch-not-watch read on demand — and it is a small new read surface on a project
whose privilege posture is deliberate, so it belongs in the guidance rather than
being discovered mid-implementation.

---

## §8 question 5, on its first outing

The guidance added question 5 — *"which claims here were inherited from a prior
report rather than verified against code?"* — because A3 found three
requirements resting on a wrong caution in A3-pre's report.

**It caught one immediately, and it was the false claim.** Claim 10 traces to
A1's report §6: *"a 100-node surging refresh → 200 slots, 100 occupied, 100
ghosts."*

**A1's measurement was correct and correctly reported.** It describes a
single-wave full-fleet surge, which is what A1's synthetic test ran, and in that
case 100 nodes really do leave 100 ghosts. What went wrong is the
*generalisation*: from one synthetic single-wave test, to real refreshes (which
are batched), and then to accumulation over a weekly cadence. Neither step
survives contact with the REUSE rule.

That is a sharper lesson than "prior reports can be wrong". The report was
right; the claim derived from it was not. **An inherited claim needs
re-verification against the case at hand, not just confirmation that its source
said it.**

Two rounds running, the false claim in a guidance document was the inherited
one.

---

## For the revision

The narrow question is what the automatic reap is for, now that accumulation is
not the answer. Three shapes, in the order I would rank them:

1. **Compaction only.** Build persistence, identity and the explicit verb; leave
   the age reap until something demonstrates a need. This is §2.1's own
   principle from A3 — *do not build it speculatively, the measurement exists to
   size the problem first* — applied one section over.
2. **Both, retention defaulting to `0` (never).** The machinery exists, opt-in,
   and the doc is honest that the window is unsized.
3. **Both, keeping 14 days**, re-justified as bounding ghosts from *shrinkage*
   rather than from churn — which means a decommissioned pool's ground is
   reclaimed automatically after a fortnight, and that is a different promise
   from the one §4.2 currently makes.

Whichever is chosen, `vacated_at` is worth keeping in the format regardless:
A5's succession and ageing want it, and compaction can report how long ground
has stood empty.

One further question the measurement raises: **is a standing batch-size set of
ghosts even undesirable?** Ten reserved slots on a 100-node fleet is the
mechanism working — ground held for nodes that may return — and A2's gate found
that *painting* that ground is what made a refresh read as stable rather than as
the continent losing pieces of itself. Reaping it eagerly would undo that.
