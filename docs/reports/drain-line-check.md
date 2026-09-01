# v1.36.0 cut, and the drain line's blocked state closed

**From:** the "Cut v1.36.0, then close the drain-line check" prompt
**Version:** 1.36.1 · **Date:** 2026-09-01

Two items. The tag went out clean; the drain check closed the last thing this
project had recorded as **unchecked, not correct**, and turned a deferred
gui-smoke state into a captured one.

---

## 1. The tag

**§1.1 — `--locked` in an order where it cannot be defeated.** The v1.34.1
finding was that a gate can be defeated by its predecessor, so this was run in
the honest form: a fresh `git worktree` at HEAD, with `cargo metadata --locked`
as the **first** cargo invocation in that tree. In sync at 1.36.0.

**The changelog roll counted both ways** — 3 bullets in, 3 out, then re-counted
from the written file rather than from the script that wrote it.

**`make lint` green**, including all four guards now in it: conversion-authority,
release-target, advisor-memo, and the licence-notices check.

**§1.2 — the licence guard held.** The 0.9.2 pin is in `ci.yml`,
`x86_64-pc-windows-msvc` is still in `about.toml`, the regenerated notices match,
and `schannel` — the crate whose absence was the original defect — is present.

**§1.3 — the release job, not just CI.** All four CI jobs green; all three builds
plus publish succeeded. macOS took 8m56s, so neither historical failure occurred:
the keychain identity imported, and both notarizations came back `Accepted` with
`source=Notarized Developer ID`. Four assets published.

---

## 2. The drain check

### 2.1 A fixture that can fail

The pre-check's two budgets were not enough on their own: with `web` and `db`
both on workers 1 and 2, **no node was covered *only* by a permissive budget**, so
§2.2's second bullet would have passed vacuously. Added a one-replica deployment
pinned to worker3 with the permissive budget, which produces all three states on
one cluster at once — the arrangement that stops the check passing because
everything reports blocked.

### 2.2 What was observed

Expectation stated before the run: worker and worker2 blocked by `web-strict`;
worker3 drainable; control-plane drainable. Exactly that:

```
budgets: 2 read
  ok   kubernation-control-plane — no budget blocks a drain
  STOP kubernation-worker  — draining blocked by kubernation-demo/web-strict
  STOP kubernation-worker2 — draining blocked by kubernation-demo/web-strict
  ok   kubernation-worker3 — no budget blocks a drain
```

**§2.4 — rendered, beside its neighbours**, not read in source:

| state | province window | SELECTION |
|---|---|---|
| blocked | `drain: draining blocked by kubernation-demo/web-strict` (red) | `drain: blocked by` / indented name (red) |
| drainable | `drain: no budget blocks a drain` (dim) | *silent* |
| unknown | `drain: disruption budgets not read - drain cost unknown` (amber) | `drain: budgets not read` (amber) |

Three states, three colours, three wordings — distinguishable at a glance and in
text. Read in context, `healthy` above `drain: draining blocked` does **not**
contradict: the node is healthy, the budget is what refuses. On the city window
the drain line sits under `on province kubernation-worker`, so the province
attribution the plurality round added is what keeps it honest there.

**§2.2's fourth bullet — the line and the button agree, in one frame.** A real
eviction attempt against a pod under `web-strict`:

```
toast:     kubernation-demo/web-f56f55fb4-scp2w is protected -
           The disruption budget web-strict needs 3 healthy pods and has 3 currently
SELECTION: drain: blocked by  kubernation-demo/web-strict
```

The pod count stayed at 3. A **named** refusal, not "evict failed", naming the
same budget the panel names. That is the PDB guidance's §0 decision — describe
Kubernetes' constraint and make the app obey it — visible together for the first
time.

**One capture bug found and fixed to get there.** The first attempt photographed
nothing: `--evict-go` had no arm in the screenshot-hold chain, so it took the
default 45 frames and fired *before* the eviction round-trip (an SSAR then a POST
through the net thread) had even been picked up. The toast then clears itself
after ~3s, so the window is bounded on both sides. Added a 120-frame arm with
that reasoning recorded — the same shape as the existing `--chaos`/`--plan-go`
arms.

### 2.3 The gui-smoke state — captured, not uncapturable

The prompt allowed recording it as uncapturable. It is not: the state renders
only against a live blocking budget, and the dev cluster's fixture is ours to
choose. `hack/samples.yaml` now carries `db-strict` (`minAvailable: 2` on a
2-replica StatefulSet → `disruptionsAllowed: 0` permanently), and gui-smoke gains
`drain-blocked`.

**On `db`, deliberately not on `web`** — `web` is the evict-demo target, and a
blocking budget there would make `--evict web` always refuse.

**Scoped to the workload, not a node** — which node `db` lands on is not
deterministic, but its city always resolves and the SELECTION box carries its
province's drain line. Verified in the harness: `ok drain-blocked`, 58 states.

The capture also exercised a case the ad-hoc fixture had not: worker runs both
`db` and `web` pods, so the box listed **two** budgets, each on its own indented
row and both inside the column width.

---

## 3. Standing questions

**2 — unknown, or fabricated?** The load-bearing one, and the prompt was right to
insist it be verified in the *rendered* output. Checked live through a
PDB-denied ServiceAccount, with the budgets still in place: the panel reads
`disruption budgets not read - drain cost unknown` in amber and the column
`drain: budgets not read`, where a drainable node reads dim or says nothing. No
covering budgets and budgets not read are visibly different claims.

That run also displayed the contrast the PDB round was designed around: the same
identity cannot read NetworkPolicies either, and the walls feature *does* conflate
denied-with-absent — `db` appears as `unwalled` and the posture drops to
`63 EXPOSED`. Two features, two choices, side by side on one screen: netpol
accepts the conflation because its fail-safe direction is the same, PDB refuses it
because its is not.

**8 — true *and* sufficient?** "The drain line renders" was what the deferral
recorded, and it was insufficient exactly as the prompt says. What is established
now is that it renders the **blocked** state, naming the budget, distinguishable
from both drainable and unknown, agreeing with the button, and beside its
neighbours without misreading.

---

## 4. Acceptance

- [x] `v1.36.0` tagged and pushed; three builds and publish succeeded
- [x] Pre-tag checks in an order where `--locked` cannot be defeated (fresh worktree)
- [x] Licence guard green; pin and Windows entry intact
- [x] Blocked state observed live, naming the budget
- [x] Blocked / drainable / unknown all distinguishable in the rendered output
- [x] The evict refusal named, and agreeing with the line, in one frame
- [x] The gui-smoke state **captured** (§2.3), not recorded as uncapturable
- [x] The kind cluster left as found — the ad-hoc fixture removed; `db-strict`
      kept, because it is now part of `hack/samples.yaml` and is what `make dev`
      produces

637 tests; gui-smoke 58; clippy clean; 0 broken doc links.

---

## 5. One diagnostic I got wrong on the way

The example hung, and I bisected it: a binary built from a commit *before* the
PDB watch changes worked, the current one did not. I read that as **"I introduced
a hang"** — the `wait_until_ready` waiter added in v1.31.0 being the obvious
suspect, since CLAUDE.md warns about exactly that call.

It was wrong. The two binaries differed by **build freshness** as well as by
commit: the old one was built fresh in the worktree, the current one was stale
from before v1.35.0/v1.36.0 landed. Rebuilt at HEAD, it works.

I do not have a full explanation for why the stale binary hung, and I am recording
that rather than inventing one. What the bisect actually established is narrower
than what I first took from it — the difference it isolated was not the one I
attributed it to. (An unrelated project's `cargo nextest` holding the package-cache
lock also cost time earlier in the same sequence, and was likewise diagnosed by
looking at the process list rather than reasoning about the symptom.)
