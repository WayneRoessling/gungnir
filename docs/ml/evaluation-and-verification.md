# Evaluation and verification

Status: first draft, 2026-09-04. What a model must show before it is promoted, how it is
watched afterwards, and the rows to add to the verification table.

## 1. The three gates

A model passes three gates in order. Failing any one stops it.

| Gate | What it tests | Where it runs |
|---|---|---|
| **Offline** | Metrics on the held-out test scenarios that were never used in training or validation | The training repository's CI |
| **Parity** | The exported ONNX artefact reproduces the Python model, and the Rust feature extractor reproduces the Python one | Both repositories' CI |
| **Shadow** | The model runs against replayed real or recorded sessions, producing output that is recorded and compared but consumed by nothing | This workspace, through `gungnir-replay` |

Only after all three does the owner promote it (`mlops.md`).

## 2. Metrics and thresholds

### ML-01 classification

| Metric | Threshold | Why |
|---|---|---|
| Macro-averaged F1 across the four classes | at least 0.85 | Overall usefulness, weighting the rare classes equally |
| Recall, friendly | at least 0.98 | A friendly classified hostile is the failure that kills people (MOE-02) |
| Recall, neutral or civil | at least 0.98 | Same |
| Precision, hostile | at least 0.90 | A civil track declared hostile is the same failure from the other side |
| Expected calibration error | at most 0.05 | The confidence is consumed as a fusion weight; an overconfident model corrupts every other piece of evidence |
| Inference latency, 200 tracks, CPU | p99 under 5 ms | `use-cases.md` |

### ML-04 anomaly detection

| Metric | Threshold | Why |
|---|---|---|
| Detection rate on planted anomalies (TT-05, TT-07) | at least 0.90 | The scenarios plant an AIS-off loiterer, a spoofed position, clock skew, and a lost sensor |
| False alerts per source per 24 hours | at most 1 | The alert-storm failure mode in the operator task analysis |
| Time from onset to alert | under 120 s | MOP-26, already confirmed |

Thresholds are proposals until the owner confirms them, exactly as the measures in
`../mission/capabilities/measures-catalogue.md` were.

## 3. Shadow mode

Shadow mode is the gate that offline metrics cannot replace, because the training data is
synthetic and the deployment is not.

- The model runs inside the normal tick, on the real snapshot, and its output is
  journalled as an event with the model name and version.
- **No consumer reads it.** Identification fuses without it; observability raises no alert
  from it.
- The analyst compares its output against what the deterministic path did and against
  operator decisions in the same session (`../ux/task-analysis/analyst.md`).
- Exit criterion: an agreed number of sessions, or an agreed number of tracks, with the
  offline thresholds still met on live features and no disagreement pattern that the
  analyst cannot explain.

Shadow mode is also the only honest way to discover that the synthetic training domain
does not match the deployment. That discovery is a success of the process, not a failure
of the model.

## 4. Drift monitoring after promotion

Through `gungnir-observability`, per model, per session:

| Signal | Alert when |
|---|---|
| Feature distribution against the training distribution | A feature's distribution moves beyond the recorded bound |
| Out-of-domain rate | More than a configured fraction of inputs fall outside the class envelopes |
| Confidence distribution | Mean confidence moves materially, in either direction |
| Disagreement with the deterministic path | Above a configured rate, where a deterministic path exists |
| Latency | Budget exceeded, which also disables the model for the session |

Drift raises an alert through the lifecycle. It never auto-rolls-back: rollback is a
recorded human decision like every other (`mlops.md`).

## 5. Rows to add to the verification table

To `../verification-capability-table.md` §2, in its existing format, when `gungnir-ml`
lands. Written here so the rows are agreed before the code:

| Crate | Capability | Verification method | Pass criterion | Data source |
|---|---|---|---|---|
| `gungnir-ml` | Model loading and signature checking | Load a manifest with a good model, a wrong hash, a wrong input signature, and a missing file | Good model loads; each bad case returns the specific `MlError` and never loads; health names the failure | Synthetic manifests |
| `gungnir-ml` | Feature-extractor parity | Golden feature batch from a fixed test-track set, compared with the training repository's extractor | Agreement within 1e-6 | TT-01 sample |
| `gungnir-ml` | Inference determinism and latency | The same batch twice; then a 200-track batch on the CPU provider | Identical outputs; p99 under 5 ms | Golden batch |
| `gungnir-ml` | Absence is honest | Run every consumer with no model set | Consumers produce their deterministic output; health reports the models as absent; no default or fabricated value appears | Synthetic |
| `gungnir-ml` | Budget breach disables the model | Inject a slow model | The model is disabled for the session, an alert is raised, health reports it, and the tick stays inside its budget | Synthetic |
| `gungnir-identification` | Model evidence is one voice | Fuse with and without model evidence, including a confidently wrong model | Classification never changes on model evidence alone below the per-class margin; the evidence list shows the model's contribution | Synthetic plus TT-08 |
| `gungnir-observability` | Anomaly alerts do not quarantine | Anomaly score above threshold on a source | An alert is raised; no detection is quarantined; the gateway's own rules are unaffected | TT-05, TT-07 |
| Cross-layer | Shadow mode consumes nothing | Replay a session with a model in shadow | Journal contains the model's output; every downstream artefact is byte-identical to the run without the model | Recorded session |

The last row is the one that matters most: it is the mechanical proof that shadow mode
is shadow.

## 6. What is not claimed

No accuracy figure in this document has been measured. They are targets set before the
work, which is the right order, and they will be wrong in some places. The first
evaluation run replaces them with observations and the model cards record both.

## Traceability

`use-cases.md` for the targets; `training.md` for what produces the artefacts;
`mlops.md` for promotion; `../verification-capability-table.md` §2 for where the rows
go.
