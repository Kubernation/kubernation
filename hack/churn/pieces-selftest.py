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

print("\n7. failure classification — the Failing/terminal trap")


def pod(ns, name, node, phase, ready=True, owner=None, kind="ReplicaSet"):
    m = {"namespace": ns, "name": name}
    if owner:
        m["ownerReferences"] = [{"kind": kind, "name": owner}]
    return {
        "metadata": m,
        "spec": {"nodeName": node},
        "status": {"phase": phase, "containerStatuses": [{"ready": ready}]},
    }


# A terminal Failed pod and a live CrashLoopBackOff pod are BOTH PodState::Failing
# in the app. They must not land in the same bucket here.
dead = P.classify_pod(pod("d", "a", "n1", "Failed"))
looping = P.classify_pod(pod("d", "b", "n1", "Running", ready=False))
check("a terminal Failed pod is 'failed'", dead[0] == "failed", dead[0])
check("a live not-ready pod is 'unhealthy'", looping[0] == "unhealthy", looping[0])
check("they are NOT the same bucket", dead[0] != looping[0])
check("Succeeded is its own bucket",
      P.classify_pod(pod("d", "c", "n1", "Succeeded"))[0] == "succeeded")
check("Running+ready is healthy",
      P.classify_pod(pod("d", "e", "n1", "Running"))[0] == "healthy")
check("Pending is unhealthy",
      P.classify_pod(pod("d", "f", "n1", "Pending", ready=False))[0] == "unhealthy")
# A ReplicaSet owner folds to its Deployment, which is the name an operator uses.
check("RS owner folds to the deployment",
      P.classify_pod(pod("d", "g", "n1", "Running", owner="web-7f9c"))[2] == "d/web",
      str(P.classify_pod(pod("d", "g", "n1", "Running", owner="web-7f9c"))[2]))
check("a non-RS owner is kept whole",
      P.classify_pod(pod("d", "h", "n1", "Running", owner="db", kind="StatefulSet"))[2] == "d/db")
check("an ownerless pod has no workload",
      P.classify_pod(pod("d", "i", "n1", "Running"))[2] is None)

print("\n8. group stats fabricate nothing, and count what they claim")
g = P.group_stats([])
check("empty: groups is None, not 1", g["groups"] is None, repr(g["groups"]))
check("empty: share is None, not 0.0", g["share"] is None, repr(g["share"]))
check("empty prints as words", P.fmt_groups(g, 0) == "no failures", P.fmt_groups(g, 0))
g2 = P.group_stats(["w", "w", "w"])
check("all one group", (g2["groups"], g2["largest"], g2["share"]) == (1, 3, 1.0), str(g2))
g3 = P.group_stats(["a", "a", "b"])
check("two groups, largest 2/3", (g3["groups"], g3["largest"]) == (2, 2), str(g3))
check("share is of the FAILING set, not the fleet", abs(g3["share"] - 2 / 3) < 1e-9)

print("\n9. end to end: a workload-shaped failure vs a node-shaped one")
with tempfile.TemporaryDirectory() as td:
    dump = os.path.join(td, "d.jsonl")
    with open(dump, "w") as f:
        for i, (n, z) in enumerate([("n1", "z-a"), ("n2", "z-b"), ("n3", "z-c")]):
            f.write(json.dumps({"tick": 0, "kind": "province", "node": n, "zone": z,
                                "pool": "p", "ordinal": i}) + "\n")

    def run(items, label):
        pj = os.path.join(td, f"{label}.json")
        json.dump({"items": items}, open(pj, "w"))
        return subprocess.run(
            [sys.executable, os.path.join(HERE, "pieces.py"), "--failures", pj,
             "--dump", dump, "--trials", "300", "--label", label],
            capture_output=True, text=True).stdout

    # WORKLOAD-shaped: every failure is one deployment, spread over all 3 nodes.
    wl = [pod("d", f"bad-{i}", f"n{i % 3 + 1}", "Running", ready=False, owner="bad-1")
          for i in range(9)]
    wl += [pod("d", f"ok-{i}", f"n{i % 3 + 1}", "Running", owner="good-1") for i in range(9)]
    out = run(wl, "workload-shaped")
    check("workload-shaped: 1 workload group", "workload  1 group(s)" in out, out)
    check("workload-shaped: spans all 3 nodes", "node      3 group(s)" in out, out)

    # NODE-shaped: every failure on one node, spread over 3 deployments.
    nd = [pod("d", f"bad-{i}", "n2", "Running", ready=False, owner=f"w{i % 3}-1")
          for i in range(9)]
    nd += [pod("d", f"ok-{i}", f"n{i % 3 + 1}", "Running", owner=f"w{i % 3}-1") for i in range(9)]
    out = run(nd, "node-shaped")
    check("node-shaped: 1 node group", "node      1 group(s)" in out, out)
    check("node-shaped: spans 3 workloads", "workload  3 group(s)" in out, out)
    check("node-shaped: the control finds it beyond chance",
          "node" in out and "clusters beyond chance" in out, out)

    # A dimension with one possible value is DEGENERATE, not a 100% cluster.
    check("a single-valued dimension is refused, not reported as clustering",
          "DEGENERATE" in out, out)


    # THE FABRICATION GUARD, found by real data: an unschedulable pod has no
    # node, so it has no zone and no pool either. Deriving a shared "?" made 9
    # such pods report "zone: 1 group, 100%, P=0.0000" — a measurement of the
    # placeholder, not the fleet.
    pend = [pod("d", f"p-{i}", None, "Pending", ready=False, owner="stuck-1") for i in range(9)]
    pend += [pod("d", f"ok-{i}", f"n{i % 3 + 1}", "Running", owner="good-1") for i in range(9)]
    out = run(pend, "unschedulable")
    check("an unschedulable pod is not attributable to a node",
          "node      not attributable (9/9" in out, out)
    check("and NOT to a zone either", "zone      not attributable (9/9" in out, out)
    check("nor a pool", "pool      not attributable (9/9" in out, out)
    check("the workload is still attributable", "workload  1 group(s)" in out, out)
    check("and the map says it cannot place them",
          "no failing pod is on a node the map places" in out, out)

    # No failures at all says so.
    out = run([pod("d", "ok", "n1", "Running")], "quiet")
    check("no failures says so in words", "no live unhealthy pods" in out, out)

print()
if fails:
    print(f"{len(fails)} FAILED: {fails}")
    sys.exit(1)
print("all self-tests passed")
