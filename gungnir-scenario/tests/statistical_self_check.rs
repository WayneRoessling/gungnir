// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The verification-capability-table.md §1 row
//! "`scenario` | Ground-truth & sensor simulation".
//!
//! Method: statistical comparison of detection and clutter rates over many trials
//! against the configured parameters. Pass criterion: **empirical rates within 2σ of
//! the configured Pd and clutter rate**. Data source: self-generated -- this row
//! validates the generator against its own configuration, not against an external
//! set, so there is no fixture and no oracle package to install.
//!
//! Two things make this a real check rather than a tautology:
//!
//! * the denominator is recomputed from the scenario plan by
//!   `GeneratedTimeline::detection_opportunities`, not counted while generating. A
//!   generator bug that skipped opportunities would shrink numerator and denominator
//!   together and pass a self-counted test; it fails this one.
//! * the trials are pooled before the 2σ test is applied. A single trial at 2σ fails
//!   about one run in twenty by construction, which would make this a flaky gate
//!   rather than a gate. Pooling `TRIALS` runs tightens the interval by √TRIALS and,
//!   because the seeds are fixed, makes the outcome reproducible.

// Counts here are sample sizes in the tens of thousands, converted to f64 to do
// statistics on them, and converted back only to print. Every such value is far
// inside the exactly-representable integer range of an f64, so these conversions
// are exact; the lints are about the general case, not this one.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use gungnir_scenario::{Scenario, ScenarioGenerator};
use rand::rngs::StdRng;
use rand::SeedableRng;

/// Fixed seeds: this test must give the same answer on every machine and every run.
const SEEDS: [u64; 12] = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233];

/// The row's tolerance, in standard deviations of the pooled estimate.
const SIGMAS: f64 = 2.0;

struct Pooled {
    detections: f64,
    opportunities: f64,
    false_alarms: f64,
    scans: f64,
}

fn pool(pd: f64, clutter_rate: f64) -> Pooled {
    let mut p = Pooled {
        detections: 0.0,
        opportunities: 0.0,
        false_alarms: 0.0,
        scans: 0.0,
    };
    for seed in SEEDS {
        let timeline = ScenarioGenerator::new(StdRng::seed_from_u64(seed))
            .generate(&Scenario::MaritimeClutter { pd, clutter_rate });
        p.detections += timeline.true_detection_count() as f64;
        p.opportunities += timeline.detection_opportunities() as f64;
        p.false_alarms += timeline.false_alarm_count() as f64;
        p.scans += timeline.scan_count() as f64;
    }
    p
}

/// Empirical probability of detection must sit within 2σ of the configured value.
///
/// The configured value is `pd_in_range × (1 − dropout)`; Scenario 2 sets dropout to
/// zero so the two coincide and a failure points at the Pd path alone.
#[test]
fn empirical_pd_matches_configuration() {
    for configured in [0.6_f64, 0.75, 0.9] {
        let p = pool(configured, 1.0);
        assert!(
            p.opportunities > 0.0,
            "no detection opportunities at Pd {configured}"
        );
        let empirical = p.detections / p.opportunities;
        // Binomial standard error of the pooled proportion.
        let sigma = (configured * (1.0 - configured) / p.opportunities).sqrt();
        let deviation = (empirical - configured).abs() / sigma;
        println!(
            "Pd {configured}: empirical {empirical:.6} over {} opportunities, \
             sigma {sigma:.6}, deviation {deviation:.2} sigma",
            p.opportunities as u64
        );
        assert!(
            deviation < SIGMAS,
            "empirical Pd {empirical} is {deviation:.2} sigma from the configured \
             {configured} (limit {SIGMAS})"
        );
    }
}

/// Empirical clutter rate per scan must sit within 2σ of the configured value.
///
/// False-alarm counts are Poisson, so the variance of the pooled total equals its
/// mean and the standard error of the rate is `sqrt(rate / scans)`.
#[test]
fn empirical_clutter_rate_matches_configuration() {
    for configured in [0.5_f64, 1.0, 2.0] {
        let p = pool(0.8, configured);
        assert!(p.scans > 0.0, "no scans at clutter rate {configured}");
        let empirical = p.false_alarms / p.scans;
        let sigma = (configured / p.scans).sqrt();
        let deviation = (empirical - configured).abs() / sigma;
        println!(
            "clutter {configured}: empirical {empirical:.6} over {} scans, \
             sigma {sigma:.6}, deviation {deviation:.2} sigma",
            p.scans as u64
        );
        assert!(
            deviation < SIGMAS,
            "empirical clutter rate {empirical} is {deviation:.2} sigma from the \
             configured {configured} (limit {SIGMAS})"
        );
    }
}

/// The two rates must be independent of each other: raising the clutter rate must not
/// move the measured Pd, and vice versa. A generator that drew both from one stream in
/// a way that coupled them would pass each test above on its own and still be wrong.
#[test]
fn detection_and_clutter_rates_are_independent() {
    let quiet = pool(0.8, 0.1);
    let noisy = pool(0.8, 4.0);
    let pd_quiet = quiet.detections / quiet.opportunities;
    let pd_noisy = noisy.detections / noisy.opportunities;
    let sigma = (0.8 * 0.2 / quiet.opportunities.min(noisy.opportunities)).sqrt();
    let deviation = (pd_quiet - pd_noisy).abs() / sigma;
    println!(
        "Pd at clutter 0.1: {pd_quiet:.6}; at clutter 4.0: {pd_noisy:.6}; \
         deviation {deviation:.2} sigma"
    );
    assert!(
        deviation < 3.0,
        "measured Pd moved by {deviation:.2} sigma when only the clutter rate changed"
    );
}

/// Every false alarm must be unattributable and every true detection attributable:
/// this is the association ground truth the JPDA and MHT rows are scored against, and
/// it is worthless if the labelling is wrong.
#[test]
fn truth_attribution_is_consistent() {
    let timeline =
        ScenarioGenerator::new(StdRng::seed_from_u64(4)).generate(&Scenario::MaritimeClutter {
            pd: 0.8,
            clutter_rate: 1.5,
        });
    let entity_ids: Vec<&str> = timeline.entities.iter().map(|e| e.id.as_str()).collect();
    let mut attributed = 0;
    for observation in &timeline.observations {
        if let Some(id) = &observation.truth_entity {
            assert!(
                entity_ids.contains(&id.as_str()),
                "observation attributed to unknown entity {id}"
            );
            attributed += 1;
        }
    }
    assert_eq!(attributed, timeline.true_detection_count());
    assert_eq!(
        attributed + timeline.false_alarm_count(),
        timeline.observations.len()
    );
    assert!(
        timeline.false_alarm_count() > 0,
        "the clutter scenario produced no clutter"
    );
}
