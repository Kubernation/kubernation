# D3/D4 pre-check — what already exists

**Guidance:** `docs/kubernation-d3-d4-precheck-guidance.md` (revision 2)
**Date:** 2026-08-19 · **No product change, no instrument built.**

**D3 as written is closed** — its stated problem was solved by D1 and D2, verified
on the running app. **D4 reduces to one decision plus two concrete gaps**, one of
which nobody had listed.

And the pre-check turned up something neither phase describes, larger than
either: **a city is a poor summary of where a spread workload actually runs**, and
the app already holds the data to say so. Measured — the city representing
`churn/api` sits on a node holding **4% of its 120 pods**.

---

## 1. §2 — the table, filled

Determined from source, because "does this surface fly / mark" is a code fact:
a handler either calls `cam.fly_to`/`jump_to` or it does not, and either writes
`selected` or does not. Every camera move and every selection write in the crate
was enumerated and attributed.

| Surface | Flies? | Marks? | Entity rows? |
|---|---|---|---|
| Concern queue (`N`, sidebar row) | **yes** `main.rs:666` | **yes** `:671` | yes |
| Workloads table | **no** — deliberate | **yes** `:3401` | yes (v1.24.0) |
| **Oracle CONSULT NEXT** | **no** | **no** | **yes** — §1.2 |
| Almanac cross-references | **yes** `:3269` | **partial** `:3275` — §1.3 | positional |
| IMPACT (sidebar) | **yes** `:2706` | **no** — deliberate | dependents, not the subject |
| Annals rows | — | — | no click handler at all |
| Charter rows | — | — | no — capability probes |
| Advisors | — | — | no — text lines |
| SELECTION box | — | — | not a list; `SidebarHit` has no variant for it |

### 1.1 The two connected surfaces are exact inverses

**IMPACT flies but does not mark. The workload table marks but does not fly.**

Both are deliberate and for different reasons. IMPACT must not touch `selected`
because the blast subject is re-derived from it each frame, so selecting a
dependent would silently re-root the radius — D1's review found that and the
constraint is expressed as "does not set `selected`". The table does not fly
because marking is not navigation — D2-brushing's §5 line, which is D4's
question verbatim.

So D4 is not a missing capability. It is one decision — *should acting in a list
move the camera?* — applied to two surfaces that each already made the opposite
call for a stated reason.

### 1.2 The gap nobody had listed

§2 left the Oracle's output as `?`. It is the **only surface with genuine entity
rows that neither flies nor marks**. A CONSULT NEXT link is a validated
`Scope::{Workload, Node}` — the same identity the selection holds — and
`oracle.rs:774 jump_to_scope` moves the Oracle's *own* scope cursor, re-seeds
deepen and requests a consult. It never touches `selected` or the camera.

D2's report recorded that as intentional ("`self.map` `selected` is untouched"),
and in context it was: the phase was about the consult, not the map. But it
leaves the app's most explicitly identity-carrying list disconnected from the map.

### 1.3 §2.1's regression, confirmed

An almanac cross-reference resolves through `draw::selection_at`, which returns
`None` for a coast marker, a gate or an island structure. Those cross-references
therefore **fly and mark nothing**. City and Node cross-references mark
correctly (and, since v1.23.1, land on land rather than on water).

### 1.4 A stale doc, recorded not fixed

`sidebar.rs:56` documents `SidebarHit.focus_impact` as *"fly to + **select** this
(local) cell"*. The select was deliberately removed by D2's inversion. The
comment now says the opposite of what the code does, which is exactly what would
mislead the next reader into believing IMPACT marks. Left in place per §8; it
belongs to the D4 phase.

---

## 2. §3 — is anything lost while working in a list?

**No. Verified on the running app, at a settled camera.**

With `--inspect web` on the kind cluster: the drill-down docks to the right of a
visible map strip, the strip shows the `web` city, and the SELECTION box reads
`web / deploy kubernation-demo . pop 3/3 / grid B0`. Three frames at three-second
intervals confirm the framing is settled rather than mid-flight — a single-shot
capture catches `aim_for_drilldown`'s lerp in progress and would have shown the
subject somewhere else.

**D3's stated problem — "working in a list or panel loses your place on the map"
— was solved by D1 (docking) and D2 (a correct, identity-derived mark).** That is
§5's "close D3" row, and it is the honest outcome.

One legibility observation, not D3: the selection diamond is drawn in the terrain
pass and a neighbouring city's name banner in the feature pass, so a marker can
sit partly under a nearby label.

### 2.1 What could not be driven, and is therefore unmeasured

- **Hover and scroll inside any list.** `--screenshot` cannot place a pointer.
  Answered from source instead — no list handler writes `selected` or moves the
  camera on hover — which is definitive for *whether* it happens and says nothing
  about how it reads in use.
- **The Oracle's CONSULT NEXT links** were not clicked live (that needs an LLM
  endpoint); `jump_to_scope` was read instead.
- **The Annals rows** have no click handler, so there was nothing to drive.

Recorded as unmeasured rather than absent, per §6.

---

## 3. The finding neither phase describes

The city window's CITIZENS list carries `CityPod.node` for every pod. **`city.rs`
renders it nowhere** — zero occurrences of `node` in the file. And a city is
sited at its pods' **plurality** node.

Measured, on the churn fleet:

```
api     120 pods / 65 nodes — the city sits on the plurality node, holding   5/120 =  4%
cache    24 pods / 20 nodes —                                                3/24  = 12%
batch    12 pods / 12 nodes —                                                1/12  =  8%
```

On kind, smaller but present: `db` spans 2 nodes, `agent` 3.

So for a spread workload **the settlement on the map represents a few percent of
where it actually runs**, the panel lists every pod without saying where any of
them is, and the model already holds the answer.

That is not visual momentum. It is a question about **what the map can express**,
and it is the one thing this pre-check found that nothing shipped so far
addresses. Marking a hovered pod row's node would be a D3-shaped *mechanism* for
it, which is presumably why §3 flagged the panel's own lists as the likeliest
residue — but the underlying gap is larger than the mechanism, and it should be
scoped on its own terms rather than relabelled as D3.

**It is also a hazard, not a free win.** D2's failure criteria named a strobing
map, and per-pod marking on hover is exactly the shape that produces one. Whether
the answer is hover-marking, a node column in the row, or something on the map
itself is a design decision with a stated failure mode — §8's "no hover
propagation experiments" applies.

---

## 4. §5 — what this decides

| Finding | Consequence |
|---|---|
| Nothing is lost while working in a list (§2) | **D3 as written is CLOSED.** Its problem was solved by D1 + D2 |
| Two connected surfaces made opposite calls, each for a reason (§1.1) | **D4 is a decision**, not a capability gap |
| The Oracle's output has entity rows and does neither (§1.2) | A concrete D4 gap, previously unlisted |
| The almanac's coast/gate/structure cross-references fly without marking (§1.3) | The second D4 gap; small and concrete |
| Four surfaces have no entity rows | Not buildable, and correctly so |
| A city represents ~4% of a spread workload's pods (§3) | **A new item**, larger than D3 and not the same question |

**D4's scope**, if taken: decide whether a list click flies; wire the Oracle's
CONSULT NEXT; restore marking for almanac coast/structure references; fix the
stale `focus_impact` doc. Small.

**D3's scope**: none. Close it, with §2 as the reason, so it is not re-proposed
from the plan's original wording.

---

## 5. §6 — standing questions

**5. Inherited claims.** Every premise came from five reports written this week,
all mine. Each was re-derived from source rather than read back:

- "The writers that flew before still fly" — **true**, and now attributed
  line by line (§1).
- "Most lists have no entity rows" — **true**, and the table now includes the
  two the guidance left as `?`. One of them (Oracle) turned out to be a gap,
  which is what re-checking is for.
- "D2-brushing deliberately does not fly" — **true**, and it is the same
  decision IMPACT made in the opposite direction.
- The one claim that had gone stale was **a doc comment**, not a report: §1.4.

**2. Unknown or fabricated.** §2.1 lists what could not be driven. Hover
behaviour is reported as a source fact about *whether* anything fires, explicitly
not as an observation of how it reads.

---

## 6. §7 — acceptance

- [x] §2's table filled for every surface, including those with no reverse index
- [x] §2.1's almanac regression placed in the inventory (§1.3)
- [x] §3 answered by looking — settled camera, live cluster
- [x] Surfaces that could not be driven recorded as unmeasured (§2.1)
- [x] **D3 closed, with a reason**
- [x] D4 scoped to the gaps found, not the plan's description
- [x] No product code changed, no instrument built
