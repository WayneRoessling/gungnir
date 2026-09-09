// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`rfs` | GLMB / LMB filter".
//!
//! Criterion, from §2 unchanged: **existence probability within 1e-3, and label
//! continuity match**. Read as: at every scan of every case, the set of live labels must
//! be exactly the oracle's (that is what "label continuity match" can mean for a filter
//! whose identifiers are its output), every label's existence probability must agree to
//! 1e-3, and every label's association probabilities -- the "label-to-track assignment"
//! the row's Method column names -- must agree to the same 1e-3. The spatial density is
//! compared as well, on the sibling rows' terms; nothing here widens the criterion.
//!
//! # There is no library oracle for this row, and that was established rather than assumed
//!
//! §1 and §2 both named "Stone Soup GLMB (partial)". Stone Soup 1.9.1 has **no GLMB and
//! no LMB at all** -- not partial, absent -- checked three ways in
//! `testdata/oracles/tools/gen_lmb_fixtures.py`, which refuses to run if it ever finds
//! one: no module in the package is named for a labelled filter; a regex scan of every
//! `.py` file it ships finds zero source lines mentioning GLMB, LMB or labelled
//! multi-Bernoulli; and all three plausible import paths raise `ModuleNotFoundError`.
//! Its nearest neighbours are the *single-target* `BernoulliParticleUpdater` and the
//! *unlabelled* `PHDUpdater`/`LCCUpdater`. This is the CPHD row's situation, not the PHD
//! row's: there is nothing to agree or disagree with.
//!
//! The oracle is therefore that generator's own derivation, whose association marginals
//! come from **literal enumeration** of every association event -- while
//! `gungnir-rfs` runs a subset dynamic program. Two derivations, not one implementation
//! compared with itself. [`the_cross_checks_are_recorded`] asserts the fixture still
//! carries the evidence, the same role `phd.json`'s Stone Soup disagreement fields and
//! `cphd.json`'s brute-force record play for their rows.
//!
//! # What is compared, and what deliberately is not
//!
//! The **label set** is compared exactly, because label identity is the entire reason
//! this row exists separately from the PHD/CPHD one, and an approximate answer to "is
//! this the same target" is not an answer. Everything continuous is compared as a
//! function or a probability: existence, the association marginals, and the spatial
//! density at fixed probe points. Never a label's component list -- two correct filters
//! that prune and merge in a different order carry the same density in a different
//! number of components, which is the reasoning the PHD and CPHD rows already record.

use gungnir_rfs::{LmbBirth, LmbFilter, LmbSettings};
use gungnir_track::ConstantVelocity;
use nalgebra::{SMatrix, SVector};

/// The row's criterion, on existence and on the association probabilities.
const EXISTENCE_TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Settings {
    probability_of_survival: f64,
    probability_of_detection: f64,
    clutter_density: f64,
    merge_distance: f64,
    max_components: usize,
    existence_prune_threshold: f64,
    spatial_prune_threshold: f64,
    sigma_a_sq: f64,
    dt: f64,
    r_diag: Vec<f64>,
    birth_cov_diag: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct Birth {
    label: u64,
    existence: f64,
    mean: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct Scan {
    labels: Vec<u64>,
    existence: Vec<f64>,
    association_labels: Vec<u64>,
    association_marginals: Vec<Vec<f64>>,
    mean: Vec<Vec<f64>>,
    density_at_probes: Vec<Vec<f64>>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    scan_count: usize,
    detections: Vec<Vec<Vec<f64>>>,
    births: Vec<Vec<Birth>>,
    probes: Vec<Vec<f64>>,
    per_scan: Vec<Scan>,
}

#[derive(serde::Deserialize)]
struct Checks {
    marginals_three_ways_worst_relative_error: f64,
    normaliser_three_ways_worst_relative_error: f64,
    single_target_bernoulli_worst_relative_error: f64,
    disjoint_labels_factorise_worst_relative_error: f64,
    spatial_mass_equals_existence_worst_absolute_error: f64,
    delta_glmb_existence_gap_per_scan: Vec<f64>,
    delta_glmb_truncated_mass: f64,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    filter_built: String,
    stonesoup_status: String,
    checks: Checks,
    settings: Settings,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/oracles/track/lmb.json");
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

/// One label's spatial density at one position, marginalised over velocity. The label's
/// weights are normalised, so this is a probability density -- the same *kind* of
/// comparison the PHD and CPHD rows make on their intensity, on a different object.
fn density_at(components: &[gungnir_rfs::GaussianComponent], point: &[f64]) -> f64 {
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

// One replay loop over one fixture: the birth check, the label set, the existence
// probabilities, the association marginals and the spatial density are five assertions
// about the same scan, and splitting them into helpers taking eight arguments each would
// scatter the case and scan context that makes a failure message readable.
#[allow(clippy::too_many_lines)]
#[test]
fn the_labelled_filter_matches_the_enumerated_derivation() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("enumeration"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert!(!fixture.cases.is_empty());
    let s = &fixture.settings;
    // The birth covariance is carried by the fixture, not assumed here: a generator that
    // changed it and a test that hard-coded the old value would agree on nothing and
    // report a filter disagreement.
    let cov = diagonal::<6>(&s.birth_cov_diag);
    // Tracked and printed so the capability table's "worst measured disagreement" column
    // is a number this test actually produced (`cargo test -p gungnir-rfs --test lmb_diff
    // -- --nocapture`) rather than a restatement of the tolerance.
    let mut worst_existence = 0.0_f64;
    let mut worst_association = 0.0_f64;
    let mut worst_density = 0.0_f64;
    let mut worst_state = 0.0_f64;

    for case in &fixture.cases {
        let mut filter = LmbFilter::new(
            LmbSettings {
                probability_of_survival: s.probability_of_survival,
                probability_of_detection: s.probability_of_detection,
                clutter_density: s.clutter_density,
                existence_prune_threshold: s.existence_prune_threshold,
                spatial_prune_threshold: s.spatial_prune_threshold,
                merge_distance: s.merge_distance,
                max_components_per_label: s.max_components,
                max_bernoullis: 100,
                extraction_threshold: 0.5,
                max_detections_per_scan: 12,
            },
            position_h(),
            diagonal::<3>(&s.r_diag),
        )
        .expect("the fixture describes a valid scene");

        let motion = ConstantVelocity {
            sigma_a_sq: s.sigma_a_sq,
        };

        for scan in 0..case.scan_count {
            let births: Vec<LmbBirth> = case.births[scan]
                .iter()
                .map(|b| LmbBirth {
                    existence: b.existence,
                    mean: SVector::<f64, 6>::from_column_slice(&b.mean),
                    cov,
                })
                .collect();
            filter
                .predict(&motion, s.dt, &births)
                .unwrap_or_else(|e| panic!("{} scan {scan}: {e}", case.name));

            // The labels the oracle expects to exist BEFORE the update: this is where a
            // label-allocation disagreement shows up first, and it is checked before the
            // update rather than after so a mismatch is attributed to birth rather than
            // to the association.
            let expected_births: Vec<u64> = case.births[scan].iter().map(|b| b.label).collect();
            for label in &expected_births {
                assert!(
                    filter.labels().contains(&gungnir_track::TrackId(*label)),
                    "{} scan {scan}: the oracle allocated label {label} at birth and the \
                     filter did not; labels are {:?}",
                    case.name,
                    filter.labels()
                );
            }

            let detections: Vec<SVector<f64, 3>> = case.detections[scan]
                .iter()
                .map(|z| SVector::<f64, 3>::from_column_slice(z))
                .collect();
            filter
                .update(&detections)
                .unwrap_or_else(|e| panic!("{} scan {scan}: {e}", case.name));

            let expected = &case.per_scan[scan];

            // Label continuity: the live label set, exactly.
            let labels: Vec<u64> = filter.labels().iter().map(|l| l.0).collect();
            assert_eq!(
                labels, expected.labels,
                "{} scan {scan}: the live label set disagrees with the oracle",
                case.name
            );

            // Existence probabilities, to the row's tolerance.
            for (i, bernoulli) in filter.bernoullis().iter().enumerate() {
                let theirs = expected.existence[i];
                let difference = (bernoulli.existence - theirs).abs();
                worst_existence = worst_existence.max(difference);
                assert!(
                    difference < EXISTENCE_TOL,
                    "{} scan {scan} label {:?}: existence {} vs oracle {theirs}, over \
                     {EXISTENCE_TOL}",
                    case.name,
                    bernoulli.label,
                    bernoulli.existence
                );
            }

            // The label-to-detection assignment probabilities themselves.
            let association = filter.last_association();
            let association_labels: Vec<u64> = association.iter().map(|(l, _)| l.0).collect();
            assert_eq!(
                association_labels, expected.association_labels,
                "{} scan {scan}: the labels the association was computed over disagree",
                case.name
            );
            for ((label, ours), theirs) in association.iter().zip(&expected.association_marginals) {
                assert_eq!(
                    ours.len(),
                    theirs.len(),
                    "{} scan {scan} label {label:?}: association row width disagrees",
                    case.name
                );
                for (k, (a, b)) in ours.iter().zip(theirs).enumerate() {
                    worst_association = worst_association.max((a - b).abs());
                    assert!(
                        (a - b).abs() < EXISTENCE_TOL,
                        "{} scan {scan} label {label:?} association {k}: {a} vs oracle \
                         {b}, over {EXISTENCE_TOL}",
                        case.name
                    );
                }
            }

            // The spatial density, as a function at fixed probe points, and its mean.
            for (i, bernoulli) in filter.bernoullis().iter().enumerate() {
                for (p, probe) in case.probes.iter().enumerate() {
                    let ours = density_at(&bernoulli.spatial, probe);
                    let theirs = expected.density_at_probes[i][p];
                    let difference = (ours - theirs).abs() / theirs.abs().max(1e-12);
                    worst_density = worst_density.max(difference);
                    assert!(
                        difference < EXISTENCE_TOL,
                        "{} scan {scan} label {:?} probe {p}: density {ours} vs oracle \
                         {theirs}, relative difference {difference}",
                        case.name,
                        bernoulli.label
                    );
                }
                let mut mean = SVector::<f64, 6>::zeros();
                for component in &bernoulli.spatial {
                    mean += component.mean * component.weight;
                }
                for (axis, theirs) in expected.mean[i].iter().enumerate() {
                    let difference = (mean[axis] - theirs).abs() / theirs.abs().max(1.0);
                    worst_state = worst_state.max(difference);
                    assert!(
                        difference < EXISTENCE_TOL,
                        "{} scan {scan} label {:?} state {axis}: {} vs oracle {theirs}",
                        case.name,
                        bernoulli.label,
                        mean[axis]
                    );
                }
            }
        }
    }

    println!(
        "lmb_diff worst measured disagreement over {} cases: existence {worst_existence:.3e},          association probability {worst_association:.3e}, spatial density {worst_density:.3e}          relative, state {worst_state:.3e} relative",
        fixture.cases.len()
    );
}

/// The evidence that stands in for a library cross-check must survive a regeneration.
/// Without this, a later run of the generator could drop the self-checks entirely and
/// the row would silently look like an ordinary library-gated one.
#[test]
fn the_cross_checks_are_recorded() {
    let fixture = fixture();
    assert!(
        fixture.stonesoup_status.contains("no GLMB and no LMB"),
        "the fixture must record that Stone Soup has nothing to compare against: {}",
        fixture.stonesoup_status
    );
    assert!(
        fixture.filter_built.contains("LMB") && fixture.filter_built.contains("not the full"),
        "the fixture must say which filter it is the oracle for: {}",
        fixture.filter_built
    );

    let c = &fixture.checks;
    for (name, value) in [
        (
            "marginals three ways",
            c.marginals_three_ways_worst_relative_error,
        ),
        (
            "normaliser three ways",
            c.normaliser_three_ways_worst_relative_error,
        ),
        (
            "single-target Bernoulli reduction",
            c.single_target_bernoulli_worst_relative_error,
        ),
        (
            "disjoint labels factorise",
            c.disjoint_labels_factorise_worst_relative_error,
        ),
        (
            "spatial mass equals existence",
            c.spatial_mass_equals_existence_worst_absolute_error,
        ),
    ] {
        assert!(
            value.is_finite() && value < 1e-9,
            "the '{name}' check must have actually run and agreed: worst error {value}"
        );
    }

    // The sharpest of the five: a single update from an LMB prior must reproduce the
    // full delta-GLMB's per-label existence EXACTLY, because moment matching is exact in
    // the marginals. Anything but zero here means the projection is wrong.
    let gaps = &c.delta_glmb_existence_gap_per_scan;
    assert!(
        gaps.len() >= 2,
        "the delta-GLMB comparison must cover more than one scan to show anything: \
         {gaps:?}"
    );
    assert!(
        gaps[0] < 1e-12,
        "the first update from an LMB prior must equal the full delta-GLMB exactly, and \
         the fixture records a gap of {}",
        gaps[0]
    );
    assert!(
        c.delta_glmb_truncated_mass < 1e-9,
        "the delta-GLMB reference truncated {} of its mass, so it is not the untruncated \
         comparison it is recorded as",
        c.delta_glmb_truncated_mass
    );
    // And the gap must actually grow after that: if it stayed at zero the comparison
    // would not be exercising the approximation at all, and the claim that this filter
    // approximates something would be untested.
    assert!(
        gaps[1..].iter().any(|g| *g > gaps[0]),
        "the delta-GLMB gap never grows past the first scan, so the LMB projection's \
         cost is not actually being measured: {gaps:?}"
    );
}

/// The cases must include the two the row is actually about. A fixture of nothing but
/// well-separated targets would pass every assertion above while proving nothing about
/// identity, which is the only thing this filter adds over the PHD/CPHD row.
#[test]
fn the_fixture_covers_the_cases_the_row_is_about() {
    let fixture = fixture();
    let names: Vec<&str> = fixture.cases.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("crossing")),
        "no case puts two targets through the same point, so label continuity is never \
         put under any strain: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("appears_midway")),
        "no case introduces a target part-way through, so 'a new target gets a new \
         label' is never exercised: {names:?}"
    );
    // And the crossing case must actually contain the tie, or it is a crossing in name
    // only -- this is the property that distinguishes a filter reporting its own
    // uncertainty from one manufacturing certainty.
    let crossing = fixture
        .cases
        .iter()
        .find(|c| c.name.contains("crossing"))
        .expect("checked above");
    let tie = crossing.per_scan.iter().any(|scan| {
        scan.association_marginals
            .iter()
            .any(|row| row.len() >= 4 && (row[2] - row[3]).abs() < 1e-6 && row[2] > 0.4)
    });
    assert!(
        tie,
        "the crossing case never produces an even association split, so the targets \
         never actually become indistinguishable"
    );
}
