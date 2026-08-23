#!/usr/bin/env python3
"""Judge the D2 selection gates from two positional dumps.

A measurement script always emits a plausible-looking number. Each gate here
therefore carries a DISCRIMINATION CHECK: it refuses to pass unless the world
actually changed in the way the gate is about, and it reports what the cell a
pre-inversion selection would still hold now points at.
"""
import json
import sys


def load(path):
    return [json.loads(l) for l in open(path) if l.strip()]


def by_tick(recs, kind):
    out = {}
    for r in recs:
        if r["kind"] == kind:
            out.setdefault(r["tick"], []).append(r)
    return out


def province_at(recs, tick, cell):
    for r in recs:
        if (r["tick"] == tick and r["kind"] == "province"
                and r["x"] <= cell[0] < r["x"] + r["w"]
                and r["y"] <= cell[1] < r["y"] + r["h"]):
            return r
    return None


fails = []


def check(name, cond, detail=""):
    print(f"  {'ok  ' if cond else 'FAIL'}  {name}" + (f"   [{detail}]" if detail else ""))
    if not cond:
        fails.append(name)


def gate_a(path):
    print("GATE A — a selected workload's city is rescheduled to another zone")
    recs = load(path)
    sels, cities = by_tick(recs, "selection"), by_tick(recs, "city")
    rows = []
    for t in sorted(sels):
        s = sels[t][0]
        c = next((c for c in cities.get(t, []) if "wanderer" in c["workload"]), None)
        rows.append((t, s, c))
    if not rows:
        check("a selection was recorded at all", False)
        return

    seen = {(c["x"], c["y"]) for _, _, c in rows if c}
    nodes = sorted({c["node"] for _, _, c in rows if c})
    # DISCRIMINATION: if the city never moved there is nothing to survive.
    check("the city actually moved", len(seen) > 1, f"{len(seen)} distinct positions, nodes {nodes}")
    check("the selection never lost its place", all(s["placed"] for _, s, _ in rows))
    check("the selection is AT the city's current position at every tick",
          all((s["x"], s["y"]) == (c["x"], c["y"]) for _, s, c in rows if c))

    old = (rows[0][2]["x"], rows[0][2]["y"])
    last = rows[-1][0]
    p = province_at(recs, last, old)
    now = f"province {p['node']} (zone {p['zone']})" if p else "nothing"
    cur = (rows[-1][1]["x"], rows[-1][1]["y"])
    check("a stored cell would now name something else",
          p is not None and (p["node"] != rows[-1][2]["node"]),
          f"stale cell {old} -> {now}; the identity resolves to {cur}")


def gate_b(path):
    print("\nGATE B — the hot world gains a zone while a WARM city is selected")
    recs = load(path)
    sels = by_tick(recs, "selection")
    rows = [(t, sels[t][0]) for t in sorted(sels)]
    if not rows:
        check("a selection was recorded at all", False)
        return

    def extent(t):
        ps = [r for r in recs if r["tick"] == t and r["kind"] == "province"]
        return max((p["x"] + p["w"] for p in ps), default=0), sorted({p["zone"] for p in ps})

    w0, z0 = extent(rows[0][0])
    w1, z1 = extent(rows[-1][0])
    # DISCRIMINATION: if the hot world did not widen, the offset never moved.
    check("the hot world actually grew a zone", w1 > w0 and len(z1) > len(z0),
          f"extent {w0}->{w1}, zones {z0}->{z1}")
    check("the selection is in the WARM cluster", all(s["cluster"] == "Warm" for _, s in rows))
    check("the selection never lost its place", all(s["placed"] for _, s in rows))

    x0, y0 = rows[0][1]["x"], rows[0][1]["y"]
    x1, y1 = rows[-1][1]["x"], rows[-1][1]["y"]
    check("it moved by exactly the hot world's growth", x1 - x0 == w1 - w0,
          f"selection dx={x1 - x0}, hot growth={w1 - w0}")
    check("only the offset moved, not the row", y0 == y1)

    p = province_at(recs, rows[-1][0], (x0, y0))
    now = f"HOT province {p['node']} (zone {p['zone']})" if p else "no hot province"
    check("a stored warm cell would now fall inside the HOT world", p is not None,
          f"stale cell {(x0, y0)} -> {now}")


gate_a(sys.argv[1])
gate_b(sys.argv[2])
print()
if fails:
    print(f"{len(fails)} FAILED: {fails}")
    sys.exit(1)
print("both gates pass, with discrimination")
