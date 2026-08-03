# KuberNation — Measurement Session

**Implementation guidance**
**Goal:** turn the A2 gate's ad-hoc measurement into a committed instrument, quantify the axis A2 left unmeasured, and retire the outstanding baseline debt.
**Produces no product change.** Its output is a number and an instrument.

Governing docs: A2 implementation report §2 (method), §9 (decisions). Decomposition §4.

---

## 0. Why this is its own session

The A2 report asks for the A3-versus-A4 ordering to be decided on a measurement that does not exist yet:

> Provinces got four measurements and 0.41%; cities got one sentence.

Writing A3 guidance now would assume that answer. This session produces it.

It also retires two debts flagged as time-sensitive:

- **The comparator is not committed** — an ad-hoc script, which A2 §2 names as a prerequisite for A3's gate producing a number of the same kind
- **The v1.6.0 baseline was withdrawn and not re-taken** — still reconstructable, but only while the tag and harness are to hand

Doing all three together means the comparator gets exercised on two real questions rather than written speculatively.

---

## 1. Verify before building

### Structural

| # | Claim | Check |
|---|---|---|
| 1 | `a2-gate/` holds 6 committed frames from one session across a 30-node refresh | the directory |
| 2 | `gate.sh` exists in `hack/churn/` and refuses a no-op run (exit 2) | the script |
| 3 | `--shot-seq N` / `--shot-interval S` exist and produce numbered captures from **one** process | `Args` |
| 4 | The v1.6.0 tag exists and builds | `git tag`, then build it |

### Semantic

| # | Assumption | Why it matters |
|---|---|---|
| 5 | `--shot-seq` does **not** exist at v1.6.0 — it was built during A2 | The baseline needs it cherry-picked onto the tag, which is why this is reconstruction rather than a plain re-run |
| 6 | Settlement pixels are distinguishable from terrain by colour class | The whole city measurement depends on it. **Verify on a real frame before building the classifier** — if settlements do not separate cleanly, say so rather than reporting a number of unknown meaning |
| 7 | Runs chain: reserved ground accumulates, so a second run starts from a partly-grey map | A2 §2. Every capture set needs a reset and settle first |

**Claim 6 is the one that can invalidate the session.** If settlements cannot be classified reliably, the honest output is "cities are not measurable this way, here is what would be needed" — not a number with a caveat.

---

## 2. Commit the comparator

Move the ad-hoc script into `hack/churn/` beside `gate.sh`.

Parameterise what A2 hardcoded:

- **Play-area crop** — A2 used `x < width − 528`, `y > 60` to exclude the docked column, whose counters change every frame and would swamp the map. Keep the same crop, but as a flag rather than a constant.
- **Classifier** — A2 used `green > blue` for land, which covers terrain, sand and ghost grey but not sea. **Cities need a different classifier.** The comparator should take the classification as a selectable mode, not bake one in.
- **Frame pair** — explicit arguments, not positional assumptions.

Output the same four buckets A2 reported, so numbers are comparable across phases: identical, A→not-A, not-A→A, changed-in-place.

**Do not add tolerance.** A2 compared exact and that is what makes "pixel-identical" mean something.

Write the method into a comment or README at the point of use: exact match, play-area only, which classifier, which frames. A2's report had to explain its method in prose because the instrument did not carry it.

---

## 3. Measure cities

From the **existing committed frames** — no new capture run needed for this part.

Report the settlement delta as a **share of map area**, directly against the province figure:

```
provinces (land silhouette):  0.41%
settlements:                  ____%
```

That is the number the A3-versus-A4 decision turns on.

Also worth reporting, cheaply, because they mean different things:

- Settlement pixels that moved **within** a province versus **across** provinces. A2 noted one workload crossed the map when its pods rescheduled — that is `city_home` following the pods, which is arguably correct behaviour rather than instability. Intra-province movement is what A3 fixes.
- Whether the movement concentrates in provinces that lost a node, or is spread across untouched ones. Spread movement is the stronger argument for A3.

**Do not tune the classifier until the number looks right.** Fix the classifier on claim 6's evidence, run it once, report what it says.

---

## 4. Retake the baseline

Cherry-pick `--shot-seq` / `--shot-interval` onto v1.6.0, build, and run `gate.sh` against a freshly reset and settled fleet.

**Framing must match the A2 capture exactly** — same `--center`, `--zoom`, `--overlay`, `--map-style`. A2's first attempt failed partly because the vertical stride changed and the same framing covered a third as many provinces; the before and after must contain comparable content or the comparison is void again.

That is the known hazard here: **A2 tripled the stride.** A frame at v1.6.0 with A2's framing may hold three times as many provinces. Decide and state how that is handled — either frame both to a fixed province count, or report the figures with the caveat stated numerically rather than in prose. If neither is satisfiable, that is a real finding: the before/after may simply not be commensurable, and it is better to say so than to publish a pair that looks comparable and is not.

Commit the baseline frames beside `a2-gate/`.

---

## 5. Standing questions — written answers required

Per the A2 report's own recommendation, this is now a checked step rather than a remembered one. Answer each in the report, even if the answer is "does not apply here."

1. **Where does a summing step precede a comparing step?**
2. **Does every reducer over a possibly-empty input express unknown, or fabricate?**
3. **Where do two sections constrain the same behaviour — and is there a fixture where they diverge?**
4. *(new, from A2)* **What existing consumers depend on the old meaning of a value this change redefines?**

For this session, question 4 is nearly vacuous — nothing product-facing changes — but questions 1 and 2 apply directly to the comparator's own arithmetic: a pixel count divided by an area, where an empty crop or a zero-area frame is a real input.

---

## 6. The instrument test

A2's largest lesson: **four of six instrument failures were silent**, and the flipbook rendered identically whether or not the carry existed.

So before trusting the comparator, break what it measures and confirm it notices:

- [ ] Compare a frame against itself → 100% identical
- [ ] Compare two frames known to differ → a non-zero figure in the expected direction
- [ ] Feed it a deliberately shifted frame (crop offset by a few pixels) → a large delta, not a small one
- [ ] Confirm the play-area crop actually excludes the docked column — cover the column in one frame and check the number does not move

That last one is the cheapest guard against the A2 failure where the counters would have swamped the map.

---

## 7. Acceptance

- [ ] Comparator committed in `hack/churn/`, method documented at the point of use
- [ ] Crop and classifier are parameters, not constants
- [ ] Instrument tests in §6 pass, and are recorded
- [ ] City delta reported as a share of map area, against the 0.41%
- [ ] Intra- versus inter-province movement separated
- [ ] v1.6.0 baseline retaken and committed, **or** a stated finding that it is not commensurable and why
- [ ] Standing questions answered in writing
- [ ] No product code changed

---

## 8. What this session must not do

**No A3 work.** Not city slots, not placement. The point of measuring is to decide whether A3 is next; doing A3 work here presumes the answer.

**No classifier tuning toward a desired result.** If the number is inconvenient, that is the finding.

**No new scenario.** The seventh churn scenario (workload churn on a settled fleet) is A3-pre's, and only needed if the measurement says A3 is next.

---

## 9. Estimate

**Half a day.** The comparator is mostly extraction from a script that already works; the baseline rebuild is the uncertain part, because the cherry-pick may not apply cleanly onto v1.6.0.

If the cherry-pick fights, **stop and report** rather than reconstructing `--shot-seq` from scratch on an old tag. A clean statement that the baseline costs more than expected is a better input to the next decision than a day spent proving it.
