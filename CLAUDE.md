# Kubernation

A 4X-inspired Kubernetes explorer — a **windowed (macroquad) map client**.
(There was a ratatui TUI; it was removed 2026-06-18 — see the "TUI removed"
decision. The headless-terminal niche is k9s's; this leans into the graphical
4X metaphor instead.) The cluster is a living **world**
the operator explores: zones are continents, nodes are provinces of
health-textured terrain, workloads are cities sited where their pods run
(population badge + name label), DaemonSets are roads, and abstract things
— custom-resource instances, zero-pod workloads — live on namespace
islands in the southern sea. An attention queue surfaces what needs focus
and parks the explorer's cursor on it — 4X's "next unit needing orders",
not a wall of dashboards.
**Reads by default; writes are deliberate and gated** (user's call,
2026-06-17). The whole write surface is one small, auditable file,
`kubernation-core/src/k8s/actions.rs` — everything else (reflectors, pure
models, on-demand log tails) is read-only. Two write paths exist, each behind
an explicit confirm: **pod eviction** (a real `DELETE`, from a pod's evict
control), and **committing the planning turn** (apply staged Scale/Cordon/
Restart/Image to the cluster). Both are **RBAC-aware**: eviction probes `delete
pods` with a `SelfSubjectAccessReview`; the planning turn validates every staged
change with a **server-side dry-run** (which also enforces RBAC) and only applies
if all pass (all-or-nothing at the gate, via `actions::commit_interventions`).
Staging itself still never writes — only Commit does. A third path, **Game Day
chaos drills**, deliberately injects failures (`actions::run_chaos`) — but it's a
*sequencer over the same two primitives* (pod delete + scale patch), so it adds
**no new verb**; it's confirmed, RBAC-gated, and refuses control-plane / system
namespaces (fail-closed). See the "Pod eviction", "Planning-turn apply", and
"Game Day — chaos drills" decisions. One **active-but-non-mutating** capability,
**port-forward** (`k8s/portforward.rs`), sits *beside* the write file rather than
in it (it changes nothing on the cluster) but is gated in the same spirit:
RBAC-pre-checked (`create pods/portforward`), explicit, and individually
stoppable. See the "Port-forward" decision. (Decision-log entries written before
2026-06-18 that say "both frontends / GUI and TUI" are historical — the logic
they describe now lives only in the GUI + core; the shared write file
`actions.rs` is unchanged.)

The full product brief lives in `kubernation-tui-mvp-prompt.md` (written for the
original TUI; the world model + write posture carried over to the GUI). Read it
before proposing scope changes.

## Conceptual model (the short version)

CNCF landscape layers reframed as concentric zones of operator agency:

| Layer          | In Kubernation                                                |
| -------------- | -------------------------------------------------------- |
| Provisioning   | the continent — out of scope for MVP                      |
| Runtime        | terrain attributes per node (shown in node detail)        |
| Orchestration  | the main game board (map view)                            |
| App Definition | what cities produce (city screen)                         |
| Observability  | a property of every view, not a separate view             |
| Platforms      | cluster metadata (status bar platform hint)               |

Built beyond MVP: the **hot/warm cluster pair** — two continents side by
side with sync-state badges (see "The pair" below). Future (designed-for,
not built): external managed services as foreign powers, chaos events as
barbarian raids, and the planning-turn staged-diff intervention model.

## Architecture (cargo workspace)

```
crates/
  kubernation-core/   NO UI DEPS — everything frontends share:
    events.rs    ClusterId / WorldDelta vocabulary
    k8s/         DATA LAYER: client+platform detect, quantity parsing,
                 reflector spawning (watch.rs; spawn() takes a DeltaSink
                 closure so any frontend can subscribe); metrics.rs (poll
                 metrics.k8s.io) and logs.rs (on-demand pod log tail) sit
                 beside the reflectors — both are fetch-not-watch; browse.rs
                 (discover any kind + LIST DynamicObjects on demand — the
                 resource browser's data, also fetch-not-watch); portforward.rs
                 (local TCP listener tunneling to a pod port — active but
                 non-mutating, RBAC-gated; sits beside actions.rs, not in it)
    state/       observed.rs  ObservedWorld (reflector stores + event ring
                              + dynamic custom-resource stores)
                 world.rs     PURE world geometry: continents/provinces/
                              cities/islands, placement, hit-testing
                 planned.rs   the planning turn: PlannedWorld (staged
                              Intervention intents) + plan_diff (pure
                              from→to diff vs observed). Preview-only — no
                              apply path.
                 model.rs     PURE derivations: map/workloads/city/node
                 attention.rs PURE detectors → severity-ordered concerns
                 blast.rs     PURE dependency fan-out of a node/workload
                              (node→workloads→services→ingresses) — blast radius
                 slo.rs       availability SLOs + error-budget tracker (the
                              treasury) + per-workload target config; PURE math
                 chaos.rs     PURE chaos-drill planner + fail-closed guards (the
                              Game Day experiments; execution is actions::run_chaos)
                 advisor.rs   PURE cluster-wide rollups (Health/Storage/
                              Network) for the advisor screens
                 inspect.rs   PURE read-only YAML of an in-store object
                              (workload/node/pod) — the inspector "dossier"
                 fixtures.rs  synthetic worlds (feature = "fixtures")
    util.rs      fnv1a64 stable hash, age/bytes formatting
    examples/smoke.rs  headless connect + world summary (the CI gate;
                 `make smoke` — UI-free since the GUI needs a display)
  kubernation/        THE PRODUCT — the macroquad windowed client (was
                 `kubernation-gui`, renamed when the TUI was removed 2026-06-18;
                 `cargo run` = this, default-members):
                 net.rs (tokio thread publishing Models +
                 ObservedWorld snapshots), draw.rs (ISOMETRIC 2:1 diamond
                 projection — iso camera/transform, dithered terrain diamonds,
                 procedural settlements, iso minimap; all original geometry, no
                 sprites), panels.rs (hover tooltip, attention strip, context
                 picker, shared helpers), sidebar.rs (the docked right column —
                 WORLD/STATUS/SELECTION, classic-4X right panel), menu.rs (the
                 classic-4X dropdown menu bar — Game/View/Orders/Advisors/
                 World/Help), window.rs (reusable modal chrome for drill-downs),
                 almanac.rs (the in-app reference / field guide), advisor.rs (the
                 4X advisor screens — Health/Storage/Network, on window.rs over
                 core's advisor reports), inspect.rs (the read-only YAML
                 inspector window, on window.rs over core's inspect),
                 browse.rs (the `:` resource browser — a mouse/wheel modal:
                 kind picker → generic table → click a row to inspect),
                 city.rs / node.rs (the 4X city + province drill-down
                 windows, on window.rs), plan.rs (the End-of-Turn review),
                 text.rs (bundled sans + serif fonts), theme.rs. See the
                 "Isometric world map" + "GUI menu bar + overlays" + "Advisor
                 screens" + "Resource browser" + "GUI spike/promotion" decisions.
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
  cpu/mem gauges show *scheduling pressure* from pod requests by default —
  always computable from core API objects; kind needs no metrics-server.
  Buckets: <0.7 calm, 0.7–0.9 elevated (yellow), ≥0.9 high (red) — shared
  constants in `state/model.rs`.
- **Live metrics** (2026-06-16): when metrics-server is present the gauges
  switch to *live usage ÷ allocatable* automatically. `k8s/metrics.rs`
  polls `metrics.k8s.io/v1beta1` NodeMetrics (a `DynamicObject` LIST every
  15s — the metrics API has no watch) into `ObservedWorld.metrics`
  (`Arc<Mutex>`, like the events ring); `WorldDelta::Metrics` nudges a
  rebuild. `build_node_tile` reads usage when available, else requests, and
  tags `NodeTile.metric_source` (Usage|Requests); `MapModel.metrics_live`
  drives the source label ("gauges live"/node detail "cpu use" / GUI "live
  usage"). First poll failure flips `available=false` and it keeps polling,
  so a later `make metrics-up` is picked up without restart. Node health
  now reflects live usage when present (≥0.9 = Pressure). `make metrics-up`
  installs metrics-server on kind (needs `--kubelet-insecure-tls`).
- **Pod-level live metrics** (2026-06-17, roadmap "polish"): the same poll now
  also LISTs `metrics.k8s.io` **PodMetrics** (plural `pods`, built by hand like
  the node resource) and sums each pod's per-container usage into
  `Metrics.pods` keyed by `(namespace, name)`. Best-effort — a PodMetrics
  failure leaves the map empty but keeps `available` true (NodeMetrics is the
  signal). `ObservedWorld::pod_usage(ns, name)` mirrors `node_usage`; the pure
  builders hang it on `CityPod.usage` / `NodePodRow.usage` (both
  `Option<NodeUsage>`). The **city CITIZENS** and **node GARRISON** pod lists
  show it via the shared `util::format_usage` (`kubectl top`-style
  `{millicores}m {human_bytes}`) — a new USE column in the TUI tables, appended
  to the row in the GUI. Unit-tested (`pod_usage_flows_into_city_and_node_models`)
  + verified live (GUI city window shows `0m 10Mi`).
- **Logs & live tail** (2026-06-16): `k8s/logs.rs` is *fetch-not-watch* — a
  one-shot `Api::<Pod>::logs` tail of the last 500 lines (`first_container`
  resolves the container so multi-container pods work without guessing).
  Frontends *poll* it every ~2s for a live tail rather than holding a kube
  log stream (simpler, survives reconnects, no stream lifecycle). Each
  frontend owns its fetching since the pure core has no client: the **TUI**
  keeps the `Cluster`s past spawn, `Action::OpenLogs` pushes a `Screen::Logs`
  (`ui/logs.rs`: follow/scroll, j/k, g/G/f), and a `log_gen` token drops
  stale `AppEvent::Logs` after the user moves on; the **GUI** net thread
  holds the clients + a `log_req` slot, fetches on request-change or every 8
  ticks, and stores into `log_tail` only if still the requested target.
  `l` on the city/node screen tails the *selected* pod (TUI); clicking a pod
  row in an open panel does the same (GUI — `draw_panel` returns `PodRowHit`
  rects, `draw_logs` paints the overlay). `ClusterId` gained `Default`
  (`#[default] Hot`) for `LogsView::default()` — the orphan rule blocks
  `impl Default` in the TUI crate. Dev flag `--tail` (with `--inspect`)
  auto-opens the first pod's logs for headless screenshots (docs/gui-logs.png).
- **Logs: `--previous` + grep/filter** (2026-06-17, roadmap "polish"):
  `logs::tail` gained a `previous: bool` → `LogParams.previous` (tail the
  *previously terminated* container — the crash loop's last words; the server
  errors if no prior instance, surfaced inline). Both frontends toggle it with
  **`p`** and re-fetch (TUI: `Action::RefetchLogs` → `fetch_logs` reads
  `LogsView.previous`; GUI: flip `LogReq.previous`, whose `PartialEq` change
  makes the poll re-fetch). **`/`** opens a **case-insensitive substring
  filter** over the *already-fetched* 500 lines (no refetch) — purely a display
  narrowing, with an `n/m` match count. The hard part is keyboard ownership:
  while editing the filter, ordinary keys must be *text*, not shortcuts — the
  **TUI** routes all keys (incl. Esc/Backspace) to `LogsView::filter_input`
  when `Screen::Logs && filtering()` (Ctrl+C still quits, intercepted earlier);
  the **GUI** computes `log_typing` to gate `Q`, gates the almanac `/` behind
  `!log_open` so `/` is the filter trigger, drains `get_char_pressed` when not
  editing (no stray leading `/`), and Esc first leaves the editor then closes
  the overlay. Filter chrome rides the *top* border (TUI) so body scroll math
  is untouched. Dev flags `--log-previous` / `--log-filter <substr>` (with
  `--tail`) verify both headlessly; verified live (`<previous>` shows crashy's
  `boom`; a `process 48` filter narrowed web to `1/31`).
- **Namespace filtering** (2026-06-17, roadmap "polish", highest-value item):
  `state/filter.rs` `NamespaceFilter` (`All` | `Only(BTreeSet)`, with
  `matches`/`matches_opt`/`toggle`/`label`) scopes the *derived* world without
  touching the reflectors (they always watch all namespaces — filtering is a
  view concern). `Models::build` delegates to **`Models::build_filtered(world,
  &filter)`**, which retains workload rows by namespace, threads the filter
  into `attention::build` (guards every namespaced concern; **node concerns are
  cluster-scoped → always kept**), and narrows the island/coast inputs
  (customs/exposure/storage/batch). **Load-bearing subtlety:** terrain
  (`build_map`) and node pressure must stay *physical* (all pods), but
  `build_world` sited cities from the map's node-tile pod census — so a
  filtered-out workload still got a 0-pop city. Fixed by gating city/road
  siting on the workload being in the (filtered) `workloads` list
  (`row_of.contains_key(owner)`), effectively a no-op when unfiltered — it
  additionally drops the transient 0-pop "ghost" city of a workload whose
  object isn't (yet) in the store (mid-sync / mid-delete), which is a strict
  improvement. `ObservedWorld::
  namespaces()` feeds the pickers. **TUI:** `N` opens `namespace_picker.rs`
  (multi-select: Space toggles, Enter applies; the status bar shows the active
  scope); filter resets on context switch. **GUI:** a chrome button (always
  shown, highlighted when active) opens a single-select picker via the existing
  `draw_picker`; the net thread holds `ns_filter` (rebuilds when it changes)
  and resets it on switch; `--namespace <ns>` launches scoped (+ verification).
  Verified live: scoping to `kubernation-demo` drops the control-plane's
  kube-system cities (coredns, local-path-provisioner) while the node terrain +
  9-pod census remain. Unit-tested in core (attention + world-model + filter).
- **Connectivity layer** (2026-06-16, first slice of "more kinds on the map"):
  Services become `Ψ` harbors and Ingresses `∏` gates, **moored in the ocean
  strip on a continent's east coast, each on the latitude of the city it
  serves** — the shoreline reads as the network boundary. `build_exposure`
  (pure, in `model.rs`) is the reverse of `build_city`'s service match: it
  resolves Service selectors → workloads (harbors) and Ingress backends →
  Services → workloads (gates), deduped per (workload, kind, name).
  `build_world` moors them as `CoastMarker`s on `Continent.coast` at
  `cont.x + PATCH_W + i` (gates sort ahead of harbors so external exposure
  is never the one dropped to `COAST_CAP`=3). They are **render-only** — not
  a `Region` hit-test variant — so the change doesn't ripple through every
  `region_at` consumer; `WorldModel::coast_at` powers the GUI hover tooltip
  and click-to-open-city, and the city screen carries the authoritative
  routes (Service `svc/` + Ingress `ing/host`). Both frontends drop the
  marks at world scale and show them at regional/local (TUI: cyan `Ψ`/`∏`;
  GUI: cyan anchor / arch line-marks). Demo: `hack/samples.yaml` adds an
  Ingress for `web` (docs/gui-connectivity.png). Deferred to later slices:
  PVCs as granaries, Jobs/CronJobs (both since built); connectivity attention
  (orphan ingress / harbor with no city) was added later — see the
  "failed-Job + connectivity detectors" decision.
- **Storage layer** (2026-06-16, second slice of "more kinds on the map"):
  PVCs become `⊞` granaries sited **inland of (west of) the city that mounts
  them** — cities took the western half, harbors the east coast, so storage
  fills the interior. `build_storage` (pure, model.rs) tallies per workload
  the PVCs it mounts (pod volumes ∪ StatefulSet volumeClaimTemplate-derived
  claims, the same union `build_city` shows) and how many are not Bound;
  `build_world` hangs the tally on `City.storage` (`CityStorage{claims,
  pending}`). One granary per city regardless of replica count (a StatefulSet
  with N PVCs is still one granary), cyan when all Bound and warning-yellow
  when any pends. Render-only, like the coast marks; dropped at world scale.
  GUI city tooltip gains an "N PVCs · M pending" line; the city screen
  already lists `pvc/` rows. `pvc_phase` helper shared with `build_city`.
  Demo: db's `data-db-*` (docs/gui-storage.png). **Unmounted PVCs**
  (standalone `stuck-pvc`) have no city, so they stay in attention only —
  surfacing them on the map (island granaries) is deferred.
- **Batch layer** (2026-06-16, third slice of "more kinds on the map"; user
  chose island-structures over first-class cities): Jobs become `◈`
  expeditions and CronJobs `◷` schedules **on their namespace island**,
  beside the CRD `✦` structures — transient/scheduled work reads as abstract
  geography, not permanent settlements. `Structure` gained `detail` (status /
  schedule suffix) and `alert` (a failed Job → warning colour); `build_batch`
  (pure, model.rs) summarises each Job (`S/C ✓` · `N active` · `N failed ✗`)
  and CronJob (its schedule + running count), **folding CronJob-spawned Jobs
  into their CronJob** so job history doesn't flood the island. `BatchEntry`
  feeds `build_world` like `CustomEntry`. Frontends render the detail + alert
  colour (GUI: pennant for Job, clock for CronJob; `ascii()` gained `✓`/`✗`).
  TUI islands are narrow (22 cells) so long CronJob schedules truncate — the
  GUI's wider labels show them in full. Demo: `migrate` Job + daily `nightly`
  CronJob in samples (docs/gui-batch.png). Job-object attention (the failed-Job
  concern) was added later — see the "failed-Job + connectivity detectors"
  decision. Deferred: Job/CronJob city screens.
- **GUI window system + Almanac** (2026-06-16, user request after pasting
  a 4X game's reference + city screens): the GUI had only bespoke overlays
  (tooltip, side panel, log overlay, picker) — no shared modal. `window.rs`
  is a reusable centered modal (dimmed scrim, parchment frame, titlebar +
  icon, a clipped body the caller fills, a button/tab row; Esc / close-box /
  click-outside dismiss), mirroring 4X's window *structure* in the
  Kubernation palette. macroquad has no easy scissor, so the body is culled +
  scrolled by the caller (per-line visibility test), like `draw_logs`.
  `almanac.rs` is the first consumer — our **field guide**: pages Legend /
  World / Controls / Reading, opened with `?`/`F1` or the top-bar `?` button
  (`--almanac` dev flag for headless shots). The Legend draws the **actual
  marks** (reuses `draw_harbor/gate/granary/job/cronjob`, now `pub(crate)`,
  + `pod_color`) beside each definition, so it can't drift from the map.
  The TUI's `?` help gained a matching compact MAP LEGEND section.
  docs/gui-almanac.png. **Polish** (2026-06-16): field-guide cross-refs —
  each Legend entry whose mark has a live example (resolved from the hot
  world via `locate()`, derived from the `Mark`) lights up with a `>` chevron
  + hover highlight; clicking it returns `AlmanacAction::Locate(cell)` and
  the main loop flies the camera there, selects it, and closes the Almanac.
  Keyboard `1`-`4` jump to tabs and `←`/`→` cycle them; `window.rs` buttons +
  close box highlight on hover.
- **GUI city drill-down** (2026-06-16, the window system's first rich
  consumer): clicking a city opens a centered **4X-style city window**
  (`city.rs`) instead of the old right-side panel — the 4X city screen
  reframed for K8s (observe-only, so no Buy/Change): title bar `kind ns/name`
  (+HOT/WARM) → a **status band** with replicas + updated **gauge bars**,
  rollout, strategy/age, attention flag, pair-sync → **CITIZENS** (a pod
  census grid à la 4X's food store + a clickable pod list that tails
  logs) → **IMPROVEMENTS** (owned svc/ingress/pvc/cm/secret) → **CHRONICLE**
  (recent events). Built on `build_city` + `window::draw_window`; fixed size
  with caps + "+N more" (4X's panels don't scroll). It's a **modal**:
  suspends map nav/zoom/tooltip while open, a `panel_just_opened` guard keeps
  the opening click from dismissing it, Esc / close-box / click-outside
  dismiss, and the pod→log overlay draws on top. `--inspect <city> --tail`
  opens the first pod's log headlessly. docs/gui-city.png.
- **GUI node "province" window** (2026-06-16, "windowize them"): nodes moved
  off the right-side panel onto the same window system (`node.rs`), so
  **every drill-down is now a centered modal** — the old side-panel machinery
  (`draw_panel`, `panel_layout`, `PanelLayout`, `PodRowHit`, `panel_cluster`)
  was deleted, and `WinAction` (close + log) moved to `window.rs` shared by
  both windows. A node reframed as terrain: title `node-name` → status band
  (zone, health, abnormal-condition flags, cpu/mem ratio gauges with the
  live-usage/pressure source) → **GARRISON** (pods stationed here, census grid
  + clickable list that tails logs) → **TERRAIN** (runtime/kubelet/os/arch/
  kernel/provider/ip from `build_node_detail`) → **CONDITIONS** (node
  conditions). `main.rs` collapsed: any open panel is a modal, so the
  click handler simplified to "minimap-jump or open a window", and the City/
  Node arms share the `WinAction` close/log plumbing. docs/gui-node.png.
- **Planning turn** (2026-06-16, preview-only, GUI first, user's "time for
  the planning turn"): the project's first write-*intent* — but still no
  writes. `state/planned.rs` is now real: `Intervention`
  (Scale{workload,replicas}, Cordon{node,on}), `PlannedWorld` (staged
  intents, latest-wins per target; stage/unstage/clear/scaled/cordoned), and
  pure `plan_diff(observed, planned) -> Vec<PlanChange>` (from→to, `noop`
  when staged==current). `PlannedWorld` is GUI-loop UI state (hot cluster
  only); the diff is computed against the snapshot's observed world.
  **Staging UX:** the city window grew a replicas stepper (`plan replicas
  [−] N [+]`, staged delta in yellow), the province window a `[cordon]`/
  `[uncordon]` toggle — both return `WinAction.stage: Option<Intervention>`.
  **`t`** (or the chrome "End Turn (N)" button) opens `plan.rs`, the
  End-of-Turn review: the diff with per-row unstage `[x]`, **Discard all**,
  and Commit. Modal like the others (suspends nav; Esc / close / click-outside).
  Originally preview-only; Commit is now wired (see "Planning-turn apply").
  `--plan` dev flag stages a demo scale+cordon and opens the review for
  headless shots (docs/gui-plan.png).
- **Planning-turn apply** (2026-06-17, the second write path): Commit now
  applies the staged turn to the **hot** cluster. `actions::apply_intervention`
  (in the one write file) patches `spec.replicas` (Deployment / StatefulSet) or
  `spec.unschedulable` (Node) with a strategic-merge patch; a `dry_run` flag
  routes it through `PatchParams.dry_run`. The GUI net thread, on a confirmed
  commit (`Net.plan_req`), **dry-runs every staged change first** — which also
  enforces RBAC, so a turn the cluster would reject is blocked before any real
  write — and only if all pass applies them for real, reporting per-row results
  in `Net.plan_outcome` (shown in the review) plus a toast. `plan.rs`'s Commit
  is enabled when there are non-noop changes, behind `panels::draw_commit_confirm`
  (the generic `Confirm` modal shared with evict); a fully-applied turn clears
  itself and closes. Dry-run is preferred over a plain SSAR here because it
  validates the actual patch + admission, not just authz. Verified live
  (`--plan --plan-go`): metrics-server 1→3 + a node cordoned, "committed 2/2",
  then reverted. Still preview-only elsewhere; staging never writes. A third
  intervention, **Restart** (rolling restart — `apply_intervention` stamps the
  pod template's `kubectl.kubernetes.io/restartedAt`, for Deploy/STS/DS; staged
  by a city's "restart" toggle, can coexist with a scale), rides the same
  commit path (verified live). A fourth, **SetImage** (set a container's image —
  `apply_intervention` uses a *strategic* merge patch so the container is merged
  by `name`, preserving its other fields + sibling containers; for Deploy/STS/DS;
  staged from a city's image field/editor, keyed per (workload, container)),
  completes the verb set (Scale/Cordon/Restart/Image). `build_city` now exposes
  `primary_container` as the default target; the strategic patch was verified
  server-side. Both frontends have the full planning turn (see "Planning turn in
  the TUI"). Deferred: nothing here — image-set was the last one.
- **Stable layout:** nodes sort within a zone by FNV-1a-64(name) — pinned by
  test so layouts never reshuffle across runs or Rust upgrades. Zones sort
  by name; `unzoned` sinks to the end.
- **Zone label:** `topology.kubernetes.io/zone` with legacy
  `failure-domain.beta.kubernetes.io/zone` fallback. kind has no zone labels,
  so `hack/kind-config.yaml` bakes z-a/z-b/z-c onto the workers.
- **Watched resources:** Node, Pod, Deployment, ReplicaSet (ownership chain +
  rollout), StatefulSet, DaemonSet, Job, CronJob, PVC, Service, Ingress,
  Event. **Secrets and ConfigMaps are never watched** — the city screen
  derives their *names* from pod-template references, so we observe
  dependency shape without reading contents (least privilege). The one
  controlled exception is the **resource browser** (`:any kind`), which can
  LIST + inspect any kind on demand — there a Secret's `data`/`stringData`
  values are **redacted** (`dynamic_yaml`), so we still never surface secret
  contents (see the "Resource browser" decision). Ingress
  shares the `Services` dirty-bit and Job/CronJob the `Workloads` dirty-bit
  (the deltas are payload-free; rebuilds are wholesale).
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
- **Multi-cluster:** `ObservedWorld` + its informer set (`WorldHandle`,
  abort-on-drop) are per-context. Context switch = connect, spawn new
  handle, drop old. The hot/warm pair proved the design: adding the second
  world was "hold two handles + a comparison model", not a refactor.
- **Platform hint:** kubeconfig heuristics first, refined by the first
  observed node's `spec.providerID` (aws/gce/azure/kind/k3s prefixes).
- **In-cluster config is not supported** (operator-laptop tool); revisit if
  a read-only web/agent mode ever appears.
- **Minimap / WORLD panel** (2026-06-12): one cell per node in zone
  columns; when a zone is taller than the panel, `k` nodes collapse into
  one cell with worst-state-wins coloring. The viewport frame hugs the
  first/last visible cell rows exactly (no half-row exists); a single-row
  frame borrows the margin rows above/below so the corners can't collide.
  It bails out silently rather than smothering the board when zones are too
  numerous to fit (~60+ zones — horizontal compression is a future step).
- **world sidebar** (2026-06-12, visual pivot at user request): on the map
  screen, ≥110 cols (≥150 paired) adds a right sidebar shaped like 4X's:
  WORLD (the minimap, permanent home), STATUS (context/platform, node/pod
  counts, concern rollup, overlay), ORDERS (the selected tile — 4X's
  "Moving Unit" box: health, zone, conditions, pressure, pod census).
  Below the threshold the floating WORLD overlay takes back over
  (`MapView::external_minimap` suppresses it when the sidebar is up). The
  sidebar always shows the *focused* world. K8s terms are never renamed to
  4X terms — the grammar is 4X, the nouns stay kubectl-greppable.
- **The world projection** (2026-06-12, "lean into the game metaphor"):
  the zone-column tile grid was replaced by a 2D world. Cities = workloads
  (Deploy/STS), sited on the province hosting the plurality of their pods
  (stable-hash tie-break; a city migrates only when its pods genuinely
  move). DaemonSets are `≣` roads on every province they touch, never
  cities. Zero-pod workloads become `◌` encampments on their namespace's
  island. The explorer cursor walks cells; the camera follows; `]`/`[`
  sail city to city; Enter opens whatever you stand on; `n` ALSO parks the
  cursor on the concern's location. ORDERS in the sidebar describes the
  region under the cursor. Geometry is pure (`state/world.rs`) with
  placement-stability tests.
- **Custom-resource projections** (2026-06-12): `--project <crd-name>`
  (repeatable; the config `projections = [...]` form went with the TUI —
  CLI flag only now) resolves CRDs once at
  connect (LIST, no CRD watch), spawns `DynamicObject` reflectors, and
  renders instances as `✦` structures on namespace islands. CRDs missing
  on a cluster are skipped with a log line — a pair may project
  asymmetrically. Demo: `gizmos.example.com` in hack/samples-crd.yaml
  (applied before samples.yaml so the kind is established).
- **Workspace split + GUI spike** (2026-06-12, "spike" decision after the
  renderer-options review): kubernation-core holds the data layer and pure
  models; `watch::spawn` takes a `DeltaSink` closure (not a TUI channel)
  so frontends subscribe their own way. crates/kubernation-gui is a macroquad
  windowed client: tokio on a net thread publishing `Arc<Models>`
  snapshots, terrain-colored provinces, city circles sized by population
  with 4X-style name plates, namespace islands, pan/wheel-zoom camera,
  click-to-inspect ORDERS, attention strip, `--screenshot` for headless
  verification. SPIKE quality: no tests, flat colors,
  ASCII-only text (macroquad default font has no exotic glyphs — `ascii()`
  sanitizer). Next steps if promoted: Kenney CC0 tile sprites, hover
  tooltips, city/node detail panels, pair view.
- **GUI promotion, round 1** (2026-06-12, "results are good, build on it"):
  procedural art instead of sprite packs first (per-cell mosaic shading,
  coast bevels, hut-tier settlements with 4X pop chips, warning banners,
  drifting sea) — self-contained, no licensing, no asset pipeline; Kenney
  CC0 tilesets remain the next rung. Interaction: hover tooltips,
  right/middle-drag pan, wheel zoom anchored at the cursor, minimap
  click-to-jump, lerped camera flights, zoom LOD for labels. Detail
  panels run the pure `build_city`/`build_node_detail` against an
  `ObservedWorld` clone published with each snapshot (stores are Arc
  clones — cheap). The minimap yields its corner while a panel is open.
  `--inspect <substr>` + `--screenshot` make panel states verifiable
  headlessly. Still no GUI tests (render-only logic).
- **GUI pair** (2026-06-12): `--warm` renders the standby as a second
  archipelago east of the hot one on a single sea — free panning replaces
  split-screen; `Camera::shifted(off)` draws each world through an
  offset camera so every painter stays world-local. Sync chips ride
  beside city pop boxes; tooltips/panels are HOT/WARM-tagged; the net
  thread publishes both worlds + PairSync + the merged tagged attention
  list in one snapshot. `F` fits the whole scene; the camera also fits
  once on first sync. A warm connect failure degrades to single-world
  with a status message instead of aborting.
- **GUI font + sprite tileset** (2026-06-12, "text could be better";
  **sprites superseded 2026-06-16** by the isometric rework — `sprites.rs`,
  the bundled Kenney PNGs, and `--tileset` were removed when the map went
  fully procedural; the font half stands, now joined by a bundled serif):
  macroquad's built-in font is a blurry ASCII bitmap, so `text.rs`
  bundles Fira Sans (OFL) via `include_bytes!` and routes all labels
  through `text`/`text_bold`/`text_size` helpers (font in a thread_local,
  falls back to default if parsing fails). `sprites.rs` embeds a curated
  Kenney "Medieval RTS" set (CC0) — tiled terrain textures health-tinted
  (grass/grass2 healthy, sand tinted for cordon/pressure, stone for
  NotReady), house→keep building sprites by population tier, tent/rock
  for island structures — each with the old procedural shapes as a
  fallback when sprites are absent. `--tileset <dir>` overrides any PNG
  by name. Assets live in `crates/kubernation-gui/assets/` with `CREDITS.md`;
  both font and sprite bytes are compiled in (binary stays
  self-contained). `ascii()` now only maps a handful of attention glyphs
  (the bundled font covers Unicode punctuation). Sprites use
  `FilterMode::Nearest` for crisp pixel edges. The TUI is untouched —
  this is all `kubernation-gui`.
- **GUI irregular coastlines** (2026-06-12, "more natural geographic
  shapes"): a *render-only* change in `draw.rs` — the core world model
  stays a rectangular grid (canonical coords both frontends share, and
  terminals can't draw smooth coasts anyway). Each continent gets a
  `Coast` whose east/west shores inset by smooth value noise (seeded by
  zone name → deterministic, no shimmer) and whose N/S ends taper into
  rounded capes for tall continents. The rectangular terrain fill is
  carved: shore margins are overdrawn with the sea texture, a sand beach
  + dark waterline drawn at the boundary. Displacement only *insets*, so
  rectangular hit-testing still lands on real provinces. Single-node
  zones just wobble; tall zones (kwok) read as genuine landmasses. The
  per-row inset is capped around every city (`Coast::max_l/max_r`, a
  `CITY_MARGIN` keep-out covering the building + pop chip) so the shore
  bulges out to keep settlements on dry land rather than carving them
  into the sea. The minimap carves the same `Coast` row by row
  (`land_span`) so its silhouette matches the explored coastline.
- **GUI context switching** (2026-06-12): `c` opens a modal context
  picker (j/k/click, current dotted); selecting calls `Net::request_switch`,
  a `Mutex<Option<String>>` the net thread drains each tick — it connects
  the new context, respawns the hot `WorldHandle` (old one drops →
  informers abort), resets the ready flag, and clears the snapshot so the
  UI shows fog until resync. Mirrors the TUI's hot-only switch; warm is
  fixed at launch. Camera refits on the None→Some snapshot transition
  (covers initial sync, reconnect, and post-switch). `--pick` opens the
  picker on sync for headless screenshot verification.
- **GUI cartographic scale tiers** (2026-06-16, informed by AIM's Monmonier
  *How to Lie with Maps* ch3 + Brewer *Designing Better Maps* ch1/7/10 —
  see [[aim-cartography-refs]]): `lod(zoom)` returns a `Scale`
  (Local ≥0.9 / Regional ≥0.5 / World <0.5) and the GUI generalizes
  resource presentation per Monmonier's point operators. World scale =
  **aggregation**: each province collapses its settlements into one badge
  (house sprite + city count + worst-concern `!`/`!!` flag) on the widest
  land row; islands show a structure count; trees/roads/per-city sprites
  drop (background **selection**). Regional = sprites + chips, names
  **abbreviated**; sparse worlds (≤`DENSE_CITIES`=12 cities) label every
  city, dense worlds select (troubled or ready≥4) — clutter-driven, not a
  fixed rule. Local = everything, full names. 4X aesthetic is
  preserved (same sprites/parchment); only the *density* changes with
  scale. Pop-chip prefers upper-left, names prefer the right. Dev flags
  `--zoom <f>` and `--inspect <node>` added for headless tier verification.
- **GUI label de-confliction** (2026-06-16, "we're getting label
  collisions"): Monmonier's *displacement* operator. `draw_world` keeps a
  per-frame `occupied: Vec<Rect>`; every label (continent → province →
  city chips → city names, in that priority order) takes the first of a
  candidate-position list that clears already-placed rects (`place()` /
  `rect_hits`). City names default to the **right** of the building
  (Brewer's preferred point-label position) with upper-right/lower-right/
  left/below fallbacks, so settlements stacked in a province's vertical
  column fan their names out instead of piling up; pop chips flip
  upper-left→upper-right→lower to dodge the province label; island
  structure labels de-conflict too. The user waived strict 4X
  name-below placement ("the 4X convention is satisfied by the shape,
  colors, minimap and behaviors") for legibility. Names sit east of
  buildings (which are placed in the western half), so they stay on land.
- **`Store::wait_until_ready` allows ONE concurrent waiter per store** (found
  2026-06-12): kube's readiness uses a `DelayedInit` over a futures oneshot
  receiver, which holds a single waker slot. Two tasks awaiting the same
  store race on that slot and the loser is never woken (it stalls until some
  unrelated timer re-polls it — we saw exactly-20s smoke runs). The
  readiness-notifier task in `k8s/watch.rs` is therefore the *only* caller;
  everything else (TUI and `--smoke` alike) listens for
  `WorldDelta::Ready` on the event channel. Don't add new
  `wait_until_ready` call sites.
- **Rename: codename → product, de-trademarked** (2026-06-16, two-job
  "Kubernation Rename Guide"): Job 1 mechanically replaced the original
  project codename in every identifier (crate names, binary, kind cluster +
  context, config/log paths under `~/.config/kubernation` &
  `~/.local/state/kubernation`, sample namespace) — whole-token only,
  **never** a blind substitution on a bare genre word (which would have
  eaten the homage and the Freeciv credit). Job 2, by hand, neutralized the
  remaining standalone game-name labels to generic 4X / terrain terms,
  renamed the default `Theme` palette method to `atlas()` (the
  `ColorMode::Auto` config string is unchanged — `color = "auto"`), and gave
  the in-app reference the nickname "field guide". Trademark posture: exactly
  **one** nominative attribution homage survives (the README intro), plus a
  §Trademark disclaimer mirrored one-line in `--help` (`clap` `after_help`);
  the Freeciv tileset credit stays. Verification: the codename and
  old-nickname greps come back empty; the bare-word and franchise-name greps
  return only the deliberate disclaimer (plus the single homage).
- **Headless map-shot framing** (2026-06-16): regenerating `docs/gui-*.png`
  after the rename needed map views (coast harbors, storage granary, batch
  island) framed without opening a panel — but `--inspect` opens one and the
  default camera only fits the whole world. Added GUI dev flags `--center
  <name>` (matches a city → node → island, centers there at `--zoom`, no
  panel) and `--pan-dx <cells>` (shift the framed point east/west — e.g. +7
  to reach a city's offshore harbors), alongside the existing
  `--inspect`/`--almanac`/`--plan` verification flags. Capture is the
  established `--screenshot` path. The four unreferenced/historical shots
  (spike, metrics, labels, world-scale) were dropped rather than reshot.
- **Isometric world map** (2026-06-16, user: "get the visuals closer to the
  original game" + a Civ II screenshot; chose **full isometric** + **evoke the
  genre with original/CC0 art**, NOT a trademarked clone): the GUI map was
  reprojected from a top-down rectangular grid to a classic-4X **isometric 2:1
  diamond** grid. **Render-only** — `state/world.rs` stays the canonical
  rectangular `(u16,u16)` grid both frontends share (the TUI still renders it
  rectangularly); all iso lives in `kubernation-gui/draw.rs`. **Camera:**
  `to_screen(wx,wy) = ((wx−wy)·hw, (wx+wy)·hh) − pos`, `cell_px()` returns
  diamond **half-extents**, `cell_at` is the algebraic inverse. **Convention
  (load-bearing):** integer cell = the diamond's **north vertex**, so
  `to_screen(x+0.5, y+0.5)` is the **center** — every existing painter that
  passed `+0.5` offsets keeps working, and `cell_at` **floors** the inverted
  coords (a round-trip unit test pins `cell_at(to_screen(center))==cell`).
  Because `pos` stays a screen translation and `zoom` a scalar, `main.rs`
  pan/zoom-anchor/drag code was **untouched**; `fit` uses the iso diamond AABB
  `(W+H)·hw × (W+H)·hh`, `shifted(off)` subtracts the diagonal `off·(hw,hh)`
  (warm world drops to the south-east), `draw_selection` is a pulsing diamond.
  **Rendering** is a back-to-front **two-pass** painter's algorithm
  (`draw_world`: all terrain — continents+islands sorted by `x+y` — then
  features/settlements/labels) so south tiles and tall buildings overlap
  correctly. **Terrain** (`draw_province_terrain`): one health-tinted, 2-shade
  dithered diamond per LAND cell (land/sea reuses the per-row `Coast` insets;
  the continent's `y0/h` mark N/S shore so inter-province band seams stay
  interior land), with sea-facing shoreline cells drawn sand + an inked
  waterline on only their sea-facing edges. **Ocean** is screen-space
  (`draw_sea`: wash + coarse dither + waves — O(screen), not O(cells)).
  **Settlements** (`draw_city`/`draw_settlement`/`iso_block`) are procedural
  iso building clusters that grow hut→walled-keep by population, with a solid
  **lower-left population box** and a **serif name banner below** (the classic
  city-label convention; serif = bundled **Liberation Serif Bold**, OFL 1.1,
  via `text::name_text`). HUD chrome (top bar, tooltip, attention strip,
  picker) went **tan carved stone** (`theme::STONE*` + `stone_panel`/
  `stone_well`); meaning colors (red/yellow attention, cyan structures, sync)
  are unchanged. **The Kenney sprite set + `sprites.rs` + `--tileset` were
  removed** — the map is now 100% original geometry, satisfying the
  de-trademark posture (evoke the genre, clone nothing). The minimap was later
  reprojected to match (see "Isometric minimap").
  `cargo test` round-trips the hit-test; `--zoom`/`--center`/`--screenshot`
  verify framing headlessly. Deferred: chunkier landmasses depend on real
  multi-node zones (the dev cluster has 1 node/zone → thin diagonal bands);
  per-tile sprite art if ever wanted.
- **Isometric minimap** (2026-06-17, roadmap "polish — zoom LOD + iso minimap"):
  the overview minimap (`draw.rs` `minimap_layout`/`draw_minimap`/
  `MinimapLayout::{pt,world_cell}`) was reprojected from the top-down chart to
  the same iso 2:1 diamond as the main map. `MinimapLayout` now holds per-cell
  half-extents `hw/hh` (fit so the scene's iso AABB `(W+H)·hw × (W+H)·hh` fills
  ~220px wide) + an `offx` placing the diamond's west tip at the panel's left;
  `pt(wx,wy)` is the scaled-down `to_screen`, `world_cell` its floor-inverse
  (round-trip-tested, like the main `cell_at`). Land is drawn as one iso
  parallelogram per province (health-tinted, 2 triangles), islands likewise; the
  per-row coast carving is dropped (a minimap is an overview). The viewport
  indicator is an **axis-aligned rectangle whose size tracks only the zoom**
  (user's call, 2026-06-17): the minimap and main view share the iso projection
  at different scales, so the play-area rect maps to a minimap rect of size
  `play·ratio` (`ratio = ml.hw / cam.cell_px().0`, capped at the panel) — it
  *translates* with the pan but never resizes, and the position (not the size)
  is clamped so it pins to the panel edge at the world boundary instead of
  shrinking. (Earlier tries — a sheared parallelogram, then a bounds-clamped
  AABB — both changed shape/size with the pan, which read as a confusing
  zoom.) **Drag-to-navigate:** clicking or dragging the minimap recenters the
  main view (`minimap_drag` + `cam.jump_to(world_cell(..))`); `world_cell`
  clamps to the grid so *every* spot is navigable, open ocean included. The
  **zoom LOD** half (World/Regional/Local `Scale` tiers, `lod(zoom)`) was
  already built (see "GUI cartographic scale tiers"). Verified live. The TUI
  minimap stays its own compact node-cell chart.
- **Stone-background severity ink** (2026-06-17, user: bottom-bar text contrast):
  the bright map attention colors (`CRIT`/`WARN`/`DIM`) washed out on the warm
  tan chrome, so `theme::severity_on_stone` (+ `STONE_CRIT`/`STONE_WARN`
  consts) gives darker, high-contrast variants used by everything that paints
  attention text on stone — the attention strip, the column STATUS rollup, and
  `panels::region_lines` (tooltip + SELECTION). The map's own colors are
  untouched (they read fine on the dark sea).
- **GUI docked right column** (2026-06-17, user: "get closer to the Civ gaming
  interface … the minimap is bound to a right column that provides spaces for
  additional information"): the floating minimap was replaced by an
  always-visible right column (`sidebar.rs`, `COL_W=264`), mirroring the TUI's
  sidebar and the classic-4X right panel. Three stacked sections: **WORLD** (the
  iso minimap — `minimap_layout` now docks it centered at the column top),
  **STATUS** (context, platform · node/pod counts, the concern rollup via
  `severity_counts`, gauge source, active namespace filter), **SELECTION** (the
  clicked-or-hovered tile, reusing `panels::region_lines` — extracted from
  `draw_tooltip` so the box and the tooltip can't drift). The map renders full
  width *under* the column (camera unchanged); `panels::map_width()` /
  `sidebar_rect()` bound the play area — map clicks and the hover tooltip are
  gated to `mouse.x < map_width()`, and `draw_attention_strip` spans only the
  play area. Drill-down modals (city/node) keep working: they're centered with
  a scrim that dims the column. `draw_minimap` moved into the column (no longer
  a `None`-panel overlay). Verified live.
- **Pod eviction — the first mutation** (2026-06-17, user's explicit call to
  "add the ability to delete pods", choosing **real live deletion** labeled
  **"evict"**): the project's first and only cluster *write*, a deliberate,
  gated break of the former absolute observe-only guarantee. **All write code
  lives in one file**, `kubernation-core/src/k8s/actions.rs` —
  `evict_pod(client, ns, pod)` does a plain `Api::<Pod>::delete` (a managed
  pod is recreated by its controller; a bare pod is gone); errors come back as
  strings. **Wiring (GUI only; the TUI stays read-only):** the city CITIZENS
  list and node GARRISON list grow a hover-revealed red **`evict`** button per
  pod (`WinAction.evict`); clicking it raises a centered **confirm modal**
  (`panels::draw_evict_confirm`, Esc/Cancel to back out) — nothing is sent
  until the operator confirms. On confirm the GUI queues an `EvictReq` in
  `Net.evict_req` (mirrors the `log_req`/`switch` slots); the net thread drains
  it once, calls `actions::evict_pod` on the cluster's client, and reports the
  result in `Net.evict_status` (a top toast it auto-clears after ~3s). The
  watch sees the pod vanish on a later tick — no optimistic UI. Metaphor: pods
  are a city's "citizens" / a node's "garrison"; **evict** matches both k8s pod
  eviction and the 4X "remove an inhabitant" idea, and is honest (a managed pod
  comes back). Dev verification flags `--evict <substr>` (open the city + raise
  the confirm on its first pod) and `--evict-go` (auto-confirm — REALLY
  deletes; verified live: `web-…-7j8fp` deleted, Deployment recreated it 2s
  later). Still **not** built: image-set / restart / scale-apply / cordon-apply
  (the planning turn stays preview-only).
- **Evict: RBAC gate + TUI eviction** (2026-06-17, user follow-up): both
  frontends now check `delete pods` permission for the pod's namespace before
  offering eviction, via `actions::can_evict_pod` (a `SelfSubjectAccessReview`
  — a read-only probe living beside `evict_pod` in the one write file). **GUI:**
  the net thread caches answers per (cluster, namespace) in `Net.evict_perm`
  (filled by draining `evict_perm_pending`, cleared on context switch);
  `Net.evict_allowed` is a poll-and-enqueue lookup the windows call per pod, so
  the shared `window::evict_button` renders enabled (red `evict`) / `locked`
  (no permission) / `...` (probe in flight) and only fires when allowed.
  **TUI** (its first write — was read-only): `e` on a city CITIZENS / node
  GARRISON pod returns `Action::EvictPod`; `apply` runs the SSAR (cached in
  `App.evict_perm`) and either raises a red y/n confirm (`render_evict_confirm`,
  snapshot-tested) or flashes "no permission"; `y` spawns `evict_pod` and
  flashes the result via `AppEvent::Evicted`. RBAC verified via
  `kubectl auth can-i delete pods` (admin → yes/enabled; unprivileged → no/
  locked). Still deferred: scale/cordon apply, image-set/restart.
- **Logos + intro splash** (2026-06-17, user supplied two logos): the compass
  **mark** (transparent, icon-grade) and the full **KuberNation** scene
  (opaque). Originals live at the repo root; downsized copies are compiled into
  the GUI (`assets/logo/{mark,full}.png`) via `logo.rs`. The mark is the OS
  window icon (`Conf.icon` — 16/32/64 RGBA decoded + resized with the `image`
  crate, the one new dep) and the top-bar emblem; the full scene is the
  fog-of-war splash. Because that splash otherwise vanished the instant the
  world synced, the loop opens with an **intro splash phase** (`splash_start`/
  `splash_skipped`): the full scene held ~2.4s with a fade in/out + slow zoom,
  skippable by any key/click, suspended (early `continue`) so nothing else
  draws or takes input. It's off under `--screenshot` (so docs shots are
  unaffected); `--splash` forces+captures it. The logos are first-party art, so
  CREDITS notes them but no third-party license applies (the new serif/`image`
  aside). **Caption fix** (2026-06-23, v0.60.1, user: the caption alignment in the
  real app differed from the preview): `logo::draw_full` now RETURNS the drawn
  `Rect`, and the splash caption (title + "press any key") is centered on that rect's
  center + placed just below its bottom edge — replacing the fixed `cy + 232`/`+256`
  offsets that drifted/overlapped on a differently-sized window — drawn via
  `text_outline` (a dark stroke so it pops on the bright scene), and `SPLASH_SECS`
  raised 2.4 → 5.0 (still skippable).
- **Attention: failed-Job + connectivity detectors** (2026-06-17, roadmap
  "deepen the attention queue"): three new pure detectors in
  `state/attention.rs`, each unit-tested against `fixtures.rs`. **Failed Job:**
  a Job with a `Failed` condition (backoff limit reached) → Critical; a Job
  still accumulating `status.failed` pod failures → Warning; a *completed* Job
  (`Complete` condition or `succeeded ≥ completions`) stays quiet. Crucially
  the detector runs **before** the pod loop and records `covered_jobs`; the
  bare-pod arm then **folds** a Job's own failing pods under the one Job concern
  (via `job_owner`, since Jobs aren't `WorkloadRef`s so `OwnerIndex` skips
  them) — preserving the "city in trouble, not 40 pod alarms" rule. Job
  *events* dedup against `covered_jobs` too. **Connectivity:** an **orphan
  Ingress** (a backend Service name absent from `world.services`) → Warning
  (reuses `model::ingress_backends`, now `pub(crate)`); a **Service whose
  selector matches no pod** ("harbor with no city") → Info, with no-selector
  (headless/ExternalName) Services skipped so healthy clusters stay silent.
  All target `WorkloadList` (Jobs/Services have no city). Verified live on kind:
  a healthy cluster adds **zero** false positives; a deliberately broken
  Ingress + a `BackoffLimitExceeded` Job both fire, and the Job collapses from
  three lines (2 pods + Job) to one. Still deferred: unmounted-PVC island
  granaries (a *map* feature, not attention).
- **Planning turn in the TUI** (2026-06-17, roadmap "frontend parity"): the
  terminal client gained the staging + End-of-Turn + commit flow that was
  GUI-only. `RenderCtx` now carries `planned: &PlannedWorld` (the one new field
  threaded through the 4 ctx sites) so the city/node views show staged deltas;
  staging is gated to the hot world (`source == Hot`) since `PlannedWorld` is
  hot-only. **City** (`ui/city.rs`): `+`/`−` emit `Action::Stage(Scale)`
  (skipped for DaemonSets), `R` emits `Action::ToggleRestart`; a header line
  shows the staged delta or a dim hint. **Node** (`ui/node_detail.rs`): `C`
  emits `Action::Stage(Cordon{on: !current})`. **`t`** pushes `Screen::Plan`
  (`ui/plan.rs`, a `PlanView`): the `plan_diff` table with `j/k` nav, `x`
  unstage, `D` discard, `c`/`Enter` commit; its keys are intercepted before the
  global bindings so `c`/`x`/`D` don't clash. Commit raises a y/n confirm
  (`pending_commit`, mirroring `pending_evict`); `spawn_commit` runs
  `actions::commit_interventions` off the loop and reports an
  `AppEvent::Committed{outcome}` (flash + per-row RESULT panel). **The commit
  orchestration (dry-run-all → apply-all) moved from the GUI net thread into
  the write file** as `commit_interventions` returning `CommitOutcome`/
  `CommitRow`; the GUI now aliases `PlanOutcome = CommitOutcome` and calls it,
  so both frontends share the one all-or-nothing gate. Verified live (the
  shared helper, via the GUI's `--plan --plan-go`: metrics-server 1→3, worker
  cordoned, web rolled — then reverted); the TUI plan view has a TestBackend
  snapshot test. The TUI is no longer read-only-plus-evict — it has both writes.
- **GUI menu bar + map overlays** (2026-06-17, user chose "Menu bar" from a
  "closer to Civ II" options menu): the scattered GUI chrome buttons (the `?`
  almanac toggle, the End-Turn badge, the namespace chip, the long help line)
  are replaced by a classic-4X **dropdown menu bar** (`menu.rs`) — **Game**
  (switch context · fit view · quit), **View** (the map overlay radio),
  **Orders** (end of turn · discard, with the staged count in the title),
  **World** (namespace filter), **Help** (field guide · version). `draw_menu_bar`
  is immediate-mode like the rest of the GUI: it both paints the bar + any open
  dropdown and hit-tests, returning a `MenuAction` the main loop maps to existing
  state; the open menu is GUI-loop state (`open_menu: Option<usize>`). Behavior
  mirrors the genre: click a title to toggle its dropdown, slide across to switch
  menus, click an item or anywhere outside to dismiss. **An open menu suspends
  map navigation** (added to the same modal-suspend conditions as picker/plan/
  panel), and the dropdown draws over the world at chrome time — so map clicks
  can't fall through. "Fit view" can't reach `bounds` from the chrome draw, so it
  defers via a `pending_fit` flag consumed next frame. The realm readout
  (context · platform · counts) moves to the right of the bar. **Map overlay**
  (the View menu's "map display"): a render-only `Overlay { Terrain, Pressure,
  Replicas, Namespace }` threaded through `draw_world` → `draw_province_terrain`
  → `land_diamond` and the minimap (`overlay_pair`/`overlay_flat` take the whole
  `&Province` since Replicas/Namespace read its `cities`). `Terrain` is the
  default node-health tinting; `Pressure` recolors each province by `max(cpu,mem)`
  ratio using the documented buckets (<0.7 green / 0.7–0.9 amber / ≥0.9 red,
  `theme::heat_pair`/`pressure_pair`); `Replicas` colors by the worst workload
  health sited there (`replica_level` over the cities — full green / gap amber /
  down-or-critical red); `Namespace` colors by the plurality namespace's stable
  hue (`dominant_namespace` + `theme::namespace_pair`, a tiny HSV from
  `fnv1a64(ns)`) — a political/territory map. Provinces with no cities show
  `idle_land_pair` (a desaturated grey-green) under the two city-based overlays.
  The non-default view is labeled in STATUS so a recolored terrain isn't mistaken
  for NotReady. Dev flags `--overlay <terrain|pressure|replicas|namespace>` and
  `--menu <name>` capture them headlessly (all four verified live on kind). The
  TUI keeps its own `1/2/3` overlays + key-driven actions (no menu bar). Deferred:
  a TUI menu bar.
- **Advisor screens** (2026-06-17, user, after the menu bar — the classic-4X
  "advisors"/Civ F1 Berater): a new **Advisors** menu opens a modal window with
  three read-only summary tabs. The reports are **pure functions of
  `ObservedWorld`** in core (`state/advisor.rs`: `health_report` /
  `storage_report` / `network_report` → `HealthReport`/`StorageReport`/
  `NetworkReport`), unit-tested against `fixtures.rs` — keeping the interesting
  logic out of the GUI. They are **cluster-wide** (deliberately *not* scoped by
  the namespace filter — an advisor reports on the whole realm) and reuse the
  existing pure builders (`build_map` for node health, `build_workloads` for
  workload strength, `pod_state` for pod phases, `ingress_backends` + the
  selector-match logic mirrored from `attention.rs` for orphan gates / idle
  harbors). **Health** rolls up provinces(nodes)/citizens(pods)/cities(workloads);
  **Storage** is granaries(PVCs) bound vs pending + the pending list; **Network**
  is harbors(services)+gates(ingresses) + orphan/idle routes. The GUI side
  (`gui/advisor.rs`) is a modal `Advisor` window on `window.rs`, mirroring the
  Almanac's tab/scroll/Esc machinery (tabs switch via click or 1/2/3/←/→); it's
  added to every modal-suspend / Esc / `menu_live` site like the Almanac, with
  an `advisor_just_opened` guard. Trouble counts use meaning colors on the dark
  panel (`theme::GOOD` green / `WARN` / `CRIT` / `STRUCT`). Dev flag `--advisor
  <health|storage|network>` (and the `--menu advisors` index). They **complement**
  the attention queue (which says *what needs orders*) rather than replacing it.
  Verified live on kind (4 nodes healthy, 2 failing pods, 1 understrength
  workload, stuck-pvc pending). Deferred: a TUI advisor view; advisor-driven
  navigation (click a row → fly there).
- **Object inspector (read-only YAML)** (2026-06-17, user: "narrow the gap with
  k9s", chose the inspector first of the candidate borrows): a k9s-style `y`
  YAML "dossier" of a single resource, in **both** frontends. Core
  `state/inspect.rs` is pure (`clean_yaml<T: Serialize>` → `serde_yaml` after
  dropping `metadata.managedFields` + the last-applied annotation; plus
  `workload_yaml`/`node_yaml`/`pod_yaml` that resolve an object **from the
  reflector store** — no fetch, no client) and unit-tested. It is deliberately
  **least-privilege**: only the *watched* kinds are inspectable, so Secrets/
  ConfigMaps are still never read (unlike k9s, which can `y` any object). Added
  the one dep `serde_yaml`. **GUI** (`gui/inspect.rs`): a scrollable modal on
  `window.rs`; opened by `y` on a city/node window (workload/node) or a pod
  row's new `yaml` button (`window::row_button`, `WinAction.inspect`); wired
  like the Almanac modal (it sits over its panel; Esc closes it first). **TUI**
  (`ui/inspect.rs`): a `Screen::Inspect` scroll view; `y` returns
  `Action::Inspect{Pod,Workload,Node}` from the city/node/workload-list views,
  and `app.rs` resolves the YAML via `observed_for(cluster)` (a new accessor for
  the handle's `ObservedWorld`) and pushes the screen. The pure builders are
  reused across both frontends. Dev flag `--yaml` (with `--inspect`); verified
  live (the GUI shows web's Deployment + worker2's Node YAML, managedFields
  stripped). This is the first of the "narrow the k9s gap" borrows; the
  `:`-style resource browser and **port-forward** followed (see those decisions),
  and candidates not yet built: workload-list sort/filter. Exec/shell is
  deliberately **not** planned (the macroquad GUI can't host a PTY, and arbitrary
  exec breaks the read-by-default / one-write-file posture).
- **Resource browser (`:any kind`)** (2026-06-17, the second k9s-gap borrow,
  user "let's work on resource browser (:any kind)"; chose **both frontends** +
  **redact Secret values**): a k9s-style escape hatch to *any* kind, not just the
  watched ones. Core `k8s/browse.rs` is the data layer (fetch-not-watch, like
  `logs`/`metrics`): `discover(client)` runs `kube::discovery::Discovery` →
  `KindEntry { api: ApiResource, namespaced }` for every recommended resource
  (subresources with `/` skipped, sorted+deduped by `label()`); `list_kind`
  does `Api::<DynamicObject>::all_with(&ar).list(limit 500)`; `row(obj)` →
  `BrowseRow { namespace, name, age }`. Re-exports `DynamicObject as Object` so
  the TUI crate (no direct `kube` dep) can name it. Drilling a row opens the
  **inspector**, serialized by **`state/inspect::dynamic_yaml`** — which
  **relaxes** the never-read-Secrets guarantee in exactly one controlled way:
  for a `v1` Secret it **redacts** every `data`/`stringData` value to
  `•••• (N bytes)` (keys + sizes kept), and shows ConfigMaps + all else in full
  (user's call — the only sanctioned read of Secret-adjacent content; values
  never leave the redactor). Both pure + unit-tested. **TUI** (`ui/browse.rs`):
  `:` opens a filterable `ResourcePicker` (type to filter, Enter lists, `r`
  refresh); rows in a `BrowseView`; payload-free actions (the `Action` enum
  derives `Eq`, which `DynamicObject` isn't — the app reads the selection from
  the picker/view). **GUI** (`gui/browse.rs`): `:` (Shift+Semicolon) opens a
  mouse + wheel modal — kind picker → generic table → click a row to inspect;
  the net thread holds a `discover_req` flag + `browse_req`/`browse_out` slots
  (drained like `log_req`). LIST-on-demand (no reflector lifecycle) was chosen
  over a live watch. Dev flags: TUI verified via the committed snapshot tests;
  GUI `--browse` (pick mode) / `--browse <kind>` (jump to that table) +
  `--screenshot`. Verified live on kind.
  **Post-review hardening** (2026-06-17, an adversarial review caught a critical
  redaction leak + others): (1) **The apiserver omits `apiVersion`/`kind` on the
  individual items inside a LIST response** (only the envelope carries them), so
  every browsed `DynamicObject` arrived with `types == None` — meaning the
  Secret check in `dynamic_yaml` never fired and Secret values were rendered in
  full. `list_kind` now **stamps** the picked kind's `TypeMeta` onto every item
  (`stamp_types`); this is load-bearing for the privilege posture *and* fixes the
  inspector title (which also reads `obj.types`). Verified live: a planted
  `browser-leak-test` Secret renders `•••• (N bytes)`, the base64 never appears.
  (2) `discover` no longer uses `Discovery::run` (which `?`-fails the *whole*
  enumeration if any one aggregated APIService is down — very common); it
  enumerates groups via the public client methods and **skips a failing group**
  rather than blanking the browser. (3) It drops kinds the server won't LIST (no
  `list` verb — tokenreviews, bindings, …; 70→63 on kind). (4) `list_kind`
  returns `ListResult { items, truncated }` and both frontends show "showing
  first N" when the 500-cap clips. (5) Long ns/name rows are truncated so the age
  column stays aligned. (6) GUI: the `:` open is gated on `panel.is_none()` (no
  opening over a city/node window → no click fall-through); the browser wheel is
  gated on `inspector.is_none()` (no double-scroll); the net thread re-LISTs when
  `browse_out` was just blanked (re-selecting the same kind no longer strands on
  "listing…") and a context switch clears `kinds`/`browse_*`. The TUI clears
  `kinds` on context switch too. Secrets = 0 on the dev cluster normally, so the
  redaction is unit-test-covered (None→stamp→redact, end-to-end) plus the
  one-off live check above.
  **FMEA round** (2026-06-18, "address medium+"): a failure-mode analysis of the
  feature (58 candidates → 43 real → 15 medium+) drove a second hardening pass.
  *Security/privilege:* redaction now fires for a Secret of **any** group/version
  (a `*.Secret` CRD/aggregated API, not just core v1) **and** fails *closed* —
  an object whose `kind` we can't determine (a hypothetical un-stamped item) has
  its `data`/`stringData` masked rather than shown; a Secret's `annotations` are
  dropped (defense-in-depth vs. a `last-applied` base64 copy — `clean_yaml`
  already strips that specific key); and an **inline-credential sweep**
  (`mask_sensitive`) masks string leaves under high-confidence credential keys
  (`password`/`token`/`apiKey`/… — exact-match, so reference fields like
  `secretName` are untouched) on **every** browsed object, catching operator CRs
  that embed secrets inline. *Robustness:* `list_kind` is wrapped in a client-side
  `tokio::timeout` (25s) + a server-side `ListParams.timeout` (20s) so a hung
  LIST can't freeze the GUI net loop (which also runs logs/evict/commit/snapshot);
  `discover` gives each group a 5s deadline and returns `Discovered { kinds,
  warnings }` so a degraded API group shows a "N unavailable" note instead of
  silently vanishing; an empty discovery shows a legible message (not a blank
  picker). *Scope/UX:* `list_kind` now honors the active `NamespaceFilter`
  (per-namespace LIST + merge for namespaced kinds — matches the rest of the app
  and avoids a whole-cluster `Forbidden` for namespace-scoped users); kube errors
  are classified (403 → "forbidden — you can't list X here", 404 → "not served");
  the GUI `browse_out` payload is an `Arc` so the per-frame pull is a refcount
  bump, not a deep copy of up to 500 (possibly large) objects; leaving the table
  stops the ~2s re-LIST poll; `:` opens off the produced character (works on
  non-US layouts); and the TUI gained a `discover_gen` guard (a slow old-cluster
  discovery can't repopulate a switched-to cluster). Deferred (risk already
  covered): a SelfSubjectAccessReview pre-flight for `secrets` (apiserver RBAC +
  redaction + scoping suffice).
  **Performance pass** (2026-06-18, "perf review + mitigation"): a profile of the
  hot paths (GUI is immediate-mode at ~60fps — anything in a `draw_*` runs every
  frame) drove four fixes. (1) The GUI table no longer re-derives every row each
  frame: `Browser` **memoizes** the formatted `{name}{age}` lines, rebuilding only
  when a new `Arc<ListResult>` arrives (`Arc::ptr_eq`), and (2) draws only the
  **visible slice** (`first = scroll/row_h`, ~25 rows) instead of walking all 500
  — turning a per-frame O(items) cost (×500 `row()` clones + `jiff::now()` reads)
  into on-LIST O(items) + per-frame O(visible). (3) `Net.kinds` is an
  `Arc<Vec<KindEntry>>` (like `browse_out`) so the picker's per-frame pull is a
  refcount bump, not a deep clone of ~70 entries. (4) `discover` fans the
  per-group resource queries out **concurrently** (`futures::join_all`) instead of
  sequentially, so `:` open is ~one round-trip deep, not N. Deferred: moving the
  LIST/discover awaits off the net tick loop onto spawned tasks — the FMEA
  timeout already bounds the rare slow-LIST stall and the common case is <1s, so
  it isn't worth reintroducing concurrency into the just-hardened request slots.
- **Log UX — Tier 0** (2026-06-18, user: "make tailing and working with logs
  easier"; chose a tiered roadmap from an ideation pass, did Tier 0 first): seven
  quick wins, most sharing a new pure module **`state/logline.rs`** (no UI deps,
  unit-tested — the single home for log-line logic the two renderers were about
  to duplicate). (1) **Severity coloring**: `classify(line) -> Level` (klog
  `E/W/I` headers, structured `level=`/`"level":`/`"severity":`, bracketed
  `[error]`, uppercase plaintext) → ERROR red / WARN yellow / DEBUG dim; a
  render-only hint, raw text untouched (honors color-discipline). (2) **Filter
  upgrade**: `FilterExpr` = space-separated AND of substrings, leading `!`
  excludes (subtractive triage); replaces the duplicated substring match in both
  frontends. (3) **Timestamps** (`T`): `logs::tail` gained a `timestamps` flag;
  the TUI peels the RFC3339 prefix into a dim gutter (`split_ts` + ratatui
  spans), the GUI shows it inline (proportional font — a measured gutter is
  deferred). (4) **History window** (`s`): a `LogWindow` enum {Tail 500 / More 2k
  / Hour since-1h, capped} so a crash past the 500-line tail is reachable.
  Timestamps + window are carried as a `LogOpts` struct (so future fetch knobs
  don't churn `tail`'s signature) and ride the existing re-fetch rail (TUI
  `RefetchLogs`; GUI `LogReq` `PartialEq`). (5) **Container cache**: both
  frontends were re-issuing `first_container` (`Api::get`) every ~2s poll; now
  resolved once per pod target (GUI: cached in the net loop, reset on switch;
  TUI: cached on `LogsView`, round-tripped via the `Logs` event) — survives
  p/T/s toggles. (6) **Smart `--previous`**: `model::prefer_previous(state,
  reason, restarts)` opens a crash-looping pod (CrashLoopBackOff, or Failing with
  ≥5 restarts) straight on the previous container's last words; threaded through
  `Action::OpenLogs.previous` / `WinAction.log`'s bool (the view can still toggle
  with `p`). (7) **Discoverability**: the TUI pod lists already show `l logs · e
  evict`; help.rs + the GUI Almanac document the in-overlay keys + filter syntax.
  Dev flag `--log-timestamps` (with `--tail`). Verified live on kind: a planted
  deployment's ERROR/`level=warn`/`W0618` lines color red/yellow; coredns shows
  inline timestamps + a `(ts)` title; crashy auto-opens on `<previous>` (smart
  default, no flag). Core: logline (3) + `prefer_previous` (1) tests; TUI:
  exclude-filter test. Deferred to later tiers: a concern→logs verb (T1),
  multi-container picker / all-containers (T2), JSON/logfmt columns (T3+),
  match-navigation + grep-context (T5), multi-pod "whole-city" tailing (B1), the
  honest since-anchored append + log stream (B3/B2).
- **Log UX — Tier 1: concern→logs (`L`)** (2026-06-18, the highest-ROI item from
  the log roadmap): the attention queue is the product's spine — `n` parks you on
  "the city in trouble" — but the next move (its logs) was a 3-step hunt through
  the pod list. The detectors *had* the offending pod's identity while aggregating
  and threw it away (`Target` is workload/node-grained by the "city in trouble,
  not 40 alarms" rule). Now `Concern` carries a `probe: Option<LogProbe>
  { namespace, pod, previous }`: the per-pod loop captures a **representative
  log-worthy pod** (one that actually ran — crash/OOM/Failed/flap; *not*
  Pending/Unschedulable/image-pull/config-error, which have no logs) into the
  workload's `Agg`, **preferring a crash-looper** so `previous` lands on the last
  words (via `model::prefer_previous`). Concerns with no single log-worthy pod
  (replica gaps, nodes, connectivity, events, jobs, pair drift) carry `None`.
  **`L`** opens that pod's logs directly — in the **TUI** from the attention panel
  (focused or `n`-parked, returning `Action::OpenLogs{..,previous}`) and globally
  (`logs_action_for`); in the **GUI** as a map nav key on the focused concern
  (`concern_idx`), enqueuing a `LogReq` from the probe. Both reuse the existing
  fetch machinery — this only routes an identity the queue already computed.
  Load-bearing GUI fix: the panel-match `None` arm used to auto-close any
  panel-less log overlay (assuming logs always had a backing panel) — it now
  doesn't, since a concern-opened log is legitimately panel-less (Esc + the
  `close_panel` path still tear down a panel-backed one); nav is suspended while
  `log_open`. Core `LogProbe` + `prefer_previous`-on-crashloop unit-tested
  (probe present with `previous=true`; a pure replica gap carries none). Dev flag
  `--concern-logs` (GUI). Verified live on kind: `L` on the crashy concern opens
  `crashy-… <previous>` showing "boom" — the previous container's last words.
- **TUI removed; GUI is the product** (2026-06-18, user's call): the ratatui
  TUI was deleted and the macroquad client renamed `kubernation-gui` →
  **`kubernation`** (the binary + `crates/kubernation/`; `cargo run` = it).
  **Rationale:** every feature was being built twice (browser, logs T0/T1, …) —
  a real, recurring tax — and the headless-terminal niche the TUI served is
  better covered by k9s; the 4X metaphor doesn't fit a terminal anyway. So
  rather than a half-maintained TUI, one well-built frontend. **What moved:** the
  `--smoke` CI gate (was a TUI flag) became a UI-free core example
  (`kubernation-core/examples/smoke.rs`, `make smoke`) since the GUI needs a
  display; the `scale_rebuild` frame-budget test (was a TUI TestBackend render)
  became a core test timing `Models::build` — the rebuild the GUI recomputes each
  tick — (`make perf-test`, ~1ms for 100 nodes/1000 pods). **What's lost:**
  headless/SSH operation, and the TUI's ~21 TestBackend snapshot tests (which
  asserted TUI rendering specifically); the *logic* stays tested in core (75
  tests) and the privilege/write posture is unchanged (one file, `actions.rs`).
  `kubernation-core` is untouched and still UI-dep-free. Decision-log entries
  above that describe "both frontends / the TUI does X" are historical record.
- **GUI testability policy** (2026-06-18, after the removal's reassessment): the
  removal deleted the TUI's ~21 TestBackend render snapshots, leaving the sole
  frontend largely render-untested (macroquad is immediate-mode + GL — not
  unit-testable). Recorded choice, two parts. **(A) Pure draw-decision fns —
  policy.** Push the *decisions* a view makes (caps, truncation, severity→role,
  HOT/WARM tagging, what-to-show) into pure functions that return assert-able
  data (no GL calls), and unit-test them against `fixtures.rs` — exactly the
  `state/logline.rs` philosophy. The template is `panels::region_lines`
  (`-> Vec<(String, Color)>`, the tooltip/SELECTION text), now unit-tested
  (`region_lines_name_the_workload_under_a_city`) via a `fixtures`-feature
  dev-dep on core. **Every new GUI view ships such a test** (restores the old
  "new views ship with a test" convention). **(B) Render-smoke — crash gate.**
  `make gui-smoke` (`hack/gui-smoke.sh`) drives all ~16 overlay/modal/map states
  through `--screenshot` and fails on any panic or blank image; needs a display
  (local, not the headless CI `make smoke`). Catches crashes, not wrong output —
  A is the durable answer. (Considered + deferred: golden-image pixel diffs —
  they need a deterministic `--fixture` launch path that doesn't exist; do A
  first, it's deterministic by construction.)
- **Port-forward** (2026-06-18, user "sure, port forward" — the first SOON-tier
  item after the TUI removal, chosen because the removal most directly unlocked
  it and it fits the gated posture): a `kubectl port-forward` equivalent in the
  GUI. **Not a cluster mutation** (it writes nothing), so it lives in
  `k8s/portforward.rs` *beside* the one write file rather than in it — but it's
  an **active capability** gated in the same spirit: RBAC-pre-checked
  (`create pods/portforward` via a `SelfSubjectAccessReview`, `can_forward` — the
  button shows *locked* without it; the apiserver is the real gate, the SSAR is
  for UX), explicit (a click), and individually stoppable. **Core:**
  `portforward::start` binds `127.0.0.1:0`, returns a `Forward` whose `Drop`
  aborts the accept-loop task — which **owns a `JoinSet` of the per-connection
  tunnels**, so the drop tears down in-flight connections too ("stop" means
  stop); each accepted socket is pumped to a fresh `Api::portforward` upgrade via
  `copy_bidirectional`. (We deliberately *don't* await `take_error` after the
  copy — it isn't guaranteed to resolve and could leave a per-conn task unreaped.)
  `default_port` resolves the target as the pod's `containerPort` else a numeric
  Service `targetPort` selecting it — the container-port path needs no Service
  LIST, so a `list services` denial can't mask a usable port; the two resolvers
  are pure + unit-tested. Needs kube's **`ws`** feature (the SPDY upgrade) +
  tokio `net`/`io-util` — the only reason `ws` is on (exec/attach stay unbuilt).
  **GUI:** a hover-revealed green **fwd** button on city CITIZENS / node GARRISON
  pod rows (beside yaml/evict; RBAC-gated; flips to **stop :PORT** when that pod
  is forwarded). The net thread holds the private `Forward` handles in a `Vec`
  paired with a public `ForwardInfo` mirror (`forward_req`/`forward_stop`/
  `forwards` slots + a `forward_perm` RBAC cache, mirroring evict); a hot-context
  switch drops the hot forwards (warm survives) and clears the cache. The right
  column's **FORWARDS** section lists live forwards (`:local>pod ns/pod`) with an
  **x** to stop — the always-visible home; the per-row stop covers the
  window-open case. Dev flag `--forward <substr>` (starts a forward, stays on the
  map so the column section is captured). Verified live on kind: a tunnel to
  `web` served **HTTP 200** then tore down on stop; the SSAR matches
  `kubectl auth can-i create pods/portforward` (admin yes → enabled, unprivileged
  no → locked). Deferred: a manual port-entry field (when neither resolver finds
  a port), forwarding a non-default port, UDP.
- **Live cpu/mem sparklines** (2026-06-18, user "live cpu/mem sparklines" — the
  next SOON-tier item; turns the instantaneous gauges into trends): the
  metrics-server poll already had the *latest* sample; this keeps a **bounded
  history ring**. **Core (`k8s/metrics.rs`):** `Metrics` gains `node_rings`
  (per-node) + `cluster_ring` (per-sample sum across nodes), appended each
  successful poll by `record_sample` (capped at `HISTORY_CAP`=60 ≈ 15 min at the
  15s poll; a vanished node's ring ages out only after `RING_GRACE`=4 consecutive
  absences, so a one-poll metrics scrape hiccup doesn't wipe the trend — a review
  finding). A poll *failure* leaves the
  rings intact but flips `available` false — so the `ObservedWorld` accessors
  (`node_usage_history` / `cluster_usage_history`) read **empty while metrics is
  down** and the trend resumes with continuity after a transient blip (rather
  than wiping 15 min on one failed poll). **Model:** `build_node_detail` turns the
  raw node ring into `cpu_history`/`mem_history` *ratio* series (usage ÷
  allocatable) via the pure `usage_ratios_series` (the node window already had
  `cpu_alloc`/`mem_alloc`) — so the sparkline's height reads like the gauge.
  **GUI:** a pure `panels::sparkline_points` (values+max+rect → clamped polyline
  points, unit-tested) + `draw_sparkline` (well + frame + baseline/top refs +
  the trace + a latest-sample dot). The **node window** draws a sparkline under
  each cpu/mem gauge, scaled to allocatable (max=1.0) and coloured by the latest
  sample's pressure bucket (shared `bucket_color`, the documented <0.7/0.7–0.9/≥0.9
  buckets) — so a capacity-relative trend that matches the gauge. The **STATUS
  column** draws cluster cpu+mem sparklines **self-scaled** to their own window
  peak (an overview "is the realm heating up"), each with the **current value**
  (`{m} / human-bytes`) at the right so a flat-but-steady cluster doesn't read as
  "maxed out" (a review finding). Both render only when metrics-server is up (empty history ⇒ nothing
  drawn — the no-metrics path is the gui-smoke `node` state). Pure parts
  unit-tested (ring cap/prune/aggregate, ratio series, sparkline points); verified
  live on kind with metrics-server (`--inspect <node> --spark` holds the shot
  ~30s for 2–3 real samples; `--spark` alone frames the STATUS sparklines). On an
  idle dev cluster the node trace hugs the floor (cpu ~5%) — honest, since it's
  capacity-relative. Deferred: per-pod sparklines (noisier, the rows are cramped),
  a selectable time window.
- **Blast-radius highlighting** (2026-06-18, user picked it from an AIM SRE
  review — chosen because Kubernation *already owns the topology* the SRE canon
  says you need for impact isolation, so it's uniquely enabled): press **`B`** to
  light up the dependency fan-out of a subject on the map. **Core
  (`state/blast.rs`, pure + unit-tested):** `blast_radius(world, Subject)` walks
  the observed topology — a **Node** cascades node → hosted workloads (pods'
  `node_name` → `OwnerIndex`) → their Services → Ingresses; a **Workload** walks
  its Services → Ingresses. Reuses `build_exposure` (the selector/ingress-backend
  resolver). Returns `Affected` (Workload/Service/Ingress) items with a **hop**
  distance, deduped to the min hop. It **deliberately invents no app-level
  "who-calls-whom" edges** — those need a service mesh / eBPF and a wrong
  dependency is worse than a missing one, so a workload with no Service has an
  honestly empty radius (the read-by-default, don't-fabricate posture). **GUI
  (`draw::draw_blast`):** for the subject's world, pulsing lines spread from the
  source to each affected cell (faded by hop), a warning diamond on each (hop 1 =
  CRIT red, further = WARN amber), a bold crisis ring on the source; `coast_cells`
  resolves Service/Ingress `Affected` to their harbor/gate marks. The subject is
  the **selected tile** (city→Workload, province→Node) else the **focused
  concern's** target, in that subject's cluster's `ObservedWorld` (pair-aware);
  recomputed each frame while on (cheap for real sizes — `build_exposure` over a
  small store). `panels::draw_blast_banner` shows the affected count. Dev flag
  `--blast <substr>` (selects a node — preferred, for the cascade — else a city);
  gui-smoke `blast-node`/`blast-workload` states. Verified live on kind: a node →
  12 affected (cascade through cities + coast), `web` → its Service + Ingress.
  Complements the attention queue (which says *what's wrong*) by showing *what
  else is affected*. **Review fixes:** `Affected::Service`/`Ingress` carry a
  `via` workload so a Service fronting several workloads highlights only the
  affected one's harbor (not healthy siblings on other nodes); the banner counts
  what's actually *placed* on the map (`draw_blast` returns the drawn count, so a
  DaemonSet subject — a road, not a city, with no on-map source — reads as "not
  shown" rather than a phantom count); and the walk is **memoized** (recomputed
  only when the subject or snapshot changes, not every frame while held on).
  Deferred: true downstream consumers (needs Hubble/mesh), a blast list in the
  SELECTION column.
- **The treasury — availability SLOs + error budgets** (2026-06-18, user "implement
  the treasury" from the AIM SRE roadmap — the central SRE observability
  primitive, and the 4X "treasury you spend" makes it more legible than a
  dashboard number): per-workload availability SLOs with an **error budget** the
  city window shows as a coin gauge. **Core (`state/slo.rs`, pure + unit-tested):**
  `SloTracker` holds a rolling per-workload availability ring; `SloStatus`
  (sli / target / budget_remaining / burn / state) is the pure math —
  budget_remaining = clamp(1 − (1−sli)/(1−target)), burn = recent-downtime ÷
  sustainable-rate, `BudgetState` {Warming &lt; MIN_SAMPLES, Healthy, Burning
  (burn&gt;1.5), Breached (budget≤0)}. **The SLI is derived from pod readiness** —
  a workload is "up" at a sample if it has **≥1 available replica** (the textbook
  uptime definition: catches outages / crash-loops, ignores healthy rolling
  deploys; partial capacity loss is the attention queue's replica-gap job). So it
  needs **no metrics-server / Prometheus** — works on any cluster, unlike the
  RED/latency signals (those still need a source). **In-session window** (a
  rolling ring, no cross-restart persistence) — honest *recent* availability, not
  30-day compliance. **Net thread:** samples every ~2s (`SLO_SAMPLE_TICKS`) from
  the **unfiltered** `build_workloads` (SLOs track the whole cluster regardless of
  the namespace view, like the reflectors), forces a rebuild so budgets stay fresh
  on an idle cluster, publishes per-workload `SloStatus` in each `WorldSnap.slo`,
  and appends a **budget concern** (Burning→Warning, Breached→Critical) for any
  workload **not already flagged** by a stronger concern (keeps "city in trouble,
  not 40 alarms"; surfaces the *flaky-but-up-now* cases the instant detectors
  miss). Hot/warm tracked separately; `slo.clear()` on context switch. **GUI:** a
  TREASURY band in the city window — a coin gauge (budget remaining) + the pure
  `treasury_summary` (state→colour/text, unit-tested). Default target 99%
  (`slo::DEFAULT_TARGET`; per-workload config is deferred). Verified live on kind:
  `web` 100% budget (green), `crashy` exhausted (avail ~12%, red). Dev: any
  `--inspect <city> --spark` holds the shot long enough for samples to reach a
  verdict. **Review fixes:** the budget *concern* respects the active namespace
  filter (the SLO *map* stays unfiltered so any city window shows its budget, but
  a filtered-out workload no longer leaks a budget alarm into the scoped queue);
  and the SLI keys on `ready` (a serving pod), not `available`, so a non-zero
  `minReadySeconds` doesn't count a mid-rollout workload down. Deferred:
  configurable/per-workload targets, latency SLOs (need a metric source), a
  multi-window burn-rate alert, persisting budgets across runs.
- **Per-workload / configurable SLO targets** (2026-06-18, user "address the
  settings for per-workload/configurable SLO targets"; design-workflow vetted):
  the treasury's target was hardcoded 0.99. **Precedence: in-session manual
  override > workload annotation > `--slo-target` default > 0.99.** **Core
  (`state/slo.rs`):** `parse_target` ("99"/"99.9" percent, "0.999" fraction;
  rejects ≤0, ≥100% zero-budget, NaN — `Err` with a reason); `SloConfig{default,
  overrides}` with `resolve(wr, annotation) -> (target, TargetSource)`;
  `annotation_target` reads `kubernation.io/slo-target` (read-only, declarative —
  no write); `SloStatus.source: TargetSource` (Manual/Annotation/Default), set by
  `statuses_with`. `build_workloads` parses `WorkloadRow.slo_target` once per
  workload (cheaper than a per-workload store walk). **Net:** per-cluster
  `SloConfig` + a `slo_override_req` slot (`set_slo_target`); captures the
  annotation-target map at each SLO sample; `statuses_with` resolves the
  effective target+source per workload. **GUI (`city.rs`):** the treasury band's
  SLO stepper (`step_target` over a 90→99.95 tier curve, `target_source_tag`) →
  `WinAction.slo_target` → `net.set_slo_target`; the source tag shows
  manual/annotated/default. Pure parts unit-tested; verified live (annotated
  `web` 99.9% reads "annotated"; stepping flips to "manual"). Deferred: writing
  the annotation back, range presets.
- **Game Day — chaos drills** (2026-06-18, user "implement chaos/game-day mode …
  as much as possible with standard Kubernetes resources"; design-workflow chose
  the safety-first spine + the blast/scorecard grafts): resilience drills that
  inject a *real* failure and let you watch the cluster respond — the treasury
  spends, the blast radius spreads, the queue lights up. **Standard resources
  only: reuses the existing write primitives, so chaos adds NO new verb / no new
  resource type** (the RBAC surface is exactly `delete pods` + `patch scale`,
  already gated). **Core (`state/chaos.rs`, pure + unit-tested):** the guards
  (`ns_protected` for kube-system/-public/-node-lease, `node_protected` for
  control-plane) live here and **fail closed**; `Experiment`{KillOne, KillAll,
  Outage}; `plan_chaos(world, exp) -> ChaosPlan{steps, restore, refused, blast}`
  enumerates the concrete `ChaosStep`s (Evict / Scale), captures the Outage
  restore replicas (`current_replicas`, now `pub(crate)`), refuses protected
  targets / DaemonSet-outage / no-pods, and computes the blast via `blast_radius`;
  `ChaosScorecard` + `scorecard_lines`. **Execute-immediately, NOT staged** (chaos
  is imperative + temporal — a poor fit for the desired-state planning turn). The
  **one new write** is `actions::run_chaos` (a sequencer: dry-run the Scale steps
  for the RBAC gate, then run all steps via `evict_pod`/`apply_intervention`) —
  the write surface stays one file. **Net:** `chaos_req`/`chaos_session` slots;
  the drain **re-checks `ns_protected` fail-closed**, captures `budget_before`,
  runs `run_chaos` under a `tokio::timeout`, and tracks **recovery (ready≥1) +
  budget spend** on the SLO samples for the scorecard; hot-cluster-only; cleared
  on context switch. **GUI:** a "Game Day" menu (between Orders and Advisors —
  shifts the `--menu` index map), the `chaos.rs` modal (target picker with
  protected namespaces filtered out, experiment radio, blast+budget preview,
  CRIT "Run drill", the scorecard + a Restore for outages), `draw_chaos_confirm`
  (a blunt CRIT confirm). Dev flags `--chaos <wl>` / `--chaos-go` (auto-runs a
  KillOne). **Review fixes (7, no Critical):** the scorecard now needs the target
  to actually *dip* (ready→0) before "recovered" (a KillOne the workload shrugs
  off reads "stayed up — no outage", not a phantom "self-healed in 0s"); an
  Outage scales the target to 0 → the SLO ring prunes it, so the scorecard keeps
  the last budget reading instead of blanking; the net thread *owns* the session
  (the GUI no longer clears it on close — that raced an in-flight drill — and the
  window shows only a session matching the open target); the chaos window's
  clicks are gated while its confirm is up, and it's in the world-nav suspend
  gate; the net protected-namespace re-check now covers Scale steps too; and
  `run_chaos`'s doc no longer claims a pod-kill RBAC pre-check it doesn't do (the
  apiserver enforces per-DELETE). Verified live (KillOne on `web` → "stayed up —
  no outage"). Node-failure + NetworkPolicy partition shipped in pass 2.
- **Game Day — round 3 (legibility / safety / depth)** (2026-06-18, user
  "do all of the quick wins and mediums" from the chaos-ideation roadmap;
  design + adversarial-review workflows): a broad enrichment of the chaos console
  in five batches, all reusing the existing write primitives (no new verb beyond
  pass 2's netpol). **Legibility:** `plan_summary` dry-run step list in the
  PREVIEW; `MAX_KILL_PODS=50` fail-closed blast cap on every mass-eviction path;
  `budget_verdict` (breach/spend/untouched) tying the scorecard to the treasury.
  **Four new experiments** (pure planner arms + GUI knobs): `KillPercent{pct}`,
  `ScaleSpike{factor}` (capped like the kills), `CordonFreeze` (cordon, no drain),
  and a directional `Partition` (`PartitionDir{Both,Ingress,Egress}` →
  `policy_types`). **Safety triad:** restore-on-exit (`prevent_quit` + a
  quit-intercept that runs the restore before exit, 27s backstop > the 25s worker
  timeout), opt-in auto-restore (`auto_restore_secs` → `auto_restore_tick`), and
  restore-on-context-switch (undo with the OLD client before reconnecting) — so a
  drill never strands the cluster; plus an all-or-nothing RBAC gate (`run_chaos`
  SSAR-pre-flights `delete pods` per evicting namespace + a self-contained
  protected-ns failsafe). **Observability:** a steady-state gate (`healthy_before`
  — "baseline noisy" when the target was already degraded), **MTTD** (when the
  attention queue first flagged the drill — Kubernation measuring its own
  observability), and a recovery-curve sparkline. **4X loop:** a live "raid
  underway" attention concern, flip-watch (auto blast-radius on the live raid),
  and an in-session CHRONICLE of finished drills. An operator undo is labelled
  "restored", never "self-healed". A follow-up shipped the deferred medium —
  **difficulty tiers** (`Tier{Skirmish,Raid,Siege}` + pure `plan_tier`): named
  *compound* drills that compose existing experiments into one sequence with a
  LIFO restore (Siege = Partition + KillAll, undone deny-all-last), run through
  the same `ChaosRun`/`run_chaos` gate (subject=Workload, watch=[target]); a TIER
  row in the window overrides the single-experiment choice. The interesting logic
  stays pure + unit-tested in `state/chaos.rs` (the scorecard/verdict/cap/summary/
  steady-state/`plan_tier` are pure draw-decision fns); the GUI window grew to a
  TIER row + 9 experiments + knobs + dry-run + scorecard (recovery sparkline) +
  chronicle. **Deferred** (teed up for a future
  pass): collateral-concern correlation;
  markdown after-action report (a local-file write); persisted run history;
  warm-cluster chaos + a hot→warm failover drill; mesh/sidecar stress + latency
  (need Istio/Linkerd or an injected Job — no exec by posture). Two known minor
  gaps: flip-watch overrides a manual `B`-dismiss during the ~30s raid window;
  MTTD can misreport a "monitoring gap" when the drill target is hidden by the
  active namespace filter.
- **Incident-value roadmap, items #1–#4** (2026-06-19, user "start building in the
  order you are recommending" from `docs/ROADMAP.md`'s top-10): four front-loaded,
  posture-safe, high-frequency incident features, each pure-core + unit-tested with
  a thin GUI surface. **#1 Pod-not-Ready explainer** (`state/diagnose.rs`):
  `diagnose(reason, restarts, oom) -> Option<Diagnosis>{kind, reason, explain,
  hint}` turns a pod's raw reason (CrashLoopBackOff / ImagePull / Config /
  Unschedulable / NotReady-probe / Pending / OOM-overrides-crash / generic) into a
  plain-English why + next-action hint; hung on `CityPod`/`NodePodRow.diag`; the
  city + province windows show a `why:`/`fix:` line for the worst not-ready pod.
  **#2 Runbook hints** (`attention::next_action(&Concern) -> Option<String>`):
  keyed on the concern's stable `key` prefix (+ probe) → the in-app verb to act on
  it (`L: logs`, `B: blast`, click→open, pvc→StorageClass, orphan-ingress→backend,
  idle-svc→selector, slo→TREASURY, pair→HOT/WARM); the sidebar ATTENTION section
  shows `next: <hint>` for the focused concern. **#3 Rollout history**
  (`state/rollout.rs`): a pure resolver over the watched ReplicaSet store —
  `revisions(world, wr)` (newest first, current marked, from
  `deployment.kubernetes.io/revision` + RS pod template), `previous()`,
  `image_changes(from, to)`, `revision_template()`; Deployment-only (STS/DS
  revisions live in unwatched ControllerRevisions); the city window grew a HISTORY
  section (prior→current image delta + recent revisions). **#4 Rollback — the
  planning turn's 5th verb** (`Intervention::Rollback{workload, to_revision: i64}`,
  Eq-safe so `ChaosStep`'s Eq derive still holds): the HISTORY section's per-revision
  `rollback` button stages it; `plan_diff` shows `rev X → rev Y`; `actions::
  apply_intervention` resolves the target revision's pod template **from the live
  cluster** (a LIST in the one write file — keeps the apply world-free, so no
  net/chaos signature churn) and merge-patches `spec.template` (containers array
  replaced wholesale, like `kubectl rollout undo`), through the same all-or-nothing
  dry-run/commit gate. Dev flag `--rollback <substr>` (+ `--plan-go` to commit).
  Verified live on kind: crashy's explainer reads "CrashLoopBackOff … 159 restarts
  / tail the previous container (p)"; web's HISTORY shows the 1.27→1.28 image
  delta; a staged rollback rolled web `nginx:1.28-alpine` (rev 7) →
  `nginx:1.27-alpine` (rev 8) and rolled out clean. 135 core tests; gui-smoke 25.
  Deferred (next in the roadmap): right-sizing advisor (#5), self-scoped RBAC
  matrix (#6), hardening scan (#7).
- **Right-sizing advisor (#5)** (2026-06-19, roadmap item #5; design-workflow
  vetted — 4 lenses → 3 judges → synthesis — then adversarially reviewed): a 4th
  **Advisors** tab comparing each workload's per-replica resource *requests* to
  metrics-server *usage*. **Pure core** `state/advisor::rightsizing_report(&world)`
  → `RightSizingReport` (over/under/unrequested `RsRow`s + reclaimable cpu/mem +
  node-equivalents), built by grouping running member pods per workload
  (`OwnerIndex`), summing requests/limits (extracted `model::sum_pod_requests`/
  `sum_pod_limits`, `spec.containers` only — init excluded) and usage
  (`world.pod_usage`, latest sample, mean over the pods that reported). **Classify
  (test-pinned consts):** Over when mean usage < 0.5·request; Under when
  CPU-mean ≥ 0.9·request or **memory-PEAK** ≥ 0.8·request (incompressible — the
  hottest replica OOMs, not the average); Unrequested when `request==0` with
  running pods (a static fact — survives degrade-dark); a `measured_pods==0`
  guard yields **Unknown** (never a false "waste" when a workload is momentarily
  unsampled); `request:=limit` for limits-only pods; **limit-ratio** escalation
  notes (CFS throttle / OOMKill). **Recommend** (VPA-style): `usage ÷ target-util`
  (0.65 cpu / 0.50 mem) clamped to the peak + VPA floors (25m / 250Mi), rounded
  up — and a **floor-negation guard** demotes an "Over" whose suggestion would
  *raise* the request (tiny workload below the floor) back to RightSized, so
  "waste" never suggests an increase. **reclaimable = Σ(request−suggested)·
  measured_pods** over Over rows (never invented dollars; node-equivalents via the
  median node allocatable cpu). **READ-ONLY** (advice only — editing
  `resources.requests` is deliberately *not* a 6th write verb; the footer says
  apply via kubectl/manifest), **cluster-wide** (not namespace-scoped, like the
  other advisors), **metrics-server only** (degrades dark to just the
  scheduler-blind list — the only finding needing no metrics), and honest about
  the single-sample basis ("directional, not a multi-day VPA fit"). GUI: a pure
  `gui/advisor::rightsizing_lines(&report) -> Vec<(text, RsRole)>` (the
  testability-policy draw-decision fn, unit-tested) rendered by `page_rightsizing`;
  the tab is `--advisor rightsizing` / Key4 / the Advisors menu. **Deferred** (the
  struct is shaped for it): a per-pod usage history ring for true P90/P95 sizing;
  per-container suggestions; latency/throttle-event signals (need a source
  metrics.k8s.io lacks). Verified live on kind (metrics-server up): coredns/kindnet/
  metrics-server/web/db flagged CPU-over with `~target` < request; kube-proxy +
  local-path-provisioner scheduler-blind (BestEffort); reclaimable 590m cpu.
  **Adversarial-review hardening** (12 confirmed findings fixed): a memory-Under
  now always recommends a genuine *raise* (suggest_mem clamps to peak·1.25, and a
  symmetric guard demotes any Over/Under whose suggestion would contradict its
  bucket) and the Under row shows the *peak* (the driver), not the mean — killing
  a `300Mi<90Mi ~raise 256Mi` contradiction; **native sidecars**
  (`restartPolicy:Always` initContainers) are counted in the request sum to match
  what metrics-server measures (also fixes `node_request_ratios`); only **Ready**
  pods feed the usage mean (a crash-looping replica no longer drags a healthy
  workload into a false Over — verified live: crashy → "not measured", not waste);
  reclaimable is summed per-resource (a cpu saving on an Under-bucket row still
  counts); **NodeMetrics-up-but-PodMetrics-empty** degrades dark instead of a
  false "all right-sized"; the count strip drops the misleading "X / Y" for a
  `not measured: N` line so the parts sum to total; rows truncate to the window
  width; QoS float-eq uses a relative tolerance. 153 core + 18 GUI tests; gui-smoke
  26. Deferred: a per-pod history ring (P90/P95 sizing); per-container suggestions;
  the mid-rollout reclaimable estimate (uses the uniform max request — covered by
  the "directional" disclaimer). Next: self-scoped RBAC matrix (#6).
- **The Charter — self-scoped RBAC (#6)** (2026-06-19, roadmap item #6;
  design-workflow vetted — 4 lenses → 3 judges → synthesis): a read-only **Help ▸
  Charter** modal showing what the *operator* can do — a curated `can-i` grid for
  the active namespace + a realm-wide (cluster-scoped) band, the DevSecOps
  beachhead. **Data: SSAR-per-cell, authoritative** — `k8s/rbac.rs`
  (`can_i`/`matrix` via `SelfSubjectAccessReview`, the exact `kubectl auth can-i`
  mechanism, `join_all` one burst + 25s timeout) decides every cell; we
  deliberately **don't** use `SelfSubjectRulesReview` for the grid (it forces
  client-side wildcard/apiGroup re-matching that can be subtly wrong + misses
  Node/Webhook authorizers). For a "kills surprise 403s" feature a false ✓/✗ is
  the one unacceptable failure, so `Verdict::Unknown` is **never fabricated** into
  allowed/denied (a missing answer renders `?`; an all-unknown grid → a
  `Trust::Unavailable` banner). **Pure** `state/charter.rs` owns the curated probe
  set (the OWASP-K03 escalation primitives — exec / secrets-list / rbac-write /
  SA-token / node patch+proxy — *and* Kubernation's own write surface, so it
  doubles as "which features work for me here") + `build_charter` (folds positional
  verdicts → grid + rollups; a short verdict vec degrades the tail to Unknown, no
  panic) — both unit-tested. Allowed **dangerous** (Critical/High risk)
  capabilities render in CRIT/WARN — the audit finding; denied is calm Dim (normal
  for a scoped user). Lives in the read/data layer (NOT `actions.rs` — it writes
  nothing); the SSAR is a self-query (no escalation, no secret values). Net mirrors
  the `discover_req`/`browse_out` slot pattern (`charter_req` → `charter_out`
  cache per (cluster, ns) + a `charter_gen` guard; cleared on context switch).
  GUI: a `window.rs` modal (`gui/charter.rs`) with a namespace scope toggle + the
  pure `charter_lines`/`charter_banner` draw-decision fns (unit-tested). Dev flag
  `--charter [ns]`. Verified live on kind: admin → 34/34 ✓ (22 dangerous granted,
  highlighted); a restricted ServiceAccount (get/list pods only, via a throwaway
  kubeconfig) → only those two ✓, everything else ✗ — matching `kubectl auth
  can-i`. **Adversarial-review hardening** (5 findings fixed): the deployments
  write-surface cell probes **`patch`, not `update`** (every Kubernation deployment
  write — scale/restart/image/rollback — is an HTTP PATCH, so `update` gave a false
  ✓/✗ for the feature's own writes — the one unacceptable failure), pinned by a
  regression test; a `create networkpolicies` probe was added (the chaos partition's
  write verb); the banner **splits capability from danger** so a locked-out grid
  ("0 of N") never renders green (green ⇒ "all good" misread); `charter_lines` groups
  by label irrespective of authoring order; and the namespace toggle from an
  out-of-list focus lands deterministically without skipping. 160 core + 21 GUI
  tests; gui-smoke 27. **Deferred**: the SSRR raw-rules
  pane (a verbatim, caveated secondary view — never colors a cell); warm-cluster
  Charter (hot-only, like advisors/SLO); resourceNames-granular cells (the grid
  answers "can you act on this resource type here"); a "denied on a Kubernation
  write-verb = CRIT" highlight. Next in the roadmap: hardening scan (#7).
- **Security / hardening scan (#7)** (2026-06-19, roadmap item #7; design-workflow
  vetted — 4 lenses → 3 judges → synthesis): a 5th **Advisors ▸ Hardening** tab +
  attention-queue concerns, linting each workload's pod *template* for security
  misconfigurations. **Pure core** `state/harden.rs`: `scan_template(&PodTemplateSpec)
  -> Vec<Finding>` (takes a template directly so every rule is unit-testable without
  a cluster) + `hardening_report(&world)` (iterates `build_workloads`, resolves via
  the now-`pub(crate)` `model::workload_template`, buckets by worst severity). **Rules**
  (each tagged with the standard it maps to): HARD01 privileged / HARD02 host-namespace
  / HARD03 dangerous-capability (add outside the PSS-baseline allow-set) / HARD04
  hostPath → **Critical** (PSS-baseline); HARD10 effective-run-as-root / HARD11
  allowPrivilegeEscalation≠false / HARD12 caps-not-dropped-ALL / HARD13
  writable-root-fs → **Warning** (PSS-restricted); HARD20 no cpu+mem limits / HARD21
  `:latest`-or-untagged (a `@sha256` digest is never flagged) / HARD22 automount
  SA-token → **Info** (Popeye/OWASP). **Local `HSeverity`** (mapped to
  `attention::Severity` only at the net boundary — no core→attention cycle).
  **Scope:** regular containers + native sidecars (`restartPolicy:Always`
  initContainers); plain init/ephemeral excluded; Deploy/STS/DS only. **Dedup:**
  privileged suppresses HARD11+HARD12 on that container. **Honesty (load-bearing):**
  **seccomp + default-ServiceAccount are deliberately NOT checked** — the kubelet's
  `SeccompDefault` makes a static-template seccomp check false-positive, and the SA
  object isn't watched — and the footer states it's a curated subset, not full PSS
  compliance. `norm_cap` strips `CAP_`/uppercases so `CAP_SYS_ADMIN`==`SYS_ADMIN`.
  **Queue:** ONLY a `worst==Critical` workload becomes **one aggregated Concern**
  (hot-only, respects the namespace filter, suppressed if a stronger concern already
  flags it — "city in trouble, not 40 alarms"); Warning/Info are advisor-only. The
  `next_action` gained a `harden:` arm → "open Advisors ▸ Hardening". **READ-ONLY**
  (lives in `state/`, no new write verb), cluster-wide advisor, metrics-free. GUI:
  the pure `gui/advisor::hardening_lines` (testability-policy draw fn) + the 5th tab
  (`--advisor hardening` / Key5 / menu). Dev flag `--advisor hardening`. Verified live
  on kind: "0/9 fortified · 2 critical · 6 warning" — kindnet + kube-proxy correctly
  Critical (real hostNetwork+hostPath), demo workloads PSS-restricted Warnings,
  metrics-server Info; the 2 criticals appear as one aggregated queue concern each
  with the runbook hint. **Adversarial-review hardening** (7 findings fixed): the
  queue dedup now suppresses a hardening Critical only when an EQUAL-severity
  (Critical) concern already covers the workload — a mere Warning/Info no longer
  masks a Critical security finding; **protected namespaces (kube-system/…) are
  excluded from the *queue*** (their CNI/kube-proxy posture isn't the operator's to
  fix and would permanently squat the queue — still shown in the advisor tab); the
  green "fortified" all-clear fires only when something was actually scanned clean
  (never on an empty/all-unresolved cluster); the headline separates the axes
  (`N critical · N warning · N info · N clean of T`) instead of a misleading
  clean/total fraction that Info nits drove to ~0; HARD20 fires on *either* missing
  limit (a missing memory limit is the real OOM risk), naming which; and the
  standard tag lists distinct standards for a mixed bucket. 171 core + 22 GUI tests;
  gui-smoke 28. **Deferred** (the report's `counts_by_*` are the raw material): the
  posture score (0–100, its own roadmap item); seccomp + default-SA
  (false-positive-prone); Jobs/CronJobs/bare pods;
  hostPort/AppArmor/SELinux/sysctls/probes; PSA-enforcement simulation.
- **Dependency / impact triage panel (#8)** (2026-06-19, roadmap item #8;
  design-workflow vetted — 3 lenses → 2 judges → synthesis): the blast-radius
  overlay (`B`) gained a navigable **IMPACT** section in the right column (a 4th
  consumer of the docked column, between ATTENTION and FORWARDS, shown only while
  the overlay is active). **No core change** — `state/blast.rs` already computes
  `BlastRadius`; the panel is a pure GUI list over the *memoized* `blast_cache`
  (never recomputes the topology walk). `sidebar::impact_rows(blast, &workload_
  severity, &WorldModel, cap) -> Vec<ImpactRow>` (pure, unit-tested): one row per
  affected city/harbor/gate with a hop badge, **ordered hop-asc then health-DESC
  within a hop** (a failing dependent floats to the top of its tier + survives the
  `IMPACT_CAP`=8 cap → "+N more"), each carrying its resolved LOCAL cell. **Health**
  cross-refs `Models.workload_severity` — a Workload uses its own, a Service/Ingress
  **inherits its `via` workload's** (no invented endpoint health; topology-only
  honesty, like the blast core's refusal of fabricated app edges — an empty radius
  shows "nothing downstream derivable"). **Navigation:** a clickable row sets
  `SidebarHit.focus_impact` = the cell; the main loop converts to global
  (`local + sw.off`), selects + `cam.fly_to`, and opens the city window when
  `region_at` is a City (coast marks have no panel — the SELECTION box describes
  them); the blast subject/highlight stay put so you walk the cascade row by row.
  **DRY:** the per-`Affected`→cell match was lifted from `draw_blast` into
  `draw::affected_cell` (`pub(crate)`), so the list and the on-map flash resolve
  through one path and can never disagree. READ-ONLY; reuses the existing `--blast`
  dev flag. Accepted tradeoff (documented): `workload_severity` is from the
  filtered `Models` while `blast_cache` is unfiltered, so a filtered-out troubled
  dependent shows neutral health. Verified live on kind: a blast on the worker node
  lists 11 affected, crashy floated to the top with its severity marker; the on-map
  flash + banner stay; clicking flies + opens. 171 core + 26 GUI tests; gui-smoke
  28. **Deferred**: a keyboard cycle of IMPACT rows; an IMPACT modal / inline
  per-row actions; true downstream-consumer edges (need a mesh/eBPF — the blast
  core refuses to fabricate them).
  **Adversarial-review fixes** (5 confirmed; HIGH + MEDIUM + 2 LOW fixed, 1 LOW
  accepted): (HIGH) clicking an IMPACT row used to set `selected`, which silently
  **re-rooted the blast subject** next frame (the subject is re-derived from
  `selected` each frame) — breaking the "walk the cascade" invariant *and* forcing
  a fresh topology walk; the handler now flies + opens the city panel **without
  touching `selected`**, so the subject + highlight stay anchored on the troubled
  source. (MEDIUM) a city-less subject (a DaemonSet road reached via the
  focused-concern / raid fallback) made `draw_blast` return None → the banner read
  "select a subject" while IMPACT showed a full list; the banner now falls back to
  the radius length (`affected.or(radius.len())`) so it reads "N affected" instead.
  (LOW) the IMPACT label **front-loads the hop** (`h{n} {kind} {ns}/{name}{via}`)
  so right-truncation eats the long name/via tail before the diagnostic cascade
  depth; (LOW) the FORWARDS loop gained a bottom-stop break guard (IMPACT above it
  can push it down on a short window). Accepted: Info-severity rows share the
  healthy stone colour — the severity glyph is the distinguisher (same convention
  as the ATTENTION column).
- **Change timeline — "The Annals" (#9)** (2026-06-19, roadmap item #9;
  design-workflow vetted — 4 lenses → 3 judges → synthesis): a recent, classified
  change-feed answering "what changed?", the **third triage axis** beside the
  attention queue (what's wrong) and blast/impact (#8, what else is affected).
  **Pure core** `state/timeline.rs` (`build_timeline(world, opts, ops, now) ->
  Timeline`, unit-tested) merges three sources newest-first: (a) the recent-events
  ring (`recent_events()`, bounded ~500, deduped — recent, not an audit log); (b)
  ReplicaSet **revisions** via `rollout::revisions` — the **authoritative deploy
  record** (the ring dedups `ScalingReplicaSet` by reason and would hide
  intermediate rollouts, so deploys come from the RS store, Deployment-only); and
  (d) an injected `&[OperatorAction]` slice of **in-session operator actions** —
  the GUI owns these facts (commits/evicts/chaos) and passes them in, keeping core
  pure + persistence-free. `classify_reason` maps event reasons → (ChangeKind,
  Severity), **regression-pinned against `attention.rs`'s vocabulary**; `now` is
  passed in (clockless core — the accepted windowed-recency exception, like
  `attention::build`). **Rules:** event-sourced entries are windowed
  (`TIMELINE_WINDOW_MIN`=15), Deploy + operator entries always kept (full rollout
  history / sparse in-session actions); untimed entries trail at a deterministic
  key-sorted tail (never epoch-0, never the fault-line anchor / a suspect); cluster
  scope applies the `NamespaceFilter` and **drops PodChurn** (Started/Killing
  floods the realm view) while subject scopes keep it; a Deploy entry suppresses
  the redundant per-pod `SuccessfulCreate`/`SuccessfulDelete` on its covered RS.
  **Correlation is honest adjacency only** (matching the blast core's refusal to
  fabricate edges): ordering + a `first_trouble` fault line + a render-time
  "(before the failure)" suspect cue (a change within `CORRELATION_WINDOW_MIN`=10
  before the first failure) — never "caused by". **GUI** `gui/timeline.rs`: the
  pure `annals_lines(tl, now, cap)` draw-decision fn (unit-tested incl. the
  **colour-discipline** invariant — only Failure/Warning+ + warn/crit operator
  actions get red/yellow; benign Deploy/Scale/churn stay cyan/calm/dim), plus the
  cluster-wide `Annals` **modal** (window.rs) opened from **View ▸ Annals**, the
  **`H`** key (History — top-level `T` is the planning turn, log-overlay `T` is
  timestamps, so `H` was the free mnemonic), and `--annals`. The **city + node
  windows replaced their separate HISTORY + CHRONICLE lists with one merged ANNALS
  section** (scope=Workload/Node), the city keeping its per-revision `rollback`
  button (reads the Deploy entry's revision; current revision excluded). **net.rs**
  holds a bounded (~64) in-session `operator_actions: Arc<Vec<OperatorAction>>`
  ring, appended on a successful **hot** eviction, each applied commit intervention
  (`op_action_for`, only when `outcome.applied` — never a failed/blocked commit),
  and a new non-restore **hot** chaos drill; cleared on context switch (no
  cross-cluster leak, no cross-run persistence). Glyphs are **ascii-safe** (the GUI
  `theme::ascii` maps non-allowlist chars to `?`, so detail uses `->`/`(none)` and
  the row glyphs are `*`/`^`/`↔`/`#`/`!`/`·`). READ-ONLY (no new write verb — the
  only write touched is the existing rollback staging). Core re-exports `Time` +
  `jiff` (`lib.rs`) so the UI crate can name the time types. Verified live on kind:
  the modal shows crashy's BackOff failures (with `×16143` counts) above the
  "trouble begins here" line, then web's rollout history (`rev 7->8 · nginx:…`);
  the city ANNALS shows the merged feed + working rollback (staged `rev 8 -> rev
  7`). 186 core + 32 GUI tests; gui-smoke 29. **Deferred** (the `Timeline` struct
  is shaped for these): postmortem/markdown export (`timeline_markdown` over the
  same struct); an alert-correlation engine (widen the suspect set to #8's
  blast radius); STS/DS revision history (ControllerRevisions unwatched); bulk
  object-creation entries (`ChangeKind::WatchedCreation` arm); warm-cluster Annals;
  configurable/wider window; SLO-burn-onset anchoring of `first_trouble`.
  **Adversarial-review fixes** (9 confirmed → 6 distinct): (HIGH) the Annals modal
  was missing from the **world-navigation suspend gate** (it opens over the bare
  map, so `panel_modal` didn't cover it) — a click on/through it leaked to map
  select / panel-open underneath; added `annals.is_some()`. (HIGH) the commit
  handler logged an operator action for **every** intervention when
  `outcome.applied` — but `applied` only means the dry-run passed; a real PATCH can
  still fail per-row, so a *phantom* write could become a false correlation
  suspect; now gated per row (`zip(rows).filter(|r| r.ok)`), matching the evict
  path's `res.is_ok()`. (MEDIUM/LOW) `touches` matched events by a raw
  workload-name prefix (build_city's heuristic), leaking a **sibling** (`web` vs
  `web-api`) and a same-named pod in **another namespace**; reworked to match the
  workload + its ReplicaSets exactly (ns,name) plus an **RS-name prefix** for pods
  (the RS pod-template-hash disambiguates siblings *and* still catches a now-deleted
  pod whose events linger in the ring; STS/DS with no RS fall back to the
  workload-name prefix), and node-scope pod matching is now (ns,name)-qualified.
  (MEDIUM) the suspect cue used `0..=window` so a change at the *exact* failure
  instant was flagged "before the failure" — now strictly `1..=window`. (LOW)
  `annals_lines`' `age` read the wall clock (non-deterministic, inconsistent with
  the `now`-based `bucket`) — added `util::format_age_at(now, then)` /
  `format_age_opt_at`. (LOW) `CityModel.events` was dead after the city dropped
  CHRONICLE — removed it (and its per-render ring scan). New regression tests pin
  each. 186 core + 32 GUI tests; gui-smoke 29.
- **NetworkPolicy coverage map — "unwalled cities" (#10)** (2026-06-19, roadmap
  item #10, the last of the Top-10; design-workflow vetted — 4 lenses → 3 judges →
  synthesis): OWASP K07 (Missing Network Segmentation) as the 4X "walls" feature.
  A workload with no NetworkPolicy isolating its **ingress** is an **unwalled
  city**, open to lateral movement. **READ-ONLY** — NetworkPolicy becomes the
  **13th watched reflector** (`k8s/watch.rs`, its own `WorldDelta::NetworkPolicies`
  bit, `ObservedWorld.networkpolicies`), but the feature only *reads* coverage;
  it adds **no write verb** (the chaos `apply_partition` write is separate +
  unchanged). **Pure core** `state/netpol.rs` (`coverage_report(world) ->
  NetpolReport`, unit-tested): a workload is "walled (ingress)" iff ≥1
  NetworkPolicy in its namespace `selector_matches` its pod-template labels and
  the policy's `effective_policy_types` include Ingress. **k8s semantics, exact:**
  empty/None podSelector selects all-in-namespace; `match_labels` exact AND
  `match_expressions` (In/NotIn/Exists/DoesNotExist, In-on-absent-key=no,
  NotIn-on-absent-key=yes); an unknown operator **fails CLOSED** (→ unwalled —
  never a false "walled" that hides a gap); policyTypes verbatim when present,
  else `[Ingress]` + Egress iff egress rules exist; per-namespace scoped; coverage
  = isolation **presence**, not allow-rules. The **headline finding** = unwalled
  **AND exposed** (`build_exposure`-fronted, reachable). `Models` gains `coverage`
  + `exposed` (cluster-wide/unfiltered, mirroring `workload_severity`); the map
  overlay, the breach mark, the advisor, and the queue all read coverage from the
  one `coverage_report`/`Coverage` so they can't disagree. **Walls surface**
  (`draw.rs`): an `Overlay::Coverage` ("walls") recolours each province
  (exposed-unwalled → amber `heat_pair(1)`, any unwalled → idle, all walled →
  calm slate `walled_pair()`), plus a per-city **breach notch** (`wall_mark` /
  `draw_breach`, drawn only under the Coverage overlay at Regional/Local — walled
  cities draw **nothing**: the *gap* is the finding, and a wall ring would collide
  with the existing population keep-wall). The **Network advisor** gains a WALLS
  section (pure `advisor::walls_lines` — axes separated, finding-first, honesty
  footer). One **Warning** concern per unwalled-&-exposed workload (net.rs, mirrors
  the harden #7 loop: namespace-filter-respecting, protected-ns + already-Critical
  suppressed; `netpol::workload_concern` keeps core attention-enum-free); a
  `next_action` "netpol" arm. **Honest limits** (stated in the advisor footer):
  matchExpressions handled, but namespaceSelector / ipBlock / port-level rules are
  not analyzed, CNI **enforcement** is not verified, and Cilium/Calico CRD
  policies are not read — the RBAC-denied/empty-store path reads "unwalled"
  (fail-safe). View-menu "Walls (segmentation)" radio + `--overlay walls` +
  gui-smoke `overlay-walls`; `hack/samples.yaml` walls `db` for the dev story.
  Verified live on kind: the Network advisor reads "1/9 cities walled · 3 unwalled
  & exposed · 1 policies", web/coredns/metrics-server listed as the K07 finding,
  `db` fortified, kube-system/local-path-storage flagged wide-open; the walls
  overlay tints web's province amber. 198 core + 36 GUI tests; gui-smoke 30.
  **Deferred** (shaped as grafts on `Coverage`/`NetpolReport`): CNI-enforcement
  probe; namespaceSelector/ipBlock/port allow-graph; egress-destination overlay
  (`Coverage.egress` is already stored); Cilium/Calico CRD policies; a
  segmentation/posture score; warm-cluster walls. **Adversarial-review fixes** (6
  confirmed, all LOW → 4 distinct): `selector_matches` now fails **closed** on a
  malformed empty-`values` `In`/`NotIn` (a `NotIn []` would otherwise match every
  pod → a false "walled"; apiserver-unreachable but the pure fn now can't be
  tricked); the netpol Warning is suppressed under a **hardening-sourced** Critical
  in the same pass (`flagged_crit` is now mutable + updated as the harden loop
  pushes — it was a pre-harden snapshot); the advisor's "no exposed city is
  unwalled" green all-clear is gated on `workloads > 0` (no false green on an empty
  cluster); and the honest-limits doc + advisor footer now state that matching is
  on **pod-template** labels (a policy keyed on a pod-only label reads unwalled).
  **This completes the Top-10 incident-value roadmap (#1–#10).**
- **Posture score — "realm defense"** (2026-06-19, the roadmap "Next"-tier item
  *Posture score, after the scans land*; design-workflow vetted — 3 lenses → 2
  judges → synthesis): a 0-100 severity-weighted rating + tier capping the
  security trio. **Pure core** `state/posture.rs` (`posture_report(world) ->
  PostureReport`, unit-tested) is the **single importer** of both security scans
  (`harden::hardening_report` #7 + `netpol::coverage_report` #10, one call each —
  so the score can't disagree with the tabs it summarizes). **Methodology:** two
  start-at-100-and-deduct axis sub-scores, blended `0.6*fortifications +
  0.4*walls` (pod-security heavier — breakout > lateral movement). FORTIFICATIONS
  = `100 − 22·crit − 6·warn − min(1.5·info, 10)` over **operator** workloads
  (worst-severity bucketed, one deduction each); WALLS = `100 − 14·unwalled_exposed
  − 5·wide_open_ns`. **System namespaces** (`chaos::ns_protected`: kube-system/…)
  are scored **separately** (`system_critical`/`_warning`, surfaced dimmed) and
  **never deducted** — the distro's CNI/kube-proxy Criticals aren't the operator's
  to fix (the load-bearing exclusion, mirroring the #7 queue). **Two anti-traps**
  (test-pinned): a high linear CRIT weight + **no presence floor** so one
  privileged pod visibly dents (fort 78), and an **Info cap** (10) so hygiene nits
  can't tank a crit-free realm (fort ≥ 90). **Honest:** `score = None` ⇒
  *Unscanned* (never a green all-clear on an empty / all-mid-sync cluster); a
  curated-subset footer (not CIS/full-PSS); *Defended* is parchment/neutral, NOT
  green/cyan (colour discipline). **Banding:** ≥90 Fortified / ≥70 Defended / ≥40
  Exposed / else Breached. **Explainable:** ranked `factors` (one per non-empty
  operator bucket, points-desc) name up to 3 offenders + the target tab; the Info
  bucket carries a `capped` flag. **GUI:** a 6th **Advisors ▸ Posture** tab (pure
  `advisor::posture_lines` — headline + per-axis sub-scores each tinted by its own
  band so a Breached WALLS axis stays red even under a Defended blend + ranked WHY
  + footer) and a **DEFENSE chip** in the STATUS column (pure `sidebar::
  posture_chip`, tier→stone colour). **Perf:** the report is memoized once per
  tick on `net::WorldSnap.posture` — the chip is on the 60fps sidebar, so it must
  not re-scan per frame; the tab reads the same memoized field. Menu item,
  `--advisor posture`, `Key6`, gui-smoke `advisor-posture`. Read-only; no new write
  verb. Verified live on kind: **DEFENSE 74/100 — DEFENDED** (fort 70 from 5
  PSS-restricted warnings, walls 81 from web's K07 + a wide-open namespace;
  kindnet/kube-proxy's 2 system Criticals excluded + shown dimmed). 209 core + 39
  GUI tests; gui-smoke 31. **Deferred** (structs sized for them): an RBAC-danger
  3rd axis (needs a cluster-wide RBAC scan, not the self-scoped Charter #6); CIS/
  full-PSS compliance; a posture trend ring; per-namespace sub-scores; clickable
  factor→tab jump; a map overlay tinting provinces by posture; warm-cluster posture.
  **Adversarial-review fix** (1 confirmed, MEDIUM): the `scanned` gate mixed
  operator-scope (`operator_total`) with the cluster-wide `h.unresolved` /
  `h.workloads_total`, so a resolvable **system** workload (kube-system) could
  unlock a green score while every operator workload was still mid-sync (a false
  all-clear). Now operator-scoped: `scanned = operator_resolved > 0`, counting
  operator workloads whose pod template actually resolves (`model::
  workload_template`) — pinned by a regression test (resolvable system + all
  operator unresolved ⇒ Unscanned). 210 core + 39 GUI tests.
- **Postmortem / after-action export** (2026-06-19, the roadmap "Next"-tier item
  *Postmortem / after-action export — one local file*; design-workflow vetted — 3
  lenses → 2 judges → synthesis): one click writes a markdown after-action report
  of the current session. **Pure core** `state/postmortem.rs`
  (`postmortem_markdown(input, now) -> String`, unit-tested) composes the things
  already built — the change timeline (#9 Annals), the attention queue, the
  posture score, and this session's chaos drills — doing **zero derivation, only
  rendering**: header/census + posture line (top-3 factors) · Open concerns
  (severity-desc, each with its `next_action` hint, `[H]/[W]` only when paired, cap
  25) · What changed (the Timeline, fault line + `(you)` + `(before the failure)`,
  cap CLUSTER_CAP) · Game Day drills (omit if none) · honest footer. Empty sections
  self-omit; `posture.score==None` → UNSCANNED (never a fake 0/100 or green).
  **Drift fix (load-bearing):** the fault-line + suspect logic was lifted from the
  GUI `annals_lines` into pure core `timeline::row_decisions(tl, cap) ->
  Vec<RowDecision>`; both the on-screen Annals and the doc's "What changed" now
  consume it, so the screen and the export can't disagree. **Pure boundary:** core
  takes a `ChaosDrill` (its own mirror of the GUI's `net::ChaosRecord`, mapped at
  the boundary) — the net type never leaks into core; `now` injected (clockless).
  **Secrets:** `redact()` masks credential-shaped `key=value` / `key: value`
  (delimiter-bounded cred keys), the `Bearer`/`Authorization:` header shape, and
  URL basic-auth (authority only) in detail strings before the file is written (it
  persists to disk); the footer states other shapes may appear. **Filename:**
  `postmortem-{sanitize_context}-{YYYYMMDD-HHMMSS}.md` (path-safe for any kube
  context incl. EKS ARNs). **GUI:** `main.rs::export_postmortem` assembles the
  inputs (the SAME `build_timeline` call the Annals modal makes) + writes via the
  existing `export_to_file` (cwd, toast); triggers are the Annals modal **Export**
  button (`AnnalsAction::Export`) and **Game ▸ Export after-action report**
  (`MenuAction::ExportPostmortem`), both calling one shared helper. `--postmortem`
  dev flag + gui-smoke `postmortem`; `postmortem-*.md` gitignored. **READ-ONLY +
  one-shot** (the sanctioned file-export exception — no cross-run history, no
  append, no daemon); honest it's an in-session snapshot (recent ~window_min,
  this-session chaos). 218 core + 39 GUI tests; gui-smoke 32. Verified live on kind
  (a real report: DEFENDED 74/100, 4 concerns with hints, the timeline + fault
  line, the honest footer; a `--namespace`-scoped export adds the Scope caveat).
  The #9-named `timeline_markdown` seed is subsumed by the "What changed" section.
  **Adversarial-review hardening** (8 confirmed, all low): the URL-basic-auth
  redactor masks only within the *authority* (a `@` in a path/query no longer
  triggers it — it was dropping the path + masking a non-secret port); the
  credential-key match is delimiter-bounded (`_word`/`-word` or exact, so a prose
  word like `mysecret` isn't masked); the `key: value` colon form + the
  `Bearer`/`Authorization:` header shape are masked too (the file persists to
  disk); `oneline` neutralizes backticks (a stray one would mis-style a row) and
  the timeline title passes through it for parity; and a **Scope** line + caveat
  is emitted when a namespace filter is active (concerns/workloads/timeline are
  filtered while the node/pod census stays cluster-wide). Accepted: a same-context
  same-second re-export clobbers the prior file (documented one-shot,
  near-identical content, the toast shows the path). **Deferred** (grafts on the same boundary): a
  per-subject "postmortem for THIS workload" (build_timeline already scopes); a
  structured JSON/SARIF sibling; stronger content-based secret redaction;
  inlining logs; warm-cluster report.
- **Saturation overlay — the 4th golden signal** (2026-06-19, the roadmap
  "Next"-tier item *Saturation overlay — the 4th golden signal*; design-workflow
  vetted — 4 lenses → 3 judges → synthesis): a new **View ▸ Saturation (strain)**
  map overlay. **Pure core** `state/saturation.rs` (`saturate_node(cpu_ratio,
  mem_ratio, nonterminal_pods, alloc_pods: Option<f64>, abnormal) ->
  NodeSaturation`, unit-tested): rolls up the worst of cpu/mem (reuse the node
  ratios, bucket on the documented 0.7/0.9), **pod-count** (nonterminal scheduled
  pods ÷ `allocatable["pods"]`, tighter buckets `SAT_PODS_ELEVATED`=0.85/`HIGH`=0.95
  — the headline new signal, the silent max-pods scheduling failure cpu/mem can't
  show, computable with NO metrics-server), and the kubelet **Disk/Mem/PID-pressure
  conditions** (boolean *pegged* High dims, `SatDim.ratio: None` — the only honest
  representation of disk/PID, never a fabricated %). `NodeSaturation{dims, worst}`
  + `worst_dim` (a condition counts as effective 1.0) + `worst_level` +
  `pod_ratio`/`pod_label`. **Decision: ADD a distinct overlay, do NOT
  rename/supersede `Pressure`** — Saturation is a strict superset (worst-of-N vs
  `max(cpu,mem)`); on a cpu/mem-bound node they agree (the honest answer), and
  Saturation additionally lights up the pod-slot / condition cases Pressure stays
  green for. **Degrade-dark honesty (load-bearing):** the pod-count dim is OMITTED
  (never assume 110, no divide-by-zero) when `allocatable["pods"]` is absent/≤0;
  there is deliberately **no numeric disk/PID dimension** (no node-usage source —
  a module doc forbids adding a fabricated one). **Wiring (no new plumbing):**
  `model::node_allocatable(node, key)` generalizes the cpu/memory allocatable
  reads; `NodeTile` gains a `saturation: NodeSaturation` field (NodeSaturation
  derives Default) computed once in `build_node_tile` (nonterminal = pods_on_node
  minus Succeeded/Failed via a new `pod_terminal`), so it rides `Province.tile`
  into `draw.rs` with zero `Models`/`build_world` churn. **GUI:** `Overlay::
  Saturation` + `theme::sat_pair(SatLevel)` (Calm→idle land so a flagged province
  pops, Elevated→amber, High→red — reusing the heat palette); `panels::
  saturation_lines(&NodeSaturation)` (pure draw-decision fn, unit-tested — non-calm
  dims worst-first, "(pegged)" for conditions, "strain: calm" otherwise) shown in
  the province SELECTION/tooltip ONLY under the Saturation overlay (the
  distinguisher Pressure lacks; `region_lines`/`draw_tooltip` gained an `overlay`
  param); a province-window strain line; menu radio "Saturation (strain)";
  `--overlay saturation`; Almanac paragraph contrasting strain vs pressure;
  gui-smoke `overlay-saturation`. **Attention:** ONE new node concern (pod-slot
  exhaustion, Warning, `pod_ratio ≥ 0.95`) inserted in the existing if/else-if node
  loop after cpu/mem-high + before cordon, so NotReady/conditions/cpu-mem outrank
  it and `covered_nodes` keeps it one-per-node; reuses the `n:` next_action. No
  Disk/Mem/PID concern (already covered by the `abnormal` arm). READ-ONLY; no new
  write verb. 234 core + 41 GUI tests; gui-smoke 33. **Verified live on kind** with
  the definitive distinctness proof: 100 pause pods (requesting nothing) pinned to
  a worker pushed it to 105/110 pods — the node reads **healthy + cpu 2%/mem 6%**
  yet **`strain: high · pods 105/110`**, its province tints **RED under Saturation
  but GREEN under Pressure**, and the pod-slot concern fires. **Adversarial-review
  hardening** (3 confirmed, all low): `worst_dim` now picks among the dims AT the
  worst *level* (tie-broken by ratio) so it can never name a lower-level dim than
  `worst_level`/the tint — the pod buckets (0.85/0.95) being tighter than cpu/mem's
  (0.7/0.9) made a raw max-by-ratio able to name an Elevated pod dim on a High
  province; the city SELECTION/tooltip now also shows the host node's strain under
  the overlay (it was only on bare province land — the distinguisher was lost on
  the settlement, `Region::City(p, c)` carries the province); and regression tests
  pin both worst_dim agreement and that a cpu-AND-pod-bound node surfaces the cpu
  headline (subordination by if/else order). **Deferred** (the
  struct/`Option<f64>` are shaped for these): an Advisors ▸ Saturation tab
  (`SaturationReport`); a numeric disk/PID dim via a future kubelet Summary-API
  reader; a pod-count history ring + trend (leading-indicator) coloring; per-pod
  saturation; folding Pressure into Saturation (kept distinct so the new dims are
  visibly additive).
- **Oracle (BYO-LLM Wonder) — P0 plumbing** (2026-06-19, the roadmap "Local-LLM
  Explain" reframed as the Civ-style **Oracle of KuberNation**; full plan +
  backlog in `docs/oracle-plan.md`, design-workflow vetted — 5 lenses → 3 judges →
  synthesis). P0 ships the **publishing-safe pipeline only — no GUI/user surface
  yet** (it rolls into P1's version bump). **This is the project's FIRST general
  outbound network egress** — a deliberate posture expansion, gated like
  `portforward.rs` (active-but-non-mutating, beside the one write file, NOT in it).
  **Pure core `state/oracle.rs`** (no UI/kube-client/HTTP, unit-tested) assembles a
  structured `ContextBundle` from the EXISTING redacted view models (attention/
  diagnose/blast/rollout/harden/posture/advisor/saturation/slo) for 4 scopes
  (Concern/Workload/Node/Realm) — **never raw API dumps** — then redacts, fences,
  budgets, renders, and produces a **byte-identical consent preview** (the same
  `chat_request`/`request_json` the client POSTs). **Impure `k8s/oracle_client.rs`**
  (feature `oracle`) is the ONLY networked file: one non-streaming POST to an
  OpenAI-compatible `/v1/chat/completions` under a single 60s timeout + an 8 MiB
  body cap; classified `LlmError`; token-redacting `Debug`; installs the `ring`
  rustls provider once to match kube. **Dependency decision (O-DEP-0):** reuse the
  hyper + hyper-rustls(ring) stack kube already pulls — **zero new crates** — vs
  reqwest's measured **+40** (QUIC/ICU/wasm). The `oracle` feature is OFF by
  default so `make smoke` (core example) never links HTTP; the `kubernation` bin
  enables it (egress stays opt-in at RUNTIME via config). **Config (no
  persistence):** endpoint/model via `--llm-url`/`--llm-model`, token via
  `KUBERNATION_LLM_TOKEN` env ONLY — never written to disk, never logged. **Safety
  rails (load-bearing):** egress = publishing → redaction runs UNCONDITIONALLY
  (reuses the now-`pub(crate)` `postmortem::redact`, made **multi-line/tab/JSON
  robust** so a credential on its own log line / in `key=value`/`key:value`/Bearer/
  URL-basic-auth is masked) over every bundle string INCLUDING the framing
  (titles/scope-label/cluster rendered outside the fence); untrusted cluster
  content is **fenced** with the sentinel stripped to a **fixed point** (a single
  pass is forgeable via split-token reconstitution); the model NEVER acts (P0 is
  explain-only; suggest-to-gate is a later phase through the existing
  dry-run/commit gate). **Adversarial review (10 confirmed, all fixed):** the
  CRITICAL was exactly the multi-line redaction miss (the scrubber assumed
  `oneline()`'d input); the HIGH was the forgeable single-pass fence; plus the
  outside-the-fence framing, an unbounded body read, and LOWs. 251 core (+17
  oracle) + 41 GUI tests; lock unchanged at 250 crates; clippy clean with AND
  without the feature. **Next:** P1 (local explain-only GUI Wonder), then P2
  (remote releasable w/ egress consent), P3 (suggest-to-gate).
- **Oracle — P1 (local, explain-only GUI Wonder)** (2026-06-19, v0.50.0 — the
  first user-facing Oracle phase): the consult modal `gui/oracle.rs` (`OracleView`,
  on `window.rs`, mirroring the `charter` modal + its net-slot pattern). Pick a
  SCOPE (realm always; + the selected workload/node; + the focused concern,
  captured at open — hot-only like the advisors), see the mandatory **Preview**
  (the byte-identical `consent_preview`), **Consult** (builds the bundle from the
  snapshot → renders the prompt → hashes → `net.request_oracle`), read the reply
  with a "model-generated — verify before acting" disclaimer. **net.rs:** the
  launch config (resolved once from `--llm-url`/`--llm-model` + the
  `KUBERNATION_LLM_TOKEN` env var — never disk, never logged), a one-shot
  `oracle_req` drained ONCE and **spawned** (a ~60s consult must not block the
  world loop), an `oracle_out` cache keyed by `bundle_hash`, an `oracle_gen`
  bumped on context switch (a late reply lands nowhere). **Local-only in P1:**
  both the GUI Consult guard AND the net drain refuse a non-localhost endpoint
  (P2 adds the remote egress-consent gate). The Oracle is added to every
  charter-style modal site in `main.rs` (suspend/Esc/menu_live/just_opened/wheel/
  draw); a new **Oracle** menu between Advisors and World (shifts the `--menu`
  index map). Dev flags `--oracle [scope]` / `--oracle-ask` (stop at preview) /
  `--oracle-go` (auto-consult). **Shipped without free-text questions** (default
  per-scope question — sidesteps the keyboard-ownership minefield; custom
  questions + conversation are deferred to a follow-up / P4). Pure draw-decision
  fns (`endpoint_kind`, `resolve_config`, `oracle_setup_lines`, `wrap`)
  unit-tested. **Adversarial review (5 confirmed, all fixed):** the HIGH was
  `endpoint_kind` classifying a host by a raw `starts_with` prefix — bypassable
  (`localhost.evil.com`, `127.0.0.1.evil.com`, `localhost@evil.com` read "local"
  and would leak the bundle + token off-box); now it parses the real host (drops
  userinfo, cuts at `/?#`, strips the port/`[ipv6]`, exact case-insensitive match,
  127.0.0.0/8 only as a true dotted-quad), fail-closed, pinned by the bypass
  cases. 251 core + 45 GUI tests; gui-smoke 35. **Verified live** against a real
  local Ollama (the OpenAI shape round-trips; the in-app `--oracle realm
  --oracle-go` fired the consult end-to-end — endpoint localhost, token shown as
  "none" never a value). Next: P2 (remote releasable w/ egress consent + the
  byte-frozen consent), P3 (suggest-to-gate).
- **Oracle — P2 (remote releasable, egress consent)** (2026-06-19, v0.51.0): a
  REMOTE OpenAI-compatible endpoint (OpenRouter/vLLM/Anthropic-shim/…) is now
  usable, but because it *publishes* data off the laptop it is gated. **Arm gate
  (net.rs `oracle_egress_armed`):** a non-local endpoint is OFF by default — the
  GUI action button becomes "Arm remote egress…" behind a red explainer, and only
  the deliberate per-session arm flips it to "Consult"; the net drain ALSO refuses
  Remote-while-unarmed (P1's blanket refusal removed; defense-in-depth behind the
  GUI gate). **Byte-frozen consent (`gui/oracle.rs` `Frozen` + the pure
  `freeze`):** clicking Preview snapshots the rendered payload (hash + messages +
  preview text), and a remote Consult sends ONLY that frozen snapshot via
  `dispatch` — what the operator reviewed IS what is published, even if the live
  world ticks between viewing and clicking (also fixes the P1 preview-drift
  deferral); a remote Consult with no frozen preview forces a Preview instead of
  sending blind. **One-shot egress audit (`write_egress_audit`/pure
  `egress_audit_content`):** each real remote send writes a metadata-only
  `oracle-egress-{ts}.txt` (when/endpoint/model/scope/bytes/redacted-count — NEVER
  the prompt, reply, or token; unit-tested for the no-token invariant; gitignored)
  — the sanctioned one-shot export. Token stays env-only; local consults
  unchanged + need no arm. Dev flag `--oracle-arm`; gui-smoke `oracle-remote` +
  `oracle-remote-armed`. **Adversarial review (5 confirmed → 2 distinct, fixed):**
  the audit was written in `dispatch` before the net's cache check, so returning
  to a previously-consulted scope (cache cleared only on context switch, not scope
  change) over-recorded an "egress" the net actually served from cache with NO
  POST — now the audit is gated on `net.oracle_reply(hash).is_none()` (a real send
  only); plus a fmt-clean fix. 251 core + 46 GUI tests; gui-smoke 37. Verified live
  (the armed preview renders the byte-exact JSON — system prompt + fenced
  `<<<KN-UNTRUSTED…>>>` cluster data + the question). Next: P3 (suggest-to-gate —
  the model PROPOSES a validated Intervention staged through the dry-run/commit
  gate).
- **Oracle — P3 (suggest-to-gate, the marquee — COMPLETES the Wonder)** (2026-06-20,
  v0.52.0): the model may PROPOSE a change; it is validated against the live store
  and offered as a **Stage** button; staging enters the planning turn and is
  committed ONLY through the existing dry-run → RBAC → `commit_interventions` gate.
  **The model never executes — it proposes; the operator + the gate dispose.**
  **Pure core `state/oracle_suggest.rs`** (kept SEPARATE so the boundary is
  visible + independently testable): `SuggestionJson` is a flat, stringly mirror
  of an intervention — **model output NEVER deserializes into `Intervention`**;
  serde only builds `SuggestionJson`, and the lone `validate(&SuggestionJson,
  &ObservedWorld) -> Result<Intervention, RejectReason>` produces a real
  intervention ONLY after re-resolving every target against the live store:
  parse_kind, non-empty ns/name, reject `chaos::ns_protected` ns, reject if the
  workload object isn't in the store (`workload_exists` — existence is the OBJECT,
  not a resolvable template, since scale/restart/cordon/rollback don't need the
  template), DaemonSet scale → `NotScalable`, replicas 0..=1000, set-image
  container must exist in `spec.containers`, rollback Deployment-only + the
  revision must exist (`rollout::revisions`), cordon node must exist + not
  `chaos::node_protected`. `parse_suggestions` is tolerant (fenced ```json or
  first-{..last-}; never panics) — the model returns prose + an optional block. A
  `_verb_drift_guard` exhaustive match fails to compile if a 6th `Intervention`
  verb is added (forcing a validator/schema update). **Prompt:** `render_prompt`
  appends `SUGGEST_INSTRUCTION` (the compact schema; the single field-name source);
  the consent-preview byte-identity holds (same `render_prompt`). **GUI:** a reply
  is parsed + `validate_envelope`'d against the snapshot once when it lands →
  `suggestions` (stage-able) + `rejects` (shown with reasons); each accepted one
  gets a [Stage] button → `OracleAction::Stage(Intervention)` → `main` stages into
  `PlannedWorld` (+ a toast) → the unchanged End-of-Turn commit gate. Dev flag
  `--oracle-suggest` synthesizes a deterministic suggestion through the SAME
  validator; gui-smoke `oracle-suggest`. **Deferred:** the in-diff "oracle —
  verify" provenance tag (the plan's O-SUGGEST-PROVENANCE — `PlanChange.target` is
  a built string, brittle to match; provenance is at the staging toast instead).
  **Adversarial review: 0 confirmed** (never-execute path, validation
  completeness, parse robustness all clean). 257 core (+6 oracle_suggest) + 46 GUI
  tests; gui-smoke 38. Verified live: `--oracle-suggest` renders a restart
  suggestion with a Stage button; a kube-system target is rejected with its
  reason. **This completes the Oracle of KuberNation (P0–P3).**
- **Oracle endpoint profiles + model picker (the FIRST persisted config + first
  on-disk secret)** (2026-06-20, v0.53.0, user "enrich the Oracle to choose between
  installed local models as well as configure to use an external model … corporate
  environment where frontier models are hosted in a company-policy-compliant way";
  design-workflow vetted — 4 lenses → 3 judges → synthesis): a **Settings** face on
  the Oracle modal manages named **endpoint profiles** (a local Ollama, remote /
  corporate endpoints) and switches between them in-app. **User decisions** (asked
  up front): in-app masked token field **+** env still works; **persist everything
  incl. the token** to disk; **multiple named profiles**. **Core
  (`state/oracle_config.rs`, PURE + unit-tested, feature-gated with `oracle`):**
  `Profile{name,base_url,model,token}` (manual **redacting Debug**;
  `#[serde(skip_serializing_if)]` so a tokenless profile omits the key) +
  `OracleConfigFile{version,profiles,active}` + `resolve_active(file, flag_url,
  flag_model, env_token) -> (LlmConfig, ActiveSource)` — **precedence: flags
  (transient, never persisted; a flag URL takes env-token only — a saved token is
  NEVER sent to a CLI-typed URL) > active profile (its saved token wins, env fills
  a None) > built-in default**. `DEFAULT_LLM_URL`/`DEFAULT_LLM_MODEL` + `endpoint_kind`
  moved here from the GUI; the **load-bearing egress classifier `host_is_local` +
  `is_loopback_v4` (+ the bypass suite) moved to always-compiled `state/oracle.rs`**
  so it's tested even without the feature. `parse_models` (OpenAI/Ollama
  `{data:[{id}]}`) + `oracle_client::list_models` (`GET /v1/models`) feed the
  picker. **`bundle_hash` now folds `base_url`** (two remote profiles sharing a
  model id no longer collide → no wrong-cached-reply / suppressed-egress-audit).
  **Persistence (`gui/oracle_config_io.rs`, the only disk-touching file):** the
  FIRST config file — `~/.config/kubernation/oracle.json`, dir `0700` + file
  `0600` set **AT CREATE** (no world-readable TOCTOU window), atomic temp+fsync+
  rename, a corrupt file renamed aside (never deleted — a recoverable token may be
  inside) → degrade to default, never panics. **Token safety (every prior
  invariant re-verified end-to-end):** the token's only wire path stays the
  `Authorization: Bearer` header — structurally absent from `ChatRequest` ⇒ absent
  from the preview / `bundle_hash` / Frozen / egress audit; the input handler makes
  **zero tracing calls**; the on-disk token is **plaintext-by-explicit-opt-in**,
  disclosed honestly in the Settings UI (steers high-sensitivity tokens to the
  env var). **Egress gate unchanged + hardened:** a non-loopback endpoint stays
  behind the per-session **arm**; `net.set_oracle_config` **re-disarms + drops
  cached replies + bumps `oracle_gen`/`models_gen` on every endpoint change** (A's
  consent never carries to B); the Local/Remote class is **recomputed from the URL
  on every load/switch, never persisted/trusted**; remote model discovery
  (token-bearing egress) is **gated on the arm** in both the GUI and the net drain.
  redaction + fencing stay **unconditional** even for a "trusted corporate"
  endpoint (it relaxes privacy only). **Text entry (`gui/textfield.rs`):** one
  reusable masked `TextField` (paste via `clipboard_get` else `pbpaste`/`wl-paste`,
  draining the stray Cmd+V `v`; flush-on-focus; bullets keep length, never reveal);
  the `main` `typing` gate ORs in field focus so typed keys aren't shortcuts; Esc
  defocuses before closing. **GUI (`gui/oracle.rs`):** a second `OracleFace`
  (Consult/Settings) — NOT a new top-level modal — with a profile list (active
  marked, REMOTE flagged), an edit form (live Local|Remote URL badge, discover
  button + auto-discover for a local profile on select, in-line two-click delete,
  explicit Save), the pure draw-decision fns `profile_rows`/`model_picker_rows`/
  `oracle_setup_lines` unit-tested. Dev flag `--oracle-settings`; gui-smoke
  `oracle-settings`. Zero new crates (`serde_json`/`tracing` already in the lock).
  266 core + 47 GUI tests; gui-smoke 39. Verified live on kind + a real local
  Ollama: the Settings face lists the pulled models (`nomic-embed-text`,
  `qwen3.5:35b`), click-to-pick switches the model; a seeded remote "corp" profile
  loads active → "REMOTE — must be armed", token rendered as bullets, "token: on
  disk", the profile list marks active + flags REMOTE. **Adversarial-review fixes
  (8 confirmed → all fixed):** (CRITICAL) the Settings text fields were uneditable
  — the global `get_char_pressed` catch-all drain (main.rs) ran before the field's
  draw-time read and ate every typed char; gating it on `&& !typing` fixes it (and
  repairs the identical pre-existing city-image-editor bug). (HIGH ×2) the
  model-discovery `discover` button built a config from the edit-form URL+token and
  sent it while the arm was held for a DIFFERENT active endpoint — a token-exfil to
  an attacker URL; remote discovery is now scoped to the **active, armed** endpoint
  only (probes `net.oracle_config()`, never the edit-form URL) in BOTH the GUI and
  the net drain (which refuses a remote cfg whose base_url ≠ the active armed one).
  (MEDIUM ×4) `set_oracle_config` re-disarms + clears cached replies on a full
  **credential-identity** change (url+token+model), not just the URL (two corp
  profiles at the same URL with different tokens now re-arm); the atomic temp write
  uses **`create_new`/O_EXCL** (+ removes a stale temp) so a planted symlink can't
  redirect the plaintext token; selecting a non-local profile **clears** the stale
  model list; a remote discovery **writes an egress audit** like a consult.
  **Deferred:** OS-keychain token storage (noted, not built — plaintext+0600 is the
  honest current state); per-profile model-discovery refresh-on-switch for
  non-active profiles; a config schema migration beyond v1 best-effort load.
  **Follow-up — "Test connection" (v0.53.1):** the Settings edit form's **test**
  button (was "discover") probes the endpoint and shows a one-line verdict via the
  pure `connection_verdict(models_out, model)` (unit-tested) — reachable + token
  accepted (401 → "FAILED") + the configured model actually available (in the
  `/v1/models` list) vs "NOT available — pull it or pick one below". It reuses the
  SAME `request_models` path (no new egress surface): local tests on select,
  remote tests behind the active-armed gate + the discovery egress audit; the
  listed models double as the click-to-pick list. A `testing` flag shows
  "testing…" until the probe lands. Deferred here: a deeper end-to-end test that
  sends a tiny chat completion (heavier — a real token spend / a slow 35B cold
  load — the `/v1/models` probe catches the common URL/auth/model-pulled failures).
  **Follow-up — two-level tests + per-profile timeout (v0.53.2):** the deferred
  level-2 test shipped — a **chat** button beside **test** runs a real tiny
  completion (`oracle::chat_test_messages` "Reply with exactly: OK") proving the
  model GENERATES; the pure `chat_verdict` shows the reply snippet (model output,
  never the token). Both tests share ONE egress gate — the new
  `resolve_test_target` (local probed directly; remote only when active+armed,
  probing the active config + writing `write_test_audit`) — so the chat path can't
  diverge from the just-hardened discover path. Net `chat_test_req/out/gen` slots
  mirror the models drain (independent remote-active-armed refusal); cleared on
  load-edit / endpoint change / context switch. A **per-profile timeout**
  (`Profile.timeout_secs` → `LlmConfig.timeout_secs`, threaded through
  consult/probe/list_models, **clamped to [5,600] at use** in `LlmConfig::timeout`
  so even a tampered `oracle.json` value is bounded; blank/invalid field ⇒ default
  180) replaces the hardcoded const; a `timeout` form field (FieldId::Timeout, in
  the Tab cycle). Pure `chat_verdict` + the timeout-resolution path unit-tested;
  verified live (the level-2 prompt returns "OK" in 13s on qwen3.5:35b). Deferred:
  a global default-timeout flag (per-profile covers the need).
- **Oracle "deepen" follow-up drill-down** (2026-06-22, v0.54.0, user: the model
  kept saying "review the logs / the PVC" — data the app holds but withheld;
  chose the FULL menu over freeform entry; design-workflow vetted — 4 lenses → 3
  judges → synthesis): the consult folds that already-held, app-curated context in
  instead. **Root-cause fix:** the consult passed `log_body: None` even though the
  pipeline supports a fenced RECENT LOGS section and the `Concern` carries a
  `LogProbe` — so for a crash/error concern the offending pod's **logs are now
  included by default** (the model reasons over the lines, doesn't ask for them).
  **DeepenLens** {Logs, Storage, Blast, Rollout, WidenNode} with ONE pure source
  of truth `available_lenses(world, scope)` (data-gated) feeding BOTH the prompt's
  offered keys AND the GUI chips (no drift) + `default_lenses` (Logs only for a
  Concern with `probe.is_some()` — the default-on-hang guard). **The model curates
  NOTHING** — it may only *rank* the offered keys (`deepen_instruction` +
  tolerant `parse_follow_up`, INTERSECTED with offered so an injected key is a
  no-op); the app decides what each lens fetches. **Pure/async split** (the
  spine): blast/rollout/storage/widen-node are synchronous reads over the
  snapshot; only LOGS cross the net boundary via a dedicated one-shot
  `oracle_log_req/out/gen` slot (mirrors `models`; gen-guarded + request-matched +
  torn down on context-switch/endpoint-change so a slow fetch can't fold the OLD
  cluster's logs into a published bundle — the highest-severity risk). **Every
  lens routes through `push_deepen_sections` → `sec()` → BundleSection**, so
  redaction + fencing + the token budget apply uniformly (no text appended after
  `render_prompt`). **Budget honesty:** an explicitly-clicked lens is promoted to
  `PRIORITY_DEEPEN`=7 (below the primary 9 — no inversion) + a roomier
  `Caps::deepened`; a dropped requested lens is recorded (`dropped_requested`) and
  the pure `deepen_chip_states` derives each chip's state (Included/Available/
  Fetching/Dropped) from the ACTUAL bundle, so a chip can never falsely claim
  "included". **Remote re-consent:** any payload-changing deepen clears the frozen
  consent (`apply_deepen_change`) and a remote deepen re-Previews the enriched
  payload + writes a fresh egress audit (the P2 byte-frozen-consent invariant
  holds for the bigger payload). **GUI:** the INVESTIGATE FURTHER chip row after
  the reply; a logs deepen shows "gathering logs…" then re-consults (local) /
  re-Previews (remote) once the fetch lands; button-only (no freeform). Pure parts
  unit-tested (lens gating, default-on-probe, sections, budget-survival,
  chip-states, parse_follow_up-intersect, button-order); dev `--oracle-deepen
  <lens>` + gui-smoke `oracle-deepen`. 274 core + 49 GUI tests; gui-smoke 40.
  **Verified live on kind + qwen3:30b:** the crashy concern consult reads "The
  only log entry is 'boom'" (it HAS the logs) with chips "v include logs:
  included" · rollout history · widen to node, and the model returned a
  `follow_up:["logs"]` ranking block. **Adversarial review (2 confirmed, both
  fixed; cross-cluster-log-leak invariant confirmed HOLDS):** (MEDIUM) switching
  the active endpoint in the Settings face — reachable without closing the modal —
  bumped the net `oracle_log_gen`, orphaning an in-flight deepen-log fetch into a
  permanent "gathering logs" spinner that blocked the deferred consult;
  `apply_active` now tears down the deepen async state (`clear_oracle_log` +
  reset pending/log/want_consult + re-seed). (LOW) a *failed* log fetch left Logs
  active-but-absent, so the chip mislabeled as "dropped to fit — narrow scope" (a
  budget message); a fetch error now drops the Logs lens so the chip reverts to a
  clickable "include logs" (retry). **Deferred:** a stateful multi-turn
  conversation (each deepen is an independent enriched re-consult); per-pod
  multi-container log selection; a recursive self-review pass (the user + design
  judged it lower-ROI than adding the right DATA).
- **Oracle "investigate" → CONSULT NEXT links** (2026-06-23, v0.55.0, user: a realm
  reply's "what to investigate first" list should be clickable consult links;
  design-workflow vetted — 3 lenses → 2 judges → synthesis): the realm/node reply
  often ends with a prioritized list of OTHER objects worth a look; those become
  **clickable links that JUMP the consult to that object** — drilling realm →
  specific without leaving the modal. Distinct from the deepen "INVESTIGATE FURTHER"
  chips (which add context to the SAME scope); this jumps to a NARROWER scope.
  **Pure core `state/oracle_investigate.rs`** (mirrors `oracle_suggest`, the
  separate-boundary pattern): `InvestigateJson` is a flat stringly mirror of model
  output (kind/namespace/name/why) that **never becomes a `Scope` except through
  `validate_investigate`**, the lone path that re-resolves each target against the
  LIVE store — Workload (Deploy/STS/DS) or Node only; a bad kind / empty field /
  hallucinated name is dropped (the security boundary, exactly like
  `oracle_suggest::validate`). `validate_envelope` dedups survivors by
  `Scope::label()` (Scope has no Eq — it embeds a Clone-only Concern).
  `parse_investigate` reuses the now-`pub(crate)` `oracle::json_blocks` (the shared
  multi-fence scanner), so the three reply blocks — `suggestions` + `follow_up` +
  `investigate` — **coexist** (each parser extracts only its own top-level key).
  `INVESTIGATE_INSTRUCTION` + `investigate_instruction(offer)` splice into
  `render_prompt` AFTER the suggest + deepen blocks; **byte-identity** is preserved
  by threading a single `offer_investigate: bool` through `render_prompt` +
  `consent_preview` + `bundle_hash` + the GUI `freeze`/`dispatch` + the `Built`
  7-tuple (a divergent arg at any caller would break the P2 byte-frozen-consent
  guarantee — pinned by extending `consent_preview_faithfully_shows_everything_sent`
  to realm scope + a `render_prompt_gates_the_investigate_block` test;
  `bundle_hash` folds it so a same-scope on/off can't collide). The instruction is
  **scope-gated** to Realm | Node (the prose list lives at realm; a node reply may
  name stationed workloads) — OFF for Workload/Concern (already narrowest; naming
  siblings is noise + injection surface). **No `chaos::ns_protected` filter** —
  DELIBERATE and the opposite of `oracle_suggest`: an investigate target is a
  READ-ONLY consult `Scope` (consumed only by `build_bundle`, never `actions.rs`),
  so a "check coredns" drill into kube-system is allowed (the same read/write
  asymmetry the Charter #6 + advisors already embody; pinned by
  `protected_ns_is_not_filtered`). **GUI (`gui/oracle.rs`):** a reply's targets are
  parsed + validated once at reply-land against the snapshot; the pure
  `investigate_label` draw-decision fn (unit-tested) renders a **CONSULT NEXT** row
  (placed after the reply/suggestions/rejects, before INVESTIGATE FURTHER) only when
  ≥1 survived; the model's untrusted `why` is display-only (`ascii()`+truncate),
  never republished, never folded into the next bundle (the jump rebuilds fresh from
  the world). Clicking calls `jump_to_scope`: dedup/locate the `Scope` by label
  (Realm + originals stay in `scopes` so the ◀▶ chip returns), run the shared
  `reset_for_scope_switch` (clears all consult-result state + in-flight log fetch),
  re-seed deepen for the new scope, then set `want_consult` — so the EXISTING drain
  fires exactly ONE consult (local send / remote re-Preview for re-consent + a fresh
  egress audit on a real send; one consult per click, no auto-cascade). `self.map`
  `selected` is untouched. `self.investigate` is cleared at every payload-change
  site (reply-poll top, `apply_deepen_change`, scope switch, `apply_active`
  endpoint change); a late reply lands nowhere (`oracle_gen`). Dev flag
  `--oracle-investigate` (synthesizes a deterministic target through the REAL
  validator) + gui-smoke `oracle-investigate`. **Adversarial review: 0 confirmed**
  (4 dimensions — validation/security, byte-identity/egress, scope-jump/state,
  correctness/UX — all clean; the never-execute path, byte-identity threading
  across all callers, and remote re-consent invariants verified). 282 core (+8) +
  49 GUI tests; gui-smoke 41. **Deferred:** a keyboard cycle of the links;
  pvc/service targets (not consult scopes); a stateful multi-turn conversation
  across jumps.
- **Oracle reply UX polish** (2026-06-23, v0.56.0, user feedback on a live consult
  screenshot + an ideation workflow — 5 lenses → 2 judges → ranked backlog; the
  user picked the "Reply UX polish" quick-wins bundle): five reply-side fixes, the
  first being a confirmed visible bug. **(1) Machine-block strip** — the GUI
  rendered the reply with `wrap(reply)`, so the model's fenced `investigate` /
  `suggestions` / `follow_up` JSON leaked into the displayed prose even though it's
  already rendered as CONSULT NEXT links / Stage buttons / deepen chips. Pure core
  `oracle::strip_machine_blocks(reply)` is the **inverse of `json_blocks`**: it
  drops a fenced/bare `{…}` block iff its content deserializes into a known
  envelope (`is_machine_block` — investigate/suggestions/follow_up present), keeping
  legit prose + unrelated code/JSON; `self.reply` stays RAW so the parsers can't
  disagree with the display. An all-block reply strips to empty → a placeholder
  ("the answer is in the actions below"). Unit-tested (strip/keep/bare/empty).
  **(2) Elapsed + Cancel** — the static spinner became `consult_progress_line(elapsed,
  timeout)` (pure, the timeout from the now-`pub LlmConfig::timeout()`), with a
  **Cancel** action button (`net.cancel_oracle()` bumps `oracle_gen` so a late
  reply is discarded — honest that a remote send was already published). **(3)
  Copy/export** — `c`/`w` on the Consult face return `OracleAction::Copy/Export`
  (the RAW reply, so the rendered actions are reproducible; export prepends a
  scope/endpoint/model header), reusing the logs/inspector clipboard + file
  helpers + a timestamped filename; gated on `!field_focused() && !show_preview`,
  and the global `C`/`W` are already suspended while the modal is open. **(4) Error
  card** — a failed consult now sets a separate `reply_error` (not `self.reply`),
  rendered as a WARN card with a one-line hint from the pure `error_hint(msg)`
  (auth/timeout/model-not-pulled/unreachable) + a **Retry** (re-dispatch). **(5)
  Sticky disclaimer** — "model-generated; verify" pinned as a footer OUTSIDE the
  scroll (reserve 22px from the `Ctx` body), via pure `disclaimer_text(has_suggestions)`
  (sharpened + WARN-tinted when a Stage button is shown). The three new GUI pure
  fns + `strip_machine_blocks` are unit-tested (the testability policy).
  **Adversarial review (6 confirmed, all low/medium, all fixed):** the fence
  scanner was rebuilt around a `find_backtick_run` (≥3 backticks, run-length-matched
  close) so single-line ```` ```{…}``` ```` and 4-backtick fences strip cleanly
  instead of leaving stray markers, and Pass 2 (bare-`{…}`) is **skipped on an
  unterminated fence** so a truncated reply is kept verbatim (not mangled);
  `error_hint` now matches the REAL `LlmError` Display strings ("did not respond in
  time" / "rejected the API token" / "could not reach" / a 404 "model … not found"
  / "(429)") — the old "timed out" arm never fired — with the test asserting the
  real strings; `reply_landed()` counts an error as landed (the `--oracle-go` shot
  no longer waits out the 210s backstop on a failed consult); `cancel_oracle(hash)`
  drops only that hash's cache entry (not the whole cache — protecting the
  remote-egress economy of other scopes' cached replies); and the pinned footer
  shows the "model-generated; verify" caveat ONLY on a real reply (an error card
  gets a "the consult failed — Retry" note instead). 283 core (+1) + 52 GUI (+3)
  tests. Origin: the ideation backlog also surfaced higher-effort bets (per-scope
  default question, wire the diagnose explainer into the bundle, a Realm deepen
  lens, free-text question, streaming) — deferred to the user's next pick.
- **Oracle CONSULT NEXT seeded from the attention queue** (2026-06-23, v0.57.0, user:
  a realm consult clearly named a critical issue in prose but showed no CONSULT NEXT
  link): the "prose-richer-than-links" gap the investigate-links decision flagged as
  an accepted risk became a real miss — a small local model (qwen3:30b) often
  describes the trouble in prose without emitting the structured `investigate` block,
  so a realm consult with a critical concern produced zero drill-down links. **Fix:
  the app seeds CONSULT NEXT from its OWN attention queue, the model only adds.**
  Pure core `oracle_investigate::concern_targets(&[Concern], cap)` maps the
  (already-severity-ordered) attention concerns → `InvestigateTarget`s
  (Workload/Node `Target`s → `Scope`; `WorkloadList` skipped; hot-only; deduped by
  label; capped at `CONSULT_NEXT_CAP`=5; the `why` is the concern *title* — trusted
  app text, never model output). GUI `OracleView::merge_consult_next` makes the
  concern targets the **floor at Realm scope** (so a clear concern always yields a
  link), then appends the model's validated block targets the queue didn't already
  flag (deduped, capped) — at node scope the model block still stands alone. So the
  realm CONSULT NEXT can never be empty when the attention queue isn't; the model's
  block can only ADD targets or reorder its own extras, never bury the critical one.
  No prompt change (the `investigate` instruction stays — the model can still name
  off-queue suspects); the demo (`--oracle-investigate`) path merges the same way.
  Honest tradeoff: the seeded concerns reflect the active namespace filter (the
  attention queue is filtered), matching the rest of the app. Core test
  (`concern_targets_maps_dedups_skips_and_caps`: map/dedup/skip-list/hot-only/cap).
  284 core + 52 GUI tests. **Deferred:** node-scope concern seeding (the node's own
  concerns + stationed workloads — needs topology); a visual model-vs-app provenance
  cue on the links.
- **Oracle answer quality — per-scope question + diagnosis fold + node seeding**
  (2026-06-23, v0.58.0, user "address the deferred seeding for node + the sharper
  answers improvement"; design-workflow vetted — 3 lenses → 2 judges → synthesis):
  three pieces sharpening a consult, all pure-core-first. **(1) Per-scope default
  question.** `oracle::default_question` returned ONE generic string; now a pure
  `default_question_for(scope, models)` produces a POINTED prompt naming the trouble
  (Concern → "Why is '{title}' happening…"; Workload → "Why is {ns/name} unhealthy…"
  vs "Is {ns/name} healthy…", branching on the SAME `workload_severity` map the
  WORKLOAD section reports so they can't disagree; Node → "What is straining node
  {n}…"; Realm → "top 1-3 worst-first"). **Byte-identity (load-bearing):** the
  question is carried on a new `ContextBundle.default_question` field — the ONE bundle
  `render_prompt`/`consent_preview`/`bundle_hash` all consume — set RAW in
  `build_bundle` and scrubbed INSIDE `redact_bundle` (both `postmortem::redact` for
  credential shapes AND `strip_sentinels`, since it embeds cluster-derived names +
  renders OUTSIDE the fence); `default_question(bundle)` is the SOLE reader. The
  `consent_preview_faithfully` test was extended to the EMPTY-question path (the
  default must appear verbatim in BOTH preview + wire). **(2) Root-cause diagnosis
  into the bundle.** A panic-safe `worst_pod_diagnosis(world, wr)` (an Option chain
  over `build_city` pods → the worst `.diag` by `diag_rank`; `None` on healthy /
  zero-pod / not-in-store) folds a pri-8 `SectionTag::Diagnosis` (the already-computed
  why + fix) into `workload_sections` AND `concern_sections` (Target::Workload) via
  `sec()` — so it rides redaction/fence/budget; pri-8 drops before the pri-9 primary
  (the question's subject always survives) — pinned by a budget-inversion test. The
  model now reasons over the CrashLoopBackOff root cause instead of re-guessing it.
  **(3) Node CONSULT NEXT seeding.** `blast.rs`'s node→workloads loop was extracted
  to `pub(crate) workloads_on_node` (so blast highlighting + node seeding can't
  disagree on "who lives here" — the `affected_cell` DRY precedent);
  `oracle_investigate::concern_targets_on_node` filters the attention queue to the
  troubled workloads STATIONED on that node (+ node-targeting concerns); the GUI
  `merge_consult_next` gained a `world` param + a Realm/Node match, updated at BOTH
  call sites (the reply-poll + the demo). An off-node workload concern is EXCLUDED
  (pinned — else node seeding == realm seeding); hot-only, deduped, capped; stays
  POST-REPLY (never enters the bundle path → byte-identity intact). **Realm deepen
  lens: DEFERRED** (`available_lenses(Realm)` stays empty) — superseded by the realm
  CONSULT NEXT seeding (which already gives the realm reply actionable drill-down
  links to the worst concern's full, now-diagnosis-bearing workload bundle); a new
  `DeepenLens` variant would ripple through ~8 touchpoints for marginal, redundant
  value. If ever revived, prefer NAVIGATION (the jump) over an inline fold. No new
  egress/write verb. 291 core (+7) + 52 GUI tests; clippy clean with + without the
  `oracle` feature. **Adversarial review: 0 confirmed** (3 dimensions —
  byte-identity/redaction, panic/budget, node-seeding topology — all clean; the
  default-question-is-redacted, worst-pod-diagnosis-can't-panic, pri-8-drops-first,
  and off-node-excluded invariants verified). **Deferred:** the realm lens (above);
  per-container log selection in the diagnosis; a free-text question (the
  keyboard-ownership minefield).
- **Oracle streaming (token-by-token replies)** (2026-06-23, v0.59.0, user "let's get
  streaming replies" — the last "bigger bet" from the ideation backlog; design-workflow
  vetted — 3 lenses → 2 judges → synthesis): a consult now POSTs `stream:true` (SSE) and
  the GUI renders the reply as it generates instead of a 60–90s block-then-dump. **Pure
  core SSE decode** (`state/oracle.rs`, always-compiled, the one piece with real parsing
  logic): `SseDecoder` (a `buf` holds the trailing partial line across hyper frames — the
  subtle correctness point) `.push(chunk) -> SseEvents { deltas, done }` extracting
  `choices[0].delta.content`, `[DONE]`-terminated, ignoring keep-alive `:` comments;
  `chunk_delta` never panics on malformed/partial JSON. 8 unit tests (incl. a JSON split
  across THREE frames + malformed non-fatal). **Impure client** (`k8s/oracle_client.rs`,
  feature `oracle`): `consult_stream(cfg, messages, on_token, is_cancelled)` reads
  `BodyExt::frame()` incrementally, feeds the decoder, calls `on_token` per delta, returns
  the full text; a **non-SSE Content-Type fallback** parses a single completion (an
  endpoint that ignores `stream:true` degrades to the old behavior, no second GUI path);
  **idle timeout** — the FIRST token gets the full per-profile timeout (cold-model
  startup), then `STREAM_IDLE_SECS`=30 between frames (a long-but-steady generation isn't
  cut); manual `MAX_RESP_BYTES` cap across frames; every failure maps to an existing
  `LlmError`. `consult()` stays (chat-test + the fallback) with `stream:false`. **Net slot**
  (`net.rs`): a `StreamBuf{text,status: Streaming|Done|Err}` per hash in `oracle_stream`
  (parallel to the durable `oracle_out` final cache); the drain pre-inserts Streaming,
  spawns `consult_stream` with `on_token` appending to the buf + `is_cancelled = gen !=
  req_gen`, and on the gen-gated terminal edge writes the status + the final reply (KEEPING
  the partial on Err); `oracle_stream` is cleared at all **3** `oracle_gen`-bump teardown
  sites (endpoint change / cancel / context switch) so an old-world partial can't paint
  into the new world. **GUI** (`gui/oracle.rs`): the pending-poll reads the live buf each
  frame (Streaming → show the growing `strip_machine_blocks(partial)` + the pure
  `stream_status_line`; Done → `finalize_reply` ONCE — the heavy parse/validate/merge
  pipeline runs only on the terminal edge, never per 60fps frame, so a half-streamed JSON
  block never becomes a premature Stage button; Err-with-partial → keep the text + a
  `stream_error_note`, Cancel → drop it). **Byte-identity (load-bearing):** `chat_request`
  gained a `stream: bool`; `consent_preview`'s literal, `bundle_hash`, and `freeze.wire_bytes`
  all reflect `stream:true` for a consult while the chat-test stays false — the operator
  reviews exactly what is sent (the request body, not how the response streams). The frozen
  consent + one-shot remote egress audit fire ONCE at stream start over the `stream:true`
  payload; redaction stays unconditional + inbound-untouched (a chunk is `delta.content`
  model output, no token — the token's only path is the Authorization header). Regression
  tests: `consent_preview_faithfully` asserts `stream: true`; `chat_request_stream_flag`
  pins the serialized flag. **Adversarial review (4 confirmed, all fixed):** (HIGH) a
  per-frame `from_utf8_lossy` corrupted a multi-byte char (em-dash / CJK / emoji) split
  on a frame boundary into U+FFFD — now the loop buffers raw BYTES and decodes only the
  valid UTF-8 prefix via the pure, unit-tested `oracle::take_utf8_prefix` (the trailing
  partial char waits for the next frame; pinned by an em-dash-split test); (MED) the
  CONSULT NEXT links + INVESTIGATE FURTHER chips (top-level blocks outside the render
  chain) painted + stayed clickable over a half-streamed re-consult — now gated on
  `pending.is_none()` so the prior reply's actions vanish the instant a (re-)consult
  starts and return only after `finalize_reply`; (MED) the outer total `timeout` was cut
  — removed so a long-but-steady stream isn't killed (the idle-between-frames bound is
  the only time bound, matching the doc); (MED) the non-SSE fallback + error-body reads
  had no timeout — both wrapped in `cfg.timeout()`. 294 core (+3) + 53 GUI tests; clippy
  clean with + without the feature. **Deferred:** a per-pod multi-container picker; a
  configurable stream idle bound; a manual prompt field (the keyboard-ownership minefield).
- **Oracle reply carousel (session history)** (2026-06-23, v0.60.0, user: drilling into
  the next reply wiped the view, but you want to ponder the prior reply while the next
  slowly streams; design-workflow vetted — 2 lenses → synthesis; carousel chosen over a
  stacked transcript because the modal is fixed-size with ONE scroll region): a
  session-local pager of consult replies, all GUI-loop state (`gui/oracle.rs`, no core/
  net/egress change). **State:** `history: Vec<ArchivedReply{scope_label,text}>` (past
  replies, oldest first; the current reply stays in `self.reply` — the carousel's implicit
  last page), `view_offset` (0 = current/live page, k = k back), `reply_scope` (the current
  reply's scope label, FROZEN in `finalize_reply` because a jump moves `scope_idx` BEFORE
  finalize). `archive_current()` pushes the prior FINALIZED reply into history before it's
  wiped — guarded on non-empty + `pending.is_none()` + `stream_error_note.is_none()` (NEVER
  a live streaming partial or a truncated stream-error partial — they'd be a misleading
  "past reply") + last-text dedup + `ORACLE_HISTORY_CAP`=10; resets `view_offset=0`
  (auto-advance so the new streaming consult is visible). Called first in
  `reset_for_scope_switch` + `apply_deepen_change` + the Consult/Retry dispatch arm.
  **READ-ONLY past pages (the safety crux):** at `view_offset>0` the body renders the
  archived page's PROSE ONLY (`strip_machine_blocks`) + a read-only hint, and the CONSULT
  NEXT / INVESTIGATE FURTHER / Stage action blocks are gated `&& view_offset==0` — so a
  stale suggestion/link (validated against a since-changed cluster) can NEVER be acted on;
  only the live page is actionable. The stored text is already-redacted display-only model
  output, so the carousel adds **no egress, no write, no token surface**; the `◀ ▶` pager
  is inert to the consult/network. **Render:** the reply-zone if/else chain was wrapped in
  a `view_offset==0 ? live-chain : past-prose` split with a `◀ reply N/M ▶` pager row (pure
  `pager_label`, unit-tested) drawn via `cx` so the scroll math holds; `◀ ▶` clamp the
  offset + reset scroll (pages differ in length). `c`/`w` copy/export the VIEWED page
  (`viewed_reply`). Cleared on endpoint change (`apply_active`); a hot-context switch
  rebuilds `OracleView::new()` (history empty per cluster). **Adversarial review: 0
  confirmed** (read-only-past-page safety, the streaming/archive guards, render brace
  balance + scroll math, and clearing all verified). 294 core + 54 GUI (+1) tests; clippy
  clean. **Deferred:** persisting history across modal close; a stacked-transcript toggle;
  per-page metadata (timestamp/question).
- **Multi-burn-rate SLO alerting** (2026-06-23, v0.61.0, user picked it from the backlog;
  design-workflow vetted — 2 lenses → synthesis — then adversarially reviewed): the
  treasury's single burn threshold (`BURN_HOT=1.5`) became the SRE multiwindow burn
  pattern, classifying a *fast* burn (page → **Critical** concern) vs a *slow* burn
  (ticket → **Warning**) so the queue separates "wake someone up" from "file it." **Pure
  core** (`state/slo.rs`, unit-tested): `from_ring` now computes burn over a SHORT window
  (`BURN_SHORT=24` samples ~48s — the recent *rate*) and a LONG window (`BURN_LONG=60`
  ~2min — *sustained?*, a strict slice of the 240-sample ring so a full-ring burn doesn't
  conflate with `Breached`), gated by an ACTIVE check (`BURN_ACTIVE=4` ~8s — *down right
  now?*). `BudgetState::Burning` split into `FastBurn`/`SlowBurn`; `SloStatus` gained
  `burn_long`. Predicates: `fast = active && burn≥BURN_FAST(6) && burn_long≥BURN_SLOW(2)`;
  `slow = active && n≥BURN_LONG_MIN(24) && !fast && burn_long≥BURN_SLOW && burn≥BURN_HOT(1.5)`
  (`BURN_FAST>BURN_SLOW>BURN_HOT>1`). Precedence unchanged: `Warming → Breached → FastBurn
  → SlowBurn → Healthy`. `budget_concern` maps Breached+FastBurn→Critical, SlowBurn→Warning,
  else None (stable `slo:ns/name` key so a SlowBurn→FastBurn escalation updates in place).
  The GUI city `treasury_summary` split the Burning arm (CRIT "burning fast Nx/Mx" / WARN
  "eroding Nx/Mx"); net + chaos `budget_verdict` needed no change (they route through
  `severity`/`Breached` only). **Two load-bearing facts the adversarial review forced** (it
  found 2 distinct HIGH issues the design missed): (1) the LONG window is **geometrically
  bounded** — while the budget is positive a sustained long-window burst can't exceed
  ≈`WINDOW/BURN_LONG`(~4×) before breaching — so the design's original `burn_long≥FAST`
  predicate was *impossible* (FastBurn permanently dead); the long window confirms "not a
  blip" at the SLOW threshold instead. (2) the SHORT window's **integer 1/w quantization**
  at a tight budget jumps the burn past the slow band (over 8 samples at a 1% budget one
  down = 12.5×, past FAST), making `SlowBurn` **unreachable at the default 0.99 and every
  tier ≥~97.9%** — the ticket tier was dead for the default operator. Fixes: widen
  `BURN_SHORT` 8→24 (one down sample lands at 4.17×, inside the slow band → SlowBurn
  reachable at 0.99, pinned by `slow_burn_tickets_at_the_default_target`), and add the
  ACTIVE gate so the wide short window doesn't false-page during its ~48s drain after a
  recovered dip (the review's 3rd confirmed finding, pinned by `a_partial_recovery_does_not_page`).
  **Honest physics** (in the module doc + almanac): the page/ticket split sharpens at
  looser targets (more budget = more dynamic range); at very tight targets (≳99.5%) the
  8-min/2s ring is too coarse for a sub-breach distinction (one 2s down already breaches) →
  page-or-breach there, by design not omission. Still in-session + readiness-derived (no
  metrics-server); no new write verb. 299 core + 54 GUI tests (the blip + recovered +
  partial-recovery no-alert invariants pin the gates' end-to-end chain — state AND
  `budget_concern.is_none()`); verified live on kind (smoke). **Deferred:** a configurable
  multiwindow tuple; a multi-window burn-rate graph in the treasury band; persisting the
  ring across runs (would enable true longer-horizon burn windows).
- **About window** (2026-06-23, v0.62.0, user request): a Help ▸ About modal
  (`gui/about.rs`, on `window.rs`, mirroring the Almanac's window/scroll machinery)
  featuring the splash logo (`logo::draw_full`) then three sections — Credits (Jason
  Olmsted + "built in collaboration with Claude"), Third-party licenses, and the
  Trademark disclaimer (the README's verbatim "unaffiliated homage … not associated with
  Take-Two / Firaxis / Civilization" wording). The content builder `about_sections()` is
  **pure + unit-tested** (the testability policy) — the test pins the legal obligations
  (both authors, the SIL-OFL, ISC+BSD-3-Clause named, the notices pointer) so they can't
  silently drift. Wired through `menu.rs` (`MenuAction::About`) + the usual `main.rs`
  modal touchpoints (the 11 world-nav suspend gates, Esc chain, wheel, just-opened guard,
  draw-on-top, `--about` dev flag + gui-smoke state). The window is sized so **all content
  including the disclaimer fits without scrolling** on the default 1380×860 window (the
  disclaimer is the point — it shouldn't be below the fold); scroll is a fallback, and the
  logo is suppressed once scrolled above the body top (macroquad has no scissor).
  **Adversarial review (3 confirmed, all legal-accuracy):** (HIGH) the crate-license line
  claimed the binary is "MIT / Apache-2.0" but the shipped rustls/`ring` TLS stack bundles
  **ISC** (ring, rustls-webpki, untrusted), **BSD-3-Clause** (subtle), **Zlib** (foldhash),
  and **Unicode-3.0** (unicode-ident) crates — an affirmatively false notice; fixed by
  broadening the wording AND generating a complete `crates/kubernation/THIRD-PARTY-NOTICES.md`
  with every crate's actual license text via **`cargo-about`** (`about.toml` + `about.hbs`
  at the repo root regenerate it), referenced from the About + CREDITS.md, with a
  test-guard asserting the line names ISC/BSD-3-Clause. (LOW) the bundled `OFL.txt` carried
  only the Fira Sans copyright — appended the Liberation (Red Hat / digitized © Google)
  reservation so both copyright statements travel with the fonts as the OFL requires. (NIT)
  the `©` glyph → `(c)` (ASCII, no font-coverage dependency). Verified live (`--about
  --screenshot`): renders the splash + both credits + the full license spread + the
  disclaimer, no scroll, no panic. Read-only; no new write verb. **Deferred:** a CI check
  that re-runs `cargo-about` so the notice can't drift from `Cargo.lock`.
- **Branding: "KuberNation" + own copyright/trademark** (2026-06-23, v0.62.1, user
  request): the displayed brand name is the camelCase **"KuberNation"** on every
  user-facing surface — the OS window titlebar (`window_conf`), the menu bar, the
  About + almanac/help text, the README, and the crate `description`s — fixed by a
  `\bKubernation\b → KuberNation` sweep (safe because every *identifier* is lowercase
  `kubernation` / `kubernation_core`, the kind context `kind-kubernation`, the config
  dir `~/.config/kubernation`, the `kubernation.io/…` annotations, and the
  `KUBERNATION_LLM_TOKEN` env var — none match the capital-K word boundary). **Kept
  lowercase deliberately:** those identifiers/paths/URLs (incl. the `repository` URL
  and clap's `name = "kubernation"`, which is the invocation name, not the brand). The
  internal CLAUDE.md / docs prose isn't swept (historical record). We also **assert our
  own rights**: "© 2026 Jason Olmsted. KuberNation™ and the KuberNation logo are
  unregistered trademarks of Jason Olmsted" (unregistered → **™, not ®**) in the About
  window + README, the `LICENSE` copyright line set to Jason Olmsted, and a workspace
  `authors` field. The About uses the real `©`/`™` glyphs (Fira Sans covers them —
  verified on screen), reverting the earlier review's defensive `(c)` now that this is
  a formal notice. The `about_sections()` test pins the copyright + the unregistered-TM
  string so they can't drift.
- **Cost cartography — "upkeep"** (2026-06-23, v0.63.0, the roadmap "bigger bet";
  design-workflow vetted — 3 lenses → 2 judges → synthesis): visualize what the cluster
  costs to run, allocated across the map. **Pure core `state/cost.rs`** (`cost_report(world,
  rates) -> CostReport`, unit-tested) — a sibling of `advisor::rightsizing_report` reusing
  `OwnerIndex`/`node_allocatable`/`world.pod_usage`; ONE report feeds the overlay + the
  advisor tab so they can't disagree. **Metaphor: UPKEEP** (the recurring coin to *hold*
  reserved capacity — code uses literal nouns `Overlay::Cost`/`NodeCost`/`cost_report`,
  display says "upkeep"), deliberately distinct from the SLO **"treasury"** (`slo.rs`) which
  money-cost must not overload. **Honest derivation** (load-bearing — k8s has no cost API,
  the app reads no cloud billing): cost from resource **requests** (any cluster, no
  metrics-server), refined by **usage** when metrics-server is present; **unitless "cost
  units"** by default (a cpu + mem/`DEFAULT_MEM_WEIGHT` weighted footprint — NEVER a `$`, no
  `/mo`), real `$` (hourly + ×730 monthly) ONLY when the operator supplies rates
  (`--cpu-rate`/`--mem-rate` *per binary GiB*/`--node-rate NODE=USD`, or a
  `kubernation.io/cost-hourly` node annotation merged at the net boundary into
  `node_overrides`); `CostRates::currency()` gates ALL `$` presentation; even in `$` mode
  it's labelled an *estimate from your rates × reservation, not a cloud invoice*.
  **Allocation = share of CAPACITY** (not share-of-used, which would zero idle): `cap_w =
  cpu + mem_gib/weight`; a node's cost is distributed to its non-terminal pods by `pod_w/cap_w`,
  the unallocated remainder is **idle** — the actionable consolidation drain. Exact partition
  `Σ(pod_cost)+idle == node_cost` (not overcommitted; overcommit clamps idle to 0, shares
  exceed cost) — pinned by tests. **GUI:** `Overlay::Cost` bronze "spend" choropleth
  (`theme::cost_pair`, terrain-family — green kept substantial so a "dear" province never
  reads as NotReady-red; ramp = `node_cost / max_node_cost`, read from the memoized
  `WorldSnap.cost` — NO `build_map` signature churn), a **gold idle coin** on nodes with
  idle ≥ `IDLE_NOTABLE` (gold = cost family, NOT the cyan that means PVC/Service), the
  SELECTION line (`panels::cost_lines`), and the **Advisors ▸ Cost** tab (`advisor::cost_lines`
  — total/by-namespace/costliest/idle, neutral INK data rows, the idle line warns at the
  cluster `IDLE_CLUSTER_WARN`). **READ-ONLY — no new write verb**; **NO attention concern**
  (cost is a standing fact, not an incident — argued, matching right-sizing #5's advisor-only
  posture). Complements right-sizing (*which* workloads are mis-sized) with the *where-does-
  the-money-go* map. **Adversarial review (7 confirmed → folded):** (MED) the `request:=limit`
  default was applied per-pod-aggregate, undercounting a mixed request-only/limit-only pod —
  fixed by a per-container `model::sum_pod_reserved` scoped to cost (the right-sizing advisor
  keeps the *literal* request); (MED) an operator-priced node with no allocatable was dropped
  — now priced all-idle; (LOW) per-node `basis` label (a globally-Usage report's unsampled
  node now reads request-based); (LOW) `--node-rate` NaN/≤0 filter (mirrors the annotation);
  (NIT) the on-map `$` SELECTION line gained the "not a cloud bill" caveat; (MED) the idle
  coin was cyan = ambiguous with PVC/Service marks → recoloured gold; (NIT) the per-node vs
  cluster idle thresholds documented as intentional. 312 core + 58 GUI tests; verified live on
  kind in both modes (unitless "UPKEEP 79.7 units · idle 100%" on the ~empty dev cluster;
  `--cpu-rate`/`--mem-rate` → "$2.25/hr · ~$1645/mo" with the honesty subline). **Deferred:**
  OpenCost/cloud-billing integration, a real pricing table, a minimap cost choropleth, an
  RBAC-cost 3rd dimension; the ramp is degenerate on a uniform 1-node-per-zone cluster (spreads
  on real multi-node clusters).
- **OpenCost integration — invoice-grade cost via the first in-cluster HTTP-source
  substrate** (2026-06-24, v0.64.0, user "the generic Prometheus/REST poller substrate +
  the OpenCost adapter on top"; design + adversarial review): the requests-derived cost
  cartography ("upkeep", v0.63.0) gains an optional **OpenCost** backend for real,
  amortized `$` — the network / load-balancer / storage / spot & reserved-discount lines
  the requests model structurally can't see. **Reachability (load-bearing):** OpenCost is
  reached **READ-ONLY through the kube API-server service proxy** (`GET /api/v1/namespaces/
  {ns}/services/{svc}:{port}/proxy/allocation?…`) — the SAME authenticated kube connection
  as the reflectors, **no port-forward, no new off-laptop egress** (distinct from the
  Oracle's external egress), gated by `get services/proxy` RBAC. `k8s/adapter::
  fetch_service_proxy` is the **generic, reusable substrate** (extracted from opencost in
  the v0.66 hardening round — the first of a planned set of optional-tool adapters,
  Prometheus/PromQL next); `opencost::spawn` is a
  fetch-not-watch poller (like `metrics`) filling a shared `OpenCostStore`; no new cargo
  feature (reuses the kube client) and no new crate (`http` + `futures` already in the
  tree). **Pure parse** `state/opencost::parse_allocation` (`/allocation` JSON →
  `OpenCostData`; `totalCost` is cumulative-over-window → hourly = totalCost/minutes×60;
  tolerant, never panics). **`cost::from_opencost(oc) -> CostReport` (basis `OpenCost`)**
  builds the SAME report the overlay/advisor/SELECTION already render — so when OpenCost is
  fresh it simply **REPLACES** the estimate (no two-sources mixing), provenance-labelled
  "from OpenCost"; absent/unreachable it degrades to the requests/usage estimate AND
  surfaces WHY (STATUS shows `OpenCost off (estimate): <error>` — never a silent fallback).
  **Scope decision (honest + certain):** the query aggregates by `namespace,controller`
  (the workload + namespace + total + idle `$` rollups — OpenCost's invoice value); the
  **per-node MAP overlay is deliberately NOT driven by OpenCost** (a multi-node controller
  has no single node under controller aggregation → unreliable per-node attribution), so
  under OpenCost `by_node` stays empty, the overlay recedes to idle-land, and the advisor
  says so; a per-node OpenCost overlay (a verified `aggregate=node` query) is a documented
  future increment. CLI: `--opencost [ns/svc:port]` (default `opencost/opencost:9003`) +
  `--opencost-window` (default `1d`); hot-cluster only. READ-ONLY — no new write verb.
  **Adversarial review (8 confirmed, all fixed):** (HIGH) the advisor gate keyed on
  `nodes_priced==0` → an OpenCost realm (no per-node data) falsely read "no priced nodes
  yet" and blanked the populated rollups — now gates on "any data"; (HIGH) `rate()` let
  **Infinity** through (`INF.max(0.0)==INF`, the lone sanitizer) — now clamps non-finite to
  0; (HIGH/MED) the false per-node "idle 0%"/"upkeep $X" mislabels are MOOT under the
  no-per-node-OpenCost scope (`by_node` empty → the SELECTION's `by_node` guard shows
  nothing); (MED) a silent fetch-failure fallback now surfaces the error in STATUS; (MED)
  OpenCost `__unmounted__`/`__unallocated__` control keys no longer leak a blank-namespace
  row (skipped in parse; empty-ns skipped in `from_opencost`); (MED) the response body is
  **capped at 8 MiB** via `request_stream` + `take` (`request_text` buffers unbounded — a
  hostile in-cluster Service could OOM the process); (LOW) ns/svc/window are validated
  (DNS-1123 labels; no URL-control chars) so a typo errors → honest fallback instead of a
  silently-wrong "from OpenCost" number. 318 core + 58 GUI tests. Verified live on kind
  (degrade path): `--opencost` with no OpenCost installed shows "OpenCost off (estimate):
  not found — is OpenCost installed…" + the estimate rollup + `view: cost (units)`; the
  OpenCost-active `$` path is unit-tested (live verification needs a cloud-billing OpenCost
  install). **Deferred:** a per-node OpenCost overlay (`aggregate=node`); a Prometheus/PromQL
  source on the same substrate (Hubble, kube-state-metrics); warm-cluster OpenCost; folding
  `__unmounted__` PV cost into a dedicated field rather than dropping it.
- **Pre-1.0 readiness — four build items** (2026-06-24, v0.65.0, user "complete the
  remaining 4 pre-1.0 items" after a v1-readiness gap analysis — design-workflow:
  5 lenses → synthesis — found the product is *feature-saturated* but *distribution-/
  robustness-short*; these are the four "build before 1.0" items, the rest deferred to a
  polish/hardening round): **(1) Release pipeline + CI.** `.github/workflows/ci.yml`
  (fmt + clippy-workspace + clippy-core-no-features + test + smoke-example build, on
  ubuntu + macos; Linux installs the macroquad X11/GL/ALSA dev libs) and `release.yml`
  (on a `v*` tag: macOS **universal** via `lipo` of aarch64 + x86_64, Linux x86_64,
  tarballs with the licenses + `THIRD-PARTY-NOTICES.md`, `SHA256SUMS`, a
  `softprops/action-gh-release` publish). The macOS binary is **unsigned** — the release
  body carries the `xattr -d com.apple.quarantine` Gatekeeper step; notarization is a
  follow-up needing an Apple Developer cert (a repo secret). `actionlint`-clean; can't be
  run without a push. **(2) Workload table** (`gui/workloads.rs`, `O` / View ▸ Workloads /
  `--workloads`): the realm-wide k9s-style triage list the map drill-downs didn't cover.
  Pure `table_rows(workloads, severity, sort, filter)` (filter = case-insensitive
  substring over kind/ns/name; sort by health / name / ready / age — clock-free, age sorts
  on the raw `Time` so it's deterministic + unit-tested) over `Models.workloads` +
  `workload_severity`; a `window.rs` modal whose filter `TextField` **owns the keyboard
  while open** (`typing` ORs `workloads.is_some()`; the `O` open flushes the queued char);
  clicking a row opens `Panel::City`. Hot-only; read-only. **(3) Connection banner**
  (`net.rs` `ConnState{Connecting,Live,Lost}` + a `spawn_liveness` `apiserver_version()`
  probe every 5s/4s-timeout; `panels::conn_banner` pure fn + `draw_conn_banner` under the
  chrome): reflector readiness can be stale-but-served while the API is unreachable, so a
  dedicated probe drives a banner ("reconnecting to <ctx> — <reason>") instead of silent
  fog on a VPN drop; self-clears on recovery. `kubernation_core` now re-exports `Client`
  so the GUI can name it without a direct `kube` dep. **(4) Multi-container log picker**:
  `LogReq.container: Option` (used directly by the net fetch, else `first_container`); a
  container tab row in `draw_logs` (returns the clicked container) populated from
  `ObservedWorld::pod_containers` (watched store, no fetch) — a sidecar/init-crash pod no
  longer silently tails the wrong container. **Adversarial review (5 confirmed: 1 HIGH, 1
  MED, 3 LOW, all fixed):** (HIGH) the liveness probe had **no gen-guard** — its `set_conn`
  runs after the await, past where `abort()` can interrupt, so a switched-away cluster's
  probe could write the old liveness into the new banner for ~4s; added `Net.conn_gen`
  (bumped on switch, checked before every write) mirroring the `oracle_gen`/`models_gen`
  pattern; (MED) the Annals `H` gate omitted `&& workloads.is_none()` (the `!typing` gate
  already blocked it, but added for explicit symmetric exclusion); (LOW) `release.yml` set
  both `generate_release_notes` + a custom body (dropped the former); (LOW) the probe now
  needs **two consecutive failures** before `Lost` (no flap on a single stutter); (LOW)
  the container re-click guard compares the resolved *active* container, not `r.container`
  (no redundant re-fetch on re-clicking the implicit-first tab). 318 core + 61 GUI tests;
  gui-smoke +`workloads`. Verified live on kind (the table shows crashy floated to the top
  in red under the health sort). **The rest of the v1 work is the polish/hardening round:**
  panic hook + net-thread `unwrap` audit, end-to-end error UX, multi-platform CI proof,
  large-cluster resilience, operator docs, generalizing the HTTP-adapter substrate into
  `k8s/adapter.rs`, a colorblind-safe palette, and a license-drift CI guard.
- **v1 hardening round** (2026-06-24, v0.66.0, user "yes, hardening"; driven by a 4-lens
  crash-safety AUDIT workflow → 34 findings deduped to ~5 real themes): the product was
  feature-complete but **robustness-short**. **(1) Crash safety** (the marquee fix): a
  global panic hook (`logging::install_panic_hook`) logs every panic (thread · location ·
  message) to `~/.local/state/kubernation/kubernation.log` *before* unwinding — a
  macroquad GUI has no console, so an un-logged crash was undiagnosable. The net thread is
  named **`kn-net`**; the hook sets a `net::NET_PANICKED` flag when it dies and the GUI
  paints a **fatal banner** (`panels::draw_fatal_banner`) instead of a silently frozen
  world. **The poison cascade** (the real correctness gap): all **~160** `.lock().unwrap()`
  in `net.rs` → `.lock().unwrap_or_else(|e| e.into_inner())` — a panic in one background
  tokio task can no longer poison a shared mutex and crash the **render** thread on its
  next read (the GUI reads `snapshot`/`conn`/`log_tail`/… every frame). The tokio runtime
  + the log-file writer degrade gracefully (no panic); the per-session Oracle reply cache
  is bounded (64). Audit **false positives verified**: the event ring IS bounded
  (`watch.rs:323`, CAP=500) and the attention index IS guarded (`main.rs:1464`) — the
  auditor flagged the field decl / wrong line. **(2) Substrate extraction:** OpenCost's
  reusable plumbing → `k8s/adapter.rs` (service-proxy fetch + body-cap + DNS/query
  validators) with a documented "pattern for a new adapter" — the extension story is now a
  design, not a one-off. **(3) Large-cluster evidence:** the `scale_rebuild` perf-test went
  100N/1kP → **500N/5kP** (~4ms/rebuild, ≪100ms budget). **(4) License-drift CI guard:** a
  `cargo-about` regenerate-and-diff job fails if `THIRD-PARTY-NOTICES.md` falls out of sync
  with `Cargo.lock` (regenerated; the `http` dep had drifted it). **(5) Operator docs:** a
  README on-ramp — Install (release binaries + the macOS Gatekeeper step + from-source),
  RBAC requirements (read verbs + the gated write verbs + the in-app Charter), and
  Troubleshooting (won't-connect, the log location, no-metrics). 318 core + 61 GUI tests;
  lint + actionlint clean; boots + renders with no panics logged. **Adversarial review of
  the crash-safety change (2 confirmed):** (CRITICAL) the poison sweep used a single-line
  regex, so **13 multi-line `.lock()\n.unwrap()`** sites (rustfmt-split) were missed — 3
  render-thread-reachable accessors, the very cascade the change prevents; all converted
  (0 bare lock-unwraps remain). (MEDIUM) the panic-hook flag keyed on the thread NAME
  `kn-net`, but the multi-threaded net runtime runs the reflectors (the world loop) on
  `tokio-runtime-worker` threads → their panic wouldn't flag it; now gates on **ThreadId !=
  the render thread**, catching any background-thread panic. **Deferred (honest
  scoping):** the **colorblind-safe palette** was scoped here as a real refactor and then
  built as a follow-up (v0.67.0, below); **persisted preferences** (a `prefs.toml`) is pure
  convenience, below the robustness/accessibility bar of this round. **Cannot be done
  here:** *proving* the multi-platform CI green needs an actual push (the workflows are
  written + `actionlint`-clean) and macOS code-signing/notarization needs the operator's
  Apple Developer cert.
- **Colour-blind-safe palette** (2026-06-24, v0.67.0, user "yes, colorblind palette"; the
  v1-hardening item deferred as a refactor): the product's whole grammar is **green
  (healthy/good/calm/low) vs red (critical/NotReady/high)**, which red-green colour-blindness
  (deuteranopia + protanopia, ~8% of men) can't distinguish. **Insight that contained the
  change:** only the GREEN axis needs to move — to a steel **blue** — because blue / amber /
  red are all mutually distinguishable; **red (CRIT) and amber (WARN) are left untouched**
  (already distinguishable, and glyph-redundant in the queue). So instead of converting all
  ~220 `CRIT`/`WARN`/`GOOD` const references to runtime, only the *green* meaning-colors
  switch: a `COLOR_MODE` atomic (`theme::set_colorblind`/`colorblind()`, set once at startup
  from `--colorblind`, before any draw), the `GOOD` const → a `good()` fn + a `gauge_ok()`
  fn (the gauge/fill green), and the green positions of the funnel fns (`terrain` /
  `iso_terrain_pair` / `heat_pair` / `sync_color` / `pod_color`) branch on `colorblind()`.
  `GOOD`→`good()` was a scripted whole-word replace (35 sites). READ-ONLY; no posture
  change. **Adversarial review (2 confirmed missed meaning-greens, both folded):** a
  `plan.rs const GREEN` (commit/plan-ok, the opposite of the CRIT-red failure rows — a const
  can't switch) and the context-picker active-dot, both → `gauge_ok()`; the review confirmed
  coverage is otherwise complete (every other green funnels through `good()`/`gauge_ok()`/
  `cb_land` or has an inline `colorblind()` arm). Pure palette-switch test + gui-smoke
  `colorblind`; verified live (the map's healthy land + minimap go steel-blue under
  `--colorblind`, distinct from the red trouble marks). **Tritanopia (rare blue-yellow) is
  out of scope** — it would want a different remap. **Deferred:** distinct
  deuteranopia/protanopia presets (one red-green-safe palette serves both); namespace-hue
  categorical colours (labelled, lower priority). (Persisting the choice landed in the
  next entry.)
- **Persisted preferences** (2026-06-24, v0.68.0, user "let's get the persisted
  preferences in place"; the last v1-hardening item): a small
  `~/.config/kubernation/prefs.json` (`gui/prefs.rs`) remembers UI choices across runs so
  they aren't re-set every launch — **CLI flags always win** (`args.x || saved.x` for
  colorblind, `flag.or(saved).unwrap_or(default)` for overlay). **Scope = surprise-free
  display state only:** the **colour-blind palette** (now also a live **View ▸ Colour-blind
  palette** toggle — the palette reads the `COLOR_MODE` atomic each frame, so flipping it
  re-colours immediately) and the **last map overlay**. JSON (not TOML — no new dep; reuses
  the `serde_json` already present + mirrors `oracle.json`'s atomic-temp+rename write, with
  a corrupt file renamed aside, never deleted). Saved on clean exit (after the render loop,
  **skipped under `--screenshot`** so a dev/CI capture never mutates the operator's prefs);
  loaded at startup before any draw. **NON-secret, NON-cluster** (the Oracle token stays in
  its own 0600 file; no cross-run cluster state). The first persisted GUI config that isn't
  the Oracle's. Serde round-trip + partial/garbage-tolerance unit-tested; verified live (a
  seeded prefs.json launches blue + the cost view with NO flags). **Deliberately deferred:**
  **context + namespace-filter** persistence — both carry a real "pin the saved value vs.
  follow your kubeconfig current-context / show an empty world" UX tension that deserves its
  own decision, not a silent default; and OS-keychain/encrypted prefs (none of this is
  sensitive). **This completes the v1 hardening round** (crash-safety, substrate, perf,
  license guard, docs, colour-blind palette, prefs); the only pre-1.0 items left need the
  operator: pushing to prove the multi-platform CI green, and macOS signing (no Apple cert).

- **"Silent crash on maximize" — Esc-quit fix + native-fault forensics**
  (2026-06-25, v0.73.0, user reported the app "quickly disappears" when
  maximizing the window on macOS; investigation-workflow: 3 hunt dimensions →
  verified findings + live repro attempts): the reported symptom left NO trace —
  no Rust panic in the log (the v0.66 hook is unbuffered + verified working), no
  macOS crash report (this machine's DiagnosticReports is inert, so absence
  proves nothing), nothing in the unified log. **Programmatic repro attempts all
  survived** (debug + release): a new storm harness drove zoom-sized resizes,
  `toggleFullScreen:` (the REAL green-button path — verified in miniquad
  source), rapid toggles, resize-mid-animation, and tiny windows through the
  live app with zero faults; the resize-math sweep found the layout panic-free
  at maximize scale (every computed-bound clamp/division is guarded). **The
  probable cause found by the exit-path audit:** the Esc chain's bare-map
  fallthrough was `break` — quit. The green button enters native **fullscreen**
  (not maximize); a reflexive Esc to leave fullscreen therefore **cleanly and
  instantly exited the app** — exactly matching "maximized, it disappeared, no
  trace" (a clean exit writes nothing anywhere). It ALSO bypassed the chaos
  restore-on-exit intercept (the `break` skips the `want_quit` gate) — a real
  safety-triad hole. **Fix:** bare-map Esc never quits; on macOS it calls
  `set_fullscreen(false)` (miniquad's macOS impl no-ops when windowed — the
  Windows/X11 backends lack that guard, hence the cfg); quit remains on Q /
  Game ▸ Quit / Cmd+Q / close — all through the restore gate. **Forensics** (the
  can't-reproduce insurance; a native EXC_BAD_ACCESS in Apple's deprecated
  GL-on-Metal layer during live-resize remains a grounded second suspect —
  allegro5 hit exactly that on macOS 26 + Apple Silicon, and miniquad draws from
  `drawRect:` during live resize; no miniquad release fixes it, 0.4.11's macOS
  backend is byte-identical to 0.4.10, and macroquad pins `=0.4.10`):
  `logging::install_fault_handler` — async-signal-safe SIGSEGV/SIGBUS/SIGILL/
  SIGFPE/SIGABRT handlers writing one static line to a pre-opened log fd
  (`write(2)` only; `SA_ONSTACK` + `SA_RESETHAND`, then re-raise) — plus a
  **session marker** (`session_begin`/`session_end` around the render loop;
  skipped under `--screenshot`) so ANY abnormal end, SIGKILL included, is
  reported at the next launch. libc was already in the lock (zero new crates).
  The storm harness shipped as the **`--resize-storm`** dev flag (exits through
  the clean path). Two latent total-function gaps hardened in passing
  (`truncate_str` max==0 underflow; oracle `wrap` width==0 chunks-panic — both
  currently unreachable, flagged by the review). Verified live: SIGSEGV → the
  FATAL log line; kill -9 → "previous session ended abnormally" on relaunch;
  clean exit removes the marker; the storm passes in both profiles; gui-smoke
  47. If the user's crash recurs on this build, the log now says which it was:
  a FATAL line = native fault (→ escalate the GL/live-resize suspect, likely a
  miniquad fork or Metal backend), an abnormal-exit warn with no FATAL = kill,
  and neither = it was the Esc-quit, now fixed.
- **Vendored miniquad + macOS resize-crash mitigations** (2026-06-25, v0.74.0,
  follow-up: the user clarified the crash was on a DIFFERENT machine — a
  business Mac whose logs can't be exported — and **Esc was not pressed**, so
  the v0.73.0 Esc fix, while real, was not their incident; this was a genuine
  native crash; user chose "ship both patches"): miniquad 0.4.10 is now
  **vendored** at `third_party/miniquad` via `[patch.crates-io]` (the FIRST
  vendored dependency — justified because macroquad 0.4.15 pins `=0.4.10`
  exactly, 0.4.11's macOS backend is byte-identical, and both fixes exist
  nowhere released), carrying two marked patches in `src/native/macos.rs`:
  **(1) `backingScaleFactor = 0` guard** — backported verbatim from upstream
  master `14c6fc31` (unreleased): macOS reports scale 0.0 while the window is
  screen-detached (startup + transiently during display/zoom transitions on
  docked/multi-monitor setups — plausible on a business Mac); 0.4.10 recorded
  `dpi_scale = 0` → 0×0/NaN dimensions downstream. **(2) skip GL frames during
  interactive live-resize** (`inLiveResize` check before `perform_redraw` in
  the GL view's `drawRect:`) — the mitigation for the lead suspect, Apple's
  GL-on-Metal `EXC_BAD_ACCESS` when drawing mid-live-resize on macOS 26 +
  Apple Silicon (allegro5 #1749 documents a guaranteed crash; miniquad ran a
  FULL app frame from the resize tracking loop — exactly the trigger; my
  programmatic storm used `setFrame`/`toggleFullScreen`, a different runloop
  regime, which is why it never reproduced). Tradeoff (accepted): window
  content freezes during a drag-resize, repaints on release; the
  `resize_event` still fires (layout stays fresh) and fullscreen/zoom
  animations still paint. `third_party/README.md` records provenance + the
  drop-when-upstream-lands exit; the license guard passes unchanged (the
  notices already covered miniquad; LICENSE files ship in the vendored copy).
  Verified: renders normally on the patched backend, `--resize-storm` passes,
  47 gui-smoke states, 69+318 tests, `--locked` clean. **Adversarial review (7
  confirmed → all addressed):** (HIGH) ci.yml's global `RUSTFLAGS: -D warnings`
  would have hard-failed macOS+Linux CI — a PATH dependency loses the
  `--cap-lints allow` registry deps get, and the vendored crate carries
  platform-specific upstream warnings (Linux's `static_mut_refs` in the x11
  backend was reproduced — un-fixable by per-lint allows enumerated on macOS);
  fixed by dropping the global env (the clippy step's `-- -D warnings`
  argument, scoped to workspace members, is the real gate). (MEDIUM) a future
  macroquad upgrade changing the `=0.4.10` pin makes cargo SILENTLY ignore the
  `[patch]` (warning + `[[patch.unused]]` only) — both crash fixes would vanish
  with green CI; added a CI step asserting `cargo tree -p miniquad` resolves to
  `third_party/`. (MEDIUM) skipping only the draw left the drawable being
  resized (`[gl_context update]` in `update_dimensions`) but never re-presented
  → implementation-defined mid-drag visuals; the skip now sits BEFORE
  `update_dimensions` in `drawRect:` AND gates `windowDidResize:` — the intact
  last-presented surface is compositor-stretched (deterministic), and the
  drawable-reallocation churn (GL work of the same class as the crash trigger)
  is deferred to drag-end too. (LOW) `== YES` → `!= NO` (objc-rs BOOL is
  c_schar on x86_64 — any non-zero is truthy; `== YES` failed open on the Intel
  half of the universal binary). (LOW, accepted+documented) one large
  frame-time delta on the first post-drag frame (lerps snap); the unreachable
  `blocking_event_loop` deferral. (LOW, pre-existing, deferred) `about.toml`'s
  pinned targets omit Windows, so the Windows zip's notices lack the
  Windows-only crates (winapi) — predates this change; fix in its own pass.
  **Diagnostics still wanted from the business machine** (viewable locally,
  nothing exported):
  install ≥v0.73.0 there and read the log tail after a recurrence (`FATAL
  native fault` = confirmed native crash), or glance at Console.app ▸ Crash
  Reports for the top frame module (`AppleMetalOpenGLRenderer`/`GLD…` would
  convict suspect 2); also: Apple Silicon or Intel, macOS version, docked to
  an external display, and whether it crashes every time.
- **Signed & notarized macOS releases** (2026-07-15, v0.75.0, user has an Apple
  Developer account + a Developer ID Application cert + an App Store Connect API
  key): the release pipeline closes the v1 "unsigned macOS binary" gap. **Chosen
  form (user's call): a `.app` bundle in a `.dmg`**, over a bare notarized binary
  or an `.app` in a tarball — because KuberNation is a *windowed GUI*, so a bare
  Mach-O double-clicked in Finder just opens Terminal, AND a notarization ticket
  **cannot be stapled to a bare binary** (only `.app`/`.dmg`/`.pkg`), so a bare
  binary would fail Gatekeeper offline. The `.app` gives a real Dock/window
  identity + an offline-verifiable stapled ticket; the `.dmg` gives drag-to-
  Applications. **`packaging/macos/release-macos.sh`** (locally runnable, not
  CI-only) does the whole flow: assemble `KuberNation.app` (Info.plist from
  `packaging/macos/Info.plist.template` with `@VERSION@` substituted; `.icns`
  built from the 256px `assets/logo/mark.png` via `sips`+`iconutil`; license/
  notice files copied into `Contents/Resources` **before** signing so codesign
  seals them) → `codesign --options runtime --timestamp` (hardened runtime, a
  notarization prerequisite; **no entitlements** — the statically-linked Rust GUI
  needs no JIT / library-validation exception) → `notarytool submit --wait` (a
  zip of the `.app`) → **staple the `.app`** → build the `.dmg` (with an
  `/Applications` symlink) → sign → **`notarytool submit` the `.dmg` too** →
  **staple the `.dmg`**. **TWO notarization round-trips, load-bearing** (found by
  the pre-tag dry-run, which failed here): a ticket is keyed to the cdhash of the
  code that was **submitted**, and the `.dmg` is its own separately-signed code
  object with its own cdhash — so notarizing only the `.app` leaves the `.dmg`
  unstapleable (`stapler` → "Record not found"). Stapling both is what makes the
  `.dmg` verify offline at mount **and** the `.app` verify offline once dragged
  out to /Applications. **CI (`release.yml`):** a `Detect macOS signing
  secrets` step gates signing on `MACOS_CERT_P12_BASE64` being present — **a tag
  pushed without secrets still releases**, degrading to the prior unsigned
  bare-binary tarball (a `::warning::`) rather than hard-failing; when signing,
  the cert `.p12` is imported into an **ephemeral throwaway keychain** (password
  generated in-runner with `openssl rand`, `set-key-partition-list` so codesign
  runs non-interactively, deleted in an `always()` cleanup step), the identity is
  resolved with `security find-identity`, and the script runs with the API key
  `.p8` (base64 secret, decoded to `$RUNNER_TEMP`, removed after). **5 repo
  secrets** (documented in `packaging/macos/README.md` with the `.p12` export +
  base64 steps): `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`,
  `APPLE_API_KEY_P8_BASE64`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID` (Team ID
  isn't needed — the identity resolves from the single-identity keychain and
  notarytool authenticates by API key). The macOS artifact is now
  `kubernation-vX.Y.Z-macos-universal.dmg` (SHA256SUMS + release body + README +
  `site/index.html` updated; the `xattr -d com.apple.quarantine` workaround is
  gone for macOS users). Linux `.tar.gz` / Windows `.zip` unchanged (Windows
  stays unsigned — SmartScreen "More info ▸ Run anyway"). **Verified end-to-end
  locally against the real cert + a real Apple notary round-trip** (v0.75.0 dry
  run, 2026-07-16): universal `lipo` binary → signed → **both** submissions
  Accepted → both stapled → `spctl -a` reports **"accepted, source=Notarized
  Developer ID"** for the `.dmg` AND for the `.app` mounted from inside it, which
  is exactly what a downloading user's Gatekeeper evaluates; `stapler validate`
  passes on both (offline tickets present); the bundled binary is `x86_64 arm64`
  and `Contents/Resources` carries the licenses + notices. `actionlint` +
  `shellcheck` clean; `cargo metadata --locked` consistent. The dry run earned its
  keep: it caught **two real bugs** that would have failed every release — the
  `set -u` empty-array crash (`"${KC_ARGS[@]}"` on macOS bash 3.2) and the
  one-vs-two notarization error above. **Apple-side caveat observed:** a
  submission sat `In Progress` for ~90 min (normal is 1–5) before being Accepted —
  their queue backs up, so `NOTARY_TIMEOUT` (default 45m) is configurable; a CI
  timeout fails the build job and `publish` (`needs: build`) never runs, so there
  is no half-published release — just re-run the workflow. **Still needs the
  operator:** proving the *CI* path green needs a real `v*` tag push with the five
  secrets set (the local flow is proven). **Deferred:** Windows Authenticode
  signing; a Homebrew cask; a `sparkle`-style auto-updater.

## The pair (hot/warm)

`--warm <context>` attaches a second cluster (the config `warm_context` form
went with the TUI — CLI flag only now):

- **Two continents:** the map splits left (HOT) / right (WARM) with a `║`
  divider and a banner per side; `h`/`l` pushed past a map edge crosses to
  the other continent (single-cluster mode ignores the edge signal). Each
  side keeps its own cursor, scroll, and minimap. Detail views and the
  workload list belong to one world — titles carry `— HOT` / `— WARM`.
- **Sync state** (`state/pair.rs`): per-workload comparison of presence,
  desired replicas, and pod-template image sets. DaemonSet replica counts
  are exempt (desired tracks node count). Badges: `=` in sync (dim), `≠r`
  replica drift, `≠i` image drift (yellow), `−w` missing on warm (red, the
  dangerous direction), `+w` only on warm (cyan). Shown as a SYNC column in
  the workload list and a "pair" line in the city screen.
- **Attention:** one merged queue, entries tagged `H`/`W`; `n` routes to
  the right world's view. Pair drift contributes ONE aggregate concern
  ("pair drift: N workloads differ"), never per-workload spam.
- **Events:** `AppEvent::World(ClusterId, WorldDelta)`; each world has its
  own `WorldHandle`, models, and ready flag. `c` (context picker) switches
  the hot cluster only; the warm context is fixed at launch.
- Dev loop: `make warm-up warm-drift` then `make pair` (drift = web scaled
  3→1, crashy deleted, agent image bumped — one of each badge kind).

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
| `▒`   | fog of war (world not yet synced)  |
| `Ψ`   | Service harbor (on the city's east coast, cyan) |
| `∏`   | Ingress gate (on the city's east coast, cyan) |
| `⊞`   | PVC granary (inland/west of the city; cyan bound, yellow pending) |
| `◈`   | Job expedition (namespace island; yellow when failed) |
| `◷`   | CronJob (namespace island; detail = schedule) |
| breach notch | unwalled city (no ingress NetworkPolicy); red when also exposed — the Walls overlay only; walled cities draw nothing |

Health precedence on a tile: NotReady > Cordoned > Pressure > Healthy.
Zone headers carry a `▪N` rollup (colored by the zone's worst node) when
any node in the zone is degraded.

**Color discipline:** color encodes meaning, never decoration. *Terrain* reads
in a parchment-gold + green-land + blue-ocean palette; **saturated red / bold
yellow are reserved for attention** — trouble pops against terrain, never
competes with it. (The removed TUI had `color = auto/plain/mono` ANSI palettes;
the GUI uses fixed RGBA themes in `theme.rs`, same meaning rules.)

## Controls (the windowed client)

Mouse-first, with a classic-4X **menu bar** (Game/View/Orders/Advisors/World/
Help) and a few keys. The authoritative, user-facing list is the in-app
**Almanac** (`?`/`F1`, "Controls" page) — keep `almanac.rs` in sync with any
change. Summary:

- **Navigate:** drag / `WASD` / arrows pan · mouse wheel zoom (cursor-anchored)
  · `F` fit · `]`/`[` next/prev city · click the minimap to recenter.
- **Inspect:** click land/city/harbor opens the node/city drill-down window ·
  click a pod row tails its logs · `y` (or a pod row's `yaml`) opens the YAML
  dossier · hover for a tooltip.
- **Logs overlay:** `/` filter (terms AND; `!term` excludes) · `p` previous
  container · `T` timestamps · `s` history window (500 / 2k / since 1h) ·
  `c` copy · `w` export · lines tinted by guessed severity.
- **Attention:** `N` fly to the next concern · `L` tail the focused concern's
  offending pod's logs · `B` blast radius (highlight what a selected node/city —
  or the focused concern — affects: cities → harbors → gates).
- **Resource browser:** `:` (any kind — pick → table → click a row's YAML).
- **Planning turn:** city window steppers stage scale / restart / image and the
  HISTORY section's `rollback` button stages a roll-back; the province window
  stages cordon; **Orders ▸ End of Turn** reviews + commits (confirm modal).
  **Evict** a pod from its row (real delete, RBAC-gated, confirm).
- **Port-forward:** hover a pod row → **fwd** opens a `127.0.0.1` tunnel to it
  (RBAC-gated; default port = its containerPort or selecting-Service targetPort);
  the right column's **FORWARDS** section lists live forwards with an **x** to
  stop. Not a cluster write, but gated like one.
- **Treasury / SLO:** the city window shows the error budget + an SLO stepper
  (per-workload target; also `kubernation.io/slo-target` annotation, `--slo-target`
  default).
- **Game Day (chaos):** the **Game Day** menu opens a chaos drill — pick a
  workload + experiment (kill one / kill all / outage), preview blast + budget,
  **Run drill** (real, confirmed, RBAC-gated; control-plane/system namespaces
  refused). A scorecard shows recovery + budget spent; an outage offers Restore.
- **Esc** closes the topmost overlay · the menus carry switch-context, fit, the
  map overlay (terrain/pressure/replicas/namespace/walls/saturation), namespace
  filter, advisors, Almanac, quit.

## Dev loop

```
make dev        # kind-up + samples + run the windowed client (standard loop)
make run        # the windowed client (macroquad) against the dev cluster
make smoke      # headless connect + world summary, exit (CI gate; core example)
make lint test  # fmt --check, clippy -D warnings, cargo test
make gui-smoke  # render every overlay/modal state, fail on panic (needs display)
make kind-down

make perf-up    # kwok-simulated 100-node / 1000-pod cluster (needs kwokctl)
make perf       # run the client against it
make perf-test  # release-mode model-rebuild budget test (<100ms asserted)
make perf-down
```

Develop against kind only (`hack/kind-config.yaml`, cluster `kubernation`,
context `kind-kubernation`). `hack/samples.yaml` provides: healthy `web`
(+Service), crash-looping `crashy`, StatefulSet `db` (+PVCs), DaemonSet
`agent`, `stuck-pvc` which never binds (keeps one Warning in the queue),
and two `Gizmo` customs (CRD in hack/samples-crd.yaml) for projection.
`make run`/`make pair` pass `--project gizmos.example.com`.

Config / logs: the windowed client is driven by CLI flags (`--context`,
`--warm`, `--project`, `--log-level`, plus the `--screenshot`/`--inspect`/… dev
flags); it reads no config file. It **does** write a log file —
`logging.rs::init` installs a `tracing_subscriber` to
`~/.local/state/kubernation/kubernation.log` (`RUST_LOG` overrides `--log-level`),
so core's `tracing` events are captured (the no-subscriber gap the TUI removal
left was fixed). No config-file support yet.

## Conventions

- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean
  before any commit.
- New state logic ships with unit tests against `state/fixtures.rs` (the
  interesting logic lives in pure core, where it's testable without a cluster
  or a display). **New GUI views ship a pure draw-decision fn + test too** (the
  `panels::region_lines` pattern — see the "GUI testability policy" decision);
  macroquad rendering itself isn't unit-testable, so `make gui-smoke` is the
  crash gate over the `--screenshot` states.
- Commit in working states with descriptive messages; the user reviews
  commits.
- **Versioning (semver):** one workspace version is the source of truth
  (`[workspace.package] version` in the root `Cargo.toml`; every crate
  inherits it via `version.workspace = true`). **Bump it in the same commit
  as a user-facing change** — pre-1.0, so `minor` = new feature/behaviour,
  `patch` = fix/docs/refactor, and (still pre-1.0) a breaking change also
  bumps `minor`. The version is surfaced by `--version` and the GUI chrome
  (`env!("CARGO_PKG_VERSION")`). Update `CHANGELOG.md` under `[Unreleased]` as
  you go; a release rolls Unreleased into a dated `[X.Y.Z]` section and is marked
  by a git tag `vX.Y.Z`.
- Document non-obvious decisions in this file's Decisions log as you make
  them.

## Performance evidence (criterion 6)

Synthetic: `make perf-test` (a core test, `scale_rebuild`) builds a fixture
world of **500 nodes / 5000 pods** (a large real cluster) and times the full
`Models::build` rebuild (map + workloads + attention — what the GUI recomputes each
tick) — **~4ms/rebuild on the M4 Max**, asserted <100ms in release. (Originally this also rendered a TUI
140×40 frame; the render moved to the GUI, which isn't unit-timed.) Live: `make
perf-up && make perf` runs
against a kwok-simulated cluster of the same size (`hack/perf-seed.sh`,
5 zones × 20 nodes, 20 deployments × 50 replicas). Input latency is
unmeasurable by eye; world rebuilds are coalesced at tick cadence so churn
never blocks input.

## Deferred (deliberately)

external services / chaos layers ·
unmounted-PVC island granaries (the *map* feature; connectivity + failed-Job
attention are now built) · Job/CronJob
city windows · pair: per-container image diffs, env/config
drift, unified single-board mode ("one continent, sync ghosts") · logs:
the kube log *stream* (we poll the tail), a multi-container picker /
all-containers (log-UX tier T2), and multi-pod "whole-city" tailing (B1) ·
a GUI log file (no `tracing` subscriber is attached today).
