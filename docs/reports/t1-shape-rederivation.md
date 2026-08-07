# T1 §3.1 re-derivation — how the diagram was made, and what it should say

**Guidance:** `docs/kubernation-t1-shape-rederivation-guidance-rev2.md`
**Version:** v1.18.1 · **Date:** 2026-08-07
**No product change** — one instrument, one doc-comment number, and corrections
to two reports.

**Answer:** T1's method was **sound** — ordinal-keyed, therefore map order, not a
hash-order artifact. Its diagram is **right in three of four columns and wrong in
one**: z-b is two pieces, not one, because the script collapsed ghost ordinals
out. The change was pool-shaped in the data far beyond chance (`P ≤ 0.005` in
every changed zone). And the re-derivation turned up a **separate error in my own
record**: the "4 of 8" region figure is **3 of 8**.

---

## 1. §1 — all eight claims verified

Six `[V]`, two `[A]`; all checked against source. All TRUE. Line references
drifted a few lines (my v1.17.1/v1.18.0 edits) — noted, not material.

Claim 7 matters more than its tag suggests: `--dump-positions` emits **`ordinal`
directly**, so §3.2's concern about re-deriving `slot_of_row` is moot — there is
nothing to convert. The inverse was not reimplemented anywhere in this session.

---

## 2. §2 — how the diagram was produced

**None of the guidance's three options.** It was a fourth, found in the session
transcript rather than reconstructed from memory: an ad-hoc script that read the
**persisted layout store**, `~/.local/state/kubernation/layouts/<context>.json`.

```python
live = [s for s in d['slots'] if s.get('occupant')]
by_zone[s['zone']].append((s['ordinal'], bool(s.get('occupied_at')), s['pool']))
...
rows = sorted(by_zone[z])            # sorted by ORDINAL
line = ''.join('X' if ch else '.' for _, ch, _ in rows)
runs = [len(list(g)) for k, g in itertools.groupby(line) if k == 'X']
```

It keys on `s['ordinal']` and sorts by it. **That is map order.** So the second
row of §2's table — "a walk over `cont.provinces` … must be withdrawn" — does not
apply, and neither does the third: this was measured, not eyeballed.

The guidance's expectation that "the third is the likeliest and the most awkward"
was reasonable and wrong. Worth recording *why* it was answerable at all: the
work happened in a session whose transcript survives, so the question had a
factual answer rather than a recollection. Absent that, the honest answer would
have been "unknown", which is materially weaker than either outcome §5 anticipated.

### 2.1 But the script had a convention the report never stated

It iterates **live slots only**, so a vacated ordinal is not a gap in the string
— it is absent from the string. A ghost between two changed slots therefore
**cannot** break a run in that diagram, while under `pool_label_pieces`' rule it
does. That is not a bug in the script; it is an unstated definition, and it is
exactly §3.1's distinction arriving from an unexpected direction.

---

## 3. The measurement

`hack/churn/pieces.py` (new, with `pieces-selftest.py` — the project convention
for a committed instrument). It reads the layout store, computes both piece
definitions by ordinal, runs the pool-blind control, and **cross-checks** every
node's `(zone, pool, ordinal)` against `--dump-positions`.

**The fleet is still in T1's exact configuration** — 18 successions, 6/6/6 across
z-a/z-b/z-c, 100% `sys`, z-d untouched — so this re-derives on identical data
rather than on a re-run.

### 3.1 A guidance correction: the dump alone cannot answer this

§3 says "From `--dump-positions`". It cannot be: the dump carries no succession
data (`ordinal, x, y, w, h, pool, zone, node, extent_source, kind, tick`), so the
*changed set* is not expressible in it. The layout store is the only source that
has `occupied_at` — and it is equally independent of the model walk that §3.4
warns about, arguably more so, since `SlotKey` **is** the ordinal.

Resolved by using both: the store for succession, the dump as an independent
second emitter for position. **100 nodes compared, 0 disagreements.**

### 3.2 Results

```
z-a: 37 live, slots 0-40, 4 ghost ordinals
    region  burst      1 piece [12]        100%
    region  sys        2 pieces [6, 4]      60%
    region  t3.xlarge  1 piece [15]        100%
    changed by-ordinal   1 piece [6]       100%
    changed by-live-pos  1 piece [6]       100%
    control  median 5 pieces (p05 4, p95 6), mean largest share 28%
             P(pieces <= 1) = 0.0000

z-b: 22 live, slots 0-25, 4 ghost ordinals
    region  burst      1 piece [12]        100%
    region  sys        3 pieces [4, 3, 3]   40%
    changed by-ordinal   2 pieces [3, 3]    50%   <-- T1 published 1
    changed by-live-pos  1 piece [6]       100%   <-- a ghost splits it
    control  median 5 pieces (p05 3, p95 6), mean largest share 33%
             P(pieces <= 2) = 0.0050

z-c: 26 live, slots 0-29, 4 ghost ordinals
    region  mem        1 piece [16]        100%
    region  sys        2 pieces [6, 4]      60%
    changed by-ordinal   1 piece [6]       100%
    changed by-live-pos  1 piece [6]       100%
    control  median 5 pieces (p05 3, p95 6), mean largest share 32%
             P(pieces <= 1) = 0.0000

z-d: 15 live, 0 ghost ordinals
    region  t3.xlarge  1 piece [15]        100%
    changed              no changed slots

FLEET regions: 3 of 8 regions in more than one piece; 8 regions in 12 pieces
FLEET changed: 18 changed slots in 4 by-ordinal pieces across 3 zones
```

**T1's diagram, corrected:** z-a ✓, z-c ✓, z-d ✓, **z-b ✗** — two pieces of 3,
not one run of 6. Fleet total 4 pieces, not 3.

### 3.3 §4 — the control

Random placement of the same `k` among the same zone's live ordinals gives a
**median of 5 pieces** and a mean largest share of ~30%. The observed values are
1, 2 and 1 pieces at 100%, 50% and 100%. `P(pieces ≤ observed)` = 0.0000, 0.0050,
0.0000 over 2000 trials each.

So the change was **pool-shaped in the data**, decisively — even in z-b, where
two pieces is still far more concentrated than chance. The shape was in the data,
not only in the reader.

**Scope of that claim, stated deliberately:** it is about the *data*, not about
legibility. Whether an operator reads the shape off the map is the usability
question T1 §3.4 declined to answer alone, and this does not answer it either.

---

## 4. §5 — what the answer decides

The first branch: *"the shape was real on this fleet, and the open item stays as
recorded — pool-shaped change reads as a shape when allocation order happens to
make it contiguous, which is a fixture property rather than a map guarantee."*

T1's own caveat is now **demonstrated rather than asserted**. `sys` is itself in
2–3 region pieces per zone (largest share 40–60%); the 18 successions happened to
fall inside one or two of them. The fixture supplied the contiguity, exactly as
T1 said — and z-b shows what happens when it does not quite: the change lands in
two pieces because a ghost is in the way.

So T1's positive evidence for the map is **not withdrawn**, and
`region ← pool ∩ zone` remains a strong suspicion rather than becoming a firm T2
blocker. (It has since shipped anyway, in v1.14.0–v1.17.0, which does not change
what this measurement decides — only how much rides on it.)

---

## 5. The other finding: "4 of 8" was 3 of 8

While computing region pieces I could not reproduce my own published figure.

`docs/reports/region-label-ordering.md` §4, and from there the
`pool_label_pieces` doc comment, the v1.17.0 CHANGELOG entry and the decision
log, all say **"4 of 8 regions are in more than one piece"**. The correct figure
is **3 of 8**.

**The per-zone data was right and is unchanged** — z-a `sys` 2 pieces, z-b `sys`
3, z-c `sys` 2, everything else 1. The summary line was not. The script printed
`8 regions in 12 pieces`, and I read the difference — 4 — as the number of
fragmented regions. It is the number of *extra pieces*. Three regions are split.

Note what this is and is not. It is **not** the v1.17.0 failure repeating: that
was a measurement derived by the same reasoning as the code. Here the instrument
was correct and independent; the error was in narrating a number it had not
printed. Different mechanism, same class of consequence — a wrong figure entering
the record — and the fix is mechanical rather than conceptual: `pieces.py` now
**emits** `regions_split of regions_total` and the piece total, with a comment
saying why, so neither can be inferred from a breakdown again.

**The design conclusion is unaffected.** 3 of 8 fragmented, with a largest piece
as low as 40%, still says name every piece — the argument in v1.17.0 §4 turns on
"most of a fragmented region's ground is outside its largest piece", which 40%
and 60% establish independently of how many regions are fragmented.

Corrected in all four places, plus the dependent sentence "half the regions, not
an eighth", which was also wrong.

---

## 6. §6 — standing questions

**1. Summing before comparing?** Yes, and it is §5. `regions_total` and
`region_pieces` were summed, printed, and then *differenced by hand* to produce a
count of a different thing. The instrument now compares before summing — it tests
`pieces > 1` per region and counts that.

**2. Unknown, or fabricated?** `summarise` returns `pieces: None` and
`share: None` for an empty set, printed as **"no changed slots"** — never "1
piece" or "0% largest", both of which read as measurements. z-d exercises it
live, and a self-test pins it.

**3. Two sections constraining one behaviour, and a fixture where they diverge?**
§3.1's two definitions of "piece". The divergence is not hypothetical — z-b is
the fixture, and it is precisely where T1's published number is wrong. The
instrument reports both rows and flags the disagreement inline (`<-- a ghost
splits it`), which is the only reason the correction was visible at all.

**4. Consumers depending on an old meaning?** None — no behaviour changed. The
`pool_label_pieces` doc comment carried the wrong figure and is corrected; the
code it documents is untouched.

**5. Inherited claims — does the state each describes actually occur?** The
guidance's §0 quote of "4 of 8" is inherited from me and is false; the state it
describes (four fragmented regions) does not occur. Caught by re-deriving rather
than re-quoting, which is the whole point of the question.

**6. One side of a comparison moved?** Yes, and this is the sharp one. "Piece"
means one thing for regions and another for the changed set, and T1's script
silently meant a *third* (positional, ghosts collapsed). Three definitions of one
word, none of them written down until now.

**7. Container adjacency vs world adjacency — and what guarantees it?** The
guidance says this is why the session exists, and applied to the **script**:
- `pieces.py` derives adjacency from **ordinal arithmetic** (`o == prev + 1`),
  never from record order in the file — guaranteed by construction, since the
  file order is the emitter's and is never consulted.
- The one place record order *is* used is deliberate and labelled: the
  `by-live-pos` row, which reproduces T1's convention so the two can be compared.
  It is reported beside the ordinal figure, never instead of it.
- The self-test asserts the two disagree on a ghost, so the distinction cannot
  quietly collapse.

---

## 7. §7 — acceptance

- [x] T1's derivation method identified and recorded — §2, from the transcript
- [x] Changed-set pieces re-derived by ordinal — §3.2 (the dumped `ordinal` used directly; nothing re-derived)
- [x] Region pieces computed alongside and explicitly distinguished — §3.1, §3.2
- [x] Unpooled sentinel excluded (0 present on this fleet; the exclusion is coded and self-tested)
- [x] Largest-piece share reported, not only counts
- [x] Pool-blind control run repeatedly (2000 trials/zone); distribution reported
- [x] T1's diagram corrected **in T1's report** — `t1-change-since.md` §3.1
- [x] Open-decisions record updated — the decision log's T1 entry
- [x] Standing questions answered — §6
- [~] **No product code changed** — one deviation: the `pool_label_pieces` doc
      comment carried the "4 of 8" figure. Leaving a known-false measurement in
      the source so that a docs-only rule could be honoured seemed the worse
      trade. No behaviour, no signature, no test changed.

---

## 8. §8 — what this session did not do

No T2 work and no `region ← pool ∩ zone` work; no re-run of T1's gate; and no
tuning toward a tidy number — z-b came back fragmented and is reported that way,
which is the more useful finding of the two.
