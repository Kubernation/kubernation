#!/usr/bin/env bash
# SCENARIO 4 — Nodepool add.  Gate: A5.
# Proves: a CATACLYSM is detected and recorded — structurally distinct from
# routine churn, because a whole new region appears rather than slots filling.
#
# The new pool uses a FOURTH label convention and lands in a new zone, so it
# also exercises the fallback cascade on previously-unseen input.
#
#   NAME=gpu  COUNT=12
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

NAME="${NAME:-gpu}"
COUNT="${COUNT:-12}"
CAPTURE="${CAPTURE:-1}"

[ "$CAPTURE" = "1" ] && ./capture.sh pooladd 0
log "nodepool add: '${NAME}' x${COUNT} (agentpool convention, zone z-e)"

spec="${NAME}|agentpool|z-e|${COUNT}|32|256Gi|a2-highgpu-1g"
pool_nodes "$spec" "${GEN:-g1}" 0 | kc apply -f - >/dev/null

wait_nodes_ready "type=kwok" 120
wait_pods_settled
[ "$CAPTURE" = "1" ] && ./capture.sh pooladd 1
log "nodepool add done: $(kc get nodes --no-headers | wc -l | tr -d ' ') nodes"
