//! track-fusion: covariance-intersection and information-matrix track-to-track fusion,
//! and sensor registration / bias estimation.
//!
//! The `docs/verification-capability-table.md` §1 rows "`track-fusion` |
//! Track-to-track fusion (CI, information-matrix)" and "`track-fusion` | Sensor
//! registration / bias estimation". Validated against `scenario-crate-narrative.md`
//! Scenario 3, the urban multi-sensor convoy.
//!
//! # Why covariance intersection and not the obvious formula
//!
//! Two sensors each report a track for the same object. The textbook way to combine two
//! Gaussian estimates is the information-matrix sum,
//! `P⁻¹ = P₁⁻¹ + P₂⁻¹`, and it is **correct only when the two estimates' errors are
//! independent**. Two tracks of the same object almost never satisfy that. They share
//! the target's own manoeuvres, they often share a registration error, and in a system
//! like this one they may share an ancestor: a track fused from A and B, fused again
//! with B, counts B's evidence twice. The consequence is not a slightly wrong
//! covariance. It is a confidently wrong one -- the fused estimate reports far less
//! uncertainty than it has, and every gate downstream tightens around it.
//!
//! Covariance intersection is the answer that needs no independence assumption:
//!
//! ```text
//! P⁻¹ = ω P₁⁻¹ + (1 − ω) P₂⁻¹,    x = P (ω P₁⁻¹ x₁ + (1 − ω) P₂⁻¹ x₂),   ω ∈ [0, 1]
//! ```
//!
//! For **any** `ω` in `[0, 1]` the result is guaranteed not to be over-confident,
//! whatever the correlation between the inputs. The price is that when the inputs really
//! are independent, CI is conservative: it reports more uncertainty than the
//! information-matrix answer, because it does not know it is allowed to be sure.
//!
//! Both are provided. [`CovarianceIntersectionFuser`] is the default because this system
//! cannot in general prove its inputs independent. [`InformationMatrixFuser`] exists for
//! the case where a deployment can, and its documentation says plainly what it assumes,
//! so choosing it is a decision somebody made rather than one that happened.
//!
//! `ω` is chosen to minimise the determinant of the fused covariance, which is the
//! standard criterion: the determinant is the squared volume of the uncertainty
//! ellipsoid, so minimising it makes the most of what the two estimates jointly say.
//!
//! **That criterion does not always determine `ω`, and the fused state depends on `ω`
//! even when the covariance does not.** When the two inputs have equal covariances the
//! fused information matrix `ω P⁻¹ + (1 − ω) P⁻¹` is `P⁻¹` for every `ω`, so the
//! determinant is exactly flat and the minimiser is the whole interval -- while the
//! fused state `ω x₁ + (1 − ω) x₂` sweeps the entire line between the two estimates. A
//! search left to its own devices on a flat objective drifts to whichever end its
//! internals favour, and the fused estimate then depends on an implementation detail of
//! the search rather than on the data. Two equally good searches will disagree, which is
//! how this was found: the differential test's two sensors with identical covariances
//! landed 1.2e-6 apart.
//!
//! So the tie is broken by specification rather than by accident: when the best `ω`
//! found does not improve the determinant by a relative [`FLAT_OBJECTIVE`] over `ω =
//! 0.5`, `ω = 0.5` is used. Two estimates the criterion cannot separate are equally
//! informative, and weighting them equally is the answer that does not depend on how the
//! minimum was looked for. The oracle generator implements the same rule, from this
//! description rather than from this code.
//!
//! # Registration is estimated before fusion, not after
//!
//! Two platforms whose positions are known imperfectly produce tracks of one object that
//! are each internally consistent and mutually offset. Fusing them without first
//! removing that offset produces an estimate somewhere between two systematically wrong
//! answers, with a covariance that says the disagreement was noise. [`SensorRegistration`]
//! estimates the offset from tracks the two platforms hold in common, and
//! [`SensorRegistration::residual_spread`] reports how well the single-offset model
//! actually explained the disagreement, so a caller can tell a real bias from a pair of
//! sensors that simply disagree.

use gungnir_coord::Ecef;
use gungnir_track::Track;
use nalgebra::{SMatrix, SVector};

/// What this crate cannot do, named rather than panicked (GAP-082).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrackFusionError {
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
    /// Fusion was asked for with nothing, or with one track, to fuse.
    ///
    /// **An error rather than the single track returned unchanged.** Returning it would
    /// be indistinguishable from a genuine fusion of two agreeing sensors, and the
    /// difference matters: one is a corroborated estimate and the other is one sensor's
    /// opinion wearing a fused label.
    #[error("track-to-track fusion needs at least two tracks; {count} were given")]
    NotEnoughTracks { count: usize },
    /// A covariance that cannot be inverted, so it carries no information to weigh.
    #[error("{what} is singular, so it cannot be weighed against another estimate")]
    SingularCovariance { what: &'static str },
    /// A state or covariance with a non-finite entry.
    #[error("{what} is not finite")]
    NotFinite { what: &'static str },
    /// Registration was asked for from track sets that cannot be paired.
    #[error("sensor registration needs matched pairs: {what}")]
    UnmatchedTracks { what: &'static str },
}

/// Combines independent local tracks of the same real-world object into one fused
/// global estimate.
pub trait TrackFuser {
    /// # Errors
    ///
    /// When the fuser cannot produce an estimate. **A `Result` rather than a `Track`**
    /// (GAP-082): there is no honest `Track` to return when nothing fused, and a default
    /// one would be a fused global estimate of an object at the origin.
    fn fuse(&self, local_tracks: &[Track]) -> Result<Track, TrackFusionError>;
}

/// How finely `ω` is searched. The fused determinant is a smooth scalar function of one
/// bounded variable, so a golden-section search converges quickly; this many iterations
/// pins `ω` to about 1e-9, far inside the row's 1e-6 on the result.
const OMEGA_ITERATIONS: u32 = 100;

/// Below this relative improvement over `ω = 0.5`, the determinant criterion is treated
/// as not having chosen. See the module documentation for why that case is real and what
/// happens if it is ignored.
const FLAT_OBJECTIVE: f64 = 1e-9;

/// Covariance-intersection fusion. The default; see the module documentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CovarianceIntersectionFuser;

/// The information-matrix sum, `P⁻¹ = Σ Pᵢ⁻¹`.
///
/// **This assumes the estimates' errors are independent, and says so here because
/// nothing downstream can check it.** Use it only where a deployment can argue the
/// inputs share no common ancestor and no common error source. Where it cannot, the
/// result is over-confident, and an over-confident covariance is worse than a wide one
/// because everything downstream believes it. [`CovarianceIntersectionFuser`] is the
/// safe default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InformationMatrixFuser;

fn check_finite(track: &Track, what: &'static str) -> Result<(), TrackFusionError> {
    if track.state.iter().all(|v| v.is_finite()) && track.covariance.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(TrackFusionError::NotFinite { what })
    }
}

fn information(track: &Track) -> Result<SMatrix<f64, 6, 6>, TrackFusionError> {
    track
        .covariance
        .try_inverse()
        .ok_or(TrackFusionError::SingularCovariance {
            what: "a local track's covariance",
        })
}

/// Build the fused track, carrying forward the identity and counters of the first input.
///
/// The fused track keeps the **first** input's id deliberately. A fused track is not a
/// new detection of a new thing, and minting a fresh id would make the picture show an
/// object appearing at the moment two sensors agreed about it. Hits and misses are
/// summed and taken as the minimum respectively: the fused track has been seen as often
/// as its inputs together, and it has been missed only as long as the sensor that has
/// seen it most recently says.
fn assemble(inputs: &[Track], state: SVector<f64, 6>, covariance: &SMatrix<f64, 6, 6>) -> Track {
    let first = &inputs[0];
    Track {
        id: first.id,
        status: first.status,
        state,
        covariance: (covariance + covariance.transpose()) * 0.5,
        misses_since_update: inputs
            .iter()
            .map(|t| t.misses_since_update)
            .min()
            .unwrap_or(first.misses_since_update),
        hits: inputs.iter().map(|t| t.hits).sum(),
    }
}

/// Fuse a pair by covariance intersection at a given `ω`.
fn intersect_at(
    omega: f64,
    a_info: &SMatrix<f64, 6, 6>,
    b_info: &SMatrix<f64, 6, 6>,
    a_state: &SVector<f64, 6>,
    b_state: &SVector<f64, 6>,
) -> Result<(SVector<f64, 6>, SMatrix<f64, 6, 6>), TrackFusionError> {
    let fused_info = a_info * omega + b_info * (1.0 - omega);
    let covariance = fused_info
        .try_inverse()
        .ok_or(TrackFusionError::SingularCovariance {
            what: "the fused information matrix",
        })?;
    let weighted = (a_info * a_state) * omega + (b_info * b_state) * (1.0 - omega);
    Ok((covariance * weighted, covariance))
}

impl TrackFuser for CovarianceIntersectionFuser {
    /// Fuse pairwise from the left, choosing `ω` at each step to minimise the
    /// determinant of the result.
    ///
    /// # Errors
    ///
    /// [`TrackFusionError::NotEnoughTracks`] below two inputs;
    /// [`TrackFusionError::SingularCovariance`] when an input carries no information;
    /// [`TrackFusionError::NotFinite`] when an input is not a number.
    fn fuse(&self, local_tracks: &[Track]) -> Result<Track, TrackFusionError> {
        if local_tracks.len() < 2 {
            return Err(TrackFusionError::NotEnoughTracks {
                count: local_tracks.len(),
            });
        }
        for track in local_tracks {
            check_finite(track, "a local track")?;
        }

        let mut state = local_tracks[0].state;
        let mut covariance = local_tracks[0].covariance;
        for next in &local_tracks[1..] {
            let a_info = covariance
                .try_inverse()
                .ok_or(TrackFusionError::SingularCovariance {
                    what: "the running fused covariance",
                })?;
            let b_info = information(next)?;

            // Golden-section search for the omega minimising det(P). The determinant of
            // the inverse of a positive-definite convex combination is unimodal in
            // omega, so a bracketing search finds the minimum without a derivative.
            let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
            let (mut low, mut high) = (0.0_f64, 1.0_f64);
            let determinant_at = |omega: f64| -> f64 {
                intersect_at(omega, &a_info, &b_info, &state, &next.state)
                    .map_or(f64::INFINITY, |(_, p)| p.determinant())
            };
            let mut c = high - phi * (high - low);
            let mut d = low + phi * (high - low);
            let (mut fc, mut fd) = (determinant_at(c), determinant_at(d));
            for _ in 0..OMEGA_ITERATIONS {
                if fc < fd {
                    high = d;
                    d = c;
                    fd = fc;
                    c = high - phi * (high - low);
                    fc = determinant_at(c);
                } else {
                    low = c;
                    c = d;
                    fc = fd;
                    d = low + phi * (high - low);
                    fd = determinant_at(d);
                }
            }
            let searched = f64::midpoint(low, high);
            // The tie-break. See the module documentation: on a flat objective the
            // search's answer is an artefact of the search.
            let at_searched = determinant_at(searched);
            let at_half = determinant_at(0.5);
            let flat =
                !at_half.is_finite() || (at_half - at_searched) <= FLAT_OBJECTIVE * at_half.abs();
            let omega = if flat { 0.5 } else { searched };
            let (fused_state, fused_covariance) =
                intersect_at(omega, &a_info, &b_info, &state, &next.state)?;
            state = fused_state;
            covariance = fused_covariance;
        }
        Ok(assemble(local_tracks, state, &covariance))
    }
}

impl TrackFuser for InformationMatrixFuser {
    /// # Errors
    ///
    /// As [`CovarianceIntersectionFuser::fuse`].
    fn fuse(&self, local_tracks: &[Track]) -> Result<Track, TrackFusionError> {
        if local_tracks.len() < 2 {
            return Err(TrackFusionError::NotEnoughTracks {
                count: local_tracks.len(),
            });
        }
        let mut total_information = SMatrix::<f64, 6, 6>::zeros();
        let mut weighted = SVector::<f64, 6>::zeros();
        for track in local_tracks {
            check_finite(track, "a local track")?;
            let info = information(track)?;
            total_information += info;
            weighted += info * track.state;
        }
        let covariance =
            total_information
                .try_inverse()
                .ok_or(TrackFusionError::SingularCovariance {
                    what: "the summed information matrix",
                })?;
        Ok(assemble(local_tracks, covariance * weighted, &covariance))
    }
}

/// Estimates the systematic position offset between two sensor platforms from tracks
/// they hold in common.
///
/// The model is a single constant translation in the local frame. That is the bias a
/// platform position error produces, and it is the one this type claims to find. An
/// orientation bias produces a range-dependent offset that a single translation cannot
/// represent, and [`Self::residual_spread`] is how a caller finds out that is what it is
/// looking at: a good translation fit leaves a residual spread near the measurement
/// noise, and an orientation bias leaves one that grows with the spread of the tracks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SensorRegistration {
    /// The estimated offset to add to platform B's tracks to bring them onto A's, once
    /// [`Self::estimate_bias`] has succeeded.
    pub estimated_bias: Option<nalgebra::Vector3<f64>>,
    /// The root-mean-square residual after removing the estimated offset, metres.
    residual_spread: Option<f64>,
    /// How many paired tracks the estimate came from.
    pairs: usize,
}

impl SensorRegistration {
    /// The root-mean-square residual left after removing the estimated offset, metres.
    ///
    /// **Read this before trusting the bias.** A registration estimate is a mean, and a
    /// mean of a set of disagreements always exists; the spread is what says whether one
    /// constant offset actually explained them.
    #[must_use]
    pub fn residual_spread(&self) -> Option<f64> {
        self.residual_spread
    }

    /// How many paired tracks the estimate was made from.
    #[must_use]
    pub fn pairs(&self) -> usize {
        self.pairs
    }

    /// Estimate the offset from tracks held in common, paired by position in the slices.
    ///
    /// The estimate is the inverse-variance-weighted mean of the per-pair
    /// disagreements. Weighting matters: a pair whose two tracks are both poorly known
    /// says less about the platforms' relative position than a pair whose tracks are
    /// both tight, and an unweighted mean would let the worst pair dominate.
    ///
    /// # Errors
    ///
    /// [`TrackFusionError::UnmatchedTracks`] when the two slices are different lengths
    /// or empty. Different lengths mean the caller has not actually paired the tracks,
    /// and guessing a pairing here would silently register two platforms against the
    /// wrong objects.
    ///
    /// [`TrackFusionError::NotFinite`] or [`TrackFusionError::SingularCovariance`] for
    /// an input that carries no usable information.
    pub fn estimate_bias(
        &mut self,
        shared_tracks_a: &[Track],
        shared_tracks_b: &[Track],
    ) -> Result<nalgebra::Vector3<f64>, TrackFusionError> {
        if shared_tracks_a.len() != shared_tracks_b.len() {
            return Err(TrackFusionError::UnmatchedTracks {
                what: "the two platforms supplied different numbers of shared tracks",
            });
        }
        if shared_tracks_a.is_empty() {
            return Err(TrackFusionError::UnmatchedTracks {
                what: "no tracks are held in common, so there is nothing to register against",
            });
        }

        let mut total_weight = SMatrix::<f64, 3, 3>::zeros();
        let mut weighted_sum = nalgebra::Vector3::<f64>::zeros();
        let mut differences = Vec::with_capacity(shared_tracks_a.len());
        for (a, b) in shared_tracks_a.iter().zip(shared_tracks_b) {
            check_finite(a, "a track from platform A")?;
            check_finite(b, "a track from platform B")?;
            let difference = nalgebra::Vector3::new(
                a.state[0] - b.state[0],
                a.state[1] - b.state[1],
                a.state[2] - b.state[2],
            );
            // The position blocks only: an offset between platforms is a position
            // quantity, and folding the velocity covariance in would weight a pair by
            // how well its speed is known, which says nothing about where it is.
            let position_covariance =
                SMatrix::<f64, 3, 3>::from_fn(|r, c| a.covariance[(r, c)] + b.covariance[(r, c)]);
            let weight =
                position_covariance
                    .try_inverse()
                    .ok_or(TrackFusionError::SingularCovariance {
                        what: "the summed position covariance of a shared pair",
                    })?;
            total_weight += weight;
            weighted_sum += weight * difference;
            differences.push(difference);
        }

        let bias = total_weight
            .try_inverse()
            .ok_or(TrackFusionError::SingularCovariance {
                what: "the summed registration weight",
            })?
            * weighted_sum;

        #[allow(clippy::cast_precision_loss)]
        let count = differences.len() as f64;
        let spread = (differences
            .iter()
            .map(|d| (d - bias).norm_squared())
            .sum::<f64>()
            / count)
            .sqrt();

        self.estimated_bias = Some(bias);
        self.residual_spread = Some(spread);
        self.pairs = differences.len();
        Ok(bias)
    }

    /// Apply the estimated offset to a track from platform B, bringing it onto A's frame.
    ///
    /// # Errors
    ///
    /// [`TrackFusionError::NotImplemented`] when no bias has been estimated yet. That is
    /// the honest answer: correcting by an unknown offset means not correcting, and
    /// returning the track unchanged would be a claim that the platforms are registered.
    pub fn correct(&self, track: &Track) -> Result<Track, TrackFusionError> {
        let Some(bias) = self.estimated_bias else {
            return Err(TrackFusionError::NotImplemented {
                what: "correcting a track before a bias has been estimated",
                waiting_on: "a call to estimate_bias with tracks held in common",
            });
        };
        let mut corrected = track.clone();
        for axis in 0..3 {
            corrected.state[axis] += bias[axis];
        }
        Ok(corrected)
    }
}

/// One observation of an object whose true position is already known.
///
/// **Plain data, and deliberately not a `gungnir-model` event.** GAP-014's action asks
/// for a calibration event in `gungnir-model` consumed by this crate's registration.
/// This crate is tracking core, `gungnir-model` is the foundation layer, and
/// `CLAUDE.md`'s first rule fixes the direction one-way -- so consuming the event here
/// would be an upward edge. `gungnir_model::CalibrationEvent` carries the same
/// information at the layer events belong to, and `gungnir-tracking-service`, which
/// already depends on both, translates. The intent of the action is met; the edge is not
/// added.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceObservation {
    /// The surveyed position, local ENU metres.
    pub truth_enu: [f64; 3],
    /// Where the sensor put it, local ENU metres.
    pub observed_enu: [f64; 3],
    /// The variance claimed for that report, per axis, metres squared. Weights this
    /// observation against the others.
    pub variance_m2: [f64; 3],
}

impl SensorRegistration {
    /// Estimate one sensor's offset against surveyed truth.
    ///
    /// **This is the absolute measurement, and [`Self::estimate_bias`] is the relative
    /// one.** Registering two platforms against each other recovers their offset from
    /// one another and says nothing about either one's offset from the world; two
    /// platforms displaced identically look perfectly registered. A surveyed reference
    /// fixes one platform to the ground, and every relative registration made afterwards
    /// inherits that.
    ///
    /// The returned offset is what to add to this sensor's reports to bring them onto
    /// truth, the same convention [`Self::correct`] applies.
    ///
    /// # Errors
    ///
    /// [`TrackFusionError::UnmatchedTracks`] when no references were supplied; there is
    /// nothing to register against, and an offset of zero would be the claim that the
    /// sensor is known to be correct.
    ///
    /// [`TrackFusionError::NotFinite`] for a reference whose numbers are not numbers,
    /// and [`TrackFusionError::SingularCovariance`] for one claiming zero variance in
    /// some direction -- a sensor asserting a perfect measurement, which would take the
    /// whole estimate on its own.
    pub fn estimate_from_references(
        &mut self,
        references: &[ReferenceObservation],
    ) -> Result<nalgebra::Vector3<f64>, TrackFusionError> {
        if references.is_empty() {
            return Err(TrackFusionError::UnmatchedTracks {
                what:
                    "no surveyed references were supplied, so there is nothing to register against",
            });
        }

        let mut total_weight = SMatrix::<f64, 3, 3>::zeros();
        let mut weighted_sum = nalgebra::Vector3::<f64>::zeros();
        let mut differences = Vec::with_capacity(references.len());
        for reference in references {
            let finite = reference.truth_enu.iter().all(|v| v.is_finite())
                && reference.observed_enu.iter().all(|v| v.is_finite())
                && reference.variance_m2.iter().all(|v| v.is_finite());
            if !finite {
                return Err(TrackFusionError::NotFinite {
                    what: "a surveyed reference observation",
                });
            }
            let difference = nalgebra::Vector3::new(
                reference.truth_enu[0] - reference.observed_enu[0],
                reference.truth_enu[1] - reference.observed_enu[1],
                reference.truth_enu[2] - reference.observed_enu[2],
            );
            let covariance = SMatrix::<f64, 3, 3>::from_diagonal(&nalgebra::Vector3::new(
                reference.variance_m2[0],
                reference.variance_m2[1],
                reference.variance_m2[2],
            ));
            let weight = covariance
                .try_inverse()
                .ok_or(TrackFusionError::SingularCovariance {
                    what: "a reference observation's claimed variance",
                })?;
            total_weight += weight;
            weighted_sum += weight * difference;
            differences.push(difference);
        }

        let bias = total_weight
            .try_inverse()
            .ok_or(TrackFusionError::SingularCovariance {
                what: "the summed registration weight",
            })?
            * weighted_sum;

        #[allow(clippy::cast_precision_loss)]
        let count = differences.len() as f64;
        let spread = (differences
            .iter()
            .map(|d| (d - bias).norm_squared())
            .sum::<f64>()
            / count)
            .sqrt();

        self.estimated_bias = Some(bias);
        self.residual_spread = Some(spread);
        self.pairs = differences.len();
        Ok(bias)
    }
}

/// A sensor platform and where it believes it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorPlatform {
    pub id: u32,
    pub position: Ecef,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_track::{TrackId, TrackStatus};

    fn track(state: [f64; 6], variances: [f64; 6]) -> Track {
        Track {
            id: TrackId(1),
            status: TrackStatus::Confirmed,
            state: SVector::<f64, 6>::from_column_slice(&state),
            covariance: SMatrix::<f64, 6, 6>::from_diagonal(&SVector::<f64, 6>::from_column_slice(
                &variances,
            )),
            misses_since_update: 0,
            hits: 4,
        }
    }

    #[test]
    fn fusing_one_track_is_refused() {
        let err = CovarianceIntersectionFuser
            .fuse(&[track([0.0; 6], [1.0; 6])])
            .unwrap_err();
        assert_eq!(err, TrackFusionError::NotEnoughTracks { count: 1 });
    }

    /// The defining property of covariance intersection: it must never be more
    /// confident than the better of its inputs. Everything downstream depends on this,
    /// and it is what makes CI safe to use on estimates that may share evidence.
    #[test]
    fn the_fused_estimate_is_never_more_confident_than_its_best_input() {
        let a = track(
            [100.0, 0.0, 50.0, 10.0, 0.0, 0.0],
            [4.0, 4.0, 9.0, 1.0, 1.0, 1.0],
        );
        let b = track(
            [104.0, 2.0, 52.0, 11.0, 0.0, 0.0],
            [400.0, 400.0, 900.0, 100.0, 100.0, 100.0],
        );
        let fused = CovarianceIntersectionFuser
            .fuse(&[a.clone(), b])
            .expect("fusable");
        // The volume of the fused ellipsoid must not be smaller than the tighter input's.
        assert!(
            fused.covariance.determinant() >= a.covariance.determinant() * (1.0 - 1e-9),
            "covariance intersection produced an over-confident estimate"
        );
    }

    /// The other half of the same property: the information-matrix fuser IS more
    /// confident, which is exactly why it is not the default. Pinned so that the two
    /// cannot quietly become the same function.
    #[test]
    fn the_information_matrix_fuser_is_more_confident_and_that_is_the_difference() {
        let a = track([100.0, 0.0, 50.0, 10.0, 0.0, 0.0], [16.0; 6]);
        let b = track([104.0, 2.0, 52.0, 11.0, 0.0, 0.0], [16.0; 6]);
        let ci = CovarianceIntersectionFuser
            .fuse(&[a.clone(), b.clone()])
            .expect("fusable");
        let info = InformationMatrixFuser.fuse(&[a, b]).expect("fusable");
        assert!(
            info.covariance.determinant() < ci.covariance.determinant(),
            "the information-matrix fuser was not tighter than covariance intersection, \
             so one of them is wrong"
        );
    }

    /// Two identical estimates carry no new information between them. CI must return
    /// the same covariance, not a tighter one: that is the double-counting case the
    /// whole method exists to survive.
    #[test]
    fn fusing_a_track_with_itself_gains_nothing() {
        let a = track([10.0, 20.0, 30.0, 1.0, 2.0, 3.0], [25.0; 6]);
        let fused = CovarianceIntersectionFuser
            .fuse(&[a.clone(), a.clone()])
            .expect("fusable");
        let dp = (fused.covariance - a.covariance).abs().max();
        assert!(
            dp < 1e-6,
            "fusing with itself changed the covariance by {dp}"
        );
        let dx = (fused.state - a.state).abs().max();
        assert!(dx < 1e-9, "fusing with itself moved the state by {dx}");
        // And the information-matrix fuser gets this wrong, which is the point.
        let doubled = InformationMatrixFuser
            .fuse(&[a.clone(), a.clone()])
            .expect("fusable");
        assert!(
            doubled.covariance.determinant() < a.covariance.determinant(),
            "the information-matrix fuser failed to double-count, so this test no longer \
             demonstrates the difference"
        );
    }

    #[test]
    fn the_fused_track_keeps_the_first_identity_and_sums_the_hits() {
        let mut a = track([0.0; 6], [10.0; 6]);
        a.id = TrackId(7);
        a.hits = 3;
        a.misses_since_update = 2;
        let mut b = track([1.0; 6], [10.0; 6]);
        b.id = TrackId(9);
        b.hits = 5;
        b.misses_since_update = 0;
        let fused = CovarianceIntersectionFuser.fuse(&[a, b]).expect("fusable");
        assert_eq!(fused.id, TrackId(7), "a fused track must not appear as new");
        assert_eq!(fused.hits, 8);
        assert_eq!(fused.misses_since_update, 0);
    }

    #[test]
    fn a_singular_covariance_is_reported() {
        let a = track([0.0; 6], [10.0; 6]);
        let mut b = track([0.0; 6], [10.0; 6]);
        b.covariance = SMatrix::<f64, 6, 6>::zeros();
        assert_eq!(
            CovarianceIntersectionFuser.fuse(&[a, b]).unwrap_err(),
            TrackFusionError::SingularCovariance {
                what: "a local track's covariance"
            }
        );
    }

    /// The registration row's own criterion: inject a known bias, recover it to within
    /// 1e-3. Here the two platforms see the same six objects and B is offset by a known
    /// constant.
    #[test]
    fn an_injected_bias_is_recovered() {
        let injected = nalgebra::Vector3::new(12.5, -7.25, 3.0);
        let truth = [
            [0.0, 0.0, 100.0],
            [500.0, 200.0, 150.0],
            [-300.0, 800.0, 90.0],
            [1200.0, -400.0, 220.0],
            [50.0, -900.0, 60.0],
            [-750.0, -150.0, 310.0],
        ];
        let a: Vec<Track> = truth
            .iter()
            .map(|p| {
                track(
                    [p[0], p[1], p[2], 0.0, 0.0, 0.0],
                    [9.0, 9.0, 16.0, 1.0, 1.0, 1.0],
                )
            })
            .collect();
        let b: Vec<Track> = truth
            .iter()
            .map(|p| {
                track(
                    [
                        p[0] - injected[0],
                        p[1] - injected[1],
                        p[2] - injected[2],
                        0.0,
                        0.0,
                        0.0,
                    ],
                    [9.0, 9.0, 16.0, 1.0, 1.0, 1.0],
                )
            })
            .collect();

        let mut registration = SensorRegistration::default();
        let recovered = registration.estimate_bias(&a, &b).expect("registrable");
        let error = (recovered - injected).abs().max();
        assert!(
            error < 1e-3,
            "recovered {recovered:?}, injected {injected:?}"
        );
        assert_eq!(registration.pairs(), 6);
        assert!(
            registration.residual_spread().is_some_and(|s| s < 1e-6),
            "a perfectly constant offset left a residual spread"
        );
    }

    /// The property `residual_spread` exists for: an orientation error is not a
    /// translation, and the fit must say so rather than reporting a plausible mean.
    #[test]
    fn an_orientation_error_shows_up_as_a_large_residual_spread() {
        let angle = 0.02_f64;
        let truth = [
            [1000.0, 0.0, 100.0],
            [0.0, 1000.0, 100.0],
            [-1000.0, 0.0, 100.0],
            [0.0, -1000.0, 100.0],
        ];
        let a: Vec<Track> = truth
            .iter()
            .map(|p| track([p[0], p[1], p[2], 0.0, 0.0, 0.0], [9.0; 6]))
            .collect();
        let b: Vec<Track> = truth
            .iter()
            .map(|p| {
                let (s, c) = angle.sin_cos();
                track(
                    [
                        p[0] * c - p[1] * s,
                        p[0] * s + p[1] * c,
                        p[2],
                        0.0,
                        0.0,
                        0.0,
                    ],
                    [9.0; 6],
                )
            })
            .collect();
        let mut registration = SensorRegistration::default();
        registration.estimate_bias(&a, &b).expect("registrable");
        let spread = registration
            .residual_spread()
            .expect("a spread is recorded whenever a bias is");
        assert!(
            spread > 10.0,
            "a 0.02 rad rotation over a 1 km baseline left a residual spread of only \
             {spread} m, so the spread is not detecting a mis-modelled bias"
        );
    }

    #[test]
    fn correcting_before_estimating_is_refused() {
        let registration = SensorRegistration::default();
        let err = registration
            .correct(&track([0.0; 6], [1.0; 6]))
            .unwrap_err();
        assert!(matches!(err, TrackFusionError::NotImplemented { .. }));
    }

    #[test]
    fn correction_moves_a_track_onto_the_other_platforms_frame() {
        let mut registration = SensorRegistration::default();
        let a = [track([100.0, 50.0, 10.0, 0.0, 0.0, 0.0], [4.0; 6])];
        let b = [track([90.0, 45.0, 8.0, 0.0, 0.0, 0.0], [4.0; 6])];
        registration.estimate_bias(&a, &b).expect("registrable");
        let corrected = registration.correct(&b[0]).expect("a bias is estimated");
        for axis in 0..3 {
            assert!(
                (corrected.state[axis] - a[0].state[axis]).abs() < 1e-9,
                "axis {axis} was not brought onto A's frame"
            );
        }
    }

    fn reference(truth: [f64; 3], observed: [f64; 3], variance: f64) -> ReferenceObservation {
        ReferenceObservation {
            truth_enu: truth,
            observed_enu: observed,
            variance_m2: [variance; 3],
        }
    }

    /// Registration against surveyed truth, which is the absolute measurement the
    /// platform-to-platform one cannot make.
    #[test]
    fn a_sensor_offset_is_recovered_from_surveyed_references() {
        let offset = [4.0, -9.5, 1.25];
        let truths = [
            [0.0, 0.0, 0.0],
            [800.0, 300.0, 40.0],
            [-450.0, 1100.0, 15.0],
            [200.0, -700.0, 80.0],
        ];
        let references: Vec<ReferenceObservation> = truths
            .iter()
            .map(|t| {
                reference(
                    *t,
                    [t[0] - offset[0], t[1] - offset[1], t[2] - offset[2]],
                    4.0,
                )
            })
            .collect();
        let mut registration = SensorRegistration::default();
        let recovered = registration
            .estimate_from_references(&references)
            .expect("registrable");
        for axis in 0..3 {
            assert!(
                (recovered[axis] - offset[axis]).abs() < 1e-3,
                "axis {axis}: recovered {} against an injected {}",
                recovered[axis],
                offset[axis]
            );
        }
        assert!(registration.residual_spread().is_some_and(|s| s < 1e-9));
    }

    /// A sensor that claims a tighter variance must count for more. Without this the
    /// variance field would be decoration.
    #[test]
    fn a_tighter_reference_pulls_the_estimate_toward_itself() {
        let mut registration = SensorRegistration::default();
        let recovered = registration
            .estimate_from_references(&[
                reference([0.0, 0.0, 0.0], [-10.0, 0.0, 0.0], 1.0),
                reference([100.0, 0.0, 0.0], [90.0 - 20.0, 0.0, 0.0], 10_000.0),
            ])
            .expect("registrable");
        assert!(
            (recovered[0] - 10.0).abs() < 0.5,
            "the loose reference dominated: recovered {}",
            recovered[0]
        );
    }

    #[test]
    fn no_references_is_refused_rather_than_reported_as_no_offset() {
        let mut registration = SensorRegistration::default();
        let err = registration.estimate_from_references(&[]).unwrap_err();
        assert!(matches!(err, TrackFusionError::UnmatchedTracks { .. }));
        assert!(
            registration.estimated_bias.is_none(),
            "a refused registration must leave no bias behind"
        );
    }

    #[test]
    fn a_reference_claiming_perfect_accuracy_is_refused() {
        let mut registration = SensorRegistration::default();
        let err = registration
            .estimate_from_references(&[reference([0.0; 3], [1.0, 0.0, 0.0], 0.0)])
            .unwrap_err();
        assert!(matches!(err, TrackFusionError::SingularCovariance { .. }));
    }

    #[test]
    fn mismatched_track_sets_are_refused() {
        let mut registration = SensorRegistration::default();
        let err = registration
            .estimate_bias(&[track([0.0; 6], [1.0; 6])], &[])
            .unwrap_err();
        assert!(matches!(err, TrackFusionError::UnmatchedTracks { .. }));
    }
}
