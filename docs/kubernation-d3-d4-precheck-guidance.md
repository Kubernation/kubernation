# KuberNation — D3/D4 Pre-check (revision 2)

**Measurement guidance**
**Goal:** find out how much of D3 and D4 already exists, before either is scoped.
**No product change.** The output is an inventory and, most likely, two smaller phases.

> **Revision 2.** v1.24.0 landed D2's brushing — the workload table now marks the
> map's selection and vice versa — which changes §2's and §3's premises again.
> Folded in below. Revision 1 was written before it existed.

---

## 0. Why a pre-check

D1, D2 and D2-brushing changed the ground both phases were scoped against.

**D1 docked the drill-down.** The map is now visible while a panel is open — which was D3's stated problem.

**D2 made the selection an identity**, derived per frame, and fixed `province_pos` pointing into the sea. So the map now marks the current subject correctly, from any writer — which is a large part of what D3 and D4 were each supposed to deliver.

**D2-brushing (v1.24.0) connected the workload table in both directions**, and — more usefully for this pre-check — **established that most lists have no entity rows at all**. Charter rows are capability probes; Advisors are text lines; IMPACT lists a subject's *dependents*, so the subject is never in it. The enabling plan's "map, Annals, workload table, Charter" was a list of views, not a list of things with map-locatable rows.

That finding does most of §2's work in advance. What remains is to confirm it and fill the gaps.

This is the move that has correctly shrunk or killed four phases in this project: A3 went from "city slots" to two lines, T2 died before rendering work, D2's free version was refuted by measurement, and D2's own gate stopped the phase at the half-day mark. **Check whether the cheap thing already covers it.**

---

## 1. What the plan asked for

| | Enabling plan §5 |
|---|---|
| **D3 — visual momentum** | *"While the user works in a list or panel, mark where they are on the map."* Wickens' mechanism for preserving context during local exploration |
| **D4 — reverse indexing** | *"Lists point back at the map: selecting a row flies the camera and marks the province."* Civ's advisors as an index *into* the world |

Both were written before D1 and D2 existed.

---

## 2. D4 — narrower than the plan describes

D2's report §8 says the writers that flew before still fly: `N`, `]`/`[`, the dev flags, the almanac. And D2's inversion made the marking correct.

So D4's two halves — **fly** and **mark** — may both already be present for the writers that have them.

**Enumerate, per surface.** D2-brushing §1 already settled the right-hand column for five of these; confirm rather than re-derive, and fill the rest.

| Surface | Flies? | Marks? | Entity rows? |
|---|---|---|---|
| Concern queue (`N`, sidebar) | | | **yes** — already writes the selection via `focus_concern` |
| Workloads table | | | **yes** — brushed both ways in v1.24.0 |
| Annals rows | | | **deferred** — stringly `(ns, name, kind)` triple; needs a validator |
| Charter (RBAC) rows | | | **no** — a row is a capability probe |
| Advisors | | | **no** — text lines, no row variant |
| IMPACT (sidebar) | | | **no** — lists dependents; the subject is never in it |
| Almanac cross-references | | | positional `Locate(cell)` — §2.1 |
| Oracle output | | | ? |
| SELECTION box | | | ? |

**So D4's remaining scope is narrow by construction.** Four surfaces have no entity rows and never will; two are already connected. What is left:

- **Does a connected surface fly?** D2-brushing §2 says clicking a workload row deliberately does *not* move the camera — marking is not navigation. **That was the correct call for D2 and it is exactly what D4 is.** So the question is whether it should now, and that is a decision, not a gap
- **The almanac's positional path** (§2.1)
- **Oracle output and the SELECTION box**, both unchecked

### 2.1 The known regression is D4-shaped

D2 §5.1 records that an almanac cross-reference to a harbour, gate or structure now **flies without marking** — a capability that worked before the inversion and does not now.

That is squarely D4's territory, it is small and concrete, and it should be in the inventory rather than tracked separately.

---

## 3. D3 — the premise needs testing

D3's stated problem was that working in a list or panel loses your place on the map. **D1 and D2 between them may have solved it**, in which case D3's remaining value is confined to a narrower case than the plan describes.

The question to answer by looking, not by reasoning:

> With a panel docked and a subject selected, **is anything still lost while working in a list?**

Candidates for what might remain:

- **Scrolling a list.** The selection marks the *selected* row's subject. A row the pointer is merely over is `hovered` — check whether that propagates to the map, and whether it should. **D2-brushing §8 records that hover deliberately does not propagate as commit**, and D2's own failure criteria named a strobing map. So this is a decision with a known hazard, not an oversight
- **The four surfaces with no entity rows** (§2). Scrolling the Charter or the Advisors has no map correlate and cannot be given one. If D3's value is concentrated there, D3 is not buildable — which is a finding
- **The panel's own internal lists.** CITIZENS, GARRISON — scrolling those inside a docked panel, does the map say which row you are on? These are *pods*, not workloads, and a pod's position is its node's province. **This is the case least covered by anything that has shipped**, and the most likely place D3 has residual value
- **The Annals**, whose deferral (§2) is about *selection*, not about marking. A row could conceivably mark without being selectable

**If the answer is "nothing is lost," say so and close D3.** That is a legitimate outcome and it is what the pre-check is for.

---

## 4. Method

**Look at the running app.** This is a behavioural inventory, not a source audit — though source will be needed to answer "why not" once a gap is found.

**Do not build an instrument.** Twelve instrument failures in this workstream, the most recent being a gate that reported green on a leftover cluster. An inventory of what a surface does needs a person driving the app, not a comparator.

**Use the churn fleet or kind, whichever has the surface populated.** Several of these lists are empty on a quiet cluster, and an empty list cannot demonstrate whether scrolling it marks anything.

**Record what was checked and what could not be**, in the shape D2-pre used: a surface whose list could not be populated is *unmeasured*, not *absent*.

**When something does not happen, read the path rather than inferring why.** Three instances across the last two rounds — a dev-flag fix added to dead code, a control-flow gate misread twice, and an error message diagnosed as a version incompatibility when a container runtime was simply stopped. An inventory is exactly the activity where "it doesn't mark, presumably because X" is tempting and wrong.

---

## 5. What this decides

| Finding | Consequence |
|---|---|
| D4's surfaces mostly fly and mark | D4 shrinks to the gaps plus §2.1's regression |
| Some surfaces have no reverse index | Those are D4, scoped by the inventory |
| Nothing is lost while working in a list | **Close D3.** Record why, so it is not re-proposed |
| Something is lost, narrowly | D3 is scoped to that, not to the plan's description |
| Hover propagation is the gap | **Weigh it carefully** — D2 rejected a strobing map as a failure criterion, and D3 must not reintroduce it |
| The panel's own lists are the gap | D3 is scoped to pod-level marking inside a docked panel — the case nothing has covered |
| D4's only question is "should a row click fly?" | That is a decision, not a phase. Decide it and close D4 |

---

## 6. Standing questions

Two apply; the rest do not, since no code changes.

**5. Which claims are inherited rather than verified?** All of §0 and §2's premises come from the D1, D2, D2-fix, D2-inversion and D2-brushing reports — five documents, all from the same week. D2-fix found a claim *hours old* that had gone stale, and D2-brushing found that the enabling plan's four-view scope was two views once the row types were checked. **Recency is not verification, and neither is a plan's own prose.**

**2. Unknown, or fabricated?** A surface that could not be populated is **unmeasured**. Reporting it as "does not mark" would be the same fabrication the instruments have been hardened against.

---

## 7. Acceptance

- [ ] §2's table filled for every surface, including the ones with no reverse index
- [ ] §2.1's almanac regression placed in the inventory
- [ ] §3 answered by looking: is anything lost while working in a list?
- [ ] Surfaces that could not be populated recorded as unmeasured
- [ ] D3 either scoped to a specific narrow case, or **closed with a reason**
- [ ] D4 scoped to the gaps found, not to the plan's original description
- [ ] No product code changed, no instrument built

---

## 8. What this must not do

**No D3 or D4 implementation.** The point is to find out what they are.

**No hover propagation experiments.** If that is the gap, it is a design decision with a stated failure mode, not something to try in a pre-check.

**No fixing the almanac regression here.** Record it; fix it in the phase it belongs to.

---

## 9. Estimate

**One to two hours** — shorter than revision 1, because D2-brushing settled five of the nine surfaces.

The likely outcome, on this project's record: **D4 reduces to a single decision** (should a row click fly?) **and D3 either closes or scopes to the panel's own pod lists** — the one case nothing shipped so far has touched.
