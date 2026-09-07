// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`filters` | Square-root / UDU-factorized KF & EKF".
//!
//! Criterion, from §2 unchanged: **< 1e-6 against the standard-form filter, and zero
//! PSD violations over 10⁵-plus cycles**. Both halves are checked here; the soak half
//! also runs in the module's own unit tests, and is repeated in this file against the
//! badly scaled case, which is the one the filter exists for.
//!
//! §2's method for this row is "compare vs. standard-form filter", and the reference
//! trajectory in the fixture is `filterpy.kalman.KalmanFilter` -- the same oracle the
//! linear row is gated against at the same tolerance, so the chain from oracle to this
//! filter is two comparisons at 1e-6 rather than one at a looser number.
//!
//! **Why not `filterpy.kalman.SquareRootKalmanFilter`.** It carries only the covariance
//! factor and exposes no process-noise factorisation, so driving it with this
//! workspace's `Q` means reconstructing `P` and refactoring it every step. That measures
//! filterpy's reconstruction, not the array recursion under test. The generator records
//! the same reasoning beside the fixture.

use gungnir_core::ConstantVelocity;
use gungnir_filters::SqrtKalmanFilter;
use nalgebra::{SMatrix, SVector};

/// §2's criterion.
const TOL: f64 = 1e-6;

#[derive(serde::Deserialize)]
struct Snapshot {
    x: Vec<f64>,
    p: Vec<Vec<f64>>,
}

#[derive(serde::Deserialize)]
struct Step {
    z: Vec<f64>,
    after_predict: Snapshot,
    after_update: Snapshot,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    dt: f64,
    sigma_a_sq: f64,
    r_diag: Vec<f64>,
    p0_diag: Vec<f64>,
    x0: Vec<f64>,
    steps: Vec<Step>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    filterpy: String,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/filters/sqrt.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

fn vector(v: &[f64]) -> SVector<f64, 6> {
    SVector::<f64, 6>::from_column_slice(v)
}

fn matrix(rows: &[Vec<f64>]) -> SMatrix<f64, 6, 6> {
    SMatrix::<f64, 6, 6>::from_fn(|r, c| rows[r][c])
}

fn diagonal<const N: usize>(d: &[f64]) -> SMatrix<f64, N, N> {
    let mut m = SMatrix::<f64, N, N>::zeros();
    for (i, v) in d.iter().enumerate() {
        m[(i, i)] = *v;
    }
    m
}

fn position_h() -> SMatrix<f64, 3, 6> {
    let mut h = SMatrix::<f64, 3, 6>::zeros();
    for axis in 0..3 {
        h[(axis, axis)] = 1.0;
    }
    h
}

fn build(case: &Case) -> SqrtKalmanFilter<ConstantVelocity, 6, 3> {
    SqrtKalmanFilter::new(
        vector(&case.x0),
        diagonal::<6>(&case.p0_diag),
        ConstantVelocity {
            sigma_a_sq: case.sigma_a_sq,
        },
        position_h(),
        diagonal::<3>(&case.r_diag),
    )
    .expect("the fixture describes a valid initial covariance")
}

#[test]
fn the_square_root_form_matches_the_standard_form_over_the_whole_trajectory() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("KalmanFilter"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.filterpy, "1.4.5", "the pinned oracle version");
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let mut filter = build(case);
        for (i, step) in case.steps.iter().enumerate() {
            filter
                .predict(case.dt)
                .unwrap_or_else(|e| panic!("{} step {i} predict: {e}", case.name));
            check(case, i, "after predict", &filter, &step.after_predict);

            filter
                .update(&SVector::<f64, 3>::from_column_slice(&step.z))
                .unwrap_or_else(|e| panic!("{} step {i} update: {e}", case.name));
            check(case, i, "after update", &filter, &step.after_update);
        }
    }
}

fn check(
    case: &Case,
    step: usize,
    half: &str,
    filter: &SqrtKalmanFilter<ConstantVelocity, 6, 3>,
    theirs: &Snapshot,
) {
    let their_x = vector(&theirs.x);
    let dx = (filter.state() - their_x).norm() / their_x.norm().max(1.0);
    assert!(
        dx < TOL,
        "{} step {step} {half}: state relative error {dx} over {TOL}",
        case.name
    );
    let their_p = matrix(&theirs.p);
    let dp = (filter.covariance() - their_p).norm() / their_p.norm().max(1.0);
    assert!(
        dp < TOL,
        "{} step {step} {half}: covariance relative error {dp} over {TOL}",
        case.name
    );
}

/// The second half of the criterion, on the badly scaled case rather than the
/// comfortable one. **Zero violations, not a tolerance**: `P` is reconstructed as
/// `S Sᵀ`, which is positive semi-definite for any `S` whatsoever, so a violation here
/// would mean the factor itself had gone non-finite.
#[test]
fn zero_psd_violations_over_a_hundred_thousand_cycles_on_the_badly_scaled_case() {
    let fixture = fixture();
    let case = fixture
        .cases
        .iter()
        .find(|c| c.name == "badly_scaled_measurement_noise")
        .expect("the fixture must carry the ill-conditioned case");
    let mut filter = build(case);
    let mut violations = 0_u32;
    let mut checks = 0_u32;
    for step in 0..100_500_i32 {
        filter.predict(case.dt).expect("a PSD process noise");
        let t = f64::from(step) * case.dt;
        filter
            .update(&SVector::<f64, 3>::new(10.0 + 3.0 * t, -5.0 + t, 100.0))
            .expect("a non-singular innovation");
        if step % 500 == 0 {
            checks += 1;
            let p = filter.covariance();
            let asymmetry = (p - p.transpose()).abs().max();
            let smallest = p.symmetric_eigenvalues().min();
            if asymmetry > 1e-9 || smallest < 0.0 || !p.iter().all(|v| v.is_finite()) {
                violations += 1;
            }
        }
    }
    assert!(
        checks > 200,
        "the soak did not run long enough to mean much"
    );
    assert_eq!(violations, 0, "the covariance left the PSD cone");
}
