//! Trajectory prediction and closest point of approach.
//!
//! Design: docs/design/DN-02-prediction-and-approach.md. Capability CAP-2.8;
//! measure MOP-28 with DN-01.
//!
//! Two predictors, and the distinction between them is the honest-status question.
//! [`ConstantVelocityPredictor`] propagates the state vector and grows the
//! covariance from the track's own 6x6; it needs nothing from the tracking core and
//! works today. A filter predictor uses the pipeline's own motion model, so a
//! turning track predicts as a turn rather than as a tangent, and it arrives with
//! GAP-011. **The predictor in use is reported on every prediction**, because a
//! constant-velocity prediction of a manoeuvring drone is wrong in a way the
//! operator can compensate for only if they know that is what they are looking at.

use crate::AssetAnchor;
use gungnir_model::{AssetId, TrackId, TrackView};
use nalgebra::{Matrix3, SMatrix, Vector3};

/// Which predictor produced a prediction. Reported so the panel can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PredictorKind {
    /// Straight-line propagation of the state vector.
    ConstantVelocity,
    /// The tracking pipeline's own motion model (GAP-011).
    Filter,
}

/// A predicted position with the uncertainty that goes with it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PredictedPoint {
    pub time_ahead_s: f64,
    pub position_enu: [f64; 3],
    /// One-sigma along the direction of travel, metres.
    ///
    /// Non-finite when the track's covariance is not positive semi-definite, which
    /// the tracking core's own invariant forbids but which this crate must survive.
    /// The panel draws no ellipse rather than a nonsense one.
    pub sigma_along_m: f64,
    /// One-sigma across the direction of travel, metres; the larger of the two
    /// perpendicular directions.
    pub sigma_cross_m: f64,
}

impl PredictedPoint {
    /// True when both sigmas are finite and can be drawn.
    pub fn uncertainty_is_drawable(&self) -> bool {
        self.sigma_along_m.is_finite() && self.sigma_cross_m.is_finite()
    }
}

/// Closest point of approach to one asset.
///
/// Distinct from time to impact: a craft that will pass a port at 400 m never
/// "impacts" it and still matters (MT-04).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClosestApproach {
    pub asset: AssetId,
    /// Seconds from now at which the range is least, within the horizon.
    pub time_ahead_s: f64,
    /// Range to the asset's boundary at that moment, metres; zero if it reaches it.
    pub distance_m: f64,
}

/// What one track is predicted to do.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Prediction {
    pub track: TrackId,
    /// Which predictor produced this. Never omitted.
    pub predictor: PredictorKind,
    /// One entry per requested horizon, in the order requested. Empty when the
    /// track is stale: a track nobody has observed for thirty seconds must not
    /// produce a confident line to a place nothing is.
    pub points: Vec<PredictedPoint>,
    pub approaches: Vec<ClosestApproach>,
}

impl Prediction {
    /// A prediction with no points: what a stale track yields.
    pub fn none_for(track: TrackId, predictor: PredictorKind) -> Self {
        Self {
            track,
            predictor,
            points: Vec::new(),
            approaches: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.approaches.is_empty()
    }
}

/// Predicts tracks forward to the horizons the caller asks for.
pub trait TrajectoryPredictor: Send + Sync {
    fn predict(
        &self,
        tracks: &[TrackView],
        horizons_s: &[f64],
        assets: &[AssetAnchor],
    ) -> Vec<Prediction>;
}

/// Straight-line propagation with linear covariance growth.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ConstantVelocityPredictor;

impl ConstantVelocityPredictor {
    /// Position covariance at `t` under constant velocity:
    /// `P_pp + t (P_pv + P_vp) + t^2 P_vv`, the position block of `F P F^T`.
    pub(crate) fn position_covariance(covariance: &SMatrix<f64, 6, 6>, t: f64) -> Matrix3<f64> {
        let pp = covariance.fixed_view::<3, 3>(0, 0).into_owned();
        let pv = covariance.fixed_view::<3, 3>(0, 3).into_owned();
        let vp = covariance.fixed_view::<3, 3>(3, 0).into_owned();
        let vv = covariance.fixed_view::<3, 3>(3, 3).into_owned();
        pp + (pv + vp) * t + vv * (t * t)
    }

    /// One-sigma along `direction`, which must be a unit vector.
    ///
    /// A negative quadratic form means the covariance is not positive
    /// semi-definite upstream; `sqrt` yields `NaN`, which is exactly what the
    /// caller must see (the same choice `TrackView::position_sigma` already makes).
    pub(crate) fn sigma_along(cov: &Matrix3<f64>, direction: &Vector3<f64>) -> f64 {
        (direction.transpose() * cov * direction)[(0, 0)].sqrt()
    }

    /// An orthonormal pair perpendicular to `along`.
    pub(crate) fn perpendicular_basis(along: &Vector3<f64>) -> (Vector3<f64>, Vector3<f64>) {
        // Pick the world axis least aligned with `along`, so the cross product is
        // well conditioned.
        let seed = if along.x.abs() <= along.y.abs() && along.x.abs() <= along.z.abs() {
            Vector3::new(1.0, 0.0, 0.0)
        } else if along.y.abs() <= along.z.abs() {
            Vector3::new(0.0, 1.0, 0.0)
        } else {
            Vector3::new(0.0, 0.0, 1.0)
        };
        let first = along.cross(&seed).normalize();
        let second = along.cross(&first).normalize();
        (first, second)
    }

    fn point_at(track: &TrackView, t: f64) -> PredictedPoint {
        let p = Vector3::new(track.state[0], track.state[1], track.state[2]);
        let v = Vector3::new(track.state[3], track.state[4], track.state[5]);
        let at = p + v * t;
        let cov = Self::position_covariance(&track.covariance, t);
        let speed = v.norm();
        let (sigma_along_m, sigma_cross_m) = if speed > f64::EPSILON {
            let along = v / speed;
            let (a, b) = Self::perpendicular_basis(&along);
            let cross = Self::sigma_along(&cov, &a).max(Self::sigma_along(&cov, &b));
            (Self::sigma_along(&cov, &along), cross)
        } else {
            // No direction of travel, so no along-track axis exists. Report the
            // largest axis-aligned sigma for both rather than inventing a heading.
            let widest = cov[(0, 0)].max(cov[(1, 1)]).max(cov[(2, 2)]).sqrt();
            (widest, widest)
        };
        PredictedPoint {
            time_ahead_s: t,
            position_enu: [at.x, at.y, at.z],
            sigma_along_m,
            sigma_cross_m,
        }
    }

    /// Closest approach of a straight-line track to one asset, within `horizon_s`.
    ///
    /// `rel(t) = (p - c) + v t` is minimized at `t* = -dot(rel0, v) / |v|^2`,
    /// clamped into `[0, horizon]`. The distance is to the asset's boundary.
    pub(crate) fn approach(
        track: &TrackView,
        anchor: &AssetAnchor,
        horizon_s: f64,
    ) -> Option<ClosestApproach> {
        let p = Vector3::new(track.state[0], track.state[1], track.state[2]);
        let v = Vector3::new(track.state[3], track.state[4], track.state[5]);
        let c = Vector3::new(
            anchor.center_enu[0],
            anchor.center_enu[1],
            anchor.center_enu[2],
        );
        let rel0 = p - c;
        let speed_sq = v.norm_squared();
        let t_star = if speed_sq > f64::EPSILON {
            (-rel0.dot(&v) / speed_sq).clamp(0.0, horizon_s)
        } else {
            0.0
        };
        let at = rel0 + v * t_star;
        let distance_m = (at.norm() - anchor.asset.extent.radius_m()).max(0.0);
        if !distance_m.is_finite() {
            return None;
        }
        Some(ClosestApproach {
            asset: anchor.asset.id,
            time_ahead_s: t_star,
            distance_m,
        })
    }
}

/// Propagation under the motion model the tracking pipeline actually runs (GAP-020).
///
/// The **state** goes forward exactly as [`ConstantVelocityPredictor`] takes it, because
/// the pipeline's model is constant velocity and a prediction that disagreed with the
/// filter about where a track is going would be a second opinion rather than a
/// prediction. What differs is the **uncertainty**: this adds the process noise
/// `Q(t)` the filter adds on every predict, so the ellipse grows the way the tracker
/// says it grows rather than the way straight-line propagation of a covariance would.
///
/// That difference is the whole point of the row. A constant-velocity predictor reports
/// an uncertainty that is too small at long horizons -- it credits the filter's current
/// covariance and nothing for the manoeuvring the process noise exists to allow -- and an
/// operator reading a confident ellipse around a two-minute prediction is being told
/// something the tracker never claimed.
///
/// The spectral density is the deployment's, passed in rather than assumed, because it
/// is `PipelineSettings::process_noise_psd` and a host that ran the tracker with one
/// value and predicted with another would be back to two opinions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterPredictor {
    /// Process-noise spectral density of the constant-velocity model, (m/s²)²/Hz.
    pub process_noise_psd: f64,
}

impl FilterPredictor {
    #[must_use]
    pub fn new(process_noise_psd: f64) -> Self {
        Self { process_noise_psd }
    }

    /// The position block of `F P Fᵀ + Q(t)`.
    fn position_covariance(self, covariance: &SMatrix<f64, 6, 6>, t: f64) -> Matrix3<f64> {
        // `MotionModel` in scope, because `q` is one of its methods.
        use gungnir_model::MotionModel as _;
        let propagated = ConstantVelocityPredictor::position_covariance(covariance, t);
        let q = gungnir_model::ConstantVelocity {
            sigma_a_sq: self.process_noise_psd,
        }
        .q(t);
        propagated + q.fixed_view::<3, 3>(0, 0)
    }

    fn point_at(self, track: &TrackView, t: f64) -> PredictedPoint {
        let p = Vector3::new(track.state[0], track.state[1], track.state[2]);
        let v = Vector3::new(track.state[3], track.state[4], track.state[5]);
        let at = p + v * t;
        let cov = self.position_covariance(&track.covariance, t);
        let speed = v.norm();
        let (sigma_along_m, sigma_cross_m) = if speed > f64::EPSILON {
            let along = v / speed;
            let (a, b) = ConstantVelocityPredictor::perpendicular_basis(&along);
            let cross = ConstantVelocityPredictor::sigma_along(&cov, &a)
                .max(ConstantVelocityPredictor::sigma_along(&cov, &b));
            (ConstantVelocityPredictor::sigma_along(&cov, &along), cross)
        } else {
            let widest = cov[(0, 0)].max(cov[(1, 1)]).max(cov[(2, 2)]).sqrt();
            (widest, widest)
        };
        PredictedPoint {
            time_ahead_s: t,
            position_enu: [at.x, at.y, at.z],
            sigma_along_m,
            sigma_cross_m,
        }
    }
}

impl TrajectoryPredictor for FilterPredictor {
    fn predict(
        &self,
        tracks: &[TrackView],
        horizons_s: &[f64],
        assets: &[AssetAnchor],
    ) -> Vec<Prediction> {
        let horizon = horizons_s.iter().copied().fold(0.0_f64, f64::max);
        tracks
            .iter()
            .map(|track| {
                if track.quality.is_stale {
                    return Prediction::none_for(track.id, PredictorKind::Filter);
                }
                Prediction {
                    track: track.id,
                    predictor: PredictorKind::Filter,
                    points: horizons_s
                        .iter()
                        .map(|t| self.point_at(track, *t))
                        .collect(),
                    approaches: assets
                        .iter()
                        .filter_map(|a| ConstantVelocityPredictor::approach(track, a, horizon))
                        .collect(),
                }
            })
            .collect()
    }
}

impl TrajectoryPredictor for ConstantVelocityPredictor {
    fn predict(
        &self,
        tracks: &[TrackView],
        horizons_s: &[f64],
        assets: &[AssetAnchor],
    ) -> Vec<Prediction> {
        let horizon = horizons_s.iter().copied().fold(0.0_f64, f64::max);
        tracks
            .iter()
            .map(|track| {
                // A stale track is not predicted at all.
                if track.quality.is_stale {
                    return Prediction::none_for(track.id, PredictorKind::ConstantVelocity);
                }
                Prediction {
                    track: track.id,
                    predictor: PredictorKind::ConstantVelocity,
                    points: horizons_s
                        .iter()
                        .copied()
                        // A horizon that is not a real, non-negative number is not
                        // clamped and presented as if it were computed.
                        .filter(|t| t.is_finite() && *t >= 0.0)
                        .map(|t| Self::point_at(track, t))
                        .collect(),
                    approaches: assets
                        .iter()
                        .filter_map(|a| Self::approach(track, a, horizon))
                        .collect(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod filter_predictor_tests {
    use super::*;
    use gungnir_model::{Classification, Provenance, Quality, Releasability, TrackStatus};

    fn track() -> TrackView {
        TrackView {
            id: gungnir_model::TrackId(1),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::new(0.0, 0.0, 1_000.0, 100.0, 0.0, 0.0),
            covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 100.0,
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: gungnir_model::MissionTime(0.0),
            releasability: Releasability::default(),
        }
    }

    /// The claim the row rests on: the filter predictor's uncertainty is the tracker's,
    /// which includes the process noise, so it is **larger** than straight-line
    /// propagation of the covariance alone. A predictor that reported the smaller number
    /// would be drawing a confidence the tracker never had.
    #[test]
    fn the_filter_predictor_carries_the_process_noise_the_tracker_adds() {
        let horizons = [10.0, 120.0];
        let cv = ConstantVelocityPredictor.predict(&[track()], &horizons, &[]);
        let filtered = FilterPredictor::new(4.0).predict(&[track()], &horizons, &[]);

        assert_eq!(cv[0].predictor, PredictorKind::ConstantVelocity);
        assert_eq!(filtered[0].predictor, PredictorKind::Filter);
        for (a, b) in cv[0].points.iter().zip(&filtered[0].points) {
            for axis in 0..3 {
                assert!(
                    (a.position_enu[axis] - b.position_enu[axis]).abs() < 1e-12,
                    "the same motion model must place the point identically"
                );
            }
            assert!(
                b.sigma_along_m > a.sigma_along_m,
                "process noise must widen the ellipse at {} s: {} vs {}",
                a.time_ahead_s,
                b.sigma_along_m,
                a.sigma_along_m
            );
        }
        // And it grows with the horizon, which is what makes a long prediction honest.
        assert!(filtered[0].points[1].sigma_along_m > filtered[0].points[0].sigma_along_m);
    }

    /// Zero process noise is the degenerate case, and it must agree with straight-line
    /// propagation exactly rather than approximately.
    #[test]
    fn with_no_process_noise_the_two_predictors_agree() {
        let horizons = [30.0];
        let cv = ConstantVelocityPredictor.predict(&[track()], &horizons, &[]);
        let filtered = FilterPredictor::new(0.0).predict(&[track()], &horizons, &[]);
        let a = cv[0].points[0].sigma_along_m;
        let b = filtered[0].points[0].sigma_along_m;
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{
        AssetExtent, AssetPriority, Classification, DefendedAsset, Geodetic, MissionTime,
        Provenance, Quality, TrackStatus,
    };
    use nalgebra::SVector;

    fn anchor(id: u32, center_enu: [f64; 3], radius_m: f64) -> AssetAnchor {
        let center = Geodetic {
            lat_rad: 0.0,
            lon_rad: 0.0,
            alt_m: 0.0,
        };
        AssetAnchor {
            asset: DefendedAsset {
                id: AssetId(id),
                name: format!("asset-{id}"),
                extent: if radius_m > 0.0 {
                    AssetExtent::Circle { center, radius_m }
                } else {
                    AssetExtent::Point { position: center }
                },
                priority: AssetPriority::High,
                warning: None,
                note: None,
            },
            center_enu,
        }
    }

    fn track(id: u64, position: [f64; 3], velocity: [f64; 3], stale: bool) -> TrackView {
        let mut kinematics = SVector::<f64, 6>::zeros();
        for (i, value) in position.iter().chain(velocity.iter()).enumerate() {
            kinematics[i] = *value;
        }
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
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    #[test]
    fn a_straight_track_predicts_to_the_analytic_position() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [0.0, 0.0, 100.0], [50.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[0.0, 10.0, 60.0], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].predictor, PredictorKind::ConstantVelocity);
        assert_eq!(out[0].points.len(), 3);
        assert!((out[0].points[1].position_enu[0] - 500.0).abs() < 1e-9);
        assert!((out[0].points[2].position_enu[0] - 3_000.0).abs() < 1e-9);
        // Altitude is unchanged by a level track.
        assert!((out[0].points[2].position_enu[2] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn uncertainty_grows_with_the_horizon() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [0.0; 3], [50.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[0.0, 30.0], &[]);
        let now = out[0].points[0];
        let later = out[0].points[1];
        assert!(now.uncertainty_is_drawable());
        assert!(later.sigma_along_m > now.sigma_along_m);
        assert!(later.sigma_cross_m > now.sigma_cross_m);
    }

    #[test]
    fn a_stale_track_is_not_predicted() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [0.0; 3], [50.0, 0.0, 0.0], true);
        let out = p.predict(&[t], &[10.0], &[anchor(1, [1_000.0, 0.0, 0.0], 0.0)]);
        assert!(out[0].is_empty(), "no line to a place nothing is");
        assert_eq!(out[0].predictor, PredictorKind::ConstantVelocity);
    }

    #[test]
    fn a_non_finite_or_negative_horizon_produces_no_point() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [0.0; 3], [50.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[f64::NAN, -5.0, f64::INFINITY, 10.0], &[]);
        assert_eq!(
            out[0].points.len(),
            1,
            "only the one real horizon is computed, and nothing is clamped"
        );
        assert!((out[0].points[0].time_ahead_s - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_non_psd_covariance_yields_undrawable_uncertainty_rather_than_a_panic() {
        let p = ConstantVelocityPredictor;
        let mut t = track(1, [0.0; 3], [50.0, 0.0, 0.0], false);
        // A negative variance is a PSD violation upstream.
        t.covariance[(0, 0)] = -4.0;
        t.covariance[(1, 1)] = -4.0;
        t.covariance[(2, 2)] = -4.0;
        let out = p.predict(&[t], &[0.0], &[]);
        assert!(
            !out[0].points[0].uncertainty_is_drawable(),
            "the panel must draw no ellipse rather than a nonsense one"
        );
    }

    #[test]
    fn closest_approach_finds_the_analytic_minimum() {
        let p = ConstantVelocityPredictor;
        // Flying east along y = 400, past an asset at the origin.
        let t = track(1, [-1_000.0, 400.0, 0.0], [100.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[60.0], &[anchor(1, [0.0; 3], 0.0)]);
        let approach = out[0].approaches[0];
        assert_eq!(approach.asset, AssetId(1));
        assert!(
            (approach.time_ahead_s - 10.0).abs() < 1e-9,
            "closest at t = 1000/100"
        );
        assert!(
            (approach.distance_m - 400.0).abs() < 1e-9,
            "it passes at 400 m and never arrives"
        );
    }

    #[test]
    fn an_area_asset_is_approached_at_its_boundary() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [-1_000.0, 400.0, 0.0], [100.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[60.0], &[anchor(1, [0.0; 3], 150.0)]);
        assert!((out[0].approaches[0].distance_m - 250.0).abs() < 1e-9);
    }

    #[test]
    fn a_receding_track_is_closest_now() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [500.0, 0.0, 0.0], [100.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[60.0], &[anchor(1, [0.0; 3], 0.0)]);
        let approach = out[0].approaches[0];
        assert!(approach.time_ahead_s.abs() < f64::EPSILON);
        assert!((approach.distance_m - 500.0).abs() < 1e-9);
    }

    #[test]
    fn the_minimum_is_clamped_into_the_horizon() {
        let p = ConstantVelocityPredictor;
        // Would be closest at t = 100 s, but the horizon is 10 s.
        let t = track(1, [-10_000.0, 0.0, 0.0], [100.0, 0.0, 0.0], false);
        let out = p.predict(&[t], &[10.0], &[anchor(1, [0.0; 3], 0.0)]);
        let approach = out[0].approaches[0];
        assert!((approach.time_ahead_s - 10.0).abs() < 1e-9);
        assert!((approach.distance_m - 9_000.0).abs() < 1e-9);
    }

    #[test]
    fn a_stationary_track_has_no_along_track_axis_and_still_reports_a_sigma() {
        let p = ConstantVelocityPredictor;
        let t = track(1, [10.0, 20.0, 0.0], [0.0; 3], false);
        let out = p.predict(&[t], &[5.0], &[anchor(1, [0.0; 3], 0.0)]);
        let point = out[0].points[0];
        assert!(point.uncertainty_is_drawable());
        assert!((point.position_enu[0] - 10.0).abs() < f64::EPSILON);
        // It is closest now and stays there.
        assert!(out[0].approaches[0].time_ahead_s.abs() < f64::EPSILON);
    }
}
