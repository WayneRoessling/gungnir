// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The `fusion-async` row of `docs/verification-capability-table.md` §1:
//! "Out-of-sequence handling, multi-rate fusion".
//!
//! Method, verbatim: *replay fixed multi-sensor timeline through async pipeline vs.
//! offline batch*. Criterion: **state convergence within 1e-4**; latency not compared.
//! Data source: a recorded/synthetic fixed timeline, which is the one built here.
//!
//! # What makes this a real test of out-of-sequence handling
//!
//! The arrival order is not shuffled arbitrarily. Each sensor is given its own fixed
//! latency, and detections reach the pipeline in **receipt-time order**, which is what
//! a deployment actually sees: the slow sensor's measurement of an earlier instant
//! arrives after the fast sensor's measurement of a later one. With three sensors at
//! 0.2 s, 0.7 s and 0.9 s of latency, on 1 s, 2 s and 3 s rates at three different
//! phases, the inbound stream is genuinely interleaved out of order: a pipeline that
//! filtered detections as they arrived would repeatedly fold a measurement of an
//! earlier instant into an estimate it had already carried past that instant.
//!
//! The batch half sorts the same detections by source time and runs them through the
//! same [`FusionPipeline`], so a disagreement is the ordering and the buffering rather
//! than two implementations of the mathematics.
//!
//! **The equality is checked to be non-vacuous.** Two runs that both dropped the same
//! detections would agree perfectly and prove nothing, so the test asserts that the
//! async run refused nothing (`too_late == 0`) and accepted every detection, and a
//! second test proves the refusal path works by pushing a detection past the horizon.

use gungnir_fusion_async::{
    ingest_with, run_batch, Detection, FusionPipeline, PipelineSettings, Submission,
};
use gungnir_track::{Track, TrackStatus};
use nalgebra::SVector;

/// Sensor latencies, seconds. Wider than the 1 s reorder horizon would be a
/// misconfiguration; these sit inside it, which is the case the horizon exists for.
const LATENCY_S: [f64; 3] = [0.2, 0.7, 0.9];

/// A scan index as seconds. Exact for every count this file uses, and written as a
/// conversion rather than a cast so the lint has nothing to warn about.
fn seconds(scan: u64) -> f64 {
    f64::from(u32::try_from(scan).unwrap_or(u32::MAX))
}

/// Deterministic, reproducible measurement noise: a small integer hash rather than a
/// random number generator, so the fixture is identical on every machine and no
/// dev-dependency is added for it.
fn jitter(seed: u64) -> f64 {
    let mut x = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    // Map into [-15, 15] metres: well inside the 20 m one-sigma the settings assume.
    (f64::from(u32::try_from(x % 1_000).unwrap_or(0)) / 1_000.0 - 0.5) * 30.0
}

/// Three sensors watching two targets for twenty seconds, each on its own rate **and
/// its own phase**.
///
/// The phases matter. Unsynchronised sensors are the ordinary case, and they are also
/// what makes this a multi-rate test rather than a multi-sensor one: each scan is one
/// sensor's, so the pipeline predicts between scans at irregular intervals rather than
/// receiving everything on a common tick. Simultaneous observation of one target by
/// two sensors is a different problem and needs track-to-track fusion, which is
/// GAP-013 and not in this pipeline; `two_sensors_at_one_instant_make_two_tracks`
/// below pins what the pipeline does instead, so the limitation is recorded rather
/// than hidden by a timeline chosen to avoid it.
fn timeline() -> Vec<Detection> {
    // Period and phase per sensor, seconds.
    const SCHEDULE: [(f64, f64); 3] = [(1.0, 0.0), (2.0, 0.37), (3.0, 0.71)];
    let mut detections = Vec::new();
    for (sensor, (period, phase)) in SCHEDULE.iter().enumerate() {
        let mut scan = 0u64;
        loop {
            let t = phase + period * seconds(scan);
            if t > 20.0 {
                break;
            }
            // Target A east-bound, target B west-bound, a kilometre apart in north so
            // they cross in one axis without ever being the same object.
            let targets = [
                [200.0 * t, 4_000.0, 1_200.0],
                [4_000.0 - 200.0 * t, 5_000.0, 1_200.0],
            ];
            for (target, position) in targets.iter().enumerate() {
                let seed = scan * 97 + sensor as u64 * 13 + target as u64;
                detections.push(Detection {
                    sensor_id: u32::try_from(sensor).expect("three sensors"),
                    timestamp_s: t,
                    measurement: SVector::<f64, 3>::new(
                        position[0] + jitter(seed),
                        position[1] + jitter(seed + 1),
                        position[2] + jitter(seed + 2),
                    ),
                });
            }
            scan += 1;
        }
    }
    detections
}

/// The order the pipeline actually sees them in: by receipt time, which is source time
/// plus that sensor's latency.
fn arrival_order(detections: &[Detection]) -> Vec<Detection> {
    let mut arrivals: Vec<Detection> = detections.to_vec();
    arrivals.sort_by(|a, b| {
        let ra = a.timestamp_s + LATENCY_S[a.sensor_id as usize];
        let rb = b.timestamp_s + LATENCY_S[b.sensor_id as usize];
        ra.total_cmp(&rb)
    });
    arrivals
}

fn compare(async_tracks: &[Track], batch_tracks: &[Track]) {
    assert_eq!(
        async_tracks.len(),
        batch_tracks.len(),
        "the two paths produced different track counts:\nasync {async_tracks:#?}\nbatch {batch_tracks:#?}"
    );
    for expected in batch_tracks {
        let found = async_tracks
            .iter()
            .find(|t| t.id == expected.id)
            .unwrap_or_else(|| panic!("the async path has no track {:?}", expected.id));
        assert_eq!(found.status, expected.status, "status of {:?}", expected.id);
        let worst = (found.state - expected.state).abs().max();
        assert!(
            worst < 1e-4,
            "track {:?} state differs by {worst}, over the row's 1e-4:\nasync {}\nbatch {}",
            expected.id,
            found.state.transpose(),
            expected.state.transpose()
        );
    }
}

/// The row. Out-of-order arrival converges on the offline batch.
#[tokio::test(flavor = "multi_thread")]
async fn out_of_order_arrival_converges_on_the_offline_batch() {
    let settings = PipelineSettings::default();
    let detections = timeline();
    assert!(
        detections.len() > 40,
        "the timeline is substantial: {} detections",
        detections.len()
    );

    let batch = run_batch(settings.clone(), &detections);
    assert_eq!(batch.len(), 2, "two targets, two tracks: {batch:#?}");
    assert!(
        batch.iter().all(|t| t.status == TrackStatus::Confirmed),
        "both tracks confirm in the batch run: {batch:#?}"
    );

    let (detection_tx, detection_rx) = crossbeam_channel::unbounded::<Submission>();
    let (track_tx, track_rx) = crossbeam_channel::unbounded::<Vec<Track>>();
    let task = tokio::spawn(ingest_with(detection_rx, track_tx, settings));
    for detection in arrival_order(&detections) {
        detection_tx
            .send(Submission::Position(detection))
            .expect("the pipeline is running");
    }
    // Closing the inbound channel is the end of the stream: the task flushes what is
    // still inside the horizon and emits a final snapshot before it stops.
    drop(detection_tx);
    task.await.expect("the ingest task ran to completion");

    let mut last = Vec::new();
    while let Ok(snapshot) = track_rx.try_recv() {
        last = snapshot;
    }
    assert!(!last.is_empty(), "the async path emitted a final snapshot");
    compare(&last, &batch);
}

/// The equality above must not come from both paths losing the same data. Driven
/// directly so the buffer's own counters can be read.
#[test]
fn the_async_ordering_refuses_nothing_inside_the_horizon() {
    let detections = timeline();
    let mut pipeline = FusionPipeline::new(PipelineSettings::default());
    for detection in arrival_order(&detections) {
        pipeline
            .push(detection)
            .expect("every arrival is inside the reorder horizon");
        pipeline.run_ready();
    }
    pipeline.flush();
    let stats = pipeline.stats();
    assert_eq!(stats.too_late, 0, "nothing was refused: {stats:?}");
    assert_eq!(
        stats.accepted,
        u64::try_from(detections.len()).expect("count fits"),
        "every detection entered the buffer: {stats:?}"
    );
    assert!(stats.associated > 0 && stats.initiated >= 2, "{stats:?}");
}

/// A detection later than the horizon is refused and counted, which is what makes the
/// counter above meaningful. The pipeline never folds a stale measurement in.
#[test]
fn a_detection_beyond_the_horizon_is_refused_rather_than_folded_in() {
    let settings = PipelineSettings::default();
    let mut pipeline = FusionPipeline::new(settings);
    for scan in 0..8u64 {
        let t = seconds(scan);
        pipeline
            .push(Detection {
                sensor_id: 0,
                timestamp_s: t,
                measurement: SVector::<f64, 3>::new(100.0 * t, 0.0, 500.0),
            })
            .expect("in order");
        pipeline.run_ready();
    }
    let before = pipeline.snapshot();
    let late = pipeline.push(Detection {
        sensor_id: 1,
        timestamp_s: 1.0,
        measurement: SVector::<f64, 3>::new(100.0, 0.0, 500.0),
    });
    assert!(late.is_err(), "a two-scan-old detection is refused");
    assert_eq!(pipeline.stats().too_late, 1);
    let after = pipeline.snapshot();
    assert_eq!(
        before.len(),
        after.len(),
        "the refused detection changed nothing"
    );
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(a.state, b.state, "no estimate moved");
    }
}

/// **A recorded limitation, not a passing behaviour.** Two sensors reporting the same
/// target at the same instant produce two tracks: the pipeline associates a scan's
/// detections to distinct tracks one-to-one, so the second sensor's view of a target
/// cannot be assigned to the track the first sensor's view already took, and it starts
/// its own. Reconciling them is track-to-track fusion across platforms, which is
/// GAP-013 and is not in this pipeline.
///
/// This test exists so that the count is a decision on the record rather than a
/// surprise, and so that GAP-013 landing has a test that must change.
#[test]
fn two_sensors_at_one_instant_make_two_tracks_until_track_fusion_lands() {
    let settings = PipelineSettings::default();
    let mut detections = Vec::new();
    for scan in 0..8u64 {
        let t = seconds(scan);
        for sensor in 0..2u32 {
            detections.push(Detection {
                sensor_id: sensor,
                timestamp_s: t,
                measurement: SVector::<f64, 3>::new(
                    100.0 * t + jitter(scan * 7 + u64::from(sensor)),
                    0.0,
                    500.0,
                ),
            });
        }
    }
    let tracks = run_batch(settings, &detections);
    assert_eq!(
        tracks.len(),
        2,
        "one target seen by two sensors is two tracks without GAP-013: {tracks:#?}"
    );
}
