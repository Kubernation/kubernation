# K8sCiv

A Civilization-inspired Kubernetes TUI. The cluster is a living **world**
the operator explores: zones are continents, nodes are provinces of
health-textured terrain, workloads are cities sited where their pods run
(population badge + name label), DaemonSets are roads, and abstract things
— custom-resource instances, zero-pod workloads — live on namespace
islands in the southern sea. An attention queue surfaces what needs focus
and parks the explorer's cursor on it — Civ's "next unit needing orders",
not a wall of dashboards.
**Observe-only.** No mutation paths exist anywhere in the codebase.

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

Built beyond MVP: the **hot/warm cluster pair** — two continents side by
side with sync-state badges (see "The pair" below). Future (designed-for,
not built): external managed services as foreign powers, chaos events as
barbarian raids, and the planning-turn staged-diff intervention model.

## Architecture (cargo workspace)

```
crates/
  k8sciv-core/   NO UI DEPS — everything frontends share:
    events.rs    ClusterId / WorldDelta vocabulary
    k8s/         DATA LAYER: client+platform detect, quantity parsing,
                 reflector spawning (watch.rs; spawn() takes a DeltaSink
                 closure so any frontend can subscribe); metrics.rs (poll
                 metrics.k8s.io) and logs.rs (on-demand pod log tail) sit
                 beside the reflectors — both are fetch-not-watch
    state/       observed.rs  ObservedWorld (reflector stores + event ring
                              + dynamic custom-resource stores)
                 world.rs     PURE world geometry: continents/provinces/
                              cities/islands, placement, hit-testing
                 planned.rs   PlannedWorld stub (future planning turn)
                 model.rs     PURE derivations: map/workloads/city/node
                 attention.rs PURE detectors → severity-ordered concerns
                 fixtures.rs  synthetic worlds (feature = "fixtures")
    util.rs      fnv1a64 stable hash, age/bytes formatting
  k8sciv/        THE TUI (the product): main/app/events/logging/config
                 + ui/ components (map, workloads, city, node_detail,
                 attention_panel, sidebar, status_bar, help, picker,
                 theme, symbols). `cargo run` = this (default-members).
  k8sciv-gui/    macroquad windowed client over the same core (promoted
                 from spike): net.rs (tokio thread publishing Models +
                 ObservedWorld snapshots), draw.rs (terrain mosaic,
                 settlements, minimap, camera), panels.rs (hover tooltip,
                 attention strip, context picker, shared helpers),
                 window.rs (reusable modal chrome for drill-downs),
                 almanac.rs (the in-app reference / Civilopedia),
                 city.rs / node.rs (the Civ-II city + province drill-down
                 windows, on window.rs), theme.rs. See "GUI spike" + "GUI
                 promotion" decisions.
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
  now reflects live usage when present (≥0.9 = Pressure). Pod-level metrics
  are a possible later add; gauges are node-level. `make metrics-up`
  installs metrics-server on kind (needs `--kubelet-insecure-tls`).
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
  PVCs as granaries, Jobs/CronJobs, and connectivity attention (orphan
  ingress / harbor with no city).
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
  CronJob in samples (docs/gui-batch.png). Deferred: Job-object attention
  (Job *pod* failures already route to the node), Job/CronJob city screens.
- **GUI window system + Almanac** (2026-06-16, user request after pasting
  Civ II's Civilopedia + city screens): the GUI had only bespoke overlays
  (tooltip, side panel, log overlay, picker) — no shared modal. `window.rs`
  is a reusable centered modal (dimmed scrim, parchment frame, titlebar +
  icon, a clipped body the caller fills, a button/tab row; Esc / close-box /
  click-outside dismiss), mirroring Civ II's window *structure* in the
  K8sCiv palette. macroquad has no easy scissor, so the body is culled +
  scrolled by the caller (per-line visibility test), like `draw_logs`.
  `almanac.rs` is the first consumer — our **Civilopedia**: pages Legend /
  World / Controls / Reading, opened with `?`/`F1` or the top-bar `?` button
  (`--almanac` dev flag for headless shots). The Legend draws the **actual
  marks** (reuses `draw_harbor/gate/granary/job/cronjob`, now `pub(crate)`,
  + `pod_color`) beside each definition, so it can't drift from the map.
  The TUI's `?` help gained a matching compact MAP LEGEND section.
  docs/gui-almanac.png. **Polish** (2026-06-16): Civilopedia cross-refs —
  each Legend entry whose mark has a live example (resolved from the hot
  world via `locate()`, derived from the `Mark`) lights up with a `>` chevron
  + hover highlight; clicking it returns `AlmanacAction::Locate(cell)` and
  the main loop flies the camera there, selects it, and closes the Almanac.
  Keyboard `1`-`4` jump to tabs and `←`/`→` cycle them; `window.rs` buttons +
  close box highlight on hover.
- **GUI city drill-down** (2026-06-16, the window system's first rich
  consumer): clicking a city opens a centered **Civ-II-style city window**
  (`city.rs`) instead of the old right-side panel — the Civ II city screen
  reframed for K8s (observe-only, so no Buy/Change): title bar `kind ns/name`
  (+HOT/WARM) → a **status band** with replicas + updated **gauge bars**,
  rollout, strategy/age, attention flag, pair-sync → **CITIZENS** (a pod
  census grid à la Civ II's food store + a clickable pod list that tails
  logs) → **IMPROVEMENTS** (owned svc/ingress/pvc/cm/secret) → **CHRONICLE**
  (recent events). Built on `build_city` + `window::draw_window`; fixed size
  with caps + "+N more" (Civ II's panels don't scroll). It's a **modal**:
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
  dependency shape without reading contents (least privilege). Ingress
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
- **Civ sidebar** (2026-06-12, visual pivot at user request): on the map
  screen, ≥110 cols (≥150 paired) adds a right sidebar shaped like Civ 2's:
  WORLD (the minimap, permanent home), STATUS (context/platform, node/pod
  counts, concern rollup, overlay), ORDERS (the selected tile — Civ's
  "Moving Unit" box: health, zone, conditions, pressure, pod census).
  Below the threshold the floating WORLD overlay takes back over
  (`MapView::external_minimap` suppresses it when the sidebar is up). The
  sidebar always shows the *focused* world. K8s terms are never renamed to
  Civ terms — the grammar is Civ, the nouns stay kubectl-greppable.
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
  (repeatable) or config `projections = [...]` resolves CRDs once at
  connect (LIST, no CRD watch), spawns `DynamicObject` reflectors, and
  renders instances as `✦` structures on namespace islands. CRDs missing
  on a cluster are skipped with a log line — a pair may project
  asymmetrically. Demo: `gizmos.example.com` in hack/samples-crd.yaml
  (applied before samples.yaml so the kind is established).
- **Workspace split + GUI spike** (2026-06-12, "spike" decision after the
  renderer-options review): k8sciv-core holds the data layer and pure
  models; `watch::spawn` takes a `DeltaSink` closure (not a TUI channel)
  so frontends subscribe their own way. crates/k8sciv-gui is a macroquad
  windowed client: tokio on a net thread publishing `Arc<Models>`
  snapshots, terrain-colored provinces, city circles sized by population
  with Civ-style name plates, namespace islands, pan/wheel-zoom camera,
  click-to-inspect ORDERS, attention strip, `--screenshot` for headless
  verification (docs/gui-spike.png). SPIKE quality: no tests, flat colors,
  ASCII-only text (macroquad default font has no exotic glyphs — `ascii()`
  sanitizer). Next steps if promoted: Kenney CC0 tile sprites, hover
  tooltips, city/node detail panels, pair view.
- **GUI promotion, round 1** (2026-06-12, "results are good, build on it"):
  procedural art instead of sprite packs first (per-cell mosaic shading,
  coast bevels, hut-tier settlements with Civ pop chips, warning banners,
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
- **GUI font + sprite tileset** (2026-06-12, "text could be better"):
  macroquad's built-in font is a blurry ASCII bitmap, so `text.rs`
  bundles Fira Sans (OFL) via `include_bytes!` and routes all labels
  through `text`/`text_bold`/`text_size` helpers (font in a thread_local,
  falls back to default if parsing fails). `sprites.rs` embeds a curated
  Kenney "Medieval RTS" set (CC0) — tiled terrain textures health-tinted
  (grass/grass2 healthy, sand tinted for cordon/pressure, stone for
  NotReady), house→keep building sprites by population tier, tent/rock
  for island structures — each with the old procedural shapes as a
  fallback when sprites are absent. `--tileset <dir>` overrides any PNG
  by name. Assets live in `crates/k8sciv-gui/assets/` with `CREDITS.md`;
  both font and sprite bytes are compiled in (binary stays
  self-contained). `ascii()` now only maps a handful of attention glyphs
  (the bundled font covers Unicode punctuation). Sprites use
  `FilterMode::Nearest` for crisp pixel edges. The TUI is untouched —
  this is all `k8sciv-gui`.
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
  fixed rule. Local = everything, full names. Civ-II aesthetic is
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
  structure labels de-conflict too. The user waived strict Civ
  name-below placement ("the Civ convention is satisfied by the shape,
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

## The pair (hot/warm)

`--warm <context>` (or config `warm_context`) attaches a second cluster:

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
| `▪` (civ) / `·` (plain) | world-panel node cell, colored by worst state |
| `┌┐└┘` | world-panel viewport frame (reversed cell = cursor) |
| `▒`   | fog of war (world not yet synced)  |
| `Ψ`   | Service harbor (on the city's east coast, cyan) |
| `∏`   | Ingress gate (on the city's east coast, cyan) |
| `⊞`   | PVC granary (inland/west of the city; cyan bound, yellow pending) |
| `◈`   | Job expedition (namespace island; yellow when failed) |
| `◷`   | CronJob (namespace island; detail = schedule) |

Health precedence on a tile: NotReady > Cordoned > Pressure > Healthy.
Zone headers carry a `▪N` rollup (colored by the zone's worst node) when
any node in the zone is degraded.

**Color discipline:** color encodes meaning, never decoration — and in the
default **civ palette** (2026-06-12, user call), *terrain*: parchment-gold
panel chrome, green for healthy land (tiles, zone headers, calm gauges,
muted-green running pods), white city-name labels, and a blue-ocean WORLD
panel with light-green land cells. Saturated red / bold yellow remain
reserved for attention — trouble pops against terrain, never competes with
it (pinned by a theme test). `color = "plain"` restores the pre-civ
restrained palette (healthy = no color at all); `color = "mono"` carries
all meanings via bold/dim/reverse only. All colors are named ANSI — safe
on 256-color terminals.

## Keymap

`h/j/k/l`+arrows explore · `]`/`[` next/prev city · `PgUp/PgDn` page,
`Ctrl+u/d` half page, `Home/End` west/east continent · `Enter` opens the
region under the cursor · `l` tail the selected pod's logs (city/node) ·
`Esc` back · `m` map ·
`w` workloads · `n` next concern · `a` attention panel · `Tab` focus panel ·
`c` context picker · `1/2/3` overlays (pressure/replicas/namespace) ·
`?` keymap · `q`/Ctrl-C quit. Keep `help.rs` in sync with any change.

## Dev loop

```
make dev        # kind-up + samples + run (the standard loop)
make smoke      # headless: connect, print world summary, exit (CI gate)
make lint test  # fmt --check, clippy -D warnings, cargo test
make kind-down

make gui        # windowed client spike (macroquad) against the dev cluster
make perf-up    # kwok-simulated 100-node / 1000-pod cluster (needs kwokctl)
make perf       # run the TUI against it
make perf-test  # release-mode rebuild+frame budget test (<100ms asserted)
make perf-down
```

Develop against kind only (`hack/kind-config.yaml`, cluster `k8sciv`,
context `kind-k8sciv`). `hack/samples.yaml` provides: healthy `web`
(+Service), crash-looping `crashy`, StatefulSet `db` (+PVCs), DaemonSet
`agent`, `stuck-pvc` which never binds (keeps one Warning in the queue),
and two `Gizmo` customs (CRD in hack/samples-crd.yaml) for projection.
`make run`/`make pair` pass `--project gizmos.example.com`.

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

mutations & the planning-turn diff UI · external services / chaos layers ·
connectivity attention (orphan ingress / harbor with no city) + unmounted-PVC
island granaries + Job-object attention (failed-Job concern) · Job/CronJob
city screens · namespace filtering · mouse
support · pod-level live metrics (node-level done) · minimap horizontal
compression for very wide zone counts (~60+) · zoom levels (compact 1-line
tiles for very large boards) · pair: per-container image diffs, env/config
drift, unified single-board mode ("one continent, sync ghosts") · logs:
the kube log *stream* (we poll the tail), `--previous`, multi-container
picker, grep/filter.
