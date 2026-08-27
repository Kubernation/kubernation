# KuberNation — Plurality Siting: Pre-check

**Measurement guidance**
**Goal:** establish what a city actually claims about a spread workload, and what the map could say instead, before anything is scoped.
**No product change.** The output is a number, an inventory of what depends on plurality, and a decision about whether this is a map problem at all.

---

## 0. Why a pre-check and not a phase

The pre-check that found this said so directly:

> That is not visual momentum. It is a question about **what the map can express**, and the underlying gap is larger than the mechanism.

It is also the item with the longest history of *consequences* and no direct examination. Four rounds have hit effects of plurality siting without naming it:

| Round | Consequence |
|---|---|
| A3 | The placement residual — a city moves when a colliding newcomer sorts ahead of it |
| D2 | Staleness source 1: a stored cell is wrong when a reschedule shifts the plurality |
| T2-pre | *"a Deployment pinned to a pool sites its city at the plurality node, so thirty spread failures would read as one troubled city"* |
| D2-brushing | A DaemonSet has no city at all — *road, not a settlement* |
| D3/D4 pre-check | `api`: 120 pods across 65 nodes; the city sits on a node holding **4%** |

Scoping a fix now would repeat the mistake this project has avoided five times. **Measure first.**

---

## 1. Verify before building

All `[A]`. VOR was unavailable. Several of these are years old in code terms and the reports carrying them span the week.

| # | Claim | Source |
|---|---|---|
| 1 | `city_home` sites a workload at the node holding the plurality of its pods, tie-broken on a stable hash | early hit-test work |
| 2 | `CityPod.node` exists on every pod in the city window's CITIZENS list, and `city.rs` renders it **nowhere** | D3/D4 pre-check §3 |
| 3 | Measured on the churn fleet: `api` 120 pods / 65 nodes, plurality node holds 5 (4%); `cache` 24/20 → 12%; `batch` 12/12 → 8% | same |
| 4 | On kind: `db` spans 2 nodes, `agent` 3 | same |
| 5 | DaemonSets are excluded from city siting (`world.rs:656`) and drawn as roads; the island-encampment fallback is for **zero-pod** workloads only (`world.rs:875`) | D2-brushing §3 |
| 6 | `workloads_on_node` is the shared node→workload authority, used by blast and the Oracle | blast work |
| 7 | `city_dx` hashes the name; `city_dy` hashes the full ref modulo `rows` — a city's cell is within its province | A3 |
| 8 | A city's province **is** its plurality node by construction, so `MOVED-ACROSS` is unrepresentable | A3-pre §2 |

**Claim 8 is the load-bearing one.** The map's entire city-to-province relationship assumes one workload sits on one node. Everything below follows from that being false in practice.

---

## 2. What to measure

### 2.1 The distribution, across real shapes

Claim 3 is three workloads on one fixture. Before scoping, establish whether 4% is typical or extreme.

For every workload on both clusters, record: pod count, distinct node count, plurality node's share, and the workload kind.

Report the **distribution**, not an average. The useful question is not "what is the mean share" but **"what fraction of workloads are meaningfully misrepresented"** — and that needs a threshold decided in advance. A workload on 3 pods across 3 nodes at 33% is arguably fine; one at 4% is not.

**State the threshold before measuring**, or it will be chosen to make the number interesting.

### 2.2 Which kinds are affected

Claim 5 says DaemonSets have no city at all — the extreme case, already handled by a visible refusal. But the spectrum matters:

| Kind | Expected spread |
|---|---|
| Deployment | wide, and the common case |
| StatefulSet | one pod per node typically, ordinals matter |
| DaemonSet | every node — already excluded |
| Job / CronJob | transient, may span many nodes briefly |
| Zero-pod workloads | island encampment, no node at all |

**A fix that helps Deployments and breaks StatefulSets is not a fix.** Establish which kinds the problem actually bites.

### 2.3 What the map already gets wrong because of it

Enumerate, and measure where possible:

- **Blast radius.** Does killing the plurality node under-report a spread workload's exposure? `workloads_on_node` (claim 6) knows the truth — check whether the map's rendering of it does
- **Failure marking.** T2-pre found thirty spread failures reading as one troubled city. Confirm on a current build
- **Selection and `city_pos`.** D2's staleness source 1 is a direct consequence — a reschedule that shifts the plurality moves the city
- **Node occupancy.** A province's cities are the workloads *sited* there, not the workloads *running* there. Does anything present the province as though its cities were its occupants?

**That last one is the question underneath the others.** If a province visually implies "these workloads run here," the map is asserting something false for every spread workload.

---

## 3. What the model already knows

Claim 2: `CityPod.node` is present on every pod and rendered nowhere.

So one candidate answer is **not a map change at all** — the city window lists every pod and could say where each one is. That would cost a column and would not touch the geography.

**Check this first**, in the shape D2's pre-check checked namespace colour: it is the cheap thing that might cover most of the gap, and if it does, the map question does not need answering yet.

**But check what it costs.** D1 §3.1 found the docked panel's rows already tight — a fixed 156px button cluster against 246px of clear space, with `row_char_budget` derived from the column. A node column may not fit, and *may not fit* is a finding.

---

## 4. Candidate directions — record, do not choose

The pre-check's job is to inform this, not settle it.

| | Shape | Chief risk |
|---|---|---|
| **Say it in the panel** | A node column in CITIZENS (§3) | May not fit; does not touch what the map asserts |
| **Say it on the city** | The city shows it is spread — a count, a mark | Adds ink to a map that is not short of it |
| **Say it on the provinces** | Every province hosting a pod shows the workload somehow | This is the road treatment DaemonSets already get, generalised — and roads were judged a compromise |
| **Change siting** | A workload with no plurality gets no city, like a DaemonSet | Removes cities from the map; a workload with 4% plurality is exactly the case an operator wants to find |
| **Nothing** | The city is a *label for a workload*, not a claim about location | Defensible if §2.3 shows nothing actually misleads |

**The last row is a real candidate.** If the map never asserts that a city's province hosts its pods, then plurality siting is an arbitrary-but-stable placement choice — which is what A6's declared frame says about position generally, and the honesty constraint plan §3.3 already carries.

**What would settle it:** does any surface let a user conclude something false? That is §2.3's job.

---

## 5. Method

**Use both clusters.** kind has real kinds and few nodes; the churn fleet has scale and one namespace. Neither alone answers §2.1.

**Do not build an instrument.** Thirteen instrument failures in this workstream, the most recent a gate that reported green on a leftover cluster. `--dump-positions` and `kubectl` cover this; the last two pre-checks were done with arithmetic and a running app.

**Read the path, do not infer it.** Three instances across recent rounds of a wrong diagnosis from reasoning about control flow rather than reading it. §2.3 asks what the map gets wrong — answer it by finding the code that renders it, not by predicting what it must do.

**Threshold before numbers** (§2.1).

---

## 6. What this decides

| Finding | Consequence |
|---|---|
| Few workloads are meaningfully spread | The item is small. A panel column (§3) probably closes it |
| Many are, and a surface misleads (§2.3) | A real map problem. Scope against the specific false claim, not against "cities are imprecise" |
| Many are, and nothing misleads | **Close it**, with §4's last row as the reason, and record that the city is a label rather than a location claim |
| The panel column does not fit (§3) | That constrains every other option, and should be known before they are weighed |
| Different kinds need different answers (§2.2) | Any single fix is wrong; scope per kind or not at all |

---

## 7. Standing questions

**2. Unknown, or fabricated?** A workload with no pods has no plurality. A workload whose pods are evenly split across two nodes has two. **Neither is "the first one"** — check what `city_home`'s tie-break actually does and whether it is honest about being arbitrary.

**5. Inherited claims?** Every §1 claim comes from a report, several from this week, one from work months old. Claim 8 in particular — *a city's province is its plurality node by construction* — has been quoted forward through three rounds and should be re-read at source.

**3. Two sections constraining one behaviour?** `city_home` decides where a workload appears; `workloads_on_node` decides what a node hosts. **These are inverses that do not agree** — a node hosts workloads whose cities are elsewhere. Check whether anything relies on them agreeing.

That last one may be the finding. It is the shape this project has paid for seven times.

---

## 8. Acceptance

- [ ] Threshold for "meaningfully misrepresented" stated before measuring
- [ ] Distribution reported for every workload on both clusters, by kind
- [ ] §2.3's enumeration done by reading the rendering code, not by inference
- [ ] The panel-column option costed against `row_char_budget` (§3)
- [ ] `city_home` / `workloads_on_node` checked for a disagreement anything depends on (§7)
- [ ] Candidate directions recorded with their risks; **none chosen**
- [ ] Surfaces that could not be exercised recorded as unmeasured
- [ ] No product code changed, no instrument built

---

## 9. Estimate

**Two to three hours.** The distribution is `kubectl` and arithmetic; §2.3 is reading; §7's inverse check is the part most likely to turn up something.
