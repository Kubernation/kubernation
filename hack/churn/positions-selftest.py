#!/usr/bin/env python3
"""Break what `positions.py` measures and confirm it notices.

    positions-selftest.py

A comparator always emits a plausible percentage. That is the shape of failure
this workstream keeps meeting — six silent instrument failures in A2 alone, and
one more last session (component ids compared across two independently-labelled
frames, which produced a confident wrong verdict). So the classifier gets tests,
and they are committed rather than run once by hand.

The cases are §2.5 of the A3-pre guidance:

  1. the same tick twice                -> every city HELD, no deltas
  2. one city's cell shifted            -> exactly one MOVED-WITHIN
  3. a city's pods move to another node -> FOLLOWED, not a defect
  4. an empty world                     -> says so, exits non-zero
  5. differing city counts              -> ARRIVED / DEPARTED counted, not dropped

Plus one the guidance did not ask for and the design needs: a province that
MOVES while its city keeps the same offset must read CARRIED, not MOVED-WITHIN.
Absolute coordinates cannot tell those apart, and charging a layout move to
placement is exactly the misattribution this instrument exists to prevent.
"""

import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
POSITIONS = os.path.join(HERE, "positions.py")


def prov(tick, node, x, y, zone="z-a", pool="p", ordinal=0):
    return {
        "tick": tick, "kind": "province", "node": node, "zone": zone, "pool": pool,
        "ordinal": ordinal, "x": x, "y": y, "w": 26, "h": 5, "extent_source": "Allocatable",
    }


def city(tick, ref, node, px, py, ox, oy, zone="z-a"):
    return {
        "tick": tick, "kind": "city", "workload": ref, "node": node, "zone": zone,
        "x": px + ox, "y": py + oy, "ox": ox, "oy": oy,
    }


def write(records):
    fd, path = tempfile.mkstemp(suffix=".jsonl")
    with os.fdopen(fd, "w") as f:
        for r in records:
            f.write(json.dumps(r) + "\n")
    return path


def run(path, extra=()):
    r = subprocess.run([sys.executable, POSITIONS, path, *extra], capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


def count(out, cls):
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] == cls:
            return int(parts[1])
    return None


FAILURES = []


def check(name, cond, detail):
    print(f"{'ok  ' if cond else 'FAIL'} {name}: {detail}")
    if not cond:
        FAILURES.append(name)


def main():
    # A two-province, three-city world; tick 1 is tick 0 unchanged.
    def world(tick, p0=(0, 1), p1=(0, 10), c_off=(3, 1), c_node="n1"):
        return [
            prov(tick, "n1", *p0),
            prov(tick, "n2", *p1),
            city(tick, "Deployment demo/alpha", c_node,
                 *(p0 if c_node == "n1" else p1), *c_off),
            city(tick, "Deployment demo/beta", "n1", *p0, 7, 2),
            city(tick, "Deployment demo/gamma", "n2", *p1, 5, 1),
        ]

    # 1 — the same tick twice.
    p = write(world(0) + world(1))
    rc, out = run(p)
    check("identical ticks are all HELD",
          rc == 0 and count(out, "HELD") == 3 and count(out, "MOVED-WITHIN") == 0,
          f"HELD={count(out,'HELD')} MOVED-WITHIN={count(out,'MOVED-WITHIN')}")
    os.unlink(p)

    # 2 — one city's cell shifted inside its own province.
    p = write(world(0) + world(1, c_off=(4, 1)))
    rc, out = run(p)
    check("a shifted cell is exactly one MOVED-WITHIN",
          rc == 0 and count(out, "MOVED-WITHIN") == 1 and count(out, "HELD") == 2,
          f"MOVED-WITHIN={count(out,'MOVED-WITHIN')} HELD={count(out,'HELD')}")
    os.unlink(p)

    # 3 — a city's pods move to another node.
    p = write(world(0) + world(1, c_node="n2"))
    rc, out = run(p)
    check("a city following its pods is FOLLOWED, not a defect",
          rc == 0 and count(out, "FOLLOWED") == 1 and count(out, "MOVED-WITHIN") == 0,
          f"FOLLOWED={count(out,'FOLLOWED')} MOVED-WITHIN={count(out,'MOVED-WITHIN')}")
    os.unlink(p)

    # 4 — an empty world.
    p = write([])
    rc, out = run(p)
    check("an empty dump refuses rather than reporting 0%",
          rc != 0 and "no records" in out,
          f"exit {rc}: {out.strip().splitlines()[0] if out.strip() else '(silent)'}")
    os.unlink(p)

    # 4b — two ticks that share no city at all.
    p = write([prov(0, "n1", 0, 1), city(0, "Deployment demo/only", "n1", 0, 1, 3, 1),
               prov(1, "n1", 0, 1), city(1, "Deployment demo/other", "n1", 0, 1, 3, 1)])
    rc, out = run(p)
    check("no shared city means no rate, not a 0% rate",
          rc != 0 and "no rate to report" in out and "MOVED-WITHIN rate" not in out,
          f"exit {rc}")
    os.unlink(p)

    # 5 — differing city counts.
    p = write(world(0) + world(1)[:-1] + [city(1, "Deployment demo/delta", "n2", 0, 10, 9, 3)])
    rc, out = run(p)
    check("a city gained and one lost are counted, not dropped",
          rc == 0 and count(out, "ARRIVED") == 1 and count(out, "DEPARTED") == 1,
          f"ARRIVED={count(out,'ARRIVED')} DEPARTED={count(out,'DEPARTED')}")
    os.unlink(p)

    # 6 — the province moves, the city keeps its offset.
    p = write(world(0) + world(1, p0=(0, 19)))
    rc, out = run(p)
    check("a moving province CARRIES its cities rather than moving them",
          rc == 0 and count(out, "CARRIED") == 2 and count(out, "MOVED-WITHIN") == 0,
          f"CARRIED={count(out,'CARRIED')} MOVED-WITHIN={count(out,'MOVED-WITHIN')}")
    os.unlink(p)

    print()
    if FAILURES:
        print(f"FAILED: {', '.join(FAILURES)}")
        return 1
    print("all instrument tests pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
