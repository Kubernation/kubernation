#!/usr/bin/env bash
# SCENARIO 9 — Node capacity.  Gate: the Substrate tab's "no capacity" tag.
#
# The fleet is built with exactly ONE node publishing no allocatable capacity
# (lib.sh: pool `sys`, index 0). Nothing schedules there, so it is missing EVERY
# fleet-wide daemonset — one node fact wearing the shape of many gaps, which is
# what the tag exists to say.
#
# This is the DISCRIMINATION for that tag: give the node capacity and the tag
# must disappear. Gaps caused only by the missing capacity become non-gaps as
# pods land; a gap caused by a nodeSelector (scenario 8 excludes this node from
# `log-agent`) stays, now as a real untagged gap. If the tag survives, it is
# reading something other than capacity.
#
#   MODE=give     give the node capacity  (assert it took)
#   MODE=restore  take it away again      (assert it took)
#
# Written under hack/README.md's shell convention: functions not command
# variables, `set -euo pipefail`, and — rule 3, the one that matters here — it
# ASSERTS THE FIXTURE CHANGED before returning, so a capture taken after it
# cannot be of the baseline.
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

MODE="${MODE:-give}"

# The node the fleet builds without allocatable. Resolved, not assumed: if the
# fixture ever stops producing one, this fails here rather than photographing a
# fleet that cannot exhibit the case.
noalloc_node() {
  kc get nodes -o json \
    | python3 -c '
import json,sys
for n in json.load(sys.stdin)["items"]:
    a = n["status"].get("allocatable") or {}
    if not a.get("cpu") and not a.get("memory"):
        print(n["metadata"]["name"]); break
'
}

# The node the fixture builds allocatable-less, whether or not it currently
# reports capacity — so `restore` can find it again after `give`.
#
# By LABEL, not by name-sort: a node's name carries a generation token that a
# rolling refresh rewrites, so "first sys node alphabetically" is not stably the
# one at index 0. The fixture's own labels are the identity.
sys0() {
  kc get nodes -l churn.kubernation.io/pool=sys,churn.kubernation.io/index=000 \
    -o name | sed 's|node/||' | head -1
}

alloc_cpu() { kc get node "$1" -o jsonpath='{.status.allocatable.cpu}'; }

# Rule 3: fail unless the value actually moved.
assert_changed() {
  local node="$1" before="$2" want="$3" after
  after="$(alloc_cpu "$node")"
  if [ "$after" = "$before" ]; then
    echo "  !! FIXTURE UNCHANGED: $node allocatable.cpu is still '${before:-<empty>}'" >&2
    echo "  !! a capture taken now would be of the baseline — refusing" >&2
    exit 2
  fi
  case "$want" in
    capacity) [ -n "$after" ] || { echo "  !! expected capacity, got '<empty>'" >&2; exit 2; } ;;
    none)     [ -z "$after" ] || { echo "  !! expected none, got '$after'" >&2; exit 2; } ;;
  esac
  echo "  ok  $node allocatable.cpu: '${before:-<empty>}' -> '${after:-<empty>}'"
}

case "$MODE" in
  give)
    NODE="$(noalloc_node)"
    [ -n "$NODE" ] || { echo "  !! no allocatable-less node in the fleet — nothing to discriminate" >&2; exit 2; }
    BEFORE="$(alloc_cpu "$NODE")"
    echo "  giving $NODE capacity (the tag must disappear; the affinity gap must remain)"
    kc patch node "$NODE" --subresource=status --type=merge \
      -p '{"status":{"allocatable":{"cpu":"8","memory":"32Gi","pods":"110"},"capacity":{"cpu":"8","memory":"32Gi","pods":"110"}}}' >/dev/null
    assert_changed "$NODE" "$BEFORE" capacity
    ;;
  restore)
    NODE="$(sys0)"
    BEFORE="$(alloc_cpu "$NODE")"
    echo "  restoring $NODE to no capacity"
    # A merge patch cannot delete a map, so replace the whole status field.
    kc patch node "$NODE" --subresource=status --type=json \
      -p '[{"op":"replace","path":"/status/allocatable","value":{}},{"op":"replace","path":"/status/capacity","value":{}}]' >/dev/null
    assert_changed "$NODE" "$BEFORE" none
    # kwok has no kubelet, so nothing evicts the pods that landed while the node
    # had capacity — they would keep the node covered and the fixture would not
    # actually be restored. Delete them and let the daemonset controller try
    # again, which now leaves them Unschedulable as before.
    # `--wait=false`: kwok finalizes fake pods, so with its controller stopped
    # (scenario 10) a waiting delete blocks forever. The daemonset recreates
    # them either way.
    kc -n churn delete pods --field-selector "spec.nodeName=$NODE" \
      --ignore-not-found --wait=false >/dev/null
    echo "  ok  cleared pods stranded on $NODE by the capacity window"
    ;;
  *) echo "  !! MODE must be give|restore" >&2; exit 2 ;;
esac
