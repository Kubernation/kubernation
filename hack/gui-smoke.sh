#!/usr/bin/env bash
# Render-smoke: drive each GUI overlay / modal / map state through the
# --screenshot export path and fail if any crashes or produces no image. This
# is the "nothing panics across the UI" gate the lost TUI snapshot tests used to
# provide. It needs a display and a reachable cluster (run after `make dev`) —
# it is NOT the headless CI gate (that is `make smoke`, a UI-free core example).
# Write flags (--evict-go / --plan-go) are deliberately excluded.
set -uo pipefail

CTX="${1:-kind-kubernation}"
BIN="target/release/kubernation"
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 1; }
DIR="$(mktemp -d)"
COMMON=(--context "$CTX" --project gizmos.example.com)

# name | extra flags (one per UI state worth a crash check)
STATES=(
  "map|"
  "almanac|--almanac"
  "advisor|--advisor health"
  "advisor-rightsizing|--advisor rightsizing"
  "advisor-hardening|--advisor hardening"
  "advisor-posture|--advisor posture"
  "charter|--charter"
  "annals|--annals"
  "browse-pick|--browse"
  "browse-table|--browse configmaps"
  "city|--inspect web"
  "city-logs|--inspect web --tail"
  "crashy-previous|--inspect crashy --tail"
  "yaml|--inspect web --yaml"
  "node|--inspect kubernation-worker"
  "plan|--plan"
  "menu|--menu view"
  "overlay-pressure|--overlay pressure"
  "overlay-namespace|--overlay namespace"
  "overlay-walls|--overlay walls"
  "concern-logs|--concern-logs"
  "namespace-scope|--namespace kubernation-demo"
  "forward|--forward web"
  "blast-node|--blast kubernation-worker"
  "blast-workload|--blast web"
  "chaos|--chaos web"
  "chaos-node|--chaos web --chaos-exp node-failure"
  "chaos-killpct|--chaos web --chaos-exp kill-percent"
  "chaos-spike|--chaos web --chaos-exp scale-spike"
  "chaos-partition|--chaos web --chaos-exp partition"
  "chaos-tier|--chaos web --chaos-tier siege"
)

fail=0
for entry in "${STATES[@]}"; do
  name="${entry%%|*}"
  flags="${entry#*|}"
  png="$DIR/$name.png"
  # shellcheck disable=SC2086  # word-splitting $flags is intentional
  if ! "$BIN" "${COMMON[@]}" $flags --screenshot "$png" >/dev/null 2>&1; then
    echo "FAIL $name (non-zero exit)"; fail=1; continue
  fi
  if [ ! -s "$png" ]; then
    echo "FAIL $name (no image produced)"; fail=1; continue
  fi
  echo "ok   $name"
done
rm -rf "$DIR"

if [ "$fail" -eq 0 ]; then
  echo "gui-smoke: all ${#STATES[@]} states rendered without panic"
else
  echo "gui-smoke: FAILURES above"; exit 1
fi
