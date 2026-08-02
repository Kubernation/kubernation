#!/usr/bin/env bash
# SCENARIO 1 — Rolling refresh.  Gate: A2 (the kill gate).
# Proves: a slot survives its occupant.
#
# THE ORDERING IS THE POINT. Immutable-infrastructure refreshes SURGE: the
# replacement is created and Ready *before* its predecessor drains, so for a
# window both exist. A delete-then-create script would quietly test an easier
# problem — there would never be more nodes than slots, and slot assignment
# would never have to choose. Surge is the default here; OVERLAP tunes how long
# the two coexist.
#
#   BATCH=10       nodes replaced per wave
#   OVERLAP=8      seconds both generations are Ready before the drain starts
#   CAPTURE=1      take a frame per wave (see capture.sh for the ~6s floor)
#   POOL=sys       which pool to refresh (empty = every pool)
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

OLD_GEN="${OLD_GEN:-g1}"
NEW_GEN="${NEW_GEN:-g2}"
BATCH="${BATCH:-10}"
OVERLAP="${OVERLAP:-8}"
CAPTURE="${CAPTURE:-1}"
POOL="${POOL:-sys}"

frame=0
capture() {
  [ "$CAPTURE" = "1" ] || return 0
  ./capture.sh refresh "$frame"
  frame=$((frame + 1))
}

old_nodes() {
  if [ -n "$POOL" ]; then
    kc get nodes -l "churn.kubernation.io/pool=${POOL}" -o name
  else
    kc get nodes -o name
  fi | sed 's|node/||' | grep -- "-${OLD_GEN}-" || true
}

log "rolling refresh: ${OLD_GEN} -> ${NEW_GEN} (pool=${POOL:-all}, batch=${BATCH}, overlap=${OVERLAP}s)"
capture

wave=0
while :; do
  # shellcheck disable=SC2207
  victims=($(old_nodes | head -n "$BATCH"))
  [ "${#victims[@]}" -eq 0 ] && break
  wave=$((wave + 1))
  log "wave ${wave}: ${#victims[@]} nodes"

  # 1. SURGE — create the replacements and wait for them to be Ready. Names are
  #    new (generation token), so nothing can be matched by identity.
  for v in "${victims[@]}"; do
    idx="${v##*-}"
    pool_of="$(kc get node "$v" -o jsonpath='{.metadata.labels.churn\.kubernation\.io/pool}')"
    for spec in "${POOLS[@]}"; do
      [ "$(pool_field "$spec" 1)" = "$pool_of" ] || continue
      pool_nodes "$spec" "$NEW_GEN" "$((10#$idx))" 1
    done
  done | kc apply -f - >/dev/null
  wait_nodes_ready "type=kwok" 120

  # 2. OVERLAP — both generations Ready at once. This is the window in which a
  #    layout has to decide, and the window a capture wants to see.
  sleep "$OVERLAP"
  capture

  # 3. DRAIN — evict the pods so they reschedule onto the surged replacements,
  #    then remove the node. Draining before deleting is what makes this churn
  #    the CITIES too, not just the provinces (guidance §3).
  for v in "${victims[@]}"; do
    kc drain "$v" --ignore-daemonsets --delete-emptydir-data --force \
      --disable-eviction --timeout=60s >/dev/null 2>&1 || true
  done
  kc delete node "${victims[@]}" --wait=false >/dev/null 2>&1 || true
  sleep 2
  capture
done

wait_no_orphan_pods
wait_pods_settled
capture
log "refresh complete: $(kc get nodes --no-headers | wc -l | tr -d ' ') nodes, all generation ${NEW_GEN}"
