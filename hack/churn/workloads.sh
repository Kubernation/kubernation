#!/usr/bin/env bash
# The workload spread that rides on the fleet.
#
# Deliberately mixed, because scenario 1 has to churn CITIES as well as nodes
# (guidance §3): a rolling refresh that moves nodes but leaves workloads pinned
# would test an easier problem than the real one.
#   - deployments of several sizes, so settlement tiers vary
#   - a DaemonSet, so every node carries infrastructure that must follow it
#   - a StatefulSet, whose pods are the ones most disturbed by a node vanishing
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh

# name:replicas:cpu:mem — a few large, several small (a realistic long tail)
DEPLOYS="${DEPLOYS:-api:120:200m:256Mi web:80:100m:128Mi worker:60:500m:512Mi cache:24:250m:1Gi batch:12:1:2Gi ingest:8:100m:128Mi}"

{
  cat <<'EOF'
---
apiVersion: v1
kind: Namespace
metadata:
  name: churn
EOF

  for entry in $DEPLOYS; do
    name=$(echo "$entry" | cut -d: -f1)
    reps=$(echo "$entry" | cut -d: -f2)
    cpu=$(echo "$entry" | cut -d: -f3)
    mem=$(echo "$entry" | cut -d: -f4)
    cat <<EOF
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ${name}
  namespace: churn
spec:
  replicas: ${reps}
  selector:
    matchLabels: {app: ${name}}
  template:
    metadata:
      labels: {app: ${name}}
    spec:
      tolerations: [{operator: Exists}]
      # Spread across zones so a zone loss is a real event, not a no-op.
      topologySpreadConstraints:
        - maxSkew: 3
          topologyKey: topology.kubernetes.io/zone
          whenUnsatisfiable: ScheduleAnyway
          labelSelector:
            matchLabels: {app: ${name}}
      containers:
        - name: app
          image: fake.registry/${name}:1.0
          resources:
            requests: {cpu: ${cpu}, memory: ${mem}}
EOF
  done

  cat <<'EOF'
---
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: node-agent
  namespace: churn
spec:
  selector:
    matchLabels: {app: node-agent}
  template:
    metadata:
      labels: {app: node-agent}
    spec:
      tolerations: [{operator: Exists}]
      containers:
        - name: agent
          image: fake.registry/node-agent:1.0
          resources:
            requests: {cpu: 50m, memory: 64Mi}
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: store
  namespace: churn
spec:
  serviceName: store
  replicas: 12
  selector:
    matchLabels: {app: store}
  template:
    metadata:
      labels: {app: store}
    spec:
      tolerations: [{operator: Exists}]
      containers:
        - name: store
          image: fake.registry/store:1.0
          resources:
            requests: {cpu: 500m, memory: 2Gi}
EOF
} | kc apply -f - >/dev/null

log "workloads applied (deployments + daemonset + statefulset in ns/churn)"
