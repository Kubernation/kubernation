# Changelog

All notable changes to **Kubernation** are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/) and the project uses
[Semantic Versioning](https://semver.org/) — pre-1.0, so `minor` covers new
features/behaviour and `patch` covers fixes/docs/refactors. One workspace
version covers every crate; releases are git tags `vX.Y.Z`.

## [Unreleased]

### Changed
- **The TUI was removed; the windowed (macroquad) client is now the product**
  and is the `kubernation` binary (renamed from `kubernation-gui`; `cargo run` /
  `make run` launch it). Every feature had been built twice; the headless niche
  is k9s's and the 4X metaphor is graphical, so the project consolidated on one
  well-built frontend. The pure data/model core (`kubernation-core`) is
  unchanged. `make smoke` (the CI gate) is now a UI-free core example
  (`examples/smoke.rs`), and `make perf-test` times the core `Models::build`
  rebuild (~1ms at 100 nodes / 1000 pods). Lost: headless/SSH operation and the
  TUI's render snapshot tests (the logic stays tested in core). A follow-up
  caught + fixed two regressions the removal introduced: **file logging was
  restored** (core's `tracing` diagnostics had no subscriber after the TUI took
  it; now written to `~/.local/state/kubernation/kubernation.log`, `RUST_LOG` /
  `--log-level`), and the **log overlay regained scrollback** (`j/k`/`g` scroll,
  `f` follow, wheel) — `s`'s larger history windows were otherwise unreachable.

### Added
- **One key from a concern to its logs (`L`).** The attention queue parks you on
  "the city in trouble"; **`L`** now tails the offending pod's logs directly
  instead of a hunt through the pod list — auto-opening on the *previous*
  container for a crash-looper (its last words). Works from the attention panel
  and the map (both frontends); concerns with no single log-worthy pod (replica
  gaps, nodes, connectivity, events) have no such jump. The offending pod is
  identified by the pure detectors (`Concern.probe`), unit-tested.
- **Logs: severity coloring + a smarter filter (both frontends).** Log lines are
  now tinted by guessed severity — ERROR/FATAL/PANIC red, WARN yellow, DEBUG dim
  — recognised from klog headers (`E0617…`), structured `level=`/`"level":`
  fields, and uppercase plaintext markers; it's a render-only hint, the text is
  never altered. The `/` filter gained **AND of terms** and a leading **`!` to
  exclude** (subtractive triage, e.g. `error !readiness`). The shared logic lives
  in a pure, unit-tested `kubernation-core` `state/logline.rs`.
- **Logs: timestamps + history window.** **`T`** toggles server timestamps (the
  TUI peels them into a dim left gutter; the GUI shows them inline) so you can
  correlate a line with an event; **`s`** cycles how much history to pull —
  **500** lines (default) → **2k** → **since 1h** — so a crash that scrolled off
  the 500-line window is reachable.
- **Logs: crash-loops open on the previous container automatically.** Opening a
  CrashLoopBackOff (or repeatedly-restarting) pod's logs now defaults to the
  *previous* container — its last words before the crash — instead of an empty
  live tail; `p` still toggles. The container is also resolved once per pod and
  cached, so the ~2s poll no longer re-issues a pod GET each time. (Completes the
  Tier-0 log-UX pass; container picker / concern→logs / multi-pod tailing are
  later tiers.)
- **Logs: graceful errors + no overflow.** Asking for the *previous* container
  on a pod that never restarted now shows a one-line "no previous container —
  press p for the live tail" instead of a raw `ApiError(Status{…})` dump (and
  403/404 are classified too). In the GUI, long unbroken lines (a SHA, a verbose
  error) are truncated to the panel width with `…` instead of running off the
  edge (macroquad has no clipping).
- **Resource browser (`:any kind`) — a k9s-style escape hatch, both frontends.**
  Press **`:`** to open a picker of every resource kind the cluster serves
  (built-ins + CRDs, via discovery), pick one, and see a generic table
  (namespace · name · age) of its objects — LISTed on demand (fetch-not-watch).
  Drilling into a row opens the YAML inspector (the same dossier). The **TUI**
  picker filters by typing and refreshes with `r`; the **GUI** is a mouse +
  wheel modal (pick a kind → click a row). **Least-privilege preserved:** a
  Secret's `data`/`stringData` values are **redacted** (keys + byte sizes shown,
  values masked); ConfigMaps and every other kind are shown in full. Discovery
  tolerates a broken/unreachable APIService (one bad aggregated API won't blank
  the browser), lists only kinds the server can LIST, and flags when a kind's
  500-object cap clips the view. Pure, unit-tested core (`kubernation-core`
  `k8s/browse.rs` + `state/inspect::dynamic_yaml`); discovery + list + both
  frontends + Secret redaction verified live on kind (configmaps in full; a
  planted Secret's values redacted). Hardened by a review + an FMEA pass:
  redaction covers `Secret` of any group and fails closed, masks inline
  credential fields on any object, and drops a Secret's annotations; discovery
  and LIST are deadline-bounded (a hung/degraded API can't freeze the UI) and
  report unavailable groups; the browser honors the active namespace filter,
  classifies RBAC/not-found errors, and opens via the `:` character (non-US
  layouts). A performance pass made the GUI table render the visible slice from a
  memoized row cache (was re-deriving all rows every frame), shares the kind list
  by `Arc`, and discovers API groups concurrently.
- **Copy + export for logs and YAML.** In the log overlay and the object
  inspector, **`c`** copies the whole buffer to the system clipboard and **`w`**
  exports it to a file in the working directory (logs → `.log`, YAML → `.yaml`),
  with a toast/flash showing the path. Both frontends copy by piping to the
  platform clipboard tool (`pbcopy` / `wl-copy` / `xclip` / `xsel` / `clip`) —
  the reliable path that actually round-trips into a paste; the GUI falls back to
  macroquad's clipboard and the TUI additionally emits **OSC 52** for SSH
  terminals that support it (the TUI's own terminal text-selection still works as
  before). Export is the always-reliable path (incl. headless / over SSH). A
  small RFC 4648 base64 is bundled for OSC 52 (no new dependency). Real
  per-character drag-selection in the GUI is intentionally **not** built
  (macroquad has no native selectable text); copy-all + export covers the
  copy/paste need.
- **Object inspector — read-only YAML (`y`), a k9s-style "dossier".** Inspect the
  full YAML of a workload, node, or pod in a scrollable modal — the GUI opens it
  with `y` on a city/node window (workload/node) or a pod row's **`yaml`** button;
  the TUI with `y` on a city/node (selected pod, else the workload/node) or a
  workload-list row. The document is serialized from the object already in the
  reflector store (no fetch), with `managedFields` and the last-applied
  annotation stripped for readability. It stays **least-privilege** — only the
  watched kinds are inspectable, so Secrets/ConfigMaps are still never read. Pure,
  unit-tested core (`kubernation-core` `state/inspect.rs`); both frontends.

### Docs
- **Regenerated the GUI screenshots** to reflect the current chrome — the
  dropdown menu bar, the docked right column (WORLD/STATUS/SELECTION), the map
  title cartouche — and added shots for the new map views (overlay), the menu
  bar, and the advisor screens. The README GUI section is updated to match
  (menu bar / map views / advisors), and the now-stale standalone connectivity
  and storage shots were folded into text (the Almanac legend documents the
  marks).

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
- **Clipboard copy now actually works (user-reported).** Both frontends now
  copy by piping to the OS clipboard tool (macOS `pbcopy`, Linux
  `wl-copy`/`xclip`/`xsel`, Windows `clip`) — reliable locally. The GUI's
  windowing-layer clipboard and the TUI's OSC 52 (unsupported by e.g.
  Terminal.app) were silently failing while the toast reported success. OSC 52
  is still emitted in the TUI as a fallback for remote/SSH terminals, and the
  windowing clipboard as a GUI fallback; the toast/flash now reflects which
  path was used.
- **Copy/export review follow-ups (adversarial).** The TUI `c`/`w` keys now
  actually reach the log / inspector views — they were shadowed by the global
  `c` (context picker) and `w` (workloads) bindings, so TUI copy/export was a
  no-op; those two globals now defer to the view on the Logs / Inspect screens.
  The OSC 52 copy flash no longer over-claims success (it says "sent … (OSC 52)"
  and points at `w` to export, since OSC 52 is fire-and-forget and some
  terminals cap or disable it).
- **Inspector: long YAML lines no longer overflow the window** (user-reported).
  macroquad has no scissor, so a long line (a uid, a long label/annotation
  value, an image ref) ran past the right edge; each line is now clipped to the
  window width with a trailing "…". (The TUI already clips via ratatui.)
- **Inspector review follow-ups (2 adversarial findings).** `y` no longer opens
  the inspector while a pod's log overlay is up (it would stack on top and Esc
  would close the hidden overlay first). And the core YAML-strip test now
  actually exercises the stripping — it builds an object carrying
  `managedFields` + a `last-applied-configuration` annotation + a benign
  annotation, and asserts the noise is removed, the benign annotation survives,
  and an annotations block left empty is dropped (the prior assertion was
  vacuous — the fixture never had those fields).
- **Advisor review follow-ups (2 adversarial findings).** Pressing `t` no longer
  opens the End-of-Turn review *underneath* an open advisor — the `t` guard was
  the one modal-suspend site missing an `advisor.is_none()` check (it had the
  Almanac twin but not the advisor), which let two stacked modals share clicks.
  And the Health advisor now reports **terminating** pods in their own row
  (dim/benign) instead of folding them into "pending" (which tinted a normal
  mid-rollout pod as trouble) — matching the rest of the app's pending-vs-
  terminating distinction. Unit test now covers both buckets.
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
