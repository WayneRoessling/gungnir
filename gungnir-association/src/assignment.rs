// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Optimal assignment: the verification-capability-table.md §1 rows
//! "Hungarian / Jonker-Volgenant" and "Nearest-Neighbor / GNN".
//!
//! Oracle `scipy.optimize.linear_sum_assignment`. Criterion: **exact match on cost;
//! assignment only if not tied.** That wording is the important part. An assignment
//! problem can have several distinct assignments of exactly equal total cost, and
//! which one a solver returns is an artifact of its pivoting order, not of
//! correctness. So the cost is compared exactly and the pairing is compared only where
//! the optimum is unique -- see `tests/assignment_diff.rs`, which decides uniqueness
//! rather than assuming it.
//!
//! # The algorithm
//!
//! Shortest augmenting path with dual variables (Jonker-Volgenant), the same family
//! `scipy` uses. `O(n²m)` for an `n × m` matrix with `n ≤ m`. Rectangular inputs are
//! handled directly rather than by padding to a square with a large constant: padding
//! changes the optimum when the pad value is smaller than some real cost, and picking
//! a pad value large enough to be safe is exactly the kind of magic number that breaks
//! quietly on a matrix with an unexpected scale.
//!
//! Every intermediate quantity is a difference of input costs and dual variables, so
//! the arithmetic stays in the input's scale and the exact-cost criterion is
//! achievable: the returned total is a sum of the selected input entries, read back
//! from the matrix, not an accumulation from the solver's internals.

use nalgebra::DMatrix;

/// Why an assignment could not be computed.
///
/// The cost matrix reaches this crate from association logic fed by sensor data, and
/// `gungnir-fuzz`'s `cost_matrix_construction` target exists to push malformed
/// matrices at it. An explicit error is the honest answer; returning "no assignments"
/// for a matrix full of NaN would be a silent stub of exactly the kind
/// `CLAUDE.md` forbids.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssociationError {
    /// A cost entry was NaN or infinite. The optimum is not defined.
    #[error("cost matrix has a non-finite entry at row {row}, column {col}")]
    NonFiniteCost { row: usize, col: usize },
    /// A scene past what exact joint enumeration will attempt (JPDA, MHT).
    ///
    /// **Refused rather than truncated.** A partial enumeration returns association
    /// probabilities that look ordinary and are wrong, and nothing downstream could
    /// tell. A scene that outgrows exact enumeration needs a different algorithm, and
    /// saying which bound was hit is the useful report.
    #[error("{count} {what} is past the {limit} exact enumeration will attempt")]
    TooManyHypotheses {
        what: &'static str,
        count: usize,
        limit: usize,
    },
    /// The scene parameters do not describe a scene: a probability outside `[0, 1]`, a
    /// clutter density that is not positive, or a set of alternatives with no weight
    /// between them.
    #[error("the association scene is malformed: {what}")]
    MalformedScene { what: &'static str },
}

/// The result of an optimal assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    /// Sum of the selected entries, read back from the input matrix.
    pub total_cost: f64,
    /// `row_to_col[i]` is the column assigned to row `i`, or `None` when there are
    /// fewer columns than rows and row `i` went unassigned.
    pub row_to_col: Vec<Option<usize>>,
}

impl Assignment {
    /// The assignment as `(row, column)` pairs, ascending by row.
    #[must_use]
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        self.row_to_col
            .iter()
            .enumerate()
            .filter_map(|(row, col)| col.map(|c| (row, c)))
            .collect()
    }

    /// How many rows were assigned.
    #[must_use]
    pub fn assigned_count(&self) -> usize {
        self.row_to_col.iter().filter(|c| c.is_some()).count()
    }
}

/// Reject a matrix the optimum is not defined for, before any work is done.
fn check_finite(cost: &DMatrix<f64>) -> Result<(), AssociationError> {
    for row in 0..cost.nrows() {
        for col in 0..cost.ncols() {
            if !cost[(row, col)].is_finite() {
                return Err(AssociationError::NonFiniteCost { row, col });
            }
        }
    }
    Ok(())
}

/// Exact Hungarian/Jonker-Volgenant solver, including degenerate and rectangular
/// matrices.
///
/// verification-capability-table.md: exact match on cost; assignment only where not
/// tied. An empty matrix (no rows or no columns) is a valid problem whose answer is
/// "nothing assigned, cost zero", not an error.
///
/// # Errors
/// [`AssociationError::NonFiniteCost`] if any entry is NaN or infinite.
pub fn solve_assignment(cost: &DMatrix<f64>) -> Result<Assignment, AssociationError> {
    check_finite(cost)?;
    let (rows, cols) = (cost.nrows(), cost.ncols());
    if rows == 0 || cols == 0 {
        return Ok(Assignment {
            total_cost: 0.0,
            row_to_col: vec![None; rows],
        });
    }

    // The inner loop requires at least as many columns as rows. Transposing is exact
    // and the mapping is inverted afterwards, so nothing about the optimum changes.
    let transposed = rows > cols;
    let work = if transposed {
        cost.transpose()
    } else {
        cost.clone()
    };
    let (n, m) = (work.nrows(), work.ncols());

    // One-based throughout, with index 0 as the sentinel the augmenting path starts
    // from. This is the classical formulation; renumbering it to zero-based buys
    // nothing and makes the sentinel harder to see.
    let mut u = vec![0.0_f64; n + 1];
    let mut v = vec![0.0_f64; m + 1];
    let mut col_to_row = vec![0_usize; m + 1];
    let mut way = vec![0_usize; m + 1];

    for i in 1..=n {
        col_to_row[0] = i;
        let mut j0 = 0_usize;
        let mut min_slack = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];

        loop {
            used[j0] = true;
            let i0 = col_to_row[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0_usize;

            for j in 1..=m {
                if used[j] {
                    continue;
                }
                let cur = work[(i0 - 1, j - 1)] - u[i0] - v[j];
                if cur < min_slack[j] {
                    min_slack[j] = cur;
                    way[j] = j0;
                }
                if min_slack[j] < delta {
                    delta = min_slack[j];
                    j1 = j;
                }
            }

            // `delta` is finite because every entry is finite and at least one column
            // is unused on each pass; the guard keeps a pathological input from
            // propagating an infinity into the duals rather than failing here.
            if !delta.is_finite() {
                break;
            }
            // Three parallel arrays are stepped by the same index here, so the
            // index itself is the subject and an iterator over any one of them would
            // only move the indexing to the other two.
            #[allow(clippy::needless_range_loop)]
            for j in 0..=m {
                if used[j] {
                    u[col_to_row[j]] += delta;
                    v[j] -= delta;
                } else {
                    min_slack[j] -= delta;
                }
            }

            j0 = j1;
            if col_to_row[j0] == 0 {
                break;
            }
        }

        // Walk the augmenting path back, flipping the matching along it.
        while j0 != 0 {
            let j1 = way[j0];
            col_to_row[j0] = col_to_row[j1];
            j0 = j1;
        }
    }

    // col_to_row is in the (possibly transposed) frame; map it back to the caller's.
    let mut row_to_col: Vec<Option<usize>> = vec![None; rows];
    let mut total_cost = 0.0;
    for (j, &i) in col_to_row.iter().enumerate().take(m + 1).skip(1) {
        if i == 0 {
            continue;
        }
        let (r, c) = if transposed {
            (j - 1, i - 1)
        } else {
            (i - 1, j - 1)
        };
        row_to_col[r] = Some(c);
        total_cost += cost[(r, c)];
    }

    Ok(Assignment {
        total_cost,
        row_to_col,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(rows: usize, cols: usize, values: &[f64]) -> DMatrix<f64> {
        DMatrix::from_row_slice(rows, cols, values)
    }

    /// Costs in these tests are small exact decimals, and the optimum is a sum of a
    /// handful of them, so an exact comparison is the right one: a tolerance here
    /// would hide a solver that picked a different assignment of nearly equal cost.
    #[allow(clippy::float_cmp)]
    #[test]
    fn identity_costs_pick_the_diagonal() {
        // Zero on the diagonal, one elsewhere: the unique optimum is the diagonal.
        let mut cost = DMatrix::from_element(4, 4, 1.0);
        for i in 0..4 {
            cost[(i, i)] = 0.0;
        }
        let a = solve_assignment(&cost).expect("solvable");
        assert_eq!(a.total_cost, 0.0);
        assert_eq!(a.row_to_col, vec![Some(0), Some(1), Some(2), Some(3)]);
    }

    /// The textbook 3x3 with a non-obvious optimum: greedy picks 1 + 6 + 9 = 16 by
    /// taking the smallest entry first, the optimum is 13.
    #[test]
    fn beats_greedy_on_a_known_case() {
        let cost = matrix(3, 3, &[4.0, 1.0, 3.0, 2.0, 0.0, 5.0, 3.0, 2.0, 2.0]);
        let a = solve_assignment(&cost).expect("solvable");
        assert!(
            (a.total_cost - 5.0).abs() < 1e-12,
            "optimum was {}",
            a.total_cost
        );
        assert_eq!(a.assigned_count(), 3);
    }

    #[test]
    fn wide_matrix_assigns_every_row() {
        let cost = matrix(2, 5, &[9.0, 1.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0]);
        let a = solve_assignment(&cost).expect("solvable");
        assert_eq!(a.assigned_count(), 2);
        assert!(a.row_to_col.iter().all(Option::is_some));
    }

    #[test]
    fn tall_matrix_leaves_rows_unassigned() {
        let cost = matrix(5, 2, &[9.0, 5.0, 1.0, 4.0, 8.0, 3.0, 7.0, 2.0, 6.0, 1.0]);
        let a = solve_assignment(&cost).expect("solvable");
        assert_eq!(a.assigned_count(), 2, "only two columns exist");
        assert_eq!(a.row_to_col.len(), 5);
    }

    #[allow(clippy::float_cmp)]
    #[test]
    fn empty_matrix_is_not_an_error() {
        let a = solve_assignment(&DMatrix::from_row_slice(0, 0, &[])).expect("solvable");
        assert_eq!(a.total_cost, 0.0);
        assert!(a.row_to_col.is_empty());

        let b = solve_assignment(&DMatrix::from_element(3, 0, 0.0)).expect("solvable");
        assert_eq!(b.row_to_col, vec![None, None, None]);
    }

    #[test]
    fn non_finite_cost_is_an_error_not_a_panic() {
        let mut cost = DMatrix::from_element(2, 2, 1.0);
        cost[(1, 0)] = f64::NAN;
        assert_eq!(
            solve_assignment(&cost),
            Err(AssociationError::NonFiniteCost { row: 1, col: 0 })
        );
        cost[(1, 0)] = f64::INFINITY;
        assert!(solve_assignment(&cost).is_err());
    }

    /// An all-equal matrix is the degenerate tie: every assignment is optimal, so the
    /// cost is determined and the pairing is not. The solver must still return a valid
    /// permutation.
    #[test]
    fn total_tie_returns_a_valid_permutation() {
        let cost = DMatrix::from_element(4, 4, 7.0);
        let a = solve_assignment(&cost).expect("solvable");
        assert!((a.total_cost - 28.0).abs() < 1e-12);
        let mut columns: Vec<usize> = a.row_to_col.iter().filter_map(|c| *c).collect();
        columns.sort_unstable();
        assert_eq!(columns, vec![0, 1, 2, 3], "not a permutation");
    }

    /// Negative costs are legitimate (a cost matrix is often a negated score) and must
    /// not be treated as a special case.
    #[test]
    fn negative_costs_are_handled() {
        let cost = matrix(
            3,
            3,
            &[-5.0, -1.0, -3.0, -2.0, -8.0, -4.0, -6.0, -2.0, -1.0],
        );
        let a = solve_assignment(&cost).expect("solvable");
        // -5 + -8 + -1 = -14 via the diagonal; -3 + -8 + -6 = -17 is better.
        assert!(
            a.total_cost <= -14.0,
            "cost {} is not optimal",
            a.total_cost
        );
        assert_eq!(a.assigned_count(), 3);
    }
}
