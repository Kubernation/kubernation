# KuberNation — Closing the Hit-Test Gaps

**Implementation guidance**
**Scope:** one structural fix and two tests. Small — half a day.

---

## 1. Fix the ocean split structurally, don't test around it

`region_lines` now has two ocean paths that behave differently:

```rust
match crate::draw::resolve_region(sw, local) {
    crate::draw::Resolved::Ocean => return lines,   // ← [cluster tag] when paired
    ...
    Region::Ocean => {
        if !paired { return Vec::new(); }
        lines.push(("open sea".into(), STONE_INK_DIM));
    }
}
```

In a paired hot/warm session `lines` already holds the cluster tag by this point, so hovering **visible water inside a province's rectangle** yields a stone panel containing only `HOT <label>`. True open sea yields `HOT <label>` + `open sea`. Single-cluster sessions are unaffected — `lines` is empty and `draw_tooltip` suppresses on empty.

**Fix:** collapse the two into one path. `Resolved::Ocean` and `Region::Ocean` mean the same thing to a reader — "there is nothing here" — and should produce the same lines.

Prefer this over adding a parity test. A test would assert the two branches agree; merging them means there is only one branch and nothing to drift. `draw_hover` already got this right (`Resolved::Ocean | Resolved::Region(Region::Ocean) => {}`) — match that shape.

> **Why this slipped:** the bug only manifests when `snap.warm.is_some()`, and every existing fixture sets `warm: None`. Fixing it structurally means you never need to build a paired fixture to prove it.

---

## 2. Move `panel_for` into `panels.rs`

It currently lives in `main.rs:3681` and has no test coverage. It returns `Panel`, which is defined in `panels.rs`, and it must agree with `region_lines`, which is also in `panels.rs`.

Moving it:
- puts the two functions that must agree side by side, where an editor changing one sees the other
- makes the drift test a same-module test with no cross-module plumbing
- needs only `draw::{Hit, locate_hit, resolve_region, Resolved}` and `Region`, all already reachable from `panels.rs`

It's ~15 lines. Do this first; §3 depends on it.

---

## 3. The drift test

**Be precise about what this catches.** I overstated this in review: the drift test guards *semantic* divergence — the tooltip naming something the click won't open. It would **not** have caught the ocean bug in §1, because there both paths agree that nothing is selectable and differ only in how much text they show. §1 is fixed structurally; this test guards a different and larger class.

The invariant:

| `panel_for` returns | `region_lines` must |
|---|---|
| `Some(Panel::City(_, r))` | mention `r.name` |
| `Some(Panel::Node(_, n))` | mention `n` |
| `None` | mention no city name and no node name |

Third row is the one that bites — it's what stops a future edit making the tooltip describe a region that opens nothing.

Walk a small grid rather than probing one point:

```rust
#[test]
fn the_tooltip_and_the_click_never_disagree() {
    // …fixture…
    let worlds = scene(&snap);
    let names: Vec<String> = /* every node name + every workload name in the world */;
    for y in 0..snap_world_height {
        for x in 0..snap_world_width {
            let panel = panel_for(&worlds, hit_at(x, y));
            let lines = region_lines(&worlds[0], (x, y), &snap, Overlay::Terrain);
            let text = lines.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(" ");
            match panel {
                Some(Panel::City(_, r)) => assert!(text.contains(&r.name), "({x},{y})"),
                Some(Panel::Node(_, n)) => assert!(text.contains(&n), "({x},{y})"),
                None => assert!(
                    !names.iter().any(|n| text.contains(n)),
                    "({x},{y}) names something the click opens nothing for: {text:?}"
                ),
            }
        }
    }
}
```

Two practical notes:

- **`panel_for` takes a `Hit`, not a cell.** Either construct `Hit { land: Some(cell), sea: Some(cell) }` directly (they're equal under `Plain`, which is the default) or add a small test helper. Do not route through the camera — this test is about region logic, not projection.
- **Use a multi-node fixture.** `Coast::new` gives a single-node continent only a gentle wobble, so a one-node world may have no sea inside any province rect and the interesting cells won't exist. `province_ring_traces_the_drawn_coastline` (draw.rs:2521) already solved this — crib its fixture.

---

## 4. The carried-over `shadow_alpha` test

Outstanding for two rounds now. It's the guard that stops a future third `MapStyle` silently inheriting whatever alpha gets typed:

```rust
#[test]
fn every_style_defines_a_deliberate_shadow_alpha() {
    for s in MapStyle::ALL {
        let a = s.shadow_alpha();
        assert!((0.0..=1.0).contains(&a), "{s:?} alpha out of range");
    }
    assert!(
        MapStyle::Relief.shadow_alpha() > MapStyle::Plain.shadow_alpha(),
        "relief lifts land, so it needs more grounding than plain"
    );
}
```

Iterating `MapStyle::ALL` is what makes it a guard rather than a snapshot — a new variant is covered the moment it's added. Mirrors the shape of `only_relief_lifts_the_land`.

---

## 5. Order and acceptance

1. Move `panel_for` → `panels.rs`
2. Collapse the ocean branches
3. Drift test
4. `shadow_alpha` test

- [ ] Hovering water inside a province's rect shows the same thing as hovering true open sea, in both single and paired sessions
- [ ] Only one ocean branch remains in `region_lines`
- [ ] `panel_for` lives beside `region_lines` and has coverage
- [ ] The drift test walks a grid with at least one sea-inside-rect cell present
- [ ] `MapStyle::ALL` is iterated, not enumerated by hand
- [ ] `cargo nextest` green
