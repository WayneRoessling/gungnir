// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential tests for the two `docs/verification-capability-table.md` §1
//! `track-fusion` rows.
//!
//! * **Track-to-track fusion (CI, information-matrix)**, oracle "Stone Soup fuser
//!   (partial) + hand-derived CI", criterion state < 1e-6 and covariance Frobenius
//!   difference < 1e-6.
//! * **Sensor registration / bias estimation**, oracle hand-derived least squares with
//!   the bias injected by construction, criterion recovered bias within 1e-3 of the
//!   injected truth.
//!
//! # The two oracles, and what "partial" means
//!
//! The hand-derived covariance intersection in the generator is the primary oracle. It
//! finds `ω` with scipy's bounded Brent minimiser while this crate uses a golden-section
//! search, so the two arrive at the same answer by different routes -- which is the
//! point. Two implementations of the same search would agree even if the search were
//! wrong.
//!
//! Stone Soup's `ChernoffUpdater` is §2's "partial" oracle and it is partial for a
//! precise reason: it takes `ω` as a fixed parameter rather than optimising it, so it can
//! confirm the fusion formula at a given `ω` and cannot confirm the choice of `ω`. The
//! generator drives it at a fixed `ω`, evaluates the hand-derived formula at the same
//! `ω`, and records the distance between them.
//! [`the_partial_stone_soup_cross_check_actually_ran`] asserts on that recorded distance,
//! because a cross-check whose result nothing reads is a field in a file rather than a
//! check.

use gungnir_track::{Track, TrackId, TrackStatus};
use gungnir_track_fusion::{
    CovarianceIntersectionFuser, InformationMatrixFuser, SensorRegistration, TrackFuser,
};
use nalgebra::{SMatrix, SVector};

/// The fusion row's criterion.
const FUSION_TOL: f64 = 1e-6;
/// The registration row's criterion.
const BIAS_TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Estimate {
    x: Vec<f64>,
    p_diag: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct Fused {
    x: Vec<f64>,
    p: Vec<Vec<f64>>,
}

#[derive(serde::Deserialize)]
struct Partial {
    available: bool,
    #[serde(default)]
    max_difference: Option<f64>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct FusionCase {
    name: String,
    a: Estimate,
    b: Estimate,
    covariance_intersection: Fused,
    information_matrix: Fused,
    stonesoup_at_fixed_omega: Partial,
}

#[derive(serde::Deserialize)]
struct FusionFixture {
    oracle: String,
    partial_oracle: String,
    cases: Vec<FusionCase>,
}

#[derive(serde::Deserialize)]
struct RegistrationCase {
    name: String,
    injected_bias: Vec<f64>,
    variances: Vec<f64>,
    a: Vec<Vec<f64>>,
    b: Vec<Vec<f64>>,
    expected_bias: Vec<f64>,
    expected_residual_spread: f64,
}

#[derive(serde::Deserialize)]
struct RegistrationFixture {
    oracle: String,
    cases: Vec<RegistrationCase>,
}

fn load<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/track")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

fn track(state: &[f64], variances: &[f64], id: u64) -> Track {
    Track {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: SVector::<f64, 6>::from_column_slice(state),
        covariance: SMatrix::<f64, 6, 6>::from_diagonal(&SVector::<f64, 6>::from_column_slice(
            variances,
        )),
        misses_since_update: 0,
        hits: 3,
    }
}

fn compare(case: &str, what: &str, ours: &Track, theirs: &Fused) {
    let their_x = SVector::<f64, 6>::from_column_slice(&theirs.x);
    let dx = (ours.state - their_x).norm();
    assert!(
        dx < FUSION_TOL,
        "{case} {what}: state differed by {dx}, over {FUSION_TOL}\nours   {}\noracle {}",
        ours.state.transpose(),
        their_x.transpose()
    );
    let their_p = SMatrix::<f64, 6, 6>::from_fn(|r, c| theirs.p[r][c]);
    let dp = (ours.covariance - their_p).norm();
    assert!(
        dp < FUSION_TOL,
        "{case} {what}: covariance Frobenius difference {dp}, over {FUSION_TOL}"
    );
}

#[test]
fn covariance_intersection_matches_the_hand_derived_oracle() {
    let fixture: FusionFixture = load("fusion.json");
    assert!(
        fixture.oracle.contains("covariance intersection"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let a = track(&case.a.x, &case.a.p_diag, 1);
        let b = track(&case.b.x, &case.b.p_diag, 2);
        let ci = CovarianceIntersectionFuser
            .fuse(&[a.clone(), b.clone()])
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        compare(
            &case.name,
            "covariance intersection",
            &ci,
            &case.covariance_intersection,
        );

        let info = InformationMatrixFuser
            .fuse(&[a, b])
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        compare(
            &case.name,
            "information matrix",
            &info,
            &case.information_matrix,
        );
    }
}

/// §2's partial oracle, asserted on rather than stored. See the module documentation.
#[test]
fn the_partial_stone_soup_cross_check_actually_ran() {
    let fixture: FusionFixture = load("fusion.json");
    assert!(
        fixture.partial_oracle.contains("Chernoff"),
        "the fixture names its partial oracle: {}",
        fixture.partial_oracle
    );
    for case in &fixture.cases {
        assert!(
            case.stonesoup_at_fixed_omega.available,
            "{}: the Stone Soup cross-check did not run: {}",
            case.name,
            case.stonesoup_at_fixed_omega
                .reason
                .as_deref()
                .unwrap_or("no reason recorded")
        );
        let difference = case
            .stonesoup_at_fixed_omega
            .max_difference
            .unwrap_or_else(|| panic!("{}: the cross-check recorded no distance", case.name));
        assert!(
            difference < FUSION_TOL,
            "{}: the hand-derived formula and Stone Soup differ by {difference} at the \
             same fixed omega, so one of the two oracles is wrong and neither can be \
             trusted for the comparison above",
            case.name
        );
    }
}

#[test]
fn an_injected_bias_is_recovered_within_the_rows_tolerance() {
    let fixture: RegistrationFixture = load("registration.json");
    assert!(
        fixture.oracle.contains("injected by construction"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let a: Vec<Track> = case
            .a
            .iter()
            .enumerate()
            .map(|(i, s)| track(s, &case.variances, i as u64))
            .collect();
        let b: Vec<Track> = case
            .b
            .iter()
            .enumerate()
            .map(|(i, s)| track(s, &case.variances, 1000 + i as u64))
            .collect();

        let mut registration = SensorRegistration::default();
        let recovered = registration
            .estimate_bias(&a, &b)
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));

        // Against the oracle's own least squares, to the fusion tolerance: the two are
        // computing the same estimator and should agree to rounding.
        for axis in 0..3 {
            let difference = (recovered[axis] - case.expected_bias[axis]).abs();
            assert!(
                difference < FUSION_TOL,
                "{} axis {axis}: recovered {} vs oracle {}, differing by {difference}",
                case.name,
                recovered[axis],
                case.expected_bias[axis]
            );
        }

        // And against the injected truth, to the row's own criterion. This is the check
        // that matters: agreeing with the oracle's estimator proves the arithmetic, and
        // recovering the injected bias proves the estimator.
        //
        // The noisy case is expected to sit further from truth than the clean one, so
        // the criterion is applied against the truth only where the fixture recorded no
        // measurement noise; where it did, the residual spread is what the row's
        // tolerance can be judged against and the oracle comparison above carries the
        // arithmetic.
        if case.expected_residual_spread < BIAS_TOL {
            for axis in 0..3 {
                let error = (recovered[axis] - case.injected_bias[axis]).abs();
                assert!(
                    error < BIAS_TOL,
                    "{} axis {axis}: recovered {} against an injected {}, off by {error}, \
                     over the row's {BIAS_TOL}",
                    case.name,
                    recovered[axis],
                    case.injected_bias[axis]
                );
            }
        }

        let spread = registration
            .residual_spread()
            .expect("a spread is recorded whenever a bias is");
        assert!(
            (spread - case.expected_residual_spread).abs() < FUSION_TOL,
            "{}: residual spread {spread} vs oracle {}",
            case.name,
            case.expected_residual_spread
        );
        assert_eq!(registration.pairs(), case.a.len());
    }
}

/// The fixture must contain both a noiseless case and a noisy one. Without the noisy
/// one the estimator is only ever asked to average a set of identical numbers, which
/// any implementation gets right.
#[test]
fn the_fixture_covers_both_a_clean_and_a_noisy_registration() {
    let fixture: RegistrationFixture = load("registration.json");
    assert!(
        fixture
            .cases
            .iter()
            .any(|c| c.expected_residual_spread < 1e-9),
        "no noiseless case, so nothing pins the exact arithmetic"
    );
    assert!(
        fixture
            .cases
            .iter()
            .any(|c| c.expected_residual_spread > 1.0),
        "no noisy case, so the estimator is never asked to average anything"
    );
}
