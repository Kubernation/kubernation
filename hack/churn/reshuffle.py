#!/usr/bin/env python3
"""How many provinces would a node refresh MOVE under the pre-A2 layout?

    reshuffle.py [--old-gen g1] [--new-gen g2]

WHY THIS EXISTS. The flipbook comparator (`compare.py`) measures the RENDERED
map, and the instability A2 removes is a permutation of *which node occupies
which ground*. On a fleet of uniformly healthy nodes those are not the same
thing: permuting green provinces inside a contiguous landmass changes almost no
pixels. Measured on the real fleet, a refresh that moved 15 of zone z-a's 27
untouched provinces registered as ~1% of land area in the comparator.

So a pixel diff cannot answer "did A2 help", and this does — directly, from the
node names, with no capture and no cluster state beyond the current node list.

THE MODEL. Before A2, `build_map` ordered nodes within a zone by
`(fnv1a64(name), name)` and `build_world` stacked provinces in that order with
`y += h`. A node's RANK in that ordering is therefore its position on the map,
and any change of rank is a province that moved. After A2 the position comes
from a durable slot instead, so a rank change moves nothing.

THE MECHANISM IT REVEALS. FNV-1a mixes trailing bytes mainly into the low bits,
so names sharing a prefix share their high bits and the ordering clusters by
NAME PREFIX — which, with conventional node naming, means by pool. A rolling
refresh rewrites a generation token mid-name, which moves that pool's whole
cluster to a different place in the ordering. Whether anything else moves then
depends on where it lands: on the reference fleet the `sys` cluster hashed
`0xd8…` as `g1` and `0xfd…` as `g2`, so in z-b (clusters: burst, sys) it stayed
last and displaced nobody, while in z-a (burst, sys, edge) it jumped from the
middle to the end and pushed every `edge` province down ten slots.

That is the failure mode in one sentence: **renaming one pool's nodes moved a
different pool's provinces.**
"""

import argparse
import subprocess
import sys

CTX = "kwok-kubernation-churn"


def fnv1a64(s):
    """Byte-for-byte the `util::fnv1a64` the layout ordering used."""
    h = 0xCBF29CE484222325
    for b in s.encode():
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def nodes_by_zone(ctx):
    jp = (
        "{range .items[*]}{.metadata.name}{' '}"
        "{.metadata.labels.topology\\.kubernetes\\.io/zone}{'\\n'}{end}"
    ).replace("'", '"')
    out = subprocess.run(
        ["kubectl", "--context", ctx, "get", "nodes", "-o", "jsonpath=" + jp],
        capture_output=True,
        text=True,
    )
    if out.returncode != 0:
        raise SystemExit(f"kubectl failed: {out.stderr.strip()}")
    zones = {}
    for line in out.stdout.strip().split("\n"):
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2:
            continue
        zones.setdefault(parts[1], []).append(parts[0])
    return zones


def rank(names):
    """The pre-A2 vertical order: sort by (hash, name), same as `build_map`."""
    return {n: i for i, n in enumerate(sorted(names, key=lambda s: (fnv1a64(s), s)))}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--context", default=CTX)
    ap.add_argument("--old-gen", default="g1", help="the generation token before the refresh")
    ap.add_argument("--new-gen", default="g2", help="the generation token after it")
    args = ap.parse_args()

    zones = nodes_by_zone(args.context)
    if not zones:
        raise SystemExit("no nodes found")
    old_tok, new_tok = f"-{args.old_gen}-", f"-{args.new_gen}-"

    total_survivors = total_moved = 0
    for zone, names in sorted(zones.items()):
        if not any(new_tok in n for n in names):
            print(f"{zone}: no {args.new_gen} nodes — nothing was refreshed here")
            continue
        # Reconstruct the pre-refresh name set: the scenario replaces a node with
        # the same index under a new generation token, so the rename is exact.
        before = rank([n.replace(new_tok, old_tok) for n in names])
        after = rank(names)
        survivors = [n for n in names if new_tok not in n]
        moved = [n for n in survivors if before[n] != after[n]]
        total_survivors += len(survivors)
        total_moved += len(moved)
        pct = 100 * len(moved) // len(survivors) if survivors else 0
        print(
            f"{zone}: {len(names):3d} nodes, {len(survivors):3d} untouched by the refresh — "
            f"{len(moved):3d} ({pct:3d}%) would MOVE under the pre-A2 ordering"
        )
        if moved:
            shifts = sorted({after[n] - before[n] for n in moved})
            print(f"       rank shifts: {shifts}")

    if total_survivors:
        pct = 100 * total_moved / total_survivors
        print(
            f"\nfleet: {total_moved} of {total_survivors} untouched provinces "
            f"({pct:.0f}%) would move. Under A2 the answer is 0 by construction —"
        )
        print("       a slot belongs to the node, so renaming one pool cannot displace another.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
