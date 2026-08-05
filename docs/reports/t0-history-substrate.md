# T0 — how much history KuberNation keeps, and where

**Phase:** T0, the measurement §4 of `kubernation-workstream-t-planning.md` calls
"the unasked question" and §5 recommends running first.
**Date:** 2026-08-04 · No code changed.

The plan says this "decides whether T1 and T3 are cheap or foundational". It does,
and it moves the recommended order.

---

## 1. What exists — measured

| Source | Bound | Reach | Survives restart | Survives context switch |
|---|---|---|---|---|
| Event ring (`watch.rs`) | 500, **deduped by (kind, ns, name, reason)** | seeded from etcd at launch, so pre-launch — but only the *latest* occurrence per key | rebuilt from etcd | no |
| RS revisions | cluster's `revisionHistoryLimit`, default 10 | the cluster's own | **yes** — it is etcd's, not ours | n/a |
| Operator actions | 64 | session | no | no (cleared) |
| Metrics rings | 60 × 15 s | ~15 min | no | no |
| SLO rings | 240 × ~2 s | ~8 min | no | no (cleared) |
| **Layout** (`occupied_at`, `vacated_at`) | per slot | unbounded | **yes** | per-context file |
| **Past world states** | — | **none** | — | — |

`snapshot: Mutex<Option<Arc<Snapshot>>>` holds exactly one, the current;
`prior_hot` carried across ticks is a `Layout`, not a `Models`. Nothing else
retains a past frame. Neither metrics nor SLO touch disk.

§4's three shapes map onto reality as:

- **in-session only** — metrics, SLO, operator actions: yes, this is what they are;
- **persisted world states** — do not exist, in any form;
- **persisted summaries** — **already exist, for exactly one fact**: the layout's
  per-slot succession and departure timestamps.

---

## 2. Four findings, in order of how much they move the plan

### 2.1 The event ring is not a history. It is a latest-state set.

On the kind cluster: **26,217 event occurrences collapse to 29 ring entries.**

The dedup key is `(kind, namespace, name, reason)` and it keeps a count — the
Annals shows "×16143" — but it discards **every intermediate timestamp**. You can
ask *"has this object most recently done this, and how many times"*. You cannot
ask *"when did it do it the fifth time"*, because that moment is gone.

Consequences:

- **T1** anchored on events can only answer "since T, has the *latest* occurrence
  of this (object, reason) moved?" — not "how many times since T", and not "when".
- **T3** cannot rebuild past frames from the ring at all. There is nothing to
  rebuild them from.

This is not a defect. The ring exists to answer "what is wrong now", and dedup is
what keeps a crash-looping pod from flooding it. But it is not a time series, and
the plan reads as though it might be.

### 2.2 The Annals reaches 15 minutes for events — which is a trap for T1's gate

`TIMELINE_WINDOW_MIN = 15`. Event-sourced entries are windowed to 15 minutes;
Deploy entries (full rollout history) and this session's operator actions escape it.

So the thing T1 is gated against shows **15 minutes of event history**, plus
rollout history, plus this session's writes. That is a far narrower baseline than
"the Annals is a working comparison" suggests.

**The trap:** a change-since overlay with a one-hour baseline would beat the Annals
on any change older than 15 minutes — trivially, and for a reason that has nothing
to do with whether space beats a list. It would pass §3's gate while telling us
nothing.

**So T1's gate must hold the window constant** — compare a spatial expression and
the Annals over the *same* span — or it measures reach, not spatiality. This is
the standing discrimination-check requirement in a new costume, and it is worth
writing into T1's guidance before anything is built.

### 2.3 The cheapest version of T1 is already shipped

A5's fresh ground answers "what changed, and where" for the occupant fact, over a
configurable window, with a persisted timestamp. §2 of the plan says so and calls
it the seed to generalise.

Measured, the position is sharper than that: **`occupied_at` is the *only*
persisted per-entity change timestamp in the product.** T1 on any other axis —
health, saturation, cost, pod count — has no timestamped baseline to difference
against and needs a new persisted summary, which does not exist.

So §3's framing of T1 as "cheapest — a new `Overlay` variant on an axis with
eight" holds only for the occupant axis, where it is not new work. For every other
axis T1 *is* T0's third shape, and should be costed as substrate, not as a tint.

Worth stating plainly because §3 already identifies "change in what?" as "the
phase's real content" — the measurement says that question is not a design choice
inside a cheap phase, it is the choice of whether the phase is cheap at all.

Incidentally: the churn fleet's layout currently carries **0** `occupied_at` and
**22** `vacated_at`. Correct, not a bug — A5 stamps only a change of hands, and
that fleet's last event was a mass departure with no successions since. But it
shows the persisted change-log is sparse by design and covers two of three
transitions (succession and departure, deliberately not first arrival, or a first
run would paint the whole map).

### 2.4 kwok badly under-represents event volume — the instrument inverts

| Fleet | Nodes | Events in etcd | Span | Occurrences behind them |
|---|---|---|---|---|
| churn (kwok) | 100, repeatedly churned | 67 | 21 h | ~67 |
| kind | 4 | 30 | 5 h | **26,217** |

kwok has no real kubelet, so it emits almost nothing. **Any T-workstream
measurement of event-driven behaviour must use kind, not the churn fleet** — the
exact inverse of Workstream A, where the churn fleet was the only instrument that
could exercise fleet-shaped features at all.

Getting this backwards would produce a confident, plausible, wrong number, which
is the failure mode this project has now hit seven times.

---

## 3. What the substrate would cost, in bytes

The plan treats T3's storage as the expensive unknown. It is not, if the strip is
what §3 says it is — *the last N polls*.

Per-province summary of ~6 numbers ≈ 40 B:

| Nodes | last 6 polls | 1 h @ 15 s | 24 h @ 15 s |
|---|---|---|---|
| 100 | 23 KiB | 938 KiB | 22 MiB |
| 500 | 117 KiB | 4.6 MiB | 110 MiB |
| 5000 | 1.1 MiB | 46 MiB | 1.1 GiB |

**A six-frame strip needs 23 KiB at 100 nodes and needs no persistence at all** —
"the last N polls" is inherently a recent window, so an in-session ring covers it.
It needs persistence only if the strip should span a restart, which §3 does not
ask for.

What *is* expensive is a long baseline: a day of history at fleet scale is
hundreds of MiB, and that is the shape T1 would want if its baseline is "this
morning". The cost is in **reach**, not in frames.

---

## 4. What this does to the recommended order

§5 recommends T0 → T1 → T2 → T3, on the premise that T1 is cheapest.

Measured, that premise holds only on the occupant axis, where T1 is already built.
On every other axis T1 requires the substrate T0 was meant to scope. Meanwhile:

**T2 is the only phase that needs nothing new.** `timeline.rs` already computes
the fault line and the suspect change; RS revisions already survive restart; the
layout already gives every province a stable, nameable position to mark. T2 is a
rendering of a finished computation over data that already exists.

That suggests swapping T1 and T2 — but with a caveat that matters more than the
ordering: §6 names T1's gate as the workstream's kill point, and moving it later
means building T2 before the thesis is tested. Two ways out, and this is a
planning decision rather than a measurement one:

1. **Keep T1 first, on the occupant axis only**, gated against the Annals with the
   window held constant. It is nearly free because A5 shipped it, and it tests the
   thesis honestly — "does a tint that says *this ground changed* beat a line that
   says the same?" That is the real question, and it can be asked this week.
2. **Run T2 first** and accept that the kill point moves later, on the grounds
   that T2's claim (changes *cluster* in one zone or pool — visible on a map,
   invisible in a list) is the strongest in the workstream and the cheapest to
   build.

Option 1 preserves §6's discipline and is the smaller commitment. Option 2 builds
the better feature first and risks building it on an untested thesis.

§3 also flags a prerequisite for T2 that this measurement did not cover: **how
often fault lines actually occur.** That should be measured on kind before T2 is
scoped, and per §2.4 it cannot be measured on the churn fleet.

---

## 5. Open, and honestly out of scope here

- **T-pre, the instrument.** §4 is right that no existing instrument measures
  "does a person learn something faster". The pixel comparator measures rendering;
  `--dump-positions` measures assignment. A6's gate was human for the same reason
  and could not be run solo. If T1's gate is human, saying so in its guidance —
  not discovering it at the end — is the lesson A6 already paid for.
- **Fault-line frequency**, per §3's own risk note.
- Whether a persisted summary ring should be per-province or per-slot. Per-slot
  composes with the layout (which already persists, already keyed that way, and
  already survives a node replacement); per-province does not survive succession,
  which is the thing being measured.
