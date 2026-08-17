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


# ---------------------------------------------------------------- failures --
#
# T2-pre: which DIMENSION do failures cluster in? Two different statistics, and
# conflating them is the error T1 §3.1 already paid for once:
#
#   groups      how many distinct values of a dimension the failures span, and
#               what share the largest holds. A CATEGORICAL statement: "they are
#               all in one workload".
#   contiguity  runs of consecutive SLOT ORDINALS among the affected nodes. A
#               SPATIAL statement: "the map draws them as a run".
#
# Only the second is T2's claim. A pool can cluster perfectly and still scatter
# on the map, which is exactly what T1 found for `sys`.


def classify_pod(pod):
    """(bucket, node, workload) for one kubectl pod object.

    Buckets mirror the app's own vocabulary rather than inventing one:

      succeeded  phase Succeeded          — terminal, history
      failed     phase Failed             — terminal
      unhealthy  live, and not Running-with-every-container-ready
      healthy    everything else

    `PodState::Failing` is deliberately NOT the filter: its own doc says it
    covers a terminal `Failed` pod AND a live CrashLoopBackOff one, and those
    cluster differently — the first follows whatever ran there, the second
    follows a live cause. T2 is about live trouble, so the analysis runs on
    `unhealthy` and the terminal counts are reported beside it.
    """
    st = pod.get("status") or {}
    phase = st.get("phase") or "Unknown"
    node = (pod.get("spec") or {}).get("nodeName")
    owner = None
    for r in (pod.get("metadata") or {}).get("ownerReferences") or []:
        owner = f"{(pod['metadata'].get('namespace') or '')}/{r.get('name')}"
        # A ReplicaSet name is <deploy>-<hash>; fold to the Deployment so the
        # workload dimension is the thing an operator would name.
        if r.get("kind") == "ReplicaSet" and "-" in (r.get("name") or ""):
            owner = f"{pod['metadata'].get('namespace')}/{r['name'].rsplit('-', 1)[0]}"
        break
    if phase == "Succeeded":
        return "succeeded", node, owner
    if phase == "Failed":
        return "failed", node, owner
    ready = phase == "Running" and all(
        c.get("ready") for c in (st.get("containerStatuses") or [{"ready": False}])
    )
    return ("healthy" if ready else "unhealthy"), node, owner


def group_stats(values):
    """Categorical clustering: distinct groups and the largest group's share.

    `None` for an empty set — no groups, no largest. An empty set does not have
    "1 group" or "0% largest"; both read as measurements.
    """
    if not values:
        return {"groups": None, "largest": None, "share": None, "sizes": []}
    c = collections.Counter(values)
    sizes = sorted(c.values(), reverse=True)
    return {
        "groups": len(c),
        "largest": sizes[0],
        "share": sizes[0] / len(values),
        "sizes": sizes,
    }


def fmt_groups(g, total):
    if g["groups"] is None:
        return "no failures"
    return (
        f"{g['groups']} group(s) {g['sizes']}  "
        f"largest {g['largest']}/{total} = {g['share']:.0%}"
    )


def failure_report(pods_path, dump_path, trials, seed, label):
    """T2-pre: which dimension do failures cluster in?"""
    with open(pods_path) as f:
        pods = json.load(f)["items"]

    # node -> (zone, pool, ordinal), from the app's own dump. An INDEPENDENT
    # emitter from kubectl, which is where the pod data comes from: the join
    # rests on two sources rather than one walk.
    site = {}
    with open(dump_path) as f:
        for line in f:
            r = json.loads(line)
            if r.get("kind") == "province":
                site[r["node"]] = (r["zone"], r["pool"], r["ordinal"])

    buckets = collections.Counter()
    rows = []
    for p in pods:
        b, node, owner = classify_pod(p)
        buckets[b] += 1
        rows.append((b, node, owner))

    total = len(pods)
    print(f"=== {label} ===")
    print(f"  {total} pods: " + ", ".join(f"{k}={v}" for k, v in sorted(buckets.items())))
    # ASSERT the total against the population rather than eyeballing it. The
    # last three arithmetic errors in this record were narrated distributions
    # that did not add up.
    assert sum(buckets.values()) == total, f"buckets {sum(buckets.values())} != {total} pods"
    print(f"  terminal: succeeded={buckets['succeeded']} failed={buckets['failed']}"
          "   (reported, not analysed — see classify_pod)")

    unhealthy = [(n, w) for b, n, w in rows if b == "unhealthy"]
    if not unhealthy:
        print("  no live unhealthy pods — nothing to cluster\n")
        return
    k = len(unhealthy)
    print(f"  live unhealthy: {k}\n")

    # The population the control shuffles over: every pod that COULD have been
    # the unhealthy one. Anything else asks a different question.
    pool_rows = [(n, w) for b, n, w in rows if b != "succeeded"]
    # A pod with no node has no zone and no pool — NOT a shared "?" group.
    # Fabricating one made 9 unschedulable pods report "zone: 1 group, 100%,
    # P=0.0000", which is a measurement of the placeholder rather than of the
    # fleet. Missing input yields None, and None is excluded from the statistic
    # and counted separately.
    def zone_of(nw):
        return site[nw[0]][0] if nw[0] in site else None

    def pool_of(nw):
        return site[nw[0]][1] if nw[0] in site else None

    dims = {
        "node": (lambda nw: nw[0] if nw[0] in site else None, None),
        "zone": (zone_of, None),
        "pool": (pool_of, None),
        "workload": (lambda nw: nw[1], None),
    }

    rng = random.Random(seed)
    idx = list(range(len(pool_rows)))
    for name, (key, _) in dims.items():
        obs = [key(nw) for nw in unhealthy]
        known = [v for v in obs if v is not None]
        missing = len(obs) - len(known)
        if not known:
            print(f"  {name:<9} not attributable ({missing}/{k} failing pods carry no value)")
            continue
        note = f"  [{missing}/{k} not attributable]" if missing else ""
        g = group_stats(known)
        universe = [key(nw) for nw in pool_rows]
        distinct_universe = len({u for u in universe if u is not None})
        # A dimension with ONE possible value cannot cluster; saying "1 group,
        # 100%" there would be a measurement of the fixture, not the fleet.
        if distinct_universe <= 1:
            print(f"  {name:<9} {fmt_groups(g, len(known))}{note}   <-- DEGENERATE: the fleet "
                  f"has {distinct_universe} distinct value(s); not measurable here")
            continue
        # The control draws the SAME NUMBER OF ATTRIBUTABLE pods, so observed and
        # chance are counting the same thing.
        attributable = [i for i in idx if key(pool_rows[i]) is not None]
        if len(attributable) < len(known):
            print(f"  {name:<9} {fmt_groups(g, len(known))}{note}   <-- fewer attributable "
                  "pods in the population than in the failing set; control skipped")
            continue
        hits = 0
        for _ in range(trials):
            pick = rng.sample(attributable, len(known))
            gs = group_stats([key(pool_rows[i]) for i in pick])
            if gs["groups"] is not None and gs["groups"] <= g["groups"]:
                hits += 1
        p_val = hits / trials
        verdict = ("clusters beyond chance" if p_val <= 0.05 else "indistinguishable from chance")
        print(f"  {name:<9} {fmt_groups(g, len(known))}{note}   universe={distinct_universe}  "
              f"P(groups<=obs)={p_val:.4f}  <-- {verdict}")

    # §2.3 — grouping is not shape. Contiguity of the AFFECTED NODES by slot
    # ordinal, per zone, using T1's changed-set definition.
    print()
    aff = collections.defaultdict(set)
    for n, _ in unhealthy:
        if n in site:
            z, _, o = site[n]
            aff[z].add(o)
    if not aff:
        print("  contiguity: no failing pod is on a node the map places")
    for z in sorted(aff):
        r = runs_of_consecutive(aff[z])
        sizes = [n_ for _, _, n_ in r]
        print(f"  contiguity {z}: {fmt(summarise(sizes, len(aff[z])), len(aff[z]))} "
              f"over {len(aff[z])} affected node(s)")
    print()


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
    ap.add_argument("--failures", help="a `kubectl get pods -A -o json` file (T2-pre mode)")
    ap.add_argument("--label", default="failures", help="what shape this capture is")
    ap.add_argument("--trials", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=20260807)
    a = ap.parse_args()

    if a.failures:
        if not a.dump:
            print("--failures needs --dump for the node -> zone/pool/ordinal join",
                  file=sys.stderr)
            return 2
        failure_report(a.failures, a.dump, a.trials, a.seed, a.label)
        return 0

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
