//! Learned models as the rest of the system may hold them (plan 09; GAP-077, GAP-079).
//!
//! `docs/ml/architecture.md` §2 names the surface: a [`Model`] is something that turns a
//! feature batch into outputs and never fabricates a value on failure; a
//! [`FeatureExtractor`] turns model views into the features a model was trained on and
//! carries the schema version it does it with; a [`ModelSet`] is what a deployment has
//! loaded and, honestly, what it has not. **There is no inference runtime here.** The
//! sign-off was deferred by the owner on 2026-09-05 (§3), so [`ModelSet::load`] refuses
//! with the reason and every consumer that holds an `Option<&dyn Model>` works without
//! one. [`FakeModel`] exists so the consumers can be tested without a runtime (§7).
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
/// declares it has. The artefact itself is not read here, because nothing can run it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelManifestEntry {
    pub name: String,
    pub file: String,
    pub sha256: String,
    pub signature: InputSignature,
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

    /// Load what the manifest names.
    ///
    /// # Errors
    ///
    /// Always, today: [`MlError::RuntimeUnavailable`], because no inference runtime is
    /// signed off (`docs/ml/architecture.md` §3, deferred 2026-09-05). The health of the
    /// set the caller keeps instead records every named model as failed for that reason,
    /// so `SystemHealth` can say which models are missing rather than behaving as though
    /// the models said nothing interesting.
    pub fn load(manifest: &[ModelManifestEntry]) -> Result<Self, MlError> {
        let reason = "no inference runtime is signed off (GAP-077; deferred by the owner \
                      2026-09-05, docs/ml/architecture.md §3)";
        let _ = manifest;
        Err(MlError::RuntimeUnavailable(reason.to_owned()))
    }

    /// The set a deployment keeps when [`load`](Self::load) refused: nothing loaded,
    /// every named model failed with the reason.
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

    #[test]
    fn a_load_refuses_for_want_of_a_runtime_and_health_says_which_models_are_missing() {
        let manifest = vec![ModelManifestEntry {
            name: "ml-01".into(),
            file: "ml-01.onnx".into(),
            sha256: "00".into(),
            signature: signature(),
        }];
        let err = ModelSet::load(&manifest).expect_err("no runtime");
        assert!(matches!(err, MlError::RuntimeUnavailable(_)));
        let set = ModelSet::unavailable(&manifest, &err.to_string());
        assert!(set.get("ml-01").is_none());
        assert_eq!(set.health().failed(), 1);
        assert_eq!(set.health().loaded(), 0);
        assert_eq!(ModelSet::empty().health().models.len(), 0);
    }
}
