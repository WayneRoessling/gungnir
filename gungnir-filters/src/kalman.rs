// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The standard-form linear Kalman filter: the verification-capability-table.md §1
//! row "Linear Kalman Filter".
//!
//! Oracle `filterpy.kalman.KalmanFilter`, criterion state < 1e-6 and covariance
//! Frobenius difference < 1e-6 over the same measurement sequence.
//!
//! # Why the formulas are written the way they are
//!
//! Two choices here are about numerics rather than taste, and both are what the
//! oracle does as well, so matching it is also the better implementation:
//!
//! * **Joseph form for the covariance update.** `P = (I - KH) P (I - KH)ᵀ + K R Kᵀ`
//!   rather than the shorter `P = (I - KH) P`. The short form is correct only for the
//!   optimal gain and in exact arithmetic; in floating point it loses symmetry and
//!   drifts indefinite, which is precisely the failure the PSD gate in
//!   agentic-coding-standards.md §2.1 exists to catch. The Joseph form is a sum of two
//!   congruences, so it stays symmetric and PSD by construction.
//! * **`K = P Hᵀ S⁻¹` with `S = H P Hᵀ + R`.** `S` is positive definite whenever `R`
//!   is and `P` is positive semi-definite, so the inverse exists for every valid
//!   filter state. [`KalmanFilter::rejected_updates`] counts the times it did not,
//!   which is a report that an invariant was already broken upstream rather than a
//!   condition this filter is expected to meet.
//!
//! The covariance is re-symmetrized after each step (`(P + Pᵀ)/2`). That is not a
//! correction for a wrong formula -- the Joseph form is already symmetric in exact
//! arithmetic -- it removes the last-bit asymmetry that rounding leaves behind, which
//! would otherwise trip `assert_psd`'s symmetry check after enough cycles.

use crate::Filter;
use gungnir_core::{assert_psd, MotionModel};
use nalgebra::{SMatrix, SVector};

/// Standard-form linear Kalman filter -- the reference every other filter here is
/// compared against or built from.
///
/// `N` is the state dimension, `M` the measurement dimension. The motion model is a
/// type parameter rather than a stored pair of matrices so that `F` and `Q` are
/// always evaluated at the `dt` actually being taken: a filter that cached them would
/// silently use the wrong ones the first time a sensor reported off its nominal rate,
/// which is exactly what Scenario 3 does on purpose.
#[derive(Debug, Clone)]
pub struct KalmanFilter<Model, const N: usize, const M: usize>
where
    Model: MotionModel<N>,
{
    /// State estimate.
    x: SVector<f64, N>,
    /// Estimate covariance.
    p: SMatrix<f64, N, N>,
    /// Supplies `F(dt)` and `Q(dt)`.
    motion: Model,
    /// Measurement matrix.
    h: SMatrix<f64, M, N>,
    /// Measurement noise covariance.
    r: SMatrix<f64, M, M>,
    /// Updates skipped because the innovation covariance was not invertible. Always
    /// zero for a well-formed filter; see the module documentation.
    rejected_updates: u32,
}

impl<Model, const N: usize, const M: usize> KalmanFilter<Model, N, M>
where
    Model: MotionModel<N>,
{
    /// Build a filter from an initial estimate and the measurement model.
    ///
    /// # Panics
    /// In debug builds only, if `p` or `r` is not a valid covariance
    /// (agentic-coding-standards.md §2.1). A caller that hands in an indefinite
    /// covariance has a bug upstream, and every later step would inherit it.
    #[must_use]
    pub fn new(
        x: SVector<f64, N>,
        p: SMatrix<f64, N, N>,
        motion: Model,
        h: SMatrix<f64, M, N>,
        r: SMatrix<f64, M, M>,
    ) -> Self {
        assert_psd(&p);
        assert_psd(&r);
        Self {
            x,
            p,
            motion,
            h,
            r,
            rejected_updates: 0,
        }
    }

    /// The current estimate covariance.
    #[must_use]
    pub fn covariance(&self) -> &SMatrix<f64, N, N> {
        &self.p
    }

    /// The measurement matrix.
    #[must_use]
    pub fn measurement_matrix(&self) -> &SMatrix<f64, M, N> {
        &self.h
    }

    /// The measurement noise covariance.
    #[must_use]
    pub fn measurement_noise(&self) -> &SMatrix<f64, M, M> {
        &self.r
    }

    /// How many updates were skipped because the innovation covariance was singular.
    ///
    /// Non-zero means a caller supplied a singular `R` together with a `P` singular in
    /// the same direction. The filter reports it rather than inventing a
    /// pseudo-inverse, because the honest answer is that the measurement carried no
    /// information the filter could use.
    #[must_use]
    pub fn rejected_updates(&self) -> u32 {
        self.rejected_updates
    }

    /// Innovation covariance `S = H P Hᵀ + R` for the current state.
    #[must_use]
    pub fn innovation_covariance(&self) -> SMatrix<f64, M, M> {
        self.h * self.p * self.h.transpose() + self.r
    }

    /// Innovation (measurement residual) `y = z - H x` for the current state.
    #[must_use]
    pub fn innovation(&self, z: &SVector<f64, M>) -> SVector<f64, M> {
        z - self.h * self.x
    }

    /// Replace the estimate wholesale.
    ///
    /// This exists for the IMM's mixing step, which starts every mode from a blend of
    /// every mode's posterior rather than from its own. It is deliberately not part of
    /// the [`Filter`] trait: an ordinary consumer setting a filter's state by hand is
    /// discarding everything the filter learned, and the one caller that legitimately
    /// does so is doing it as a defined step of a documented recursion.
    ///
    /// # Panics
    /// In debug builds only, if `p` is not a valid covariance.
    pub fn set_estimate(&mut self, x: SVector<f64, N>, p: SMatrix<f64, N, N>) {
        assert_psd(&p);
        self.x = x;
        self.p = p;
    }

    /// Force exact symmetry, removing the last-bit asymmetry rounding leaves behind.
    fn symmetrize(p: &SMatrix<f64, N, N>) -> SMatrix<f64, N, N> {
        (p + p.transpose()) * 0.5
    }
}

impl<Model, const N: usize, const M: usize> Filter for KalmanFilter<Model, N, M>
where
    Model: MotionModel<N>,
{
    type State = SVector<f64, N>;
    type Measurement = SVector<f64, M>;

    /// `x = F x`, `P = F P Fᵀ + Q`.
    fn predict(&mut self, dt: f64) {
        let f = self.motion.f(dt);
        let q = self.motion.q(dt);
        self.x = f * self.x;
        self.p = Self::symmetrize(&(f * self.p * f.transpose() + q));
        assert_psd(&self.p);
    }

    /// Joseph-form update; see the module documentation for why.
    fn update(&mut self, z: &Self::Measurement) {
        let ht = self.h.transpose();
        let pht = self.p * ht;
        let s = self.h * pht + self.r;
        let Some(s_inv) = s.try_inverse() else {
            // Unreachable for a positive-definite R; counted rather than papered over.
            debug_assert!(
                false,
                "innovation covariance is singular: R and P are singular in the same \
                 direction, so this measurement carries no usable information"
            );
            self.rejected_updates = self.rejected_updates.saturating_add(1);
            return;
        };
        let k = pht * s_inv;
        let y = z - self.h * self.x;
        self.x += k * y;

        let i_kh = SMatrix::<f64, N, N>::identity() - k * self.h;
        self.p = Self::symmetrize(&(i_kh * self.p * i_kh.transpose() + k * self.r * k.transpose()));
        assert_psd(&self.p);
    }

    fn state(&self) -> &Self::State {
        &self.x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_core::ConstantVelocity;

    /// A 6-state constant-velocity filter observing position only, which is the shape
    /// every scenario in `gungnir-scenario` produces.
    fn position_only() -> KalmanFilter<ConstantVelocity, 6, 3> {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        KalmanFilter::new(
            SVector::<f64, 6>::zeros(),
            SMatrix::<f64, 6, 6>::identity() * 100.0,
            ConstantVelocity { sigma_a_sq: 1.0 },
            h,
            SMatrix::<f64, 3, 3>::identity() * 25.0,
        )
    }

    /// A measurement must move the estimate toward it and shrink the covariance:
    /// the two things a filter is for.
    #[test]
    fn an_update_pulls_the_estimate_toward_the_measurement() {
        let mut kf = position_only();
        let before_trace = kf.covariance().trace();
        kf.predict(1.0);
        kf.update(&SVector::<f64, 3>::new(10.0, 0.0, 0.0));
        assert!(kf.state()[0] > 0.0, "estimate did not move toward 10 m");
        assert!(kf.state()[0] < 10.0, "estimate overshot the measurement");
        assert!(
            kf.covariance().trace() < before_trace,
            "covariance did not shrink after an update"
        );
        assert_eq!(kf.rejected_updates(), 0);
    }

    /// Covariance must stay symmetric and positive semi-definite over a long run.
    /// This is the property the Joseph form is chosen for, and the one that decides
    /// whether the 10^5-cycle stability row can ever pass.
    #[test]
    fn covariance_stays_psd_over_a_long_run() {
        let mut kf = position_only();
        for step in 0..20_000_i32 {
            kf.predict(0.1);
            let t = f64::from(step) * 0.1;
            kf.update(&SVector::<f64, 3>::new(t, -t, 5.0));
            let p = kf.covariance();
            let asymmetry = (p - p.transpose()).abs().max();
            assert!(asymmetry < 1e-12, "asymmetry {asymmetry} at step {step}");
            assert!(
                p.iter().all(|v| v.is_finite()),
                "non-finite covariance at step {step}"
            );
        }
        assert!(kf.covariance().cholesky().is_some(), "covariance lost rank");
    }

    /// Repeating the same measurement must converge, not oscillate.
    #[test]
    fn repeated_measurements_converge() {
        let mut kf = position_only();
        let z = SVector::<f64, 3>::new(50.0, -20.0, 5.0);
        for _ in 0..200 {
            kf.predict(0.1);
            kf.update(&z);
        }
        for axis in 0..3 {
            assert!(
                (kf.state()[axis] - z[axis]).abs() < 1.0,
                "axis {axis} did not converge: {} vs {}",
                kf.state()[axis],
                z[axis]
            );
        }
    }

    /// A zero-length step must be the identity: no information appears from nowhere.
    #[test]
    fn zero_dt_predict_changes_nothing() {
        let mut kf = position_only();
        kf.update(&SVector::<f64, 3>::new(3.0, 4.0, 5.0));
        let x = *kf.state();
        let p = *kf.covariance();
        kf.predict(0.0);
        assert!((kf.state() - x).abs().max() < 1e-15);
        assert!((kf.covariance() - p).abs().max() < 1e-15);
    }
}
