// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The sequential-importance-resampling particle filter: the
//! verification-capability-table.md §1 row "`filters` | Particle Filter".
//!
//! Oracle: a reference SIR filter in Python built on `filterpy.monte_carlo`'s
//! resamplers, criterion "statistical comparison across N trials, mean/variance within
//! 2σ". §2 records the oracle as Python-only, and it has to be: a particle filter's
//! output depends on its random draws, so two implementations in two languages cannot
//! be compared step for step the way the Kalman rows are. What *can* be compared is the
//! distribution each converges to, over many independent trials, and that is what the
//! row asks for and what `gungnir-filters/tests/particle_diff.rs` does.
//!
//! # Why a particle filter is here at all
//!
//! Every other filter in this crate reports a Gaussian. A particle filter does not: it
//! carries a weighted cloud, so it can hold "the target went left **or** right" as two
//! lumps rather than averaging them into a confident estimate of the gap between them.
//! That is the case it earns its cost in, and it is why [`ParticleFilter::mean`] is
//! documented as a lossy summary rather than the filter's answer.
//!
//! # Why the noise factor is an eigendecomposition and not a Cholesky
//!
//! Sampling process noise means drawing from `N(0, Q)`, which usually means multiplying
//! a standard normal draw by a Cholesky factor of `Q`. That works for *this*
//! workspace's constant-velocity model and it is worth being precise about why, because
//! the obvious reason is wrong.
//!
//! There are two constant-velocity process-noise matrices in common use. The
//! **discrete** white-noise-acceleration form, `σ²[[dt⁴/4, dt³/2], [dt³/2, dt²]]`, is
//! singular: its determinant is `dt⁶/4 − dt⁶/4 = 0`, because one acceleration sample
//! drives both position and velocity. The **continuous** form,
//! `σ²[[dt³/3, dt²/2], [dt²/2, dt]]`, is not: its determinant is `dt⁴/12 > 0`.
//! `gungnir_core::ConstantVelocity` uses the continuous form, so its `Q` is positive
//! definite and has a perfectly good Cholesky factor. An earlier draft of this module
//! asserted the opposite and the test below caught it.
//!
//! The eigendecomposition is still the right choice, for two reasons that survive that
//! correction. First, `psd_factor` is applied to caller-supplied covariances as well --
//! the initial `P` here and in the square-root filter, and that `R` -- and those are
//! only ever required to be positive *semi*-definite. A track initiated with no
//! uncertainty at all in one axis is a legitimate initial covariance and a singular one.
//! Second, a factor routine that fails on a semi-definite input fails on a valid input,
//! and it would fail deep inside a predict where the honest report is hard to make.
//!
//! `Q = V Λ Vᵀ` gives `A = V √Λ` with `A Aᵀ = Q` for any PSD `Q`. Negative eigenvalues
//! -- which on a matrix that is PSD by construction can only be rounding -- are clamped
//! to zero rather than having their absolute value taken, because a negative variance is
//! not a small positive one.
//!
//! # Randomness is the caller's
//!
//! Every stochastic method takes `&mut impl Rng`, per `agentic-coding-standards.md`
//! §2.4. Nothing in this module calls `thread_rng`. That is what lets the differential
//! test seed a fixed generator and get the same cloud twice, and it is why the filter
//! can be replayed from a journal at all.

use crate::FilterError;
use gungnir_core::MotionModel;
use nalgebra::{SMatrix, SVector};
use rand::Rng;
use rand_distr::{Distribution, StandardNormal};

/// Resample when the effective sample size falls below this fraction of the cloud.
///
/// The standard choice, and the one the reference filter uses. Resampling every step
/// throws away diversity for nothing; never resampling lets one particle take all the
/// weight and the cloud stops representing anything.
const RESAMPLE_FRACTION: f64 = 0.5;

/// How a depleted cloud is replaced.
///
/// All three draw the same expected number of copies of each particle and differ only
/// in variance. [`ResampleStrategy::Systematic`] is the default because it has the
/// lowest resampling variance of the three and is what the reference filter uses, so
/// the comparison is between two filters rather than between two resamplers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResampleStrategy {
    /// One uniform draw, then equally spaced positions along the cumulative weights.
    #[default]
    Systematic,
    /// One uniform draw per stratum of the cumulative weights.
    Stratified,
    /// One independent uniform draw per particle. Highest variance; kept because it is
    /// the textbook definition and the other two are judged against it.
    Multinomial,
}

/// Sequential Monte Carlo filter over a fixed-size cloud of `N`-dimensional states.
///
/// `PARTICLES` is a runtime length rather than a const parameter: the cloud size is a
/// tuning decision a deployment makes, and `agentic-coding-standards.md` §2.1 reserves
/// fixed-size types for the state, which this still uses.
#[derive(Debug, Clone)]
pub struct ParticleFilter<Motion, const N: usize, const M: usize>
where
    Motion: MotionModel<N>,
{
    particles: Vec<SVector<f64, N>>,
    weights: Vec<f64>,
    motion: Motion,
    h: SMatrix<f64, M, N>,
    r: SMatrix<f64, M, M>,
    strategy: ResampleStrategy,
    resamples: u32,
}

impl<Motion, const N: usize, const M: usize> ParticleFilter<Motion, N, M>
where
    Motion: MotionModel<N>,
{
    /// Build a filter by drawing `count` particles from `N(x0, p0)`.
    ///
    /// # Errors
    ///
    /// [`FilterError::MalformedParticleCloud`] for an empty cloud or a non-finite
    /// initial estimate. [`FilterError::NotPositiveDefinite`] if `p0` has a negative
    /// eigenvalue beyond rounding, which means the caller's initial covariance is not
    /// one.
    pub fn new<R: Rng + ?Sized>(
        count: usize,
        x0: SVector<f64, N>,
        p0: SMatrix<f64, N, N>,
        motion: Motion,
        h: SMatrix<f64, M, N>,
        r: SMatrix<f64, M, M>,
        rng: &mut R,
    ) -> Result<Self, FilterError> {
        if count == 0 {
            return Err(FilterError::MalformedParticleCloud {
                what: "a cloud of zero particles represents nothing",
            });
        }
        if x0.iter().any(|v| !v.is_finite()) || p0.iter().any(|v| !v.is_finite()) {
            return Err(FilterError::MalformedParticleCloud {
                what: "the initial estimate is not finite",
            });
        }
        let spread = psd_factor(&p0, "the initial covariance")?;
        let particles = (0..count)
            .map(|_| x0 + spread * standard_normal(rng))
            .collect();
        Ok(Self {
            particles,
            weights: vec![1.0 / lossless(count); count],
            motion,
            h,
            r,
            strategy: ResampleStrategy::default(),
            resamples: 0,
        })
    }

    /// Choose the resampler. See [`ResampleStrategy`].
    #[must_use]
    pub fn with_strategy(mut self, strategy: ResampleStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// How many particles the cloud holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    /// Always false: a cloud of zero particles cannot be constructed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// The particles, for a consumer that needs the cloud rather than its mean.
    #[must_use]
    pub fn particles(&self) -> &[SVector<f64, N>] {
        &self.particles
    }

    /// The normalised importance weights, in the same order as [`Self::particles`].
    #[must_use]
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// How many times the cloud has been resampled.
    #[must_use]
    pub fn resamples(&self) -> u32 {
        self.resamples
    }

    /// `1 / Σ wᵢ²`: roughly how many particles are actually carrying the distribution.
    ///
    /// Equal to the cloud size when every weight is equal and falls toward one as the
    /// weight concentrates. This is the number the resampling decision is made on, and
    /// it is worth exposing because a cloud whose effective size has collapsed is
    /// reporting a mean with far less support than its particle count suggests.
    #[must_use]
    pub fn effective_sample_size(&self) -> f64 {
        let sum_sq: f64 = self.weights.iter().map(|w| w * w).sum();
        if sum_sq > 0.0 {
            1.0 / sum_sq
        } else {
            0.0
        }
    }

    /// The weighted mean of the cloud.
    ///
    /// **A lossy summary, not the filter's answer.** The point of a particle filter is
    /// that the posterior need not be unimodal; the mean of a two-lump cloud sits in
    /// the gap between the lumps, where the target certainly is not. A consumer that
    /// can use the cloud should read [`Self::particles`].
    #[must_use]
    pub fn mean(&self) -> SVector<f64, N> {
        let mut m = SVector::<f64, N>::zeros();
        for (p, w) in self.particles.iter().zip(&self.weights) {
            m += p * *w;
        }
        m
    }

    /// The weighted covariance of the cloud about its mean.
    #[must_use]
    pub fn covariance(&self) -> SMatrix<f64, N, N> {
        let mean = self.mean();
        let mut c = SMatrix::<f64, N, N>::zeros();
        for (p, w) in self.particles.iter().zip(&self.weights) {
            let d = p - mean;
            c += d * d.transpose() * *w;
        }
        (c + c.transpose()) * 0.5
    }

    /// Propagate every particle through the motion model and its process noise.
    ///
    /// # Errors
    ///
    /// [`FilterError::NotPositiveDefinite`] if the motion model returns a `Q` that is
    /// not positive semi-definite, which is a broken motion model rather than a
    /// condition this filter can recover from.
    pub fn predict<R: Rng + ?Sized>(&mut self, dt: f64, rng: &mut R) -> Result<(), FilterError> {
        let f = self.motion.f(dt);
        let q = self.motion.q(dt);
        let spread = psd_factor(&q, "the process-noise covariance")?;
        for particle in &mut self.particles {
            *particle = f * *particle + spread * standard_normal(rng);
        }
        Ok(())
    }

    /// Reweight the cloud by the likelihood of `z`, then resample if it has collapsed.
    ///
    /// # Errors
    ///
    /// [`FilterError::ParticleDegeneracy`] when every weight underflows, meaning the
    /// measurement is inconsistent with every particle. See the variant's own
    /// documentation for why that is reported rather than smoothed over.
    ///
    /// [`FilterError::SingularInnovation`] when `R` cannot be inverted.
    pub fn update<R: Rng + ?Sized>(
        &mut self,
        z: &SVector<f64, M>,
        rng: &mut R,
    ) -> Result<(), FilterError> {
        let r_inv = self
            .r
            .try_inverse()
            .ok_or(FilterError::SingularInnovation)?;
        // Weights are formed in the log domain and shifted by their maximum before
        // exponentiating. Forming them directly underflows to zero for every particle
        // as soon as the cloud is a few standard deviations from the measurement, which
        // on a 3-dimensional measurement happens routinely rather than exceptionally --
        // the filter would report degeneracy on a perfectly healthy cloud.
        let mut log_weights = Vec::with_capacity(self.particles.len());
        for (particle, prior) in self.particles.iter().zip(&self.weights) {
            let y = z - self.h * particle;
            let quadratic = (y.transpose() * r_inv * y)[(0, 0)];
            log_weights.push(prior.ln() - 0.5 * quadratic);
        }
        let peak = log_weights
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(f64::NEG_INFINITY, f64::max);
        if !peak.is_finite() {
            return Err(FilterError::ParticleDegeneracy);
        }
        let mut total = 0.0;
        for (w, log_w) in self.weights.iter_mut().zip(&log_weights) {
            *w = (log_w - peak).exp();
            if !w.is_finite() {
                *w = 0.0;
            }
            total += *w;
        }
        if total <= 0.0 || !total.is_finite() {
            return Err(FilterError::ParticleDegeneracy);
        }
        for w in &mut self.weights {
            *w /= total;
        }

        if self.effective_sample_size() < RESAMPLE_FRACTION * lossless(self.particles.len()) {
            self.resample(rng);
        }
        Ok(())
    }

    /// Replace the cloud with a uniformly weighted draw from itself.
    fn resample<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        let count = self.particles.len();
        let n = lossless(count);
        let positions: Vec<f64> = match self.strategy {
            ResampleStrategy::Systematic => {
                let offset: f64 = rng.gen();
                (0..count).map(|i| (lossless(i) + offset) / n).collect()
            }
            ResampleStrategy::Stratified => (0..count)
                .map(|i| {
                    let offset: f64 = rng.gen();
                    (lossless(i) + offset) / n
                })
                .collect(),
            ResampleStrategy::Multinomial => {
                let mut draws: Vec<f64> = (0..count).map(|_| rng.gen()).collect();
                draws.sort_by(f64::total_cmp);
                draws
            }
        };

        let mut cumulative = Vec::with_capacity(count);
        let mut running = 0.0;
        for w in &self.weights {
            running += *w;
            cumulative.push(running);
        }
        // Guard the last edge against the rounding that makes a cumulative sum of
        // normalised weights end at 0.999... : a position past the end would otherwise
        // fall off the cloud.
        if let Some(last) = cumulative.last_mut() {
            *last = 1.0;
        }

        let mut resampled = Vec::with_capacity(count);
        let mut source = 0;
        for position in positions {
            while source + 1 < count && cumulative[source] < position {
                source += 1;
            }
            resampled.push(self.particles[source]);
        }
        self.particles = resampled;
        self.weights = vec![1.0 / n; count];
        self.resamples = self.resamples.saturating_add(1);
    }
}

/// `usize` to `f64` for counts that are always far below 2^53.
#[allow(clippy::cast_precision_loss)]
fn lossless(n: usize) -> f64 {
    n as f64
}

/// A standard normal draw in `N` dimensions.
fn standard_normal<R: Rng + ?Sized, const N: usize>(rng: &mut R) -> SVector<f64, N> {
    SVector::<f64, N>::from_fn(|_, _| StandardNormal.sample(rng))
}

/// A matrix `A` with `A Aᵀ = m`, for any positive semi-definite `m`.
///
/// See the module documentation: a Cholesky factor does not exist for the process noise
/// of a constant-velocity model, so this goes through a symmetric eigendecomposition.
///
/// The decomposition runs on a `DMatrix` because nalgebra's `symmetric_eigen` is not
/// available on a matrix whose size is a *generic* const parameter -- it needs
/// `Const<N>: DimSub<U1>`, which holds for each concrete size and not for a bare `N`.
/// The scratch matrix is built and dropped inside this function and the result is
/// fixed-size, so nothing dynamic escapes; see the `sqrt` module for the same note about
/// `agentic-coding-standards.md` §2.1.
///
/// # Errors
///
/// [`FilterError::NotPositiveDefinite`] if an eigenvalue is negative beyond what
/// rounding on a PSD matrix explains.
pub(crate) fn psd_factor<const N: usize>(
    m: &SMatrix<f64, N, N>,
    what: &'static str,
) -> Result<SMatrix<f64, N, N>, FilterError> {
    let scale = m.abs().max().max(1.0);
    let tolerance = -1e-12 * scale;
    let symmetric =
        nalgebra::DMatrix::<f64>::from_fn(N, N, |r, c| f64::midpoint(m[(r, c)], m[(c, r)]));
    let eigen = symmetric.symmetric_eigen();
    if eigen.eigenvalues.iter().any(|v| *v < tolerance) {
        return Err(FilterError::NotPositiveDefinite { what });
    }
    Ok(SMatrix::<f64, N, N>::from_fn(|r, c| {
        eigen.eigenvectors[(r, c)] * eigen.eigenvalues[c].max(0.0).sqrt()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_core::ConstantVelocity;
    use rand::{rngs::StdRng, SeedableRng};

    fn position_h() -> SMatrix<f64, 3, 6> {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        h
    }

    fn filter(count: usize, rng: &mut StdRng) -> ParticleFilter<ConstantVelocity, 6, 3> {
        ParticleFilter::new(
            count,
            SVector::<f64, 6>::zeros(),
            SMatrix::<f64, 6, 6>::identity() * 100.0,
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            SMatrix::<f64, 3, 3>::identity() * 25.0,
            rng,
        )
        .expect("a well-formed cloud")
    }

    /// The distinction the module documentation draws, pinned in both directions.
    ///
    /// This test exists because an earlier draft of this module claimed the workspace's
    /// `Q` was singular and had no Cholesky factor. It is not and it does. The singular
    /// one is the *discrete* white-noise-acceleration form, which `gungnir-core` does
    /// not use. Both are checked here so the claim in the documentation cannot drift
    /// back to the wrong one.
    #[test]
    fn the_continuous_form_is_definite_and_the_discrete_form_is_not() {
        let q = ConstantVelocity { sigma_a_sq: 1.0 }.q(1.0);
        assert!(
            q.cholesky().is_some(),
            "the continuous-form Q lost its Cholesky factor, so gungnir-core's motion \
             model changed and this module's rationale must be re-read"
        );
        assert!(psd_factor(&q, "test").is_ok());

        // The discrete form, for one axis, written out: this is the matrix the
        // documentation says is singular.
        let dt = 1.0_f64;
        let discrete = SMatrix::<f64, 2, 2>::new(
            dt.powi(4) / 4.0,
            dt.powi(3) / 2.0,
            dt.powi(3) / 2.0,
            dt * dt,
        );
        assert!(
            discrete.determinant().abs() < 1e-15,
            "the discrete form is supposed to be singular"
        );
        assert!(
            psd_factor(&discrete, "test").is_ok(),
            "the eigendecomposition must factor a singular PSD matrix, which is the \
             case it is chosen for"
        );
    }

    /// The case the eigendecomposition is actually chosen for: a legitimate initial
    /// covariance with no uncertainty at all in one direction.
    #[test]
    fn a_rank_deficient_initial_covariance_is_accepted() {
        let mut rng = StdRng::seed_from_u64(5);
        let mut p0 = SMatrix::<f64, 6, 6>::identity() * 100.0;
        p0[(2, 2)] = 0.0;
        let pf = ParticleFilter::<ConstantVelocity, 6, 3>::new(
            64,
            SVector::<f64, 6>::zeros(),
            p0,
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            SMatrix::<f64, 3, 3>::identity() * 25.0,
            &mut rng,
        )
        .expect("a semi-definite initial covariance is valid");
        assert!(
            pf.particles().iter().all(|p| p[2].abs() < 1e-12),
            "the zero-variance axis picked up spread from somewhere"
        );
    }

    #[test]
    fn the_factor_reproduces_the_matrix() {
        let q = ConstantVelocity { sigma_a_sq: 3.0 }.q(0.5);
        let a = psd_factor(&q, "test").expect("PSD");
        let error = (a * a.transpose() - q).abs().max();
        assert!(error < 1e-12, "A Aᵀ differed from Q by {error}");
    }

    #[test]
    fn an_indefinite_covariance_is_refused() {
        let mut bad = SMatrix::<f64, 6, 6>::identity();
        bad[(0, 0)] = -1.0;
        assert_eq!(
            psd_factor(&bad, "test").unwrap_err(),
            FilterError::NotPositiveDefinite { what: "test" }
        );
    }

    #[test]
    fn an_empty_cloud_is_refused() {
        let mut rng = StdRng::seed_from_u64(1);
        let err = ParticleFilter::<ConstantVelocity, 6, 3>::new(
            0,
            SVector::<f64, 6>::zeros(),
            SMatrix::<f64, 6, 6>::identity(),
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            SMatrix::<f64, 3, 3>::identity(),
            &mut rng,
        )
        .unwrap_err();
        assert_eq!(
            err,
            FilterError::MalformedParticleCloud {
                what: "a cloud of zero particles represents nothing"
            }
        );
    }

    #[test]
    fn an_update_pulls_the_cloud_toward_the_measurement_and_shrinks_it() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut pf = filter(4000, &mut rng);
        let before = pf.covariance().trace();
        pf.predict(1.0, &mut rng).expect("PSD");
        pf.update(&SVector::<f64, 3>::new(30.0, 0.0, 0.0), &mut rng)
            .expect("a live cloud");
        assert!(pf.mean()[0] > 0.0, "the cloud did not move toward 30 m");
        assert!(pf.mean()[0] < 30.0, "the cloud overshot the measurement");
        assert!(
            pf.covariance().trace() < before,
            "the cloud did not tighten after a measurement"
        );
    }

    /// The property that makes a particle filter worth its cost: a cloud that is
    /// genuinely bimodal must stay bimodal, rather than collapsing onto the empty middle
    /// the way a Gaussian filter's mean does -- while still discarding a lump the
    /// measurement really does rule out.
    ///
    /// **The measurement has to be genuinely ambiguous for this to test anything.** An
    /// earlier version of this test put both lumps a hundred standard deviations from
    /// the measurement and then asserted both survived. They did not, and they should
    /// not have: when a measurement rules out the entire cloud, weight concentrates on
    /// whichever few particles are least impossible and resampling copies them. That is
    /// a correct SIR filter degenerating, and asserting otherwise would have been a test
    /// demanding wrong behaviour. Here the two near lumps are about three sigma out on
    /// either side, which is ambiguous, and the far group is fifty sigma out, which is
    /// not.
    #[test]
    fn a_bimodal_cloud_keeps_both_plausible_lumps_and_drops_the_impossible_one() {
        let mut rng = StdRng::seed_from_u64(11);
        let count = 6000;
        let mut pf = ParticleFilter::<ConstantVelocity, 6, 3>::new(
            count,
            SVector::<f64, 6>::zeros(),
            SMatrix::<f64, 6, 6>::identity() * 100.0,
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            SMatrix::<f64, 3, 3>::identity() * 400.0,
            &mut rng,
        )
        .expect("a well-formed cloud");
        // One sixth at -60 m, one sixth at +60 m, two thirds at +2000 m. Zero the other
        // components so the only thing separating the groups is the axis under test.
        for (i, particle) in pf.particles.iter_mut().enumerate() {
            *particle = SVector::<f64, 6>::zeros();
            particle[0] = match i % 6 {
                0 => -60.0,
                1 => 60.0,
                _ => 2000.0,
            };
        }
        pf.predict(1.0, &mut rng).expect("PSD");
        pf.update(&SVector::<f64, 3>::new(0.0, 0.0, 0.0), &mut rng)
            .expect("a live cloud");

        let left = pf.particles().iter().filter(|p| p[0] < -20.0).count();
        let right = pf
            .particles()
            .iter()
            .filter(|p| p[0] > 20.0 && p[0] < 1000.0)
            .count();
        let far = pf.particles().iter().filter(|p| p[0] > 1000.0).count();
        assert!(
            left > count / 10 && right > count / 10,
            "a plausible lump was lost: {left} left, {right} right"
        );
        assert_eq!(far, 0, "the ruled-out group survived resampling");
        assert!(
            pf.resamples() >= 1,
            "the cloud never resampled, so this test did not exercise the resampler"
        );
        // And the headline: the mean sits between the lumps, where nothing is. This is
        // the number a Gaussian filter would report as its answer.
        assert!(
            pf.mean()[0].abs() < 20.0,
            "the mean was expected in the empty middle, and was {}",
            pf.mean()[0]
        );
    }

    /// A measurement no particle can explain must be reported, not resampled away.
    #[test]
    fn a_measurement_that_rules_out_every_particle_is_reported() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut pf = filter(200, &mut rng);
        for particle in &mut pf.particles {
            particle[0] = f64::NAN;
        }
        let err = pf
            .update(&SVector::<f64, 3>::new(0.0, 0.0, 0.0), &mut rng)
            .unwrap_err();
        assert_eq!(err, FilterError::ParticleDegeneracy);
    }

    /// Every resampler must be unbiased: a particle with twice the weight must be drawn
    /// about twice as often. This is the property the three strategies share and the
    /// only one that makes them interchangeable.
    #[test]
    fn every_resampler_draws_in_proportion_to_weight() {
        for strategy in [
            ResampleStrategy::Systematic,
            ResampleStrategy::Stratified,
            ResampleStrategy::Multinomial,
        ] {
            let mut rng = StdRng::seed_from_u64(29);
            let mut pf = filter(3, &mut rng).with_strategy(strategy);
            pf.particles[0][0] = 0.0;
            pf.particles[1][0] = 1.0;
            pf.particles[2][0] = 2.0;
            pf.weights = vec![1.0 / 6.0, 2.0 / 6.0, 3.0 / 6.0];
            let mut counts = [0_u32; 3];
            for _ in 0..6000 {
                let mut copy = pf.clone();
                copy.resample(&mut rng);
                for particle in copy.particles() {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let which = particle[0].round() as usize;
                    counts[which] += 1;
                }
            }
            let total = f64::from(counts.iter().sum::<u32>());
            for (i, expected) in [1.0 / 6.0, 2.0 / 6.0, 3.0 / 6.0].into_iter().enumerate() {
                let seen = f64::from(counts[i]) / total;
                assert!(
                    (seen - expected).abs() < 0.02,
                    "{strategy:?} drew particle {i} {seen} of the time, expected {expected}"
                );
            }
        }
    }

    /// Resampling must fire when the cloud collapses, and the effective sample size
    /// must recover when it does.
    #[test]
    fn a_collapsed_cloud_is_resampled() {
        let mut rng = StdRng::seed_from_u64(13);
        let mut pf = filter(2000, &mut rng);
        assert_eq!(pf.resamples(), 0);
        // A measurement far from the prior concentrates the weight on the few particles
        // nearest it, which is exactly the collapse resampling exists for.
        pf.predict(1.0, &mut rng).expect("PSD");
        pf.update(&SVector::<f64, 3>::new(25.0, 25.0, 25.0), &mut rng)
            .expect("a live cloud");
        assert!(
            pf.resamples() >= 1,
            "the collapse did not trigger a resample"
        );
        assert!(
            pf.effective_sample_size() > 0.9 * lossless(pf.len()),
            "resampling did not restore the effective sample size"
        );
    }
}
