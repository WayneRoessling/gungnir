//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`allocation` | Bellman/DP resource-to-track assignment".
//!
//! Oracle: the custom textbook DP in `testdata/oracles/tools/gen_allocation_fixtures.py`.
//! Pass criterion: **exact match on the value function (1e-9)**.
//!
//! The fixture records the whole value function, every layer and every subset, and this
//! test checks all of it. Comparing only the optimal value would let a wrong recursion
//! reach a right answer on the full set; comparing the policy instead would report a
//! tie between two equally optimal matchings as a disagreement, which is why the row
//! names the value function and not the policy.
//!
//! The oracle enumerates matchings by choosing a resource subset and a track
//! permutation; the Rust walks resources depth-first. The two constructions are
//! deliberately different, so agreement is evidence rather than a shared transcription.

use gungnir_allocation::{value_function, BellmanDpAllocator, ResourceAllocator};
use nalgebra::DMatrix;

/// The row's tolerance, verbatim.
const TOL: f64 = 1e-9;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    reward: Vec<Vec<f64>>,
    horizon: usize,
    value_function: Vec<Vec<f64>>,
    optimal_value: f64,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/allocation/bellman_dp.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

fn matrix(reward: &[Vec<f64>]) -> DMatrix<f64> {
    let rows = reward.len();
    let cols = reward[0].len();
    DMatrix::from_fn(rows, cols, |r, c| reward[r][c])
}

#[test]
fn the_value_function_matches_the_textbook_dp_at_every_state() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("textbook"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert!(!fixture.cases.is_empty(), "the fixture has cases");

    for case in &fixture.cases {
        let reward = matrix(&case.reward);
        let ours =
            value_function(&reward, case.horizon).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            ours.len(),
            case.value_function.len(),
            "{}: layer count",
            case.name
        );
        for (layer, (mine, theirs)) in ours.iter().zip(&case.value_function).enumerate() {
            assert_eq!(
                mine.len(),
                theirs.len(),
                "{}: layer {layer} width",
                case.name
            );
            for (subset, (a, b)) in mine.iter().zip(theirs).enumerate() {
                let diff = (a - b).abs();
                assert!(
                    diff <= TOL,
                    "{}: layer {layer}, track set {subset:b}: {a} vs oracle {b}, \
                     differ by {diff} over {TOL}",
                    case.name
                );
            }
        }
    }
}

/// The solver's own answer is the value function's top layer at the full track set, so
/// the policy a caller receives is the one the checked recursion produced.
#[test]
fn the_solved_policy_carries_the_oracles_optimal_value() {
    for case in &fixture().cases {
        let reward = matrix(&case.reward);
        let policy = BellmanDpAllocator
            .solve(&reward, case.horizon)
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let diff = (policy.value - case.optimal_value).abs();
        assert!(
            diff <= TOL,
            "{}: policy value {} vs oracle {}, differ by {diff}",
            case.name,
            policy.value,
            case.optimal_value
        );
        // The assignment must be a legal first step: each resource once, each track
        // once. An optimal value reported beside an illegal assignment would be worse
        // than a wrong number, because the number would look right.
        let mut resources: Vec<usize> = policy.assignment.iter().map(|(r, _)| *r).collect();
        let mut tracks: Vec<usize> = policy.assignment.iter().map(|(_, t)| *t).collect();
        resources.sort_unstable();
        tracks.sort_unstable();
        let unique_resources = {
            let mut r = resources.clone();
            r.dedup();
            r.len()
        };
        let unique_tracks = {
            let mut t = tracks.clone();
            t.dedup();
            t.len()
        };
        assert_eq!(
            unique_resources,
            resources.len(),
            "{}: a resource was assigned twice in one step",
            case.name
        );
        assert_eq!(
            unique_tracks,
            tracks.len(),
            "{}: a track took two resources in one step",
            case.name
        );
        assert!(
            resources.iter().all(|r| *r < case.reward.len()),
            "{}: an assignment names a resource outside the matrix",
            case.name
        );
        assert!(
            tracks.iter().all(|t| *t < case.reward[0].len()),
            "{}: an assignment names a track outside the matrix",
            case.name
        );
    }
}
