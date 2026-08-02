#!/usr/bin/env bash
# SCENARIO 2 — Scale up.  Gate: A2.
# Proves: new slots APPEND; nothing that already exists moves.
#
#   COUNT=20   nodes to add
#   POOL=burst which pool grows
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

COUNT="${COUNT:-20}"
POOL="${POOL:-burst}"
GEN="${GEN:-g1}"
CAPTURE="${CAPTURE:-1}"

[ "$CAPTURE" = "1" ] && ./capture.sh scaleup 0
before=$(kc get nodes --no-headers | wc -l | tr -d ' ')

# Start indices above the pool's existing range so names never collide.
start=$(kc get nodes -l "churn.kubernation.io/pool=${POOL}" --no-headers | wc -l | tr -d ' ')
log "scale up: +${COUNT} nodes in pool '${POOL}' (from index ${start})"
for spec in "${POOLS[@]}"; do
  [ "$(pool_field "$spec" 1)" = "$POOL" ] || continue
  pool_nodes "$spec" "$GEN" "$start" "$COUNT"
done | kc apply -f - >/dev/null

wait_nodes_ready "type=kwok" 120
wait_pods_settled
[ "$CAPTURE" = "1" ] && ./capture.sh scaleup 1
log "scale up done: ${before} -> $(kc get nodes --no-headers | wc -l | tr -d ' ') nodes"
