# K8sCiv

A Civilization-inspired Kubernetes TUI. The cluster is a living map: nodes
are terrain tiles grouped into zone columns, workloads are cities with a
full-context "city screen", and an attention queue surfaces what needs the
operator's focus — Civ's "next unit needing orders", not a wall of dashboards.
**MVP is observe-only.** No mutation paths exist anywhere in the codebase.

The full product brief lives in `k8s-civ-tui-mvp-prompt.md`. Read it before
proposing scope changes.

## Conceptual model (the short version)

CNCF landscape layers reframed as concentric zones of operator agency:

| Layer          | In K8sCiv                                                |
| -------------- | -------------------------------------------------------- |
| Provisioning   | the continent — out of scope for MVP                      |
| Runtime        | terrain attributes per node (shown in node detail)        |
| Orchestration  | the main game board (map view)                            |
| App Definition | what cities produce (city screen)                         |
| Observability  | a property of every view, not a separate view             |
| Platforms      | cluster metadata (status bar platform hint)               |

Future (designed-for, not built): hot/warm cluster pair as two continents,
external managed services as foreign powers, chaos events as barbarian
raids, and the planning-turn staged-diff intervention model.

## Architecture

```
src/
  main.rs        entry; clap CLI; --smoke headless mode; terminal lifecycle
  app.rs         composition root: event loop, screen stack, context switch
  events.rs      AppEvent/WorldDelta; blocking input thread → tokio channel
  logging.rs     tracing → file (never stderr; stderr corrupts the TUI)
  util.rs        fnv1a64 stable hash, age/bytes formatting
  config/        Config (~/.config/k8sciv/config.toml) + clap Args
  k8s/           DATA LAYER: client+platform detect, quantity parsing,
                 reflector spawning (watch.rs)
  state/         observed.rs  ObservedWorld (reflector stores + event ring)
                 planned.rs   PlannedWorld stub (future planning turn)
                 model.rs     PURE derivations: map/workloads/city/node models
                 attention.rs PURE detectors → severity-ordered concerns
                 fixtures.rs  test-only synthetic worlds
  ui/            components implementing the Component trait
                 (map, workloads, city, node_detail, attention_panel,
                  status_bar, help, context_picker, theme, symbols)
```

**Data flow:** watchers (kube 3.x reflectors) keep `ObservedWorld` stores
current and push payload-free `WorldDelta` dirty-bits through one mpsc
channel. The app coalesces: input events redraw immediately (sub-100ms);
world changes rebuild `Models` (pure functions of the world) at tick cadence
(250ms default). Detail views re-derive their own models in `update()`.

**Separation rule:** everything in `state/model.rs` and `state/attention.rs`
must remain a pure function of `ObservedWorld` — no I/O, no `Store` writes,
no clock reads except where explicitly windowed (event recency). That is
what makes the interesting logic unit-testable without a cluster.

## Decisions log

- **Pressure = requests ÷ allocatable** (user's call, 2026-06-12): the map's
  cpu/mem gauges show *scheduling pressure* from pod requests, not live
  usage. Always computable from core API objects; kind needs no
  metrics-server. Buckets: <0.7 calm, 0.7–0.9 elevated (yellow), ≥0.9 high
  (red) — shared constants in `state/model.rs`. metrics-server actuals are a
  planned upgrade behind the same gauge interface.
- **Stable layout:** nodes sort within a zone by FNV-1a-64(name) — pinned by
  test so layouts never reshuffle across runs or Rust upgrades. Zones sort
  by name; `unzoned` sinks to the end.
- **Zone label:** `topology.kubernetes.io/zone` with legacy
  `failure-domain.beta.kubernetes.io/zone` fallback. kind has no zone labels,
  so `hack/kind-config.yaml` bakes z-a/z-b/z-c onto the workers.
- **Watched resources:** Node, Pod, Deployment, ReplicaSet (ownership chain +
  rollout), StatefulSet, DaemonSet, PVC, Service, Event. **Secrets and
  ConfigMaps are never watched** — the city screen derives their *names*
  from pod-template references, so we observe dependency shape without
  reading contents (least privilege).
- **Events:** no reflector store; a bounded ring (500) deduped by
  (kind, ns, name, reason). Attention considers Warning events from the last
  15 minutes, skipping objects already covered by a stronger concern.
- **Attention aggregation:** pod-level failures aggregate per owning
  workload ("city in trouble", not 40 pod alarms). Severity: container
  crash/image/config failures and stalled rollouts are Critical; replica
  gaps, unschedulable, OOM-kills, flapping (≥5 restarts), pressure, pending
  PVCs are Warning; cordons and grouped events are Info. Jobs have no city
  screen yet, so Job-pod concerns target the pod's node.
- **kube 3.1 / k8s-openapi 0.27:** k8s-openapi re-exports **jiff** (not
  chrono) for time. `ratatui-crossterm` exposes no `event-stream` feature,
  so terminal input runs on a dedicated blocking thread feeding the tokio
  loop; crossterm types come only from `ratatui_crossterm::crossterm` to
  avoid version skew.
- **Multi-cluster readiness:** `ObservedWorld` + its informer set
  (`WorldHandle`, abort-on-drop) are per-context. Context switch = connect,
  spawn new handle, drop old. A hot/warm pair later is "hold two handles",
  not a refactor.
- **Platform hint:** kubeconfig heuristics first, refined by the first
  observed node's `spec.providerID` (aws/gce/azure/kind/k3s prefixes).
- **In-cluster config is not supported** (operator-laptop tool); revisit if
  a read-only web/agent mode ever appears.
- **Minimap** (2026-06-12): auto-appears bottom-right of the map only when
  the board exceeds the viewport — no toggle, no config. One cell per node
  in zone columns; when a zone is taller than the panel, `k` nodes collapse
  into one cell with worst-state-wins coloring. The viewport frame hugs the
  first/last visible cell rows exactly (there is no half-row to sit
  between). It bails out silently rather than smothering the board when
  zones are too numerous to fit (~60+ zones — horizontal compression is a
  future step).
- **`Store::wait_until_ready` allows ONE concurrent waiter per store** (found
  2026-06-12): kube's readiness uses a `DelayedInit` over a futures oneshot
  receiver, which holds a single waker slot. Two tasks awaiting the same
  store race on that slot and the loser is never woken (it stalls until some
  unrelated timer re-polls it — we saw exactly-20s smoke runs). The
  readiness-notifier task in `k8s/watch.rs` is therefore the *only* caller;
  everything else (TUI and `--smoke` alike) listens for
  `WorldDelta::Ready` on the event channel. Don't add new
  `wait_until_ready` call sites.

## Symbol grammar (do not improvise new glyphs)

| Glyph | Meaning                       |
| ----- | ----------------------------- |
| `▣`   | node healthy                  |
| `▤`   | node cordoned                 |
| `▥`   | node under pressure           |
| `▦`   | node NotReady                 |
| `●`   | pod running & ready           |
| `◐`   | pod running, not ready        |
| `○`   | pod pending                   |
| `◌`   | pod terminating               |
| `✗`   | pod failing (crash/image/...) |
| `◆`   | pod succeeded (completed)     |
| `‼ ! ·` | critical / warning / info   |
| `▓░`  | gauge filled / empty          |
| `·` / `▪` | minimap node cell: healthy / degraded (colored by worst state) |
| `┌┐└┘` | minimap viewport frame (reversed cell = cursor) |

Health precedence on a tile: NotReady > Cordoned > Pressure > Healthy.
Zone headers carry a `▪N` rollup (colored by the zone's worst node) when
any node in the zone is degraded.

**Color discipline:** color encodes meaning, never decoration. Running is
the *absence* of red, not green. Saturated red/yellow are reserved for
attention. Namespace overlay uses a muted no-red palette. Mono mode
(`color = "mono"`) carries the same meanings via bold/dim/reverse only.
All colors are named ANSI — safe on 256-color terminals.

## Keymap

`h/j/k/l`+arrows move · `PgUp/PgDn` page, `Ctrl+u/d` half page,
`Home/End` first/last zone · `Enter` opens · `Esc` back · `m` map ·
`w` workloads · `n` next concern · `a` attention panel · `Tab` focus panel ·
`c` context picker · `1/2/3` overlays (pressure/replicas/namespace) ·
`?` keymap · `q`/Ctrl-C quit. Keep `help.rs` in sync with any change.

## Dev loop

```
make dev        # kind-up + samples + run (the standard loop)
make smoke      # headless: connect, print world summary, exit (CI gate)
make lint test  # fmt --check, clippy -D warnings, cargo test
make kind-down

make perf-up    # kwok-simulated 100-node / 1000-pod cluster (needs kwokctl)
make perf       # run the TUI against it
make perf-test  # release-mode rebuild+frame budget test (<100ms asserted)
make perf-down
```

Develop against kind only (`hack/kind-config.yaml`, cluster `k8sciv`,
context `kind-k8sciv`). `hack/samples.yaml` provides: healthy `web`
(+Service), crash-looping `crashy`, StatefulSet `db` (+PVCs), DaemonSet
`agent`, and `stuck-pvc` which never binds (keeps one Warning in the queue).

Logs: `~/.local/state/k8sciv/k8sciv.log` (`--log-level`, `RUST_LOG`).
Config: `~/.config/k8sciv/config.toml` (`tick_ms`, `color`,
`attention_expanded`) — all optional.

## Conventions

- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean
  before any commit.
- New state logic ships with unit tests against `state/fixtures.rs`; new
  views ship with a TestBackend snapshot-style test asserting rendered
  content.
- Commit in working states with descriptive messages; the user reviews
  commits.
- Document non-obvious decisions in this file's Decisions log as you make
  them.

## Performance evidence (criterion 6)

Synthetic: `make perf-test` builds a fixture world of 100 nodes / 1000 pods
and times full rebuild (map + workloads + attention) plus a rendered
140×40 frame — measured 2026-06-12 on the M4 Max at **avg ~0.5ms, worst
<1ms**, asserted <100ms in release. Live: `make perf-up && make perf` runs
against a kwok-simulated cluster of the same size (`hack/perf-seed.sh`,
5 zones × 20 nodes, 20 deployments × 50 replicas). Input latency is
unmeasurable by eye; world rebuilds are coalesced at tick cadence so churn
never blocks input.

## Deferred (deliberately)

Metrics-server live usage · mutations & the planning-turn diff UI ·
hot/warm pair · external services / chaos layers · logs & live tail ·
Job/CronJob city screens · namespace filtering · mouse support · minimap
horizontal compression for very wide zone counts (~60+) · zoom levels
(compact 1-line tiles for very large boards).
