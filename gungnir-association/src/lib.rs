//! association: NN/GNN, Hungarian/JV, gating, JPDA, MHT --
//! verification-capability-table.md `association` rows.
//!
//! Implemented as of 2026-09-06: the Hungarian/Jonker-Volgenant solver
//! ([`assignment`]), global nearest-neighbour association over it, ellipsoidal
//! chi-square gating ([`gating`]), Joint Probabilistic Data Association ([`jpda`]) and
//! Multi-Hypothesis Tracking ([`mht`]).
//!
//! The four are alternatives, not a progression, and the choice is a deployment's:
//! nearest neighbour commits to one pairing and is right nearly always in a clean
//! scene; JPDA refuses to commit within a scan and pays for it in covariance; MHT
//! defers the commitment across scans and pays for it in memory and in the risk that
//! the deferred decision is never made. `PipelineSettings::from_baseline` selects
//! between them, and a baseline naming one this build does not implement is refused by
//! name rather than run as the default.
//!
//! # Fallible by design
//!
//! [`Associator::associate`] and [`solve_assignment`] return a `Result`. The cost
//! matrix arrives from association logic driven by sensor data, and `gungnir-fuzz`'s
//! `cost_matrix_construction` target exists to push malformed matrices through this
//! crate. A NaN in the cost matrix means the optimum is not defined; returning "no
//! associations" for it would be indistinguishable from "these tracks matched
//! nothing", which is the silent-stub failure `CLAUDE.md` rules out. The scaffold's
//! infallible signatures were changed here for that reason, before anything depended
//! on them.

pub mod assignment;
pub mod gating;
pub mod jpda;
pub mod mht;

pub use assignment::{solve_assignment, Assignment, AssociationError};
pub use gating::ChiSquareGate;
pub use jpda::{jpda, AssociationProbabilities, JpdaSettings, TrackPrediction};
pub use mht::{Hypothesis, HypothesisTree, MhtSettings};

/// The oracle-comparable surface for measurement-to-track association strategies.
pub trait Associator {
    /// Cost matrix in, assignment (`track_idx` -> `Option<detection_idx>`) out.
    ///
    /// # Errors
    /// [`AssociationError`] when the cost matrix does not define an optimum.
    fn associate(
        &mut self,
        cost_matrix: &nalgebra::DMatrix<f64>,
    ) -> Result<Vec<Option<usize>>, AssociationError>;
}

/// Global nearest-neighbor association via Hungarian/Jonker-Volgenant assignment.
///
/// "Global" is the whole point: the optimum over the matrix as a whole, not the
/// greedy pick of the smallest entry and then the next. The two differ exactly when
/// association is hard, which is the case Scenario 2 is built to produce.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlobalNearestNeighbor;

impl Associator for GlobalNearestNeighbor {
    fn associate(
        &mut self,
        cost_matrix: &nalgebra::DMatrix<f64>,
    ) -> Result<Vec<Option<usize>>, AssociationError> {
        Ok(solve_assignment(cost_matrix)?.row_to_col)
    }
}

/// Joint Probabilistic Data Association -- probability-weighted multi-track/multi-detection
/// association rather than a single hard assignment.
pub struct Jpda;

/// Multi-Hypothesis Tracking -- maintains a pruned tree of competing association
/// hypotheses across time.
pub struct Mht {
    pub max_hypotheses: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::DMatrix;

    /// GNN must return the optimal assignment, not the greedy one. On this matrix the
    /// greedy choice takes the 1 at (0,1) first and ends at 1 + 2 + 2 = 5 by a
    /// different route; the optimum is what the solver's own test pins.
    #[test]
    fn gnn_returns_the_optimal_assignment() {
        let cost = DMatrix::from_row_slice(3, 3, &[4.0, 1.0, 3.0, 2.0, 0.0, 5.0, 3.0, 2.0, 2.0]);
        let mut gnn = GlobalNearestNeighbor;
        let assignment = gnn.associate(&cost).expect("solvable");
        let total: f64 = assignment
            .iter()
            .enumerate()
            .filter_map(|(r, c)| c.map(|c| cost[(r, c)]))
            .sum();
        assert!((total - 5.0).abs() < 1e-12, "GNN total was {total}");
    }

    #[test]
    fn gnn_propagates_a_malformed_matrix_as_an_error() {
        let mut cost = DMatrix::from_element(2, 2, 1.0);
        cost[(0, 1)] = f64::NAN;
        let mut gnn = GlobalNearestNeighbor;
        assert!(gnn.associate(&cost).is_err());
    }
}
