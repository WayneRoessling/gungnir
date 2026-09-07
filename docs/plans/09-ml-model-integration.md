# Plan 09: ML model integration into the UI and services

## Purpose

Add learned models where they improve the operational picture without displacing the
verified estimators: classification from kinematics and evidence, learned threat
scoring, track-quality and staleness prediction, anomaly detection on sensor feeds,
and gating assistance. Models run in every deployment profile through ONNX Runtime
in Rust, are governed by `gungnir-modelops` promotion and rollback, and never act
without the same human decision boundary as everything else.

## Scope

In scope: the use cases, a `gungnir-ml` crate design, the data pipeline from
journals and test tracks to training sets, the training and export workflow, the
evaluation and verification rows, UI presentation with confidence and provenance,
MLOps (versioning, promotion, monitoring, rollback), and security.

Out of scope: replacing the Kalman, association, or allocation mathematics with
learned components; those stay classical and oracle-verified.

## Inputs

- `docs/test-tracks/` (plan 07) as labelled training and evaluation data.
- `gungnir-model` (features come from `TrackView`, `DetectionView`, events),
  `gungnir-identification` and `gungnir-assessment` (the consumers),
  `gungnir-modelops` (promotion and rollback), `gungnir-observability` (monitoring),
  `gungnir-store` journals (operational data with provenance).
- `docs/verification-capability-table.md` §2 for how rows are added.

## Deliverables and target location

All under `docs/ml/`:

| File | Content |
|---|---|
| `README.md` | Index and the principle: augment, never replace, never act |
| `use-cases.md` | Each use case with the decision it informs, inputs, outputs, consumer crate, accuracy target, latency budget, failure behaviour, and how a human sees it |
| `architecture.md` | `gungnir-ml` crate: `Model` trait, ONNX Runtime binding, feature extraction from model views, batching, execution providers per profile (GPU on the desktop, CPU on the node), model registry integration, error handling when a model is missing or fails |
| `data-pipeline.md` | Sources (journals, test tracks, scenario generator), labelling, dataset schema (Arrow via `gungnir-interop`), versioning, splits, provenance, retention |
| `training.md` | The Python training repository layout, experiment tracking, export to ONNX with fixed input signatures, validation against holdout, artefact signing |
| `evaluation-and-verification.md` | Metrics per use case, thresholds, shadow-mode evaluation on live data before promotion, drift monitoring, the verification-table rows to add |
| `ui-integration.md` | How model outputs appear: classification with confidence and evidence list, risk score with contributing factors, staleness prediction as a visual state, anomaly alerts through the alert lifecycle; never an auto-applied action |
| `mlops.md` | Versioning, `gungnir-modelops` promotion gates, rollback, monitoring through `gungnir-observability`, model cards per model |
| `security.md` | Model supply chain (signed artefacts, hashes in config), adversarial-input considerations, data handling per profile |

## Use cases (initial)

| Id | Use case | Consumer | Output | Human sees |
|---|---|---|---|---|
| ML-01 | Classification from kinematics and evidence | `gungnir-identification` | Class probabilities as `IdentificationEvidence` with source "ml:model-name@version" | Suggested class with confidence; fused with other evidence by the existing engine |
| ML-02 | Learned threat scoring | `gungnir-assessment` | Score and factors, as an alternative `ThreatAssessor` | Score with contributing factors beside the closing-speed baseline |
| ML-03 | Track-quality and staleness prediction | `gungnir-tracking-service` projection | Probability the track is stale or about to be lost | Visual state before the deterministic stale flag fires |
| ML-04 | Sensor-feed anomaly detection | `gungnir-ingest`, `gungnir-observability` | Anomaly score per source | An alert through the lifecycle, never a quarantine on its own |
| ML-05 | Gating assistance | `gungnir-association` (advisory input only) | Suggested gate scale | Not shown; logged; the associator's verified gate decides |

Each use case has a failure behaviour: when the model is missing, fails, or is out of
its validated domain, the consumer proceeds without it and reports the absence in
health, never with a fabricated value.

## Architecture (summary)

- **Crate `gungnir-ml`** (productization layer, depends on `gungnir-model` and the
  ONNX Runtime binding): `Model` trait (`name`, `version`, `input_signature`,
  `infer`), `OnnxModel` implementation, `FeatureExtractor`s for track and detection
  views, a `ModelSet` loaded from a signed manifest named in `gungnir-config`.
- **Runtime:** ONNX Runtime through the `ort` crate. Execution providers: CUDA or
  DirectML on the desktop where available, CPU otherwise and on the node. Adding
  `ort` is a stack change recorded in `docs/agentic-coding-standards.md` §2.9 when it
  lands; alternatives (`candle`, `burn`, `tract`) are recorded with the reasons they
  were not chosen.
- **Governance:** models are `gungnir-modelops` baselines: candidate, validated
  (evaluation thresholds met), promoted (shadow-mode passed), rolled back.
- **Consumers** receive model outputs as evidence or scores through existing traits;
  no consumer depends on `gungnir-ml` being present.

## Method

1. **Select** the first two use cases (proposed: ML-01 and ML-03) and their
   thresholds with the owner.
2. **Data.** Build the dataset pipeline from test tracks and journals; define the
   schema; version the first datasets.
3. **Crate.** Design and scaffold `gungnir-ml` with the trait, a stub model, and
   the manifest loading; wire the first consumer behind a config flag, off by
   default.
4. **Train** the first models in the Python repository; export; validate; produce
   model cards.
5. **Evaluate** offline, then in shadow mode on replayed sessions; add the
   verification-table rows.
6. **Promote** through `gungnir-modelops`; enable in config; monitor.
7. **Repeat** for the remaining use cases.

## Roles

- Owner: use-case selection, thresholds, promotion decisions.
- ML agent: pipeline, training, evaluation, model cards.
- Engineering agent: `gungnir-ml`, consumer wiring, UI presentation.
- Security reviewer (human-owned): supply chain and adversarial-input review.

## Dependencies

Plan 07 for data; plan 06 for how outputs appear; the tracking pipeline being
implemented for live features (`ARCHITECTURE.md` §10); the stack sign-off for
`ort`.

## Effort and sequencing

12 to 20 agent-assisted days for the documents, crate scaffold, and first two
models; 3 weeks elapsed after plan 07.

## Acceptance criteria

- Every use case has a consumer, an accuracy target, a latency budget, a failure
  behaviour, and a UI presentation.
- `gungnir-ml` runs the same model file in all three profiles.
- A model reaches promotion only through evaluation thresholds and shadow mode,
  and can be rolled back from the config baseline.
- No model output ever changes a plan, a decision, or a quarantine without the
  existing human or verified-logic path.

## Risks

- Training data that is synthetic only; mitigate by mixing journals from real
  sessions as they become available and by stating the domain of validity.
- GPU dependency on the desktop; mitigate with the CPU execution provider and
  latency budgets set for CPU.
- Silent degradation; mitigate with health reporting when a model is absent and
  drift monitoring when present.

## Open questions

- Whether the Python training repository lives inside this workspace or beside it.
- The first two use cases and their thresholds.
- Whether model artefacts ship with the product or are distributed separately.
