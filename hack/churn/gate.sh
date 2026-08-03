#!/usr/bin/env bash
# The A2 stability flipbook, captured from ONE long-lived session.
#
#   gate.sh [scenario-args-via-env]   -> out/gate/frame-00.png … frame-NN.png
#
# WHY NOT capture.sh IN A LOOP. Each capture.sh frame is its own process, so its
# layout starts empty and is assigned from scratch. Assignment is deterministic
# in the node set, so a process-per-frame flipbook renders the same thing whether
# or not the layout carry exists at all — delete the carry from net.rs and the
# flipbook is unchanged. It measures determinism, which was never in doubt, and
# is structurally blind to the mechanism the gate exists to judge.
#
# A long-lived session is the only regime the product ever runs in, and the only
# one where a slot can outlive its occupant. It is also the LESS flattering one:
# from scratch, a replaced node simply inherits the departed one's ordinal and
# the map looks perfect; carried, the departed slot stays reserved and the
# replacement appends below it, which is a real, visible change the honest
# flipbook has to show.
#
#   SHOTS=14      frames to take
#   INTERVAL=18   seconds between frames
#   CENTER=…      camera anchor — MUST be outside the pool under refresh
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

OUT="${OUT:-out/gate}"
SHOTS="${SHOTS:-14}"
INTERVAL="${INTERVAL:-18}"
# Anchored in `mem`, which no scenario refreshes, and zoomed to a framing that
# contains SETTLEMENTS as well as provinces. The first gate run framed a view
# with no cities in it at all, which quietly excluded the one axis A2 does not
# fix (city placement is A3's) from the measurement.
CENTER="${CENTER:-churn-mem-g1-000}"
ZOOM="${ZOOM:-0.55}"
SETTLE="${SETTLE:-12}"

mkdir -p "$OUT"
rm -f "$OUT"/frame-*.png

# CENTER= (empty) frames the whole world instead of an anchor. The fit runs once
# on first sync, so the camera still holds still for the rest of the session.
anchor=()
[ -n "$CENTER" ] && anchor=("--center" "$CENTER")

log "flipbook: ${SHOTS} frames, ${INTERVAL}s apart, one session (anchor ${CENTER:-fit})"
( cd ../.. && cargo run -q -p kubernation -- \
    --context "$CTX" \
    ${anchor[@]+"${anchor[@]}"} \
    --zoom "$ZOOM" \
    --overlay terrain \
    --map-style plain \
    --shot-seq "$SHOTS" \
    --shot-interval "$INTERVAL" \
    --screenshot "hack/churn/$OUT/frame.png" ) >/dev/null 2>&1 &
app=$!

# Let it connect, sync and take frame 00 before anything churns.
sleep "$SETTLE"
if ! kill -0 "$app" 2>/dev/null; then
  echo "  !! the client exited before the scenario started" >&2
  exit 1
fi

# A gate whose scenario churned NOTHING returns a perfect, meaningless result.
# Scenario 1 breaks at wave 0 when no OLD_GEN node exists — which is what you get
# by re-running it without a reset — and the flipbook then photographs a static
# fleet and reports that the map held perfectly still. Count first, and refuse.
before_old="$(kc get nodes -o name | grep -c -- "-${OLD_GEN:-g1}-" || true)"
if [ "$before_old" -eq 0 ]; then
  echo "  !! no ${OLD_GEN:-g1} nodes to refresh — the scenario would churn nothing" >&2
  echo "     and the flipbook would report a perfectly still map. Run ./reset.sh" >&2
  echo "     (or set OLD_GEN to the generation actually on the fleet) first." >&2
  kill "$app" 2>/dev/null || true
  exit 2
fi
log "refreshing ${before_old} nodes of generation ${OLD_GEN:-g1}"

CAPTURE=0 ./scenarios/1-rolling-refresh.sh

log "scenario done; waiting for the remaining frames"
wait "$app" || true
log "$(find "$OUT" -name 'frame-*.png' | wc -l | tr -d ' ') frames in $OUT"
