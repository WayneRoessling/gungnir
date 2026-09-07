// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`filters` | Interacting Multiple Model (IMM)".
//!
//! **The oracle is not the one §2 planned, and that is recorded rather than hidden.**
//! §2 names Stone Soup's IMM. Stone Soup 1.9.1, the version this workspace pins in
//! `.venv-oracles`, has no IMM: no such symbol exists anywhere in the installed package.
//! The oracle here is `filterpy.kalman.IMMEstimator` 1.4.5, an independent
//! implementation of the same Blom/Bar-Shalom recursion and the oracle four other rows
//! in this table already use.
//!
//! **The criterion is unchanged**: state relative error < 1e-4, mode probabilities
//! within 1e-3. Those are §2's own numbers. Substituting an oracle is a change to the
//! Method column; loosening the numbers to make a substitute fit would be a change to
//! the pass criterion, which `CLAUDE.md` forbids.
//!
//! Every predict and every update of every case is checked, for the reason the linear
//! row gives: a filter can reach the right answer by a wrong path.
//!
//! The mode probabilities are checked as well as the state, and they are the half that
//! matters most. Two IMMs can report nearly the same combined estimate while disagreeing
//! completely about *why* -- one believing the target is turning, the other that it is
//! flying straight through noisy measurements. The combined state hides that; the mode
//! probabilities are the thing an operator would be shown.

use gungnir_core::{ConstantVelocity, CoordinatedTurn};
use gungnir_filters::{Imm, KalmanFilter, ModeFilter};
use nalgebra::{SMatrix, SVector};

/// §2's criterion for the state.
const STATE_TOL: f64 = 1e-4;
/// §2's criterion for the mode probabilities.
const MODE_TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Snapshot {
    x: Vec<f64>,
    p: Vec<Vec<f64>>,
}

#[derive(serde::Deserialize)]
struct Step {
    z: Vec<f64>,
    after_predict: Snapshot,
    mode_probabilities_after_predict: Vec<f64>,
    after_update: Snapshot,
    mode_probabilities_after_update: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    dt: f64,
    omega: f64,
    sigma_a_sq: f64,
    r_diag: Vec<f64>,
    p0_diag: Vec<f64>,
    x0: Vec<f64>,
    initial_mode_probabilities: Vec<f64>,
    transition: Vec<Vec<f64>>,
    steps: Vec<Step>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    filterpy: String,
    oracle_substitution: String,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/filters/imm.json");
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

/// `||a - b|| / max(||b||, 1)`, the same reading of "relative error" the EKF and UKF
/// rows use, and for the same reason: a six-state vector mixing metres with metres per
/// second has entries of wildly different magnitudes.
fn relative(ours: &SVector<f64, 6>, theirs: &SVector<f64, 6>) -> f64 {
    (ours - theirs).norm() / theirs.norm().max(1.0)
}

fn build(case: &Case) -> Imm<6, 3> {
    let x0 = vector(&case.x0);
    let p0 = diagonal::<6>(&case.p0_diag);
    let r = diagonal::<3>(&case.r_diag);
    let cv: Box<dyn ModeFilter<6, 3>> = Box::new(KalmanFilter::new(
        x0,
        p0,
        ConstantVelocity {
            sigma_a_sq: case.sigma_a_sq,
        },
        position_h(),
        r,
    ));
    let ct: Box<dyn ModeFilter<6, 3>> = Box::new(KalmanFilter::new(
        x0,
        p0,
        CoordinatedTurn {
            sigma_a_sq: case.sigma_a_sq,
            omega: case.omega,
        },
        position_h(),
        r,
    ));
    Imm::new(
        vec![cv, ct],
        &case.initial_mode_probabilities,
        &case.transition,
    )
    .expect("the fixture describes a well-formed IMM")
}

fn check_modes(case: &str, step: usize, half: &str, ours: &[f64], theirs: &[f64]) {
    assert_eq!(ours.len(), theirs.len(), "{case}: mode count");
    for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
        assert!(
            (a - b).abs() < MODE_TOL,
            "{case} step {step} {half}: mode {i} probability {a}, oracle {b}, over {MODE_TOL}"
        );
    }
}

#[test]
fn the_imm_matches_filterpy_over_the_whole_trajectory() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("IMMEstimator"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.filterpy, "1.4.5", "the pinned oracle version");
    assert!(
        fixture.oracle_substitution.contains("Stone Soup"),
        "the fixture must carry the record of why this is not the planned oracle"
    );
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let mut imm = build(case);
        for (i, step) in case.steps.iter().enumerate() {
            imm.predict(case.dt);
            let dx = relative(imm.state(), &vector(&step.after_predict.x));
            assert!(
                dx < STATE_TOL,
                "{} step {i} after predict: state relative error {dx} over {STATE_TOL}",
                case.name
            );
            let dp = (imm.covariance() - matrix(&step.after_predict.p)).norm()
                / matrix(&step.after_predict.p).norm().max(1.0);
            assert!(
                dp < STATE_TOL,
                "{} step {i} after predict: covariance relative error {dp}",
                case.name
            );
            check_modes(
                &case.name,
                i,
                "after predict",
                imm.mode_probabilities(),
                &step.mode_probabilities_after_predict,
            );

            imm.update(&SVector::<f64, 3>::from_column_slice(&step.z));
            let dx = relative(imm.state(), &vector(&step.after_update.x));
            assert!(
                dx < STATE_TOL,
                "{} step {i} after update: state relative error {dx} over {STATE_TOL}",
                case.name
            );
            let dp = (imm.covariance() - matrix(&step.after_update.p)).norm()
                / matrix(&step.after_update.p).norm().max(1.0);
            assert!(
                dp < STATE_TOL,
                "{} step {i} after update: covariance relative error {dp}",
                case.name
            );
            check_modes(
                &case.name,
                i,
                "after update",
                imm.mode_probabilities(),
                &step.mode_probabilities_after_update,
            );
        }
    }
}

/// The fixture's own cases must actually separate the modes. Without this, the test
/// above would pass just as happily on three straight-line runs where every mode
/// probability sat at 0.5 and the IMM was never asked to decide anything.
#[test]
fn the_fixture_cases_exercise_a_mode_decision() {
    let fixture = fixture();
    let mut any_decided = false;
    for case in &fixture.cases {
        let last = case
            .steps
            .last()
            .expect("a case with no steps proves nothing");
        let spread =
            last.mode_probabilities_after_update[0] - last.mode_probabilities_after_update[1];
        if spread.abs() > 0.3 {
            any_decided = true;
        }
    }
    assert!(
        any_decided,
        "no fixture case ends with the IMM preferring one mode, so the mode-probability \
         comparison above is vacuous"
    );
}
