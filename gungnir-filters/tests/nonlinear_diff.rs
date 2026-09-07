// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential tests for the `docs/verification-capability-table.md` §1 rows
//! "`filters` | Extended Kalman Filter (EKF)" and "`filters` | Unscented Kalman Filter
//! (UKF)".
//!
//! Oracles `filterpy.kalman.ExtendedKalmanFilter` and
//! `filterpy.kalman.UnscentedKalmanFilter` with `MerweScaledSigmaPoints`, both 1.4.5.
//! Pass criterion: **relative error < 1e-4**, and for the UKF additionally that the
//! sigma weights sum to one.
//!
//! Both fixtures record the state and covariance after *every* predict and after every
//! update, and this file checks all of them, for the reason the linear row's test gives:
//! a filter can reach the right answer by a wrong path, and comparing only the end would
//! hide it and would not say which half of the cycle failed.
//!
//! The measurement is range, azimuth and elevation from a sensor at the ENU origin,
//! which is the nonlinearity a radar actually has rather than one chosen to be
//! convenient. The fixture's geometry stays in one quadrant, so azimuth never crosses
//! the branch cut and this compares two filters rather than two conventions for wrapping
//! an angle; `gungnir_filters::nonlinear` documents that limitation where it lives.
//!
//! **Relative error is taken on the vector and on the matrix as wholes**, `||a - b|| /
//! max(||b||, 1)`, rather than entry by entry. A six-state vector mixing metres with
//! metres per second has entries of very different magnitudes, and an entrywise relative
//! error on the smallest of them would be a different and much harsher criterion than
//! the row's words; the floor of one keeps a near-zero oracle value from making the
//! denominator meaningless.

use gungnir_core::ConstantVelocity;
use gungnir_filters::{
    ExtendedKalmanFilter, Filter, RangeAzimuthElevation, SigmaPointSettings, UnscentedKalmanFilter,
};
use nalgebra::{SMatrix, SVector};

/// The rows' tolerance, verbatim.
const TOL: f64 = 1e-4;

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
    #[serde(default)]
    alpha: f64,
    #[serde(default)]
    beta: f64,
    #[serde(default)]
    kappa: f64,
    #[serde(default)]
    weights_mean: Vec<f64>,
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

fn fixture(name: &str) -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/filters")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

fn vector(v: &[f64]) -> SVector<f64, 6> {
    SVector::<f64, 6>::from_column_slice(v)
}

fn matrix(rows: &[Vec<f64>]) -> SMatrix<f64, 6, 6> {
    SMatrix::<f64, 6, 6>::from_fn(|r, c| rows[r][c])
}

fn diagonal3(d: &[f64]) -> SMatrix<f64, 3, 3> {
    let mut m = SMatrix::<f64, 3, 3>::zeros();
    for (i, v) in d.iter().enumerate() {
        m[(i, i)] = *v;
    }
    m
}

fn diagonal6(d: &[f64]) -> SMatrix<f64, 6, 6> {
    let mut m = SMatrix::<f64, 6, 6>::zeros();
    for (i, v) in d.iter().enumerate() {
        m[(i, i)] = *v;
    }
    m
}

/// `||a - b|| / max(||b||, 1)`, the reading of "relative error" this file uses.
fn relative_state(ours: &SVector<f64, 6>, theirs: &SVector<f64, 6>) -> f64 {
    (ours - theirs).norm() / theirs.norm().max(1.0)
}

fn relative_covariance(ours: &SMatrix<f64, 6, 6>, theirs: &SMatrix<f64, 6, 6>) -> f64 {
    (ours - theirs).norm() / theirs.norm().max(1.0)
}

fn check(
    case: &str,
    step: usize,
    half: &str,
    ours_x: &SVector<f64, 6>,
    ours_p: &SMatrix<f64, 6, 6>,
    theirs: &Snapshot,
) {
    let their_x = vector(&theirs.x);
    let their_p = matrix(&theirs.p);
    let dx = relative_state(ours_x, &their_x);
    assert!(
        dx < TOL,
        "{case} step {step} {half}: state relative error {dx} over {TOL}\nours   {}\noracle {}",
        ours_x.transpose(),
        their_x.transpose()
    );
    let dp = relative_covariance(ours_p, &their_p);
    assert!(
        dp < TOL,
        "{case} step {step} {half}: covariance relative error {dp} over {TOL}"
    );
}

#[test]
fn the_ekf_matches_filterpy_over_the_whole_trajectory() {
    let fixture = fixture("ekf.json");
    assert!(
        fixture.oracle.contains("ExtendedKalmanFilter"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.filterpy, "1.4.5", "the pinned oracle version");
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let mut filter = ExtendedKalmanFilter::new(
            vector(&case.x0),
            diagonal6(&case.p0_diag),
            ConstantVelocity {
                sigma_a_sq: case.sigma_a_sq,
            },
            RangeAzimuthElevation::default(),
            diagonal3(&case.r_diag),
        );
        for (i, step) in case.steps.iter().enumerate() {
            filter.predict(case.dt);
            check(
                &case.name,
                i,
                "after predict",
                filter.state(),
                filter.covariance(),
                &step.after_predict,
            );
            filter.update(&SVector::<f64, 3>::from_column_slice(&step.z));
            check(
                &case.name,
                i,
                "after update",
                filter.state(),
                filter.covariance(),
                &step.after_update,
            );
        }
        assert_eq!(
            filter.rejected_updates(),
            0,
            "{}: an update was skipped, so the comparison above is not of the same \
             sequence the oracle ran",
            case.name
        );
    }
}

#[test]
fn the_ukf_matches_filterpy_over_the_whole_trajectory() {
    let fixture = fixture("ukf.json");
    assert!(
        fixture.oracle.contains("UnscentedKalmanFilter"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.filterpy, "1.4.5", "the pinned oracle version");
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let points = SigmaPointSettings {
            alpha: case.alpha,
            beta: case.beta,
            kappa: case.kappa,
        };
        // The row's second criterion, checked against the oracle's own weights rather
        // than only against one: a weight set that summed to one but differed from
        // filterpy's would pass a self-check and fail the comparison below.
        let (wm, _) = points.weights(6);
        let sum: f64 = wm.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-12,
            "{}: mean weights sum to {sum}",
            case.name
        );
        assert_eq!(wm.len(), case.weights_mean.len(), "{}", case.name);
        for (i, (ours, theirs)) in wm.iter().zip(&case.weights_mean).enumerate() {
            assert!(
                (ours - theirs).abs() < 1e-12,
                "{}: mean weight {i} is {ours}, filterpy's is {theirs}",
                case.name
            );
        }

        let mut filter = UnscentedKalmanFilter::new(
            vector(&case.x0),
            diagonal6(&case.p0_diag),
            ConstantVelocity {
                sigma_a_sq: case.sigma_a_sq,
            },
            RangeAzimuthElevation::default(),
            diagonal3(&case.r_diag),
            points,
        );
        for (i, step) in case.steps.iter().enumerate() {
            filter
                .try_predict(case.dt)
                .unwrap_or_else(|e| panic!("{} step {i}: {e}", case.name));
            check(
                &case.name,
                i,
                "after predict",
                filter.state(),
                filter.covariance(),
                &step.after_predict,
            );
            filter
                .try_update(&SVector::<f64, 3>::from_column_slice(&step.z))
                .unwrap_or_else(|e| panic!("{} step {i}: {e}", case.name));
            check(
                &case.name,
                i,
                "after update",
                filter.state(),
                filter.covariance(),
                &step.after_update,
            );
        }
    }
}
