// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-114: the late-data policy is applied, one variant at a time.
//!
//! The `gungnir-time` row "Late-data policy" in `docs/verification-capability-table.md`
//! §1 names the method: deliver late data under each `LateDataPolicy`. GAP-114's closing
//! action states what each must do, and each test below is one of its clauses:
//!
//! - `Reject` drops an out-of-order detection and counts it;
//! - `BufferAndReorder { max_lateness_s }` reorders one inside the bound and drops one
//!   beyond it -- beyond the bound itself, not merely behind the processed cursor;
//! - `AcceptAsIs` processes one as delivered.
//!
//! Every detection is driven as the ingest loop drives it (`push`, then `run_ready`), and
//! a refusal is checked to have changed no estimate: a counter that went up while a track
//! moved anyway would be the silent fold-in the policy exists to prevent. The row itself
//! is the owner's to walk; these are the tests that walk would read.

use gungnir_fusion_async::{
    ingest_with, run_batch, BearingDetection, BearingOutcome, BearingRefusal, Detection,
    FusionPipeline, InvalidLateData, LateDataPolicy, PipelineSettings, PipelineSnapshot, PushError,
    Submission, MIN_BEARING_WINDOW_S,
};
use gungnir_track::Track;
use nalgebra::SVector;

/// One target moving east at 100 m/s, as one sensor reports it at `t`.
fn detection(t: f64) -> Detection {
    Detection {
        sensor_id: 1,
        timestamp_s: t,
        measurement: SVector::<f64, 3>::new(100.0 * t, 0.0, 500.0),
    }
}

fn settings(policy: LateDataPolicy) -> PipelineSettings {
    PipelineSettings::default()
        .with_late_data(policy)
        .expect("a valid policy")
}

/// Push and process as `gungnir_fusion_async::ingest_with` does for every position.
fn deliver(pipeline: &mut FusionPipeline, t: f64) -> Result<(), PushError> {
    let result = pipeline.push(detection(t));
    pipeline.run_ready();
    result
}

fn states(tracks: &[Track]) -> Vec<SVector<f64, 6>> {
    tracks.iter().map(|t| t.state).collect()
}

/// **`Reject` drops and counts.** Nothing is held, so each detection is processed as it
/// arrives; the one delivered after a later one is refused, counted as too late, and
/// moves nothing.
#[test]
fn reject_drops_an_out_of_order_detection_and_counts_it() {
    let mut pipeline = FusionPipeline::new(settings(LateDataPolicy::Reject));
    for k in 0..=6 {
        deliver(&mut pipeline, f64::from(k)).expect("in order");
        // Nothing is held: every detection is its own epoch the moment it arrives.
        assert_eq!(pipeline.buffered(), 0, "Reject holds nothing");
        assert_eq!(
            pipeline.stats().epochs,
            u64::try_from(k + 1).expect("small")
        );
    }
    let before = pipeline.snapshot();

    assert_eq!(deliver(&mut pipeline, 4.5), Err(PushError::TooLate));

    let stats = pipeline.stats();
    assert_eq!(stats.too_late, 1, "{stats:?}");
    assert_eq!(stats.accepted, 7, "the late one was not taken: {stats:?}");
    assert_eq!((stats.reordered, stats.accepted_late), (0, 0), "{stats:?}");
    assert_eq!(stats.epochs, 7, "no epoch ran for it: {stats:?}");
    assert_eq!(
        states(&before),
        states(&pipeline.snapshot()),
        "the refused detection moved no estimate"
    );
    // Not late: a detection at the newest instant already taken is in order.
    deliver(&mut pipeline, 6.0).expect("the same instant is not out of order");
    assert_eq!(pipeline.stats().too_late, 1);
}

/// **`BufferAndReorder` reorders inside the bound.** A detection that arrives after a
/// later one, but no more than `max_lateness_s` behind it, is put back in source-time
/// order: the run ends on exactly the answer the offline batch gives for the same
/// detections sorted, and the reordering is counted.
#[test]
fn buffer_and_reorder_puts_a_detection_inside_the_bound_back_in_order() {
    let policy = LateDataPolicy::BufferAndReorder {
        max_lateness_s: 2.0,
    };
    // 0..=10, with 3 delivered after 4: one second behind the front, inside two.
    let arrival = [0.0, 1.0, 2.0, 4.0, 3.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    let mut pipeline = FusionPipeline::new(settings(policy));
    for t in arrival {
        deliver(&mut pipeline, t).expect("every arrival is inside the bound");
    }
    pipeline.flush();
    let stats = pipeline.stats();
    assert_eq!(stats.reordered, 1, "{stats:?}");
    assert_eq!((stats.too_late, stats.accepted_late), (0, 0), "{stats:?}");
    assert_eq!(stats.accepted, 11, "{stats:?}");

    let sorted: Vec<Detection> = arrival.iter().map(|t| detection(*t)).collect();
    let batch = run_batch(settings(policy), &sorted);
    assert_eq!(batch.len(), 1, "one target, one track: {batch:?}");
    assert_eq!(
        states(&pipeline.snapshot()),
        states(&batch),
        "reordered inside the bound, the answer is the in-order answer exactly"
    );
}

/// **`BufferAndReorder` drops beyond the bound**, and the bound is what decides it.
///
/// The late detection here is *ahead* of the processed cursor: nothing arrived between
/// the first detection and the jump, so no epoch moved the cursor past it. Before GAP-114
/// only the cursor was checked, so this detection -- two and a half seconds late under a
/// two-second policy -- was taken, and whether a late detection survived depended on
/// whether other traffic happened to arrive. Now the policy's bound refuses it.
#[test]
fn buffer_and_reorder_drops_a_detection_beyond_the_bound() {
    let mut pipeline = FusionPipeline::new(settings(LateDataPolicy::BufferAndReorder {
        max_lateness_s: 2.0,
    }));
    deliver(&mut pipeline, 0.0).expect("first");
    deliver(&mut pipeline, 5.0).expect("in order");
    // t = 0 has ripened and been processed; t = 5 is held. The cursor is at 0.
    assert_eq!(pipeline.stats().epochs, 1);
    assert_eq!(pipeline.buffered(), 1);
    let before = pipeline.snapshot();

    assert_eq!(
        deliver(&mut pipeline, 2.5),
        Err(PushError::TooLate),
        "2.5 s behind the front under a 2 s bound, although ahead of the cursor"
    );
    assert_eq!(pipeline.stats().too_late, 1);
    assert_eq!(pipeline.buffered(), 1, "nothing was added to the buffer");
    assert_eq!(states(&before), states(&pipeline.snapshot()));

    // The same shape one second closer is inside the bound, and is reordered.
    deliver(&mut pipeline, 3.5).expect("1.5 s behind the front, inside 2 s");
    let stats = pipeline.stats();
    assert_eq!((stats.too_late, stats.reordered), (1, 1), "{stats:?}");

    // And behind the cursor is refused whatever the bound would say.
    pipeline.flush();
    assert_eq!(deliver(&mut pipeline, 4.9), Err(PushError::TooLate));
    assert_eq!(pipeline.stats().too_late, 2);
}

/// **`AcceptAsIs` processes as delivered.** The late detection is taken, applied to the
/// estimate as it stands -- at the newest time already taken, never retrodicted to its
/// own -- and counted, so a replay that chose this policy says so in its counters.
///
/// The target is slow (5 m/s) so the stale measurement still gates into its track; a
/// fast one would miss the gate and start a ghost track instead, which is the corruption
/// the policy's own documentation warns of, and not this test's subject.
#[test]
fn accept_as_is_processes_a_late_detection_as_delivered() {
    let slow = |t: f64| Detection {
        sensor_id: 1,
        timestamp_s: t,
        measurement: SVector::<f64, 3>::new(5.0 * t, 0.0, 500.0),
    };
    let mut pipeline = FusionPipeline::new(settings(LateDataPolicy::AcceptAsIs));
    for k in 0..=6 {
        pipeline.push(slow(f64::from(k))).expect("in order");
        pipeline.run_ready();
    }
    assert_eq!(pipeline.buffered(), 0, "AcceptAsIs holds nothing");
    let before = pipeline.timed_snapshot();
    assert_eq!(before.len(), 1);

    pipeline
        .push(slow(4.5))
        .expect("AcceptAsIs refuses nothing for lateness");
    pipeline.run_ready();

    let stats = pipeline.stats();
    assert_eq!(stats.accepted_late, 1, "{stats:?}");
    assert_eq!((stats.too_late, stats.reordered), (0, 0), "{stats:?}");
    assert_eq!(stats.accepted, 8, "{stats:?}");
    assert_eq!(
        stats.epochs, 8,
        "it was processed as its own epoch: {stats:?}"
    );
    assert_eq!(stats.associated, 7, "it updated the track: {stats:?}");
    let after = pipeline.timed_snapshot();
    assert_eq!(
        after.len(),
        1,
        "it updated the one track rather than starting one"
    );
    assert_ne!(
        before[0].track.state, after[0].track.state,
        "the estimate moved: the detection was processed"
    );
    assert!(
        (after[0].estimate_time_s - 6.0).abs() < 1e-12,
        "applied at the front, not retrodicted to 4.5: {}",
        after[0].estimate_time_s
    );
}

/// The counters reach the snapshot the ingest task publishes, which is how the
/// tracking service, the node's wire view and the health panel read them.
#[tokio::test(flavor = "multi_thread")]
async fn the_ingest_task_applies_the_policy_and_publishes_its_counters() {
    let (detection_tx, detection_rx) = crossbeam_channel::unbounded::<Submission>();
    let (snapshot_tx, snapshot_rx) = crossbeam_channel::unbounded::<PipelineSnapshot>();
    let task = tokio::spawn(ingest_with(
        detection_rx,
        snapshot_tx,
        settings(LateDataPolicy::Reject),
    ));
    for t in [0.0, 1.0, 2.0, 3.0, 1.5, 4.0] {
        detection_tx
            .send(Submission::Position(detection(t)))
            .expect("the pipeline is running");
    }
    drop(detection_tx);
    task.await.expect("the ingest task ran to completion");
    let mut last = None;
    while let Ok(snapshot) = snapshot_rx.try_recv() {
        last = Some(snapshot);
    }
    let stats = last.expect("the task published").stats;
    assert_eq!((stats.accepted, stats.too_late), (5, 1), "{stats:?}");
}

/// A bound the pipeline cannot honour is refused, never replaced by one of its own.
#[test]
fn a_bound_that_is_not_a_positive_number_of_seconds_is_refused() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let err = PipelineSettings::default()
            .with_late_data(LateDataPolicy::BufferAndReorder {
                max_lateness_s: bad,
            })
            .expect_err("refused");
        let InvalidLateData { max_lateness_s } = err;
        assert_eq!(
            max_lateness_s.to_bits(),
            bad.to_bits(),
            "the refusal names the value: {err}"
        );
    }
    // Reject and AcceptAsIs carry no number to refuse; the pipeline takes both (it is
    // `gungnir-config` that keeps AcceptAsIs out of a deployment's baseline).
    assert!(settings(LateDataPolicy::Reject).reorder_horizon_s().abs() < f64::EPSILON);
    assert!(
        settings(LateDataPolicy::AcceptAsIs)
            .reorder_horizon_s()
            .abs()
            < f64::EPSILON
    );
}

fn bearing(t: f64) -> BearingDetection {
    BearingDetection {
        sensor_id: 9,
        timestamp_s: t,
        sensor_enu: [0.0, -5_000.0, 0.0],
        // Due north of the sensor, where the target is at x = 0.
        azimuth_rad: 0.0,
        elevation_rad: None,
        azimuth_variance_rad2: 1e-4,
        elevation_variance_rad2: None,
    }
}

/// Under `Reject` a bearing behind the cursor is late and is refused as such; the window
/// bearings are otherwise judged in keeps its floor, so a current bearing is still used.
#[test]
fn reject_refuses_a_late_bearing_and_keeps_the_bearing_window() {
    let reject = settings(LateDataPolicy::Reject);
    assert!((reject.bearing_window_s() - MIN_BEARING_WINDOW_S).abs() < f64::EPSILON);
    let wide = settings(LateDataPolicy::BufferAndReorder {
        max_lateness_s: 3.0,
    });
    assert!(
        (wide.bearing_window_s() - 3.0).abs() < f64::EPSILON,
        "the window grows with the horizon, or current bearings would be refused"
    );

    let mut pipeline = FusionPipeline::new(reject);
    for k in 0..=5 {
        let t = f64::from(k);
        pipeline
            .push(Detection {
                sensor_id: 1,
                timestamp_s: t,
                measurement: SVector::<f64, 3>::new(0.0, 0.0, 500.0),
            })
            .expect("in order");
        pipeline.run_ready();
    }
    assert_eq!(
        pipeline.offer_bearing(&bearing(4.8)),
        BearingOutcome::Refused(BearingRefusal::Late)
    );
    assert!(
        matches!(
            pipeline.offer_bearing(&bearing(5.0)),
            BearingOutcome::Updated(_)
        ),
        "a bearing at the cursor is not late"
    );
}
