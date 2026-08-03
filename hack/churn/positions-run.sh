#!/usr/bin/env bash
# Record a positional dump across a scenario, from ONE long-lived session.
#
#   positions-run.sh <scenario> [out.jsonl]
#
# The session must span the whole scenario for the same reason `gate.sh` must:
# a fresh process assigns its layout from scratch, and assignment is
# deterministic in the node set, so per-invocation dumps would compare two
# independent from-scratch builds rather than one evolving world.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=hack/churn/lib.sh
. ./lib.sh
require_cluster

SCENARIO="${1:?usage: positions-run.sh <scenario> [out.jsonl]}"
OUT="${2:-out/positions.jsonl}"
[ -f "$SCENARIO" ] || SCENARIO="scenarios/${SCENARIO}"
[ -f "$SCENARIO" ] || { echo "  !! no scenario at $SCENARIO" >&2; exit 2; }

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"

# `--shot-seq` with a long interval is just a way to bound the session's life:
# the client exits after its last frame, so the run cannot hang if the scenario
# dies. The screenshots themselves are incidental here.
log "starting a session dumping positions to ${OUT}"
( cd ../.. && cargo run -q -p kubernation -- \
    --context "$CTX" \
    --zoom 0.55 --overlay terrain --map-style plain \
    --dump-positions "hack/churn/$OUT" \
    --shot-seq 60 --shot-interval 20 \
    --screenshot "hack/churn/out/positions-frame.png" ) >/dev/null 2>&1 &
app=$!

sleep 12
kill -0 "$app" 2>/dev/null || { echo "  !! the client exited before the scenario started" >&2; exit 1; }

set +e
"./$SCENARIO"
rc=$?
set -e

kill "$app" 2>/dev/null || true
wait "$app" 2>/dev/null || true

ticks=$(python3 -c "
import json,sys
ts={json.loads(l)['tick'] for l in open('$OUT') if l.strip()}
print(len(ts))
" 2>/dev/null || echo 0)
log "${ticks} ticks recorded in ${OUT}"
if [ "$ticks" -lt 2 ]; then
  echo "  !! fewer than two ticks — nothing to compare" >&2
  exit 2
fi
exit "$rc"
