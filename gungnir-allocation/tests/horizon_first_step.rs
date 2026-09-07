//! The first step of a multi-step policy must task something.
//!
//! This file exists because it did not, and nothing caught it. `solve_exact` returned an
//! **empty first step for every input at every horizon above one**, while reporting the
//! full optimal value, so a deployment running the shipped default of ten saw a plan that
//! was permanently empty and a policy value that read as a solved problem.
//!
//! The cause was a tie-break. The model has no time preference -- a track engaged now is
//! worth exactly what it is worth next step -- so acting and deferring tie at every state,
//! and the tie was broken by enumeration order, which kept the matching with the fewest
//! pairs. The empty one.
//!
//! These tests pin both halves of the fix: the reported **value** is unchanged, because
//! the tie-break chooses between policies that were always equally optimal, and the
//! reported **first step** now acts.

use gungnir_allocation::bellman::solve_exact;
use nalgebra::DMatrix;

/// A matrix whose optimum is the diagonal, so the right answer is obvious by eye.
fn diagonal_preference() -> DMatrix<f64> {
    DMatrix::from_row_slice(3, 3, &[9.0, 1.0, 1.0, 1.0, 8.0, 1.0, 1.0, 1.0, 7.0])
}

#[test]
fn the_first_step_tasks_something_at_every_horizon() {
    let rewards = diagonal_preference();
    for horizon in [1_usize, 2, 3, 5, 10, 16] {
        let policy = solve_exact(&rewards, horizon).expect("solvable");
        assert!(
            !policy.assignment.is_empty(),
            "horizon {horizon} recommended doing nothing, which is the defect this file \
             exists for"
        );
        assert_eq!(
            policy.assignment,
            vec![(0, 0), (1, 1), (2, 2)],
            "horizon {horizon} did not take the diagonal"
        );
    }
}

/// The value is a property of the problem, not of the tie-break. If this ever moves, the
/// tie-break has started changing the optimum rather than choosing between optima, and
/// the oracle comparison in `bellman_diff.rs` would be measuring a different function.
#[test]
fn the_horizon_does_not_change_the_optimal_value() {
    let rewards = diagonal_preference();
    let first = solve_exact(&rewards, 1).expect("solvable").value;
    for horizon in [2_usize, 3, 5, 10, 16] {
        let value = solve_exact(&rewards, horizon).expect("solvable").value;
        assert!(
            (value - first).abs() < 1e-9,
            "horizon {horizon} reported {value} against {first} at horizon 1, so the \
             tie-break is changing the optimum"
        );
    }
}

/// More resources than tracks: the extra resource has nothing to take, and the policy
/// must still task the ones that do rather than deferring the lot.
#[test]
fn a_surplus_of_resources_still_tasks_every_track() {
    let rewards = DMatrix::from_row_slice(4, 2, &[5.0, 1.0, 1.0, 6.0, 2.0, 2.0, 1.0, 1.0]);
    let policy = solve_exact(&rewards, 10).expect("solvable");
    let tracks: Vec<usize> = {
        let mut t: Vec<usize> = policy.assignment.iter().map(|(_, c)| *c).collect();
        t.sort_unstable();
        t
    };
    assert_eq!(tracks, vec![0, 1], "a track was left unengaged: {tracks:?}");
}

/// Fewer resources than tracks: every resource is used, and the policy does not defer
/// the whole engagement because it cannot cover everything.
#[test]
fn a_shortage_of_resources_still_uses_all_of_them() {
    let rewards = DMatrix::from_row_slice(2, 4, &[9.0, 1.0, 1.0, 1.0, 1.0, 8.0, 1.0, 1.0]);
    let policy = solve_exact(&rewards, 10).expect("solvable");
    assert_eq!(
        policy.assignment.len(),
        2,
        "with two resources and four tracks the first step should use both: {:?}",
        policy.assignment
    );
}
