// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The nonlinear estimators: the EKF and the UKF rows of
//! `docs/verification-capability-table.md` §1 (GAP-011, Area A).
//!
//! * EKF: oracle `filterpy.kalman.ExtendedKalmanFilter`, criterion relative error
//!   < 1e-4 over the same nonlinear scenario and the same Jacobians.
//! * UKF: oracle `filterpy.kalman.UnscentedKalmanFilter` with
//!   `MerweScaledSigmaPoints`, criterion relative error < 1e-4 and sigma weights
//!   summing to one.
//!
//! # Why these exist beside the linear filter
//!
//! A radar does not measure position. It measures range, bearing and elevation, and the
//! map from a Cartesian state to those three is nonlinear. The linear filter can only be
//! used on a measurement someone has already converted, and converting a measurement
//! before filtering it throws away the shape of its uncertainty: a range-accurate,
//! bearing-vague measurement becomes a fat circle instead of the thin arc it really is.
//! Both filters here take the measurement as the sensor reports it.
//!
//! # Angles, and what is deliberately not handled
//!
//! An innovation in an angle needs wrapping onto `(-pi, pi]` when the measurement and
//! the prediction sit either side of the branch cut. **Neither filter here wraps**, and
//! that is a stated limitation rather than an oversight: wrapping is a property of the
//! measurement model, not of the estimator, and putting it in the estimator would apply
//! it to every component including the ranges. A measurement model whose geometry
//! crosses the cut needs [`MeasurementModel::residual`] overridden, which is why that
//! method exists on the trait with a default rather than being absent. The fixtures keep
//! their geometry in one quadrant so that this row compares two filters rather than two
//! wrapping conventions.
//!
//! # Numerics
//!
//! Both use the Joseph-form covariance update for the same reason
//! [`crate::KalmanFilter`] does, both re-symmetrize, and both call `assert_psd` after
//! mutating a covariance. The UKF's sigma points come from a Cholesky factor, which
//! exists only for a positive-definite covariance; a factorization that fails is
//! reported through [`FilterError::NotPositiveDefinite`] rather than worked around,
//! because a covariance that cannot be factored is a broken filter state and not a
//! numerical inconvenience.

use crate::{Filter, FilterError};
use gungnir_core::{assert_psd, MotionModel};
use nalgebra::{SMatrix, SVector};

/// A nonlinear measurement model: what a sensor would report for a state, and how that
/// report changes as the state does.
pub trait MeasurementModel<const N: usize, const M: usize> {
    /// The measurement a noiseless sensor would report for this state.
    fn predict_measurement(&self, x: &SVector<f64, N>) -> SVector<f64, M>;

    /// `dh/dx` at this state. Used by the EKF; the UKF never calls it.
    fn jacobian(&self, x: &SVector<f64, N>) -> SMatrix<f64, M, N>;

    /// The residual `z - h(x)`.
    ///
    /// Overridable because a model with an angle in it must wrap the difference onto
    /// `(-pi, pi]`, and only the model knows which components are angles. The default
    /// is the plain difference, which is right for every component that is not one.
    fn residual(&self, z: &SVector<f64, M>, predicted: &SVector<f64, M>) -> SVector<f64, M> {
        z - predicted
    }
}

/// Range, azimuth and elevation from a fixed sensor position, in the local ENU frame.
///
/// Azimuth is `atan2(east, north)`, which is a compass bearing: zero at north and
/// increasing to the east. Elevation is measured from the horizontal plane.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RangeAzimuthElevation {
    /// Where the sensor is, ENU metres.
    pub sensor_enu: [f64; 3],
}

impl RangeAzimuthElevation {
    #[must_use]
    pub fn at(sensor_enu: [f64; 3]) -> Self {
        Self { sensor_enu }
    }
}

impl<const N: usize> MeasurementModel<N, 3> for RangeAzimuthElevation {
    fn predict_measurement(&self, x: &SVector<f64, N>) -> SVector<f64, 3> {
        let e = x[0] - self.sensor_enu[0];
        let n = x[1] - self.sensor_enu[1];
        let u = x[2] - self.sensor_enu[2];
        let ground = e.hypot(n);
        SVector::<f64, 3>::new((e * e + n * n + u * u).sqrt(), e.atan2(n), u.atan2(ground))
    }

    fn jacobian(&self, x: &SVector<f64, N>) -> SMatrix<f64, 3, N> {
        let e = x[0] - self.sensor_enu[0];
        let n = x[1] - self.sensor_enu[1];
        let u = x[2] - self.sensor_enu[2];
        let ground_sq = e * e + n * n;
        let ground = ground_sq.sqrt();
        let r_sq = ground_sq + u * u;
        let r = r_sq.sqrt();
        let mut j = SMatrix::<f64, 3, N>::zeros();
        j[(0, 0)] = e / r;
        j[(0, 1)] = n / r;
        j[(0, 2)] = u / r;
        j[(1, 0)] = n / ground_sq;
        j[(1, 1)] = -e / ground_sq;
        j[(2, 0)] = -e * u / (r_sq * ground);
        j[(2, 1)] = -n * u / (r_sq * ground);
        j[(2, 2)] = ground / r_sq;
        j
    }
}

/// Azimuth alone, from a fixed sensor position, in the local ENU frame: **the
/// range-azimuth-elevation model with the range and elevation rows removed**
/// (docs/design/DN-27-bearing-only-detections.md §5 rule 1).
///
/// What an acoustic array, a ground-based direction finder and a person with a compass
/// report. DN-27 §5: "`gungnir-filters` already has the machinery for the update -- the
/// extended and unscented filters take a `MeasurementModel`, and a bearing-only model is
/// that model with the range row removed -- so an existing track's estimate is refined
/// by a bearing exactly as it is by a range-azimuth-elevation report, with no new
/// mathematics." This type is that sentence.
///
/// **It models an update and never an initiation.** One bearing does not determine a
/// position, so there is no state for a new track to start in; a filter given a sequence
/// of bearings from one fixed sensor converges confidently to the wrong range and the
/// failure looks exactly like success. The prohibition is enforced where tracks are
/// created, in `gungnir_fusion_async::FusionPipeline`, because a measurement model
/// cannot enforce anything on its own.
///
/// Azimuth is `atan2(east, north)`: a compass bearing, zero at north and increasing to
/// the east, the same convention [`RangeAzimuthElevation`] and
/// `gungnir_model::Measurement` state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BearingOnly {
    /// Where the sensor is, ENU metres.
    pub sensor_enu: [f64; 3],
}

impl BearingOnly {
    #[must_use]
    pub fn at(sensor_enu: [f64; 3]) -> Self {
        Self { sensor_enu }
    }
}

/// Wrap an angular residual onto `(-pi, pi]`.
///
/// Needed here and not in [`RangeAzimuthElevation`] for a stated reason: this model's
/// measurement is **entirely** an angle, so a track north of a sensor whose reported
/// bearing crosses from `+179` to `-179` degrees would otherwise produce a residual of
/// nearly a full turn and throw the estimate across the map. The module documentation
/// says wrapping belongs to the measurement model rather than to the estimator, and
/// this is a model that needs it.
fn wrap_to_pi(angle: f64) -> f64 {
    let wrapped = (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU);
    wrapped - std::f64::consts::PI
}

impl<const N: usize> MeasurementModel<N, 1> for BearingOnly {
    fn predict_measurement(&self, x: &SVector<f64, N>) -> SVector<f64, 1> {
        let e = x[0] - self.sensor_enu[0];
        let n = x[1] - self.sensor_enu[1];
        SVector::<f64, 1>::new(e.atan2(n))
    }

    fn jacobian(&self, x: &SVector<f64, N>) -> SMatrix<f64, 1, N> {
        let e = x[0] - self.sensor_enu[0];
        let n = x[1] - self.sensor_enu[1];
        let ground_sq = e * e + n * n;
        let mut j = SMatrix::<f64, 1, N>::zeros();
        // Undefined directly over the sensor, where the bearing carries no information
        // at all. Left at zero there rather than infinite, so the update contributes
        // nothing instead of destroying the estimate.
        if ground_sq > 0.0 {
            j[(0, 0)] = n / ground_sq;
            j[(0, 1)] = -e / ground_sq;
        }
        j
    }

    fn residual(&self, z: &SVector<f64, 1>, predicted: &SVector<f64, 1>) -> SVector<f64, 1> {
        SVector::<f64, 1>::new(wrap_to_pi(z[0] - predicted[0]))
    }
}

/// Azimuth and elevation, from a fixed sensor position, in the local ENU frame: the
/// range-azimuth-elevation model with the **range** row removed
/// (docs/design/DN-27-bearing-only-detections.md §5 rule 1).
///
/// The two-row form of [`BearingOnly`], for a direction finder that reports an
/// elevation as well. It is a separate type rather than an option on one because the
/// measurement dimension is part of the model's type, and because a missing elevation is
/// not a zero one (DN-27 §4): a model that filled an absent elevation with zero would
/// fold "on the horizon" into every track it touched.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AzimuthElevation {
    /// Where the sensor is, ENU metres.
    pub sensor_enu: [f64; 3],
}

impl AzimuthElevation {
    #[must_use]
    pub fn at(sensor_enu: [f64; 3]) -> Self {
        Self { sensor_enu }
    }
}

impl<const N: usize> MeasurementModel<N, 2> for AzimuthElevation {
    fn predict_measurement(&self, x: &SVector<f64, N>) -> SVector<f64, 2> {
        let e = x[0] - self.sensor_enu[0];
        let n = x[1] - self.sensor_enu[1];
        let u = x[2] - self.sensor_enu[2];
        SVector::<f64, 2>::new(e.atan2(n), u.atan2(e.hypot(n)))
    }

    fn jacobian(&self, x: &SVector<f64, N>) -> SMatrix<f64, 2, N> {
        let e = x[0] - self.sensor_enu[0];
        let n = x[1] - self.sensor_enu[1];
        let u = x[2] - self.sensor_enu[2];
        let ground_sq = e * e + n * n;
        let ground = ground_sq.sqrt();
        let r_sq = ground_sq + u * u;
        let mut j = SMatrix::<f64, 2, N>::zeros();
        // As `BearingOnly`: over the sensor, neither angle is defined, and a zero row
        // contributes nothing rather than a non-finite gain.
        if ground_sq > 0.0 && r_sq > 0.0 {
            j[(0, 0)] = n / ground_sq;
            j[(0, 1)] = -e / ground_sq;
            j[(1, 0)] = -e * u / (r_sq * ground);
            j[(1, 1)] = -n * u / (r_sq * ground);
            j[(1, 2)] = ground / r_sq;
        }
        j
    }

    fn residual(&self, z: &SVector<f64, 2>, predicted: &SVector<f64, 2>) -> SVector<f64, 2> {
        SVector::<f64, 2>::new(
            wrap_to_pi(z[0] - predicted[0]),
            // Elevation lives on `[-pi/2, pi/2]` and cannot cross a branch cut, so it
            // is the plain difference. Wrapping it would be wrong, not merely useless.
            z[1] - predicted[1],
        )
    }
}

/// Extended Kalman filter: the linear update taken at the Jacobian of a nonlinear
/// measurement model.
#[derive(Debug, Clone)]
pub struct ExtendedKalmanFilter<Motion, Meas, const N: usize, const M: usize>
where
    Motion: MotionModel<N>,
    Meas: MeasurementModel<N, M>,
{
    x: SVector<f64, N>,
    p: SMatrix<f64, N, N>,
    motion: Motion,
    measurement: Meas,
    r: SMatrix<f64, M, M>,
    rejected_updates: u32,
}

impl<Motion, Meas, const N: usize, const M: usize> ExtendedKalmanFilter<Motion, Meas, N, M>
where
    Motion: MotionModel<N>,
    Meas: MeasurementModel<N, M>,
{
    /// # Panics
    ///
    /// In debug builds only, if `p` or `r` is not a valid covariance.
    #[must_use]
    pub fn new(
        x: SVector<f64, N>,
        p: SMatrix<f64, N, N>,
        motion: Motion,
        measurement: Meas,
        r: SMatrix<f64, M, M>,
    ) -> Self {
        assert_psd(&p);
        assert_psd(&r);
        Self {
            x,
            p,
            motion,
            measurement,
            r,
            rejected_updates: 0,
        }
    }

    #[must_use]
    pub fn covariance(&self) -> &SMatrix<f64, N, N> {
        &self.p
    }

    /// Updates skipped because the innovation covariance was not invertible.
    #[must_use]
    pub fn rejected_updates(&self) -> u32 {
        self.rejected_updates
    }

    fn symmetrize(p: &SMatrix<f64, N, N>) -> SMatrix<f64, N, N> {
        (p + p.transpose()) * 0.5
    }
}

impl<Motion, Meas, const N: usize, const M: usize> Filter
    for ExtendedKalmanFilter<Motion, Meas, N, M>
where
    Motion: MotionModel<N>,
    Meas: MeasurementModel<N, M>,
{
    type State = SVector<f64, N>;
    type Measurement = SVector<f64, M>;

    fn predict(&mut self, dt: f64) {
        let f = self.motion.f(dt);
        self.x = f * self.x;
        self.p = Self::symmetrize(&(f * self.p * f.transpose() + self.motion.q(dt)));
        assert_psd(&self.p);
    }

    fn update(&mut self, z: &Self::Measurement) {
        let h = self.measurement.jacobian(&self.x);
        let pht = self.p * h.transpose();
        let s = h * pht + self.r;
        let Some(s_inv) = s.try_inverse() else {
            tracing::error!(
                "innovation covariance is singular: this measurement carries no usable \
                 information"
            );
            self.rejected_updates = self.rejected_updates.saturating_add(1);
            return;
        };
        let k = pht * s_inv;
        let y = self
            .measurement
            .residual(z, &self.measurement.predict_measurement(&self.x));
        self.x += k * y;
        // Joseph form, for the reason `kalman.rs` documents at length.
        let i_kh = SMatrix::<f64, N, N>::identity() - k * h;
        self.p = Self::symmetrize(&(i_kh * self.p * i_kh.transpose() + k * self.r * k.transpose()));
        assert_psd(&self.p);
    }

    fn state(&self) -> &Self::State {
        &self.x
    }
}

/// The scaled sigma-point set of Van der Merwe, which is what `filterpy`'s
/// `MerweScaledSigmaPoints` produces and what the UKF row compares against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SigmaPointSettings {
    /// Spread of the points around the mean; small and positive.
    pub alpha: f64,
    /// Prior knowledge of the distribution; 2 is optimal for a Gaussian.
    pub beta: f64,
    /// Secondary scaling, commonly `0` or `3 - n`.
    pub kappa: f64,
}

impl Default for SigmaPointSettings {
    fn default() -> Self {
        Self {
            alpha: 0.1,
            beta: 2.0,
            kappa: 0.0,
        }
    }
}

impl SigmaPointSettings {
    /// `lambda = alpha^2 (n + kappa) - n`.
    ///
    /// The state dimension is small by construction (six here, and no filter in this
    /// workspace has a state a double cannot hold exactly), so the conversion is exact.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn lambda(&self, n: usize) -> f64 {
        let n_f = n as f64;
        self.alpha * self.alpha * (n_f + self.kappa) - n_f
    }

    /// The mean and covariance weights, in `filterpy`'s order: the centre point first,
    /// then the `2n` others.
    ///
    /// # Panics
    ///
    /// If `n + lambda` is zero, which no usable parameter set produces and which would
    /// make every weight infinite.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn weights(&self, n: usize) -> (Vec<f64>, Vec<f64>) {
        let lambda = self.lambda(n);
        let denom = n as f64 + lambda;
        assert!(
            denom.abs() > f64::EPSILON,
            "sigma-point scaling is degenerate: n + lambda = {denom}"
        );
        let common = 0.5 / denom;
        let mut wm = vec![common; 2 * n + 1];
        let mut wc = vec![common; 2 * n + 1];
        wm[0] = lambda / denom;
        wc[0] = lambda / denom + (1.0 - self.alpha * self.alpha + self.beta);
        (wm, wc)
    }
}

/// Unscented Kalman filter: the distribution is propagated through the nonlinearity by
/// a deterministic set of sample points rather than by a Jacobian.
#[derive(Debug, Clone)]
pub struct UnscentedKalmanFilter<Motion, Meas, const N: usize, const M: usize>
where
    Motion: MotionModel<N>,
    Meas: MeasurementModel<N, M>,
{
    x: SVector<f64, N>,
    p: SMatrix<f64, N, N>,
    motion: Motion,
    measurement: Meas,
    r: SMatrix<f64, M, M>,
    points: SigmaPointSettings,
    /// The sigma points after the last predict, which the update needs for the cross
    /// covariance. `filterpy` keeps them the same way and for the same reason.
    propagated: Vec<SVector<f64, N>>,
}

impl<Motion, Meas, const N: usize, const M: usize> UnscentedKalmanFilter<Motion, Meas, N, M>
where
    Motion: MotionModel<N>,
    Meas: MeasurementModel<N, M>,
{
    /// # Panics
    ///
    /// In debug builds only, if `p` or `r` is not a valid covariance.
    #[must_use]
    pub fn new(
        x: SVector<f64, N>,
        p: SMatrix<f64, N, N>,
        motion: Motion,
        measurement: Meas,
        r: SMatrix<f64, M, M>,
        points: SigmaPointSettings,
    ) -> Self {
        assert_psd(&p);
        assert_psd(&r);
        Self {
            x,
            p,
            motion,
            measurement,
            r,
            points,
            propagated: Vec::new(),
        }
    }

    #[must_use]
    pub fn covariance(&self) -> &SMatrix<f64, N, N> {
        &self.p
    }

    #[must_use]
    pub fn sigma_point_settings(&self) -> SigmaPointSettings {
        self.points
    }

    /// The `2n + 1` sigma points of the current estimate.
    ///
    /// `filterpy` factors `(n + lambda) P` with `scipy.linalg.cholesky`, whose default
    /// is the **upper** triangular `U` with `P = U^T U`, and uses its rows. `nalgebra`
    /// gives the lower `L` with `P = L L^T`, and the `k`th row of `U` is the `k`th
    /// column of `L`; the columns are used here for exactly that reason.
    ///
    /// # Errors
    ///
    /// [`FilterError::NotPositiveDefinite`] when the scaled covariance has no Cholesky
    /// factor, which means the filter state is already broken.
    #[allow(clippy::cast_precision_loss)]
    pub fn sigma_points(&self) -> Result<Vec<SVector<f64, N>>, FilterError> {
        let scale = N as f64 + self.points.lambda(N);
        let scaled = self.p * scale;
        let chol = scaled.cholesky().ok_or(FilterError::NotPositiveDefinite {
            what: "(n + lambda) P",
        })?;
        let l = chol.l();
        let mut points = Vec::with_capacity(2 * N + 1);
        points.push(self.x);
        for k in 0..N {
            points.push(self.x + l.column(k));
        }
        for k in 0..N {
            points.push(self.x - l.column(k));
        }
        Ok(points)
    }

    /// Predict, reporting a covariance that cannot be factored rather than panicking.
    ///
    /// # Errors
    ///
    /// As [`UnscentedKalmanFilter::sigma_points`].
    pub fn try_predict(&mut self, dt: f64) -> Result<(), FilterError> {
        let f = self.motion.f(dt);
        let points = self.sigma_points()?;
        let (wm, wc) = self.points.weights(N);
        self.propagated = points.iter().map(|s| f * s).collect();

        let mut mean = SVector::<f64, N>::zeros();
        for (w, s) in wm.iter().zip(&self.propagated) {
            mean += s * *w;
        }
        let mut cov = self.motion.q(dt);
        for (w, s) in wc.iter().zip(&self.propagated) {
            let d = s - mean;
            cov += (d * d.transpose()) * *w;
        }
        self.x = mean;
        self.p = (cov + cov.transpose()) * 0.5;
        assert_psd(&self.p);
        Ok(())
    }

    /// Update, reporting a singular innovation covariance rather than ignoring it.
    ///
    /// # Errors
    ///
    /// [`FilterError::SingularInnovation`] when the innovation covariance cannot be
    /// inverted, and [`FilterError::NotPositiveDefinite`] when no sigma points exist.
    pub fn try_update(&mut self, z: &SVector<f64, M>) -> Result<(), FilterError> {
        if self.propagated.is_empty() {
            self.propagated = self.sigma_points()?;
        }
        let (wm, wc) = self.points.weights(N);
        let measured: Vec<SVector<f64, M>> = self
            .propagated
            .iter()
            .map(|s| self.measurement.predict_measurement(s))
            .collect();

        let mut z_mean = SVector::<f64, M>::zeros();
        for (w, m) in wm.iter().zip(&measured) {
            z_mean += m * *w;
        }
        let mut s = self.r;
        let mut cross = SMatrix::<f64, N, M>::zeros();
        for ((w, m), point) in wc.iter().zip(&measured).zip(&self.propagated) {
            let dz = m - z_mean;
            s += (dz * dz.transpose()) * *w;
            cross += ((point - self.x) * dz.transpose()) * *w;
        }
        let s_inv = s.try_inverse().ok_or(FilterError::SingularInnovation)?;
        let k = cross * s_inv;
        self.x += k * self.measurement.residual(z, &z_mean);
        let p = self.p - k * s * k.transpose();
        self.p = (p + p.transpose()) * 0.5;
        assert_psd(&self.p);
        Ok(())
    }
}

impl<Motion, Meas, const N: usize, const M: usize> Filter
    for UnscentedKalmanFilter<Motion, Meas, N, M>
where
    Motion: MotionModel<N>,
    Meas: MeasurementModel<N, M>,
{
    type State = SVector<f64, N>;
    type Measurement = SVector<f64, M>;

    /// The infallible surface the trait requires. A failure is logged and the estimate
    /// is left where it was, which is the honest outcome: an un-predicted estimate is
    /// stale and says so through its timestamp, whereas a fabricated one does not.
    /// Callers that must act on the failure use [`UnscentedKalmanFilter::try_predict`].
    fn predict(&mut self, dt: f64) {
        if let Err(err) = self.try_predict(dt) {
            tracing::error!(%err, "UKF predict failed; the estimate was not advanced");
        }
    }

    fn update(&mut self, z: &Self::Measurement) {
        if let Err(err) = self.try_update(z) {
            tracing::error!(%err, "UKF update failed; the measurement was not applied");
        }
    }

    fn state(&self) -> &Self::State {
        &self.x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_core::ConstantVelocity;

    fn state() -> SVector<f64, 6> {
        SVector::<f64, 6>::new(8_000.0, 12_000.0, 3_000.0, 120.0, 60.0, 5.0)
    }

    /// The Jacobian is the derivative it claims to be. Checked against a central
    /// difference rather than against itself, because an analytic Jacobian that is
    /// quietly wrong is the classic way an EKF diverges slowly enough to look plausible.
    #[test]
    fn the_jacobian_agrees_with_a_central_difference() {
        let model = RangeAzimuthElevation::at([100.0, -200.0, 30.0]);
        let x = state();
        let analytic: SMatrix<f64, 3, 6> = model.jacobian(&x);
        let h = 1e-4;
        for col in 0..3 {
            let mut forward = x;
            let mut back = x;
            forward[col] += h;
            back[col] -= h;
            let numeric = (model.predict_measurement(&forward) - model.predict_measurement(&back))
                / (2.0 * h);
            for row in 0..3 {
                let a = analytic[(row, col)];
                let n = numeric[row];
                assert!(
                    (a - n).abs() <= 1e-6 * a.abs().max(1.0),
                    "d h[{row}] / d x[{col}]: analytic {a}, numeric {n}"
                );
            }
        }
        // The velocity columns are zero: this sensor does not measure velocity, and a
        // non-zero entry would claim it does.
        for col in 3..6 {
            for row in 0..3 {
                assert!(analytic[(row, col)].abs() < f64::EPSILON);
            }
        }
    }

    /// The row's own second criterion, and the property that makes a sigma-point set a
    /// distribution rather than an arbitrary cloud.
    #[test]
    fn the_sigma_weights_sum_to_one() {
        for settings in [
            SigmaPointSettings::default(),
            SigmaPointSettings {
                alpha: 0.5,
                beta: 2.0,
                kappa: 0.0,
            },
            SigmaPointSettings {
                alpha: 0.1,
                beta: 2.0,
                kappa: -3.0,
            },
        ] {
            let (wm, wc) = settings.weights(6);
            assert_eq!(wm.len(), 13);
            let sum: f64 = wm.iter().sum();
            assert!((sum - 1.0).abs() < 1e-12, "mean weights sum to {sum}");
            // The covariance weights sum to one plus the beta correction on the centre
            // point, which is deliberate and is what filterpy produces.
            let sum_c: f64 = wc.iter().sum();
            let expected = 1.0 + (1.0 - settings.alpha * settings.alpha + settings.beta);
            assert!(
                (sum_c - expected).abs() < 1e-12,
                "covariance weights sum to {sum_c}, expected {expected}"
            );
        }
    }

    /// A linear measurement makes the two filters agree with each other and with the
    /// linear one, which is the sanity check that neither is merely self-consistent.
    #[test]
    fn on_a_linear_measurement_both_agree_with_the_linear_filter() {
        /// Position, measured directly.
        struct Position;
        impl MeasurementModel<6, 3> for Position {
            fn predict_measurement(&self, x: &SVector<f64, 6>) -> SVector<f64, 3> {
                SVector::<f64, 3>::new(x[0], x[1], x[2])
            }
            fn jacobian(&self, _x: &SVector<f64, 6>) -> SMatrix<f64, 3, 6> {
                let mut h = SMatrix::<f64, 3, 6>::zeros();
                h[(0, 0)] = 1.0;
                h[(1, 1)] = 1.0;
                h[(2, 2)] = 1.0;
                h
            }
        }

        let p0 = SMatrix::<f64, 6, 6>::identity() * 500.0;
        let r = SMatrix::<f64, 3, 3>::identity() * 100.0;
        let motion = ConstantVelocity { sigma_a_sq: 4.0 };
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        h[(0, 0)] = 1.0;
        h[(1, 1)] = 1.0;
        h[(2, 2)] = 1.0;

        let mut linear = crate::KalmanFilter::new(state(), p0, motion, h, r);
        let mut ekf = ExtendedKalmanFilter::new(state(), p0, motion, Position, r);
        let mut ukf = UnscentedKalmanFilter::new(
            state(),
            p0,
            motion,
            Position,
            r,
            SigmaPointSettings::default(),
        );

        for step in 1..8 {
            let z = SVector::<f64, 3>::new(
                8_000.0 + 120.0 * f64::from(step),
                12_000.0 + 60.0 * f64::from(step),
                3_000.0 + 5.0 * f64::from(step),
            );
            linear.predict(1.0);
            ekf.predict(1.0);
            ukf.predict(1.0);
            linear.update(&z);
            ekf.update(&z);
            ukf.update(&z);
        }
        for i in 0..6 {
            let l = linear.state()[i];
            assert!(
                (ekf.state()[i] - l).abs() < 1e-8,
                "EKF differs from the linear filter at {i}: {} vs {l}",
                ekf.state()[i]
            );
            assert!(
                (ukf.state()[i] - l).abs() < 1e-6,
                "UKF differs from the linear filter at {i}: {} vs {l}",
                ukf.state()[i]
            );
        }
    }
}
