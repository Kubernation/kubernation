#!/usr/bin/env bash
# Return the fleet to its pristine fixture state without recreating the cluster.
# Scenarios mutate the fleet, so run this between them for a repeatable start.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

log "resetting fleet to the baseline fixture"
kc delete nodes -l type=kwok --wait=false >/dev/null 2>&1 || true
kc delete ns churn --wait=false >/dev/null 2>&1 || true
# Wait for the namespace to actually go, or the re-apply races its termination.
for _ in $(seq 1 40); do
  kc get ns churn >/dev/null 2>&1 || break
  sleep 2
done
GEN="${GEN:-g1}" ./up.sh
