// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Model lifecycle & algorithm governance, per docs/gungnir-capabilities.md §5.4 and
//! docs/design/DN-24-mission-profiles-and-algorithm-baselines.md.
//!
//! `gungnir-filters`/`gungnir-association` offer multiple algorithm choices (KF/EKF/UKF/IMM;
//! GNN/JPDA/MHT), but nothing else manages *which configuration is in use where*, or governs
//! changing it safely across mission profiles. A baseline must be `Validated` before it can
//! be `Promoted`, and rollback restores the previously promoted baseline for that profile.
//!
//! # What changed with DN-24
//!
//! A baseline used to be keyed on a profile name alone and matched by its *configuration*.
//! **Two candidates in one profile with the same settings were the same baseline**, and a
//! rollback could not say which it restored. Identity is now
//! [`gungnir_model::AlgorithmBaselineId`] -- the profile and the candidate's own name -- and
//! matching is on that.
//!
//! # What reaches the picture, and what this crate still cannot do
//!
//! A promoted baseline **now governs the filtering** and not only the record, which it did
//! not when this crate was written: `PIPELINE_IMPLEMENTED` is true (GAP-011) and both
//! binaries build the pipeline with `gungnir_fusion_async::PipelineSettings::from_baseline`
//! (GAP-053). DN-24 §7's rule is held at that boundary: **the tracking service stamps an
//! `AlgorithmBaselineId` into `Provenance` only when it applied one**, so a baseline naming
//! a filter outside `IMPLEMENTED_FILTERS` leaves the tracker ungoverned and says so.
//!
//! What is still not here is runtime promotion: promotion is what the baseline declares,
//! and changing it is `config.apply`.

use gungnir_config::{ConfigBaseline, TrackingConfig};
use gungnir_model::{AlgorithmBaselineId, MissionProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PromotionState {
    Candidate,
    Validated,
    Promoted,
    RolledBack,
}

/// A model artefact as this crate's promotion machinery governs it (GAP-078;
/// `docs/ml/mlops.md` §1's recommended option, chosen over a parallel registry): carried
/// by [`ModelBaseline`] alongside the tracking configuration so both are validated,
/// promoted, and rolled back together, through the one state machine
/// [`ModelBaseline::state`] already is -- "one registry, one promotion state machine,
/// one audit trail for what is in force" (`mlops.md` §1). `PromotionState` is therefore
/// not repeated here: it already lives on the baseline this type is a field of.
///
/// Deliberately does not depend on `gungnir-ml`: nothing in `ARCHITECTURE.md` draws that
/// edge (`gungnir-ml` is documented as having no dependents at all, `docs/ml/
/// architecture.md` §1), and this type is what governance needs to know about an
/// artefact, not what `gungnir-ml::ModelSet::load` needs to load one -- the two crates
/// describe the same artefact from different sides of the promotion boundary on
/// purpose.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelArtifactManifest {
    /// The model's own name, e.g. `"ml-01"` -- distinct from `ModelBaseline::id`'s
    /// candidate name, which labels a tuning attempt, not a model.
    pub name: String,
    /// Semantic, `major.minor.patch` (`docs/ml/mlops.md` §2).
    pub version: String,
    /// The artefact's SHA-256, lowercase hex (`docs/ml/architecture.md` §5: the manifest
    /// states this so a hash mismatch refuses the artefact before it reaches a runtime).
    pub sha256: String,
    pub evaluation: EvaluationRecord,
}

/// What a model showed before promotion was considered: the three gates
/// `docs/ml/evaluation-and-verification.md` §1 names, offline, parity and shadow.
/// Free-form gate names rather than one field per named gate, because this crate does
/// not own the gate catalogue -- that document does, and a fourth gate added there must
/// not need a schema change here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EvaluationRecord {
    /// Every gate this artefact has passed, e.g. `["offline", "parity"]`. Order is not
    /// meaningful; membership is what `validate` checks.
    pub gates_passed: Vec<String>,
    /// Where the evaluation report lives -- a path, a report id, or a citation. Never
    /// empty for a real record: an evaluation nobody can point to is indistinguishable
    /// from one that did not happen, which is the invented-field failure GAP-078's own
    /// closing action in the gap register warns against.
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelBaseline {
    /// Which candidate, in which profile.
    pub id: AlgorithmBaselineId,
    pub config: TrackingConfig,
    pub state: PromotionState,
    /// What validated this candidate, when the baseline said (DN-24 §4).
    #[serde(default)]
    pub validated_by: Option<String>,
    /// The model artefact this candidate promotes alongside its tracking configuration,
    /// if it names one (GAP-078). `None` for every baseline before this field existed,
    /// and for any baseline that governs tracking configuration only -- carrying no
    /// model is a valid, common answer, not a missing one.
    #[serde(default)]
    pub model: Option<ModelArtifactManifest>,
}

pub trait ModelRegistry: Send + Sync {
    fn candidates(&self, profile: &MissionProfile) -> Vec<ModelBaseline>;
    fn promote(&mut self, baseline: &ModelBaseline) -> Result<(), ModelOpsError>;
    fn rollback(&mut self, profile: &MissionProfile) -> Result<(), ModelOpsError>;
    /// The baseline currently in force for the profile, if any.
    fn promoted(&self, profile: &MissionProfile) -> Option<&ModelBaseline>;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelOpsError {
    #[error("baseline failed validation gates: {0}")]
    ValidationFailed(String),
    #[error("baseline is {0:?}; only Validated baselines can be promoted")]
    NotValidated(PromotionState),
    #[error("nothing to roll back for profile {0}")]
    NothingToRollBack(MissionProfile),
    /// The registry has no such candidate.
    ///
    /// Its own error rather than a silent no-op: a promotion naming a baseline that is not
    /// there is a typo somebody has to see, and treating it as nothing done would leave the
    /// deployment running the old configuration while the operator believed otherwise.
    #[error("no candidate {0} in this registry")]
    UnknownBaseline(AlgorithmBaselineId),
}

#[derive(Debug, Default)]
pub struct InMemoryModelRegistry {
    baselines: Vec<ModelBaseline>,
    /// Promotion history per profile, newest last.
    history: Vec<ModelBaseline>,
}

impl InMemoryModelRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry a deployment's baseline describes (DN-24 §6).
    ///
    /// Every candidate is registered, **run through the real validation gate**, and the one
    /// the file marks promoted is promoted through the state machine. The file asserting
    /// `promoted: true` does not bypass `promote`'s requirement that a baseline be
    /// `Validated` first -- a promoted candidate that fails the gate refuses the baseline,
    /// which is the point of running it here rather than trusting the flag.
    ///
    /// # Errors
    ///
    /// [`ModelOpsError::ValidationFailed`] for a candidate the gate rejects, and whatever
    /// `promote` returns.
    pub fn from_baseline(config: &ConfigBaseline) -> Result<Self, ModelOpsError> {
        let mut registry = Self::new();
        let candidates = config.algorithm_candidates();
        for candidate in &candidates {
            registry.register(ModelBaseline {
                id: candidate.id.clone(),
                config: candidate.config.clone(),
                state: PromotionState::Candidate,
                validated_by: candidate.validated_by.clone(),
                // `gungnir-config`'s schema carries no model manifest field (GAP-078's
                // closing action builds the governance type and its validation, not a
                // config-file producer for it -- no model exists yet to name, GAP-080).
                model: None,
            });
        }
        for index in 0..registry.baselines.len() {
            registry.validate(index)?;
        }
        for candidate in candidates.iter().filter(|c| c.promoted) {
            let baseline = registry
                .baselines
                .iter()
                .find(|b| b.id == candidate.id)
                .cloned()
                .ok_or_else(|| ModelOpsError::UnknownBaseline(candidate.id.clone()))?;
            registry.promote(&baseline)?;
        }
        Ok(registry)
    }

    /// Register a candidate; validation is a separate, explicit step.
    pub fn register(&mut self, baseline: ModelBaseline) {
        self.baselines.push(baseline);
    }

    /// Run the validation gates. Today the gate is structural (the config must be
    /// valid under `gungnir-config`'s rules, and a model artefact if one is named must
    /// be a real-shaped one -- GAP-078); the oracle-comparison gates from
    /// `verification-capability-table.md` §1 plug in here.
    ///
    /// # Errors
    ///
    /// [`ModelOpsError::ValidationFailed`] when there is no such candidate, when its
    /// configuration is one no filter could run, or when it names a model artefact this
    /// crate cannot recognise as real (an empty name, version, or evidence citation, an
    /// artefact hash that is not 64 lowercase hex characters, or no gate recorded as
    /// passed at all -- `docs/ml/mlops.md` §1's own warning that an unpopulated field is
    /// invented wrongly, made structural rather than left to convention).
    pub fn validate(&mut self, index: usize) -> Result<(), ModelOpsError> {
        let b = self
            .baselines
            .get_mut(index)
            .ok_or_else(|| ModelOpsError::ValidationFailed("no such baseline".into()))?;
        if !(b.config.gate_threshold.is_finite() && b.config.gate_threshold > 0.0)
            || b.config.filter_selection.trim().is_empty()
        {
            return Err(ModelOpsError::ValidationFailed(format!(
                "invalid tracking config for {}",
                b.id
            )));
        }
        if let Some(model) = &b.model {
            if model.name.trim().is_empty()
                || model.version.trim().is_empty()
                || model.evaluation.evidence.trim().is_empty()
            {
                return Err(ModelOpsError::ValidationFailed(format!(
                    "model artefact for {} names an empty field",
                    b.id
                )));
            }
            let is_sha256_hex = model.sha256.len() == 64
                && model
                    .sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
            if !is_sha256_hex {
                return Err(ModelOpsError::ValidationFailed(format!(
                    "model artefact for {} has a sha256 that is not 64 lowercase hex characters",
                    b.id
                )));
            }
            if model.evaluation.gates_passed.is_empty() {
                return Err(ModelOpsError::ValidationFailed(format!(
                    "model artefact for {} names no evaluation gate as passed",
                    b.id
                )));
            }
        }
        b.state = PromotionState::Validated;
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: &AlgorithmBaselineId) -> Option<&ModelBaseline> {
        self.baselines.iter().find(|b| &b.id == id)
    }

    /// Every profile this registry holds candidates for.
    #[must_use]
    pub fn profiles(&self) -> Vec<MissionProfile> {
        let mut out: Vec<MissionProfile> = Vec::new();
        for b in &self.baselines {
            if !out.contains(&b.id.profile) {
                out.push(b.id.profile.clone());
            }
        }
        out
    }
}

impl ModelRegistry for InMemoryModelRegistry {
    fn candidates(&self, profile: &MissionProfile) -> Vec<ModelBaseline> {
        self.baselines
            .iter()
            .filter(|b| &b.id.profile == profile)
            .cloned()
            .collect()
    }

    fn promote(&mut self, baseline: &ModelBaseline) -> Result<(), ModelOpsError> {
        if baseline.state != PromotionState::Validated {
            return Err(ModelOpsError::NotValidated(baseline.state));
        }
        let mut promoted = baseline.clone();
        promoted.state = PromotionState::Promoted;
        self.history.push(promoted.clone());
        match self.baselines.iter_mut().find(|b| b.id == baseline.id) {
            Some(existing) => existing.state = PromotionState::Promoted,
            None => self.baselines.push(promoted),
        }
        Ok(())
    }

    fn rollback(&mut self, profile: &MissionProfile) -> Result<(), ModelOpsError> {
        let mut promoted: Vec<usize> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, b)| &b.id.profile == profile)
            .map(|(i, _)| i)
            .collect();
        let Some(current) = promoted.pop() else {
            return Err(ModelOpsError::NothingToRollBack(profile.clone()));
        };
        let current_id = self.history[current].id.clone();
        self.history.remove(current);
        if let Some(b) = self.baselines.iter_mut().find(|b| b.id == current_id) {
            b.state = PromotionState::RolledBack;
        }
        if let Some(previous) = promoted.pop() {
            let previous_id = self.history[previous].id.clone();
            if let Some(b) = self.baselines.iter_mut().find(|b| b.id == previous_id) {
                b.state = PromotionState::Promoted;
            }
        }
        Ok(())
    }

    fn promoted(&self, profile: &MissionProfile) -> Option<&ModelBaseline> {
        self.history.iter().rev().find(|b| &b.id.profile == profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_config::TrackingProfileConfig;

    fn coastal() -> MissionProfile {
        MissionProfile::new("coastal")
    }

    fn baseline(name: &str, filter: &str) -> ModelBaseline {
        ModelBaseline {
            id: AlgorithmBaselineId::new("coastal", name),
            config: TrackingConfig {
                filter_selection: filter.into(),
                gate_threshold: 9.21,
                // Inert here: this crate's own `validate` (unlike `gungnir-config`'s)
                // does not read the imm-cv-ct fields, or measurement_noise_var, at all.
                imm_turn_rate_rad_s: 0.0,
                imm_mode_transition: [[0.0; 2]; 2],
                imm_initial_mode_probabilities: [0.0; 2],
                measurement_noise_var: [0.0; 3],
            },
            state: PromotionState::Candidate,
            validated_by: None,
            model: None,
        }
    }

    fn model_artifact(sha256: &str, gates_passed: Vec<&str>) -> ModelArtifactManifest {
        ModelArtifactManifest {
            name: "ml-01".into(),
            version: "1.0.0".into(),
            sha256: sha256.into(),
            evaluation: EvaluationRecord {
                gates_passed: gates_passed.into_iter().map(str::to_owned).collect(),
                evidence: "docs/ml/evaluation-and-verification.md run 2026-09-08".into(),
            },
        }
    }

    #[test]
    fn promotion_requires_validation() {
        let mut r = InMemoryModelRegistry::new();
        assert!(matches!(
            r.promote(&baseline("ekf", "ekf")),
            Err(ModelOpsError::NotValidated(PromotionState::Candidate))
        ));
    }

    #[test]
    fn validate_promote_rollback_restores_previous() {
        let mut r = InMemoryModelRegistry::new();
        r.register(baseline("ekf", "ekf"));
        r.register(baseline("imm", "imm-cv-ct"));
        r.validate(0).expect("validate ekf");
        r.validate(1).expect("validate imm");
        let ekf = r.candidates(&coastal())[0].clone();
        let imm = r.candidates(&coastal())[1].clone();
        r.promote(&ekf).expect("promote ekf");
        r.promote(&imm).expect("promote imm");
        assert_eq!(r.promoted(&coastal()).expect("in force").id.name, "imm");
        r.rollback(&coastal()).expect("rollback");
        assert_eq!(r.promoted(&coastal()).expect("restored").id.name, "ekf");
        let states: Vec<PromotionState> =
            r.candidates(&coastal()).iter().map(|b| b.state).collect();
        assert_eq!(
            states,
            vec![PromotionState::Promoted, PromotionState::RolledBack]
        );
    }

    /// **What identity by name buys.** Two candidates whose settings happen to match are
    /// still two baselines, and a rollback says which one it restored. Keyed on the
    /// configuration, as this registry used to be, both of these were one row.
    #[test]
    fn two_candidates_with_identical_settings_are_still_two_baselines() {
        let mut r = InMemoryModelRegistry::new();
        r.register(baseline("as tuned in March", "imm-cv-ct"));
        r.register(baseline("as tuned in April", "imm-cv-ct"));
        r.validate(0).expect("validate");
        r.validate(1).expect("validate");
        let march = r.candidates(&coastal())[0].clone();
        let april = r.candidates(&coastal())[1].clone();
        r.promote(&march).expect("promote march");
        r.promote(&april).expect("promote april");

        assert_eq!(
            r.promoted(&coastal()).expect("in force").id.name,
            "as tuned in April"
        );
        r.rollback(&coastal()).expect("rollback");
        assert_eq!(
            r.promoted(&coastal()).expect("restored").id.name,
            "as tuned in March",
            "the rollback could not say which candidate it restored"
        );
    }

    #[test]
    fn rollback_with_no_history_is_an_error() {
        let mut r = InMemoryModelRegistry::new();
        assert!(matches!(
            r.rollback(&coastal()),
            Err(ModelOpsError::NothingToRollBack(_))
        ));
    }

    fn candidate(profile: &str, name: &str, promoted: bool) -> TrackingProfileConfig {
        TrackingProfileConfig {
            profile: profile.into(),
            name: name.into(),
            filter_selection: "imm-cv-ct".into(),
            gate_threshold: 9.21,
            imm_turn_rate_rad_s: 0.0,
            imm_mode_transition: [[0.0; 2]; 2],
            imm_initial_mode_probabilities: [0.0; 2],
            measurement_noise_var: [0.0; 3],
            promoted,
            validated_by: Some("oracle comparison 2026-09-05".into()),
        }
    }

    /// The registry a deployment's file describes, built through the real state machine.
    #[test]
    fn a_registry_from_a_baseline_promotes_what_the_file_marks() {
        let config = ConfigBaseline {
            mission_profiles: vec!["air-defence".into(), "counter-uas".into()],
            tracking_profiles: vec![
                candidate("air-defence", "imm baseline", true),
                candidate("air-defence", "tighter gate", false),
                candidate("counter-uas", "short range", true),
            ],
            active_profile: Some("air-defence".into()),
            ..ConfigBaseline::default()
        };
        let r = InMemoryModelRegistry::from_baseline(&config).expect("registry");

        assert_eq!(r.profiles().len(), 2);
        assert_eq!(
            r.promoted(&MissionProfile::new("air-defence"))
                .expect("in force")
                .id
                .name,
            "imm baseline"
        );
        assert_eq!(
            r.promoted(&MissionProfile::new("counter-uas"))
                .expect("in force")
                .id
                .name,
            "short range"
        );
        // The un-promoted candidate is there to be rolled forward to, which is the whole
        // reason a profile has more than one.
        let states: Vec<PromotionState> = r
            .candidates(&MissionProfile::new("air-defence"))
            .iter()
            .map(|b| b.state)
            .collect();
        assert_eq!(
            states,
            vec![PromotionState::Promoted, PromotionState::Validated]
        );
        assert_eq!(
            r.get(&AlgorithmBaselineId::new("air-defence", "imm baseline"))
                .and_then(|b| b.validated_by.as_deref()),
            Some("oracle comparison 2026-09-05")
        );
    }

    /// **The file asserting `promoted` does not bypass the gate.** A candidate that cannot
    /// run refuses the baseline rather than being promoted because a flag said so.
    #[test]
    fn a_promoted_candidate_that_fails_the_gate_refuses_the_baseline() {
        let mut bad = candidate("air-defence", "broken", true);
        bad.gate_threshold = f64::NAN;
        let config = ConfigBaseline {
            mission_profiles: vec!["air-defence".into()],
            tracking_profiles: vec![bad],
            ..ConfigBaseline::default()
        };
        assert!(matches!(
            InMemoryModelRegistry::from_baseline(&config),
            Err(ModelOpsError::ValidationFailed(_))
        ));
    }

    /// A baseline written before DN-24 is one implicit `default` profile, promoted. The
    /// registry it produces is real, and it is honest that there is nothing to choose.
    #[test]
    fn a_baseline_with_only_tracking_yields_one_promoted_default() {
        let config = ConfigBaseline {
            tracking: Some(TrackingConfig {
                filter_selection: "imm-cv-ct".into(),
                gate_threshold: 9.21,
                imm_turn_rate_rad_s: 0.0,
                imm_mode_transition: [[0.0; 2]; 2],
                imm_initial_mode_probabilities: [0.0; 2],
                measurement_noise_var: [0.0; 3],
            }),
            ..ConfigBaseline::default()
        };
        let r = InMemoryModelRegistry::from_baseline(&config).expect("registry");
        let default = MissionProfile::new(MissionProfile::DEFAULT);
        assert_eq!(r.candidates(&default).len(), 1);
        assert_eq!(
            r.promoted(&default).expect("in force").id.name,
            "configured"
        );
        // And a rollback has nothing to restore, which is the truth about such a
        // deployment rather than a failure of this crate.
        let mut r = r;
        assert!(matches!(
            r.rollback(&default),
            Ok(()) | Err(ModelOpsError::NothingToRollBack(_))
        ));
        assert!(r.promoted(&default).is_none());
    }

    /// The default deployment governs nothing, and the registry says so rather than
    /// inventing a configuration to govern.
    #[test]
    fn a_baseline_with_no_configuration_governs_nothing() {
        let r = InMemoryModelRegistry::from_baseline(&ConfigBaseline::default()).expect("registry");
        assert!(r.profiles().is_empty());
        assert!(r
            .promoted(&MissionProfile::new(MissionProfile::DEFAULT))
            .is_none());
    }

    // GAP-078: a model artefact is carried on `ModelBaseline` and promoted, validated,
    // and rolled back through the same state machine as the tracking configuration
    // beside it, rather than a parallel registry (`docs/ml/mlops.md` §1).

    /// A baseline naming no model validates and promotes exactly as before GAP-078:
    /// carrying no model is the common case (no model exists to promote yet, GAP-080),
    /// not a degraded one.
    #[test]
    fn a_baseline_naming_no_model_is_unaffected_by_the_model_gate() {
        let mut r = InMemoryModelRegistry::new();
        r.register(baseline("ekf", "ekf"));
        r.validate(0)
            .expect("a config-only baseline still validates");
        let b = r.candidates(&coastal())[0].clone();
        r.promote(&b)
            .expect("a config-only baseline still promotes");
        assert!(r.promoted(&coastal()).expect("in force").model.is_none());
    }

    /// A real-shaped model artefact -- name, version, a 64-hex-character hash, and at
    /// least one gate recorded passed with evidence -- validates and is promoted
    /// alongside its tracking configuration in the one state machine.
    #[test]
    fn a_baseline_naming_a_well_formed_model_validates_and_promotes_with_it() {
        let mut b = baseline("ekf", "ekf");
        b.model = Some(model_artifact(&"a".repeat(64), vec!["offline", "parity"]));
        let mut r = InMemoryModelRegistry::new();
        r.register(b);
        r.validate(0)
            .expect("a well-formed model artefact validates");
        let promoted = r.candidates(&coastal())[0].clone();
        r.promote(&promoted).expect("promotes with its model");
        let in_force = r.promoted(&coastal()).expect("in force");
        let model = in_force
            .model
            .as_ref()
            .expect("the model rode along with the baseline");
        assert_eq!(model.name, "ml-01");
        assert_eq!(model.evaluation.gates_passed, vec!["offline", "parity"]);
    }

    /// Each of the fields `docs/ml/mlops.md` §1 warns get invented wrongly when nothing
    /// populates them -- name, version, hash shape, and evaluation evidence -- is
    /// checked structurally, named individually so the fix is unambiguous.
    #[test]
    fn a_model_artefact_missing_any_stated_field_refuses_validation() {
        let cases: Vec<ModelArtifactManifest> = vec![
            ModelArtifactManifest {
                name: String::new(),
                ..model_artifact(&"a".repeat(64), vec!["offline"])
            },
            ModelArtifactManifest {
                version: String::new(),
                ..model_artifact(&"a".repeat(64), vec!["offline"])
            },
            ModelArtifactManifest {
                evaluation: EvaluationRecord {
                    evidence: String::new(),
                    ..model_artifact(&"a".repeat(64), vec!["offline"]).evaluation
                },
                ..model_artifact(&"a".repeat(64), vec!["offline"])
            },
            // Too short.
            model_artifact("abc123", vec!["offline"]),
            // Right length, not hex.
            model_artifact(&"z".repeat(64), vec!["offline"]),
            // Uppercase hex is still refused: the manifest's own convention
            // (`gungnir-ml`'s `format!("{:x}", ...)`) is always lowercase, and accepting
            // both would let two manifests name the same artefact two different ways.
            model_artifact(&"A".repeat(64), vec!["offline"]),
            // No gate recorded passed at all.
            model_artifact(&"a".repeat(64), vec![]),
        ];
        for model in cases {
            let mut b = baseline("ekf", "ekf");
            b.model = Some(model.clone());
            let mut r = InMemoryModelRegistry::new();
            r.register(b);
            assert!(
                matches!(r.validate(0), Err(ModelOpsError::ValidationFailed(_))),
                "expected {model:?} to fail validation"
            );
        }
    }

    /// Two candidates in one profile, one naming a model and one not, are still
    /// governed independently -- a model is a property of the candidate, not of the
    /// profile.
    #[test]
    fn rollback_restores_the_previous_candidates_model_along_with_its_config() {
        let mut r = InMemoryModelRegistry::new();
        let mut with_model = baseline("with-model", "ekf");
        with_model.model = Some(model_artifact(&"a".repeat(64), vec!["offline", "parity"]));
        r.register(with_model);
        r.register(baseline("without-model", "imm-cv-ct"));
        r.validate(0).expect("validate with-model");
        r.validate(1).expect("validate without-model");
        let with_model = r.candidates(&coastal())[0].clone();
        let without_model = r.candidates(&coastal())[1].clone();
        r.promote(&with_model).expect("promote with-model");
        r.promote(&without_model).expect("promote without-model");
        assert!(r.promoted(&coastal()).expect("in force").model.is_none());
        r.rollback(&coastal()).expect("rollback");
        assert!(r.promoted(&coastal()).expect("restored").model.is_some());
    }
}
