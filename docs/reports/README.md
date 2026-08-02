# Reports

Session reports written for planning: what was built, what the governing guidance
got wrong, and what needs a decision. One per round, newest first.

They are kept here — in the repo, in markdown — rather than as hosted links, so they
are diffable, greppable, and survive alongside the code they describe.

| Report | Date | Outcome |
|---|---|---|
| [A1 — the layout engine](a1-layout-engine.md) | 2026-08-02 | Shipped, unversioned (consumer-less) |
| [Unmeasurable capacity must not read as idle](unmeasurable-capacity.md) | 2026-08-02 | Shipped as v1.6.0 |
| [A-pre — the churn harness](a-pre-churn-harness.md) | 2026-08-02 | Shipped, unversioned (test asset) |
| [A0 — pod resource data in the map model](a0-pod-resource-data.md) | 2026-07-31 | Shipped, unversioned (gated prerequisite) |
| [Substrate overlay — DaemonSet coverage gaps](substrate-overlay.md) | 2026-07-30 | Shipped as v1.5.0 |
| [The cutaway fork](cutaway-fork.md) | 2026-07-30 | Stopped at gate 2; the finding transfers |

## Conventions

- **Verification first.** Every guidance doc so far has contained one or two wrong
  mechanism claims. Each report records which claims were checked and what was false —
  that section is the one that feeds back into how the next phase gets specified.
- **Findings carry their evidence.** A claim about Kubernetes semantics is verified
  against a live API server; a claim about test coverage is verified by a mutation.
- **Decisions for the room** at the end: the things that genuinely need a human call,
  not a summary of what was already decided.

Detailed engineering rationale lives in the decision log in `CLAUDE.md`. These are the
planning-facing summaries; the gate documents (`../cutaway-gate-*.md`) are the raw
round-by-round notes behind one of them.
