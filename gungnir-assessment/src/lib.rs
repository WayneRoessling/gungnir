// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Threat/risk assessment, per docs/gungnir-capabilities.md §5.4.
//! gungnir-intercept-service optimizes a reward matrix it is *given*; this crate is
//! the "why does this track matter more than that one" layer that derives those
//! rewards from kinematics, confidence, and asset exposure. Stale tracks always
//! score zero so they are never allocated against (§5.2).

pub mod assets;
pub mod kinematics;
pub mod prediction;

pub use assets::{AssetAnchor, AssetExposure, AssetListAssessor};
pub use kinematics::{KinematicFactor, CLOSING_SIGNIFICANCE_SIGMA, DEFAULT_URGENCY_HALF_TIME_S};
pub use prediction::{
    ClosestApproach, ConstantVelocityPredictor, FilterPredictor, PredictedPoint, Prediction,
    PredictorKind, TrajectoryPredictor,
};

use gungnir_model::{TrackId, TrackView};
use nalgebra::DMatrix;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RiskScore {
    pub track_id: TrackId,
    /// 0.0 (no risk) .. 1.0 (maximum).
    pub score: f32,
    /// Seconds until the track reaches the protected point or asset at its current
    /// closing speed; `None` if it is not approaching.
    pub time_to_impact_s: Option<f32>,
    /// Which asset produced this score (docs/design/DN-01-defended-assets.md).
    ///
    /// `None` means the score rests on no asset: either none is configured, or
    /// none is in range. Callers must report that state rather than presenting a
    /// zero score as a computed one.
    #[serde(default)]
    pub exposure: Option<AssetExposure>,
    /// The kinematic terms the score rests on -- proximity, closing confidence, urgency
    /// from time to impact, and the factor they make (GAP-124, D-83) -- so the evidence
    /// card can show what was used rather than recompute it. `None` exactly when no
    /// kinematics were scored: a stale track, or no exposure.
    #[serde(default)]
    pub kinematics: Option<KinematicFactor>,
}

pub trait ThreatAssessor: Send + Sync {
    fn assess(&self, tracks: &[TrackView]) -> Vec<RiskScore>;
}

/// Kinematic baseline: risk grows as a track gets closer to one protected point and
/// as its time to impact falls, by the same kinematic factor the asset-list assessor uses
/// (`kinematics`, GAP-124). Stale tracks score zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClosingSpeedAssessor {
    /// ENU position of the asset being protected, meters.
    pub protected_point_enu: [f64; 3],
    /// Range at and beyond which risk is zero, meters.
    pub max_range_m: f64,
    /// Time to impact at which urgency is half its maximum, seconds
    /// ([`DEFAULT_URGENCY_HALF_TIME_S`] unless the deployment says otherwise).
    pub urgency_half_time_s: f64,
}

impl ClosingSpeedAssessor {
    // Scores are reported as f32 by design (`RiskScore`); the narrowing is intentional
    // and the values are bounded.
    #[allow(clippy::cast_possible_truncation)]
    fn score_one(&self, track: &TrackView) -> RiskScore {
        let unscored = RiskScore {
            track_id: track.id,
            score: 0.0,
            time_to_impact_s: None,
            exposure: None,
            kinematics: None,
        };
        if track.quality.is_stale {
            return unscored;
        }
        let p = track.position_enu();
        let Some(k) = kinematics::kinematics(&kinematics::Geometry {
            relative_enu: [
                p[0] - self.protected_point_enu[0],
                p[1] - self.protected_point_enu[1],
                p[2] - self.protected_point_enu[2],
            ],
            velocity_enu: [track.state[3], track.state[4], track.state[5]],
            covariance: &track.covariance,
            boundary_radius_m: 0.0,
            max_range_m: self.max_range_m,
            urgency_half_time_s: self.urgency_half_time_s,
        }) else {
            // A state that is not a number scores nothing rather than a NaN that would
            // sort to the top of every ranking.
            return unscored;
        };
        RiskScore {
            track_id: track.id,
            score: k.factor.value as f32,
            time_to_impact_s: k.time_to_impact_s,
            // The single-point assessor has no asset list to expose.
            exposure: None,
            kinematics: Some(k.factor),
        }
    }
}

impl ThreatAssessor for ClosingSpeedAssessor {
    fn assess(&self, tracks: &[TrackView]) -> Vec<RiskScore> {
        tracks.iter().map(|t| self.score_one(t)).collect()
    }
}

/// The reward matrix `gungnir-intercept-service` solves over: one row per ready
/// resource, one column per track, each entry the track's risk score (every
/// resource values a track equally until resource-specific reachability exists).
pub fn reward_matrix(scores: &[RiskScore], ready_resources: usize) -> DMatrix<f64> {
    DMatrix::from_fn(ready_resources, scores.len(), |_, col| {
        f64::from(scores[col].score)
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, MissionTime, Provenance, Quality, TrackStatus};
    use nalgebra::{SMatrix, SVector};

    fn track(id: u64, e: f64, ve: f64, stale: bool) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: SVector::<f64, 6>::new(e, 0.0, 0.0, ve, 0.0, 0.0),
            covariance: SMatrix::<f64, 6, 6>::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality {
                is_stale: stale,
                ..Quality::default()
            },
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn assessor() -> ClosingSpeedAssessor {
        ClosingSpeedAssessor {
            protected_point_enu: [0.0; 3],
            max_range_m: 1000.0,
            urgency_half_time_s: DEFAULT_URGENCY_HALF_TIME_S,
        }
    }

    #[test]
    fn closer_approaching_track_scores_higher() {
        let scores =
            assessor().assess(&[track(1, 900.0, -10.0, false), track(2, 100.0, -10.0, false)]);
        assert!(scores[1].score > scores[0].score);
        assert!(scores[1].time_to_impact_s.unwrap() < scores[0].time_to_impact_s.unwrap());
    }

    /// A receding track has no time to impact and scores half its proximity; the same
    /// track closing scores above half, however slowly it arrives (GAP-124).
    #[test]
    fn receding_track_has_no_time_to_impact_and_half_its_proximity() {
        let scores =
            assessor().assess(&[track(1, 500.0, 10.0, false), track(2, 500.0, -10.0, false)]);
        assert!(scores[0].time_to_impact_s.is_none());
        assert!(
            (scores[0].score - 0.5 * 0.5).abs() < 1e-6,
            "{}",
            scores[0].score
        );
        assert!(scores[1].score > 0.5);
        let k = scores[0].kinematics.expect("scored");
        assert!(k.urgency.abs() < f64::EPSILON && k.closing_confidence.abs() < f64::EPSILON);
    }

    #[test]
    fn stale_tracks_score_zero() {
        let scores = assessor().assess(&[track(1, 10.0, -10.0, true)]);
        assert_eq!(scores[0].score, 0.0);
        assert!(scores[0].time_to_impact_s.is_none());
    }

    #[test]
    fn reward_matrix_has_one_row_per_resource() {
        let scores =
            assessor().assess(&[track(1, 100.0, -10.0, false), track(2, 200.0, -10.0, false)]);
        let m = reward_matrix(&scores, 3);
        assert_eq!(m.shape(), (3, 2));
        assert_eq!(m[(2, 1)], f64::from(scores[1].score));
    }
}
