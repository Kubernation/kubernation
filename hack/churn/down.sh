#!/usr/bin/env bash
# Tear the churn cluster down. Captures in out/ are left alone.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh

if kwokctl get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  log "deleting kwok cluster '$CLUSTER'"
  kwokctl delete cluster --name "$CLUSTER" >/dev/null 2>&1
  log "gone"
else
  log "cluster '$CLUSTER' not present"
fi
