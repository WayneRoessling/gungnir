// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Learned models as the rest of the system may hold them (plan 09; GAP-077, GAP-079).
//!
//! `docs/ml/architecture.md` §2 names the surface: a [`Model`] is something that turns a
//! feature batch into outputs and never fabricates a value on failure; a
//! [`FeatureExtractor`] turns model views into the features a model was trained on and
//! carries the schema version it does it with; a [`ModelSet`] is what a deployment has
//! loaded and, honestly, what it has not. **The inference runtime is built as of
//! 2026-09-08 (D-40; `agentic-coding-standards.md` §2.9, "ONNX inference runtime"),
//! behind the `onnx-runtime` feature, off by default.** [`onnx::OnnxModel`] is the real
//! [`Model`], backed by `ort::session::Session`; with the feature on, [`ModelSet::load`]
//! tries to build one per manifest entry and reports, per model, which loaded and which
//! failed and why. **The feature is off by default because of what was found building
//! it, not because it fails to compile**: `ort` 2.0.0-rc.13's `load-dynamic` path panics
//! -- and can hard-abort the process from an atexit handler afterwards -- rather than
//! returning a `Result` when no compatible ONNX Runtime library is reachable, which is
//! every environment this change has run in. Compiling `gungnir-ml` with the feature on
//! costs no C++ toolchain and no network fetch either way (checked 2026-09-08); the gate
//! is about never letting default `cargo test` call into code that can crash the whole
//! test binary, not about build time. See `onnx`'s own module documentation and
//! `agentic-coding-standards.md` §2.9 for the full account. Every consumer still holds an
//! `Option<&dyn Model>` and works without one: no gap yet wires a consumer to this crate
//! (GAP-080 trains the first model there would be something to wire to). [`FakeModel`]
//! exists so the consumers can be tested without a runtime (§7).
//!
//! `dataset` is the other half of plan 09 that needs no runtime: the extraction from a
//! test-track set into the Arrow rows `docs/ml/data-pipeline.md` §2 documents, split by
//! scenario, with provenance and a content hash on every dataset.
//!
//! Position: `gungnir-model ──► gungnir-ml`, and `gungnir-interop` for the catalogue
//! entry the dataset schema is (§1; ARCHITECTURE.md §7.1 edge (q)). Nothing depends on
//! this crate yet: the consumers take model output through their existing traits, and
//! wiring them is GAP-080's promotion, not this crate's construction.

pub mod dataset;
pub mod features;
#[cfg(feature = "onnx-runtime")]
pub mod onnx;

use gungnir_model::TrackId;

/// Why a model could not be loaded or run. Never a value: an error is the only answer
/// to a failure (`docs/ml/architecture.md` §2).
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum MlError {
    #[error("model manifest invalid: {0}")]
    ManifestInvalid(String),
    #[error("model {model}: input signature mismatch, expected {expected}, found {found}")]
    SignatureMismatch {
        model: String,
        expected: String,
        found: String,
    },
    #[error("model {model}: artefact hash does not match its manifest")]
    ArtefactHashMismatch { model: String },
    #[error("model {model}: artefact unavailable: {reason}")]
    ArtefactUnavailable { model: String, reason: String },
    #[error("no inference runtime: {0}")]
    RuntimeUnavailable(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("model {model}: input out of its domain of validity: {reason}")]
    OutOfDomain { model: String, reason: String },
}

/// The shape a model was exported with, checked at load (§2 rule 2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InputSignature {
    /// The feature names, in order. A model trained on a different extractor schema
    /// fails here rather than producing plausible nonsense.
    pub features: Vec<String>,
    pub feature_schema_version: u32,
}

impl InputSignature {
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "v{} [{}]",
            self.feature_schema_version,
            self.features.join(", ")
        )
    }
}

/// One batch of features: one row per track, in the extractor's column order.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureBatch {
    pub feature_schema_version: u32,
    pub feature_names: Vec<String>,
    pub tracks: Vec<TrackId>,
    /// `rows[i]` is `tracks[i]`'s features, `feature_names.len()` long.
    pub rows: Vec<Vec<f32>>,
}

impl FeatureBatch {
    #[must_use]
    pub fn signature(&self) -> InputSignature {
        InputSignature {
            features: self.feature_names.clone(),
            feature_schema_version: self.feature_schema_version,
        }
    }
}

/// One track's scores per class, as evidence and never a decision (plan 09): the
/// identification engine fuses these with everything else it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassScores {
    pub track: TrackId,
    /// Class name and score in `0..=1`, every class the model knows.
    pub scores: Vec<(String, f32)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outputs {
    pub per_track: Vec<ClassScores>,
}

/// What a model is, independent of the runtime that executes it.
pub trait Model: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    /// Input shape the model was exported with; checked against every batch.
    fn input_signature(&self) -> &InputSignature;
    /// One batch in, one batch out. Never panics: a runtime failure is an error.
    ///
    /// # Errors
    ///
    /// [`MlError::SignatureMismatch`] when the batch is not what the model was trained
    /// on; the runtime's error otherwise.
    fn infer(&self, batch: &FeatureBatch) -> Result<Outputs, MlError>;
}

/// Turns model views into the feature vectors a model was trained on.
pub trait FeatureExtractor: Send + Sync {
    /// Written into the dataset and the manifest; a mismatch is a load-time error.
    fn schema_version(&self) -> u32;
    fn feature_names(&self) -> &[String];
}

/// A model that answers with fixed scores, for the consumers' tests (§7). It still
/// checks the signature, because a test that passed a wrong batch to a fake would hide
/// the one thing the surface exists to catch.
#[derive(Debug, Clone)]
pub struct FakeModel {
    name: String,
    version: String,
    signature: InputSignature,
    scores: Vec<(String, f32)>,
}

impl FakeModel {
    #[must_use]
    pub fn new(name: &str, signature: InputSignature, scores: Vec<(String, f32)>) -> Self {
        Self {
            name: name.to_owned(),
            version: "fake".to_owned(),
            signature,
            scores,
        }
    }
}

impl Model for FakeModel {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn input_signature(&self) -> &InputSignature {
        &self.signature
    }

    fn infer(&self, batch: &FeatureBatch) -> Result<Outputs, MlError> {
        let offered = batch.signature();
        if offered != self.signature {
            return Err(MlError::SignatureMismatch {
                model: self.name.clone(),
                expected: self.signature.describe(),
                found: offered.describe(),
            });
        }
        Ok(Outputs {
            per_track: batch
                .tracks
                .iter()
                .map(|t| ClassScores {
                    track: *t,
                    scores: self.scores.clone(),
                })
                .collect(),
        })
    }
}

/// One model the manifest names, as the loader found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStatus {
    Loaded { version: String },
    Failed { reason: String },
}

/// Honest health: which models were expected, which loaded, which failed and why.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelSetHealth {
    pub models: Vec<(String, ModelStatus)>,
}

impl ModelSetHealth {
    #[must_use]
    pub fn loaded(&self) -> usize {
        self.models
            .iter()
            .filter(|(_, s)| matches!(s, ModelStatus::Loaded { .. }))
            .count()
    }

    #[must_use]
    pub fn failed(&self) -> usize {
        self.models.len() - self.loaded()
    }
}

/// One line of a model manifest (`docs/ml/architecture.md` §5): what a deployment
/// declares it has.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelManifestEntry {
    pub name: String,
    /// Semantic, `major.minor.patch` (`docs/ml/mlops.md` §2). `name`@`version` together
    /// are ML-01's evidence-source string, `"ml:model-name@version"` (`use-cases.md`).
    pub version: String,
    pub file: String,
    pub sha256: String,
    pub signature: InputSignature,
    /// The class each output column names, in column order. Stated here rather than
    /// read from the ONNX graph: a graph's own metadata does not reliably carry semantic
    /// class labels, and inventing them from the file would be exactly the kind of
    /// unpopulated field `docs/ml/mlops.md`'s GAP-078 closing action warns against.
    #[serde(default)]
    pub output_classes: Vec<String>,
}

/// The models a deployment has. Empty by default: every model is off until a
/// configuration change enables it (§5).
#[derive(Default)]
pub struct ModelSet {
    models: Vec<Box<dyn Model>>,
    health: ModelSetHealth,
}

impl std::fmt::Debug for ModelSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelSet")
            .field("health", &self.health)
            .finish_non_exhaustive()
    }
}

impl ModelSet {
    /// No models, and health that says so.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Load what the manifest names. With the `onnx-runtime` feature on, each entry's
    /// artefact is read from disk, verified against its stated SHA-256
    /// (`docs/ml/architecture.md` §5), and handed to `onnx::OnnxModel::load`; without it
    /// (the workspace default -- `agentic-coding-standards.md` §2.9, "ONNX inference
    /// runtime"), every entry fails with [`MlError::RuntimeUnavailable`] naming the
    /// feature rather than attempting a load that cannot succeed. Either way, one
    /// entry's failure is recorded in [`Self::health`] and does not stop the rest of the
    /// manifest from loading: honest, per-model health is the entire reason
    /// [`ModelSetHealth`] exists, and a single missing artefact must not read as every
    /// model being absent.
    ///
    /// # Errors
    ///
    /// [`MlError::ManifestInvalid`] when the manifest itself is malformed -- today, two
    /// entries sharing one name, which would make [`Self::get`] and this set's own
    /// health ambiguous about which entry it refers to. A model that individually fails
    /// to load is not a `load` error; see [`Self::health`] for which model and why.
    pub fn load(manifest: &[ModelManifestEntry]) -> Result<Self, MlError> {
        let mut seen = std::collections::HashSet::new();
        for entry in manifest {
            if !seen.insert(entry.name.as_str()) {
                return Err(MlError::ManifestInvalid(format!(
                    "duplicate model name in manifest: {}",
                    entry.name
                )));
            }
        }

        let mut models: Vec<Box<dyn Model>> = Vec::new();
        let mut health = Vec::with_capacity(manifest.len());
        for entry in manifest {
            match Self::load_one(entry) {
                Ok(model) => {
                    health.push((
                        entry.name.clone(),
                        ModelStatus::Loaded {
                            version: model.version().to_owned(),
                        },
                    ));
                    models.push(model);
                }
                Err(e) => {
                    health.push((
                        entry.name.clone(),
                        ModelStatus::Failed {
                            reason: e.to_string(),
                        },
                    ));
                }
            }
        }
        Ok(Self {
            models,
            health: ModelSetHealth { models: health },
        })
    }

    /// One manifest entry, hash-verified then handed to the runtime. Split out of
    /// [`Self::load`] so one entry's failure is a `?` rather than a per-entry closure.
    #[cfg(feature = "onnx-runtime")]
    fn load_one(entry: &ModelManifestEntry) -> Result<Box<dyn Model>, MlError> {
        let bytes = std::fs::read(&entry.file).map_err(|e| MlError::ArtefactUnavailable {
            model: entry.name.clone(),
            reason: format!("reading {}: {e}", entry.file),
        })?;
        onnx::OnnxModel::verify_hash(&entry.name, &entry.sha256, &bytes)?;
        let model = onnx::OnnxModel::load(
            &entry.name,
            &entry.version,
            entry.signature.clone(),
            entry.output_classes.clone(),
            &bytes,
        )?;
        Ok(Box::new(model))
    }

    /// The `onnx-runtime` feature is off (the workspace default). Refuse plainly rather
    /// than attempt a load this build cannot possibly satisfy -- `ort` is not even
    /// compiled in, let alone reachable at runtime (`agentic-coding-standards.md` §2.9,
    /// "ONNX inference runtime": the feature is off because of a checked panic-on-missing-
    /// runtime defect in `ort` 2.0.0-rc.13's `load-dynamic` path, not because this cannot
    /// build).
    #[cfg(not(feature = "onnx-runtime"))]
    fn load_one(entry: &ModelManifestEntry) -> Result<Box<dyn Model>, MlError> {
        let _ = entry;
        Err(MlError::RuntimeUnavailable(
            "this build was compiled without the `onnx-runtime` feature (gungnir-ml's \
             Cargo.toml; agentic-coding-standards.md §2.9, \"ONNX inference runtime\")"
                .to_owned(),
        ))
    }

    /// The set a deployment keeps when nothing can even be attempted -- e.g. no manifest
    /// source is configured -- rather than calling [`Self::load`] with an empty list:
    /// nothing loaded, every named model failed with the given reason.
    #[must_use]
    pub fn unavailable(manifest: &[ModelManifestEntry], reason: &str) -> Self {
        Self {
            models: Vec::new(),
            health: ModelSetHealth {
                models: manifest
                    .iter()
                    .map(|m| {
                        (
                            m.name.clone(),
                            ModelStatus::Failed {
                                reason: reason.to_owned(),
                            },
                        )
                    })
                    .collect(),
            },
        }
    }

    /// A set holding one fake, for tests.
    #[must_use]
    pub fn with_fake(fake: FakeModel) -> Self {
        let health = ModelSetHealth {
            models: vec![(
                fake.name().to_owned(),
                ModelStatus::Loaded {
                    version: fake.version().to_owned(),
                },
            )],
        };
        Self {
            models: vec![Box::new(fake)],
            health,
        }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn Model> {
        self.models
            .iter()
            .find(|m| m.name() == name)
            .map(AsRef::as_ref)
    }

    #[must_use]
    pub fn health(&self) -> &ModelSetHealth {
        &self.health
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature() -> InputSignature {
        InputSignature {
            features: vec!["speed_mps".into()],
            feature_schema_version: 1,
        }
    }

    #[test]
    fn the_fake_answers_with_its_scores_and_refuses_the_wrong_signature() {
        let fake = FakeModel::new("ml-01", signature(), vec![("air.owa-prop".into(), 0.9)]);
        let batch = FeatureBatch {
            feature_schema_version: 1,
            feature_names: vec!["speed_mps".into()],
            tracks: vec![TrackId(3)],
            rows: vec![vec![40.0]],
        };
        let out = fake.infer(&batch).expect("fixed scores");
        assert_eq!(out.per_track[0].track, TrackId(3));
        assert!((out.per_track[0].scores[0].1 - 0.9).abs() < f32::EPSILON);

        let wrong = FeatureBatch {
            feature_schema_version: 2,
            ..batch
        };
        assert!(matches!(
            fake.infer(&wrong),
            Err(MlError::SignatureMismatch { .. })
        ));
    }

    fn manifest_entry(name: &str) -> ModelManifestEntry {
        ModelManifestEntry {
            name: name.into(),
            version: "1.0.0".into(),
            file: format!("does-not-exist-{name}.onnx"),
            sha256: "00".into(),
            signature: signature(),
            output_classes: vec!["hostile".into()],
        }
    }

    /// A missing artefact (or no `ORT_DYLIB_PATH`, which every environment this crate has
    /// actually run in also lacks) is health, not a whole-set refusal: `load` itself
    /// still succeeds, and the one entry that could not be built is named as failed.
    /// This is the direct successor of the test this crate had before a runtime existed
    /// at all, when `load` had no honest way to succeed for *any* manifest.
    #[test]
    fn load_reports_a_missing_artefact_in_health_rather_than_refusing_the_whole_set() {
        let manifest = vec![manifest_entry("ml-01")];
        let set =
            ModelSet::load(&manifest).expect("a missing artefact is health, not a load error");
        assert!(set.get("ml-01").is_none());
        assert_eq!(set.health().failed(), 1);
        assert_eq!(set.health().loaded(), 0);
        assert_eq!(ModelSet::empty().health().models.len(), 0);
    }

    #[test]
    fn load_refuses_a_manifest_with_a_duplicate_name() {
        let manifest = vec![manifest_entry("ml-01"), manifest_entry("ml-01")];
        assert!(matches!(
            ModelSet::load(&manifest),
            Err(MlError::ManifestInvalid(_))
        ));
    }

    #[test]
    fn unavailable_marks_every_named_model_failed_with_the_given_reason() {
        let manifest = vec![manifest_entry("ml-01")];
        let set = ModelSet::unavailable(&manifest, "no manifest source configured");
        assert!(set.get("ml-01").is_none());
        assert_eq!(set.health().failed(), 1);
        assert_eq!(set.health().loaded(), 0);
    }
}
