# KuberNation — Hit-Test Correctness + Hover Marker

**Implementation guidance**
**Goal:** make the map's click targets correct, then make them legible before the click.

---

## 0. Corrections to earlier assumptions

Two things I asserted in discussion were wrong. Both matter for scoping.

**Hover already exists.** `panels.rs` has a `// --- hover tooltip ---` section, and `draw_tooltip` is called from `main.rs:543` — immediately after `draw_world`. The per-frame pointer → region resolution **already runs every frame**. I searched for symbols named `hover`/`cursor`, found none, and wrongly concluded the feature was absent.

What this means: the state you'd need is already computed and already in scope at the call site. **The missing half is purely the map-side marker.** That makes Part B considerably cheaper than a from-scratch hover system.

**Selection outlines are not worth building.** `panel_size` is `(sw - 80).clamp(900, 1100) × (sh - 80).clamp(560, 1000)`, centered — on a default 1380×860 window it covers essentially the whole map. A selection outline would be visible only between the click and the window paint. Dropped from scope.

---

## 1. The plumbing as it stands

```
pointer → Hit { land: Option<(u16,u16)>, sea: Option<(u16,u16)> }
        → draw::locate_hit(worlds, hit) → (&SceneWorld, local (x,y))
        → sw.world.coast_at(x, y)   → harbour/gate wins if present
        → sw.world.region_at(x, y)  → Region::{Ocean|Province|City|Island|Structure}
```

Two consumers run this sequence:

| Consumer | File | Purpose |
|---|---|---|
| `panel_for` | `main.rs:3658` | click → `Panel::City` / `Panel::Node` |
| `region_lines` | `panels.rs:126` | hover tooltip **and** sidebar SELECTION text |

**They each reimplement the probe order independently.** That is the first thing to fix — see §2.3.

---

## 2. Part A — Hit-test correctness (no rendering)

### 2.1 A city's hit region is text-shaped

```rust
let label_w = (c.r.name.len() as u16 + 2).max(6);
if (y == c.y || y == c.y + 1) && x >= c.x && x < c.x + label_w {
    return Region::City(p, c);
}
```

Two rows tall, as wide as the **workload's name**, extending east from `(c.x, c.y)`. A 20-character name yields a 2 × 22 cell hit box. This is an ASCII-map leftover: the city once *was* its label text.

The city is drawn as a settlement centred on `(c.x + 0.5, c.y + 0.5)`, with the pixel radii the contact-shadow work established (9 / 14 / 18 / 22 by tier). Against a half-tile width, that is roughly **one cell**, reaching about 1.5 cells at tier 3. The hit box and the drawn object have never agreed.

**Fix:** make the city region the settlement's own cell plus a one-cell forgiveness ring. Do not derive it from the name.

**Name the regression honestly:** this shrinks the target from up to ~22 cells wide to ~3. That is correct — the target now matches the drawing — but it is a real change in feel, and Part B is what compensates by making the target visible before you commit.

**If you want the name banner clickable too:** it cannot be done in `region_at`. `draw_name_banner` places the banner at runtime via `place()` against the frame's `occupied` list, choosing between below / below-left / below-right / above depending on crowding. The model cannot express a position the view decides per-frame. If you want it, it must be a **view-layer pass over the `occupied` rects, tagged with what they label** — which is a tidy use of a vector that already exists. Recommend deferring it; do the honest small target first and see whether it actually bites.

### 2.2 Ocean inside a province rect resolves to Province

`region_at` tests the full rectangle — `y ∈ [p.y, p.y+p.h)`, `x ∈ [cont.x, cont.x+cont.w)`. It never consults `Coast`, because `Coast` lives in `draw.rs` and is generated from value noise. `kubernation-core` genuinely does not know which cells are land.

**Do not move `Coast` into core.** The coastline is a rendering concern, and relocating it would put procedural noise generation into the world model. The model/view split is right; the fix belongs on the view side.

**Fix:** gate the `Region::Province` result on a land test in the view. Order matters — `coast_at` must still be probed *first*, because harbours and gates are moored in open water east of the continent and must stay clickable.

Cost is negligible: `Coast::new` already runs per continent per frame inside `draw_world`, so building one on demand for a probe is nothing.

### 2.3 Unify the two probes

Both consumers do `coast_at` → `region_at` in their own code. That means the tooltip and the click can drift out of agreement, and any fix applied to one silently misses the other.

Introduce a single resolver in `draw.rs` — the crate that owns `Coast` — and have both call it:

```rust
/// What the pointer is actually over, with the land test the model cannot do.
/// The probe order is authoritative and lives here ONCE: a coast marker (moored
/// in water) wins, then sea cells inside a province's footprint resolve to open
/// sea, then the model's region. `panel_for` and `region_lines` must both route
/// through this or the tooltip and the click will disagree.
pub fn resolve_region<'a>(sw: &'a SceneWorld, local: (u16, u16)) -> Resolved<'a>;
```

This is the highest-value part of Part A. The two bugs above are symptoms; the duplicated probe order is the cause of them being able to diverge.

### 2.4 Tests

`region_lines_name_the_workload_under_a_city` (panels.rs:1151) probes at exactly `(cx, cy)` — the city's origin cell — so it **survives** the fix unchanged. Note that it never probes the `label_w` tail, which is precisely why this went unnoticed. Worth saying in the commit message.

Add, in core (pure, cheap):
- [ ] the settlement's own cell resolves to `Region::City`
- [ ] a cell 10 east of a city on the same row resolves to `Region::Province`, not `City`
- [ ] a long workload name does not widen the city's region

Add, in the view:
- [ ] a sea cell inside a province's rect resolves to open sea, not `Province`
- [ ] a coast marker still wins over the land probe (regression guard for the ordering)
- [ ] `panel_for` and `region_lines` agree on the same probe point — the anti-drift test

---

## 3. Part B — Hover marker

### 3.1 Insertion point

`main.rs:543` already calls `draw_tooltip(sw, local, snap, overlay, mouse)` with `sw` and `local` in hand, under whatever guard keeps the tooltip off chrome and modals. **Put the marker call adjacent to it, under the same guard.** Do not invent a second gating path.

Draw the marker *before* `draw_tooltip` so the tooltip's stone panel stays on top.

### 3.2 Plane discipline

`locate_hit` returns the cell on the plane its feature is drawn on. Re-check `coast_at` to pick the projection, mirroring exactly what `region_lines` already does:

```rust
let p = if sw.world.coast_at(local.0, local.1).is_some() {
    cam.to_screen(local.0 as f32 + 0.5, local.1 as f32 + 0.5) // moored in water
} else {
    cam.to_land(local.0 as f32 + 0.5, local.1 as f32 + 0.5)   // stands on land
};
```

Getting this wrong detaches the marker from what it marks under `Relief`, and looks fine under `Plain` — so verify in `Relief`.

### 3.3 Phase B1 — cell marks

For `City`, `Structure`, and coast markers, stroke a diamond at the resolved cell. `draw_blast` already establishes the idiom (`stroke_diamond(c, hw * 1.05, hh * 1.05, 2.0, col)`).

**This phase is only honest after Part A.** Before the fix, a city's region is a 2 × `label_w` rect, so a single diamond would misrepresent it. After the fix the region genuinely is about one cell, and the diamond tells the truth.

**Do not reuse `CRIT` or `WARN`, and do not pulse.** Those belong to `draw_blast` and mean danger. Hover is ambient, not an alert: a neutral bright colour, thinner stroke, no animation. If hover and blast are ever on screen together they must be instantly distinguishable.

### 3.4 Phase B2 — province outline

Marking one cell does not teach you a province's boundary, and that boundary is the thing users currently cannot see. For `Region::Province`:

- walk `coast.land_span(wy, prov.w)` for `wy ∈ [p.y, p.y + p.h)`
- emit the west and east edge points per row; close north and south
- project via `to_land`; under `Relief` the striking version drops the south/east silhouette down the cliff faces

Provinces share `x` and `w` and stack in `y`, so the result is a vertical strip with ragged sides — cheap to build and cheap to stroke.

This is the phase that delivers the actual value. B1 is the cheap down payment.

### 3.5 Suppression

- [ ] no marker while a `Panel` is open — the modal covers the map anyway
- [ ] no marker over chrome (menu bar, minimap, sidebar) — reuse the tooltip's guard
- [ ] no marker on `Region::Ocean`
- [ ] keep it at `Scale::World` — province marks are *more* useful zoomed out, not less

---

## 4. Sequencing

| Phase | Work | Estimate |
|---|---|---|
| **A** | `resolve_region` unification + both hit-test fixes + tests | ~0.5 day |
| **B1** | Cell marks at the hover point | ~0.5 day |
| **B2** | Province outline via `land_span` | ~1 day |

**A must come first.** It is pure, testable, needs no rendering, and it is what makes B1 an honest depiction rather than a careful drawing of a wrong shape.

B2 is the one worth doing if you only do one of the B phases — but it is also the one that can wait, since A alone already removes the wrong-modal-on-mis-click failure.

---

## 5. Acceptance checklist

- [ ] Clicking a settlement opens that workload; clicking 10 cells east of it opens the **node**, not the workload
- [ ] Workload name length no longer affects the clickable area
- [ ] Clicking visible water inside a province's footprint opens nothing
- [ ] Clicking a harbour or gate still opens the city it serves
- [ ] The tooltip and the click agree at every probe point — no cell where they name different things
- [ ] Hover marker tracks the pointer and sits on the correct plane under **both** styles
- [ ] Hover marker is visually distinct from blast marks (colour, weight, no pulse)
- [ ] No marker while a panel is open or the pointer is over chrome
- [ ] `cargo nextest` green, including the new anti-drift test
