// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`rfs` | PHD / CPHD filter".
//!
//! Criterion, from §2 unchanged: **intensity weights within 1e-3, and exact cardinality
//! where unambiguous**.
//!
//! # The oracle is the textbook recursion, not Stone Soup, and here is why
//!
//! §2 names Stone Soup's GM-PHD. It was driven for real by
//! `testdata/oracles/tools/gen_phd_fixtures.py` -- not sketched, not skipped -- and
//! **Stone Soup 1.9.1 disagrees**, by several percent of a target per target in the
//! scene. One cause is confirmed and reproducible:
//! `GaussianMixtureReducer.merge_components` clamps a merged component's weight to 1.0,
//! checked directly by merging components of weight 0.7 and 0.6 and getting 1.0. That is
//! wrong for a PHD intensity, whose weights are expected target counts rather than
//! probabilities: a component representing two unresolved targets has weight 2 by
//! definition. The clamp makes the library under-report cardinality exactly when merging
//! combines past one target, which is why the gap grows with the number of targets.
//!
//! **The first-scan difference is explained as of 2026-09-08, and the dominant half of it
//! was the generator's own fault.** It used to be recorded here as untraced. Two
//! conventions:
//!
//! 1. The generator stamped Stone Soup's birth components one scan in the past, and
//!    `DistanceHypothesiser` predicts every component to the detection's timestamp -- so
//!    each birth was predicted forward by a second before the update saw it, entering with
//!    `diag(P) = [500.333, 401, ...]` instead of the stated `[100, 400, ...]`. Vo--Ma adds
//!    the birth intensity to the already-predicted intensity and does not predict it. This
//!    was a transcription error against the library's interface, of the same species as
//!    the `mapping=(0, 1, 2)` one the generator already records, and it is **fixed**:
//!    births are stamped at the scan's own timestamp, a zero predict interval.
//! 2. Stone Soup applies `prob_survival` inside the update rather than in the predict, and
//!    exempts only components carrying its `TaggedWeightedGaussianState.BIRTH` sentinel --
//!    which these tags are not. That is a real convention difference and is **not** fixed:
//!    setting the sentinel makes Stone Soup drop the birth's missed-detection component
//!    entirely, which is a third convention and not Vo--Ma's either.
//!
//! Adopting both conventions in the reference collapses the first-scan cardinality gap
//! from 0.281 / 0.842 / 1.684 to 1.11e-16 / 4.44e-16 / 8.88e-16 across the three cases,
//! with the worst component-wise mean difference exactly zero. Scans 1 and 2 then agree to
//! machine epsilon and the divergence from scan 3 is the merge clamp above, so the two
//! documented causes account for the whole disagreement and none is outstanding.
//!
//! **Explained is not agreed.** The clamp is still wrong for a PHD intensity and
//! convention 2 is still there, so the row stays gated against the textbook recursion; the
//! fixture's recorded disagreement fell from 0.842 to 0.139 on the worst case because a
//! transcription error left the comparison, not because the library became an oracle.
//!
//! So the row is gated against the Vo--Ma Gaussian-mixture PHD recursion written out in
//! numpy in the generator. §2's Oracle column already reads "hand-derived" for two other
//! rows, so this is a form the table uses. The criterion is untouched; what changed is
//! which oracle it is measured against, and that change is written down here, in the
//! fixture, and in the row's own Method column.
//! [`the_stone_soup_disagreement_is_recorded_rather_than_forgotten`] asserts the fixture
//! still carries the record, so a later regeneration cannot quietly drop it.
//!
//! # The intensity is compared as a function, not as a component list
//!
//! §2's method is "compare intensity function + cardinality". A Gaussian mixture is a
//! *representation* of an intensity, and two correct filters that prune and merge in a
//! different order carry the same intensity in a different number of components. So the
//! comparison is on properties of the function itself: its integral, which is the
//! cardinality, and its value at fixed probe points. Comparing component lists would fail
//! on two correct filters, and loosening a tolerance until that passed would be widening
//! a pass criterion to make a test pass.

use gungnir_rfs::{GaussianComponent, PhdFilter, PhdSettings};
use gungnir_track::ConstantVelocity;
use nalgebra::{SMatrix, SVector};

/// The row's criterion on the weights.
const WEIGHT_TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Settings {
    probability_of_survival: f64,
    probability_of_detection: f64,
    clutter_density: f64,
    prune_threshold: f64,
    merge_distance: f64,
    max_components: usize,
    sigma_a_sq: f64,
    dt: f64,
    r_diag: Vec<f64>,
    birth_cov_diag: Vec<f64>,
    birth_weight: f64,
}

#[derive(serde::Deserialize)]
struct Scan {
    cardinality: f64,
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
    stonesoup_cardinality_disagreement: Option<f64>,
    stonesoup_first_scan_disagreement: Option<f64>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    stonesoup_status: String,
    stonesoup_confirmed_defect: String,
    stonesoup_first_scan_explained: String,
    settings: Settings,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/oracles/track/phd.json");
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

/// The mixture evaluated at one position, marginalised over velocity: the intensity as
/// a function, which is what the two implementations must agree on.
fn intensity_at(components: &[GaussianComponent], point: &[f64]) -> f64 {
    let mut total = 0.0;
    for component in components {
        let d = SVector::<f64, 3>::new(
            point[0] - component.mean[0],
            point[1] - component.mean[1],
            point[2] - component.mean[2],
        );
        // Marginalising a Gaussian over some components is dropping them, so the
        // position marginal is the leading 3x3 block.
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
fn the_phd_filter_matches_the_textbook_recursion() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("Vo-Ma") || fixture.oracle.contains("textbook"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert!(!fixture.cases.is_empty());
    let s = &fixture.settings;

    for case in &fixture.cases {
        let mut filter = PhdFilter::new(
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
            let cardinality = filter.cardinality();
            assert!(
                (cardinality - expected.cardinality).abs() < WEIGHT_TOL,
                "{} scan {scan}: cardinality {cardinality} vs oracle {}, over {WEIGHT_TOL}",
                case.name,
                expected.cardinality
            );

            for (i, probe) in case.probes.iter().enumerate() {
                let ours = intensity_at(&filter.intensity_components, probe);
                let theirs = expected.intensity_at_probes[i];
                // The intensity at a probe is a density, so its scale depends on the
                // covariance there. The comparison is relative against the oracle's own
                // value, with a floor so a probe where both filters say "essentially
                // nothing" is not judged as a ratio of two near-zeros.
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

/// The record of what the Stone Soup comparison established must survive a
/// regeneration. Without this, a later run of the generator could drop the fields and
/// the row would silently look like an ordinary library-gated one.
#[test]
fn the_stone_soup_disagreement_is_recorded_rather_than_forgotten() {
    let fixture = fixture();
    assert!(
        fixture.stonesoup_status.contains("disagrees"),
        "the fixture must record that Stone Soup is not a cross-check: {}",
        fixture.stonesoup_status
    );
    assert!(
        fixture
            .stonesoup_confirmed_defect
            .contains("merge_components"),
        "the confirmed cause must be named: {}",
        fixture.stonesoup_confirmed_defect
    );
    // Stricter than the assertion this replaced, which only required that *something*
    // was recorded as unexplained. Now that the cause is known, the record has to name
    // both halves of it -- the birth-timestamp transcription error and the prob_survival
    // convention -- or a regeneration could quietly reduce it to "explained" with nothing
    // behind the word.
    for required in [
        "timestamp",
        "prob_survival",
        "DistanceHypothesiser",
        "Vo-Ma",
    ] {
        assert!(
            fixture.stonesoup_first_scan_explained.contains(required),
            "the first-scan explanation must still name {required}: {}",
            fixture.stonesoup_first_scan_explained
        );
    }
    assert!(
        fixture
            .cases
            .iter()
            .all(|c| c.stonesoup_first_scan_disagreement.is_some()),
        "every case must record what the first-scan gap actually is now"
    );
    assert!(
        fixture.cases.iter().any(|c| c
            .stonesoup_cardinality_disagreement
            .is_some_and(|d| d > 0.0)),
        "no case records an actual disagreement, so the library may not have been run"
    );
}

/// The fixture's scenes must actually exercise more than one target, since the confirmed
/// Stone Soup defect and the merging logic both only bite above a cardinality of one.
#[test]
fn the_fixture_covers_a_multi_target_scene() {
    let fixture = fixture();
    assert!(
        fixture.cases.iter().any(|c| c.truth.len() >= 3),
        "every fixture scene has fewer than three targets, so merging is never \
         meaningfully exercised"
    );
}
