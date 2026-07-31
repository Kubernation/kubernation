# KuberNation — `Overlay::Substrate`

**Implementation guidance**
**Question it answers:** which nodes are missing infrastructure the rest of the fleet has.

---

## 1. Scope, and one correction

**In scope:** DaemonSet coverage gaps, as an eighth overlay on the existing axis.

**Out of scope — and this corrects the original proposal:** kubelet pressure marks. `NodeTile::saturation` is already documented as "worst of cpu/mem/pod-count + the kubelet Disk/Mem/PID-pressure conditions," and `Overlay::Saturation` already renders it. Putting pressure into Substrate would be a second path to a fact the map already shows.

There is a real gap nearby — Saturation *blends* pressure with utilisation, so the map can't tell you which caused a hot node. But that argues for better marks on Saturation, not for smuggling pressure into a different overlay. Keep overlays single-purpose.

So: **Substrate is DaemonSet coverage and nothing else.**

---

## 2. The correctness crux: what "expected" means

This is the whole feature. Everything else is wiring.

A naive definition — `union(all DaemonSets) − node.infra` — over-reports badly, because DaemonSets legitimately don't run everywhere. `nodeSelector`, taints and tolerations, and node affinity all produce *correct* absences. A GPU device plugin missing from your CPU nodes is not a finding.

**Use prevalence.** For each DaemonSet name, count the nodes carrying it. Treat it as expected only if it is on most of them:

```rust
/// A DaemonSet on at least this share of nodes is treated as fleet-wide, so
/// its absence is a finding rather than a nodeSelector doing its job. A
/// heuristic, deliberately: the model has no access to the DaemonSet's spec,
/// so prevalence is the only evidence available that it was MEANT to be
/// everywhere.
const FLEET_PREVALENCE: f64 = 0.8;
```

Consequences to accept and document:

- **Small clusters under-report.** On four nodes, a DaemonSet on three is 75% and won't be flagged. That's fine — the feature earns its keep at fleet scale, which is exactly what the cutaway fork established.
- **Newly scaled-up nodes show gaps** until their DaemonSet pods land. This is arguably a feature (rollout progress is visible) but it is noise on an autoscaling cluster. Name it in the Almanac entry.
- **NotReady nodes may show gaps** because their pods went Unknown. The Terrain overlay already colours NotReady, so the operator has context — but a node red in Terrain and red in Substrate is telling you one thing, not two.

---

## 3. Core: `SubstrateReport`

Model it on `CostReport` — a whole-cluster rollup with per-node lookup plus fleet-wide context. New file `crates/kubernation-core/src/state/substrate.rs`.

```rust
/// Whole-cluster DaemonSet coverage. One report feeds the overlay + SELECTION.
#[derive(Debug, Clone, Default)]
pub struct SubstrateReport {
    /// DaemonSets present on >= FLEET_PREVALENCE of nodes, sorted. Empty when
    /// the cluster has no fleet-wide DaemonSets at all — the overlay falls
    /// back to terrain in that case rather than colouring everything "clean".
    pub expected: Vec<String>,
    /// Per node: the expected DaemonSets it lacks. Absent key == fully covered.
    pub missing_by_node: HashMap<String, Vec<String>>,
    pub nodes_total: usize,
    /// Nodes with at least one gap — the legend's headline number.
    pub nodes_with_gaps: usize,
}

impl SubstrateReport {
    /// PURE, and derived entirely from the world — unlike cost, this needs no
    /// external pricing source, which makes it cheap and trivially testable.
    pub fn from_world(world: &WorldModel) -> Self { … }
}
```

**Where it lives:** compute it in `build_world` and hang it on `WorldModel`. `build_world` already walks every province, so prevalence is nearly free there, and it means one computation per model build rather than per frame.

**Why per-world and not per-snapshot:** in a paired hot/warm session the two clusters have different DaemonSets, so each needs its own expected set. Deriving from `WorldModel` gets this right for free; a snapshot-level report would not.

---

## 4. View: the colour decision

`Overlay::Cost` is the precedent for an overlay needing fleet-wide context. Thread the same way:

```rust
fn overlay_pair(
    overlay: Overlay,
    prov: &Province,
    walls: Option<&WallData>,
    cost: Option<&CostReport>,
    substrate: Option<&SubstrateReport>,   // new
) -> (Color, Color)
```

**Do not use a continuous ramp.** Cost's bronze choropleth is right for a normalised ratio; substrate gaps are small integers where zero is the overwhelming common case. A ramp would render a healthy fleet as a wash of near-identical tints.

Discrete, with the good state receding:

| Gaps | Pair | Rationale |
|---|---|---|
| report absent, or `expected` empty | `iso_terrain_pair(health)` | Coverage's precedent for "no data to show" |
| 0 | `idle_land_pair()` | Cost's precedent — the uninteresting case recedes so anomalies pop |
| 1 | warn pair | |
| 2+ | crit pair | |

```rust
Overlay::Substrate => substrate
    .filter(|r| !r.expected.is_empty())
    .map(|r| match r.missing_by_node.get(&prov.tile.name).map_or(0, Vec::len) {
        0 => idle_land_pair(),
        1 => heat_pair(WARN_LEVEL),
        _ => heat_pair(CRIT_LEVEL),
    })
    .unwrap_or_else(|| iso_terrain_pair(prov.tile.health)),
```

**Minimap.** `overlay_flat` falls back to terrain for `Coverage` and `Cost` because their per-node data isn't threaded to the overview. Substrate joins that list — add it to the same match arm and say why in the existing comment.

**Colour-blind.** Route the warn/crit pairs through the existing `cb_*` funnel like every other meaning-bearing colour. Substrate is instrumentation, not scenery.

---

## 5. Wiring checklist

Follow `Overlay::Cost`'s trail — it is the most recently added variant and touches every site.

- [ ] `Overlay::Substrate` variant + `Overlay::label` → `"substrate"`
- [ ] `overlay_from_str` — `"substrate" => Overlay::Substrate`
- [ ] `overlay_pair` — new arm and new parameter
- [ ] `overlay_flat` — terrain fallback, alongside Coverage/Cost
- [ ] Every `overlay_pair` call site threads `substrate`
- [ ] `menu.rs` — new item in the `MAP OVERLAY` group
- [ ] `--overlay substrate` works (no new flag needed; it's an existing enum-valued flag)
- [ ] Prefs round-trip — persisted as a string, no `PREFS_VERSION` bump
- [ ] `almanac::page_legend` — a Substrate entry stating the prevalence heuristic in plain terms
- [ ] SELECTION / province window — list the specific missing DaemonSet names, since SUBSTRATE already shows what *is* there

That last one matters. The overlay tells you *which nodes*; the panel must tell you *which DaemonSets*. Without it the map raises a question it can't answer, and the operator goes to `kubectl` — which is exactly the failure Round 1 hit.

---

## 6. Tests

`SubstrateReport::from_world` is pure and needs no GL context, so most of the value is testable directly.

- [ ] a DaemonSet on every node is expected, and nobody has gaps
- [ ] a DaemonSet on one node out of ten is **not** expected — the nodeSelector case
- [ ] a DaemonSet on nine of ten is expected, and the tenth node has one gap
- [ ] `expected` is sorted (stable rendering across frames)
- [ ] an empty cluster, and a cluster with no DaemonSets, both produce an empty report without panicking
- [ ] `nodes_with_gaps` counts nodes, not gaps — a node missing three DaemonSets counts once

Plus the overlay-level tests mirroring `pressure_overlay_heats_by_bucket`:

- [ ] 0 gaps recedes to idle land; 1 warns; 2 crits
- [ ] an empty `expected` falls back to terrain rather than colouring everything clean
- [ ] the variant round-trips through `overlay_from_str` ⇄ `label`

---

## 7. Honest limitations to put in the Almanac entry

Prevalence is inference, not intent. The model never sees the DaemonSet spec, so it cannot distinguish "should be here and isn't" from "correctly excluded by a nodeSelector you wrote deliberately." At 80% the false-positive shape is predictable — targeted DaemonSets on most-but-not-all nodes — and stating that plainly is better than implying certainty the data doesn't support.

This is the same honesty discipline as `fmt_hourly` refusing to print `$` for a unitless cost basis.

---

## 8. Estimate

| Work | |
|---|---|
| `SubstrateReport` + `from_world` + core tests | ~0.5 day |
| Overlay wiring, following the Cost trail | ~0.5 day |
| Panel integration (missing names in SELECTION) + Almanac entry | ~0.5 day |

**~1.5 days.** No new architecture: one variant on an axis that already carries seven, one derived report modelled on one that already exists, and no rendering work beyond a colour decision.
