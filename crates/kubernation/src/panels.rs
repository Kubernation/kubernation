//! Chrome that floats over the world: the hover tooltip, the blast banner,
//! the context picker, and the shared helpers the drill-down windows reuse.
//! Everything is cluster-aware: in pair mode it says which world it belongs to.
//! (Detail drill-downs themselves live in `city.rs` / `node.rs`; the attention
//! queue now lives in the right column's ATTENTION section — see `sidebar.rs`.)

use kubernation_core::events::ClusterId;
use kubernation_core::state::cost::{self, CostBasis, NodeCost};
use kubernation_core::state::logline::{self, FilterExpr, Level};
use kubernation_core::state::model::{ExtentSource, NodeHealth, PodState, WorkloadRef};
use kubernation_core::state::saturation::{NodeSaturation, SatLevel};
use kubernation_core::state::world::{CoastKind, Region};
use macroquad::prelude::*;

use crate::draw::{Overlay, SceneWorld};
use crate::net::{ConnState, LogTail, Snapshot};
use crate::text::{
    mono_text, mono_text_size, name_text, name_text_size, text, text_bold, text_size,
};
use crate::theme::*;

pub const CHROME_H: f32 = 32.0;
/// Width of the docked right column (the WORLD / STATUS / SELECTION sidebar,
/// after the classic-4X right panel). The map fills everything to its left.
pub const COL_W: f32 = 264.0;

/// The right column's rect (below the top chrome, full height to the bottom).
pub fn sidebar_rect() -> Rect {
    Rect::new(
        screen_width() - COL_W,
        CHROME_H,
        COL_W,
        screen_height() - CHROME_H,
    )
}

/// The play area to the left of the column (where the map lives, now full
/// height — the attention queue moved into the column's ATTENTION section).
pub fn map_width() -> f32 {
    play_width(screen_width())
}

/// The play area's width on a screen of `sw` — the map, less the docked column.
///
/// PURE. `map_width` is this against the live screen; anything that is handed an
/// `sw` must use THIS, or it silently ignores its own argument and cannot be
/// tested at a size other than the one the window happens to be.
pub fn play_width(sw: f32) -> f32 {
    (sw - COL_W).max(0.0)
}

/// A cartographic title cartouche centered over the top of the play area —
/// classic-4X "<realm> map" labeling. `title` is the realm name (serif);
/// `subtitle` is an optional small suffix (the active map view), dimmed. A
/// small iso-diamond flourish sits at each end so it reads as a map title.
pub fn draw_map_title(title: &str, subtitle: Option<&str>, map_w: f32) {
    let fs = 21.0;
    let sub_fs = 13.0;
    let pad = 24.0;
    let sub = subtitle.unwrap_or("");
    let sw = if sub.is_empty() {
        0.0
    } else {
        text_size(sub, sub_fs).width + 12.0
    };
    // Keep the cartouche inside the play area: truncate the (serif) title to the
    // width left after padding + the subtitle, so a long context / narrow window
    // can't overdraw the right column. The realm readout does the same.
    let max_bw = (map_w - 6.0).max(60.0);
    let avail_title = (max_bw - pad * 2.0 - sw).max(0.0);
    let mut title = title.to_string();
    let mut tw = name_text_size(&title, fs).width;
    if tw > avail_title && avail_title > 0.0 {
        let budget = ((title.chars().count() as f32) * (avail_title / tw)) as usize;
        title = truncate_str(&title, budget.max(3));
        tw = name_text_size(&title, fs).width;
    }
    let bw = (tw + sw + pad * 2.0).min(max_bw);
    let bx = (map_w / 2.0 - bw / 2.0).clamp(2.0, (map_w - bw - 2.0).max(2.0));
    let by = CHROME_H + 5.0;
    let bh = 27.0;
    stone_panel(bx, by, bw, bh);

    // Iso-diamond flourishes tucked into the side padding.
    let cy = by + bh / 2.0;
    let diamond = |dx: f32| {
        let d = 4.0;
        draw_triangle(
            vec2(dx - d, cy),
            vec2(dx, cy - d),
            vec2(dx + d, cy),
            PARCHMENT,
        );
        draw_triangle(
            vec2(dx - d, cy),
            vec2(dx, cy + d),
            vec2(dx + d, cy),
            PARCHMENT,
        );
    };
    diamond(bx + 11.0);
    diamond(bx + bw - 11.0);

    let ty = by + 20.0;
    name_text(title, bx + pad, ty, fs, STONE_INK);
    if !sub.is_empty() {
        text(sub, bx + pad + tw + 12.0, ty - 2.0, sub_fs, STONE_INK_DIM);
    }
}

pub(crate) fn pod_color(s: PodState) -> Color {
    match s {
        PodState::Ok if colorblind() => Color::new(0.40, 0.66, 0.95, 1.0),
        PodState::Ok => Color::new(0.45, 0.70, 0.40, 1.0),
        PodState::Starting => Color::new(0.40, 0.75, 0.80, 1.0),
        PodState::Pending => DIM,
        PodState::Terminating => DIM,
        PodState::Failing => CRIT,
        PodState::Succeeded => Color::new(0.55, 0.55, 0.50, 1.0),
    }
}

fn cluster_tag(id: ClusterId) -> (&'static str, Color) {
    match id {
        ClusterId::Hot => ("HOT", Color::new(0.95, 0.65, 0.35, 1.0)),
        ClusterId::Warm => ("WARM", Color::new(0.55, 0.78, 0.92, 1.0)),
    }
}

/// The drill-down a pointer hit opens. **Lives beside [`region_lines`] on
/// purpose:** the two must agree — the window a click opens has to be the thing
/// the tooltip just named — so they sit in one module where an edit to either
/// has the other in view, and the drift test between them is a same-module test.
///
/// `locate_hit` picks the plane each feature is drawn on (coast markers float at
/// sea level, everything else stands on land), then `resolve_region` applies the
/// one authoritative probe order.
pub fn panel_for(worlds: &[SceneWorld], hit: crate::draw::Hit) -> Option<Panel> {
    use crate::draw::{Resolved, locate_hit, resolve_region};
    let (sw, local) = locate_hit(worlds, hit)?;
    match resolve_region(sw, local) {
        // A coast marker opens the city it serves.
        Resolved::Coast(m) => Some(Panel::City(sw.id, m.workload.clone())),
        Resolved::Ocean => None,
        Resolved::Region(region) => match region {
            Region::City(_, c) => Some(Panel::City(sw.id, c.r.clone())),
            Region::Province(p) => Some(Panel::Node(sw.id, p.tile.name.clone())),
            Region::Structure(_, s) => s.workload.clone().map(|r| Panel::City(sw.id, r)),
            _ => None,
        },
    }
}

// --- hover tooltip ------------------------------------------------------

/// The text lines describing whatever is at `local` in `sw` — shared by the
/// hover tooltip and the right column's SELECTION panel. Empty for open sea in
/// a single-cluster session (nothing worth saying).
pub fn region_lines(
    sw: &SceneWorld,
    local: (u16, u16),
    snap: &Snapshot,
    overlay: Overlay,
    graticule: bool,
    new_ground: kubernation_core::state::layout::NewGround,
) -> Vec<(String, Color)> {
    let paired = snap.warm.is_some();
    let mut lines: Vec<(String, Color)> = Vec::new();
    if paired {
        let (tag, color) = cluster_tag(sw.id);
        lines.push((format!("{tag} {}", sw.label), color));
    }
    // This world's upkeep (for the Cost-overlay SELECTION line) — already on the snap.
    let cost = match sw.id {
        ClusterId::Hot => &snap.hot.cost,
        ClusterId::Warm => snap.warm.as_ref().map_or(&snap.hot.cost, |w| &w.cost),
    };
    // Likewise this world's DaemonSet coverage (the Substrate-overlay line) —
    // per-world, since a hot/warm pair runs different infrastructure.
    let substrate = match sw.id {
        ClusterId::Hot => &snap.hot.models.substrate,
        ClusterId::Warm => snap
            .warm
            .as_ref()
            .map_or(&snap.hot.models.substrate, |w| &w.models.substrate),
    };
    // Route through the ONE resolver so the tooltip can never name something
    // different from what a click at the same pixel would open.
    match crate::draw::resolve_region(sw, local) {
        // ONE ocean path. `Resolved::Ocean` (visible water inside a province's
        // bounding rectangle — the model reports Province, the view knows
        // better) and `Region::Ocean` (true open sea) mean the same thing to a
        // reader: there is nothing here. They must therefore SAY the same
        // thing. Two branches used to diverge in a paired session, where
        // `lines` already holds the cluster tag: sea-inside-rect showed a bare
        // "HOT <label>" panel while open sea showed "HOT <label> / open sea".
        // Merged rather than parity-tested — one branch cannot drift from
        // itself. (`draw::draw_hover` already had this shape.)
        crate::draw::Resolved::Ocean
        | crate::draw::Resolved::Region(kubernation_core::state::world::Region::Ocean) => {
            if !paired {
                return Vec::new();
            }
            lines.push(("open sea".into(), STONE_INK_DIM));
            return lines;
        }
        crate::draw::Resolved::Coast(m) => {
            // A coast marker (not a land region): the city's harbor / gate.
            let (title, what) = match m.kind {
                CoastKind::Harbor => ("harbor", format!("service {} . {}", m.name, m.detail)),
                CoastKind::Gate => ("gate", format!("ingress {} . {}", m.name, m.detail)),
            };
            lines.push((title.into(), STONE_STRUCT));
            lines.push((what, STONE_INK));
            lines.push((format!("-> {}", m.workload.name), STONE_INK_DIM));
        }
        crate::draw::Resolved::Region(region) => {
            match region {
                Region::City(p, c) => {
                    lines.push((c.r.name.clone(), STONE_INK));
                    let gap = if c.ready < c.desired {
                        STONE_WARN
                    } else {
                        STONE_INK_DIM
                    };
                    lines.push((
                        format!(
                            "{} {} . pop {}/{}",
                            c.r.kind, c.r.namespace, c.ready, c.desired
                        ),
                        gap,
                    ));
                    if let Some(sev) = c.severity {
                        lines.push(("needs attention".into(), severity_on_stone(sev)));
                    }
                    if let Some(store) = c.storage {
                        let (txt, col) = if store.pending > 0 {
                            (
                                format!("{} PVCs . {} pending", store.claims, store.pending),
                                STONE_WARN,
                            )
                        } else {
                            (format!("{} PVCs", store.claims), STONE_STRUCT)
                        };
                        lines.push((txt, col));
                    }
                    if let Some(pair) = &snap.pair
                        && let Some(st) = pair.state(&c.r)
                    {
                        lines.push((st.describe(sw.id), sync_on_stone(st)));
                    }
                    // The city sits on the tinted province — show its host node's
                    // strain / upkeep too, so the distinguisher isn't lost on the settlement.
                    lines.extend(grid_ref_line(p.reference.as_ref(), graticule));
                    lines.extend(pool_line(
                        &p.tile.pool,
                        p.tile.pool_source,
                        overlay == Overlay::Pool,
                    ));
                    lines.extend(extent_line(p.extent_source));
                    lines.extend(fresh_line(sw.fresh.get(&p.tile.name).copied(), new_ground));
                    if overlay == Overlay::Saturation {
                        lines.extend(saturation_lines(&p.tile.saturation));
                    }
                    if overlay == Overlay::Cost
                        && let Some(nc) = cost.by_node.get(&p.tile.name)
                    {
                        lines.extend(cost_lines(nc));
                    }
                    if overlay == Overlay::Substrate {
                        lines.extend(substrate_gap_lines(
                            substrate.missing(&p.tile.name),
                            substrate.has_data(),
                        ));
                    }
                }
                Region::Province(p) => {
                    lines.push((p.tile.name.clone(), STONE_INK));
                    let health = match p.tile.health {
                        NodeHealth::Healthy => ("healthy", STONE_INK_DIM),
                        NodeHealth::Cordoned => ("cordoned", STONE_WARN),
                        NodeHealth::Pressure => ("under pressure", STONE_WARN),
                        NodeHealth::NotReady => ("NotReady", STONE_CRIT),
                    };
                    lines.push((
                        format!("{} . {} pods", health.0, p.tile.pods.len()),
                        health.1,
                    ));
                    // How to say where this is. First, because a reference is
                    // what you write down or read out before anything else.
                    lines.extend(grid_ref_line(p.reference.as_ref(), graticule));
                    // Ungated, unlike the three below: fresh ground is tinted under
                    // every overlay, so its explanation must be too.
                    lines.extend(extent_line(p.extent_source));
                    lines.extend(fresh_line(sw.fresh.get(&p.tile.name).copied(), new_ground));
                    // Under the Saturation overlay, name the binding strain
                    // dimension(s) — the distinguisher the Pressure overlay lacks.
                    if overlay == Overlay::Saturation {
                        lines.extend(saturation_lines(&p.tile.saturation));
                    }
                    // Under the Cost overlay, name the node's upkeep + idle drain.
                    if overlay == Overlay::Cost
                        && let Some(nc) = cost.by_node.get(&p.tile.name)
                    {
                        lines.extend(cost_lines(nc));
                    }
                    // Under the Substrate overlay, name the missing DaemonSets —
                    // the overlay says which node, this says which infrastructure.
                    if overlay == Overlay::Substrate {
                        lines.extend(substrate_gap_lines(
                            substrate.missing(&p.tile.name),
                            substrate.has_data(),
                        ));
                    }
                }
                Region::Structure(_, s) => {
                    lines.push((format!("{}/{}", s.kind, s.name), STONE_INK));
                    if s.workload.is_some() {
                        lines.push(("encampment - no pods on any land".into(), STONE_WARN));
                    }
                }
                Region::Island(isl) => {
                    lines.push((format!("isle of {}", isl.label), STONE_INK));
                }
                // Handled by the single ocean arm above, before this match.
                Region::Ocean => {}
            }
        }
    }
    lines
}

/// PURE draw-decision fn: the per-dimension saturation breakdown for a province
/// — the strain dimensions that are non-calm (worst first), each named + colored
/// by its own level on the stone column. A fully-calm node yields one "calm"
/// line. Unit-tested (the testability policy). Conditions render "(pegged)"; an
/// omitted dimension (no honest source) simply isn't in `sat.dims`.
/// SELECTION/tooltip lines for a node's upkeep, shown under the Cost overlay.
/// PURE + unit-tested. Unitless shows "cost units" (no `$`); the idle line is the
/// actionable bit (on-stone cyan when notable, matching the map's idle coin).
pub fn cost_lines(nc: &NodeCost) -> Vec<(String, Color)> {
    if !nc.priced {
        return vec![("upkeep: unpriced".into(), STONE_INK_DIM)];
    }
    let idle = 1.0 - nc.used_frac;
    let idle_col = if idle >= cost::IDLE_NOTABLE {
        STONE_STRUCT
    } else {
        STONE_INK_DIM
    };
    let mut lines = vec![
        (
            format!("upkeep: {}", cost::fmt_monthly(nc.per_hour, nc.mode)),
            STONE_INK,
        ),
        (
            format!(
                "idle {:.0}% · {}",
                idle * 100.0,
                cost::fmt_monthly(nc.idle_per_hour, nc.mode)
            ),
            idle_col,
        ),
    ];
    if nc.basis == CostBasis::OpenCost {
        lines.push(("(from OpenCost)".into(), STONE_STRUCT));
    } else {
        if nc.basis == CostBasis::Requests {
            lines.push(("(idle est. from requests)".into(), STONE_INK_DIM));
        }
        // The only on-map $ figure carries the same honesty caveat the advisor does.
        if nc.mode == cost::CostMode::Currency {
            lines.push(("(est., not a cloud bill)".into(), STONE_INK_DIM));
        }
    }
    lines
}

/// State the frame the graticule is anchored to, bottom-left of the play area.
///
/// §3's requirement, and the half that matters more than the grid: a grid makes
/// positions LOOK meaningful, so a map that draws one and says nothing asserts
/// by implication that adjacency means something. On this map it does not —
/// column order is when a zone was first seen, row order is allocation order.
///
/// On the map rather than only in the Almanac because the gate is run by handing
/// someone a CAPTURE, and a screenshot travels without the Almanac. Drawn once
/// per frame in the chrome pass, not per world: the frame is the same statement
/// for a hot/warm pair, and saying it twice would suggest it were two frames.
pub fn draw_frame_note(on: bool) {
    if !on {
        return;
    }
    use kubernation_core::state::graticule::FRAME_DECLARATION;
    let fs = 12.0;
    let mut y = screen_height() - 10.0 - fs * FRAME_DECLARATION.len() as f32 * 1.35;
    for line in FRAME_DECLARATION {
        text(ascii(line), 12.0, y, fs, crate::theme::GRATICULE_INK);
        y += fs * 1.35;
    }
}

/// How a province's SIZE was arrived at — the marking A2's acceptance asked for
/// ("declared and marked") and which, until now, was declared only.
///
/// `None` for a measured size: a province drawn from the node's own reported
/// memory needs no caveat, exactly as a declared pool needs none (`pool_line`).
/// The two fallback rungs read differently from each other, which they did not
/// before — `InstanceType` and `Default` produced the same extent and nothing
/// distinguished them, or either of them from a genuinely mid-sized node.
///
/// **Deliberately a panel line and NOT the v1.6.0 hatch.** Three reasons, in
/// order of weight. (1) The hatch means *this reading has no denominator*, and
/// it is gated to the ratio overlays; extent is drawn under **every** overlay,
/// so hatching it would put two unrelated meanings on one texture — and on a
/// node that has both (no allocatable at all) they would be indistinguishable.
/// (2) `ExtentSource`'s own doc says it travels "exactly as `metric_source` and
/// `pool_source` do", and both of those are named in the panel, not drawn on
/// the map. (3) Extent carries no cluster state — it is scenery — so a fallback
/// size misleads about the machine, not about the cluster, which is a
/// panel-sized claim rather than a terrain-sized one.
///
/// The hatch does not cover this case anyway: `province_unmeasured` fires only
/// when `worst_known(cpu, mem)` is `None`, so a node reporting allocatable cpu
/// but not memory gets a fallback extent and no hatch at all.
pub fn extent_line(source: ExtentSource) -> Option<(String, Color)> {
    match source {
        ExtentSource::Allocatable => None,
        ExtentSource::InstanceType => Some((
            "size from instance type - not measured".into(),
            STONE_INK_DIM,
        )),
        ExtentSource::Default => Some(("size not reported - default extent".into(), STONE_INK_DIM)),
    }
}

/// PURE draw-decision fn: which nodepool holds this ground, and how that was
/// determined.
///
/// The label half of the plan's "pool identity travels by colour and label".
/// Both, not either: the colours are hashed hues with no natural order, so a
/// reader can see that two provinces belong together without being able to name
/// what they belong to. The colour groups; the label says what the group IS.
///
/// `pool_source` rides along for the same reason `metric_source` and `CostBasis`
/// do — an `instance-type` pool merges pools the provider considers distinct,
/// and a reader comparing two nodes deserves to know the grouping is inferred
/// rather than declared.
pub fn pool_line(
    pool: &str,
    source: kubernation_core::state::layout::PoolSource,
    on: bool,
) -> Option<(String, Color)> {
    use kubernation_core::state::layout::PoolSource;
    if !on {
        return None;
    }
    // An unresolvable node is not a member of anything, and saying "pool
    // unpooled" would dress the absence up as a name.
    if pool == kubernation_core::state::model::DEFAULT_POOL {
        return Some(("no nodepool label - not grouped".into(), STONE_INK_DIM));
    }
    let how = match source {
        // A declared pool needs no caveat.
        PoolSource::Override | PoolSource::Provider(_) => String::new(),
        PoolSource::InstanceType => " (from instance type)".into(),
        PoolSource::Default => " (inferred)".into(),
    };
    Some((format!("pool {pool}{how}"), STONE_STRUCT))
}

/// PURE draw-decision fn: the graticule reference for a position, e.g. `C4`.
///
/// Shown only while the frame is drawn. That gating is deliberate and is what
/// makes the gate's discrimination check meaningful (§4.1): with the frame off,
/// the app offers no naming aid at all, so a second person finding the slot
/// anyway would be demonstrating familiarity with the fleet rather than that the
/// graticule works.
///
/// `None` for a node with no durable position — never a fabricated `A0`, which
/// would name a slot some other node really holds.
pub fn grid_ref_line(
    reference: Option<&kubernation_core::state::graticule::GridRef>,
    on: bool,
) -> Option<(String, Color)> {
    if !on {
        return None;
    }
    Some(match reference {
        Some(r) => (format!("grid {r}"), STONE_STRUCT),
        // Said out loud rather than omitted: a blank where a reference belongs
        // reads as "not loaded yet", and this is a standing fact about the node.
        None => ("grid - no durable position".into(), STONE_INK_DIM),
    })
}

/// PURE draw-decision fn: the SELECTION/tooltip line for new ground. Unit-tested.
///
/// The panel half of the succession mark. The map says *which* ground; without
/// this it raises a question it cannot answer (A5-render §3.3), and the wording
/// has to be true in BOTH modes — under `Since` ground stays marked
/// indefinitely, so "just changed hands" would be a lie there.
///
/// **It speaks the unknown state, which the fill deliberately does not.** Under
/// `Since` the three answers are different claims and all three are said:
/// changed, unchanged, or *no record*, which is not the same as unchanged (see
/// [`kubernation_core::state::layout::GroundState`]). Under `Fading` there is no
/// unknown to lose — an absent record means "not recently new", full stop — so
/// settled ground stays silent there rather than repeating a non-answer on every
/// hover.
pub fn fresh_line(
    state: Option<kubernation_core::state::layout::GroundState>,
    mode: kubernation_core::state::layout::NewGround,
) -> Option<(String, Color)> {
    use kubernation_core::state::layout::{GroundState, NewGround};
    let words = match (mode, state?) {
        (NewGround::Off, _) | (_, GroundState::Unasked) => return None,
        // A fixed baseline does not decay, so there is no age to report — only
        // that this ground is on the changed side of the line.
        (NewGround::Since(_), GroundState::New(_)) => "new ground . changed since the baseline",
        // The informative one: the map HAS a record for this ground and it
        // predates the baseline. Worth saying, because it is the case the
        // reader would otherwise have to assume.
        (NewGround::Since(_), GroundState::Settled) => "unchanged since the baseline",
        // The caveat: an absent record is not evidence of absence. A baseline
        // can reach back past the point this map began keeping records.
        (NewGround::Since(_), GroundState::Unknown) => "no succession on record",
        // Deliberately relative, not a timestamp: freshness is a fraction of the
        // ageing window, and reconstructing a duration from it would state a
        // precision the fraction doesn't carry.
        (NewGround::Fading(_), GroundState::New(f)) => {
            match crate::theme::fresh_tier(f, crate::theme::FRESH_STEPS) {
                3 => "new ground . just changed hands",
                2 => "new ground . changed hands recently",
                _ => "new ground . settling",
            }
        }
        // No unknown to lose under a rolling window — see `NewGround::state`.
        (NewGround::Fading(_), GroundState::Settled | GroundState::Unknown) => return None,
    };
    Some((words.into(), STONE_INK_DIM))
}

/// PURE draw-decision fn: SELECTION/tooltip lines naming the fleet-wide
/// DaemonSets a node LACKS, shown under the Substrate overlay. Unit-tested.
///
/// Naming the specific DaemonSets is the point: the overlay says *which nodes*,
/// and without this the map raises a question it can't answer and the operator
/// goes to `kubectl`. (Distinct from `node::substrate_lines`, which shows what
/// a node HAS — this is the complement, and only under this overlay.)
pub fn substrate_gap_lines(missing: &[String], report_has_data: bool) -> Vec<(String, Color)> {
    if !report_has_data {
        // No fleet-wide DaemonSets ⇒ nothing to be missing from. Say so rather
        // than imply a clean bill of health we didn't earn.
        return vec![("substrate: no fleet-wide daemonsets".into(), STONE_INK_DIM)];
    }
    if missing.is_empty() {
        return vec![("substrate: complete".into(), STONE_INK_DIM)];
    }
    let col = if missing.len() > 1 {
        STONE_CRIT
    } else {
        STONE_WARN
    };
    let mut lines = vec![(format!("substrate: {} missing", missing.len()), col)];
    lines.extend(missing.iter().map(|m| (format!("  {m}"), col)));
    lines
}

pub fn saturation_lines(sat: &NodeSaturation) -> Vec<(String, Color)> {
    // No dimensions at all ⇒ the node reports no allocatable, so nothing about
    // its strain is computable. "calm" would be a claim we cannot make; this is
    // the SELECTION twin of the map's hatching.
    let Some(worst) = sat.worst_level() else {
        return vec![(
            "strain: unknown - node reports no capacity".into(),
            STONE_WARN,
        )];
    };
    let ink = |l: SatLevel| match l {
        SatLevel::Calm => STONE_INK_DIM,
        SatLevel::Elevated => STONE_WARN,
        SatLevel::High => STONE_CRIT,
    };
    let mut strained: Vec<_> = sat
        .dims
        .iter()
        .filter(|d| d.level > SatLevel::Calm)
        .collect();
    strained.sort_by_key(|d| std::cmp::Reverse(d.level));
    if strained.is_empty() {
        return vec![("strain: calm".into(), STONE_INK_DIM)];
    }
    let mut lines = vec![("strain:".into(), ink(worst))];
    for d in strained {
        lines.push((format!("  {}", d.label), ink(d.level)));
    }
    lines
}

pub fn draw_tooltip(
    sw: &SceneWorld,
    local: (u16, u16),
    snap: &Snapshot,
    overlay: Overlay,
    graticule: bool,
    new_ground: kubernation_core::state::layout::NewGround,
    mouse: Vec2,
) {
    let lines = region_lines(sw, local, snap, overlay, graticule, new_ground);
    if lines.is_empty() {
        return;
    }
    let fs = 14.0;
    let w = lines
        .iter()
        .map(|(t, _)| text_size(ascii(t), fs).width)
        .fold(0.0_f32, f32::max)
        + 16.0;
    let h = lines.len() as f32 * 17.0 + 10.0;
    let x = (mouse.x + 16.0).min(screen_width() - w - 8.0);
    let y = (mouse.y + 18.0).min(screen_height() - h - 8.0);
    stone_panel(x, y, w, h);
    for (i, (content, color)) in lines.iter().enumerate() {
        text(
            ascii(content),
            x + 8.0,
            y + 17.0 + i as f32 * 17.0,
            fs,
            *color,
        );
    }
}

// --- detail panels -------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    City(ClusterId, WorkloadRef),
    Node(ClusterId, String),
}

pub(crate) fn observed_for(
    snap: &Snapshot,
    id: ClusterId,
) -> Option<&kubernation_core::state::observed::ObservedWorld> {
    match id {
        ClusterId::Hot => Some(&snap.hot.observed),
        ClusterId::Warm => snap.warm.as_ref().map(|w| &w.observed),
    }
}

// --- log tail overlay -----------------------------------------------------

/// A centered scrollback panel showing the tail of one pod's logs. The net
/// thread keeps `tail` fresh on a ~2s poll; this just paints the latest.
/// `filter` narrows the shown lines (terms AND; `!term` excludes); `previous`
/// reflects the `--previous` toggle, `filter_active` the live filter editor,
/// `timestamps`/`window` the ts and history-window state (for the title; the
/// fetched lines already carry inline timestamps when on).
#[allow(clippy::too_many_arguments)]
pub fn draw_logs(
    tail: &LogTail,
    filter: &str,
    filter_active: bool,
    previous: bool,
    timestamps: bool,
    window: kubernation_core::k8s::logs::LogWindow,
    // The pod's containers (for the in-overlay picker; a tab row shows only when
    // there's more than one) and the active container name. Returns the clicked
    // container, if any, so the caller can re-issue the tail.
    containers: &[String],
    active: Option<&str>,
    // Scrollback: when `follow`, pin to the tail; else `scroll` is the top
    // visible line. Clamped here against the fetched/filtered length and
    // written back so the caller's state stays in range.
    scroll: &mut usize,
    follow: &mut bool,
) -> Option<String> {
    let w = (screen_width() * 0.72).min(940.0);
    let h = (screen_height() - CHROME_H - 40.0).max(200.0);
    let x = (screen_width() - w) / 2.0;
    let y = CHROME_H + 20.0;
    draw_rectangle(x, y, w, h, Color::new(0.06, 0.07, 0.09, 0.97));
    draw_rectangle_lines(x, y, w, h, 2.0, PARCHMENT);

    let title = match &tail.target {
        Some(t) => {
            let tag = if t.cluster == ClusterId::Warm {
                "WARM "
            } else {
                ""
            };
            let prev = if previous { " <previous>" } else { "" };
            let win = if window == kubernation_core::k8s::logs::LogWindow::default() {
                String::new()
            } else {
                format!(" [{}]", window.label())
            };
            let ts = if timestamps { " (ts)" } else { "" };
            format!("logs · {tag}{}/{}{prev}{win}{ts}", t.namespace, t.pod)
        }
        None => "logs".into(),
    };
    text_bold(ascii(&title), x + 14.0, y + 22.0, 16.0, PARCHMENT);
    text(
        "Esc · / filter · p prev · T ts · s window · j/k/g scroll · f follow · c copy · w export",
        x + 14.0,
        y + 40.0,
        12.0,
        DIM,
    );
    draw_line(x, y + 48.0, x + w, y + 48.0, 1.0, darker(PARCHMENT, 0.5));

    // Container picker: a tab row, shown only for a multi-container pod. Drawn
    // before the early returns so a tab click is honoured even while waiting/erroring.
    let mut clicked: Option<String> = None;
    let mut picker_h = 0.0;
    if containers.len() > 1 {
        picker_h = 24.0;
        let ty = y + 52.0;
        let (mx, my) = mouse_position();
        let pressed = is_mouse_button_pressed(MouseButton::Left);
        let mut tx = x + 14.0;
        for name in containers {
            let label = ascii(name);
            let tw = text_size(&label, 12.0).width + 16.0;
            let r = Rect::new(tx, ty, tw, 18.0);
            let hover = r.contains(vec2(mx, my));
            let is_active = active == Some(name.as_str());
            let bg = if is_active {
                PARCHMENT
            } else if hover {
                Color::new(0.18, 0.20, 0.24, 1.0)
            } else {
                Color::new(0.10, 0.12, 0.14, 1.0)
            };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            let fg = if is_active {
                Color::new(0.06, 0.07, 0.09, 1.0)
            } else {
                PARCHMENT
            };
            text(&label, tx + 8.0, ty + 13.0, 12.0, fg);
            if pressed && hover {
                clicked = Some(name.clone());
            }
            tx += tw + 6.0;
        }
    }

    let body_top = y + 64.0 + picker_h;
    let line_h = 15.0;
    // Inner width available to a body line (left + right margin off the panel).
    let body_w = w - 28.0;
    if let Some(err) = &tail.error {
        text(
            ascii(&fit_width(&format!("error: {err}"), 14.0, body_w)),
            x + 14.0,
            body_top,
            14.0,
            CRIT,
        );
        return clicked;
    }
    if tail.text.is_empty() {
        text("(waiting for log lines…)", x + 14.0, body_top, 14.0, DIM);
        return clicked;
    }

    // Apply the filter expression (space-separated AND; `!term` excludes).
    let expr = FilterExpr::parse(filter);
    let total = tail.text.lines().count();
    let all: Vec<&str> = if expr.is_empty() {
        tail.text.lines().collect()
    } else {
        tail.text.lines().filter(|l| expr.matches(l)).collect()
    };

    // The live filter editor / active-filter summary, on the right of the
    // subtitle row.
    if filter_active {
        text(
            ascii(&format!("filter: {filter}_")),
            x + w - 320.0,
            y + 40.0,
            13.0,
            PARCHMENT,
        );
    } else if !filter.is_empty() {
        text(
            ascii(&format!("filter: {filter}  ({}/{total})", all.len())),
            x + w - 320.0,
            y + 40.0,
            12.0,
            DIM,
        );
    }

    if all.is_empty() {
        text(
            ascii(&fit_width(
                &format!("(no lines match \"{filter}\")"),
                14.0,
                body_w,
            )),
            x + 14.0,
            body_top,
            14.0,
            DIM,
        );
        return clicked;
    }

    // Window: `follow` pins to the tail (newest), else `scroll` is the top
    // visible line — clamped against the fitted row count and written back.
    let rows = (((y + h - 12.0) - body_top) / line_h).floor().max(1.0) as usize;
    let max_top = all.len().saturating_sub(rows);
    if *follow {
        *scroll = max_top;
    } else {
        *scroll = (*scroll).min(max_top);
    }
    let start = *scroll;
    let end = (start + rows).min(all.len());
    let plain = Color::new(0.80, 0.84, 0.80, 1.0);
    let mut ly = body_top;
    for raw in &all[start..end] {
        // Bound to the panel width (no clipping in macroquad) — monospace so
        // timestamps + columns line up the way logs read.
        let s = fit_width_mono(raw, 13.0, body_w);
        // Tint by guessed severity so an error stands out (text unchanged).
        let color = match logline::classify(raw) {
            Level::Error => CRIT,
            Level::Warn => WARN,
            Level::Debug => DIM,
            Level::Info | Level::Plain => plain,
        };
        mono_text(ascii(&s), x + 14.0, ly, 13.0, color);
        ly += line_h;
    }
    // Position readout: hidden-above count, or "following" at the tail.
    let pos = if start > 0 {
        format!("↑ {start} earlier")
    } else if *follow {
        "following".to_string()
    } else {
        String::new()
    };
    if !pos.is_empty() {
        text(ascii(&pos), x + w - 170.0, y + 22.0, 12.0, DIM);
    }
    clicked
}

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        // saturating_sub: total on max == 0 (all callers pass ≥ 1 today, but a
        // future screen-derived width must not arm an underflow panic here).
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}~")
    }
}

/// Truncate `s` (appending `…`) to fit within `max_w` pixels at font `size`.
/// macroquad has no clipping, so a long unbroken line would otherwise run past
/// the panel edge; a char-count cap can't bound a proportional font. Binary
/// searches the longest char prefix that fits.
pub(crate) fn fit_width(s: &str, size: f32, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if text_size(s, size).width <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2); // upper-biased so it makes progress
        let mut cand: String = chars[..mid].iter().collect();
        cand.push('…');
        if text_size(&cand, size).width <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut out: String = chars[..lo].iter().collect();
    out.push('…');
    out
}

// --- Drill-down window sizing + per-column scroll (node.rs / city.rs) --------
// macroquad has no scissor, so the windows cull rows against a view rect and
// clamp the scroll offset with these. Pure + unit-tested so node + city can't
// drift on the math.

/// Request a drill-down window size that uses the screen (`draw_window` already
/// caps to screen − 40), clamped to a sane band.
pub(crate) fn panel_size(sw: f32, sh: f32) -> Vec2 {
    vec2(
        // Narrower than the old centred window (900–1100): the panel now shares
        // the play area with the map instead of covering it. 760 keeps the pod
        // rows intact — the left column is 0.55·body and 156px of it is the
        // fixed evict/yaml/fwd cluster, leaving ~247px for a 22-char name.
        (play_width(sw) * 0.68).floor().clamp(560.0, 760.0),
        (sh - CHROME_H - 16.0).clamp(560.0, 1000.0),
    )
}

/// The x boundary between the left (garrison/citizens) and right (terrain/…)
/// columns, for routing the scroll wheel by hover. Takes the frame from
/// [`window::window_rect`] — the placement authority — and adds the windows'
/// inter-column gutter (left ends ~0.55·body, right starts ~0.58·body).
pub(crate) fn panel_split_x(sw: f32, sh: f32) -> f32 {
    let f = panel_frame(sw, sh);
    let body_x = f.x + 14.0; // window PAD
    let body_w = f.w - 28.0; // PAD * 2
    body_x + body_w * 0.565 // midway through the gutter
}

/// The drill-down window's frame, for gating the scroll wheel to its bounds.
///
/// Derived from [`window::window_rect`] rather than re-centred here, so hit
/// testing and scroll routing cannot disagree with what was drawn.
pub(crate) fn panel_frame(sw: f32, sh: f32) -> Rect {
    crate::window::window_rect_at(
        panel_size(sw, sh),
        sw,
        sh,
        crate::window::Place::DockRightOfMap,
    )
}

/// How many characters of a pod row fit in a column of `col_w`, once the
/// hover-revealed button strip is reserved.
///
/// PURE draw-decision fn, unit-tested. D1 docked the drill-down, taking the left
/// column from ~590px to ~402px — at which point the FIXED 156px fwd/yaml/evict
/// cluster stopped being a rounding error. A full row (name + state + restarts +
/// age + usage, ~47 chars ≈ 329px) had 246px of clear space, so the text ran
/// under the buttons on hover. That is D1 §7.2's second failure criterion, and
/// it is why the budget is derived from the column instead of being a constant.
///
/// Deliberately an ESTIMATE from an average advance rather than a measurement:
/// the alternative needs `text_size`, which needs the font, which cannot be
/// called from a unit test. A conservative advance under-fills slightly, which
/// errs toward whitespace rather than toward a collision.
pub(crate) fn row_char_budget(col_w: f32, font_px: f32) -> usize {
    const BUTTONS_PX: f32 = 156.0 + 10.0; // the three buttons, plus a gap
    let advance = (font_px * 0.52).max(1.0);
    (((col_w - BUTTONS_PX).max(0.0)) / advance).floor() as usize
}

/// Clamp a scroll offset to its content/view heights.
pub(crate) fn clamp_scroll(offset: f32, content_h: f32, view_h: f32) -> f32 {
    offset.clamp(0.0, (content_h - view_h).max(0.0))
}

/// Scrollbar thumb `(top_y, height)` within a `view_h` track from `view_top`, or
/// `None` when the content fits. `offset` is treated as already clamped.
pub(crate) fn scroll_thumb(
    view_top: f32,
    view_h: f32,
    content_h: f32,
    offset: f32,
) -> Option<(f32, f32)> {
    if content_h <= view_h || view_h <= 0.0 {
        return None;
    }
    let frac = (view_h / content_h).clamp(0.05, 1.0);
    let thumb_h = view_h * frac;
    let max = (content_h - view_h).max(1.0);
    let t = offset.clamp(0.0, max) / max;
    Some((view_top + t * (view_h - thumb_h), thumb_h))
}

/// `fit_width` for the fixed-width log face: every glyph has the same advance,
/// so the fit is a single char-width division — no binary search.
pub(crate) fn fit_width_mono(s: &str, size: f32, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if mono_text_size(s, size).width <= max_w {
        return s.to_string();
    }
    let cw = mono_text_size("M", size).width.max(1.0);
    let max_chars = (max_w / cw).floor() as usize;
    if max_chars <= 1 {
        return "…".into();
    }
    let cut: String = s.chars().take(max_chars - 1).collect();
    format!("{cut}…")
}

// --- evict confirm ------------------------------------------------------

#[derive(Default)]
pub struct Confirm {
    pub yes: bool,
    pub cancel: bool,
}

/// A destructive-action confirm modal for pod eviction (the app's only write).
/// `tag` is "" or "WARM " in pair mode. Returns which button was clicked.
pub fn draw_evict_confirm(tag: &str, ns: &str, pod: &str, mouse: Vec2, click: bool) -> Confirm {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    let w = 480.0;
    let h = 158.0;
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    stone_panel(x, y, w, h);
    text_bold("Evict pod?", x + 16.0, y + 28.0, 18.0, CRIT);
    text(
        ascii(&format!("{tag}{ns}/{pod}")),
        x + 16.0,
        y + 52.0,
        14.0,
        STONE_INK,
    );
    text(
        "Deletes the pod from the cluster now.",
        x + 16.0,
        y + 72.0,
        13.0,
        STONE_INK_DIM,
    );
    text(
        "A managed pod is recreated by its controller; a bare pod is gone.",
        x + 16.0,
        y + 89.0,
        12.0,
        STONE_INK_DIM,
    );

    let bh = 28.0;
    let by = y + h - bh - 12.0;
    let cancel = Rect::new(x + 16.0, by, 150.0, bh);
    let evict = Rect::new(x + w - 166.0, by, 150.0, bh);
    let cbg = if cancel.contains(mouse) {
        lighter(STONE_DARK, 1.4)
    } else {
        STONE_DARK
    };
    draw_rectangle(cancel.x, cancel.y, cancel.w, cancel.h, cbg);
    draw_rectangle_lines(cancel.x, cancel.y, cancel.w, cancel.h, 1.0, STONE_EDGE);
    let cm = text_size("Cancel", 15.0);
    text(
        "Cancel",
        cancel.x + (cancel.w - cm.width) / 2.0,
        by + 19.0,
        15.0,
        STONE_LIGHT,
    );
    let ebg = if evict.contains(mouse) {
        CRIT
    } else {
        darker(CRIT, 0.8)
    };
    draw_rectangle(evict.x, evict.y, evict.w, evict.h, ebg);
    draw_rectangle_lines(evict.x, evict.y, evict.w, evict.h, 1.0, CRIT);
    let em = text_size("Evict", 15.0);
    text(
        "Evict",
        evict.x + (evict.w - em.width) / 2.0,
        by + 19.0,
        15.0,
        INK,
    );
    Confirm {
        yes: click && evict.contains(mouse),
        cancel: click && cancel.contains(mouse),
    }
}

/// Confirm modal for committing the planning turn (applies N changes to the
/// cluster). Returns (commit, cancel).
pub fn draw_commit_confirm(n: usize, mouse: Vec2, click: bool) -> Confirm {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    let w = 480.0;
    let h = 150.0;
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    stone_panel(x, y, w, h);
    text_bold("Commit the turn?", x + 16.0, y + 28.0, 18.0, WARN);
    text(
        format!("Apply {n} staged change(s) to the cluster."),
        x + 16.0,
        y + 54.0,
        14.0,
        STONE_INK,
    );
    text(
        "Each is dry-run validated first; anything rejected is blocked.",
        x + 16.0,
        y + 74.0,
        12.0,
        STONE_INK_DIM,
    );
    let bh = 28.0;
    let by = y + h - bh - 12.0;
    let cancel = Rect::new(x + 16.0, by, 150.0, bh);
    let commit = Rect::new(x + w - 166.0, by, 150.0, bh);
    let cbg = if cancel.contains(mouse) {
        lighter(STONE_DARK, 1.4)
    } else {
        STONE_DARK
    };
    draw_rectangle(cancel.x, cancel.y, cancel.w, cancel.h, cbg);
    draw_rectangle_lines(cancel.x, cancel.y, cancel.w, cancel.h, 1.0, STONE_EDGE);
    let cm = text_size("Cancel", 15.0);
    text(
        "Cancel",
        cancel.x + (cancel.w - cm.width) / 2.0,
        by + 19.0,
        15.0,
        STONE_LIGHT,
    );
    let ebg = if commit.contains(mouse) {
        WARN
    } else {
        darker(WARN, 0.8)
    };
    draw_rectangle(commit.x, commit.y, commit.w, commit.h, ebg);
    draw_rectangle_lines(commit.x, commit.y, commit.w, commit.h, 1.0, WARN);
    let em = text_size("Commit", 15.0);
    text(
        "Commit",
        commit.x + (commit.w - em.width) / 2.0,
        by + 19.0,
        15.0,
        PLATE,
    );
    Confirm {
        yes: click && commit.contains(mouse),
        cancel: click && cancel.contains(mouse),
    }
}

/// Confirm modal for a chaos drill (a real, deliberate failure injection).
/// CRIT-red, blunt copy: `title` names the drill, `line1` the concrete effect,
/// `line2` the blast/impact, `action` the button label. Returns (yes, cancel).
pub fn draw_chaos_confirm(
    title: &str,
    line1: &str,
    line2: &str,
    action: &str,
    mouse: Vec2,
    click: bool,
) -> Confirm {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    let w = 520.0;
    let h = 158.0;
    let x = ((screen_width() - w) / 2.0).floor();
    let y = ((screen_height() - h) / 2.0).floor();
    stone_panel(x, y, w, h);
    text_bold(ascii(title), x + 16.0, y + 28.0, 18.0, CRIT);
    text(ascii(line1), x + 16.0, y + 54.0, 14.0, STONE_INK);
    text(ascii(line2), x + 16.0, y + 74.0, 13.0, STONE_INK_DIM);
    text(
        "A real action on the live cluster.",
        x + 16.0,
        y + 91.0,
        12.0,
        STONE_INK_DIM,
    );
    let bh = 28.0;
    let by = y + h - bh - 12.0;
    let cancel = Rect::new(x + 16.0, by, 170.0, bh);
    let run = Rect::new(x + w - 186.0, by, 170.0, bh);
    let cbg = if cancel.contains(mouse) {
        lighter(STONE_DARK, 1.4)
    } else {
        STONE_DARK
    };
    draw_rectangle(cancel.x, cancel.y, cancel.w, cancel.h, cbg);
    draw_rectangle_lines(cancel.x, cancel.y, cancel.w, cancel.h, 1.0, STONE_EDGE);
    let cm = text_size("Cancel", 15.0);
    text(
        "Cancel",
        cancel.x + (cancel.w - cm.width) / 2.0,
        by + 19.0,
        15.0,
        STONE_LIGHT,
    );
    let rbg = if run.contains(mouse) {
        CRIT
    } else {
        darker(CRIT, 0.8)
    };
    draw_rectangle(run.x, run.y, run.w, run.h, rbg);
    draw_rectangle_lines(run.x, run.y, run.w, run.h, 1.0, CRIT);
    let rm = text_size(action, 15.0);
    text(
        action,
        run.x + (run.w - rm.width) / 2.0,
        by + 19.0,
        15.0,
        INK,
    );
    Confirm {
        yes: click && run.contains(mouse),
        cancel: click && cancel.contains(mouse),
    }
}

// --- context picker -----------------------------------------------------

pub struct PickerLayout {
    pub rows: Vec<Rect>,
}

/// Modal single-select list; the dot marks the active item, the highlight bar
/// the keyboard cursor. `title`/`hint` chrome it (so the same widget serves the
/// context switcher and the namespace filter). Returns row rects for click hits.
pub fn draw_picker(
    items: &[String],
    current: &str,
    idx: usize,
    title: &str,
    hint: &str,
) -> PickerLayout {
    let contexts = items;
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.45),
    );
    let w = 480.0_f32;
    let row_h = 26.0;
    let h = 58.0 + contexts.len().max(1) as f32 * row_h;
    let x = (screen_width() - w) / 2.0;
    let y = (screen_height() - h) / 2.0;
    stone_panel(x, y, w, h);
    text_bold(ascii(title), x + 16.0, y + 26.0, 18.0, STONE_INK);
    text(ascii(hint), x + 16.0, y + 45.0, 13.0, STONE_INK_DIM);
    let mut rows = Vec::new();
    let list_y = y + 58.0;
    if contexts.is_empty() {
        text(
            "no contexts in kubeconfig",
            x + 16.0,
            list_y + 18.0,
            14.0,
            STONE_INK_DIM,
        );
    }
    for (i, ctx) in contexts.iter().enumerate() {
        let ry = list_y + i as f32 * row_h;
        let r = Rect::new(x + 8.0, ry, w - 16.0, row_h);
        if i == idx {
            stone_well(r.x, r.y, r.w, r.h);
        }
        if ctx == current {
            draw_circle(r.x + 12.0, ry + 13.0, 4.0, gauge_ok());
        }
        let row_ink = if i == idx { INK } else { STONE_INK };
        text(ascii(ctx), r.x + 26.0, ry + 18.0, 15.0, row_ink);
        rows.push(r);
    }
    PickerLayout { rows }
}

/// A small banner announcing the blast-radius overlay is active — the affected
/// count, or a hint when no subject resolves. Sits at the bottom-left of the
/// play area.
pub fn draw_blast_banner(affected: Option<usize>, _map_w: f32) {
    let msg = match affected {
        Some(0) => "BLAST RADIUS · nothing downstream derivable · B to clear".to_string(),
        Some(n) => format!("BLAST RADIUS · {n} affected · B to clear"),
        None => "BLAST RADIUS · select a city/node or focus a concern · B".to_string(),
    };
    let fs = 13.0;
    let bw = text_size(&msg, fs).width + 20.0;
    let bx = 6.0;
    let by = screen_height() - 26.0 - 8.0; // just above the screen bottom
    stone_panel(bx, by, bw, 22.0);
    let col = if affected.unwrap_or(0) > 0 {
        STONE_CRIT
    } else {
        STONE_INK
    };
    text(&msg, bx + 10.0, by + 15.0, fs, col);
}

/// PURE draw-decision: the connection banner text + whether it's an error (red),
/// or `None` when the API is live (no banner). Unit-tested.
pub fn conn_banner(conn: &ConnState, context: &str) -> Option<(String, bool)> {
    match conn {
        ConnState::Live => None,
        ConnState::Connecting => Some((format!("connecting to {context}…"), false)),
        ConnState::Lost(why) => Some((format!("reconnecting to {context} — {why}"), true)),
    }
}

/// Draw the connection banner (a strip just under the chrome) when not live.
pub fn draw_conn_banner(conn: &ConnState, context: &str) {
    let Some((msg, is_err)) = conn_banner(conn, context) else {
        return;
    };
    let msg = ascii(&msg);
    let fs = 13.0;
    let h = 22.0;
    let y = CHROME_H + 2.0;
    let w = map_width();
    // A semi-opaque dark strip so it reads over the map; meaning colour on top.
    let bg = if is_err {
        Color::new(0.22, 0.05, 0.05, 0.92)
    } else {
        Color::new(0.10, 0.09, 0.05, 0.90)
    };
    draw_rectangle(0.0, y, w, h, bg);
    draw_line(0.0, y + h, w, y + h, 1.0, darker(bg, 0.5));
    let col = if is_err { CRIT } else { WARN };
    text(&msg, 12.0, y + 15.0, fs, col);
}

/// A persistent CRIT banner under the chrome — used when the net thread has
/// crashed (the world is frozen). Takes precedence over the connection banner.
pub fn draw_fatal_banner(msg: &str) {
    let h = 22.0;
    let y = CHROME_H + 2.0;
    let w = map_width();
    let bg = Color::new(0.24, 0.04, 0.04, 0.96);
    draw_rectangle(0.0, y, w, h, bg);
    draw_line(0.0, y + h, w, y + h, 1.0, darker(bg, 0.5));
    text(ascii(msg), 12.0, y + 15.0, 13.0, CRIT);
}

/// Map a value series to polyline points inside `rect` — x runs oldest→newest
/// left to right, y is bottom (0) to top (`max`), each value clamped to
/// `[0, max]`. Empty series or a non-positive `max` yields no points. Pure +
/// unit-tested (the sparkline draw-decision per the GUI testability policy).
pub fn sparkline_points(values: &[f32], max: f32, rect: Rect) -> Vec<Vec2> {
    if values.is_empty() || max <= 0.0 {
        return Vec::new();
    }
    let n = values.len();
    let dx = if n > 1 {
        rect.w / (n as f32 - 1.0)
    } else {
        0.0
    };
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let t = (v / max).clamp(0.0, 1.0);
            vec2(rect.x + i as f32 * dx, rect.y + rect.h - t * rect.h)
        })
        .collect()
}

/// Draw a small trend sparkline: a faint well + top (100%/`max`) reference
/// line, the value polyline in `line`, and a dot on the latest sample. A
/// single sample renders as just the dot; an empty series draws only the well.
pub fn draw_sparkline(rect: Rect, values: &[f32], max: f32, line: Color) {
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Color::new(0.0, 0.0, 0.0, 0.28),
    );
    // A faint frame so the chart area reads even when the trace hugs the floor
    // (a near-idle node), plus a baseline + top (max) reference.
    draw_rectangle_lines(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        1.0,
        Color::new(1.0, 1.0, 1.0, 0.10),
    );
    draw_line(
        rect.x,
        rect.y + 0.5,
        rect.x + rect.w,
        rect.y + 0.5,
        1.0,
        Color::new(1.0, 1.0, 1.0, 0.12),
    );
    let pts = sparkline_points(values, max, rect);
    // A flat single-sample series still shows a short stub, not just a dot.
    if pts.len() == 1 {
        let p = pts[0];
        draw_line(rect.x, p.y, rect.x + rect.w, p.y, 1.5, line);
    }
    for w in pts.windows(2) {
        draw_line(w[0].x, w[0].y, w[1].x, w[1].y, 1.5, line);
    }
    if let Some(last) = pts.last() {
        draw_circle(last.x, last.y, 2.0, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::scene;
    use crate::net::{Snapshot, WorldSnap};
    use kubernation_core::state::fixtures as fx;
    use kubernation_core::state::layout::{GroundState, NewGround};
    use kubernation_core::state::model::Models;
    use std::sync::Arc;

    /// Drawing, hit-testing and scroll routing agree about the window's edge.
    ///
    /// They have to: `draw_window` paints the frame, `panel_frame` gates the
    /// scroll wheel to it, and `panel_split_x` routes the wheel between the two
    /// columns inside it. Before D1 each re-derived the same clamp-and-centre
    /// and agreed by convention — so moving the geometry was a three-way edit
    /// with nothing enforcing the third. Now all of them go through
    /// `window::window_rect`, and this pins that they still do.
    /// The row budget shrinks with the column, and never runs under the buttons.
    ///
    /// The number that matters: at D1's docked width the left column is ~402px,
    /// and the fwd/yaml/evict strip is a FIXED 156px. A budget that ignored the
    /// column would let a full row (~47 chars) draw straight through them.
    #[test]
    fn row_budget_reserves_the_button_strip() {
        // The old centred window's column had room for a whole row.
        let wide = row_char_budget(590.0, 14.0);
        // D1's docked column does not, and the budget says so.
        let docked = row_char_budget(402.0, 14.0);
        assert!(wide > docked, "{wide} vs {docked}");
        assert!(docked >= 8, "still enough to identify a pod: {docked}");

        // The budget never claims space the buttons occupy.
        for col_w in [402.0f32, 500.0, 590.0, 760.0] {
            let px = row_char_budget(col_w, 14.0) as f32 * 14.0 * 0.52;
            assert!(
                px <= col_w - 156.0,
                "budget {px}px overruns the button strip in a {col_w}px column"
            );
        }
        // A column narrower than the buttons yields nothing, not a negative.
        assert_eq!(row_char_budget(100.0, 14.0), 0);
        assert_eq!(row_char_budget(0.0, 14.0), 0);
    }

    #[test]
    fn placement_has_one_authority() {
        use crate::window::{Place, window_rect_at};
        let centred = |sz, sw, sh| window_rect_at(sz, sw, sh, Place::Centred);

        // CONCRETE, on the default window. `panel_frame == window_rect(..)` is
        // a tautology once panel_frame delegates, so it cannot detect the
        // geometry moving; only a stated rect can. Change the placement and
        // this is what tells you, deliberately.
        assert_eq!(
            panel_frame(1380.0, 860.0),
            Rect::new(358.0, 40.0, 758.0, 812.0),
            "the drill-down docks right of the map on the default window"
        );
        // Two of the twelve windows D1 does not touch, pinned at today's values.
        assert_eq!(
            centred(vec2(640.0, 800.0), 1380.0, 860.0),
            Rect::new(370.0, 30.0, 640.0, 800.0),
            "About — one of the twelve D1 does not touch"
        );
        assert_eq!(
            centred(vec2(720.0, 600.0), 1380.0, 860.0),
            Rect::new(330.0, 130.0, 720.0, 600.0),
            "Inspector — likewise"
        );
        for (sw, sh) in [
            (1380.0, 860.0),  // the default window
            (800.0, 600.0),   // smaller than the clamp band
            (3840.0, 2160.0), // larger than it
            (1000.0, 1000.0),
            (2560.0, 1440.0),
        ] {
            let want = window_rect_at(panel_size(sw, sh), sw, sh, Place::DockRightOfMap);
            assert_eq!(panel_frame(sw, sh), want, "panel_frame at {sw}x{sh}");

            // The split lies strictly inside the frame, and moves with it.
            let split = panel_split_x(sw, sh);
            assert!(
                split > want.x && split < want.x + want.w,
                "split {split} outside frame {want:?} at {sw}x{sh}"
            );
            assert!(
                (split - (want.x + 14.0 + (want.w - 28.0) * 0.565)).abs() < 0.01,
                "split stopped tracking the authority at {sw}x{sh}"
            );

            // On screen, and it leaves the docked column alone: the panel's
            // right edge is the play area's, never the screen's.
            assert!(want.x >= 0.0 && want.y >= 0.0);
            assert!(want.w <= sw - 40.0 && want.h <= sh - 40.0);
            assert!(
                (want.x + want.w - play_width(sw)).abs() < 0.51,
                "panel right edge {} != play edge {} at {sw}x{sh}",
                want.x + want.w,
                play_width(sw)
            );
        }
    }

    #[test]
    fn panel_scroll_and_size_helpers() {
        // clamp_scroll
        assert_eq!(clamp_scroll(50.0, 100.0, 200.0), 0.0); // content fits → pinned
        assert_eq!(clamp_scroll(30.0, 300.0, 200.0), 30.0); // in range, unchanged
        assert_eq!(clamp_scroll(999.0, 300.0, 200.0), 100.0); // clamped to max (300-200)
        // scroll_thumb: None when it fits; in-track at top and at max.
        assert!(scroll_thumb(10.0, 200.0, 150.0, 0.0).is_none());
        let (ty, th) = scroll_thumb(10.0, 100.0, 400.0, 0.0).unwrap();
        assert!((ty - 10.0).abs() < 1e-3 && th > 0.0 && th <= 100.0);
        let (ty2, th2) = scroll_thumb(10.0, 100.0, 400.0, 300.0).unwrap(); // max offset
        assert!((ty2 + th2 - 110.0).abs() < 1e-3); // thumb bottom flush with view bottom
        // panel_size: clamps small up, big down.
        // D1: the drill-down docks beside the map instead of covering it, so
        // the band is narrower than the old centred window's 900–1100 and is a
        // share of the PLAY area, not of the screen.
        assert_eq!(panel_size(400.0, 400.0), vec2(560.0, 560.0));
        assert_eq!(panel_size(4000.0, 4000.0), vec2(760.0, 1000.0));
        // A window too small to leave a usable strip: the panel is clamped to
        // the play area and no aim point is offered, so the camera declines to
        // pan rather than panning at a sliver (§7.2's "too small to locate
        // anything in", refused rather than rendered).
        assert!(panel_frame(400.0, 400.0).w <= play_width(400.0));
        assert!(crate::window::map_strip(400.0, 400.0).is_none());
        // The default window leaves a real strip, and it is what the camera aims at.
        let strip = crate::window::map_strip(1380.0, 860.0).expect("a usable strip");
        assert_eq!(strip.x, 0.0);
        assert_eq!(strip.w, 358.0);
        assert!(strip.w + panel_frame(1380.0, 860.0).w == play_width(1380.0));
        // panel_split_x sits between the columns (≈0.565 of the body).
        let sx = panel_split_x(1380.0, 860.0);
        assert!(sx > 600.0 && sx < 800.0);
    }

    #[test]
    fn conn_banner_states() {
        assert_eq!(conn_banner(&ConnState::Live, "kind"), None);
        let (t, err) = conn_banner(&ConnState::Connecting, "kind").unwrap();
        assert!(
            t.contains("connecting") && t.contains("kind") && !err,
            "{t}"
        );
        let (t, err) = conn_banner(
            &ConnState::Lost("can't reach the API server".into()),
            "prod",
        )
        .unwrap();
        assert!(t.contains("reconnecting") && t.contains("prod") && t.contains("reach") && err);
    }

    /// A reference is shown only with the frame, and unknown is said out loud.
    ///
    /// The gating is what makes the gate's discrimination check meaningful: with
    /// the frame off the app must offer NO naming aid, or a second person finding
    /// the slot would be demonstrating familiarity with the fleet instead.
    #[test]
    fn a_reference_appears_only_with_the_frame_and_is_never_invented() {
        use kubernation_core::state::graticule::GridRef;
        let r = GridRef {
            column: "C".into(),
            row: 4,
        };
        assert_eq!(grid_ref_line(Some(&r), false), None, "off: no naming aid");
        assert_eq!(grid_ref_line(None, false), None);

        let (text, _) = grid_ref_line(Some(&r), true).expect("on: the reference");
        assert!(text.contains("C4"), "got {text:?}");

        // A node with no durable position says so. Never `A0`, which is a real
        // slot some other node holds — the one unacceptable failure for a
        // scheme whose entire job is naming exactly one thing.
        let (unknown, _) = grid_ref_line(None, true).expect("unknown is stated");
        assert!(
            !unknown.contains('0'),
            "fabricated a reference: {unknown:?}"
        );
        assert!(unknown.to_lowercase().contains("no durable"), "{unknown:?}");
    }

    /// The label half: names the pool, admits when the grouping was inferred,
    /// and refuses to dress an absent label up as a name.
    #[test]
    fn extent_line_marks_a_guessed_size_and_stays_quiet_on_a_measured_one() {
        // A measured size needs no caveat — the `pool_line` rule.
        assert_eq!(extent_line(ExtentSource::Allocatable), None);
        // Both fallback rungs speak, and they are DISTINGUISHABLE from each
        // other, which was the gap: they produce the same extent, so without
        // words they read identically to each other and to a measured node.
        let (it, _) = extent_line(ExtentSource::InstanceType).expect("marked");
        let (df, _) = extent_line(ExtentSource::Default).expect("marked");
        assert_ne!(it, df);
        assert!(it.contains("instance type"), "{it}");
        assert!(df.contains("not reported"), "{df}");
        for m in [&it, &df] {
            assert!(
                m.contains("not measured") || m.contains("not reported"),
                "a fallback size must not read as a measurement: {m}"
            );
        }
    }

    #[test]
    fn the_pool_line_names_the_group_and_how_it_was_inferred() {
        use kubernation_core::state::layout::PoolSource;
        use kubernation_core::state::model::DEFAULT_POOL;

        assert_eq!(pool_line("sys", PoolSource::Provider("eks"), false), None);

        // A declared pool needs no caveat.
        let (declared, _) = pool_line("sys", PoolSource::Provider("eks"), true).unwrap();
        assert_eq!(declared, "pool sys");

        // An inferred one says so: instance-type grouping merges pools the
        // provider considers distinct, and a reader comparing two nodes has to
        // know the grouping is a guess.
        let (inferred, _) = pool_line("m5.large", PoolSource::InstanceType, true).unwrap();
        assert!(inferred.contains("m5.large") && inferred.contains("instance type"));

        // The sentinel is an ABSENCE, not a pool. "pool unpooled" would read as
        // membership in something called unpooled.
        let (none_at_all, _) = pool_line(DEFAULT_POOL, PoolSource::Default, true).unwrap();
        assert!(!none_at_all.contains(DEFAULT_POOL), "{none_at_all:?}");
        assert!(none_at_all.contains("not grouped"), "{none_at_all:?}");
    }

    /// The panel speaks all three answers under a fixed baseline.
    ///
    /// This is the assertion that was missing when the merge collapsed three
    /// states into `Option<f64>`: the core test
    /// `changed_since_separates_unknown_from_unchanged` kept passing, because it
    /// pins the core function, not what reaches the operator. An absent
    /// succession record is not evidence that nothing happened, and under a
    /// fixed baseline the panel is the only place that says so.
    #[test]
    fn the_panel_distinguishes_no_record_from_unchanged() {
        let since = NewGround::Since(std::time::SystemTime::UNIX_EPOCH);
        let say = |g| fresh_line(Some(g), since).map(|(t, _)| t);

        let new = say(GroundState::New(1.0)).expect("marked ground speaks");
        let settled = say(GroundState::Settled).expect("unchanged ground speaks");
        let unknown = say(GroundState::Unknown).expect("unknown ground speaks");

        assert_ne!(settled, unknown, "an absent record is not 'unchanged'");
        assert_ne!(new, settled);
        assert!(unknown.contains("no succession on record"), "{unknown}");
        assert!(settled.contains("unchanged"), "{settled}");
        // Under a fixed baseline nothing decays, so no wording may claim an age.
        for w in [&new, &settled, &unknown] {
            assert!(!w.contains("recently") && !w.contains("just"), "{w}");
        }

        // A rolling window has no unknown to lose, so it stays quiet rather
        // than repeating a non-answer on every hover.
        let fading = NewGround::Fading(std::time::Duration::from_secs(3600));
        assert!(fresh_line(Some(GroundState::Unknown), fading).is_none());
        assert!(fresh_line(Some(GroundState::Settled), fading).is_none());

        // Off says nothing in either shape, and an unlisted node says nothing.
        assert!(fresh_line(Some(GroundState::New(1.0)), NewGround::Off).is_none());
        assert!(fresh_line(Some(GroundState::Unasked), since).is_none());
        assert!(fresh_line(None, since).is_none());
    }

    /// Settled ground says nothing; fresh ground says something at every tier.
    ///
    /// The load-bearing half is the second assertion: the words must change at
    /// exactly the freshness values the COLOUR changes at. Both go through
    /// `theme::fresh_tier`, so this pins that they still do — a second,
    /// independent bucketing would let the map paint a province "just changed
    /// hands" while the panel called it "settling", and nothing else would fail.
    #[test]
    fn fresh_wording_changes_where_the_colour_does() {
        assert!(
            fresh_line(
                Some(GroundState::Settled),
                NewGround::Fading(std::time::Duration::from_secs(3600))
            )
            .is_none(),
            "settled ground says nothing under a rolling window"
        );

        // Every tier produces a line, and adjacent tiers produce different ones.
        let at = |f: f64| {
            fresh_line(
                Some(GroundState::New(f)),
                NewGround::Fading(std::time::Duration::from_secs(3600)),
            )
            .expect("fresh ground speaks")
            .0
        };
        let (newest, middle, oldest) = (at(1.0), at(0.5), at(0.01));
        assert_ne!(newest, middle);
        assert_ne!(middle, oldest);

        // The boundaries agree with the paint. With FRESH_STEPS = 3 the tiers
        // break at 1/3 and 2/3; sample just either side of each.
        for edge in [1.0 / 3.0, 2.0 / 3.0] {
            let (lo, hi) = (edge - 1e-6, edge + 1e-6);
            assert_ne!(at(lo), at(hi), "wording must change at the tier edge");
            assert_ne!(
                crate::theme::fresh_land_pair(lo, crate::theme::FRESH_STEPS)
                    .0
                    .r,
                crate::theme::fresh_land_pair(hi, crate::theme::FRESH_STEPS)
                    .0
                    .r,
                "and the colour must change at the SAME edge",
            );
        }
    }

    /// The tooltip / SELECTION text is pure draw-decision logic — testable
    /// without a GL context (it formats strings + picks colors, no macroquad
    /// calls). This is the Option-A pattern: every GUI view's *decisions*
    /// should live in a fn like this, asserted against a fixture world.
    #[test]
    fn region_lines_name_the_workload_under_a_city() {
        let (world, mut s) = fx::world();
        s.node(fx::node("n1", Some("z-a")));
        s.deployment(fx::deployment("demo", "web", 1, 1));
        s.replicaset(fx::replicaset("demo", "web-rs", "web"));
        s.pod(fx::pod_owned(
            fx::pod("demo", "web-rs-1", Some("n1")),
            "ReplicaSet",
            "web-rs",
        ));
        let models = Arc::new(Models::build(&world));
        let (cx, cy) = {
            let c = models.world.cities().next().expect("a city was sited");
            (c.x, c.y)
        };
        let posture = kubernation_core::state::posture::posture_report(&world);
        let cost = kubernation_core::state::cost::cost_report(
            &world,
            &kubernation_core::state::cost::CostRates::default(),
        );
        let snap = Snapshot {
            hot: WorldSnap {
                models,
                observed: world,
                slo: Arc::new(std::collections::HashMap::new()),
                posture,
                cost,
                opencost_note: None,
                fresh: Arc::new(std::collections::HashMap::new()),
            },
            warm: None,
            pair: None,
            attention: Arc::new(Vec::new()),
        };
        let worlds = scene(&snap);
        let lines = region_lines(
            &worlds[0],
            (cx, cy),
            &snap,
            Overlay::Terrain,
            false,
            NewGround::Off,
        );
        assert!(
            lines.iter().any(|(t, _)| t.contains("web")),
            "the SELECTION/tooltip lines should name the workload: {lines:?}"
        );
        assert!(
            !lines.iter().any(|(t, _)| t.starts_with("grid")),
            "with the frame off, no naming aid at all: {lines:?}"
        );

        // With the frame on, the same position is nameable. Asserted through the
        // real `region_lines` rather than only against `grid_ref_line`, because
        // the unit test cannot catch the line being wired to the wrong arm or
        // dropped on the way out.
        let named = region_lines(
            &worlds[0],
            (cx, cy),
            &snap,
            Overlay::Terrain,
            true,
            NewGround::Off,
        );
        let reference = named
            .iter()
            .find(|(t, _)| t.starts_with("grid"))
            .unwrap_or_else(|| panic!("the frame is on, so say where this is: {named:?}"));
        assert!(
            !reference.0.contains("no durable"),
            "a placed province has a real reference: {:?}",
            reference.0,
        );
    }

    /// Build the fixture scene the resolver tests share.
    fn probe_fixture() -> (Snapshot, (u16, u16)) {
        let (world, mut s) = fx::world();
        // Several nodes in one zone: `Coast::new` gives a SINGLE-node continent
        // only a gentle wobble, so a one-node world may contain no sea-inside-
        // the-rectangle cell at all and the interesting case would go untested.
        for n in ["n1", "n2", "n3", "n4"] {
            s.node(fx::node(n, Some("z-a")));
        }
        s.deployment(fx::deployment("demo", "a-long-workload-name", 1, 1));
        s.replicaset(fx::replicaset("demo", "rs", "a-long-workload-name"));
        let mut p = fx::pod_owned(fx::pod("demo", "rs-1", Some("n1")), "ReplicaSet", "rs");
        p.metadata
            .labels
            .get_or_insert_with(Default::default)
            .insert("app".into(), "a-long-workload-name".into());
        s.pod(p);
        // A Service selecting that pod moors a HARBOUR on the coast. Without one
        // the sweep contains no coast marker, and the divergence between
        // `subject_at` and `panel_for` that this fixture exists to exercise
        // would go unexercised — the guard-the-guard assertion catches that.
        s.service(fx::service(
            "demo",
            "a-long-workload-name",
            &[("app", "a-long-workload-name")],
        ));
        let models = Arc::new(Models::build(&world));
        let city = {
            let c = models.world.cities().next().expect("a city was sited");
            (c.x, c.y)
        };
        let posture = kubernation_core::state::posture::posture_report(&world);
        let cost = kubernation_core::state::cost::cost_report(
            &world,
            &kubernation_core::state::cost::CostRates::default(),
        );
        let snap = Snapshot {
            hot: WorldSnap {
                models,
                observed: world,
                slo: Arc::new(std::collections::HashMap::new()),
                posture,
                cost,
                opencost_note: None,
                fresh: Arc::new(std::collections::HashMap::new()),
            },
            warm: None,
            pair: None,
            attention: Arc::new(Vec::new()),
        };
        (snap, city)
    }

    /// D2 step 2: the cell→identity conversion has one home, and the one thing
    /// that looks like it is deliberately not it.
    ///
    /// `subject_at` is what the Oracle's scope list and the blast subject both
    /// use. `panel_for` looks like the same conversion and is richer: it goes
    /// through `resolve_region`, so a **coast marker** opens the city it serves,
    /// which `subject_at` does not see at all. Folding them together would give
    /// the Oracle and the blast radius a resolution they have never had.
    ///
    /// So this asserts BOTH halves — agreement on land, divergence on the coast —
    /// because a test that only checked agreement would license the fold.
    #[test]
    fn subject_at_is_the_one_conversion_and_panel_for_is_not_it() {
        use crate::draw::{Hit, subject_at};
        use kubernation_core::state::blast::Subject;
        let (snap, _) = probe_fixture();
        let worlds = scene(&snap);
        let (bw, bh) = (snap.hot.models.world.width, snap.hot.models.world.height);

        let mut saw_city = false;
        let mut saw_province = false;
        let mut saw_coast_divergence = false;
        let mut saw_carved_divergence = false;

        for y in 0..bh {
            for x in 0..bw {
                let cell = (x, y);
                let subj = subject_at(&worlds, cell);
                let panel = panel_for(&worlds, Hit::at(cell));

                match (&subj, &panel) {
                    // On land the two must name the same entity.
                    (Some((_, Subject::Workload(r))), Some(Panel::City(_, pr))) => {
                        assert_eq!(r, pr, "city at {cell:?}");
                        saw_city = true;
                    }
                    (Some((_, Subject::Node(n))), Some(Panel::Node(_, pn))) => {
                        assert_eq!(n, pn, "province at {cell:?}");
                        saw_province = true;
                    }
                    // A coast marker: `panel_for` resolves it, `subject_at` does
                    // not. Intended — see `subject_at`'s doc.
                    (None, Some(Panel::City(..))) => saw_coast_divergence = true,
                    // The other direction, and the more interesting one:
                    // `region_at` tests a province's RECTANGLE while
                    // `resolve_region` applies the coast carving, so a cell the
                    // shoreline made sea is a node to one and ocean to the
                    // other (the v1.3.0 finding, from the other side).
                    //
                    // Pre-existing and preserved here: steps 1-2 change no
                    // behaviour. §3.3's inversion dissolves it — an identity is
                    // a node or it is not, and no ambiguous cell survives to
                    // disagree about.
                    (Some((_, Subject::Node(_))), None) => saw_carved_divergence = true,
                    (None, None) => {}
                    other => panic!("subject_at and panel_for disagree at {cell:?}: {other:?}"),
                }
            }
        }

        // Guard the guard: without these the sweep could be vacuous.
        assert!(saw_city, "the fixture never produced a city");
        assert!(saw_province, "the fixture never produced a province");
        assert!(
            saw_carved_divergence,
            "the fixture never produced a carved-sea cell inside a province rect"
        );
        assert!(
            saw_coast_divergence,
            "the fixture never produced a coast marker — the divergence this \
             test exists to pin was never exercised"
        );
    }

    /// THE anti-drift test: the tooltip must never NAME something the click
    /// won't open. Before `resolve_region`, the click path and the text path
    /// each reimplemented the `coast_at` -> `region_at` order, so a fix to one
    /// silently missed the other.
    ///
    /// It guards *semantic* divergence, and the third arm is the one that
    /// bites: when a click opens nothing, the tooltip must not describe a node
    /// or workload as if it would. (It would NOT have caught the paired-session
    /// ocean split — there both paths agreed nothing was selectable and differed
    /// only in how much text they showed. That is fixed structurally instead,
    /// by there being one ocean branch.)
    #[test]
    fn the_tooltip_and_the_click_never_disagree() {
        use crate::draw::Hit;
        let (snap, _) = probe_fixture();
        let worlds = scene(&snap);
        // Every name the tooltip could utter that implies something openable.
        let mut names: Vec<String> = snap
            .hot
            .models
            .world
            .cities()
            .map(|c| c.r.name.clone())
            .collect();
        for cont in &snap.hot.models.world.continents {
            for p in &cont.provinces {
                names.push(p.tile.name.clone());
            }
        }
        let (bw, bh) = (snap.hot.models.world.width, snap.hot.models.world.height);
        let mut saw_sea_in_rect = false;
        for y in 0..bh {
            for x in 0..bw {
                let panel = panel_for(&worlds, Hit::at((x, y)));
                let lines = region_lines(
                    &worlds[0],
                    (x, y),
                    &snap,
                    Overlay::Terrain,
                    false,
                    NewGround::Off,
                );
                let text = lines
                    .iter()
                    .map(|(t, _)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                // Track that the grid actually contains the interesting case,
                // or this test is quietly vacuous.
                if matches!(
                    crate::draw::resolve_region(&worlds[0], (x, y)),
                    crate::draw::Resolved::Ocean
                ) {
                    saw_sea_in_rect = true;
                }
                match panel {
                    Some(Panel::City(_, r)) => assert!(
                        text.contains(&r.name),
                        "({x},{y}) click opens {} but the tooltip says {text:?}",
                        r.name
                    ),
                    Some(Panel::Node(_, n)) => assert!(
                        text.contains(&n),
                        "({x},{y}) click opens node {n} but the tooltip says {text:?}"
                    ),
                    None => assert!(
                        !names.iter().any(|n| text.contains(n.as_str())),
                        "({x},{y}) the tooltip names something the click opens \
                         nothing for: {text:?}"
                    ),
                }
            }
        }
        assert!(
            saw_sea_in_rect,
            "the fixture produced no sea-inside-a-province-rect cell, so the \
             most interesting divergence went unexercised"
        );
    }

    /// A long workload name must no longer widen its clickable region — the
    /// view-side half of `city_hit_region_matches_the_settlement_not_its_name`.
    #[test]
    fn a_long_name_does_not_steal_the_node_beside_it() {
        use crate::draw::{Resolved, resolve_region};
        let (snap, (cx, cy)) = probe_fixture();
        let worlds = scene(&snap);
        // 10 cells east of a 20-char name: the node, never the workload.
        assert!(
            !matches!(
                resolve_region(&worlds[0], (cx + 10, cy)),
                Resolved::Region(kubernation_core::state::world::Region::City(..))
            ),
            "a long name still steals terrain east of its settlement"
        );
    }

    #[test]
    fn saturation_lines_name_strained_dims_and_peg_conditions() {
        use kubernation_core::state::saturation::saturate_node;
        // A pod-bound + DiskPressure node: the binding dims are named, the
        // condition is "(pegged)", and calm cpu/mem are omitted.
        let sat = saturate_node(Some(0.20), Some(0.30), 108, Some(110.0), &["Disk"]);
        let lines = saturation_lines(&sat);
        let joined: String = lines
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            joined.contains("pods 108/110"),
            "names the pod-slot strain: {joined}"
        );
        assert!(
            joined.contains("DiskPressure (pegged)"),
            "condition pegged, no %: {joined}"
        );
        assert!(!joined.contains("cpu"), "calm cpu omitted: {joined}");
        // High dims color CRIT.
        assert!(lines.iter().any(|(_, c)| *c == STONE_CRIT));

        // A fully-calm node yields one calm line.
        let calm = saturate_node(Some(0.2), Some(0.3), 10, Some(110.0), &[]);
        let cl = saturation_lines(&calm);
        assert_eq!(cl.len(), 1);
        assert!(cl[0].0.contains("calm"));
    }

    /// The unknown branch: a node with no dimensions must NOT read "calm".
    /// Without this the branch could be reverted undetected — which is exactly
    /// how the defect existed in the first place.
    #[test]
    fn saturation_lines_say_unknown_for_a_node_that_reports_no_capacity() {
        use kubernation_core::state::saturation::{NodeSaturation, saturate_node};
        for sat in [
            NodeSaturation::default(),
            saturate_node(None, None, 0, None, &[]),
        ] {
            let lines = saturation_lines(&sat);
            assert_eq!(lines.len(), 1);
            assert!(lines[0].0.contains("unknown"), "got {:?}", lines[0].0);
            assert!(!lines[0].0.contains("calm"), "an unearned all-clear");
            assert_eq!(lines[0].1, STONE_WARN, "worth seeing, not dim");
        }
    }

    /// The overlay says WHICH NODE; these lines must say WHICH DAEMONSET —
    /// without the names the map raises a question it can't answer and the
    /// operator leaves for `kubectl`.
    #[test]
    fn substrate_gap_lines_name_the_missing_daemonsets() {
        let join = |l: &[(String, Color)]| {
            l.iter()
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>()
                .join("|")
        };

        let one = substrate_gap_lines(&["cilium".to_string()], true);
        assert!(
            join(&one).contains("cilium"),
            "the specific daemonset is named: {}",
            join(&one)
        );
        assert!(one.iter().all(|(_, c)| *c == STONE_WARN), "one gap warns");

        let two = substrate_gap_lines(&["cilium".to_string(), "fluent-bit".to_string()], true);
        let t = join(&two);
        assert!(
            t.contains("cilium") && t.contains("fluent-bit"),
            "both: {t}"
        );
        assert!(t.contains('2'), "counted: {t}");
        assert!(two.iter().all(|(_, c)| *c == STONE_CRIT), "2+ gaps crit");

        // A covered node says so plainly, and calmly.
        let none = substrate_gap_lines(&[], true);
        assert_eq!(none.len(), 1);
        assert!(none[0].0.contains("complete"));
        assert_eq!(none[0].1, STONE_INK_DIM);

        // Nothing fleet-wide is NOT a clean bill of health — it must not read
        // "complete", which would imply a check that never ran.
        let no_data = substrate_gap_lines(&[], false);
        assert!(
            !join(&no_data).contains("complete"),
            "an unearned all-clear: {}",
            join(&no_data)
        );
        assert!(join(&no_data).contains("no fleet-wide"));
    }

    #[test]
    fn cost_lines_selection_unitless_and_unpriced() {
        let nc = NodeCost {
            per_hour: 6.0,
            idle_per_hour: 3.0,
            used_frac: 0.5,
            priced: true,
            mode: cost::CostMode::Unitless,
            basis: CostBasis::Requests,
            overcommitted: false,
        };
        let lines = cost_lines(&nc);
        let txt: String = lines
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            txt.contains("upkeep") && txt.contains("units") && !txt.contains('$'),
            "{txt}"
        );
        assert!(txt.contains("idle 50%"), "{txt}");
        // The idle is notable (50% ≥ 40%) → on-stone cyan.
        assert!(lines.iter().any(|(_, c)| *c == STONE_STRUCT));
        // An unpriced node says so, never a false 0.
        let up = cost_lines(&NodeCost {
            priced: false,
            ..Default::default()
        });
        assert!(up[0].0.contains("unpriced"));
    }

    #[test]
    fn sparkline_points_map_values_into_the_rect() {
        let r = Rect::new(10.0, 20.0, 100.0, 40.0);
        // 0 → bottom, max → top; evenly spaced across the width.
        let pts = sparkline_points(&[0.0, 0.5, 1.0], 1.0, r);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0], vec2(10.0, 60.0)); // value 0 → bottom (y0+h)
        assert_eq!(pts[1], vec2(60.0, 40.0)); // value .5 → middle
        assert_eq!(pts[2], vec2(110.0, 20.0)); // value 1 → top (y0), right edge
        // Over-max clamps to the top, not above it.
        let over = sparkline_points(&[2.0], 1.0, r);
        assert_eq!(over[0].y, 20.0);
        // Degenerate inputs yield nothing (the draw helper then skips the line).
        assert!(sparkline_points(&[], 1.0, r).is_empty());
        assert!(sparkline_points(&[0.5], 0.0, r).is_empty());
    }
}
