//! filters: KF, EKF, UKF, PF, IMM, sqrt/UDU, RTS -- verification-capability-table.md
//! rows "Linear Kalman Filter" through "RTS smoother / fixed-lag smoothing".
//! Fixed-size nalgebra types only (SVector/SMatrix); DVector/DMatrix reserved for
//! genuinely variable-cardinality state per agentic-coding-standards.md §2.1.

use gungnir_coord::Ecef;

/// The oracle-comparable surface every concrete filter implements, per
/// agentic-coding-standards.md §1.3 -- gungnir-oracle writes one differential-test
/// harness against this trait rather than per concrete type.
pub trait Filter {
    type State;
    type Measurement;

    /// # Correctness
    /// Implementations must call the debug-only `gungnir_core::assert_psd` helper
    /// after mutating covariance in place (agentic-coding-standards.md §2.1).
    fn predict(&mut self, dt: f64);
    fn update(&mut self, z: &Self::Measurement);
    fn state(&self) -> &Self::State;
}

pub mod imm;
pub mod kalman;
pub mod nonlinear;
pub mod particle;
pub mod sqrt;

pub use imm::{Imm, ModeFilter};
pub use kalman::KalmanFilter;
pub use nonlinear::{
    AzimuthElevation, BearingOnly, ExtendedKalmanFilter, MeasurementModel, RangeAzimuthElevation,
    SigmaPointSettings, UnscentedKalmanFilter,
};
pub use particle::{ParticleFilter, ResampleStrategy};
pub use sqrt::SqrtKalmanFilter;

/// One filtered estimate of a trajectory: the input the smoother runs backwards over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoothed<const N: usize> {
    pub state: nalgebra::SVector<f64, N>,
    pub covariance: nalgebra::SMatrix<f64, N, N>,
}

/// Rauch-Tung-Striebel backward smoothing pass over an already-filtered trajectory.
///
/// Row: "`filters` | RTS smoother / fixed-lag smoothing" in
/// `docs/verification-capability-table.md` §1. Oracle `filterpy.kalman.rts_smoother`,
/// criterion relative error < 1e-6.
///
/// The recursion, which is linear whatever filter produced the forward pass:
///
/// ```text
/// P⁻(k+1) = F P(k) Fᵀ + Q
/// C(k)    = P(k) Fᵀ [P⁻(k+1)]⁻¹
/// x(k|N)  = x(k) + C(k) [x(k+1|N) − F x(k)]
/// P(k|N)  = P(k) + C(k) [P(k+1|N) − P⁻(k+1)] C(k)ᵀ
/// ```
///
/// **The signature takes covariances, and the scaffold's did not.** A smoother given
/// only states cannot compute `C(k)`, so the old signature could never have been
/// implemented; it returned an error saying so, which was the right answer to the wrong
/// question. The last estimate is returned unchanged, because there is nothing after it
/// to smooth towards, and an empty result would read as a track that was never there.
///
/// # Errors
///
/// [`FilterError::NotPositiveDefinite`] when a predicted covariance cannot be inverted,
/// which means the forward pass handed in a broken estimate.
pub fn rts_smooth<Motion, const N: usize>(
    forward: &[Smoothed<N>],
    motion: &Motion,
    dt: f64,
) -> Result<Vec<Smoothed<N>>, FilterError>
where
    Motion: gungnir_core::MotionModel<N>,
{
    if forward.is_empty() {
        return Ok(Vec::new());
    }
    let f = motion.f(dt);
    let q = motion.q(dt);
    let mut out = forward.to_vec();
    for k in (0..forward.len() - 1).rev() {
        let p = forward[k].covariance;
        let predicted_p = f * p * f.transpose() + q;
        let inverse = predicted_p
            .try_inverse()
            .ok_or(FilterError::NotPositiveDefinite {
                what: "the predicted covariance in the smoother's backward pass",
            })?;
        let gain = p * f.transpose() * inverse;
        let predicted_x = f * forward[k].state;
        out[k].state = forward[k].state + gain * (out[k + 1].state - predicted_x);
        let dp = out[k + 1].covariance - predicted_p;
        let smoothed_p = p + gain * dp * gain.transpose();
        out[k].covariance = (smoothed_p + smoothed_p.transpose()) * 0.5;
    }
    Ok(out)
}

/// What this crate cannot do yet, named rather than panicked (GAP-082).
///
/// **Returning an empty smoothed trajectory would have been the worst available answer**:
/// a smoother that returns no states reads as a track that was never there, and this is
/// the crate whose output the whole picture rests on.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FilterError {
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
    /// A covariance that should have had a Cholesky factor did not, so the sigma points
    /// a UKF needs do not exist. Reported rather than worked around: a covariance that
    /// cannot be factored is a broken filter state, and a shifted or clipped one would
    /// hide the break behind a plausible number.
    #[error("{what} is not positive definite, so it has no Cholesky factor")]
    NotPositiveDefinite { what: &'static str },
    /// The innovation covariance could not be inverted, so the measurement carries no
    /// information this filter can use.
    #[error("the innovation covariance is singular; this measurement cannot be applied")]
    SingularInnovation,
    /// An IMM was given fewer than two modes. One mode is a Kalman filter with extra
    /// bookkeeping and the mixing step is undefined, so this is refused rather than
    /// degenerating quietly into a filter the caller did not ask for.
    #[error("an IMM needs at least two modes; {modes} were given")]
    TooFewModes { modes: usize },
    /// A mode-transition matrix that is not row-stochastic over the mode set.
    ///
    /// **Not renormalised.** A row that does not sum to one is a matrix somebody built
    /// wrongly -- most often transposed, or normalised down the wrong axis -- and
    /// scaling it produces a filter that runs and switches modes in a way nobody
    /// specified.
    #[error("the mode-transition matrix is malformed: {what}")]
    MalformedTransitionMatrix { what: &'static str },
    /// Initial mode probabilities that are not a distribution over the mode set.
    #[error("the mode probabilities are malformed: {what}")]
    MalformedModeProbabilities { what: &'static str },
    /// A particle filter whose weights all underflowed: every particle is inconsistent
    /// with the measurement, so there is no posterior to report.
    ///
    /// **Reported rather than resampled from a uniform.** Uniform weights would say the
    /// particles are equally good when the measurement said they are all wrong, and the
    /// filter would carry on drawing a confident cloud around the wrong place.
    #[error("every particle weight underflowed; the measurement rules out the whole cloud")]
    ParticleDegeneracy,
    /// A particle filter built with no particles, or with a non-finite initial state.
    #[error("the particle cloud is malformed: {what}")]
    MalformedParticleCloud { what: &'static str },
}

/// Radar-style nonlinear measurement: range (m), bearing (rad), elevation (rad).
pub struct RangeBearingElevation {
    pub range_m: f64,
    pub bearing_rad: f64,
    pub elevation_rad: f64,
    pub origin: Ecef,
}
