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
Staging itself still never writes — only Commit does. See the "Pod eviction" and
"Planning-turn apply" decisions. One **active-but-non-mutating** capability,
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
  aside).
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
- **Planning turn:** city window steppers stage scale / restart / image, the
  province window stages cordon; **Orders ▸ End of Turn** reviews + commits
  (confirm modal). **Evict** a pod from its row (real delete, RBAC-gated, confirm).
- **Port-forward:** hover a pod row → **fwd** opens a `127.0.0.1` tunnel to it
  (RBAC-gated; default port = its containerPort or selecting-Service targetPort);
  the right column's **FORWARDS** section lists live forwards with an **x** to
  stop. Not a cluster write, but gated like one.
- **Esc** closes the topmost overlay · the menus carry switch-context, fit, the
  map overlay (terrain/pressure/replicas/namespace), namespace filter, advisors,
  Almanac, quit.

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
`--warm`, `--project`, plus the `--screenshot`/`--inspect`/… dev flags); it does
not read a config file or write a log file (the TUI's `config.rs`/`logging.rs`
went with it). `tracing` events from core have no subscriber attached, so they're
dropped — add one to the GUI's `main` if a log file is ever wanted.

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
world of 100 nodes / 1000 pods and times the full `Models::build` rebuild (map +
workloads + attention — what the GUI recomputes each tick) — **~1ms/rebuild on
the M4 Max**, asserted <100ms in release. (Originally this also rendered a TUI
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
