// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! gungnir-testkit: shared proptest strategies (agentic-coding-standards.md §2.5).
//! A dev-dependency of every tracking-core crate; depends on no workspace crate, so
//! that it never creates a dev-dependency cycle that would split a crate's types into
//! two instances under `cargo test`.
//!
//! The debug-only PSD assertion (`assert_psd`) that §2.1 requires every filter to
//! call lives in `gungnir-core::numeric`, not here: library code cannot call into a
//! dev-dependency. This crate only provides test-data strategies.

/// Shared proptest strategies. Each is written once here and imported by every crate
/// that needs it (§2.5: no per-crate duplication).
pub mod strategies {
    use nalgebra::SMatrix;
    use proptest::prelude::*;

    /// A valid (symmetric, positive semi-definite) covariance matrix of size `N`,
    /// built as `A Aᵀ + ε I` from a random `A`, so it is PSD by construction and
    /// strictly positive-definite by the ε shift.
    pub fn psd_covariance<const N: usize>() -> impl Strategy<Value = SMatrix<f64, N, N>> {
        prop::collection::vec(-10.0_f64..10.0, N * N).prop_map(|entries| {
            let a = SMatrix::<f64, N, N>::from_iterator(entries);
            a * a.transpose() + SMatrix::<f64, N, N>::identity() * 1e-6
        })
    }

    /// A rectangular cost matrix (rows x cols) with optionally degenerate entries
    /// (ties and large values) for association/assignment tests.
    pub fn cost_matrix(rows: usize, cols: usize) -> impl Strategy<Value = nalgebra::DMatrix<f64>> {
        prop::collection::vec(prop_oneof![Just(0.0), Just(1.0), 0.0_f64..1e3], rows * cols)
            .prop_map(move |entries| nalgebra::DMatrix::from_iterator(rows, cols, entries))
    }
}

#[cfg(test)]
mod tests {
    use super::strategies::*;
    use proptest::prelude::*;

    proptest! {
        /// Invariant: every matrix the PSD strategy produces is symmetric and has a
        /// Cholesky factorization.
        #[test]
        fn psd_strategy_yields_psd(cov in psd_covariance::<4>()) {
            let asym = (cov - cov.transpose()).abs().max();
            prop_assert!(asym < 1e-9);
            prop_assert!(cov.cholesky().is_some());
        }

        /// Invariant: cost matrices have the requested shape.
        #[test]
        fn cost_matrix_has_shape(m in cost_matrix(3, 5)) {
            prop_assert_eq!(m.shape(), (3, 5));
        }
    }
}
