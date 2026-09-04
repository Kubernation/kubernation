# Caveats wrap on every advisor page — declared, not guessed

**Prompt:** "the other seven advisor pages wrap their Dim caveats the way
Substrate's page does, without inheriting the indent-stripping regression."
**Version:** 1.39.0.
**Gate:** on one page, a real caveat wraps and reads to the end while a real
dimmed row truncates — **PASSED**, with a before/after discrimination on the
same page and window.

The ask could not be done as written. Routing the other pages through
`is_prose` would have wrapped hardening's INFO section, cost's system-namespace
rows, posture's Info factors and every `+N more` trailer — all dimmed,
unindented **rows**. The fix is to stop guessing.

---

## 1. Verify first — two premise corrections and the answer to the question asked

**It is six pages, not eight.** `page_health` and `page_storage` do not use the
`*_lines` + role pattern at all; they call `cx.heading` / `cx.stat` / `cx.note`
directly and have no caveat lines to wrap. The pages with the pattern are
Right-sizing, Cost, Hardening, Substrate, Posture and Network (its WALLS
section).

**"Whether any of the seven already has a dimmed row" — yes, five of the six
do**, and the answer changes the design rather than just naming a gate:

| page | dimmed ROWS |
|---|---|
| Cost | system-namespace rows (`if ns.system { Dim }`), `+N more` |
| Hardening | **the entire INFO (hygiene) section** — `hardening_section(.., RsRole::Dim)` |
| Posture | `FactorKind::Info` factors |
| Substrate | the tagged node rows (v1.38.0) |
| Network/WALLS | `+N more` |
| Right-sizing | `+N more`, count-strip cells |

**None of them is indented.** So `is_prose` — `Dim && !starts_with(' ')` —
classifies every one as prose. It is correct on the Substrate page, where the
dimmed rows happen to be indented, and false on the five others. Routing them
through it would not have inherited the indent-stripping regression; it would
have introduced a different one, wrapping every dimmed row on five pages.

That is the finding, and it was reached by reading `hardening_section` rather
than by trusting a grep over literals.

---

## 2. The fix: the emitter declares, the renderer obeys

`RsRole::Caveat` — prose; **wraps**. `RsRole::Dim` — a dimmed row; **truncates**.
Both render in the same dim ink, because the distinction was never colour.

Inferring it from the text is what does not generalise: `Dim` had been carrying
two meanings, and no property of the *text* separates hardening's INFO row from
hardening's footer — neither is indented, both are long, both are dim. The
function that builds the line knows which it is writing. It now says so.

28 prose lines reclassified across the six pages; every remaining `Dim`
emission is a row, a blank spacer, a count-strip cell, or Posture's Unscanned
headline. `is_prose` is deleted.

**One home, per line, not per page.** The six pages genuinely differ in
*colour* — Cost renders `Good` as neutral INK rather than green, Posture
overrides its headline, Network bolds conditionally — so a single
`render_lines(cx, lines)` would have had to take a palette anyway. Instead
`emit_line(cx, line, role, color, bold)` owns the wrap-or-truncate decision and
the caller keeps colour. Six pages, one decision.

**And the decision is extracted again, one level down**, as the pure
`wraps(role)`. `emit_line` paints, so no test can watch it; a `wraps` inverted
inside it would truncate every caveat and wrap every row at once with nothing
to notice. As a pure fn it is one assertion.

---

## 3. Tests and the mutation floor

Three new tests (650 total, up from 648). `make lint` was run **before** the
count was quoted.

`every_advisor_page_declares_its_caveats_and_wraps_none_of_its_rows` runs all
six pages against **real reports** from a fixture world, and asserts per page:

- the closing caveat is `Caveat` — the feature, and the line most likely to be
  cut mid-sentence;
- no line that starts with a space is `Caveat` — the regression guard.

| | mutation | |
|---|---|---|
| W1 ×6 | demote **one page's** closing caveat to `Dim` | all six CAUGHT, each naming its own page |
| W2 | mark an indented row as prose (the v1.37.0 regression, re-introduced) | CAUGHT |
| W3 | a page re-decides locally, calling `fit_width` itself | CAUGHT by the lint |
| W4 | the wrap decision inverted | CAUGHT by `only_a_caveat_wraps` |
| W5 | the lint's parse matches nothing (guard-the-guard) | CAUGHT |

**W1 for right-sizing SURVIVED on its first run** — the hazard the prompt named
("eight pages, so the test has to be per-page or the mutation survives"), except
the cause was the fixture, not the test's shape. `rightsizing_lines` early-returns
when `metrics_available` is false, and the probe world had no metrics, so the
page's main-path footer was never rendered and demoting it changed nothing
observable. The probe now seeds pod metrics **and** the assertions run over both
a with-metrics and a no-metrics world; both right-sizing footers are then caught
separately. A per-page test only covers the branch its fixture reaches.

**The lint** (`hack/check-advisor-render.sh`, in `make lint` + CI) asserts
`almanac::wrap` and `panels::fit_width` appear only inside `emit_line` —
computing that range from the file rather than hardcoding it. A lint because the
page functions are GL-driven with no test module, so no behavioural test can see
a second copy of the decision appear in one (D2 §3.4). It carries a
guard-the-guard: if it ever matches fewer than both calls it fails rather than
passing on an empty set — the `check-release-targets.sh` lesson.

---

## 4. The gate

**Discrimination, on the Hardening page** — chosen because it carries both a
long footer and a whole section of dimmed rows. Same cluster, same window, one
line changed (`wraps` forced to `false`, which is v1.38.0's behaviour):

| | footer | INFO row |
|---|---|---|
| before | `…seccomp & default-SA deferr…` — **cut mid-word**, losing "(often set at the namespace default)" | one line, truncated |
| after | wraps to two lines, ending `deferred (often set at the namespace default).` | **byte-identical** |

So the caveat gained its ending and the row was untouched, which is the whole
claim. Verified live on kind against real findings (2 critical, 6 warning,
1 info).

**Failure criteria, stated in advance:** a row wrapping onto a second line where
it would read as another row; a caveat still ending in an ellipsis; a page whose
colour changed. None occurred.

---

## 5. What this does not do

- **Health and Storage are unchanged** — they have no caveat lines (§1).
- **`almanac::wrap` is unchanged**; only who is wrapped changed.
- No page's colour mapping moved: `Caveat` renders exactly where `Dim` did.
- The blank spacer lines stay `Dim`; wrapping an empty string is harmless, but
  they are not prose and are not labelled as such.

**Counts:** 650 workspace tests, `make lint` green before quoting them,
gui-smoke 59, clippy clean with and without features, 0 broken doc links.
