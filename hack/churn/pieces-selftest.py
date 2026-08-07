#!/usr/bin/env python3
"""Self-tests for pieces.py.

A measurement script always emits a plausible-looking number; these check it
emits the RIGHT one. Run: python3 hack/churn/pieces-selftest.py
"""

import importlib.util
import json
import os
import random
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("pieces", os.path.join(HERE, "pieces.py"))
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)

fails = []


def check(name, cond, detail=""):
    print(f"  {'ok  ' if cond else 'FAIL'}  {name}" + (f"  [{detail}]" if detail and not cond else ""))
    if not cond:
        fails.append(name)


def slot(zone, pool, ordinal, occupant=True, changed=False):
    s = {"zone": zone, "pool": pool, "ordinal": ordinal}
    if occupant:
        s["occupant"] = f"n-{zone}-{ordinal}"
    if changed:
        s["occupied_at"] = 1
    return s


print("1. runs of consecutive ordinals")
check("consecutive merge", P.runs_of_consecutive([3, 4, 5]) == [(3, 5, 3)])
check("a gap breaks", [n for _, _, n in P.runs_of_consecutive([0, 1, 3, 4, 5])] == [2, 3])
check("unsorted input is sorted", P.runs_of_consecutive([5, 3, 4]) == [(3, 5, 3)])
check("empty", P.runs_of_consecutive([]) == [])
check("singleton", P.runs_of_consecutive([7]) == [(7, 7, 1)])

print("\n2. positional runs (ghosts already collapsed)")
check("contiguous", P.runs_of_positions([False, True, True, False]) == [2])
check("split", P.runs_of_positions([True, False, True]) == [1, 1])
check("empty", P.runs_of_positions([False, False]) == [])

print("\n3. THE divergence: a ghost between two changed slots")
# Ordinals 0 and 2 changed; ordinal 1 is a GHOST, so it is absent from the live
# list entirely. By ordinal that is two pieces; collapsed to live positions it
# looks like one. Both are defensible; they must not be confused.
ordinals = [0, 2]  # live
changed = {0, 2}
by_ord = [n for _, _, n in P.runs_of_consecutive(sorted(changed))]
by_live = P.runs_of_positions([o in changed for o in ordinals])
check("by-ordinal splits on the ghost", by_ord == [1, 1], str(by_ord))
check("by-live-position does not", by_live == [2], str(by_live))
check("they disagree, which is the point", by_ord != by_live)

print("\n4. an empty changed set fabricates nothing")
s = P.summarise([], 0)
check("pieces is None, not 1", s["pieces"] is None, repr(s["pieces"]))
check("share is None, not 0.0", s["share"] is None, repr(s["share"]))
check("prints as words", P.fmt(s, 0) == "no changed slots", P.fmt(s, 0))
s1 = P.summarise([3], 3)
check("a real single piece IS 1 piece", s1["pieces"] == 1 and s1["share"] == 1.0)

print("\n5. end to end, on a layout with a known answer")
with tempfile.TemporaryDirectory() as td:
    lay = os.path.join(td, "l.json")
    slots = []
    # z-a: pool A at 0-3, pool B at 4-7. Changed = A's 1,2 (one piece).
    for o in range(4):
        slots.append(slot("z-a", "A", o, changed=(o in (1, 2))))
    for o in range(4, 8):
        slots.append(slot("z-a", "B", o))
    # z-b: pool A at 0,1 and 3,4 — ordinal 2 is a ghost, so TWO region pieces.
    for o in (0, 1, 3, 4):
        slots.append(slot("z-b", "A", o))
    slots.append(slot("z-b", "A", 2, occupant=False))
    # z-c: nothing changed at all.
    for o in range(3):
        slots.append(slot("z-c", "A", o))
    # an unpooled node that must be excluded everywhere
    slots.append(slot("z-a", P.UNPOOLED, 99, changed=True))
    json.dump({"slots": slots}, open(lay, "w"))

    out = subprocess.run(
        [sys.executable, os.path.join(HERE, "pieces.py"), "--layout", lay, "--trials", "200"],
        capture_output=True, text=True,
    ).stdout
    check("unpooled excluded and reported", "1 unpooled excluded" in out, out[:200])
    check("z-a changed is one piece", "changed  by-ordinal  1 piece(s) [2]" in out, out[:400])
    check("z-b region A splits on its ghost", "region  A          2 piece(s) [2, 2]" in out)
    check("z-c says so in words", "no changed slots" in out)
    check("the unpooled changed slot is NOT counted", "FLEET changed: 2 changed" in out, out[-300:])
    check("region totals are emitted, not inferred", "FLEET regions: 1 of 4 regions" in out, out[-300:])

print("\n6. the control discriminates — in both directions")
rng = random.Random(1)


def p_value(k, n, observed_pieces, trials=4000):
    ords = list(range(n))
    hits = 0
    for _ in range(trials):
        pick = rng.sample(ords, k)
        if len(P.runs_of_consecutive(pick)) <= observed_pieces:
            hits += 1
    return hits / trials


contiguous = p_value(6, 37, 1)
scattered = p_value(6, 37, 5)
check("a contiguous block is far from chance", contiguous < 0.01, f"p={contiguous}")
check("a typical scatter is not", scattered > 0.05, f"p={scattered}")
check("and they are ordered", contiguous < scattered)

print()
if fails:
    print(f"{len(fails)} FAILED: {fails}")
    sys.exit(1)
print("all self-tests passed")
