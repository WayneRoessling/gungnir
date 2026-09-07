// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`association` | JPDA".
//!
//! Oracle Stone Soup 1.9.1's `JPDA` associator over its `PDAHypothesiser`, driven
//! directly by `testdata/oracles/tools/gen_jpda_fixtures.py` rather than reimplemented
//! in the generator. Criterion, from §2 unchanged: **relative error < 1e-3** on the
//! per-track association probabilities.
//!
//! Every probability of every track of every case is compared, including the
//! missed-detection probability. That last one matters as much as the rest: a JPDA that
//! gets the detection probabilities right but the miss wrong is one that will either
//! coast a track that was never seen or drop one that was.
//!
//! The comparison is on the **absolute** difference of the probabilities rather than a
//! ratio. These are probabilities in `[0, 1]`, so an absolute difference already is a
//! relative one against the only scale they have, and a ratio would make a disagreement
//! of 1e-9 versus 2e-9 -- two ways of saying "impossible" -- read as a 100% error.

use gungnir_association::{jpda, ChiSquareGate, JpdaSettings, TrackPrediction};
use nalgebra::{SMatrix, SVector};

/// §2's criterion.
const TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct TrackSpec {
    east: f64,
    north: f64,
    innovation_variance: f64,
}

#[derive(serde::Deserialize)]
struct Expected {
    missed: f64,
    detections: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    probability_of_detection: f64,
    probability_of_gate: f64,
    clutter_density: f64,
    tracks: Vec<TrackSpec>,
    detections: Vec<Vec<f64>>,
    expected: Vec<Expected>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    stonesoup: String,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/association/jpda.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

#[test]
fn jpda_matches_stone_soup_on_every_case() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("JPDA"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.stonesoup, "1.9.1", "the pinned oracle version");
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let settings = JpdaSettings {
            probability_of_detection: case.probability_of_detection,
            probability_of_gate: case.probability_of_gate,
            clutter_density: case.clutter_density,
            // The oracle gates at the chi-square quantile for the gate probability and
            // the measurement dimension, which for 0.99 and two dimensions is this.
            gate: ChiSquareGate::at_99_percent(2),
        };
        let tracks: Vec<TrackPrediction<2>> = case
            .tracks
            .iter()
            .map(|t| TrackPrediction {
                predicted_measurement: SVector::<f64, 2>::new(t.east, t.north),
                innovation_covariance: SMatrix::<f64, 2, 2>::identity() * t.innovation_variance,
            })
            .collect();
        let detections: Vec<SVector<f64, 2>> = case
            .detections
            .iter()
            .map(|d| SVector::<f64, 2>::new(d[0], d[1]))
            .collect();

        let ours =
            jpda(&settings, &tracks, &detections).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            ours.len(),
            case.expected.len(),
            "{}: track count",
            case.name
        );

        for (t, (ours, theirs)) in ours.iter().zip(&case.expected).enumerate() {
            assert!(
                (ours.missed - theirs.missed).abs() < TOL,
                "{} track {t}: missed-detection probability {} vs oracle {}, over {TOL}",
                case.name,
                ours.missed,
                theirs.missed
            );
            assert_eq!(
                ours.detections.len(),
                theirs.detections.len(),
                "{} track {t}: detection count",
                case.name
            );
            for (j, (a, b)) in ours.detections.iter().zip(&theirs.detections).enumerate() {
                assert!(
                    (a - b).abs() < TOL,
                    "{} track {t} detection {j}: {a} vs oracle {b}, over {TOL}",
                    case.name
                );
            }
            let sum = ours.missed + ours.detections.iter().sum::<f64>();
            assert!(
                (sum - 1.0).abs() < 1e-9,
                "{} track {t}: our probabilities summed to {sum}",
                case.name
            );
        }
    }
}

/// The fixture must contain a case where the association is genuinely contested.
/// Without one, the comparison above would pass on a set of scenes where every track
/// had exactly one candidate and JPDA was doing nothing a nearest neighbour could not.
#[test]
fn the_fixture_contains_a_genuinely_ambiguous_case() {
    let fixture = fixture();
    let contested = fixture.cases.iter().any(|case| {
        case.expected.iter().any(|row| {
            row.detections
                .iter()
                .filter(|p| **p > 0.15 && **p < 0.85)
                .count()
                >= 2
        })
    });
    assert!(
        contested,
        "no fixture case leaves a track split between two candidate detections, so the \
         comparison does not exercise what JPDA is for"
    );
}
