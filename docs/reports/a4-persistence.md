# A4 — Persistence

**Report** · 2026-08-03 · **v1.9.0**
**Governing doc:** [`kubernation-a4-persistence-guidance-rev2.md`](../kubernation-a4-persistence-guidance-rev2.md)
**Supersedes:** [the revision-1 verification](a4-verification.md), stopped at §0

---

## The gate

> **Open, close, reopen. Is it the same map?**
>
> **Yes — and the headline formulation of that question cannot tell.**

Measured on the 100-node churn fleet, closing the app, refreshing thirty nodes
while it was shut, and reopening:

| | surviving provinces | holding their exact ground |
|---|---|---|
| **with the saved map** | 70 | **70 (100%)** |
| with no saved map | 70 | 55 (78%) |

Cities: **13 of 14 HELD**, one `FOLLOWED` — a workload whose pods genuinely
moved, which is correct behaviour and not what A3 fixed. Zero `MOVED-WITHIN`.

### The gate as stated does not discriminate

§8 frames the primary gate as *open, close, reopen on an unchanged fleet*, with
the refresh case as "then the harder version". The first is not merely easier —
**it is vacuous.** Run on a static fleet it reports every city HELD and every
province in place *with the layout file deleted*, because assignment from
scratch is deterministic in the node set. I ran exactly that and got a perfect
score from a build with no persistence in play at all.

This is the third appearance of one blind spot in this workstream:

| | measured | actually |
|---|---|---|
| A2's flipbook | a frame per process | blind to the layout carry |
| A4's restart unit tests, first attempt | single-shot fixture | mutation floor passed straight through |
| A4's gate as specified | unchanged fleet | passes with the file deleted |

Every time, the cause is the same: **assignment from scratch is deterministic,
so any test whose fixture has no history cannot tell a restored layout from a
rebuilt one.** Only a difference between sessions — a refresh, a departure —
makes persistence observable.

The number above is therefore reported *with* its discrimination check, which is
what makes 100% mean something rather than being the same 100% a broken build
would score.

---

## 1. Verification — all eleven §2 claims TRUE

Including the three tagged inherited, each checked against the case at hand
rather than against its source.

| # | Claim | |
|---|---|---|
| 1–4 | `Layout` private, no serde derives; `SlotState`; `SlotKey`; `ghosts()` | ✅ |
| 5 | `prefs.rs` atomic, XDG-aware, versioned, degrades | ✅ |
| **6** | `Namespace` **not watched** — the fingerprint needs a new read | ✅ *inherited* |
| **7** | `browse.rs` reports what it could not enumerate | ✅ *inherited* |
| 8 | `build_with` / `build_carrying` | ✅ |
| 9–10 | no native cluster ID; a context can be re-pointed | ✅ domain |
| **11** | ghosts steady-state at batch size | ✅ *inherited*, re-measured |

**Claim 6 sits next to a trap.** `ObservedWorld::namespaces()` exists, and reads
as though namespaces were observed. It is not: it derives namespace *names* from
the metadata of the thirteen watched kinds and never reads a Namespace object,
so it cannot yield a UID. The claim holds, and it holds for a reason worth
writing down.

**Claim 7 held exactly as stated** — `Discovered { kinds, warnings }`, with
`warn_skip` collecting what could not be enumerated. A closer precedent turned
up for the other half of §4.1's requirement: `logs::first_container` is a single
named `api.get` degrading to `None`, which is the shape the fingerprint read
takes.

**Claim 11 was re-measured rather than accepted**, and is now a committed test:
four successive batched refreshes of ten nodes on a hundred-node fleet leave ten
ghosts each time, and a genuine scale-down to eighty leaves thirty.

---

## 2. What was built

**`state/layout_store.rs`** — the DTO and conversions, pure, no filesystem.
**`layout_io.rs`** — the only file that touches disk for layouts.
**`k8s/fingerprint.rs`** — the one-shot identity read.

The pure/impure split mirrors `state/oracle_config.rs` + `oracle_config_io.rs`,
which means the round trip and every identity rule are testable without a temp
directory, and the format cannot quietly acquire an I/O dependency.

Layouts live under `~/.local/state/kubernation/layouts/`, one file per context —
**state, not config**. A layout is derived fact about a cluster rather than a
user-authored preference, and `logging.rs` already established that directory
for exactly this distinction.

Context names are sanitised into a single path component. An EKS context is a
full ARN with slashes and colons; joined naively it would create directories or
escape the layouts directory, and there is a test for both.

### The fingerprint, and the read surface it adds

One object, by name, once per connection — not a list, not a watch. Failure is
not fatal and is *reported*: 403, 404 and other errors are classified into
something an operator can act on, and an unreadable fingerprint yields
**unverified**, never **mismatched**.

That distinction is the one §4.2 rests on, and it is load-bearing: conflating
them would discard a working map every time an RBAC-restricted user opened it —
the population stability helps most. It has its own test.

### Compaction

Explicit, user-triggered, reported, and **it never renumbers**. A reclaimed
ordinal is left unused. The test constructs interior ghosts and asserts the
survivors' ordinals are unchanged; mutating `compact` to close the gaps fails
it, which is the point — renumbering would move live provinces and A4 would
undo A1.

Compacting with nothing to reclaim reports zero rather than succeeding silently,
because the caller says so to a user.

### `vacated_at`

Carried, stamped, cleared on re-occupation — and read by nothing. There is no
automatic reap, per §1. `assign_layout` stays clockless: it *carries* the
timestamp across rebuilds and *clears* it on re-occupation, but stamping is a
separate `stamp_vacancies(now)` the caller drives, matching how
`attention::build` and `build_timeline` take `now`.

Stamping only unstamped vacancies is what makes the value mean "how long has
this vacancy stood" rather than restarting every tick — pinned by test, because
nothing reads it yet and a silent regression would only surface when A5 needs it.

---

## 3. Standing questions

**1. Summing before comparing.** In the gate: provinces "holding their ground"
is a count over the *intersection* of two sessions, not over either session's
total — a refresh changes the node set, so the wrong denominator would report
78% as something else entirely. The comparison prints the intersection size.

**2. Reducers over possibly-empty input.** Three, all expressing unknown:
a missing layout file is `Fresh` (not an error, not an empty map presented as
restored); an unreadable fingerprint is `Unavailable(why)` whose `value()` is
`None`, which reaches the identity check as *absent* and never as `Some("")`
that would read as a mismatch; and `vacated_at` absent is unknown, never
infinitely old — the guard matters only for whoever adds ageing, and is
documented at the field for them.

**3. Two sections constraining the same behaviour.** §5 says compaction "selects
all ghosts", §5.1 says it must not renumber. They diverge on a layout with
*interior* ghosts — reclaiming them is exactly when closing the gaps is
tempting — and that is the fixture the test uses.

**4. Consumers of a redefined value.** `SlotState` gained a field, so every
literal construction had to be revisited; the compiler found them. The subtler
one is `Occupancy::pool_source`, which is deliberately **not persisted**: it
records how a live node's pool was read, so restoring it would let a stale
answer outlive the labels it came from. It is re-derived on the next
observation.

**5. Inherited claims.** Three, tagged in §2's third table, all verified
independently — see §1. Claim 11 was re-measured as a committed test rather than
accepted from my own prior report, which is the discipline the question exists
to enforce.

---

## 4. Acceptance

| §10 criterion | Status |
|---|---|
| Persists per context; DTO with explicit conversion; fields stay private | ✅ |
| Atomic write, XDG path, corrupt degrades to empty **and says so** | ✅ renamed aside, never deleted |
| Fingerprint read is one object by name, once per connection | ✅ |
| Mismatch discards and declares; absent **or unreadable** loads unverified | ✅ |
| Compaction reclaims all ghosts, records an event, **never renumbers** | ✅ mutation-verified |
| No automatic reap; `vacated_at` carried | ✅ |
| Ghost steady-state test present | ✅ |
| Gate run positionally, both restart cases; **mutation floor exercised** | ✅ and it changed the tests — §5 |
| Standing questions answered, question 5 with sources tagged | ✅ |
| `cargo nextest` green | ✅ 419 core + 94 GUI |

---

## 5. The mutation floor changed the work

§10 asks for it to be *exercised, not merely written*, and doing so is what
found the defect in this phase.

Run against the first version of the restart tests, the mutation — make the load
path return an empty layout — **passed straight through both of them.** They
used single-shot fixtures, so a rebuild produced the same map and the tests
proved nothing.

The fix took two attempts, and the first failed too. Growing the fleet
node-by-node is not enough: whether arrival order differs from hash order is
luck, and the guard caught it. A *departure leaving an interior gap* is not luck
— a rebuild has no reason to skip an ordinal — but which ordinal a departing
node holds is itself hash order, so the candidate is now searched for and the
fixture fails loudly if none is found.

Both restart tests now carry a **guard-the-guard** assertion: if a from-scratch
assignment already matches, the test declares itself non-discriminating rather
than passing. With that in place the mutation fails four tests instead of zero.

---

## 6. Decisions for the room

### The kill point: §8.1 says stop moving it, and this report does

A4's gate is real and binary and it passed. Whether spatial memory actually
accrues in a user is answerable only by someone living with this for weeks, and
no gate in this workstream can deliver that verdict. Reported as what it is.

### The static gate should be struck from the method

Not just noted — **removed**, or it will be run again by someone who reasonably
trusts the document. The refresh case is the gate; the unchanged-fleet case
gives a perfect score to a build with the feature deleted.

Generalised: **any future gate here needs a discrimination check** — run it
against a build with the mechanism disabled and confirm the number moves. That
is now three phases where the absence of one would have published a meaningless
result.

### Warm clusters do not persist

Deliberate and worth confirming: the warm cluster is fixed at launch and is a
comparison view rather than somewhere the operator navigates, so it has no
spatial memory to accrue. One line to change if that reading is wrong.

### Save cadence is 5s, plus clean exit

The world loop saves on a cadence (covering a kill) and `main` saves on clean
exit (covering the ordinary close between two of those). The first version used
30s and lost the map when a session ended sooner — found by the gate, not by
reasoning.

Note the deliberate asymmetry: the clean-exit save is skipped under
`--screenshot`, the cadence save is not. A layout is a fact about where nodes
sit, so a capture computes the same one a normal run would; prefs are a user's
choice and a capture forcing `--overlay walls` must not persist it.

### Still open from earlier rounds

Unchanged and still unanswered — see [open-decisions.md](open-decisions.md).
The release drift is now **five** versions behind the last tag.
