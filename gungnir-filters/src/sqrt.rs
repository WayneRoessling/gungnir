// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The square-root-factorized Kalman filter: the verification-capability-table.md §1
//! row "`filters` | Square-root / UDU-factorized KF & EKF".
//!
//! Criterion, unchanged from §2: agreement with the standard-form filter to better than
//! 1e-6, and **zero PSD violations over 10⁵-plus cycles**.
//!
//! # What this buys, and why the standard form is still the default
//!
//! The standard form carries `P`. This carries a factor `S` with `P = S Sᵀ` and never
//! forms `P` at all inside the recursion. The difference is conditioning: every
//! operation here is an orthogonal transformation of `S`, and `S Sᵀ` is positive
//! semi-definite for *any* `S`, including one full of rounding error. A covariance that
//! is only ever reconstructed from a factor cannot go indefinite, whereas the standard
//! form's `P` can and does under enough cycles of a badly scaled problem. The price is
//! roughly a QR per step.
//!
//! [`crate::KalmanFilter`] stays the default because it is the one the linear row is
//! gated against and the cheaper of the two. This is the filter for a deployment whose
//! measurement noise spans orders of magnitude -- a radar reporting metres of range
//! error and milliradians of angle in the same vector -- where the standard form's
//! condition number is squared and the square-root form's is not.
//!
//! # One implementation covers both the KF and the EKF halves of the row
//!
//! The array update below needs a measurement matrix and a predicted measurement. For a
//! linear filter those are `H` and `H x`; for an extended one they are the Jacobian at
//! the current estimate and `h(x)`. That is the *only* difference between the two, so
//! [`SqrtKalmanFilter::update`] takes the linear path and
//! [`SqrtKalmanFilter::update_nonlinear`] evaluates a [`crate::MeasurementModel`] and
//! takes the same one. Writing the recursion twice would have given two chances to get
//! the array layout wrong.
//!
//! # The pre-array is dynamically sized, and that is not a §2.1 violation
//!
//! The QR pre-array is `(M + N) × (M + N)`, and stable Rust cannot express `M + N` as a
//! const-generic expression. It is built as a `DMatrix` inside the update and dropped
//! before the function returns. `agentic-coding-standards.md` §2.1 reserves dynamic
//! types for genuinely variable-cardinality **state**; this is scratch space whose size
//! is fixed at compile time by the type parameters, and the filter's state and factor
//! stay `SVector`/`SMatrix` throughout.

use crate::{FilterError, MeasurementModel};
use gungnir_core::MotionModel;
use nalgebra::{DMatrix, SMatrix, SVector};

/// Square-root-factorized Kalman filter.
///
/// Carries `S` with `P = S Sᵀ`; see the module documentation.
#[derive(Debug, Clone)]
pub struct SqrtKalmanFilter<Motion, const N: usize, const M: usize>
where
    Motion: MotionModel<N>,
{
    x: SVector<f64, N>,
    /// The covariance factor. `P` is never stored.
    s: SMatrix<f64, N, N>,
    motion: Motion,
    h: SMatrix<f64, M, N>,
    /// The measurement-noise factor, `R = Rc Rcᵀ`, formed once at construction.
    rc: SMatrix<f64, M, M>,
}

impl<Motion, const N: usize, const M: usize> SqrtKalmanFilter<Motion, N, M>
where
    Motion: MotionModel<N>,
{
    /// Build a filter from an initial estimate.
    ///
    /// # Errors
    ///
    /// [`FilterError::NotPositiveDefinite`] if `p0` or `r` is not, which is a caller
    /// error rather than a filter state this can recover from.
    pub fn new(
        x: SVector<f64, N>,
        p: SMatrix<f64, N, N>,
        motion: Motion,
        h: SMatrix<f64, M, N>,
        r: SMatrix<f64, M, M>,
    ) -> Result<Self, FilterError> {
        Ok(Self {
            x,
            s: crate::particle::psd_factor(&p, "the initial covariance")?,
            motion,
            h,
            rc: crate::particle::psd_factor(&r, "the measurement-noise covariance")?,
        })
    }

    /// The current estimate.
    #[must_use]
    pub fn state(&self) -> &SVector<f64, N> {
        &self.x
    }

    /// The covariance factor `S`, with `P = S Sᵀ`.
    #[must_use]
    pub fn factor(&self) -> &SMatrix<f64, N, N> {
        &self.s
    }

    /// `P = S Sᵀ`, reconstructed for a consumer that needs a covariance.
    ///
    /// **Positive semi-definite by construction**, whatever rounding has done to `S`.
    /// That is the whole point of this filter, and it is why the soak test can assert
    /// zero PSD violations rather than a tolerance.
    #[must_use]
    pub fn covariance(&self) -> SMatrix<f64, N, N> {
        self.s * self.s.transpose()
    }

    /// `x = F x`; the factor is re-triangularised from `[F S, G]` with `G Gᵀ = Q`.
    ///
    /// # Errors
    ///
    /// [`FilterError::NotPositiveDefinite`] if the motion model returns a `Q` that is
    /// not positive semi-definite. A *singular* `Q` is accepted, which is why the factor
    /// comes from an eigendecomposition rather than a Cholesky; see the `particle`
    /// module for which constant-velocity form is singular and which is not.
    pub fn predict(&mut self, dt: f64) -> Result<(), FilterError> {
        let f = self.motion.f(dt);
        let q = self.motion.q(dt);
        let g = crate::particle::psd_factor(&q, "the process-noise covariance")?;
        self.x = f * self.x;
        let fs = f * self.s;

        // Pre-array [F S, G] is N × 2N; its lower-triangular factor is the new S.
        let mut pre = DMatrix::<f64>::zeros(N, 2 * N);
        pre.view_mut((0, 0), (N, N)).copy_from(&fs);
        pre.view_mut((0, N), (N, N)).copy_from(&g);
        let triangular = lower_triangular_factor(&pre, N);
        self.s = SMatrix::<f64, N, N>::from_fn(|r, c| triangular[(r, c)]);
        Ok(())
    }

    /// The linear measurement update, in array form.
    ///
    /// # Errors
    ///
    /// [`FilterError::SingularInnovation`] when the innovation factor cannot be
    /// inverted, which means this measurement carries no information the filter can use.
    pub fn update(&mut self, z: &SVector<f64, M>) -> Result<(), FilterError> {
        let predicted = self.h * self.x;
        self.array_update(z, &self.h.clone(), &predicted)
    }

    /// The extended measurement update: the same array, with the Jacobian at the
    /// current estimate and the model's own predicted measurement.
    ///
    /// # Errors
    ///
    /// As [`Self::update`].
    pub fn update_nonlinear<Meas>(
        &mut self,
        z: &SVector<f64, M>,
        model: &Meas,
    ) -> Result<(), FilterError>
    where
        Meas: MeasurementModel<N, M>,
    {
        let jacobian = model.jacobian(&self.x);
        let predicted = model.predict_measurement(&self.x);
        // The residual goes through the model so a geometry that must wrap an angle
        // can; see `MeasurementModel::residual`.
        let residual = model.residual(z, &predicted);
        let equivalent = predicted + residual;
        self.array_update(&equivalent, &jacobian, &predicted)
    }

    /// The shared array update. See the module documentation for the block layout.
    fn array_update(
        &mut self,
        z: &SVector<f64, M>,
        h: &SMatrix<f64, M, N>,
        predicted: &SVector<f64, M>,
    ) -> Result<(), FilterError> {
        let hs = h * self.s;
        // Pre-array, (M + N) × (M + N):
        //     [ Rc     H S ]
        //     [ 0      S   ]
        // Its lower-triangular factor is
        //     [ Sy     0   ]
        //     [ K̄      S⁺  ]
        // with Sy Syᵀ the innovation covariance and K = K̄ Sy⁻¹.
        let dim = M + N;
        let mut pre = DMatrix::<f64>::zeros(dim, dim);
        pre.view_mut((0, 0), (M, M)).copy_from(&self.rc);
        pre.view_mut((0, M), (M, N)).copy_from(&hs);
        pre.view_mut((M, M), (N, N)).copy_from(&self.s);
        let post = lower_triangular_factor(&pre, dim);

        let sy = SMatrix::<f64, M, M>::from_fn(|r, c| post[(r, c)]);
        let k_bar = SMatrix::<f64, N, M>::from_fn(|r, c| post[(M + r, c)]);
        let s_next = SMatrix::<f64, N, N>::from_fn(|r, c| post[(M + r, M + c)]);

        let sy_inv = sy.try_inverse().ok_or(FilterError::SingularInnovation)?;
        let gain = k_bar * sy_inv;
        self.x += gain * (z - predicted);
        self.s = s_next;
        Ok(())
    }
}

/// The lower-triangular `L` with `L Lᵀ = A Aᵀ`, via a QR of `Aᵀ`.
///
/// `A = Lᵀ Qᵀ` for the QR factorisation `Aᵀ = Q U`, so `L = Uᵀ` is lower triangular and
/// `L Lᵀ = Uᵀ U = Aᵀᵀ A ᵀ= A Aᵀ`. Only the leading `size × size` block is returned,
/// which is the whole content of the factor.
///
/// The diagonal signs QR happens to produce are left alone. A factor is only ever used
/// as `L Lᵀ` or through the same `Sy` that produced the gain beside it, and both are
/// invariant to flipping the sign of a column, so normalising the signs would be work
/// that changes nothing.
fn lower_triangular_factor(a: &DMatrix<f64>, size: usize) -> DMatrix<f64> {
    let qr = a.transpose().qr();
    let u = qr.r();
    let mut out = DMatrix::<f64>::zeros(size, size);
    for r in 0..size {
        for c in 0..=r {
            out[(r, c)] = u[(c, r)];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Filter, KalmanFilter, RangeAzimuthElevation};
    use gungnir_core::ConstantVelocity;

    fn position_h() -> SMatrix<f64, 3, 6> {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        h
    }

    fn pair() -> (
        SqrtKalmanFilter<ConstantVelocity, 6, 3>,
        KalmanFilter<ConstantVelocity, 6, 3>,
    ) {
        let x = SVector::<f64, 6>::new(10.0, -5.0, 100.0, 3.0, 1.0, 0.0);
        let p = SMatrix::<f64, 6, 6>::from_diagonal(&SVector::<f64, 6>::new(
            400.0, 400.0, 900.0, 40.0, 40.0, 40.0,
        ));
        let motion = ConstantVelocity { sigma_a_sq: 4.0 };
        let r = SMatrix::<f64, 3, 3>::from_diagonal(&SVector::<f64, 3>::new(25.0, 25.0, 100.0));
        (
            SqrtKalmanFilter::new(x, p, motion, position_h(), r).expect("PSD"),
            KalmanFilter::new(x, p, motion, position_h(), r),
        )
    }

    /// The row's first criterion: the two forms must agree to better than 1e-6.
    #[test]
    fn the_square_root_form_tracks_the_standard_form() {
        let (mut sqrt, mut standard) = pair();
        for step in 0..400 {
            sqrt.predict(0.5).expect("PSD");
            standard.predict(0.5);
            let t = f64::from(step) * 0.5;
            let z = SVector::<f64, 3>::new(10.0 + 3.0 * t, -5.0 + t, 100.0);
            sqrt.update(&z).expect("non-singular");
            standard.update(&z);

            let dx = (sqrt.state() - standard.state()).norm() / standard.state().norm().max(1.0);
            assert!(dx < 1e-6, "state differed by {dx} at step {step}");
            let dp = (sqrt.covariance() - standard.covariance()).norm()
                / standard.covariance().norm().max(1.0);
            assert!(dp < 1e-6, "covariance differed by {dp} at step {step}");
        }
    }

    /// The row's second criterion, and the reason this filter exists: **zero** PSD
    /// violations over more than 100,000 cycles. Not a tolerance -- a count.
    #[test]
    fn zero_psd_violations_over_a_hundred_thousand_cycles() {
        let (mut sqrt, _) = pair();
        let mut violations = 0_u32;
        for step in 0..100_500_i32 {
            sqrt.predict(0.1).expect("PSD");
            let t = f64::from(step) * 0.1;
            sqrt.update(&SVector::<f64, 3>::new(t, -t, 100.0))
                .expect("non-singular");
            if step % 500 == 0 {
                let p = sqrt.covariance();
                let asymmetry = (p - p.transpose()).abs().max();
                let smallest = p.symmetric_eigenvalues().min();
                if asymmetry > 1e-9 || smallest < 0.0 || !p.iter().all(|v| v.is_finite()) {
                    violations += 1;
                }
            }
        }
        assert_eq!(violations, 0, "the covariance left the PSD cone");
    }

    /// The extended path must agree with the standard-form EKF as well, which is the
    /// other half of the row.
    #[test]
    fn the_extended_path_tracks_the_standard_form_ekf() {
        use crate::ExtendedKalmanFilter;
        let x = SVector::<f64, 6>::new(3000.0, 1500.0, 900.0, -40.0, 20.0, 0.0);
        let p = SMatrix::<f64, 6, 6>::from_diagonal(&SVector::<f64, 6>::new(
            2500.0, 2500.0, 2500.0, 400.0, 400.0, 400.0,
        ));
        let motion = ConstantVelocity { sigma_a_sq: 2.0 };
        let r = SMatrix::<f64, 3, 3>::from_diagonal(&SVector::<f64, 3>::new(100.0, 1e-6, 1e-6));
        let mut sqrt =
            SqrtKalmanFilter::new(x, p, motion, SMatrix::<f64, 3, 6>::zeros(), r).expect("PSD");
        let mut ekf = ExtendedKalmanFilter::new(x, p, motion, RangeAzimuthElevation::default(), r);
        let model = RangeAzimuthElevation::default();

        for step in 0..120 {
            sqrt.predict(1.0).expect("PSD");
            ekf.predict(1.0);
            let t = f64::from(step + 1);
            let truth = SVector::<f64, 6>::new(
                3000.0 - 40.0 * t,
                1500.0 + 20.0 * t,
                900.0,
                -40.0,
                20.0,
                0.0,
            );
            let z = model.predict_measurement(&truth);
            sqrt.update_nonlinear(&z, &model).expect("non-singular");
            ekf.update(&z);
            let dx = (sqrt.state() - ekf.state()).norm() / ekf.state().norm().max(1.0);
            assert!(dx < 1e-6, "extended state differed by {dx} at step {step}");
        }
    }

    /// The factorisation helper must actually factor: `L Lᵀ = A Aᵀ`.
    #[test]
    fn the_triangular_factor_reproduces_the_gram_matrix() {
        let a = DMatrix::<f64>::from_row_slice(
            3,
            5,
            &[
                2.0, 1.0, 0.5, 0.0, 1.0, 1.0, 3.0, 0.0, 2.0, 0.5, 0.5, 0.0, 4.0, 1.0, 0.0,
            ],
        );
        let l = lower_triangular_factor(&a, 3);
        let error = (&l * l.transpose() - &a * a.transpose()).abs().max();
        assert!(error < 1e-12, "L Lᵀ differed from A Aᵀ by {error}");
        for r in 0..3 {
            for c in (r + 1)..3 {
                assert!(
                    l[(r, c)].abs() < f64::EPSILON,
                    "not lower triangular at ({r}, {c}): {}",
                    l[(r, c)]
                );
            }
        }
    }
}
