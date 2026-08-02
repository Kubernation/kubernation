#!/usr/bin/env bash
# SCENARIO 6 — Zone loss.  Gate: A5.
# Proves: the failure-domain claim in the hierarchy — a continent vanishes.
#
# This is the scenario that tests "zone stays primary, because zone IS the
# failure domain": the 'sys' pool spans three zones, so losing one takes PART of
# that pool plus all of any pool confined to it. A hierarchy that nested pool
# above zone would draw the wrong thing here.
#
#   ZONE=z-b
#   MODE=delete    the only supported mode — see the kwok note below
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

ZONE="${ZONE:-z-b}"
MODE="${MODE:-delete}"
CAPTURE="${CAPTURE:-1}"

# A real zone outage does NOT delete Node objects — the kubelets stop reporting
# and the nodes go NotReady, which is a different map event (NotReady terrain
# that persists) from a scale-down (terrain that disappears).
#
# kwok cannot reproduce that. Its heartbeat continuously rewrites the Ready
# condition, and does so whether or not the node carries the
# `kwok.x-k8s.io/node` annotation: a status patch setting Ready=Unknown applies
# and is reverted inside one second (verified — removing the annotation first,
# with a settle, does not help either).
#
# So this scenario deletes. That matches the guidance's own wording for it
# ("continent vanishing") and is honest about what it demonstrates. Refusing
# loudly beats a `notready` mode that runs, changes nothing, and lets a reviewer
# conclude the map survived an outage it never actually saw.
if [ "$MODE" != "delete" ]; then
  cat >&2 <<'MSG'
  MODE=notready is not supported: kwok rewrites the Ready condition within ~1s,
  so the nodes would stay Ready and this scenario would silently do nothing.
  Use MODE=delete (the default). Holding a node NotReady needs a real kubelet,
  or a kwok Stage override this harness deliberately does not install.
MSG
  exit 2
fi

[ "$CAPTURE" = "1" ] && ./capture.sh zoneloss 0
# shellcheck disable=SC2207
victims=($(kc get nodes -l "topology.kubernetes.io/zone=${ZONE}" -o name | sed 's|node/||'))
if [ "${#victims[@]}" -eq 0 ]; then log "zone '${ZONE}' is empty"; exit 0; fi

log "zone loss: '${ZONE}' — ${#victims[@]} nodes vanish"
kc get nodes -l "topology.kubernetes.io/zone=${ZONE}" -L churn.kubernation.io/pool --no-headers \
  | awk '{print $6}' | sort | uniq -c | sed 's/^/    pool /' >&2

kc delete node "${victims[@]}" --wait=false >/dev/null 2>&1 || true

sleep 4
[ "$CAPTURE" = "1" ] && ./capture.sh zoneloss 1
wait_no_orphan_pods
wait_pods_settled
[ "$CAPTURE" = "1" ] && ./capture.sh zoneloss 2
log "zone loss done: $(kc get nodes --no-headers | wc -l | tr -d ' ') nodes remain"
