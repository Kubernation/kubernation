# Advisors ▸ Substrate — report

**Prompt:** "KuberNation — Advisors ▸ Substrate" (implementation prompt, 2026-09-01).
**Shipped:** v1.37.0.
**Gate:** the tab and the overlay never disagree about which nodes have gaps —
**PASSED**, with both discrimination checks, on the 100-node churn fleet.

The overlay answers *which nodes* and the province window *which daemonsets,
for this node*. Neither answered the fleet question — "is my log agent
everywhere?" — which is one row per daemonset. This round adds that table as an
eighth Advisors tab, built from the `SubstrateReport` that already existed. No
new report, no change to prevalence, no reading of nodeSelectors, no change to
the overlay or the province window (§7 of the prompt, all held).

---

## §1 — what already existed: claims checked by reading

| # | Claim | Verdict |
|---|---|---|
| 1 | `SubstrateReport` is a whole-cluster rollup with `expected`, `missing_by_node`, `nodes_total`, `nodes_with_gaps`, computed on `Models` per tick | **TRUE** — `Models.substrate`, unfiltered, built in `Models::build_filtered` |
| 2 | Identity is `namespace/name` | **TRUE** (the v1.5.0 review fix) |
| 3 | Prevalence ≥ 80% → expected; the Almanac says it is inference | **TRUE** — but the Almanac's wording was its own sentence, not shared (see §2) |
| 4 | At n ≤ 4 no gap is representable | **TRUE**, and now a function (`floor_binds`) pinned against the report at every n in 1..=25 |
| 5 | Overlay levels 0 / 1 / 2+; empty `expected` falls back to terrain | **TRUE** — `overlay_pair` reads `missing(node).len()` and `has_data()` |
| 6 | The province window lists what a node runs and what it is missing | **TRUE** — `node::substrate_lines`, unchanged here |
| 7 | `ReportCache` is the memo; a lint keeps build calls inside it | **TRUE** — `hack/check-advisor-memo.sh` |
| 8 | "Since `SubstrateReport` is already on `Models`, the slot may be trivial … check which before writing a builder" | **Checked: read directly.** The report is computed once per tick for the map renderer and lives on `Models`; a `ReportCache` slot would memoize a field read. The tab's match arm reads `s.hot.models.substrate`, and the reason is recorded at the arm. The memo lint still passes — there is no build call to keep inside anything |
| 9 | §4.1: "two DaemonSets at 98/100 and 99/100 … the tab should show two rows, three nodes" | **TRUE of v1.5.0's fixture, and no longer what the fleet shows.** The churn fleet has carried `churn/node-agent` (100 desired) since the A3 fixture, and its allocatable-less node (`hack/churn/lib.sh:131`, `churn-sys-g2-000`) cannot schedule *any* daemonset pod — so the same two-daemonset fixture now yields **three rows and three nodes**, and `sys-g2-000` is missing all three. The gate was run against that, not against the prompt's number |

Claim 9 is the round's inherited-claim finding: accurate about the fixture it
described, silent about what else the fleet has grown since.

---

## §2 — what was built

**Core, one authority for words two surfaces share** (`state/substrate.rs`):
`prevalence_note()` — the heuristic stated in one sentence, used verbatim by the
tab AND the Almanac (which used to carry its own paraphrase); `floor_binds(n)`
— `ceil(0.8n) >= n`, the arithmetic the Almanac had described in prose; and
`floor_nodes()` — the largest such n (4), so the Almanac's "5 nodes is the
smallest fleet…" is computed, not typed. A test runs `coverage_report` at every
size 1..=25 with one daemonset on n−1 nodes and asserts `floor_binds` agrees
with whether a gap appeared — the function cannot drift from the report.

**GUI, the pure draw-decision fns** (`gui/advisor.rs`):
- `substrate_rows(report, not_ready) -> Vec<SubstrateRow>` inverts
  `missing_by_node` into one row per expected daemonset — `on = nodes_total −
  missing.len()`, missing nodes sorted, each tagged when NotReady. Row order is
  `expected`'s, which is sorted, so rows do not reorder between ticks.
- `substrate_lines(report, not_ready)` — the page's text. Four states, each
  saying which it is: *no nodes observed yet*; *N nodes: no gap is representable
  at this size* (the floor, with the arithmetic and the prevalence note);
  *N nodes: no daemonset reaches the fleet bar* (an empty `expected`, explicitly
  "not 'all covered'"); and the table, whose headline is Good only when
  `nodes_with_gaps == 0` **and** something was expected. The prevalence note is
  stated in all three non-trivial states. A NotReady node is tagged "the node is
  the story" rather than dropped — dropping it would hide a real gap on a node
  that comes back without its daemonset.
- `page_substrate` reads `Models.substrate` and `Models.map` (for readiness —
  the one fact the report lacks; see §6 q8). It walks the map only when there
  are gaps to tag.
- **Caveats wrap; rows truncate.** Every other advisor page truncates each
  line to the window width with `fit_width`. The first kind capture cut the
  prevalence note at "…beca…" — a caveat cut mid-sentence is not stated. `Dim`
  lines (the caveats) now wrap through `almanac::wrap` (made `pub(crate)`);
  rows keep truncating. The other pages' footers still truncate; that is a
  pre-existing pattern and a candidate for a small pass, noted not done.

Wiring: `AdvisorTab::Substrate` (index 7, with a `LABELS` array pinned to
`ALL` by test so a tab cannot be added without a label), key `8`, Advisors menu
item, `--advisor substrate`, gui-smoke `advisor-substrate` (59 states).

**Instrument:** `kubernation-core/examples/substrate.rs` prints the report for
a context headlessly — the same shape as `drain.rs` and `rightsize.rs`. It
exists because the gate needs something exact to check a screenshot against,
and the tab is the thing under test.

---

## §3 — tests and the mutation floor

Five new tests (`floor_binds_agrees_with_the_report_at_every_size`,
`every_advisor_tab_has_a_label_at_its_index`,
`substrate_rows_invert_by_daemonset_and_count_nodes`,
`substrate_lines_name_each_empty_state`,
`substrate_tab_and_overlay_agree_on_which_nodes_have_gaps`). The last is the
anti-drift test the gate rests on: six nodes, three daemonsets (`cni` on five,
`logs` on five, `rare` on two), and it asserts the set of provinces
`overlay_pair` colours away from idle land equals the set of nodes
`substrate_rows` lists — {n4, n5} — and that `rare` appears in neither.

| | Mutation | Test | Result |
|---|---|---|---|
| M1 | tab derives its rows with a different threshold than the report (only nodes missing ≥ 2) | agreement | **CAUGHT** |
| M2 | empty `expected` renders as an empty (clean) table | empty states | **CAUGHT** (second run) |
| M3 | the floor branch removed — the dev cluster shows a clean table | empty states | **CAUGHT** |
| M4 | a `coverage_report` build call placed in the draw | `check-advisor-memo.sh` | **CAUGHT** |
| M5 | a tab added to `ALL` without a label | label test / compile | **CAUGHT** |

Each asserted applied (`count(old) == 1`, replacement present). M2's first
run reported NOT APPLIED — `cargo fmt` had reflowed the `format!` the target
matched — and was re-targeted onto the bare `if !r.has_data() {` condition,
which fmt does not move. The seventh such catch this stretch.

**A test silently stopped running, and every green run this round was one
short.** Inserting the agreement test left a stray `#[test]` above its doc
comment and took the attribute that belonged to the next test,
`substrate_overlay_recedes_when_clean_and_escalates_by_gap_count` (the v1.5.0
overlay's own test). rustc treats both a duplicated attribute and an unused fn
as warnings, so `cargo test` reported 641 passed with the orphan never
compiled into the runner; only clippy under `-D warnings` refused. Re-armed, it
passes — so the overlay behaviour it pins still holds under this round's
changes, which is the one thing the run count could not have told me. The
v1.29.0 finding, recurring: a green suite does not say which tests ran.

---

## §4 — the gate

### Failure criteria, stated before the run

1. The tab lists a node the overlay does not colour, or vice versa.
2. The dev cluster shows a clean table with no explanation.
3. An empty `expected` renders as "all covered".
4. The table is keyed by node.

None fired. A fifth was added *at* the run, having fired: a caveat truncated to
meaninglessness by the window width (§2).

### The fixture, and a corrected expectation

`churn/log-agent` (nodeAffinity excluding `churn-sys-g2-000`, `-001`) and
`churn/node-exporter` (excluding `churn-edge-g1-000`) were applied to the churn
fleet beside the pre-existing `churn/node-agent`. **Expectation, stated first:**
three rows; gaps at sys-g2-000 [log-agent], sys-g2-001 [log-agent],
edge-g1-000 [node-exporter]; node-agent 100/100 clean.

The ground truth from `kubectl` corrected it before any capture was read: the
allocatable-less node cannot schedule any daemonset pod, so **`sys-g2-000` is
missing all three**, and `node-agent` is 99/100. The corrected expectation:

| daemonset | on | missing from |
|---|---|---|
| `churn/log-agent` | 98 / 100 | sys-g2-000, sys-g2-001 |
| `churn/node-agent` | 99 / 100 | sys-g2-000 |
| `churn/node-exporter` | 98 / 100 | edge-g1-000, sys-g2-000 |

**An instrument failure in that ground truth, caught and recorded.** My node
count read `node-agent` as "on 100 nodes" while also missing one — which I
first explained as a pod outliving its Node object (the ghost the v1.5.0 review
bounded the numerator against). Wrong. A pod with no `nodeName` prints an
empty line, and `sort -u` counted the blank as a node; the pod is simply
`Pending / Unschedulable`. No ghost exists on the fleet (checked directly: no
pod names a node outside the live list). The report never saw the blank — it
skips unscheduled pods — so the *product* was right and my *instrument*
fabricated a mechanism to explain its own artefact. Same class as the A3-pre
`"?"` placeholder.

### Baseline: three sources agree exactly

`kubectl` (per-node presence), the headless example, and the rendered tab all
name the same three nodes with the same per-daemonset attribution. The overlay
headline reads `3/100 nodes with gaps`; centred captures show `sys-g2-000`
**red** (three gaps → the 2+ bucket), `edge-g1-000` and `sys-g2-001` **amber**
(one each). Colour by count and names by count come from one `missing(node)`.

### Discrimination 1 — a daemonset drops below the bar

`log-agent`'s affinity was changed to exclude the whole `sys` pool (30 nodes):
70/100 < 80%. Within 4 s: headless **2 fleet-wide · 2 of 100 with gaps**,
`log-agent` absent, gaps edge-g1-000 [node-exporter] and sys-g2-000
[node-agent, node-exporter], **`sys-g2-001` gone** (its only gap was
log-agent's). The tab rendered exactly that; the overlay headline went
3/100 → 2/100 and zone B's amber band (sys-g2-001) vanished between the two
fit-zoom captures. Restoring the fixture returned the exact baseline set.

**The first attempt never applied.** The chain stored `kubectl --context …`
in a variable and called `$K …`; zsh does not word-split an unquoted variable,
so every call was "command not found", the wait loop ran out its 120 s, and the
script photographed the *unchanged baseline* under discrim filenames. Caught by
reading the task's output rather than its captures; the captures were deleted
and the run repeated with a shell function. Had the pictures been read first
they would have shown a perfectly plausible "discrim" state that was the
baseline.

**And a third instrument slip, in the gate summary itself:** my gate runner
printed `exit=0` beside two failing gates, because it read `PIPESTATUS`
(bash) where zsh has `pipestatus`. The error text beside it was the evidence;
the number was decoration. Three instrument failures in one round, each caught
by reading the source of the number rather than the number.

### Discrimination 2 — the dev cluster

`--advisor substrate --context kind-kubernation`: **"4 nodes: no gap is
representable at this size"** with the arithmetic and the prevalence note in
full. The headless example confirms why this state must precede the
`has_data` check: at n = 4 `expected` is *not* empty (`kube-system/kindnet`,
`kube-system/kube-proxy` at 4/4) — a table would have shown two clean rows
and read as fortified.

### Not shown in the fleet-wide frame

At the fit zoom the play area clips zone A's west end (the zoom floor of
0.30 against a wanted 0.279 — the v1.29.0 limit), and both remaining gap nodes
sit in zone A. The centred captures are what show them; the clip is
pre-existing and unrelated.

---

## §5 — standing questions

**2 — unknown, or fabricated?** Four states, each named: no nodes yet; the
floor binds; no daemonset reaches the bar; the table (Good only when something
was expected and nothing is missing). The floor state is reached on the dev
cluster *with a non-empty `expected`*, which is exactly the case where an
unguarded table would have fabricated a clean fleet.

**3 — two sections constraining one behaviour?** The tab and the overlay both
summarise `SubstrateReport`, one by name and one by colour bucket. The bucket
and the name both derive from `missing(node).len()` of the same report, and the
agreement test sweeps every province asserting coloured-set == listed-set. The
prevalence sentence had two homes (Almanac prose, and now the tab); it has one.

**8 — true and sufficient?** "`SubstrateReport` is already shaped for it" —
true, and sufficient with one join: `expected` is sorted (stable rows) and
`missing_by_node` inverts cleanly, but the report carries **no node
readiness**, so the NotReady tag reads `MapModel.zones[].nodes[].ready` from
the same `Models`. Recorded rather than added to the report: readiness is the
map's fact, and the report should keep saying only what coverage is.

---

## §6 — what was not done

- The NotReady per-row tag is exercised by unit test only; kwok cannot hold a
  node NotReady (the A-pre finding), so it has no live capture.
- The other advisor pages' footers still truncate; only this page wraps.
- The fixture is committed as `hack/churn/substrate-gaps.sh` (up/down) so the
  gate is re-runnable; it was ad hoc in v1.5.0.
- Deferred, unchanged: reading `nodeSelector`/tolerations to replace inference
  with intent; a minimap tint; warm-cluster substrate.

**Counts:** 642 workspace tests; gui-smoke 59. Cluster left as found (the two
fixture daemonsets deleted; `node-agent` is the fleet's own).
