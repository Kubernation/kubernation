# KuberNation — Handoff to planning

**Covers:** v1.17.1 → v1.29.0 (32 commits)
**Date:** 2026-08-19
**State:** tree clean · 434 core (443 with `oracle`) + 139 GUI tests · gui-smoke 57 ·
44 reports · churn fleet at its 100-node reference size · **nothing in flight**

---

## 1. What this stretch actually was

Not feature work. **Consolidation: finding and retiring claims the app was making
that were not true.** That was not the plan going in — it emerged because each
phase's pre-check kept finding the previous phase's premise had shifted.

The shape of what was found is the useful part:

| where the defect lived | count |
|---|---|
| **documentation and panel wording** | 4 |
| rendering geometry | 3 |
| model / derivation | 2 |
| dev instruments (measuring the product) | 3 |

**Four of the twelve were in prose**, not code: the field guide saying a city
sits where *most* of its pods are (it is the plurality), the same guide's two
pages disagreeing with each other, `SidebarHit.focus_impact` documenting a
selection it deliberately does not make, and `--dump-positions` naming a field
`node` that is not the node a reader would assume.

**Three were in the instruments** — the things this project measures itself with.
That is the recurring hazard, now at fifteen instances across the workstreams: an
instrument emits a plausible number for a reason unrelated to what it claims to
measure. The defence that works is checking whether the metric *can* discriminate
before trusting it, and it earned its keep twice this stretch (a gate that
reported green on a leftover cluster; a colour classifier that saw a fifth of what
it was measuring).

---

## 2. Workstreams, all closed

| | outcome |
|---|---|
| **A** (stable layout) | closed at A6 earlier; its invariants held under every phase since, and twice ruled out an option |
| **T** (temporal expression) | **refuted at its own kill point.** Failures cluster by workload, not location; even a 100%-pool-confined failure renders as 8 disconnected pieces. Salvage shipped four improvements anyway |
| **D** (coordinated views) | D1 dock · D2 brushing · **D3 closed by measurement** (its problem was already solved) · D4 shrunk to three items |
| **Plurality siting** | closed **without a map change** — the four map-shaped candidates were all unneeded |
| **Two-thirds ocean** | lever B shipped (ground 33% → 55%); lever A closed, not deferred |

**Three phases were killed or shrunk by measuring first**, which is now the
project's most reliable move: D3 (already solved), T2 (premise refuted), and the
plurality item's map candidates (the map turned out to be honest).

---

## 3. Decisions worth not re-deriving

- **A guaranteed whole-world-at-detail view is not achievable** at arbitrary
  cluster size, and is retired as a goal. Lever A (per-zone stride) is *closed* —
  it would have reintroduced instability source 1 at zone granularity. If height
  ever returns, shrinking `EXTENT_CLASSES` is the strictly better option and the
  numbers are in `map-height.md`.
- **A city is a label for a workload, not a claim about its location.** Every
  surface now either says so or says what it means. Established by enumerating
  every surface and reading the code, not by assumption.
- **Two surfaces deliberately answer "should acting here move the camera?"
  differently** (IMPACT flies without marking; the workload table marks without
  flying), each for a stated reason. Pinned by test so a tidy-up cannot unify them.
- **`main.rs` cannot be protected by behavioural tests.** It has no test module by
  the GUI testability policy, so a *re-mirror* there is catchable only by a
  structural lint (`hack/check-conversion-authorities.sh`). *Drift* inside tested
  files is catchable normally. The two halves need different defences.

---

## 4. What is open

**Nothing is in flight.** Three genuine choices, none urgent:

1. **More consolidation, or new capability?** The last stretch found twelve real
   defects, which suggests the seam is productive — but the rate of *user-visible*
   defects is falling, and the last four were documentation.
2. **The map's zoom range**, if the floor ever bites in practice. Retired as a
   guarantee; still a knob.
3. **Deferred grafts**, each recorded with its shaped extension point: an
   Advisors ▸ Substrate tab, per-pod metrics history for true P90 sizing, a CNI
   enforcement probe, warm-cluster parity for several features, Annals brushing
   (needs an identity resolver for its stringly subjects).

---

## 5. Method notes that changed the work

Recorded because they cost time and will again:

- **Verify a guidance doc's claims before building.** Roughly one claim per
  document was false or stale, and stopping to report rather than adapting around
  it changed the work three times.
- **Recency is not verification.** The claim most often wrong was *my own*, from a
  report written hours earlier. Seven consecutive rounds turned on re-examining a
  statement I had just made.
- **Assert a mutation applied.** `cargo fmt` reflowed the target and the
  replacement matched nothing — eight times this stretch. A green suite under an
  unapplied mutation refutes nothing.
- **Guard the guard.** Several tests passed against fixtures that could not
  express the thing under test (a fixture with no island structure; one whose city
  had one pod on one node; view rects where the height constraint never bound).
  Each was closed only after a mutation survived.
- **A test count is not self-verifying.** One test lost its `#[test]` to a bad
  insertion and silently stopped running while the suite stayed green; only
  clippy's dead-code lint noticed.
