# The rustdoc lint pass

**Follows:** `docs/reports/prose-audit.md` §2.6, §6 ("recommended as its own small pass")
**Version:** 1.33.1 · **Date:** 2026-08-29

The audit left seven doc links unresolved and recorded them as **cosmetic, names
verified, cause not isolated**. The cause is isolated, and it was not cosmetic —
it was §4's subject, one level up.

**Prose now has a compiler for the one class a machine can check**, denied at
both crate roots and run in CI.

---

## 1. The cause: two doc comments on one module

Every one of the seven links named an item that exists and is public, yet
rustdoc reported *"no item named `MAX_RESP_BYTES` in scope"* — from inside the
module that defines it.

`k8s/mod.rs` carried an **outer** `///` doc on `pub mod adapter;` while
`adapter.rs` carried its own **inner** `//!` header. When both exist, rustdoc
resolves the file's own links in the **parent** module's scope — where
`adapter`'s items are not visible. `super::opencost` failed the same way: from
`k8s`'s scope, `super` is the crate root.

Confirmed by experiment rather than argued: stripping the outer docs took the
count **7 → 0**.

So the broken links were a *symptom* of the same defect the audit spent its time
on — two homes for one claim — and the compiler had been reporting it in a form
nobody read. Three modules had it (`adapter`, `opencost`, `oracle_client`);
all three outer docs were pure duplication, checked line by line against the
inner headers before deletion, which are strictly richer in every case.

`k8s/mod.rs` now carries a plain `//` comment saying why a `///` must not go
back — a non-doc comment, so it cannot recreate the problem it warns about.

---

## 2. What that turned up in passing

`oracle_client.rs`'s header — the Oracle's egress module, where the doc matters
for the posture — opened:

> *"It does ONE thing: a single non-streaming POST…"*

False three ways. `consult_stream` (SSE, **v0.59.0**), `probe`, and
`list_models` (**v0.53.x**) all live there. The header now names all four calls
and says which change falsified the old sentence, so the correction carries its
own reason (§4's shape).

Found only because the lint pass sent me into a file the audit's §3.1 list did
not point at — which is the argument for the guard rather than another audit.

---

## 3. The guard, and where its line is

`#![deny(rustdoc::broken_intra_doc_links)]` at both crate roots, plus a
`cargo doc --workspace --no-deps --locked` step in CI. Nothing else in CI invokes
rustdoc, so without the step the deny is inert.

**Deliberately NOT `-D warnings`.** A second lint fires — nine
`private_intra_doc_links`, public docs linking to private items (`ChangeSince`,
`sum_pod_requests`, `json_blocks`, …). Every one names an item that **exists**;
only the hyperlink fails for a reader of the rendered public docs, and the reader
§2 of the audit cares about is the next editor, reading source, where the name is
correct.

The distinction is the audit's own, applied to itself:

> **broken = a false name. private = a true name that will not hyperlink.**

Denying the second would push toward widening a type's visibility to satisfy a
documentation tool, which is the tail wagging the dog. Left as a warning, counted
here so a future round knows the number was chosen rather than overlooked.

---

## 4. Mutation floor

| | mutation | |
|---|---|---|
| Q1 | a doc link names a function D1 renamed (the real defect, re-introduced) | caught |
| Q2 | a second doc home reappears on a module | caught |
| Q3 | a link to an item that never existed | caught |

Q1 first reported NOT APPLIED — the target appears twice, and the harness
asserts a single occurrence. Re-run against both. That assertion has now caught
a bad mutation nine times this stretch; without it Q1 would have been recorded as
"caught" on the strength of one of two edits landing.

---

## 5. Acceptance

- [x] The seven unresolved links understood, not silenced — cause isolated by experiment (§1)
- [x] Duplicate module docs removed, nothing lost (checked against each inner header)
- [x] `broken_intra_doc_links` denied at both crate roots
- [x] CI runs rustdoc, so the deny is not inert
- [x] The second lint's status decided explicitly and counted, not ignored (§3)
- [x] Mutations asserted applied
- [x] `cargo nextest` green — 623 tests; clippy clean with and without features

**Deferred:** `cargo doc` runs on all three matrix OSes, which is redundant for a
platform-independent check — folding it into one job is a workflow tidy, not a
correctness fix, and would trade a little waste for a job-graph change.
