#!/usr/bin/env bash
# Capture a consistently-framed screenshot of the churn fleet.
#
#   capture.sh <label> [n]        -> out/<label>.png  (or <label>-<n>.png)
#
# EVERY view-affecting flag is pinned, not just framing. --overlay and
# --map-style PERSIST between runs in ~/.config/kubernation/prefs.json and are
# restored at launch, so an unpinned capture inherits whatever the operator last
# used — during development this harness's own author got a capture tinted by
# the Namespace overlay in Relief for exactly that reason. A before/after pair
# taken on different days would otherwise differ for reasons that have nothing
# to do with churn.
#
# Each capture is a separate process that connects and waits for sync (~5-6s
# against a 100-node fleet), so frames are >=6s apart and not precisely spaced.
# Scenarios take an OVERLAP/PAUSE parameter for this reason: a refresh that
# completes in three seconds is invisible to a six-second sampling interval.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

LABEL="${1:?usage: capture.sh <label> [n]}"
SEQ="${2:-}"

OUT="${OUT:-out}"
CENTER="${CENTER:-churn-sys-g1-000}"
ZOOM="${ZOOM:-0.55}"
OVERLAY="${OVERLAY:-terrain}"
STYLE="${STYLE:-plain}"

mkdir -p "$OUT"
if [ -n "$SEQ" ]; then
  name=$(printf "%s-%02d" "$LABEL" "$SEQ")
else
  name="$LABEL"
fi
path="${OUT}/${name}.png"

# --center takes a node/city NAME. During a rolling refresh the node it names is
# replaced, so framing must not depend on a node that can vanish mid-scenario:
# fall back to a fit-the-world view if the anchor is gone.
anchor=("--center" "$CENTER")
if ! kc get node "$CENTER" >/dev/null 2>&1; then
  anchor=()
fi

( cd ../.. && cargo run -q -p kubernation -- \
    --context "$CTX" \
    ${anchor[@]+"${anchor[@]}"} \
    --zoom "$ZOOM" \
    --overlay "$OVERLAY" \
    --map-style "$STYLE" \
    --screenshot "hack/churn/$path" ) >/dev/null 2>&1

log "captured ${path}  (overlay=${OVERLAY} style=${STYLE} zoom=${ZOOM})"
