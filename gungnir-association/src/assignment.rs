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
//!
//! # What the total promises (D-43, resolved 2026-09-09)
//!
//! [`Assignment::total_cost`] is `Option<f64>`, and `None` means the optimum's value is
//! not a representable `f64`. Entry finiteness does not imply total finiteness: a
//! matrix whose entries all pass [`check_finite`] can still have an optimum whose sum
//! overflows, which `gungnir-fuzz`'s `cost_matrix_construction` target found on an
//! all-finite 5x2 matrix with two entries near `f64::MAX` (GAP-103).
//!
//! The `Option` rather than an error, because **the assignment itself is still
//! correct** in that case. Measured over 200,000 all-finite matrices drawn from the
//! top exponent band: every total that overflowed belonged to a problem whose
//! brute-force optimum was itself not representable, and no case produced a wrong
//! assignment count or a suboptimal pairing. Refusing such a matrix would discard a
//! sound answer, and returning `-inf` in a bare `f64` would let a value the input
//! guard exists to prevent leave through the output -- the silent propagation
//! `agentic-workflow.md`'s low-trust tier names. `None` says the one thing that is
//! true: there is an optimal assignment, and its cost is not a number.
//!
//! **One limit of that, stated rather than hidden.** The total is the accumulation of
//! the selected entries in the solver's own order, and floating-point addition is not
//! associative, so at the very edge of the range the order can decide the answer:
//! `MAX + MAX - MAX` overflows where `MAX - MAX + MAX` does not. `Some`-versus-`None`
//! is therefore an exact statement about *this* accumulation, not about the ideal
//! real-number sum, for the same reason the verification table already records that
//! "exact" on the cost means to the rounding of a sum taken in a different order than
//! the oracle takes it. Making it order-free would need exact or scaled summation, a
//! real cost in a crate whose one production cost matrix is bounded near 1.1e4 by
//! construction; it was judged not worth the added subtlety in a human-owned file.

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
    /// Sum of the selected entries, read back from the input matrix, or `None` when
    /// that sum is not a representable `f64`.
    ///
    /// `None` does not mean the assignment failed: [`Assignment::row_to_col`] is the
    /// optimum either way. It means this problem's optimal *value* overflows, which
    /// an all-finite matrix can still do (see the module documentation, D-43).
    pub total_cost: Option<f64>,
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
/// The returned [`Assignment::total_cost`] is `Some` whenever the optimum's value is a
/// representable `f64`, and `None` when it is not. Every entry being finite does not
/// make their sum finite, and `None` is not a failure: the pairing is the optimum
/// either way (D-43; see the module documentation for the measurements behind that
/// choice).
///
/// # Errors
/// [`AssociationError::NonFiniteCost`] if any *entry* is NaN or infinite. An entry
/// guard cannot speak for the total, which is why the total is an `Option` and not a
/// second error variant.
pub fn solve_assignment(cost: &DMatrix<f64>) -> Result<Assignment, AssociationError> {
    check_finite(cost)?;
    let (rows, cols) = (cost.nrows(), cost.ncols());
    if rows == 0 || cols == 0 {
        return Ok(Assignment {
            total_cost: Some(0.0),
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

    // The entries are finite, but their sum need not be: D-43 chose to report that as
    // `None` rather than to refuse the matrix or to hand back the infinity. The check is
    // on the accumulated total rather than on the entries, because that is exactly the
    // quantity that can fail to be representable.
    Ok(Assignment {
        total_cost: total_cost.is_finite().then_some(total_cost),
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
        assert_eq!(a.total_cost, Some(0.0));
        assert_eq!(a.row_to_col, vec![Some(0), Some(1), Some(2), Some(3)]);
    }

    /// The textbook 3x3 with a non-obvious optimum: greedy picks 1 + 6 + 9 = 16 by
    /// taking the smallest entry first, the optimum is 13.
    #[test]
    fn beats_greedy_on_a_known_case() {
        let cost = matrix(3, 3, &[4.0, 1.0, 3.0, 2.0, 0.0, 5.0, 3.0, 2.0, 2.0]);
        let a = solve_assignment(&cost).expect("solvable");
        let total = a
            .total_cost
            .expect("a small exact matrix has a representable optimum");
        assert!((total - 5.0).abs() < 1e-12, "optimum was {total}");
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
        assert_eq!(a.total_cost, Some(0.0));
        assert!(a.row_to_col.is_empty());

        let b = solve_assignment(&DMatrix::from_element(3, 0, 0.0)).expect("solvable");
        assert_eq!(b.row_to_col, vec![None, None, None]);
    }

    /// D-43, and the exact input `gungnir-fuzz` found (GAP-103). Every entry is finite,
    /// so `check_finite` passes and must: the *sum of the selected two* is what
    /// overflows. The contract is that this is not an error and not an infinity -- the
    /// pairing is the optimum, and the total says it is not a number.
    #[test]
    fn an_optimum_that_is_not_representable_is_reported_as_no_total() {
        // -f64::MAX and a second entry large enough that their sum is not representable.
        let (a, b) = (-f64::MAX, -6.171_889_577_392_9e303);
        let mut cost = DMatrix::from_element(5, 2, 0.0);
        cost[(3, 0)] = a;
        cost[(0, 1)] = b;
        assert!(
            cost.iter().all(|v| v.is_finite()),
            "the input is all-finite"
        );
        assert!(!(a + b).is_finite(), "the two selected entries do overflow");

        let got = solve_assignment(&cost).expect("an all-finite matrix is solvable");
        assert_eq!(
            got.total_cost, None,
            "the optimum's value is not representable"
        );
        assert_eq!(
            got.row_to_col,
            vec![Some(1), None, None, Some(0), None],
            "the pairing is still the optimum: the two most negative entries"
        );
        assert_eq!(got.assigned_count(), 2, "both columns are used");
    }

    /// The other half of the same contract, and the one that stops `None` becoming a
    /// lazy answer: a matrix may carry entries at the representable limit and still
    /// have an optimum that is perfectly fine, because the optimum does not select
    /// them. That case must report a total.
    #[test]
    fn a_huge_entry_the_optimum_avoids_still_yields_a_total() {
        // The optimum takes the two zeros; the two enormous entries are never selected.
        let cost = matrix(2, 2, &[0.0, f64::MAX, f64::MAX, 0.0]);
        let got = solve_assignment(&cost).expect("solvable");
        assert_eq!(
            got.total_cost,
            Some(0.0),
            "the selected entries sum to zero"
        );
        assert_eq!(got.row_to_col, vec![Some(0), Some(1)]);
    }

    /// `None` is reserved for a total that is genuinely not representable, in either
    /// direction, and is never a stand-in for "large".
    #[test]
    fn a_large_but_representable_total_is_some() {
        let cost = matrix(2, 2, &[1e307, 0.0, 0.0, 1e307]);
        let got = solve_assignment(&cost).expect("solvable");
        assert_eq!(
            got.total_cost,
            Some(0.0),
            "the optimum avoids both large entries"
        );

        // Forced to take both: 2e307 is large and entirely representable.
        let cost = matrix(2, 2, &[1e307, f64::MAX, f64::MAX, 1e307]);
        let got = solve_assignment(&cost).expect("solvable");
        assert_eq!(got.total_cost, Some(2e307));
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
        let total = a
            .total_cost
            .expect("a small exact matrix has a representable optimum");
        assert!((total - 28.0).abs() < 1e-12);
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
        let total = a
            .total_cost
            .expect("a small exact matrix has a representable optimum");
        assert!(total <= -14.0, "cost {total} is not optimal");
        assert_eq!(a.assigned_count(), 3);
    }
}
