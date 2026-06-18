# Changelog

All notable changes to **Kubernation** are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/) and the project uses
[Semantic Versioning](https://semver.org/) — pre-1.0, so `minor` covers new
features/behaviour and `patch` covers fixes/docs/refactors. One workspace
version covers every crate; releases are git tags `vX.Y.Z`.

## [Unreleased]

### Added
- **Advisor screens (Civ's F1 "Berater").** A new **Advisors** menu opens a
  modal window with three read-only summary tabs of the whole realm: **Health**
  (provinces/nodes by health, citizens/pods by phase, cities/workloads at
  strength), **Storage** (granaries/PVCs bound vs. pending, with the pending
  claims listed), and **Network** (harbors/services + gates/ingresses, plus
  orphan gates and idle harbors). The reports are pure functions of the observed
  world (`kubernation-core` `state/advisor.rs`, unit-tested) and cluster-wide
  (deliberately not scoped by the namespace filter — an advisor reports on the
  whole realm); they complement the attention queue. Tabs switch with clicks or
  1/2/3/←/→; dev flag `--advisor <health|storage|network>`.
- **GUI: a cartographic map title bar.** A centered stone cartouche over the top
  of the board names the realm — "Cluster Map — &lt;context&gt;" in the serif map
  font, with an iso-diamond flourish at each end — and, when a non-default map
  view is active, a dimmed "&lt;view&gt; view" suffix. Classic-4X map labeling
  ("&lt;realm&gt; Landkarte").
- **GUI: a classic-4X dropdown menu bar.** The scattered chrome buttons (the
  `?` almanac toggle, the End-Turn badge, the namespace-filter chip) are
  replaced by a real menu bar — **Game** (switch context · fit · quit),
  **View** (the map overlay), **Orders** (end of turn · discard, with the
  staged count in the title), **World** (namespace filter), **Help** (field
  guide · version) — the iconic menu of the genre, in the carved-stone palette.
  Click a title to open its dropdown; slide across to switch menus; click an
  item or outside to dismiss. An open menu suspends map navigation like the
  other modals. The realm readout (context · platform · counts) moves to the
  right of the bar.
- **GUI: four map overlays (the View menu's "map display").** Beyond the
  default **Terrain (health)** view, the map (and minimap) can recolor every
  province by: **Pressure** — a cpu/mem heat-map (calm green / elevated amber /
  high red, the documented pressure buckets); **Replicas** — the worst workload
  health sited there (full strength green / replica gap amber / down or critical
  red); or **Namespace** — a stable per-namespace hue, a political/territory map
  of which namespace dominates each node. The active non-default view is labeled
  in STATUS so a recolored terrain isn't mistaken for node health. Dev flags
  `--overlay <terrain|pressure|replicas|namespace>` and `--menu <name>` capture
  them headlessly.

### Changed
- **Minimap viewport box: constant-size + drag-to-navigate.** The box now
  sizes purely from the zoom level (it's the play area scaled by the
  minimap-to-main ratio) and only *translates* as you pan — it no longer
  shrinks near the world edge. Click anywhere on the minimap to recenter the
  main view there, or hold and drag to scrub the box around; every spot is
  navigable, open ocean included (the click resolves to the nearest cell).

### Fixed
- **Map-title review follow-ups (2 adversarial findings).** The title cartouche
  is now bounded to the play area — the (serif) title truncates and the box is
  clamped so a long context name or a narrow window can't overdraw the right
  column (only the left edge was clamped before, like the realm readout already
  guards). In pair mode the centered title names the pair generically ("Hot /
  Warm pair") instead of labeling both continents with the hot context (the
  per-side HOT/WARM banners disambiguate each).
- **Menu-bar review follow-ups (4 adversarial findings).** Clicking one menu
  title while another is open now *switches* to it instead of closing the whole
  bar (the toggle keys off the pre-slide open state, not the value the
  slide-across hover just set). **Esc** now dismisses an open dropdown instead of
  falling through to quit the app. The context / namespace pickers opened from a
  menu get a `*_just_opened` guard, so the opening click can't fall through to a
  picker row the same frame (latent under window resize). The right-aligned realm
  readout is truncated/clamped to the space right of the menu bar so a long
  paired/error label can't overdraw the rightmost menu titles on a narrow window.
- **Minimap-nav review follow-ups.** A minimap drag no longer latches if a
  modal is opened mid-drag (the flag is cleared on button-up, outside the
  modal-suspended block), so it can't cause a stray camera jump on the next
  click; and the hover tooltip is suppressed while scrubbing the minimap. The
  remaining bright-cyan chrome text — the harbor/gate tooltip title, the
  all-bound "N PVCs" line, and the pair-sync line — now uses dark stone-legible
  variants (`STONE_STRUCT`, `sync_on_stone`), completing the contrast pass.
- **Bottom-bar / chrome text contrast.** Attention text on the warm-stone
  chrome (the attention strip, the column's STATUS rollup, the tooltip /
  SELECTION lines) used the bright map colors, which washed out on tan — now
  it uses dark, high-contrast stone variants (`severity_on_stone`): deep red
  for critical, dark amber for warning, near-black for info.

### Added
- **Docked right column (GUI) — the classic-4X right panel.** The floating
  minimap was replaced by an always-visible right column (`sidebar.rs`) with
  three stacked sections: **WORLD** (the isometric minimap), **STATUS**
  (context, platform · node/pod counts, the concern rollup `N crit / N warn /
  N info`, the gauge source, and the active namespace filter), and
  **SELECTION** (the clicked-or-hovered tile, reusing the hover tooltip's
  lines — 4X's "moving unit" box). The map fills everything to the column's
  left; the attention strip now spans only that play area. Drill-down modals
  dim the column behind their scrim. Brings the GUI much closer to the Civ-II
  reference interface.

### Changed
- **Isometric minimap (GUI).** The overview minimap was reprojected from a
  top-down chart to the same 2:1 isometric diamond as the main map: landmasses
  are drawn as iso parallelograms (one per province, health-tinted), so the
  chart reads as a scaled-down view of the world you're exploring. The viewport
  indicator is an axis-aligned **rectangle** bounding the visible region (a true
  sheared parallelogram degenerated into a confusing triangle when the view
  clipped a world edge). Click-to-jump un-projects the iso click
  (round-trip-tested). The zoom level-of-detail tiers (World / Regional / Local
  generalization) were already in place.

### Fixed
- **Docked-column interaction polish** (adversarial-review follow-up). Wheel-zoom
  is now gated to the play area, so scrolling with the cursor over the column
  (or chrome/strip) no longer anchors the zoom on a hidden cell and jolts the
  map. The minimap's viewport rectangle now boxes only the un-occluded play
  area (it was overstating the visible region by the column's width). The
  SELECTION panel falls back to its "click a tile" placeholder over open sea
  (instead of a bare header) and stops drawing before it can spill off the
  column bottom on a shrunk window.
- **GUI image editor input hygiene** (adversarial-review follow-up). Opening
  the city window's image field now flushes macroquad's stray char queue (so
  nav keys pressed before opening don't pre-fill the buffer — matching the log
  filter), the editing cursor stays visible for long image strings (the field
  windows to the tail), and clicking a pod row while editing no longer opens
  the log overlay on top of the still-open editor.
- **Namespace filter now scopes pair-drift too** (adversarial-review follow-up).
  `PairSync::build` takes the `NamespaceFilter`, so the hot/warm "pair drift: N
  workloads differ" concern counts only in-scope namespaces instead of leaking
  filtered-out ones. The GUI namespace picker now renders its own title
  ("NAMESPACE FILTER") and hint instead of the context-switcher's, and pressing
  `t` no longer opens the End-of-Turn review on top of an open namespace picker.

### Added
- **Image-set intervention.** The planning turn's last verb: stage a new image
  for a workload's primary container (`kubectl set image`) and commit it through
  the same dry-run/RBAC gate as scale/cordon/restart. The apply uses a
  **strategic** merge patch so the container is matched by name and its other
  fields (ports, env, …) and sibling containers are preserved. **TUI:** `i` on
  the city screen opens an inline image editor (type, Enter stages). **GUI:** a
  click-to-edit image field in the city window. The verb set is now complete:
  Scale / Cordon / Restart / **Image**. Verified the strategic patch
  server-side; core unit-tested (diff from→to, latest-wins).
- **Namespace filtering.** Scope the whole world — cities, the workload list,
  attention, island structures, coast/storage marks — to one or more
  namespaces, while terrain (nodes are cluster-scoped) and node pressure stay
  physical. Applied purely in the derived layer (`Models::build_filtered`); the
  reflectors still watch everything. **TUI:** `N` opens a multi-select picker
  (Space toggles, Enter applies); the active scope shows in the status bar.
  **GUI:** a chrome button (always shown, highlighted when active) opens a
  picker; `--namespace <ns>` launches scoped. Cluster-scoped node concerns are
  always kept; the filter resets on a context switch (namespaces differ).
  Verified live: filtering to `kubernation-demo` drops the control-plane's
  kube-system cities (coredns, local-path-provisioner) while the node terrain +
  9-pod census remain.
- **Log `--previous` + grep/filter.** The log tail gains two controls in both
  frontends: **`p`** toggles tailing the *previously terminated* container
  (`kubectl logs --previous` — the crash-loop's last words), with the tail
  re-fetched on toggle; and **`/`** opens a case-insensitive substring
  **filter** over the fetched lines (live `n/m` match count, `(no lines
  match)` when empty). The filter is client-side over the last 500 lines (no
  refetch); typing into it never triggers global shortcuts. The TUI shows a
  USE-style filter chip on the top border and routes edit keystrokes (incl.
  Esc/Backspace) to the editor; the GUI captures text input and gates `q`/`/`.
  Verified live: `<previous>` shows a crashed container's output, and a
  `process 48` filter narrowed nginx logs to `1/31`. Dev flags
  `--log-previous` / `--log-filter <substr>` (with `--tail`).
- **Pod-level live metrics.** When metrics-server is present, the metrics poll
  now also lists `metrics.k8s.io` PodMetrics (summed across containers) into a
  per-pod usage map, and the **city CITIZENS** and **node GARRISON** pod lists
  show each pod's live cpu/mem (`kubectl top`-style `12m 45Mi`) in both
  frontends — a new **USE** column in the TUI, appended to the row in the GUI.
  Best-effort: a PodMetrics failure leaves usage blank without affecting the
  node gauges. Verified live (GUI city window shows `0m 10Mi` per pod).
- **The planning turn comes to the TUI.** The terminal client gains the full
  staging + commit flow the GUI has: on a **city** screen `+`/`−` stage a scale
  delta and `R` toggles a rolling restart; on a **node** screen `C` stages a
  cordon/uncordon — each shown as a staged delta in the header. **`t`** opens
  the **End of Turn** review (the `plan_diff`), where `x` unstages a row, `D`
  discards the turn, and `c`/`Enter` commits behind a y/n confirm. Commit runs
  through the same all-or-nothing dry-run gate as the GUI. The TUI now has both
  write paths (evict + commit); staging stays preview-only. Keymap/help updated.

### Changed
- **Commit orchestration moved into the one write file.** The dry-run-all →
  apply-all-for-real step (with its per-row result) now lives in
  `k8s::actions::commit_interventions` instead of the GUI's net thread, so both
  frontends share it and the "decide to write for real" logic stays inside the
  single auditable write surface. The GUI's `PlanOutcome`/`PlanRow` are now the
  core `CommitOutcome`/`CommitRow`.

### Added
- **Deeper attention queue: failed Jobs & broken routes.** Three new pure
  detectors (`state/attention.rs`, unit-tested against fixtures):
  - a **failed Job** surfaces as its own concern — Critical when it hit its
    backoff limit (a `Failed` condition), Warning while it's still racking up
    pod failures; a *completed* Job stays quiet. The Job's own failing pods
    **fold under it** (no one-line-per-pod spam), keeping the queue's
    "city in trouble, not 40 alarms" discipline.
  - an **orphan Ingress** (a route whose backend Service doesn't exist) is a
    Warning — a gate to nowhere.
  - a **Service that selects no pods** (a harbor with no city) is an Info;
    headless / external Services (no selector) are skipped, so healthy
    clusters stay silent. Verified live (failed Job + orphan Ingress fire;
    a healthy cluster adds zero false positives).
- **Rolling restart intervention.** A city window's plan controls gain a
  **restart** toggle that stages a `Restart` intervention; committing it stamps
  the workload's pod template with a `kubectl.kubernetes.io/restartedAt`
  annotation (Deployment / StatefulSet / DaemonSet), rolling its pods — exactly
  like `kubectl rollout restart`. Goes through the same dry-run + confirm + RBAC
  commit path as scale/cordon. A workload can stage a restart and a scale at
  once. Verified live (`--plan-go`).
- **Commit the planning turn (apply staged interventions).** The End-of-Turn
  review's Commit is now live (GUI): it applies staged Scale (Deployment /
  StatefulSet `spec.replicas`) and Cordon (Node `spec.unschedulable`) to the
  hot cluster via `actions::apply_intervention`, behind a confirm. Every staged
  change is **server-side dry-run validated first** (which also enforces RBAC),
  so a turn the cluster would reject is blocked before any real write; per-row
  results show in the review. The planning turn is the project's second write
  path (after evict); staging still never writes. Verified live (`--plan-go`).
- **RBAC-aware evict control.** Before offering eviction, both frontends probe
  `delete pods` permission for the pod's namespace via a `SelfSubjectAccessReview`
  (`k8s::actions::can_evict_pod`). In the GUI the per-pod button renders enabled
  (red `evict`), **`locked`** (no permission), or `...` (probe in flight); the
  net thread caches answers per (cluster, namespace) and clears them on context
  switch. In the TUI the check runs on press and refuses with a clear message.
- **TUI eviction.** `e` on the selected pod in the city (citizens) or node
  (garrison) screen raises a red y/n confirm; `y` issues the same real `DELETE`
  as the GUI (`k8s::actions::evict_pod`), reported via a status flash. The TUI
  was previously read-only; this is its first write, gated by the RBAC check
  and the confirm. Keymap/help updated.
- **Bundled logos.** The compass **mark** is the OS window icon and the
  top-bar emblem; the full **KuberNation** scene is the splash on the
  fog-of-war screen. Downsized copies are compiled in (`assets/logo/`).
- **Intro splash.** On launch the GUI now holds the full Kubernation scene for
  a couple of seconds (fade in/out + a slow Ken-Burns zoom) so it's actually
  seen instead of flashing past as the world syncs; any key / click skips it.
  Suppressed for headless `--screenshot` runs (a `--splash` flag holds/captures
  it for demos).
- **Pod eviction — the project's first cluster write.** Hover a pod in a
  city's *citizens* list or a node's *garrison* list and a red **`evict`**
  button appears; it raises a confirm modal, and on confirm the GUI issues a
  real `DELETE` (a managed pod is recreated by its controller; a bare pod is
  gone). All write code is one file, `kubernation-core/src/k8s/actions.rs`
  (`evict_pod`); the GUI queues it through the net thread and shows a result
  toast. This deliberately relaxes the former absolute observe-only guarantee
  to "near observe-only" (one gated, confirmed write); the planning turn
  (scale/cordon) stays preview-only. Dev flags `--evict <substr>` / `--evict-go`
  verify the UI and the write path headlessly.

### Changed
- **Isometric world map (GUI).** The macroquad map was reprojected from a
  top-down rectangular grid to a classic-4X **isometric 2:1 diamond** grid.
  The core world model stays a rectangular `(u16,u16)` grid (both frontends'
  canonical coords) — this is render-only inside `kubernation-gui`. `Camera`'s
  `to_screen` / `cell_at` / `fit` / `fly_to` / `shifted` became the iso
  forward/inverse transforms (integer cell = diamond north vertex,
  `+0.5,+0.5` = center; pan/zoom-anchor math is unchanged); rendering is a
  back-to-front two-pass painter's algorithm so tiles and tall buildings
  overlap correctly. A round-trip unit test pins `cell_at(to_screen(center))`.
- **Softer sea & shore.** The ocean's hard 2-colour checker was replaced with
  a smooth mottle of overlapping faint swell patches (no grid), and coastlines
  now blend through graded **shallows rings** (deep → shallow → beach) drawn
  under the shore instead of a hard inked diamond waterline.
- **All-procedural terrain & settlements.** Terrain is now health-tinted,
  dithered iso diamonds with soft shallows coasts; cities are procedural building
  clusters that grow with population (hut → walled keep), with a solid
  lower-left **population box** and a **serif name banner** below
  (classic-4X city labels). The Kenney "Medieval RTS" sprite set and the
  `--tileset` override were removed — the map is original geometry only.
- **Tan-stone HUD chrome.** The top bar, hover tooltip, attention strip, and
  context picker were retinted from near-black plates to warm carved stone;
  meaning colors (red/yellow attention, cyan structures, sync chips) are
  untouched and read harder against stone.
- **Map labels are now constant screen size** (a `label_scale` clamp) instead
  of scaling with zoom, so they no longer balloon or clip when zoomed in; the
  continent name is pinned on-screen; and a namespace island's structures are
  drawn as a tidy scrim-backed legend list rather than scattered labels that
  overprinted each other. Un-plated labels (continent / province / island) get
  a dark text halo (`text_outline`) so they stay legible over both land and sea.

### Added
- Bundled **Liberation Serif Bold** (OFL 1.1) for the map's place-name banners.

## [0.3.0] — 2026-06-16

### Added
- GUI dev flags `--center <name>` (frame the camera on a city / node / island
  without opening a panel) and `--pan-dx <cells>`, so map screenshots (coast,
  storage, island marks) can be captured deterministically headlessly.

### Changed
- Completed the **Kubernation** rename: the codename is gone from every
  identifier — crate names (`kubernation-core` / `kubernation` /
  `kubernation-gui`), the `kubernation` binary, the kind cluster, kubeconfig
  context, config/log paths, and the sample namespace.
- Neutralized the trademark surface: internal game-name labels and the
  default palette were renamed to generic terms (4X / `atlas`); a single
  nominative attribution homage is kept, and a trademark disclaimer was added
  to the README and `--help`.
- Regenerated all GUI screenshots in `docs/` so the window chrome reads
  **Kubernation** (they predated the rebrand and still showed the codename).

### Removed
- Four stale/unreferenced screenshots (`gui-spike`, `gui-metrics`,
  `gui-labels`, `gui-world-scale`); the spike shot was historical and no
  longer linked.

## [0.2.0] — 2026-06-16

The first managed version: everything built on the 0.1.0 core MVP, and the
point from which the version is bumped per change. Still **observe-only** —
no mutation paths exist.

### Added
- **Live metrics** — node cpu/mem gauges read metrics-server usage when
  present, falling back to scheduling pressure (requests ÷ allocatable).
- **Pod log tailing** — `l` on a city/node screen (TUI) or click a pod row
  (GUI); a polled live tail.
- **Connectivity layer** — Services as `Ψ` harbors and Ingresses as `∏` gates
  on each city's coast.
- **Storage layer** — PVCs as `⊞` granaries inland of the cities that mount
  them.
- **Batch layer** — Jobs as `◈` expeditions and CronJobs as `◷` schedules on
  the namespace islands.
- **GUI window system** + the **Almanac** (in-app field guide; its legend
  cross-references live map examples) and 4X-style **city** and
  **province** drill-down windows.
- **The planning turn (preview-only)** — stage scale/cordon interventions
  from the drill-downs and review their from→to diff at "End of Turn".
  Nothing is applied to the cluster (Commit is shown but disabled).
- GUI cartographic **scale tiers** and **label de-confliction**.

### Changed
- Branded the app **Kubernation** (display name).

## [0.1.0]

Initial MVP: the Kubernetes data layer, the observed-world model, and the
map / city / attention TUI — the cluster as a living world.
