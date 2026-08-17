#!/usr/bin/env bash
# Place unhealthy pods on the 100-node churn fleet, reversibly, in a chosen shape.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# T2-pre could not measure the pool dimension: kind has no pools, and the churn
# fleet has pools but kwok emits no real failures. That gap is two questions
# wearing one name:
#
#   (a) do real-world failures TEND to be pool-shaped?
#       Not answerable on any test cluster. Every failure here is induced, so the
#       frequency is whatever the fixture chooses. A bigger fleet does not help.
#
#   (b) IF failures are pool-shaped, does the map render them as a SHAPE?
#       Answerable, and it is the one that decides T2 (§6's third branch). It
#       needs real PLACEMENT, not real causes.
#
# This script answers (b). The placement is real: the churn cluster runs a real
# kube-scheduler, so where these pods sit is where Kubernetes put them. Only the
# unhealthy STATUS is written directly, because kwok has no failure stage and
# adding one needs `kwokctl --enable-crds=Stage`, which means recreating the
# cluster — destroying the layout store that carries T1's succession record.
#
#   MODE=pool      every node-agent pod on the `sys` pool's nodes  (30 nodes, 3 zones)
#                  the canonical pool-shaped incident: a bad node image rolled to
#                  one nodepool, breaking its per-node agent
#   MODE=workload  every pod of one Deployment, wherever the scheduler put them
#   MODE=down      delete the marked pods; their controllers recreate them clean
#
# Nothing here touches nodes, so slots, ordinals and the succession record are
# untouched.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh

MODE="${MODE:-down}"
POOL="${POOL:-sys}"
DEPLOY="${DEPLOY:-cache}"
NS="${NS:-churn}"

mark() {
  # A status shaped exactly like a real crash-loop: the API state the app reads
  # and the instrument classifies. Written to the status subresource.
  local pod="$1"
  kc -n "$NS" patch pod "$pod" --subresource=status --type=merge -p '{
    "status": {"containerStatuses": [{"name":"app","ready":false,"restartCount":9,
      "image":"x","imageID":"",
      "state":{"waiting":{"reason":"CrashLoopBackOff","message":"back-off 5m0s restarting failed container"}}}]}
  }' >/dev/null
}

case "$MODE" in
  down)
    log "clearing marked pods (their controllers recreate them clean)"
    n=$(kc -n "$NS" get pods -l t2=marked --no-headers 2>/dev/null | wc -l | tr -d ' ')
    kc -n "$NS" delete pod -l t2=marked --wait=false >/dev/null 2>&1 || true
    log "deleted $n"
    ;;

  pool)
    log "EXPECT pool-shaped: victims are every node of one nodepool"
    # macOS ships bash 3.2, which has no `mapfile`. A read loop is portable.
    nodes=$(kc get nodes -l "churn.kubernation.io/pool=${POOL}" \
      -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}')
    log "$(echo "$nodes" | grep -c .) nodes in pool '${POOL}'"
    for n in $nodes; do
      pod=$(kc -n "$NS" get pods --field-selector "spec.nodeName=${n}" \
        -l app=node-agent -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)
      [ -z "$pod" ] && continue
      kc -n "$NS" label pod "$pod" t2=marked --overwrite >/dev/null
      mark "$pod"
    done
    ;;

  workload)
    log "EXPECT workload-shaped: victims are one Deployment, wherever it landed"
    pods=$(kc -n "$NS" get pods -l "app=${DEPLOY}" \
      -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}')
    log "$(echo "$pods" | grep -c .) pods in deployment '${DEPLOY}'"
    for p in $pods; do
      kc -n "$NS" label pod "$p" t2=marked --overwrite >/dev/null
      mark "$p"
    done
    ;;

  *) echo "unknown MODE=$MODE" >&2; exit 2 ;;
esac
log "done"
