# Planning handoff — v1.35.0

**Date:** 2026-08-31 · **Head:** `d43e4af`, CI green · **Last tag:** `v1.34.1`
**Supersedes** `docs/kubernation-planning-handoff-v1.32.md` entirely. Its
still-open items are folded into §3 below rather than left to be cross-read; its
method notes (including note 6, on its own staleness) still stand.

---

## 1. State

Clean tree. **460 core / 485 with the `oracle` feature / 148 GUI**, 633 under
`cargo nextest run --workspace`. gui-smoke 57. Clippy clean with and without
features. Zero broken doc links. `make lint` green.

**v1.35.0 is unreleased** — 3 commits past `v1.34.1`, 2 changelog bullets. Small,
so no urgency; noted because the last accumulation reached 23 versions, and
cutting the tag is what caught the stale-lockfile defect that would otherwise have
failed a release.

---

## 2. What happened since v1.32.0

Released **v1.32.0** (signed, notarized, three platforms) and **v1.34.1**. Between
them, work that was almost entirely about *what the app says* rather than what it
does — and then one phase that was capability.

- **The prose audit** (v1.33.0): nine false claims, including a write-surface
  header still describing eviction as a delete, and a field guide naming eight of
  nine map overlays for fourteen versions.
- **Doc links became a build failure** (v1.33.1). The seven that looked cosmetic
  had one cause: two doc comments on one module make rustdoc resolve a file's own
  links in its parent's scope.
- **The Windows archive** (v1.33.2) had shipped licence notices missing six of its
  own crates, `schannel` among them, for six releases.
- **Panel wording** (v1.34.0), checked by rendering rather than reading: the
  queue's `×N` now says it counts pods — it was reporting one pod past a
  five-restart threshold in a way that read as one restart — and the cost panel
  says which kind of idle it means.
- **The Oracle** (v1.34.1) stopped claiming to stream before a token arrived.
- **The lockfile guard** (v1.34.1) was made to actually guard; see §5.
- **P90 right-sizing** (v1.35.0): sizing advice moved off a single instantaneous
  reading onto each pod's own usage history.

---

## 3. What is open

Nothing is in flight. Everything below is a choice, and none is blocking.

1. **Memoize the advisor reports.** Newly well-shaped: every advisor tab builds
   its report inside the DRAW, so it runs at frame rate — ~4 ms at the documented
   ceiling, about a quarter of a 60fps frame. Measured, with an A/B establishing
   the cost is the 5000-pod walk and **not** the P90 work added in v1.35.0
   (4.20 ms with it, 4.76 ms without). The pattern exists twice already
   (`browse.rs`'s `Arc::ptr_eq` memo, the posture chip). One change fixes it for
   all six tabs. `rightsizing_report_cost_at_scale` guards it meanwhile.
2. **The PDB deferrals**: a map mark for blocked nodes (a per-node question, and
   the guidance's own call to defer), the workload-side view (*protected, and by
   how much* — a different feature), and a gui-smoke state for the drain line.
3. **The remaining grafts**, each with a shaped extension point: an Advisors ▸
   Substrate tab (`SubstrateReport` is already built and memoized), a CNI
   enforcement probe, warm-cluster parity for several features, Annals brushing
   (needs an identity resolver for its stringly subjects).
4. **One loose end**: the drain line's *blocked* state is recorded as
   **unchecked, not correct** — rendering it needs a live blocking budget.

---

## 4. The consolidation-versus-capability question is answered

The v1.32.0 handoff put this as a fork and leaned on a falling defect rate. Both
halves of that framing turned out wrong.

The rate did not fall: the last stretch produced roughly twenty real findings.
And the fork is false — **v1.35.0 was capability work, and it still found two of
my own errors**: the footer, which I got wrong twice in opposite directions
(understating, then collapsing to the weakest row), and a test fixture whose
*design* made the case under test unrepresentable.

So the useful conclusion is not "keep consolidating" but: **the checking
discipline is what finds things, and it applies to new work as readily as old.**
Choose the next item on value, not on which category it belongs to.

---

## 5. Method notes that earned their keep

**Guards need their own mutation.** Two shipped this stretch and one was **broken
on arrival**: `check-release-targets.sh` used `[a-z0-9-]+`, so `linux-x86_64` and
`windows-x86_64` — which contain `_` — failed the anchor and were skipped. It had
only ever checked macOS, and reported success either way. Caught only by asking it
to fail at the thing it exists for. It now also fails if it extracts fewer
platforms than it should, because *"the parse silently matched less than it
should"* is the shape that recurs, not the character class.

**A gate can be defeated by its predecessor.** CI's `--locked` check was meant to
catch a version bump not written into `Cargo.lock` — and an earlier unlocked
`cargo clippy` in the same job silently corrected the file first. v1.34.1 was
pushed with a stale lock and CI went green, while the release job (whose first
cargo call *is* `--locked`, on a fresh checkout) would have failed on all three
platforms. Found by running the pre-tag checks by hand.

**A fixture's design can make the negative case unrepresentable.** Sharper than a
fixture merely lacking a case: `set_pod_history` seeded one pod per call and
replaced the metrics map, so a two-pod test collapsed to one — and the mutation
"a row claims its longest window" survived twice, because with one pod longest and
shortest are the same number.

**Render, don't read, for anything an operator reads.** Neither panel-wording
defect was visible in source review; both were only wrong *as seen on screen,
beside their neighbours*.

**Instrument failures reached nineteen catalogued**, three of them in this stretch
— including my own licence-drift check and a guard written minutes earlier. The
rule is stable enough to state plainly: **a check that passes has not told you it
works.**

---

## 6. VOR

Adopted this stretch for symbol-shaped questions; `docs/reports/vor-feedback.md`
is written for the VOR session and has been re-checked twice against their
updates. Both substantive findings are fixed — `find_references` now returns
`call_sites`, including the path reference (`and_then(Foo::bar)`) that has no call
node and that a grep for `primary()` cannot see.

Worth carrying: of my four smaller criticisms, **two were simply wrong**, and §1's
framing was unfair because the tool's description already documented the fields
its implementation lacked. The report keeps them rather than deleting them, so the
correction is visible.

Current split: **VOR for callers, blast radius, definitions, reading one body;
grep for prose, config, workflows.** The prose audit found nine defects VOR could
not have found, and one it could have — had it answered at call-site granularity,
which it now does.
