//! gungnir-oracle: differential-test harness, CI gate #1 (oracle-diff.yml).
//! Depends on everything; nothing depends on it (agentic-coding-standards.md §1).
//! Captures trace!-level span output on failure via a test-scoped (not global)
//! `tracing_subscriber`, per §2.8.

use gungnir_filters::Filter;

/// Run the same scenario through a Rust `Filter` and assert agreement against a
/// recorded oracle trajectory (produced offline by filterpy/MATLAB and checked in
/// as a fixture) within the tolerance specified for that capability-table row.
/// # Errors
///
/// Always: the harness is designed and not written (GAP-082).
///
/// **A `Result` even here, and especially here.** This is the differential-test harness:
/// a `todo!()` in it fails a test run with a panic that reads like the filter under test
/// blew up, when what happened is that nothing compared anything. The oracle fixtures it
/// needs are the MATLAB and filterpy trajectories that are not in the repository.
pub fn diff_test_filter<F: Filter>(
    _filter: &mut F,
    _oracle_trajectory_fixture: &str,
    _tolerance: f64,
) -> Result<(), OracleError> {
    Err(OracleError::NotImplemented {
        what: "the differential-test harness",
        waiting_on: "the recorded oracle trajectories, which are not checked in",
    })
}

/// What the oracle harness cannot do yet, named rather than panicked (GAP-082).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OracleError {
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
}
