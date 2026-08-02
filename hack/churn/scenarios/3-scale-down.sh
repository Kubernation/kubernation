#!/usr/bin/env bash
# SCENARIO 3 — Scale down.  Gate: A4.
# Proves: a departure leaves a GHOST, not a reshuffle.
#
# Drains before deleting, so the workloads genuinely relocate — a scale-down
# that stranded its pods would not exercise the city churn A3 has to survive.
#
#   COUNT=15   nodes to remove
#   POOL=edge  which pool shrinks
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

COUNT="${COUNT:-15}"
POOL="${POOL:-edge}"
CAPTURE="${CAPTURE:-1}"

[ "$CAPTURE" = "1" ] && ./capture.sh scaledown 0
before=$(kc get nodes --no-headers | wc -l | tr -d ' ')

# Take from the END of the pool, so this is a shrink rather than a hole.
# shellcheck disable=SC2207
victims=($(kc get nodes -l "churn.kubernation.io/pool=${POOL}" -o name \
  | sed 's|node/||' | sort | tail -n "$COUNT"))
log "scale down: -${#victims[@]} nodes from pool '${POOL}'"

for v in "${victims[@]}"; do
  kc drain "$v" --ignore-daemonsets --delete-emptydir-data --force \
    --disable-eviction --timeout=60s >/dev/null 2>&1 || true
done
kc delete node "${victims[@]}" --wait=false >/dev/null 2>&1 || true

sleep 3
[ "$CAPTURE" = "1" ] && ./capture.sh scaledown 1
wait_no_orphan_pods
wait_pods_settled
[ "$CAPTURE" = "1" ] && ./capture.sh scaledown 2
log "scale down done: ${before} -> $(kc get nodes --no-headers | wc -l | tr -d ' ') nodes"
