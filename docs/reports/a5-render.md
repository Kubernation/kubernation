# A5 — fresh ground rendered, and the wave gate

**Phase:** A5 (rendering half), from `kubernation-a5-render-guidance.md`
**Version:** v1.10.0 · **Date:** 2026-08-03
**Preceded by:** `a5-succession-core.md` (v1.9.1, the core half)

---

## 1. What shipped

Ground that changed hands is tinted and fades back to ordinary terrain over an
ageing window. A rolling node refresh now reads as a wave crossing the fleet.

| Piece | Where |
| --- | --- |
| Tier bucketing (the single authority) | `theme::fresh_tier` |
| Colour, 3 tiers, both palettes | `theme::fresh_land_pair` |
| Per-node freshness for the frame | `net::freshness_by_node` → `WorldSnap.fresh` |
| Tint under every overlay | `draw::overlay_pair` (checks fresh first) |
| The panel half | `panels::fresh_line` (province **and** city arms) |
| Live setting | `Net::fresh_window` (atomic, read per tick) |
| Control | **View ▸ AGEING WINDOW** radio + `--fresh-minutes` + pref |
| Documentation | Almanac ▸ World, "Ground that changed hands" |

425 core + 100 GUI tests; gui-smoke 51 states; clippy clean.

---

## 2. The gate — and why the first run failed

§4's gate is that a rolling refresh reads as a wave. **It passed, but only on the
second run, and the first run's failure is the more useful result.**

### Run 1 — the declared 60-minute default

Scenario 1 (30-node surging refresh), 14 frames 16s apart, all four framing flags
pinned. Fresh pixels per frame:

```
frames 00-02        0
frames 03-04   26,532
frames 05-13   52,171   (plateau — never recedes)
```

A leading edge with **no trailing edge**. This is §4.2's named failure mode,
*"too long and everything is marked."*

It is not a defect in the code. The harness compresses a 30-node refresh into
about 4 minutes; a 60-minute window swallows it whole, so at the end of the run
everything that changed is still marked. **The declared default is therefore
untestable on this harness by construction** — not because the harness is
inadequate, but because a simulated fleet refreshes far faster than the
production cadence the default is sized for.

### Run 2 — the window matched to the observed cadence

Same scenario, `--fresh-minutes 1`:

```
00-03      256      baseline (see note)
04-06  258,262      first wave arrives
07     517,575      PEAK — two waves on screen at once
08-09  259,569      first wave has aged out
10-15      256      passed
```

Arrival, body, recession. **Gate condition met.**

*Note on the 256:* my counter's ochre predicate is deliberately tolerant (the
terrain painter adds per-cell jitter, so exact-colour matching undercounts badly).
That tolerance catches a few hundred sand pixels. Signal-to-noise is ~1000:1.

### The discrimination check (§4.1) — run, not assumed

Same scenario, `--fresh-minutes 0`:

```
peak with marking OFF:       0
peak with marking ON:  517,575
```

The instrument responds to the mechanism, not to the scenario. This check is
mandatory in this workstream because **six instruments have now emitted a
plausible number for a reason unrelated to what they claimed to measure** (A2's
process-per-frame flipbook, A2's incomparable framings, `gate.sh` able to churn
nothing, the measurement session's component-ID bug, A3's name generator, A4's
gate specification). It is the only defence that has ever worked.

### Mutation floor

Five mutations, all caught:

| Mutation | Caught by |
| --- | --- |
| `freshness` always `None` | 2 core tests |
| Fresh ground painted as ghost ground | 2 GUI tests |
| Gradient flattened (every age one colour) | 1 GUI test |
| Quantisation dropped (continuous fade) | 1 GUI test |
| `fresh_line` buckets independently of the colour | the new shared-authority test |

---

## 3. Decisions worth carrying back

### 3.1 The window is a setting, and had to become an in-app one

Run 1 vs run 2 is not "I picked the wrong number." It is a structural property:
**the window must exceed one refresh but not the whole observation period**, and
where that lands depends entirely on a fleet's refresh cadence, which varies by
orders of magnitude between a simulated fleet (minutes) and a conservative
production one (hours).

That is why the setting gained a **View ▸ AGEING WINDOW** radio rather than
staying a launch flag. If the only way to change it is `--fresh-minutes`, finding
a workable value means restarting the app once per guess against a live cluster.
Consequences:

- `Net.fresh_window` is an atomic read **per tick**, so the map re-tints within a
  tick of the change.
- The exit-save persists the **live** value, not the launch value — saving the
  latter would silently discard the choice just made.
- The menu offers five values; the flag still takes any.

### 3.2 One authority for the bucketing

`theme::fresh_tier` is used by both the colour and the words, pinned by a test
asserting they change at the same freshness values. Two independent
quantisations of one number is precisely the drift this codebase keeps paying
for, and here it would have let the map paint a province "just changed hands"
while the panel called it "settling" — with nothing else failing.

### 3.3 The panel half is not optional

By the standard the substrate round set: *the overlay says which node, so
something must say which DaemonSet, or the map raises a question it can't answer
and the operator leaves for `kubectl`.* Fresh ground raises exactly that
question. So a fresh province reads `new ground · just changed hands` (or
`recently` / `settling`) in SELECTION — **ungated by overlay**, unlike the
saturation/cost/substrate lines beside it, because fresh ground is tinted under
*every* overlay and so is most surprising where those lines are absent.

The wording is relative, not a reconstructed duration. Freshness is a fraction of
the window; inverting it to "4 minutes ago" would state a precision the fraction
does not carry.

---

## 4. What the Almanac check turned up

The Almanac entry as first drafted named a **"Game ▸ Ageing window"** menu item
that did not exist. Verifying it before shipping also surfaced that **ghost
ground has been undocumented since A2 introduced it** — so the map already had
one colour the operator could not interpret, and this phase was about to add a
second on top of it.

Both are now documented together in Almanac ▸ World, which is the right pairing:
they are the same family of fact (ground with no live node vs ground whose node
just changed hands), and neither is derivable from anything else on screen.

A menu test pins that an **off-list window marks nothing**. This is reachable,
not hypothetical — the flag takes any value while the menu offers five. The
failure it forbids is a tick beside "5 minutes" when the window is really 7: a
control claiming a state the app is not in, which is worse than admitting it has
no name for the current one.

---

## 5. Open questions for planning

1. **The 60-minute default is unvalidated.** It is a reasonable guess for a
   production refresh cadence, but nothing in this project can test it — the
   harness cannot run slowly enough, and the dev cluster does not churn. It will
   first be exercised on a real fleet. The gate that *can* run is the shape of
   the wave at a matched window, which is what run 2 measured.

2. **Fresh-vs-ghost separability during a surge was not measured.** Both were on
   screen simultaneously at the gate and plainly distinct (ochre / green / grey),
   and a unit test pins the fresh–ghost colour distance in both palettes, but no
   metric pins it *under surge conditions specifically*. §2.2 called this out as a
   risk; it is judged visually, not numerically.

3. **The minimap does not tint fresh ground.** Same gap as walls, cost and
   substrate — no per-node data for those is threaded to the minimap. Worth
   deciding once for all four rather than per-feature.

4. **Warm-cluster ageing is empty by construction**, matching layout persistence
   (A4 deferred warm for the same reason: the warm world is a comparison view,
   not somewhere the operator navigates). If warm ever becomes navigable, layout
   persistence and ageing should arrive together.

5. **Workstream A is now complete through A5.** The provinces hold still (A2),
   the cities hold still (A3), the map survives restart (A4), and succession is
   both recorded and visible (A5). The remaining named item from the
   decomposition is `region ← pool ∩ zone` visual grouping, which A2 gave up when
   ordinals went zone-wide, and which nothing since has claimed.
