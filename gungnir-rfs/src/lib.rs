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
//! **Built and gated: the Gaussian-mixture PHD filter** ([`PhdFilter`]), **the
//! Gaussian-mixture CPHD filter** ([`CphdFilter`]) **and the labelled multi-Bernoulli
//! filter** ([`LmbFilter`]).
//!
//! **Not built, and returning an explicit error rather than a plausible answer**: the
//! full δ-GLMB ([`GlmbFilter`]). It is named separately from [`LmbFilter`] rather than
//! folded into it, because [`LmbFilter`] implements the LMB *approximation* to it and
//! naming a type for a filter it does not implement is how a reader ends up believing
//! the build has something it does not.
//!
//! **The CPHD and LMB builds are written and gated, not signed.** Both are reached by
//! `docs/agentic-workflow.md`'s numerical-stability clause the same way the PHD filter
//! is (`ARCHITECTURE.md` §10), and neither has a library oracle -- Stone Soup 1.9.1 has
//! no CPHD updater and no labelled filter at all, established by evidence in each
//! generator rather than assumed. Each is gated against this crate's own hand
//! derivation, independently checked before any Rust was written. That is real
//! verification, not a substitute for the owner's review this class of code still needs
//! before it is signed.
//!
//! # Identity is the whole difference between the two halves of this crate
//!
//! [`PhdFilter`] and [`CphdFilter`] answer *how many* and *where*. [`LmbFilter`] answers
//! *which one is which*, and that is the only reason it is a separate row rather than a
//! refinement: a PHD intensity of 1.0 at a point does not say it is the same target that
//! was there last scan, and both PHD-family `extract_tracks` methods mint fresh
//! identifiers every call and say so. [`LmbFilter::extract_tracks`] returns the
//! Bernoulli's own label instead, issued once at birth and never reissued.
//!
//! The two halves are also for different scenes, and the code says which by refusing
//! rather than by convention: exact association marginals are a permanent computation, so
//! [`LmbFilter`] bounds the detections it will accept in one scan
//! ([`RfsError::TooManyDetections`]) and the dense swarm past that bound stays the
//! PHD/CPHD filters', which never form an association at all.
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

/// Generalized Labeled Multi-Bernoulli: the full δ-GLMB, which carries the joint
/// association hypotheses forward between scans instead of projecting them away.
///
/// **Not implemented, and deliberately still named separately from what is.**
/// [`LmbFilter`] is built and gated, and it is the LMB *approximation* to this filter:
/// its single-scan update is the exact δ-GLMB update of an LMB prior, but it then
/// projects the posterior back onto a product of independent Bernoullis, which discards
/// the inter-label dependence a δ-GLMB keeps. What a δ-GLMB would add over it is
/// therefore not accuracy within a scan -- there is none to add, the marginals are exact
/// -- but memory of the dependence *across* scans.
///
/// That difference is measured rather than asserted:
/// `testdata/oracles/tools/gen_lmb_fixtures.py` runs an untruncated δ-GLMB alongside the
/// LMB over the same detections, and `gungnir-rfs/tests/lmb_diff.rs` asserts the fixture
/// still carries the result. See [`LmbFilter`] for the numbers, which are a property of
/// that scenario and not a general bound.
///
/// This type is kept as an explicit refusal rather than deleted because naming it is how
/// a reader can tell which of the two this build has.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlmbFilter;

impl GlmbFilter {
    /// # Errors
    ///
    /// Always. The full δ-GLMB is not built; [`LmbFilter`] is the built approximation to
    /// it and says what it approximates.
    pub fn labelled_tracks(&self) -> Result<Vec<Track>, RfsError> {
        Err(RfsError::NotImplemented {
            what: "the full delta-GLMB filter, which keeps the joint association \
                   hypotheses between scans",
            waiting_on: "a hypothesis-truncation scheme (ranked assignment or Gibbs \
                         sampling); `LmbFilter` is built and is the LMB approximation \
                         to it",
        })
    }
}

/// A birth: where a new target may appear, and how strongly it is believed to be there.
///
/// The label is **not** a field. [`LmbFilter::predict`] mints it from the filter's own
/// monotone counter, because a label chosen by a caller is a label that can be
/// duplicated or reused, and this filter's entire contribution is that its identifiers
/// mean something across scans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LmbBirth {
    /// `r`, the probability a target is actually there. A probability, unlike a
    /// [`GaussianComponent`]'s weight.
    pub existence: f64,
    pub mean: SVector<f64, N>,
    pub cov: SMatrix<f64, N, N>,
}

/// One labelled Bernoulli: a target that exists with probability `existence` and, if it
/// exists, is distributed as `spatial`.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelledBernoulli {
    /// The identity. Allocated once at birth, carried unchanged for as long as this
    /// Bernoulli lives, and never reused after it dies.
    pub label: TrackId,
    /// `r`, in `[0, 1]`. A probability of existence, **not** an expected count: a
    /// [`GaussianComponent`]'s weight can exceed one and this cannot.
    pub existence: f64,
    /// The spatial density given existence, as a Gaussian mixture whose weights sum to
    /// **one**. This is the sharpest structural difference from [`PhdFilter`], whose
    /// mixture weights sum to the expected target count instead: here the count lives
    /// entirely in `existence` and the mixture only says *where*.
    pub spatial: Vec<GaussianComponent>,
}

/// How the labelled filter manages its Bernoullis, and what the scene looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LmbSettings {
    /// `p_S`, the probability a target present last scan is still present.
    pub probability_of_survival: f64,
    /// `p_D`, the probability a present target is detected.
    pub probability_of_detection: f64,
    /// `κ`, the clutter intensity per unit measurement volume.
    pub clutter_density: f64,
    /// A Bernoulli whose existence probability falls below this is dropped and its label
    /// retired. Retired labels are never reissued.
    pub existence_prune_threshold: f64,
    /// Within one label's spatial density, a component holding less than this share of
    /// the label's own mass is dropped. A share of a normalised density, not the PHD's
    /// absolute intensity weight, which is why it is not that row's `1e-5`.
    pub spatial_prune_threshold: f64,
    /// Components within this squared Mahalanobis distance of a stronger one in the
    /// same label are merged into it.
    pub merge_distance: f64,
    /// Each label's spatial mixture is truncated to this many components.
    pub max_components_per_label: usize,
    /// The filter keeps at most this many Bernoullis, strongest existence first.
    ///
    /// **This truncation is an approximation and the only one in the filter besides the
    /// LMB projection itself**: a discarded Bernoulli is a target the filter has stopped
    /// believing in on grounds of budget rather than evidence. It is bounded, not
    /// silent -- the count is observable through [`LmbFilter::bernoullis`].
    pub max_bernoullis: usize,
    /// A Bernoulli is extracted as a track above this existence probability.
    pub extraction_threshold: f64,
    /// The largest number of detections in one scan this filter will accept.
    ///
    /// **Not a performance knob: a statement about what is exactly computable.** The
    /// association marginals below are a permanent, whose exact evaluation is `#P`-hard;
    /// this filter computes them exactly in `O(n · 2^m · m)` time and `O(n · 2^m)` space
    /// for `n` Bernoullis and `m` detections, and refuses past the bound rather than
    /// silently truncating the hypothesis space and reporting the result as if it were
    /// exact. A scene denser than this is [`PhdFilter`]'s and [`CphdFilter`]'s regime,
    /// where no association is formed at all.
    ///
    /// At the default 12 and 100 Bernoullis the two tables are about 3 MB each; the
    /// settings validation refuses anything above 16, where they are 53 MB each.
    pub max_detections_per_scan: usize,
}

impl Default for LmbSettings {
    /// The same sky as [`PhdSettings::default`] -- survival, detection, clutter and
    /// merge distance are shared, so the three `rfs` rows are validated against one
    /// scene and differ only in the filter. The values here that have no PHD counterpart
    /// are the ones the oracle fixture uses.
    fn default() -> Self {
        Self {
            probability_of_survival: 0.99,
            probability_of_detection: 0.95,
            clutter_density: 1e-6,
            existence_prune_threshold: 1e-4,
            spatial_prune_threshold: 1e-6,
            merge_distance: 4.0,
            max_components_per_label: 20,
            max_bernoullis: 100,
            extraction_threshold: 0.5,
            max_detections_per_scan: 12,
        }
    }
}

impl LmbSettings {
    fn validate(&self) -> Result<(), RfsError> {
        let probabilities_valid = (0.0..=1.0).contains(&self.probability_of_survival)
            && (0.0..=1.0).contains(&self.probability_of_detection);
        if !probabilities_valid {
            return Err(RfsError::MalformedScene {
                what: "the survival or detection probability is not a probability",
            });
        }
        if !self.clutter_density.is_finite() || self.clutter_density <= 0.0 {
            return Err(RfsError::MalformedScene {
                what: "the clutter intensity must be finite and positive: every \
                       detection's association weight is divided by it",
            });
        }
        if self.max_components_per_label == 0 || self.max_bernoullis == 0 {
            return Err(RfsError::MalformedScene {
                what: "a filter truncated to no components or no Bernoullis represents \
                       nothing",
            });
        }
        if self.max_detections_per_scan > MAX_SUPPORTED_DETECTIONS {
            return Err(RfsError::MalformedScene {
                what: "the exact association marginals are exponential in the detection \
                       count; this bound is past what the tables can be allocated for",
            });
        }
        Ok(())
    }
}

/// The hard ceiling on [`LmbSettings::max_detections_per_scan`]: `2^16` subsets times a
/// hundred Bernoullis is already 53 MB per dynamic-programming table, and the next
/// doubling is not a bound worth offering.
const MAX_SUPPORTED_DETECTIONS: usize = 16;

/// Index into an association-marginal row: the label does not exist.
const ASSOCIATION_ABSENT: usize = 0;
/// Index into an association-marginal row: the label exists and was not detected.
const ASSOCIATION_MISSED: usize = 1;
/// Index into an association-marginal row of detection `j`: `ASSOCIATION_FIRST + j`.
const ASSOCIATION_FIRST_DETECTION: usize = 2;

/// Labelled Multi-Bernoulli filter: a set filter that carries target **identity**.
///
/// # What this is, and what it is not
///
/// This is the LMB filter of Reuter, Vo, Vo and Dietmayer ("The Labeled Multi-Bernoulli
/// Filter", IEEE Transactions on Signal Processing 62(12), 2014). It is **not** the full
/// δ-GLMB; [`GlmbFilter`] is still an explicit refusal, and this type is deliberately
/// not named for one it does not implement.
///
/// Its update is the **exact** δ-GLMB update of an LMB prior -- every association
/// hypothesis, enumerated with no sampling, no ranked-assignment truncation and no
/// gating -- followed by a moment-matched projection back onto an LMB. Precisely one
/// thing is approximated by that projection, and it is worth stating exactly: the true
/// posterior couples the labels (if label 1 took detection 3, label 2 cannot have), and
/// an LMB is a product of independent Bernoullis. **Every per-label marginal is exact
/// after a single update** -- existence probability, spatial density, and hence the PHD.
/// The error is what the *next* scan inherits, because it starts from the projected
/// product rather than the true joint.
///
/// That is measured, not asserted. `testdata/oracles/tools/gen_lmb_fixtures.py` runs an
/// untruncated δ-GLMB beside this filter on the same detections and records the largest
/// existence-probability disagreement per scan. On its two-target scene the gap is
/// `1.1e-16` after the first update -- zero to machine precision, as the derivation
/// requires -- then `5.2e-7` and `1.1e-4`. Those are that scenario's numbers, not a
/// general bound.
///
/// # Why this is a separate row from [`PhdFilter`] rather than a refinement of it
///
/// A PHD intensity says "there is about one target here". It does not say it is the same
/// target that was there last scan, and [`PhdFilter::extract_tracks`] says so by minting
/// fresh identifiers every call. **This filter's [`TrackId`] is the Bernoulli's label**:
/// allocated once at birth, carried unchanged for the Bernoulli's whole life, and never
/// reissued after it dies. `gungnir-rfs/tests/lmb_label_continuity.rs` gates that
/// property directly, including through a crossing where the two targets occupy the same
/// point at the same scan.
///
/// # No library oracle exists for this row, established rather than assumed
///
/// `docs/verification-capability-table.md` named "Stone Soup GLMB (partial)". Stone Soup
/// 1.9.1 -- the pinned version, already driven for real for the PHD row -- has **no GLMB
/// and no LMB at all**: not partial, absent. Checked three ways in the generator, which
/// refuses to run if any of them ever finds one: no module in the package is named for
/// one; a regex scan of every `.py` file it ships finds **zero** source lines mentioning
/// GLMB, LMB or labelled multi-Bernoulli; and all three plausible import paths raise
/// `ModuleNotFoundError`. Its nearest neighbours are the *single-target*
/// `BernoulliParticleUpdater` and the *unlabelled* `PHDUpdater`/`LCCUpdater`, neither of
/// which is a labelled multi-target filter.
///
/// The oracle is therefore the generator's own derivation, whose association marginals
/// are produced by **literal enumeration** of every association event -- the definition
/// transcribed, with no bookkeeping to get wrong -- while this filter runs the fast
/// dynamic program below. Five independent checks are run before any fixture is written;
/// see the generator's module docstring for all five, and
/// `gungnir-rfs/tests/lmb_diff.rs` for the ones re-asserted here.
///
/// # The scenes this filter is for
///
/// [`LmbSettings::max_detections_per_scan`] bounds it, because exact association
/// marginals are a permanent computation. This filter is for the case where *which one
/// is which* matters and there are few enough returns to answer it exactly; the dense
/// swarm, where the association cannot be formed at all, is [`PhdFilter`]'s and
/// [`CphdFilter`]'s. Past the bound this filter refuses rather than approximating
/// quietly.
/// Per label, per spatial component: the Kalman quantities [`LmbFilter::update`] reuses
/// across every detection, and that component's likelihood for each of them.
///
/// The **prior** covariance is carried alongside the updated one because the
/// missed-detection branch keeps the prior and every detection branch takes the update.
/// Conflating the two would leave the filter right on cardinality and wrong on spread,
/// which is the quiet kind of wrong -- it looks correct in every count-based test.
struct Prepared {
    weight: f64,
    mean: SVector<f64, N>,
    prior_cov: SMatrix<f64, N, N>,
    gain: SMatrix<f64, N, M>,
    updated_cov: SMatrix<f64, N, N>,
    likelihoods: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct LmbFilter {
    bernoullis: Vec<LabelledBernoulli>,
    settings: LmbSettings,
    h: SMatrix<f64, M, N>,
    r: SMatrix<f64, M, M>,
    next_label: u64,
    last_association: Vec<(TrackId, Vec<f64>)>,
}

impl LmbFilter {
    /// An empty filter: no targets, no labels issued yet.
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] when the settings do not describe a scene.
    pub fn new(
        settings: LmbSettings,
        h: SMatrix<f64, M, N>,
        r: SMatrix<f64, M, M>,
    ) -> Result<Self, RfsError> {
        settings.validate()?;
        Ok(Self {
            bernoullis: Vec::new(),
            settings,
            h,
            r,
            next_label: 0,
            last_association: Vec::new(),
        })
    }

    /// The Bernoullis, in ascending label order.
    #[must_use]
    pub fn bernoullis(&self) -> &[LabelledBernoulli] {
        &self.bernoullis
    }

    /// The labels currently alive, ascending. Two calls a scan apart returning the same
    /// label mean the same target -- that is the whole promise of this filter.
    #[must_use]
    pub fn labels(&self) -> Vec<TrackId> {
        self.bernoullis.iter().map(|b| b.label).collect()
    }

    /// This label's existence probability, or `None` if it is not alive.
    #[must_use]
    pub fn existence_of(&self, label: TrackId) -> Option<f64> {
        self.bernoullis
            .iter()
            .find(|b| b.label == label)
            .map(|b| b.existence)
    }

    /// The expected number of targets: the sum of the existence probabilities.
    ///
    /// Comparable to [`PhdFilter::cardinality`] and arrived at very differently -- there
    /// it is the integral of an intensity, here it is the mean of a sum of independent
    /// Bernoullis, which is also the point at which this filter's cardinality
    /// distribution is a Poisson-binomial rather than the exact one a δ-GLMB would carry.
    #[must_use]
    pub fn cardinality(&self) -> f64 {
        self.bernoullis.iter().map(|b| b.existence).sum()
    }

    /// The association marginals the last [`Self::update`] computed, per label as it
    /// stood **before** that update (a label can be pruned by it).
    ///
    /// Each row is a probability distribution over `[does not exist, exists but was not
    /// detected, produced detection 0, produced detection 1, ...]`. This is the
    /// "label-to-track assignment" half of the row's own criterion, exposed so it can be
    /// compared directly rather than inferred from where the means ended up -- and it is
    /// the honest output when the answer is a tie: two targets at the same point at the
    /// same scan produce a row of 0.5 and 0.5, not a fabricated certainty.
    #[must_use]
    pub fn last_association(&self) -> &[(TrackId, Vec<f64>)] {
        &self.last_association
    }

    /// Propagate every Bernoulli forward, then admit this scan's births under **fresh
    /// labels**.
    ///
    /// Survival thins the existence probability by `p_S` and leaves the spatial density's
    /// mixture weights alone -- they are a normalised density, and thinning them would
    /// double-count the survival that `existence` already carries. Each birth is issued
    /// the next label from a monotone counter that is never rewound, so a label that has
    /// been retired cannot come back attached to a different target.
    ///
    /// # Errors
    ///
    /// [`RfsError::NotFinite`] for a birth that is not a number, and
    /// [`RfsError::MalformedScene`] for a birth whose existence is not a probability.
    pub fn predict<Motion>(
        &mut self,
        motion: &Motion,
        dt: f64,
        births: &[LmbBirth],
    ) -> Result<(), RfsError>
    where
        Motion: MotionModel<N>,
    {
        let f = motion.f(dt);
        let q = motion.q(dt);
        for bernoulli in &mut self.bernoullis {
            bernoulli.existence *= self.settings.probability_of_survival;
            for component in &mut bernoulli.spatial {
                component.mean = f * component.mean;
                let predicted = f * component.cov * f.transpose() + q;
                component.cov = symmetrize(&predicted);
            }
        }
        for birth in births {
            if !birth.existence.is_finite()
                || !birth.mean.iter().all(|v| v.is_finite())
                || !birth.cov.iter().all(|v| v.is_finite())
            {
                return Err(RfsError::NotFinite { what: "a birth" });
            }
            if !(0.0..=1.0).contains(&birth.existence) {
                return Err(RfsError::MalformedScene {
                    what: "a birth's existence is not a probability",
                });
            }
            let label = TrackId(self.next_label);
            self.next_label = self.next_label.saturating_add(1);
            self.bernoullis.push(LabelledBernoulli {
                label,
                existence: birth.existence,
                spatial: vec![GaussianComponent {
                    // The spatial density is normalised: all of the "how strongly do we
                    // believe this" lives in `existence`, none of it here.
                    weight: 1.0,
                    mean: birth.mean,
                    cov: birth.cov,
                }],
            });
        }
        self.bernoullis.sort_by_key(|b| b.label.0);
        Ok(())
    }

    /// Update every Bernoulli with this scan's detections: the exact δ-GLMB update of the
    /// current LMB, projected back onto an LMB.
    ///
    /// Per label `ℓ` and extended association `a ∈ {absent, missed, z_0 .. z_{m-1}}` the
    /// unnormalised weight is
    ///
    /// ```text
    /// u_ℓ(absent) = 1 - r_ℓ
    /// u_ℓ(missed) = r_ℓ (1 - p_D)
    /// u_ℓ(z_j)    = r_ℓ p_D q_ℓ(j) / κ,   q_ℓ(j) = <p_ℓ, g(z_j | ·)>
    /// ```
    ///
    /// and the exact posterior over joint assignments is their product over labels,
    /// normalised, restricted to assignments injective on the detections. Only the
    /// per-label marginals of that posterior are needed, and this crate's own
    /// `association_marginals` computes them exactly. The projection then sets
    /// `r̂_ℓ = 1 - P(a_ℓ = absent)` and mixes each label's missed-detection branch (which
    /// keeps the **prior** covariance) with one Kalman-updated branch per detection,
    /// weighted by that detection's marginal.
    ///
    /// An unassigned detection is clutter and contributes weight 1 here, because the
    /// clutter density it would contribute is exactly the one already divided out of
    /// every `u_ℓ(z_j)`.
    ///
    /// # Errors
    ///
    /// [`RfsError::NotFinite`] for a detection that is not a number;
    /// [`RfsError::TooManyDetections`] past
    /// [`LmbSettings::max_detections_per_scan`], which is a refusal to approximate
    /// rather than a failure; [`RfsError::SingularCovariance`] for an innovation
    /// covariance that cannot be inverted; and [`RfsError::MalformedScene`] when no
    /// assignment of this scan has any weight at all, which no real scene produces and
    /// which is named here rather than dividing by the zero it would otherwise give.
    // One cohesive derivation (see this method's own doc comment for the maths); the
    // per-component Kalman preparation, the association weights and the moment-matched
    // projection read as three paragraphs of one argument, and splitting them at a line
    // count would scatter `prepared`, `u` and `marginals` across function boundaries
    // rather than make any of it clearer. The same judgement `CphdFilter::update` records.
    #[allow(clippy::too_many_lines)]
    pub fn update(&mut self, detections: &[SVector<f64, M>]) -> Result<(), RfsError> {
        for z in detections {
            if !z.iter().all(|v| v.is_finite()) {
                return Err(RfsError::NotFinite {
                    what: "a detection",
                });
            }
        }
        if detections.len() > self.settings.max_detections_per_scan {
            return Err(RfsError::TooManyDetections {
                detections: detections.len(),
                limit: self.settings.max_detections_per_scan,
            });
        }
        self.last_association.clear();
        if self.bernoullis.is_empty() {
            return Ok(());
        }

        let mut prepared: Vec<Vec<Prepared>> = Vec::with_capacity(self.bernoullis.len());
        let mut u: Vec<Vec<f64>> = Vec::with_capacity(self.bernoullis.len());
        #[allow(clippy::cast_precision_loss)]
        let m_dim = M as f64;

        for bernoulli in &self.bernoullis {
            let mut per_component = Vec::with_capacity(bernoulli.spatial.len());
            for component in &bernoulli.spatial {
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
                let gain = pht * s_inv;
                let i_kh = SMatrix::<f64, N, N>::identity() - gain * self.h;
                let updated_cov = symmetrize(
                    &(i_kh * component.cov * i_kh.transpose() + gain * self.r * gain.transpose()),
                );
                let predicted_z = self.h * component.mean;
                let normaliser = ((2.0 * std::f64::consts::PI).powf(m_dim) * determinant).sqrt();
                let likelihoods = detections
                    .iter()
                    .map(|z| {
                        let y = z - predicted_z;
                        let quadratic = (y.transpose() * s_inv * y)[(0, 0)];
                        (-0.5 * quadratic).exp() / normaliser
                    })
                    .collect();
                per_component.push(Prepared {
                    weight: component.weight,
                    mean: component.mean,
                    prior_cov: component.cov,
                    gain,
                    updated_cov,
                    likelihoods,
                });
            }

            // The label's predictive likelihood for each detection, under its own
            // normalised spatial density.
            let q: Vec<f64> = (0..detections.len())
                .map(|j| {
                    per_component
                        .iter()
                        .map(|p| p.weight * p.likelihoods[j])
                        .sum()
                })
                .collect();
            let mut row = Vec::with_capacity(ASSOCIATION_FIRST_DETECTION + detections.len());
            row.push(1.0 - bernoulli.existence);
            row.push(bernoulli.existence * (1.0 - self.settings.probability_of_detection));
            row.extend(q.iter().map(|qj| {
                bernoulli.existence * self.settings.probability_of_detection * qj
                    / self.settings.clutter_density
            }));
            u.push(row);
            prepared.push(per_component);
        }

        let Some(marginals) = association_marginals(&u, detections.len()) else {
            return Err(RfsError::MalformedScene {
                what: "no assignment of this scan to these targets has any weight",
            });
        };

        let mut updated = Vec::with_capacity(self.bernoullis.len());
        for ((bernoulli, per_component), probability) in
            self.bernoullis.iter().zip(&prepared).zip(marginals.iter())
        {
            self.last_association
                .push((bernoulli.label, probability.clone()));

            let existence = 1.0 - probability[ASSOCIATION_ABSENT];
            if !existence.is_finite() || existence <= self.settings.existence_prune_threshold {
                continue;
            }

            let mut mixture = Vec::with_capacity(per_component.len() * (1 + detections.len()));
            let missed = probability[ASSOCIATION_MISSED];
            if missed > 0.0 {
                for p in per_component {
                    mixture.push(GaussianComponent {
                        weight: missed * p.weight,
                        mean: p.mean,
                        cov: p.prior_cov,
                    });
                }
            }
            for (j, z) in detections.iter().enumerate() {
                let p_j = probability[ASSOCIATION_FIRST_DETECTION + j];
                if p_j <= 0.0 {
                    continue;
                }
                let q_j: f64 = per_component
                    .iter()
                    .map(|p| p.weight * p.likelihoods[j])
                    .sum();
                if q_j <= 0.0 {
                    continue;
                }
                for p in per_component {
                    let share = p.weight * p.likelihoods[j] / q_j;
                    if share <= 0.0 {
                        continue;
                    }
                    let y = z - self.h * p.mean;
                    mixture.push(GaussianComponent {
                        weight: p_j * share,
                        mean: p.mean + p.gain * y,
                        cov: p.updated_cov,
                    });
                }
            }
            if mixture.is_empty() {
                continue;
            }

            // Kept before pruning, for the guard below.
            let strongest = mixture.iter().copied().reduce(|best, candidate| {
                if candidate.weight > best.weight {
                    candidate
                } else {
                    best
                }
            });

            // The mixture's mass is `existence` by construction; the spatial density is
            // what is left after dividing that out, and it must integrate to one.
            let mut spatial = prune_and_merge_mixture(
                mixture,
                self.settings.spatial_prune_threshold * existence,
                self.settings.merge_distance,
                self.settings.max_components_per_label,
            );
            if spatial.is_empty() {
                // Not reachable at any configured threshold -- the mixture's mass is
                // `existence` over at most `max_components_per_label * (1 + detections)`
                // components, so the strongest is far above `1e-6 * existence`. It is
                // guarded anyway because the alternative is *dropping the label*, and a
                // label that disappears without its existence probability ever falling
                // is a silent loss of identity: precisely the failure this filter exists
                // to prevent, and one no cardinality-based test would notice.
                spatial = strongest.into_iter().collect();
            }
            let mass: f64 = spatial.iter().map(|c| c.weight).sum();
            if mass <= 0.0 || !mass.is_finite() {
                continue;
            }
            let spatial = spatial
                .into_iter()
                .map(|c| GaussianComponent {
                    weight: c.weight / mass,
                    ..c
                })
                .collect();
            updated.push(LabelledBernoulli {
                label: bernoulli.label,
                existence,
                spatial,
            });
        }

        if updated.len() > self.settings.max_bernoullis {
            updated.sort_by(|a, b| b.existence.total_cmp(&a.existence));
            updated.truncate(self.settings.max_bernoullis);
        }
        updated.sort_by_key(|b| b.label.0);
        self.bernoullis = updated;
        Ok(())
    }

    /// Commit to a target set: one track per Bernoulli above the extraction threshold,
    /// **carrying its label as the [`TrackId`]**.
    ///
    /// This is the one place in this crate where a returned identifier means something
    /// across scans. [`PhdFilter::extract_tracks`] and [`CphdFilter::extract_tracks`]
    /// both mint fresh identifiers every call and document that they do, because a PHD
    /// intensity carries no identity; here the identifier *is* the Bernoulli's label,
    /// issued once at birth and never reissued.
    ///
    /// `hits` is not a hit count -- this filter keeps no such history -- and is reported
    /// as 1 for every extracted track rather than fabricated. A consumer wanting a hit
    /// history should count the scans in which [`Self::last_association`] shows the
    /// label associated to a detection.
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] if an existence probability is not finite, which
    /// means the filter state is already broken.
    ///
    /// **Returns a `Result` rather than an empty `Vec`**, for the reason
    /// [`PhdFilter::extract_tracks`] gives: an empty track set is a claim that nothing is
    /// out there, and it is the most dangerous empty in this system.
    pub fn extract_tracks(&self) -> Result<Vec<Track>, RfsError> {
        let mut out = Vec::new();
        for bernoulli in &self.bernoullis {
            if !bernoulli.existence.is_finite() {
                return Err(RfsError::MalformedScene {
                    what: "an existence probability is not finite",
                });
            }
            if bernoulli.existence <= self.settings.extraction_threshold {
                continue;
            }
            let mut mean = SVector::<f64, N>::zeros();
            for component in &bernoulli.spatial {
                mean += component.mean * component.weight;
            }
            let mut covariance = SMatrix::<f64, N, N>::zeros();
            for component in &bernoulli.spatial {
                let d = component.mean - mean;
                covariance += (component.cov + d * d.transpose()) * component.weight;
            }
            out.push(Track {
                id: bernoulli.label,
                status: TrackStatus::Confirmed,
                state: mean,
                covariance: symmetrize(&covariance),
                misses_since_update: 0,
                hits: 1,
            });
        }
        Ok(out)
    }
}

/// The exact per-label association marginals of the δ-GLMB posterior of an LMB prior.
///
/// `u[l]` is label `l`'s extended-association weight vector, indexed by
/// [`ASSOCIATION_ABSENT`], [`ASSOCIATION_MISSED`] and
/// `ASSOCIATION_FIRST_DETECTION + j`. Returns one probability distribution per label, or
/// `None` when no assignment has any weight.
///
/// # The algorithm, and why it is not the enumeration the oracle uses
///
/// The normalising sum runs over every assignment vector injective on the detections,
/// which is a permanent -- `#P`-hard in general, and there is no polynomial exact
/// algorithm to reach for. What there is, is a dynamic program whose state is the **set
/// of detections already consumed**, which is exponential in the detections only:
///
/// ```text
/// F[l][S] = weight of assignments of labels 0..l-1 consuming EXACTLY the set S
/// B[l][S] = weight of assignments of labels l..n-1 consuming ANY SUBSET of S
/// ```
///
/// with `c_l = u_l(absent) + u_l(missed)` the weight of label `l` consuming no detection.
/// Label `l` takes detection `j` exactly when some prefix consumed a set `S` without
/// `j` and the suffix consumed a subset of what remains, which is the marginal below.
/// `absent` and `missed` have identical combinatorial structure, so they are summed
/// together and split in proportion afterwards.
///
/// The oracle in `testdata/oracles/tools/gen_lmb_fixtures.py` deliberately does **not**
/// use this recursion: it enumerates every association event literally, and a third
/// implementation there transposes the index order (a dynamic program over detections
/// whose state is the set of labels consumed) to check the normaliser a third way. All
/// three agree to `2.5e-15` relative, which is the evidence that this program's
/// bookkeeping is right; a shared derivation could not have provided it.
///
/// # Scaling
///
/// Each label's weight vector is divided by its own largest entry first. Scaling one
/// label scales every joint assignment weight by the same factor, so the marginals are
/// unchanged -- but the products taken across labels are not, and with `κ` at `1e-6` a
/// per-label weight of order `1e4` raised to the number of labels overflows a `f64`
/// long before the scene is interesting.
fn association_marginals(u: &[Vec<f64>], detection_count: usize) -> Option<Vec<Vec<f64>>> {
    let n = u.len();
    let width = ASSOCIATION_FIRST_DETECTION + detection_count;
    if n == 0 {
        return Some(Vec::new());
    }
    let size = 1_usize.checked_shl(u32::try_from(detection_count).ok()?)?;
    let full = size - 1;

    let scaled: Vec<Vec<f64>> = u
        .iter()
        .map(|row| {
            let peak = row.iter().copied().fold(0.0_f64, f64::max);
            if peak > 0.0 && peak.is_finite() {
                row.iter().map(|v| v / peak).collect()
            } else {
                row.clone()
            }
        })
        .collect();
    let c: Vec<f64> = scaled
        .iter()
        .map(|row| row[ASSOCIATION_ABSENT] + row[ASSOCIATION_MISSED])
        .collect();

    let mut forward = vec![vec![0.0_f64; size]; n + 1];
    forward[0][0] = 1.0;
    for l in 0..n {
        // Split the borrow rather than index the same `Vec` twice: the recursion reads
        // row `l` and writes row `l + 1`, which is a disjoint pair and not an aliasing
        // question the reader should have to resolve for themselves.
        let (head, tail) = forward.split_at_mut(l + 1);
        let (previous, next) = (&head[l], &mut tail[0]);
        for (s, &here) in previous.iter().enumerate() {
            if here == 0.0 {
                continue;
            }
            next[s] += here * c[l];
            for j in 0..detection_count {
                if s & (1 << j) != 0 {
                    continue;
                }
                next[s | (1 << j)] += here * scaled[l][ASSOCIATION_FIRST_DETECTION + j];
            }
        }
    }

    let mut backward = vec![vec![0.0_f64; size]; n + 1];
    backward[n].fill(1.0);
    for l in (0..n).rev() {
        let (head, tail) = backward.split_at_mut(l + 1);
        let (current, next) = (&mut head[l], &tail[0]);
        for (s, slot) in current.iter_mut().enumerate() {
            let mut acc = next[s] * c[l];
            for j in 0..detection_count {
                if s & (1 << j) != 0 {
                    acc += next[s & !(1 << j)] * scaled[l][ASSOCIATION_FIRST_DETECTION + j];
                }
            }
            *slot = acc;
        }
    }

    let total = backward[0][full];
    if total <= 0.0 || !total.is_finite() {
        return None;
    }

    let mut out = vec![vec![0.0_f64; width]; n];
    for (l, row) in out.iter_mut().enumerate() {
        let mut no_detection = 0.0;
        for (s, &here) in forward[l].iter().enumerate() {
            if here == 0.0 {
                continue;
            }
            let rest = full & !s;
            // Labels after `l` may use anything left over; `backward` is indexed by the
            // set they are ALLOWED, not the set they consume, which is why `rest` and
            // not an exact-set lookup appears here.
            no_detection += here * c[l] * backward[l + 1][rest];
            for j in 0..detection_count {
                if s & (1 << j) != 0 {
                    continue;
                }
                row[ASSOCIATION_FIRST_DETECTION + j] += here
                    * scaled[l][ASSOCIATION_FIRST_DETECTION + j]
                    * backward[l + 1][rest & !(1 << j)];
            }
        }
        if c[l] > 0.0 {
            row[ASSOCIATION_ABSENT] = no_detection * scaled[l][ASSOCIATION_ABSENT] / c[l];
            row[ASSOCIATION_MISSED] = no_detection * scaled[l][ASSOCIATION_MISSED] / c[l];
        }
        for value in row.iter_mut() {
            *value /= total;
        }
    }
    Some(out)
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
    /// More detections in one scan than [`LmbSettings::max_detections_per_scan`] allows.
    ///
    /// **A refusal, not a failure.** [`LmbFilter`] computes its association marginals
    /// exactly, and doing so is exponential in the detection count; past the bound it
    /// says so rather than truncating the hypothesis space and reporting an
    /// approximation as though it were the exact answer. A scene this dense belongs to
    /// [`PhdFilter`] or [`CphdFilter`], which form no association at all.
    #[error(
        "{detections} detections in one scan is past the exact-association bound of \
         {limit}: a scene this dense is the PHD/CPHD filters' regime, not the labelled \
         filter's"
    )]
    TooManyDetections { detections: usize, limit: usize },
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
        let taken = std::mem::take(&mut self.intensity_components);
        self.intensity_components = prune_and_merge_mixture(
            taken,
            self.settings.prune_threshold,
            self.settings.merge_distance,
            self.settings.max_components,
        );
    }
}

/// Drop components below `prune_threshold`, merge everything within `merge_distance`
/// squared Mahalanobis of a stronger component into it, sort strongest first and
/// truncate to `max_components`.
///
/// Split out of [`PhdFilter::prune_and_merge`] unchanged so [`LmbFilter`] can use the
/// same mixture management on each label's spatial density: keeping a mixture to a
/// workable size is the same operation whether the weights are an intensity's or a
/// normalised density's, and having two copies of it would be two things to keep in
/// agreement with the oracle generators, which share one implementation of it.
fn prune_and_merge_mixture(
    components: Vec<GaussianComponent>,
    prune_threshold: f64,
    merge_distance: f64,
    max_components: usize,
) -> Vec<GaussianComponent> {
    let mut remaining = components;
    remaining.retain(|c| c.weight > prune_threshold && c.weight.is_finite());

    {
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
                if distance <= merge_distance {
                    group.push(*c);
                    false
                } else {
                    true
                }
            });
            merged.push(merge_group(&group));
        }

        merged.sort_by(|a, b| b.weight.total_cmp(&a.weight));
        merged.truncate(max_components);
        merged
    }
}

impl PhdFilter {
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

    /// The refusal must name the *full δ-GLMB* and must not be worded so that a reader
    /// concludes labelled filtering as a whole is missing -- `LmbFilter` is built, and
    /// the point of keeping this type is to say precisely which of the two is not.
    #[test]
    fn the_unbuilt_filter_is_the_full_glmb_and_says_so_by_name() {
        let err = GlmbFilter.labelled_tracks().unwrap_err();
        let RfsError::NotImplemented { what, waiting_on } = err else {
            panic!("the delta-GLMB must refuse with NotImplemented, got {err}");
        };
        assert!(
            what.contains("delta-GLMB"),
            "the refusal must name the delta-GLMB, not labelled filtering in general: {what}"
        );
        assert!(
            waiting_on.contains("LmbFilter"),
            "the refusal must point at the filter that IS built: {waiting_on}"
        );
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

    // ------------------------------------------------------------------------------
    // LmbFilter
    // ------------------------------------------------------------------------------

    fn lmb() -> LmbFilter {
        LmbFilter::new(
            LmbSettings::default(),
            position_h(),
            SMatrix::<f64, M, M>::identity() * 25.0,
        )
        .expect("valid settings")
    }

    fn lmb_birth(position: [f64; 3], velocity: [f64; 3]) -> LmbBirth {
        let mut mean = SVector::<f64, N>::zeros();
        for axis in 0..3 {
            mean[axis] = position[axis];
            mean[3 + axis] = velocity[axis];
        }
        LmbBirth {
            existence: 0.4,
            mean,
            cov: SMatrix::<f64, N, N>::from_diagonal(&SVector::<f64, N>::from_column_slice(&[
                100.0, 100.0, 100.0, 400.0, 400.0, 400.0,
            ])),
        }
    }

    /// The association marginals by literal enumeration of every extended assignment
    /// vector, filtered for injectivity on the detections. Manifestly the definition,
    /// with no bookkeeping to get wrong -- and deliberately not the recursion
    /// [`association_marginals`] uses, so it is a second derivation rather than the same
    /// one run twice. Feasible only for the tiny cases the tests below use.
    fn marginals_by_brute_force(u: &[Vec<f64>], detections: usize) -> Vec<Vec<f64>> {
        let n = u.len();
        let width = ASSOCIATION_FIRST_DETECTION + detections;
        let mut out = vec![vec![0.0_f64; width]; n];
        let mut total = 0.0;
        let mut assignment = vec![0_usize; n];
        // Odometer over {0..width}^n.
        loop {
            let claimed: Vec<usize> = assignment
                .iter()
                .copied()
                .filter(|a| *a >= ASSOCIATION_FIRST_DETECTION)
                .collect();
            let mut seen = claimed.clone();
            seen.sort_unstable();
            seen.dedup();
            if seen.len() == claimed.len() {
                let weight: f64 = assignment
                    .iter()
                    .enumerate()
                    .map(|(l, &a)| u[l][a])
                    .product();
                if weight > 0.0 {
                    total += weight;
                    for (l, &a) in assignment.iter().enumerate() {
                        out[l][a] += weight;
                    }
                }
            }
            let mut position = 0;
            loop {
                if position == n {
                    for row in &mut out {
                        for value in row.iter_mut() {
                            *value /= total;
                        }
                    }
                    return out;
                }
                assignment[position] += 1;
                if assignment[position] < width {
                    break;
                }
                assignment[position] = 0;
                position += 1;
            }
        }
    }

    fn random_association_weights(rng: &mut SplitMix64, n: usize, m: usize) -> Vec<Vec<f64>> {
        (0..n)
            .map(|_| {
                let r = 0.05 + rng.next_unit() * 0.9;
                let mut row = vec![1.0 - r, r * (0.02 + rng.next_unit() * 0.48)];
                row.extend((0..m).map(|_| r * (1e-3 + rng.next_unit() * 50.0)));
                row
            })
            .collect()
    }

    /// The check that stands behind the whole filter: the `O(n · 2^m · m)` dynamic
    /// program must agree with literal enumeration of every association event. The
    /// oracle generator runs the same comparison in numpy against a third
    /// implementation; this one keeps it inside the crate, so a change to the recursion
    /// fails here without needing a fixture regenerated.
    #[test]
    fn the_association_marginals_match_a_brute_force_enumeration() {
        let mut rng = SplitMix64(0xA55E_C1A7_1057);
        let mut worst = 0.0_f64;
        for trial in 0..200 {
            let n = 1 + (trial % 4);
            let m = trial % 4;
            let u = random_association_weights(&mut rng, n, m);
            let dp = association_marginals(&u, m).expect("a weighted scene");
            let brute = marginals_by_brute_force(&u, m);
            for (a, b) in dp.iter().zip(&brute) {
                for (x, y) in a.iter().zip(b) {
                    worst = worst.max((x - y).abs());
                }
            }
        }
        assert!(
            worst < 1e-12,
            "the association dynamic program disagrees with brute-force enumeration by \
             {worst}"
        );
    }

    /// Two invariants the enumeration cannot accidentally satisfy together: each label's
    /// row is a probability distribution, and no detection is claimed by more than one
    /// target in total. The second is what fails if the injectivity constraint is
    /// dropped, and the first would still pass if it were.
    #[test]
    fn the_association_marginals_are_a_distribution_and_no_detection_is_double_spent() {
        let mut rng = SplitMix64(0x1234_5678_9ABC);
        for trial in 0..120 {
            let n = 1 + (trial % 5);
            let m = trial % 5;
            let u = random_association_weights(&mut rng, n, m);
            let marginals = association_marginals(&u, m).expect("a weighted scene");
            for row in &marginals {
                let sum: f64 = row.iter().sum();
                assert!(
                    (sum - 1.0).abs() < 1e-12,
                    "a label's association marginals sum to {sum}, not 1"
                );
                assert!(
                    row.iter().all(|p| *p >= 0.0),
                    "a negative marginal in {row:?}"
                );
            }
            for j in 0..m {
                let claimed: f64 = marginals
                    .iter()
                    .map(|row| row[ASSOCIATION_FIRST_DETECTION + j])
                    .sum();
                assert!(
                    claimed <= 1.0 + 1e-12,
                    "detection {j} is claimed with total probability {claimed}, so the \
                     injectivity constraint is not being applied"
                );
            }
        }
    }

    /// With one label the joint sum is trivial and the update must equal the textbook
    /// single-target Bernoulli filter. An exact reduction to a known simpler filter,
    /// which is the check that catches an error in the normalisation that the
    /// distribution invariants above would not.
    #[test]
    fn one_label_reduces_to_the_single_target_bernoulli_filter() {
        let mut rng = SplitMix64(0xDEAD_BEEF_0F1E);
        let mut worst = 0.0_f64;
        for _ in 0..100 {
            let r = 0.05 + rng.next_unit() * 0.9;
            let p_d = 0.3 + rng.next_unit() * 0.65;
            let kappa = 0.01 + rng.next_unit() * 2.0;
            let m = (rng.next_u64() % 4) as usize;
            let q: Vec<f64> = (0..m).map(|_| 1e-3 + rng.next_unit() * 3.0).collect();
            let mut row = vec![1.0 - r, r * (1.0 - p_d)];
            row.extend(q.iter().map(|qj| r * p_d * qj / kappa));
            let marginals = association_marginals(&[row], m).expect("a weighted scene");
            let got = 1.0 - marginals[0][ASSOCIATION_ABSENT];
            // r(1 - pD + pD Σ q/κ) / (1 - r pD + r pD Σ q/κ)
            let ratio: f64 = q.iter().map(|qj| qj / kappa).sum();
            let want = r * (1.0 - p_d + p_d * ratio) / (1.0 - r * p_d + r * p_d * ratio);
            worst = worst.max((got - want).abs() / want.abs().max(1e-12));
        }
        assert!(
            worst < 1e-12,
            "a one-label update does not equal the single-target Bernoulli filter: \
             worst relative error {worst}"
        );
    }

    /// A Bernoulli's spatial density is a probability density, unlike a PHD component's
    /// weight, and every step must leave it integrating to one. This is the invariant
    /// that separates the two representations, and getting it wrong would make existence
    /// and position disagree about how much belief there is.
    #[test]
    fn every_labels_spatial_density_stays_normalised() {
        let mut filter = lmb();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let truth = [[0.0, 0.0, 100.0], [250.0, 0.0, 100.0]];
        for scan in 0..12 {
            let births = if scan == 0 {
                truth
                    .iter()
                    .map(|p| lmb_birth(*p, [0.0, 0.0, 0.0]))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            filter.predict(&motion, 1.0, &births).expect("finite");
            let detections: Vec<_> = truth.iter().map(|p| detection(*p)).collect();
            filter.update(&detections).expect("valid");
            for bernoulli in filter.bernoullis() {
                let mass: f64 = bernoulli.spatial.iter().map(|c| c.weight).sum();
                assert!(
                    (mass - 1.0).abs() < 1e-9,
                    "scan {scan}: label {:?}'s spatial density integrates to {mass}, not 1",
                    bernoulli.label
                );
                assert!(
                    (0.0..=1.0).contains(&bernoulli.existence),
                    "scan {scan}: existence {} is not a probability",
                    bernoulli.existence
                );
            }
        }
    }

    /// The headline property, in miniature: a tracked target keeps its label, and a
    /// target that appears later gets one never used before. The dedicated integration
    /// test exercises this much harder; this one keeps the property inside the crate.
    #[test]
    fn a_tracked_label_persists_and_a_new_target_gets_an_unused_one() {
        let mut filter = lmb();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let first = [0.0, 0.0, 100.0];
        let second = [400.0, 0.0, 100.0];
        let mut seen_labels: Vec<TrackId> = Vec::new();
        let mut first_label = None;
        for scan in 0..14 {
            let births = match scan {
                0 => vec![lmb_birth(first, [0.0, 0.0, 0.0])],
                6 => vec![lmb_birth(second, [0.0, 0.0, 0.0])],
                _ => Vec::new(),
            };
            filter.predict(&motion, 1.0, &births).expect("finite");
            let mut detections = vec![detection(first)];
            if scan >= 6 {
                detections.push(detection(second));
            }
            filter.update(&detections).expect("valid");
            let labels = filter.labels();
            if scan == 0 {
                first_label = labels.first().copied();
            }
            assert!(
                labels.contains(&first_label.expect("a label at scan 0")),
                "scan {scan}: the first target's label disappeared; labels {labels:?}"
            );
            for label in labels {
                if !seen_labels.contains(&label) {
                    assert!(
                        scan == 0 || scan == 6,
                        "scan {scan}: label {label:?} appeared without a birth"
                    );
                    seen_labels.push(label);
                }
            }
        }
        assert_eq!(
            seen_labels.len(),
            2,
            "two births should have issued exactly two labels, issued {seen_labels:?}"
        );
    }

    /// A retired label must never come back. If it could, a consumer holding an
    /// identifier across a gap would silently be handed a different target.
    #[test]
    fn a_retired_label_is_never_reissued() {
        let mut filter = lmb();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        let position = [0.0, 0.0, 100.0];
        filter
            .predict(&motion, 1.0, &[lmb_birth(position, [0.0, 0.0, 0.0])])
            .expect("finite");
        filter.update(&[detection(position)]).expect("valid");
        let original = filter.labels();
        assert_eq!(original.len(), 1);

        // Starve it until it is pruned.
        for _ in 0..400 {
            filter.predict(&motion, 1.0, &[]).expect("finite");
            filter.update(&[]).expect("valid");
            if filter.labels().is_empty() {
                break;
            }
        }
        assert!(
            filter.labels().is_empty(),
            "an undetected target never faded: {:?}",
            filter
                .bernoullis()
                .iter()
                .map(|b| b.existence)
                .collect::<Vec<_>>()
        );

        filter
            .predict(&motion, 1.0, &[lmb_birth(position, [0.0, 0.0, 0.0])])
            .expect("finite");
        filter.update(&[detection(position)]).expect("valid");
        let reborn = filter.labels();
        assert_eq!(reborn.len(), 1);
        assert_ne!(
            reborn[0], original[0],
            "a retired label was reissued to a different target"
        );
    }

    /// Past the bound the filter refuses rather than approximating. The alternative --
    /// truncating the hypothesis space and returning the result as if it were exact --
    /// is the failure mode this crate's whole approach exists to avoid.
    #[test]
    fn a_scan_past_the_exact_association_bound_is_refused_not_approximated() {
        let settings = LmbSettings {
            max_detections_per_scan: 4,
            ..LmbSettings::default()
        };
        let mut filter = LmbFilter::new(
            settings,
            position_h(),
            SMatrix::<f64, M, M>::identity() * 25.0,
        )
        .expect("valid");
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        filter
            .predict(
                &motion,
                1.0,
                &[lmb_birth([0.0, 0.0, 100.0], [0.0, 0.0, 0.0])],
            )
            .expect("finite");
        let detections: Vec<_> = (0..5)
            .map(|i| detection([f64::from(i) * 50.0, 0.0, 100.0]))
            .collect();
        assert_eq!(
            filter.update(&detections).unwrap_err(),
            RfsError::TooManyDetections {
                detections: 5,
                limit: 4,
            }
        );
    }

    /// Settings that cannot be allocated for must be refused when they are set, not when
    /// a scan happens to arrive.
    #[test]
    fn an_unsupportable_detection_bound_is_refused_at_construction() {
        let err = LmbFilter::new(
            LmbSettings {
                max_detections_per_scan: MAX_SUPPORTED_DETECTIONS + 1,
                ..LmbSettings::default()
            },
            position_h(),
            SMatrix::<f64, M, M>::identity(),
        )
        .unwrap_err();
        assert!(matches!(err, RfsError::MalformedScene { .. }));
    }

    /// A clutter intensity of zero would divide every association weight by zero. The
    /// PHD filter tolerates it (its normalisation only adds the clutter term); this one
    /// cannot, and says so rather than producing infinities.
    #[test]
    fn a_zero_clutter_intensity_is_refused() {
        let err = LmbFilter::new(
            LmbSettings {
                clutter_density: 0.0,
                ..LmbSettings::default()
            },
            position_h(),
            SMatrix::<f64, M, M>::identity(),
        )
        .unwrap_err();
        assert!(matches!(err, RfsError::MalformedScene { .. }));
    }

    /// When two targets are at the same point at the same scan, the honest answer is
    /// that the filter cannot tell the two detections apart -- and it must say so, with
    /// an even split, rather than committing to one. This is the case that distinguishes
    /// a filter reporting its own uncertainty from one manufacturing certainty.
    #[test]
    fn coincident_targets_produce_an_even_association_split() {
        let mut filter = lmb();
        let motion = ConstantVelocity { sigma_a_sq: 1.0 };
        filter
            .predict(
                &motion,
                1.0,
                &[
                    lmb_birth([-40.0, 0.0, 100.0], [20.0, 0.0, 0.0]),
                    lmb_birth([40.0, 0.0, 100.0], [-20.0, 0.0, 0.0]),
                ],
            )
            .expect("finite");
        // Scan 0: well separated.
        filter
            .update(&[
                detection([-40.0, 0.0, 100.0]),
                detection([40.0, 0.0, 100.0]),
            ])
            .expect("valid");
        // Scans 1 and 2 bring them together at the same point.
        for x in [20.0_f64, 0.0] {
            filter.predict(&motion, 1.0, &[]).expect("finite");
            filter
                .update(&[detection([-x, 0.0, 100.0]), detection([x, 0.0, 100.0])])
                .expect("valid");
        }
        let association = filter.last_association();
        assert_eq!(association.len(), 2);
        for (label, row) in association {
            let to_first = row[ASSOCIATION_FIRST_DETECTION];
            let to_second = row[ASSOCIATION_FIRST_DETECTION + 1];
            assert!(
                (to_first - to_second).abs() < 1e-9,
                "label {label:?} at a coincident crossing should be an even split, got \
                 {to_first} and {to_second}"
            );
            assert!(
                to_first > 0.4,
                "label {label:?} should still be confident it was detected at all: \
                 {row:?}"
            );
        }
    }
}
