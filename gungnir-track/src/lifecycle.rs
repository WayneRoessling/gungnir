// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The track lifecycle state machine: the verification-capability-table.md §1 row
//! "Track lifecycle (init/confirm/coast/delete)".
//!
//! Oracle: Stone Soup's track initiators and deleters. Criterion: **exact match on
//! step index** for an identical detection/miss sequence -- not just the final state,
//! but the cycle each transition happened on.
//!
//! # The two rules, and where they come from
//!
//! The rules were read off the oracle rather than assumed, because "exact match on
//! step index" leaves no room for an off-by-one:
//!
//! * **Confirm on cumulative hits.** `stonesoup.initiator.simple.MultiMeasurementInitiator`
//!   confirms when `sum(1 for state in track if isinstance(state, Update)) >= min_points`
//!   -- a running total of updates, not a run of consecutive ones. A miss during
//!   establishment therefore delays confirmation but does not undo it.
//! * **Delete on consecutive misses.** `stonesoup.deleter.time.UpdateTimeStepsDeleter`
//!   deletes once the steps since the last update reach `time_steps_since_update`;
//!   driving it directly shows deletion on the *n*th miss for `n = 1, 2, 3`, so the
//!   comparison is `>=`, not `>`.
//!
//! **This corrects the scaffold.** `TrackManager::confirm_threshold`'s doc comment
//! previously said "cycles of *consecutive* hits". Consecutive counting is both
//! stricter than the oracle -- one miss at Pd = 0.8 would restart establishment, so a
//! real target takes far longer to confirm than it should -- and impossible to
//! reconcile with a row whose criterion is exact agreement with Stone Soup. Cumulative
//! is what the oracle does and the more usual choice; the comment moved to match the
//! behaviour, not the other way round.
//!
//! # Why the order of the checks is fixed
//!
//! Hits are applied before misses, and deletion is evaluated after coasting, within
//! one step. That ordering is what makes the step index well defined: evaluating
//! deletion first would delete a track on the same cycle it was re-acquired, and
//! applying misses first would let a track coast and confirm in the same step. The
//! scaffold's own comment warned against reordering these casually, and the
//! differential test is what holds it.

use crate::{Track, TrackId, TrackStatus};
use gungnir_association::Associator;
use nalgebra::{SMatrix, SVector};

/// What one cycle of the state machine did.
///
/// Returned rather than logged so a caller can act on it and a test can read the step
/// index a transition happened on, which is exactly what the row compares. Nothing
/// here is inferred: each list holds the tracks that actually changed on this step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepOutcome {
    /// The index of the cycle this outcome describes, counting from zero.
    pub step: u64,
    /// Tracks that became [`TrackStatus::Confirmed`] on this step.
    pub confirmed: Vec<TrackId>,
    /// Tracks that became [`TrackStatus::Coasting`] on this step.
    pub coasting: Vec<TrackId>,
    /// Tracks that became [`TrackStatus::Deleted`] on this step. They remain readable
    /// through [`TrackManager::tracks`] until the next step, then are pruned.
    pub deleted: Vec<TrackId>,
    /// Ids named in `hits` or `misses` that match no live track. Reported rather than
    /// ignored: an association result naming a track that does not exist means the
    /// caller and the manager disagree about what is being tracked.
    pub unknown: Vec<TrackId>,
    /// Ids named in *both* `hits` and `misses`. Contradictory input; the hit is taken,
    /// because evidence of presence outweighs evidence of absence, and the conflict is
    /// reported so it cannot pass unnoticed.
    pub conflicting: Vec<TrackId>,
}

impl StepOutcome {
    /// Whether anything at all changed on this step.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.confirmed.is_empty() && self.coasting.is_empty() && self.deleted.is_empty()
    }

    /// Whether the caller and the manager disagreed about the track set.
    #[must_use]
    pub fn has_disagreement(&self) -> bool {
        !self.unknown.is_empty() || !self.conflicting.is_empty()
    }
}

/// Owns the confirm/coast/delete state machine. Consumers (association output) feed
/// hit/miss events in; this decides transitions.
#[derive(Debug)]
pub struct TrackManager<A: Associator> {
    associator: A,
    tracks: Vec<Track>,
    confirm_threshold: u32,
    delete_after_misses: u32,
    next_id: u64,
    step_index: u64,
}

impl<A: Associator> TrackManager<A> {
    /// Create an empty manager with the given confirm/delete thresholds (in cycles).
    pub fn new(associator: A, confirm_threshold: u32, delete_after_misses: u32) -> Self {
        Self {
            associator,
            tracks: Vec::new(),
            confirm_threshold,
            delete_after_misses,
            next_id: 0,
            step_index: 0,
        }
    }

    /// Every track the manager holds, including any deleted on the most recent step.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// The tracks a consumer should act on: everything not deleted.
    pub fn live_tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks
            .iter()
            .filter(|t| t.status != TrackStatus::Deleted)
    }

    /// Cumulative hits before a tentative track is confirmed.
    ///
    /// Cumulative, not consecutive: see the module documentation, which records why
    /// this differs from the scaffold's original comment.
    pub fn confirm_threshold(&self) -> u32 {
        self.confirm_threshold
    }

    /// Cycles of consecutive misses before a track is deleted. Deletion happens *on*
    /// the nth consecutive miss, matching the oracle.
    pub fn delete_after_misses(&self) -> u32 {
        self.delete_after_misses
    }

    /// The index of the next cycle [`TrackManager::step`] will run.
    pub fn step_index(&self) -> u64 {
        self.step_index
    }

    /// The association strategy this manager was built with.
    pub fn associator(&mut self) -> &mut A {
        &mut self.associator
    }

    /// Look one track up by id, deleted ones included until they are pruned.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Start a new tentative track from an initial estimate.
    ///
    /// Ids are drawn from a monotonic counter and are never reused, which is stronger
    /// than the "never reused while the track is active" that
    /// `gungnir_core::ident::TrackId` requires: an id that reappeared after a deletion
    /// would silently splice two different objects together in the journal, and the
    /// journal outlives the session.
    ///
    /// A zero [`TrackManager::confirm_threshold`] confirms the track immediately,
    /// which is degenerate but well defined rather than a special case.
    pub fn initiate(&mut self, state: SVector<f64, 6>, covariance: SMatrix<f64, 6, 6>) -> TrackId {
        let id = TrackId(self.next_id);
        self.next_id += 1;
        let status = if self.confirm_threshold == 0 {
            TrackStatus::Confirmed
        } else {
            TrackStatus::Tentative
        };
        self.tracks.push(Track {
            id,
            status,
            state,
            covariance,
            misses_since_update: 0,
            hits: 0,
        });
        id
    }

    /// Write a track's kinematics back after a filter has moved them.
    ///
    /// The lifecycle owns the track *record*; an estimator owns the *estimate*. The
    /// pipeline predicts and updates a filter per track and hands the result back here,
    /// rather than reaching into the record, so this crate keeps deciding what a track
    /// is and never has to know which filter produced its numbers. An unknown id is
    /// ignored and reported by the return value rather than creating a track: a
    /// caller estimating a track this manager has deleted is a disagreement, and
    /// resurrecting it silently is how a deleted track comes back to life.
    ///
    /// Returns whether a live track was updated.
    pub fn update_estimate(
        &mut self,
        id: TrackId,
        state: SVector<f64, 6>,
        covariance: SMatrix<f64, 6, 6>,
    ) -> bool {
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|t| t.id == id && t.status != TrackStatus::Deleted)
        else {
            return false;
        };
        track.state = state;
        track.covariance = covariance;
        true
    }

    /// Advance the lifecycle state machine by one cycle given association results.
    ///
    /// Exact-match pass criterion means the *step index* of each transition matters,
    /// not just the final state -- don't reorder confirm/coast/delete checks casually.
    /// The order is: prune what was deleted last step, apply hits, then apply misses,
    /// evaluating coasting before deletion. The module documentation says why.
    ///
    /// A track named in neither list is left unchanged. That is deliberate: inferring
    /// a miss from silence would age -- and eventually delete -- tracks a caller merely
    /// forgot to mention, and this API takes both lists explicitly so it does not have
    /// to guess.
    pub fn step(&mut self, hits: &[TrackId], misses: &[TrackId]) -> StepOutcome {
        // Tracks deleted on the previous step were kept so the caller could see them;
        // they go now, before anything else can reference them.
        self.tracks.retain(|t| t.status != TrackStatus::Deleted);

        let mut outcome = StepOutcome {
            step: self.step_index,
            ..StepOutcome::default()
        };

        for id in hits {
            let Some(track) = self.tracks.iter_mut().find(|t| t.id == *id) else {
                outcome.unknown.push(*id);
                continue;
            };
            track.misses_since_update = 0;
            track.hits = track.hits.saturating_add(1);
            // A coasting track that is re-acquired is confirmed again; a tentative one
            // is confirmed once its cumulative hits reach the threshold.
            let confirms = match track.status {
                TrackStatus::Tentative => track.hits >= self.confirm_threshold,
                TrackStatus::Coasting => true,
                TrackStatus::Confirmed | TrackStatus::Deleted => false,
            };
            if confirms {
                track.status = TrackStatus::Confirmed;
                outcome.confirmed.push(*id);
            }
        }

        for id in misses {
            if hits.contains(id) {
                // Contradictory: the hit above already stands. Reported, not silent.
                outcome.conflicting.push(*id);
                continue;
            }
            let Some(track) = self.tracks.iter_mut().find(|t| t.id == *id) else {
                outcome.unknown.push(*id);
                continue;
            };
            track.misses_since_update = track.misses_since_update.saturating_add(1);
            if track.status == TrackStatus::Confirmed {
                track.status = TrackStatus::Coasting;
                outcome.coasting.push(*id);
            }
            // Deletion is evaluated after coasting, so a confirmed track that runs out
            // of patience on the same step both coasts and dies, in that order.
            if track.misses_since_update >= self.delete_after_misses {
                track.status = TrackStatus::Deleted;
                outcome.deleted.push(*id);
            }
        }

        self.step_index += 1;
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_association::{AssociationError, GlobalNearestNeighbor};

    fn manager(confirm: u32, delete: u32) -> TrackManager<GlobalNearestNeighbor> {
        TrackManager::new(GlobalNearestNeighbor, confirm, delete)
    }

    fn new_track<A: Associator>(m: &mut TrackManager<A>) -> TrackId {
        m.initiate(SVector::<f64, 6>::zeros(), SMatrix::<f64, 6, 6>::identity())
    }

    fn status<A: Associator>(m: &TrackManager<A>, id: TrackId) -> Option<TrackStatus> {
        m.track(id).map(|t| t.status)
    }

    #[test]
    fn a_new_track_is_tentative() {
        let mut m = manager(3, 3);
        let id = new_track(&mut m);
        assert_eq!(status(&m, id), Some(TrackStatus::Tentative));
        assert_eq!(m.step_index(), 0);
    }

    /// Confirmation is on cumulative hits, so the step index is the third hit even
    /// though a miss intervened. This is the behaviour that differs from the
    /// scaffold's original "consecutive" comment.
    #[test]
    fn confirmation_counts_cumulative_hits_across_a_miss() {
        let mut m = manager(3, 10);
        let id = new_track(&mut m);
        m.step(&[id], &[]); // step 0: 1 hit
        m.step(&[], &[id]); // step 1: miss, run broken
        assert_eq!(status(&m, id), Some(TrackStatus::Tentative));
        m.step(&[id], &[]); // step 2: 2 hits
        assert_eq!(status(&m, id), Some(TrackStatus::Tentative));
        let out = m.step(&[id], &[]); // step 3: 3 hits -> confirm
        assert_eq!(status(&m, id), Some(TrackStatus::Confirmed));
        assert_eq!(out.confirmed, vec![id]);
        assert_eq!(out.step, 3, "confirmation step index");
    }

    /// A confirmed track that misses coasts, and dies on the nth consecutive miss.
    #[test]
    fn coasting_then_deletion_on_the_nth_miss() {
        let mut m = manager(1, 3);
        let id = new_track(&mut m);
        let out = m.step(&[id], &[]);
        assert_eq!(out.confirmed, vec![id]);

        let out = m.step(&[], &[id]); // miss 1
        assert_eq!(out.coasting, vec![id]);
        assert!(out.deleted.is_empty());
        assert_eq!(status(&m, id), Some(TrackStatus::Coasting));

        let out = m.step(&[], &[id]); // miss 2
        assert!(out.deleted.is_empty());

        let out = m.step(&[], &[id]); // miss 3 -> delete
        assert_eq!(out.deleted, vec![id]);
        assert_eq!(out.step, 3);
        assert_eq!(status(&m, id), Some(TrackStatus::Deleted));
    }

    /// A deleted track stays readable for the step it died on, then is pruned.
    #[test]
    fn a_deleted_track_survives_one_step_then_is_pruned() {
        let mut m = manager(1, 1);
        let id = new_track(&mut m);
        m.step(&[id], &[]);
        m.step(&[], &[id]);
        assert_eq!(status(&m, id), Some(TrackStatus::Deleted));
        assert_eq!(m.live_tracks().count(), 0);
        m.step(&[], &[]);
        assert_eq!(status(&m, id), None, "deleted track was not pruned");
    }

    /// Re-acquiring a coasting track confirms it again and clears the miss run.
    #[test]
    fn a_hit_rescues_a_coasting_track() {
        let mut m = manager(1, 3);
        let id = new_track(&mut m);
        m.step(&[id], &[]);
        m.step(&[], &[id]);
        m.step(&[], &[id]);
        assert_eq!(status(&m, id), Some(TrackStatus::Coasting));
        let out = m.step(&[id], &[]);
        assert_eq!(out.confirmed, vec![id]);
        assert_eq!(m.track(id).map(|t| t.misses_since_update), Some(0));
        // The miss run restarts, so it survives two more misses.
        m.step(&[], &[id]);
        m.step(&[], &[id]);
        assert_eq!(status(&m, id), Some(TrackStatus::Coasting));
    }

    /// Ids are never reused, even after a deletion: a reused id would splice two
    /// different objects together in the journal.
    #[test]
    fn ids_are_never_reused() {
        let mut m = manager(1, 1);
        let first = new_track(&mut m);
        m.step(&[id_of(first)], &[]);
        m.step(&[], &[first]);
        m.step(&[], &[]); // prune
        let second = new_track(&mut m);
        assert_ne!(first, second);
    }

    fn id_of(id: TrackId) -> TrackId {
        id
    }

    /// An unknown id is reported, not silently ignored.
    #[test]
    fn unknown_ids_are_reported() {
        let mut m = manager(1, 1);
        let out = m.step(&[TrackId(99)], &[TrackId(98)]);
        assert_eq!(out.unknown, vec![TrackId(99), TrackId(98)]);
        assert!(out.has_disagreement());
    }

    /// A contradictory id is reported and the hit wins.
    #[test]
    fn a_conflicting_id_takes_the_hit_and_is_reported() {
        let mut m = manager(1, 1);
        let id = new_track(&mut m);
        let out = m.step(&[id], &[id]);
        assert_eq!(out.conflicting, vec![id]);
        assert_eq!(status(&m, id), Some(TrackStatus::Confirmed));
        assert_eq!(m.track(id).map(|t| t.misses_since_update), Some(0));
    }

    /// A track named in neither list is left alone rather than aged.
    #[test]
    fn an_unmentioned_track_is_unchanged() {
        let mut m = manager(2, 2);
        let id = new_track(&mut m);
        m.step(&[id], &[]);
        let before = m.track(id).map(|t| (t.hits, t.misses_since_update));
        let out = m.step(&[], &[]);
        assert!(out.is_quiet());
        assert_eq!(m.track(id).map(|t| (t.hits, t.misses_since_update)), before);
    }

    /// Independent tracks do not interfere: one dying must not disturb another.
    #[test]
    fn tracks_are_independent() {
        let mut m = manager(2, 2);
        let a = new_track(&mut m);
        let b = new_track(&mut m);
        m.step(&[a, b], &[]); // step 0: both get their first hit
        let out = m.step(&[a], &[b]); // step 1: a reaches two hits, b misses once
        assert_eq!(out.confirmed, vec![a], "a confirms on its second hit");
        assert!(out.deleted.is_empty(), "b has only missed once");
        let out = m.step(&[a], &[b]); // step 2: b reaches two misses
        assert!(out.confirmed.is_empty(), "a is already confirmed");
        assert_eq!(out.deleted, vec![b], "b dies on its second miss");
        assert_eq!(status(&m, a), Some(TrackStatus::Confirmed));
        assert_eq!(m.track(a).map(|t| t.hits), Some(3), "a kept accruing hits");
    }

    /// A zero confirm threshold is degenerate but defined: confirmed at birth.
    #[test]
    fn zero_confirm_threshold_confirms_at_birth() {
        let mut m = manager(0, 3);
        let id = new_track(&mut m);
        assert_eq!(status(&m, id), Some(TrackStatus::Confirmed));
    }

    /// The associator is reachable for the pipeline that will drive both.
    #[test]
    fn the_associator_is_reachable() {
        let mut m = manager(1, 1);
        let cost = nalgebra::DMatrix::from_row_slice(2, 2, &[0.0, 1.0, 1.0, 0.0]);
        let result: Result<Vec<Option<usize>>, AssociationError> = m.associator().associate(&cost);
        assert_eq!(result.expect("solvable"), vec![Some(0), Some(1)]);
    }
}
