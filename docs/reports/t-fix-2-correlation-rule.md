# T-fix-2 — the correlation rule

**Phase:** T-fix-2, from `kubernation-t-fix-2-correlation-rule-guidance.md`
**Version:** v1.11.3 · **Date:** 2026-08-06
**Preceded by:** `t-fix-onset-fault-lines.md` (the anchor), whose §3.2 found defect B

All eleven §1 claims verified. **Gate passes, discrimination check passes.**
443 core + 103 GUI tests; gui-smoke 53; dev cluster restored.

---

## 1. Both defects, and that one was mine

**Defect A — the asymmetric comparison.** T-fix moved `first_trouble` to onset and
left the change side on `e.when`. So `d` measured *"from this change's most recent
sighting to the incident's start"* — not the quantity the rule claims. Verified in
source: `ftt` is the onset `anchors()` produces, `w` was `e.when`.

I introduced it, and it is invisible in review in a specific way: the expression
still typechecks and still reads sensibly. Only the *quantity* changed. That is
why §6's sixth standing question is worth keeping.

Impact was confined to event-sourced changes. `Deploy` takes `when` from
`rev.created` — an RS is created once, so its onset *is* its `when` — and operator
actions are single instants. Only `Scale` and `NodeChange` could differ, and a
repeating `ScalingReplicaSet` correlated from its latest refresh rather than from
when the scaling began.

**Defect B — the lower bound doing two jobs.** `1..=600` excludes `d == 0` for a
stated causality reason. But Kubernetes Event timestamps are second-granularity
and a kubelet fails an image pull immediately, so it is also a *resolution* rule —
and in that role it dropped the canonical incident.

The fix uses the asymmetry §3.1 identifies: **a deploy cannot be caused by a
failure it precedes.** `correlation_floor` admits `d == 0` for acts (`Deploy`,
`Operator`) and keeps the exclusion for observations, which may be the failure's
effect. No new constant — the distinction is one the type already carries.

### What deliberately did *not* change

`fault_line_above` still compares against `e.when`, and that is correct: it is a
**position** in a list sorted by `when`, so the sort key is the right side.
The suspect test is a **duration**, so both its sides must be the same quantity.
Recorded at both sites, because "make them consistent" would have been the wrong
instinct here.

---

## 2. The gate

§5: *a deploy and its immediate failure, sharing a timestamp, are correlated.*

**Making `d == 0` deterministic.** §5.2 notes `d` is not controllable because it
depends on kubelet reaction speed, and asks for a distribution. It can be forced:
an **invalid image name** is rejected by the kubelet with no network round-trip,
so the failure lands in the same second as the RS. Measured: RS created
`00:52:34Z`, failure onset `00:52:34Z` — `d = 0` exactly, on demand. That turns
the gate's central case from a coin flip into a repeatable fixture, and is worth
carrying into T2's testing.

**Contamination.** Per §5.1, the cluster was quiesced until zero Warning onsets
remained inside the window. `stuck-pvc` had to be parked for this — its event
series restarts periodically, so its onset keeps refreshing and it would have
anchored ahead of any induced incident. Restored afterwards.

**Result** — one capture, showing both halves of the policy:

```
2m · web — rev 18->19 · nginx:1.27-alpine -> INVALID_IMAGE_NAME  (before the failure)
2m · web — ScalingReplicaSet: Scaled up replica set web-788dbd9b45
```

The act is flagged; the observation in the same second is not.

### The discrimination check

Same incident, same data, one variable:

| | suspects |
|---|---|
| fixed build | **1** |
| `d == 0` admission reverted | **0** |

They **disagree**, which is §5.2's pass condition — the opposite signature from
T-fix, because here the fix is meant to change the outcome rather than stop
something else from changing it.

### The `d` distribution

Four induced incidents across T-fix and this phase:

| RS created | failure onset | d | flagged before | flagged now |
|---|---|---|---|---|
| 22:42:13Z | 22:42:24Z | 11 s | yes | yes |
| 23:04:45Z | 23:04:46Z | 1 s | yes | yes |
| 22:58:44Z | 22:58:44Z | **0 s** | no | yes |
| 00:52:34Z | 00:52:34Z | **0 s** | no | yes |

Half the sample sat at the boundary the old rule excluded. On this evidence
`d == 0` is not an edge case for the deploy-then-fail shape — it is roughly a
coin flip, decided by whether the kubelet needs a network round-trip.

### Mutation floor, exercised

| Mutation | Caught by |
|---|---|
| restore the `1..` floor for acts | `a_same_second_act_correlates_but_a_same_second_observation_does_not` |
| restore `e.when` on the change side | `a_repeating_change_correlates_from_its_onset` |

Each caught by exactly the intended test, and by no other — so neither test is
passing for the other's reason.

---

## 3. An existing test changed, and why

`annals_lines_flags_suspect_change_before_failure` asserted *"a change at the
failure instant isn't a precursor"* using a **Deploy** at `d == 0` — precisely the
case §3.1 changes. Replaced with something strictly stronger rather than merely
green: it now pins both directions, adding a same-instant `Scale` for contrast, so
it asserts the *distinction* rather than the old blanket rule.

---

## 4. §6 — standing questions

**1. Where does a summing step precede a comparing step?**
Not this time — the defects were in a comparison, not a reduction. T-fix's was the
reduction (`first_trouble`); this phase is the other side of the same expression.

**2. Does every reducer over a possibly-empty input express unknown, or fabricate?**
`e.onset` is `Option` and the suspect test yields `false` when it is absent, as
`e.when` did. `correlation_floor` is total over `ChangeKind`, with the catch-all
deliberately the *conservative* branch — a kind added later gets the stricter
floor until someone argues otherwise, which is the safe default direction.

**3. Where do two sections constrain the same behaviour?**
§2 (use onset) and §3 (the floor) both constrain the same three lines and pull
opposite ways at `d == 0`: §2 makes the change side earlier, widening `d`, while
§3 admits the zero case. They compose without conflict because they act on
different terms, and the tests fix each independently — verified by the mutation
floor catching them separately.

**4. What existing consumers depend on the old meaning?**
`suspect` is produced once, in `row_decisions`, consumed by `annals_lines` (the
modal, city and province) and the postmortem export. Both move together by
construction, and there is now a GUI-side test asserting the rendered rows carry
the decisions they were given — near-tautological today, which is the point: it
fails the moment a refactor recomputes either side locally.

**5. Which claims were inherited, and does each state occur?**
All eleven verified this round. The states occur and were produced: a repeating
event-sourced change with onset ≠ `when`, an act at `d == 0` (four times), and an
observation at `d == 0` in the same capture as the act.

**6. When a change moves one side of a comparison, does the other side still mean
the same thing?**
This is the question the phase exists for, and the answer here was no. Applied
across the whole expression it also produced a *negative* result worth keeping:
`fault_line_above` legitimately keeps `when`, because it compares against a sort
key rather than measuring a duration. So the rule is not "make both sides match"
— it is "know which quantity each side is, and say so."

---

## 5. What this does and does not settle for T2

T2's premise — *put the Annals' existing conclusion on the map* — is now sound:
the anchor marks when the incident began, and the correlation measures the right
quantity from it in the right units.

Still open, and not this phase's to fix: **the correlation is adjacency, never
causation.** The rule's own wording is `"preceded by", never "caused by"`, and
nothing here strengthens that. What changed is that it no longer misses the
adjacency it was built to catch, or invent one from a mismatched comparison.
