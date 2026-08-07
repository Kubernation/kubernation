#!/usr/bin/env python3
"""Piece structure of a KuberNation layout, by SLOT ORDINAL.

Answers "is this change pool-shaped, or is it speckle?" and does it from the
persisted layout store rather than from anything that walks the model — the
v1.17.0 lesson: a measurement must not be derived by the same reasoning as the
thing it measures.

TWO DEFINITIONS OF "PIECE", and they are not comparable:

  region pieces       over all live provinces of ONE POOL
                      broken by another pool's province, and by a vacant
                      (ghost) ordinal.  This is `draw::pool_label_pieces`.

  changed-set pieces  over the provinces that CHANGED since the baseline
                      broken by ANY slot that did not change - ghosts, other
                      pools, and unchanged same-pool nodes alike.

The second is what a "the change read as a shape" claim is about. Report it as
such; it is not the same number as the region-piece figure quoted for the map's
labelling, and conflating them is the error this script exists to avoid.

GHOSTS are reported both ways, because it matters and is easy to get wrong:

  ordinal   a vacant ordinal BREAKS a run (the `pool_label_pieces` convention:
            grey reserved ground sits between the two halves on screen)
  live      vacant ordinals are collapsed out first, so a run survives a ghost

Usage:
  pieces.py [--layout PATH] [--dump POSITIONS.jsonl] [--trials N] [--seed N]

`--dump` cross-checks (node -> zone, pool, ordinal) against `--dump-positions`
output, so the numbers rest on two independent emitters rather than one.
"""

import argparse
import collections
import itertools
import json
import os
import random
import sys

# `model::DEFAULT_POOL` — an absence is not a region and is never named or
# counted as one (`pool_label_pieces` skips it; so do we, or the numbers are
# not comparable to anything else in the record).
UNPOOLED = "unpooled"


def runs_of_consecutive(ordinals):
    """Maximal runs of consecutive integers. [(first, last, len), ...]."""
    out = []
    for o in sorted(ordinals):
        if out and o == out[-1][1] + 1:
            f, _, n = out[-1]
            out[-1] = (f, o, n + 1)
        else:
            out.append((o, o, 1))
    return out


def runs_of_positions(flags):
    """Maximal runs of True in a positional sequence — ghosts already collapsed."""
    return [len(list(g)) for k, g in itertools.groupby(flags) if k]


def summarise(sizes, total):
    """Piece stats. An EMPTY set has no pieces and no largest — say so rather
    than reporting '1 piece' or '0% largest', which both read as a measurement.
    """
    if total == 0:
        return {"pieces": None, "sizes": [], "largest": None, "share": None}
    return {
        "pieces": len(sizes),
        "sizes": sorted(sizes, reverse=True),
        "largest": max(sizes),
        "share": max(sizes) / total,
    }


def fmt(s, total):
    if s["pieces"] is None:
        return "no changed slots"
    return (
        f"{s['pieces']} piece(s) {s['sizes']}  "
        f"largest {s['largest']}/{total} = {s['share']:.0%}"
    )


def load_layout(path):
    with open(path) as f:
        d = json.load(f)
    live = [s for s in d["slots"] if s.get("occupant")]
    ghosts = [s for s in d["slots"] if not s.get("occupant")]
    return live, ghosts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--layout",
        default=os.path.expanduser(
            "~/.local/state/kubernation/layouts/kwok-kubernation-churn.json"
        ),
    )
    ap.add_argument("--dump", help="a --dump-positions jsonl, to cross-check")
    ap.add_argument("--trials", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=20260807)
    a = ap.parse_args()

    live, ghosts = load_layout(a.layout)
    if not live:
        print("no live slots in the layout — nothing to measure", file=sys.stderr)
        return 2

    dropped = [s for s in live if s["pool"] == UNPOOLED]
    live = [s for s in live if s["pool"] != UNPOOLED]
    ghost_ord = collections.defaultdict(set)
    for g in ghosts:
        ghost_ord[g["zone"]].add(g["ordinal"])

    print(f"layout: {a.layout}")
    print(
        f"{len(live)} live slots ({len(dropped)} unpooled excluded), "
        f"{len(ghosts)} ghosts, seed {a.seed}, {a.trials} control trials"
    )

    if a.dump:
        by_node = {}
        with open(a.dump) as f:
            for line in f:
                r = json.loads(line)
                if r.get("kind") == "province":
                    by_node[r["node"]] = (r["zone"], r["pool"], r["ordinal"])
        checked = mismatch = 0
        for s in live:
            got = by_node.get(s["occupant"])
            if got is None:
                continue
            checked += 1
            if got != (s["zone"], s["pool"], s["ordinal"]):
                mismatch += 1
        print(
            f"cross-check vs --dump-positions: {checked} nodes compared, "
            f"{mismatch} disagreements"
            + ("  <-- INVESTIGATE" if mismatch else "  (two sources agree)")
        )
    print()

    by_zone = collections.defaultdict(list)
    for s in live:
        by_zone[s["zone"]].append(s)

    rng = random.Random(a.seed)
    fleet_changed = fleet_pieces_ord = 0
    fleet_ctrl = []
    # Region totals are EMITTED, not counted by eye from the per-zone rows.
    # Narrating "N of M" from a printed breakdown is how the previously
    # published figure went wrong: 8 regions in 12 pieces was read as "4 of 8
    # regions fragmented" when it is 3 (12 - 8 counts EXTRA pieces, not
    # regions). The instrument prints both, so neither can be inferred.
    regions_total = regions_split = region_pieces = 0
    worst_share = None

    for z in sorted(by_zone):
        rows = sorted(by_zone[z], key=lambda s: s["ordinal"])
        ords = [s["ordinal"] for s in rows]
        changed = [s["ordinal"] for s in rows if s.get("occupied_at")]
        k, n = len(changed), len(rows)
        print(f"{z}: {n} live provinces, slots {ords[0]}-{ords[-1]}, "
              f"{len(ghost_ord[z])} ghost ordinals")

        # --- region pieces (the pool_label_pieces rule) --------------------
        for pool in sorted({s["pool"] for s in rows}):
            po = [s["ordinal"] for s in rows if s["pool"] == pool]
            r = runs_of_consecutive(po)
            sizes = [n_ for _, _, n_ in r]
            st = summarise(sizes, len(po))
            print(f"    region  {pool:<10} {fmt(st, len(po))}")
            regions_total += 1
            region_pieces += st["pieces"]
            if st["pieces"] > 1:
                regions_split += 1
            worst_share = st["share"] if worst_share is None else min(worst_share, st["share"])

        # --- changed-set pieces --------------------------------------------
        if k == 0:
            print("    changed              no changed slots")
            print()
            continue

        by_ord = [n_ for _, _, n_ in runs_of_consecutive(changed)]
        chg = set(changed)
        by_live = runs_of_positions([o in chg for o in ords])
        print(f"    changed  by-ordinal  {fmt(summarise(by_ord, k), k)}")
        print(f"    changed  by-live-pos {fmt(summarise(by_live, k), k)}"
              + ("" if by_ord == by_live else "   <-- a ghost splits it"))

        # --- pool-blind control (§4) ---------------------------------------
        # Same k, placed at random among THIS zone's live ordinals: the null
        # hypothesis is "the change was not pool-shaped".
        counts, shares = [], []
        for _ in range(a.trials):
            pick = rng.sample(ords, k)
            r = [n_ for _, _, n_ in runs_of_consecutive(pick)]
            counts.append(len(r))
            shares.append(max(r) / k)
        atleast = sum(1 for c in counts if c <= len(by_ord)) / a.trials
        counts.sort()
        print(
            f"    control  {k} of {n} at random: pieces median "
            f"{counts[len(counts) // 2]} "
            f"(p05 {counts[len(counts) // 20]}, p95 {counts[-len(counts) // 20]}), "
            f"mean largest share {sum(shares) / len(shares):.0%}"
        )
        print(
            f"             P(pieces <= observed {len(by_ord)}) = {atleast:.4f}"
            + ("   <-- indistinguishable from random" if atleast > 0.05
               else "   <-- more concentrated than chance")
        )
        print()

        fleet_changed += k
        fleet_pieces_ord += len(by_ord)
        fleet_ctrl.append(atleast)

    print(f"FLEET regions: {regions_split} of {regions_total} regions are in more "
          f"than one piece; {regions_total} regions in {region_pieces} pieces total"
          + (f"; smallest largest-piece share {worst_share:.0%}" if worst_share is not None else ""))
    print(f"FLEET changed: {fleet_changed} changed slots in {fleet_pieces_ord} "
          f"by-ordinal piece(s) across {len(fleet_ctrl)} zone(s) that changed")
    if fleet_ctrl:
        print("       per-zone P(pieces <= observed): "
              + ", ".join(f"{p:.4f}" for p in fleet_ctrl))
    return 0


if __name__ == "__main__":
    sys.exit(main())
