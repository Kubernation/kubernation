#!/usr/bin/env bash
# SCENARIO 7 — Workload churn.  Gate: A3.
# Proves: whether a city stays put when a DIFFERENT workload changes.
#
# TOUCHES NO NODES. That is the whole point: the other six scenarios churn the
# fleet, and mixing the two makes the gate ambiguous about which change moved
# what. Here the fleet is settled and untouched, and only workloads move.
#
# COVERAGE IS THE REQUIREMENT, NOT THE STEP COUNT. On the stock fixture every
# province carries exactly ONE city, and a single-city province cannot exhibit
# the sibling-order effect at all — cities are placed in `WorkloadRef` order
# (kind, namespace, name) and each takes the first free cell, so a city's cell
# depends on how many siblings sort ahead of it. A scenario that adds one
# workload to an empty province would pass a broken A3. So this pins several
# workloads onto ONE node to build a genuinely multi-city province, and keeps a
# bystander on a different province in the same zone.
#
# The added workload is named to sort FIRST among its siblings, because that is
# what shifts every incumbent's index. A name sorting last would change nothing
# and the scenario would quietly test nothing.
#
#   TARGET=<node>   the province to crowd (default: first `mem` node)
#   SETTLE=25       seconds to let each step settle before the next
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

SETTLE="${SETTLE:-25}"
NS=churn

# A node no scenario refreshes, so a rerun cannot be confused by node churn.
TARGET="${TARGET:-$(kc get nodes -l churn.kubernation.io/pool=mem -o name | head -1 | sed 's|node/||')}"
[ -n "$TARGET" ] || { echo "  !! no target node found" >&2; exit 2; }
TARGET_ZONE="$(kc get node "$TARGET" -o jsonpath='{.metadata.labels.topology\.kubernetes\.io/zone}')"
BYSTANDER="$(kc get nodes -l "topology.kubernetes.io/zone=${TARGET_ZONE}" -o name \
  | sed 's|node/||' | grep -v "^${TARGET}$" | head -1)"
[ -n "$BYSTANDER" ] || { echo "  !! no same-zone bystander node found" >&2; exit 2; }

log "target province ${TARGET} (zone ${TARGET_ZONE}), bystander ${BYSTANDER}"

# pinned <name> <replicas> <node>
pinned() {
  cat <<EOF
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ${1}
  namespace: ${NS}
spec:
  replicas: ${2}
  selector:
    matchLabels: { app: ${1} }
  template:
    metadata:
      labels: { app: ${1} }
    spec:
      nodeSelector:
        kubernetes.io/hostname: ${3}
      tolerations:
        - key: kwok.x-k8s.io/node
          effect: NoSchedule
          operator: Exists
      containers:
        - name: app
          image: registry.k8s.io/pause:3.9
          resources:
            requests: { cpu: 10m, memory: 16Mi }
EOF
}

settle() {
  sleep "$SETTLE"
  wait_pods_settled 2>/dev/null || true
}

# --- 1. build the crowded province --------------------------------------
# `nodeSelector`, not `nodeName`: the real kube-scheduler stays in the loop, so
# these are genuinely scheduled pods rather than hand-placed ones.
log "seeding incumbents on ${TARGET} + a bystander on ${BYSTANDER}"
{
  pinned m-incumbent-1 3 "$TARGET"
  pinned m-incumbent-2 3 "$TARGET"
  pinned m-incumbent-3 3 "$TARGET"
  pinned z-bystander 3 "$BYSTANDER"
} | kc apply -f - >/dev/null
settle

# --- the no-op guard ----------------------------------------------------
# A scenario that churns nothing reports perfect stability, which is precisely
# how the first A2 gate answer went wrong. Refuse rather than mislead.
placed=$(kc get pods -n "$NS" --field-selector "spec.nodeName=${TARGET}" \
  -l 'app in (m-incumbent-1,m-incumbent-2,m-incumbent-3)' \
  --no-headers 2>/dev/null | grep -c Running || true)
if [ "${placed:-0}" -lt 3 ]; then
  echo "  !! only ${placed:-0} incumbent pods are Running on ${TARGET}" >&2
  echo "     The target province has no incumbent cities to displace, so this" >&2
  echo "     run would measure nothing and report it as stability." >&2
  exit 2
fi
log "incumbents settled (${placed} pods on ${TARGET})"

# --- 2. THE EVENT: a new workload that sorts FIRST -----------------------
log "adding a-newcomer (sorts ahead of every incumbent)"
pinned a-newcomer 3 "$TARGET" | kc apply -f - >/dev/null
settle

# --- 3. scale one up, then down -----------------------------------------
log "scaling m-incumbent-2 up then down"
kc scale deploy/m-incumbent-2 -n "$NS" --replicas=9 >/dev/null
settle
kc scale deploy/m-incumbent-2 -n "$NS" --replicas=3 >/dev/null
settle

# --- 4. delete one ------------------------------------------------------
# PodGC strands a deleted workload's pods for 30-60s, so a measurement taken
# straight after reads a transitional state rather than the settled one.
log "deleting m-incumbent-3"
kc delete deploy/m-incumbent-3 -n "$NS" --wait=false >/dev/null 2>&1 || true
settle
wait_no_orphan_pods 2>/dev/null || true
settle

log "workload churn complete (nodes untouched: $(kc get nodes --no-headers | wc -l | tr -d ' '))"
