# The Windows notices gap

**Follows:** `docs/reports/prose-audit.md` §6, the last open item from the v0.62.0
About review
**Version:** 1.33.2 · **Date:** 2026-08-29

`about.toml` pinned macOS and Linux while `release.yml` had shipped a Windows zip
since **v0.65.0** — and that zip carries `THIRD-PARTY-NOTICES.md`. So for six
releases the licence notices distributed with the Windows binary omitted the
crates only that binary links.

---

## 1. What was missing

cargo-about resolves `cfg` per pinned triple, so a platform absent from the list
contributes none of its crates. Adding `x86_64-pc-windows-msvc` took the count
**MIT 202 → 208**:

| crate | |
|---|---|
| `schannel` | **Windows' TLS implementation** — the one that matters |
| `windows-sys`, `windows-link`, `winapi` | Windows API bindings |
| `anstyle-wincon` | console styling |
| `once_cell_polyfill` | |

No crate was lost, and no new licence *kind* appeared — all six are MIT — so
`accepted` is unchanged and the About window's claim (ISC, BSD-3-Clause, Zlib,
Unicode-3.0, named after the v0.62.0 review found the old wording affirmatively
false) still holds. Its guard test passes.

Both facts were checked rather than assumed: the crate sets were diffed in both
directions, and the licence-section headers compared.

---

## 2. The guard, and why a lint

Nothing compared the two lists. `about.toml` was correct about what it pinned and
`release.yml` was correct about what it built; they simply disagreed, and no build
step looked. It was **recorded in the v0.62.0 review and still true at v1.33.1** —
which is how long a two-list agreement survives unchecked.

`hack/check-release-targets.sh` asserts every platform `release.yml` ships has its
triples in `about.toml`, in `make lint` and CI beside
`check-conversion-authorities.sh`. A lint rather than a test because neither file
is reachable from Rust, and the failure is a legal-accuracy defect in a shipped
artifact rather than a behaviour a test could observe.

---

## 3. The finding: the guard was broken, and the mutation floor is what said so

| | mutation | first run | after |
|---|---|---|---|
| R1 | the Windows target is removed — **the original defect** | *** SURVIVED *** | caught |
| R2 | release.yml ships a platform the guard has no triples for | caught | caught |

R1 is the one the guard exists for, and it passed.

The cause: the matrix parse used `[a-z0-9-]+`, **without the underscore**. So
`linux-x86_64` and `windows-x86_64` — which contain `_` — failed the `$` anchor
and were silently skipped, while `macos-universal` matched. The guard had only
ever been checking macOS, and reported success in the same words either way.

R2 passed anyway and would have been reported as proof the guard works: the
platform it happens to rename to, `linux-aarch64`, contains no underscore. **One
mutation passing for an accidental reason nearly certified a guard that checked
one of three platforms** — the nineteenth catalogued case in this project of an
instrument producing a plausible answer for a reason unrelated to what it claimed
to measure, and the first where the instrument was a guard written in the same
sitting.

Fixed, and given a guard-the-guard: the script now fails if it extracts fewer
than three platforms, so a future parse regression cannot pass quietly. That
assertion is the durable part — the character class was one bug, but *"the parse
silently matched less than it should"* is the shape that recurs.

**bash 3.2, again.** The first draft used `declare -A`, which macOS's bash does
not have — the same version whose empty-array behaviour under `set -u` broke the
macOS release script during its dry run. Rewritten as a `case`; `shellcheck` and
`actionlint` clean.

---

## 4. Acceptance

- [x] `about.toml` names every target `release.yml` builds
- [x] Notices regenerated with the pinned cargo-about 0.9.2; CI's own `diff -uB` comparison in sync
- [x] No crate lost, no new licence kind, About window's claim re-verified
- [x] Guard in `make lint` + CI; both drift directions mutation-tested
- [x] The guard's own parse guarded, after it failed the one mutation that mattered
- [x] `cargo nextest` green — 623 tests

**This closes the last item carried from the v0.62.0 About review.**
