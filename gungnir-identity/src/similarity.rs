// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cross-session correlation by similarity (GAP-019).
//!
//! When a session-local track id is new, the resolver asks whether it is an entity it
//! already knows: the last state it saw for each lineage, propagated to the candidate's
//! time at constant velocity, against the candidate's position under both covariances;
//! and whether the two classifications agree. The answer is a **confidence**, and a
//! merge made on it is recorded with its basis so a reviewer can see why two tracks
//! became one entity (`docs/design/DN-19-order-of-battle.md` reads these records).
//!
//! What it does not do: correlate across a gap longer than `max_gap_s`, because a
//! constant-velocity guess over ten minutes is a guess about a different craft; or
//! merge on class alone, because two hostiles are not one hostile.

use gungnir_model::{Classification, MissionTime, TrackView};

/// The last state a lineage was seen in.
#[derive(Debug, Clone, PartialEq)]
pub struct LastSeen {
    pub position_enu: [f64; 3],
    pub velocity_mps: [f64; 3],
    /// Position variance per axis, from the covariance diagonal.
    pub variance_m2: [f64; 3],
    pub classification: Classification,
    pub mission_time: MissionTime,
}

impl LastSeen {
    #[must_use]
    pub fn of(track: &TrackView) -> Self {
        Self {
            position_enu: track.position_enu(),
            velocity_mps: [track.state[3], track.state[4], track.state[5]],
            variance_m2: [
                track.covariance[(0, 0)],
                track.covariance[(1, 1)],
                track.covariance[(2, 2)],
            ],
            classification: track.classification,
            mission_time: track.mission_time,
        }
    }
}

/// How similar a candidate is to a lineage's last sighting, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Similarity {
    /// In `[0, 1]`: the kinematic likelihood scaled by the class agreement.
    pub confidence: f64,
    /// Normalised squared distance between the propagated position and the candidate.
    pub distance_sq: f64,
    pub gap_s: f64,
    pub class_agrees: bool,
}

/// The thresholds the resolver correlates under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrelationSettings {
    /// Confidence at or above which a candidate is taken to be the known entity.
    pub merge_threshold: f64,
    /// Longest gap over which a propagated position is still worth comparing.
    pub max_gap_s: f64,
    /// Added to each axis variance per second of gap, so an old sighting is wide.
    pub process_noise_m2_per_s: f64,
    /// Shortest gap over which a new session id can be a known entity. Two ids the
    /// tracker holds at the same instant are two objects by the tracker's own rule;
    /// correlation is for a sighting that ended before this one began.
    pub min_gap_s: f64,
}

impl Default for CorrelationSettings {
    fn default() -> Self {
        Self {
            merge_threshold: 0.5,
            max_gap_s: 300.0,
            process_noise_m2_per_s: 4.0,
            min_gap_s: 1.0,
        }
    }
}

/// Compare a candidate with a last sighting.
///
/// `None` when the candidate is earlier than the sighting, the gap is too long, or a
/// variance is not finite and positive: nothing is compared rather than compared badly.
#[must_use]
pub fn similarity(
    candidate: &TrackView,
    last: &LastSeen,
    settings: &CorrelationSettings,
) -> Option<Similarity> {
    let gap_s = candidate.mission_time.0 - last.mission_time.0;
    if !(settings.min_gap_s..=settings.max_gap_s).contains(&gap_s) {
        return None;
    }
    let p = candidate.position_enu();
    let mut distance_sq = 0.0;
    for (i, (&position, &velocity)) in last.position_enu.iter().zip(&last.velocity_mps).enumerate()
    {
        let predicted = position + velocity * gap_s;
        let variance = last.variance_m2[i]
            + candidate.covariance[(i, i)]
            + settings.process_noise_m2_per_s * gap_s;
        if !(variance.is_finite() && variance > 0.0) {
            return None;
        }
        let d = p[i] - predicted;
        distance_sq += d * d / variance;
    }
    if !distance_sq.is_finite() {
        return None;
    }
    let class_agrees = match (candidate.classification, last.classification) {
        (a, b) if a == b => true,
        // Unknown agrees with anything: it is the absence of a claim, not a rival one.
        (Classification::Unknown, _) | (_, Classification::Unknown) => true,
        _ => false,
    };
    let kinematic = (-0.5 * distance_sq).exp();
    let confidence = if class_agrees {
        kinematic
    } else {
        kinematic * 0.1
    };
    Some(Similarity {
        confidence,
        distance_sq,
        gap_s,
        class_agrees,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Provenance, Quality, TrackId, TrackStatus};

    fn track(id: u64, t: f64, e: f64, ve: f64, class: Classification) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::new(e, 0.0, 0.0, ve, 0.0, 0.0),
            covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 100.0,
            classification: class,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(t),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    #[test]
    fn a_track_where_the_last_one_was_heading_is_confidently_the_same() {
        let last = LastSeen::of(&track(1, 0.0, 0.0, 10.0, Classification::Hostile));
        let s = similarity(
            &track(2, 30.0, 300.0, 10.0, Classification::Hostile),
            &last,
            &CorrelationSettings::default(),
        )
        .expect("compared");
        assert!(s.confidence > 0.9, "{s:?}");
        assert!(s.class_agrees);
    }

    #[test]
    fn a_track_far_from_the_propagated_position_is_not() {
        let last = LastSeen::of(&track(1, 0.0, 0.0, 10.0, Classification::Hostile));
        let s = similarity(
            &track(2, 30.0, 3000.0, 10.0, Classification::Hostile),
            &last,
            &CorrelationSettings::default(),
        )
        .expect("compared");
        assert!(s.confidence < 0.01, "{s:?}");
    }

    #[test]
    fn a_disagreeing_class_cuts_the_confidence_and_unknown_does_not() {
        let last = LastSeen::of(&track(1, 0.0, 0.0, 10.0, Classification::Hostile));
        let settings = CorrelationSettings::default();
        let friendly = similarity(
            &track(2, 30.0, 300.0, 10.0, Classification::Friendly),
            &last,
            &settings,
        )
        .expect("compared");
        let unknown = similarity(
            &track(3, 30.0, 300.0, 10.0, Classification::Unknown),
            &last,
            &settings,
        )
        .expect("compared");
        assert!(!friendly.class_agrees && friendly.confidence < 0.2);
        assert!(unknown.class_agrees && unknown.confidence > 0.9);
    }

    #[test]
    fn two_ids_at_the_same_instant_are_two_objects() {
        let last = LastSeen::of(&track(1, 0.0, 0.0, 10.0, Classification::Unknown));
        assert!(similarity(
            &track(2, 0.0, 0.0, 10.0, Classification::Unknown),
            &last,
            &CorrelationSettings::default()
        )
        .is_none());
    }

    #[test]
    fn too_long_a_gap_or_the_past_is_not_compared() {
        let last = LastSeen::of(&track(1, 100.0, 0.0, 10.0, Classification::Unknown));
        let settings = CorrelationSettings::default();
        assert!(similarity(
            &track(2, 50.0, 0.0, 10.0, Classification::Unknown),
            &last,
            &settings
        )
        .is_none());
        assert!(similarity(
            &track(2, 1000.0, 9000.0, 10.0, Classification::Unknown),
            &last,
            &settings
        )
        .is_none());
    }
}
