// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The words the interface uses for domain values (D-12, GAP-070).
//!
//! D-12 chose the NATO and joint terms of `docs/mission/glossary.md` as the default UI
//! labels, with a per-deployment override table in the configuration baseline. This is
//! both halves: the defaults, and the lookup an override goes through.
//!
//! # Why this is not just a nicety
//!
//! Before it, several of these values reached the operator as `format!("{:?}", ..)` --
//! Rust variant names. A screen reading `SelfDefence` or `Free` is showing an operator
//! an identifier from a type definition, not a term from their doctrine, and the two
//! differ in ways that matter: the joint term is *weapons free*, and "Free" on its own
//! is close to the opposite of what an unfamiliar reader would guess. The glossary says
//! so in as many words.
//!
//! # Overriding is renaming, never remapping
//!
//! An override changes the word shown for a value. It cannot change which value is
//! shown, merge two values into one word, or introduce a value the model does not have.
//! A deployment that calls a hostile track something else still has a hostile track, and
//! every policy engine still treats it as one. That is why the table is keyed by
//! [`Term`] rather than being free-form text substitution: there is no key for a concept
//! the system does not hold, and `gungnir-config` refuses a key it does not know rather
//! than ignoring it, so a typo in a baseline is reported instead of silently leaving the
//! default in place.

use crate::{Classification, EffectorLayer, SensorMode, WeaponsControlStatus};
use gungnir_core::TrackStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A domain value the interface has to put a word to.
///
/// Deliberately a closed enum over the model's own types rather than an open string
/// space: every term the interface can show is one of these, which is what makes
/// [`Term::ALL`] an exhaustive check that no label is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    Classification(Classification),
    TrackStatus(TrackStatus),
    Layer(EffectorLayer),
    ControlStatus(WeaponsControlStatus),
    SensorMode(SensorMode),
}

impl Term {
    /// Every term the interface can show. The exhaustiveness the label test relies on.
    pub const ALL: &'static [Term] = &[
        Term::Classification(Classification::Hostile),
        Term::Classification(Classification::Friendly),
        Term::Classification(Classification::Neutral),
        Term::Classification(Classification::Unknown),
        Term::TrackStatus(TrackStatus::Tentative),
        Term::TrackStatus(TrackStatus::Confirmed),
        Term::TrackStatus(TrackStatus::Coasting),
        Term::TrackStatus(TrackStatus::Deleted),
        Term::Layer(EffectorLayer::Area),
        Term::Layer(EffectorLayer::Point),
        Term::Layer(EffectorLayer::SelfDefence),
        Term::Layer(EffectorLayer::NonKinetic),
        Term::ControlStatus(WeaponsControlStatus::Free),
        Term::ControlStatus(WeaponsControlStatus::Tight),
        Term::ControlStatus(WeaponsControlStatus::Hold),
        Term::SensorMode(SensorMode::Standby),
        Term::SensorMode(SensorMode::Search),
        Term::SensorMode(SensorMode::Track),
        Term::SensorMode(SensorMode::Calibrating),
        Term::SensorMode(SensorMode::Offline),
    ];

    /// The key a baseline overrides this term by. Stable: it is part of the
    /// configuration schema, so it may not be renamed without a schema version.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Term::Classification(Classification::Hostile) => "classification.hostile",
            Term::Classification(Classification::Friendly) => "classification.friendly",
            Term::Classification(Classification::Neutral) => "classification.neutral",
            Term::Classification(Classification::Unknown) => "classification.unknown",
            Term::TrackStatus(TrackStatus::Tentative) => "track_status.tentative",
            Term::TrackStatus(TrackStatus::Confirmed) => "track_status.confirmed",
            Term::TrackStatus(TrackStatus::Coasting) => "track_status.coasting",
            Term::TrackStatus(TrackStatus::Deleted) => "track_status.deleted",
            Term::Layer(EffectorLayer::Area) => "layer.area",
            Term::Layer(EffectorLayer::Point) => "layer.point",
            Term::Layer(EffectorLayer::SelfDefence) => "layer.self_defence",
            Term::Layer(EffectorLayer::NonKinetic) => "layer.non_kinetic",
            Term::ControlStatus(WeaponsControlStatus::Free) => "control_status.free",
            Term::ControlStatus(WeaponsControlStatus::Tight) => "control_status.tight",
            Term::ControlStatus(WeaponsControlStatus::Hold) => "control_status.hold",
            Term::SensorMode(SensorMode::Standby) => "sensor_mode.standby",
            Term::SensorMode(SensorMode::Search) => "sensor_mode.search",
            Term::SensorMode(SensorMode::Track) => "sensor_mode.track",
            Term::SensorMode(SensorMode::Calibrating) => "sensor_mode.calibrating",
            Term::SensorMode(SensorMode::Offline) => "sensor_mode.offline",
        }
    }

    /// The default word, from `docs/mission/glossary.md` and the joint and NATO usage it
    /// maps to.
    ///
    /// Two of these are not the variant name, on purpose. `Friendly` shows as **Friend**,
    /// which is the NATO standard identity; and the control statuses show as **Weapons
    /// free**, **Weapons tight** and **Weapons hold**, which the glossary gives as the
    /// joint terms and which are the only form that reads unambiguously -- "Free" alone
    /// invites the opposite reading.
    #[must_use]
    pub fn default_label(self) -> &'static str {
        match self {
            Term::Classification(Classification::Hostile) => "Hostile",
            Term::Classification(Classification::Friendly) => "Friend",
            Term::Classification(Classification::Neutral) => "Neutral",
            Term::Classification(Classification::Unknown) => "Unknown",
            Term::TrackStatus(TrackStatus::Tentative) => "Tentative",
            Term::TrackStatus(TrackStatus::Confirmed) => "Confirmed",
            Term::TrackStatus(TrackStatus::Coasting) => "Coasting",
            Term::TrackStatus(TrackStatus::Deleted) => "Deleted",
            Term::Layer(EffectorLayer::Area) => "Area defence",
            Term::Layer(EffectorLayer::Point) => "Point defence",
            Term::Layer(EffectorLayer::SelfDefence) => "Self-defence",
            Term::Layer(EffectorLayer::NonKinetic) => "Non-kinetic",
            Term::ControlStatus(WeaponsControlStatus::Free) => "Weapons free",
            Term::ControlStatus(WeaponsControlStatus::Tight) => "Weapons tight",
            Term::ControlStatus(WeaponsControlStatus::Hold) => "Weapons hold",
            Term::SensorMode(SensorMode::Standby) => "Standby",
            Term::SensorMode(SensorMode::Search) => "Search",
            Term::SensorMode(SensorMode::Track) => "Track",
            Term::SensorMode(SensorMode::Calibrating) => "Calibrating",
            Term::SensorMode(SensorMode::Offline) => "Offline",
        }
    }

    /// The term a key names, or `None` for a key this schema does not define.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Term> {
        Term::ALL.iter().copied().find(|t| t.key() == key)
    }
}

/// The display vocabulary in force: the defaults, with a deployment's overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vocabulary {
    /// Overrides keyed by [`Term::key`]. Absent keys keep the default.
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
}

impl Vocabulary {
    /// The word to show for a value.
    ///
    /// Always returns something: an unconfigured term falls back to the glossary
    /// default, so no code path can leave a label empty and no panel needs to handle a
    /// missing one.
    #[must_use]
    pub fn label(&self, term: Term) -> &str {
        self.overrides
            .get(term.key())
            .map_or_else(|| term.default_label(), String::as_str)
    }

    /// Whether this deployment has renamed a term. Shown in the configuration panel so
    /// an administrator can see which words are theirs and which are the defaults.
    #[must_use]
    pub fn is_overridden(&self, term: Term) -> bool {
        self.overrides.contains_key(term.key())
    }

    /// How many terms this deployment has renamed.
    #[must_use]
    pub fn override_count(&self) -> usize {
        self.overrides.len()
    }

    /// Convenience for the common case.
    #[must_use]
    pub fn classification(&self, c: Classification) -> &str {
        self.label(Term::Classification(c))
    }

    #[must_use]
    pub fn track_status(&self, s: TrackStatus) -> &str {
        self.label(Term::TrackStatus(s))
    }

    #[must_use]
    pub fn layer(&self, l: EffectorLayer) -> &str {
        self.label(Term::Layer(l))
    }

    #[must_use]
    pub fn control_status(&self, s: WeaponsControlStatus) -> &str {
        self.label(Term::ControlStatus(s))
    }

    #[must_use]
    pub fn sensor_mode(&self, m: SensorMode) -> &str {
        self.label(Term::SensorMode(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gap's closing action, as an assertion: every term the interface can show has
    /// a label, and it is never empty.
    #[test]
    fn every_term_resolves_to_a_non_empty_label() {
        let vocabulary = Vocabulary::default();
        for term in Term::ALL {
            let label = vocabulary.label(*term);
            assert!(
                !label.trim().is_empty(),
                "{} has no default label",
                term.key()
            );
        }
    }

    /// Keys are unique and round-trip, because they are configuration schema: two terms
    /// sharing a key would make one of them unoverridable and the other's override
    /// silently apply to both.
    #[test]
    fn every_key_is_unique_and_resolves_back() {
        let mut keys: Vec<&str> = Term::ALL.iter().map(|t| t.key()).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two terms share a key");

        for term in Term::ALL {
            assert_eq!(Term::from_key(term.key()), Some(*term));
        }
        assert_eq!(Term::from_key("classification.suspect"), None);
    }

    /// No label is a Rust variant name where the doctrine term differs. This is what
    /// GAP-070 is actually for: the two cases the glossary calls out explicitly.
    #[test]
    fn the_doctrine_terms_are_not_the_variant_names() {
        let v = Vocabulary::default();
        assert_eq!(
            v.classification(Classification::Friendly),
            "Friend",
            "the NATO standard identity is Friend, not the variant name"
        );
        for (status, expected) in [
            (WeaponsControlStatus::Free, "Weapons free"),
            (WeaponsControlStatus::Tight, "Weapons tight"),
            (WeaponsControlStatus::Hold, "Weapons hold"),
        ] {
            assert_eq!(
                v.control_status(status),
                expected,
                "the glossary gives the joint term for {status:?}"
            );
        }
        assert_eq!(v.layer(EffectorLayer::SelfDefence), "Self-defence");
    }

    /// An override renames one term and leaves every other alone.
    #[test]
    fn an_override_renames_exactly_one_term() {
        let vocabulary = Vocabulary {
            overrides: BTreeMap::from([("classification.friendly".to_owned(), "Blue".to_owned())]),
        };
        assert_eq!(vocabulary.classification(Classification::Friendly), "Blue");
        assert!(vocabulary.is_overridden(Term::Classification(Classification::Friendly)));

        for term in Term::ALL {
            if *term == Term::Classification(Classification::Friendly) {
                continue;
            }
            assert_eq!(
                vocabulary.label(*term),
                term.default_label(),
                "{} changed when another term was overridden",
                term.key()
            );
            assert!(!vocabulary.is_overridden(*term));
        }
        assert_eq!(vocabulary.override_count(), 1);
    }

    /// The table round-trips through JSON, because the baseline is a file people edit.
    #[test]
    fn the_table_round_trips_through_json() {
        let vocabulary = Vocabulary {
            overrides: BTreeMap::from([
                ("classification.hostile".to_owned(), "Rouge".to_owned()),
                ("layer.point".to_owned(), "Defense rapprochee".to_owned()),
            ]),
        };
        let text = serde_json::to_string(&vocabulary).expect("serialize");
        let back: Vocabulary = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(vocabulary, back);
        assert_eq!(back.classification(Classification::Hostile), "Rouge");
    }

    /// Renaming is not remapping: overriding two terms to the same word does not make
    /// them the same value. The interface shows one word; every policy engine still
    /// sees two classifications.
    #[test]
    fn two_terms_may_share_a_word_without_becoming_one_value() {
        let vocabulary = Vocabulary {
            overrides: BTreeMap::from([
                (
                    "classification.neutral".to_owned(),
                    "Non-combatant".to_owned(),
                ),
                (
                    "classification.unknown".to_owned(),
                    "Non-combatant".to_owned(),
                ),
            ]),
        };
        assert_eq!(
            vocabulary.classification(Classification::Neutral),
            vocabulary.classification(Classification::Unknown)
        );
        assert_ne!(Classification::Neutral, Classification::Unknown);
    }
}
