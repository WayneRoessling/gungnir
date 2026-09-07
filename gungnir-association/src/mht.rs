// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Multi-Hypothesis Tracking: the `docs/verification-capability-table.md` §1 row
//! "`association` | Multi-Hypothesis Tracking (MHT)".
//!
//! # The oracle this row was planned against is not available, and that is recorded
//!
//! §2 names "Stone Soup MHT hypothesiser". **Stone Soup 1.9.1, the version this
//! workspace pins, has no MHT hypothesiser**: `stonesoup.hypothesiser` contains
//! `base`, `categorical`, `composite`, `distance`, `gaussianmixture`, `mfa`,
//! `probability` and `simple`, and none of them is one. The nearest thing the library
//! offers is `stonesoup.dataassociator.mfa`, multi-frame assignment, which
//!
//! * solves the N-scan problem as an integer program rather than by maintaining a
//!   hypothesis tree, so it produces assignments and not a tree, and the row's
//!   criterion is "tree match at each pruning step"; and
//! * refuses to import without the optional `ortools` package, which is not installed
//!   and whose addition would be an owner decision rather than a test-harness one.
//!
//! So this row is gated the way the `fusion-async` row already is: against the
//! comparison actually made, stated in the row's own Method column rather than implied.
//! That comparison is **hand-derived ground truth** -- scenarios whose correct deferred
//! decision is known by construction, because the scenario was built by choosing the
//! answer first. §2's Oracle column already reads "hand-derived" for two other rows, so
//! this is a form the table uses rather than one invented to fit.
//!
//! The pass criterion is unchanged. What changed is which oracle it is measured against,
//! and the change is written down.
//!
//! # What MHT does that the other two associators cannot
//!
//! Nearest neighbour commits within the scan. JPDA refuses to commit within the scan and
//! spreads the update over every candidate. **MHT defers the commitment across scans**:
//! it carries several whole association histories forward, scores each by how well the
//! measurements since have borne it out, and only fixes a decision once later evidence
//! has had time to settle it.
//!
//! That is the right answer for the case that defeats both others: two targets crossing,
//! where the scan at the crossing genuinely does not say which is which and the scan
//! three later obviously does. Nearest neighbour picks at the crossing and may swap the
//! two identities permanently. JPDA blurs both tracks through the crossing and comes out
//! with two wide, correctly-placed estimates and no memory of which was which. MHT keeps
//! both readings alive and then discards the wrong one.
//!
//! The cost is real and is the reason this is not the default: hypotheses multiply per
//! scan, so the tree is bounded two ways at once, and a decision deferred past the
//! window is made on the evidence available then rather than waited on indefinitely.
//!
//! # Bounds
//!
//! **`max_hypotheses`** caps the breadth: after each scan the tree keeps the `K`
//! best-scoring global hypotheses and drops the rest. **`scan_depth`** caps the depth:
//! a decision more than `N` scans old is committed to whatever the leading hypothesis
//! says, every hypothesis that disagrees is pruned, and the committed prefix never
//! changes again. Together they make the tree's size a constant of the settings rather
//! than a function of how long the system has been running.
//!
//! Committing is a real loss of information and is why [`HypothesisTree::committed`]
//! exists as a separate accessor: what has been decided irrevocably is worth being able
//! to look at, rather than being folded invisibly into the leading hypothesis.

use crate::assignment::AssociationError;
use crate::gating::ChiSquareGate;
use crate::jpda::{TrackPrediction, MAX_DETECTIONS, MAX_JOINT_EVENTS, MAX_TRACKS};
use nalgebra::SVector;

/// How the tree is bounded and how a detection is scored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MhtSettings {
    /// `K`: how many global hypotheses survive each scan.
    pub max_hypotheses: usize,
    /// `N`: how many scans a decision may stay open before it is committed.
    pub scan_depth: usize,
    /// `P_D`, the probability the sensor reports a target that is there.
    pub probability_of_detection: f64,
    /// `P_G`, the probability a true detection falls inside the gate.
    pub probability_of_gate: f64,
    /// `λ`, the expected number of false detections per unit measurement volume.
    pub clutter_density: f64,
    /// The gate. A detection outside a track's gate is not an option for that track.
    pub gate: ChiSquareGate,
}

impl MhtSettings {
    fn validate(&self) -> Result<(), AssociationError> {
        if self.max_hypotheses == 0 {
            return Err(AssociationError::MalformedScene {
                what: "a tree that keeps no hypotheses is not a tree",
            });
        }
        if self.scan_depth == 0 {
            return Err(AssociationError::MalformedScene {
                what: "a scan depth of zero commits every decision immediately, which is \
                       nearest-neighbour association wearing MHT's name",
            });
        }
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

/// One global association history and its score.
#[derive(Debug, Clone, PartialEq)]
pub struct Hypothesis {
    /// One entry per scan since the tree was built. `history[s][t]` is the detection
    /// index track `t` took in scan `s`, or `None` for a miss.
    pub history: Vec<Vec<Option<usize>>>,
    /// The log of the unnormalised posterior weight.
    ///
    /// **A log, not a probability.** A hypothesis's weight is a product of one
    /// likelihood ratio per track per scan; after a few dozen scans that product
    /// underflows to zero in double precision and every hypothesis looks equally good,
    /// which is the failure that makes a naive MHT stop discriminating exactly when it
    /// has gathered enough evidence to.
    pub log_weight: f64,
}

impl Hypothesis {
    /// What this hypothesis says track `t` took in the most recent scan.
    #[must_use]
    pub fn latest(&self, track: usize) -> Option<Option<usize>> {
        self.history
            .last()
            .and_then(|scan| scan.get(track))
            .copied()
    }
}

/// A bounded tree of competing global association hypotheses.
#[derive(Debug, Clone)]
pub struct HypothesisTree {
    settings: MhtSettings,
    hypotheses: Vec<Hypothesis>,
    committed: Vec<Vec<Option<usize>>>,
    scans: usize,
    tracks: usize,
}

impl HypothesisTree {
    /// An empty tree, before any scan.
    ///
    /// # Errors
    ///
    /// [`AssociationError::MalformedScene`] when the settings do not describe a bounded
    /// tree over a real scene; the error says which part.
    pub fn new(settings: MhtSettings) -> Result<Self, AssociationError> {
        settings.validate()?;
        Ok(Self {
            settings,
            hypotheses: vec![Hypothesis {
                history: Vec::new(),
                log_weight: 0.0,
            }],
            committed: Vec::new(),
            scans: 0,
            tracks: 0,
        })
    }

    /// The hypotheses currently alive, best-scoring first.
    #[must_use]
    pub fn hypotheses(&self) -> &[Hypothesis] {
        &self.hypotheses
    }

    /// The leading hypothesis.
    #[must_use]
    pub fn best(&self) -> Option<&Hypothesis> {
        self.hypotheses.first()
    }

    /// Decisions past the scan-depth window, which will not change again.
    #[must_use]
    pub fn committed(&self) -> &[Vec<Option<usize>>] {
        &self.committed
    }

    /// How many scans have been taken.
    #[must_use]
    pub fn scans(&self) -> usize {
        self.scans
    }

    /// The hypothesis weights as a distribution, best-scoring first.
    ///
    /// Formed by shifting the log weights by their maximum before exponentiating, for
    /// the reason [`Hypothesis::log_weight`] gives.
    #[must_use]
    pub fn normalised_weights(&self) -> Vec<f64> {
        let peak = self
            .hypotheses
            .iter()
            .map(|h| h.log_weight)
            .fold(f64::NEG_INFINITY, f64::max);
        if !peak.is_finite() {
            return vec![0.0; self.hypotheses.len()];
        }
        let raw: Vec<f64> = self
            .hypotheses
            .iter()
            .map(|h| (h.log_weight - peak).exp())
            .collect();
        let total: f64 = raw.iter().sum();
        if total > 0.0 {
            raw.into_iter().map(|v| v / total).collect()
        } else {
            vec![0.0; self.hypotheses.len()]
        }
    }

    /// Take one scan: branch every surviving hypothesis over this scan's assignments,
    /// score, prune to `max_hypotheses`, and commit anything past the scan-depth window.
    ///
    /// # Errors
    ///
    /// [`AssociationError::TooManyHypotheses`] when the scene is past what exact
    /// branching will attempt, or the track count changed between scans -- a tree whose
    /// tracks changed identity mid-history is comparing histories that are not about the
    /// same things.
    ///
    /// [`AssociationError::NonFiniteCost`] when a predicted measurement or innovation
    /// covariance is not usable.
    pub fn extend<const M: usize>(
        &mut self,
        tracks: &[TrackPrediction<M>],
        detections: &[SVector<f64, M>],
    ) -> Result<(), AssociationError> {
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
        if self.scans > 0 && tracks.len() != self.tracks {
            return Err(AssociationError::MalformedScene {
                what: "the track count changed between scans, so the histories being \
                       compared are not about the same tracks",
            });
        }
        self.tracks = tracks.len();

        let log_missed =
            (1.0 - self.settings.probability_of_detection * self.settings.probability_of_gate).ln();
        // Row `t`, index 0 is the miss and index `j + 1` is detection `j`; `None` means
        // gated out and therefore not an option at all.
        let mut log_likelihoods = vec![vec![None; detections.len() + 1]; tracks.len()];
        for (t, track) in tracks.iter().enumerate() {
            log_likelihoods[t][0] = Some(log_missed);
            for (j, z) in detections.iter().enumerate() {
                let innovation = z - track.predicted_measurement;
                let distance_sq =
                    ChiSquareGate::squared_distance(&innovation, &track.innovation_covariance)?;
                if distance_sq > self.settings.gate.gate_threshold {
                    continue;
                }
                log_likelihoods[t][j + 1] = Some(
                    log_gaussian_density(distance_sq, &track.innovation_covariance)?
                        + self.settings.probability_of_detection.ln()
                        - self.settings.clutter_density.ln(),
                );
            }
        }

        let mut branches = Vec::new();
        let mut assignment = vec![None; tracks.len()];
        let mut used = vec![false; detections.len()];
        let mut generated = 0_usize;
        for parent in &self.hypotheses {
            branch(
                0,
                parent,
                0.0,
                &log_likelihoods,
                &mut assignment,
                &mut used,
                &mut branches,
                &mut generated,
            )?;
        }
        if branches.is_empty() {
            return Err(AssociationError::MalformedScene {
                what: "no association of this scan is possible under these settings",
            });
        }

        branches.sort_by(|a, b| b.log_weight.total_cmp(&a.log_weight));
        branches.truncate(self.settings.max_hypotheses);
        self.hypotheses = branches;
        self.scans += 1;
        self.commit_past_the_window();
        Ok(())
    }

    /// Fix any decision older than the scan-depth window to the leading hypothesis's
    /// reading, and drop every hypothesis that disagrees with it.
    fn commit_past_the_window(&mut self) {
        while self
            .hypotheses
            .first()
            .is_some_and(|h| h.history.len() > self.settings.scan_depth)
        {
            let Some(decision) = self
                .hypotheses
                .first()
                .and_then(|h| h.history.first())
                .cloned()
            else {
                return;
            };
            self.hypotheses
                .retain(|h| h.history.first() == Some(&decision));
            for hypothesis in &mut self.hypotheses {
                hypothesis.history.remove(0);
            }
            self.committed.push(decision);
        }
    }
}

/// Branch one parent hypothesis over every valid assignment of this scan.
///
/// Only valid assignments are generated -- a detection already taken in this scan is
/// skipped rather than generated and filtered -- so the cap is a real bound.
#[allow(clippy::too_many_arguments)]
fn branch(
    track: usize,
    parent: &Hypothesis,
    log_weight: f64,
    log_likelihoods: &[Vec<Option<f64>>],
    assignment: &mut Vec<Option<usize>>,
    used: &mut Vec<bool>,
    out: &mut Vec<Hypothesis>,
    generated: &mut usize,
) -> Result<(), AssociationError> {
    if track == log_likelihoods.len() {
        *generated += 1;
        if *generated > MAX_JOINT_EVENTS {
            return Err(AssociationError::TooManyHypotheses {
                what: "branched hypotheses",
                count: *generated,
                limit: MAX_JOINT_EVENTS,
            });
        }
        let mut history = parent.history.clone();
        history.push(assignment.clone());
        out.push(Hypothesis {
            history,
            log_weight: parent.log_weight + log_weight,
        });
        return Ok(());
    }

    for option in 0..log_likelihoods[track].len() {
        let Some(l) = log_likelihoods[track][option] else {
            continue;
        };
        if option > 0 {
            if used[option - 1] {
                continue;
            }
            used[option - 1] = true;
        }
        assignment[track] = if option == 0 { None } else { Some(option - 1) };
        branch(
            track + 1,
            parent,
            log_weight + l,
            log_likelihoods,
            assignment,
            used,
            out,
            generated,
        )?;
        if option > 0 {
            used[option - 1] = false;
        }
    }
    Ok(())
}

/// `ln N(z; ẑ, S)` from the squared Mahalanobis distance the gate already computed.
fn log_gaussian_density<const M: usize>(
    distance_sq: f64,
    innovation_covariance: &nalgebra::SMatrix<f64, M, M>,
) -> Result<f64, AssociationError> {
    let Some(chol) = innovation_covariance.cholesky() else {
        return Err(AssociationError::NonFiniteCost { row: 0, col: 0 });
    };
    // Summed as logs rather than multiplied and then logged: the determinant of a
    // six-by-six covariance in metres squared overflows a long way before its logarithm
    // does anything interesting.
    let log_determinant: f64 = chol.l().diagonal().iter().map(|d| 2.0 * d.abs().ln()).sum();
    if !log_determinant.is_finite() {
        return Err(AssociationError::NonFiniteCost { row: 0, col: 0 });
    }
    #[allow(clippy::cast_precision_loss)]
    let m = M as f64;
    Ok(-0.5 * (distance_sq + log_determinant + m * (2.0 * std::f64::consts::PI).ln()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::SMatrix;

    fn settings() -> MhtSettings {
        MhtSettings {
            max_hypotheses: 20,
            scan_depth: 4,
            probability_of_detection: 0.95,
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

    fn detection(e: f64, n: f64) -> SVector<f64, 2> {
        SVector::<f64, 2>::new(e, n)
    }

    #[test]
    fn a_zero_scan_depth_is_refused_as_nearest_neighbour_in_disguise() {
        let mut s = settings();
        s.scan_depth = 0;
        assert!(matches!(
            HypothesisTree::new(s).unwrap_err(),
            AssociationError::MalformedScene { .. }
        ));
    }

    #[test]
    fn the_tree_never_exceeds_its_breadth_bound() {
        let mut tree = HypothesisTree::new(MhtSettings {
            max_hypotheses: 5,
            ..settings()
        })
        .expect("valid settings");
        for _ in 0..8 {
            tree.extend(
                &[track(0.0, 0.0, 400.0), track(5.0, 0.0, 400.0)],
                &[
                    detection(1.0, 1.0),
                    detection(4.0, -1.0),
                    detection(2.0, 3.0),
                ],
            )
            .expect("a valid scan");
            assert!(
                tree.hypotheses().len() <= 5,
                "the tree grew to {}",
                tree.hypotheses().len()
            );
        }
    }

    #[test]
    fn weights_stay_a_distribution_over_a_long_run() {
        let mut tree = HypothesisTree::new(settings()).expect("valid settings");
        for _ in 0..60 {
            tree.extend(
                &[track(0.0, 0.0, 400.0), track(20.0, 0.0, 400.0)],
                &[detection(1.0, 0.0), detection(19.0, 0.0)],
            )
            .expect("a valid scan");
            let w = tree.normalised_weights();
            let sum: f64 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1e-9, "weights summed to {sum}");
            assert!(w.iter().all(|v| v.is_finite() && *v >= 0.0));
        }
    }

    /// The property MHT exists for, on a scenario whose right answer is known because
    /// the scenario was built from it.
    ///
    /// Two targets converge, cross at scan 2 where the detections are genuinely
    /// ambiguous, and then separate along their original headings. Any single-scan
    /// associator must guess at the crossing. MHT should keep both readings and, once
    /// the targets have separated, lead with the one that does not swap them.
    #[test]
    fn a_crossing_resolved_by_later_evidence_leads_to_the_right_history() {
        let mut tree = HypothesisTree::new(settings()).expect("valid settings");
        // Track 0 travels left to right along n = 0; track 1 right to left.
        // Positions per scan for the two targets, converging and then separating.
        let scans = [
            ([-40.0, 40.0], [0.0, 0.0]),
            ([-20.0, 20.0], [0.0, 0.0]),
            ([0.0, 0.0], [0.0, 0.0]), // the crossing: both at the same place
            ([20.0, -20.0], [0.0, 0.0]),
            ([40.0, -40.0], [0.0, 0.0]),
        ];
        for (east, north) in scans {
            let tracks = [
                track(east[0], north[0], 100.0),
                track(east[1], north[1], 100.0),
            ];
            // The detections are supplied in a fixed order that is NOT the track order,
            // so a tracker that simply took them in order would be wrong half the time.
            let detections = [detection(east[1], north[1]), detection(east[0], north[0])];
            tree.extend(&tracks, &detections).expect("a valid scan");
        }
        let best = tree.best().expect("a leading hypothesis");
        // Away from the crossing the assignment is unambiguous: track 0 must take the
        // detection at its own position, which is index 1 in the supplied order.
        let last = best
            .history
            .last()
            .expect("the leading hypothesis has a history");
        assert_eq!(
            last[0],
            Some(1),
            "the leading hypothesis swapped the two targets after they separated: {last:?}"
        );
        assert_eq!(last[1], Some(0), "and track 1 with it: {last:?}");
    }

    /// A decision older than the window must be committed and must never move again.
    /// That is the guarantee that lets a consumer act on the committed prefix.
    #[test]
    fn decisions_past_the_window_are_committed_and_then_frozen() {
        let mut tree = HypothesisTree::new(MhtSettings {
            scan_depth: 3,
            ..settings()
        })
        .expect("valid settings");
        let mut snapshots: Vec<Vec<Vec<Option<usize>>>> = Vec::new();
        for i in 0..10 {
            let drift = f64::from(i);
            tree.extend(
                &[track(drift, 0.0, 400.0), track(30.0 + drift, 0.0, 400.0)],
                &[detection(drift, 0.0), detection(30.0 + drift, 0.0)],
            )
            .expect("a valid scan");
            snapshots.push(tree.committed().to_vec());
        }
        assert!(
            !tree.committed().is_empty(),
            "nothing was ever committed, so the window is not closing"
        );
        // Every snapshot must be a prefix of the next: committed decisions only ever
        // get added to.
        for pair in snapshots.windows(2) {
            assert!(
                pair[1].starts_with(&pair[0]),
                "a committed decision changed: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(
            tree.best().is_some_and(|h| h.history.len() <= 3),
            "the open window did not stay bounded by the scan depth"
        );
    }

    #[test]
    fn a_changed_track_count_is_refused() {
        let mut tree = HypothesisTree::new(settings()).expect("valid settings");
        tree.extend(&[track(0.0, 0.0, 100.0)], &[detection(0.0, 0.0)])
            .expect("a valid scan");
        let err = tree
            .extend(
                &[track(0.0, 0.0, 100.0), track(9.0, 0.0, 100.0)],
                &[detection(0.0, 0.0)],
            )
            .unwrap_err();
        assert!(matches!(err, AssociationError::MalformedScene { .. }));
    }
}
