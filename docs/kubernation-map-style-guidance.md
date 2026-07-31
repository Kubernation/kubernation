# KuberNation — Runtime-Selectable Map Rendering Styles

**Implementation guidance**
**Goal:** make map rendering style a first-class configuration capability, selectable by the user at runtime.

> **Status:** Phases 0–2 have shipped. `MapStyle::{Plain, Relief}` is live with menu, prefs, CLI, `fill_prism`, `cliff_pair`, and tests (`only_relief_lifts_the_land`, `plain_style_collapses_both_hit_planes`, `relief_keeps_sea_level_marks_clickable`, `shifted_camera_keeps_the_map_style`).
> **§1–10 below are retained as background.** The active work is **§11 — Phase 4: Contact Shadows**.

---

## 1. The decision that shapes everything else

KuberNation already has two precedents for a render-affecting mode flag. **Copy one and not the other.**

| | `colorblind` (theme.rs:27) | `Overlay` (draw.rs / menu.rs) |
|---|---|---|
| Storage | `static COLOR_MODE: AtomicU8` | enum, passed as a parameter |
| When set | Startup only — its own doc says *"call once at startup, before any draw"* | Any frame |
| User control | `--colorblind` flag + a View-menu toggle | View menu radio group |
| Persistence | `Prefs.colorblind: bool` | `Prefs.overlay: Option<String>` |
| Round-trip | — | `overlay_from_str` ⇄ `Overlay::label` |
| Tested | `colorblind_palette_swaps_meaning_greens_to_blue` | `overlay_default_is_terrain` |

**Model `MapStyle` on `Overlay`.** The `--overlay` flag's own doc comment states the target pattern outright: *"Set from the View menu at runtime; flag is for shots."* That is precisely the capability being asked for, and the codebase already has a complete, tested, persisted implementation of it to mirror.

Do **not** reach for the atomic-global pattern. It is documented as startup-only, and a global is the wrong home for values that participate in the inverse projection (see §3).

---

## 2. Three-part split

A "map style" is not one thing. Splitting it correctly up front is what keeps hex viable later without a rewrite.

```
                    MapStyle  (enum — the user-facing selection)
                        │
        ┌───────────────┴───────────────┐
        │                               │
   GEOMETRY knobs                  PALETTE knobs
   → live on Camera                → live in theme.rs
   (lift, tile shape)              (terrain pairs, cliff colours)
   Must stay in sync with          Follow the existing `cb_*`
   to_screen / cell_at             funnel convention
```

- **Geometry → `Camera`.** Anything that changes where a world cell lands on screen must also change `cell_at`, or clicks silently miss by the offset. Keeping them on the same struct makes desync a compile-time concern rather than a bug hunt.
- **Palette → `theme.rs`.** Same shape as `cb_land` / `iso_terrain_pair`: one funnel function per colour role, style checked inside.
- **Selection → app state**, threaded exactly like `Overlay`, and *pushed into* Camera and theme when it changes.

### Naming

Recommend `MapStyle::{Plain, Relief}`.

*Relief map* is a real cartographic term for raised terrain, which fits the project's naming voice (Charter, Annals, Almanac, Wonders). `Plain` carries a useful double meaning. Two names to avoid: `Flat` collides mildly with the existing `draw::overlay_flat` (draw.rs:229), and `Chart` collides with `Charter` (the RBAC matrix).

---

## 3. The one real correctness trap

If land tops lift by *N* px, the inverse projection must undo it or **every click lands on the wrong tile**. This is the single most important line in the change:

```diff
     pub fn cell_at(&self, screen: Vec2, bounds: (u16, u16)) -> Option<(u16, u16)> {
         let (hw, hh) = self.cell_px();
         let a = (screen.x + self.pos.x) / hw;
-        let b = (screen.y + self.pos.y) / hh;
+        let b = (screen.y + self.pos.y + self.lift_px()) / hh;
```

`Camera::shifted()` must propagate the style too, or the warm-standby / pair-sync scene renders flat next to a raised primary.

---

## 4. Suggested `Camera` shape

```rust
pub struct Camera {
    pub pos: Vec2,
    pub zoom: f32,
    pub target: Option<...>,
    /// Active map style. Geometry only — palette lives in `theme`.
    pub style: MapStyle,
}

impl Camera {
    /// Land's height above sea level in SCREEN px (0.0 under `Plain`).
    /// Scaled by zoom so the extrusion tracks the tiles.
    pub fn lift_px(&self) -> f32 {
        self.style.land_lift() * self.zoom
    }

    /// Screen point for something STANDING ON LAND. Sea-level things —
    /// shallows, harbours and gates moored in water, the minimap — keep
    /// using `to_screen`.
    pub fn to_land(&self, wx: f32, wy: f32) -> Vec2 {
        let p = self.to_screen(wx, wy);
        vec2(p.x, p.y - self.lift_px())
    }
}
```

**Why this makes runtime switching almost free:** switching style is `cam.style = new;`. The next frame draws differently. No rebuild, no reload, no cache to invalidate, and `draw_world`'s signature never changes (it already receives `&Camera`).

---

## 5. Phasing

Land the *capability* before the *pixels*. Phase 0 ships a fully working runtime selector with zero visual risk, which de-risks everything after it.

### Phase 0 — The seam (no visual change)

`MapStyle::Relief` is implemented as a no-op identical to `Plain` (`land_lift()` returns `0.0` for both). Everything else is real: menu, prefs, CLI, tests.

- [ ] `MapStyle` enum + `land_lift()` + `label()` + `map_style_from_str()`
- [ ] `Camera.style` field, `lift_px()`, `to_land()`, `cell_at` fix, `shifted()` propagation
- [ ] `MenuAction::SetMapStyle(MapStyle)` + `MenuCtx.style` + a `MAP STYLE` header/radio group in the View menu
- [ ] `Prefs.map_style: Option<String>` + save on change
- [ ] `--map-style plain|relief` on `Args`
- [ ] Tests: default-is-Plain, `from_str` round-trip, `cell_at` round-trip with a non-zero lift

**Exit criteria:** flipping the menu item, restarting, and confirming the choice persisted — all with a pixel-identical map.

### Phase 1 — Relief terrain

- [ ] `fill_prism` beside `fill_diamond`
- [ ] `draw_province_terrain` passes `right_sea` / `dn_sea` as the silhouette flags
- [ ] `land_diamond` takes the lift
- [ ] `to_screen` → `to_land` sweep across land-standing draws (§7)
- [ ] `MapStyle::Relief.land_lift()` returns a real value (start at `7.0` unzoomed)

### Phase 2 — Coverage and polish

- [ ] Islands (`draw_island_terrain` — both south edges open unconditionally)
- [ ] `fit` / `fly_to` framing accounts for the lift
- [ ] Cliff palette funnelled through `theme.rs` with a colour-blind variant
- [ ] Minimap stays deliberately flat — confirm and comment

### Phase 3 — Out of scope, do not preclude

Hex topology. Phase 0's `MapStyle` should be the enum a `Hex` variant slots into. Do **not** build a `trait MapGeometry` yet — there is no second geometry to abstract over until then, and premature indirection will cost you.

---

## 6. Change inventory

| File | Line | Change |
|---|---|---|
| `draw.rs` | ~289–335 (`impl Camera`) | Add `style`; add `lift_px()`, `to_land()`; fix `cell_at`; propagate in `shifted()` |
| `draw.rs` | 402 (`fill_diamond`) | Add `fill_prism` beside it; reuse `diamond_pts` |
| `draw.rs` | 660 (`draw_world`) | **No signature change** — style rides on `&Camera` |
| `draw.rs` | 939 (`draw_province_shallows`) | **Keep `to_screen`** — sea level |
| `draw.rs` | 997 (`draw_province_terrain`) | Main diff — `right_sea`/`dn_sea` become silhouette flags |
| `draw.rs` | 1068 (`land_diamond`) | Take lift; colour math unchanged |
| `draw.rs` | 1154 / 1175 | `draw_forest_iso`, `draw_road_iso` → `to_land` |
| `draw.rs` | 1251 (`iso_block`) | Reference only — reuse its wall-shading convention |
| `draw.rs` | 1303 / 1382 | `draw_breach`, `draw_city` → `to_land` |
| `draw.rs` | 1515 (`draw_coast`) | **Keep `to_screen`** — harbours/gates moor in water |
| `draw.rs` | 1617 (`draw_island_terrain`) | 3× `fill_diamond` → `fill_prism`, both edges open |
| `draw.rs` | 1794 (`MinimapLayout`) | Stays flat |
| `draw.rs` | 2036 / 2133 | Test templates: `overlay_default_is_terrain`, `minimap_iso_roundtrips` |
| `menu.rs` | 25–38 (`MenuAction`) | Add `SetMapStyle(MapStyle)` |
| `menu.rs` | 43 (`MenuCtx`) | Add `style: MapStyle`; update the struct's doc comment |
| `menu.rs` | 97 (`menus`) | `MAP STYLE` header + radio items in View, mirroring `MAP OVERLAY` |
| `prefs.rs` | 3 (module doc) | Doc **enumerates** the persisted fields — add map style or it drifts |
| `prefs.rs` | 23 (`Prefs`) | Add `map_style: Option<String>` |
| `main.rs` | 77 (`Args`) | Add `--map-style`, documented in the house style (see §8) |
| `main.rs` | 514 (`overlay_from_str`) | Add `map_style_from_str` alongside |
| `main.rs` | 538 | `draw_world` call site — the seam |
| `theme.rs` | 151 (`iso_terrain_pair`) | Cliff colours; funnel through a `cb_*`-style helper |

Also sweep to `to_land`: `draw_province_aggregate`, `draw_idle_coin`, `draw_settlement`, and the province/continent label anchors inside `draw_world`.

---

## 7. Sea level vs. land level

The `to_screen` → `to_land` decision rule: **does this thing stand on land, or float at sea level?**

| Keep `to_screen` | Switch to `to_land` |
|---|---|
| Shallows ring | Terrain tiles, beaches |
| Coast markers (harbours, gates) | Forests, daemonset roads |
| Minimap | Cities, settlements, granaries, walls |
| Ocean fill | Breach notches, idle coins |
| | Province / continent labels |

Getting one wrong is visually obvious immediately (a floating or sunken prop), so this sweep is low-risk despite touching many call sites.

---

## 8. Notes, gotchas, and things already in your favour

**Depth ordering is already safe.** `draw_province_terrain`'s own doc comment records that `coast.y0`/`coast.h` are *continent* extents, so inter-province band seams stay interior land. Cliffs therefore only ever hang over water — never over another province's tops. The existing row-major iteration needs no change. Only cliff-over-cliff can seam, and at ~7px it is invisible. **If you push the lift past a full tile height, pass 1 needs re-sorting by `wx + wy`.**

**No stale-cache hazard today.** `Coast::new` is rebuilt every frame inside `draw_world`, so a mid-run style change cannot leave stale geometry. Preserve this: if you ever add a style-keyed geometry cache, invalidate it on style change.

**No `PREFS_VERSION` bump needed.** `Prefs` is `#[serde(default)]` and the version doc says bump on an *incompatible* schema change. Adding an `Option<String>` is backward compatible — an old file yields `None`, which falls back to the default style. Leave it at `1`.

**Camera framing.** `fit` and `fly_to` targets computed under `Plain` will be off by the lift under `Relief`. Imperceptible at 7px; revisit if a future style lifts further.

**CLI flag convention.** `Args` documents flags in a consistent voice, and dev-verification flags say so explicitly. Match `--overlay`'s phrasing: *"Set from the View menu at runtime; flag is for shots."*

**Verification.** The project already has a `--screenshot` capture path plus `--center` / `--zoom` / `--pan_dx` for framing. Use them for visual diffing:

```
--map-style plain  --center <node> --zoom 1.4 --screenshot plain.png
--map-style relief --center <node> --zoom 1.4 --screenshot relief.png
```

Frame a coastline for the cliff faces and an island for the both-edges-open case.

---

## 9. Acceptance checklist

- [ ] Style switches from the View menu with a radio check mark, mid-session, with no restart
- [ ] Choice survives a restart via `prefs.json`
- [ ] `--map-style` overrides the saved value for that run
- [ ] Unrecognised persisted or flag value falls back to the default (mirrors `overlay_from_str`)
- [ ] Clicking a city selects the correct city under **both** styles (the `cell_at` trap)
- [ ] Warm-standby / pair-sync scene renders in the same style as the primary (`shifted()`)
- [ ] Colour-blind palette composes correctly with each style
- [ ] Every overlay (terrain / pressure / replicas / namespace / walls / saturation / cost) renders correctly under each style
- [ ] No prop floats above or sinks below its tile in either style
- [ ] `cargo nextest` green, including the new `cell_at` round-trip with non-zero lift

---

## 10. Estimate

| Phase | Effort |
|---|---|
| 0 — Seam, menu, prefs, CLI, tests | ~1 day |
| 1 — Relief terrain + `to_land` sweep | ~1 day |
| 2 — Islands, framing, palette, polish | ~0.5 day |
| **Total to a shipped, runtime-selectable second style** | **~2.5 days** |

Hex (Phase 3) is 1–2 weeks on top and needs a geometry abstraction; the reference sprite-art fidelity is a separate, much larger asset-pipeline project. Neither is blocked by this work — and Phase 0 is what makes them cheap to add later.

---

# 11. Phase 4 — Contact Shadows

**Goal:** ground standing objects on the terrain they sit on. The cheapest 3D cue available, and it improves `Plain` as well as `Relief`.

## 11.1 Light direction is already committed

Two places in `draw.rs` pin the light down. Shadows must agree with them, or the scene reads as subtly wrong in a way nobody can name:

| Source | What it establishes |
|---|---|
| `iso_block` (~1251) | *"front-left in shadow, front-right sunlit"* |
| `fill_prism` (569) | E→S face gets `sunlit`, S→W face gets `shadow` |

**Light comes from the east. Contact shadows therefore offset west (screen `−x`).** Keep the offset small — this is an ambient grounding pool, not a cast shadow. `1.5 * zoom` is enough to register.

## 11.2 The helper

```rust
/// A contact shadow: the ambient pool a standing object casts where it meets
/// the ground. An ELLIPSE, not a circle — a ground circle projects to the iso
/// plane squashed by hh/hw, so derive the ratio from `cell_px` rather than
/// hardcoding 0.5. That keeps it honest if the geometry ever changes.
///
/// Offset west, matching the light `iso_block` and `fill_prism` already commit
/// to. `r` is an UNZOOMED radius; zoom is applied here.
fn contact_shadow(base: Vec2, r: f32, cam: &Camera) {
    let a = cam.style.shadow_alpha();
    if a <= 0.0 {
        return;
    }
    let (hw, hh) = cam.cell_px();
    let rx = r * cam.zoom;
    draw_ellipse(
        base.x - 1.5 * cam.zoom,
        base.y,
        rx,
        rx * (hh / hw),
        0.0,
        Color { a, ..CONTACT_SHADOW },
    );
}
```

`MapStyle` grows a second knob beside `land_lift()`:

```rust
/// Contact-shadow opacity. Relief carries slightly more — taller objects need
/// more grounding — and a schematic style would return 0.0 to suppress them.
pub fn shadow_alpha(self) -> f32 {
    match self {
        MapStyle::Plain => 0.13,
        MapStyle::Relief => 0.20,
    }
}
```

This is the first knob on `MapStyle` that is *not* geometry, which is worth having before considering a third style — it's the evidence the enum generalizes on both axes rather than being a lift-shaped special case.

`CONTACT_SHADOW` goes in `theme.rs` as a neutral dark. It needs **no colour-blind variant**: shadow is a depth cue, not a meaning channel, so it does not belong in the `cb_*` funnel. Say so in the doc comment or someone will add one later.

## 11.3 One pool per settlement, not one per block

**This is the trap.** `draw_settlement` (1535) stacks up to six `iso_block`s at overlapping offsets. Per-block shadows would compound alpha into a dark blob at the cluster centre. Draw **one** pool before the blocks, sized from the actual `blk` offsets in that function:

| Tier | Radius | Derived from |
|---|---|---|
| 0 | 9.0 | single block, `w: 13` |
| 1 | 14.0 | `dx` spans −6…+6 |
| 2 | 18.0 | `dx` spans −8…+10, `w` up to 15 |
| 3 | 22.0 | matches `draw_city_wall`'s `hw: 22` exactly |

Centre it at `c + (0, 4.0 * z)` — the blocks skew south (`dy` runs up to 8). For tier 3 it goes **before** `draw_city_wall(c, z)` so the wall sits inside its own pool.

Trees are the opposite case: sparse, non-overlapping, so a per-tree pool inside `draw_tree` (~1348) at its `base` with `r ≈ 3.0` is correct and simpler.

## 11.4 Where it goes — and where it must not

The rule: **contact shadows go on architecture standing on terrain. Marks and glyphs do not get them.**

| Function | Line | Shadow? | Why |
|---|---|---|---|
| `draw_tree` | ~1348 | **Yes** | per tree, at `base`, r ≈ 3.0 |
| `draw_settlement` | 1535 | **Yes** | one pooled, by tier (§11.3) |
| `draw_island_features` structures | ~1617+ | **Yes** | they stand on land |
| `draw_granary` | 1782 | **No** | see §11.5 — already has a backing disc |
| `draw_job` / `draw_cronjob` | 1800+ | **No** | glyph marks, not buildings |
| `draw_breach` | 1515 | **No** | sits on the settlement, already inside its pool |
| `draw_idle_coin` | — | **No** | a cost-overlay mark, not a thing on the ground |
| `draw_province_aggregate` | — | **No** | world-scale map furniture |
| `draw_coast` harbours / gates | 1515+ | **No** | moored in water — no ground to contact |

LOD mostly handles itself: `draw_forest_iso` already returns early at `Scale::World`, and settlements become aggregates at that scale.

> **Correction to earlier advice:** an initial pass at this suggested giving `draw_granary` a shadow. That was wrong — see below.

## 11.5 The `draw_granary` scrim (do not double up)

`draw_granary` already opens with:

```rust
draw_circle(c.x, c.y, u * 1.5, Color::new(0.04, 0.06, 0.10, 0.5));
```

That is a **legibility scrim**, not a contact shadow, and the difference matters:

| | Scrim (existing) | Contact shadow (new) |
|---|---|---|
| Purpose | Keep a thin cyan/yellow mark readable over busy terrain | Ground an object on the surface |
| Shape | True circle | Iso-squashed ellipse |
| Position | Centred on the glyph | At the object's base |
| Alpha | 0.5 — deliberately strong | 0.13–0.20 — deliberately subtle |

Adding a shadow under the granary would stack a 0.20 pool under a 0.5 disc and read as mud. **Leave it alone.**

Worth noting: the scrim is a one-off. `draw_job` has no equivalent, so this is not an established convention you need to extend — just an existing element to avoid colliding with. If the inconsistency bothers you, that's a separate cleanup, not part of this change.

## 11.6 Two things to watch

**Draw ordering.** Props are drawn sorted by `x + y`, so a nearer prop's shadow can darken a farther prop's body where they overlap. Keeping each radius inside its own prop's footprint (the §11.3 table does) makes this a non-issue — which is precisely why the radii should not be generous. If a dense city column looks smudged, the radii are too big, not the alpha.

**`draw_ellipse` availability.** It lives in `macroquad::shapes` in 0.4.x. If the pinned version predates it, `draw_poly` with ~24 sides plus a manual y-squash is the drop-in replacement.

## 11.7 Verification

This is a visual change; most of it is not unit-testable, and pretending otherwise wastes the session.

**Do test:**
- `shadow_alpha()` returns 0.0 for no current style (guards a future schematic style being added carelessly)
- `Relief` alpha > `Plain` alpha — mirrors the shape of `only_relief_lifts_the_land`

**Do not** try to assert on rendered pixels. Use the existing capture path instead:

```
--map-style relief --center <node> --zoom 1.4 --screenshot relief-shadows.png
--map-style plain  --center <node> --zoom 1.4 --screenshot plain-shadows.png
```

Frame a **tier-3 city** — the walled keep is where the single-pool decision either works or obviously doesn't. Then frame a forested province for the per-tree case, and an island for `draw_island_features`.

Also capture one shot under a dark overlay (`--overlay pressure` on a hot node) to confirm the shadow doesn't vanish or turn to mud against a saturated tile colour. Shadows over dark terrain being nearly invisible is physically correct and fine; shadows turning the tile to sludge is not.

## 11.8 Acceptance checklist

- [ ] Every tree, settlement, and island structure sits on a visible pool at `Relief`
- [ ] Tier-3 keep has exactly **one** pool, aligned to the wall footprint, not six stacked
- [ ] Pools are elliptical and match the tile's squash — not circular
- [ ] Shadows offset **west**, consistent with `iso_block` and `fill_prism`
- [ ] `Plain` gets subtler pools; neither style looks smudged at any zoom
- [ ] Granary keeps its scrim and gains no second disc
- [ ] Harbours and gates in open water have no pool
- [ ] No shadow appears at `Scale::World`
- [ ] Readable under all seven overlays, including the darkest
- [ ] `cargo nextest` green

## 11.9 Estimate

**Half a day.** The helper and the `MapStyle` knob are an hour; the rest is placing radii and looking at screenshots until the settlement pools sit right.

## 11.10 What this unlocks next

Contact shadows are the first item of the "sell the height you already have" track. Once they land, the remaining cheap wins in the same vein are:

1. **Surf line** at the cliff base — a light band where cliff meets water. Sells vertical scale better than a taller cliff.
2. **Cliff strata** — one or two darker horizontal bands across the wall face, derived through `cliff_pair` the same way the faces already are.
3. **Sea depth gradation** — now that land visibly sits above water, the ocean can carry more than the current `SHALLOWS_DEEP`/`SHALLOWS` pair.

The larger item beyond those is **per-province elevation** driven by `Province.tile.saturation`, which is unusually cheap here: `build_world` stacks provinces as bands sharing `x` and `w` with `y += h`, so iterating `cont.provinces` in order is *already* back-to-front and needs no re-sorting. Per-**cell** elevation is the version to avoid — it creates interior cliffs on arbitrary neighbours, breaking the invariant `MapStyle::land_lift`'s doc comment depends on.
