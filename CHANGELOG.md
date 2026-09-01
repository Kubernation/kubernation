# Changelog

All notable changes to **KuberNation** are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/) and the project uses
[Semantic Versioning](https://semver.org/) — `major` covers breaking changes,
`minor` new features/behaviour, and `patch` fixes/docs/refactors. One workspace
version covers every crate; releases are git tags `vX.Y.Z`.

## [Unreleased]

### Changed
- **The Advisors screens no longer rebuild their report on every frame.** Each
  tab recomputed its whole analysis sixty times a second while open — about a
  quarter of a frame's budget on a large cluster, spent recalculating an answer
  that had not changed. They are now computed once per refresh, which is what the
  Posture and Cost tabs already did. The reports themselves are unchanged; they
  are simply at most one refresh (a quarter-second) behind, as those two always
  have been.

### Changed
- **Right-sizing advice now comes from what a pod has actually been using, not
  from one instant.** The advisor recommends CPU and memory requests — numbers
  you act on by editing a manifest — and it derived them from a single
  metrics-server reading, with "peak" meaning the busiest replica at that one
  moment. It now keeps a short history per pod and uses the 90th percentile of
  each pod's own usage, so a workload that idles and spikes is no longer sized
  from whichever second the poll happened to land on.
- **And the advisor says which reading it used.** A recommendation from a
  two-minute percentile and one from a single sample deserve different trust, so
  the footer names the window (`P90 of each pod's usage over the last 40 samples
  (~10 min)`) and counts any rows too new to have one. Until a pod has about two
  minutes of history it stays on the single-sample basis and says so, rather than
  calling three readings a percentile.

## [1.34.1] — 2026-08-29

### Changed
- **The build's lockfile check now actually checks.** It was meant to catch a
  version bump that had not been written into `Cargo.lock`, but an earlier step
  in the same job quietly corrected the file first, so the check always saw a
  good one — while the release build, which starts from a clean copy, would have
  failed. Every step now uses the strict flag, and the stale lockfile that
  exposed this is fixed.

- The third-party licence check now pins the tool it runs, so it reports a
  dependency appearing, vanishing or changing terms — and not a cosmetic
  reformatting from a new release of the checker itself.

### Fixed

- **The Oracle said "streaming" before the model had produced anything.** While
  waiting for a local model's first token — 10 to 30 seconds on a large one — the
  consult window read `streaming… 0s · 0 chars` and dropped the countdown, which
  is exactly the wait that the timeout governs. It now says `consulting the
  Oracle… 0s (timeout 600s)` with the "local models can take a while" hint until
  a token actually arrives.
- **The attention queue's "×N" now says what it counts, and it was understating
  trouble.** A concern reading `restarting repeatedly ×1` meant *one pod that has
  restarted at least five times*, not one restart — while two lines below it in
  the same list, `events: ProvisioningFailed ×522` meant 522 occurrences. The
  count now carries its unit (`×1 pod`, `×3 pods`) where it aggregates, and is
  omitted on a concern about a single named pod, where it only repeated the
  title.
- **The cost panel now says which kind of idle it means.** "Idle" is capacity
  nobody *reserved* when there is no metrics-server, and capacity nobody is
  *using* when there is — on a busy node those are nearly opposite numbers, and
  the panel named the basis in one case and stayed silent in the other. It now
  always says (`idle 100% unused`), from the same source of words the Cost
  advisor uses.
- **The Windows download's licence notices were missing six of its own
  dependencies.** `THIRD-PARTY-NOTICES.md` ships inside every release archive,
  but it was generated for macOS and Linux only — so the file in the Windows zip
  omitted the crates that only the Windows build links, among them `schannel`,
  the Windows TLS implementation. Six releases were affected. The notices now
  cover every platform the release builds, and a check in CI fails if the two
  lists ever drift apart again.
- **Corrected the description of the Oracle's network code**, which said it made
  a single non-streaming request. It has streamed replies token by token since
  v0.59.0, and also probes the endpoint and lists available models. Documentation
  only — no behaviour changed — but it is the file whose description matters most
  for understanding what leaves your laptop.
- **The field guide was naming eight of the nine map overlays.** The Pool
  (nodepools) view has existed since v1.14.0 and the Controls page never
  mentioned it — the one surface whose job is to tell you what the menus contain
  was the surface that did not know. The list is now built from the overlays
  themselves, so it cannot fall behind again.
- **Corrected a set of stale descriptions** that no longer matched what the app
  does: the write-surface documentation still called evicting a pod a delete
  (it goes through the eviction subresource, so disruption budgets are enforced),
  listed two planning-turn verbs where there are five, and referred to two
  frontends where there has been one since the terminal client was removed. The
  evict button's own notes named the wrong permission. None of this changed
  behaviour; all of it would have misled someone reading to understand what the
  app is allowed to do.

## [1.32.0] — 2026-08-28

### Added

- **A cordoned node now tells you what would refuse to let it drain.** Its entry
  in the attention queue — and so in the sidebar, the Oracle's context and the
  after-action report — names the disruption budget standing in the way:
  *"draining blocked by kubernation-demo/web-strict"*. Deliberately only for a
  cordoned node: a budget written to allow no disruptions at all is a permanent
  state on a healthy cluster, and a standing alarm on every node running that
  workload would be noise rather than news. The province window and the selection
  panel report it for any node, whatever map view you have open.

- **KuberNation now knows which nodes a PodDisruptionBudget would refuse to give
  up.** Budgets are watched like every other resource, and the app works out, per
  node, whether draining it would currently be blocked and by which budget. The
  reading is deliberately careful in three ways: a budget the cluster has not
  finished reconciling is reported as *unknown headroom* rather than as its stale
  number; a budget with a null selector covers nothing (which is the opposite of
  how the same field reads on a NetworkPolicy); and if the app is not permitted to
  read budgets at all, every node says so — "budgets not read" — instead of
  claiming to be drainable. Read-only; nothing new is written to the cluster.

- **Following an Oracle "consult next" link now marks that object on the map**,
  so when you close the Oracle the selection panel is describing the thing you
  just drilled into. The camera deliberately stays put: drilling into a suspect
  is speculative and you often come back, and a mark is replaced by the next one
  while a camera position is not.
- **The field guide says which cross-references it can mark.** Clicking a legend
  entry flies you to a live example; harbours, gates and island structures are
  not things the selection can name, so those entries now read "flies, no
  marker" instead of leaving the selection panel describing something else.
- **The workload list and the map now point at the same thing.** The row you
  have selected on the map is marked in the Workloads table, and picking a row
  there marks it on the map — without moving the camera, so you keep your place.
  Rows the map has nowhere to put say so: a DaemonSet is drawn as a road across
  the provinces it runs on, never a settlement, so its row reads "road - not a
  settlement" instead of quietly failing to highlight.

- **Nodepools are now visible on the map** — *View ▸ Pool (nodepools)* tints each
  province by the pool that holds it, and the panel names it; a province's own
  window shows its pool beside its zone whichever view you are in. Until now the
  map could show you that a whole band of the fleet had been replaced without
  being able to say *which pool* — the grouping an operator actually acts on.

  Each region is **named on the map** — a region being one pool within one zone.
  Nodes are placed in allocation order, so pools interleave and a region is
  usually in several pieces (3 of 8 on the churn fleet). **Every piece carries
  the name**, because the largest piece can hold as little as 40% of a region
  and naming only that one leaves the rest anonymous at any zoom close enough to
  fill the screen with a different piece. Repetition does not read as several
  pools: the shared fill colour is what carries identity.

  Names **follow the view** down their own ground, the way the graticule's
  column letters do, so panning across a region — or zooming into one — keeps
  its name on screen instead of leaving it at a midpoint that has scrolled off.
  A name never leaves the ground it names to chase the view; if its region is
  off-frame it is simply not drawn, rather than pinned to an edge where it would
  appear to label whatever is there.

  A **POOLS legend** in the right column names each colour, largest pool first,
  with how many nodes and how many zones it spans — the colours are generated
  from the pool name, so they tell you two provinces belong together and only the
  legend tells you what they belong to.

  A pool spanning three zones deliberately reads as three regions rather than one
  shape: zone stays primary because zone is the failure domain, so a zone outage
  takes exactly one of them — which is why the legend shows the span. A node with
  no pool label recedes instead of being given a colour, and is listed as "no pool
  label" rather than named, since it is not a member of anything.

- **A position on the map can now be named.** *View ▸ Reference frame* (or
  `--graticule`) draws a letter per zone and a number per slot, so a province is
  "C4" — something you can put in a ticket, say on a call, or write on a
  screenshot. Select a province and the panel shows its reference. A reference
  names a **slot**, not a spot on screen, so it survives a rolling refresh, a
  restart, and the node being replaced.
- The map states what the frame is anchored to, on the map itself rather than
  only in the Almanac — a screenshot travels without the Almanac. Columns are
  zones in the order first observed, rows are slot ordinals, and **position
  asserts nothing about the cluster**: neighbouring provinces are unrelated, and
  zone is the only grouping that means anything real.
- A zone whose nodes have all departed keeps its letter and its ground, and the
  map now shows that letter over the empty column instead of leaving an
  unexplained gap in the sea.
- **Ground that just changed hands is marked on the map**, fading back to
  ordinary terrain over an ageing window. A rolling node refresh now reads as a
  wave crossing the fleet instead of an invisible substitution — the provinces
  hold still (that is what the last several releases bought), so the one thing
  that *did* change is the thing you can see. Select a marked province and the
  panel says how recently.
- **View ▸ AGEING WINDOW** sets how long that lasts — off, 5 minutes, 15
  minutes, 1 hour, or 4 hours — and is remembered between runs. `--fresh-minutes`
  takes any value. Match it to your refresh cadence: much longer than a refresh
  takes and everything stays marked at once, much shorter and the wave has passed
  before you look. The default is 1 hour.
- The Almanac's World page now explains both kinds of unusual ground — the new
  ochre and the grey left where a node departed, which had gone undocumented
  since it was introduced.
- **Reclaiming empty ground now appears in the Annals**, alongside the other
  changes KuberNation itself made this session, rather than only as a toast that
  vanishes when you look away.


- **A province says when its size was a guess.** A node that reports no
  allocatable memory is still drawn at some size, and until now nothing
  distinguished that from a measurement — or an instance-type guess from a
  pure default. The province window and the SELECTION box now say
  "size from instance type - not measured" or "size not reported - default
  extent", alongside the existing pool and gauge-source markings. Deliberately
  words rather than the no-data hatching: the hatch means "this reading has no
  denominator" and is gated to the ratio overlays, whereas size is drawn under
  every overlay, so hatching it would put two unrelated meanings on one texture.


- **A concern now says when a workload's failures are confined to one
  nodepool** — `... CrashLoopBackOff ×29 · all 29 on pool sys`. A bad node image
  rolled to a single nodepool is a common incident and was, until now, unnamed
  on every surface: the map scatters it (nodes are laid out by zone, so one
  pool's provinces interleave with the rest) and the queue aggregates it by
  workload, which is right but says only how many, never where.

  It stays silent unless the claim is real: one failing pod is trivially in one
  pool, a fleet with only one pool makes the sentence vacuous, an unresolvable
  pool label is an absence rather than a group, and pods that were never
  scheduled have no pool at all — those are excluded and change the wording to
  "all N placed on pool X" rather than being counted in.

### Changed

- **Evicting a pod now respects PodDisruptionBudgets, and says so when one
  refuses.** The evict button — and the Game Day drill's pod kills, which share
  the same primitive — went through a plain delete, which the apiserver does not
  check a budget against. So a workload declaring "keep at least three of us
  running" could be taken to two from inside the app, silently, by the one
  control whose whole purpose is to disturb a running workload. Eviction now
  goes through the eviction subresource, the way `kubectl drain` does, so the
  apiserver enforces the budget. A refusal is reported as a refusal rather than
  an error, naming the budget in the apiserver's own words:
  `web-f56f55fb4-scp2w is protected - The disruption budget web-strict needs 3
  healthy pods and has 3 currently`. A chaos drill whose eviction is blocked
  continues and reports that step as refused, rather than stopping half-drained.

- **A development diagnostic no longer calls the plurality node "the node".**
  `--dump-positions` labelled each city's province `node`, which reads as where
  the workload runs; it is where the city is *drawn*, often on a node holding a
  few percent of the pods. It is now `plurality_node`. Nothing user-facing.
- **The map stopped drawing sea over ground it has reserved.** Every node's plot
  is the same size so that adding or replacing a node never shifts the world
  around it, but most nodes are smaller than that — and the unused part of each
  plot was being drawn as open water. About a quarter of the world was ocean
  that no ship could ever sail. It now shows as bare reserved ground, so a
  continent reads as land with structure rather than stripes in a sea, and each
  node's own size is still legible: the built part is green and, in the relief
  style, stands proudly above its unbuilt remainder.

- **What you have selected now survives the cluster changing under you.** The
  map remembered a *place*, so a workload rescheduled to another node left the
  selection quietly pointing at whatever was on the old ground — and, in a
  hot/warm pair, adding a zone to the hot cluster shifted every warm selection
  onto a different cluster's node entirely. It now remembers *what* you picked
  and works out where that is each frame. Measured on a live cluster: a
  workload moved across continents and the selection followed it; a warm
  selection stayed put while the cell it used to occupy was swallowed by a
  newly added hot zone.
- **A selected workload that gets deleted says so** instead of disappearing or
  leaving a mark on ground that is no longer its own.
- **Clicking water no longer selects a node.** Where the shoreline cut into a
  province's rectangle, a click could give you a selection the tooltip called
  ocean and the blast radius called a node.
- **The map's selection decisions are now covered by tests.** Which node or
  workload the blast radius points at, which one the Oracle will consult, and
  which window an impact row opens were all decided in the one file the test
  suite cannot reach — so a wrong answer there could only be found by using the
  app. They moved somewhere testable, and five deliberate breakages of them now
  fail the build. Nothing about how they behave changed.
- **One home for turning a map cell into the thing it names.** Two places
  converted a selected cell into a workload-or-node identity with the same ten
  lines; they now share `subject_at`. The test written to pin it against the
  richer panel resolver found a divergence nobody had named: a cell inside a
  province's rectangle that the shoreline carving turned to water is *ocean* to
  the tooltip and *a node* to the blast radius. Recorded, not yet fixed — the
  selection rework that dissolves it did not clear its gate.

- **Ground that changed hands is marked by one setting with two modes** —
  *View ▸ NEW GROUND*. **Fading** lets go, decaying over a window, which is what
  makes a rolling refresh read as a wave crossing the fleet. **Since** holds
  instead: pick a moment, and every change of hands from then on stays marked
  until you move it. Fading answers *how recently*; since answers *whether, from
  a point you fixed*. Use the second when an investigation outlasts the window.
  `--changed-since N` selects since, `--fresh-minutes N` fading; a baseline
  belongs to the session you set it in and is not remembered between runs.

  This replaces a separate "changed" map view built earlier in the same
  unreleased cycle. On our own evaluation it marked the same ground the ageing
  marks already did — the only thing it added was reach, and reach is a setting
  on the window too. One fact now has one colour, one setting and one panel line
  instead of two of each.


- `ExtentInput::Capacity` and `ExtentSource::Capacity` are now **`Allocatable`**,
  because that is the field they read. `status.capacity` is a different
  Kubernetes field, reports a different number, and is never consulted — a
  mismatch that had already misled one design proposal. This changes the
  `extent_source` value emitted by the `--dump-positions` developer flag from
  `"Capacity"` to `"Allocatable"`; nothing persisted is affected.


- **The map stays visible while you read a city or a province.** Both drill-down
  windows used to open centred, at up to 1100px wide with a dimming wash over
  everything else — which covered essentially the whole map, so the overview you
  clicked from vanished the moment you drilled into it.

  They now dock beside the map rather than over it, and the camera moves once, on
  open, so the province you clicked stays in the visible strip rather than
  sliding under the panel. No dimming wash: the point is to keep the map
  readable. The other twelve windows — the Almanac, About, the Charter and so on
  — are unchanged, because they are not about a place on the map.

  Pod rows now fit their column rather than a fixed character count, so the
  narrower panel shortens a pod's name-hash instead of running its text under the
  hover buttons. Every field on the row survives.

### Removed

- `WorldModel::province_index_at` and `WorldModel::visible_provinces` — dead
  since the TUI was removed, and both returned a vector index documented as a
  map row. A false contract on an unused public helper is a trap for whoever
  calls it next.

### Fixed

- **A Game Day drill the cluster refused no longer reports that the workload
  shrugged it off.** Now that evicting a pod respects disruption budgets, a drill
  can be turned down before it does anything — and the scorecard was still
  reading "stayed up — no outage", which sounds like resilience the drill never
  actually tested. It now says *"no disruption landed — every step was refused"*,
  or, when only some steps were refused, how much smaller the experiment was than
  the one you asked for.
- **The evict button was asking the cluster for the wrong permission.** Since the
  previous release evicting a pod goes through the eviction subresource, which
  Kubernetes authorizes under its own verb — but the permission check still asked
  about deleting pods. Those are separately grantable, so the button could be
  greyed out for someone who is in fact allowed to evict, or offered to someone
  who is not and would hit a refusal. Both directions were reproduced on a real
  cluster. The check, the Game Day drill's pre-flight, and the in-app Charter now
  name the verb the app actually uses.

- **The field guide's "node" and "road" cross-references now land on the
  province they name**, instead of on the water just west of it.
- **Jumping to a node that needs attention no longer lands you in the sea.**
  Pressing `N` to park on a node concern put the marker on open water just west
  of the province and left the SELECTION panel blank, because the position it
  jumped to was computed without reference to the coastline the map actually
  draws. The blast radius drew its ring there too. Every province measured was
  affected. The map now works out where a node's land is the same way it works
  out what you are pointing at, so the two cannot disagree.

- **"Fit view" now fits into the part of the map you can actually see.** It
  framed the world against the whole window, so on a large cluster a sixth of
  the map landed behind the docked right column and the northern edge was
  clipped under the menu bar. It now frames into the map area. On a fleet big
  enough to hit the zoom floor the world still will not all fit at once — no
  framing can make an arbitrary number of nodes readable together — but nothing
  is hidden behind the chrome any more.

- **Asking the Oracle to "widen to node" now says how much of the workload is
  actually on that node.** It folded one node's health and strain into a
  workload's consult with nothing saying it was one node of sixty-five — and the
  node it picked was an arbitrary pod's, not even the one the city is drawn on.
  A model reading that would reasonably blame the workload's trouble on a node
  running two of its hundred and twenty pods, and on a remote endpoint that
  inference leaves your laptop. The section now opens with the share.
- **The map no longer implies a workload runs on one node.** A city is drawn on
  the node holding the most of its pods, which for a spread workload is a small
  minority — one 120-pod workload's city sat on a node running five of them —
  and selecting it went on to show that node's strain, cost and pool as though
  they were the workload's. The panel now states the real footprint ("120 pods
  across 65 nodes") and says the readings beside it belong to the province.
- **The field guide said a city sits where *most* of its pods are.** It does
  not, and the same document said so correctly two pages later. Both now come
  from one sentence, and it explains that a city marks where a workload is
  drawn rather than where it runs.

- **A deploy and the failure it caused in the same second are now connected.**
  Kubernetes timestamps only go down to the second and a kubelet rejects a bad
  image immediately, so the two routinely land together — and the rule required a
  gap of at least one second, silently dropping the most common incident there
  is. Measured across four induced rollouts, half sat at exactly zero. A rollout
  or an action you took is now treated as a precursor even at the same instant; a
  passively-observed change is not, since at the same instant it may just as
  easily be the failure's *consequence*.
- **A repeating change is now correlated from when it started**, not from the last
  time it was seen. A scaling event that had been recurring since before a failure
  could be measured from its most recent repeat and land on the wrong side of it.
- **"Trouble begins here" now marks when the incident began**, not when the app
  last saw the oldest long-running failure. If anything in view had been failing
  for hours or days, it kept refreshing its own timestamp and stole the marker —
  so on a cluster with a crash-looping pod anywhere, the Annals drew the line in
  the wrong place and silently dropped the "(before the failure)" cue that is the
  section's whole point. A deploy that broke something now gets flagged even
  while unrelated chronic noise is present. Affected the realm feed and a city's
  or province's own feed alike.
- The "(before the failure)" cue is no longer truncated away before you can read
  it. It used to be appended and the row then cut to fit, so it was the first
  thing lost — and a city row is capped at 30 characters when it shares space
  with a rollback button, which an entry title alone exceeds. It was therefore
  effectively invisible in the city and province panels, which is where you look
  after the map sends you somewhere. The wording is also one string now; two of
  the three places that drew it had drifted apart.


- **Three false ordering contracts in `state/world.rs`** — an audit prompted by
  the v1.17.0 region-label defect found the same mistake in three more places.
  `WorldModel::cities()` promised "west→east, north→south"; both halves were
  true until A2 moved rows to layout ordinals and continent x to durable
  first-observed ordinals while the vectors kept their old sort keys
  (alphabetical, and `fnv1a64(name)`). Verified: with `z-m` observed before
  `z-a`, the continents vector is `[(z-a, x=30), (z-m, x=0)]` — the first entry
  is the eastern one. The order is still deterministic, so `]` / `[` cycles
  every city exactly once; it is just not geographic. `province_index_at` and
  `visible_provinces` return vector indices documented as rows and have had no
  callers since the TUI was removed. Documentation only — no behaviour change.
  Full audit in `docs/reports/region-label-ordering.md`.


- **Terrain is painted back to front.** Under the Relief map style a raised tile
  extends about 7px north of its own ground, so a southern band has to paint
  over its northern neighbour. Land and ghost ground were painted in the order
  the model happened to store them (name hash, and pool-then-slot) and in two
  separate passes, which put every patch of ghost ground behind every province
  regardless of where it sat. Both now sort together into one back-to-front
  sequence. No visible change today — bands only touch at the largest size
  class, which no test cluster reaches — which is precisely why the order is now
  asserted by test rather than left to a fleet that happens not to show it.


- **The map says when it has no record of a change, instead of staying quiet.**
  Under a fixed baseline ("new ground since …"), an unmarked province could mean
  two different things — it genuinely did not change, or nothing was ever
  recorded for it — and the panel said neither. It now distinguishes "unchanged
  since the baseline" from "no succession on record", because an absent record
  is not evidence that nothing happened. The rolling-window mode is unaffected:
  there, an absent record honestly means "not recently new".

  The map's *colour* deliberately still treats both as unmarked. Marking them
  would tint 82% of a churned fleet — and every province of one that has never
  churned — under every overlay at once, and would reuse the texture that
  already means "this reading has no denominator".


- **A node at a nominal size boundary now gets the province size its machine
  implies.** The size classes are written in nominal memory sizes (32, 128,
  512 GiB) but compared against what the node *reports*, which is always lower —
  firmware and reserved RAM, plus any kubelet reservation. So a machine sold as
  32 GiB reports about 30.9, missed the threshold, and was drawn one class too
  small; the same at every boundary. A named `EXTENT_HEADROOM` now scales the
  reported figure before the comparison, so the thresholds stay readable as
  machine sizes. Genuinely in-between machines — 24, 96 and 384 GiB are all real
  instance sizes — are unaffected, and there are tests in both directions.

  No visible change on kind or on the kwok test fleet, which report exact round
  numbers; this only moves on real cloud nodes.

### Documentation

- Corrected the churn-fleet region-fragmentation figure from "4 of 8" to
  **3 of 8** wherever it appeared (8 regions in 12 pieces; the difference counts
  extra pieces, not fragmented regions). The per-zone data and the design
  conclusion it supported are unchanged. Re-derived by the new
  `hack/churn/pieces.py`, which now emits the fleet figure rather than leaving it
  to be read off a breakdown.
- Corrected T1's per-column run diagram: one of four columns is two pieces, not
  one, because the original script collapsed vacated slots out instead of letting
  them break a run. Full account in `docs/reports/t1-shape-rederivation.md`.


- Corrected the churn-fleet region-fragmentation figure from "4 of 8" to
  **3 of 8** wherever it appeared (8 regions in 12 pieces; the difference counts
  extra pieces, not fragmented regions). The per-zone data and the design
  conclusion it supported are unchanged. Re-derived by the new
  `hack/churn/pieces.py`, which now emits the fleet figure rather than leaving it
  to be read off a breakdown.
- Corrected T1's per-column run diagram: one of four columns is two pieces, not
  one, because the original script collapsed vacated slots out instead of letting
  them break a run. Full account in `docs/reports/t1-shape-rederivation.md`.


- Measured which dimension pod failures actually cluster in, before scoping a
  feature around the assumption that they cluster by location. On the dev
  cluster they cluster by **workload** in every failure shape that could be
  induced, and scatter evenly across nodes; unschedulable pods have no node at
  all, so no map can place them; and a node going down produced no failing pods
  whatsoever — that signal lives in the node's own condition, which the map
  already shows. Report in `docs/reports/t2-pre-failure-clustering.md`; no
  product change.


- Closed the measurement gap the previous round flagged: whether a failure
  confined to a single nodepool reads as a shape on the map. It does not — 100%
  of one nodepool renders as eight disconnected pieces across three columns, and
  in the case measured it produced no trouble marking on the map at all. Neither
  the map nor the attention queue currently says "these failures are confined to
  one pool", which is the actual gap and is a sentence rather than a map feature.
  Report in `docs/reports/t2-pre-pool-gap.md`; no product change.

## [1.9.0] — 2026-08-03

The first release since 1.6.0. It carries the work versioned 1.7.0, 1.7.1, 1.7.2
and 1.8.0, none of which was tagged — so the map fixes below reach users for the
first time here. **If your cluster has more than one nodepool in a zone, the
1.7.0 fix matters: nodes were being drawn on top of each other.**

### Added
- **The map is remembered between sessions.** Where each node sits is saved per
  context and restored when you reopen, so the shape you learned yesterday is
  the shape you get today — including after nodes were replaced while the app
  was closed. Measured on a hundred-node fleet across a thirty-node refresh:
  every surviving node kept its exact ground, where without the saved map
  fifteen of them moved.
- **Game ▸ Reclaim empty ground** releases the slots left by nodes that have
  gone for good, for when you have decommissioned something and want the map to
  show it. Explicit and reported — it is never done automatically, because
  ground held for a node that may return is the map working rather than clutter,
  and occupied ground never moves when it runs.
- If a saved map cannot be used — it was written by a newer version, or the
  context now points at a different cluster — the map starts fresh and **says
  so** rather than quietly changing shape. A cluster whose identity cannot be
  confirmed still loads, and says that too.

## [1.8.0] — 2026-08-03

### Fixed
- **A workload no longer shifts its neighbours on the map.** Deploying something
  used to move every other workload already sited on that node by a row, and
  deleting it moved them all back — because a city's row came from its position
  in an alphabetical list of its neighbours rather than from anything about
  itself. Cities now sit where their own identity puts them, so an unrelated
  deployment leaves them where you last saw them.

## [1.7.2] — 2026-08-03

### Development
- `--dump-positions <PATH>` records what the model *assigned* — every city and
  province, per tick, as JSON-lines. A screenshot shows what was rendered, and
  the two differ: permuting uniformly-coloured provinces changes almost no
  pixels, so a pixel diff reported ~1% for a layout that had moved 27% of its
  provinces. A pure read of the existing world model; changes no behaviour.

## [1.7.1] — 2026-08-02

### Fixed
- Two workloads sharing a row on one node could moor their Service and Ingress
  markers on the same cell offshore, so the anchor drawn there belonged to one
  workload and clicking it opened another.

## [1.7.0] — 2026-08-02

### Added
- **The map holds still when nodes are replaced.** Each node now has a durable
  place on the map, so a rolling node refresh no longer rearranges the world:
  provinces keep their ground across a replacement, and a zone keeps its
  continent position even as zones come and go. Measured across a full 30-node
  refresh on a 100-node fleet, the land and sea change shape by 0.4%.
- **A province's size comes from the node's capacity**, not from how many
  workloads happen to be running on it — so deploying something no longer
  resizes terrain. A node that reports no capacity gets a declared default size
  rather than being drawn as a small one.
- **Ground held by a departed node stays on the map**, painted plain. The slot
  is reserved for a successor; without this a node replacement read as the
  continent losing a piece of itself.
- Nodes carry their **pool** and how it was inferred (a provider's own label, an
  instance type, or the default), because there is no standard nodepool key and
  a guess should not be mistaken for an answer.

### Fixed
- **Nodes in different pools within one zone were drawn on top of each other**,
  so on a cluster with several nodepools in a zone a large share of nodes were
  invisible and unclickable, and clicking one province could open another node's
  detail. Introduced with the map layout change above and fixed before release.
- Long-named workloads sharing a node could be stacked on the same cell, leaving
  one of them with no clickable position anywhere on the map. A settlement's own
  cell now always resolves to that settlement.
- Settlements could be drawn in open water, and stretches of a continent could
  hit-test as ocean, on any continent past the first few provinces.
- The zone label and coastline could drift away from the land they belong to,
  and land could fall outside the navigable world — painted, but impossible to
  hover, click or frame with `F`.
- A node departing no longer re-shapes the coastline of every other province in
  its zone.

### Development
- `--shot-seq N` / `--shot-interval S` capture a numbered sequence of
  screenshots from one session. A screenshot per process starts each frame from
  an empty layout, which makes the layout carry structurally invisible — the
  first stability flipbook could not have detected its own absence.

## [1.6.0] — 2026-08-02

Includes the work previously versioned 1.2.0, 1.3.1, 1.4.0 and 1.5.0, none of
which was ever tagged for release — so all of it reaches users for the first time
here.

### Fixed
- **A node that does not report its capacity no longer reads as an idle node.**
  `node_request_ratios` and `node_usage_ratios` fabricated a `0.0` ratio when
  `status.allocatable` was absent, so a node whose load is *unknown* rendered
  identically to one that is genuinely empty — cpu 0% / mem 0% gauges, calm
  terrain, no concern. Both helpers now return `Option` **per resource** (cpu
  and memory are separate keys; one can be present without the other), and the
  absence is carried through to the map: such a province is **hatched** — the
  cartographic idiom for *no data* — rather than tinted somewhere on the
  pressure ramp, its gauges read "unknown" instead of 0%, its strain reads
  "unknown" instead of "calm", and it raises an Info concern saying its capacity
  was not reported. The one caller that legitimately defaults (cost allocation,
  which feeds an immediate no-capacity guard) now says so inline.

### Added
- **Churn harness (`hack/churn/`) — a reproducible 100-node fleet that can be
  made to churn on command.** Test asset only; no production code, no
  user-visible change. Six scenarios (rolling refresh, scale up/down, nodepool
  add/remove, zone loss) plus a capture helper, so the map's behaviour under
  fleet-scale churn can actually be evaluated — the 4-node dev cluster cannot
  exercise it. The rolling refresh *surges*: replacements come up Ready before
  their predecessors drain, verified at a peak of 115 nodes during a 15-node
  wave. `make churn-up` / `churn-capture` / `churn-refresh` / `churn-reset` /
  `churn-down`.
- **Per-pod resource facts in the map model (enabling change, no user-visible
  behaviour).** `PodGlyph` now carries literal requests, limits, optional live
  usage, QoS class and a container count; `NodeTile` carries request-ratio and
  usage-ratio *separately* rather than one number whose meaning changed with
  `metric_source`. That polymorphism made a requests-versus-usage comparison —
  waste on one diagonal, OOM risk on the other — literally inexpressible. The
  existing `cpu_ratio` / `mem_ratio` are unchanged and still derived the same
  way, so nothing that reads them today behaves differently. QoS moved to a
  shared `state::qos` so the map and the right-sizing advisor cannot drift.
- **Substrate map overlay — which nodes are missing infrastructure the rest of
  the fleet has.** An eighth **View ▸ Substrate (daemonsets)** overlay. A node
  quietly missing its log agent or CNI looks perfectly healthy — its own pods
  are fine — while lacking something every other node runs. Fully-covered
  provinces recede to plain land so only the gaps show: amber for one missing
  DaemonSet, red for two or more, and STATUS carries the headline
  (`N/M nodes with gaps`). "Expected" is inferred from **prevalence** — a
  DaemonSet on ≥80% of nodes is treated as fleet-wide — because the map never
  reads a DaemonSet's spec and so cannot distinguish "should be here and isn't"
  from "correctly excluded by a `nodeSelector` you wrote deliberately"; the
  Almanac states that plainly, along with the consequence that a cluster of four
  nodes or fewer can report *nothing* (80% of 4 rounds to 4, so being fleet-wide
  and having a gap are mutually exclusive). With no fleet-wide DaemonSet at all
  the overlay falls back to terrain rather than paint an all-clear it hasn't
  earned.
- **The province window's SUBSTRATE section now names what a node LACKS**, not
  just what it has — `2 missing vs the fleet · log-agent · node-exporter`. Shown
  regardless of the active overlay: once you have drilled into a node, its
  missing infrastructure is a fact worth having without turning on the right
  view. The Substrate overlay's SELECTION line is the at-a-glance twin.

### Fixed
- **Same-named DaemonSets in different namespaces are no longer merged.**
  Coverage keyed on the bare name, so `monitoring/agent` and `tenant-a/agent`
  counted as one — which both *hid* real gaps (a node lacking the fleet's agent
  read "complete" because an unrelated tenant DaemonSet of the same name ran
  there) and *invented* them (two half-covered sets summing past the 80% bar).
  Identity is now `namespace/name`, which is also what you need to go act on it;
  the province window's DaemonSet inventory is qualified to match.
- **A pod outliving its node no longer inflates coverage.** The prevalence
  numerator counted pods by `spec.nodeName` while the denominator was the node
  store, so a pod left behind by a scaled-down node (pod GC runs on a delay)
  could push a DaemonSet over the fleet-wide bar and fabricate gaps on live
  nodes.
- **The province window distinguishes "covered" from "nothing fleet-wide to
  compare against".** Rendering both as silence claimed a check that never ran.
- **The map-view label in STATUS no longer overlaps the memory sparkline.** The
  sparkline rows hung below the layout cursor while every text line in the
  section treats it as a baseline, so the next line landed 11px into the mem
  trace (visible under any non-default overlay, on any cluster reporting
  metrics).

## [1.4.0] — 2026-07-30

### Added
- **SUBSTRATE section in the province window.** A node's drill-down now names
  the **DaemonSets stationed on it** — the CNI, kube-proxy, log and metric
  agents that run *under* your workloads — alongside any kubelet
  Memory/Disk/PID pressure. That pairing is the point: kubelet pressure is the
  node refusing or evicting work for reasons no workload's own pods can explain,
  so it belongs beside the things running underneath. Previously this was only
  an anonymous road count on the map.

### Fixed
- **Hovering water inside a province now reads the same as hovering open sea.**
  In a paired hot/warm session the two took different paths — one showed a bare
  cluster tag, the other added "open sea" — so the same "there's nothing here"
  had two different appearances. Merged into one path rather than tested for
  parity: one branch cannot drift from itself. (Single-cluster sessions were
  never affected, which is why it went unnoticed.)

## [1.3.0] — 2026-07-30

### Added
- **Hover marker.** Pointing at the map now outlines what a click would open,
  before you click it: a diamond on a city, harbour or gate, and — more useful —
  the full ragged boundary of a **province**, which was otherwise invisible,
  since a province is only implied by where its coastline happens to fall. It's
  ambient white and never pulses, so it can't be mistaken for the red/amber
  blast-radius marks that mean trouble, and it obeys the same rules as the
  tooltip (map only, nothing while a window is open). Under the relief style the
  marker sits on the raised land, and harbours stay at sea level with the water
  they're moored in.

### Fixed
- **Map click targets now match what's drawn.** Two long-standing hit-test bugs,
  both invisible until you went looking:
  - **A city's clickable area was as wide as its name.** It was a leftover from
    the original ASCII map, where a city literally *was* its label text — so a
    workload called `a-very-long-workload-name` claimed ~22 cells of terrain,
    and clicking well east of the settlement opened the workload instead of the
    node it sits on. The size of the mistake scaled with how long you'd named
    things. The region is now the settlement's own cell plus a one-cell
    forgiveness ring, matching the drawing. **This is a real shrink** — the
    target is smaller than it was, but it is now honest.
  - **Visible water inside a province opened the node.** `region_at` tests a
    province's bounding rectangle, and the coastline is generated in the
    renderer, so the model had no way to know which cells are land. The view now
    applies that test.
- **The tooltip and the click can no longer disagree.** Both paths reimplemented
  the same probe order independently, so either could be fixed while the other
  silently kept the bug. They now route through one resolver, pinned by a test
  that sweeps every cell in a fixture world and asserts the two agree.

## [1.2.0] — 2026-07-30

### Added
- **Contact shadows.** Trees and settlements now cast a soft ambient pool where
  they meet the ground, which sells the height the relief style already has.
  One pool per settlement sized to its cluster — never one per building, which
  would compound into a dark blob at the centre of a walled keep — and an
  ellipse squashed to the isometric plane rather than a circle. Shadows fall
  west, matching the light the buildings and cliff faces were already lit by.
  Both styles carry them (Relief slightly stronger); the world-scale badges,
  glyph marks and water-moored harbours correctly get none.

## [1.1.0] — 2026-07-30

### Added
- **Map styles — a runtime-selectable relief map.** **View ▸ MAP STYLE** switches
  the world between **Plain** (the flat isometric chart) and **Relief**, which
  raises the land off the sea so coastlines read as cliffs — the same data drawn
  as a cartographer's relief map. It's geometry only: every overlay, mark and
  the minimap are unaffected, and the choice persists between runs
  (`--map-style plain|relief` overrides it for one run).
  **Clicking is unaffected under either style**, which took the interesting part
  of the work: with land raised and the sea not, one screen point sits on *two*
  planes, so the app now resolves each feature on the plane it is drawn on —
  harbours and gates (moored in water) on the sea plane, terrain and cities on
  the land plane. A single lift-corrected inverse would have silently broken
  "click a harbour → open the city it serves" over roughly half of each mark.

## [1.0.0] — 2026-07-17

**First stable release.** 1.0.0 is the milestone, not a big-bang feature drop:
the two items that had been holding it back — proving the multi-platform CI
green, and shipping a code-signed, notarized macOS build — both landed here.
The API/UX surface is now considered stable, so from this point a breaking
change bumps `major`.

> This section aggregates the `0.4`–`0.75` development line, whose entries had
> accumulated under *Unreleased* rather than being rolled into dated sections as
> each version was tagged. Those changes did ship — the git tags `vX.Y.Z` record
> which version each one actually landed in. Everything below `[0.3.0]` is
> unaffected.

### Added
- **Signed & notarized macOS releases (`.dmg`).** The release pipeline now wraps
  the universal binary in a `KuberNation.app` bundle (Info.plist + `.icns` from
  the logo mark), signs it with a Developer ID Application certificate under the
  hardened runtime, notarizes it with Apple (App Store Connect API key), staples
  the ticket, and ships it in a drag-to-Applications `.dmg`. Gatekeeper clears it
  on first launch with no `xattr -d com.apple.quarantine` workaround. The bundle
  carries the licenses + third-party notices. Signing is gated on repo secrets
  (`MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `APPLE_API_KEY_P8_BASE64`,
  `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`); a tag pushed without them still
  releases, degrading to the prior unsigned tarball. The bundle/sign/notarize/
  staple/dmg flow lives in `packaging/macos/release-macos.sh`, runnable locally
  for a signed build. Linux `.tar.gz` and Windows `.zip` are unchanged.

### Fixed
- **macOS crash mitigations for window maximize/resize (vendored miniquad
  patches).** The reported silent disappearance on maximize occurred on a
  machine where Esc was not pressed — a genuine native-layer crash. Two
  grounded suspects are now mitigated by carrying miniquad 0.4.10 in-repo
  (`third_party/miniquad`, via `[patch.crates-io]` — macroquad pins the exact
  version, so no released upgrade can deliver either fix) with two targeted
  patches: **(1)** the upstream (unreleased) `backingScaleFactor = 0` guard —
  macOS reports scale 0 while a window is detached from a screen (transiently
  during display/zoom transitions on docked/multi-monitor setups), which
  poisoned dimensions to 0×0/NaN; **(2)** GL frames are no longer drawn during
  an interactive live-resize — Apple's deprecated GL-on-Metal layer can
  segfault when an app draws mid-resize on macOS 26 + Apple Silicon
  (allegro5 hit a guaranteed crash there), and miniquad ran a full app frame
  from the resize tracking loop. The window content now freezes during a
  drag-resize and repaints on release (the standard mitigation); fullscreen /
  zoom animations still paint. See `third_party/README.md`; the vendored copy
  is dropped when a fixed upstream release lands.
- **Esc on the bare map no longer quits the app.** The Esc key chain's final
  fallthrough silently exited the app when nothing was open — so on macOS,
  clicking the green button (which enters native *fullscreen*, not a maximize)
  and then reflexively pressing Esc to leave fullscreen made the app instantly
  disappear with no trace: a clean exit that read as a crash. Bare-map Esc now
  **leaves fullscreen on macOS** (a no-op when windowed) and otherwise does
  nothing; quitting stays on `Q`, **Game ▸ Quit**, Cmd+Q, and the window close
  button — all of which (unlike the removed Esc path) run the Game Day
  restore-on-exit intercept, closing a safety gap where an Esc-quit could
  strand a live chaos drill unrestored.

### Added
- **Native-crash forensics.** A fault in the windowing / GL / Objective-C layer
  kills the process with no Rust unwind, so the v0.66 panic hook never saw it —
  such a crash left zero trace. Now (1) an **async-signal-safe fault handler**
  (SIGSEGV / SIGBUS / SIGILL / SIGFPE / SIGABRT, Unix) writes one `FATAL native
  fault: …` line to `kubernation.log` before the OS terminates the process, and
  (2) a **session marker** detects any abnormal end (including SIGKILL / power
  loss): the next launch logs "the previous session ended abnormally". Verified
  live (SIGSEGV → the FATAL line; kill -9 → the next-launch warning; a clean
  exit removes the marker).
- **`--resize-storm` dev flag** — drives the window through the transition
  gauntlet (mid-splash resize, rapid ping-pong resizes, back-to-back fullscreen
  toggles, resize during the fullscreen animation, tiny window) and exits: the
  crash gate for the window-transition paths, added while investigating the
  report above (the storm passes clean in both debug and release).

### Changed
- **Oracle replies render markdown for easier reading.** The model's answer is now shown
  with lightweight markdown formatting instead of flat text: `#`…`######` headings render
  bold, `-`/`*`/`+` and `1.`/`1)` lists get proper `•` bullets, fenced ```` ``` ```` code
  blocks render in monospace, blank lines become spacing, and `**bold**` markers are
  stripped (the immediate-mode row renderer is one style per line, so emphasis can't be
  inline). A non-markdown reply still renders verbatim, and the streaming/past-page/demo
  replies use the same path. Pure `render_markdown` helper, unit-tested.
- **City & province drill-down windows now scale + scroll for busy clusters.** On a
  real node with 29+ pods (or a workload with many pods/revisions) the fixed
  900×580 window clipped content (the pod list capped at "+N more", ANNALS fell off
  the bottom). The windows are now **adaptive** (they use the screen, up to
  ~1100×1000) and each column **scrolls independently** with the mouse wheel — the
  citizen/garrison pod list on the left, terrain/conditions/annals (or
  improvements/annals) on the right — with a scrollbar, while the status band stays
  pinned. Every per-row action (evict / port-forward / yaml / logs / rollback) is
  gated to fully-visible rows, so a scrolled-off control can never be clicked.
- **Node CONDITIONS column aligned.** The value (True/False) is now right-aligned at
  the column edge and the condition name truncates to fit, so a long name like
  `FilesystemCorruptionProblem` no longer overruns its value. New pure helpers
  (`clamp_scroll`, `scroll_thumb`, `panel_size`, `panel_split_x`) are unit-tested.

### Fixed
- **Oracle reply no longer scrolls under the header.** When a long consult reply was
  scrolled, its top lines drew *over* the pinned `scope:` chip above the reply zone (the
  scroll-zone cull was too lenient at the top edge). The zone now clips cleanly below the
  header. Interactive controls (Stage / CONSULT NEXT / deepen chips) were already gated to
  fully-visible rows, so none could be clicked while scrolled off.
- **Oracle Settings text fields — long values + held-key erase.** A long value (e.g.
  a corporate/Azure endpoint URL) now **scrolls to follow the caret** — an overflowing
  field shows its *tail* (leading `…`) while focused so you can see what you're typing,
  and its head (trailing `…`) when unfocused so it's recognizable; previously it always
  showed only the first ~48 characters. **Backspace / Delete now auto-repeat when held**
  (after a short delay, ~28 chars/sec) so you can clear a long field by holding the key
  instead of one tap per character. The full value was always stored correctly — these
  fix the *display* and *editing ergonomics*. Pure `field_view` + `erase_repeats` helpers,
  unit-tested.

### Added
- **Windows x86_64 binary.** The release now builds and publishes a Windows
  binary (`kubernation-vX.Y.Z-windows-x86_64.zip`) alongside the macOS-universal
  and Linux tarballs, and CI builds + tests on `windows-latest` on every push so
  the target can't silently break. The code was already largely Windows-ready
  (the unix-only bits are `cfg(unix)`-gated, the clipboard has a Windows branch);
  config/log directories now resolve to `%APPDATA%`/`%LOCALAPPDATA%` on Windows
  instead of falling back to the cwd. Unsigned (SmartScreen warns on first launch
  → "More info ▸ Run anyway").

### Documentation
- **README refreshed for the v1 release.** Rewrote the README to reflect the full
  current feature set — added the **Oracle** (bring-your-own-LLM consult, streaming
  + suggest-to-gate), **cost cartography** + OpenCost, the **security suite**
  (Charter / hardening scan / NetworkPolicy walls / posture score), the **Annals**
  change timeline, the **workload table**, **postmortem export**, **multi-burn-rate
  SLO alerting**, the new **saturation / walls / upkeep** map views, and the four
  new **Advisor** tabs — and corrected stale claims (perf is now 500 nodes / 5000
  pods ≈ 4ms; the 8-menu bar with **Wonders**; the two config files; the full CLI
  flag list). Recaptured every screenshot against the current UI (16 images: added
  Oracle / cost / Charter / Annals / workloads; dropped the menu / evict shots) and
  marked `docs/ROADMAP.md` as a historical triage doc.

### Added
- **Persisted preferences.** A small `~/.config/kubernation/prefs.json` remembers the
  colour-blind palette choice and the last map overlay across runs, so you don't re-set
  them every launch — **CLI flags always win** over the saved value. Non-secret,
  non-cluster convenience state only (written atomically; a corrupt file is renamed
  aside, never deleted). The colour-blind palette is now also a **View ▸ Colour-blind
  palette** toggle (live, not just a launch flag). Persisting the context and namespace
  filter is deliberately deferred (they need a "pin vs. follow your kubeconfig" decision).

- **Colour-blind-safe palette** (`--colorblind`). The product's grammar rides on
  green (healthy / good / calm / low) vs red (critical / NotReady / high), which
  red-green colour-blindness (deuteranopia + protanopia, ~8% of men) can't tell
  apart. The flag moves the green meaning-axis to a steel **blue** — blue / amber /
  red are all mutually distinguishable — across the map terrain, every overlay, the
  marks, the gauges, and the advisor text; red (critical) and amber (warning) are
  left unchanged. One switch (`theme::set_colorblind`) read by every meaning-green.
  (Tritanopia — rare blue-yellow — is out of scope.)

- **Hardening — crash safety + error visibility (v1 polish round).** A global panic
  hook now logs every panic (thread · location · message) to
  `~/.local/state/kubernation/kubernation.log` before it unwinds, so a crash launched
  from a launcher (no terminal) is diagnosable. The cluster-connection thread is named
  and watched: if its world loop crashes, a **fatal banner** appears ("the world loop
  crashed — restart") instead of a silently frozen world. All ~160 shared-mutex locks
  are now **poison-tolerant** — a panic in one background task can no longer cascade
  into a render-thread crash that takes the whole app down. The tokio runtime and the
  log-file writer degrade gracefully instead of panicking, and the per-session Oracle
  reply cache is bounded.

- **Pre-1.0 readiness — four build items.**
  - **Release pipeline + CI.** GitHub Actions: `ci.yml` runs fmt + clippy + test on
    macOS and Linux on every push/PR; `release.yml` builds pre-built binaries on a
    `v*` tag — a macOS universal binary (Apple Silicon + Intel, `lipo`'d) and a Linux
    x86_64 binary — packages them with the licenses + third-party notices, generates
    `SHA256SUMS`, and publishes a GitHub Release. (The macOS binary is not yet
    code-signed/notarized — the release notes carry the Gatekeeper workaround; signing
    is a follow-up that needs an Apple Developer cert.)
  - **Workload table** (`O`, or **View ▸ Workloads**). A realm-wide, sortable +
    filterable list of every workload — kind · namespace/name · ready · status · age,
    with a health glyph — the k9s-style triage view ("show me everything
    CrashLoopBackOff") the map's drill-downs didn't cover. Sort by health / name /
    ready / age; type to filter; click a row to open its city window. Read-only.
  - **Connection banner.** A lightweight periodic API probe surfaces the hot cluster's
    liveness; when the API stops answering (VPN/link drop), a banner under the menu bar
    says "reconnecting to <context> — <reason>" instead of leaving you with silent fog.
    Self-clears the moment the connection is back.
  - **Multi-container log picker.** The log overlay shows a container tab row for
    multi-container pods (sidecars, init containers); click a tab to tail that
    container instead of silently getting the first one.

### Changed
- **Brand name is "KuberNation" (camelCase) on every user-facing surface** — the OS
  window titlebar, the menu bar, the About window, the in-app help/almanac text, the
  README, and the crate descriptions. Technical identifiers stay lowercase
  (`kubernation` binary/crate, `kind-kubernation` context, `~/.config/kubernation`,
  `kubernation.io/…` annotations, `KUBERNATION_LLM_TOKEN`).

### Added
- **Cost cartography ("upkeep").** A new **View ▸ Upkeep (cost)** map overlay paints
  each province by what its node costs (a bronze "spend" choropleth, with a gold idle
  coin on nodes carrying a lot of unrequested capacity), an **Advisors ▸ Cost** tab
  rolls it up (total · by-namespace · costliest workloads · idle/waste), and the
  SELECTION line shows a node's upkeep + idle. Honest by construction: with no pricing
  it's a relative **"cost units"** score derived from resource requests (works on any
  cluster, no metrics-server, never a `$`); supply rates — `--cpu-rate` / `--mem-rate`
  (per binary GiB) / `--node-rate NODE=USD`, or a `kubernation.io/cost-hourly` node
  annotation — and it shows real `$/hr` + a `~$/mo` projection, always labelled an
  estimate from your rates × reservation, never a cloud invoice. Idle (the unrequested
  remainder you could consolidate) is the actionable signal; metrics-server sharpens it
  to paid-for-but-unused. Read-only — no new write verb. Complements the right-sizing
  advisor (which says *which* workloads are mis-sized) with the *where-does-the-money-go*
  map.

- **OpenCost integration — invoice-grade cost.** Pass `--opencost` (optionally
  `--opencost ns/service:port`, default `opencost/opencost:9003`) and KuberNation reads
  the in-cluster [OpenCost](https://opencost.io) `/allocation` API for real, amortized
  cost — the network / load-balancer / storage / spot & reserved-discount lines the
  requests-based estimate structurally can't see — and the cost overlay, the Advisors ▸
  Cost tab, and the SELECTION line all switch to it (labelled "from OpenCost"). It's
  reached **read-only through the kube API-server service proxy** — the same
  authenticated connection as the reflectors, no port-forward and no new off-laptop
  egress (needs RBAC `get services/proxy`). When OpenCost is absent or unreachable it
  degrades cleanly to the requests/usage estimate. `--opencost-window` sets the
  allocation window (default `1d`). The fetch path is a reusable in-cluster HTTP-source
  substrate (`k8s/opencost::fetch_service_proxy`) — the first of a planned set of
  optional-tool adapters. Read-only; no new write verb.

- **Own copyright + trademark assertion.** The About window and README now assert
  "© 2026 Jason Olmsted. KuberNation™ and the KuberNation logo are unregistered
  trademarks of Jason Olmsted" (unregistered — ™, not ®), alongside the existing
  Civilization-rights-holder disclaimer. The license copyright line and a workspace
  `authors` field name Jason Olmsted, and the dual license now ships both texts as
  `LICENSE-MIT` + `LICENSE-APACHE` (the asserted "MIT OR Apache-2.0" was missing the
  Apache text).

### Fixed
- **Intro splash caption.** The title/"press any key" caption is now anchored to the
  logo's actual drawn bounds (centered on it, just below it) instead of a fixed
  offset that drifted on different window sizes, given a dark stroke so it pops on
  the bright scene art, and the splash holds a few seconds longer (still skippable
  with any key).

### Added
- **About window.** Help ▸ About opens a window featuring the splash logo, then
  credits (Jason Olmsted; built in collaboration with Claude), the third-party license
  obligations (the bundled SIL-OFL fonts + the Rust crate ecosystem — MIT/Apache-2.0
  plus the ISC/BSD-3-Clause/Zlib/Unicode-3.0 components, with full per-crate notices in
  `THIRD-PARTY-NOTICES.md`), and the trademark disclaimer (an unaffiliated homage, not
  associated with Take-Two / Firaxis / the Civilization franchise). Adds a generated
  `crates/kubernation/THIRD-PARTY-NOTICES.md` (via `cargo-about`).

- **Multi-burn-rate SLO alerting.** The treasury's single burn threshold became the
  SRE multiwindow pattern: a short (~48s) burn rate, a long (~2 min) sustained-burn
  window, and a small "is it down right now" gate drive a *fast* burn (severe +
  active → pages as a **Critical** queue concern, "burning fast") vs a *slow* burn
  (sustained but mild + active → tickets as a **Warning**, "eroding"). The gates are
  the point — a one-sample blip (long window cold) and a recovered incident (not
  active) both stay quiet, so the queue doesn't churn on noise; the ticket tier is
  reachable at the default 99% target. The city window's TREASURY band shows the
  classification + both rates. Still in-session and derived from pod readiness (no
  metrics-server); the window sizes/thresholds are recent-window rates tuned to the
  ring, not a 30-day compliance burn rate.

- **Oracle reply carousel.** Drilling into the next consult no longer wipes the
  prior reply — a `◀ reply N/M ▶` pager keeps the session's replies so you can flip
  back and re-read reply 1 *while* reply 2 streams (handy when a local model takes a
  while). The latest page is the current/streaming consult with its actions
  (Stage / CONSULT NEXT / deepen); earlier pages are read-only prose (acting on a
  stale suggestion against a since-changed cluster would be unsafe). `c`/`w`
  copy/export the page you're viewing. The carousel is session-local and clears on
  an endpoint or cluster change.

- **Oracle replies now stream token-by-token.** Instead of a 60–90s spinner then one
  block, the answer appears as the model generates it. The request goes `stream:true`
  (SSE), the client reads the response incrementally, and the consult window shows the
  text growing in place with a `streaming… {n}s · {chars} chars` status and a Cancel.
  The byte-frozen consent reflects `stream:true` (the operator still reviews exactly
  what is sent). A cold model still gets the full timeout to its first token; once
  tokens flow, only a stall is cut. If a stream ends in error after some text arrived,
  the partial is kept with a note (not discarded). An endpoint that ignores streaming
  degrades to the previous whole-reply behavior automatically.

- **Oracle answer quality.** Three changes that make a consult sharper: (1) the
  **default question is now scope-aware** — instead of a generic "explain this", the
  model is asked the operator's real question ("Why is demo/crashy unhealthy and
  what should I do?", "What is straining node worker?", realm "What are the top 1-3
  issues to fix first, worst-first?"); (2) the **root-cause diagnosis** of the worst
  not-ready pod (the why + the fix KuberNation already computes) is **folded into
  the workload and concern consults**, so the model reasons over the actual failure
  instead of re-guessing it; (3) **node consults now seed CONSULT NEXT** from the
  troubled workloads stationed on that node (mirroring the realm seeding), so a node
  drill-down always offers its on-node suspects. The scope-aware question is carried
  on the bundle so the byte-frozen consent stays exact, and every folded string gets
  the same unconditional redaction + fencing.

### Changed
- **Oracle CONSULT NEXT is now seeded from the attention queue.** Previously the
  "drill into one of these" links came only from the model's structured
  `investigate` block — so when a small local model described the problem in prose
  but didn't emit the block, a realm consult with a clear critical issue showed *no*
  links. Now a realm consult seeds CONSULT NEXT from KuberNation's OWN attention
  queue (the concerns it already computed, severity-ordered), and the model's
  validated block only *adds* targets the queue didn't flag. The app curates; the
  model advises — so a clearly identified concern always yields a one-click
  drill-down.

### Added
- **Oracle reply UX polish.** A bundle of reply-side improvements: (1) the model's
  fenced machine-readable blocks (`investigate` / `suggestions` / `follow_up`) no
  longer leak into the displayed answer — they're already rendered as CONSULT NEXT
  links / Stage buttons / deepen chips, so the raw JSON is stripped from the prose
  (a pure inverse of the block scanner; the parsers still read the raw reply, so
  the two can't disagree); (2) the in-flight spinner shows an **elapsed counter +
  the timeout**, with a **Cancel** button; (3) **`c` copy / `w` export** a consult
  to the clipboard / a local file; (4) a failed consult renders a **WARN error
  card with a one-line hint** (auth / timeout / model-not-pulled / unreachable) and
  a **Retry**, instead of fake answer prose; (5) the "model-generated; verify"
  caveat is **pinned as a footer** outside the scroll, sharpened when a Stage
  button is on screen.
- **Oracle "investigate" → CONSULT NEXT links.** A realm (or node) consult often
  ends with a prioritized "what to investigate first" list. Those entries are now
  **clickable consult links**: the model emits a validated, ordered `investigate`
  block (workload/node targets, each re-resolved against the live store so a
  hallucinated/garbage name is dropped) rendered as a **CONSULT NEXT** row under
  the reply. Clicking **jumps the Oracle's scope** to that object and runs a fresh
  consult — drilling realm → specific without leaving the modal (a crash target's
  logs then auto-include; its own deepen chips appear). Distinct from INVESTIGATE
  FURTHER (which adds context to the *same* scope). No prose-parsing (the model
  proposes a validated scope, the app acts on a click); the jump rebuilds the
  bundle fresh, gets the same redaction + fencing, and re-Previews for re-consent
  on a remote endpoint. One consult per click (no auto-cascade).
- **Oracle "deepen" follow-up drill-down.** Instead of the model telling you to
  "go review the logs / the PVC" (data KuberNation already holds but withheld), it
  now folds that context in. For a crash/error concern the offending pod's **logs
  are included by default** — the model reasons over the actual log lines instead
  of recommending you fetch them. After any reply, an **INVESTIGATE FURTHER** row
  of one-click lenses — *include logs · storage detail · blast radius · rollout
  history · widen to node* — re-consults with that context added; the model may
  rank which to try next (it only reorders the app-curated menu, never fetches).
  Button-only (no freeform). Each lens routes through the same redaction + fencing
  + token budget; an explicitly-requested lens is promoted so it isn't silently
  dropped (and a chip honestly reads "dropped to fit" if it is). A remote endpoint
  re-Previews the enriched payload for re-consent and writes a fresh egress audit.

### Changed
- **Oracle default model is now `qwen3:30b`** (was `qwen3.5:35b`) — a fast
  Mixture-of-Experts (30B total, ~3B active) that answers a realm consult in
  ~10–15s instead of 60–90s, so it won't time out on a typical laptop. Pull it
  with `ollama pull qwen3:30b`; override with `--llm-model` or a saved profile.

### Added
- **Oracle: two levels of connection test + a per-profile timeout.** The endpoint
  Settings face now has **two** test buttons: **test** (level 1 — the quick `GET
  /v1/models` reachability/auth/model-availability check) and **chat** (level 2 —
  a real tiny chat completion that proves the model actually generates, the
  strongest endpoint check). Each profile also has a **timeout** field (seconds,
  5–600; default 180) that bounds its consults and tests — a fast local model can
  use 30s, a slow 35B can use the full default. Both tests share one egress gate
  (a remote one is allowed only for the active, armed endpoint and writes an
  egress audit); the chat verdict shows the model's reply, never the token.
- **Oracle "Test connection" on the endpoint Settings face.** A **test** button
  on each profile probes the endpoint (`GET /v1/models`) and shows a one-line
  verdict that validates the whole config at once: reachable, the token accepted
  (a 401 reads "FAILED — token rejected"), and the configured model actually
  available ("OK — model available", or "reachable, but model 'X' is NOT
  available — pull it or pick one below"). A local endpoint tests on select; a
  remote one tests behind the same per-session Arm gate as a consult (and records
  the same egress audit). The discovered models double as the click-to-pick list.
- **Oracle endpoint profiles + model picker (Wonders ▸ Oracle ▸ Settings).** The
  Oracle window gained a Settings face to manage named ENDPOINT PROFILES — a local
  Ollama and one or more remote/corporate endpoints — and switch between them
  in-app. For a local endpoint it discovers the models you've pulled (`GET
  /v1/models`) and lets you click to pick one; for a remote endpoint you enter a
  URL + a masked API token + model. Profiles persist to
  `~/.config/kubernation/oracle.json` (created `0700`/`0600`, written atomically),
  **including the token by your explicit per-profile opt-in** (stored as plaintext
  on disk — the Settings face says so plainly and steers high-sensitivity tokens
  to the `KUBERNATION_LLM_TOKEN` env var, which is never persisted). This serves
  the corporate case: a policy-compliant hosted frontier model where you supply
  the URL, token, and model. Safety is unchanged — a remote endpoint still
  publishes off-laptop, so it stays behind the per-session **Arm remote egress**
  gate (switching profiles re-disarms), the Local/Remote class is always recomputed
  from the URL (never trusted from the file), and redaction + prompt-injection
  fencing remain unconditional regardless of how "trusted" the endpoint is.

### Changed
- **Oracle default model is now `qwen3.5:35b`** (was `llama3.1`) — a model that
  must be pulled in your local Ollama; override with `--llm-model`. The consult
  timeout was raised 60s → 180s to fit a large local model (a realm consult on a
  35B measured 60–90s incl. the cold model load; the consult runs off the world
  loop, so a longer ceiling only means a longer "consulting…"). The headless
  `--oracle-go` verification now holds the screenshot until the reply actually
  lands instead of firing on a fixed frame count.
- **Oracle polish.** The consult lives under a new **Wonders** menu (room for
  future marvels). The pre-send Preview is now a legible, structured rendering —
  the model, parameters, and each message's full text with real line breaks (plus
  the exact wire byte count) instead of an escaped-JSON wall. And a failed consult
  surfaces the endpoint's own error message — a 404 now reads "model 'x' not
  found" (so a model you haven't pulled is obvious) instead of a bare HTTP code.

### Added
- **The Oracle — suggest-to-gate (the model can PROPOSE a fix).** When you
  consult the Oracle it may now propose a concrete change — scale, restart,
  set-image, rollback, or cordon. Each proposal is validated against the live
  cluster (a hallucinated workload, a protected namespace, a DaemonSet "scale",
  an out-of-range value, or a missing container/revision is **rejected**, with
  the reason shown) and the valid ones get a **Stage** button. Staging drops the
  change into the planning turn — you still review it and **Commit** through the
  existing confirmed, RBAC-checked, server-side-dry-run gate. The model never
  touches the cluster: it proposes; you (and the gate) dispose. This completes
  the Oracle Wonder.
- **The Oracle — remote endpoints (opt-in, gated).** The Oracle can now consult a
  remote OpenAI-compatible model (OpenRouter, vLLM, an Anthropic shim, …), but
  because that *publishes* data off the laptop it is **off by default behind an
  explicit per-session "Arm remote egress"** action. A remote consult sends only
  the **exact bytes you previewed** (the consent is frozen at Preview time, so it
  can't drift), and each one writes a one-shot, metadata-only local audit record
  (when / endpoint / model / size — never the prompt, reply, or token). The API
  token stays env-only. Local (Ollama) consults are unchanged and need no arming.
- **The Oracle of KuberNation (BYO-LLM Wonder) — local, explain-only.** A new
  **Oracle** menu consults a bring-your-own language model to *explain* a scope —
  the whole realm, a selected workload or node, or a focused concern. It is
  advisory only: the model is shown a redacted, fenced summary built from what
  KuberNation already observed (never raw cluster dumps, never Secret values) and
  it can **never** change the cluster. A mandatory **Preview** shows the exact
  text that will be sent before you Consult. Local-first: point `--llm-url` at a
  local model (default: Ollama at `localhost:11434`), pick `--llm-model`; any API
  token comes from the `KUBERNATION_LLM_TOKEN` env var only (never written to
  disk). Remote endpoints arrive in a later version; replies are model-generated
  — verify before acting. (This is the project's first outbound network egress —
  opt-in, gated like port-forward.)
- **Saturation overlay — the 4th golden signal.** A new **View ▸ Saturation
  (strain)** map overlay tints each province by how full it is toward its hard
  limits: the worst of cpu/mem usage, **scheduled pods vs the kubelet max-pods**,
  and the kubelet **Disk/Mem/PID-pressure** conditions. Unlike the cpu/mem
  Pressure overlay, it catches the silent scheduling failure — a node at max-pods
  (or under a kubelet condition) lights up red even while cpu/mem look calm. The
  province SELECTION names the binding dimension (e.g. `pods 105/110`,
  `DiskPressure (pegged)`); the province window shows a strain line; a node near
  max-pods (≥95%) raises one queue concern. Pod-count + conditions need no
  metrics-server; disk/PID are shown only via the kubelet's own conditions (never
  a fabricated percentage).
- **Postmortem / after-action export.** One click writes a markdown after-action
  report of the current session — cluster context, the realm-defense posture, the
  open concerns (each with its next-action hint), the change timeline (the Annals,
  with the "trouble begins here" fault line and the change that preceded it), and
  any Game Day drills run — pasteable straight into a postmortem doc. Export from
  the Annals modal (the **Export** button) or **Game ▸ Export after-action
  report**; the file lands in the working directory. A one-shot local export (no
  cross-run history); honest that it's an in-session snapshot; credential-shaped
  values in event text are masked best-effort before the file is written.
- **Posture score — "realm defense".** A 0–100 severity-weighted rating + tier
  (Fortified / Defended / Exposed / Breached) that rolls up the two security
  scans — pod hardening (#7) and NetworkPolicy walls (#10) — into one glanceable
  meter, capping the security trio. A 6th **Advisors ▸ Posture** tab shows the
  headline, two axis sub-scores, and the ranked "why" factors (linking to
  Hardening / Network), plus a `DEFENSE` chip in the STATUS column. Honest +
  explainable: system-namespace findings (the distro's CNI/kube-proxy) are scored
  separately and never drag the operator score, Info hygiene nits are capped so
  they can't tank a crit-free realm, an empty/unscanned cluster never reads green,
  and the footer states it's a curated subset — a defense indicator, not CIS/
  full-PSS compliance.
- **NetworkPolicy coverage map — "unwalled cities".** OWASP-K07 (Missing Network
  Segmentation) on the 4X map: a workload with no NetworkPolicy isolating its
  ingress is an *unwalled city*, open to lateral movement. A new **Walls**
  map overlay (View ▸ Walls) tints provinces by coverage and marks unwalled
  cities with a breach notch — red when the city is also reachable (Service/
  Ingress-fronted), the real finding. The **Network advisor** gains a WALLS
  section (cities walled, unwalled-&-exposed, wide-open namespaces), and an
  unwalled-&-exposed workload raises one Warning in the attention queue.
  Read-only — KuberNation watches NetworkPolicies, never writes them; honest
  about its limits (matchExpressions handled; namespaceSelector / ipBlock /
  port rules not analyzed; CNI *enforcement* not verified). (Roadmap #10.)
- **Change timeline — "The Annals".** A recent, classified change-feed answering
  "what changed?" — the third triage axis beside the attention queue (what's
  wrong) and the blast/impact panel (what else is affected). A cluster-wide modal
  (View ▸ Annals, or the `H` key) merges the recent-events ring, Deployment
  rollout history (the authoritative deploy record), and this session's operator
  actions (commits / evicts / chaos drills) into one newest-first feed; failures
  are coloured, a **"trouble begins here"** fault line marks the first failure,
  and a change just before it is flagged **"(before the failure)"** — honest
  adjacency, never fabricated causality. The city and node windows replace their
  old separate HISTORY + CHRONICLE lists with the same merged **ANNALS** section
  (the city keeps its per-revision `rollback` button). Read-only; the event ring
  is a bounded ~15-min window, stated in the footer — not a full audit log.
  (Roadmap #9.)
- **Dependency / impact triage panel.** The blast-radius overlay (`B`) now also
  lists what a troubled node/workload affects — an **IMPACT** section in the right
  column: the cascade of cities → harbors (services) → gates (ingresses) with hop
  badges, each **clickable to fly to + open** it, and affected resources that are
  *themselves* already in trouble float to the top of their hop tier with a
  severity marker (cross-referenced from the attention queue). The on-map flash +
  banner stay visible beside the list. Pure, unit-tested `impact_rows`; topology-
  only (no fabricated dependency edges — an empty radius is shown honestly).
  (Roadmap #8.)
- **Security / hardening scan.** A 5th **Advisors ▸ Hardening** tab lints every
  workload's pod template for security misconfigurations — privileged containers,
  host namespaces, dangerous capabilities, hostPath mounts (Critical); run-as-root,
  allowPrivilegeEscalation, un-dropped capabilities, writable root filesystem
  (PSS-restricted Warnings); missing resource limits, `:latest`/untagged images,
  automount SA token (Info) — each tagged with the standard it maps to (OWASP-K01 /
  PSS-baseline / PSS-restricted / Popeye). Critical misconfigs also surface in the
  attention queue as **one aggregated concern per workload** (never per-finding).
  Read-only; pure `state/harden.rs` (unit-tested); honest about being a curated
  subset (seccomp + default-SA deferred to avoid false positives at the namespace
  default). (Roadmap #7.)
- **The Charter — self-scoped RBAC.** A new **Help ▸ Charter** modal shows what
  *you* can do in the cluster — a curated `can-i` grid (✓ allowed / ✗ denied /
  ? unknown) for the active namespace plus a realm-wide (cluster-scoped) band,
  with allowed *dangerous* capabilities (exec, secrets-list, rbac-write,
  SA-token, node patch/proxy) highlighted as the audit finding. Read-only
  self-query — the exact `SelfSubjectAccessReview` mechanism `kubectl auth can-i`
  uses (one authoritative apiserver decision per cell, never client-side guessed;
  an unanswered cell shows `?`, never a fabricated verdict). Kills surprise 403s
  and doubles as a "which features will work for me here?" check. Pure
  `state/charter.rs` + `k8s/rbac.rs` (unit-tested); cluster scope togglable by
  namespace. (Roadmap #6.)
- **Right-sizing advisor.** A 4th Advisors tab compares each workload's
  per-replica resource **requests** against actual **metrics-server usage** and
  flags **over-provisioned** (reclaimable waste), **under-provisioned**
  (CPU-throttle / memory-OOM risk), and **scheduler-blind** (no requests)
  workloads — each with a directional suggested request (VPA-style floors +
  target-utilization headroom) and a cluster-wide reclaimable-cpu/mem total in
  node-equivalents (never invented dollars). Read-only; metrics-server only
  (degrades dark to just the scheduler-blind list when absent); honest about the
  single-sample basis. Pure `state/advisor.rs::rightsizing_report` + unit-tested
  classification (mean for over, *peak* for memory-under since it's
  incompressible; a `measured==0` guard so a momentarily-unsampled workload is
  never a false "waste"; a floor-negation guard so "waste" never suggests a
  *raise*). (Roadmap #5.)
- **Rollback — the planning turn's 5th verb.** A Deployment's city window HISTORY
  section now has a `rollback` button on each prior revision; it stages a rollback
  (restore that revision's pod template) reviewed and committed through the same
  server-side-dry-run + confirm rail as the other staged changes. New
  `Intervention::Rollback { workload, to_revision }` (Eq-safe — the apply path
  resolves the target revision's template from the live cluster). Deployment-only.
  Verified live: web rolled `nginx:1.28-alpine` → `nginx:1.27-alpine`. (Roadmap #4.)
- **Rollout history + revision diff.** A Deployment's city window now has a
  HISTORY section listing its recent revisions (newest first, the current one
  marked) with the container image each ran, plus the image change that produced
  the live revision — the "which change is running / broke it?" answer at a
  glance. Pure `state/rollout.rs` over the watched ReplicaSet store (unit-tested);
  Deployment-only (StatefulSet/DaemonSet track revisions in unwatched
  ControllerRevisions). Sets up one-click rollback. (Roadmap #3.)
- **Runbook / next-action hints on concerns.** The focused concern in the
  ATTENTION column now shows a `next:` hint pointing at the in-app verb that acts
  on it — `L: tail logs`, `B: blast radius`, `click: open the city/province`, or a
  type-specific remediation (a pending PVC → check the StorageClass; an orphan
  Ingress → fix the backend Service; an idle Service → check its selector; a
  burning SLO → open the TREASURY band). Pure `attention::next_action` keyed on the
  concern's stable kind (unit-tested). (Roadmap #2.)
- **Pod-not-Ready explainer.** The city + province windows now show a plain-English
  "why / fix" for a degraded workload's worst pod — turning the raw Kubernetes
  reason (`CrashLoopBackOff`, `ImagePullBackOff`, `Unschedulable`, OOMKilled, a
  failing readiness probe, a missing ConfigMap/Secret, …) into one sentence plus
  the next action (often an in-app verb, e.g. "tail the previous container (p)").
  Pure `state/diagnose.rs` (unit-tested); OOM is distinguished from a generic
  crash, and high restart counts are called out. (Roadmap #1.)

### Fixed
- **Stale "observe-only" text in the in-app Almanac** — it claimed "KuberNation
  only watches; there are no mutation paths anywhere", which predates the gated
  write paths (evict / planning turn / Game Day chaos). Now describes the
  read-first-with-gated-writes posture accurately.

### Changed
- **README rewritten for newcomers / as a website seed** — explains the game
  metaphor without assuming Civilization or k9s knowledge, adds a Kubernetes→map
  mapping table and glyph **legend tables** (replacing a hard-to-read ASCII
  diagram), documents the current right-column sections and the three write
  paths + Game Day, and drops an unverifiable latency figure. CLAUDE.md's stale
  "no log file" note was corrected (the client does write `kubernation.log`).

### Added
- **Monospace log overlay.** The log viewer now renders in a fixed-width face
  (Liberation Mono, OFL 1.1 — bundled, same family as the map's serif), so
  timestamps and columns line up the way logs are meant to read. Severity tinting
  + width-fitting are unchanged; the overlay chrome stays in the UI sans face.
- **Game Day enhancements (round 3).** Making the chaos console safer, more
  legible, and more measurable:
  - **Dry-run preview** — the drill's PREVIEW now lists the concrete steps that
    would run ("kill pod demo/web-…", "scale demo/web → 0"), so you see exactly
    what a drill does before running it.
  - **Blast cap** — a drill that would delete more than `MAX_KILL_PODS` (50) pods
    at once is refused (fail-closed), a guardrail against a fat-fingered
    cluster-wide raid.
  - **Budget-breach verdict** — the scorecard headlines what the drill cost the
    error budget: "drill BREACHED the error budget" / "spent N% of budget" /
    "error budget untouched", tying chaos to the treasury.
  - **Four new experiments** (reusing the existing write primitives — no new
    verbs): **kill a percentage** of a workload's pods (a slider between kill-one
    and kill-all), **scale spike** (surge a workload by Nx to test scheduling
    headroom, then restore), **cordon freeze** (cordon a node *without* draining —
    new pods won't land), and **directional partition** (the deny-all gains
    `deny [both|ingress|egress]` — egress = "lost its backend", ingress = "out of
    rotation"). The Game Day window grew per-experiment knobs (kill %, surge
    factor, partition direction).
  - **Safety triad.** **Restore-on-exit** — quitting with a live, restorable
    drill undoes it first (uncordon / scale back / unpartition) so you never
    strand the cluster, with an 8s backstop so exit can't hang. **Auto-restore** —
    an opt-in "auto-restore after 60s" toggle on restorable drills. (Immediate
    undo is the existing Restore button.)
  - **All-or-nothing RBAC gate.** A drill now pre-flights `delete pods` permission
    per evicting namespace (evicts can't be dry-run) alongside the existing
    dry-run gate, so a cordon+drain whose drain is forbidden can't half-apply
    (cordon then stop) — nothing is written unless every step is permitted.
  - **Deeper scorecard.** A **steady-state gate** warns when the target was
    already degraded before the drill (a noisy baseline); **MTTD** reports how
    long the attention queue took to flag the drill ("flagged in Ns" or "never
    flagged it — a monitoring gap", KuberNation measuring its own observability);
    and a **recovery curve** sparkline shows the watch set's ready-fraction over
    the drill.
  - **"Raid underway" in the queue.** While a drill is fresh (~30s) the attention
    queue announces it ("Game Day: raid underway — …"), so `n`/`B` route to it —
    closing the loop with the product's spine.
  - **Flip-watch.** The blast-radius overlay auto-engages on the live raid's
    subject so you can watch the cities flip; it disengages when the raid clears,
    and a manual selection still overrides.
  - **Chronicle.** Finished drills accumulate into an in-session CHRONICLE in the
    Game Day window (experiment · target · outcome) — recent history at a glance
    (no cross-restart persistence).
  - **Difficulty tiers (compound drills).** A TIER row — **Skirmish** (kill one),
    **Raid** (kill ~half), **Siege** (partition + kill all) — composes existing
    experiments into one sequenced drill with a LIFO restore (so Siege's deny-all
    is undone after). `plan_tier` is pure + unit-tested; selecting a tier
    overrides the single-experiment choice. Verified live on kind (Siege: netpol
    + kill-all, then auto-restore removed the netpol and pods recovered 3/3).
  - **Round-3 review hardening** (adversarial review, 3 medium + cheap lows):
    a context switch with a live restorable drill now **undoes it with the old
    client before switching** (don't strand the cluster, matching restore-on-exit,
    whose backstop was raised past the worker timeout); an operator undo (auto /
    manual / exit / switch) is labeled **"restored"**, not "self-healed", and no
    longer pollutes the chronicle; `run_chaos` gained a self-contained
    protected-namespace failsafe (like the partition's empty-selector guard);
    **ScaleSpike** is capped like the destructive paths; a node drill with no
    watchable workloads no longer shows a spurious "baseline noisy"; the scorecard
    culls at the modal frame and yields to the chronicle; quit is honored during
    the intro splash.

### Changed
- **The attention queue moved from the bottom strip into a docked ATTENTION
  section in the right column**, between STATUS and FORWARDS — and its rows are
  now **clickable**: clicking a concern flies the camera there and opens its
  drill-down (the same path as the `N` key, so keyboard and mouse can't drift).
  The map play area now extends to the screen bottom (the 64px strip is gone),
  and the column shows up to 6 concerns (was 3) to use the reclaimed space. The
  focused concern wears the picker-cursor highlight. Pure `attention_rows()` +
  unit test per the GUI testability policy.
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
  To backfill the lost render coverage: a **`make gui-smoke`** render-smoke gate
  (every overlay/modal state through `--screenshot`, fail on panic) plus a policy
  of testing each view's pure draw-decision fn (`panels::region_lines` is the
  first).

### Added
- **Game Day — chaos drills.** A new Game Day menu opens a chaos console: pick a
  target, choose an experiment, preview its blast radius + the budget it'll spend,
  then run it (a real, CRIT-confirmed failure). A **scorecard** shows the
  cluster's response: recovery time, budget spent, self-healed or not. Six
  experiments across two passes:
  - **Pass 1 (workload kills, no new verb):** **kill one pod**, **kill all
    pods**, and an **outage** (scale to 0, with Restore) — reusing the existing
    gated write primitives (`evict_pod`, scale patch).
  - **Pass 2:** **node failure** (cordon the node + drain its pods, Restore
    uncordons — existing verbs), **broken image** (roll a workload onto an
    unresolvable `*.invalid` ref → ImagePullBackOff, caught by readiness so the
    old replicas keep serving; Restore re-applies the captured original — existing
    verb), and **partition** (a deny-all NetworkPolicy scoped to the workload's
    pods, Restore deletes it — the **one new write verb/resource type** chaos
    adds; its effect depends on the CNI enforcing NetworkPolicy). The scorecard
    adapts per experiment class (workload dip/recover + budget, node pods-drained
    + cordon state, isolation note).
  Control-plane / system namespaces and control-plane nodes are never targetable
  (fail-closed in the pure core + re-checked at execution, covering every step
  including the restore); the patchable steps are server-side dry-run-gated
  (which also enforces RBAC); hot-cluster-only. Service-mesh fault injection
  (Istio/Linkerd) is deferred to a later pass. Drill planning + guards are pure +
  unit-tested; verified live on kind.
- **Per-workload / configurable SLO targets.** The treasury's SLO target is no
  longer fixed at 99% — set it per workload via a `kubernation.io/slo-target`
  annotation (e.g. "99.9", read-only/declarative) or the city window's new SLO
  stepper (an in-session override), with a global `--slo-target` default.
  Precedence: manual override > annotation > default; the band tags the source
  (manual / annotated / default). Pure + unit-tested; verified live.
- **The treasury — availability SLOs + error budgets.** Each city window gains a
  **TREASURY** band: an availability SLO (default 99%) and the error budget it
  spends down — a coin gauge that's full when the workload stays up, draining
  when it flaps, exhausted when availability falls below target. In the 4X
  framing the error budget is a treasury you spend. Availability is **derived
  from pod readiness** (≥1 replica up over a recent ~8-min window) — no
  metrics-server or Prometheus needed, works on any cluster; partial capacity
  loss stays the attention queue's job. A burning or exhausted budget raises a
  queue concern (deduped against workloads a stronger concern already covers, so
  the budget surfaces the *flaky-but-up-now* cases the point-in-time detectors
  miss). Honest scope: an in-session rolling window (no cross-restart history),
  i.e. *recent* availability, not 30-day compliance. The math is pure +
  unit-tested; verified live on kind (web 100% budget, crashy exhausted).
- **Blast-radius highlighting (`B`).** Select a node or workload (or focus a
  concern) and press **`B`** to light up its dependency fan-out on the map —
  pulsing lines spread from the troubled subject to every affected city, harbor
  (Service), and gate (Ingress), with a count in a banner. A *node* cascades
  node → hosted workloads → their Services → Ingresses ("if this province falls,
  these cities lose citizens and their routes go dark"); a *workload* walks to
  its own Services + Ingresses. It's the SRE practice of topology-driven impact
  isolation — and KuberNation already owns the topology, so it's a pure graph
  walk (no traffic data / service mesh needed). We deliberately don't invent
  app-level "who calls whom" edges, so a workload with no Service has an honestly
  empty radius. Pure + unit-tested; verified live on kind (a node → 12 affected;
  `web` → its Service + Ingress).
- **Live cpu/mem trend sparklines.** Metrics-server samples now accumulate into a
  bounded history ring, and the node ("province") window draws a small trend
  sparkline under each cpu/mem gauge — usage ÷ allocatable over the last ~15
  minutes, coloured by the same pressure bucket as the gauge — so you see whether
  a node is climbing, not just where it sits now. The right column's STATUS
  section gains cluster-wide cpu + mem sparklines (self-scaled) as an at-a-glance
  "is the realm heating up". Both appear only when metrics-server is reporting;
  the history is retained across a transient poll blip (hidden while down, resumes
  with continuity). The ring + ratio math are pure + unit-tested; verified live on
  kind with metrics-server.
- **Port-forward a pod to `127.0.0.1` (`kubectl port-forward`, in the GUI).**
  Hover a pod in a city's CITIZENS or a node's GARRISON list and click **fwd** to
  open a local tunnel; the **FORWARDS** section of the right column lists every
  live forward (`:local>pod ns/pod`) with an **x** to stop it. The default port
  is resolved for you — the pod's declared `containerPort`, else a numeric
  `targetPort` of a Service that selects it. It's **not a cluster write** (so it
  stays out of the one write file), but it's gated like one: RBAC-pre-checked
  (`create pods/portforward` via `SelfSubjectAccessReview` — the button shows
  *locked* without it), explicit, and individually stoppable. Tunnels tear down
  cleanly on stop and on a context switch (the listener + every in-flight
  connection abort together). Port resolution is pure + unit-tested; the tunnel
  was verified live (HTTP 200 through a forward to `web`, then torn down).
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
- **Intro splash.** On launch the GUI now holds the full KuberNation scene for
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
- Completed the **KuberNation** rename: the codename is gone from every
  identifier — crate names (`kubernation-core` / `kubernation` /
  `kubernation-gui`), the `kubernation` binary, the kind cluster, kubeconfig
  context, config/log paths, and the sample namespace.
- Neutralized the trademark surface: internal game-name labels and the
  default palette were renamed to generic terms (4X / `atlas`); a single
  nominative attribution homage is kept, and a trademark disclaimer was added
  to the README and `--help`.
- Regenerated all GUI screenshots in `docs/` so the window chrome reads
  **KuberNation** (they predated the rebrand and still showed the codename).

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
- Branded the app **KuberNation** (display name).

## [0.1.0]

Initial MVP: the Kubernetes data layer, the observed-world model, and the
map / city / attention TUI — the cluster as a living world.
