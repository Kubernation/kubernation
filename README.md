# K8sCiv

**The cluster as a living world.** A terminal UI for observing Kubernetes,
built on the interface grammar of early Sid Meier's Civilization: a 2D
world you explore — zones are continents, nodes are provinces of terrain,
workloads are cities sited where their pods run — plus a "city screen"
giving one workload full context, and an attention queue that brings
problems to you — the *next unit needing orders* — instead of making you
go hunting through dashboards.

This is not a retro skin on k9s. It is a different operator model:

- **Spatial, not tabular.** Your resources project onto a stable world
  map; geography means something (failure domains, placement, drift).
- **Attention-driven.** Failing pods, stuck rollouts, pending PVCs, nodes
  under pressure — aggregated, ranked, and one keypress (`n`) from their
  full context.
- **Observe-only (for now).** The MVP contains no mutation paths at all.
  The architecture reserves a `PlannedWorld` for a future planning-turn
  model: staged diffs committed deliberately, like ending a turn.

```text
 K8SCIV ▏kind-k8sciv ▏kind ▏https://127.0.0.1:50970/ ▏4n·25p                     overlay PRESSURE ▏? help
~≈ z-a · 1 ≈                   ≈ z-b · 1 ≈                  ≈ z-c · 1 ≈        ┌ WORLD ────────────┐
 ▣ k8sciv-worker ●5 ≣3      ~  ▣ k8sciv-worker2 ●6 ≣3       ▣ k8sciv-worker3   │ ┌              ┐  │
 ,    ,    ,    ,    ,         ,   ◍0‼   ,    ,    ,    ~   ,    ,    ,    ,   │  ▪ ▪ ▪ ▪          │
    ,    ,    ,    ,    ,  ~      ,crashy   ,    ,          ,    ,   ◍2   ,    │ └              ┘  │
                 ~              ,◍3  ,    ,    ,            ,  coredns  ,      ├ STATUS ───────────┤
       ~                  ~   ,  web    ,    ,    ,    ~    ,    ,    ,    ,   │ 4 provinces 6 cities
                ~                ◍2   ,    ,    ,           ~                  │ 25 pods  ‼1 !1 ·1
      ~                  ~     , db ,    ,    ,    ,                  ~        ├ ORDERS ───────────┤
  ≈ k8sciv-demo ≈  ·          ~                  ~                  ~          │ ◍ crashy
   ✦ gizmo/alpha-frob…                  ~                  ~                   │ pop 0 of 2 desired
 · ✦ gizmo/beta-frobn…     ~                  ~                  ~             │ ‼ needs attention
┌ ATTENTION (3) ───────────────────────────────────────────────────────────────────────────────────┐
│▸‼ deploy k8sciv-demo/crashy — CrashLoopBackOff ×2     0/2 ready · rollout Progressing (0/2)      │
│ ! pvc k8sciv-demo/stuck-pvc — Pending                 storageClass does-not-exist                │
└──────────────────────────────────────────────── n cycles · Tab focuses · a collapses ────────────┘
```

*(Real capture from `make dev` — crashy's city flies a `‼` flag with
population 0, the `✦` structures on the isle of k8sciv-demo are live
custom resources, and `≣3` marks three daemonset roads per province.)*

## Quick start

Requirements: Rust (stable), Docker, `kind`, `kubectl`.

```sh
make dev          # create a 4-node kind cluster, apply samples, launch TUI
```

Or against any cluster you can already reach:

```sh
cargo run --release -- --context <kubeconfig-context>
```

Useful targets: `make smoke` (headless connect + world summary),
`make lint`, `make test`, `make kind-down`.

### Hot/warm pair

```sh
make warm-up warm-drift   # second kind cluster + deliberate drift
make pair                 # both worlds side by side
```

Or against real clusters: `k8sciv --context prod --warm prod-standby`.
The map splits into two continents (`h`/`l` past the edge crosses over),
the workload list gains a SYNC column (`=` in sync, `≠r` replica drift,
`≠i` image drift, `−w` missing on warm), the city screen gets a pair line,
and the attention queue merges both worlds with `H`/`W` tags plus a single
aggregate drift concern.

### GUI client (windowed)

![GUI client](docs/gui-world.png)

```sh
make gui    # or: cargo run -p k8sciv-gui --release -- --context <ctx>
```

The same `k8sciv-core` world rendered as a real strategy-game view
(macroquad). Zones are **continents with irregular, noise-carved
coastlines** — bays, capes, sand beaches — so a cluster reads as
geography rather than a grid (the rectangular model underneath stays the
canonical coordinate system; the GUI just paints organic shores over it).
**Kenney CC0 sprite terrain** (tiled grass/sand/stone keyed to node
health, trees on healthy land); **building sprites** that grow from a
single house to a walled keep with population, with Civ-style white pop
chips and warning banners over troubled cities; namespace isles with
structure sprites, hover tooltips, right-drag panning, wheel zoom around
the cursor, minimap click-to-jump, smooth camera flights on `]`/`[` and
`N`, and in-window detail panels: click a city for its city screen (pods,
owned resources, recent events), click land for the node panel
(conditions, request gauges, pods). **Click any pod row** in an open panel
to tail its logs in a live overlay (refreshed every couple of seconds):

![GUI logs](docs/gui-logs.png)

Press **`c`** to switch the hot
cluster from a context picker — no restart. Text is **Fira Sans**
(bundled OFL); both font and sprites are embedded so the binary is
self-contained. Swap the look with `--tileset <dir>` (PNGs named `grass`,
`house`, `keep`, … override the bundled set, Freeciv-style).

With `--warm` (`make gui-pair`) the standby cluster rises as a **second
archipelago** east of the hot one — one sea, free panning between them,
`F` fits both on screen:

![GUI pair](docs/gui-pair.png)

Every city carries a sync chip beside its population box (`=` in sync,
`#r`/`#i` drift, `-w` missing on warm), tooltips and panels are tagged
HOT/WARM, the city panel gains a pair line, and the attention strip
merges both worlds with `[H]`/`[W]` tags plus the single aggregate
drift concern.

### Performance rig

```sh
make perf-up      # kwok-simulated cluster: 100 nodes (5 zones), 1000 pods
make perf         # run the TUI against it
make perf-test    # release-mode budget test: rebuild + frame < 100ms
make perf-down
```

Measured on an M4 Max: a full world rebuild (map + workloads + attention)
plus a rendered 140×40 frame at 100 nodes / 1000 pods takes **~0.5ms
average, <1ms worst** (`make perf-test`); against the live kwok cluster, 40
freshly scaled-up pods were reflected in the UI **81ms** after `kubectl
scale` returned. Input redraws immediately; world churn coalesces at the
tick (250ms default), so a noisy cluster can never make typing lag.

## Keys

| Key | Action |
| --- | ------ |
| `h j k l` / arrows | move cursor / selection |
| `]` / `[` | sail to next / previous city |
| `PgUp/PgDn` · `Ctrl+u/d` · `Home/End` | page the map · half page · west/east continent |
| `Enter` | open the thing under the cursor |
| `l` | tail the selected pod's logs (city / node screen) |
| `Esc` / `Backspace` | back |
| `m` / `w` | map · workload list |
| `n` | **next concern** — jump to the top problem's view |
| `a` / `Tab` | expand attention panel · focus it |
| `1` `2` `3` | map overlay: pressure · replica health · namespace |
| `c` | switch kube context |
| `?` | full keymap |
| `q` / `Ctrl-C` | quit |

## Reading the world

```
≈ z-a · 1 ≈                    ≈ z-b · 1 ≈
 ▣ k8sciv-worker ●5 ≣3      ~  ▣ k8sciv-worker2 ●6 ≣3
 ,    ,    ,    ,    ,         ,   ◍0‼   ,    ,    ,
    ,    ,    ,    ,    ,  ~      ,crashy   ,    ,
                 ~              ,◍3  ,    ,    ,
       ~                  ~   ,  web    ,    ,    ,
  ≈ k8sciv-demo ≈  ·   ~
   ✦ gizmo/alpha-frob…          ~
 · ✦ gizmo/beta-frobn…
```

Zones are **continents**; each node is a **province** of land whose
terrain texture tells its state (`,` grass · `=` cordon fence · `∩`
drought/pressure · `×` wasteland/NotReady). Workloads are **cities**
(`◍N` — population = ready replicas, flagged `‼`/`!` when concerning)
sited on the province hosting most of their pods, so a city *migrates
when its pods do*. DaemonSets pave `≣` roads instead of building cities.
Anything with no place on the land — projected custom resources (`✦`) and
zero-pod workloads (`◌`) — lives on **namespace islands** in the southern
sea. Walk anywhere with `h/j/k/l`; `]`/`[` sail city to city; `Enter`
opens whatever you stand on.

Pods keep their glyphs in city and node screens: `●` ready · `◐` starting
· `○` pending · `◌` terminating · `✗` failing · `◆` succeeded. The cpu/mem
gauges show **scheduling pressure** (requests ÷ allocatable) by default;
calm is green, elevated (≥70%) yellow, high (≥90%) red. Install
metrics-server (`make metrics-up`) and the gauges switch automatically to
**live usage** — the status bar reads `gauges live`, node detail shows
`cpu use`, the GUI panel says `live usage`. No metrics-server, no problem:
it falls back to requests on its own.

On terminals ≥110 columns the map gains the Civ sidebar: **WORLD** (the
chart: green land on blue ocean, `┌┐└┘` framing your viewport), **STATUS**
(provinces/cities/pods/concerns — your people and gold), and **ORDERS**
(whatever the cursor stands on — city, province, structure, or open sea).
Narrower terminals get a floating WORLD chart instead.

### Projecting custom resources

```sh
k8sciv --context prod --project certificates.cert-manager.io --project gizmos.example.com
```

Each `--project` (or config `projections = [...]`) resolves the CRD at
connect and watches its instances live; they appear as `✦` structures on
their namespace's island. CRDs absent on a cluster are skipped quietly —
a hot/warm pair may project asymmetrically.

The default palette is **civ**: parchment chrome, green terrain, white
city labels, blue ocean — with red and yellow strictly reserved for things
needing attention. Prefer the old restrained look? `color = "plain"` in
`~/.config/k8sciv/config.toml`; `color = "mono"` for no color at all.

## The conceptual model

The CNCF landscape's layers, reframed as concentric zones of operator
agency: provisioning is the continent (out of scope), runtime is terrain
(node detail), orchestration is the game board (the map), app definition is
what your cities produce (the city screen), observability is a property of
every view, platforms are the politics of the world (status bar). The full
design brief is in [k8s-civ-tui-mvp-prompt.md](k8s-civ-tui-mvp-prompt.md);
architecture and decisions live in [CLAUDE.md](CLAUDE.md).

## Configuration

Optional file at `~/.config/k8sciv/config.toml`:

```toml
tick_ms = 250              # world-change coalescing cadence
color = "auto"             # "auto" (civ) | "plain" | "mono"
attention_expanded = false # start with the panel expanded
```

CLI: `--context`, `--kubeconfig`, `--log-level`, `--smoke`. Diagnostics go
to `~/.local/state/k8sciv/k8sciv.log` — never stderr, which would corrupt
the TUI.

## Status

Observe-only MVP, with several post-MVP features built: hot/warm cluster
pairs, metrics-server live usage, the minimap, and pod log tailing (`l` on
a city or node; click a pod row in the GUI). Deferred, by design: mutations
and the planning-turn diff UI, external managed services, chaos layers, and
Job/CronJob city screens. See CLAUDE.md for the full list and the reasoning.
