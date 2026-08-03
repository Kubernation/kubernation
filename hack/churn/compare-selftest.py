#!/usr/bin/env python3
"""Break what `compare.py` measures and confirm it notices.

    compare-selftest.py [FRAME]        (default: the committed A2 gate frame 00)

A2's largest lesson was that four of its six instrument failures were SILENT —
the flipbook rendered identically whether or not the mechanism it existed to
validate was present. A comparator is exactly the kind of tool that fails that
way: it always emits a plausible percentage. So it gets its own tests, and they
are committed rather than run once by hand.

The four cases are §6 of the measurement-session guidance:

  1. a frame against itself           -> 100% identical, every other bucket zero
  2. two frames known to differ       -> non-zero, in the expected direction
  3. a frame shifted by a few pixels  -> a LARGE delta, not a small one
                                         (a comparator that quietly tolerates
                                          misalignment would report ~0 here)
  4. the docked column overpainted    -> the number does not move
                                         (proof the crop excludes the chrome
                                          whose counters change every frame)

Case 4 is the cheapest guard against the specific A2 failure where the column's
counters would have swamped the map.
"""

import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import compare  # noqa: E402


def to_bmp(path, w, h, rgb, out):
    """Write RGB bytes as a 32bpp top-down BMP (negative height)."""
    row = bytearray()
    for i in range(w * h):
        row += bytes((rgb[i * 3 + 2], rgb[i * 3 + 1], rgb[i * 3], 255))
    hdr = struct.pack(
        "<2sIHHIIiiHHIIiiII",
        b"BM",
        54 + len(row),
        0,
        0,
        54,
        40,
        w,
        -h,
        1,
        32,
        0,
        len(row),
        2835,
        2835,
        0,
        0,
    )
    with open(out, "wb") as f:
        f.write(hdr)
        f.write(row)
    return out


def shifted(w, h, rgb, dx):
    """The image moved `dx` px east, edge-filled. Mimics a misaligned capture."""
    out = bytearray(len(rgb))
    for y in range(h):
        base = y * w * 3
        for x in range(w):
            sx = max(0, x - dx)
            out[base + x * 3 : base + x * 3 + 3] = rgb[base + sx * 3 : base + sx * 3 + 3]
    return bytes(out)


def painted_column(w, h, rgb, right_margin):
    """The docked column overpainted solid — inside the excluded region only."""
    out = bytearray(rgb)
    x0 = w - right_margin
    for y in range(h):
        base = y * w * 3
        for x in range(x0, w):
            out[base + x * 3 : base + x * 3 + 3] = b"\xff\x00\xff"
        # And the menu bar, which `--top` also excludes.
        if y < 60:
            for x in range(w):
                out[base + x * 3 : base + x * 3 + 3] = b"\xff\x00\xff"
    return bytes(out)


def run(a, b, cls="land", right_margin=528, top=60):
    with tempfile.TemporaryDirectory() as tmp:
        fa = compare.load(a, tmp)
        fb = compare.load(b, tmp)
        return compare.compare(fa, fb, right_margin, top, compare.CLASSES[cls])


def main():
    frame = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        HERE, "..", "..", "docs", "reports", "a2-gate", "refresh-00.png"
    )
    other = os.path.join(HERE, "..", "..", "docs", "reports", "a2-gate", "refresh-13.png")
    if not os.path.exists(frame):
        raise SystemExit(f"no frame at {frame}")

    failures = []

    def check(name, cond, detail):
        mark = "ok  " if cond else "FAIL"
        print(f"{mark} {name}: {detail}")
        if not cond:
            failures.append(name)

    with tempfile.TemporaryDirectory() as tmp:
        w, h, rgb = compare.load(frame, tmp)
        base = to_bmp(frame, w, h, rgb, os.path.join(tmp, "base.bmp"))

        # 1 — a frame against itself.
        r = run(base, base)
        check(
            "self-compare is total",
            r["identical"] == r["total"] and r["lost"] == r["gained"] == r["changed"] == 0,
            f"{100.0 * r['identical'] / r['total']:.3f}% identical, "
            f"{r['lost'] + r['gained'] + r['changed']} px otherwise",
        )

        # 2 — two frames known to differ.
        if os.path.exists(other):
            r2 = run(frame, other)
            d = 100.0 * (r2["lost"] + r2["gained"]) / r2["total"]
            check(
                "known-different frames differ",
                r2["identical"] < r2["total"] and d > 0.0,
                f"{100.0 * r2['identical'] / r2['total']:.3f}% identical, class delta {d:.3f}%",
            )
        else:
            check("known-different frames differ", False, f"missing {other}")

        # 3 — a shifted frame must read as a LARGE change, not a rounding blip.
        sh = to_bmp(frame, w, h, shifted(w, h, rgb, 4), os.path.join(tmp, "shift.bmp"))
        r3 = run(base, sh)
        changed = 100.0 * (r3["total"] - r3["identical"]) / r3["total"]
        check(
            "a 4px shift is a large delta",
            changed > 10.0,
            f"{changed:.3f}% of the crop changed (a tolerant comparator would report ~0)",
        )

        # 4 — the excluded chrome must not reach the number.
        pc = to_bmp(frame, w, h, painted_column(w, h, rgb, 528), os.path.join(tmp, "col.bmp"))
        r4 = run(base, pc)
        check(
            "the crop excludes the docked column",
            r4["identical"] == r4["total"],
            f"overpainting the column and menu bar moved {r4['total'] - r4['identical']} px",
        )

    print()
    if failures:
        print(f"FAILED: {', '.join(failures)}")
        return 1
    print("all instrument tests pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
