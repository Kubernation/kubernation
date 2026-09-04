#!/usr/bin/env bash
# The advisor pages must not decide for themselves how a line reaches the screen.
#
# `emit_line` is the one home: a RsRole::Caveat WRAPS, everything else
# TRUNCATES. A page that calls `almanac::wrap` or `panels::fit_width` directly
# has re-made that decision locally, and the two failure modes are opposite and
# both silent — a wrapped ROW loses the indent that puts it under its heading
# (wrap splits on whitespace), and a truncated CAVEAT ends at "…beca", which is
# not a stated caveat.
#
# A lint rather than a test, for the reason D2 §3.4 established: the page
# functions are GL-driven and have no test module, so no behavioural test can
# observe a second copy of the decision appearing in one of them.
set -euo pipefail
cd "$(dirname "$0")/.."
F=crates/kubernation/src/advisor.rs

# The line range of `fn emit_line`, from its signature to its closing brace at
# column 0 — computed, not hardcoded, so it survives the file moving.
emit_line_range() {
  awk '
    /^fn emit_line\(/ { start = NR; inside = 1 }
    inside && /^}/    { print start "," NR; exit }
  ' "$F"
}

RANGE="$(emit_line_range)"
[ -n "$RANGE" ] || { echo "  !! could not find fn emit_line in $F" >&2; exit 2; }
LO="${RANGE%,*}"; HI="${RANGE#*,}"

# Every line that makes the wrap-or-truncate call, with its line number.
callers() { grep -n 'almanac::wrap(\|panels::fit_width(' "$F" | cut -d: -f1; }

TOTAL=0
BAD=0
while read -r n; do
  [ -n "$n" ] || continue
  TOTAL=$((TOTAL + 1))
  if [ "$n" -lt "$LO" ] || [ "$n" -gt "$HI" ]; then
    echo "  !! $F:$n decides wrap-vs-truncate outside emit_line ($LO-$HI):" >&2
    sed -n "${n}p" "$F" | sed 's/^/     /' >&2
    BAD=$((BAD + 1))
  fi
done <<< "$(callers)"

# Guard the guard. `check-release-targets.sh` shipped checking one of three
# platforms and reporting success either way, because its parse silently
# matched less than it should. If the calls are ever renamed, this must fail
# rather than pass on an empty set.
if [ "$TOTAL" -lt 2 ]; then
  echo "  !! found only $TOTAL wrap/truncate call(s) — expected both;" >&2
  echo "  !! the parse matched less than it should. Fix this script." >&2
  exit 2
fi

[ "$BAD" -eq 0 ] || exit 1
echo "advisor lines are rendered only through emit_line"
