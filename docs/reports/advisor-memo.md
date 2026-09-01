# Memoizing the advisor reports

**From:** the "Memoize the Advisor Reports" prompt
**Follows:** `docs/reports/p90-rightsizing.md` §5, which measured the cost and
deferred the fix
**Version:** 1.36.0 · **Date:** 2026-08-31

The advisor reports are built once per snapshot instead of once per frame. The
memo took twenty minutes; §2.1's enumeration and §3's mutations were the work,
and one of them found that the memo could be bypassed with the whole suite green.

---

## 1. The prompt's premise, corrected

**It is not six reports across six tabs.** Read at the call site: **Posture and
Cost are already memoized** on the snapshot by the net thread, and the advisor
renders those values rather than building them. The per-frame rebuilds were
**six report calls across five of the seven tabs** — Network builds two
(`network_report` and `netpol::coverage_report`).

Everything else in §1 held: ~4 ms at the ceiling, and the A/B establishing that
cost is the 5000-pod walk rather than the P90 work.

---

## 2. §2.1 — the input list, per report

Written down, as the acceptance artifact.

| report | inputs |
|---|---|
| `health_report` | `&ObservedWorld` |
| `storage_report` | `&ObservedWorld` |
| `network_report` | `&ObservedWorld` |
| `netpol::coverage_report` | `&ObservedWorld` |
| `rightsizing_report` | `&ObservedWorld` |
| `harden::hardening_report` | `&ObservedWorld` |

**Every one takes `&ObservedWorld` and nothing else.** The prompt's candidate
list resolves as:

- **Namespace filter — NOT an input.** The reports take no filter, and advisors
  are deliberately cluster-wide ("an advisor reports on the whole realm"). Proved
  rather than assumed, by a test (§3).
- **P90 window / history — an input, via `ObservedWorld`.** `P90_MIN_SAMPLES` and
  `HISTORY_CAP` are consts, not runtime settings; the per-pod rings live behind
  `ObservedWorld.metrics`, so they are covered by whatever covers the world.
- **Cost basis / pricing — not in scope.** `cost_report` is the one report taking
  a second argument, and it is already memoized with its rates applied.
- **`oracle` feature — no.** None of the six is gated.
- **Runtime-changeable settings (the ageing window precedent) — no.** None of the
  six reads one.

---

## 3. §2.2 — why the key covers everything

The key is `Arc::ptr_eq` on the snapshot's `Models`.

`ObservedWorld` is **live shared state** — reflector `Store`s and an
`Arc<Mutex<Metrics>>` — so a key over it would be worthless if the world could
move without the key moving. It cannot, and this was verified by reading rather
than assumed:

- the delta sink sets `dirty` on **every** `WorldDelta`, from every watcher —
  reflectors, metrics, events, custom stores, and the `Pdbs` flag added in
  v1.31.0;
- the net loop rebuilds and republishes `Arc<Models>` whenever `dirty` is set or
  the filter changed;
- the SLO sampler forces a rebuild every few ticks besides, so an idle cluster
  still republishes.

So nothing a report reads can change without the key changing. Stated at the
key's definition, per §2.2's requirement that a derivation be written down rather
than relied on.

**The filter is doubly safe** and the report says so: it is not an input at all,
and were that ever to change, a filter change also republishes `Models`. The key
errs toward recomputing, never toward serving stale.

**One honest semantic change:** the reports now reflect the world as of the
snapshot rather than as of the frame. That is ≤250 ms of staleness, and it is
exactly what Posture and Cost have always done.

---

## 4. THE FINDING — the memo could be bypassed with every test green

§3's mutations on the key all failed correctly (drop the snapshot; never
invalidate; invalidate only one slot). Then the §6.1 discrimination mutation —
**disable the memo at the draw site**, `&health_report(obs)` in place of
`c.health(obs)` — **survived.**

`ReportCache`'s tests pin the key and the invalidation. Nothing pinned that the
*draw* uses it, and `Advisor::draw` is GL-driven with no test module. Same
structural limit as D2 §3.4 and `progress_row` before it: **no behavioural test
can observe code in a function that has none.**

Two responses, both taken:

1. **The build calls moved into `ReportCache`'s impl** — `c.health(obs)`,
   `c.rightsizing(obs)` — so the draw contains no build call to get wrong.
2. **`hack/check-advisor-memo.sh`** asserts they stay there, in `make lint` and
   CI beside the conversion-authority and release-target guards. Verified to
   bite: re-applying the bypass now fails the lint and names the line.

Without this, a future edit returns every tab to a per-frame rebuild and nothing
reports it — the silent-regression shape this codebase keeps paying for.

---

## 5. §4.1 — three memo homes, and why that is right

`SubstrateReport` is **not** the `browse.rs` pattern. There are three:

| home | computed | for |
|---|---|---|
| `Models.substrate`, `.coverage`, `.pools` | per tick, in core | the MAP renderer, every frame |
| `WorldSnap.posture`, `.cost` | per tick, in the net thread | the sidebar chip, every frame |
| `browse.rs` / `ReportCache` | per snapshot, in the view | modals, only while open |

**Not unified, deliberately.** The split is principled: always-visible surfaces
must have their value ready every tick; an on-demand modal should cost nothing
while closed. Folding the advisor reports onto `WorldSnap` would build five
reports every tick for tabs nobody opened — more expensive than the per-frame
rebuild it replaces, for most sessions.

So the deferred Advisors ▸ Substrate tab inherits `ReportCache` (add a slot), not
a fourth shape.

---

## 6. §4.2 — the cost guard still guards

`rightsizing_report_cost_at_scale` lives in **core** and calls
`rightsizing_report` directly. The memo is in the GUI crate and cannot reach it,
so the guard still measures the uncached path — exactly what it was written for.
Checked rather than assumed, because a guard that passes for the wrong reason is
this stretch's recurring failure.

---

## 7. The gate

**Failure criteria, stated in advance:** a filter or setting change serving the
previous answer; frame cost unchanged with the memo enabled; the cost guard
passing on the cached path; six tabs ending up with six memos. **None occurred.**

**Frame cost, as arithmetic over measured values** rather than a flaky GUI timing
test. `a_report_is_built_once_per_snapshot_not_once_per_frame` counts builds
directly:

| | builds | frame cost |
|---|---|---|
| before | 1 per frame | ~4 ms — a quarter of a 60 fps frame |
| after | 1 per snapshot (~250 ms ≈ 15 frames) | 0 ms on 14 frames in 15 |

**§6.1's discrimination check** is that same count under mutation: with `sync`
forced to always invalidate, 60 frames produce 60 builds and the test fails. So
the instrument distinguishes a working memo from a memo-shaped no-op, which is
what it is for.

**The A/B re-run:** `rightsizing_report` is 3.84 ms/call (was 4.20 ms) — the same
number within noise, so the cost has not moved somewhere unexpected; it is still
the 5000-pod walk, now paid once per tick. `scale_rebuild` reads 9.24 ms against
7.0 ms earlier today, inside the 7.0–9.2 ms range seen across today's runs and far
under the 100 ms budget; the model rebuild is untouched by this change.

---

## 8. Standing questions

**8 — true *and* sufficient?** The load-bearing one. "Each report is a pure
function of `ObservedWorld`" is true, and on its own **insufficient**: it says
nothing about whether `ObservedWorld` can move without the key. §3 is that
missing half, established by reading the delta sink and the rebuild gate.

**4 — consumers on the old meaning?** Enumerated rather than assumed: the core
report fns are also called by the net thread (per-tick posture, and hardening for
the attention concerns), the Oracle bundle, the postmortem and test fixtures.
**None goes through the advisor's draw**, so nothing relied on the per-frame
rebuild. (net.rs building `hardening_report` every tick for the concern loop is a
separate per-tick cost, noted and out of scope.)

**2 — unknown, or fabricated?** A miss is a rebuild, never a default; pinned by
`a_cache_miss_rebuilds_rather_than_defaulting`, which asserts the filled value
matches a freshly-built report and is not an empty one. An empty report would
render as a clean bill of health nobody earned.

---

## 9. Acceptance

- [x] §2.1's input list written down, per report (§2)
- [x] The key covers every input, with the derivation stated at its definition (§3)
- [x] §3's mutations run for every input; the filter proved not-an-input by test
- [x] `SubstrateReport`'s pattern checked — three homes, split on principle, recorded (§5)
- [x] §4.2 decided: the guard still measures the uncached path (§6)
- [x] Gate at the 5000-pod ceiling with the discrimination check (§7)
- [x] Failure criteria stated before the run
- [x] The A/B re-run and reported (§7)
- [x] Standing questions answered (§8)
- [x] `cargo nextest` green — 637 tests

**Mutation floor: five, one survived and was closed structurally** (§4) — the
survivor being the one that mattered, since it is the one a future edit would
reproduce.
