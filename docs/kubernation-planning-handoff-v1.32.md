# Planning handoff — v1.32.0 released

> **SUPERSEDED by `docs/kubernation-planning-handoff-v1.35.md` (2026-08-31).**
> That document's §3 folds in everything still open here, so read it for current
> state. Kept for the record rather than deleted — §5.6 is about this document's
> own staleness, which is the reason the next one replaced it instead of being a
> fourth amendment.


**Date:** 2026-08-29 · **Tag:** `v1.32.0` (pushed) · **Previous tag:** `v1.9.0`
**Amended 2026-08-29**, later the same day: v1.33.0–v1.33.2 landed after this was
written and closed two of its five open items. See §4 — and §5.6, which is about
this document.
**Supersedes:** `docs/kubernation-planning-handoff.md` for status; that document's
method notes still stand.

---

## 1. What happened

**v1.32.0 is tagged and pushed** — the first release since `v1.9.0`, covering
**54 commits, 16 of them features**, and 53 phase reports.

The changelog roll was itself overdue work: 23 versions of entries had
accumulated under `[Unreleased]`, which is the drift the conventions warn about
by name and which had already happened once before v1.0.0. Rolled into a dated
section, one heading per category, **49 bullets in and 49 out** (counted).

**What the release contains**, by workstream:

- **Workstream A** — the map holds still. Durable layout slots, persistence
  across restarts, succession ageing, a graticule so a position can be named.
- **Workstream D** — drill-downs dock beside the map instead of covering it, and
  the selection became an *identity* rather than a scene coordinate, so it
  survives a reschedule and a cluster growing underneath it.
- **Plurality siting** — the map stopped implying a city's position says where
  its workload runs, across the field guide, the selection panel, the Oracle
  bundle and the measurement dumps.
- **PodDisruptionBudgets** — eviction goes through the eviction subresource so
  the app obeys the budgets it now reports on; a cordoned node names what would
  refuse to let it drain; a drill the cluster refused stops reading as
  resilience.

---

## 2. The two "needs the operator" items — both answered

Both were listed as blocked on a real tag push. Both are now demonstrated.

**Multi-platform CI is proven green.** `fmt · clippy · test` passed on
**ubuntu-latest, macos-latest and windows-latest** in the run triggered by this
push. That had never been demonstrated.

**The signed macOS release path is proven in CI.** All three build jobs and the
publish job succeeded; **v1.32.0 is published** with four assets (Linux
`.tar.gz`, macOS-universal `.dmg`, Windows `.zip`, `SHA256SUMS`).

The macOS-universal job — `lipo`, sign, notarize twice (the `.app` and the
`.dmg`, which is its own code object with its own cdhash), staple both — had
never run outside a local dry run, and its only previous CI attempt failed on
something structurally invisible locally: the imported keychain was not added to
`security list-keychains`, and `codesign` resolves an identity through the search
list. A dev Mac's login keychain is already there, so no local run could catch
it. That fix is now demonstrated. From the run's own output:

```
Current status: Accepted......Processing complete     (the .app)
The staple and validate action worked!
Current status: Accepted........Processing complete   (the .dmg)
kubernation-v1.32.0-macos-universal.dmg: accepted   source=Notarized Developer ID
KuberNation.app:                        accepted   source=Notarized Developer ID
```

That last pair is what a downloading user's Gatekeeper evaluates, checked against
the stapled ticket rather than the network. Total 8m1s, so Apple's queue was
cooperative — it has been observed at ~90 minutes against a normal 1–5, and
`NOTARY_TIMEOUT` defaults to 45. A timeout fails `build`, and `publish` depends
on `build`, so there is no half-published state — just re-run.

---

## 3. THE FINDING — CI has been red for ten days, and it is not what it looks like

The `third-party license notices` job has failed on every push to `main` since
**2026-08-18**. It went unnoticed because all work has been local, and the other
three CI jobs pass.

**It is not dependency drift.** Diagnosed by reproducing it rather than reading
the log:

| | committed file | what CI generates |
|---|---|---|
| crates listed | **208** | **208** |
| crate → license attribution | — | **identical, 0 differences** |
| `## ` section headers | 110 | 129 |
| header counts | MIT 199 · Apache 9 · ISC 3 | MIT 202 · ISC 18 · Apache 10 |

**Cause: cargo-about version skew.** The workflow runs `cargo install --locked
cargo-about`, where `--locked` pins *cargo-about's own dependencies*, not
cargo-about itself. It has moved 0.9.0 → 0.9.2, and 0.9.2 emits a separate
license section per crate rather than merging crates that share a license text.
Reproduced exactly by installing 0.9.2 into a throwaway root and regenerating.

**Legal impact: none.** No crate is unlisted and no crate's license attribution
changed. The difference is section granularity — arguably 0.9.2 is *more*
precise, since it reproduces each crate's own copy of the licence text verbatim.

**Recommended fix — pin the tool, then regenerate.** The guard exists to detect
*dependency* drift; as written it also fires on *tool* drift, which will recur on
every cargo-about release. Pinning the version in `ci.yml` makes the guard
reproducible; regenerating with that pinned version makes it green. Two lines and
a regenerated file. **Not applied** — it touches a legal-notices artifact that
ships in every release tarball, so it is offered rather than assumed.

**One near-miss worth recording.** My first local check of this guard reported
DRIFTED. That was my own instrument: I captured with `>` where CI uses `-o`, and
the whole difference was a trailing newline — precisely the artifact CI's
`diff -uB` exists to ignore. Had I trusted it I would have "fixed" a file that
was already correct. Seventeenth catalogued case this stretch of an instrument
producing a plausible answer for an unrelated reason.

---

## 4. What is open

**Nothing is in flight in the code.** Working tree clean; **453 core (478 with
the `oracle` feature) + 145 GUI**, 623 under `cargo nextest run --workspace`;
gui-smoke 57; clippy clean with and without features; zero broken doc links.

**Closed after this was written** (§5.6):

- ~~The licence guard~~ — **v1.33.1**. Pinned cargo-about, since `--locked` fixes
  its dependencies and not the tool; the guard now measures dependency drift.
  Diagnosing it turned up the real cause of seven "cosmetic" doc links: a module
  with two doc comments makes rustdoc resolve the file's own links in the
  parent's scope. `broken_intra_doc_links` is now denied and CI runs rustdoc.
- ~~`about.toml` omits Windows~~ — **v1.33.2**. The Windows zip had shipped
  notices missing six of its own crates, `schannel` among them, for six releases.
  Guarded, and the guard was broken on arrival — see §5.6.

**Still open, all choices rather than debts:**

1. **More consolidation, or new capability?** The evidence has moved further
   since this was drafted. The claim it argued against — that the defect rate was
   falling — is now clearly false: the two days since produced roughly fifteen
   real findings (three live defects in the PDB round, nine false claims in the
   prose audit, two more plus a broken guard in the lint pass). The seam is
   productive. What is genuinely unknown is how much longer it stays that way.
2. **The prose audit's second pass.** Its §5 capped the first deliberately — *"a
   second pass is cheaper than a first pass that never finishes"* — and named a
   gap it could not close: **§2's second kind, panel and concern wording**, the
   surface an operator reads during an incident. Checking it needs rendering
   against a live cluster, not reading source, which is why the first pass
   recorded it unmeasured.
3. **Deferred grafts**, each with a shaped extension point: an Advisors ▸
   Substrate tab, per-pod metrics history for true P90 sizing, a CNI enforcement
   probe, warm-cluster parity for several features, Annals brushing (needs an
   identity resolver for its stringly subjects).
4. **The PDB round's deferrals, with reasons**: a map mark for blocked nodes (a
   per-node question, and the guidance's own call), the workload-side budget view
   (*protected, and by how much* — a different feature), and a gui-smoke state for
   the drain line (it renders only against a live blocking budget).

---

## 5. Method notes from this stretch

- **Standing question 5 earned its keep again, on my own claims.** Item 2 of the
  PDB work found the RBAC defect *because* it re-read item 1's claim, written the
  previous day, that item 1 was complete. Recency is not verification.
- **Two guidance claims were true and insufficient**, which is a distinct failure
  mode from a false claim and is not caught by verifying claims one at a time:
  `selector_matches` has the right expression semantics and the opposite null
  case; `NodeTile.pods` exists and carries no labels.
- **`kubectl auth can-i create pods/eviction` asks the wrong question** — it
  parses `eviction` as a resource *name*. It answers `yes` as admin, which is the
  right answer to a different question, and it nearly validated a fix.
  `--subresource=eviction` is the correct form.
- **The mutation floor keeps measuring half a rule.** Three times this stretch a
  fixture could express the positive case and not the negative one: the
  cross-namespace memo key, the silent-when-drainable arm, and (earlier) the
  identity/position agreement. The fix is always the same shape — make the
  fixture able to express the thing being denied.
- **Stating failure criteria before a gate is what turns a detail into a
  finding.** The truncated selection line fired a criterion written in advance;
  without it I would have looked past a screenshot that "showed the feature
  working".

**6. This document went stale the day it was written, and that is the point.**
Two of its five open items were closed hours later, so a reader taking §4 at
face value would have had a wrong list. That is precisely what the prose audit
spent the same day fixing elsewhere, and precisely what its §1 predicts: *the
claim most often wrong is the author's own, from the same week.* A handoff is a
claim-bearing surface by §2's first kind — someone acts on it — and nothing
compiles it.

The correction here is the audit's own §4 shape: state what is true now, and say
why the omission is deliberate rather than deleting the history. The rows are
struck through rather than removed, so a reader who remembers the old list can
see it was answered instead of wondering whether it was dropped.

The general lesson is narrower than "keep documents updated": **a document that
lists open work has a shelf life measured in commits, not days**, and should say
what it was true *of* — hence the amendment line under the title.
