# `hack/` — fixtures, gates and guards

Scripts here build clusters, drive scenarios and check things Rust cannot reach
(workflow files, `about.toml`, the shape of the source tree). They are
**instruments**: their output is evidence, and a wrong number here becomes a
wrong claim in a report.

## The shell convention

Twenty-three instrument failures are catalogued in `docs/reports/`. Three
happened in a single round (v1.37.0), and all three were shell:

- `PIPESTATUS` where zsh has `pipestatus`, so a gate summary printed `exit=0`
  beside two failing gates;
- a `kubectl --context …` stored in a **variable**, which zsh does not
  word-split, so a discrimination run applied nothing and photographed the
  baseline under the discrimination filenames;
- a `sort -u` over `spec.nodeName` that counted an unscheduled pod's blank line
  as a node, and I explained the phantom with a mechanism that does not exist.

Three rules. They apply to anything written here, and to an ad-hoc command
whose output will be quoted.

**1. Functions, not command variables.** `K="kubectl --context foo"; $K get pods`
is word-split differently across shells and silently becomes "command not
found" — which a loop then swallows. Write `kc() { kubectl --context "$CTX"
"$@"; }` and call it. `hack/churn/lib.sh` does this already; use it.

**2. `set -euo pipefail`.** An unset variable must fail loudly rather than
expand to nothing. Note that `bash 3.2` ships on macOS: no `declare -A`, and
`"${arr[@]}"` on an empty array trips `set -u`.

*Corollary, and it cost a gate run:* **never end a function with
`[ cond ] && cmd`.** When the test is false the compound returns 1, that becomes
the function's exit status, and `set -e` aborts the CALLER — after the function
has printed every one of its success lines. `10-node-notready.sh` did exactly
this: both "ok" reads printed, then the script died silently before its most
important assertion, and the caller reported nothing wrong. Use `if`.

**3. Assert the fixture CHANGED before you photograph it.** A run that applies
nothing and captures the baseline is indistinguishable from a passing run —
this is the one that cost the most, and it is the same shape as
`hack/churn/gate.sh`'s no-op guard and the guard-the-guard assertions in the
Rust tests. Check the thing you changed is different, and fail if it is not.

A corollary, from the same round: **read the source of a number before quoting
it.** A count printed by a pipeline you just wrote has not been verified by
printing.

## Guards run by `make lint`

| script | asserts |
|---|---|
| `check-conversion-authorities.sh` | `region_at` is called only from files under test |
| `check-release-targets.sh` | `about.toml` covers every platform `release.yml` ships |
| `check-advisor-memo.sh` | advisor reports are built only through `ReportCache` |

Each of these is itself an instrument, so each carries a guard-the-guard: the
release-target one shipped **broken**, checking one of three platforms while
reporting success, and only its own mutation said so.
