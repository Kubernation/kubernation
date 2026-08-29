# The panel-wording pass

**Follows:** `docs/reports/prose-audit.md` §2 (kind 2, the one the audit recorded
as unmeasured) and the amended handoff §4.2
**Version:** 1.34.0 · **Date:** 2026-08-29

The audit checked doc comments and the field guide by reading. This is §2's
second kind — **panel and concern wording**, the surface an operator reads during
an incident — and it can only be checked by rendering, which is why the first
pass left it open.

Two defects, both found on screen against the live cluster, neither visible in
source review. One **understates a problem in the attention queue**, the product's
spine.

---

## 1. `×N` had two units, adjacent in one list

The queue rendered:

```
! ds kube-system/kindnet — restarting repeatedly ×1     — 4/4 ready
· events: ProvisioningFailed ×522 on persistentvolumeclaim …
```

Every counter in `Agg` counts **pods** — the queue aggregates a workload's
failing pods into one concern ("city in trouble, not 40 pod alarms"). So `×1`
means *one flapping pod*. Two lines below, `×522` means *522 occurrences*. Same
suffix, different unit, same screen.

**And the pod reading understates it.** `RESTART_THRESHOLD` is 5, so a pod is
only counted once it has restarted five times. On the dev cluster, verified
against the API:

```
kindnet-gl5qv   5     <- the one counted
kindnet-glgpt   4
kindnet-gnkzh   4
kindnet-zvj5v   4
```

`restarting repeatedly ×1` reads as one restart. It is one pod that restarted
five times. The number was right and the sentence was not.

Fixed by giving the count its unit — `×1 pod` / `×3 pods` — via a `tally()`
helper carrying the reasoning.

### 1.1 …which made a second surface worse, so the decision moved to the caller

The first fix rendered `pod kube-system/kube-scheduler-… — restarting repeatedly
×1 pod` on the **bare-pod** path, whose title already names one pod. The tally
restated its own subject.

Same `Agg::primary`, two callers, and only the caller knows whether the count
carries information. So `primary()` now returns `(Severity, label, n)` and each
site renders: a **workload** concern tallies (it aggregates, the number is the
point), a **bare pod** says the label alone. Both directions asserted, because a
single rendering is wrong at one of the two sites.

**A third caller existed and my grep missed it.** `and_then(Agg::primary)` — a
path reference, not a call — so `grep 'primary()'` did not see the workload
headline, the most-read concern in the product. The compiler caught it. Same
lesson as D2-fix: enumerate consumers, don't grep for one spelling of them.

---

## 2. "idle" meant two different things and usually said neither

The cost SELECTION line read `idle 100% · 19.9 units`.

Under `CostBasis::Requests` idle is capacity **nobody reserved**; under `Usage` it
is capacity **nobody is using**. On a node fully reserved and lightly used —
precisely what the cost feature exists to find — those differ by nearly the whole
range.

The panel printed `(idle est. from requests)` for the requests basis and
**nothing** for usage, on the `pool_line` rule that a measured value needs no
caveat. That rule fits a *fallback cascade*, where alternatives answer one
question with decreasing confidence. These answer **two questions**, so the
common case was the ambiguous one.

**The advisor had always distinguished them** — its footer says "idle is
paid-for-but-unused" versus "(requests)". One mechanism, two surfaces, and only
one of them said which.

Fixed with `cost::idle_meaning(basis)` as the single home, consumed by the panel
line and interpolated into the advisor footer, so they cannot describe it
differently. The line now reads `idle 100% unused · 19.9 units`, always naming
the basis, and fits the ~40-char column.

**Honest about the evidence.** On this cluster both bases give ≈97–100% — it is
idle, and under-reserved as well as under-used, so the swing is *not* visible
here. The defect is a design fact (two claims, one word) evidenced by the
advisor's own text, not by a number I can point at on kind.

---

## 2a. The Oracle said "streaming" before anything had streamed

The audit recorded the Oracle's wording as **unchecked** because it needs a live
endpoint. With a local Ollama up, a real realm consult rendered:

```
streaming… 0s · 0 chars
(Cancel to stop)
```

Zero chars: no token had arrived. `stream_status_line`'s own doc says *"used once
tokens start arriving"* — the contract was written down, and the caller branched
on `self.reply.is_some()` instead. The net thread pre-inserts an **empty**
`StreamBuf` when it spawns the request, so `reply` becomes `Some("")` at once and
the view flipped to the streaming row immediately.

Wrong three ways, all inside the window where the operator most needs help — on a
30B local model that window is 10–30s:

- it said *streaming* when nothing had streamed;
- it **dropped the timeout clause**, and that clause governs precisely the first
  token (the client gives it the full per-profile timeout, then a 30s
  idle-per-token bound) — so the countdown vanished exactly where it applies;
- it replaced *"local models can take a while"* with the terser hint, exactly
  when the wait is longest.

Now `consulting the Oracle… 0s (timeout 600s)` until a token lands, verified on
screen against the live model.

**The decision moved into a pure `progress_row`**, and then one step further.
The first version left the caller filtering (`reply.filter(|r| !r.is_empty())`)
and passing the result in — mutation **U3 replaced that filter and survived**,
because the authority was pinned and the caller was not, in a GL-driven function
no test can watch. That is D2 §3.4 exactly. So `progress_row` now takes the RAW
`Option<&str>` and owns the has-a-token test: there is no filtered value left for
a caller to get wrong. U3 re-run still survives — and now *should*, because
`progress_row(Some(""))` and `progress_row(None)` are asserted equal, so the
mutation is a no-op rather than a defect.

---

## 3. Checked and found correct

Recorded so a later pass does not re-derive them: `strain: calm · pods 7/110`,
the `0 crit · 8 warn · 1 info` rollup against `ATTENTION (9)`, `no nodepool
label - not grouped`, `grid B0` against the legend's "rows are slot ordinals",
`drain: no budget blocks a drain`, the substrate list, and the drain
panel-vs-queue divergence (unconditional in the panel, cordon-only in the queue —
by design, asserted both ways in the PDB round).

One thing deliberately **not** changed: `ingress ns/name — backend a, b has no
Service` is ungrammatical when several backends are missing. §8 says correct what
is false, not what is terse. It is not false.

---

## 4. Mutation floor

| | mutation | |
|---|---|---|
| S1 | the idle figure stops naming its basis | caught |
| S2 | both bases render the same word (the label becomes decoration) | caught |
| T1 | the workload count loses its unit | caught |
| T2 | the bare pod gets the tally too | caught |
| U1 | an empty stream buffer reads as streaming | caught |
| U2 | the cold-start row loses its timeout clause | caught |
| U3 | the caller re-mirrors the has-a-token test | survived → made unrepresentable |

S2 matters as much as S1: a basis label that says the same thing for both bases
is decoration, and the test would pass on the strength of the word being present.

---

## 5. Method

**Rendering, not reading — and it is the whole reason this was a separate pass.**
Neither defect is visible in source review: `tally` was arithmetically correct
and `cost_lines` followed an established rule. Both are only wrong *as read on a
screen, beside their neighbours*. The `×1` / `×522` collision needed the two
concerns adjacent in one export; the idle ambiguity needed the advisor's footer
and the panel line side by side.

**Live data mattered too.** `×1` looked harmless until the API showed the
counted pod at five restarts and its three siblings at four.

---

## 6. Acceptance

- [x] §2 kind-2 surfaces enumerated (14 line-builders, 10 concern strings) and rendered
- [x] Both defects corrected, each stating what the code does
- [x] One authority where two surfaces described one mechanism (§2)
- [x] Surfaces checked-and-correct recorded, so a later pass need not redo them (§3)
- [x] Mutations asserted applied
- [x] `cargo nextest` green; clippy clean with and without features; 0 broken doc links

**Also checked and correct:** the consent preview's `POST
{endpoint}/chat/completions` — I suspected a missing `/v1` and the code was
right: the client builds `{base_url}/chat/completions`, and `DEFAULT_LLM_URL`
already ends in `/v1`. My test run passed a `--llm-url` without it; my error, not
a defect.

**Not done:** the drain line's blocked state still needs a live blocking budget to
render, so it remains **unchecked**, not correct.
