# KuberNation — D2 §3.3: The Inversion

**Implementation guidance**
**Goal:** the selection becomes an identity; its position is derived per frame.
**Gate:** a selection survives a reschedule and a zone addition, pointing at the right thing.

D2's remaining phase. D2-fix made the mutation floor able to detect a regression here.

---

## 0. Why the selection is inverted

`selected: Option<(u16, u16)>` is a **scene cell**, and it is wrong silently in two independent ways:

1. **The subject moves.** A city sites at its pods' plurality node, so any reschedule that shifts the plurality moves it.
2. **The scene shifts.** A warm cell is `local + off`, where `off` is the hot world's width — so **adding a zone to the hot cluster moves every stored warm cell.**

Neither produces an error. The selection quietly starts pointing at a different province.

D2's pre-check also established the shape of the fix: **five of seven readers immediately convert the cell back to an identity.** The state is stored as a position and consumed as an identity by most of its consumers. Inverting it makes both staleness sources dissolve — a moved city resolves to its new position, a shifted scene resolves through the current `off`.

**And it dissolves a third thing for free.** D2's gate found that `region_at` tests a province's *rectangle* while `resolve_region` applies the coast carving, so a click on carved-away sea yields a selection the tooltip calls ocean and the blast subject calls a node. An identity is a node or it is not; no ambiguous cell survives to disagree about.

---

## 1. Verify before building

Everything is `[A]`. **D2-fix moved several of these**, so the inventory in §3 is the part most likely to be stale.

| # | Claim | Source |
|---|---|---|
| 1 | `selected: Option<(u16, u16)>` is a SCENE cell; warm cells carry `+ sw.off` | D2 rev2 claims 2–3 |
| 2 | `off = hot.models.world.width + WORLD_GAP` | D2 rev2 claim 4 |
| 3 | `city_pos(&WorkloadRef)` and `province_pos(&str)` map identity → position | D2 rev2 claim 5 |
| 4 | `subject_at(worlds, cell) -> Option<(ClusterId, Subject)>` is the collapsed authority, in `draw.rs` | D2-fix §2 |
| 5 | `blast_subject`, `selected_scope`, `city_at` are in `draw.rs`; `impact_panel` in `panels.rs`; each has **one** production caller in `main.rs` | D2-fix §2 |
| 6 | `main.rs` no longer imports `Region` — the conversions are gone from it | D2-fix §2 |
| 7 | `hovered` is already separate from `selected`; `sidebar_sel = selected…or(hovered)` | D2 rev2 §3.5 |
| 8 | The structural guard confines `region_at` callers to `draw.rs` | D2-fix §5 |
| 9 | `draw_selection` genuinely wants a cell and was deliberately not moved | D2-fix §2 |

**Claim 5 is the one to re-read.** D2 rev2 §4 enumerated seven readers and eight writers of `selected` **before** D2-fix moved four decisions out. That list is now wrong in detail, and §3 depends on the corrected version.

---

## 2. The shape

The selection holds what `Panel` holds — an identity, including its `ClusterId`, which a bare cell does not carry at all.

Position is derived per frame via `city_pos` / `province_pos` (claim 3).

### 2.1 `draw_selection` is the one place that still wants a cell

Claim 9. It takes the **derived** position and must handle `None`: a selected workload that has left the cluster has no position.

**That `None` is a decision, not a check** — see §5.

### 2.2 Do not derive once and cache

Caching the derived position reintroduces exactly the staleness being removed, one layer up. Derive where it is used, each frame.

If profiling later shows that matters, the fix is memoising against a frame counter, not storing the position in the selection.

---

## 3. Re-enumerate before editing

D2 rev2 §4 listed fifteen sites. D2-fix changed that list.

**Run the enumeration again and record it**, per D2-fix's own precedent: its §3.2 found a hand-rolled conversion nobody knew about precisely because the gate forced a caller sweep.

Expected shape after D2-fix, to be confirmed rather than assumed:

| Readers | Now via |
|---|---|
| Oracle scope | `selected_scope` |
| blast subject | `blast_subject` |
| `Enter` → panel | `panel_for` |
| IMPACT-row focus | `impact_panel` |
| `draw_selection` | direct — the cell consumer (§2.1) |
| SELECTION box (`sidebar_sel`) | direct |
| concern nav | `&mut` |

**Writers are the harder half** and were not moved: map click, `]`/`[` sail, `--inspect` (city and node arms), concern nav, IMPACT-row focus, almanac locate, cleared on context switch and on `Esc`.

Every writer currently produces a *cell*. After the inversion each must produce an *identity* — and a writer that only has a cell (the map click) must convert through `subject_at`, which is now the one authority for exactly that.

**The map click is the interesting one:** it is the only writer whose natural input is a position. Everything else already knows what it selected.

---

## 4. Tests

- [ ] A selected workload whose city **moves** resolves to the new position, not the old — the first staleness source, and the reason for the phase
- [ ] A warm-world selection survives the hot world **growing a zone** — the second source, and the less obvious one
- [ ] A selection made by map click on carved-away sea resolves to the same thing the tooltip says — the free dissolution (§0)
- [ ] Every writer produces an identity; the map click converts through `subject_at`
- [ ] Hover does not persist; commit does
- [ ] Closing the panel does not clear the selection
- [ ] A subject that has left the cluster resolves to `None` and is **said**, not drawn at a stale position

**Mutation floor, asserted applied** — five false survivals this session from `cargo fmt` reflowing targets:

- Make the derivation cache its result → the moved-city test must fail
- Make a writer store a cell instead of an identity → a writer test must fail
- Make `draw_selection` treat `None` as "draw nothing" silently → the stale-subject test must fail

The first is the one that matters. **§2.2's hazard is invisible in review** — a cached position looks like an optimisation and behaves correctly until something moves.

---

## 5. The decision this phase must make

**A selection whose subject has left the cluster is stale, not absent.**

Three options, and the requirement is that the SELECTION box **says which**:

| | Behaviour |
|---|---|
| **Clear** | The selection disappears when its subject does |
| **Tombstone** | It persists, marked as gone, until dismissed |
| **Refuse** | It cannot be set for something already absent |

A silent disappearance and a silent stale mark are both wrong in the same way. This codebase has refused that shape repeatedly — `SubstrateReport` falling back to terrain, `GroundState::Unknown` reaching the panel, `extent_line` speaking a guessed size.

I lean **tombstone**, weakly: a workload vanishing while you have it selected is itself information, and clearing hides it. But the SELECTION box has to carry the wording, and that is worth looking at before committing.

---

## 6. What this does not do

- **No camera movement on selection.** Marking is not navigation — D4. `aim_for_drilldown` fires once on *open* and must not be extended.
- **No where-am-I marker during scroll** — D3.
- **No third selection level.** Hover and commit, no more (claim 7).
- **No selection for rows without a map position**, and the refusal must be visible.
- **No namespace swatches.** D2 rev2 §2 refuted them; they are a separate legibility change if wanted at all.

---

## 7. Standing questions — written answers required

1. Where does a summing step precede a comparing step?
2. Does every reducer over a possibly-empty input express unknown, or fabricate?
3. Where do two sections constrain the same behaviour — and is there a fixture where they diverge?
4. What existing consumers depend on the old meaning of a value this change redefines?
5. Which claims are inherited rather than verified — and does the state each describes actually occur?
6. When a change moves one side of a comparison, does the other side still mean the same thing?
7. Where does the code treat neighbouring entries in a container as neighbouring things in the world — and what guarantees that?

**Question 6 is the phase.** `selected` currently means *a scene cell*; after this it means *an entity*. Every site in §3 must still mean the same thing about the same subject, and the ones that want a position must get it from one derivation.

**Question 4:** re-enumerate rather than trusting §3's expected shape (claim 5). D2-fix's own finding was that the consumer which bites is the one not named — and its list was hours old when it went stale.

**Question 2 is §5**, and it is a decision rather than a check.

---

## 8. The gate

**A selection survives a reschedule and a zone addition, pointing at the right thing.**

Both are constructible: the churn fleet reschedules on demand, and scenario 4 or 5 adds and removes a nodepool. Run both, positionally — `--dump-positions` before and after, comparing what the selection resolves to, not what it looks like.

### 8.1 Check the metric can discriminate first

D1's occlusion figure conflated covering the map with moving it, and the honest number was geometric rather than a pixel diff. Here the equivalent trap is measuring that the *mark* moved — it should move, because the subject did. **What must be verified is that it moved to the right place.**

That is a positional comparison against the identity's current `city_pos`, not a before/after image.

### 8.2 The discrimination check

Run both gates against a build with the inversion reverted. **Both must fail** — the pre-inversion selection is stale in exactly these two cases, which is the phase's entire justification.

If either passes on the old build, that case was not actually broken and the reason should be found before the fix is credited.

---

## 9. Acceptance

- [ ] §3's site inventory re-enumerated and recorded, not inherited
- [ ] Selection is an identity carrying `ClusterId`; position derived per frame
- [ ] No cached derived position (§2.2)
- [ ] Both staleness sources tested, and both fail on the reverted build (§8.2)
- [ ] The carved-sea divergence dissolves, and is asserted to
- [ ] §5 decided, and the SELECTION box says which
- [ ] `draw_selection` handles `None` audibly
- [ ] Mutations asserted applied
- [ ] Standing questions answered, claims tagged
- [ ] `cargo nextest` green

---

## 10. Estimate

**One day.** The inversion itself is small; §3's writers are the bulk, and §5 needs the SELECTION box in front of you.

D2-fix bought the thing that makes this safe: a regression here now fails a test rather than passing silently in `main.rs`.
