// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Scoring tracks against the defended-asset list.
//!
//! Design: docs/design/DN-01-defended-assets.md §5. Capability
//! CAP-3.1 and CAP-3.2; measures MOP-27 and MOP-28.
//!
//! **Frame refinement made during implementation.** The design showed
//! `AssetListAssessor` holding an `AssetListView` directly. Assets are geodetic and
//! tracks are local ENU metres, and the conversion lives in `gungnir-coord`, which
//! this crate does not depend on and may not add an edge to: the five edges the
//! engineering reviewer accepted on 2026-09-05 do not include one, and
//! `ARCHITECTURE.md` does not draw it. So the caller supplies each asset's ENU
//! centre through [`AssetAnchor`], exactly as `ClosingSpeedAssessor` already takes
//! `protected_point_enu`. Nothing else in the design changes.

use crate::kinematics::{kinematics, Geometry, KinematicFactor, DEFAULT_URGENCY_HALF_TIME_S};
use crate::prediction::closest_on_course;
use crate::RiskScore;
use gungnir_model::{AssetId, AssetListView, DefendedAsset, TrackView};
use nalgebra::Vector3;

/// One asset with its centre in the local ENU frame the tracks use.
///
/// The caller converts, because the conversion lives in `gungnir-coord` and the
/// scoring path may not depend on it.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetAnchor {
    pub asset: DefendedAsset,
    /// Centre of the asset's extent, `[e, n, u]` metres.
    pub center_enu: [f64; 3],
}

/// Which asset a track threatens, how far away it is, and how soon it arrives.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssetExposure {
    pub asset: AssetId,
    /// Range to the asset's boundary, metres; zero inside an area asset.
    pub range_m: f64,
    /// Seconds to reach the asset at the current closing speed, `None` if the
    /// track is not approaching it.
    pub time_to_impact_s: Option<f32>,
    /// Least range this track will ever reach on its current course, metres, to
    /// the asset's boundary (docs/design/DN-02-prediction-and-approach.md).
    ///
    /// Unbounded in time rather than clipped to a horizon: it answers "how close
    /// will it come", which is what a surface craft passing a port needs and what
    /// time to impact cannot express. `None` when the track is not moving.
    #[serde(default)]
    pub closest_approach_m: Option<f64>,
    /// Seconds until the closest approach on the current course, `None` when the
    /// track is not moving; zero when the closest point is already behind it (DN-03
    /// amendment 2: a pass-close warning is due by this, not merely within the lead
    /// time).
    #[serde(default)]
    pub time_to_closest_approach_s: Option<f64>,
}

/// Scores every track against the defended-asset list.
///
/// Supersedes `ClosingSpeedAssessor`, which is kept for the degenerate case of a
/// deployment that genuinely defends one point.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetListAssessor {
    anchors: Vec<AssetAnchor>,
    /// Baseline version the list came from, so a score traces to its list.
    baseline_version: u32,
    /// Range at and beyond which a track scores zero against any asset.
    max_range_m: f64,
    /// Platform-class lethality per track (GAP-027), from the host's evidence and the
    /// baseline's table; a track not listed weighs 1.0.
    class_weights: std::collections::HashMap<gungnir_model::TrackId, f64>,
    /// The time to impact at which urgency is half its maximum, seconds (GAP-124, D-83):
    /// the baseline's `assessment.urgency_half_time_s`.
    urgency_half_time_s: f64,
}

impl AssetListAssessor {
    /// Builds an assessor from anchored assets.
    ///
    /// An empty set is legal and is **not** an error: the assessor reports itself
    /// unconfigured and every score carries no exposure, so the interface can say
    /// so. A zero score and an unconfigured system look identical to an operator
    /// otherwise, which is what the honest-status rule exists to prevent.
    pub fn new(baseline_version: u32, anchors: Vec<AssetAnchor>, max_range_m: f64) -> Self {
        Self {
            anchors,
            baseline_version,
            max_range_m,
            class_weights: std::collections::HashMap::new(),
            urgency_half_time_s: DEFAULT_URGENCY_HALF_TIME_S,
        }
    }

    /// The deployment's urgency half-time (GAP-124, D-83), from the baseline's
    /// `assessment.urgency_half_time_s`. It sets how steeply the score falls with time to
    /// impact and never the order. A value that is not finite and positive -- which the
    /// baseline refuses -- is not substituted: the time term then gives no urgency, and
    /// every closing track scores on its closing alone.
    #[must_use]
    pub fn with_urgency_half_time_s(mut self, seconds: f64) -> Self {
        self.urgency_half_time_s = seconds;
        self
    }

    /// Platform-class lethality per track (GAP-027): the host knows the class (from a
    /// cooperative declaration, or a classifier) and the baseline's table; the score is
    /// monotonic in it the way it is in the affiliation.
    #[must_use]
    pub fn with_class_weights(
        mut self,
        weights: std::collections::HashMap<gungnir_model::TrackId, f64>,
    ) -> Self {
        self.class_weights = weights;
        self
    }

    /// True when no asset is configured. Callers must surface this.
    pub fn is_unconfigured(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn baseline_version(&self) -> u32 {
        self.baseline_version
    }

    /// Exposure of one track to one asset with the kinematic factor behind its score, or
    /// `None` beyond `max_range_m` or for a track whose state is not a finite number.
    fn expose(
        &self,
        track: &TrackView,
        anchor: &AssetAnchor,
    ) -> Option<(AssetExposure, KinematicFactor)> {
        let p = track.position_enu();
        let rel = [
            p[0] - anchor.center_enu[0],
            p[1] - anchor.center_enu[1],
            p[2] - anchor.center_enu[2],
        ];
        let v = [track.state[3], track.state[4], track.state[5]];
        // An area asset is reached at its boundary, not at its centre.
        let k = kinematics(&Geometry {
            relative_enu: rel,
            velocity_enu: v,
            covariance: &track.covariance,
            boundary_radius_m: anchor.asset.extent.radius_m(),
            max_range_m: self.max_range_m,
            urgency_half_time_s: self.urgency_half_time_s,
        })?;
        if k.centre_range_m > self.max_range_m {
            return None;
        }
        // Least range on the current course, ever: the predictor's own routine, unbounded
        // in time (DN-02; GAP-124 took this function's copy of it out).
        let (closest_approach_m, time_to_closest_approach_s) =
            match closest_on_course(Vector3::from(rel), Vector3::from(v), f64::INFINITY) {
                Some((t_star, centre_m)) => (
                    Some((centre_m - anchor.asset.extent.radius_m()).max(0.0)),
                    Some(t_star),
                ),
                None => (None, None),
            };
        Some((
            AssetExposure {
                asset: anchor.asset.id,
                range_m: k.range_m,
                time_to_impact_s: k.time_to_impact_s,
                closest_approach_m,
                time_to_closest_approach_s,
            },
            k.factor,
        ))
    }

    /// The kinematic factor (proximity, closing, time to impact; GAP-124) times priority
    /// weight times the track's lethality (GAP-027): what decides which asset a track is
    /// scored against, and the value MOP-28 requires to be monotonic in time to impact,
    /// asset priority, and class.
    fn weighted(&self, factor: &KinematicFactor, anchor: &AssetAnchor, track: &TrackView) -> f64 {
        let class = self.class_weights.get(&track.id).copied().unwrap_or(1.0);
        let weighted = factor.value
            * anchor.asset.priority.weight()
            * track.classification.lethality_weight()
            * class;
        if weighted.is_finite() {
            weighted
        } else {
            0.0
        }
    }

    fn score_one(&self, track: &TrackView) -> RiskScore {
        // Stale tracks always score zero so they are never allocated against.
        if track.quality.is_stale {
            return RiskScore {
                track_id: track.id,
                score: 0.0,
                time_to_impact_s: None,
                exposure: None,
                kinematics: None,
            };
        }
        let best = self
            .anchors
            .iter()
            .filter_map(|a| self.expose(track, a).map(|(e, k)| (e, k, a)))
            .max_by(|(_, ka, aa), (_, kb, ab)| {
                self.weighted(ka, aa, track)
                    .total_cmp(&self.weighted(kb, ab, track))
            });
        match best {
            #[allow(clippy::cast_possible_truncation)]
            Some((exposure, factor, anchor)) => RiskScore {
                track_id: track.id,
                score: self.weighted(&factor, anchor, track) as f32,
                time_to_impact_s: exposure.time_to_impact_s,
                exposure: Some(exposure),
                kinematics: Some(factor),
            },
            // Either no asset is configured, or none is within range. Both are
            // reported as "no exposure" rather than as a computed zero.
            None => RiskScore {
                track_id: track.id,
                score: 0.0,
                time_to_impact_s: None,
                exposure: None,
                kinematics: None,
            },
        }
    }
}

impl crate::ThreatAssessor for AssetListAssessor {
    fn assess(&self, tracks: &[TrackView]) -> Vec<RiskScore> {
        tracks.iter().map(|t| self.score_one(t)).collect()
    }
}

/// Anchors an asset list at a local origin the caller has already chosen.
///
/// A convenience for the binaries, which hold both the baseline and the frame.
/// `to_enu` converts a geodetic position into the tracks' ENU frame.
pub fn anchor_list(
    list: &AssetListView,
    mut to_enu: impl FnMut(gungnir_model::Geodetic) -> [f64; 3],
) -> Vec<AssetAnchor> {
    list.assets
        .iter()
        .map(|asset| AssetAnchor {
            center_enu: to_enu(asset.extent.center()),
            asset: asset.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThreatAssessor;
    use gungnir_model::{
        AssetExtent, AssetPriority, Classification, Geodetic, Provenance, Quality, TrackId,
        TrackStatus,
    };
    use nalgebra::{SMatrix, SVector};

    fn anchor(id: u32, priority: AssetPriority, center_enu: [f64; 3]) -> AssetAnchor {
        AssetAnchor {
            asset: DefendedAsset {
                id: AssetId(id),
                name: format!("asset-{id}"),
                extent: AssetExtent::Point {
                    position: Geodetic {
                        lat_rad: 0.0,
                        lon_rad: 0.0,
                        alt_m: 0.0,
                    },
                },
                priority,
                warning: None,
                note: None,
            },
            center_enu,
        }
    }

    fn track(id: u64, position: [f64; 3], velocity: [f64; 3], stale: bool) -> TrackView {
        let mut kinematics = SVector::<f64, 6>::zeros();
        kinematics[0] = position[0];
        kinematics[1] = position[1];
        kinematics[2] = position[2];
        kinematics[3] = velocity[0];
        kinematics[4] = velocity[1];
        kinematics[5] = velocity[2];
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: kinematics,
            covariance: SMatrix::<f64, 6, 6>::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality {
                is_stale: stale,
                ..Quality::default()
            },
            mission_time: gungnir_model::MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    #[test]
    fn an_unconfigured_list_yields_no_exposure_rather_than_a_zero_score() {
        let a = AssetListAssessor::new(1, Vec::new(), 10_000.0);
        assert!(a.is_unconfigured());
        let scores = a.assess(&[track(1, [100.0, 0.0, 0.0], [-10.0, 0.0, 0.0], false)]);
        assert_eq!(scores.len(), 1);
        assert!(scores[0].exposure.is_none(), "no asset, so no exposure");
        assert!(scores[0].score.abs() < f32::EPSILON);
    }

    #[test]
    fn a_stale_track_scores_zero_and_is_never_exposed() {
        let a = AssetListAssessor::new(
            1,
            vec![anchor(1, AssetPriority::Critical, [0.0, 0.0, 0.0])],
            10_000.0,
        );
        let scores = a.assess(&[track(1, [100.0, 0.0, 0.0], [-50.0, 0.0, 0.0], true)]);
        assert!(scores[0].score.abs() < f32::EPSILON);
        assert!(scores[0].exposure.is_none());
    }

    /// MOP-28: monotonic in class lethality (GAP-027). A friendly track scores nothing,
    /// so it is never allocated against.
    #[test]
    #[allow(clippy::float_cmp)]
    fn the_score_rises_with_lethality_and_a_friendly_track_scores_zero() {
        let a = AssetListAssessor::new(1, vec![anchor(1, AssetPriority::High, [0.0; 3])], 10_000.0);
        let score_for = |class: gungnir_model::Classification| {
            let mut t = track(1, [2_000.0, 0.0, 0.0], [-50.0, 0.0, 0.0], false);
            t.classification = class;
            a.assess(&[t])[0].score
        };
        let hostile = score_for(gungnir_model::Classification::Hostile);
        let unknown = score_for(gungnir_model::Classification::Unknown);
        let neutral = score_for(gungnir_model::Classification::Neutral);
        let friendly = score_for(gungnir_model::Classification::Friendly);
        assert!(
            hostile > unknown && unknown > neutral && neutral > friendly,
            "{hostile} {unknown} {neutral} {friendly}"
        );
        assert_eq!(friendly, 0.0);
    }

    #[test]
    fn the_score_rises_with_asset_priority() {
        // MOP-28: monotonic in asset priority.
        let position = [1_000.0, 0.0, 0.0];
        let velocity = [-100.0, 0.0, 0.0];
        let mut previous = -1.0_f32;
        for priority in [
            AssetPriority::Low,
            AssetPriority::Medium,
            AssetPriority::High,
            AssetPriority::Critical,
        ] {
            let a = AssetListAssessor::new(1, vec![anchor(1, priority, [0.0; 3])], 10_000.0);
            let score = a.assess(&[track(1, position, velocity, false)])[0].score;
            assert!(
                score > previous,
                "score must rise with priority: {score} after {previous}"
            );
            previous = score;
        }
    }

    #[test]
    fn the_score_rises_as_time_to_impact_falls() {
        // MOP-28: monotonic in time to impact.
        let a = AssetListAssessor::new(1, vec![anchor(1, AssetPriority::High, [0.0; 3])], 10_000.0);
        let far = a.assess(&[track(1, [5_000.0, 0.0, 0.0], [-100.0, 0.0, 0.0], false)])[0];
        let near = a.assess(&[track(1, [500.0, 0.0, 0.0], [-100.0, 0.0, 0.0], false)])[0];
        assert!(near.score > far.score);
        let (Some(near_t), Some(far_t)) = (near.time_to_impact_s, far.time_to_impact_s) else {
            panic!("both tracks are approaching, so both have a time to impact");
        };
        assert!(near_t < far_t);
    }

    #[test]
    fn the_reported_asset_is_the_one_the_track_actually_threatens() {
        let a = AssetListAssessor::new(
            1,
            vec![
                anchor(1, AssetPriority::Low, [0.0, 0.0, 0.0]),
                anchor(2, AssetPriority::Critical, [800.0, 0.0, 0.0]),
            ],
            10_000.0,
        );
        let scores = a.assess(&[track(1, [1_000.0, 0.0, 0.0], [-100.0, 0.0, 0.0], false)]);
        let exposure = scores[0]
            .exposure
            .expect("a track in range has an exposure");
        assert_eq!(
            exposure.asset,
            AssetId(2),
            "the near critical asset outweighs the far low one"
        );
    }

    #[test]
    fn an_area_asset_is_reached_at_its_boundary() {
        let mut area = anchor(1, AssetPriority::High, [0.0; 3]);
        area.asset.extent = AssetExtent::Circle {
            center: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            radius_m: 400.0,
        };
        let a = AssetListAssessor::new(1, vec![area], 10_000.0);
        let scores = a.assess(&[track(1, [500.0, 0.0, 0.0], [-100.0, 0.0, 0.0], false)]);
        let exposure = scores[0].exposure.expect("in range");
        assert!(
            (exposure.range_m - 100.0).abs() < 1e-9,
            "500 m from the centre of a 400 m asset is 100 m from its edge"
        );
    }

    #[test]
    fn a_track_beyond_max_range_has_no_exposure() {
        let a = AssetListAssessor::new(
            1,
            vec![anchor(1, AssetPriority::Critical, [0.0; 3])],
            1_000.0,
        );
        let scores = a.assess(&[track(1, [5_000.0, 0.0, 0.0], [-100.0, 0.0, 0.0], false)]);
        assert!(scores[0].exposure.is_none());
        assert!(scores[0].score.abs() < f32::EPSILON);
    }

    #[test]
    fn a_receding_track_scores_below_an_approaching_one() {
        let a = AssetListAssessor::new(1, vec![anchor(1, AssetPriority::High, [0.0; 3])], 10_000.0);
        let approaching = a.assess(&[track(1, [1_000.0, 0.0, 0.0], [-100.0, 0.0, 0.0], false)])[0];
        let receding = a.assess(&[track(2, [1_000.0, 0.0, 0.0], [100.0, 0.0, 0.0], false)])[0];
        assert!(approaching.score > receding.score);
        assert!(receding.time_to_impact_s.is_none());
    }

    #[test]
    fn anchoring_preserves_the_list_order_and_ids() {
        let list = AssetListView {
            baseline_version: 4,
            assets: vec![
                anchor(7, AssetPriority::Low, [0.0; 3]).asset,
                anchor(9, AssetPriority::High, [0.0; 3]).asset,
            ],
        };
        let anchors = anchor_list(&list, |_| [1.0, 2.0, 3.0]);
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].asset.id, AssetId(7));
        assert_eq!(anchors[1].asset.id, AssetId(9));
        assert!(anchors[0]
            .center_enu
            .iter()
            .zip([1.0, 2.0, 3.0])
            .all(|(a, b)| (a - b).abs() < f64::EPSILON));
    }

    #[test]
    fn a_passing_track_reports_how_close_it_will_come() {
        // Flying east along y = 400, past an asset at the origin: it never
        // arrives, so it has no time to impact, but it does pass at 400 m.
        let a = AssetListAssessor::new(1, vec![anchor(1, AssetPriority::High, [0.0; 3])], 10_000.0);
        let scores = a.assess(&[track(1, [-1_000.0, 400.0, 0.0], [100.0, 0.0, 0.0], false)]);
        let exposure = scores[0].exposure.expect("in range");
        let closest = exposure
            .closest_approach_m
            .expect("a moving track has a closest approach");
        assert!(
            (closest - 400.0).abs() < 1e-6,
            "it passes at 400 m: {closest}"
        );
    }

    #[test]
    fn a_stationary_track_has_no_closest_approach() {
        let a = AssetListAssessor::new(1, vec![anchor(1, AssetPriority::High, [0.0; 3])], 10_000.0);
        let scores = a.assess(&[track(1, [500.0, 0.0, 0.0], [0.0; 3], false)]);
        let exposure = scores[0].exposure.expect("in range");
        assert!(exposure.closest_approach_m.is_none());
    }
}
