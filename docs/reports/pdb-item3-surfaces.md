# PDB item 3 — surfacing the drain constraint

**Guidance:** `docs/kubernation-pdb-guidance.md` §4
**Follows:** `docs/reports/pdb-item2-observe.md`
**Version:** 1.32.0 · **Date:** 2026-08-28

The derivation reaches the operator: the attention queue, the province window,
the SELECTION box — and the Game Day scorecard stops claiming a drill succeeded
when a budget refused every step of it. **This closes the PDB guidance
(items 1–3).**

---

## 1. The queue — enrichment, not a new concern

§4 says "take the attention concern", and names `pool_confinement` as the
precedent. Read carefully, that precedent is **a fact appended to an existing
concern's `detail`**, not a concern of its own — and taking it literally is what
makes the feature safe.

**A concern per blocked node would squat the queue forever.** A budget written
`minAvailable: 3` on a three-replica workload sits at `disruptionsAllowed: 0`
permanently and *by design*; every node running that workload would carry a
standing alarm on a perfectly healthy cluster. That is the failure the hardening
round had to fix by excluding system namespaces — in a form no exclusion could
reach, because the workloads involved are the operator's own.

**So the note rides the cordon concern.** The queue answers *what needs orders*,
and a blocked drain is only an obstacle to someone draining; a cordon is the
observable signal that the operator is taking the node out of service. A blocked
budget on a node nobody is touching is a standing fact, which the province panel
reports unconditionally.

`pdb::drain_note` is silent for a drainable node — the `pool_line` / `extent_line`
rule that a caveat carrying no information is noise. That `None` arm turned out
to be the one thing the mutation floor could not see (§4).

Because it lands in `detail`, it rides the sidebar, the Oracle bundle and the
postmortem for free, exactly as §4 predicted — and §5 shows it doing so.

---

## 2. The panels — ungated, and shaped for the column

The province window and the SELECTION box show the constraint **whatever the map
overlay**, unlike the saturation / cost / substrate lines. There is no "drain
overlay" whose absence should hide it: what would refuse to give a node up is a
standing property of the machine, like its pool.

**The SELECTION line had to change shape, and the live capture is what showed
it.** The first version reused the concern's sentence:

```
drain: draining blocked by kubernation...     <- truncated at the column edge
```

The column is ~40 characters and `namespace/name` is the *end* of the sentence,
so right-truncation ate precisely the part the operator needs. This is the D1
finding that the IMPACT row front-loads its hop for. Now a header and an indented
name — the `substrate_gap_lines` shape, three lines away in the same file:

```
drain: blocked by
  kubernation-demo/web-strict
```

Pinned by a test asserting every row fits in 40 characters, so the next long
budget name fails a test rather than a screenshot.

The province window keeps the full sentence: it is a wide surface, and there the
truncation does not occur.

---

## 3. The drill verdict — the gap item 1 recorded

Item 1 noted honestly that `chaos_outcome_summary` describes the *cluster's
recovery*, so a drill whose evictions were all refused still read **"stayed up —
no outage"**. True, and readable as the workload shrugging the drill off rather
than as the drill never landing. That became reachable the moment eviction moved
to `pods/eviction`: a plain DELETE always landed.

`ChaosScorecard` now carries `steps_refused` / `steps_total`, taken from the run's
own per-step `CommitRow`s, and the verdict branches:

- **all refused** → *"no disruption landed — every step was refused"* (Warn).
  Reporting resilience here would be the drill taking credit for a disruption it
  never caused.
- **some refused** → the recovery verdict stands, above a line reading *"N of M
  steps refused — a partial drill"*: the experiment was smaller than the one
  requested, and the card says how much smaller.
- **none refused** → unchanged.
- **no recorded outcome** → `(0, 0)`, and the card says nothing rather than
  "0 of 0".

---

## 4. Mutation floor, asserted applied

| | mutation | |
|---|---|---|
| N1 | the cordoned node's concern stops naming the budget | caught |
| N2 | `drain_note` speaks for a drainable node too | **survived, then closed** |
| N3 | the SELECTION line goes silent | caught |
| N4 | the SELECTION line stops naming the budget | caught |
| N5 | a wholly refused drill claims the workload stayed up | caught |
| N6 | a node the report never examined reads as drainable | caught |

**N2 is the finding.** The `None` arm exists solely to keep noise out of the
queue, and every test aimed at the *presence* of the note — the anti-squatting
assertion was about an **un**cordoned node, which has no concern at all, so a
cordoned-but-drainable node could gain a useless line and nothing would notice.
The whole justification for the arm was untested. Closed by the case that was
missing: a cordoned node with nothing blocking it, whose concern must not mention
draining.

That is the same shape as item 2's M11 and, before it, D2's M-D: **the fixture
could express the positive case and not the negative one**, so the mutation floor
measured half the rule.

---

## 5. §6 — the gate, on real surfaces

**Failure criteria, stated first:** a blocked-but-untouched node squatting the
queue; the concern or panel saying "blocked" without naming the budget; the
SELECTION box truncating the name away; a refused drill still reading as
resilience.

Fixture: `web-strict` (`minAvailable: 3` on 3 replicas → `disruptionsAllowed: 0`)
and `kubernation-worker` cordoned.

**The queue, read from the postmortem export** — a surface the operator actually
reads, which also demonstrates §4's "rides the sidebar, Oracle bundle and
postmortem for free":

```
- **· node kubernation-worker — cordoned** — zone z-a · 7 pods · cpu 0% mem 2%
  · draining blocked by kubernation-demo/web-strict
  - next: B: blast radius · T: what changed · click: open the province
```

**Exactly one mention.** `kubernation-worker2` also runs a covered web pod and is
equally blocked, but is not cordoned — and the queue says nothing about it. The
anti-squatting rule, live.

**The province window and the SELECTION box**, captured together: the status band
reads `drain: draining blocked by kubernation-demo/web-strict` in refusal red
beside `strain: calm · pods 7/110`, and the column reads `drain: blocked by` /
`  kubernation-demo/web-strict` in full.

The truncation criterion was the one that fired — on the first capture, before the
reshape. Stating the criteria in advance is what made it a finding rather than a
detail I looked past.

Cluster left as found: budget deleted, node uncordoned, all four nodes Ready.

---

## 6. §7 — standing questions

**1. Summing before comparing?** `steps_refused == steps_total` is a comparison of
two counts of the same rows, not a sum of unlike things. Guarded on
`steps_total > 0` so an absent outcome does not satisfy it vacuously — the
`next_ordinal` shape.

**2. Unknown, or fabricated?** Three places. `drain_gap_lines` returns nothing for
a node the report never examined, rather than "drainable" (N6). The scorecard's
`(0, 0)` says nothing rather than "0 of 0 refused". And an unread budget set earns
a row in both panels rather than silence — silence there would read as "nothing is
stopping you", which is the whole §3.3 constraint arriving at the surface.

**3. Two sections constraining one behaviour?** Yes, deliberately, and they
diverge on purpose: the queue speaks only for a cordoned node, the panels speak
unconditionally. Both call the same `DrainReport`, so they cannot disagree about
the *fact*; they differ about when it is worth saying, and each carries its
reason in its doc comment. The divergence is asserted by test in both directions
(the queue silent for an uncordoned blocked node, the panel not).

**4. Consumers depending on the old meaning?** `attention::build` gained a
parameter, so the compiler enumerated its call sites — one production, four test.
`ChaosScorecard` gained two fields, likewise.

**5. Inherited claims?** §4's recommendation to "take the attention concern",
which I read as a new concern before re-reading the precedent it names. The
precedent is enrichment, and following the words rather than the example would
have produced the squatting queue.

**6. One side of a comparison moved?** Item 1 moved eviction, which is what made
a chaos step refusable — so the scorecard's "did the workload stay up" was being
compared against a drill that might never have run. §3 is that repair.

**7. Container adjacency?** None here.

---

## 7. Acceptance — the guidance, complete

- [x] Eviction uses `pods/eviction`; a PDB refusal is a named refusal (item 1)
- [x] The chaos drill's partial-failure behaviour decided, recorded — and now
      reported honestly in the verdict (item 1 §2.2, closed here)
- [x] PDB watched; the RBAC verb declared in the Charter (item 2)
- [x] `selector_matches` reused, not reimplemented (item 2)
- [x] Derivation cost measured against the rebuild budget (item 2 §4)
- [x] Unprotected and unknown distinguishable everywhere — now including both
      panels and the concern
- [x] Gate run on kind, with the discrimination checks
- [x] Failure criteria stated before the run
- [x] Mutations asserted applied; the survival reported and closed
- [x] Standing questions answered
- [x] `cargo test --workspace` green

453 core (478 with the `oracle` feature) + 143 GUI tests; gui-smoke 57; clippy
clean with and without features.

**Deferred, with reasons.** A map mark for blocked nodes — *can this node be
drained* is a question about one node, not a fleet pattern, and the map is not
short of ink (§4's own call). The workload surfaces (*protected, and by how
much*) — honest, and a different feature. A gui-smoke state for the drain line —
it renders only when a budget blocks, which needs a live fixture the crash gate
does not have.
