# Substrate overlay — DaemonSet coverage gaps

**Implementation report** · 2026-07-30 · shipped as **v1.5.0**
**Commits:** `5104638` (feature) · `817f565` (review fixes)
**Governing doc:** [`kubernation-substrate-overlay-guidance.md`](../kubernation-substrate-overlay-guidance.md)

An eighth map overlay answering one question — which nodes are missing
infrastructure the rest of the fleet has. Adversarially reviewed; two real defects
found and fixed.

| | |
|---|---|
| Lines | +1168 / −80 |
| New core module (`state/substrate.rs`) | 383 loc |
| Tests | 328 core + 83 GUI |
| Render-smoke states | 51, no panic |
| Confirmed defects | 2, fixed |

CI green across Linux, macOS and Windows.

---

## What shipped

A node quietly missing its log agent or CNI looks perfectly healthy — its own pods
are fine — while lacking something every other node runs. Nothing in the product
surfaced that. The overlay does, and the province window names the specific
DaemonSets.

The colour rule is the feature's whole information design: fully-covered provinces
**recede** so the anomalies are the only thing that pops. It is deliberately discrete
rather than a ramp — gaps are small integers where zero overwhelmingly dominates, and
a ramp would wash a healthy fleet into near-identical tints.

| State | Rendering |
|---|---|
| Covered | Recedes to plain land. The common case costs no attention |
| One missing | Amber. The panel names which DaemonSet |
| Two or more | Red. A node materially under-provisioned |

With no fleet-wide DaemonSet at all, the overlay falls back to ordinary terrain
rather than paint an all-clear it hasn't earned.

---

## The judgment call that defines the feature

Everything else was wiring. The substance is **what "expected" means**. The naive
definition — every DaemonSet, everywhere — over-reports badly, because DaemonSets
legitimately don't run everywhere. A GPU plugin absent from your CPU nodes is not a
finding.

We use **prevalence**: a DaemonSet on ≥80% of nodes is treated as fleet-wide, so its
absence is a finding.

> This is inference, not intent, and we say so in the product. The report never reads
> a DaemonSet's spec, so prevalence is the only evidence available that something was
> *meant* to be everywhere. Stating that plainly is better than implying a certainty
> the data can't support.

That is the same honesty discipline the cost feature applies when it refuses to print
a `$` for a unitless basis, and the same one the NetworkPolicy feature applies when it
reports isolation *presence* rather than claiming to have verified enforcement.

---

## A constraint worth planning around

The guidance noted that small clusters "under-report". The real constraint is sharper,
and it is arithmetic rather than a tuning preference: at *n* nodes a DaemonSet is
expected at `ceil(0.8n)`, and a gap requires it on *fewer* than *n*. For **n ≤ 4 those
are mutually exclusive** — no gap is representable at all.

| Fleet size | Expected at | Gap representable? | Consequence |
|---|---|---|---|
| 1–4 | = n | No | Reports nothing, ever |
| 5 | 4 | Yes | Smallest fleet that can find one |
| 100 | 80 | Yes | Where the feature earns its keep |

**The dev kind cluster has four nodes.** It is therefore permanently clean under this
overlay by arithmetic, and cannot exercise the feature at all. Live verification ran
against the 100-node kwok simulation instead — two DaemonSets at 98/100 and 99/100
produced exactly two coloured bands among a hundred provinces.

Pinned by a test sweeping every sub-5 configuration, and stated in the in-app field
guide. Listed here because it generalises: **any future feature whose signal is
fleet-shaped will have the same problem with our default dev target.**

---

## Review outcome

Four independent lenses — correctness, the filtered/unfiltered boundary, rendering,
and honesty — each followed by an adversarial verification pass whose job was to
refute every finding.

```
13  raised across four lenses
 4  survived verification
 2  distinct defects
```

Both survivors were reported independently by **two different lenses**. The nine
refuted findings were all single-lens. Cross-lens convergence turned out to be the
reliable signal. *(See the A0 report for a later correction to this conclusion.)*

### Confirmed — identity collapse

`state/substrate.rs`, coverage keyed on the bare DaemonSet name. Two same-named
DaemonSets in different namespaces merged into one identity, unioning their node sets.
It broke in both directions:

- **Hid a real gap** — `monitoring/agent` fleet-wide and absent from a node, while an
  unrelated `tenant-a/agent` ran only there: that node read "substrate: complete".
  Precisely the unearned all-clear the module says it refuses.
- **Invented one** — two 4-of-10 sets, neither fleet-wide, union to 8/10 ≥ 80%: an
  expectation neither earned, flagging the nodes running neither. Exactly the false
  positive the prevalence rule exists to prevent.

**Fixed:** identity is now `namespace/name` — which is also what an operator needs in
order to act on it, and reads better on a real cluster, where it distinguishes a
tenant DaemonSet from system infrastructure at a glance.

### Confirmed — ghost-node inflation

Prevalence counted pods by `spec.nodeName` while the denominator was the node store. A
pod outliving its Node object — an autoscaler scale-down, with pod GC running on a
delay — therefore got a vote on what the fleet runs.

Verified: a DaemonSet on 3 of 5 nodes correctly reports nothing; add one pod bound to
a departed node and it clears the threshold, fabricating gaps on two live nodes.
Routine on autoscaled fleets.

**Fixed:** the numerator is bounded to nodes present in the store.

Both were reproduced by running the code before being accepted, and both regression
tests were mutation-verified. Three documentation corrections rode along, including
**one false claim in my own commit message** about a bug that never existed; corrected
in the permanent decision log rather than quietly softened.

---

## Against estimate

| Guidance estimate | Actual |
|---|---|
| ~1.5 days — core report and tests, overlay wiring, panel integration | One session, including a full four-lens adversarial review and its fix round |

The scope estimate was accurate. What it did not budget for was the review round,
which found two real defects — both of which would have shipped as **silent wrong
answers** rather than visible failures.

> For a feature whose entire value is that an operator trusts what it says about their
> fleet, a wrong "complete" is the one unacceptable output. The review round should be
> treated as part of the estimate, not an optional extra after it.

---

## What changed versus the guidance

The doc was accurate in most respects and wrong in one substantive place — the fourth
round running. Its §3 said to compute the report inside the world builder. That
builder's DaemonSet collection is **scoped by the active namespace filter**, because it
exists to decide which roads to draw. Computing prevalence over it would have reported
phantom gaps the moment anyone applied a filter.

We followed the NetworkPolicy coverage precedent instead: a pure report over the
observed world, explicitly unfiltered, hung on the model. One report now feeds the map,
the selection box and the province window, so they cannot disagree.

**The pattern is consistent enough to plan against:** these documents are reliable on
intent and scope, and reliably contain one or two wrong mechanism claims. Verifying the
mechanism before building has caught a real trap every round.

---

## Deferred

| Item | Note |
|---|---|
| Advisors ▸ Substrate tab | The report is already shaped for it — a cluster-wide rollup is mostly presentation |
| Read `nodeSelector` / tolerations | Would replace inference with intent. Needs the DaemonSet spec; a wrong "expected" is worse than a stated heuristic |
| Minimap tint | No per-node data threaded there — same as the walls and cost overlays |
| Warm-cluster headline parity | The status label reads hot; the map and panels are already per-world |

---

## Decisions for the room

### Four shipped versions on main carry no tag

v1.2.0, v1.3.1, v1.4.0 and v1.5.0 are all pushed and green but untagged, with their
changelog entries under *Unreleased*. This is the exact drift the 1.0.0 release notes
warn about — it previously accumulated across seventy versions before needing a cleanup
pass. A tag triggers the signed and notarized release pipeline, so it needs a deliberate
call rather than a default.

**Ask:** cut a release now, or set a cadence?

### The dev cluster cannot exercise fleet-shaped features

Four nodes is below this feature's arithmetic floor, and the same will be true of
anything else whose signal only appears across a fleet. Live verification worked, but
only because the kwok simulation happened to be available and I seeded it by hand.

**Ask:** make a seeded fleet-scale target a standing part of the dev loop?

### Is prevalence-as-inference good enough to keep?

It is honest, documented in-product, and needs nothing beyond what we already watch.
Reading `nodeSelector` and tolerations would replace the guess with the author's actual
intent — at the cost of a real increase in scope and a new way to be confidently wrong.

**Ask:** hold the heuristic, or fund the upgrade?
