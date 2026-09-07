//! allocation: Bellman/DP resource-to-track assignment --
//! verification-capability-table.md `allocation` row. Exact match on value function
//! (1e-9) vs. textbook-verified DP -- a deterministic optimization problem with one
//! correct answer, same rationale as the `core` motion-model row. Deterministic math:
//! no logging here (agentic-coding-standards.md §2.8).

pub mod bellman;

pub use bellman::{solve_exact, value_function, MAX_RESOURCES, MAX_TRACKS};
pub use gungnir_core::{ResourceId, TrackId};

/// Why an allocation could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum AllocationError {
    /// Kept for a caller written against the scaffold; the solver no longer returns
    /// it (GAP-029, 2026-09-06). Removing the variant would be a breaking change for
    /// no gain, and a `match` that still handles it is not wrong.
    #[error("Bellman/DP allocator is not implemented yet")]
    NotImplemented,
    /// The reward matrix had zero rows or zero columns, or the horizon was zero.
    #[error("degenerate input: {0}")]
    DegenerateInput(&'static str),
}

/// Solves which resource should be tasked to which track over a planning horizon,
/// maximizing total reward under a Bellman-equation formulation.
///
/// `reward_matrix` is resources (rows) by tracks (columns); `horizon` is the number of
/// planning steps. Returns the optimal policy for the first step and its value.
pub trait ResourceAllocator {
    fn solve(
        &self,
        reward_matrix: &nalgebra::DMatrix<f64>,
        horizon: usize,
    ) -> Result<AllocationPolicy, AllocationError>;
}

/// The reference dynamic-programming allocator.
#[derive(Debug, Default, Clone, Copy)]
pub struct BellmanDpAllocator;

impl ResourceAllocator for BellmanDpAllocator {
    fn solve(
        &self,
        reward_matrix: &nalgebra::DMatrix<f64>,
        horizon: usize,
    ) -> Result<AllocationPolicy, AllocationError> {
        bellman::solve_exact(reward_matrix, horizon)
    }
}

/// A first-step assignment plus the value of the full-horizon policy it begins.
#[derive(Debug, Clone, PartialEq)]
pub struct AllocationPolicy {
    /// `(row, column)` **indices into the reward matrix**, one pair per resource tasked.
    ///
    /// **Not identifiers, and the type says so on purpose.** An allocator is handed an
    /// anonymous matrix; only the caller that built it knows which resource a row stands
    /// for. An earlier version of this field held `(ResourceId, TrackId)` filled with the
    /// row and column numbers, which was type-correct and wrong: the caller put them
    /// straight into a plan and then looked the track up by identifier, so wherever
    /// identifiers did not happen to coincide with indices the plan named the wrong
    /// effector against the wrong track and the intercept geometry silently vanished.
    /// Nothing could catch it, because every value involved was a valid value of its type.
    pub assignment: Vec<(usize, usize)>,
    pub value: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_matrix_is_degenerate() {
        let m = nalgebra::DMatrix::<f64>::zeros(0, 3);
        assert!(matches!(
            BellmanDpAllocator.solve(&m, 1),
            Err(AllocationError::DegenerateInput(_))
        ));
    }

    /// The trait implementation is the module's solver, not a second one.
    #[test]
    fn the_trait_solves_through_the_dynamic_program() {
        let m = nalgebra::DMatrix::from_row_slice(2, 2, &[3.0, 1.0, 1.0, 4.0]);
        let policy = BellmanDpAllocator.solve(&m, 1).expect("solvable");
        assert!((policy.value - 7.0).abs() < 1e-12, "{}", policy.value);
        assert_eq!(policy.assignment.len(), 2);
    }
}
