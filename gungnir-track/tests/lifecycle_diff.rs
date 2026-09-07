// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the verification-capability-table.md §1 row
//! "`track-manager` | Track lifecycle (init/confirm/coast/delete)".
//!
//! Oracle: Stone Soup's initiators and deleters. Criterion: **exact match on step
//! index** for an identical detection/miss sequence. So this compares integers, not
//! floats: the cycle a track confirms on and the cycle it dies on, or `None` where the
//! sequence never gets there. There is no tolerance to state.
//!
//! # What the oracle side actually is
//!
//! Recorded plainly, because the two halves are not equally strong:
//!
//! * the **deletion** step index is `stonesoup.deleter.time.UpdateTimeStepsDeleter`
//!   driven for real against a genuine Stone Soup `Track`;
//! * the **confirmation** step index is the condition from
//!   `MultiMeasurementInitiator.initiate`, lifted from Stone Soup's source and
//!   evaluated against that same real `Track`, rather than the whole initiator run end
//!   to end -- which would need a predictor, updater, associator and measurement model
//!   standing between the fixture and the one rule under test.
//!
//! `testdata/oracles/tools/gen_lifecycle_fixtures.py` says the same thing, and its
//! header records the bug that made the distinction matter: an update built with
//! `hypothesis=None` is not counted as an update by the deleter at all, because it
//! tests `isinstance(state, Update) and state.hypothesis`. A first draft of the
//! fixture did that and reported a track deleted while it was being hit on every
//! step. `dense_hits_never_deleted` and `hit_resets_the_miss_run` are in the case list
//! because they are what caught it.
//!
//! MATLAB's `trackHistoryLogic` and `trackScoreLogic` are named by the row and were
//! **not** run: MATLAB is not installed; `testdata/oracles/README.md` records that.

use gungnir_association::GlobalNearestNeighbor;
use gungnir_track::{TrackManager, TrackStatus};
use nalgebra::{SMatrix, SVector};

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    sequence: String,
    min_points: u32,
    time_steps_since_update: u32,
    confirmed_at_step: Option<u64>,
    deleted_at_step: Option<u64>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    row: String,
    oracle: String,
    cases: Vec<Case>,
}

fn load() -> Fixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/oracles/track/lifecycle.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("oracle fixture present; see testdata/oracles/README.md");
    serde_json::from_str(&raw).expect("oracle fixture parses")
}

/// Replay one hit/miss sequence through the manager.
///
/// Returns `(confirmed_at, deleted_at)` as step indices. The track is initiated before
/// the first cycle, so cycle `k` of the manager is index `k` of the fixture's
/// sequence: the fixture's step 0 is the state the track is created from, and the
/// manager's step 0 is the cycle that consumes it.
fn replay(case: &Case) -> (Option<u64>, Option<u64>) {
    let mut manager = TrackManager::new(
        GlobalNearestNeighbor,
        case.min_points,
        case.time_steps_since_update,
    );
    let id = manager.initiate(SVector::<f64, 6>::zeros(), SMatrix::<f64, 6, 6>::identity());

    let mut confirmed_at = None;
    let mut deleted_at = None;

    for symbol in case.sequence.chars() {
        let outcome = match symbol {
            'H' => manager.step(&[id], &[]),
            'M' => manager.step(&[], &[id]),
            other => panic!("{}: sequence has an unexpected symbol {other:?}", case.name),
        };
        assert!(
            !outcome.has_disagreement(),
            "{} at step {}: the manager did not recognise its own track ({:?} unknown, \
             {:?} conflicting)",
            case.name,
            outcome.step,
            outcome.unknown,
            outcome.conflicting
        );

        if confirmed_at.is_none() && outcome.confirmed.contains(&id) {
            confirmed_at = Some(outcome.step);
        }
        if outcome.deleted.contains(&id) {
            deleted_at = Some(outcome.step);
            // The oracle records only the first deletion, and a deleted track is
            // pruned on the next cycle, so there is nothing further to compare.
            break;
        }
    }

    (confirmed_at, deleted_at)
}

#[test]
fn fixture_is_the_expected_row() {
    let f = load();
    assert_eq!(f.row, "Track lifecycle (init/confirm/coast/delete)");
    assert!(f.oracle.contains("Stone Soup"));
    assert!(!f.cases.is_empty());
    assert!(
        f.cases.iter().any(|c| c.confirmed_at_step.is_some()),
        "no case confirms, so the confirmation rule is never compared"
    );
    assert!(
        f.cases.iter().any(|c| c.deleted_at_step.is_some()),
        "no case deletes, so the deletion rule is never compared"
    );
    assert!(
        f.cases.iter().any(|c| c.deleted_at_step.is_none()),
        "every case deletes, so 'never deleted' is never compared -- that is the case \
         a broken deleter passes"
    );
}

/// The row itself: exact match on the confirm and delete step indices.
#[test]
fn lifecycle_transitions_match_stone_soup() {
    let fixture = load();
    let mut compared = 0;

    for case in &fixture.cases {
        let (confirmed, deleted) = replay(case);
        assert_eq!(
            confirmed, case.confirmed_at_step,
            "{} ({}): confirmed at step {:?}, oracle says {:?}",
            case.name, case.sequence, confirmed, case.confirmed_at_step
        );
        assert_eq!(
            deleted, case.deleted_at_step,
            "{} ({}): deleted at step {:?}, oracle says {:?}",
            case.name, case.sequence, deleted, case.deleted_at_step
        );
        compared += 2;
    }

    println!(
        "track lifecycle: {compared} step indices compared exactly across {} sequences; \
         all agree with Stone Soup",
        fixture.cases.len()
    );
}

/// The case that separates cumulative confirmation from consecutive confirmation,
/// called out on its own because it is the one that showed the scaffold's original
/// doc comment to be wrong. Under consecutive counting `HMHHH` with `min_points = 3`
/// confirms at step 4; the oracle says 3.
#[test]
fn cumulative_confirmation_is_what_the_oracle_does() {
    let fixture = load();
    let case = fixture
        .cases
        .iter()
        .find(|c| c.name == "confirm_counts_cumulative_across_a_miss")
        .expect("the distinguishing case is in the fixture");
    assert_eq!(case.sequence, "HMHHH");
    assert_eq!(case.min_points, 3);
    assert_eq!(
        case.confirmed_at_step,
        Some(3),
        "the oracle confirms on the third cumulative hit"
    );
    let (confirmed, _) = replay(case);
    assert_eq!(confirmed, Some(3));
}

/// Status after the replay must agree with the transitions, not merely the transition
/// steps: a manager that reported the right step indices while leaving the track in
/// the wrong state would pass the row and still be wrong.
#[test]
fn final_status_agrees_with_the_recorded_transitions() {
    let fixture = load();
    for case in &fixture.cases {
        let mut manager = TrackManager::new(
            GlobalNearestNeighbor,
            case.min_points,
            case.time_steps_since_update,
        );
        let id = manager.initiate(SVector::<f64, 6>::zeros(), SMatrix::<f64, 6, 6>::identity());
        let mut deleted = false;
        for symbol in case.sequence.chars() {
            let outcome = if symbol == 'H' {
                manager.step(&[id], &[])
            } else {
                manager.step(&[], &[id])
            };
            if outcome.deleted.contains(&id) {
                deleted = true;
                break;
            }
        }

        let status = manager.track(id).map(|t| t.status);
        if deleted {
            assert_eq!(
                status,
                Some(TrackStatus::Deleted),
                "{}: reported a deletion but the track is {status:?}",
                case.name
            );
        } else if case.confirmed_at_step.is_some() {
            let last = case.sequence.chars().last().expect("non-empty sequence");
            let expected = if last == 'H' {
                TrackStatus::Confirmed
            } else {
                TrackStatus::Coasting
            };
            assert_eq!(
                status,
                Some(expected),
                "{} ({}): a confirmed track ending on '{last}' should be {expected:?}",
                case.name,
                case.sequence
            );
        } else {
            assert_eq!(
                status,
                Some(TrackStatus::Tentative),
                "{}: never confirmed, so it should still be tentative",
                case.name
            );
        }
    }
}
