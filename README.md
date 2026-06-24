# KuberNation

<p align="center">
  <img src="kubernation-logo-full.png" alt="KuberNation" width="420">
</p>

**Your Kubernetes cluster as an explorable world map.**

KuberNation is a desktop application that renders a Kubernetes cluster the way a
turn-based strategy game renders its world. Instead of scrolling tables of pods
and nodes, you look at a map: each node is a patch of terrain whose colour shows
its health, each workload is a city sited on the node its pods run on, and the
problems that need you are surfaced in a queue — the **next thing needing your
attention** — rather than buried in dashboards you have to go hunting through.

If you've played a 4X strategy game — the explore-and-build kind, like
*Civilization* — the interface will feel familiar (a world you pan and zoom,
cities with name banners, a right-hand info column, drill-down "city screens").
If you haven't, you don't need to — every
game term below is explained, and the underlying objects are always plain
Kubernetes (a "city" is a Deployment, a "province" is a node).

> **New here?** Jump to **[The world](#the-world-how-kubernetes-becomes-a-map)**
> for the one-table explanation of how Kubernetes maps onto the game, then
> **[Quick start](#quick-start)** to run it.

![A KuberNation world](docs/gui-world.png)

*A live cluster from `make dev`: an isometric world where one city (`crashy`)
flies a warning flag, the `✦` structures on the southern island are custom
resources, and DaemonSets pave roads across the provinces.*

---

## Why a map?

Most Kubernetes UIs — `kubectl`, and table/dashboard tools like k9s, Lens, or
Headlamp — present your cluster as lists you filter and sort. That works, but it
hides two things a map makes obvious: **where** things run (failure domains,
placement, drift between clusters) and **what matters right now**. KuberNation is
built around three ideas:

- **Spatial, not tabular.** Resources project onto a stable world map, and the
  geography means something — a node's terrain is its health, a workload's city
  moves when its pods reschedule, two clusters sit side by side so drift is
  visible at a glance.
- **Attention-driven.** Crash-looping pods, stalled rollouts, pending volumes,
  nodes under pressure, burning error budgets — all aggregated and ranked into one
  **attention queue**. Press `N` to fly to the next problem, `L` to jump straight
  into the offending pod's logs.
- **Read-first, with deliberate writes.** KuberNation observes by default. It can
  change the cluster, but only through a few explicit, confirmed, RBAC-checked
  actions (evict a pod, commit a batch of staged changes, run a chaos drill) —
  and every line of write code lives in one small, auditable file.

---

## Install

**Pre-built binaries** are attached to each [GitHub release](../../releases) — a
macOS universal binary (Apple Silicon + Intel) and a Linux x86_64 binary. Download,
verify against `SHA256SUMS`, and run it against your current kube-context:

```sh
tar xzf kubernation-vX.Y.Z-macos-universal.tar.gz
cd kubernation-vX.Y.Z-macos-universal
./kubernation                 # uses your current kubeconfig context
```

> **macOS:** the binary is not yet code-signed/notarized, so Gatekeeper blocks it on
> first launch — clear the quarantine flag once: `xattr -d com.apple.quarantine
> ./kubernation` (or right-click ▸ Open).

**From source** (Rust stable; on Linux you also need the X11/GL/ALSA dev libraries
`libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev`):

```sh
cargo run --release -- --context <kubeconfig-context>
```

It needs a display — it's a windowed desktop app (not a TUI), so it runs on your
laptop, not over SSH.

## Quick start

Point it at any cluster your kubeconfig can already reach:

```sh
kubernation --context <kubeconfig-context>   # omit --context to use the current one
```

Or spin up a local 4-node `kind` cluster with sample workloads (needs Docker +
[`kind`](https://kind.sigs.k8s.io/) + `kubectl`) and launch against it:

```sh
make dev
```

Other useful targets: `make smoke` (a headless connect-and-summarize check, no
window), `make lint`, `make test`, `make kind-down`.

---

## The world: how Kubernetes becomes a map

Everything on screen is a real Kubernetes object. The mapping is:

| Kubernetes object | On the map | Notes |
| --- | --- | --- |
| Zone / failure domain | a **continent** | nodes in the same `topology.kubernetes.io/zone` cluster together |
| Node | a **province** of terrain | the terrain's colour/texture is the node's health |
| Workload (Deployment, StatefulSet) | a **city** | population = ready replicas; sited on the node running most of its pods, so it *moves when its pods do* |
| Pod | a **citizen** of its city / **garrison** of its node | listed inside the city and province drill-downs |
| DaemonSet | a **road** | paved across every node it runs on (it isn't a city — it's everywhere) |
| Service | a **harbor** on the city's coast | the shoreline is the network boundary |
| Ingress | a **gate** on the coast | external traffic enters here |
| PersistentVolumeClaim | a **granary** inland of the city | yellow if the claim is unbound |
| Job / CronJob | an **expedition** / **scheduled structure** | on the workload's "namespace island" |
| Custom resource (projected) | a **structure** (`✦`) | on the namespace island |
| A problem | an **attention-queue entry** + a flag on the map | ranked by severity |

A docked column on the right is your at-a-glance dashboard, mirroring a strategy
game's info panel: **WORLD** (a minimap — click to recenter), **STATUS** (context,
node/pod counts, the concern roll-up, cluster CPU/memory trend), **ATTENTION**
(the live problem queue — click a row to fly there), **FORWARDS** (any live
port-forwards), and **SELECTION** (whatever tile you last clicked or are hovering).

---

## Feature tour

### Navigating the map

Drag (or `WASD`/arrows) to pan, scroll to zoom around the cursor, `F` to fit the
whole world, `]`/`[` to sail to the next/previous city, and click the minimap to
recenter. The map is rendered on a classic isometric **2:1 diamond** grid with
all-original procedural art — health-tinted, dithered land, inked shorelines,
trees on healthy ground, and procedural cities that grow from a single hut to a
walled keep as their population rises, each with a population chip and a serif
name banner. As you zoom out, detail generalizes (cities collapse into province
badges) so a big cluster stays readable. Nothing here is a sprite asset — it's all
drawn from geometry, so the binary stays self-contained.

The chrome is a dropdown **menu bar** (Game · View · Orders · Game Day · Advisors
· World · Help) plus the docked right column and a cartographic map title.

### Drill-downs: cities and provinces

Click a city to open its **city window** — the workload in full: replica and
update gauges, a pod **census** grid, a clickable pod list, **improvements** it
owns (Services, Ingresses, PVCs, ConfigMap/Secret references), an availability
**treasury** (see [Reliability](#reliability-slos--the-error-budget-treasury)),
and a **chronicle** of recent events.

![City window](docs/gui-city.png)

Click open land to open the matching **province (node) window**: zone and health,
CPU/memory gauges with trend sparklines, the **garrison** of pods stationed there,
the node's **terrain** facts (container runtime, kubelet, OS, arch), and its
conditions.

![Province window](docs/gui-node.png)

### Logs

Click any pod row — in a city's citizens list or a province's garrison — to tail
its logs in a live overlay (refreshed every couple of seconds, rendered in a
monospace face so timestamps and columns line up). Lines are tinted by guessed
severity; `/` filters (space-separated terms AND together, `!term` excludes), `p`
shows the previous (crashed) container, `T` toggles timestamps, `s` widens the
history window, `c` copies, `w` exports. From the attention queue, `L` opens the
logs of the exact pod behind a concern.

![Log overlay](docs/gui-logs.png)

### Map views

The **View** menu recolours the whole board (and the minimap) like a strategy
game's map modes:

- **Terrain** — node health (the default).
- **Pressure** — CPU/memory heat per node.
- **Replicas** — the worst workload health on each node (red where a city is
  understrength).
- **Namespace** — a stable hue per namespace, a political/territory map.

The active view is named in the title and STATUS so a recoloured map is never
mistaken for a health signal.

![Map views](docs/gui-overlay.png)

![Menu bar](docs/gui-menu.png)

### More Kubernetes kinds as geography

Beyond nodes and workloads, the rest of the cluster reads as terrain too: a
city's **harbors** (Services) and **gates** (Ingresses) moor off its east coast on
the latitude of the city they serve; a **granary** (PVC) sits inland of any city
that mounts storage (cyan when bound, yellow when pending); and batch work lands
on the **namespace islands** in the southern sea — Jobs as expeditions (with a
status pennant, yellow when failed), CronJobs as clocks showing their schedule,
beside any projected custom resources.

![Batch and islands](docs/gui-batch.png)

### The Almanac (in-app field guide)

Press `?` (or `F1`, or **Help**) for the **Almanac** — an in-app reference that
documents the entire visual vocabulary with the *actual marks* drawn beside each
definition (so it can never drift from the map), plus the world metaphor, the
controls, and how to read state. Legend entries that have a live example light up
with a `>`; click one to fly straight to it.

![Almanac](docs/gui-almanac.png)

### Advisors

The **Advisors** menu opens read-only summary reports of the whole realm that
complement the attention queue — **Health** (nodes by health, pods by phase,
workloads at strength), **Storage** (PVCs bound vs. pending), and **Network**
(Services and Ingresses, plus orphaned Ingresses and idle Services). They're pure
functions of the observed cluster and always cluster-wide.

![Advisors](docs/gui-advisors.png)

### Reliability: SLOs & the error-budget treasury

Each city window shows an availability **SLO** and the **error budget** it spends
down — a coin gauge that's full when the workload stays up, drains when it flaps,
and is exhausted when availability falls below target. Availability is derived
from pod readiness over a recent window, so it needs **no Prometheus or
metrics-server** — it works on any cluster. Set a per-workload target with the
in-window stepper or a `kubernation.io/slo-target` annotation; a burning or
exhausted budget also raises an attention-queue concern.

### Acting on the cluster

KuberNation performs only a handful of writes, each explicit, confirmed, and
RBAC-checked — and all of them live in one small file (`crates/kubernation-core/src/k8s/actions.rs`).

**Evict a pod.** Hover a pod in a city's citizens (or node's garrison) list and an
`evict` button appears; on confirm, KuberNation issues a real `DELETE` (a managed
pod is recreated by its controller; a bare pod is gone). The button is disabled
(`locked`) unless an RBAC check says you may delete pods there.

![Evict confirm](docs/gui-evict.png)

**The planning turn.** Changes are *staged*, not applied imperatively. Step a
city's replicas, or stage a cordon / rolling restart / image change; press `t`
(or **Orders ▸ End of Turn**) for a from→to review of everything staged, with
per-row unstage and discard. **Commit** validates every change with a server-side
dry-run first (which also enforces RBAC), so a change the cluster would reject is
blocked before anything is written — all-or-nothing. Staging never writes; only
Commit does.

![Planning turn](docs/gui-plan.png)

**Game Day (chaos).** The **Game Day** menu opens a chaos-engineering console:
inject a *real* failure and watch the cluster respond — the attention queue lights
up ("raid underway"), the blast radius spreads across the map, the error budget
spends. Pick a target and an experiment:

- **kill one / a percentage / all pods**, **outage** (scale to 0), **scale spike**
  (a surge), **broken image**, **node failure** (cordon + drain), **cordon freeze**
  (cordon, no drain), or **partition** (a deny-all NetworkPolicy — both directions,
  ingress-only, or egress-only);
- or a compound **difficulty tier** — **Skirmish**, **Raid**, or **Siege** — that
  sequences several experiments into one drill.

The console previews the exact steps, the blast radius, and the budget cost
*before* you run it (a confirmed write); afterward a **scorecard** reports the
response: a steady-state check, recovery time, **MTTD** (how long the attention
queue took to notice — KuberNation grading its own observability), a recovery
sparkline, and budget spent. Everything reuses the existing gated write primitives
(so chaos adds no new powers beyond the NetworkPolicy), control-plane and system
namespaces are refused, and reversible drills auto-restore — on demand, after a
timer, or automatically when you quit or switch clusters — so a drill never
strands the cluster.

![Game Day](docs/gui-chaos.png)

**Port-forward.** Hover a pod row and click **fwd** to open a local
`127.0.0.1` tunnel to it (the port is auto-resolved; RBAC-checked). Live forwards
appear in the right column's FORWARDS section with a stop button. This changes
nothing on the cluster, but it's gated like a write.

### Two clusters: the hot/warm pair

Run with `--warm` (`make pair`) and a standby cluster rises as a **second
archipelago** east of the first — one sea, free panning between them, `F` fits
both:

```sh
kubernation --context prod --warm prod-standby
```

Every city carries a **sync chip** showing how it compares to its twin (`=` in
sync, replica/image drift, missing-on-warm), tooltips and windows are tagged
HOT/WARM, and the attention queue merges both worlds (entries tagged `[H]`/`[W]`)
plus a single aggregate "drift" concern.

![Hot/warm pair](docs/gui-pair.png)

---

## Reading the world

On the map, each of these is drawn as a small **procedural shape and colour** —
not a literal text character. The glyphs below are a **legend shorthand** (the
in-app Almanac shows the exact drawn mark beside each definition).

### Map marks

| Mark | Element | Meaning |
| --- | --- | --- |
| `▣` `▤` `▥` `▦` | province (node) | healthy · cordoned · under pressure · NotReady |
| city + a population chip | workload | the number is ready replicas; the building grows with it |
| `‼` `!` | flag over a city | a critical · warning concern lives there |
| `Ψ` | harbor (east coast) | a Service |
| `∏` | gate (east coast) | an Ingress |
| `⊞` | granary (inland) | a PVC — yellow if unbound |
| `◈` | expedition (island) | a Job — yellow if failed |
| `◷` | clock (island) | a CronJob — shows its schedule |
| `✦` | structure (island) | a projected custom resource |
| `◌` | encampment (island) | a zero-pod workload |
| `≣` | road | a DaemonSet |

### Pod states

Inside the city and province windows, each pod keeps a glyph:

| `●` | `◐` | `○` | `◌` | `✗` | `◆` |
| --- | --- | --- | --- | --- | --- |
| ready | running, not ready | pending | terminating | failing | succeeded |

### Gauges

The CPU/memory gauges show **scheduling pressure** (requests ÷ allocatable) by
default — green is calm, yellow ≥ 70%, red ≥ 90%. Install metrics-server
(`make metrics-up`) and they switch automatically to **live usage**, labelled so
you can tell which you're looking at; with no metrics-server they quietly fall
back to requests.

### Colour discipline

The palette is deliberately restrained: parchment chrome, green land, blue ocean
— with **saturated red and yellow reserved strictly for things that need
attention**, so trouble pops against terrain instead of competing with it.

Run with `--colorblind` for a red-green-safe palette: the "healthy" greens become a
steel blue (so blue / amber / red are all distinguishable), red and amber unchanged.

---

## Controls

KuberNation is mouse-first with a strategy-game menu bar and a few keys. The
in-app Almanac (`?`) always has the complete, current list.

| Input | Action |
| --- | --- |
| drag · `WASD`/arrows · scroll | pan · pan · zoom (cursor-anchored) |
| `F` · `]` / `[` | fit the world · sail to next / previous city |
| click land / city / harbor | open the province / city drill-down |
| click a pod row | tail its logs |
| hover a pod row → **fwd** | port-forward it to `127.0.0.1` |
| `y` | inspect a resource's YAML (read-only "dossier") |
| `N` · `L` · `B` | next concern · tail its pod's logs · its blast radius (what else it would take down) |
| `:` | resource browser — list/inspect *any* kind |
| `t` | the End-of-Turn planning review |
| `c` · `Esc` | switch cluster context · close the topmost overlay |
| `?` / `F1` | the Almanac |
| menu bar | context · fit · map view · namespace filter · advisors · Game Day · quit |

Two more ways to explore any object, including kinds that aren't on the map:

- **`y` — the YAML inspector.** A read-only dossier of a workload, node, or pod,
  with `managedFields` and last-applied noise stripped. It only inspects *watched*
  kinds, so Secrets and ConfigMaps are never read this way.
- **`:` — the resource browser.** A k9s-style escape hatch: pick any kind the API
  server knows, list its instances, and open one's YAML. Secret values are
  redacted (keys and sizes shown, contents masked), so secret contents never
  surface.

---

## Architecture & design

KuberNation is a Cargo workspace with a clean split:

- **`kubernation-core`** — the data + model layer, with **no UI dependencies**:
  the Kubernetes client and watch/reflector layer, and a set of **pure functions**
  that turn observed cluster state into render-ready models (the map geometry, the
  attention queue, SLOs, blast radius, chaos plans, advisor reports). Because this
  logic is pure, the interesting behaviour is unit-tested without a cluster or a
  display.
- **`kubernation`** — the windowed client (built on [macroquad](https://macroquad.rs/)):
  a background thread runs the watchers and publishes snapshots; the render loop
  draws the isometric world and panels, never blocking on the cluster.

**Data flow.** Reflectors keep an in-memory view of the cluster current and push
payload-free "something changed" signals through one channel. Input redraws
immediately (sub-100ms); cluster changes rebuild the models at a tick cadence
(250ms) — coalesced, so a noisy cluster can't make the UI lag.

**Posture.** Read-by-default; the entire write surface is one auditable file, every
write confirmed and RBAC-checked. There is deliberately **no exec/attach/shell**
(a graphical app can't host a PTY, and arbitrary exec would break the read-first
guarantee), and Secret contents are never surfaced. It's an operator-laptop tool —
it talks to a cluster through your kubeconfig and runs no in-cluster agent.

**Performance.** A full model rebuild (map + workloads + attention) at 100 nodes /
1000 pods takes ~1ms on an M4 Max (`make perf-test`). World rebuilds are coalesced
at a 250ms tick and input redraws stay sub-100ms, so a busy cluster never makes
the UI lag. A built-in rig stands a big synthetic cluster up:

```sh
make perf-up      # kwok-simulated: 100 nodes (5 zones), 1000 pods
make perf         # run the client against it
make perf-down
```

**The conceptual model.** The CNCF landscape's layers, reframed as concentric
zones of operator agency: provisioning is the continent (out of scope), runtime is
terrain (the node window), orchestration is the game board (the map), application
definition is what your cities produce (the city window), observability is a
property of every view, and platform metadata is the politics of the world (the
status line). The original design brief is
[kubernation-tui-mvp-prompt.md](kubernation-tui-mvp-prompt.md); the architecture
and the full decision log live in [CLAUDE.md](CLAUDE.md).

> **History.** A terminal (TUI) frontend shipped first and was removed in mid-2026
> to focus on the single windowed client — the headless-terminal niche is well
> served by k9s, and the map metaphor is inherently graphical. The pure
> `kubernation-core` was untouched by that change.

---

## Configuration

The client is driven by CLI flags — `--context`, `--kubeconfig`, `--warm <context>`,
`--project <crd>` (repeatable, to project a custom resource onto the islands), and
`--log-level`. Diagnostics are written to
`~/.local/state/kubernation/kubernation.log` (`RUST_LOG` is also honored). There is
no config file yet.

Projecting custom resources:

```sh
kubernation --context prod \
  --project certificates.cert-manager.io \
  --project gizmos.example.com
```

Each `--project` resolves the CRD at connect and watches its instances live; they
appear as `✦` structures on their namespace's island. A CRD that's absent on a
cluster is skipped quietly (so a hot/warm pair may project asymmetrically).

### RBAC requirements

KuberNation is **read-by-default**. To explore a cluster it needs `get` / `list` /
`watch` on the watched kinds — Nodes, Pods, Deployments, ReplicaSets, StatefulSets,
DaemonSets, Jobs, CronJobs, PersistentVolumeClaims, Services, Ingresses, Events, and
NetworkPolicies — plus `create` on **SelfSubjectAccessReview** (the read-only
`kubectl auth can-i` probe behind the Charter and the write-gating). A standard
read-only `ClusterRole` (or the built-in `view`) covers it. Optional: `get` on
`metrics.k8s.io` (live gauges; otherwise it derives scheduling pressure from
requests) and `get services/proxy` (only if you use `--opencost`).

The deliberate, gated **write** actions each need their own verb — if you lack it,
the control shows as *locked* (checked via `SelfSubjectAccessReview` before any
write): `delete pods` (evict, Game Day), `patch deployments/statefulsets/daemonsets`
and `patch nodes` (the planning turn — scale / restart / image / rollback / cordon),
`create pods/portforward` (port-forward), and `create networkpolicies` (a Game Day
network partition). See **Help ▸ Charter** in-app for exactly what *you* can do on
the current cluster.

### Troubleshooting

- **It won't connect / the world is fog.** It uses your kubeconfig exactly as
  `kubectl` does — confirm `kubectl --context <ctx> get nodes` works. A banner under
  the menu bar reports "connecting…" or "reconnecting to *ctx* — *reason*" when the
  API isn't answering.
- **Diagnostics / a crash.** Everything is logged to
  `~/.local/state/kubernation/kubernation.log` (set `RUST_LOG=debug` for more). If the
  background world loop ever crashes, a red banner says so (and the cause is in that
  log) — restart the app.
- **No cpu/mem gauges.** Install
  [metrics-server](https://github.com/kubernetes-sigs/metrics-server); without it the
  gauges show *scheduling pressure* (pod requests ÷ allocatable) instead of live usage.

---

## Status & roadmap

KuberNation is well past its MVP and in active development. **Built today:** the
isometric world map with overlays and a minimap; city/node drill-downs; the
attention queue; the Almanac and Advisor screens; log tailing (severity colours,
filters, timestamps, history, previous-container, concern→logs); metrics-server
live usage with CPU/memory trend sparklines; blast-radius impact highlighting;
availability SLOs + the error-budget treasury (per-workload targets); the resource
browser (any kind) and read-only YAML inspector; the hot/warm cluster pair; the
network/storage/batch/custom-resource map layers; RBAC-gated port-forward; and the
three write paths — pod eviction, the planning turn, and Game Day chaos (nine
experiments + difficulty tiers, with a steady-state/MTTD/recovery scorecard and
restore-on-exit).

**Deliberately deferred:** deeper chaos that needs a service mesh or in-cluster
agent (latency/CPU stress injection), persisted run history, external managed
services on the map, and the larger log tiers (multi-container picker, whole-app
multi-pod tailing). See [CLAUDE.md](CLAUDE.md) for the complete list and the
reasoning behind each decision.

---

## Copyright, trademark & inspiration

© 2026 Jason Olmsted. **KuberNation**™ and the KuberNation logo are unregistered
trademarks of Jason Olmsted. The software is dual-licensed under **MIT OR
Apache-2.0** (see `LICENSE-MIT` and `LICENSE-APACHE`).

*KuberNation is an independent, unaffiliated homage. It is not associated with,
endorsed by, or sponsored by Take-Two Interactive Software, Inc., Firaxis Games,
or the Civilization franchise. "Sid Meier's Civilization" and "Civ" are trademarks
of Take-Two Interactive, referenced here only to describe this project's design
inspiration.* Bundled fonts (Fira Sans, Liberation Serif, Liberation Mono) are
licensed under the SIL Open Font License 1.1; see `crates/kubernation/assets/CREDITS.md`.
Third-party crate licenses (mostly MIT/Apache-2.0, plus ISC/BSD-3-Clause/Zlib/Unicode-3.0)
are in `crates/kubernation/THIRD-PARTY-NOTICES.md`. The in-app **Help ▸ About** window
surfaces the same credits, licenses, and disclaimer.
