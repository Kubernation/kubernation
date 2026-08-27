# Plurality siting — pre-check

**Guidance:** `docs/kubernation-plurality-siting-precheck.md`
**Date:** 2026-08-19 · **No product change, no instrument built.**

**It is a real map problem, and §2.3's decisive question is answered YES: two
surfaces let a user conclude something false.** One of them is a sentence in the
field guide that the field guide itself contradicts three pages later.

The distribution is not "sometimes" — on a fleet with more nodes than replicas it
is **7 of 7**, all in the sharp band. And §3's cheap escape hatch **does not fit**,
measured, which constrains every remaining option.

---

## 0. The threshold, stated before any number was looked at

> A city is **meaningfully misrepresentative** when the plurality node holds
> **< 50%** of the workload's placed pods *and* the workload has **≥ 3 placed
> pods**. Below half, most of the workload is somewhere other than where the map
> draws it; the ≥3 floor exists because a 2-pod workload on 2 nodes is 50% by
> construction. A sharper band at **< 20%** marks cities standing for a small
> minority.

Recorded here because §2.1 and §5 both require it, and because it turned out to
be **exactly the falsity condition of a documented claim** (§3.1) — which would
have looked like a convenient choice if it had been made afterwards.

---

## 1. §1 — claims verified

All eight TRUE at source. Claim 8, quoted forward through three rounds, was
re-read as §7 asked: `world.rs` emits a city only where `*home == tile.name`, so
a city's province **is** its plurality node by construction.

Claim 1's tie-break is `max_by_key(|(node, n)| (**n, u64::MAX - fnv1a64(node)))`
— plurality, ties to the lowest hash. §7's question 2 asks whether it is honest
about being arbitrary: the code comment explains the *stability* purpose ("the
city only migrates when its pods genuinely move") and never says the node chosen
among ties is otherwise meaningless. No user-facing surface says so either.

---

## 2. §2.1 — the distribution

Only **Deployment** and **StatefulSet** are sited as cities. `WorkloadKind` has
exactly three variants, so §2.2's Job/CronJob row is moot — they are not
`WorkloadRow`s and cannot get a city at all. DaemonSets are excluded (claim 5)
and already carry a visible refusal.

| cluster | placed workloads | sited as cities | eligible (≥3 pods) | **misrepresented** | sharp (<20%) |
|---|---|---|---|---|---|
| kind (4 nodes) | 9 | 5 | 1 | **0** | 0 |
| churn (100 nodes) | 11 | 10 | 7 | **7** | **7** |

Churn, worst first:

```
 3.3%  Deployment   churn/worker    60 pods / 45 nodes
 4.2%  Deployment   churn/api      120 pods / 65 nodes
 5.0%  Deployment   churn/web       80 pods / 55 nodes
 8.3%  Deployment   churn/batch     12 pods / 12 nodes
 8.3%  StatefulSet  churn/store     12 pods / 12 nodes
12.5%  Deployment   churn/cache     24 pods / 20 nodes
12.5%  Deployment   churn/ingest     8 pods /  8 nodes
```

### 2.1 Is 4% typical or extreme? Neither — it is arithmetic

The two clusters disagree completely, and the reason is not the fixtures. The
scheduler spreads, so a workload lands on `k ≈ min(replicas, nodes)` nodes and
the plurality share is about **1/k**. On kind, 3 replicas and 4 nodes put every
Deployment on one node (100%). On churn, 120 replicas over 100 nodes give 4%.

**So the determinant is `nodes vs replicas`, not the fixture.** Any cluster with
more nodes than a workload's replica count produces this, and that is the
ordinary shape of a production fleet.

### 2.2 Which kinds — and the risk §2.2 warned about does not arise

`churn/store` is a **StatefulSet** at 8.3%, sitting mid-pack among the
Deployments. Deployments and StatefulSets behave **identically** here, so the
brief's "a fix that helps Deployments and breaks StatefulSets" hazard is not
live. There is no per-kind split to scope around.

### 2.3 Honest limits

Both clusters are synthetic in opposite directions: kind is too small to spread,
kwok has no resource pressure so spreading is maximal. **A real cluster with
bin-packing, affinity or topology constraints sits between them and was not
measured.** What generalises is the arithmetic in §2.1, not the 7/7.

---

## 3. §2.3 — what the map already gets wrong, by reading the rendering code

**Two surfaces state something false.** Both found by reading, neither inferred.

### 3.1 The field guide contradicts itself

| | |
|---|---|
| `almanac.rs:393` (Legend, City) | *"sited on the province holding **most** of its pods"* |
| `almanac.rs:502` (World page) | *"sited on the province hosting the **plurality** of their pods"* |

The code implements plurality. **The Legend sentence is false whenever no node
holds a majority** — which is precisely §0's threshold, and 7 of 7 eligible
workloads on the churn fleet.

Two pages of one document, disagreeing about the same mechanism, one of them
wrong. §7's question 3 shape, landing on documentation.

### 3.2 The city borrows its province's attributes — the stronger one

Selecting a city appends the **province's node attributes** to the SELECTION box:
grid reference, pool, extent, freshness, and — under the active overlay — the
node's **strain**, **upkeep** and **substrate gaps**. The code comment says it
outright:

> *"The city sits on the tinted province — show its **host node's** strain /
> upkeep too"*

For `churn/api` that is the strain and cost of a node running **2 of its 120
pods**, presented as the selected workload's context. The workload's real
footprint is 65 nodes, none of which the box mentions.

This is the surface that most directly lets a user conclude something false, and
it is not a wording problem — the panel is correctly describing the province; the
error is that the *city* is presented as having a host node at all.

### 3.3 What is NOT wrong

Read rather than assumed:

- **Province surfaces are honest.** A province's SELECTION line is
  `"{health} . {N} pods"` from `tile.pods` — the *physical* census, not its
  cities. The node window's GARRISON is likewise actual pods.
- **Blast is honest.** `blast_radius`'s Node arm walks `workloads_on_node`
  (actual pods) and `affected_cell` resolves each affected workload to *its own*
  city, wherever that is. A line from a node to a distant city is the correct
  statement.
- **The city drill-down asserts nothing.** `city.rs` renders no node at all
  (which is the D3/D4 pre-check's other half).

### 3.4 §7 question 3 — the inverse check

`city_home` maps a workload to one node; `workloads_on_node` maps a node to every
workload with a pod there. **They disagree for every spread workload, by
construction** — a node hosts workloads whose cities are elsewhere.

**Nothing relies on them agreeing.** Both consumers of `workloads_on_node` (blast,
the Oracle's node seeding) resolve a workload to its own position rather than
assuming it is local. The disagreement is not the finding; §3.2's borrowed
attributes are, and those come from the *province*, not from either function.

---

## 4. §3 — the panel column does not fit

Costed against `row_char_budget`, not estimated:

```
docked column 402px            budget  32 chars
today's tail "Running . r4 . 67d . 0m 10Mi"   28 chars
left for the pod name           4  ->  clamped UP to its floor of 8
```

**The row is already over budget**; only the `.clamp(8, 22)` floor keeps the name
legible. Adding a node name (`kubernation-worker2` is 19 chars, a churn node 15)
makes the tail 46–50 against a 32-char budget — 14–18 characters straight into
the fixed 156px button strip, which is D1 §7.2's second failure criterion.

**So the cheap thing does not cover it.** Per §6 that constrains every other
option and is worth knowing before any is weighed.

A narrower variant — the node shown only for the *worst* pod, or on hover, or
replacing the usage suffix — was not costed; that is scoping, and §4 says record,
do not choose.

---

## 5. §4 — candidate directions, recorded, none chosen

> **CLOSED 2026-08-19.** The last row won. Every other candidate is closed as
> unneeded — see §5.1. No map change was made, and none is proposed.

| Shape | Status after this pre-check | Final |
|---|---|---|
| Say it in the panel (node column) | **Ruled out as stated** — does not fit (§4). A narrower variant is uncosted | closed |
| Say it on the city (spread count/mark) | Open. Adds ink; would address §3.2 only indirectly | **closed — §5.1** |
| Say it on the provinces (road treatment, generalised) | Open. Roads were judged a compromise once already | **closed — §5.1** |
| Change siting (no plurality → no city) | Open, and §4's own note stands: a 4%-plurality workload is exactly what an operator wants to find, and removing its city hides it | **closed — §5.1** |
| **Nothing — a city is a label, not a location claim** | **Weakened but not dead.** §3.3 shows most surfaces are honest; §3.1 and §3.2 show two are not. Fixing those two is a smaller change than any map work, and would make this row true | **TAKEN — and now true** |

**The last row was the interesting one, and it held.** The pre-check went looking
for a map problem and found that most of the map is honest — the dishonesty was
concentrated in one sentence and one panel behaviour.

### 5.1 Why the three map-shaped candidates are closed

Correcting the two false claims was sufficient, and this was established by
enumeration rather than by assumption:

- **v1.26.0** fixed both (`plurality-false-claims.md`): the field guide now
  states the plurality rule once, and a city's SELECTION box states its real
  footprint and attributes the province's readings to the province.
- **The §7 enumeration** (`plurality-residual.md`) then read *every* surface that
  could assert a workload's location and found two residuals, both narrower and
  neither a map problem. **Both are now fixed** — v1.27.0 gave the Oracle's node
  lens the workload's footprint, and v1.28.1 renamed `--dump-positions`'
  misleading `node` field to `plurality_node`.

So nothing that remains would be improved by changing *where a city is drawn*.
The three map-shaped candidates were answers to a problem that turned out to live
in wording and in one panel behaviour, not in geometry. **A city is a label for a
workload, not a claim about its location** — and every surface now either says so
or says what it actually means.

**One accepted cost, recorded so it is not mistaken for an oversight:** the
concentrated case under-claims. A workload whose pods all run on one node reads
`3 pods on 1 node` / `on province X` rather than "all of it runs here". That is
the price of refusing spread-conditional wording, and the footprint line supplies
what a reader needs.

---

## 6. §7 — standing questions

**2. Unknown, or fabricated?** A workload with no pods has no plurality and gets
an island encampment; an evenly-split workload has two, and `city_home` picks the
lower hash — deterministic and stable, which is its purpose. It is **not**
labelled arbitrary anywhere the operator can see (§1), and §3.2 is what makes
that matter: the map does not merely place the city arbitrarily, it then
describes the arbitrary province as the workload's host.

**3. Two sections constraining one behaviour?** Three: `city_home` vs
`workloads_on_node` (§3.4 — they disagree and nothing depends on it), and the two
almanac pages (§3.1 — they disagree and one is wrong). The second is the finding.

**5. Inherited claims?** All eight re-read at source, including claim 8, which had
been quoted forward through three rounds and is correct. Claim 3's figures were
re-measured rather than copied, and the re-measurement changed the picture: the
D3/D4 pre-check reported three workloads; the distribution over all of them is
what shows 4% is the rule at scale rather than an outlier.

---

## 7. §8 — acceptance

- [x] Threshold stated before measuring (§0)
- [x] Distribution reported for every workload on both clusters, by kind (§2)
- [x] §2.3's enumeration done by reading the rendering code (§3)
- [x] The panel-column option costed against `row_char_budget` (§4)
- [x] `city_home` / `workloads_on_node` checked for a depended-upon disagreement (§3.4)
- [x] Candidate directions recorded with their risks; **none chosen** (§5)
- [x] Unmeasured recorded: a real cluster with resource pressure (§2.3); a narrower panel variant (§4)
- [x] No product code changed, no instrument built

**Per §6's table:** *many workloads are misrepresented and a surface misleads* →
a real problem, to be scoped **against the two specific false claims**, not
against "cities are imprecise".
