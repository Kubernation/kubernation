#!/usr/bin/env bash
# SCENARIO 8 — Substrate gaps.  Gate: Advisors ▸ Substrate (v1.37.0).
# Proves: the Substrate tab, the `substrate` overlay and the headless
# `substrate` example name the SAME nodes as missing the SAME daemonsets.
#
# TOUCHES NO NODES. It adds two daemonsets that each exclude a few named nodes
# by hostname affinity, so the fleet has gaps with a KNOWN answer — the
# excluded names — and the answer is checkable with `kubectl` before any
# capture is read. v1.5.0 verified the overlay this way ad hoc; this pins it.
#
# The fleet's own `node-agent` (from workloads.sh) stays, so expect THREE rows,
# not two: its pod for the allocatable-less node is Unschedulable, so that node
# is missing every daemonset. Coverage is presence, and an unscheduled pod is
# not present anywhere.
#
#   MODE=up       apply log-agent (2 sys nodes excluded) + node-exporter (1 edge)
#   MODE=discrim  exclude log-agent from the WHOLE sys pool: 70/100 < 80%, so it
#                 must leave the tab AND its former gaps must stop colouring
#   MODE=down     delete both daemonsets — the fleet as found
#
# Check with:  cargo run -p kubernation-core --example substrate -- \
#                --context kwok-kubernation-churn
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

MODE="${MODE:-up}"
NS=churn

sys_nodes() { kc get nodes -l churn.kubernation.io/pool=sys -o name | sed 's|node/||' | sort; }
edge_nodes() { kc get nodes -l churn.kubernation.io/pool=edge -o name | sed 's|node/||' | sort; }

# One daemonset manifest: $1 name, $2 affinity key, $3 operator values (yaml list)
ds() {
  cat <<YAML
apiVersion: apps/v1
kind: DaemonSet
metadata: {name: $1, namespace: $NS}
spec:
  selector: {matchLabels: {app: $1}}
  template:
    metadata: {labels: {app: $1}}
    spec:
      tolerations: [{operator: Exists}]
      affinity:
        nodeAffinity:
          requiredDuringSchedulingIgnoredDuringExecution:
            nodeSelectorTerms:
              - matchExpressions:
                  - {key: $2, operator: NotIn, values: [$3]}
      containers: [{name: agent, image: fake.registry/$1:1.0}]
YAML
}

case "$MODE" in
  up)
    S0="$(sys_nodes | sed -n 1p)"; S1="$(sys_nodes | sed -n 2p)"; E0="$(edge_nodes | sed -n 1p)"
    [ -n "$S1" ] && [ -n "$E0" ] || { echo "  !! need two sys nodes and one edge node" >&2; exit 2; }
    echo "  expect gaps: $S0 + $S1 missing log-agent; $E0 missing node-exporter"
    ds log-agent kubernetes.io/hostname "$S0, $S1" | kc apply -f -
    ds node-exporter kubernetes.io/hostname "$E0" | kc apply -f -
    ;;
  discrim)
    echo "  expect: log-agent leaves the table; only node-exporter's gap (and any unschedulable node) remains"
    ds log-agent churn.kubernation.io/pool sys | kc apply -f -
    ;;
  down)
    kc -n "$NS" delete ds log-agent node-exporter --ignore-not-found
    ;;
  *) echo "  !! MODE must be up|discrim|down" >&2; exit 2 ;;
esac
kc -n "$NS" get ds
