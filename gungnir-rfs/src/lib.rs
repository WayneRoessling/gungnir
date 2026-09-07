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
//! **Built and gated: the Gaussian-mixture PHD filter** ([`PhdFilter`]).
//!
//! **Not built, and returning an explicit error rather than a plausible answer**: the
//! CPHD's separate cardinality distribution ([`CphdFilter`]), and the labelled filters
//! [`GlmbFilter`] and [`LmbFilter`]. Each is its own §2 row and each will want its own
//! oracle comparison. They are named individually rather than behind one "not
//! implemented" so a reader can tell which of the four this build has.
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
/// **Not implemented.** The PHD's cardinality estimate is the sum of the intensity
/// weights, which is its *mean* and nothing more; a CPHD propagates the whole
/// distribution, which is what makes it far less prone to the PHD's characteristic
/// cardinality swings when detections are missed. That is a separate §2 row with its own
/// oracle and it has not been written.
#[derive(Debug, Clone)]
pub struct CphdFilter {
    pub phd: PhdFilter,
    pub cardinality_dist: Vec<f64>,
}

impl CphdFilter {
    /// # Errors
    ///
    /// Always. See the type's own documentation: this is a distinct filter, not a
    /// setting on the PHD, and pretending the PHD's weight sum is a cardinality
    /// distribution would be the silent stub `CLAUDE.md` forbids.
    pub fn cardinality_distribution(&self) -> Result<&[f64], RfsError> {
        Err(RfsError::NotImplemented {
            what: "the CPHD's cardinality distribution",
            waiting_on: "its own §2 row and a Stone Soup GM-CPHD oracle comparison",
        })
    }
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
        let cphd = CphdFilter {
            phd: filter(),
            cardinality_dist: Vec::new(),
        };
        assert!(cphd.cardinality_distribution().is_err());
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
}
