// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The dense-group mode (GAP-015): what engages it, what it reports, and -- the load
//! bearing one -- that it changes nothing about the tracks beside it.

use gungnir_association::jpda::MAX_DETECTIONS;
use gungnir_fusion_async::{
    run_batch, DenseGroupFilter, DenseGroupSettings, Detection, FusionPipeline, PipelineSettings,
};
use gungnir_track::{Track, TrackStatus};
use nalgebra::SVector;

fn detection(sensor: u32, t: f64, position: [f64; 3]) -> Detection {
    Detection {
        sensor_id: sensor,
        timestamp_s: t,
        measurement: SVector::<f64, 3>::new(position[0], position[1], position[2]),
    }
}

/// A raid: `count` targets in a line abreast, `spacing` metres apart, all crossing at the
/// same constant velocity, sampled once a second for `scans` scans.
///
/// The spacing is deliberately inside a neighbour's gate: with the default measurement
/// noise the innovation standard deviation is 20 m per axis, so 60 m neighbours sit about
/// three of those apart and every target competes with the one beside it for the same
/// assignment. That is the scene GAP-015's Impact describes.
fn raid(count: usize, spacing: f64, scans: usize) -> Vec<Detection> {
    let mut out = Vec::new();
    for scan in 0..scans {
        #[allow(clippy::cast_precision_loss)]
        let t = scan as f64;
        for target in 0..count {
            #[allow(clippy::cast_precision_loss)]
            let offset = target as f64 * spacing;
            out.push(detection(1, t, [120.0 * t, offset + 30.0 * t, 2_000.0]));
        }
    }
    out
}

/// The mode is off by default (`PipelineSettings::dense_group` records the measured cost
/// that decides that), so every test that wants it says so.
fn with_dense_group() -> PipelineSettings {
    PipelineSettings {
        dense_group: Some(DenseGroupSettings::default()),
        ..PipelineSettings::default()
    }
}

/// Every field of a track that a consumer can observe, so "identical" means identical.
fn fingerprint(tracks: &[Track]) -> Vec<String> {
    let mut out: Vec<String> = tracks
        .iter()
        .map(|t| {
            format!(
                "{:?}|{:?}|{:?}|{}|{}|{:?}",
                t.id, t.status, t.state, t.hits, t.misses_since_update, t.covariance
            )
        })
        .collect();
    out.sort();
    out
}

/// **The load-bearing test.** Over a raid dense enough to engage the mode, the tracks the
/// pipeline produces are identical -- every identifier, status, state, covariance and
/// counter -- whether the dense-group mode is running or switched off entirely.
///
/// The mode adds an output. It is not allowed to change one, and this is the proof rather
/// than the assurance.
#[test]
fn the_dense_group_mode_changes_no_track_the_pipeline_produces() {
    let detections = raid(16, 60.0, 8);

    let with = run_batch(with_dense_group(), &detections);
    let without = run_batch(PipelineSettings::default(), &detections);

    assert_eq!(
        fingerprint(&with),
        fingerprint(&without),
        "the dense-group mode must add output and change none"
    );
    assert!(
        !with.is_empty(),
        "the comparison is worthless if the pipeline produced no tracks at all"
    );
}

/// The same, for the epochs where the mode is present but never engages: a sparse
/// timeline must be bit-for-bit what it was before the mode existed.
#[test]
fn a_sparse_timeline_is_untouched_by_the_modes_presence() {
    let detections = raid(3, 4_000.0, 15);

    let with = run_batch(with_dense_group(), &detections);
    let without = run_batch(PipelineSettings::default(), &detections);

    assert_eq!(fingerprint(&with), fingerprint(&without));
    assert_eq!(with.len(), 3, "three well-separated targets, three tracks");
    assert!(with.iter().all(|t| t.status == TrackStatus::Confirmed));
}

/// The mode engages at the association limit this workspace already states, and not
/// before it. `MAX_DETECTIONS` detections in a scan is resolvable by the exact
/// association; one more is not, and that is where the trigger sits.
#[test]
fn the_mode_engages_past_the_association_limit_and_not_at_it() {
    let at_the_limit = raid(MAX_DETECTIONS, 4_000.0, 4);
    let mut pipeline = FusionPipeline::new(with_dense_group());
    for d in at_the_limit {
        pipeline.push(d).expect("accepted");
    }
    pipeline.flush();
    assert!(
        !pipeline.dense_group_engaged(),
        "a scan at the stated limit is still an association problem"
    );
    assert!(pipeline.dense_group_estimate().is_none());

    let past_the_limit = raid(MAX_DETECTIONS + 1, 4_000.0, 4);
    let mut pipeline = FusionPipeline::new(with_dense_group());
    for d in past_the_limit {
        pipeline.push(d).expect("accepted");
    }
    pipeline.flush();
    assert!(
        pipeline.dense_group_engaged(),
        "a scan past the stated limit is what the mode is for"
    );
    let estimate = pipeline
        .dense_group_estimate()
        .expect("an engaged mode reports its epoch");
    assert_eq!(estimate.scan_size, MAX_DETECTIONS + 1);
    assert_eq!(estimate.filter, DenseGroupFilter::Phd);
    assert_eq!(pipeline.dense_group_refusals(), 0);
}

/// A raid of twenty is counted as about twenty, and the estimate is stamped with the
/// epoch it is of rather than left for a poller to date.
#[test]
fn a_raid_is_counted() {
    let scans = 8;
    let detections = raid(20, 60.0, scans);
    let mut pipeline = FusionPipeline::new(with_dense_group());
    for d in detections {
        pipeline.push(d).expect("accepted");
    }
    pipeline.flush();

    let estimate = pipeline
        .dense_group_estimate()
        .expect("the mode engaged and reported");
    #[allow(clippy::cast_precision_loss)]
    let last_epoch = (scans - 1) as f64;
    assert!(
        (estimate.epoch_s - last_epoch).abs() < 1e-9,
        "the estimate is of the last epoch, not of the poll: {}",
        estimate.epoch_s
    );
    assert!(
        (estimate.expected_targets - 20.0).abs() < 2.0,
        "twenty targets should be counted as about twenty, got {}; a count far below \
         truth is the mixture truncation discarding mass, which is what \
         DenseGroupSettings::phd sizes the cap against",
        estimate.expected_targets
    );
    assert!(!estimate.components.is_empty());
}

/// The mode is off unless a deployment asks for it, and while it is off it is not merely
/// idle -- no filter is built and no estimate is ever produced.
///
/// Asserted because the default is a *decision*, taken on a measured cost
/// (`PipelineSettings::dense_group`), and a default that drifted on silently would put
/// hundreds of milliseconds an epoch onto the `tokio` executor thread of every deployment
/// that never asked for it.
#[test]
fn the_mode_is_off_unless_a_deployment_asks_for_it() {
    assert!(PipelineSettings::default().dense_group.is_none());

    let detections = raid(20, 60.0, 6);
    let mut pipeline = FusionPipeline::new(PipelineSettings::default());
    for d in detections {
        pipeline.push(d).expect("accepted");
    }
    pipeline.flush();
    assert!(!pipeline.dense_group_engaged());
    assert!(pipeline.dense_group_estimate().is_none());
    assert_eq!(pipeline.dense_group_refusals(), 0);
}

/// Selecting the CPHD gets the whole cardinality distribution; the PHD reports a mean and
/// declines to invent a mode.
#[test]
fn the_cphd_selection_reports_a_distribution_and_the_phd_does_not() {
    let detections = raid(14, 60.0, 6);

    let mut phd = FusionPipeline::new(with_dense_group());
    let cphd_settings = PipelineSettings {
        dense_group: Some(DenseGroupSettings {
            filter: DenseGroupFilter::Cphd,
            ..DenseGroupSettings::default()
        }),
        ..PipelineSettings::default()
    };
    let mut cphd = FusionPipeline::new(cphd_settings);
    for d in detections {
        phd.push(d.clone()).expect("accepted");
        cphd.push(d).expect("accepted");
    }
    phd.flush();
    cphd.flush();

    let phd_estimate = phd.dense_group_estimate().expect("engaged");
    assert_eq!(phd_estimate.most_probable_count, None);
    assert!(phd_estimate.count_distribution.is_empty());

    let cphd_estimate = cphd.dense_group_estimate().expect("engaged");
    assert_eq!(cphd_estimate.filter, DenseGroupFilter::Cphd);
    assert!(cphd_estimate.most_probable_count.is_some());
    assert!(!cphd_estimate.count_distribution.is_empty());
    let mass: f64 = cphd_estimate.count_distribution.iter().sum();
    assert!(
        (mass - 1.0).abs() < 1e-6,
        "a cardinality distribution must be one: {mass}"
    );

    // And the tracks are still the tracks, under either selection.
    assert_eq!(fingerprint(&phd.snapshot()), fingerprint(&cphd.snapshot()));
}

/// When the raid ends, the mode is released rather than reporting a group for ever, and
/// the estimate goes with it rather than going stale.
#[test]
fn the_mode_is_released_when_the_group_is_gone() {
    let mut detections = raid(14, 60.0, 6);
    // Then one lone target, far away: the scan is no longer dense, and after the
    // deletion window the filter is released.
    for scan in 6..14 {
        let t = f64::from(scan);
        detections.push(detection(1, t, [500_000.0, 500_000.0, 2_000.0]));
    }
    let mut pipeline = FusionPipeline::new(with_dense_group());
    for d in detections {
        pipeline.push(d).expect("accepted");
    }
    pipeline.flush();

    assert!(
        !pipeline.dense_group_engaged(),
        "the raid is over; the filter must not still be running"
    );
    assert!(
        pipeline.dense_group_estimate().is_none(),
        "a released mode reports nothing, rather than the last group it saw"
    );
    assert_eq!(pipeline.dense_group_refusals(), 0);
}
