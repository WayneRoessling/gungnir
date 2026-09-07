//! The Interacting Multiple Model filter: the verification-capability-table.md §1 row
//! "`filters` | Interacting Multiple Model (IMM)".
//!
//! # The oracle this row was planned against does not exist
//!
//! `docs/verification-capability-table.md` §2 planned this row against "Stone Soup
//! `IMM`". **Stone Soup 1.9.1, the version pinned in `.venv-oracles`, has no IMM**:
//! there is no `IMM` symbol anywhere in the installed package, and no
//! `stonesoup.predictor`/`updater` module implementing one. The row is therefore gated
//! against **`filterpy.kalman.IMMEstimator` 1.4.5** instead, which is an independent
//! third-party implementation of the same Blom--Bar-Shalom recursion and is the oracle
//! already trusted by four other rows in this table.
//!
//! The substitution is recorded rather than quietly made, and the criterion is
//! unchanged: state within 1e-4 and mode probabilities within 1e-3, exactly what §2
//! asked for. Widening either to make a different oracle fit would be the move
//! `CLAUDE.md` rules out.
//!
//! # The recursion, and why the order matters
//!
//! One cycle of the IMM is four steps, and this module runs them in the order
//! `IMMEstimator` does, because a differential test against an oracle that mixes at a
//! different point in the cycle compares two different algorithms:
//!
//! 1. **Mixing**, at the top of [`Imm::predict`]. Each mode starts its prediction not
//!    from its own posterior but from a blend of every mode's posterior, weighted by
//!    the probability that the target was in mode `i` given that it is now in mode
//!    `j`. This is the step that makes an IMM more than a bank of independent filters:
//!    a mode that has been idle inherits the state the active mode learned, so it is
//!    ready the instant the target switches.
//! 2. **Model-conditioned filtering**: every mode predicts and updates on the same
//!    measurement.
//! 3. **Mode-probability update** from each mode's own measurement likelihood.
//! 4. **Combination** into one Gaussian for the consumer.
//!
//! The mixing weights used by step 1 are the ones computed at the end of the previous
//! step 3, which is why [`Imm::new`] computes them once before any cycle runs.
//!
//! # What the combined covariance means, and what it does not
//!
//! The combined `P` includes the spread *between* the modes,
//! `Σ μⱼ [Pⱼ + (xⱼ − x)(xⱼ − x)ᵀ]`, not just the average of their covariances. During a
//! manoeuvre the modes disagree, the spread term grows, and the reported uncertainty
//! grows with it. That is the honest answer and it is the reason an IMM is worth the
//! cost, but it means the combined Gaussian is a moment-matched summary of a mixture
//! and not the mixture itself. A consumer that needs the modes -- a gate that should
//! not swallow a turning target, say -- should read [`Imm::mode_probabilities`] and the
//! per-mode estimates rather than treating this one Gaussian as the whole belief.

use crate::{Filter, FilterError};
use gungnir_core::{assert_psd, MotionModel};
use nalgebra::{SMatrix, SVector};

/// Rows of a transition matrix, and mode probabilities, must sum to one within this.
/// Loose enough for a matrix written down in decimal (0.97 + 0.03), tight enough that a
/// row summing to 0.9 or 1.1 -- a typo, or a matrix built by normalising the wrong axis
/// -- is refused rather than silently renormalised.
const STOCHASTIC_TOLERANCE: f64 = 1e-9;

/// One mode of an [`Imm`]: a filter whose estimate the IMM can read, replace, and step.
///
/// This exists because an IMM's modes have **different motion models and therefore
/// different Rust types** -- constant velocity and coordinated turn are the pair this
/// system's baselines name -- so they cannot be held in a `Vec` of one concrete filter
/// type. The trait is deliberately narrow: it is the operations mixing needs and
/// nothing else.
pub trait ModeFilter<const N: usize, const M: usize> {
    /// Advance the estimate by `dt`.
    fn predict(&mut self, dt: f64);

    /// Apply `z`, returning the Gaussian likelihood of the innovation **under the
    /// prior**, `N(z − H x⁻; 0, S)`.
    ///
    /// The likelihood must be evaluated before the update is applied, because it is
    /// the probability of having seen this measurement given the state the mode
    /// predicted, and after the update the mode has already moved toward it. Computing
    /// it afterwards would make every mode look equally good and the mode
    /// probabilities would never move.
    fn update_with_likelihood(&mut self, z: &SVector<f64, M>) -> f64;

    /// The current estimate.
    fn state(&self) -> &SVector<f64, N>;

    /// The current estimate covariance.
    fn covariance(&self) -> &SMatrix<f64, N, N>;

    /// Replace the estimate, which is what mixing does to every mode each cycle.
    fn set_estimate(&mut self, x: SVector<f64, N>, p: SMatrix<f64, N, N>);
}

/// `N(y; 0, s)`, the innovation likelihood every mode is weighted by.
///
/// Floored at [`f64::MIN_POSITIVE`] rather than allowed to reach zero. A mode whose
/// likelihood underflows is one the measurement rules out, and zero would make the
/// normalisation in [`Imm::update`] divide by zero the moment *every* mode was ruled
/// out at once -- which happens on a real outlier, not only in theory. The floor keeps
/// the relative ordering of ruled-out modes and lets the filter carry on reporting an
/// estimate it can label rather than a NaN it cannot. This is what `filterpy` does with
/// `sys.float_info.min`, so matching it is also what the oracle comparison requires.
fn innovation_likelihood<const M: usize>(y: &SVector<f64, M>, s: &SMatrix<f64, M, M>) -> f64 {
    // Both the determinant and the inverse come from one Cholesky factor. `S` is an
    // innovation covariance, so it is positive definite for any valid filter state and
    // the factor exists; taking `determinant()` directly is not available on a matrix
    // whose size is a generic const parameter, and the factor is the cheaper route in
    // any case. A `None` here means the mode's covariance is already broken, which is
    // the same situation as an underflowed likelihood: the mode explains nothing.
    let Some(chol) = s.cholesky() else {
        return f64::MIN_POSITIVE;
    };
    let det = chol.l().diagonal().iter().map(|d| d * d).product::<f64>();
    if det <= 0.0 || !det.is_finite() {
        return f64::MIN_POSITIVE;
    }
    let quadratic = (y.transpose() * chol.inverse() * y)[(0, 0)];
    #[allow(clippy::cast_precision_loss)]
    let m = M as f64;
    let normaliser = ((2.0 * std::f64::consts::PI).powf(m) * det).sqrt();
    let value = (-0.5 * quadratic).exp() / normaliser;
    if value.is_finite() && value > f64::MIN_POSITIVE {
        value
    } else {
        f64::MIN_POSITIVE
    }
}

impl<Model, const N: usize, const M: usize> ModeFilter<N, M> for crate::KalmanFilter<Model, N, M>
where
    Model: MotionModel<N>,
{
    fn predict(&mut self, dt: f64) {
        Filter::predict(self, dt);
    }

    fn update_with_likelihood(&mut self, z: &SVector<f64, M>) -> f64 {
        let y = self.innovation(z);
        let s = self.innovation_covariance();
        let likelihood = innovation_likelihood(&y, &s);
        Filter::update(self, z);
        likelihood
    }

    fn state(&self) -> &SVector<f64, N> {
        Filter::state(self)
    }

    fn covariance(&self) -> &SMatrix<f64, N, N> {
        crate::KalmanFilter::covariance(self)
    }

    fn set_estimate(&mut self, x: SVector<f64, N>, p: SMatrix<f64, N, N>) {
        crate::KalmanFilter::set_estimate(self, x, p);
    }
}

/// Interacting Multiple Model filter over a set of [`ModeFilter`]s.
///
/// See the module documentation for the recursion and for the oracle this row is
/// gated against.
pub struct Imm<const N: usize, const M: usize> {
    modes: Vec<Box<dyn ModeFilter<N, M>>>,
    /// Row-major `modes × modes`; `transition[i * n + j]` is `P(mode j now | mode i before)`.
    transition: Vec<f64>,
    mode_probabilities: Vec<f64>,
    /// Row-major mixing weights `ω[i][j]`, recomputed after every mode-probability update.
    mixing: Vec<f64>,
    /// Normalising constants `c̄ⱼ = Σᵢ pᵢⱼ μᵢ`, carried between update and the next one.
    cbar: Vec<f64>,
    likelihoods: Vec<f64>,
    x: SVector<f64, N>,
    p: SMatrix<f64, N, N>,
}

impl<const N: usize, const M: usize> std::fmt::Debug for Imm<N, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Imm")
            .field("modes", &self.modes.len())
            .field("mode_probabilities", &self.mode_probabilities)
            .field("state", &self.x)
            .finish_non_exhaustive()
    }
}

impl<const N: usize, const M: usize> Imm<N, M> {
    /// Build an IMM from its modes, their initial probabilities, and the mode-transition
    /// matrix.
    ///
    /// `transition` is row-major and `transition[i][j]` is the probability of being in
    /// mode `j` now given mode `i` at the previous step, which is the convention
    /// `filterpy` and every textbook statement of the recursion use. Getting it
    /// transposed produces a filter that switches modes in the wrong direction and
    /// still looks plausible, so the orientation is stated here and pinned by a test.
    ///
    /// # Errors
    ///
    /// [`FilterError::TooFewModes`] below two modes: one mode is a Kalman filter with
    /// extra bookkeeping, and the mixing step is undefined.
    ///
    /// [`FilterError::MalformedTransitionMatrix`] when the matrix is not square, is not
    /// the size of the mode set, holds a value outside `[0, 1]` or a non-finite one, or
    /// has a row that does not sum to one. **Not renormalised**: a row that does not sum
    /// to one is a matrix somebody built wrongly, and quietly scaling it produces a
    /// filter that runs and is not the one that was specified.
    ///
    /// [`FilterError::MalformedModeProbabilities`] on the same grounds for the initial
    /// probabilities.
    pub fn new(
        modes: Vec<Box<dyn ModeFilter<N, M>>>,
        mode_probabilities: &[f64],
        transition: &[Vec<f64>],
    ) -> Result<Self, FilterError> {
        let n = modes.len();
        if n < 2 {
            return Err(FilterError::TooFewModes { modes: n });
        }
        if transition.len() != n {
            return Err(FilterError::MalformedTransitionMatrix {
                what: "the matrix does not have one row per mode",
            });
        }
        for row in transition {
            if row.len() != n {
                return Err(FilterError::MalformedTransitionMatrix {
                    what: "a row does not have one entry per mode",
                });
            }
            if row.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0) {
                return Err(FilterError::MalformedTransitionMatrix {
                    what: "an entry is not a probability",
                });
            }
            let sum: f64 = row.iter().sum();
            if (sum - 1.0).abs() > STOCHASTIC_TOLERANCE {
                return Err(FilterError::MalformedTransitionMatrix {
                    what: "a row does not sum to one",
                });
            }
        }
        if mode_probabilities.len() != n {
            return Err(FilterError::MalformedModeProbabilities {
                what: "there is not one probability per mode",
            });
        }
        if mode_probabilities
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0)
        {
            return Err(FilterError::MalformedModeProbabilities {
                what: "an entry is not a probability",
            });
        }
        let sum: f64 = mode_probabilities.iter().sum();
        if (sum - 1.0).abs() > STOCHASTIC_TOLERANCE {
            return Err(FilterError::MalformedModeProbabilities {
                what: "the probabilities do not sum to one",
            });
        }

        let mut imm = Self {
            modes,
            transition: transition.iter().flatten().copied().collect(),
            mode_probabilities: mode_probabilities.to_vec(),
            mixing: vec![0.0; n * n],
            cbar: vec![0.0; n],
            likelihoods: vec![0.0; n],
            x: SVector::<f64, N>::zeros(),
            p: SMatrix::<f64, N, N>::zeros(),
        };
        imm.compute_mixing_probabilities();
        imm.combine();
        Ok(imm)
    }

    /// How many modes this filter runs.
    #[must_use]
    pub fn mode_count(&self) -> usize {
        self.modes.len()
    }

    /// The current mode probabilities, in the order the modes were given.
    ///
    /// This is the output an operator display should read alongside the state: "the
    /// target is 90% likely to be turning" is the thing an IMM knows that a single
    /// filter does not.
    #[must_use]
    pub fn mode_probabilities(&self) -> &[f64] {
        &self.mode_probabilities
    }

    /// The per-mode innovation likelihoods from the most recent update, zero before any.
    #[must_use]
    pub fn likelihoods(&self) -> &[f64] {
        &self.likelihoods
    }

    /// The combined estimate.
    #[must_use]
    pub fn state(&self) -> &SVector<f64, N> {
        &self.x
    }

    /// The combined covariance, including the between-mode spread. See the module
    /// documentation for what that does and does not mean.
    #[must_use]
    pub fn covariance(&self) -> &SMatrix<f64, N, N> {
        &self.p
    }

    /// The estimate of one mode, for a consumer that needs the mixture rather than its
    /// moment-matched summary.
    #[must_use]
    pub fn mode_state(&self, mode: usize) -> Option<&SVector<f64, N>> {
        self.modes.get(mode).map(|m| m.state())
    }

    /// The covariance of one mode.
    #[must_use]
    pub fn mode_covariance(&self, mode: usize) -> Option<&SMatrix<f64, N, N>> {
        self.modes.get(mode).map(|m| m.covariance())
    }

    /// Mix, then predict every mode from its mixed initial condition.
    pub fn predict(&mut self, dt: f64) {
        let n = self.modes.len();
        let mut mixed_x = Vec::with_capacity(n);
        let mut mixed_p = Vec::with_capacity(n);
        for j in 0..n {
            let mut x = SVector::<f64, N>::zeros();
            for (i, mode) in self.modes.iter().enumerate() {
                x += mode.state() * self.mixing[i * n + j];
            }
            let mut p = SMatrix::<f64, N, N>::zeros();
            for (i, mode) in self.modes.iter().enumerate() {
                let d = mode.state() - x;
                p += (d * d.transpose() + mode.covariance()) * self.mixing[i * n + j];
            }
            mixed_x.push(x);
            mixed_p.push(symmetrize(&p));
        }
        for (mode, (x, p)) in self.modes.iter_mut().zip(mixed_x.into_iter().zip(mixed_p)) {
            mode.set_estimate(x, p);
            mode.predict(dt);
        }
        self.combine();
    }

    /// Update every mode on `z`, move the mode probabilities toward whichever explained
    /// it, and recombine.
    pub fn update(&mut self, z: &SVector<f64, M>) {
        for (i, mode) in self.modes.iter_mut().enumerate() {
            self.likelihoods[i] = mode.update_with_likelihood(z);
        }
        let mut total = 0.0;
        for i in 0..self.modes.len() {
            self.mode_probabilities[i] = self.cbar[i] * self.likelihoods[i];
            total += self.mode_probabilities[i];
        }
        if total > 0.0 && total.is_finite() {
            for p in &mut self.mode_probabilities {
                *p /= total;
            }
        }
        // Left untouched when the total underflows: the previous probabilities are the
        // last thing this filter actually knew, and a uniform reset would be a claim
        // that the modes are equally likely, which no measurement said.
        self.compute_mixing_probabilities();
        self.combine();
    }

    /// `c̄ⱼ = Σᵢ pᵢⱼ μᵢ` and `ωᵢⱼ = pᵢⱼ μᵢ / c̄ⱼ`.
    fn compute_mixing_probabilities(&mut self) {
        let n = self.modes.len();
        for j in 0..n {
            let mut cbar = 0.0;
            for i in 0..n {
                cbar += self.transition[i * n + j] * self.mode_probabilities[i];
            }
            self.cbar[j] = cbar;
            for i in 0..n {
                self.mixing[i * n + j] = if cbar > 0.0 {
                    self.transition[i * n + j] * self.mode_probabilities[i] / cbar
                } else {
                    // No probability flows into mode j at all. Mixing from nothing is
                    // undefined; keeping mode j's own estimate is the one choice that
                    // invents no information.
                    f64::from(u8::from(i == j))
                };
            }
        }
    }

    /// Moment-match the mixture into one Gaussian.
    fn combine(&mut self) {
        let mut x = SVector::<f64, N>::zeros();
        for (mode, mu) in self.modes.iter().zip(&self.mode_probabilities) {
            x += mode.state() * *mu;
        }
        let mut p = SMatrix::<f64, N, N>::zeros();
        for (mode, mu) in self.modes.iter().zip(&self.mode_probabilities) {
            let d = mode.state() - x;
            p += (d * d.transpose() + mode.covariance()) * *mu;
        }
        self.x = x;
        self.p = symmetrize(&p);
        assert_psd(&self.p);
    }
}

/// Remove the last-bit asymmetry a sum of outer products leaves behind.
fn symmetrize<const N: usize>(p: &SMatrix<f64, N, N>) -> SMatrix<f64, N, N> {
    (p + p.transpose()) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KalmanFilter;
    use gungnir_core::{ConstantVelocity, CoordinatedTurn};

    fn position_h() -> SMatrix<f64, 3, 6> {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        h
    }

    fn cv_ct_imm(x0: SVector<f64, 6>) -> Imm<6, 3> {
        let p0 = SMatrix::<f64, 6, 6>::identity() * 100.0;
        let r = SMatrix::<f64, 3, 3>::identity() * 25.0;
        let cv = KalmanFilter::new(
            x0,
            p0,
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            r,
        );
        let ct = KalmanFilter::new(
            x0,
            p0,
            CoordinatedTurn {
                sigma_a_sq: 1.0,
                omega: 0.15,
            },
            position_h(),
            r,
        );
        Imm::new(
            vec![Box::new(cv), Box::new(ct)],
            &[0.5, 0.5],
            &[vec![0.97, 0.03], vec![0.03, 0.97]],
        )
        .expect("a well-formed two-mode IMM")
    }

    #[test]
    fn one_mode_is_refused() {
        let cv = KalmanFilter::new(
            SVector::<f64, 6>::zeros(),
            SMatrix::<f64, 6, 6>::identity(),
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            SMatrix::<f64, 3, 3>::identity(),
        );
        let err = Imm::<6, 3>::new(vec![Box::new(cv)], &[1.0], &[vec![1.0]]).unwrap_err();
        assert_eq!(err, FilterError::TooFewModes { modes: 1 });
    }

    #[test]
    fn a_transition_row_that_does_not_sum_to_one_is_refused_not_renormalised() {
        let x0 = SVector::<f64, 6>::zeros();
        let p0 = SMatrix::<f64, 6, 6>::identity();
        let r = SMatrix::<f64, 3, 3>::identity();
        let a = KalmanFilter::new(
            x0,
            p0,
            ConstantVelocity { sigma_a_sq: 1.0 },
            position_h(),
            r,
        );
        let b = KalmanFilter::new(
            x0,
            p0,
            ConstantVelocity { sigma_a_sq: 9.0 },
            position_h(),
            r,
        );
        let err = Imm::<6, 3>::new(
            vec![Box::new(a), Box::new(b)],
            &[0.5, 0.5],
            &[vec![0.9, 0.2], vec![0.03, 0.97]],
        )
        .unwrap_err();
        assert_eq!(
            err,
            FilterError::MalformedTransitionMatrix {
                what: "a row does not sum to one"
            }
        );
    }

    #[test]
    fn mode_probabilities_stay_a_distribution_over_a_long_run() {
        let mut imm = cv_ct_imm(SVector::<f64, 6>::zeros());
        for step in 0..500 {
            imm.predict(0.5);
            let t = f64::from(step) * 0.5;
            imm.update(&SVector::<f64, 3>::new(t * 10.0, 0.0, 100.0));
            let sum: f64 = imm.mode_probabilities().iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-9,
                "mode probabilities summed to {sum} at step {step}"
            );
            assert!(
                imm.mode_probabilities().iter().all(|p| *p >= 0.0),
                "a negative mode probability at step {step}"
            );
        }
    }

    /// The property an IMM exists for: a straight-line target must drive probability
    /// onto the constant-velocity mode, and a turning one onto the turn mode. If this
    /// did not hold, the filter would be a more expensive Kalman filter.
    #[test]
    fn probability_moves_to_the_mode_that_explains_the_motion() {
        let mut straight = cv_ct_imm(SVector::<f64, 6>::new(0.0, 0.0, 100.0, 10.0, 0.0, 0.0));
        for step in 0..80 {
            straight.predict(1.0);
            let t = f64::from(step + 1);
            straight.update(&SVector::<f64, 3>::new(t * 10.0, 0.0, 100.0));
        }
        assert!(
            straight.mode_probabilities()[0] > straight.mode_probabilities()[1],
            "a straight run did not favour constant velocity: {:?}",
            straight.mode_probabilities()
        );

        let omega = 0.15_f64;
        let speed = 100.0_f64;
        let radius = speed / omega;
        let mut turning = cv_ct_imm(SVector::<f64, 6>::new(0.0, 0.0, 100.0, speed, 0.0, 0.0));
        for step in 0..80 {
            turning.predict(1.0);
            let t = f64::from(step + 1);
            let angle = omega * t;
            turning.update(&SVector::<f64, 3>::new(
                radius * angle.sin(),
                radius * (1.0 - angle.cos()),
                100.0,
            ));
        }
        assert!(
            turning.mode_probabilities()[1] > turning.mode_probabilities()[0],
            "a coordinated turn did not favour the turn mode: {:?}",
            turning.mode_probabilities()
        );
    }

    /// The combined covariance must widen while the modes disagree. This is the
    /// property the module documentation claims and the reason the spread term is in
    /// the formula at all.
    #[test]
    fn disagreement_between_modes_widens_the_combined_covariance() {
        let mut imm = cv_ct_imm(SVector::<f64, 6>::new(0.0, 0.0, 100.0, 100.0, 0.0, 0.0));
        for _ in 0..10 {
            imm.predict(1.0);
        }
        let spread = {
            let x = *imm.state();
            let mut s = SMatrix::<f64, 6, 6>::zeros();
            for (mode, mu) in (0..imm.mode_count())
                .filter_map(|i| imm.mode_state(i))
                .zip(imm.mode_probabilities())
            {
                let d = mode - x;
                s += d * d.transpose() * *mu;
            }
            s
        };
        assert!(
            spread.trace() > 0.0,
            "the two modes never diverged, so this test proves nothing"
        );
        let average: SMatrix<f64, 6, 6> = (0..imm.mode_count())
            .filter_map(|i| imm.mode_covariance(i))
            .zip(imm.mode_probabilities())
            .map(|(p, mu)| p * *mu)
            .sum();
        assert!(
            imm.covariance().trace() > average.trace(),
            "the combined covariance did not include the between-mode spread"
        );
    }

    #[test]
    fn the_likelihood_floor_keeps_a_ruled_out_mode_finite() {
        let y = SVector::<f64, 3>::new(1e6, 1e6, 1e6);
        let s = SMatrix::<f64, 3, 3>::identity();
        let l = innovation_likelihood(&y, &s);
        assert!(l > 0.0, "a ruled-out mode underflowed to zero");
        assert!(l.is_finite());
    }
}
