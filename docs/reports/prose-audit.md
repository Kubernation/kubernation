# The prose audit

**Guidance:** `docs/kubernation-prose-audit-guidance.md`
**Version:** 1.33.0 · **Date:** 2026-08-29

Nine false claims found and corrected, one mechanism given a single home, and
the license guard closed first as the guidance directs. **The audit's own
known-open item was already closed**, which is §6 question 5 firing on the
guidance before any code was read.

---

## 1. §1's claims check, applied to the guidance itself

**§3.2's "known-open item" is not open.** The doc says `--dump-positions` names
a field `node` that a reader takes for "where this workload runs", marks it
**still open**, and makes closing it acceptance item 4.

It was closed in **v1.28.1**. The city record emits `plurality_node`, with the
reasoning at the emitter; `hack/churn/positions.py` and its self-tests consume
that name. Province records still say `node`, which is correct — a province *is*
its node, and v1.28.1 scoped the rename to the record that was wrong.

So acceptance item 4 is moot. The guidance inherited it from handoff §4, and the
handoff inherited it from a report — the failure mode §6 names, with the author's
own week-old claim again the one that was wrong.

**§0's other four rows check out.** `SITING_CLAIM` is one constant with two
consumers; `focus_impact`'s doc correctly states that it deliberately does not
select, and why; the truncated selection line was reshaped. Verified by reading,
not assumed from the reports.

---

## 2. The findings

### 2.1 The one that matters most — the write file describes the wrong write

`k8s/actions.rs`'s module header is the highest-stakes doc comment in the
codebase for the privilege posture: it is the file's own statement of what the
one write surface does. Three claims in it were false.

| said | actually |
|---|---|
| "pod eviction (a delete)" | the `pods/eviction` subresource since **v1.30.0** — the change whose entire purpose was that a delete does *not* enforce disruption budgets |
| planning-turn intervention "(scale a workload, cordon a node)" | five verbs: scale, cordon, restart, set-image, rollback |
| "Both frontends call it" | one frontend since **2026-06-18** |

The first is the sharpest: a reader auditing the write surface would have been
told the app deletes pods, which is exactly the behaviour v1.30.0 removed *and*
the reason it was removed.

### 2.2 The evict button's RBAC verb, again

`window::evict_button`'s doc said `Some(false)` means "no delete permission".
v1.31.0 established that eviction is authorized under `create pods/eviction`,
separately grantable from `delete pods` — proven in both directions on a live
cluster. The probe was fixed; **its doc was not**. The rendered label ("locked")
was right, so nothing user-facing was wrong; the next editor was the reader being
misled, which is §2's third kind.

### 2.3 The v1.26 sweep was not complete

`state/world.rs`'s module doc still read *"cities sited on the province hosting
**most** of their pods"* — the exact falsehood v1.26.0 existed to remove, in the
module that **implements** the siting. The sweep covered the almanac (user-facing)
and the test guards the almanac text, so nothing was looking here.

§3.1 asks "check the sweep was complete." It was not.

### 2.4 The field guide named eight of nine overlays

The Controls page describes the View menu in prose. `Pool` shipped in **v1.14.0**
and the sentence went **fourteen versions** naming eight — so the one surface
whose job is to tell an operator what the menu contains was the surface that did
not know.

Three places listed the overlay set: `Overlay::ALL` (the authority), `menu.rs`
(nine hand-written rows, correct), and this sentence (eight, wrong).

**§4's fix shape, taken:** the sentence is now BUILT from `Overlay::ALL`, so a
tenth overlay appears in the guide by construction. The `SITING_CLAIM` pattern's
second instance, and the tenth one-home-for-the-rule in this codebase.

The menu's rows are *not* looped — each carries its own display label and check
state — so they got the guarantee as a test instead: every `Overlay::ALL` variant
must have a View row. Recorded as the weaker form, deliberately.

### 2.5 Present-tense claims about a removed frontend

`browse.rs` said "(the TUI has the keyboard/filter idiom)" and `draw.rs` said
"the coordinate system both frontends share". The TUI was removed 2026-06-18.

`logline.rs` is the counter-example and the model: *"originally shared by the TUI
and GUI log views; the TUI is gone but the split still earns its keep"* — §4's
shape exactly, stating what the code does and why the arrangement is deliberate.
The corrections follow it rather than deleting the history.

### 2.6 Broken doc links — a compiler for prose, switched off

`cargo doc` reports unresolved intra-doc links, and this is the one class of
prose claim a machine *can* check. Fifteen warnings, of three kinds:

- **2 genuinely wrong names.** `window::window_rect` (×2), renamed to
  `window_rect_at` by **D1**, which made it "the ONE home for placement" — so the
  doc pointed at a function that no longer exists, in the comment explaining that
  placement has one home.
- **2 unlinkable but correct** — an unqualified method (`zone_ordinal` →
  `Layout::zone_ordinal`) and a `pub(crate)` item; corrected.
- **4 lint false positives** — notational brackets (`[0,1]`, `[0,360)`,
  `[default]`, `[standard]`) that rustdoc reads as links. Escaped.
- **7 left, cosmetic.** All in `k8s/adapter.rs`'s "pattern for a new adapter"
  header plus one in `opencost.rs`. Every referenced item was checked at its
  definition and **exists and is public**, so the words mislead no one; only the
  hyperlink fails to render, for a rustdoc reason I did not isolate. Recorded as
  **unresolved-cause, names verified** rather than as correct — §5's discipline.

---

## 3. The license guard, closed first

As §7 of the guidance directs. Diagnosed by reproducing rather than reading the
log: `cargo install --locked cargo-about` pins cargo-about's *dependencies*, not
cargo-about, so the job tracked the latest release and 0.9.0 → 0.9.2 changed the
output structure. **Same 208 crates, identical attributions, 110 → 129 section
headers.** No legal impact.

Pinned to 0.9.2, regenerated with it, and the error message now names the pinned
version. The About window's license claim (ISC, BSD-3-Clause, Zlib, Unicode-3.0)
still holds against the regenerated file; its guard test passes.

---

## 4. Mutation floor

| | mutation | |
|---|---|---|
| P1 | the field guide restates the overlay set instead of building it | caught |
| P2 | an overlay loses its View menu row | caught |
| P3 | the siting claim reverts to "most" | caught |

---

## 5. §6 standing questions

**8 (new). True *and* sufficient?** The audit's own §3.2 is the case: "the field
is named `node`" was *true when written* and insufficient as a statement about
now. And §2.2 is the same shape one level down — "the probe checks permission" is
true and says nothing about *which* permission, which is exactly where it was
wrong.

**5. Inherited claims?** §0 and §3.1 are inherited from reports, as the guidance
says. One of five was stale (§1). Fourth consecutive round in which the wrong
claim was the author's own, from the same week.

**3. Two sections constraining one behaviour?** §2.4 — three places listing the
overlays. Resolved to one authority plus a guard, not to a comparison test.

---

## 6. Acceptance

- [x] §3.1's surfaces enumerated and checked against source
- [x] Every correction states what the code does; §2.5 states why the history is kept
- [x] One authority where two claims described one mechanism (§2.4)
- [~] `--dump-positions` renamed — **already done in v1.28.1** (§1)
- [x] Unchecked surfaces recorded as unchecked (§2.6's seven)
- [x] The claims check applied to the audit's own inherited claims (§1, §5)
- [x] `cargo nextest` green — 623 tests

453 core (478 with `oracle`) + 145 GUI; gui-smoke 57; clippy clean both ways.

**Not done, deliberately.** Denying `rustdoc::broken_intra_doc_links` would turn
§2.6's class into a build failure and is the durable form of this fix — but it
needs a `cargo doc` step in CI, which is beyond an audit's scope and wants the
seven cosmetic warnings understood first. Recommended as its own small pass.
