// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Threat/risk assessment, per docs/gungnir-capabilities.md §5.4.
//! gungnir-intercept-service optimizes a reward matrix it is *given*; this crate is
//! the "why does this track matter more than that one" layer that derives those
//! rewards from kinematics, confidence, and asset exposure. Stale tracks always
//! score zero so they are never allocated against (§5.2).

pub mod assets;
pub mod prediction;

pub use assets::{AssetAnchor, AssetExposure, AssetListAssessor};
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
}

pub trait ThreatAssessor: Send + Sync {
    fn assess(&self, tracks: &[TrackView]) -> Vec<RiskScore>;
}

/// Kinematic baseline: risk grows as a track gets closer to one protected point and
/// approaches it. Confidence-aware in the minimal sense that stale tracks score zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClosingSpeedAssessor {
    /// ENU position of the asset being protected, meters.
    pub protected_point_enu: [f64; 3],
    /// Range at and beyond which risk is zero, meters.
    pub max_range_m: f64,
}

impl ClosingSpeedAssessor {
    // Scores and times are reported as f32 by design (`RiskScore`); the narrowing is
    // intentional and the values are bounded.
    #[allow(clippy::cast_possible_truncation)]
    fn score_one(&self, track: &TrackView) -> RiskScore {
        if track.quality.is_stale {
            return RiskScore {
                track_id: track.id,
                score: 0.0,
                time_to_impact_s: None,
                exposure: None,
            };
        }
        let p = track.position_enu();
        let rel = [
            p[0] - self.protected_point_enu[0],
            p[1] - self.protected_point_enu[1],
            p[2] - self.protected_point_enu[2],
        ];
        let range = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
        let v = [track.state[3], track.state[4], track.state[5]];
        // Positive when the track moves toward the protected point.
        let closing = if range > f64::EPSILON {
            -(v[0] * rel[0] + v[1] * rel[1] + v[2] * rel[2]) / range
        } else {
            0.0
        };
        let proximity = (1.0 - range / self.max_range_m).clamp(0.0, 1.0);
        let approaching = closing > 0.0;
        let score = if approaching {
            proximity
        } else {
            proximity * 0.5
        };
        let time_to_impact_s = approaching.then(|| (range / closing) as f32);
        RiskScore {
            track_id: track.id,
            score: score as f32,
            time_to_impact_s,
            // The single-point assessor has no asset list to expose.
            exposure: None,
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
        }
    }

    #[test]
    fn closer_approaching_track_scores_higher() {
        let scores =
            assessor().assess(&[track(1, 900.0, -10.0, false), track(2, 100.0, -10.0, false)]);
        assert!(scores[1].score > scores[0].score);
        assert!(scores[1].time_to_impact_s.unwrap() < scores[0].time_to_impact_s.unwrap());
    }

    #[test]
    fn receding_track_has_no_time_to_impact_and_half_score() {
        let scores =
            assessor().assess(&[track(1, 500.0, 10.0, false), track(2, 500.0, -10.0, false)]);
        assert!(scores[0].time_to_impact_s.is_none());
        assert!((scores[0].score - scores[1].score * 0.5).abs() < 1e-6);
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
