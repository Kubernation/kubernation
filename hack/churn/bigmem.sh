#!/usr/bin/env bash
# Add (or remove) two top-class nodes, so extent class 9 renders at all.
#
# WHY THIS IS A SEPARATE SCRIPT AND NOT PART OF up.sh
#
# The 100-node fleet is the reference state every recorded measurement in
# docs/reports/ is judged against — node counts, region pieces, the A2 gate.
# Growing it would invalidate all of them silently. This is additive and
# reversible: `MODE=down` takes the nodes away again.
#
# WHY TWO NODES, IN A NEW POOL, IN ONE ZONE
#
# Class 9 is the largest extent, and SLOT_STRIDE is the largest extent, so a
# class-9 province exactly fills its slot and TOUCHES its neighbour's band —
# rows 1+9n .. 1+9n+8, with the next slot starting at 1+9(n+1). That adjacency
# is the precondition for the Relief occlusion case, and it needs TWO class-9
# provinces at CONSECUTIVE ordinals.
#
#   - a new pool, because a node joining an existing (zone, pool) would RECLAIM
#     one of that pool's ghost slots instead of appending, and reclaimed
#     ordinals are wherever the vacancies happen to be
#   - one zone, because ordinals are per-zone
#
# 512Gi is nominal. EXTENT_HEADROOM means the bound admits from ~474 GiB, so a
# node declared at exactly the bound is comfortably inside it.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh

MODE="${MODE:-up}"
ZONE="${ZONE:-z-d}"
COUNT="${COUNT:-2}"
POOL="hpc"

if [ "$MODE" = "down" ]; then
  log "removing the ${POOL} pool"
  kc delete node -l "churn.kubernation.io/pool=${POOL}" --ignore-not-found >/dev/null
  log "done — the fleet is back to its reference size"
  exit 0
fi

log "adding ${COUNT} nominal-512Gi nodes to ${ZONE} as pool '${POOL}'"
{
  for i in $(seq 0 $((COUNT - 1))); do
    name=$(printf "churn-%s-%03d" "$POOL" "$i")
    node_yaml "$name" "$ZONE" "$POOL" "cloud.google.com/gke-nodepool" \
      "64" "512Gi" "m2-ultramem-208" "" "$i"
  done
} | kc apply -f - >/dev/null

log "waiting for them to register"
for _ in $(seq 30); do
  n=$(kc get nodes -l "churn.kubernation.io/pool=${POOL}" --no-headers 2>/dev/null | wc -l | tr -d ' ')
  [ "$n" -ge "$COUNT" ] && break
  sleep 1
done

# VERIFY rather than assume: the whole point is the class, so check the input
# the classifier actually reads.
log "reported allocatable memory (what province_extent classifies on):"
kc get nodes -l "churn.kubernation.io/pool=${POOL}" \
  -o custom-columns=NAME:.metadata.name,MEM:.status.allocatable.memory --no-headers |
  sed 's/^/    /'
log "run the app and confirm these two provinces render 9 rows tall and adjacent"
