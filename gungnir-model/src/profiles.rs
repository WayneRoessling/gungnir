// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mission profiles and algorithm-baseline identity, per
//! docs/design/DN-24-mission-profiles-and-algorithm-baselines.md §4.
//!
//! # What a mission profile is
//!
//! A named operating context in which one algorithm configuration is in force. **Not an
//! enumeration**: a sector defending a harbour against small UAS and one covering an
//! approach corridor against fast movers want different gates, and no list written here
//! would survive contact with a deployment. A profile is declared in the baseline by name,
//! the way an endpoint is, and referenced by name from the candidates that belong to it.
//!
//! The word is the mission layer's already -- CAP-5.7's statement and
//! `docs/gungnir-capabilities.md` §5.4 both use it -- so these types give it a schema, not
//! a meaning.
//!
//! # Why the identity has two parts
//!
//! `gungnir-modelops` keyed its registry on the profile alone, which meant **two candidates
//! in one profile were indistinguishable in the record**. A rollback names what it restored
//! and a promotion names what went into force; neither can be written down without the
//! candidate's own name.

/// A named operating context. One algorithm configuration is in force per profile.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct MissionProfile(pub String);

impl MissionProfile {
    /// The profile a baseline declaring only `tracking` is read as (DN-24 §6).
    ///
    /// A deployment with one configuration and no profiles means "this is what we run", and
    /// this is that sentence in the new vocabulary rather than a reinterpretation of it.
    pub const DEFAULT: &'static str = "default";

    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MissionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which candidate, in which profile.
///
/// What a promotion puts into force, what a rollback restores, and what
/// `Provenance::algorithm_version` carries **once the pipeline actually applies one**
/// (DN-24 §7). Until then the honest stamp names what is missing instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AlgorithmBaselineId {
    pub profile: MissionProfile,
    pub name: String,
}

impl AlgorithmBaselineId {
    #[must_use]
    pub fn new(profile: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            profile: MissionProfile::new(profile),
            name: name.into(),
        }
    }
}

impl std::fmt::Display for AlgorithmBaselineId {
    /// `profile/name`, which is what goes in a provenance stamp and a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.profile, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_reads_as_profile_then_name() {
        let id = AlgorithmBaselineId::new("air-defence", "tighter gate");
        assert_eq!(id.to_string(), "air-defence/tighter gate");
    }

    /// Two candidates in one profile are different baselines. The registry keyed on the
    /// profile alone could not tell them apart, which is why the name is part of identity.
    #[test]
    fn two_candidates_in_one_profile_are_not_the_same_baseline() {
        assert_ne!(
            AlgorithmBaselineId::new("air-defence", "imm baseline"),
            AlgorithmBaselineId::new("air-defence", "tighter gate")
        );
    }

    #[test]
    fn the_same_name_in_two_profiles_is_not_the_same_baseline() {
        assert_ne!(
            AlgorithmBaselineId::new("air-defence", "baseline"),
            AlgorithmBaselineId::new("counter-uas", "baseline")
        );
    }
}
