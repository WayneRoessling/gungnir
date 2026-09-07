# `gungnir-ml` architecture

Status: first draft, 2026-09-04. The crate that loads and runs learned models in every
deployment profile. It is a productization-layer crate: it depends on `gungnir-model`
and the ONNX Runtime binding, and **nothing depends on it being present**.

## 1. Position in the workspace

```
gungnir-model  ──►  gungnir-ml  ──►  (nothing)
                        ▲
     consumers hold an Option<&dyn ModelSet> and work without it:
     gungnir-identification (ML-01), gungnir-observability (ML-04),
     gungnir-assessment (ML-02), gungnir-association (ML-05, advisory)
```

The dependency direction follows the workspace rule: `gungnir-ml` sits beside the other
productization crates and above `gungnir-model`, and the consumers take model output
through their existing traits rather than depending on the crate. A build without
`gungnir-ml` compiles, runs, and reports that learned models are absent.

**The crate and its edge exist as of 2026-09-06** (`gungnir-ml`, edge (q) in
`ARCHITECTURE.md` §7.1, with a second edge to `gungnir-interop` for the dataset schema the
catalogue names). The runtime does not: §3's sign-off stays deferred, and `ModelSet::load`
refuses with that reason. The consumers' edges are still not drawn; nothing depends on the
crate until GAP-080 promotes a model.

## 2. Surface

```rust
/// What a model is, independent of the runtime that executes it.
pub trait Model: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    /// Input shape and dtype the model was exported with; checked at load.
    fn input_signature(&self) -> &InputSignature;
    /// One batch in, one batch out. Never panics: a runtime failure is an error.
    fn infer(&self, batch: &FeatureBatch) -> Result<Outputs, MlError>;
}

/// Turns model views into the feature vectors a model was trained on.
pub trait FeatureExtractor: Send + Sync {
    fn schema_version(&self) -> u32;
    fn extract(&self, input: &ExtractorInput) -> Result<FeatureBatch, MlError>;
}

/// The models a deployment has, loaded from a signed manifest.
pub struct ModelSet { /* name -> Box<dyn Model> */ }

impl ModelSet {
    pub fn load(manifest: &ModelManifest, providers: &[ExecutionProvider]) -> Result<Self, MlError>;
    pub fn get(&self, name: &str) -> Option<&dyn Model>;
    /// Honest health: which models were expected, which loaded, which failed and why.
    pub fn health(&self) -> ModelSetHealth;
}

pub enum MlError {
    ManifestInvalid(String),
    SignatureMismatch { model: String, expected: String, found: String },
    ArtefactHashMismatch { model: String },
    RuntimeUnavailable(String),
    Inference(String),
    OutOfDomain { model: String, reason: String },
}
```

Three rules the surface enforces:

- `infer` returns a `Result`; there is no path that yields a fabricated value on
  failure. This is the same rule as the rest of the workspace: an explicit error over a
  silent stub.
- `input_signature` is checked against the loaded artefact at load time, so a model
  trained on a different feature schema fails immediately rather than producing
  plausible nonsense.
- `health()` reports what is missing. A consumer that finds no model says so in
  `SystemHealth` rather than behaving as though the model said nothing interesting.

## 3. Runtime

**ONNX Runtime through the `ort` crate.** One model file runs unchanged in all three
profiles, which is the acceptance criterion. Execution providers are tried in order and
the chosen one is reported in health:

| Profile | Order | Note |
|---|---|---|
| Desktop with a discrete GPU | CUDA, DirectML, CPU | The desktop already has a GPU for the viewport and the fusion compute device; a third consumer of it is a scheduling question, not a correctness one |
| Desktop without | CPU | The latency budgets in `use-cases.md` are set for CPU so this is the reference case |
| On-prem or cloud node | CPU | Nodes are Linux x86_64 containers (`../../ARCHITECTURE.md` §8.7) and are not assumed to have a GPU |

Alternatives considered and why not, recorded so the choice can be revisited:

| Option | Why not |
|---|---|
| `candle` | Pure Rust and attractive, but the training side would have to export to its formats; ONNX is the interchange format the Python ecosystem exports natively |
| `burn` | Same, plus it is a training framework and the training happens in Python |
| `tract` | Pure-Rust ONNX inference with no native dependency, which is genuinely appealing for the disconnected profile; slower on the desktop and with narrower operator coverage. **Worth reconsidering** if the native ONNX Runtime dependency proves awkward to ship under the release-governance gates |

`ort` is not in the workspace dependencies and is not signed off. Adding it is a §2.9
entry (GAP-077) and brings a native library into a workspace that has none, which the
security reviewer should see before it lands.

**Deliberately deferred, 2026-09-05.** The owner was walked this decision alongside the
other outstanding sign-offs and chose to defer it rather than sign a runtime now, for
three stated reasons: GAP-077 targets increment 4, no model has been trained so nothing
needs a runtime yet, and the security reviewer this section names has not been appointed.
`tract` remains the alternative worth reconsidering when the decision is taken, precisely
because it avoids the native dependency. This is recorded so the absence reads as a
decision rather than as an oversight.

## 4. Feature extraction

Features are derived from `gungnir-model` views only, never from a crate's internals, so
the extractor cannot drift from what the rest of the system sees. Each extractor carries
a `schema_version` that is written into the dataset and into the model manifest; a
mismatch is a load-time error, not a runtime surprise.

The window state an extractor needs (per-track history for ML-01, per-source history for
ML-04) lives in the extractor, is bounded, and is cleared when a track is deleted.

## 5. Configuration and loading

`gungnir-config`'s baseline names the model manifest; the manifest lists each model with
its file, its SHA-256 hash, its feature schema version, its input signature, and its
model card. Loading verifies the hash before the file reaches the runtime. A model whose
hash does not match does not load, and health says which one.

Enabling a model is a configuration change, audited like any other, and every model is
**off by default**.

## 6. Where inference runs in the tick

Batched once per tick, after the tracking service's poll and before assessment, so a
model sees the same snapshot every other consumer sees:

```
ingest → tracking.poll → [ml: one batch per enabled model] → identification/assessment
       → planning → policy → approval → health → journal
```

Inference is on the tick thread within its latency budget. If a model exceeds its budget
it is disabled for the session, an alert is raised, and health reports it: a slow model
must never stretch the frame that the whole picture depends on.

## 7. Testing

- A `FakeModel` returning fixed outputs, so consumers can be tested without a runtime.
- Golden-file tests: a recorded feature batch and the expected outputs, so a runtime or
  version change that alters results is caught.
- The consumers' existing tests run with the model absent, which is the default path.

## Traceability

Use cases `use-cases.md`; governance `mlops.md`; data `data-pipeline.md`; the crate map
and dependency rules `../../ARCHITECTURE.md` §7 and `../../CLAUDE.md`; gaps GAP-077 and
GAP-078.
