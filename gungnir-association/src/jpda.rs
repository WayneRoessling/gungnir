// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Joint Probabilistic Data Association: the `docs/verification-capability-table.md`
//! §1 row "`association` | JPDA".
//!
//! Oracle Stone Soup 1.9.1's `JPDA` data associator over its `PDAHypothesiser`,
//! criterion relative error < 1e-3 on the per-track association probabilities.
//!
//! # What JPDA is for, and why it is not a better nearest neighbour
//!
//! [`crate::GlobalNearestNeighbor`] answers "which detection belongs to which track"
//! with one hard assignment. When two targets pass close together in clutter that
//! question has no confident answer, and a hard assignment invents one: the tracker
//! commits, and if it commits wrongly the two tracks swap identity permanently. JPDA
//! refuses the question. It returns, for every track, a probability over *all* the
//! detections in its gate plus the possibility that the track was not detected at all,
//! and the filter update is then weighted by those probabilities rather than driven by
//! one chosen detection.
//!
//! The cost is that a track's posterior is a Gaussian mixture that has to be
//! moment-matched back to one Gaussian, so a JPDA track's covariance grows while the
//! association is ambiguous. That growth is the honest report: the tracker really does
//! know less about where the target is while two of them are crossing.
//!
//! # The recursion, matching the oracle exactly
//!
//! For each track `t` and each detection `j` inside its gate, the likelihood ratio is
//!
//! ```text
//! L_j(t) = N(z_j; ẑ_t, S_t) · P_D / λ
//! ```
//!
//! and the missed-detection alternative is `L_0(t) = 1 − P_D · P_G`, where `λ` is the
//! clutter spatial density, `P_D` the probability of detection and `P_G` the gate
//! probability. A **joint event** assigns each track either one detection or nothing,
//! with no detection used twice; its weight is the product of the per-track `L` values
//! it selects. The joint weights are normalised across all valid events, and the
//! marginal `β_tj` is the sum of the normalised weights of the events in which track `t`
//! took detection `j`.
//!
//! The per-track values are deliberately *not* normalised before the joint step. A
//! per-track normalisation is a constant factor per track that cancels in the joint
//! normalisation, so doing it would change nothing except to make the correspondence
//! with the oracle harder to check.
//!
//! # Enumeration is exact, bounded, and refuses rather than hangs
//!
//! The number of valid joint events grows factorially. This module enumerates them
//! exactly -- an approximation would have to be argued against the oracle separately --
//! and bounds the work three ways: a cap on tracks, a cap on detections, and a cap on
//! events actually enumerated. Exceeding any of them is
//! [`AssociationError::TooManyHypotheses`], not a truncated answer. A truncated
//! enumeration returns probabilities that look ordinary and are wrong, which is the
//! silent-stub failure `CLAUDE.md` rules out; a dense scene that outgrows exact JPDA
//! needs a different algorithm, and saying so is the useful report.
//!
//! Gating is what keeps this tractable in practice: a detection outside a track's gate
//! is not an option for that track at all, so the branching factor is the number of
//! detections genuinely competing for one track rather than the number in the scene.

use crate::assignment::AssociationError;
use crate::gating::ChiSquareGate;
use nalgebra::{SMatrix, SVector};

/// The most tracks exact enumeration will attempt.
pub const MAX_TRACKS: usize = 8;
/// The most detections exact enumeration will attempt.
pub const MAX_DETECTIONS: usize = 12;
/// The most joint events that will be enumerated before the problem is refused.
///
/// Sized so the worst case finishes in well under a frame budget on the hardware this
/// system targets, rather than chosen for mathematical elegance. The number matters
/// less than that there is one and that passing it is an error.
pub const MAX_JOINT_EVENTS: usize = 200_000;

/// One track's predicted measurement and the covariance of the innovation about it.
///
/// This is deliberately the *predicted measurement*, not the track: JPDA needs no
/// access to a track's state, its identity, or its history, and a type that carried
/// them would tempt a caller into making association depend on identity, which is how
/// a tracker starts confirming its own guesses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPrediction<const M: usize> {
    /// `ẑ = h(x⁻)`.
    pub predicted_measurement: SVector<f64, M>,
    /// `S = H P⁻ Hᵀ + R`.
    pub innovation_covariance: SMatrix<f64, M, M>,
}

/// The scene parameters JPDA weighs a detection against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JpdaSettings {
    /// `P_D`, the probability the sensor reports a target that is there.
    pub probability_of_detection: f64,
    /// `P_G`, the probability a true detection falls inside the gate. Should agree
    /// with the gate: a 99% gate has `P_G = 0.99`.
    pub probability_of_gate: f64,
    /// `λ`, the expected number of false detections per unit measurement volume.
    ///
    /// **This is the parameter that decides how much JPDA trusts a detection**, and it
    /// has units: whatever the measurement's volume is measured in. A deployment that
    /// sets it from a guess gets association probabilities from a guess.
    pub clutter_density: f64,
    /// The gate. A detection outside a track's gate is not an option for that track.
    pub gate: ChiSquareGate,
}

impl JpdaSettings {
    /// Check the settings describe a scene rather than a mistake.
    fn validate(&self) -> Result<(), AssociationError> {
        let probabilities_valid = (0.0..=1.0).contains(&self.probability_of_detection)
            && (0.0..=1.0).contains(&self.probability_of_gate);
        if !probabilities_valid {
            return Err(AssociationError::MalformedScene {
                what: "the detection or gate probability is not a probability",
            });
        }
        if !self.clutter_density.is_finite() || self.clutter_density <= 0.0 {
            return Err(AssociationError::MalformedScene {
                what: "the clutter density must be finite and positive",
            });
        }
        Ok(())
    }
}

/// One track's association probabilities: over the detections, and over not having
/// been detected at all.
///
/// The two together sum to one, which is the property that makes them usable as
/// weights in a filter update.
#[derive(Debug, Clone, PartialEq)]
pub struct AssociationProbabilities {
    /// `β_t0`: the probability this track was not detected this scan.
    pub missed: f64,
    /// `β_tj` for each detection, in the order they were supplied. A detection outside
    /// the track's gate has probability exactly zero, which is a statement that it is
    /// impossible rather than merely unlikely.
    pub detections: Vec<f64>,
}

impl AssociationProbabilities {
    /// The most likely single association, or `None` when a miss is more likely than
    /// any detection.
    ///
    /// Provided for a consumer that must eventually commit -- a display label, say --
    /// and named so that using it is a visible choice. A filter update should use the
    /// whole distribution; that is the entire point of running JPDA.
    #[must_use]
    pub fn most_likely(&self) -> Option<usize> {
        let (index, best) = self.detections.iter().enumerate().fold(
            (None, self.missed),
            |(index, best), (i, p)| {
                if *p > best {
                    (Some(i), *p)
                } else {
                    (index, best)
                }
            },
        );
        let _ = best;
        index
    }
}

/// Joint Probabilistic Data Association over a set of tracks and this scan's detections.
///
/// Returns one [`AssociationProbabilities`] per track, in the order the tracks were
/// supplied. See the module documentation for the recursion and its bounds.
///
/// # Errors
///
/// [`AssociationError::TooManyHypotheses`] when the scene is past what exact
/// enumeration will attempt; the error names which bound was hit.
///
/// [`AssociationError::MalformedScene`] when the settings are not a scene.
///
/// [`AssociationError::NonFiniteCost`] when a predicted measurement or innovation
/// covariance is not finite, or a covariance is not positive definite, in which case no
/// Mahalanobis distance exists for it.
pub fn jpda<const M: usize>(
    settings: &JpdaSettings,
    tracks: &[TrackPrediction<M>],
    detections: &[SVector<f64, M>],
) -> Result<Vec<AssociationProbabilities>, AssociationError> {
    settings.validate()?;
    if tracks.len() > MAX_TRACKS {
        return Err(AssociationError::TooManyHypotheses {
            what: "tracks",
            count: tracks.len(),
            limit: MAX_TRACKS,
        });
    }
    if detections.len() > MAX_DETECTIONS {
        return Err(AssociationError::TooManyHypotheses {
            what: "detections",
            count: detections.len(),
            limit: MAX_DETECTIONS,
        });
    }
    if tracks.is_empty() {
        return Ok(Vec::new());
    }

    // Row `t` holds `L_0(t)` at index 0 and `L_j(t)` at index `j + 1`, with zero for a
    // detection outside the gate. Zero rather than a small number: outside the gate the
    // association is being called impossible, and every joint event containing it drops
    // out of the sum by multiplication.
    let missed = 1.0 - settings.probability_of_detection * settings.probability_of_gate;
    let mut likelihoods = vec![vec![0.0; detections.len() + 1]; tracks.len()];
    for (t, track) in tracks.iter().enumerate() {
        likelihoods[t][0] = missed;
        for (j, z) in detections.iter().enumerate() {
            let innovation = z - track.predicted_measurement;
            let distance_sq =
                ChiSquareGate::squared_distance(&innovation, &track.innovation_covariance)?;
            if distance_sq > settings.gate.gate_threshold {
                continue;
            }
            let density = gaussian_density(distance_sq, &track.innovation_covariance)?;
            likelihoods[t][j + 1] =
                density * settings.probability_of_detection / settings.clutter_density;
        }
    }

    // Marginals accumulate unnormalised joint weights; one normalisation at the end.
    let mut marginals = vec![vec![0.0; detections.len() + 1]; tracks.len()];
    let mut total = 0.0;
    let mut events = 0_usize;
    let mut choice = vec![0_usize; tracks.len()];
    let mut used = vec![false; detections.len()];
    enumerate(
        0,
        1.0,
        &likelihoods,
        &mut choice,
        &mut used,
        &mut marginals,
        &mut total,
        &mut events,
    )?;

    if total <= 0.0 || !total.is_finite() {
        // Every joint event had zero weight. With a positive missed-detection term this
        // is unreachable for well-formed settings, so it means `P_D · P_G` reached one
        // and nothing was gated in: the scene says every track was certainly detected
        // and certainly not by any of these detections.
        return Err(AssociationError::MalformedScene {
            what: "no joint association event has any weight, so the scene is contradictory",
        });
    }

    Ok(marginals
        .into_iter()
        .map(|row| {
            let mut normalised = row;
            for value in &mut normalised {
                *value /= total;
            }
            let missed = normalised[0];
            AssociationProbabilities {
                missed,
                detections: normalised[1..].to_vec(),
            }
        })
        .collect())
}

/// Depth-first over tracks, assigning each either a gated detection or the miss.
///
/// Only *valid* events are generated -- a detection already taken is skipped rather
/// than generated and filtered -- which is the difference between this and a product
/// over all options, and is what makes the event cap a real bound rather than a hope.
#[allow(clippy::too_many_arguments)]
fn enumerate(
    track: usize,
    weight: f64,
    likelihoods: &[Vec<f64>],
    choice: &mut Vec<usize>,
    used: &mut Vec<bool>,
    marginals: &mut [Vec<f64>],
    total: &mut f64,
    events: &mut usize,
) -> Result<(), AssociationError> {
    if track == likelihoods.len() {
        *events += 1;
        if *events > MAX_JOINT_EVENTS {
            return Err(AssociationError::TooManyHypotheses {
                what: "joint association events",
                count: *events,
                limit: MAX_JOINT_EVENTS,
            });
        }
        *total += weight;
        for (t, option) in choice.iter().enumerate() {
            marginals[t][*option] += weight;
        }
        return Ok(());
    }

    for option in 0..likelihoods[track].len() {
        let l = likelihoods[track][option];
        if l <= 0.0 {
            continue;
        }
        if option > 0 {
            if used[option - 1] {
                continue;
            }
            used[option - 1] = true;
        }
        choice[track] = option;
        enumerate(
            track + 1,
            weight * l,
            likelihoods,
            choice,
            used,
            marginals,
            total,
            events,
        )?;
        if option > 0 {
            used[option - 1] = false;
        }
    }
    Ok(())
}

/// `N(z; ẑ, S)` from the squared Mahalanobis distance already computed for the gate.
///
/// Taking the distance as an argument rather than recomputing it is not only cheaper:
/// it guarantees the density and the gate decision are made on the same number, so a
/// detection cannot be admitted by the gate and then weighted as though it were
/// somewhere else.
fn gaussian_density<const M: usize>(
    distance_sq: f64,
    innovation_covariance: &SMatrix<f64, M, M>,
) -> Result<f64, AssociationError> {
    let Some(chol) = innovation_covariance.cholesky() else {
        return Err(AssociationError::NonFiniteCost { row: 0, col: 0 });
    };
    let determinant: f64 = chol.l().diagonal().iter().map(|d| d * d).product();
    if !determinant.is_finite() || determinant <= 0.0 {
        return Err(AssociationError::NonFiniteCost { row: 0, col: 0 });
    }
    #[allow(clippy::cast_precision_loss)]
    let m = M as f64;
    let normaliser = ((2.0 * std::f64::consts::PI).powf(m) * determinant).sqrt();
    Ok((-0.5 * distance_sq).exp() / normaliser)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> JpdaSettings {
        JpdaSettings {
            probability_of_detection: 0.9,
            probability_of_gate: 0.99,
            clutter_density: 1e-6,
            gate: ChiSquareGate::at_99_percent(2),
        }
    }

    fn track(e: f64, n: f64, var: f64) -> TrackPrediction<2> {
        TrackPrediction {
            predicted_measurement: SVector::<f64, 2>::new(e, n),
            innovation_covariance: SMatrix::<f64, 2, 2>::identity() * var,
        }
    }

    fn probabilities_sum_to_one(p: &AssociationProbabilities) -> f64 {
        p.missed + p.detections.iter().sum::<f64>()
    }

    #[test]
    fn one_track_and_one_clear_detection_associates_almost_certainly() {
        let out = jpda(
            &settings(),
            &[track(0.0, 0.0, 25.0)],
            &[SVector::<f64, 2>::new(1.0, 1.0)],
        )
        .expect("a well-formed scene");
        assert_eq!(out.len(), 1);
        assert!(
            out[0].detections[0] > 0.99,
            "a clean detection was only {} likely",
            out[0].detections[0]
        );
        assert!((probabilities_sum_to_one(&out[0]) - 1.0).abs() < 1e-12);
    }

    /// The case JPDA exists for. Two tracks, two detections, everything symmetric
    /// about the midpoint: neither assignment is better than the other, and the honest
    /// answer is a half each rather than a confident pairing.
    #[test]
    fn a_symmetric_crossing_splits_the_probability_rather_than_committing() {
        let out = jpda(
            &settings(),
            &[track(-10.0, 0.0, 100.0), track(10.0, 0.0, 100.0)],
            &[
                SVector::<f64, 2>::new(0.0, 5.0),
                SVector::<f64, 2>::new(0.0, -5.0),
            ],
        )
        .expect("a well-formed scene");
        assert_eq!(out.len(), 2);
        for (t, p) in out.iter().enumerate() {
            assert!(
                (p.detections[0] - p.detections[1]).abs() < 1e-9,
                "track {t} preferred one of two symmetric detections: {:?}",
                p.detections
            );
            assert!((probabilities_sum_to_one(p) - 1.0).abs() < 1e-12);
        }
    }

    /// A detection outside the gate must be impossible, not merely unlikely. This is
    /// the difference between a probability of exactly zero and a very small one, and
    /// it is what stops a distant clutter return from slowly dragging a track.
    #[test]
    fn a_detection_outside_the_gate_has_probability_exactly_zero() {
        let out = jpda(
            &settings(),
            &[track(0.0, 0.0, 1.0)],
            &[
                SVector::<f64, 2>::new(0.5, 0.0),
                SVector::<f64, 2>::new(500.0, 500.0),
            ],
        )
        .expect("a well-formed scene");
        assert!(
            out[0].detections[1].abs() < f64::EPSILON,
            "a gated-out detection kept probability {}",
            out[0].detections[1]
        );
        assert!(out[0].detections[0] > 0.9);
    }

    /// Two tracks contending for a single detection must share it: one detection cannot
    /// have come from both targets, and the joint enumeration is what enforces that.
    #[test]
    fn two_tracks_contending_for_one_detection_share_it() {
        let out = jpda(
            &settings(),
            &[track(-3.0, 0.0, 100.0), track(3.0, 0.0, 100.0)],
            &[SVector::<f64, 2>::new(0.0, 0.0)],
        )
        .expect("a well-formed scene");
        let claimed: f64 = out.iter().map(|p| p.detections[0]).sum();
        assert!(
            claimed <= 1.0 + 1e-9,
            "the one detection was claimed {claimed} times over, so the joint \
             constraint is not being enforced"
        );
        assert!(
            out[0].detections[0] > 0.2 && out[1].detections[0] > 0.2,
            "one track took the whole detection: {:?} and {:?}",
            out[0].detections,
            out[1].detections
        );
    }

    /// With no detections at all, every track was missed. The alternative -- returning
    /// nothing, or an empty probability vector -- would be a tracker with no opinion
    /// about a scan that told it something.
    #[test]
    fn no_detections_means_every_track_was_missed() {
        let out = jpda(&settings(), &[track(0.0, 0.0, 25.0)], &[]).expect("a valid empty scan");
        assert!((out[0].missed - 1.0).abs() < 1e-12);
        assert!(out[0].detections.is_empty());
    }

    #[test]
    fn a_scene_past_the_bound_is_refused_rather_than_truncated() {
        let tracks: Vec<TrackPrediction<2>> = (0..=MAX_TRACKS)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let e = i as f64;
                track(e, 0.0, 25.0)
            })
            .collect();
        let err = jpda(&settings(), &tracks, &[]).unwrap_err();
        assert_eq!(
            err,
            AssociationError::TooManyHypotheses {
                what: "tracks",
                count: MAX_TRACKS + 1,
                limit: MAX_TRACKS,
            }
        );
    }

    #[test]
    fn a_clutter_density_of_zero_is_refused() {
        let mut s = settings();
        s.clutter_density = 0.0;
        assert!(matches!(
            jpda(&s, &[track(0.0, 0.0, 25.0)], &[]).unwrap_err(),
            AssociationError::MalformedScene { .. }
        ));
    }

    /// More clutter must make the tracker trust a detection less. If this did not hold,
    /// the clutter density would be a parameter with no effect and the settings would
    /// be decoration.
    #[test]
    fn heavier_clutter_lowers_the_association_probability() {
        let z = [SVector::<f64, 2>::new(2.0, 0.0)];
        let mut sparse = settings();
        sparse.clutter_density = 1e-8;
        let mut dense = settings();
        dense.clutter_density = 1e-2;
        let a = jpda(&sparse, &[track(0.0, 0.0, 25.0)], &z).expect("valid");
        let b = jpda(&dense, &[track(0.0, 0.0, 25.0)], &z).expect("valid");
        assert!(
            a[0].detections[0] > b[0].detections[0],
            "clutter density did not change the association: {} vs {}",
            a[0].detections[0],
            b[0].detections[0]
        );
    }
}
