#!/usr/bin/env bash
# The advisor's reports are built through `ReportCache`, never in the draw.
#
# WHY THIS IS A LINT AND NOT A TEST
#
# Each report used to be rebuilt inside `Advisor::draw`, so it ran at frame rate
# — ~4ms at the documented ceiling, a quarter of a 60fps frame. The memo fixes
# that, and `ReportCache`'s own tests pin the key and the invalidation.
#
# They cannot pin the CALLER. `draw` is GL-driven and has no test, so a mutation
# that bypassed the cache there — `&health_report(obs)` in place of
# `c.health(obs)` — passed the entire suite. That is the structural limit D2 §3.4
# recorded for `main.rs`: no behavioural test can observe code in a function that
# has none.
#
# So the build calls live in `ReportCache`'s impl, and this asserts they stay
# there. A bypass becomes a visible `make lint` failure rather than a silent
# return to a per-frame rebuild that nothing reports.
#
# `cost_report`/`posture_report` are deliberately absent from the list: both are
# already memoized on the snapshot by the net thread, and the advisor renders the
# memoized value rather than building one.
set -euo pipefail
cd "$(dirname "$0")/.."

ADV=crates/kubernation/src/advisor.rs
FNS='health_report|storage_report|network_report|rightsizing_report|hardening_report|coverage_report'

# Lines after `impl ReportCache` closes and before the test module: the draw and
# its helpers. Comments are skipped so the explanations above may name the fns.
bad=$(awk '
  /^impl ReportCache \{/ { inimpl = 1 }
  inimpl && /^\}/        { inimpl = 0; past = 1; next }
  /^#\[cfg\(test\)\]/    { exit }
  past && $0 !~ /^[[:space:]]*\/\// { print NR ": " $0 }
' "$ADV" | grep -E "($FNS)\(" || true)

if [ -n "$bad" ]; then
  echo "advisor report built outside ReportCache — this returns it to a per-frame" >&2
  echo "rebuild, and no test can see it:" >&2
  echo "$bad" >&2
  echo "  Use the cache accessors: c.health(obs), c.rightsizing(obs), …" >&2
  exit 1
fi
echo "advisor reports are built only through ReportCache"
