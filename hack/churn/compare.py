#!/usr/bin/env python3
"""Compare two churn-flipbook frames and report how much of the map changed.

    compare.py FRAME_A FRAME_B [--class land|settlement] [--blobs]

THE METHOD, so a number produced here means the same thing next phase:

  * **Exact match, no tolerance.** Two pixels are the same or they are not.
    That is what makes "pixel-identical" mean anything. Do not add a threshold.
  * **Play area only.** The docked right column carries counters that change
    every frame and would swamp the map; the menu bar likewise. Both are cropped
    out by default (`--right-margin`, `--top`), and §6's fourth instrument test
    confirms the crop actually excludes them.
  * **A selectable class.** The comparison is always "did this pixel change",
    but the two derived buckets — gained and lost — are relative to a CLASS
    predicate, and different questions need different ones:
      - `land`       green > blue. Covers terrain, sand and ghost grey, but not
                     sea. Answers "did the land/sea silhouette move?"
      - `settlement` the exact parchment `POP_CALM` (0.88, 0.83, 0.66) that the
                     GUI uses in exactly two places, both of them a settlement's
                     name banner or its population chip. Answers "did the cities
                     move?"
    Verified on the committed A2 frames: settlement pixels form one compact
    region and neither CRIT nor WARN appears anywhere in the crop, so no
    severity-tinted chip and no attention chrome is being counted. Note the
    name banner is POP_CALM whatever the severity, so a flagged city loses its
    chip from this count but not its banner — the class undercounts such a city
    rather than dropping it.

  * **Four buckets**, matching what the A2 gate reported so figures stay
    comparable across phases: identical, class-lost (was, is not), class-gained
    (is, was not), and changed-in-place (changed but the class did not).

Colour note: the GUI's f32 colours reach the framebuffer by TRUNCATION, not
rounding — `0.83 * 255` is 211, not 212. Getting that backwards silently matches
nothing at all, which is how this was first written and why the constants below
go through `_c8`.

Input is PNG. Decoding uses `sips` (macOS) or ImageMagick when present, and
falls back to a pure-Python decoder that is correct but slow.
"""

import argparse
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib


def _c8(r, g, b):
    """A GUI f32 colour as the framebuffer stores it — truncated, not rounded."""
    return (int(r * 255), int(g * 255), int(b * 255))


POP_CALM = _c8(0.88, 0.83, 0.66)


# --- decoding ---------------------------------------------------------------


def _via_tool(path, tmpdir):
    """PNG -> BMP with whatever the host has. Returns a path or None."""
    out = os.path.join(tmpdir, os.path.basename(path) + ".bmp")
    if shutil.which("sips"):
        cmd = ["sips", "-s", "format", "bmp", path, "--out", out]
    elif shutil.which("magick"):
        cmd = ["magick", path, "BMP3:" + out]
    elif shutil.which("convert"):
        cmd = ["convert", path, "BMP3:" + out]
    else:
        return None
    if subprocess.run(cmd, capture_output=True).returncode != 0:
        return None
    return out if os.path.exists(out) else None


def _read_bmp(path):
    d = open(path, "rb").read()
    off = struct.unpack("<I", d[10:14])[0]
    w, h = struct.unpack("<ii", d[18:26])
    bpp = struct.unpack("<H", d[28:30])[0]
    if bpp != 32:
        raise SystemExit(f"{path}: expected 32bpp BMP, got {bpp}")
    top_down = h < 0
    h = abs(h)
    px = d[off : off + w * h * 4]
    # BGRA -> RGB triples, top-down.
    rows = [px[y * w * 4 : (y + 1) * w * 4] for y in range(h)]
    if not top_down:
        rows.reverse()
    return w, h, b"".join(rows)


def _decode_png(path):
    """Pure-Python PNG -> BGRA bytes. Correct, slow; the portable fallback."""
    d = open(path, "rb").read()
    pos, w, h, idat = 8, None, None, []
    while pos < len(d):
        ln = struct.unpack(">I", d[pos : pos + 4])[0]
        typ = d[pos + 4 : pos + 8]
        if typ == b"IHDR":
            w, h, depth, ctype = struct.unpack(">IIBB", d[pos + 8 : pos + 18])
            if depth != 8 or ctype not in (2, 6):
                raise SystemExit(f"{path}: only 8-bit RGB/RGBA PNG is supported")
            ch = 3 if ctype == 2 else 4
        elif typ == b"IDAT":
            idat.append(d[pos + 8 : pos + 8 + ln])
        pos += 12 + ln
    raw = zlib.decompress(b"".join(idat))
    stride = w * ch
    out = bytearray()
    prev = bytearray(stride)
    i = 0
    for _ in range(h):
        f = raw[i]
        i += 1
        line = bytearray(raw[i : i + stride])
        i += stride
        if f == 1:
            for x in range(ch, stride):
                line[x] = (line[x] + line[x - ch]) & 255
        elif f == 2:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif f == 3:
            for x in range(stride):
                a = line[x - ch] if x >= ch else 0
                line[x] = (line[x] + ((a + prev[x]) >> 1)) & 255
        elif f == 4:
            for x in range(stride):
                a = line[x - ch] if x >= ch else 0
                c = prev[x - ch] if x >= ch else 0
                b = prev[x]
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out += line
        prev = line
    if ch == 3:
        return w, h, bytes(out)
    # drop alpha
    return w, h, bytes(b"".join(out[o : o + 3] for o in range(0, len(out), 4)))


def load(path, tmpdir):
    """-> (w, h, rgb_bytes) with three bytes per pixel, top-down."""
    with open(path, "rb") as f:
        magic = f.read(2)
    if magic == b"BM":
        # Accepted directly so the self-tests can synthesize their inputs
        # without an imaging library.
        w, h, bgra = _read_bmp(path)
        rgb = bytearray(w * h * 3)
        for i in range(w * h):
            rgb[i * 3] = bgra[i * 4 + 2]
            rgb[i * 3 + 1] = bgra[i * 4 + 1]
            rgb[i * 3 + 2] = bgra[i * 4]
        return w, h, bytes(rgb)
    bmp = _via_tool(path, tmpdir)
    if bmp:
        w, h, bgra = _read_bmp(bmp)
        rgb = bytearray(w * h * 3)
        for i in range(w * h):
            rgb[i * 3] = bgra[i * 4 + 2]
            rgb[i * 3 + 1] = bgra[i * 4 + 1]
            rgb[i * 3 + 2] = bgra[i * 4]
        return w, h, bytes(rgb)
    print(f"note: no sips/magick found — decoding {path} in Python (slow)", file=sys.stderr)
    return _decode_png(path)


# --- classes ----------------------------------------------------------------


# A slot's ground its node is too small to fill (`theme::reserved_land_pair`).
#
# The renderer does NOT paint the pair flat: `land_diamond` picks a shade by
# checkerboard and then adds `theme::cell_jitter` as (d, 1.3d, d). So the class
# is 2 shades x 5 jitter values = 10 exact colours, ENUMERATED from the same
# constants rather than sampled from a frame or approximated by a range —
# sampling would drift the moment the palette moved, and a range would start
# catching ghost grey.
#
# Written out because it is the discrimination trap of §5.2 one level down: the
# first version of this classifier listed only the two base shades and caught
# about a fifth of the ground it was measuring.
_RESERVED_BASE = ((0.39, 0.44, 0.46), (0.42, 0.47, 0.49))
_JITTER = (-0.030, -0.012, 0.0, 0.018, 0.034)  # theme::cell_jitter
RESERVED = frozenset(
    _c8(min(max(r + d, 0.0), 1.0), min(max(g + d * 1.3, 0.0), 1.0), min(max(b + d, 0.0), 1.0))
    for (r, g, b) in _RESERVED_BASE
    for d in _JITTER
)


def is_reserved(r, g, b):
    """Reserved in-slot ground — its own class, and it has to be.

    `is_land` is `g > b`, and reserved ground is a cool grey whose blue EXCEEDS
    its green, so without this it counts as sea and a change that paints it
    would appear to do nothing. That is the discrimination trap §5.2 names: the
    metric must be able to see the thing before it is used to judge it.
    """
    return (r, g, b) in RESERVED


def is_land(r, g, b):
    """Terrain, sand and ghost grey, but not sea and not reserved ground."""
    return g > b and not is_reserved(r, g, b)


def is_ground(r, g, b):
    """Anything the layout has spoken for: land, ghost or reserved."""
    return is_land(r, g, b) or is_reserved(r, g, b)


def is_settlement(r, g, b):
    return (r, g, b) == POP_CALM


CLASSES = {
    "land": is_land,
    "reserved": is_reserved,
    "ground": is_ground,
    "settlement": is_settlement,
}


# --- comparison -------------------------------------------------------------


def crop_rows(w, h, px, right_margin, top):
    """Row slices of the play area, as (offset, length) byte ranges."""
    keep = w - right_margin
    if keep <= 0 or top >= h:
        return []
    return [((y * w) * 3, keep * 3) for y in range(top, h)]


def compare(a, b, right_margin, top, cls):
    (wa, ha, pa), (wb, hb, pb) = a, b
    # A summing step is about to precede a comparing step (standing question 1):
    # totals from two differently-sized frames are not comparable, and dividing
    # by the wrong area would silently rescale every figure. Refuse instead.
    if (wa, ha) != (wb, hb):
        raise SystemExit(f"frames differ in size: {wa}x{ha} vs {wb}x{hb} — not comparable")
    rows = crop_rows(wa, ha, pa, right_margin, top)
    total = sum(ln for _, ln in rows) // 3
    # An empty crop is a real input (a bad margin, a tiny frame). Express it as
    # unknown rather than dividing by zero or reporting a confident 0%
    # (standing question 2).
    if total == 0:
        return None
    same = lost = gained = moved = foot_a = foot_b = 0
    for off, ln in rows:
        for o in range(off, off + ln, 3):
            ca = (pa[o], pa[o + 1], pa[o + 2])
            cb = (pb[o], pb[o + 1], pb[o + 2])
            ia, ib = cls(*ca), cls(*cb)
            foot_a += ia
            foot_b += ib
            if ca == cb:
                same += 1
                continue
            if ia and not ib:
                lost += 1
            elif ib and not ia:
                gained += 1
            else:
                moved += 1
    return {
        "total": total,
        "identical": same,
        "lost": lost,
        "gained": gained,
        "changed": moved,
        # The class's own area in each frame. Reported because a delta given
        # only as a share of MAP area is not comparable between classes of very
        # different size: land covers ~30% of this map and settlements ~0.14%,
        # so the same map-area percentage means two wildly different things.
        "footprint_a": foot_a,
        "footprint_b": foot_b,
    }


def blobs(w, h, px, right_margin, top, cls, gap=6):
    """Bounding boxes of class runs, merged when within `gap` px.

    Coarse on purpose: it answers "where are the settlements" well enough to
    pair them across frames, and nothing finer is claimed.
    """
    boxes = []
    keep = w - right_margin
    for y in range(top, h):
        base = (y * w) * 3
        x = 0
        while x < keep:
            o = base + x * 3
            if not cls(px[o], px[o + 1], px[o + 2]):
                x += 1
                continue
            x0 = x
            while x < keep:
                o = base + x * 3
                if not cls(px[o], px[o + 1], px[o + 2]):
                    break
                x += 1
            run = (x0, x - 1, y)
            for bx in boxes:
                if (
                    run[0] <= bx[1] + gap
                    and run[1] >= bx[0] - gap
                    and run[2] <= bx[3] + gap
                    and run[2] >= bx[2] - gap
                ):
                    bx[0] = min(bx[0], run[0])
                    bx[1] = max(bx[1], run[1])
                    bx[2] = min(bx[2], run[2])
                    bx[3] = max(bx[3], run[2])
                    break
            else:
                boxes.append([run[0], run[1], run[2], run[2]])
    # One merge pass: runs seen before a neighbour existed can leave two boxes.
    merged = True
    while merged:
        merged = False
        for i in range(len(boxes)):
            for j in range(len(boxes) - 1, i, -1):
                p, q = boxes[i], boxes[j]
                if (
                    p[0] <= q[1] + gap
                    and p[1] >= q[0] - gap
                    and p[2] <= q[3] + gap
                    and p[3] >= q[2] - gap
                ):
                    p[0], p[1] = min(p[0], q[0]), max(p[1], q[1])
                    p[2], p[3] = min(p[2], q[2]), max(p[3], q[3])
                    del boxes[j]
                    merged = True
    return boxes


def land_components(w, h, px, right_margin, top, step=4):
    """Label contiguous land regions, at 1/`step` resolution.

    Provinces are separated by open sea on this map, so a contiguous land region
    IS a province for the purpose of asking whether a settlement moved WITHIN
    one or ACROSS two. Settlement pixels are excluded from the mask: a name
    banner overhanging the shore could otherwise bridge two provinces across a
    narrow strait and turn an inter-province move into a false intra one.

    Coarse on purpose — it answers a yes/no question about province-scale blobs.
    """
    cw = (w - right_margin) // step
    ch = (h - top) // step
    mask = bytearray(cw * ch)
    for cy in range(ch):
        y = top + cy * step
        base = (y * w) * 3
        for cx in range(cw):
            o = base + (cx * step) * 3
            c = (px[o], px[o + 1], px[o + 2])
            if is_land(*c) and c != POP_CALM:
                mask[cy * cw + cx] = 1
    label = [0] * (cw * ch)
    nxt = 0
    for i in range(cw * ch):
        if not mask[i] or label[i]:
            continue
        nxt += 1
        stack = [i]
        label[i] = nxt
        while stack:
            j = stack.pop()
            jy, jx = divmod(j, cw)
            for ny, nx in ((jy - 1, jx), (jy + 1, jx), (jy, jx - 1), (jy, jx + 1)):
                if 0 <= ny < ch and 0 <= nx < cw:
                    k = ny * cw + nx
                    if mask[k] and not label[k]:
                        label[k] = nxt
                        stack.append(k)
    return cw, ch, step, top, label


def component_at(comp, x, y, radius=5):
    """The land component around (x, y) in full-frame coords, or 0.

    Majority vote over a neighbourhood rather than first-found: a settlement's
    banner can overhang a shore, and a first-found scan then reports whichever
    neighbouring province happens to be nearest in scan order.
    """
    cw, ch, step, top, label = comp
    cx, cy = x // step, (y - top) // step
    votes = {}
    for dy in range(-radius, radius + 1):
        for dx in range(-radius, radius + 1):
            nx, ny = cx + dx, cy + dy
            if 0 <= nx < cw and 0 <= ny < ch:
                v = label[ny * cw + nx]
                if v:
                    votes[v] = votes.get(v, 0) + 1
    if not votes:
        return 0
    return max(votes.items(), key=lambda kv: kv[1])[0]


def settlements(w, h, px, right_margin, top):
    """Settlement blobs merged into whole settlements (banner + population chip).

    A settlement draws two parchment rectangles: a name banner, and a population
    chip below it. They must be merged, but two NEIGHBOURING settlements must not
    be — and on the committed frames a banner sits ~22px above its own chip while
    an adjacent settlement's banner is ~21px to the side, so a plain proximity
    gap cannot tell them apart (it fused three settlements into two on the first
    attempt). The chip is always drawn within its banner's horizontal span, so
    the discriminator is X-OVERLAP plus vertical closeness, not distance.
    """
    bs = blobs(w, h, px, right_margin, top, is_settlement, gap=6)
    merged = True
    while merged:
        merged = False
        for i in range(len(bs)):
            for j in range(len(bs) - 1, i, -1):
                p, q = bs[i], bs[j]
                x_overlap = p[0] <= q[1] + 4 and p[1] >= q[0] - 4
                y_close = p[2] <= q[3] + 35 and p[3] >= q[2] - 35
                if x_overlap and y_close:
                    p[0], p[1] = min(p[0], q[0]), max(p[1], q[1])
                    p[2], p[3] = min(p[2], q[2]), max(p[3], q[3])
                    del bs[j]
                    merged = True
    return bs


def report_movement(a, b, right_margin, top):
    """Pair settlements across two frames and split intra- from inter-province."""
    (w, h, pa), (_, _, pb) = a, b
    sa = settlements(w, h, pa, right_margin, top)
    sb = settlements(w, h, pb, right_margin, top)
    comp_a = land_components(w, h, pa, right_margin, top)
    comp_b = land_components(w, h, pb, right_margin, top)
    mid = lambda s: ((s[0] + s[1]) // 2, (s[2] + s[3]) // 2)

    print(f"settlements: {len(sa)} -> {len(sb)}")
    used = set()
    held = moved_in = moved_across = indeterminate = 0
    for s in sa:
        ax, ay = mid(s)
        best, bd = None, None
        for k, t in enumerate(sb):
            if k in used:
                continue
            bx, by = mid(t)
            d = ((bx - ax) ** 2 + (by - ay) ** 2) ** 0.5
            if bd is None or d < bd:
                best, bd = k, d
        if best is None:
            print(f"  ({ax:5d},{ay:5d})  GONE")
            continue
        used.add(best)
        bx, by = mid(sb[best])
        if bd < 1.0:
            held += 1
            print(f"  ({ax:5d},{ay:5d})  held")
            continue
        # WITHIN or ACROSS is decided inside ONE frame's labelling at a time:
        # component ids come from an independent scan per frame and are NOT
        # comparable between them. Comparing them directly — which this did at
        # first — yields a confident verdict from two different namespaces.
        # Both frames must agree, or the answer is honestly unknown.
        same_in_a = component_at(comp_a, ax, ay) == component_at(comp_a, bx, by)
        same_in_b = component_at(comp_b, ax, ay) == component_at(comp_b, bx, by)
        if same_in_a and same_in_b:
            moved_in += 1
            verdict = "WITHIN its province"
        elif not same_in_a and not same_in_b:
            moved_across += 1
            verdict = "ACROSS provinces"
        else:
            indeterminate += 1
            verdict = "province membership INDETERMINATE (the frames disagree)"
        print(f"  ({ax:5d},{ay:5d}) -> ({bx:5d},{by:5d})  moved {bd:6.1f}px  {verdict}")
    for k, t in enumerate(sb):
        if k not in used:
            bx, by = mid(t)
            print(f"  ({bx:5d},{by:5d})  NEW")
    print(
        f"\n  held {held} · moved within {moved_in} · moved across {moved_across}"
        + (f" · indeterminate {indeterminate}" if indeterminate else "")
    )
    print("  (a move WITHIN a province is what A3 fixes; a move ACROSS is `city_home`")
    print("   following the pods, which is arguably correct rather than instability)")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("frame_a")
    ap.add_argument("frame_b")
    ap.add_argument("--class", dest="cls", choices=sorted(CLASSES), default="land")
    ap.add_argument(
        "--right-margin",
        type=int,
        default=528,
        help="pixels excluded on the right — the docked column (default 528 = COL_W 264 at dpr 2)",
    )
    ap.add_argument("--top", type=int, default=60, help="rows excluded at the top — the menu bar")
    ap.add_argument("--blobs", action="store_true", help="also list class bounding boxes per frame")
    ap.add_argument(
        "--movement",
        action="store_true",
        help="pair settlements across the frames and split intra- from inter-province movement",
    )
    args = ap.parse_args()

    cls = CLASSES[args.cls]
    with tempfile.TemporaryDirectory() as tmp:
        a = load(args.frame_a, tmp)
        b = load(args.frame_b, tmp)
        r = compare(a, b, args.right_margin, args.top, cls)
        if r is None:
            print("crop is empty — no area to compare (check --right-margin / --top)")
            return 2
        t = r["total"]
        pct = lambda n: f"{100.0 * n / t:7.3f}%"
        print(f"class            {args.cls}")
        print(f"crop             {a[0] - args.right_margin} x {a[1] - args.top}  ({t} px)")
        print(f"identical        {r['identical']:9d}  {pct(r['identical'])}")
        print(f"{args.cls} lost     {r['lost']:9d}  {pct(r['lost'])}")
        print(f"{args.cls} gained   {r['gained']:9d}  {pct(r['gained'])}")
        print(f"changed in place {r['changed']:9d}  {pct(r['changed'])}")
        delta = r["lost"] + r["gained"]
        print(f"CLASS DELTA      {delta:9d}  {pct(delta)}   (of map area)")
        print(f"{args.cls} footprint {r['footprint_a']:9d} -> {r['footprint_b']:d}"
              f"   ({100.0 * r['footprint_a'] / t:.3f}% -> {100.0 * r['footprint_b'] / t:.3f}% of map)")
        # The figure that is actually comparable between classes: how much of
        # the class's OWN area changed. A map-area share flatters a small class.
        if r["footprint_a"] > 0:
            print(f"DELTA / FOOTPRINT          {100.0 * delta / r['footprint_a']:7.2f}%"
                  f"   <- compare THIS across classes")
        else:
            print("DELTA / FOOTPRINT          unknown (the class is absent from frame A)")
        if args.movement:
            print()
            report_movement(a, b, args.right_margin, args.top)
        if args.blobs:
            for name, fr in ((args.frame_a, a), (args.frame_b, b)):
                bs = blobs(fr[0], fr[1], fr[2], args.right_margin, args.top, cls)
                print(f"\n{os.path.basename(name)}: {len(bs)} {args.cls} blobs")
                for x0, x1, y0, y1 in sorted(bs, key=lambda q: (q[2], q[0])):
                    print(f"  x {x0:5d}-{x1:5d}  y {y0:5d}-{y1:5d}  ({x1 - x0 + 1}x{y1 - y0 + 1})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
