# Planning handoff — v1.37.0 released

**Date:** 2026-09-01 · **Head:** `d95ec34` = tag `v1.37.0`, pushed, release
published · **Previous tag:** `v1.36.0`
**Supersedes** `docs/kubernation-planning-handoff-v1.35.md` entirely. Three of
its four open items have shipped; the fourth and the grafts are folded into §3.
Its §4 (consolidation versus capability is a false fork — choose on value) and
§5 (method notes) still stand and are referenced here, not repeated.

---

## 1. State

Clean tree; HEAD is the tag. **460 core / 486 with the `oracle` feature / 156
GUI**, 642 under `cargo nextest run --workspace`. gui-smoke 59. Clippy clean
with and without features, `--locked`. Zero broken doc links (nine private-item
link warnings are pre-existing and allowed — CI denies broken links only).
`make lint` green with all four guards (conversion-authority, release-target,
advisor-memo, licence notices).

**v1.37.0 is released**: CI green on the feature commit and on the roll commit;
the release workflow succeeded on all three platforms plus publish. The macOS
artifact is the signed, notarized `.dmg` (24.5 MB) — the signing path ran —
beside the Linux tarball, the Windows zip and `SHA256SUMS`. v1.36.0 was
released the day before the same way (macOS 8m56s, both notarizations
Accepted). Nothing is unreleased.

---

## 2. What happened since v1.35.0

Three rounds, two releases. One perf item, one check closed, one capability.

- **The advisor reports are built once per snapshot** (v1.36.0). The prompt's
  premise was corrected on reading: not six reports across six tabs but six
  report calls across five of seven — Posture and Cost were already memoized on
  the snapshot. Every report takes `&ObservedWorld` and nothing else, so the
  key is `Arc::ptr_eq` on `Models`; the namespace filter is deliberately NOT an
  input (advisors are cluster-wide), proved by test. One mutation found the memo
  could be **bypassed** — a build call placed in the draw — with the whole suite
  green, so `hack/check-advisor-memo.sh` now confines build calls to the
  `ReportCache` impl. `docs/reports/advisor-memo.md`.
- **The drain line's blocked state closed** (v1.36.1) — the last thing recorded
  as *unchecked, not correct*. The first fixture would have passed vacuously (no
  node was covered *only* by a permissive budget); a one-replica deployment on
  worker3 made all three drain states appear on one cluster. gui-smoke gained
  `drain-blocked`; the `--evict-go` screenshot, which fired before the request
  was sent, was fixed. `docs/reports/drain-line-check.md`.
- **Advisors ▸ Substrate** (v1.37.0): the fleet question — *is my log agent
  everywhere?* — as one row per fleet-wide daemonset, read directly from
  `Models.substrate` (decided by reading: a `ReportCache` slot would memoize a
  field read). Four states each named; the floor state matters on the dev
  cluster precisely because `expected` is non-empty there. The prevalence
  caveat became one shared sentence for the tab and the Almanac. Gate on the
  100-node fleet: `kubectl`, a new headless `substrate` example and the rendered
  tab named the same three nodes; dropping a daemonset below the bar removed
  its row and its gap's colour in the same tick. `docs/reports/advisor-substrate.md`.

---

## 3. What is open

Nothing is in flight. Everything below is a choice, and none is blocking.

1. **"The node is the story", for a node reporting no capacity.** New, from the
   Substrate gate: the churn fleet's allocatable-less node is missing *every*
   daemonset because nothing can schedule on it, so the tab lists it as three
   gaps when the truth is one node fact. The tab already tags NotReady nodes
   this way; "reports no capacity" is one more field on the same `NodeTile`.
   Half a day, and the same reasoning as PDB item 3 (a per-node fact, not a
   per-row symptom). The Almanac's "why a node shows gaps" list would gain the
   case too.
2. **The PDB deferrals**: a map mark for blocked nodes (a per-node question, and
   the guidance's own call to defer) and the workload-side view (*protected, and
   by how much* — a different feature). The gui-smoke state is done.
3. **The remaining grafts**, each with a shaped extension point: a CNI
   enforcement probe; warm-cluster parity for several features; Annals brushing
   (needs an identity resolver for its stringly subjects — the
   `oracle_investigate::validate` shape).
4. **Caveats wrap on one advisor page.** The Substrate page wraps its `Dim`
   caveat lines through `almanac::wrap`; the other seven pages still truncate
   theirs to the window width, so a footer can end at "beca…". About twenty
   lines; could ride with any round.
5. **The decision log has a hole — DONE, same day** (`c18af76`, eight entries
   backfilled from their reports). CLAUDE.md's dated entries stopped at v1.32.0
   and resumed at v1.37.0. Eight rounds — v1.33.0 (prose audit), v1.33.1
   (rustdoc lint), v1.33.2 (about.toml Windows), v1.34.0 (panel wording),
   v1.34.1 (VOR feedback + Oracle), v1.35.0 (P90 right-sizing), v1.36.0 (memo),
   v1.36.1 (drain check) — have reports but no log entry. The log is what a
   fresh session loads; the reports are not. A decision on backfilling; my view
   is yes, briefly, since the project's own convention says to record decisions
   as they are made and these were not.
6. **A shell-instrument convention for `hack/`**, optional (§5).

---

## 4. Carried forward, unchanged

The v1.35 handoff's §4 stands: the checking discipline finds things in new
work as readily as in old, so choose the next item on value. This stretch was
one perf change, one check closed and one capability, and it found the same
class of things as the consolidation rounds did — a test silently not running,
a fixture count inherited past its truth, three instruments emitting numbers for
the wrong reason.

---

## 5. Method notes that earned their keep (new this stretch)

**A green suite does not say which tests ran.** The v1.29.0 shape recurred in
v1.37.0: inserting a test left a stray `#[test]` above its doc comment and took
the attribute from the next test, which then compiled as an unused function and
ran in no green run that round. rustc treats both the duplicate attribute and
the dead function as warnings; only clippy under `-D warnings` refused. Rule:
**run `make lint` before quoting a test count**, not after.

**An inherited fixture count goes stale as the fleet accumulates scenarios.**
The prompt's "two rows, three nodes" was true of v1.5.0's fixture and stale for
a fleet that had since gained `node-agent` and an allocatable-less node. A prompt
should cite the scenario that produces the state, not the number it produced.

**The most dangerous instrument failure comes with an explanation.** My
`kubectl` node count read a Pending pod's empty `nodeName` as a node, and I
explained the phantom — in a message — as a ghost node, the mechanism the v1.5.0
review had bounded the report against. The product was right; the instrument
fabricated a cause for its own artefact, and the cause was plausible because it
was a real mechanism from the project's history. Caught by checking the
mechanism directly rather than the number.

**Shell instruments written ad hoc failed three times in one round**: the
blank-line count above; a `kubectl --context …` stored in a variable that zsh
does not word-split, so a discrimination run applied nothing and photographed
the baseline under the discrim filenames; and a gate summary reading
`PIPESTATUS` where zsh has `pipestatus`, printing `exit=0` beside two failing
gates. Each was caught by reading the source of the number, not the number. A
`hack/` convention would cost little: functions not command variables, `set -u`,
and an assertion that a fixture *changed* before it is photographed.

**A memo with a bypass passes every test that measures the cached path.**
(v1.36.0.) The structural answer was a lint confining build calls to one impl —
the same answer D2-fix reached for conversion authorities.

**Words with two homes drift.** The Almanac had paraphrased the prevalence
heuristic in its own sentence for two versions; the tab would have been a third
home. `prevalence_note()` is now the one source, with `floor_binds` pinned
against the report it describes.

**Instrument failures: nineteen catalogued at v1.35, three more this round —
and one more while writing this document**, when a test-count pattern returned
0 for every crate and was believed for one turn because it printed a number.
The rule has not changed: *a check that passes has not told you it works*, and
a number is not a measurement until its source has been read.

---

## 6. VOR

Not exercised this round: the work was presentation over a report whose shape
was already known, and the questions that arose were prose- and config-shaped
(changelog anchors, scenario scripts, a doc-comment attribute). The v1.35 §6
split stands — **VOR for callers, blast radius, definitions, reading one body;
grep for prose, config, workflows** — and `docs/reports/vor-feedback.md` is
unchanged.
