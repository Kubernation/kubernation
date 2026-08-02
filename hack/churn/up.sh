#!/usr/bin/env bash
# Stand up the churn cluster and its 100-node fleet fixture.
# Idempotent: safe to re-run against an existing cluster.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh

GEN="${GEN:-g1}"

if ! kwokctl get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  log "creating kwok cluster '$CLUSTER' (real apiserver, scheduler, controller-manager)"
  kwokctl create cluster --name "$CLUSTER" >/dev/null
else
  log "cluster '$CLUSTER' already exists"
fi

log "applying the fleet fixture (generation ${GEN})"
{
  for spec in "${POOLS[@]}"; do
    pool_nodes "$spec" "$GEN"
  done
} | kc apply -f - >/dev/null

./workloads.sh

# Settle before declaring the fleet up. The baseline capture is the reference
# image every A2 comparison is judged against, so it must not be taken while the
# scheduler is still placing pods — an empty world is not a baseline.
log "waiting for pods to schedule"
wait_pods_settled 240

total=$(kc get nodes --no-headers | wc -l | tr -d ' ')
log "fleet up: ${total} nodes, $(kc get pods -n churn --no-headers 2>/dev/null | grep -c Running) pods running"
kc get nodes -L topology.kubernetes.io/zone -L churn.kubernation.io/pool --no-headers \
  | awk '{print $6, $7}' | sort | uniq -c | sed 's/^/    /' >&2
log "next: hack/churn/capture.sh baseline   ·   hack/churn/scenarios/1-rolling-refresh.sh"
