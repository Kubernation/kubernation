#!/usr/bin/env bash
# Induce one failure shape on the kind dev cluster, reversibly.
#
# T2-pre asks which DIMENSION failures cluster in. Each shape below is chosen
# because its expected dimension differs; the expectation is stated here, before
# running, so the result can disagree with it.
#
#   MODE=crashloop   a container exiting nonzero on a loop     expect: workload
#   MODE=rollout     a Deployment rolled to an invalid image   expect: workload
#   MODE=storage     a PVC on an unsatisfiable StorageClass    expect: workload
#   MODE=nodedown    a worker's kubelet stops                  expect: node
#   MODE=down        restore everything
#
# NOT constructible here, and that is a result rather than an omission:
# memory-pressure EVICTION, the canonical node-shaped failure. All four kind
# nodes are containers inside one Docker VM sharing its ~15.6 GiB, so filling
# "a node's" memory fills every node's and the host's — the pressure would not
# be node-scoped even if it were safe to induce. `nodedown` is the substitute:
# genuinely node-shaped (victims chosen by location, not identity), reversible,
# and it does not risk the cluster.
#
# The kind cluster is a reference state like the churn fleet. Nothing here adds
# nodes; every change is undone by MODE=down.
set -euo pipefail

CTX="${CTX:-kind-kubernation}"
NS="${NS:-kubernation-demo}"
MODE="${MODE:-down}"
REPLICAS="${REPLICAS:-9}"
VICTIM="${VICTIM:-kubernation-worker2}"

kc() { kubectl --context "$CTX" "$@"; }
log() { printf '  %s\n' "$*" >&2; }

restore() {
  log "restoring baseline"
  kc -n "$NS" delete deploy stuck-storage --ignore-not-found >/dev/null 2>&1 || true
  kc -n "$NS" delete pvc -l t2pre=storage --ignore-not-found >/dev/null 2>&1 || true
  kc -n "$NS" scale deploy/crashy --replicas=0 >/dev/null 2>&1 || true
  kc -n "$NS" set image deploy/web nginx=nginx:1.27-alpine >/dev/null 2>&1 || true
  kc -n "$NS" scale deploy/web --replicas=3 >/dev/null 2>&1 || true
  if ! docker ps --format '{{.Names}}' | grep -qx "$VICTIM"; then
    log "restarting $VICTIM"
    docker start "$VICTIM" >/dev/null
  fi
}

case "$MODE" in
  down) restore ;;

  crashloop)
    log "EXPECT workload-shaped: a crash-looping container follows its Deployment's pods"
    kc -n "$NS" scale deploy/crashy --replicas="$REPLICAS" >/dev/null
    ;;

  rollout)
    log "EXPECT workload-shaped: a bad image follows the Deployment wherever it lands"
    kc -n "$NS" scale deploy/web --replicas="$REPLICAS" >/dev/null
    kc -n "$NS" set image deploy/web nginx=nginx:no-such-tag-t2pre >/dev/null
    ;;

  storage)
    log "EXPECT workload-shaped: an unbindable PVC blocks this Deployment only"
    cat <<EOF | kc apply -f - >/dev/null
apiVersion: apps/v1
kind: Deployment
metadata:
  name: stuck-storage
  namespace: ${NS}
spec:
  replicas: ${REPLICAS}
  selector: { matchLabels: { app: stuck-storage } }
  template:
    metadata:
      labels: { app: stuck-storage }
    spec:
      containers:
        - name: app
          image: nginx:1.27-alpine
          volumeMounts: [{ name: data, mountPath: /data }]
      volumes:
        - name: data
          persistentVolumeClaim: { claimName: stuck-storage-pvc }
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: stuck-storage-pvc
  namespace: ${NS}
  labels: { t2pre: storage }
spec:
  accessModes: [ReadWriteOnce]
  storageClassName: no-such-storage-class
  resources: { requests: { storage: 1Gi } }
EOF
    ;;

  nodedown)
    log "EXPECT node-shaped: victims are chosen by LOCATION, not identity"
    log "stopping $VICTIM"
    docker stop "$VICTIM" >/dev/null
    ;;

  *) echo "unknown MODE=$MODE" >&2; exit 2 ;;
esac
log "done"
