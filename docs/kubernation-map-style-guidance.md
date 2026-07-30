# KuberNation — Runtime-Selectable Map Rendering Styles

**Implementation guidance — v2**
**Goal:** make map rendering style a first-class configuration capability, selectable by the user at runtime.

> **v2 (2026-07-29)** — revised after fact-checking v1 against the tree at `708eb36`.
> 28 confirmed corrections folded in. The three that change the *design* (not just
> line numbers): hit-testing becomes a **two-plane** problem (§3), cliff colour
> **cannot** come from `iso_terrain_pair` (§4), and the island row prisms the
> **sea** if followed literally (§6). v1's §1 rationale was also built on a stale
> doc comment (§1). Everything else is inventory completeness.

---

## 1. The decision that shapes everything else

KuberNation has two precedents for a render-affecting mode flag.

| | `colorblind` (theme.rs:23) | `Overlay` (draw.rs / menu.rs) |
|---|---|---|
| Storage | `static COLOR_MODE: AtomicU8` | enum, passed as a parameter |
| When set | Startup **or any frame** — `MenuAction::ToggleColorblind` (menu.rs:37) flips it live at main.rs:2800-2803 | Any frame |
| User control | `--colorblind` flag + a View-menu toggle | View menu radio group |
| Persistence | `Prefs.colorblind: bool` | `Prefs.overlay: Option<String>` |
| Round-trip | — | `overlay_from_str` ⇄ `Overlay::label` |
| Tested | `colorblind_palette_swaps_meaning_greens_to_blue` | `overlay_default_is_terrain` |

> ⚠️ **v1 correction.** v1 claimed the atomic is "startup only," citing
> `set_colorblind`'s doc comment (*"call once at startup, before any draw"*).
> **That comment is stale** — the live View-menu toggle shipped in v0.68.0 and the
> palette re-reads the atomic every frame. Fix the comment while you're in there.

**Model `MapStyle` on `Overlay` anyway** — but for the right reason. It is *not*
that the atomic can't switch at runtime; it demonstrably can. It is that map style
is **geometry**, and geometry is per-camera:

1. It participates in the **inverse** projection (§3). A global can't be
   type-checked against the transform that must undo it.
2. `Camera::shifted()` (draw.rs:317) mints a **second** camera for the warm world.
   A process-global cannot express "this camera's style" if the pair ever needs to
   differ — and it makes the desync in §3 a runtime bug instead of a compile error.

### Naming

`MapStyle::{Plain, Relief}`. *Relief map* is a real cartographic term fitting the
project's voice (Charter, Annals, Almanac, Wonders); `Plain` carries a useful
double meaning. Avoid `Flat` (collides with `draw::overlay_flat`, draw.rs:229) and
`Chart` (collides with `Charter`, the RBAC matrix).

**Put the enum, `label()`, and the parser all in `draw.rs`.** v1 routed
`map_style_from_str` to main.rs beside `overlay_from_str` (main.rs:512) — but
**main.rs has no `#[cfg(test)]` module at all**, so the round-trip test v1's own
checklist demands would have nowhere to live. `Overlay` already keeps its `label()`
and its test in draw.rs; keep the parser there too and the test lands beside it.

---

## 2. Three-part split

```
                    MapStyle  (enum — the user-facing selection)
                        │
        ┌───────────────┴───────────────┐
        │                               │
   GEOMETRY knobs                  PALETTE knobs
   → live on Camera                → live in theme.rs
   (lift, tile shape)              (cliff SHADING — see §4)
   Must stay in sync with          Derived from the top colour,
   BOTH inverses (§3)              never from a fixed pair
```

- **Geometry → `Camera`.** Anything that changes where a world cell lands on
  screen must also change the inverse, or clicks silently miss by the offset.
- **Palette → `theme.rs`,** but as a *shading function*, not a colour table (§4).
- **Selection → app state**, threaded exactly like `Overlay`, pushed into the
  Camera when it changes.

---

## 3. The real correctness trap: hit-testing becomes two-plane

v1 framed this as one line. The line is right; the framing is not.

If land tops lift by *L* px, the inverse must undo it — but **only for things drawn
on the lifted plane.** §7 deliberately keeps coast markers, shallows and ocean at
sea level, and **coast markers are hit-tested**:

- `panel_for` (main.rs:3634) consults `coast_at` **before** `region_at` — clicking
  a harbour opens the city it serves.
- `region_lines` (panels.rs:143) does the same for the hover tooltip and the
  SELECTION column.

A single corrected inverse serves land and breaks those. **Magnitude** (from
`cell_px()`: `hw=16·zoom`, `hh=8·zoom`; `land_lift=7.0` ⇒ `L=7·zoom`): because L
and `hh` both scale with zoom, the error is **zoom-invariant in cell units** —
`Δb = L/hh = 0.875`, so `Δwx = Δwy = 0.4375` cells at *every* zoom. ~68% of
play-area pixels would resolve to a different cell than they do today, and only
~32% of a harbour's footprint would still resolve to it.

### Prescription: two inverses, selected per plane

Do **not** "fix" `cell_at`. Keep it as the sea-level inverse and add a land one,
then resolve each thing on the plane it is *drawn* on — the same split §7 already
documents for drawing.

```rust
/// Screen point → world cell on the SEA plane (ocean, shallows, coast markers).
/// Unchanged from the pre-relief behaviour.
pub fn cell_at(&self, screen: Vec2, bounds: (u16, u16)) -> Option<(u16, u16)>

/// Screen point → world cell on the LAND plane (terrain, cities, structures).
/// Undoes `to_land`'s lift. Identical to `cell_at` when `lift_px() == 0`.
pub fn cell_at_land(&self, screen: Vec2, bounds: (u16, u16)) -> Option<(u16, u16)> {
    // b = (sy + pos.y + L) / hh   — the y-only lift leaves `a` untouched
}

/// Both planes for one screen point, so callers can resolve per-feature.
pub fn hit(&self, screen: Vec2, bounds: (u16, u16)) -> Hit { … }

pub struct Hit { pub sea: Option<(u16, u16)>, pub land: Option<(u16, u16)> }
```

Then `panel_for(&worlds, hit)` resolves `coast_at` from `hit.sea` and `region_at`
from `hit.land`; `region_lines` takes the same `Hit`. **Under `Plain`, `L == 0` so
`sea == land`** and every call path is byte-identical to today — which is exactly
Phase 0's exit criterion.

`Camera::shifted()` must propagate `style`, or the warm world renders flat beside
a raised primary.

**Known residual (accept and document):** a click on a *cliff face* has no correct
answer — the wall belongs to the tile above it but occupies the pixels of the
water below. Resolving `region_at` from the land plane means the cliff band reads
as its own tile, which is the intuitive answer; the trade is an L-px band of
ocean along each south/east shore that no longer selects the sea.

---

## 4. Cliff colour: derive, don't tabulate

> ⚠️ **v1 correction.** v1's §6 sent cliff colours to `theme::iso_terrain_pair`
> (theme.rs:151). That is the wrong funnel and would break v1's own acceptance
> items 7-8. Land fill comes from **`overlay_pair`'s nine producers**
> (draw.rs:190-222) — one per overlay (terrain / pressure / replicas / namespace /
> walls / saturation / cost / idle / walled) — and the cells that actually
> *silhouette* take the other branch entirely: `fill_diamond(sand)` from the
> `ISO_SAND` const (draw.rs:1050-1058). Wiring cliffs to `iso_terrain_pair` would
> give every non-terrain overlay health-coloured cliffs, and sand cliffs would be
> wrong everywhere.

Make the cliff a **function of whatever colour the top was drawn with**:

```rust
/// Shade for the cliff wall under a land top of `top`. Derived so it composes
/// with all nine `overlay_pair` producers AND the colour-blind variants for
/// free — there is no cliff colour to keep in sync.
pub fn cliff_shade(top: Color) -> Color
```

One function, one test, zero drift. It follows `iso_block`'s existing wall-shading
convention (draw.rs:1251) — reuse its factor so buildings and cliffs agree.

---

## 5. Suggested `Camera` shape

```rust
pub struct Camera {
    pub pos: Vec2,
    pub zoom: f32,
    target: Option<Vec2>,      // NOTE: private — v1's snippet marked it `pub`
    /// Active map style. Geometry only — palette shading lives in `theme`.
    pub style: MapStyle,
}

impl Camera {
    /// Land's height above sea level in SCREEN px (0.0 under `Plain`).
    pub fn lift_px(&self) -> f32 { self.style.land_lift() * self.zoom }

    /// Screen point for something STANDING ON LAND. Sea-level things —
    /// shallows, harbours and gates moored in water, the minimap — keep
    /// using `to_screen`.
    pub fn to_land(&self, wx: f32, wy: f32) -> Vec2 {
        let p = self.to_screen(wx, wy);
        vec2(p.x, p.y - self.lift_px())
    }
}
```

Switching style is `cam.style = new;`. The next frame draws differently: no
rebuild, no reload, no cache to invalidate, and `draw_world`'s signature never
changes (it already receives `&Camera`).

---

## 6. Phasing

Land the *capability* before the *pixels*. Phase 0 ships a working runtime
selector with zero visual risk.

### Phase 0 — The seam (no visual change)

`MapStyle::Relief` is a no-op identical to `Plain` (`land_lift()` returns `0.0`
for both). Everything else is real: menu, prefs, CLI, tests.

- [ ] `MapStyle` enum + `land_lift()` + `label()` + `from_str` — **in draw.rs**, beside `Overlay`
- [ ] `Camera.style`, `lift_px()`, `to_land()`, **`cell_at_land()` + `Hit` + `hit()`** (§3), `shifted()` propagation
- [ ] `panel_for` / `region_lines` take a `Hit` and resolve per plane
- [ ] `MenuAction::SetMapStyle(MapStyle)` + `MenuCtx.style` + a `MAP STYLE` header/radio group in View, mirroring `MAP OVERLAY` (menu.rs:127)
- [ ] `Prefs.map_style: Option<String>` — **plus** the two exhaustive struct literals (main.rs:3496, prefs.rs:113) and the partial-field test (prefs.rs:126). Saved at **exit**, matching the existing design — not "on change"
- [ ] `--map-style plain|relief` on `Args`, phrased like `--overlay`: *"Set from the View menu at runtime; flag is for shots."*
- [ ] Tests: default-is-Plain · `from_str` round-trip · **`cell_at_land` round-trip at non-zero lift** · `sea == land` when lift is 0

**Exit criteria:** flip the menu item, restart, confirm it persisted — with a
pixel-identical map and unchanged click behaviour.

### Phase 1 — Relief terrain

- [ ] `fill_prism` beside `fill_diamond` (draw.rs:402; reuse `diamond_pts`, draw.rs:392)
- [ ] `draw_province_terrain` passes `right_sea` (1047) / `dn_sea` (1049) as silhouette flags
- [ ] `land_diamond` (1068) takes the lift; `cliff_shade` (§4) for the walls
- [ ] `to_screen` → `to_land` sweep across land-standing draws (§8)
- [ ] Cull margins account for the lift (§9)
- [ ] `MapStyle::Relief.land_lift()` returns a real value (start at `7.0`)

### Phase 2 — Coverage and polish

- [ ] Islands: **only the sand body** (draw.rs:1659) becomes a prism
- [ ] Island *features* — labels, count badge, structure marks, legend anchor (1668 / 1686 / 1719 / 1755) → `to_land`
- [ ] `fit` / `fly_to` framing accounts for the lift
- [ ] Minimap stays deliberately flat — confirm and comment
- [ ] Almanac + STATUS readout decision (§9)

### Phase 3 — Out of scope, do not preclude

Hex topology. `MapStyle` should be the enum a `Hex` variant slots into. Do **not**
build a `trait MapGeometry` yet — there is no second geometry to abstract over.

---

## 7. Change inventory

| File | Line | Change |
|---|---|---|
| `draw.rs` | 285-296 (`struct Camera` + `new`) | Add `style` — the struct itself, which v1's range excluded |
| `draw.rs` | 303-337 (`impl Camera`) | `lift_px()`, `to_land()`, `cell_at_land()`, `hit()`; `shifted()` propagation |
| `draw.rs` | 339-370 (`fly_to`/`jump_to`/`fit`) | Framing accounts for the lift (Phase 2) |
| `draw.rs` | 402 (`fill_diamond`) | Add `fill_prism` beside it; reuse `diamond_pts` (392) |
| `draw.rs` | 660 (`draw_world`) | **No signature change** — style rides on `&Camera` |
| `draw.rs` | 673 | Pair HOT/WARM banner — classify (chrome; likely stays `to_screen`) |
| `draw.rs` | **812-818 (`draw_selection`)** | **→ `to_land`. Absent from v1.** The selection diamond is on screen after every click |
| `draw.rs` | **870-913 (`draw_blast`)** | **→ per-target plane. Absent from v1.** One `center` closure paints both cities (land) and harbours (sea) |
| `draw.rs` | **917-932 (`province_offscreen`)** | **Cull margins ignore the lift.** Absent from v1 |
| `draw.rs` | 939 / 977-984 (`draw_province_shallows`) | **Keep `to_screen`** — sea level; but its per-cell cull needs the lift margin |
| `draw.rs` | 997-1063 (`draw_province_terrain`) | Main diff — `right_sea`/`dn_sea` become silhouette flags; per-cell cull margin |
| `draw.rs` | 1050-1058 | The `ISO_SAND` beach branch — the cells that actually silhouette (§4) |
| `draw.rs` | 1068 (`land_diamond`) | Take lift; colour math unchanged |
| `draw.rs` | **1100 (`draw_province_features`)** | Province name-label anchor lives **here**, not in `draw_world` as v1 said |
| `draw.rs` | 1154 / 1175 | `draw_forest_iso`, `draw_road_iso` → `to_land` |
| `draw.rs` | 1202 / 800 / 1321 | `draw_province_aggregate`, `draw_idle_coin`, `draw_settlement` → `to_land` |
| `draw.rs` | 1251 (`iso_block`) | Reference — reuse its wall-shading factor for `cliff_shade` |
| `draw.rs` | 1303 / 1382 | `draw_breach`, `draw_city` → `to_land` |
| `draw.rs` | 1515 (`draw_coast`) | **Keep `to_screen`** — harbours/gates moor in water |
| `draw.rs` | 1617 (`draw_island_terrain`) | **Only the sand body (1659) → `fill_prism`.** 1639/1640 are the `SHALLOWS_DEEP`/`SHALLOWS` ring — **sea level, stay flat** |
| `draw.rs` | **1666-1773 (`draw_island_features`)** | **→ `to_land`. Absent from v1** — every structure mark, isle label, count badge, legend box |
| `draw.rs` | 1794 (`MinimapLayout`) | Stays flat |
| `draw.rs` | 2001-2015 (`roundtrip` helper) | Breaks on compile (Camera literal); its invariant changes meaning under a lift |
| `draw.rs` | 2036 / 2133 | Test templates: `overlay_default_is_terrain`, `minimap_iso_roundtrips` |
| `menu.rs` | 20-38 (`MenuAction`) | Add `SetMapStyle(MapStyle)` |
| `menu.rs` | 40-43 (`MenuCtx`) | Add `style`; update the doc comment |
| `menu.rs` | 97 / 127 (`menus`) | `MAP STYLE` header + radio items in View, mirroring `MAP OVERLAY` |
| `panels.rs` | 130 / 143 / 314 | `region_lines` takes a `Hit`; resolves `coast_at` on sea, `region_at` on land |
| `sidebar.rs` | 406-426 | STATUS `view: …` readout — decide whether style is named (§9) |
| `prefs.rs` | 3-4 (module doc) | Doc **enumerates** the persisted fields — add map style or it drifts |
| `prefs.rs` | 19-23 (`Prefs`) | Add `map_style: Option<String>`; **leave `PREFS_VERSION` at 1** (`#[serde(default)]`) |
| `prefs.rs` | 113-117 / 126-135 | Existing struct literal + partial-field test |
| `main.rs` | 77 (`Args`) | Add `--map-style` |
| `main.rs` | 1879 / 2290 | The two hit-test sites — compute `hit()` once, pass to `panel_for` / `region_lines` |
| `main.rs` | **2183** | The real `draw_world` call site (v1 said 538 — that's inside `main`'s preamble) |
| `main.rs` | 2370-2371 | Map-title cartouche subtitle |
| `main.rs` | 2800-2803 | The live colourblind toggle — the pattern for `SetMapStyle` |
| `main.rs` | 3496-3502 | `prefs::save` literal |
| `main.rs` | 3634-3640 (`panel_for`) | Takes a `Hit` |
| `theme.rs` | 23-33 | Fix the stale *"call once at startup"* comments (§1) |
| `theme.rs` | new | `cliff_shade(top: Color) -> Color` (§4) — **not** a change to `iso_terrain_pair` |
| `almanac.rs` | 460 / 551 | Field-guide sync is a CLAUDE.md rule — persistence sentence + menu-bar paragraph |
| `hack/gui-smoke.sh` | 46-52 | Add a `map-style-relief` state — the project's only render crash gate |
| `CHANGELOG.md` | 9 | `[Unreleased]` entry |
| `CLAUDE.md` | decisions | Decision-log entry + Controls list |
| `README.md` | 236 / 580 | Feature prose + CLI flag list |
| `Cargo.toml` | 10 | Version bump (minor — new user-facing behaviour) |

---

## 8. Sea level vs. land level

Rule: **does this thing stand on land, or float at sea level?**

| Keep `to_screen` | Switch to `to_land` |
|---|---|
| Shallows ring (incl. the island ring) | Terrain tiles, beaches |
| Coast markers (harbours, gates) | Forests, daemonset roads |
| Minimap | Cities, settlements, granaries, walls |
| Ocean fill (screen-space anyway) | Breach notches, idle coins |
| Chrome (banners, titles) | Province / continent / isle labels |
| | **The selection diamond** |
| | **Island structures, badges, legend boxes** |
| | Province aggregate badges |

`draw_blast` spans both columns in one closure — split it by `Affected` kind
(cities land, harbours/gates sea), reusing `affected_cell` (draw.rs:828).

A misplaced prop is visually obvious, so the sweep is low-risk — **but a
misplaced *hit-test* is silent.** That asymmetry is why §3 is a design change and
this table is only a sweep.

---

## 9. Notes and gotchas

**Depth ordering is safe, and for a stronger reason than v1 gave.** `L` and tile
height both scale with zoom, so the ratio is a **fixed 7/16 ≈ 44% of a tile
height at every zoom** — v1's "if you push past a full tile height, re-sort by
`wx + wy`" is unreachable by zooming; it only matters if you raise `land_lift`
itself past ~16.0. Cliffs hang only over water (`draw_province_terrain`'s doc,
994-996: `coast.y0`/`coast.h` are *continent* extents, so inter-province band
seams stay interior land). Only cliff-over-cliff can seam, invisibly at 7px.

**Culling must learn the lift.** The per-province (917-932), shallows (977-984)
and per-cell terrain (1037-1043) culls use unscaled `TILE_W`/`TILE_H` margins and
test the *sea-level* point. Left alone, land at the bottom edge is culled while
its lifted top face is still visible — from zoom ~1.07, i.e. **below the default
`--zoom 1.4`**. Add `lift_px()` to the margin.

**No stale-cache hazard today.** `Coast::new` is rebuilt every frame inside
`draw_world` (draw.rs:702), so a mid-run style change cannot leave stale geometry.
Preserve this: a style-keyed geometry cache would need invalidating on change.

**No `PREFS_VERSION` bump.** `Prefs` is `#[serde(default)]` (prefs.rs:22) and the
version doc says bump on an *incompatible* change. An added `Option<String>`
yields `None` on an old file → default style. Leave it at `1`.

**Prefs are saved at exit,** once, after the render loop (main.rs:3496) — and
skipped under `--screenshot`. Do not add a save-on-change path; just include the
field in that one literal.

**STATUS readout — decide, don't skip.** `sidebar.rs:406` labels a non-default
overlay (`view: …`) precisely so a recoloured map isn't misread as NotReady, and
README.md:236 promises it. Relief is self-evident in a way a recolour is not, so
*not* labelling it is defensible — but record the decision.

**Verification.** Use the existing capture path; note the flag is `--pan-dx`
(clap kebab-cases it), not `--pan_dx`:

```
--map-style plain  --center <node> --zoom 1.4 --screenshot plain.png
--map-style relief --center <node> --zoom 1.4 --screenshot relief.png
```

Frame a coastline for the cliff faces and an island for the sand-body case.

---

## 10. Acceptance checklist

- [ ] Style switches from the View menu with a radio check mark, mid-session, no restart
- [ ] Choice survives a restart via `prefs.json`
- [ ] `--map-style` overrides the saved value for that run
- [ ] An unrecognised persisted or flag value falls back to the default
- [ ] **Clicking a city selects the correct city under both styles** (land plane)
- [ ] **Clicking a harbour still opens its city under both styles** (sea plane — the §3 trap)
- [ ] Hover tooltip and SELECTION name the same feature the pointer is over, both styles
- [ ] Warm-standby / pair scene renders in the same style as the primary (`shifted()`)
- [ ] Colour-blind palette composes with each style (`cliff_shade` derivation)
- [ ] **Every overlay** (terrain / pressure / replicas / namespace / walls / saturation / cost) has correctly-coloured cliffs
- [ ] No prop floats above or sinks below its tile in either style
- [ ] No land pops in/out at the screen edges while panning at `--zoom 1.4`
- [ ] `make lint test` green (**`cargo test` — this repo does not use `nextest`**), including the `cell_at_land` round-trip at non-zero lift
- [ ] `make gui-smoke` green, with a new `map-style-relief` state

---

## 11. Estimate

| Phase | v1 | v2 |
|---|---|---|
| 0 — Seam, two-plane hit-test, menu, prefs, CLI, tests | ~1 day | ~1.5 days |
| 1 — Relief terrain, `cliff_shade`, `to_land` sweep, culls | ~1 day | ~1.5 days |
| 2 — Islands, island features, framing, almanac/docs | ~0.5 day | ~1 day |
| **Total** | **~2.5 days** | **~4 days** |

The delta is the two-plane hit-test (a design change, not a line), the
derive-don't-tabulate cliff palette, and a sweep roughly twice the advertised size
(28 `to_screen` sites to classify, not the ~12 v1 named).

Hex (Phase 3) remains 1-2 weeks on top and needs a geometry abstraction; sprite-art
fidelity is a separate asset-pipeline project. Neither is blocked by this work.
