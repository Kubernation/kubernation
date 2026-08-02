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
# Anchor on a pool the scenarios do NOT refresh. Scenario 1 refreshes `sys` by
# default, and an anchor inside the pool under test vanishes mid-run — the camera
# then reframes and the resulting "movement" is the camera, not the map, which is
# the precise trap a stability flipbook exists to avoid.
CENTER="${CENTER:-churn-mem-g1-000}"
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
  # LOUD, not silent: a reframed capture is not comparable with its neighbours,
  # and a flipbook that quietly changes framing mid-sequence reads as movement.
  echo "  !! anchor '$CENTER' is gone — this frame is NOT comparable." >&2
  echo "     Set CENTER to a node outside the pool under test." >&2
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
