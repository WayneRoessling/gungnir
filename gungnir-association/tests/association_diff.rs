// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential tests for three verification-capability-table.md §1 rows:
//! "Hungarian / Jonker-Volgenant", "Nearest-Neighbor / GNN", and
//! "Gating (ellipsoidal / chi-square)".
//!
//! Oracles: `scipy.optimize.linear_sum_assignment` for the two assignment rows, and a
//! closed-form chi-square (thresholds from `scipy.stats.chi2.ppf`) for gating.
//!
//! # On "exact match"
//!
//! The assignment rows say **exact match on cost; assignment only if not tied**, and
//! that qualifier is load-bearing. Several distinct assignments can attain the same
//! optimal cost, and which one a solver returns is an artifact of its pivoting order.
//! The fixture therefore carries a `unique` flag per case, decided in the generator by
//! brute force over all permutations rather than assumed; the pairing is compared only
//! where it is true, and the cost is compared everywhere.
//!
//! "Exact" on a floating-point sum means to the rounding of that sum, not bit
//! equality: our solver and scipy add the selected entries in different orders, so the
//! two totals can differ in the last bit of a large sum. [`COST_TOL`] is a relative
//! tolerance at the scale of the cost, which is what "exact" can mean here; the margin
//! actually observed is printed so it is visible how far from that limit the result
//! sits.
//!
//! MATLAB's `assignjv`, `trackerGNN`, and the `trackingEKF` internal distance are
//! named by these rows and were **not** run: MATLAB is not installed, and
//! `testdata/oracles/README.md` records that.

use gungnir_association::{solve_assignment, Associator, ChiSquareGate, GlobalNearestNeighbor};
use nalgebra::{DMatrix, SMatrix, SVector};

/// Relative tolerance for the assignment cost: exact to the rounding of a sum of the
/// selected entries, taken in a different order than the oracle takes them.
const COST_TOL: f64 = 1e-12;

/// Tolerance on the squared Mahalanobis distance. The *membership* comparison is
/// exact; this only bounds the distance itself, which the row does not gate on but
/// which a disagreement in would explain a membership failure.
const D2_REL_TOL: f64 = 1e-9;

#[derive(serde::Deserialize)]
struct AssignmentCase {
    name: String,
    rows: usize,
    cols: usize,
    cost: Vec<Vec<f64>>,
    total_cost: f64,
    row_to_col: Vec<Option<usize>>,
    unique: Option<bool>,
}

#[derive(serde::Deserialize)]
struct AssignmentFixture {
    row: String,
    oracle: String,
    cases: Vec<AssignmentCase>,
}

#[derive(serde::Deserialize)]
struct GatingCase {
    name: String,
    dof: usize,
    confidence: f64,
    threshold: f64,
    innovation: Vec<f64>,
    innovation_covariance: Vec<Vec<f64>>,
    squared_distance: f64,
    admitted: bool,
}

#[derive(serde::Deserialize)]
struct GatingFixture {
    row: String,
    cases: Vec<GatingCase>,
}

fn load_assignment() -> AssignmentFixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/oracles/association/assignment.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("oracle fixture present; see testdata/oracles/README.md");
    serde_json::from_str(&raw).expect("oracle fixture parses")
}

fn load_gating() -> GatingFixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/oracles/association/gating.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("oracle fixture present; see testdata/oracles/README.md");
    serde_json::from_str(&raw).expect("oracle fixture parses")
}

fn to_matrix(case: &AssignmentCase) -> DMatrix<f64> {
    DMatrix::from_fn(case.rows, case.cols, |i, j| case.cost[i][j])
}

/// Relative difference at the scale of the larger magnitude, so a zero-cost optimum
/// is compared absolutely and a large one proportionally.
fn relative_difference(a: f64, b: f64) -> f64 {
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() / scale
}

#[test]
fn fixtures_are_the_expected_rows() {
    let a = load_assignment();
    assert_eq!(a.row, "Hungarian / Jonker-Volgenant");
    assert_eq!(a.oracle, "scipy.optimize.linear_sum_assignment");
    assert!(!a.cases.is_empty());
    assert!(
        a.cases.iter().any(|c| c.unique == Some(true)),
        "no case has a decidably unique optimum, so no pairing is ever compared"
    );
    assert!(
        a.cases.iter().any(|c| c.unique == Some(false)),
        "no tied case, so the tie handling this row calls for is never exercised"
    );

    let g = load_gating();
    assert_eq!(g.row, "Gating (ellipsoidal / chi-square)");
    assert!(g.cases.iter().any(|c| c.admitted), "no case is admitted");
    assert!(g.cases.iter().any(|c| !c.admitted), "no case is rejected");
}

/// The Hungarian/JV row: exact on cost everywhere, exact on the pairing where the
/// optimum is unique.
#[test]
fn assignment_matches_scipy() {
    let fixture = load_assignment();
    let mut worst_cost = 0.0_f64;
    let mut worst_where = String::new();
    let mut pairings_compared = 0;

    for case in &fixture.cases {
        let cost = to_matrix(case);
        let got = solve_assignment(&cost)
            .unwrap_or_else(|e| panic!("{}: solver rejected a well-formed matrix: {e}", case.name));

        let d = relative_difference(got.total_cost, case.total_cost);
        assert!(
            d < COST_TOL,
            "{}: total cost {} differs from scipy's {} by {d:e} relative, \
             tolerance {COST_TOL:e}",
            case.name,
            got.total_cost,
            case.total_cost
        );
        if d > worst_cost {
            worst_cost = d;
            worst_where = case.name.clone();
        }

        // A returned assignment must be a valid partial permutation regardless of
        // ties: no column used twice, every index in range.
        let mut seen = vec![false; case.cols];
        for (row, col) in got.row_to_col.iter().enumerate() {
            if let Some(c) = col {
                assert!(
                    *c < case.cols,
                    "{}: row {row} -> column {c} out of range",
                    case.name
                );
                assert!(!seen[*c], "{}: column {c} assigned twice", case.name);
                seen[*c] = true;
            }
        }
        assert_eq!(
            got.assigned_count(),
            case.rows.min(case.cols),
            "{}: assigned {} of a possible {}",
            case.name,
            got.assigned_count(),
            case.rows.min(case.cols)
        );

        // The pairing itself is only meaningful where the optimum is unique. `None`
        // means the generator could not decide, and is treated as "do not compare".
        if case.unique == Some(true) {
            assert_eq!(
                got.row_to_col, case.row_to_col,
                "{}: pairing differs from scipy on a case with a unique optimum",
                case.name
            );
            pairings_compared += 1;
        }
    }

    println!(
        "assignment: {} cases, {pairings_compared} pairings compared exactly; \
         worst relative cost difference {worst_cost:e} at {worst_where} \
         (tolerance {COST_TOL:e})",
        fixture.cases.len()
    );
}

/// The NN/GNN row rides on the same solver, so it is checked through the trait the
/// track manager will actually call rather than through the free function.
#[test]
fn gnn_matches_scipy_through_the_associator_trait() {
    let fixture = load_assignment();
    let mut gnn = GlobalNearestNeighbor;
    for case in &fixture.cases {
        let cost = to_matrix(case);
        let assignment = gnn
            .associate(&cost)
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let total: f64 = assignment
            .iter()
            .enumerate()
            .filter_map(|(r, c)| c.map(|c| cost[(r, c)]))
            .sum();
        assert!(
            relative_difference(total, case.total_cost) < COST_TOL,
            "{}: GNN total {total} differs from scipy's {}",
            case.name,
            case.total_cost
        );
        if case.unique == Some(true) {
            assert_eq!(assignment, case.row_to_col, "{}: GNN pairing", case.name);
        }
    }
}

/// Read a fixed-size innovation and covariance out of a case and run the gate.
fn gate_case<const M: usize>(case: &GatingCase) -> (f64, bool) {
    let y = SVector::<f64, M>::from_iterator(case.innovation.iter().copied());
    let s = SMatrix::<f64, M, M>::from_fn(|i, j| case.innovation_covariance[i][j]);
    let gate = ChiSquareGate {
        gate_threshold: case.threshold,
    };
    let d2 =
        ChiSquareGate::squared_distance(&y, &s).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    let admitted = gate
        .admits(&y, &s)
        .unwrap_or_else(|e| panic!("{}: {e}", case.name));
    (d2, admitted)
}

/// The gating row: **exact match on membership**.
#[test]
fn gating_membership_matches_the_oracle() {
    let fixture = load_gating();
    let mut worst_d2 = 0.0_f64;
    let mut worst_where = String::new();

    for case in &fixture.cases {
        let (d2, admitted) = match case.innovation.len() {
            1 => gate_case::<1>(case),
            2 => gate_case::<2>(case),
            3 => gate_case::<3>(case),
            other => panic!("{}: fixture has {other} measurement dimensions", case.name),
        };

        let d = relative_difference(d2, case.squared_distance);
        if d > worst_d2 {
            worst_d2 = d;
            worst_where = case.name.clone();
        }
        assert!(
            d < D2_REL_TOL,
            "{}: squared distance {d2} differs from the oracle's {} by {d:e} relative",
            case.name,
            case.squared_distance
        );
        assert_eq!(
            admitted, case.admitted,
            "{}: gate membership differs (d2 {d2} vs threshold {})",
            case.name, case.threshold
        );
    }

    println!(
        "gating: {} cases, membership exact on all; worst relative squared-distance \
         difference {worst_d2:e} at {worst_where}",
        fixture.cases.len()
    );
}

/// The quantile table in the crate is hard-coded. This checks it against the
/// thresholds scipy produced, so a typo in a constant is caught rather than trusted.
#[test]
fn hard_coded_quantiles_match_scipy() {
    let fixture = load_gating();
    let mut checked = 0;
    for case in &fixture.cases {
        #[allow(clippy::float_cmp)]
        let gate = if case.confidence == 0.95 {
            ChiSquareGate::at_95_percent(case.dof)
        } else if case.confidence == 0.99 {
            ChiSquareGate::at_99_percent(case.dof)
        } else {
            continue;
        };
        let d = relative_difference(gate.gate_threshold, case.threshold);
        assert!(
            d < 1e-12,
            "chi-square quantile for {} dof at {} differs from scipy: {} vs {}",
            case.dof,
            case.confidence,
            gate.gate_threshold,
            case.threshold
        );
        checked += 1;
    }
    assert!(checked > 0, "no quantile was actually compared");
    println!("gating: {checked} hard-coded quantiles agree with scipy.stats.chi2.ppf");
}
