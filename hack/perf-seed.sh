#!/usr/bin/env bash
# Seed the kwok perf cluster with fake nodes and bulk workloads.
# kwok simulates kubelet for these nodes, so 100 nodes / 1000 pods cost
# almost nothing — this is the rig for the responsiveness criterion.
set -euo pipefail

CTX="${1:-kwok-kubernation-perf}"
NODES="${NODES:-100}"
DEPLOYS="${DEPLOYS:-20}"
REPLICAS="${REPLICAS:-50}"
ZONES=(z-a z-b z-c z-d z-e)

nodes_yaml() {
  for i in $(seq 0 $((NODES - 1))); do
    zone="${ZONES[$((i % ${#ZONES[@]}))]}"
    name=$(printf "perf-node-%03d" "$i")
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
status:
  allocatable:
    cpu: "8"
    memory: 32Gi
    pods: "110"
  capacity:
    cpu: "8"
    memory: 32Gi
    pods: "110"
  nodeInfo:
    architecture: arm64
    containerRuntimeVersion: kwok
    kubeletVersion: fake-v1.33.0
    operatingSystem: linux
EOF
  done
}

deploys_yaml() {
  cat <<EOF
---
apiVersion: v1
kind: Namespace
metadata:
  name: perf
EOF
  for d in $(seq 0 $((DEPLOYS - 1))); do
    name=$(printf "app-%02d" "$d")
    cat <<EOF
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ${name}
  namespace: perf
spec:
  replicas: ${REPLICAS}
  selector:
    matchLabels:
      app: ${name}
  template:
    metadata:
      labels:
        app: ${name}
    spec:
      tolerations:
        - operator: Exists
      containers:
        - name: app
          image: fake.registry/app:latest
          resources:
            requests:
              cpu: 100m
              memory: 128Mi
EOF
  done
}

nodes_yaml | kubectl --context "$CTX" apply -f - >/dev/null
deploys_yaml | kubectl --context "$CTX" apply -f - >/dev/null
echo "seeded ${CTX}: ${NODES} nodes, ${DEPLOYS} deployments x ${REPLICAS} replicas"
