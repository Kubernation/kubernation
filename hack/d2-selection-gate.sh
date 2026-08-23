#!/usr/bin/env bash
# D2 §8: does a selection survive a reschedule and a zone addition, still
# pointing at the right thing?
#
# WHY THIS BUILDS ITS OWN CLUSTERS
#
# The gate needs real placement, not the churn fleet's specific reference state
# — and that fleet was created by kwokctl 0.7 and cannot be started by 0.8
# ("component etcd does not exist"), so running it here would mean recreating
# it and destroying the layout store carrying T1's succession record, which
# `docs/reports/t2-pre-pool-gap.md` deliberately declined to do. So this stands
# up two throwaway kwok clusters, measures, and tears them down.
#
# WHAT IT MEASURES, AND WHAT IT REFUSES TO MEASURE
#
# Not a before/after image. The MARK is supposed to move — the subject moved —
# so a pixel diff would confirm the wrong thing. This compares what the
# selection RESOLVES TO against the identity's current position, from
# `--dump-positions`, which now emits the selection alongside the world.
#
# Each gate also runs its own discrimination check: it reports what the cell a
# PRE-INVERSION selection would still hold now points at. If that were still the
# same thing, the staleness would not be real and the fix should not be credited.
set -euo pipefail
cd "$(dirname "$0")/.."

HOT=kn-d2gate
WARM=kn-d2warm
KEEP="${KEEP:-0}"

node() { # cluster name zone
  cat <<YAML | kubectl --context "kwok-$1" apply -f - >/dev/null
apiVersion: v1
kind: Node
metadata:
  name: $2
  labels: {kubernetes.io/hostname: $2, topology.kubernetes.io/zone: $3, type: kwok}
  annotations: {kwok.x-k8s.io/node: fake}
spec: {}
status:
  allocatable: {cpu: "8", memory: 32Gi, pods: "110"}
  capacity: {cpu: "8", memory: 32Gi, pods: "110"}
  nodeInfo: {architecture: amd64, operatingSystem: linux, kubeletVersion: kwok-v0.8.0}
  conditions: [{type: Ready, status: "True", reason: KubeletReady}]
YAML
}

deploy() { # cluster name pinned-node replicas
  kubectl --context "kwok-$1" create ns gate >/dev/null 2>&1 || true
  cat <<YAML | kubectl --context "kwok-$1" apply -f - >/dev/null
apiVersion: apps/v1
kind: Deployment
metadata: {name: $2, namespace: gate, labels: {app: $2}}
spec:
  replicas: $4
  selector: {matchLabels: {app: $2}}
  template:
    metadata: {labels: {app: $2}}
    spec:
      nodeSelector: {kubernetes.io/hostname: $3}
      containers: [{name: app, image: nginx:1.27, resources: {requests: {cpu: 100m, memory: 64Mi}}}]
YAML
}

cleanup() {
  [ "$KEEP" = "1" ] && { echo "  KEEP=1 — leaving $HOT / $WARM up"; return; }
  for c in "$HOT" "$WARM"; do
    kwokctl delete cluster --name "$c" >/dev/null 2>&1 || true
    rm -f "$HOME/.local/state/kubernation/layouts/kwok-$c.json"
  done
}
trap cleanup EXIT

# Always from scratch. Re-using a cluster left over from a previous run is how
# this gate first reported a false pass: gate B's precondition (the hot world
# GAINS a zone) was already satisfied before the session started, so the extent
# went 86 -> 86 and every other assertion still held. The discrimination check
# caught it; starting clean is what stops it happening.
echo "== standing up two throwaway kwok clusters (deleting any leftovers first) =="
for c in "$HOT" "$WARM"; do
  kwokctl delete cluster --name "$c" >/dev/null 2>&1 || true
  rm -f "$HOME/.local/state/kubernation/layouts/kwok-$c.json"
  kwokctl create cluster --name "$c" >/dev/null 2>&1
done
for n in a1 a2 a3; do node "$HOT" "$n" z-a; done
for n in b1 b2 b3; do node "$HOT" "$n" z-b; done
node "$WARM" w1 z-w; node "$WARM" w2 z-w
deploy "$HOT" wanderer a1 3
deploy "$WARM" mirror w1 2
sleep 5

run_session() { # out.jsonl inspect-needle [extra args...]
  local out="$1" needle="$2"; shift 2
  rm -f "$out"
  ( cargo run -q -p kubernation -- --context "kwok-$HOT" \
      --overlay terrain --map-style plain \
      --inspect "$needle" --dump-positions "$out" \
      --shot-seq 12 --shot-interval 5 \
      --screenshot /tmp/d2-gate-frame.png "$@" ) >/tmp/d2-gate.log 2>&1 &
  echo $!
}

echo
echo "== GATE A: a selected workload's city is RESCHEDULED to another zone =="
app=$(run_session /tmp/d2-gate-a.jsonl wanderer --zoom 0.6)
sleep 14
kill -0 "$app" 2>/dev/null || { echo "  !! client exited early"; tail -5 /tmp/d2-gate.log; exit 1; }
kubectl --context "kwok-$HOT" patch deploy wanderer -n gate --type merge \
  -p '{"spec":{"template":{"spec":{"nodeSelector":{"kubernetes.io/hostname":"b2"}}}}}' >/dev/null
# Wait for the reschedule to actually LAND, rather than for a fixed number of
# seconds. A fixed sleep is how gate A first reported "the city never moved" on
# a freshly-built cluster: the assertion was right, the wait was too short.
for _ in $(seq 40); do
  on_b2=$(kubectl --context "kwok-$HOT" get pods -n gate -l app=wanderer \
    -o jsonpath='{range .items[*]}{.spec.nodeName}{"\n"}{end}' 2>/dev/null | grep -c '^b2$' || true)
  total=$(kubectl --context "kwok-$HOT" get pods -n gate -l app=wanderer --no-headers 2>/dev/null | wc -l | tr -d ' ')
  [ "$on_b2" -gt 0 ] && [ "$on_b2" = "$total" ] && break
  sleep 2
done
echo "   pods on b2: ${on_b2}/${total}"
sleep 16   # let the world rebuild and the dump record the new placement
kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true

echo
echo "== GATE B: the HOT world gains a zone while a WARM city is selected =="
app=$(run_session /tmp/d2-gate-b.jsonl mirror --zoom 0.4 --warm "kwok-$WARM")
sleep 16
kill -0 "$app" 2>/dev/null || { echo "  !! client exited early"; tail -5 /tmp/d2-gate.log; exit 1; }
for n in c1 c2; do node "$HOT" "$n" z-c; done
sleep 26   # the zone appears as soon as the Node objects are watched
kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true

echo
python3 hack/d2-selection-gate.py /tmp/d2-gate-a.jsonl /tmp/d2-gate-b.jsonl
