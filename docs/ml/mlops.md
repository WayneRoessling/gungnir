# MLOps

Status: first draft, 2026-09-04. Versioning, promotion, rollback, and monitoring for
model artefacts.

## 1. The governance gap this plan found

`gungnir-modelops` used to govern **algorithm configuration** only, not model artefacts.
Its `ModelBaseline` held a `TrackingConfig` with a mission profile and a `PromotionState`
of `Candidate`, `Validated`, `Promoted`, or `RolledBack`. That state machine was exactly
what a learned model needed, but the payload was not there.

Two options were on the table, and the recommendation is what got built:

| Option | Effect |
|---|---|
| **Extend `ModelBaseline` with an optional model manifest** (recommended, built 2026-09-08) | One registry, one promotion state machine, one audit trail for "what is in force". The tracking configuration and the model set are promoted together, which matches reality: they are the algorithm in force |
| A parallel registry for models | Two state machines and two answers to "what is running", which is how a deployment ends up with a promoted model against a rolled-back configuration |

**Built 2026-09-08 (GAP-078), alongside GAP-077's runtime rather than before it, per this
gap's own closing action.** `ModelBaseline` gained `model: Option<ModelArtifactManifest>`
-- name, semantic version, SHA-256, and an `EvaluationRecord` of gates passed with
evidence -- validated structurally by `InMemoryModelRegistry::validate` (every stated
field non-empty, the hash exactly 64 lowercase hex characters, at least one gate
recorded) and promoted, rolled back, and audited through the very `PromotionState`
machine §3 below describes, never a second one. **This closes the governance gap, not
the deployment one**: nothing in `gungnir-app` or `gungnir-node` ever constructs a
`ModelBaseline` with `model: Some(_)` today, because `gungnir-config`'s schema has no
model-manifest field for a baseline file to name one from, and no model exists yet to
name (GAP-080, open by design). A model can now be promoted through this governance the
moment one exists to promote; enabling one in a deployment is still the separate,
audited, off-by-default configuration change §3 always required.

## 2. Versioning

- **Model version**: semantic, `name@major.minor.patch`. Major changes when the input
  signature or the feature schema changes; minor when retrained on new data; patch for a
  re-export with no behavioural change (which the export verification must confirm).
- **Feature schema version**: separate, shared with the training repository, and checked
  at load.
- **Dataset hash**: recorded in the model card, not in the version.

A deployment's answer to "what is running" is the promoted `ModelBaseline`: the tracking
configuration, the model manifest, and the profile it applies to.

## 3. Promotion

```
Candidate ──offline gates pass──► Validated ──shadow mode passes──► Promoted
    ▲                                                                  │
    └──────────────── RolledBack ◄──── recorded human decision ─────────┘
```

- `Candidate` on registration, with its card and evaluation report.
- `Validated` when the offline and parity gates pass. `gungnir-modelops` already refuses
  to promote anything that is not `Validated`.
- `Promoted` only after shadow mode, and only by a recorded human decision: the analyst
  proposes with the evidence, the supervisor concurs, per the authority matrix
  (`../mission/roles-and-stakeholders.md` §4, model promotion).
- Enabling the promoted model in a deployment is a **separate** configuration change,
  audited, and off by default.

Nothing here is automatic. There is no continuous deployment of models into a system
that informs engagement recommendations.

## 4. Rollback

- Rollback restores the previous promoted baseline, which is the tracking configuration
  and the model manifest together.
- Triggered by a human, from drift alerts, an operator report, or an evaluation finding.
- The rolled-back version stays in the registry with its state; nothing is deleted, so
  the audit trail of what was running when a decision was made stays intact.
- **The deployment must work with the model off**, which is the failure behaviour every
  use case already specifies. Rollback to no model at all is always available and is the
  safe default.

## 5. Monitoring

Through `gungnir-observability`, reported in `SystemHealth` and the alert lifecycle:

| What | Where it shows |
|---|---|
| Which models are loaded, their versions, and their execution provider | The health panel, per session |
| Which expected models failed to load and why | An alert, and health; never silence |
| Drift signals (`evaluation-and-verification.md` §4) | An alert through the lifecycle |
| Inference latency against budget | Health; a breach disables the model for the session |
| Shadow-mode output | Journalled events, read by the analyst, consumed by nothing |

## 6. Delivery

Owner decision, 2026-09-04: **default models ship signed with the release; customer
models are delivered separately.**

- Baseline models are release artefacts, covered by the existing gates in
  `../release-governance.md`: signed, hashed, and listed in the software bill of
  materials alongside the binaries.
- The configuration baseline names the manifest and the hashes, so a deployment can
  prove which model file it is running.
- A model trained on a customer's operational data is delivered to that customer only,
  never in a release, and its card records the data's origin. This is a data-handling
  rule as much as a delivery one (`security.md`).

## 7. Cadence

- Retraining is event-driven, not scheduled: a drift alert, a new data source, a new
  class in the catalogue, or a failed operational expectation.
- Every retrain goes through all three gates again. There is no fast path.
- Model cards are reviewed at the same cadence as the compliance assessment in plan 10.

## Traceability

`gungnir-modelops` for the state machine; `../release-governance.md` for signing and the
bill of materials; `../mission/roles-and-stakeholders.md` §4 for who promotes;
`evaluation-and-verification.md` for the gates; gaps GAP-078 and GAP-080.
