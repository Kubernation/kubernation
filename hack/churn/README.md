# The churn harness

A reproducible ~100-node fleet that can be made to churn on command, so
Workstream A's gates can actually be evaluated.

**This is a fixture, not a test.** It proves nothing on its own. Its job is to
put a fleet in front of a human at the A2 gate — the point where someone watches
a rolling refresh and judges whether the map holds still. That judgment cannot be
faked, and it is the gate that can kill Workstream A.

Correctness is a different instrument: A1's layout engine is verified by synthetic
fixtures in Rust with no cluster at all. If scripts here start asserting slot
coordinates, that logic belongs in unit tests instead.

## Use

```bash
hack/churn/up.sh                              # cluster + 100-node fleet (~5s)
hack/churn/capture.sh baseline                # a framed screenshot
hack/churn/scenarios/1-rolling-refresh.sh     # the kill-gate scenario
hack/churn/reset.sh                           # back to the pristine fixture
hack/churn/down.sh                            # tear the cluster down
```

Also available as `make churn-up`, `make churn-capture`, `make churn-reset`,
`make churn-down`.

## The ten scenarios

| # | Script | Proves | Gate |
|---|---|---|---|
| 1 | `1-rolling-refresh.sh` | A slot survives its occupant | **A2 — the kill gate** |
| 2 | `2-scale-up.sh` | New slots append; nothing existing moves | A2 |
| 3 | `3-scale-down.sh` | A departure leaves a ghost, not a reshuffle | A4 |
| 4 | `4-pool-add.sh` | A cataclysm is detected and recorded | A5 |
| 5 | `5-pool-remove.sh` | Structural loss, distinct from routine churn | A5 |
| 6 | `6-zone-loss.sh` | The failure-domain claim in the hierarchy | A5 |
| 7 | `7-workload-churn.sh` | A city stays put when a DIFFERENT workload changes (touches no nodes) | A3 |
| 8 | `8-substrate-gaps.sh` | The Substrate tab, the overlay and the headless `substrate` example name the same gaps; `MODE=discrim` drops a daemonset below the bar (touches no nodes) | Advisors ▸ Substrate |
| 9 | `9-node-capacity.sh` | The Substrate tab's "reports no capacity" tag reads capacity — `MODE=give` removes it (touches no nodes) | Advisors ▸ Substrate |
| 10 | `10-node-notready.sh` | A node that is BOTH NotReady and allocatable-less — the only combination no single cluster can express (stops the kwok controller; ~200s window) | the both-reasons tag |

Every scenario takes env-var overrides (`BATCH`, `OVERLAP`, `COUNT`, `POOL`,
`ZONE`, `CAPTURE`); read the header of each.

**Scenario 1 surges.** The replacement node is created and Ready *before* its
predecessor drains, so both exist at once — verified: node count peaks at 115
during a 15-node wave. A delete-then-create script would quietly test an easier
problem, because there would never be more nodes than slots and slot assignment
would never have to choose. `OVERLAP` controls how long the two generations
coexist. Replacements always get **new names** (a generation token), so nothing
can be matched by identity. Pods are drained onto the replacements, so the
scenario churns cities as well as provinces.

## The fixture

100 nodes, four pools, four zones, deliberately uneven:

| Pool | Pool-label convention | Zones | Nodes | Capacity | Instance type |
|---|---|---|---|---|---|
| `sys` | `cloud.google.com/gke-nodepool` | z-a, z-b, z-c | 30 | 8 / 32Gi | yes |
| `burst` | `karpenter.sh/nodepool` | z-a, z-b | 24 | 16 / 64Gi | yes |
| `mem` | `eks.amazonaws.com/nodegroup` | z-c | 16 | 8 / 128Gi | **none** |
| `edge` | **none** | z-a, z-d | 30 | 4 / 16Gi | yes |

Zones fill 37 / 22 / 26 / 15 — even zones hide layout bugs. `sys` spans three
zones, which is the case that broke the naive zone-contains-pool nesting. `edge`
has no pool label at all, which is what a fallback cascade's last resort is for.
One node (`churn-sys-g1-000`) has **no `status.allocatable`** — see the limits
below for what that actually exercises.

Workloads: six deployments of different sizes with zone spread constraints, a
DaemonSet on every node, and a StatefulSet. ~416 pods.

**The baseline has exactly one standing warning**, and it is expected: the
DaemonSet cannot schedule onto the allocatable-less node, because a node with no
declared capacity cannot accept pods. That is correct Kubernetes behaviour, not a
harness bug, and a baseline with one known concern is easy to diff against.

## What this harness cannot tell you

Read this before concluding anything from a green run.

**No metrics.** kwok has no metrics-server, so `Metrics.pods` is empty and every
`usage_ratio` is `None`. That is fine for Workstream A — extent and slots are
request- and capacity-derived — but it means **the harness cannot exercise A0's
usage path at all**. A clean run here says nothing about usage handling.

**The allocatable-less node renders as 0%, not as unknown.** `node_allocatable`
returns `Option` and its doc says callers must not fabricate a default, but six
of its seven callers do exactly that (`unwrap_or(0.0)`). So the node that is
*missing* its capacity renders with cpu 0% / mem 0% gauges — visually identical
to a genuinely idle node, not flagged as unmeasured. The fixture exercises the
path; it does not exercise the honest handling of it, because there isn't any
yet. Verified on this fleet.

**Pool labels are not read by anything yet.** No pool concept exists in the
codebase — no `pool_source`, no provider-label handling. The fixture's three
conventions plus the unlabelled pool are there so A1 has something real to build
against, but until A1 lands those fixture requirements can only be checked
*structurally* (the labels are present), never behaviourally.

**Zone loss deletes; it cannot make a node NotReady.** kwok rewrites the Ready
condition within ~1s, whether or not the node is annotated as kwok-managed, so a
status patch does not stick. Scenario 6 therefore deletes the nodes and refuses
`MODE=notready` with an explanation rather than running and changing nothing.

**Captures sample at ~6s.** Each is a separate process that connects and waits
for sync, so frames are at least six seconds apart and not precisely spaced. A
churn event that completes in three seconds is invisible — that is what the
`OVERLAP` and `COUNT` parameters are for. Slow the scenario until it is
observable.

**Deleting a node strands its pods for ~30–60s.** PodGC has a quarantine delay,
so immediately after a wave there are pods bound to nodes that no longer exist.
This is real Kubernetes behaviour, not a kwok artefact, and it is worth knowing
because a capture taken inside that window shows workloads on departed nodes.
The destructive scenarios call `wait_no_orphan_pods` before their final capture.

## Captures

`capture.sh <label> [n]` writes `out/<label>-<n>.png`, pinning **every**
view-affecting flag: `--center`, `--zoom`, `--overlay` and `--map-style`.

The last two matter more than they look. They persist in
`~/.config/kubernation/prefs.json` and are restored at launch, so an unpinned
capture inherits whatever the operator last used — during development a capture
came back tinted by the Namespace overlay in Relief for exactly that reason. A
before/after pair taken on different days would otherwise differ for reasons
having nothing to do with churn.

A flipbook beats prose: "does the map hold still?" is answered by alternating two
images, not by describing them. Set `CAPTURE=1` (the default) on a scenario to
get a frame per wave.

**Use `gate.sh`, not `capture.sh` in a loop.** Each `capture.sh` frame is its own
process, so its layout is assigned from scratch — and assignment is deterministic
in the node set, so such a flipbook renders identically whether or not the layout
carry exists at all. It measures determinism, which was never in doubt.
`gate.sh` drives one long-lived session through a scenario with
`--shot-seq`/`--shot-interval`, which is the only regime in which the carry is
observable. It refuses to run when the scenario would churn nothing.

**Reset and settle before every capture set.** Runs chain: reserved ground
accumulates, so a second run starts from a partly-grey map and is not comparable
with the first.

## Measuring

`compare.py A B --class land|settlement` diffs two frames over the play area,
exact match, and reports identical / class-lost / class-gained / changed-in-place.
The method is documented at the top of the file; `compare-selftest.py` breaks
what it measures and confirms it notices, including that the crop really does
exclude the docked column whose counters change every frame.

Read `DELTA / FOOTPRINT`, not the share of map area. Land covers ~33% of this map
and settlements ~0.14%, so the same map-area percentage means two wildly
different things — by map area the cities look *more* stable than the terrain,
and by their own footprint they are about ninety times less so.

`reshuffle.py` answers a question `compare.py` structurally cannot: **how many
provinces a refresh would MOVE under the pre-A2 ordering.** A pixel diff sees the
rendered map, and permuting uniformly-green provinces inside a contiguous
landmass changes almost nothing — a refresh that moved 15 of one zone's 27
untouched provinces registered as ~1% of land area. It works from the node names
alone, needs no capture, and is the instrument to reach for when the question is
about placement rather than appearance.

## Not

**Not CI.** A hundred-node cluster per PR is not a reasonable ask. This is a
local instrument for gate evaluation.

**Not a Rust project.** Shell and YAML, deliberately. A fixture that needs its
own test suite has failed at being a fixture.
