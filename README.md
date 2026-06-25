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
flies a warning flag, the right-column attention queue ranks what needs you, the
`✦` structures on the southern island are custom resources, and DaemonSets pave
roads across the provinces.*

---

## Why a map?

Most Kubernetes UIs — `kubectl`, and table/dashboard tools like k9s, Lens, or
Headlamp — present your cluster as lists you filter and sort. That works, but it
hides two things a map makes obvious: **where** things run (failure domains,
placement, drift between clusters) and **what matters right now**. KuberNation is
built around four ideas:

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
- **Ask a model — bring your own.** Consult a local or corporate LLM to explain a
  workload, a node, or the whole realm. It sees only a redacted, fenced summary
  KuberNation already built — never raw API dumps, never Secret values — shown to
  you in full before anything is sent. Any fix it proposes still goes through the
  same confirmed, dry-run-gated commit. The model proposes; you and the gate dispose.

---

## Install

**Pre-built binaries** are attached to each [GitHub release](../../releases) — a
macOS universal binary (Apple Silicon + Intel), a Linux x86_64 binary, and a
Windows x86_64 binary. Download, verify against `SHA256SUMS`, and run it against
your current kube-context:

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
node/pod counts, the concern roll-up, cluster CPU/memory trend, and a `DEFENSE`
security-posture chip), **ATTENTION** (the live problem queue — click a row to fly
there), **FORWARDS** (any live port-forwards), and **SELECTION** (whatever tile you
last clicked or are hovering). While a blast radius is active an **IMPACT** list of
affected dependents appears too — click one to fly to it.

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
· Wonders · World · Help) plus the docked right column and a cartographic map title.

### Drill-downs: cities and provinces

Click a city to open its **city window** — the workload in full: replica and
update gauges, a pod **census** grid, a clickable pod list, **improvements** it
owns (Services, Ingresses, PVCs, ConfigMap/Secret references), an availability
**treasury** (see [Reliability](#reliability-slos--the-error-budget-treasury)), a
rollout **history** with one-click **rollback**, and a merged **annals** of recent
changes and events.

![City window](docs/gui-city.png)

Click open land to open the matching **province (node) window**: zone and health,
CPU/memory gauges (with live-usage trend sparklines when metrics-server is
installed), the **garrison** of pods stationed there,
the node's **terrain** facts (container runtime, kubelet, OS, arch), and its
conditions.

![Province window](docs/gui-node.png)

### Logs

Click any pod row — in a city's citizens list or a province's garrison — to tail
its logs in a live overlay (refreshed every couple of seconds, rendered in a
monospace face so timestamps and columns line up). Lines are tinted by guessed
severity; `/` filters (space-separated terms AND together, `!term` excludes), `p`
shows the previous (crashed) container, `T` toggles timestamps, `s` widens the
history window, `c` copies, `w` exports. For a multi-container pod (sidecars, init
containers) a tab row at the top picks which container to tail. From the attention
queue, `L` opens the logs of the exact pod behind a concern.

![Log overlay](docs/gui-logs.png)

### Triage: the queue, the blast radius, and what changed

Three axes answer an incident, and KuberNation gives each a home. The **attention
queue** (the right column, or `N` to fly through it) says *what's wrong* — crash
loops, stalled rollouts, pending volumes, nodes under pressure, burning budgets,
security and segmentation gaps — aggregated per workload (a city in trouble, not
forty pod alarms) and ranked, each with a one-line next-action hint.

**Blast radius** (`B`) says *what else is affected*: it lights up the dependency
fan-out of a node or workload across the map — cities → harbors → gates — and
lists them in an **IMPACT** panel you can click to fly to. It's honest topology,
never invented "who-calls-whom" edges.

**The Annals** (View ▸ Annals, or `H`) say *what changed*: a classified change-feed
that merges Kubernetes events, Deployment rollouts, and your own actions, newest
first, with a "trouble begins here" fault line marking the first failure and
flagging the change just before it — adjacency, never fabricated causality. One
click exports a markdown **after-action report** (posture + open concerns + the
timeline + any Game Day drills) from the Annals or **Game ▸ Export after-action
report**.

![The Annals — what changed](docs/gui-annals.png)

Prefer a list to a map? The realm-wide **Workload table** (`O`, or View ▸
Workloads) sorts and filters every workload — CrashLoopBackOff floats to the top
under the health sort — and a click on any row opens its city.

![Workload table](docs/gui-workloads.png)

### Map views

The **View** menu recolours the whole board (and the minimap) like a strategy
game's map modes:

- **Terrain** — node health (the default).
- **Pressure** — CPU/memory heat per node.
- **Replicas** — the worst workload health on each node (red where a city is
  understrength).
- **Namespace** — a stable hue per namespace, a political/territory map.
- **Saturation (strain)** — how full each node is toward its *hard* limits: the
  worst of CPU/memory usage, scheduled-pods-vs-max-pods, and the kubelet's
  Disk/Memory/PID-pressure conditions. Catches the silent max-pods scheduling
  failure that Pressure stays green for.
- **Walls (segmentation)** — NetworkPolicy ingress coverage; an unwalled city
  (open to lateral movement) gets a breach notch, red when it's also reachable.
- **Upkeep (cost)** — what each node costs to run, a bronze choropleth with a gold
  coin on idle, reclaimable capacity. See [Cost](#cost-what-the-cluster-costs-to-run).

The active view is named in the title and STATUS so a recoloured map is never
mistaken for a health signal.

![Map views — the Namespace territory map](docs/gui-overlay.png)

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

The **Advisors** menu opens read-only summary reports of the whole realm — pure
functions of the observed cluster, always cluster-wide — that complement the
attention queue:

- **Health** — nodes by health, pods by phase, workloads at strength.
- **Storage** — PVCs bound vs. pending.
- **Network** — Services and Ingresses, orphaned Ingresses, idle Services, plus
  NetworkPolicy walls coverage.
- **Right-sizing** — per-replica resource requests vs. live metrics-server usage:
  over-provisioned (reclaimable waste), under-provisioned (throttle / OOM risk),
  and scheduler-blind (no requests) workloads, with a cluster-wide reclaimable
  total.
- **Hardening** — a pod-template security lint (privileged, host namespaces,
  dangerous capabilities, hostPath, run-as-root, missing limits, `:latest`, …),
  each finding tagged with the Pod Security Standard / OWASP control it maps to.
- **Posture** — a 0–100 "realm defense" score rolling up Hardening + Walls, with
  the ranked factors behind it.
- **Cost** — the upkeep roll-up (total · by namespace · costliest workloads ·
  idle). See [Cost](#cost-what-the-cluster-costs-to-run).

![Advisors — the Right-sizing report](docs/gui-advisors.png)

### The Oracle — ask a model about your cluster (bring your own LLM)

The **Wonders ▸ Oracle** consults a language model *you* bring to explain a scope —
the whole realm, a selected workload or node, or a focused concern. It's advisory
and tightly bounded:

- **It sees a redacted, fenced summary, never your cluster.** The prompt is built
  from what KuberNation already observed (attention, the not-ready diagnosis, blast
  radius, rollout history, posture, and the offending pod's recent logs for a
  crash) — never raw API dumps, never Secret values — and a **mandatory Preview**
  shows you the exact bytes before anything is sent.
- **Bring your own endpoint.** Point it at a local model (Ollama is the default) or
  any OpenAI-compatible / corporate endpoint via **Wonders ▸ Oracle ▸ Settings** —
  named profiles with a model picker and a connection test. Local is the default;
  a *remote* endpoint publishes off your laptop, so it stays behind an explicit
  per-session **"Arm remote egress"** gate and you send the exact bytes you
  previewed.
- **It streams its answer** and ends with one-click **CONSULT NEXT** links (drill
  into the worst concern) and **INVESTIGATE FURTHER** lenses (fold in logs /
  storage / blast / rollout for a deeper look).
- **It can propose a fix, but never applies one.** A suggestion (scale / restart /
  image / rollback / cordon) is validated against the live cluster and offered as a
  **Stage** button — it enters the planning turn and commits only through the same
  confirmed, RBAC-checked, server-side-dry-run gate. The model proposes; you and
  the gate dispose.

This is the only network call KuberNation makes that leaves your laptop for a
third-party endpoint (OpenCost, below, is read through your existing cluster
connection) — opt-in, off by default for remote, gated in the same spirit as
port-forward. Replies are model-generated; verify before acting.

![The Oracle — the mandatory pre-send Preview](docs/gui-oracle.png)

### Cost: what the cluster costs to run

The **Upkeep** map view (View ▸ Upkeep) paints each province by what its node costs
and drops a gold coin on idle, reclaimable capacity; the **Advisors ▸ Cost** tab
rolls it up by namespace and workload. It's honest by construction: with no pricing
it shows a relative **"cost units"** score derived from resource requests (works on
any cluster, never a fake `$`). Supply rates — `--cpu-rate` / `--mem-rate` /
`--node-rate`, or a `kubernation.io/cost-hourly` node annotation — and it shows real
`$/hr` and a `~$/mo` estimate (your rates × reservation, *not* a cloud invoice).
Point `--opencost` at an in-cluster [OpenCost](https://opencost.io) install and it
reports invoice-grade, amortized cost (network, load balancers, storage, spot /
reserved discounts), read-only through the kube API-server proxy.

![Cost cartography — the Upkeep overlay](docs/gui-cost.png)

### Security & posture

A small security suite, all read-only and all reusing what KuberNation already
watches:

- **The Charter** (Help ▸ Charter) — a self-scoped `can-i` grid of what *you* can
  do on this cluster (✓ allowed / ✗ denied / ? unknown), with allowed *dangerous*
  capabilities flagged. It's the exact read-only `SelfSubjectAccessReview`
  mechanism `kubectl auth can-i` uses — it kills surprise 403s.
- **Hardening scan** (Advisors ▸ Hardening) — lints every workload's pod template
  against the Pod Security Standards and OWASP Kubernetes Top-10; a Critical
  finding also raises one aggregated attention-queue concern per workload.
- **Walls** (NetworkPolicy coverage) — unwalled-and-exposed cities surface on the
  map (View ▸ Walls) and in the Network advisor — OWASP K07, missing segmentation.
- **Posture score** (Advisors ▸ Posture) — a 0–100 "realm defense" rating and tier
  rolling up the two scans, surfaced as a `DEFENSE` chip in the STATUS column.

![The Charter — what you can do here](docs/gui-charter.png)

### Reliability: SLOs & the error-budget treasury

Each city window shows an availability **SLO** and the **error budget** it spends
down — a coin gauge that's full when the workload stays up, drains when it flaps,
and is exhausted when availability falls below target. Availability is derived
from pod readiness over a recent in-session window (about 8 minutes, not a 30-day
compliance window), so it needs **no Prometheus or metrics-server** — it works on
any cluster. Set a per-workload target with the
in-window stepper or a `kubernation.io/slo-target` annotation; a burning or
exhausted budget also raises an attention-queue concern. A **fast** burn (severe
and down right now) pages as a **Critical** concern; a slow, sustained burn
tickets as a **Warning** — the SRE multi-window burn-rate pattern, with gates so a
one-sample blip or an already-recovered incident stays quiet.

### Acting on the cluster

KuberNation performs only a handful of writes, each explicit, confirmed, and
RBAC-checked — and all of them live in one small file (`crates/kubernation-core/src/k8s/actions.rs`).

**Evict a pod.** Hover a pod in a city's citizens (or node's garrison) list and an
`evict` button appears; on confirm, KuberNation issues a real `DELETE` (a managed
pod is recreated by its controller; a bare pod is gone). The button is disabled
(`locked`) unless an RBAC check says you may delete pods there.

**The planning turn.** Changes are *staged*, not applied imperatively. Step a
city's replicas, or stage a cordon / rolling restart / image change / revision
rollback; press `t`
(or **Orders ▸ End of Turn**) for a from→to review of everything staged, with
per-row unstage and discard. **Commit** validates every change with a server-side
dry-run first (which also enforces RBAC), so a change the cluster would reject is
blocked before anything is written — all-or-nothing. Staging never writes; only
Commit does.

![Planning turn](docs/gui-plan.png)

**Game Day (chaos).** The **Game Day** menu opens a chaos-engineering console:
inject a *real* failure and watch the cluster respond — the attention queue lights
up ("raid underway"), the blast radius spreads across the map, the error budget
spends. Pick a target and one of **nine experiments** (kill one / a percentage /
all pods, outage, scale spike, broken image, node failure, cordon freeze, network
partition), or a compound **difficulty tier** — Skirmish, Raid, or Siege — that
sequences several into one drill.

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
sync, replica or image drift, or present on only one side), tooltips and windows
are tagged HOT/WARM, and the attention queue merges both worlds (entries tagged
`[H]`/`[W]`) plus a single aggregate "drift" concern.

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

For a red-green-safe palette, toggle **View ▸ Colour-blind palette** (or launch
with `--colorblind`): the "healthy" greens become a steel blue, so blue / amber /
red are all distinguishable; red and amber are unchanged. Your choice — and your
last map view — is remembered across runs.

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
| `O` · `H` | the realm-wide workload table · the Annals (what changed) |
| `:` | resource browser — list/inspect *any* kind |
| `t` | the End-of-Turn planning review |
| `c` · `Esc` | switch cluster context · close the topmost overlay |
| `?` / `F1` | the Almanac |
| menu bar | context · fit · map views · namespace filter · advisors · Game Day · the Oracle (Wonders) · quit |

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
  attention queue, SLOs, blast radius, chaos plans, advisor reports, cost, the
  posture score, the change timeline, and the redacted Oracle context bundle).
  Because this logic is pure, the interesting behaviour is unit-tested without a
  cluster or a display.
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
it talks to a cluster through your kubeconfig and runs no in-cluster agent. The one
network exception is the optional Oracle, which publishes a redacted, fenced
summary to a language-model endpoint *you* configure — opt-in, off by default for
remote, and gated like port-forward; everything else stays cluster-only.

**Performance.** A full model rebuild (map + workloads + attention) at **500 nodes
/ 5000 pods** takes **~4ms** on an M4 Max (`make perf-test`) — well under the 100ms
budget. World rebuilds are coalesced at a 250ms tick and input redraws stay
sub-100ms, so a busy cluster never makes the UI lag. A built-in rig stands a
smaller live cluster up for interactive testing:

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
status line). The original (TUI-era) design brief and the full decision log are in
[kubernation-tui-mvp-prompt.md](kubernation-tui-mvp-prompt.md) and
[CLAUDE.md](CLAUDE.md) — historical, internal references.

> **History.** A terminal (TUI) frontend shipped first and was removed in mid-2026
> to focus on the single windowed client — the headless-terminal niche is well
> served by k9s, and the map metaphor is inherently graphical. The pure
> `kubernation-core` was untouched by that change.

---

## Configuration

The client is driven by CLI flags — `--context`, `--kubeconfig`, `--namespace <ns>`
(launch scoped to one namespace; changeable at runtime via World ▸ Namespace
filter), `--warm <context>`,
`--project <crd>` (repeatable, to project a custom resource onto the islands),
`--log-level`, `--colorblind`, `--overlay <terrain|pressure|replicas|namespace|walls|saturation|cost>`,
`--slo-target`, the cost flags (`--cpu-rate` / `--mem-rate` / `--node-rate` /
`--opencost [ns/svc:port]` / `--opencost-window`), and the Oracle flags
(`--llm-url` / `--llm-model`; the API token comes from the `KUBERNATION_LLM_TOKEN`
environment variable, never a flag).

It persists two small JSON files under `~/.config/kubernation/`: **`oracle.json`**
(Oracle endpoint profiles — written `0600`, with optional opt-in token persistence)
and **`prefs.json`** (UI preferences: the colour-blind palette and your last map
view; CLI flags always override). The kube context and namespace filter are
deliberately *not* persisted — they follow your kubeconfig on each launch.
Diagnostics are written to `~/.local/state/kubernation/kubernation.log`
(`RUST_LOG` is also honored).

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

KuberNation is well past its MVP and approaching a 1.0 release. **Built today:**
the isometric world map with **seven map views** (terrain, pressure, replicas,
namespace, walls, saturation, upkeep) and a minimap; city/node drill-downs with
rollout history and rollback; the realm-wide **workload table**; the attention
queue with runbook hints, the **blast-radius impact** panel, and the **Annals**
change timeline; the Almanac and **seven Advisor tabs** (Health, Storage, Network,
Right-sizing, Hardening, Posture, Cost); the **Oracle** bring-your-own-LLM Wonder
(explain · streaming · suggest-to-gate, local or gated-remote); **cost cartography**
+ OpenCost; the **security suite** (Charter, hardening scan, NetworkPolicy walls,
realm-defense posture score); log tailing (severity colours, filters, timestamps,
history, previous-container, multi-container picker, concern→logs); metrics-server
live usage with CPU/memory trend sparklines; availability SLOs + the error-budget
treasury with **multi-burn-rate alerting**; the resource browser (any kind) and
read-only YAML inspector; the hot/warm cluster pair; the network/storage/batch/
custom-resource map layers; RBAC-gated port-forward; **postmortem export**; a
**colour-blind palette** + persisted preferences; a connection banner and
crash-safety panic logging; and the three write paths — pod eviction, the planning
turn (scale / cordon / restart / image / rollback), and Game Day chaos (nine
experiments + difficulty tiers, with a steady-state / MTTD / recovery scorecard and
restore-on-exit).

**Deliberately deferred:** deeper chaos that needs a service mesh or in-cluster
agent (latency / CPU stress injection), persisted run history, external managed
services on the map, and whole-app multi-pod log tailing. See [CLAUDE.md](CLAUDE.md)
for the complete list and the reasoning behind each decision.

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
