# ML model integration

Deliverables of [plan 09](../plans/09-ml-model-integration.md): where learned models
help, how they run, how they are trained, evaluated, governed, presented, and secured.

**The principle, and it is not a slogan:** learned models produce evidence or a score.
They augment the verified estimators, never replace them, and never act. Every use case
names what happens when the model is absent, and in every case the answer is that the
system carries on deterministically and says the model is not running.

Status: first draft 2026-09-04. Nothing here is built: `gungnir-ml` does not exist, no
model has been trained, and every accuracy figure is a target set before the work rather
than a measurement.

| File | Content |
|---|---|
| [`use-cases.md`](use-cases.md) | Five use cases; ML-01 classification and ML-04 anomaly detection specified in depth, each with consumer, targets, latency budget, failure behaviour, and how a human sees it |
| [`architecture.md`](architecture.md) | The `gungnir-ml` crate: the `Model` and `FeatureExtractor` traits, ONNX Runtime through `ort`, execution providers per profile, manifest loading, where inference sits in the tick |
| [`data-pipeline.md`](data-pipeline.md) | Sources, the Arrow dataset schema, splitting by scenario and entity, provenance, and the handling rules for data derived from real sessions |
| [`training.md`](training.md) | The separate training repository, the feature-extractor parity test, model choices, ONNX export with verification, and the model card |
| [`evaluation-and-verification.md`](evaluation-and-verification.md) | Three gates (offline, parity, shadow), metrics and thresholds, drift monitoring, and the eight rows to add to the verification table |
| [`ui-integration.md`](ui-integration.md) | Model output inside the panels that already own the information, labelled, with confidence, with a "why", and with absence made visible |
| [`mlops.md`](mlops.md) | Versioning, the promotion state machine, rollback, monitoring, and delivery |
| [`security.md`](security.md) | Model supply chain, adversarial input, data handling, inference-time exposure, and the reviewer's checklist |

## Decisions this revision was built on

| Decision | Answer | Date |
|---|---|---|
| First two use cases | ML-01 classification and ML-04 anomaly detection, both of which have a consumer and a waiting gap | 2026-09-04 |
| Training code location | A separate repository producing signed artefacts this workspace consumes by hash | 2026-09-04 |
| Model delivery | Default models ship signed with the release; customer-specific models are delivered separately | 2026-09-04 |

## What this plan found

Two things worth surfacing rather than leaving in the detail:

1. **`gungnir-modelops` cannot govern models yet.** Its promotion state machine
   (`Candidate`, `Validated`, `Promoted`, `RolledBack`) is exactly right, but its
   baseline holds a tracking configuration, not a model manifest. Extending it is
   GAP-078, and until then no model can be promoted through the governance the rest of
   the system uses.
2. **The feature extractor exists twice**, in Rust for inference and in Python for
   training, and silent drift between them would be undetectable from the outputs. The
   answer is a parity test in both repositories' continuous integration
   (`training.md` §3), not care.

## Engineering items filed

| Gap | Item | Increment |
|---|---|---|
| GAP-077 | The `gungnir-ml` crate, the `ort` sign-off, and the dependency edge | I4 |
| GAP-078 | Model manifests as `gungnir-modelops` baselines | I4 |
| GAP-079 | The dataset pipeline from test tracks and journals | I3 |
| GAP-080 | The first two models trained, evaluated, and promoted | I4 |

## Open items

- The accuracy thresholds are proposals; the owner confirms them as the measures were.
- Real session journals do not exist yet, so the first models' domain of validity is the
  test-track generator's parameter space, and every model card says so.
- The security reviewer has not seen `security.md`.
