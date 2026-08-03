#!/usr/bin/env bash
# Shared helpers for the churn harness. Sourced, not executed.
#
# Bash 3.2 compatible on purpose — macOS ships 3.2 and this repo has already
# been bitten once by a 3.2 incompatibility (see the release-macos.sh note in
# the decision log). No associative arrays, no `mapfile`, no `${var,,}`.

CLUSTER="${CLUSTER:-kubernation-churn}"
CTX="${CTX:-kwok-${CLUSTER}}"

# Pool table: name|label-key|zones(space-sep)|count|cpu|mem|instance-type
#
# Deliberately heterogeneous, because A depends on all of it (guidance §4):
#   - four zones, UNEVENLY filled (a-37 b-22 c-26 d-15) — even zones hide bugs
#   - three DIFFERENT provider label conventions, so the fallback cascade has
#     something real to cascade over
#   - `edge` carries NO pool label at all — the case that exercises the
#     fallback's last resort
#   - `sys` SPANS THREE ZONES — the case that broke the naive zone/pool nesting
#   - `mem` has NO instance-type — exercises extent fallback
#   - capacities differ per pool, so capacity-derived extent varies visibly
# shellcheck disable=SC2034  # consumed by the scripts that source this file
POOLS=(
  "sys|cloud.google.com/gke-nodepool|z-a z-b z-c|30|8|32Gi|n2-standard-8"
  "burst|karpenter.sh/nodepool|z-a z-b|24|16|64Gi|c6i.4xlarge"
  "mem|eks.amazonaws.com/nodegroup|z-c|16|8|128Gi|"
  "edge||z-a z-d|30|4|16Gi|t3.xlarge"
)

pool_field() { echo "$1" | cut -d'|' -f"$2"; }

log() { printf '  %s\n' "$*" >&2; }

kc() { kubectl --context "$CTX" "$@"; }

# node_yaml <name> <zone> <pool> <label-key> <cpu> <mem> <instance-type> [no-alloc]
#
# `no-alloc` omits status.allocatable entirely. kwok does NOT backfill it
# (verified), so this reproduces a real, if rare, node. See the README for what
# KuberNation actually renders for such a node — it is not what you would hope.
node_yaml() {
  local name="$1" zone="$2" pool="$3" key="$4" cpu="$5" mem="$6" itype="$7" noalloc="${8:-}" idx="${9:-}"
  cat <<EOF
---
apiVersion: v1
kind: Node
metadata:
  name: ${name}
  annotations:
    kwok.x-k8s.io/node: fake
    node.alpha.kubernetes.io/ttl: "0"
  labels:
    type: kwok
    kubernetes.io/hostname: ${name}
    kubernetes.io/os: linux
    kubernetes.io/arch: arm64
    node-role.kubernetes.io/agent: ""
    topology.kubernetes.io/zone: ${zone}
    churn.kubernation.io/pool: ${pool}
    churn.kubernation.io/index: "${idx}"
EOF
  # The provider-specific key is the one a real fallback cascade must handle.
  # `edge` has none — that is the point of it.
  if [ -n "$key" ]; then
    echo "    ${key}: ${pool}"
  fi
  if [ -n "$itype" ]; then
    echo "    node.kubernetes.io/instance-type: ${itype}"
  fi
  if [ -z "$noalloc" ]; then
    cat <<EOF
status:
  allocatable:
    cpu: "${cpu}"
    memory: ${mem}
    pods: "110"
  capacity:
    cpu: "${cpu}"
    memory: ${mem}
    pods: "110"
  nodeInfo:
    architecture: arm64
    containerRuntimeVersion: kwok
    kubeletVersion: fake-v1.33.0
    operatingSystem: linux
EOF
  else
    # LOAD-BEARING: the explicit Ready condition is what keeps allocatable absent.
    # kwok's node-initialize stage fires only on a node with no Ready condition,
    # and it backfills a default capacity (1k cpu / 1Ti / 1M pods) — so a node
    # that merely omits `allocatable` silently acquires an enormous one, which is
    # both untrue and pollutes the heterogeneous-capacity spread. Supplying the
    # condition ourselves opts out of that stage and the field stays genuinely
    # absent (verified: still absent after 20s).
    cat <<EOF
status:
  conditions:
    - type: Ready
      status: "True"
      reason: KubeletReady
  nodeInfo:
    architecture: arm64
    containerRuntimeVersion: kwok
    kubeletVersion: fake-v1.33.0
    operatingSystem: linux
EOF
  fi
}

# Emit every node of one pool for a given generation.
# pool_nodes <pool-spec> <gen> [start-index] [count-override]
pool_nodes() {
  local spec="$1" gen="$2" start="${3:-0}" override="${4:-}"
  local pool key zones count cpu mem itype
  pool=$(pool_field "$spec" 1)
  key=$(pool_field "$spec" 2)
  zones=$(pool_field "$spec" 3)
  count=$(pool_field "$spec" 4)
  cpu=$(pool_field "$spec" 5)
  mem=$(pool_field "$spec" 6)
  itype=$(pool_field "$spec" 7)
  [ -n "$override" ] && count="$override"

  # shellcheck disable=SC2206
  local zarr=($zones)
  local i zone name noalloc
  for i in $(seq "$start" $((start + count - 1))); do
    zone="${zarr[$((i % ${#zarr[@]}))]}"
    name=$(printf "churn-%s-%s-%03d" "$pool" "$gen" "$i")
    # Exactly one node in the fleet has no allocatable (guidance §4).
    noalloc=""
    if [ "$pool" = "sys" ] && [ "$i" -eq 0 ]; then noalloc="yes"; fi
    node_yaml "$name" "$zone" "$pool" "$key" "$cpu" "$mem" "$itype" "$noalloc" "$(printf '%03d' "$i")"
  done
}

# Wait until every node matching a label selector reports Ready.
wait_nodes_ready() {
  local selector="$1" timeout="${2:-120}"
  kc wait --for=condition=Ready node -l "$selector" --timeout="${timeout}s" >/dev/null
}

# Wait until the deployments settle, so a capture is not of a half-scheduled world.
wait_pods_settled() {
  local timeout="${1:-180}"
  kc wait --for=condition=Available deploy -n churn --all --timeout="${timeout}s" >/dev/null 2>&1 || true
}

# Wait until no pod is bound to a node that no longer exists.
#
# Deleting a Node leaves its pods Running in the store until PodGC collects them
# (~30-60s under kwok's real controller-manager) — the same "pod outliving its
# node" window that bit the substrate report's prevalence maths. A capture taken
# inside it shows workloads on departed nodes, which is honest but is NOT the
# settled state an A2 gate judgment wants.
wait_no_orphan_pods() {
  local timeout="${1:-90}" i=0 orphans
  while [ "$i" -lt "$timeout" ]; do
    kc get nodes -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}' 2>/dev/null \
      | sort -u > /tmp/.churn-nodes || true
    kc get pods -A -o jsonpath='{range .items[*]}{.spec.nodeName}{"\n"}{end}' 2>/dev/null \
      | grep -v '^$' | sort -u > /tmp/.churn-podnodes || true
    # Pod-referenced node names that are not in the node list.
    orphans=$(comm -23 /tmp/.churn-podnodes /tmp/.churn-nodes | wc -l | tr -d ' ')
    [ "$orphans" = "0" ] && { rm -f /tmp/.churn-nodes /tmp/.churn-podnodes; return 0; }
    sleep 3
    i=$((i + 3))
  done
  rm -f /tmp/.churn-nodes /tmp/.churn-podnodes
  log "note: pods still bound to departed nodes after ${timeout}s (PodGC lag)"
}

require_cluster() {
  if ! kwokctl get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
    echo "cluster '$CLUSTER' not found — run hack/churn/up.sh first" >&2
    exit 1
  fi
}
