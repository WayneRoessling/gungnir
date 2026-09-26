// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The one-step answer: the best assignment for this step alone, in polynomial time
//! (GAP-156, D-93; `docs/design/DN-04-effector-model.md` §11).
//!
//! # What this is, and what it is not
//!
//! **Not the optimum over the horizon, and never presented as one.** [`crate::bellman`]
//! solves the multi-step problem exactly and refuses anything else, because a caller that
//! got a greedy answer from a function documented as optimal would have no way to know.
//! That refusal stands: this module is a different function with a different name, and
//! what it returns says what it is. A planner that stands this in for an exact solve it
//! cannot finish labels the plan as such (`gungnir_model::PlanBasis::OneStep`).
//!
//! **It is an exact optimum of a stated problem.** The first step returned is the best
//! matching of resources to tracks for one step, which is exactly what
//! [`crate::bellman::solve_exact`] answers at a horizon of one -- the same reward, the
//! same one-to-one constraint, the same freedom to leave a resource idle, and the same tie
//! rule. `tests/one_step_oracle.rs` holds the two to the same assignment. What it gives up
//! is the look-ahead: a pairing that forecloses a better one next step is not weighed.
//!
//! **And it says how far from the horizon's optimum it can be.** [`stand_in`] follows the
//! first step with one-step answers over the rest of the horizon -- a policy the plan can
//! actually be continued with, so its value is achievable -- and bounds the exact optimum
//! from above by two facts of the model: a track is serviced at most once and is worth at
//! most its best reward, and no step is worth more than the best one-step matching of the
//! whole picture. The exact optimum lies between the two, so "worth at least this share of
//! the best plan" is a statement the numbers support rather than an estimate.
//!
//! # Why polynomial
//!
//! The exact solve enumerates matchings over subsets of tracks, which is what makes it
//! exponential; at its limits it runs for longer than an engagement lasts, and past them
//! it refuses. A one-step optimum is an assignment problem, solved here by the Hungarian
//! method (Kuhn-Munkres with potentials) in `O(R^2 (T + R))` for `R` resources and `T`
//! tracks, with no size limit: sixteen tracks and eight effectors take microseconds.
//!
//! # The tie rule, reproduced
//!
//! [`crate::bellman`]'s rule, at one step: the higher total; of equal totals, more pairs;
//! of both equal, the first matching in its enumeration order -- resources in row order,
//! each trying idle first and then tracks in ascending column order. Reproducing it is not
//! decoration. The planner stands this answer in only until the exact solve finishes, and
//! the planner the desktop and the node run today plans on a uniform reward matrix, where
//! every assignment ties; an answer that broke the tie differently would replace one plan
//! with another naming different effectors the moment the exact answer arrived, for no
//! reason an operator could see.
//!
//! Rules one and two are carried inside the assignment problem: a cost is the pair (minus
//! the total, minus the pair count), compared lexicographically, so the minimum is the
//! highest total and, of those, the most pairs. Rule three is applied resource by resource:
//! each takes the first choice, in the enumeration's order, that some optimal assignment
//! consistent with the choices already made still takes. The potentials the Hungarian
//! method leaves are a certificate for the whole problem -- no optimal assignment uses a
//! choice whose reduced cost is positive -- so most choices are ruled out without a
//! second solve, and a choice is confirmed by re-solving the rest only where the
//! certificate cannot decide.
//!
//! **One difference, stated.** [`crate::bellman`] treats totals within
//! [`crate::bellman::TIE_TOLERANCE`] as equal; here rules one and two compare the totals
//! as they are. The two agree wherever totals that are equal are computed equal, which
//! includes every integer-valued matrix and the uniform one; where two different sums of
//! fractions round to within a hair of each other, the exact solve may call a tie that
//! this does not. The value is the optimum either way.

use crate::bellman::TIE_TOLERANCE;
use crate::{AllocationError, AllocationPolicy};
use nalgebra::DMatrix;

/// A cost to minimise: minus the reward taken and minus the pairs made, compared in that
/// order. The pair count is exact; only the reward is floating point.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cost {
    reward: f64,
    pairs: i64,
}

impl Cost {
    const ZERO: Self = Self {
        reward: 0.0,
        pairs: 0,
    };
    /// Only ever a starting point for a minimum; never added to anything but a finite
    /// cost, where it stays infinite.
    const UNREACHED: Self = Self {
        reward: f64::INFINITY,
        pairs: 0,
    };

    fn add(self, other: Self) -> Self {
        Self {
            reward: self.reward + other.reward,
            pairs: self.pairs + other.pairs,
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            reward: self.reward - other.reward,
            pairs: self.pairs - other.pairs,
        }
    }

    /// Lexicographic: the reward first, then the pairs. Exact comparison on purpose (see
    /// the module documentation): the Hungarian method's potentials need a total order,
    /// and a tolerance is not one.
    #[allow(clippy::float_cmp)]
    fn less(self, other: Self) -> bool {
        self.reward < other.reward || (self.reward == other.reward && self.pairs < other.pairs)
    }

    /// Whether a reduced cost this large rules a choice out of every optimal assignment.
    ///
    /// **Generous to the choice.** Rounding can leave a reduced cost that is zero in exact
    /// arithmetic a hair above or below it, so only a reward part clearly positive, or a
    /// reward part that is zero to within the tie tolerance with a positive pair part,
    /// rules the choice out. Anything else is re-solved, which costs time and never an
    /// answer.
    fn rules_out(self) -> bool {
        self.reward > TIE_TOLERANCE || (self.reward.abs() <= TIE_TOLERANCE && self.pairs > 0)
    }

    /// Whether two costs are the same optimum: the same pair count, and rewards within the
    /// tie tolerance, since the two were summed in different orders.
    fn same_optimum(self, other: Self) -> bool {
        self.pairs == other.pairs && (self.reward - other.reward).abs() <= TIE_TOLERANCE
    }
}

/// What one row is given in an assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Idle,
    Track(usize),
}

/// An optimal assignment of `rows` to `tracks` (or to idle), and the potentials that
/// certify it.
struct Solved {
    /// Per row, in the order given.
    choices: Vec<Choice>,
    cost: Cost,
    /// Row potentials, 1-indexed as the method keeps them (`u[0]` is unused).
    u: Vec<Cost>,
    /// Column potentials, 1-indexed: the tracks in the order given, then one idle column
    /// per row.
    v: Vec<Cost>,
}

/// The cost of giving `row` the column `column` (0-indexed) of a problem over `tracks`.
fn cost_of(reward: &DMatrix<f64>, row: usize, tracks: &[usize], column: usize) -> Cost {
    match tracks.get(column) {
        Some(&track) => Cost {
            reward: -reward[(row, track)],
            pairs: -1,
        },
        None => Cost::ZERO,
    }
}

/// The Hungarian method over `rows` x (`tracks` then one idle column per row), every row
/// assigned exactly once.
///
/// The textbook shortest-augmenting-path form with potentials, over [`Cost`] rather than a
/// number. Idle columns cost nothing whoever takes them, so a resource never takes a track
/// it is worse than useless against, and there are always enough of them for every row.
/// Deterministic: every minimum is taken strictly, so of equal candidates the lowest
/// column wins.
fn hungarian(reward: &DMatrix<f64>, rows: &[usize], tracks: &[usize]) -> Solved {
    let n = rows.len();
    let m = tracks.len() + n;
    let mut u = vec![Cost::ZERO; n + 1];
    let mut v = vec![Cost::ZERO; m + 1];
    // p[j]: the row (1-indexed) holding column j; 0 for none.
    let mut p = vec![0_usize; m + 1];
    let mut way = vec![0_usize; m + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0_usize;
        let mut minv = vec![Cost::UNREACHED; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = Cost::UNREACHED;
            let mut j1 = 0_usize;
            for j in 1..=m {
                if used[j] {
                    continue;
                }
                let cur = cost_of(reward, rows[i0 - 1], tracks, j - 1)
                    .sub(u[i0])
                    .sub(v[j]);
                if cur.less(minv[j]) {
                    minv[j] = cur;
                    way[j] = j0;
                }
                if minv[j].less(delta) {
                    delta = minv[j];
                    j1 = j;
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] = u[p[j]].add(delta);
                    v[j] = v[j].sub(delta);
                } else {
                    minv[j] = minv[j].sub(delta);
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut choices = vec![Choice::Idle; n];
    let mut cost = Cost::ZERO;
    for (j, &i) in p.iter().enumerate().skip(1) {
        if i == 0 {
            continue;
        }
        let column = j - 1;
        choices[i - 1] = match tracks.get(column) {
            Some(&track) => Choice::Track(track),
            None => Choice::Idle,
        };
    }
    for (i, choice) in choices.iter().enumerate() {
        if let Choice::Track(track) = choice {
            cost = cost.add(Cost {
                reward: -reward[(rows[i], *track)],
                pairs: -1,
            });
        }
    }
    Solved {
        choices,
        cost,
        u,
        v,
    }
}

/// Check a matrix the way [`crate::bellman::solve_exact`] does, minus the size limits.
fn check(reward: &DMatrix<f64>) -> Result<(), AllocationError> {
    if reward.nrows() == 0 || reward.ncols() == 0 {
        return Err(AllocationError::DegenerateInput("empty reward matrix"));
    }
    if reward.iter().any(|v| !v.is_finite()) {
        return Err(AllocationError::DegenerateInput(
            "a reward is not finite, so no optimum is defined",
        ));
    }
    Ok(())
}

/// The best assignment for one step, under [`crate::bellman`]'s tie rule (GAP-156).
///
/// `assignment` holds `(row, column)` indices into `reward`, sorted, as
/// [`crate::bellman::solve_exact`]'s do; `value` is the step's total reward. For any
/// problem [`crate::bellman::solve_exact`] takes, this is its answer at a horizon of one
/// (up to the rounding the module documentation states); unlike it, this takes a problem
/// of any size.
///
/// # Errors
///
/// [`AllocationError::DegenerateInput`] for an empty matrix or a non-finite reward.
pub fn solve_one_step(reward: &DMatrix<f64>) -> Result<AllocationPolicy, AllocationError> {
    check(reward)?;
    let resources = reward.nrows();
    let tracks: Vec<usize> = (0..reward.ncols()).collect();
    let rows: Vec<usize> = (0..resources).collect();
    let whole = hungarian(reward, &rows, &tracks);
    let target = whole.cost;

    let mut taken = vec![false; reward.ncols()];
    let mut spent = Cost::ZERO;
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for row in 0..resources {
        let u = whole.u[row + 1];
        let reduced = |column: usize| {
            cost_of(reward, row, &tracks, column)
                .sub(u)
                .sub(whole.v[column + 1])
        };
        // The choices the certificate leaves open, in the enumeration's order: idle
        // first, then the tracks still free in ascending order.
        let idle_open =
            (tracks.len()..tracks.len() + resources).any(|column| !reduced(column).rules_out());
        let mut open: Vec<Choice> = Vec::new();
        if idle_open {
            open.push(Choice::Idle);
        }
        open.extend(
            tracks
                .iter()
                .filter(|&&t| !taken[t] && !reduced(t).rules_out())
                .map(|&t| Choice::Track(t)),
        );

        let rest_rows: Vec<usize> = (row + 1..resources).collect();
        let completes = |choice: Choice| {
            let free: Vec<usize> = tracks
                .iter()
                .copied()
                .filter(|&t| !taken[t] && choice != Choice::Track(t))
                .collect();
            let rest = hungarian(reward, &rest_rows, &free);
            let here = match choice {
                Choice::Idle => Cost::ZERO,
                Choice::Track(t) => Cost {
                    reward: -reward[(row, t)],
                    pairs: -1,
                },
            };
            spent.add(here).add(rest.cost).same_optimum(target)
        };
        // Some optimal assignment is consistent with the choices made so far, and every
        // optimal assignment takes an open choice here, so if every open choice but the
        // last fails, the last one is the answer without asking.
        let chosen = if let Some((last, earlier)) = open.split_last() {
            earlier
                .iter()
                .copied()
                .find(|&c| completes(c))
                .unwrap_or(*last)
        } else {
            // Only rounding could close every choice (the certificate is generous to
            // them); then the rest is solved outright and this row takes what that
            // solution gives it, which is an optimal completion whatever the tie.
            let free: Vec<usize> = tracks.iter().copied().filter(|&t| !taken[t]).collect();
            let here_on: Vec<usize> = (row..resources).collect();
            hungarian(reward, &here_on, &free)
                .choices
                .first()
                .copied()
                .unwrap_or(Choice::Idle)
        };
        if let Choice::Track(t) = chosen {
            taken[t] = true;
            spent = spent.add(Cost {
                reward: -reward[(row, t)],
                pairs: -1,
            });
            pairs.push((row, t));
        }
    }
    let value = pairs.iter().map(|&(r, t)| reward[(r, t)]).sum();
    Ok(AllocationPolicy {
        assignment: pairs,
        value,
    })
}

/// A one-step answer standing in for the exact solve over `horizon` steps, with how good
/// it is known to be (GAP-156, D-93).
#[derive(Debug, Clone, PartialEq)]
pub struct StandIn {
    /// The first step: [`solve_one_step`]'s answer, whose `value` is that step's reward.
    pub first_step: AllocationPolicy,
    /// What the first step is worth over the horizon when each later step is also the
    /// best one-step matching of what is left: a value this plan can actually reach, and
    /// so a floor under what it is worth.
    pub value_at_least: f64,
    /// A ceiling on the exact optimum over the horizon: the smaller of every track's best
    /// reward summed (a track is serviced once) and the horizon times the best one-step
    /// value (no step is worth more). Never below `value_at_least`.
    pub optimum_at_most: f64,
}

impl StandIn {
    /// The share of the best plan's value this answer is known to reach: see
    /// [`share_of_optimum`].
    #[must_use]
    pub fn share_of_optimum(&self) -> f64 {
        share_of_optimum(self.value_at_least, self.optimum_at_most)
    }
}

/// The share of the best plan's value a plan worth at least `value_at_least` is known to
/// reach when the optimum is at most `optimum_at_most`: their ratio, clamped to 0 to 1,
/// and 1 where the optimum is nothing -- a plan that does nothing is then as good as any.
///
/// Free of [`StandIn`] so a caller that carries only the two bounds -- a planner's outcome,
/// a node's word on the wire -- computes it by the same rule.
#[must_use]
pub fn share_of_optimum(value_at_least: f64, optimum_at_most: f64) -> f64 {
    if optimum_at_most <= TIE_TOLERANCE {
        1.0
    } else {
        (value_at_least / optimum_at_most).clamp(0.0, 1.0)
    }
}

/// [`solve_one_step`], and the bounds that say how far from the horizon's optimum it can
/// be (GAP-156, D-93).
///
/// # Errors
///
/// As [`solve_one_step`], and [`AllocationError::DegenerateInput`] for a zero horizon.
pub fn stand_in(reward: &DMatrix<f64>, horizon: usize) -> Result<StandIn, AllocationError> {
    if horizon == 0 {
        return Err(AllocationError::DegenerateInput("zero horizon"));
    }
    let first_step = solve_one_step(reward)?;
    let rows: Vec<usize> = (0..reward.nrows()).collect();
    let mut left: Vec<usize> = (0..reward.ncols())
        .filter(|t| !first_step.assignment.iter().any(|&(_, taken)| taken == *t))
        .collect();
    // The continuation: each later step the best matching of what is left. Any
    // continuation gives a floor; this one is the natural one, and it needs no tie rule
    // because only its value is kept.
    let mut value_at_least = first_step.value;
    for _ in 1..horizon {
        if left.is_empty() {
            break;
        }
        let step = hungarian(reward, &rows, &left);
        let mut matched = false;
        for (row, choice) in step.choices.iter().enumerate() {
            if let Choice::Track(t) = choice {
                value_at_least += reward[(row, *t)];
                left.retain(|x| x != t);
                matched = true;
            }
        }
        if !matched {
            break;
        }
    }
    let per_track: f64 = (0..reward.ncols())
        .map(|t| reward.column(t).iter().copied().fold(0.0_f64, f64::max))
        .sum();
    #[allow(clippy::cast_precision_loss)] // a horizon is a handful of steps
    let per_step = first_step.value * horizon as f64;
    let optimum_at_most = per_track.min(per_step).max(value_at_least);
    Ok(StandIn {
        first_step,
        value_at_least,
        optimum_at_most,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_resource_takes_its_best_track() {
        let reward = DMatrix::from_row_slice(1, 3, &[2.0, 9.0, 4.0]);
        let policy = solve_one_step(&reward).expect("solvable");
        assert_eq!(policy.assignment, vec![(0, 1)]);
        assert!((policy.value - 9.0).abs() < 1e-12);
    }

    /// A pairing worth less than nothing is declined, as the exact solve declines it.
    #[test]
    fn a_negative_pairing_is_declined() {
        let reward = DMatrix::from_row_slice(2, 1, &[-1.0, -0.5]);
        let policy = solve_one_step(&reward).expect("solvable");
        assert!(policy.assignment.is_empty(), "{policy:?}");
        assert!(policy.value.abs() < 1e-12);
    }

    /// Rule two: a zero-reward pairing is taken, because of equal totals the one that
    /// acts is reported.
    #[test]
    fn of_equal_totals_the_matching_that_acts_is_reported() {
        let reward = DMatrix::from_row_slice(1, 1, &[0.0]);
        let policy = solve_one_step(&reward).expect("solvable");
        assert_eq!(policy.assignment, vec![(0, 0)]);
    }

    /// Rule three on a full tie: three and three is the diagonal, and where a resource
    /// must idle it is the earlier one -- `bellman`'s "idle first" read backwards.
    #[test]
    fn a_full_tie_falls_to_the_documented_order() {
        let square = DMatrix::from_element(3, 3, 1.0);
        assert_eq!(
            solve_one_step(&square).expect("solvable").assignment,
            vec![(0, 0), (1, 1), (2, 2)]
        );
        let more_resources = DMatrix::from_element(3, 2, 1.0);
        assert_eq!(
            solve_one_step(&more_resources)
                .expect("solvable")
                .assignment,
            vec![(1, 0), (2, 1)]
        );
    }

    /// No size limit: a picture past the exact solver's is answered, and its assignment
    /// is one-to-one.
    #[test]
    fn a_picture_past_the_exact_limits_is_answered() {
        let reward = DMatrix::from_fn(12, 40, |r, t| {
            f64::from(u32::try_from((r * 7 + t * 13) % 17).unwrap_or(0)) - 3.0
        });
        let policy = solve_one_step(&reward).expect("solvable");
        let mut rows: Vec<usize> = policy.assignment.iter().map(|p| p.0).collect();
        let mut cols: Vec<usize> = policy.assignment.iter().map(|p| p.1).collect();
        rows.dedup();
        cols.sort_unstable();
        cols.dedup();
        assert_eq!(rows.len(), policy.assignment.len());
        assert_eq!(cols.len(), policy.assignment.len());
    }

    #[test]
    fn degenerate_and_non_finite_inputs_are_refused() {
        assert!(matches!(
            solve_one_step(&DMatrix::<f64>::zeros(0, 3)),
            Err(AllocationError::DegenerateInput(_))
        ));
        assert!(matches!(
            solve_one_step(&DMatrix::from_element(1, 1, f64::NAN)),
            Err(AllocationError::DegenerateInput(_))
        ));
        assert!(matches!(
            stand_in(&DMatrix::from_element(1, 1, 1.0), 0),
            Err(AllocationError::DegenerateInput(_))
        ));
    }

    /// The look-ahead is what a one-step answer gives up, and the bounds say how much it
    /// can have cost. Two resources, three tracks, two steps: track 0 is worth 10 to
    /// either resource, tracks 1 and 2 are worth 9 to resource 0 and 1 to resource 1.
    #[test]
    fn the_bounds_bracket_the_optimum() {
        let reward = DMatrix::from_row_slice(2, 3, &[10.0, 9.0, 9.0, 10.0, 1.0, 1.0]);
        let s = stand_in(&reward, 2).expect("solvable");
        let exact = crate::bellman::solve_exact(&reward, 2).expect("solvable");
        assert!(s.value_at_least <= exact.value + 1e-9, "{s:?} {exact:?}");
        assert!(exact.value <= s.optimum_at_most + 1e-9, "{s:?} {exact:?}");
        assert!(s.share_of_optimum() > 0.0 && s.share_of_optimum() <= 1.0);
    }
}
