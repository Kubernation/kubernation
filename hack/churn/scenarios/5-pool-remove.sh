#!/usr/bin/env bash
# SCENARIO 5 — Nodepool remove.  Gate: A5.
# Proves: STRUCTURAL LOSS, distinct from routine churn — an entire region goes,
# not a scattering of slots.
#
#   POOL=mem
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

POOL="${POOL:-mem}"
CAPTURE="${CAPTURE:-1}"

[ "$CAPTURE" = "1" ] && ./capture.sh poolremove 0
# shellcheck disable=SC2207
victims=($(kc get nodes -l "churn.kubernation.io/pool=${POOL}" -o name | sed 's|node/||'))
if [ "${#victims[@]}" -eq 0 ]; then log "pool '${POOL}' not present — nothing to remove"; exit 0; fi
log "nodepool remove: '${POOL}' (${#victims[@]} nodes)"

for v in "${victims[@]}"; do
  kc drain "$v" --ignore-daemonsets --delete-emptydir-data --force \
    --disable-eviction --timeout=60s >/dev/null 2>&1 || true
done
kc delete node "${victims[@]}" --wait=false >/dev/null 2>&1 || true

sleep 3
[ "$CAPTURE" = "1" ] && ./capture.sh poolremove 1
wait_no_orphan_pods
wait_pods_settled
[ "$CAPTURE" = "1" ] && ./capture.sh poolremove 2
log "nodepool remove done: $(kc get nodes --no-headers | wc -l | tr -d ' ') nodes"
