#!/usr/bin/env bash
# Return the fleet to its pristine fixture state without recreating the cluster.
# Scenarios mutate the fleet, so run this between them for a repeatable start.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

log "resetting fleet to the baseline fixture"
# Delete the WORKLOAD OBJECTS, not the namespace. Terminating a namespace holding
# ~400 kwok pods takes minutes, and re-applying into one that is still Terminating
# silently applies nothing — which yields a 100-node fleet with zero pods that
# renders as a perfectly plausible map. That invalidated two gate runs.
kc delete deploy,sts,ds --all -n churn --wait=false >/dev/null 2>&1 || true
kc delete nodes -l type=kwok --wait=false >/dev/null 2>&1 || true
# Let the controllers observe the deletions before re-applying, so the new
# objects are not immediately reconciled against stale replica sets.
sleep 5
GEN="${GEN:-g1}" ./up.sh
