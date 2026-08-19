#!/usr/bin/env bash
# Confine the map's cell -> identity conversion to files that are under test.
#
# WHY THIS IS A LINT AND NOT A TEST
#
# D2's §3.4 gate re-introduced a second copy of that conversion in `main.rs` —
# first verbatim, then carrying a `Region::Structure` arm that genuinely changed
# behaviour — and the whole suite stayed green both times. `main.rs` has no
# `#[cfg(test)]` module, by the v0.66.0 GUI testability policy (macroquad is
# immediate-mode + GL), so there was nothing there to go red. No behavioural
# test can catch a re-mirror in a file that has no tests.
#
# So the decision was moved out (see `draw::subject_at`, `blast_subject`,
# `selected_scope`, `city_at`), and this keeps it out. Drift INSIDE draw.rs is
# caught by its tests; a copy re-introduced anywhere else is caught here.
#
# The sanctioned list is one FILE, not a list of functions, and it is meant to
# stay that way: §4 of the D2-fix guidance notes that a lint which fires
# spuriously gets suppressed rather than fixed. If this list ever needs to grow
# more than rarely, delete the guard instead of shipping one people ignore.
set -euo pipefail
cd "$(dirname "$0")/.."

AUTHORITY="crates/kubernation/src/draw.rs"
bad=0

for f in $(find crates/kubernation/src -name '*.rs' | sort); do
  [ "$f" = "$AUTHORITY" ] && continue
  # Consider production code only: everything above the file's test module.
  cut="$(grep -n '#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1 || true)"
  body="$(if [ -n "$cut" ]; then head -n "$((cut - 1))" "$f"; else cat "$f"; fi)"
  if hits="$(printf '%s' "$body" | grep -n 'region_at(' || true)"; [ -n "$hits" ]; then
    echo "::error::$f calls region_at outside the conversion authority:"
    printf '%s\n' "$hits"
    bad=1
  fi
done

if [ "$bad" -ne 0 ]; then
  cat >&2 <<'MSG'

  `region_at` turns a map cell into the thing it names. That conversion lives in
  draw.rs -- `subject_at` (identity), `city_at` (a city, for callers holding a
  local cell) and `resolve_region` (the two-plane panel question) -- because
  those are the ones that are tested.

  A copy elsewhere is what D2's gate demonstrated goes unnoticed. Call one of
  the three, or add the new home to AUTHORITY here and give it tests.
MSG
  exit 1
fi
echo "region_at is confined to $AUTHORITY"
