# Kubernation — Feature Roadmap

A living triage doc for feature opportunities. Generated 2026-06-19 from a
five-lens research scan (SRE · DevSecOps · cluster admin · app developer ·
competitive landscape) grounded in the AIM reference library and 2025–26
practitioner/competitor research; 41 raw candidates deduped to ~24 distinct
features. Update the **Status** column as items are built; move shipped items to
the CHANGELOG / decision log.

Posture reminders that shape every item: **read-by-default**, the whole write
surface is one auditable file (`k8s/actions.rs`), every write confirmed +
RBAC-checked; **no exec/PTY**; Secret contents never surfaced; **operator-laptop
tool** (no in-cluster agent, no background daemon, no cross-run persistence yet);
metrics from metrics-server (no Prometheus assumed); the 4X map metaphor is the
identity; interesting logic lives in pure, unit-tested core.

---

## Top 10 — recommended build order

| # | Feature | Why | Effort | Status |
|---|---|---|---|---|
| 1 | **Pod-not-Ready explainer** (probe + image-pull cards) | Closes the attention-queue "last mile": red → root cause + next step. CrashLoop ≈ 40% of K8s tickets. | M | in progress |
| 2 | **Runbook / next-action hints on concerns** | Cheapest high-signal win — every concern points to its in-app verb. | S | todo |
| 3 | **Rollout history + revision diff** | "Which change broke it?" Pure over the watched ReplicaSet store; unblocks #4. | M | todo |
| 4 | **Rollback (staged turn — 5th planning verb)** | The #1 incident remediation, on the existing dry-run/commit rail. | M | todo |
| 5 | **Right-sizing advisor** (requests vs usage) | Biggest cost + reliability lever; metrics rings already kept; no Prometheus. | M | todo |
| 6 | **Self-scoped RBAC matrix (SSAR)** | Reuses the existing SSAR; kills surprise-403s; zero extra perms. DevSecOps beachhead. | M | todo |
| 7 | **Security / hardening scan** (OWASP K01 + PodSecurity + sanitizer) | Live deployed-state lint into the queue + an advisor tab. | M | todo |
| 8 | **Dependency / impact triage panel** | Promotes the blast-radius flash into a navigable incident list. | M | todo |
| 9 | **Change timeline** ("what changed 5 min ago?") | The central triage question; underpins postmortems + correlation. | L | todo |
| 10 | **NetworkPolicy coverage map** ("unwalled cities") | OWASP K07; the 4X walls metaphor is the most on-brand security feature. | M | todo |

Ordering rationale: front-load pure, posture-safe, high-frequency incident value
(1–5), then the first DevSecOps beachhead reusing existing SSAR + watched data
(6–7), then incident-tooling depth (8–9), capped by an identity-aligned spatial
security feature (10). Rollback (4) follows history (3) because it depends on it.

---

## Tiers (the full set)

### Quick wins — pure / read-only, small effort, high value
- Runbook / next-action hints on concerns (S)
- Node-condition health board (S)
- Image-pull diagnostics (S) — standalone, or seed of the explainer
- Self-scoped RBAC access matrix via `SelfSubjectAccessReview` (M, low)
- Pod-not-Ready explainer (M)

### Next — medium effort, clearly worth it, fits the posture
- Rollout history + revision diff (M) → prerequisite for rollback
- Rollback as a staged turn (M)
- Right-sizing advisor (M)
- Security / hardening scan — K01 + PodSecurity + Popeye-style sanitizer (M)
- Dependency / impact triage panel (M)
- NetworkPolicy coverage map (M)
- Multi-burn-rate SLO alerting (M) — two-window burn over the treasury rings
- Alert-fatigue dedup + actionability tags (M)
- Saturation overlay — the 4th golden signal (M)
- Secret-hygiene audit, no values (M)
- Live-spec drift — declared vs running (M)
- Postmortem / after-action export — one local file (M)
- Image provenance — latest-tag / digest / registry (M)
- Posture score — "realm defense" meter, after the scans land (M)

### Bigger bets — large but high-value
- Change timeline (L) — folds events + RS revisions into per-subject "what changed"
- Reachability tracer — inverse blast, "can my pod reach its DB?" (L)
- Cost cartography — OpenCost-aware + pure pricing fallback (L)
- GitOps drift — Argo/Flux read, extends the desired-vs-observed identity (L)
- Full RBAC matrix + escalation paths — needs broad cluster-read RBAC (L)
- Local-LLM "Explain" — opt-in, BYO-model, suggest-only (L)

---

## Decisions needed (posture-stretch)

Each conflicts with a current constraint — build only on an explicit yes:

- **No-exec ephemeral debug** — ephemeral container + a *curated, non-interactive*
  command menu, output read back through the log tail. The cleanest principled
  path to the "probe DNS/config from inside the pod" need, but it adds a new write
  verb + an in-container action (a posture expansion). Gate behind opt-in + a
  fixed command set if taken.
- **Hubble / eBPF flow overlay** — the only honest way to draw *real* call-graph
  edges (vs. topology-only blast). Needs Cilium/Hubble + a gRPC dep; ship as an
  opt-in enhancement, topology-only stays default.
- **Black-box probe over port-forward** — fine *only* if strictly per-click; any
  background polling becomes a daemon and violates the no-daemon posture.
- **Latency / RED SLIs** — need a discoverable metric source; ship dark-when-absent.
- **CIS posture checks** — must bucket host-filesystem controls as "needs
  kube-bench on the node, out of scope" or it overclaims.
- **Helm awareness** — reads Helm release Secret *metadata* only; confirm the
  value-redaction boundary holds (the browser precedent says yes).
- **Config / env inspector** — widens on-demand ConfigMap fetching; keep Secret
  values redacted.

### Probably not (out of scope / against identity)
- Native **audit-log review** — needs an external sink a laptop tool doesn't own.
- Any **background scheduler/daemon** that outlives the process, or **cross-run
  persistence** beyond explicit one-shot file exports.

---

## Cross-cutting enablers (build once, unlock many)

- **`state/timeline.rs` change-feed** (event ring + RS revisions) → Change Timeline,
  Postmortem export, alert correlation, the existing chaos chronicle.
- **ReplicaSet-revision resolver** (newest RS = current, prior = last-known-good)
  → Rollout history, Rollback, Live-spec drift, Change timeline.
- **`securityContext` / pod-template extractor** (one pure pass) → Hardening scan,
  PodSecurity, secret-hygiene, image provenance, posture score.
- **Metrics-ring analytics helper** (rolling P95 / smoothed headroom / slope) →
  Right-sizing, Saturation overlay, cost fallback.
- **"Degrade-dark-when-absent" capability detection** (already used for
  metrics-server / `--project` / `browse.rs`) → GitOps CRDs, trivy reports,
  OpenCost, latency SLIs.
- **Opt-in BYO-model AI adapter** (redacted-context bundle → Ollama/HTTP,
  suggest-only through the planning gate) — the one new external-dependency
  enabler worth a decision now; the genre is moving here (k8sgpt, Headlamp AI,
  kagent).

**Not recommended yet** (would unlock features but conflict with the stated
posture): a persistence layer (keep to one-shot file exports); a background
daemon/scheduler (keep everything tick- or click-driven).
