# Training and export

Status: first draft, 2026-09-04. The training code lives in a **separate repository**
(owner decision, 2026-09-04), which produces signed ONNX artefacts and dataset manifests
that this workspace consumes by hash.

## 1. Why a separate repository

- This workspace has a strict lint policy, a dependency policy, a pinned toolchain, and
  CI jobs that would all have to accommodate Python, notebooks, and large files.
- Training data derived from real sessions may be operational data with handling rules
  the software repository does not have (`data-pipeline.md` §6).
- The interface between the two is narrow and checkable: a model file, a hash, a
  signature, and a model card. That is a better boundary than a shared directory.

The cost is that provenance now spans two repositories, so the model manifest must
record the training repository's commit for every artefact.

## 2. Training repository layout

```
gungnir-ml-training/
  datasets/          builders that read test-track sets and journals, write Arrow
  features/          the feature extractors, mirrored from gungnir-ml
  models/            one directory per model: config, training script, evaluation
  export/            ONNX export with a fixed input signature, plus verification
  cards/             the model card template and the generated cards
  experiments/       tracking output; not the source of truth
  requirements.txt   pinned
```

## 3. The feature-extractor mirror problem

The features must be identical in Python at training time and in Rust at inference time.
Two implementations of the same arithmetic drift, and the drift is silent: the model is
fine, the deployment is wrong, and nothing errors.

The mitigation is a **parity test**, not discipline:

1. The Rust extractor writes a golden file: for a fixed test-track set, the exact feature
   batch it produces.
2. The Python extractor reads the same set and must reproduce that batch within 1e-6.
3. The test runs in both repositories' CI. A change to either extractor that breaks
   parity fails the build.

The `feature_schema_version` is bumped together in both, and a model trained under one
version refuses to load against another (`architecture.md` §2).

## 4. Model choices for the first two

Deliberately small and boring, because the value is in the pipeline, the evaluation, and
the governance, not in the architecture of the model:

| Model | Approach | Why |
|---|---|---|
| ML-01 classifier | Gradient-boosted trees on the window features, or a small multilayer perceptron if calibration is better | Tabular features, modest data, needs calibrated probabilities and explainable feature importance. A sequence model is not justified until real journals exist |
| ML-04 anomaly detector | A per-source statistical baseline with a learned residual model; isolation-forest style scoring for the track features | Anomaly detection with almost no labelled anomalies is a one-class problem; the planted anomalies in the test tracks are for evaluation, not for training a supervised detector |

Both must produce a calibrated score, because the consumer uses the number as a weight.
Calibration is checked in evaluation and is a promotion gate.

## 5. Export

- Export to ONNX with **fixed input shapes and dtypes** and a named input and output, so
  `input_signature` in the manifest is exact.
- Opset pinned; the runtime version that will execute it recorded in the model card.
- **Verify after export**: run the holdout through both the Python model and the exported
  ONNX file and require agreement within 1e-5. An export that changes behaviour is a
  common and quiet failure.
- Quantization is not used for the first models. If it is later, the quantized artefact
  is a separate model version with its own evaluation, not a silent substitution.

## 6. Model card

One per model version, generated from the training run, and shipped beside the artefact:

- Name, version, task, and the consumer that uses it.
- Training dataset hash, its sources, its splits, and the class counts.
- **Domain of validity**, stated plainly: which classes, which sensor configurations,
  which scenarios, and that the data is synthetic.
- Metrics on validation and on the held-out test set, including the per-class recall that
  the target names.
- Calibration.
- Known failure modes and what the consumer does when they occur.
- Training repository commit, exporter version, opset, and runtime version.
- The person who approved promotion, once promoted.

## 7. Reproducibility

Seeded training, pinned dependencies, and the dataset hash recorded. A model version can
be rebuilt from its card. This is not optional for a system whose outputs inform an
engagement recommendation: "we cannot reproduce that model" and "we cannot explain that
declaration" are the same sentence.

## Traceability

`data-pipeline.md` for the inputs; `evaluation-and-verification.md` for the gates;
`mlops.md` for promotion; `security.md` for signing; gap GAP-080.
