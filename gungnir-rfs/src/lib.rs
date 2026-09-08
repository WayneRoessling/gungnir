// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! rfs: random-finite-set filtering. The `docs/verification-capability-table.md` §1
//! rows "`rfs` | PHD / CPHD filter" and "`rfs` | GLMB / LMB filter".
//!
//! Cardinality-first: the pass criteria are about *how many* targets there are and, for
//! the labelled filters, *which one is which* -- not only where they are. Validated
//! against `scenario-crate-narrative.md` Scenario 4, the dense swarm; well-separated
//! targets never meaningfully exercise these rows.
//!
//! # What is built here, and what is not
//!
//! **Built and gated: the Gaussian-mixture PHD filter** ([`PhdFilter`]) **and the
//! Gaussian-mixture CPHD filter** ([`CphdFilter`], GAP-009's sibling row GAP-015).
//!
//! **Not built, and returning an explicit error rather than a plausible answer**: the
//! labelled filters [`GlmbFilter`] and [`LmbFilter`]. Each is its own §2 row and each
//! will want its own oracle comparison. They are named individually rather than behind
//! one "not implemented" so a reader can tell which of the four this build has.
//!
//! **The CPHD build is written and gated, not signed.** It is reached by
//! `docs/agentic-workflow.md`'s numerical-stability clause the same way the PHD filter
//! is (`ARCHITECTURE.md` §10), and its own oracle -- there being no library one; see
//! [`CphdFilter`]'s doc comment -- is this crate's own hand derivation, independently
//! checked against a brute-force enumeration and against known reductions before any
//! Rust was written. That is real verification, not a substitute for the owner's
//! review this class of code still needs before it is signed.
//!
//! # What a PHD filter is, and why the cardinality is the interesting output
//!
//! Every other filter in this workspace tracks a *fixed* set of objects: something else
//! decides a track exists, and the filter estimates where it is. A PHD filter estimates
//! the whole set at once -- how many objects there are and where they are -- as a single
//! intensity function over the state space, whose integral is the expected number of
//! targets.
//!
//! That is the right shape for a swarm. With forty objects in a volume, the association
//! problem that a conventional tracker must solve first has more hypotheses than can be
//! enumerated, and getting it wrong produces confidently wrong tracks. A PHD filter never
//! forms the association at all.
//!
//! The price is the thing to be honest about: **a PHD filter does not carry identity**.
//! The intensity says "there is about one target here"; it does not say it is the same
//! target that was there last scan. [`PhdFilter::extract_tracks`] therefore mints fresh
//! identifiers every scan, and its documentation says so, because a consumer that
//! assumed continuity would be reading target identity out of a filter that has none.
//! That continuity is what the labelled filters add, and it is why they are a separate
//! row rather than a refinement of this one.
//!
//! # Pruning and merging are part of the filter, not an optimisation
//!
//! The mixture gains a component per existing component per detection every scan, so
//! after ten scans of a busy sky it has more components than atoms worth counting.
//! Pruning drops components too weak to matter and merging combines components that are
//! describing the same target. Both change the answer slightly and both are in every
//! reference implementation, so the oracle comparison is against a filter that does them
//! too, with the same thresholds carried in the fixture.

use gungnir_track::{MotionModel, Track, TrackId, TrackStatus};
use nalgebra::{SMatrix, SVector};

/// The state dimension these filters work in: position and velocity per axis.
const N: usize = 6;
/// The measurement dimension: position per axis.
const M: usize = 3;

/// One Gaussian component of the intensity function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaussianComponent {
    /// The expected number of targets this component accounts for. **Not a probability**:
    /// the weights of a PHD intensity sum to the expected target count, not to one, and
    /// a single component's weight can exceed one when it represents several targets that
    /// have not been resolved from each other.
    pub weight: f64,
    pub mean: SVector<f64, N>,
    pub cov: SMatrix<f64, N, N>,
}

/// How the mixture is kept to a workable size, and what the scene looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhdSettings {
    /// `p_S`, the probability a target present last scan is still present.
    pub probability_of_survival: f64,
    /// `p_D`, the probability a present target is detected.
    pub probability_of_detection: f64,
    /// `κ`, the clutter intensity per unit measurement volume.
    pub clutter_density: f64,
    /// Components below this weight are dropped.
    pub prune_threshold: f64,
    /// Components within this squared Mahalanobis distance of a stronger one are merged
    /// into it.
    pub merge_distance: f64,
    /// The mixture is truncated to this many components, strongest first.
    pub max_components: usize,
    /// A component is extracted as a track above this weight. The conventional 0.5: a
    /// component accounting for less than half a target is not one.
    pub extraction_threshold: f64,
}

impl Default for PhdSettings {
    /// The conventional values from the Gaussian-mixture PHD literature, which are also
    /// the ones the oracle fixture uses.
    fn default() -> Self {
        Self {
            probability_of_survival: 0.99,
            probability_of_detection: 0.95,
            clutter_density: 1e-6,
            prune_threshold: 1e-5,
            merge_distance: 4.0,
            max_components: 100,
            extraction_threshold: 0.5,
        }
    }
}

impl PhdSettings {
    fn validate(&self) -> Result<(), RfsError> {
        let probabilities_valid = (0.0..=1.0).contains(&self.probability_of_survival)
            && (0.0..=1.0).contains(&self.probability_of_detection);
        if !probabilities_valid {
            return Err(RfsError::MalformedScene {
                what: "the survival or detection probability is not a probability",
            });
        }
        if !self.clutter_density.is_finite() || self.clutter_density < 0.0 {
            return Err(RfsError::MalformedScene {
                what: "the clutter intensity must be finite and non-negative",
            });
        }
        if self.max_components == 0 {
            return Err(RfsError::MalformedScene {
                what: "a mixture truncated to no components represents nothing",
            });
        }
        Ok(())
    }
}

/// Gaussian-mixture Probability Hypothesis Density filter.
#[derive(Debug, Clone)]
pub struct PhdFilter {
    /// The intensity function, as a Gaussian mixture.
    pub intensity_components: Vec<GaussianComponent>,
    settings: PhdSettings,
    h: SMatrix<f64, M, N>,
    r: SMatrix<f64, M, M>,
}

/// Cardinalized PHD: the PHD plus an explicit distribution over the target count.
///
/// The PHD's cardinality estimate ([`PhdFilter::cardinality`]) is the sum of the
/// intensity weights, which is that distribution's *mean* and nothing more; a mean of
/// 2.4 does not say whether the truth is "almost always 2, sometimes 3" or "often 0,
/// occasionally 5" -- two beliefs a commander would act on very differently. A CPHD
/// propagates the whole distribution ([`Self::cardinality_distribution`]), which is also
/// what makes it far less prone to the PHD's characteristic cardinality swings when
/// detections are missed (`gungnir-rfs/tests/cphd_diff.rs` measures this directly rather
/// than asserting it).
///
/// # No library oracle exists for this row
///
/// `docs/verification-capability-table.md` §2 named Stone Soup's GM-CPHD as the
/// intended oracle. Stone Soup 1.9.1 -- the pinned version, already driven for real for
/// the PHD row next to this one -- has no CPHD updater at all: `stonesoup.updater.
/// pointprocess` exports `PHDUpdater` and nothing else, checked directly rather than
/// assumed from the package's name. So unlike the PHD row, where the library exists and
/// was found to disagree, here there is no library implementation to compare against,
/// confirmed rather than skipped.
///
/// The oracle is therefore this crate's own derivation, in
/// `testdata/oracles/tools/gen_cphd_fixtures.py`, built and checked in four
/// independent ways before being trusted to gate anything: against a literal
/// brute-force enumeration of every target-to-measurement association (not the same
/// elementary-symmetric-function bookkeeping the closed form uses, so an error in that
/// bookkeeping would not be repeated by the check); against the identity that the
/// updated intensity's integral must equal the updated cardinality distribution's mean,
/// which any correct PHD/CPHD posterior satisfies by construction; against reducing to
/// the plain GM-PHD update exactly when the cardinality prior is Poisson, since the PHD
/// filter is the CPHD filter restricted to that assumption; and against a Monte Carlo
/// comparison showing materially lower cardinality-estimate variance than PHD under
/// frequent missed detections, which is the property this filter exists for rather than
/// an incidental one.
///
/// # The birth cardinality distribution
///
/// [`Self::predict`] needs a count distribution for however many targets are born this
/// scan, and `births` supplies Gaussian shapes, not a count distribution. This filter
/// assumes the birth count is **Poisson, with mean equal to the summed weight of
/// `births`** -- the same distributional family the clutter model already assumes
/// elsewhere in this filter, and the standard choice in the CPHD literature. A birth
/// intensity summing to 0.6 predicts a birth count centred there, not a certainty of
/// exactly zero or exactly one.
#[derive(Debug, Clone)]
pub struct CphdFilter {
    /// The intensity, in the same Gaussian-mixture representation [`PhdFilter`] uses.
    /// Once [`Self::update`] has run, this mixture's own weights sum to the posterior
    /// cardinality *mean* only -- [`Self::cardinality_distribution`] is where the rest
    /// of what this filter knows about the count lives.
    pub phd: PhdFilter,
    /// `cardinality_dist[n]` is the probability there are exactly `n` targets, for `n`
    /// from 0 to this filter's truncation bound ([`Self::max_cardinality`]).
    pub cardinality_dist: Vec<f64>,
}

impl CphdFilter {
    /// An empty CPHD: no targets, certain of it (`cardinality_dist == [1.0, 0.0, ...]`).
    ///
    /// `max_cardinality` truncates the cardinality distribution's support. The update
    /// and predict steps below cost `O(max_cardinality)` work per detection, so this is
    /// a real budget, not a formality; the scenes this crate is validated against
    /// (`docs/scenario-crate-narrative.md` Scenario 4) top out at a few dozen targets, so
    /// a bound comfortably above that is generous without being unbounded.
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] when `settings` do not describe a scene (the same
    /// check [`PhdFilter::new`] makes), or when `max_cardinality` is 0 -- a distribution
    /// over target counts that admits only zero, forever, is not one.
    pub fn new(
        settings: PhdSettings,
        h: SMatrix<f64, M, N>,
        r: SMatrix<f64, M, M>,
        max_cardinality: usize,
    ) -> Result<Self, RfsError> {
        if max_cardinality == 0 {
            return Err(RfsError::MalformedScene {
                what: "a cardinality distribution truncated to zero targets is not one",
            });
        }
        let phd = PhdFilter::new(settings, h, r)?;
        let mut cardinality_dist = vec![0.0; max_cardinality + 1];
        cardinality_dist[0] = 1.0;
        Ok(Self {
            phd,
            cardinality_dist,
        })
    }

    /// The truncation bound: [`Self::cardinality_distribution`] holds this many entries
    /// past zero.
    #[must_use]
    pub fn max_cardinality(&self) -> usize {
        self.cardinality_dist.len().saturating_sub(1)
    }

    /// The full posterior distribution over the target count: `[n]` is the probability
    /// there are exactly `n` targets, `n` from 0 to [`Self::max_cardinality`].
    #[must_use]
    pub fn cardinality_distribution(&self) -> &[f64] {
        &self.cardinality_dist
    }

    /// The distribution's mean -- comparable to [`PhdFilter::cardinality`], and enough
    /// for a caller that only wants a point estimate rather than the whole shape.
    #[must_use]
    pub fn cardinality_mean(&self) -> f64 {
        self.cardinality_dist
            .iter()
            .enumerate()
            .map(|(n, p)| {
                #[allow(clippy::cast_precision_loss)]
                let n = n as f64;
                n * p
            })
            .sum()
    }

    /// The most probable target count: the mode of [`Self::cardinality_distribution`],
    /// and what [`Self::extract_tracks`] commits to.
    #[must_use]
    pub fn cardinality_map(&self) -> usize {
        self.cardinality_dist
            .iter()
            .enumerate()
            .fold((0_usize, f64::NEG_INFINITY), |(best_n, best_p), (n, &p)| {
                if p > best_p {
                    (n, p)
                } else {
                    (best_n, best_p)
                }
            })
            .0
    }

    /// Propagate the intensity and the cardinality distribution forward together.
    ///
    /// The intensity half is exactly [`PhdFilter::predict`] -- survival at `p_S` through
    /// the motion model, births at full weight -- because the Gaussian-mixture predict
    /// step does not depend on the cardinality distribution; only the update couples
    /// them. The cardinality half binomially thins the current distribution by `p_S`
    /// (each of however many targets survives independently) and convolves the result
    /// with the birth cardinality distribution the type's own documentation names.
    ///
    /// # Errors
    ///
    /// [`RfsError::NotFinite`] for a birth component that is not a number, from the same
    /// check [`PhdFilter::predict`] makes.
    pub fn predict<Motion>(
        &mut self,
        motion: &Motion,
        dt: f64,
        births: &[GaussianComponent],
    ) -> Result<(), RfsError>
    where
        Motion: MotionModel<N>,
    {
        self.phd.predict(motion, dt, births)?;
        let birth_mean: f64 = births.iter().map(|c| c.weight).sum();
        self.cardinality_dist = predict_cardinality(
            &self.cardinality_dist,
            self.phd.settings.probability_of_survival,
            birth_mean,
        );
        Ok(())
    }

    /// Update the intensity and the cardinality distribution together with this scan's
    /// detections.
    ///
    /// This is the Gaussian-mixture CPHD update (Vo, Vo and Cantoni, "Analytic
    /// Implementations of the Cardinalized Probability Hypothesis Density Filter", IEEE
    /// Transactions on Signal Processing 55(7), 2007), re-derived and independently
    /// checked in `testdata/oracles/tools/gen_cphd_fixtures.py` rather than transcribed
    /// -- see [`Self`]'s own doc comment for what checked it. Every candidate
    /// component's *shape* (mean and covariance) is identical to [`PhdFilter::update`]'s:
    /// a missed-detection candidate is unmoved, and a candidate matched to detection `z`
    /// is the standard Kalman update of that component by `z`. What CPHD changes is the
    /// *weight*: instead of PHD's per-detection normalisation by `clutter + Σ weights`,
    /// every missed-detection candidate is scaled by one cardinality-derived factor and
    /// every `z`-matched candidate by another (and each detection can get its own),
    /// computed from the elementary symmetric functions of the detections' predictive
    /// likelihoods against the prior cardinality distribution.
    ///
    /// # Errors
    ///
    /// The same as [`PhdFilter::update`] for a malformed detection or a singular
    /// innovation covariance, plus [`RfsError::MalformedScene`] when the prior
    /// cardinality distribution and `p_D` are jointly inconsistent with this scan ever
    /// having been observed (only reachable at `p_D`'s extreme, and named here rather
    /// than dividing by the zero it would otherwise produce).
    // One cohesive derivation (see the module and type doc comments for the maths);
    // splitting it at an arbitrary line count would scatter the tightly coupled local
    // state (`prepared`, `q`, `xi`, the two `Lambda` tables) across function
    // boundaries rather than make it clearer.
    #[allow(clippy::too_many_lines)]
    pub fn update(&mut self, detections: &[SVector<f64, M>]) -> Result<(), RfsError> {
        for z in detections {
            if !z.iter().all(|v| v.is_finite()) {
                return Err(RfsError::NotFinite {
                    what: "a detection",
                });
            }
        }

        let settings = self.phd.settings;
        let n_max = self.max_cardinality();
        let total_weight: f64 = self.phd.intensity_components.iter().map(|c| c.weight).sum();

        // Per component, the same Kalman-update quantities `PhdFilter::update` prepares:
        // gain, innovation inverse and its Cholesky determinant (for the likelihood
        // normaliser), the updated covariance (shared by every detection, since the
        // linear-Gaussian posterior covariance does not depend on which measurement is
        // used), and the predicted measurement.
        let mut prepared = Vec::with_capacity(self.phd.intensity_components.len());
        for component in &self.phd.intensity_components {
            let pht = component.cov * self.phd.h.transpose();
            let s = self.phd.h * pht + self.phd.r;
            let Some(s_inv) = s.try_inverse() else {
                return Err(RfsError::SingularCovariance {
                    what: "an innovation covariance",
                });
            };
            let Some(chol) = s.cholesky() else {
                return Err(RfsError::SingularCovariance {
                    what: "an innovation covariance",
                });
            };
            let determinant: f64 = chol.l().diagonal().iter().map(|d| d * d).product();
            let k = pht * s_inv;
            let i_kh = SMatrix::<f64, N, N>::identity() - k * self.phd.h;
            let cov = symmetrize(
                &(i_kh * component.cov * i_kh.transpose() + k * self.phd.r * k.transpose()),
            );
            prepared.push((k, s_inv, determinant, cov, self.phd.h * component.mean));
        }

        #[allow(clippy::cast_precision_loss)]
        let m_dim = M as f64;
        let likelihood_at = |z: &SVector<f64, M>,
                             s_inv: &SMatrix<f64, M, M>,
                             determinant: f64,
                             predicted_z: &SVector<f64, M>|
         -> f64 {
            let y = z - predicted_z;
            let quadratic = (y.transpose() * s_inv * y)[(0, 0)];
            let normaliser = ((2.0 * std::f64::consts::PI).powf(m_dim) * determinant).sqrt();
            (-0.5 * quadratic).exp() / normaliser
        };

        // q_j: the predictive likelihood of detection j under the NORMALISED mixture --
        // zero, rather than an undefined 0/0, when there is no believed intensity at
        // all to predict anything.
        let q: Vec<f64> = detections
            .iter()
            .map(|z| {
                if total_weight <= 0.0 {
                    return 0.0;
                }
                self.phd
                    .intensity_components
                    .iter()
                    .zip(&prepared)
                    .map(|(c, (_, s_inv, determinant, _, predicted_z))| {
                        (c.weight / total_weight)
                            * likelihood_at(z, s_inv, *determinant, predicted_z)
                    })
                    .sum()
            })
            .collect();
        let xi: Vec<f64> = q.iter().map(|&qj| qj / settings.clutter_density).collect();

        let e_full = elementary_symmetric(&xi);
        let e_leave_one_out = elementary_symmetric_leave_one_out(&xi);

        let lambda = |n: usize, e: &[f64]| -> f64 {
            let r_max = n.min(e.len() - 1);
            // n and r are cardinalities bounded by `max_cardinality`, nowhere near
            // i32::MAX in any scene this filter is meant for.
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            (0..=r_max)
                .map(|r| {
                    falling_factorial(n, r)
                        * (1.0 - settings.probability_of_detection).powi((n - r) as i32)
                        * settings.probability_of_detection.powi(r as i32)
                        * e[r]
                })
                .sum()
        };

        let lam: Vec<f64> = (0..=n_max).map(|n| lambda(n, &e_full)).collect();
        let lam_leave_one_out: Vec<Vec<f64>> = e_leave_one_out
            .iter()
            .map(|e| (0..=n_max).map(|n| lambda(n, e)).collect())
            .collect();

        let z_norm: f64 = self
            .cardinality_dist
            .iter()
            .zip(&lam)
            .map(|(p, l)| p * l)
            .sum();
        if !z_norm.is_finite() || z_norm <= 0.0 {
            return Err(RfsError::MalformedScene {
                what: "no cardinality is consistent with this scan under the prior and p_D",
            });
        }

        let posterior_cardinality: Vec<f64> = self
            .cardinality_dist
            .iter()
            .zip(&lam)
            .map(|(p, l)| p * l / z_norm)
            .collect();

        #[allow(clippy::cast_precision_loss)]
        let weighted_shift = |lam_at: &[f64]| -> f64 {
            self.cardinality_dist
                .iter()
                .enumerate()
                .map(|(n, &p)| {
                    if n == 0 {
                        0.0
                    } else {
                        n as f64 * p * lam_at[n - 1]
                    }
                })
                .sum()
        };
        let a = weighted_shift(&lam);
        let b: Vec<f64> = lam_leave_one_out
            .iter()
            .map(|lam_j| weighted_shift(lam_j))
            .collect();

        let mut updated =
            Vec::with_capacity(self.phd.intensity_components.len() * (1 + detections.len()));
        if total_weight > 0.0 {
            let miss_scale =
                (1.0 - settings.probability_of_detection) * (a / z_norm) / total_weight;
            for component in &self.phd.intensity_components {
                updated.push(GaussianComponent {
                    weight: component.weight * miss_scale,
                    ..*component
                });
            }
            for (j, z) in detections.iter().enumerate() {
                let det_scale = settings.probability_of_detection * (b[j] / z_norm)
                    / (total_weight * settings.clutter_density);
                for (component, (k, s_inv, determinant, cov, predicted_z)) in
                    self.phd.intensity_components.iter().zip(&prepared)
                {
                    let lik = likelihood_at(z, s_inv, *determinant, predicted_z);
                    let y = z - predicted_z;
                    updated.push(GaussianComponent {
                        weight: component.weight * lik * det_scale,
                        mean: component.mean + k * y,
                        cov: *cov,
                    });
                }
            }
        }

        self.phd.intensity_components = updated;
        self.phd.prune_and_merge();
        self.cardinality_dist = posterior_cardinality;
        Ok(())
    }

    /// Commit to a target set: the [`Self::cardinality_map`] strongest components,
    /// exactly the number the cardinality distribution's mode says there are.
    ///
    /// **Not [`PhdFilter::extract_tracks`]'s per-component threshold.** That rule is the
    /// right one for a filter whose only cardinality knowledge is a mean; this filter
    /// knows the whole distribution, and committing to its mode's count directly is the
    /// standard CPHD extraction rather than an arbitrary per-component cutoff applied to
    /// a filter that has more to say. The identifiers are minted fresh every call for
    /// the same reason [`PhdFilter::extract_tracks`]'s are: this is still a PHD-family
    /// filter, and it carries no identity across scans.
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] if a component's weight is not finite.
    pub fn extract_tracks(&self) -> Result<Vec<Track>, RfsError> {
        let mut components: Vec<&GaussianComponent> =
            self.phd.intensity_components.iter().collect();
        for component in &components {
            if !component.weight.is_finite() {
                return Err(RfsError::MalformedScene {
                    what: "a component's weight is not finite",
                });
            }
        }
        components.sort_by(|a, b| b.weight.total_cmp(&a.weight));
        let count = self.cardinality_map().min(components.len());
        Ok(components
            .into_iter()
            .take(count)
            .enumerate()
            .map(|(index, component)| Track {
                id: TrackId(u64::try_from(index).unwrap_or(u64::MAX)),
                status: TrackStatus::Confirmed,
                state: component.mean,
                covariance: component.cov,
                misses_since_update: 0,
                hits: 1,
            })
            .collect())
    }
}

/// The predicted cardinality distribution: `previous` binomially thinned by
/// `p_survival` (each target survives independently), convolved with a Poisson birth
/// distribution of mean `birth_mean`. `previous.len() - 1` is the truncation bound,
/// carried through unchanged.
fn predict_cardinality(previous: &[f64], p_survival: f64, birth_mean: f64) -> Vec<f64> {
    let n_max = previous.len() - 1;
    let mut thinned = vec![0.0; n_max + 1];
    for (l, &p_l) in previous.iter().enumerate() {
        if p_l == 0.0 {
            continue;
        }
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        for (j, slot) in thinned.iter_mut().enumerate().take(l + 1) {
            let term = binomial(l, j)
                * p_survival.powi(j as i32)
                * (1.0 - p_survival).powi((l - j) as i32)
                * p_l;
            *slot += term;
        }
    }
    let birth = poisson_pmf(birth_mean, n_max);
    let mut out = vec![0.0; n_max + 1];
    for (n, slot) in out.iter_mut().enumerate() {
        for j in 0..=n {
            *slot += birth[n - j] * thinned[j];
        }
    }
    out
}

/// The Poisson PMF at `0..=n_max`, built by the standard ratio recursion
/// (`p(n) = p(n-1) * mean / n`) rather than raw factorials, so it stays finite for a
/// truncation bound where `n_max!` would not.
fn poisson_pmf(mean: f64, n_max: usize) -> Vec<f64> {
    let mut out = vec![0.0; n_max + 1];
    out[0] = (-mean).exp();
    for n in 1..=n_max {
        #[allow(clippy::cast_precision_loss)]
        let n_f = n as f64;
        out[n] = out[n - 1] * mean / n_f;
    }
    out
}

/// `n` choose `k`, by the standard iterative ratio (`C(n,k) = C(n,k-1)*(n-k+1)/k`)
/// rather than raw factorials, and exploiting `C(n,k) = C(n,n-k)` so the loop runs over
/// whichever of `k`, `n - k` is smaller.
fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut result = 1.0;
    #[allow(clippy::cast_precision_loss)]
    for i in 0..k {
        result *= (n - i) as f64 / (i + 1) as f64;
    }
    result
}

/// The falling factorial `n! / (n - r)!`; zero for `r > n`, matching the convention that
/// more targets can be assigned to detections than exist.
fn falling_factorial(n: usize, r: usize) -> f64 {
    if r > n {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    (0..r).fold(1.0, |acc, k| acc * (n - k) as f64)
}

/// The elementary symmetric functions `e[0]..e[values.len()]` of `values`
/// (`e[0] == 1`, `e[r] == 0` past `values.len()`), by the standard `O(n^2)` dynamic
/// program: each value is folded in from the top index down, so a slot is updated from
/// the previous value's contribution before it is read for the current one.
fn elementary_symmetric(values: &[f64]) -> Vec<f64> {
    let mut e = vec![1.0];
    for &v in values {
        e.push(0.0);
        for r in (1..e.len()).rev() {
            e[r] += e[r - 1] * v;
        }
    }
    e
}

/// [`elementary_symmetric`] of `values` with each index left out in turn, in
/// `O(len(values)^2)` total rather than `O(len(values)^3)` from recomputing per index.
///
/// `E(x) = Π(1 + v_i x)` factors as `(1 + v_j x) · Q_j(x)`, and `Q_j`'s coefficients --
/// exactly the leave-`j`-out elementary symmetric functions -- come from one pass of
/// synthetic division of `E` by `(1 + v_j x)`: matching the coefficient of `x^k` on both
/// sides of `E(x) = Q_j(x) + v_j x Q_j(x)` gives `e_k = q_k + v_j q_{k-1}`, so
/// `q_k = e_k - v_j q_{k-1}`, computed forward from `q_0 = e_0 = 1`.
fn elementary_symmetric_leave_one_out(values: &[f64]) -> Vec<Vec<f64>> {
    let e = elementary_symmetric(values);
    let m = values.len();
    (0..m)
        .map(|j| {
            let mut q = vec![0.0; m];
            if m > 0 {
                q[0] = 1.0;
            }
            for k in 1..m {
                q[k] = e[k] - values[j] * q[k - 1];
            }
            q
        })
        .collect()
}

/// Generalized Labeled Multi-Bernoulli: PHD-style set filtering that also carries target
/// identity.
///
/// **Not implemented**, and it is the interesting one that is missing: identity across
/// scans is exactly what [`PhdFilter`] does not provide.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlmbFilter;

/// Labeled Multi-Bernoulli: a cheaper GLMB approximation. **Not implemented.**
#[derive(Debug, Clone, Copy, Default)]
pub struct LmbFilter;

impl GlmbFilter {
    /// # Errors
    ///
    /// Always, until the GLMB/LMB row is built.
    pub fn labelled_tracks(&self) -> Result<Vec<Track>, RfsError> {
        Err(RfsError::NotImplemented {
            what: "labelled multi-Bernoulli filtering",
            waiting_on: "the `rfs` GLMB/LMB row and its Stone Soup oracle",
        })
    }
}

impl LmbFilter {
    /// # Errors
    ///
    /// Always, until the GLMB/LMB row is built.
    pub fn labelled_tracks(&self) -> Result<Vec<Track>, RfsError> {
        Err(RfsError::NotImplemented {
            what: "labelled multi-Bernoulli filtering",
            waiting_on: "the `rfs` GLMB/LMB row and its Stone Soup oracle",
        })
    }
}

/// What this crate cannot do, named rather than panicked (GAP-082).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RfsError {
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
    /// The scene parameters do not describe a scene.
    #[error("the scene is malformed: {what}")]
    MalformedScene { what: &'static str },
    /// A covariance that carries no information, so no update can be formed from it.
    #[error("{what} is singular")]
    SingularCovariance { what: &'static str },
    /// A component or detection that is not finite.
    #[error("{what} is not finite")]
    NotFinite { what: &'static str },
}

impl PhdFilter {
    /// An empty intensity: no targets are believed to be present.
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] when the settings do not describe a scene.
    pub fn new(
        settings: PhdSettings,
        h: SMatrix<f64, M, N>,
        r: SMatrix<f64, M, M>,
    ) -> Result<Self, RfsError> {
        settings.validate()?;
        Ok(Self {
            intensity_components: Vec::new(),
            settings,
            h,
            r,
        })
    }

    /// The expected number of targets: the integral of the intensity, which for a
    /// Gaussian mixture is the sum of its weights.
    ///
    /// **A mean, not a count.** A value of 2.4 means the filter's best estimate is
    /// somewhere between two and three targets, and reporting it as either would throw
    /// away what it actually knows. [`Self::extract_tracks`] is where a count is
    /// committed to, and it applies a stated threshold to do so.
    #[must_use]
    pub fn cardinality(&self) -> f64 {
        self.intensity_components.iter().map(|c| c.weight).sum()
    }

    /// How many components the mixture currently holds.
    #[must_use]
    pub fn component_count(&self) -> usize {
        self.intensity_components.len()
    }

    /// Propagate the intensity forward and add this scan's birth components.
    ///
    /// Surviving components are scaled by `p_S` and moved through the motion model;
    /// births enter at full weight. Birth components are supplied per scan rather than
    /// configured once because where targets may appear is a property of the scene --
    /// the edge of a sensor's coverage, a runway, a known launch area -- and not of the
    /// filter.
    ///
    /// # Errors
    ///
    /// [`RfsError::NotFinite`] for a birth component that is not a number.
    pub fn predict<Motion>(
        &mut self,
        motion: &Motion,
        dt: f64,
        births: &[GaussianComponent],
    ) -> Result<(), RfsError>
    where
        Motion: MotionModel<N>,
    {
        let f = motion.f(dt);
        let q = motion.q(dt);
        for component in &mut self.intensity_components {
            component.weight *= self.settings.probability_of_survival;
            component.mean = f * component.mean;
            let predicted = f * component.cov * f.transpose() + q;
            component.cov = symmetrize(&predicted);
        }
        for birth in births {
            if !birth.weight.is_finite()
                || !birth.mean.iter().all(|v| v.is_finite())
                || !birth.cov.iter().all(|v| v.is_finite())
            {
                return Err(RfsError::NotFinite {
                    what: "a birth component",
                });
            }
            self.intensity_components.push(*birth);
        }
        Ok(())
    }

    /// Update the intensity with this scan's detections, then prune and merge.
    ///
    /// # Errors
    ///
    /// [`RfsError::SingularCovariance`] when an innovation covariance cannot be
    /// inverted, and [`RfsError::NotFinite`] for a detection that is not a number.
    pub fn update(&mut self, detections: &[SVector<f64, M>]) -> Result<(), RfsError> {
        for z in detections {
            if !z.iter().all(|v| v.is_finite()) {
                return Err(RfsError::NotFinite {
                    what: "a detection",
                });
            }
        }

        // The missed-detection term: every component survives at reduced weight,
        // because a target that was not detected is still there.
        let mut updated: Vec<GaussianComponent> = self
            .intensity_components
            .iter()
            .map(|c| GaussianComponent {
                weight: c.weight * (1.0 - self.settings.probability_of_detection),
                ..*c
            })
            .collect();

        // Per component, the quantities every detection's update reuses.
        let mut prepared = Vec::with_capacity(self.intensity_components.len());
        for component in &self.intensity_components {
            let pht = component.cov * self.h.transpose();
            let s = self.h * pht + self.r;
            let Some(s_inv) = s.try_inverse() else {
                return Err(RfsError::SingularCovariance {
                    what: "an innovation covariance",
                });
            };
            let Some(chol) = s.cholesky() else {
                return Err(RfsError::SingularCovariance {
                    what: "an innovation covariance",
                });
            };
            let determinant: f64 = chol.l().diagonal().iter().map(|d| d * d).product();
            let k = pht * s_inv;
            let i_kh = SMatrix::<f64, N, N>::identity() - k * self.h;
            let cov =
                symmetrize(&(i_kh * component.cov * i_kh.transpose() + k * self.r * k.transpose()));
            prepared.push((k, s_inv, determinant, cov, self.h * component.mean));
        }

        for z in detections {
            let mut candidates = Vec::with_capacity(self.intensity_components.len());
            let mut total = self.settings.clutter_density;
            for (component, (k, s_inv, determinant, cov, predicted_z)) in
                self.intensity_components.iter().zip(&prepared)
            {
                let y = z - predicted_z;
                let quadratic = (y.transpose() * s_inv * y)[(0, 0)];
                #[allow(clippy::cast_precision_loss)]
                let m = M as f64;
                let normaliser = ((2.0 * std::f64::consts::PI).powf(m) * determinant).sqrt();
                let likelihood = (-0.5 * quadratic).exp() / normaliser;
                let weight = self.settings.probability_of_detection * component.weight * likelihood;
                total += weight;
                candidates.push(GaussianComponent {
                    weight,
                    mean: component.mean + k * y,
                    cov: *cov,
                });
            }
            // The normalisation is per detection and includes the clutter intensity:
            // that is what makes a detection in a cluttered region contribute less
            // weight than the same detection in a clean one.
            if total > 0.0 && total.is_finite() {
                for candidate in &mut candidates {
                    candidate.weight /= total;
                }
                updated.extend(candidates);
            }
        }

        self.intensity_components = updated;
        self.prune_and_merge();
        Ok(())
    }

    /// Drop weak components, merge near-coincident ones, and truncate.
    fn prune_and_merge(&mut self) {
        self.intensity_components
            .retain(|c| c.weight > self.settings.prune_threshold && c.weight.is_finite());

        let mut remaining = std::mem::take(&mut self.intensity_components);
        let mut merged: Vec<GaussianComponent> = Vec::new();
        while !remaining.is_empty() {
            // Take the strongest component and absorb everything close to it. Strongest
            // first, so a merged component sits at the mode rather than between two.
            let (index, _) = remaining.iter().enumerate().fold(
                (0_usize, f64::NEG_INFINITY),
                |(best, weight), (i, c)| {
                    if c.weight > weight {
                        (i, c.weight)
                    } else {
                        (best, weight)
                    }
                },
            );
            let leader = remaining[index];
            let Some(leader_inverse) = leader.cov.try_inverse() else {
                // A component whose covariance carries no information cannot absorb
                // others, and cannot be absorbed sensibly either; it is kept as it is
                // rather than dropped, because dropping it would lose the target it
                // represents.
                merged.push(leader);
                remaining.remove(index);
                continue;
            };

            let mut group = Vec::new();
            remaining.retain(|c| {
                let d = c.mean - leader.mean;
                let distance = (d.transpose() * leader_inverse * d)[(0, 0)];
                if distance <= self.settings.merge_distance {
                    group.push(*c);
                    false
                } else {
                    true
                }
            });
            merged.push(merge_group(&group));
        }

        merged.sort_by(|a, b| b.weight.total_cmp(&a.weight));
        merged.truncate(self.settings.max_components);
        self.intensity_components = merged;
    }

    /// Commit to a target set: one track per component above the extraction threshold,
    /// repeated for a component whose weight accounts for more than one target.
    ///
    /// **The identifiers are minted fresh every call and mean nothing across scans.** A
    /// PHD filter carries no identity, so a consumer that treated the returned
    /// [`TrackId`]s as continuous would be reading target identity out of a filter that
    /// has none. That continuity is what a labelled filter adds; see [`GlmbFilter`].
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] if a component's weight is not finite, which means
    /// the intensity is already broken.
    ///
    /// **Returns a `Result` rather than an empty `Vec`.** An empty track set from a
    /// filter is a claim that nothing is out there, and it is the single most dangerous
    /// empty in this system: the picture would be blank and correct-looking. An empty
    /// `Ok` here is a real claim, made only when the intensity genuinely holds nothing
    /// above the threshold.
    pub fn extract_tracks(&self) -> Result<Vec<Track>, RfsError> {
        let mut out = Vec::new();
        for component in &self.intensity_components {
            if !component.weight.is_finite() {
                return Err(RfsError::MalformedScene {
                    what: "a component's weight is not finite",
                });
            }
            if component.weight <= self.settings.extraction_threshold {
                continue;
            }
            // A component of weight 2.4 represents about two targets that have not been
            // resolved from each other; emitting one track for it would under-report the
            // scene, which in a swarm is the error that matters.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let copies = component.weight.round().max(1.0) as usize;
            for _ in 0..copies {
                let id = TrackId(u64::try_from(out.len()).unwrap_or(u64::MAX));
                out.push(Track {
                    id,
                    status: TrackStatus::Confirmed,
                    state: component.mean,
                    covariance: component.cov,
                    misses_since_update: 0,
                    hits: 1,
                });
            }
        }
        Ok(out)
    }
}

/// Moment-match a set of components into one.
fn merge_group(group: &[GaussianComponent]) -> GaussianComponent {
    let weight: f64 = group.iter().map(|c| c.weight).sum();
    if weight <= 0.0 || group.is_empty() {
        return group.first().copied().unwrap_or(GaussianComponent {
            weight: 0.0,
            mean: SVector::<f64, N>::zeros(),
            cov: SMatrix::<f64, N, N>::identity(),
        });
    }
    let mut mean = SVector::<f64, N>::zeros();
    for c in group {
        mean += c.mean * c.weight;
    }
    mean /= weight;
    let mut cov = SMatrix::<f64, N, N>::zeros();
    for c in group {
        let d = c.mean - mean;
        cov += (c.cov + d * d.transpose()) * c.weight;
    }
    cov /= weight;
    GaussianComponent {
        weight,
        mean,
        cov: symmetrize(&cov),
    }
}

fn symmetrize(p: &SMatrix<f64, N, N>) -> SMatrix<f64, N, N> {
    (p + p.transpose()) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_track::ConstantVelocity;

    fn position_h() -> SMatrix<f64, M, N> {
        let mut h = SMatrix::<f64, M, N>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        h
    }

    fn filter() -> PhdFilter {
        PhdFilter::new(
            PhdSettings::default(),
            position_h(),
            SMatrix::<f64, M, M>::identity() * 25.0,
        )
        .expect("valid settings")
    }

    fn birth(position: [f64; 3], weight: f64) -> GaussianComponent {
        let mut mean = SVector::<f64, N>::zeros();
        for axis in 0..3 {
            mean[axis] = position[axis];
        }
        GaussianComponent {
            weight,
            mean,
            cov: SMatrix::<f64, N, N>::from_diagonal(&SVector::<f64, N>::from_column_slice(&[
                100.0, 100.0, 100.0, 400.0, 400.0, 400.0,
            ])),
        }
    }

    fn detection(position: [f64; 3]) -> SVector<f64, M> {
        SVector::<f64, M>::from_column_slice(&position)
    }

    #[test]
    fn an_empty_intensity_reports_no_targets_and_no_tracks() {
        let filter = filter();
        assert!((filter.cardinality() - 0.0).abs() < f64::EPSILON);
        assert!(filter.extract_tracks().expect("valid").is_empty());
    }

    /// The headline property: the cardinality estimate must track the number of targets
    /// actually there. This is what the row's criterion is about.
    #[test]
    fn the_cardinality_converges_to_the_number_of_targets() {
        let mut filter = filter();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let truth = [[0.0, 0.0, 100.0], [300.0, 0.0, 100.0], [0.0, 400.0, 100.0]];
        for scan in 0..25 {
            let births = if scan == 0 {
                truth.iter().map(|p| birth(*p, 0.4)).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            filter.predict(&motion, 1.0, &births).expect("finite");
            let detections: Vec<_> = truth.iter().map(|p| detection(*p)).collect();
            filter.update(&detections).expect("valid");
        }
        let cardinality = filter.cardinality();
        assert!(
            (cardinality - 3.0).abs() < 0.35,
            "three targets, cardinality estimated as {cardinality}"
        );
        let tracks = filter.extract_tracks().expect("valid");
        assert_eq!(tracks.len(), 3, "extracted {} tracks", tracks.len());
    }

    /// A target that stops being detected must fade out of the intensity rather than
    /// persisting forever. This is the other half of what a cardinality estimate is for.
    #[test]
    fn a_target_that_stops_being_detected_fades_from_the_intensity() {
        let mut filter = filter();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        filter
            .predict(&motion, 1.0, &[birth([0.0, 0.0, 100.0], 0.5)])
            .expect("finite");
        for _ in 0..12 {
            filter.predict(&motion, 1.0, &[]).expect("finite");
            filter
                .update(&[detection([0.0, 0.0, 100.0])])
                .expect("valid");
        }
        let established = filter.cardinality();
        assert!(
            established > 0.8,
            "the target never established: cardinality {established}"
        );
        for _ in 0..40 {
            filter.predict(&motion, 1.0, &[]).expect("finite");
            filter.update(&[]).expect("valid");
        }
        let faded = filter.cardinality();
        assert!(
            faded < 0.2,
            "the target did not fade after forty missed scans: cardinality {faded}"
        );
        assert!(
            filter.extract_tracks().expect("valid").is_empty(),
            "a faded target was still extracted as a track"
        );
    }

    /// Merging must actually keep the mixture bounded. Without it the component count
    /// grows by a factor of the detection count every scan, and the filter becomes
    /// unusable after about ten scans of a busy sky.
    #[test]
    fn the_mixture_stays_bounded_over_a_long_dense_run() {
        let mut filter = filter();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let targets: Vec<[f64; 3]> = (0..8)
            .map(|i| {
                let x = f64::from(i) * 200.0;
                [x, 0.0, 100.0]
            })
            .collect();
        for scan in 0..40 {
            let births = if scan % 10 == 0 {
                targets.iter().map(|p| birth(*p, 0.2)).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            filter.predict(&motion, 1.0, &births).expect("finite");
            let detections: Vec<_> = targets.iter().map(|p| detection(*p)).collect();
            filter.update(&detections).expect("valid");
            assert!(
                filter.component_count() <= PhdSettings::default().max_components,
                "the mixture grew to {} components at scan {scan}",
                filter.component_count()
            );
        }
    }

    /// Heavier clutter must make the filter less willing to declare targets. If it did
    /// not, the clutter intensity would be a parameter with no effect.
    #[test]
    fn heavier_clutter_lowers_the_cardinality_estimate() {
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let run = |clutter: f64| {
            let mut filter = PhdFilter::new(
                PhdSettings {
                    clutter_density: clutter,
                    ..PhdSettings::default()
                },
                position_h(),
                SMatrix::<f64, M, M>::identity() * 25.0,
            )
            .expect("valid");
            filter
                .predict(&motion, 1.0, &[birth([0.0, 0.0, 100.0], 0.5)])
                .expect("finite");
            for _ in 0..6 {
                filter.predict(&motion, 1.0, &[]).expect("finite");
                filter
                    .update(&[detection([0.0, 0.0, 100.0])])
                    .expect("valid");
            }
            filter.cardinality()
        };
        let clean = run(1e-8);
        let cluttered = run(1e-1);
        assert!(
            clean > cluttered,
            "clutter did not change the estimate: {clean} vs {cluttered}"
        );
    }

    #[test]
    fn the_unbuilt_filters_say_so_by_name() {
        assert_eq!(
            GlmbFilter.labelled_tracks().unwrap_err(),
            RfsError::NotImplemented {
                what: "labelled multi-Bernoulli filtering",
                waiting_on: "the `rfs` GLMB/LMB row and its Stone Soup oracle",
            }
        );
        assert!(LmbFilter.labelled_tracks().is_err());
    }

    #[test]
    fn a_malformed_scene_is_refused() {
        let err = PhdFilter::new(
            PhdSettings {
                probability_of_detection: 1.5,
                ..PhdSettings::default()
            },
            position_h(),
            SMatrix::<f64, M, M>::identity(),
        )
        .unwrap_err();
        assert!(matches!(err, RfsError::MalformedScene { .. }));
    }

    #[test]
    fn a_non_finite_detection_is_reported() {
        let mut filter = filter();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        filter
            .predict(&motion, 1.0, &[birth([0.0, 0.0, 0.0], 0.5)])
            .expect("finite");
        assert_eq!(
            filter
                .update(&[detection([f64::NAN, 0.0, 0.0])])
                .unwrap_err(),
            RfsError::NotFinite {
                what: "a detection"
            }
        );
    }

    fn cphd(max_cardinality: usize) -> CphdFilter {
        CphdFilter::new(
            PhdSettings::default(),
            position_h(),
            SMatrix::<f64, M, M>::identity() * 25.0,
            max_cardinality,
        )
        .expect("valid settings")
    }

    #[test]
    fn a_new_cphd_is_certain_of_zero_targets() {
        let filter = cphd(10);
        assert_eq!(filter.cardinality_distribution(), {
            let mut expected = vec![0.0; 11];
            expected[0] = 1.0;
            expected
        });
        assert!((filter.cardinality_mean() - 0.0).abs() < f64::EPSILON);
        assert_eq!(filter.cardinality_map(), 0);
        assert!(filter.extract_tracks().expect("valid").is_empty());
    }

    #[test]
    fn a_cardinality_truncated_to_zero_targets_is_refused() {
        let err = CphdFilter::new(
            PhdSettings::default(),
            position_h(),
            SMatrix::<f64, M, M>::identity(),
            0,
        )
        .unwrap_err();
        assert!(matches!(err, RfsError::MalformedScene { .. }));
    }

    /// A basic probability-theory invariant: whatever predict and update do, the result
    /// must still be a distribution. This is checked over several scans of a
    /// multi-target scene with genuine clutter and missed detections, not just on the
    /// starting point-mass, since that is where a bookkeeping error would show up.
    #[test]
    fn the_cardinality_distribution_always_sums_to_one() {
        let mut filter = cphd(20);
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let truth = [[0.0, 0.0, 100.0], [300.0, 0.0, 100.0], [0.0, 400.0, 100.0]];
        for scan in 0..15 {
            let births = if scan == 0 {
                truth.iter().map(|p| birth(*p, 0.4)).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            filter.predict(&motion, 1.0, &births).expect("finite");
            // Every other scan, one target goes unreported -- exactly the situation a
            // plain PHD estimate swings under and a CPHD should not lose track of.
            let detections: Vec<_> = if scan % 2 == 0 {
                truth.iter().map(|p| detection(*p)).collect()
            } else {
                truth[1..].iter().map(|p| detection(*p)).collect()
            };
            filter.update(&detections).expect("valid");
            let total: f64 = filter.cardinality_distribution().iter().sum();
            assert!(
                (total - 1.0).abs() < 1e-9,
                "scan {scan}: cardinality distribution sums to {total}, not 1"
            );
            assert!(
                filter.cardinality_distribution().iter().all(|&p| p >= 0.0),
                "scan {scan}: a negative probability in {:?}",
                filter.cardinality_distribution()
            );
        }
    }

    /// The same headline property [`the_cardinality_converges_to_the_number_of_targets`]
    /// checks for the PHD, restated for the CPHD's own point estimate: the mode of the
    /// full distribution, not the mixture's weight sum.
    #[test]
    fn the_cardinality_map_converges_to_the_number_of_targets() {
        let mut filter = cphd(15);
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let truth = [[0.0, 0.0, 100.0], [300.0, 0.0, 100.0], [0.0, 400.0, 100.0]];
        for scan in 0..25 {
            let births = if scan == 0 {
                truth.iter().map(|p| birth(*p, 0.4)).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            filter.predict(&motion, 1.0, &births).expect("finite");
            let detections: Vec<_> = truth.iter().map(|p| detection(*p)).collect();
            filter.update(&detections).expect("valid");
        }
        assert_eq!(
            filter.cardinality_map(),
            3,
            "three targets, cardinality mode estimated as {}",
            filter.cardinality_map()
        );
        let tracks = filter.extract_tracks().expect("valid");
        assert_eq!(tracks.len(), 3, "extracted {} tracks", tracks.len());
    }

    /// The other half of the same property: a target that stops being detected must
    /// fade from the cardinality distribution, not persist in it forever.
    #[test]
    fn a_target_that_stops_being_detected_fades_from_the_cardinality_distribution() {
        let mut filter = cphd(10);
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        filter
            .predict(&motion, 1.0, &[birth([0.0, 0.0, 100.0], 0.5)])
            .expect("finite");
        for _ in 0..12 {
            filter.predict(&motion, 1.0, &[]).expect("finite");
            filter
                .update(&[detection([0.0, 0.0, 100.0])])
                .expect("valid");
        }
        assert_eq!(
            filter.cardinality_map(),
            1,
            "the target never established: cardinality mode {}",
            filter.cardinality_map()
        );
        for _ in 0..40 {
            filter.predict(&motion, 1.0, &[]).expect("finite");
            filter.update(&[]).expect("valid");
        }
        assert_eq!(
            filter.cardinality_map(),
            0,
            "the target did not fade after forty missed scans: cardinality mode {}",
            filter.cardinality_map()
        );
        assert!(
            filter.extract_tracks().expect("valid").is_empty(),
            "a faded target was still extracted as a track"
        );
    }

    /// A tiny deterministic PRNG so the trial-based comparison below can run many
    /// independent scenarios without a `rand` dependency this crate does not otherwise
    /// need. `SplitMix64` (Steele, Lea and Flood, 2014): fast, well distributed, and short
    /// enough to read in full.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        /// Uniform in `[0, 1)`.
        fn next_unit(&mut self) -> f64 {
            #[allow(clippy::cast_precision_loss)]
            let out = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
            out
        }
    }

    /// The property [`CphdFilter`]'s own doc comment claims: under frequent missed
    /// detections, its cardinality *estimate* swings far less from trial to trial than
    /// the PHD's does, because it tracks the whole distribution rather than only its
    /// mean. This is what CAP-2.4 -- "estimate the number of targets... when tracks
    /// cannot be separated" -- is actually about, so it is measured directly across many
    /// independent trials rather than inferred from the update algebra being
    /// self-consistent. Both filters see the *same* missed-detection pattern and the
    /// same noise per trial (one `SplitMix64` stream per trial, replayed identically for
    /// both filters), so the comparison isolates what the two filters do with identical
    /// evidence.
    #[test]
    fn cphd_shows_less_cardinality_swing_than_phd_under_frequent_missed_detections() {
        const TRIALS: u64 = 500;
        let h = position_h();
        let r = SMatrix::<f64, M, M>::identity() * 25.0;
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        // Everything but `probability_of_detection` stays at the calibrated default
        // this file's own other tests already prove sensible at this covariance scale
        // (`heavier_clutter_lowers_the_cardinality_estimate` demonstrates the default
        // `clutter_density` of 1e-6 is neither so small it is inert nor so large it
        // swamps a genuine detection's likelihood, which is a real, sharp effect in a
        // 3-D Gaussian at this scale: a clutter density picked without checking it
        // against `1 / sqrt((2*pi)^3 * det(S))` for this problem's actual covariance is
        // how the first draft of this test made every scan look like a miss regardless
        // of whether one occurred, at `clutter_density = 2e-3`).
        let settings = PhdSettings {
            probability_of_detection: 0.55, // deliberately low: misses are frequent
            ..PhdSettings::default()
        };

        // Stationary, matching `birth`'s own zero-velocity convention: what is under
        // test is the response to missed detections and clutter, which a moving target
        // would also exercise but only if its birth component's velocity matched the
        // motion exactly -- a second way to get this test wrong that a stationary
        // target sidesteps entirely.
        let true_pos = [0.0, 0.0, 100.0];

        let run = |seed: u64, use_cphd: bool| -> f64 {
            let mut rng = SplitMix64(seed);
            let mut phd = PhdFilter::new(settings, h, r).expect("valid");
            let mut cphd = CphdFilter::new(settings, h, r, 15).expect("valid");
            for scan in 0..30 {
                let births = if scan == 0 {
                    vec![birth(true_pos, 0.9)]
                } else {
                    Vec::new()
                };
                if use_cphd {
                    cphd.predict(&motion, 1.0, &births).expect("finite");
                } else {
                    phd.predict(&motion, 1.0, &births).expect("finite");
                }
                let mut detections = Vec::new();
                if rng.next_unit() < settings.probability_of_detection {
                    // Within about two standard deviations of R (std-dev 5).
                    let noise = (rng.next_unit() * 2.0 - 1.0) * 10.0;
                    detections.push(detection([true_pos[0] + noise, true_pos[1], true_pos[2]]));
                }
                if rng.next_unit() < 0.3 {
                    // Tens of standard deviations away: a genuine outlier, not a
                    // plausible reading of the real target.
                    let noise = (rng.next_unit() * 2.0 - 1.0) * 150.0;
                    detections.push(detection([true_pos[0] + noise, true_pos[1], true_pos[2]]));
                }
                if use_cphd {
                    cphd.update(&detections).expect("valid");
                } else {
                    phd.update(&detections).expect("valid");
                }
            }
            if use_cphd {
                cphd.cardinality_mean()
            } else {
                phd.cardinality()
            }
        };

        let cphd_estimates: Vec<f64> = (0..TRIALS).map(|seed| run(seed, true)).collect();
        let phd_estimates: Vec<f64> = (0..TRIALS).map(|seed| run(seed, false)).collect();

        let variance = |xs: &[f64]| -> f64 {
            #[allow(clippy::cast_precision_loss)]
            let n = xs.len() as f64;
            let mean = xs.iter().sum::<f64>() / n;
            xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n
        };
        let cphd_var = variance(&cphd_estimates);
        let phd_var = variance(&phd_estimates);

        // The measured ratio at these seeds and this scenario is close to 0.62 (a 500-
        // and a 150-trial run agree to within a few percent, so this is the scenario's
        // real effect size, not sampling noise); 0.8 leaves comfortable room without
        // being so loose the assertion stops meaning anything.
        assert!(
            cphd_var < phd_var * 0.8,
            "CPHD should show materially lower cardinality-estimate variance than PHD \
             under frequent missed detections: CPHD {cphd_var}, PHD {phd_var}"
        );
    }
}
