# K8sCiv

**The cluster as a living map.** A terminal UI for observing Kubernetes,
built on the interface grammar of early Sid Meier's Civilization: a spatial
main view of node tiles grouped by failure zone, a "city screen" giving one
workload full context, and an attention queue that brings problems to you —
the *next unit needing orders* — instead of making you go hunting through
dashboards.

This is not a retro skin on k9s. It is a different operator model:

- **Spatial, not tabular.** Nodes are terrain tiles in zone columns, laid
  out by stable hash so the map never reshuffles under your eyes.
- **Attention-driven.** Failing pods, stuck rollouts, pending PVCs, nodes
  under pressure — aggregated, ranked, and one keypress (`n`) from their
  full context.
- **Observe-only (for now).** The MVP contains no mutation paths at all.
  The architecture reserves a `PlannedWorld` for a future planning-turn
  model: staged diffs committed deliberately, like ending a turn.

```text
 K8SCIV ▏kind-k8sciv ▏kind ▏https://127.0.0.1:50970/ ▏nodes 4 · pods 26                  overlay PRESSURE ▏? help
─ z-a · 1 ────────────  ─ z-b · 1 ────────────  ─ z-c · 1 ────────────  ─ unzoned · 1 ────────
▣ k8sciv-worker         ▣ k8sciv-worker2        ▣ k8sciv-worker3        ▣ k8sciv-control-pl…
c ░░░░░░░░░░░░░░   1%   c ░░░░░░░░░░░░░░   1%   c ░░░░░░░░░░░░░░   1%   c ▓░░░░░░░░░░░░░   6%
m ░░░░░░░░░░░░░░   1%   m ░░░░░░░░░░░░░░   1%   m ░░░░░░░░░░░░░░   1%   m ░░░░░░░░░░░░░░   2%
◌●●●●●              6p  ●✗●●●●              6p  ●✗●●●               5p  ●●●●●●●●●           9p

┌ ATTENTION (10) ────────────────────────────────────────────────────────────────────────────────┐
│▸‼ deploy k8sciv-demo/crashy — CrashLoopBackOff ×2     0/2 ready · rollout Progressing (0/2)    │
│ ! pvc k8sciv-demo/stuck-pvc — Pending                 storageClass does-not-exist              │
│ · events: ProvisioningFailed ×42 on persistentvolumeclaim k8sciv-demo/stuck-pvc                │
│ · events: Unhealthy ×2 on pod kube-system/coredns-589f44dc88-rxdp2                             │
└─────────────────────────────────────────────── n cycles · Tab focuses · a collapses ───────────┘
```

*(Real capture from `make dev` — the `✗` glyphs are the intentionally
crash-looping sample deployment, and `◌` is a pod mid-termination.)*

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
| `Enter` | open the thing under the cursor |
| `Esc` / `Backspace` | back |
| `m` / `w` | map · workload list |
| `n` | **next concern** — jump to the top problem's view |
| `a` / `Tab` | expand attention panel · focus it |
| `1` `2` `3` | map overlay: pressure · replica health · namespace |
| `c` | switch kube context |
| `?` | full keymap |
| `q` / `Ctrl-C` | quit |

## Reading the map

| Glyph | Node | | Glyph | Pod |
| --- | --- | --- | --- | --- |
| `▣` | healthy | | `●` | running & ready |
| `▤` | cordoned | | `◐` | running, not ready |
| `▥` | under pressure | | `○` | pending |
| `▦` | NotReady | | `◌` | terminating |
| | | | `✗` | failing |
| | | | `◆` | succeeded |

The `c`/`m` gauges show **scheduling pressure** — the sum of pod resource
*requests* on the node versus allocatable — not live usage. Calm is gray,
elevated (≥70%) is yellow, high (≥90%) is red. Color is reserved for
meaning: a running pod is the absence of red, not green.

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
color = "auto"             # "auto" | "mono"
attention_expanded = false # start with the panel expanded
```

CLI: `--context`, `--kubeconfig`, `--log-level`, `--smoke`. Diagnostics go
to `~/.local/state/k8sciv/k8sciv.log` — never stderr, which would corrupt
the TUI.

## Status

Observe-only MVP. Deferred, by design: mutations and the planning-turn diff
UI, hot/warm cluster pairs, metrics-server live usage, minimap, external
managed services, chaos layers, logs. See CLAUDE.md for the full list and
the reasoning.
