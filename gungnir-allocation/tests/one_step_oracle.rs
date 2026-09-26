// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The one-step answer against two oracles (GAP-156, D-93; DN-04 §11).
//!
//! **The exact solve at a horizon of one.** A one-step optimum is exactly what
//! `bellman::solve_exact` answers at a horizon of one, with the same tie rule, so inside
//! the exact solver's limits the two must name the same assignment. Rewards are drawn from
//! integers -- negative ones included, so declining is exercised -- because the one
//! difference the module documentation states is between totals that are equal and are
//! rounded apart; on integers they are computed equal, and ties are everywhere.
//!
//! **An independent dynamic program over resource subsets** for pictures past the exact
//! solver's limits, where there is no exact solve to compare with: tracks one at a time,
//! each given to an unused resource or to nobody. It knows nothing of the Hungarian
//! method and gives the best total by a different route, so the two agreeing on the value
//! is a check of the method rather than of itself.
//!
//! **The bounds against the exact optimum.** `stand_in` claims a floor the plan can reach
//! and a ceiling the optimum cannot pass; the exact solve's value must lie between them.

use gungnir_allocation::bellman::solve_exact;
use gungnir_allocation::{solve_one_step, stand_in};
use nalgebra::DMatrix;

/// The best one-step total by dynamic programming over which resources are used.
fn best_total_by_resource_subsets(reward: &DMatrix<f64>) -> f64 {
    let resources = reward.nrows();
    let states = 1_usize << resources;
    let mut best = vec![f64::NEG_INFINITY; states];
    best[0] = 0.0;
    for track in 0..reward.ncols() {
        let mut next = best.clone();
        for (used, &so_far) in best.iter().enumerate() {
            if so_far == f64::NEG_INFINITY {
                continue;
            }
            for r in 0..resources {
                if used & (1 << r) == 0 {
                    let with = used | (1 << r);
                    next[with] = next[with].max(so_far + reward[(r, track)]);
                }
            }
        }
        best = next;
    }
    best.into_iter().fold(f64::NEG_INFINITY, f64::max)
}

proptest::proptest! {
    /// Inside the exact limits: the same assignment as the exact solve at horizon one.
    #[test]
    fn the_one_step_answer_is_the_exact_answer_at_a_horizon_of_one(
        resources in 1_usize..=4,
        tracks in 1_usize..=6,
        cells in proptest::collection::vec(-3_i32..=5, 24),
    ) {
        let reward = DMatrix::from_fn(resources, tracks, |r, t| f64::from(cells[r * 6 + t]));
        let exact = solve_exact(&reward, 1).expect("inside the limits");
        let one = solve_one_step(&reward).expect("solvable");
        proptest::prop_assert_eq!(&one.assignment, &exact.assignment, "{}", reward);
        proptest::prop_assert!((one.value - exact.value).abs() < 1e-9);
    }

    /// Fractional rewards, where totals round: the value still matches the exact solve's.
    #[test]
    fn the_one_step_value_is_the_exact_value_on_fractional_rewards(
        resources in 1_usize..=4,
        tracks in 1_usize..=6,
        cells in proptest::collection::vec(-2.0_f64..7.0, 24),
    ) {
        let reward = DMatrix::from_fn(resources, tracks, |r, t| cells[r * 6 + t]);
        let exact = solve_exact(&reward, 1).expect("inside the limits");
        let one = solve_one_step(&reward).expect("solvable");
        proptest::prop_assert!((one.value - exact.value).abs() < 1e-9, "{} vs {}", one.value, exact.value);
    }

    /// Past the exact limits: the best total, by a route that shares nothing with it.
    #[test]
    fn past_the_exact_limits_the_value_is_the_best_total(
        resources in 1_usize..=10,
        tracks in 17_usize..=30,
        cells in proptest::collection::vec(-3.0_f64..9.0, 300),
    ) {
        let reward = DMatrix::from_fn(resources, tracks, |r, t| cells[r * 30 + t]);
        let one = solve_one_step(&reward).expect("any size is answered");
        let oracle = best_total_by_resource_subsets(&reward);
        proptest::prop_assert!((one.value - oracle).abs() < 1e-9, "{} vs {}", one.value, oracle);
    }

    /// The floor is reachable and the ceiling is not passed: the exact optimum over the
    /// horizon lies between them.
    #[test]
    fn the_exact_optimum_lies_between_the_stand_ins_bounds(
        resources in 1_usize..=3,
        tracks in 1_usize..=5,
        horizon in 1_usize..=4,
        cells in proptest::collection::vec(-2_i32..=6, 15),
    ) {
        let reward = DMatrix::from_fn(resources, tracks, |r, t| f64::from(cells[r * 5 + t]));
        let exact = solve_exact(&reward, horizon).expect("inside the limits");
        let s = stand_in(&reward, horizon).expect("solvable");
        proptest::prop_assert!(s.value_at_least <= exact.value + 1e-9, "{s:?} {exact:?}");
        proptest::prop_assert!(exact.value <= s.optimum_at_most + 1e-9, "{s:?} {exact:?}");
        proptest::prop_assert_eq!(&s.first_step, &solve_one_step(&reward).expect("solvable"));
    }
}

/// **The planner's own case.** The desktop and the node plan on a uniform matrix, where
/// every assignment ties; there the one-step answer and the exact answer at the shipped
/// horizon of ten must be the same pairing, or the exact answer arriving would replace a
/// stand-in with a plan naming different effectors for no reason an operator could see.
#[test]
fn on_a_uniform_matrix_the_stand_in_is_the_exact_first_step() {
    for (resources, tracks) in [(1, 1), (1, 4), (3, 3), (3, 5), (4, 2), (5, 8)] {
        let reward = DMatrix::from_element(resources, tracks, 1.0);
        let exact = solve_exact(&reward, 10).expect("inside the limits");
        let s = stand_in(&reward, 10).expect("solvable");
        assert_eq!(
            s.first_step.assignment, exact.assignment,
            "{resources} resources, {tracks} tracks"
        );
        assert!(
            (s.value_at_least - exact.value).abs() < 1e-9,
            "{resources} x {tracks}: {s:?} against {exact:?}"
        );
        assert!((s.share_of_optimum() - 1.0).abs() < 1e-12);
    }
}

/// At the exact solver's own limits the answer is immediate, and past them it is still
/// an answer.
#[test]
fn the_exact_limits_are_no_limit_here() {
    let at_limits = DMatrix::from_fn(8, 16, |r, t| {
        f64::from(u32::try_from((r * 5 + t * 3) % 11).unwrap_or(0))
    });
    let s = stand_in(&at_limits, 10).expect("solvable");
    assert_eq!(s.first_step.assignment.len(), 8);
    assert!(s.value_at_least <= s.optimum_at_most);
}
