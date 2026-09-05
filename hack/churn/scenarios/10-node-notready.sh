#!/usr/bin/env bash
# SCENARIO 10 — A node that is BOTH NotReady and allocatable-less.
# Gate: the Substrate tab's composed both-reasons tag.
#
# THE POINT. Two reasons can dim a node's row — NotReady, and reporting no
# capacity — and a node can carry both. That combination was unit-tested only,
# because the two reasons lived on two clusters that each express one:
#
#     kind   : NotReady yes (docker stop), no-capacity NO  (a real kubelet)
#     churn  : NotReady NO  (kwok heartbeat), no-capacity yes (sys index 0)
#
# A path no fixture can reach is where regressions accumulate: exactly one such
# row existed at v1.37.0, `wrap` stripped its indent, and nobody could see it
# for a version. This closes the gap on the churn side, where the capacity half
# already exists and needs no construction.
#
# MECHANISM. The kwok controller runs with `--manage-all-nodes=true`, so a node
# cannot fall out of its managed set and removing the `kwok.x-k8s.io/node`
# annotation does nothing (A-pre found the behaviour; the flag is the reason).
# The controller is therefore STOPPED for the window, and the Ready condition
# patched while nothing is rewriting it.
#
# THE CLOCK, and why this is not a gui-smoke state. kwok renews node leases with
# `--node-lease-duration-seconds=200` and kube-controller-manager runs
# `--node-monitor-grace-period=3m20s` (200s). With the controller stopped, EVERY
# node goes NotReady once that elapses — so the window is bounded, and the
# script asserts no other node has gone NotReady before it lets you capture.
#
#   MODE=notready   stop the kwok controller and mark the node NotReady
#   MODE=restore    put the condition back and restart the controller
#
# Written under hack/README.md: functions not command variables, set -euo
# pipefail, and rule 3 — assert the fixture changed before photographing it.
# Here that assertion is doubled: a state that reverts within a second (which is
# exactly what kwok does) would pass a single read, so every check is made
# TWICE, `SETTLE` seconds apart.
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

MODE="${MODE:-notready}"
SETTLE="${SETTLE:-12}"
KWOK="kwok-${CLUSTER}-kwok-controller"   # CLUSTER comes from lib.sh

# The node the fixture builds allocatable-less — by LABEL, because a node's name
# carries a generation token a refresh rewrites.
target() {
  kc get nodes -l churn.kubernation.io/pool=sys,churn.kubernation.io/index=000 \
    -o name | sed 's|node/||' | head -1
}
ready_of() { kc get node "$1" -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}'; }
alloc_of() { kc get node "$1" -o jsonpath='{.status.allocatable.cpu}'; }
# Nodes whose Ready condition is anything but True.
not_ready_nodes() {
  kc get nodes -o json | python3 -c '
import json,sys
for n in json.load(sys.stdin)["items"]:
    r=[c for c in n["status"].get("conditions",[]) if c["type"]=="Ready"]
    if not r or r[0]["status"]!="True": print(n["metadata"]["name"])
'
}

# Flip ONLY the Ready condition's status, preserving every other condition.
#
# Not a whole-array replace: kwok's node-initialize stage has since given this
# node MemoryPressure/DiskPressure/PIDPressure/NetworkUnavailable, and dropping
# them would be collateral change by the instrument — the failure this round
# already committed once (see below).
set_ready() { # set_ready <node> <True|False>
  local node="$1" want="$2" patch
  patch="$(kc get node "$node" -o json | WANT="$want" python3 -c '
import json,os,sys
conds = json.load(sys.stdin)["status"].get("conditions", [])
want = os.environ["WANT"]
seen = False
for c in conds:
    if c["type"] == "Ready":
        c["status"] = want
        c["reason"] = "KubeletReady" if want == "True" else "KubeletNotReady"
        seen = True
if not seen:
    conds.append({"type": "Ready", "status": want, "reason": "fixture",
                  "lastHeartbeatTime": "2026-01-01T00:00:00Z",
                  "lastTransitionTime": "2026-01-01T00:00:00Z"})
print(json.dumps([{"op": "replace", "path": "/status/conditions", "value": conds}]))
')"
  kc patch node "$node" --subresource=status --type=json -p "$patch" >/dev/null
}

# Rule 3, doubled. A single read cannot tell a held state from one that reverts.
assert_twice() { # assert_twice <node> <want-ready> <want-alloc: empty|set>
  local node="$1" want_ready="$2" want_alloc="$3" i r a
  for i in 1 2; do
    r="$(ready_of "$node")"; a="$(alloc_of "$node")"
    if [ "$r" != "$want_ready" ]; then
      echo "  !! read $i: $node Ready='$r', wanted '$want_ready'" >&2
      echo "  !! the state did not hold — do NOT capture" >&2
      exit 2
    fi
    case "$want_alloc" in
      empty) [ -z "$a" ] || { echo "  !! read $i: allocatable.cpu='$a', wanted empty" >&2; exit 2; } ;;
      set)   [ -n "$a" ] || { echo "  !! read $i: allocatable.cpu empty, wanted a value" >&2; exit 2; } ;;
    esac
    echo "  ok  read $i: $node Ready=$r allocatable.cpu='${a:-<empty>}'"
    # `if`, not `[ .. ] && ..`: on the second pass the test is false, that would
    # be the function's last command, and `set -e` would abort the CALLER after
    # both reads printed "ok" — which is what happened on this script's first
    # run, skipping the fleet-wide check below.
    if [ "$i" = 1 ]; then sleep "$SETTLE"; fi
  done
}

NODE="$(target)"
[ -n "$NODE" ] || { echo "  !! no sys/index-000 node — the fixture cannot exhibit this" >&2; exit 2; }

case "$MODE" in
  notready)
    # The capacity half must already be true, or this proves nothing.
    [ -z "$(alloc_of "$NODE")" ] || {
      echo "  !! $NODE reports capacity — run 9-node-capacity.sh MODE=restore first" >&2; exit 2; }
    docker stop "$KWOK" >/dev/null
    echo "  stopped $KWOK (the window is ~200s before the whole fleet ages out)"
    set_ready "$NODE" False
    assert_twice "$NODE" False empty
    # The 200s hazard: if the lease has already lapsed, the capture would show a
    # NotReady FLEET rather than one node that is both.
    OTHERS="$(not_ready_nodes | grep -v "^${NODE}$" || true)"
    [ -z "$OTHERS" ] || {
      echo "  !! other nodes have gone NotReady — the lease lapsed, restore and retry:" >&2
      echo "$OTHERS" | head | sed 's/^/     /' >&2; exit 2; }
    echo "  ok  $NODE is the ONLY NotReady node — capture now"
    ;;
  restore)
    set_ready "$NODE" True || true
    docker start "$KWOK" >/dev/null
    echo "  started $KWOK"
    assert_twice "$NODE" True empty
    LEFT="$(not_ready_nodes || true)"
    if [ -n "$LEFT" ]; then
      echo "  !! still NotReady after restore:" >&2
      while IFS= read -r n; do echo "     $n" >&2; done <<< "$LEFT"
      exit 2
    fi
    echo "  ok  the whole fleet is Ready again"
    ;;
  *) echo "  !! MODE must be notready|restore" >&2; exit 2 ;;
esac
