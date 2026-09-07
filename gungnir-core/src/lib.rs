//! Verifies against verification-capability-table.md: "Motion models: CV, CA, CT".
//! Exact-match (~1e-10) vs. filterpy/hand-derived and MATLAB initcvkf/initcakf/initctekf.
//! Deterministic closed-form math -- no logging (per agentic-coding-standards.md §2.8).
//!
//! This crate is also the lowest crate in the workspace, so it owns the primitives
//! that both the tracking core and the productization layer share without either
//! depending on the other: [`TrackId`], [`TrackStatus`], [`ResourceId`] (module
//! [`ident`]) and the debug-only [`assert_psd`] helper (module [`numeric`]), per
//! agentic-coding-standards.md §1.2 (one owning crate, re-exported upward) and §2.1.
//!
//! # State-vector convention
//!
//! Every model here is written for the **block ordering** that `gungnir_track::Track`
//! already documents for its `state` field: position triple, then velocity triple,
//! then (for [`ConstantAcceleration`]) acceleration triple, in the local ENU tangent
//! frame the tracking service was configured with:
//!
//! ```text
//! CV, CT (N = 6):  [e, n, u, ve, vn, vu]
//! CA     (N = 9):  [e, n, u, ve, vn, vu, ae, an, au]
//! ```
//!
//! This is not a free choice: `gungnir_assessment::ConstantVelocityPredictor` already
//! reads the position block at `(0, 0)` and the velocity block at `(3, 3)`, and
//! `gungnir_rfs::GaussianComponent` carries the same 6-vector. It is also the ordering
//! `filterpy` produces with `order_by_dim=False`, which is what the differential test
//! compares against.
//!
//! # Process-noise convention
//!
//! `Q` is the **continuous** white-noise model, integrated over the step, not the
//! discrete piecewise-constant one: the tuning parameters are spectral densities
//! (`(m/s²)²/Hz` for CV and CT, `(m/s³)²/Hz` for CA), which is what their field
//! documentation says they are. In `filterpy` terms that is
//! `Q_continuous_white_noise`, not `Q_discrete_white_noise`; picking the wrong one
//! rescales every covariance in the workspace by a factor of `dt`, so the
//! differential test in `tests/motion_models_diff.rs` pins it.
//!
//! [`CoordinatedTurn`] takes the same acceleration spectral density as
//! [`ConstantVelocity`] and produces the same `Q`. That is the standard treatment
//! (the noise enters as an acceleration in Cartesian coordinates, not in the rotating
//! frame) and is what Stone Soup's `KnownTurnRate.covar` returns; the fixture records
//! both oracles agreeing.

pub mod ident;
pub mod numeric;

pub use ident::{ResourceId, TrackId, TrackStatus};
pub use numeric::assert_psd;

use nalgebra::SMatrix;

/// State-transition (F) and process-noise (Q) matrices for an N-dimensional motion model.
pub trait MotionModel<const N: usize> {
    /// State-transition matrix for the given timestep, seconds.
    fn f(&self, dt: f64) -> SMatrix<f64, N, N>;
    /// Process-noise covariance matrix for the given timestep, seconds.
    fn q(&self, dt: f64) -> SMatrix<f64, N, N>;
}

/// Below this `|ω dt|`, the coordinated-turn transition is evaluated from its Taylor
/// series rather than from `sin(x)/x` and `(1 - cos x)/x` directly. Chosen so that the
/// truncated series and the direct form agree to better than the row's 1e-10: the next
/// dropped term of `sinc` is `x⁶/5040`, which at `x = 1e-3` is 1.4e-22.
const SMALL_ANGLE: f64 = 1e-3;

/// `sin(x) / x`, continuous and exact at `x = 0`.
///
/// The direct quotient loses every significant digit as `x` approaches zero, which is
/// precisely the case a coordinated-turn model degenerating into constant velocity
/// hits every time a target stops turning.
#[inline]
fn sinc(x: f64) -> f64 {
    if x.abs() < SMALL_ANGLE {
        let x2 = x * x;
        1.0 - x2 / 6.0 + x2 * x2 / 120.0
    } else {
        x.sin() / x
    }
}

/// `(1 - cos x) / x`, continuous and exact at `x = 0`.
///
/// Evaluated as `(x / 2) · sinc(x / 2)²` via `1 - cos x = 2 sin²(x/2)`, which avoids
/// the cancellation in `1 - cos x` for small `x` as well as the division.
#[inline]
fn vers_over_x(x: f64) -> f64 {
    let h = x / 2.0;
    h * sinc(h) * sinc(h)
}

/// Continuous white-noise process covariance for one axis of a kinematic chain of
/// `ORDER` derivatives (2 = position/velocity, 3 = position/velocity/acceleration),
/// i.e. `∫₀^dt F(τ) G Gᵀ F(τ)ᵀ dτ` with the noise entering the highest derivative.
///
/// Element `(i, j)` of the closed form is
/// `dt^(2·ORDER - i - j - 1) / ((ORDER - i - 1)! (ORDER - j - 1)! (2·ORDER - i - j - 1))`.
/// Written out rather than assembled from a factorial table because `ORDER` is 2 or 3
/// and the literal form is the one a reviewer can check against the textbook.
fn axis_q<const ORDER: usize>(dt: f64) -> SMatrix<f64, ORDER, ORDER> {
    /// `n!` for the `n <= ORDER - 1 <= 2` this function indexes.
    const FACTORIAL: [f64; 3] = [1.0, 1.0, 2.0];

    debug_assert!(
        ORDER == 2 || ORDER == 3,
        "axis_q is derived and oracle-checked for ORDER 2 (CV, CT) and 3 (CA) only"
    );
    let mut q = SMatrix::<f64, ORDER, ORDER>::zeros();
    for i in 0..ORDER {
        for j in 0..ORDER {
            // `power` is at least 1 (reached at i = j = ORDER - 1) and at most 5.
            // Accumulated rather than cast so no integer-to-float conversion appears
            // in a crate whose pass criterion is exact agreement.
            let power = 2 * ORDER - i - j - 1;
            let mut dt_pow = 1.0_f64;
            let mut power_f = 0.0_f64;
            for _ in 0..power {
                dt_pow *= dt;
                power_f += 1.0;
            }
            q[(i, j)] = dt_pow / (FACTORIAL[ORDER - i - 1] * FACTORIAL[ORDER - j - 1] * power_f);
        }
    }
    q
}

/// Expand a per-axis `ORDER x ORDER` block matrix into the `N = 3 · ORDER` state
/// matrix under this crate's block ordering: entry `(i, j)` of the per-axis matrix
/// becomes `value · I₃` at block `(i, j)`.
fn per_axis_to_block<const ORDER: usize, const N: usize>(
    per_axis: &SMatrix<f64, ORDER, ORDER>,
) -> SMatrix<f64, N, N> {
    debug_assert_eq!(N, 3 * ORDER, "block expansion is over three spatial axes");
    let mut out = SMatrix::<f64, N, N>::zeros();
    for i in 0..ORDER {
        for j in 0..ORDER {
            for axis in 0..3 {
                out[(3 * i + axis, 3 * j + axis)] = per_axis[(i, j)];
            }
        }
    }
    out
}

/// Constant-velocity motion model (position + velocity per axis).
#[derive(Debug, Clone, Copy)]
pub struct ConstantVelocity {
    /// Process-noise spectral density, (m/s^2)^2/Hz.
    pub sigma_a_sq: f64,
}

impl MotionModel<6> for ConstantVelocity {
    fn f(&self, dt: f64) -> SMatrix<f64, 6, 6> {
        let mut f = SMatrix::<f64, 6, 6>::identity();
        for axis in 0..3 {
            f[(axis, 3 + axis)] = dt;
        }
        f
    }

    fn q(&self, dt: f64) -> SMatrix<f64, 6, 6> {
        let q = per_axis_to_block::<2, 6>(&axis_q::<2>(dt)) * self.sigma_a_sq;
        assert_psd(&q);
        q
    }
}

/// Constant-acceleration motion model.
#[derive(Debug, Clone, Copy)]
pub struct ConstantAcceleration {
    /// Process-noise spectral density, (m/s^3)^2/Hz.
    pub sigma_j_sq: f64,
}

impl MotionModel<9> for ConstantAcceleration {
    fn f(&self, dt: f64) -> SMatrix<f64, 9, 9> {
        let mut f = SMatrix::<f64, 9, 9>::identity();
        let half_dt_sq = 0.5 * dt * dt;
        for axis in 0..3 {
            f[(axis, 3 + axis)] = dt;
            f[(axis, 6 + axis)] = half_dt_sq;
            f[(3 + axis, 6 + axis)] = dt;
        }
        f
    }

    fn q(&self, dt: f64) -> SMatrix<f64, 9, 9> {
        let q = per_axis_to_block::<3, 9>(&axis_q::<3>(dt)) * self.sigma_j_sq;
        assert_psd(&q);
        q
    }
}

/// Coordinated-turn motion model: a constant-rate turn in the horizontal (east-north)
/// plane about the local vertical, with constant velocity on the vertical axis.
///
/// `omega` is signed: positive turns from east toward north (counter-clockwise seen
/// from above). At `omega == 0` the model degenerates exactly into
/// [`ConstantVelocity`], which the small-angle branch of [`sinc`] makes numerically
/// true rather than merely true in the limit.
#[derive(Debug, Clone, Copy)]
pub struct CoordinatedTurn {
    /// Turn rate, radians/second.
    pub omega: f64,
    /// Process-noise spectral density, (m/s^2)^2/Hz. The noise is an acceleration in
    /// Cartesian coordinates, so this is the same quantity, and produces the same `Q`,
    /// as [`ConstantVelocity::sigma_a_sq`] -- see the crate documentation.
    pub sigma_a_sq: f64,
}

impl MotionModel<6> for CoordinatedTurn {
    fn f(&self, dt: f64) -> SMatrix<f64, 6, 6> {
        let theta = self.omega * dt;
        let (sin_theta, cos_theta) = theta.sin_cos();
        // sin(ωdt)/ω and (1 - cos(ωdt))/ω, both written through dt so that ω = 0 is
        // an ordinary value rather than a division by zero.
        let s = dt * sinc(theta);
        let c = dt * vers_over_x(theta);

        let mut f = SMatrix::<f64, 6, 6>::identity();
        // Horizontal position from horizontal velocity.
        f[(0, 3)] = s;
        f[(0, 4)] = -c;
        f[(1, 3)] = c;
        f[(1, 4)] = s;
        // Vertical position from vertical velocity: plain constant velocity.
        f[(2, 5)] = dt;
        // Horizontal velocity rotates.
        f[(3, 3)] = cos_theta;
        f[(3, 4)] = -sin_theta;
        f[(4, 3)] = sin_theta;
        f[(4, 4)] = cos_theta;
        f
    }

    fn q(&self, dt: f64) -> SMatrix<f64, 6, 6> {
        ConstantVelocity {
            sigma_a_sq: self.sigma_a_sq,
        }
        .q(dt)
    }
}

pub use nalgebra::{SMatrix as StateMatrix, SVector as StateVector};

#[cfg(test)]
mod tests {
    use super::*;

    /// At `ω = 0` the coordinated turn must be the constant-velocity model exactly,
    /// not approximately: an IMM that switches between them otherwise sees a
    /// discontinuity at the moment a target stops turning.
    #[test]
    fn coordinated_turn_degenerates_to_constant_velocity() {
        let cv = ConstantVelocity { sigma_a_sq: 2.0 };
        let ct = CoordinatedTurn {
            omega: 0.0,
            sigma_a_sq: 2.0,
        };
        for dt in [0.001, 0.1, 1.0, 10.0] {
            let diff = (cv.f(dt) - ct.f(dt)).abs().max();
            assert!(diff < 1e-15, "F differs by {diff} at dt = {dt}");
            let diff_q = (cv.q(dt) - ct.q(dt)).abs().max();
            assert!(diff_q < 1e-15, "Q differs by {diff_q} at dt = {dt}");
        }
    }

    /// The small-angle branch must agree with the direct form where they meet, or the
    /// model has a step discontinuity at the threshold.
    ///
    /// The two are compared against the *direct form's own* error bound, because at
    /// the threshold the direct form is the less accurate of the pair -- which is the
    /// entire reason the branch exists. `sin x / x` carries only the relative rounding
    /// of `sin`, a few ulp; `(1 - cos x) / x` first cancels `1 - cos x` down to about
    /// `x²/2`, an absolute error of up to `ε/2` near 1.0, and then divides by `x`,
    /// which inflates that error to `ε / (2x)`. Asserting anything tighter than that
    /// would be asserting that a formula is more accurate than it is.
    #[test]
    fn small_angle_branch_is_continuous() {
        for x in [SMALL_ANGLE * 0.999, SMALL_ANGLE, SMALL_ANGLE * 1.001] {
            let sinc_bound = 4.0 * f64::EPSILON;
            assert!(
                (sinc(x) - x.sin() / x).abs() < sinc_bound,
                "sinc at {x}: {} vs {}",
                sinc(x),
                x.sin() / x
            );
            let vers_bound = 4.0 * f64::EPSILON / x;
            assert!(
                (vers_over_x(x) - (1.0 - x.cos()) / x).abs() < vers_bound,
                "vers at {x}: {} vs {} (bound {vers_bound:e})",
                vers_over_x(x),
                (1.0 - x.cos()) / x
            );
        }
    }

    /// Above the threshold the series is not used, so the two forms must be the same
    /// computation: this pins the branch point rather than the formulas.
    #[test]
    fn direct_branch_is_taken_above_the_threshold() {
        for x in [0.01, 0.5, 1.0, 3.0] {
            assert_eq!(sinc(x).to_bits(), (x.sin() / x).to_bits(), "sinc at {x}");
        }
    }

    /// A zero step is the identity for every model, and contributes no noise.
    #[test]
    fn zero_timestep_is_the_identity() {
        let cv = ConstantVelocity { sigma_a_sq: 1.0 };
        let ca = ConstantAcceleration { sigma_j_sq: 1.0 };
        let ct = CoordinatedTurn {
            omega: 0.5,
            sigma_a_sq: 1.0,
        };
        assert_eq!(cv.f(0.0), SMatrix::<f64, 6, 6>::identity());
        assert_eq!(ca.f(0.0), SMatrix::<f64, 9, 9>::identity());
        assert_eq!(ct.f(0.0), SMatrix::<f64, 6, 6>::identity());
        assert_eq!(cv.q(0.0), SMatrix::<f64, 6, 6>::zeros());
        assert_eq!(ca.q(0.0), SMatrix::<f64, 9, 9>::zeros());
    }

    /// Composition: turning for `dt` twice is turning for `2 dt` once. This is the
    /// property that says `F` really is a matrix exponential and not a first-order
    /// approximation of one.
    #[test]
    fn transition_composes() {
        let ct = CoordinatedTurn {
            omega: 0.37,
            sigma_a_sq: 1.0,
        };
        let once = ct.f(2.0);
        let twice = ct.f(1.0) * ct.f(1.0);
        let diff = (once - twice).abs().max();
        assert!(diff < 1e-12, "F(2dt) differs from F(dt)^2 by {diff}");
    }
}
