// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`rfs` | PHD / CPHD filter", the CPHD half.
//!
//! Criterion, from §2 unchanged: **intensity weights within 1e-3, and exact cardinality
//! where unambiguous**. Read as: the cardinality distribution's mean and mode, and the
//! intensity function, must agree with the oracle to that tolerance.
//!
//! # There is no library oracle for this row
//!
//! Unlike the PHD row next to this one -- where Stone Soup 1.9.1's `PHDUpdater` exists
//! and was found to disagree -- Stone Soup 1.9.1 has **no CPHD updater at all**:
//! `stonesoup.updater.pointprocess` exports only `PHDUpdater`, checked by import in
//! `testdata/oracles/tools/gen_cphd_fixtures.py` rather than assumed. So this row is
//! gated against that generator's own closed-form derivation, which is independently
//! checked -- before the fixture is ever written -- against a brute-force enumeration of
//! every possible target-to-measurement association, against the identity that an
//! updated intensity's integral must equal the updated cardinality distribution's mean,
//! and against reducing exactly to the plain GM-PHD update when fed a Poisson
//! cardinality prior. [`the_cross_check_is_recorded`] asserts the fixture still carries
//! that evidence, the same role `phd.json`'s own Stone Soup disagreement fields play for
//! the PHD row.
//!
//! # The intensity is compared as a function, not as a component list
//!
//! Unchanged from the PHD row's own reasoning: a Gaussian mixture is a representation of
//! an intensity, and two correct filters that prune and merge in a different order carry
//! the same intensity in a different number of components. The comparison is on the
//! function itself -- its value at fixed probe points -- and on the cardinality
//! distribution's mean and mode, never on the component list or the raw distribution
//! vector component-by-component (which a different but equally valid truncation bound
//! could shift without changing what either filter believes).

use gungnir_rfs::{CphdFilter, GaussianComponent, PhdSettings};
use gungnir_track::ConstantVelocity;
use nalgebra::{SMatrix, SVector};

/// The row's criterion on the weights and the cardinality mean/mode.
const WEIGHT_TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Settings {
    probability_of_survival: f64,
    probability_of_detection: f64,
    clutter_density: f64,
    prune_threshold: f64,
    merge_distance: f64,
    max_components: usize,
    max_cardinality: usize,
    sigma_a_sq: f64,
    dt: f64,
    r_diag: Vec<f64>,
    birth_cov_diag: Vec<f64>,
    birth_weight: f64,
}

#[derive(serde::Deserialize)]
struct Scan {
    cardinality_mean: f64,
    cardinality_map: usize,
    intensity_at_probes: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    truth: Vec<Vec<f64>>,
    scan_count: usize,
    birth_scans: Vec<usize>,
    probes: Vec<Vec<f64>>,
    per_scan: Vec<Scan>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    stonesoup_status: String,
    lambda_cross_check_worst_relative_error: f64,
    settings: Settings,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/track/cphd.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

fn position_h() -> SMatrix<f64, 3, 6> {
    let mut h = SMatrix::<f64, 3, 6>::zeros();
    for axis in 0..3 {
        h[(axis, axis)] = 1.0;
    }
    h
}

fn diagonal<const N: usize>(d: &[f64]) -> SMatrix<f64, N, N> {
    let mut m = SMatrix::<f64, N, N>::zeros();
    for (i, v) in d.iter().enumerate() {
        m[(i, i)] = *v;
    }
    m
}

/// The mixture evaluated at one position, marginalised over velocity -- identical to
/// `phd_diff.rs`'s own, since it is the same intensity representation in both filters.
fn intensity_at(components: &[GaussianComponent], point: &[f64]) -> f64 {
    let mut total = 0.0;
    for component in components {
        let d = SVector::<f64, 3>::new(
            point[0] - component.mean[0],
            point[1] - component.mean[1],
            point[2] - component.mean[2],
        );
        let p = SMatrix::<f64, 3, 3>::from_fn(|r, c| component.cov[(r, c)]);
        let Some(inverse) = p.try_inverse() else {
            continue;
        };
        let determinant = p.determinant();
        if determinant <= 0.0 {
            continue;
        }
        let quadratic = (d.transpose() * inverse * d)[(0, 0)];
        let normaliser = ((2.0 * std::f64::consts::PI).powi(3) * determinant).sqrt();
        total += component.weight * (-0.5 * quadratic).exp() / normaliser;
    }
    total
}

#[test]
fn the_cphd_filter_matches_the_closed_form_derivation() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("closed-form"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert!(!fixture.cases.is_empty());
    let s = &fixture.settings;

    for case in &fixture.cases {
        let mut filter = CphdFilter::new(
            PhdSettings {
                probability_of_survival: s.probability_of_survival,
                probability_of_detection: s.probability_of_detection,
                clutter_density: s.clutter_density,
                prune_threshold: s.prune_threshold,
                merge_distance: s.merge_distance,
                max_components: s.max_components,
                extraction_threshold: 0.5,
            },
            position_h(),
            diagonal::<3>(&s.r_diag),
            s.max_cardinality,
        )
        .expect("the fixture describes a valid scene");

        let motion = ConstantVelocity {
            sigma_a_sq: s.sigma_a_sq,
        };
        let detections: Vec<SVector<f64, 3>> = case
            .truth
            .iter()
            .map(|p| SVector::<f64, 3>::from_column_slice(p))
            .collect();

        for scan in 0..case.scan_count {
            let births: Vec<GaussianComponent> = if case.birth_scans.contains(&scan) {
                case.truth
                    .iter()
                    .map(|p| {
                        let mut mean = SVector::<f64, 6>::zeros();
                        for axis in 0..3 {
                            mean[axis] = p[axis];
                        }
                        GaussianComponent {
                            weight: s.birth_weight,
                            mean,
                            cov: diagonal::<6>(&s.birth_cov_diag),
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            filter
                .predict(&motion, s.dt, &births)
                .unwrap_or_else(|e| panic!("{} scan {scan}: {e}", case.name));
            filter
                .update(&detections)
                .unwrap_or_else(|e| panic!("{} scan {scan}: {e}", case.name));

            let expected = &case.per_scan[scan];
            let mean = filter.cardinality_mean();
            assert!(
                (mean - expected.cardinality_mean).abs() < WEIGHT_TOL,
                "{} scan {scan}: cardinality mean {mean} vs oracle {}, over {WEIGHT_TOL}",
                case.name,
                expected.cardinality_mean
            );
            assert_eq!(
                filter.cardinality_map(),
                expected.cardinality_map,
                "{} scan {scan}: cardinality mode disagrees with the oracle",
                case.name
            );

            for (i, probe) in case.probes.iter().enumerate() {
                let ours = intensity_at(&filter.phd.intensity_components, probe);
                let theirs = expected.intensity_at_probes[i];
                let difference = (ours - theirs).abs() / theirs.abs().max(1e-9);
                assert!(
                    difference < WEIGHT_TOL,
                    "{} scan {scan} probe {i}: intensity {ours} vs oracle {theirs}, \
                     relative difference {difference} over {WEIGHT_TOL}",
                    case.name
                );
            }
        }
    }
}

/// The evidence that stands in for a library cross-check must survive a regeneration.
/// Without this, a later run of the generator could drop the self-check entirely and
/// the row would silently look like an ordinary library-gated one.
#[test]
fn the_cross_check_is_recorded() {
    let fixture = fixture();
    assert!(
        fixture.stonesoup_status.contains("no CPHD updater"),
        "the fixture must record that Stone Soup has nothing to compare against: {}",
        fixture.stonesoup_status
    );
    assert!(
        fixture.lambda_cross_check_worst_relative_error.is_finite()
            && fixture.lambda_cross_check_worst_relative_error < 1e-6,
        "the brute-force cross-check must have actually run and agreed: worst relative \
         error {}",
        fixture.lambda_cross_check_worst_relative_error
    );
}

/// The fixture's scenes must actually exercise more than one target, since merging and
/// the cardinality distribution's shape both only become interesting above one.
#[test]
fn the_fixture_covers_a_multi_target_scene() {
    let fixture = fixture();
    assert!(
        fixture.cases.iter().any(|c| c.truth.len() >= 3),
        "every fixture scene has fewer than three targets, so merging is never \
         meaningfully exercised"
    );
}
