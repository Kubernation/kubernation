#!/usr/bin/env python3
"""Classify what moved between two ticks of a `--dump-positions` file.

    positions.py DUMP.jsonl [--from TICK] [--to TICK] [--verbose]

WHY NOT A PIXEL DIFF. The map renders a projection, and a projection can be
stable in appearance while unstable in assignment: permuting uniformly-green
provinces inside a contiguous landmass changes almost no pixels, so the
flipbook comparator reported ~1% of land area for a layout that moved 27% of its
provinces. Placement has to be read where it is decided. It also has no coverage
problem — the dump carries every city, where no viewport holds more than about
three.

THE CLASSES, and why they are these:

  HELD          same node, same offset inside its province, province unmoved
  CARRIED       same node, same offset, but the PROVINCE itself moved.
                A2's domain, not A3's — the settlement did not move relative to
                the ground it stands on, the ground moved. Counted separately so
                it cannot be mistaken for placement instability.
  MOVED-WITHIN  same node, DIFFERENT offset inside its province.
                **This is A3's target**: nothing about the city changed, and it
                sits somewhere else on the same node.
  FOLLOWED      different node. Its pods went elsewhere and it went with them.
  ARRIVED /
  DEPARTED      present in only one tick.

**There is no MOVED-ACROSS class, and that is a finding rather than an
omission.** A city is emitted only on the province whose node is its pod
plurality (`world.rs`: `city_home` takes `max_by_key` over pods-per-node, and
the render loop skips any province that is not that node). So a city's province
IS its plurality node, by construction, and a cross-province move cannot mean
anything except that the plurality moved. The guidance's suggested extra
pod-plurality column would have been the same column twice.

Comparing OFFSETS rather than absolute cells is what makes that distinction
possible; on absolute coordinates, a province relocating would charge every
settlement it carries to A3.
"""

import argparse
import json
import sys
from collections import defaultdict


def load(path):
    """-> {tick: {"cities": {ref: rec}, "provinces": {node: rec}}}"""
    ticks = defaultdict(lambda: {"cities": {}, "provinces": {}})
    with open(path) as f:
        for line_no, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                print(f"{path}:{line_no}: not JSON — skipped", file=sys.stderr)
                continue
            t = ticks[r["tick"]]
            if r["kind"] == "city":
                t["cities"][r["workload"]] = r
            elif r["kind"] == "province":
                t["provinces"][r["node"]] = r
    return ticks


def classify(a, b, prov_a, prov_b):
    """One city's fate between two ticks."""
    if a["node"] != b["node"]:
        return "FOLLOWED"
    if (a["ox"], a["oy"]) != (b["ox"], b["oy"]):
        return "MOVED-WITHIN"
    pa, pb = prov_a.get(a["node"]), prov_b.get(b["node"])
    if pa and pb and (pa["x"], pa["y"]) != (pb["x"], pb["y"]):
        return "CARRIED"
    return "HELD"


def compare(ticks, t0, t1, verbose=False):
    a, b = ticks[t0], ticks[t1]
    common = sorted(set(a["cities"]) & set(b["cities"]))
    arrived = sorted(set(b["cities"]) - set(a["cities"]))
    departed = sorted(set(a["cities"]) - set(b["cities"]))

    counts = defaultdict(int)
    rows = []
    for ref in common:
        cls = classify(a["cities"][ref], b["cities"][ref], a["provinces"], b["provinces"])
        counts[cls] += 1
        if cls != "HELD":
            rows.append((cls, ref, a["cities"][ref], b["cities"][ref]))

    print(f"ticks {t0} -> {t1}")
    print(f"cities: {len(a['cities'])} -> {len(b['cities'])}   provinces: "
          f"{len(a['provinces'])} -> {len(b['provinces'])}")
    print()
    for cls in ("HELD", "CARRIED", "MOVED-WITHIN", "FOLLOWED"):
        print(f"  {cls:<13} {counts[cls]:4d}")
    print(f"  {'ARRIVED':<13} {len(arrived):4d}")
    print(f"  {'DEPARTED':<13} {len(departed):4d}")

    # An empty intersection is a real input — two ticks that share no city at
    # all. Say so rather than dividing by zero or printing a confident 0%.
    if not common:
        print("\nno city is present in BOTH ticks — there is no rate to report")
        return 2
    # The denominator is stated because the last session's headline metric
    # inverted for want of one: a per-class delta had been divided by the whole
    # map's area rather than by the class's own.
    rate = 100.0 * counts["MOVED-WITHIN"] / len(common)
    print(f"\nMOVED-WITHIN rate  {rate:.1f}%   ({counts['MOVED-WITHIN']} of "
          f"{len(common)} cities present in BOTH ticks)")
    print("  MOVED-WITHIN is A3's target: the city moved on ground that did not.")

    if verbose or rows:
        for cls, ref, x, y in rows:
            if cls == "FOLLOWED":
                print(f"  {cls:<13} {ref}  {x['node']} -> {y['node']}")
            else:
                print(f"  {cls:<13} {ref}  on {x['node']}  "
                      f"offset ({x['ox']},{x['oy']}) -> ({y['ox']},{y['oy']})")
        for ref in arrived:
            print(f"  {'ARRIVED':<13} {ref}  on {b['cities'][ref]['node']}")
        for ref in departed:
            print(f"  {'DEPARTED':<13} {ref}  was on {a['cities'][ref]['node']}")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dump")
    ap.add_argument("--from", dest="t0", type=int, default=None)
    ap.add_argument("--to", dest="t1", type=int, default=None)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    ticks = load(args.dump)
    if not ticks:
        print(f"{args.dump}: no records — nothing to compare", file=sys.stderr)
        return 2
    order = sorted(ticks)
    if len(order) < 2 and (args.t0 is None or args.t1 is None):
        print(f"{args.dump}: only tick {order[0]} present — a comparison needs two",
              file=sys.stderr)
        return 2
    t0 = order[0] if args.t0 is None else args.t0
    t1 = order[-1] if args.t1 is None else args.t1
    for t in (t0, t1):
        if t not in ticks:
            print(f"{args.dump}: no tick {t} (have {order[0]}..{order[-1]})", file=sys.stderr)
            return 2
    return compare(ticks, t0, t1, args.verbose)


if __name__ == "__main__":
    sys.exit(main())
