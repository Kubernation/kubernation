# KuberNation — The Prose Audit

**Measurement guidance**
**Goal:** find the claims this product makes in words that the code no longer supports.
**Output:** an inventory, the corrections, and — where two prose claims describe one mechanism — one authority instead of two.

Do the license guard first (`handoff v1.32.0` §3, §4.5). It is the only red thing and it is a decision, not a puzzle.

---

## 0. Why this seam

Across the last two stretches, **prose was where the defects lived**:

| | |
|---|---|
| the field guide said a city sits where *most* of its pods are | it is the plurality — false whenever no node holds a majority, which is 7 of 7 eligible workloads at fleet scale |
| two almanac pages disagreed about the same mechanism | nothing compared them |
| `SidebarHit.focus_impact` documented a selection it deliberately does not make | went stale at D2's inversion, found two phases later |
| `--dump-positions` names a field `node` that is not the node a reader assumes | **still open** — and it misled this project's own measurement |
| the truncated selection line | rendered, and did not say what it meant |

**None was found by looking for it.** Each surfaced while doing something else, and three of the five were stale for phases rather than days.

The reason is structural: **prose makes claims and has no compiler.** A doc comment describing removed behaviour compiles. A field guide contradicting itself compiles. A JSON field named for the wrong thing compiles, and other tools parse it happily.

---

## 1. The claims check — new, and it applies to this audit itself

§5 of the v1.32.0 handoff found a failure mode that verifying claims one at a time cannot catch:

> **Two guidance claims were true and insufficient.** `selector_matches` has the right expression semantics *and the opposite null case*; `NodeTile.pods` exists *and carries no labels*.

Both accurate. Both omitted the thing that mattered for the use they were put to.

**So, as a standing check from here on:**

> **For each claim, state what you intend to do with it, and ask whether the claim licenses that.**

"Reuse `selector_matches`" needs more than "it does selectors correctly." A claim is verified when it is true *and* sufficient for its use — and the second half has to be asked separately, because the first half passing is what hides it.

Apply it to §3's inventory: a doc comment can be *true* about what a function does and *insufficient* about what a caller may conclude.

---

## 2. What counts as a claim-bearing surface

Not all prose. A comment explaining an algorithm is not making a claim to a user. **A claim-bearing surface is one where a reader — human or machine — will act on the words.**

Four kinds:

| | Read by | Goes stale when |
|---|---|---|
| **Field guide / Almanac** | operators | a mechanism changes |
| **Panel and concern wording** | operators | the underlying computation changes |
| **Doc comments on shared authorities** | the next editor | a consumer or the behaviour changes |
| **Dev-instrument vocabulary** — field names, flag help, output labels | this project's own measurements | the meaning shifts under a stable name |

The fourth is the one that has actually cost the most: `--dump-positions`' `node` field is what led to the plurality item, and the pre-check that found it *read the field as "where this workload runs"*.

---

## 3. The inventory

Enumerate and check each against the code it describes. **Read the code — do not infer it.** Four wrong diagnoses in recent rounds came from reasoning about a path rather than reading it.

### 3.1 Where to look hardest

**A behaviour change that did not sweep its own prose** is the pattern. The workstreams give the candidates:

- **A2–A6** — layout, stride, ghosts, graticule. `slot_row`/`slot_of_row`'s doc comments, extent wording, anything describing where a province sits
- **A5** — succession and ageing. `NewGround`'s modes, `GroundState`'s four states, the ageing window's wording
- **D1–D4** — docking, selection inversion, brushing. Anything describing what a click does, what is selected, what marks
- **T-fix / T-fix-2** — fault lines and correlation. The Annals' own explanatory text, and any comment describing the anchor or the suspect window
- **v1.26–v1.27** — plurality. Already swept once; check the sweep was complete
- **v1.30–v1.32** — PDB and eviction. **Newest, so most likely to have prose written against the pre-change behaviour.** The evict button's label and any help text describing what it does

### 3.2 The known-open item

`--dump-positions`' `node` field (handoff §4). Recorded rather than renamed last time because `positions.py` and its self-tests match the literal.

**This is the pass that should close it.** The precedent is `ExtentSource::Capacity` → `Allocatable` in v1.20.0: renamed at all six sites with the instrument updated in the same commit, on the reasoning that it gets more expensive once anyone scripts against the values.

Same category, same mechanics, and this one has already misled someone.

---

## 4. The fix shape

**Where two prose claims describe one mechanism, unify them.** Do not write a test that compares them.

`SITING_CLAIM` is the pattern: one constant, both pages built from it, and the test kept but sharpened to name the specific falsehood so it cannot return by paraphrase. That is the ninth instance of one-home-for-the-rule in this codebase and the first where it applied to words rather than code.

**Where a doc comment describes removed behaviour**, state what the code does *and why the omission is deliberate* — as `focus_impact`'s correction did. A bare removal reads as an oversight someone should tidy away.

**Where an instrument's vocabulary is wrong**, rename and update its consumers in the same commit. A script silently matching a string that no longer appears is the shape that produced the "1 of 8" figure.

---

## 5. Method

**No instrument.** Seventeen catalogued instrument failures, the most recent in a check *of a guard*. This is reading.

**Grep is a starting point, not the method.** A claim can be wrong without containing any keyword you would search for — the Legend's "most" is a case in point.

**Record unmeasured as unmeasured.** A surface that could not be rendered (the drain line needs a live blocking budget; the Oracle bundle needs an endpoint) is *unchecked*, not *correct*.

**Cap the scope.** This could run indefinitely. Take §3.1's list, work it, and stop — a second pass is cheaper than a first pass that never finishes.

---

## 6. Standing questions

Two apply directly; the eighth is new.

**8. Is each claim true *and* sufficient for what is being done with it?** (§1)

**5. Which claims are inherited rather than verified?** This audit's own §0 and §3.1 are inherited from reports. Recency is not verification — the claim most often wrong in this project has been the author's own, from a report written the same week.

**3. Where do two sections constrain the same behaviour?** That is §4's whole subject, applied to words.

---

## 7. Acceptance

- [ ] §3.1's surfaces enumerated and checked against source
- [ ] Every correction states what the code does, and why an omission is deliberate where one exists
- [ ] Where two claims described one mechanism, they are built from one authority (§4)
- [ ] `--dump-positions`' field renamed and its consumers updated in the same commit (§3.2)
- [ ] Surfaces that could not be rendered recorded as unchecked
- [ ] The claims check (§1) applied to this audit's own inherited claims
- [ ] `cargo nextest` green

---

## 8. What this is not

**Not a rewrite.** Correct what is false; do not improve what is merely terse.

**Not a comment sweep.** §2's four kinds only — a comment explaining an algorithm to its next editor is not in scope unless it describes behaviour that changed.

**Not the deferred grafts.** Advisors ▸ Substrate, P90 sizing, CNI probe, warm parity, Annals brushing — all separate.

---

## 9. Estimate

**Half a day.** Reading, plus one rename with its consumers. Longer if §3.1 turns up a cluster in one workstream, which would itself be a finding about where prose goes stale.
