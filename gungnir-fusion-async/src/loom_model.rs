// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Gate 4: `loom` model checks over this crate's cross-task state (GAP-061).
//!
//! Compiled only under `--cfg loom`, which is the only configuration in which the `loom`
//! dependency exists at all. The gate runs
//! `cargo test -p gungnir-fusion-async --release --lib -- --nocapture loom_`, so every
//! check below is reached through this module's path and no ordinary unit test can
//! satisfy the gate by being counted -- which is what the gate did 22 times before this
//! module existed (`.github/workflows/loom.yml`, and GAP-061 for the numbers).
//!
//! # What is actually being model-checked
//!
//! This crate's cross-task state is **two channels and nothing else**. There is no
//! `Arc<Mutex<..>>` and there are no atomics in either `gungnir-fusion-async` or
//! `gungnir-tracking-service` (§2.2's channels-by-default rule, held to). The pipeline
//! itself is owned by one task and mutated only between awaits, so its interior is not
//! shared and cannot be raced. The interleavings that exist are therefore exactly:
//!
//! - the ingest task's `out.send(..)` against the consumer's `try_recv` drain, and
//! - the ingest task's **return**, which drops `out`, against that same drain.
//!
//! Each model below runs the **real** [`crate::ingest_with`] -- the same loop, the same
//! branches, the same `FusionPipeline` doing real association and real Kalman updates --
//! on one loom thread, against a consumer on another that is the drain-to-latest
//! protocol `gungnir_tracking_service::LiveTrackingService::poll` runs for real.
//!
//! # What these models do not reach, and why
//!
//! **The inbound channel's idle path.** Every model fills the detection channel and
//! drops the producer's sender *before* the ingest task is spawned, so `try_recv` on it
//! returns a submission or `Disconnected` and never `Empty`. That is deliberate and it
//! is a real limitation: the `Empty` arm is a `try_recv`/back-off spin against a channel
//! that stays open, loom explores schedules in which one thread runs indefinitely, and a
//! model that reached it would exhaust `LOOM_MAX_BRANCHES` instead of reporting
//! anything. So **the race between a producer still submitting and the ingest loop
//! draining is not covered here**; what is covered is every ordering of the outbound
//! side, which is where the snapshot a poller acts on is actually published.
//!
//! **`crossbeam-channel`'s own implementation.** loom sees only operations performed
//! through its own primitives, so the channel underneath these models is
//! `crate::sync`'s model of crossbeam's contract rather than crossbeam. See that
//! module's documentation; the limitation is stated there in full.
//!
//! # Each check can fail, and one of them proves it on every run
//!
//! A model check that cannot fail is worth nothing -- it is the same defect as a gate
//! that model-checks nothing, one level down. [`unbundled_publication_is_caught`] is
//! therefore a **negative** check that is part of the suite permanently: it runs
//! [`assert_epoch_coherent`] -- the very assertion
//! [`snapshot_fields_come_from_one_epoch`] relies on -- against a publisher that sends
//! the snapshot's parts over two channels the way this crate did before GAP-096
//! bundled them, and requires loom to find an interleaving that violates it. Weaken
//! `assert_epoch_coherent` and that check stops passing.
//!
//! **That check is also the evidence that the search is doing the work**, and the
//! numbers are worth writing down, because an unread number is how this gap happened.
//! Run at `LOOM_MAX_PREEMPTIONS=0` it does *not* fire: the single unpreempted schedule
//! happens to be coherent, and the negative check fails. At 1, 2 and 3 it fires. So the
//! epoch skew exists only in schedules where the publisher is preempted between its two
//! sends, and finding it is exploration rather than execution. The interleaving counts
//! move with the same dial -- 1, 9, 36 and 99 executions at preemption bounds 0, 1, 2
//! and 3 -- which is why [`checked_model`] prints the count and refuses a model that
//! explored one.
//!
//! The two positive checks were verified able to fail by hand, and both mutations are
//! recorded where the code they change lives so a reviewer can repeat them: the
//! drain-to-latest mutation in [`poll`], and the check-then-act mutation in
//! `crate::sync`'s `try_recv`. Unlike the skew above, **both of those are caught at
//! every preemption bound including zero**, so they prove the assertions have teeth
//! without needing the search; that distinction is stated rather than blurred.

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use crate::sync::{unbounded, Receiver, Sender, TryRecvError};
use crate::{
    ingest_with, BearingDetection, Detection, FusionPipeline, PipelineSettings, PipelineSnapshot,
    PipelineStats, RetainedBearing, Submission, TimedTrack,
};

/// One degree of azimuth variance, a direction finder's order of accuracy -- the same
/// figure `tests/bearing.rs` uses.
const ONE_DEGREE_VARIANCE_RAD2: f64 =
    (std::f64::consts::PI / 180.0) * (std::f64::consts::PI / 180.0);

/// How many times the consumer polls concurrently with the ingest task before joining
/// it. Bounded, because the consumer's drain is called once per frame by a real caller
/// and because an unbounded poll loop is the same spin the module documentation rules
/// out above. Three is enough for the interleavings that matter: the ingest task emits
/// exactly three snapshots for [`submissions`].
const CONCURRENT_POLLS: usize = 3;

/// The three submissions every model feeds the real pipeline, and the reason for each.
///
/// Measured, not assumed: driving these through `FusionPipeline` directly produces
/// exactly three snapshots out of [`crate::ingest_with`], and this is what each carries.
///
/// | after | tracks | retained | epochs | initiated | bearings retained |
/// |---|---|---|---|---|---|
/// | A: the second position lets the first epoch run | 1 | 0 | 1 | 1 | 0 |
/// | B: the bearing matches nothing and is retained | 1 | 1 | 1 | 1 | 1 |
/// | C: the stream ends and the buffer is flushed | 2 | 1 | 2 | 2 | 1 |
///
/// The shape is chosen so that **the three parts of a `PipelineSnapshot` move at
/// different times**: A moves `tracks` without `retained_bearings`, B moves
/// `retained_bearings` without `tracks`, and C moves `tracks` again. A reader that
/// paired one part of C with another part of A would therefore be detectable, which is
/// what makes [`assert_epoch_coherent`] able to fail at all.
fn submissions() -> Vec<Submission> {
    let position = |t: f64, enu: [f64; 3]| {
        Submission::Position(Detection {
            sensor_id: 1,
            timestamp_s: t,
            measurement: nalgebra::SVector::<f64, 3>::new(enu[0], enu[1], enu[2]),
        })
    };
    vec![
        // Buffered: one reorder horizon (1.0 s) has not elapsed, so no epoch runs and
        // no snapshot is sent for it.
        position(0.0, [1_000.0, 1_000.0, 500.0]),
        // 1.5 s later, so the first detection's horizon has elapsed: epoch 1 runs, a
        // track is initiated, snapshot A is sent. This detection stays buffered.
        position(1.5, [9_000.0, -9_000.0, 500.0]),
        // Inside the horizon of the processed cursor (0.0 s), pointed south-west while
        // the only track sits to the north-east: it gates into nothing, is retained
        // under DN-27 §5 rule 3, and snapshot B is sent.
        Submission::Bearing(BearingDetection {
            sensor_id: 9,
            timestamp_s: 0.5,
            sensor_enu: [0.0, 0.0, 0.0],
            azimuth_rad: (-1.0f64).atan2(-1.0),
            elevation_rad: None,
            azimuth_variance_rad2: ONE_DEGREE_VARIANCE_RAD2,
            elevation_variance_rad2: None,
        }),
    ]
}

/// What the last of those three snapshots says, and therefore what a consumer that has
/// seen the pipeline stop must be holding.
const FINAL_EPOCHS: u64 = 2;
const FINAL_INITIATED: u64 = 2;
const FINAL_TRACKS: usize = 2;
const FINAL_RETAINED: usize = 1;

/// The three fields `LiveTrackingService` replaces wholesale out of one
/// [`PipelineSnapshot`], plus what the consumer has learned about the pipeline's
/// liveness -- the state a real poller carries between polls.
#[derive(Default)]
struct Applied {
    /// The snapshot last applied, held whole -- which is the point: a real consumer
    /// replaces all three of its fields out of one of these.
    last: Option<PipelineSnapshot>,
    /// How many snapshots were applied over the whole execution. **Asserted non-zero**,
    /// because every per-snapshot assertion in [`poll`] is vacuous in an execution that
    /// applied none, and a check that can pass by never looking is this gap's own defect.
    snapshot_count: usize,
    stats: PipelineStats,
    /// Set the first time a poll observes the track channel disconnected, which is
    /// `LiveTrackingService`'s `pipeline_alive = false`.
    saw_pipeline_gone: bool,
    /// `stats.epochs` at that instant. The point of the flush check: the consumer must
    /// not learn the pipeline has stopped while a snapshot it has not taken is still
    /// queued behind that news.
    epochs_when_gone: Option<u64>,
}

/// Three identities that hold at **every instant of one `FusionPipeline`**, and so must
/// hold of every `PipelineSnapshot`, which is a copy of one such instant.
///
/// Each of them relates a *different pair* of the snapshot's three fields, which is why
/// this function is the test of GAP-096's bundling rather than of any one field:
///
/// 1. `retained_bearings` against `stats`: `offer_bearing` pushes exactly one retained
///    bearing per `bearings_retained`, and `expire_bearings` removes exactly as many as
///    it adds to `bearings_expired`; nothing else touches the retained set. So the live
///    set's length is the difference, exactly.
/// 2. `tracks` against `stats`: every live track was initiated once and `initiated`
///    never decreases, so it can never be below the number of live tracks.
/// 3. `tracks` against `stats` again, at the boundary: a track can only be created
///    inside `process_next_epoch`, so there are no tracks before the first epoch.
///
/// **This is the assertion [`unbundled_publication_is_caught`] proves has teeth.**
fn assert_epoch_coherent(snapshot: &PipelineSnapshot) {
    assert_epoch_coherent_parts(
        &snapshot.tracks,
        &snapshot.retained_bearings,
        &snapshot.stats,
    );
}

fn assert_epoch_coherent_parts(
    tracks: &[TimedTrack],
    retained: &[RetainedBearing],
    stats: &PipelineStats,
) {
    let live_by_counter = stats.bearings_retained - stats.bearings_expired;
    assert_eq!(
        retained.len() as u64,
        live_by_counter,
        "retained_bearings and stats came from different instants: the list holds {} \
         bearings while the counters say {} were retained and {} expired",
        retained.len(),
        stats.bearings_retained,
        stats.bearings_expired
    );
    assert!(
        stats.initiated >= tracks.len() as u64,
        "tracks and stats came from different instants: {} live tracks against an \
         initiated count of {}",
        tracks.len(),
        stats.initiated
    );
    assert!(
        stats.epochs > 0 || tracks.is_empty(),
        "tracks and stats came from different instants: {} live tracks before any epoch \
         had been processed",
        tracks.len()
    );
}

/// The drain-to-latest protocol `LiveTrackingService::poll` runs, with the assertions
/// this gate exists to make.
///
/// Deliberately the same shape as the real one, down to `Disconnected` breaking the
/// loop *after* whatever it already drained is kept: taking only the newest snapshot,
/// applying its three fields together, and ageing nothing when there is no new one.
fn poll(rx: &Receiver<PipelineSnapshot>, applied: &mut Applied) {
    let mut latest: Option<PipelineSnapshot> = None;
    loop {
        match rx.try_recv() {
            // **Mutation, run 2026-09-08 to prove these checks can fail.** Replacing
            // this arm with `if latest.is_none() { latest = Some(snapshot) }` -- keep the
            // first snapshot of a drain rather than the newest, which is the ordinary way
            // to get drain-to-latest wrong -- fails both
            // `applied_snapshots_never_regress` ("the consumer finished holding epoch 1
            // rather than the last one") and
            // `flush_snapshot_survives_the_producers_disconnect` ("learned the pipeline
            // had stopped while holding epoch Some(1)"). It fails at every preemption
            // bound from 0 to 3: the defect is in the drain and not in the schedule, so
            // the assertions catch it without the search having to find anything.
            Ok(snapshot) => latest = Some(snapshot),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                applied.saw_pipeline_gone = true;
                break;
            }
        }
    }
    if let Some(snapshot) = latest {
        assert_epoch_coherent(&snapshot);
        assert!(
            snapshot.stats.epochs >= applied.stats.epochs,
            "the picture went backwards: epoch {} applied after epoch {}",
            snapshot.stats.epochs,
            applied.stats.epochs
        );
        assert!(
            snapshot.stats.accepted >= applied.stats.accepted,
            "the picture went backwards: {} detections accepted after {}",
            snapshot.stats.accepted,
            applied.stats.accepted
        );
        applied.stats = snapshot.stats;
        applied.last = Some(snapshot);
        applied.snapshot_count += 1;
    }
    if applied.saw_pipeline_gone && applied.epochs_when_gone.is_none() {
        applied.epochs_when_gone = Some(applied.stats.epochs);
    }
}

/// Run the real ingest task against a real consumer, under one loom execution, and hand
/// the caller what the consumer ended up holding.
///
/// The producer's sender is dropped before the task is spawned -- see the module
/// documentation for why the inbound idle path is out of reach.
fn drive() -> Applied {
    let (detection_tx, detection_rx) = unbounded::<Submission>();
    let (track_tx, track_rx) = unbounded::<PipelineSnapshot>();

    for submission in submissions() {
        assert!(
            detection_tx.send(submission).is_ok(),
            "the ingest task has not been spawned yet, so the receiver cannot be gone"
        );
    }
    drop(detection_tx);

    let ingest = loom::thread::spawn(move || {
        loom::future::block_on(ingest_with(
            detection_rx,
            track_tx,
            PipelineSettings::default(),
        ));
    });

    let mut applied = Applied::default();
    for _ in 0..CONCURRENT_POLLS {
        poll(&track_rx, &mut applied);
        loom::thread::yield_now();
    }
    assert!(ingest.join().is_ok(), "the ingest task panicked");
    // The task has returned and dropped its sender, so this poll drains whatever is left
    // and then sees the disconnect. Everything raced above; this is where it is judged.
    poll(&track_rx, &mut applied);
    applied
}

/// `loom::model`, plus the number a gate that model-checks nothing cannot produce: a
/// count of the interleavings actually explored.
///
/// **This is GAP-061's own lesson applied one level down.** The workflow's execution
/// check counts *tests*, and a test that ran a single interleaving would satisfy it while
/// checking almost nothing -- the same shape as the four ordinary unit tests that
/// satisfied this gate 22 times. So each model prints its count, the count is asserted to
/// be more than one here, and `loom.yml` sums the printed counts and fails a run whose
/// total is zero. A reader of the log can see the number rather than infer it from a
/// green tick.
///
/// The counter is a real `std` atomic on purpose: it is incremented from inside a loom
/// execution but is not part of what is being modelled, and loom's own atomics are only
/// meaningful within one execution.
fn checked_model(
    name: &str,
    executions: &'static AtomicUsize,
    model: impl Fn() + Sync + Send + 'static,
) {
    executions.store(0, AtomicOrdering::Relaxed);
    loom::model(move || {
        executions.fetch_add(1, AtomicOrdering::Relaxed);
        model();
    });
    let explored = executions.load(AtomicOrdering::Relaxed);
    println!("loom: {name} explored {explored} interleavings");
    assert!(
        explored > 1,
        "{name} ran {explored} execution(s): a model that explores one interleaving \
         is not a model check (GAP-061)"
    );
}

/// **Every snapshot a consumer acts on describes one instant of the pipeline**, under
/// every interleaving of the ingest task's sends against the consumer's drain.
///
/// This is what `PipelineSnapshot` exists for (GAP-096): before it, a poller drained
/// `tracks` and the retained bearings and the counters from separate channels and could
/// pair up parts of different epochs -- a picture showing two tracks beside a counter
/// saying one had ever been initiated. [`unbundled_publication_is_caught`] below is that
/// same assertion run against that same shape, and it fails, which is the evidence that
/// this check is not vacuous.
#[test]
fn snapshot_fields_come_from_one_epoch() {
    static EXECUTIONS: AtomicUsize = AtomicUsize::new(0);
    checked_model("snapshot_fields_come_from_one_epoch", &EXECUTIONS, || {
        let applied = drive();
        assert!(
            applied.snapshot_count > 0,
            "no snapshot was applied in this execution, so `poll`'s per-snapshot \
             coherence assertions checked nothing"
        );
        let Some(last) = applied.last.as_ref() else {
            panic!("a snapshot was applied, so one must be held")
        };
        assert_epoch_coherent(last);
    });
}

/// **The operator's picture never goes backwards.**
///
/// `LiveTrackingService::poll` drains to the newest snapshot and replaces its state
/// wholesale, so applying a stale one would rewind the picture -- tracks reappearing at
/// old positions, counters decreasing -- rather than merely delaying it. The assertions
/// are inside [`poll`], on every poll of every interleaving; this check drives them.
///
/// **Verified able to fail** by the drain-to-latest mutation recorded in [`poll`]: the
/// consumer then finishes holding epoch 1 rather than epoch 2, and this assertion says
/// so by number.
#[test]
fn applied_snapshots_never_regress() {
    static EXECUTIONS: AtomicUsize = AtomicUsize::new(0);
    checked_model("applied_snapshots_never_regress", &EXECUTIONS, || {
        let applied = drive();
        assert_eq!(
            applied.stats.epochs, FINAL_EPOCHS,
            "the consumer finished holding epoch {} rather than the last one",
            applied.stats.epochs
        );
    });
}

/// **The end of the stream is a flush and not a truncation, all the way to the
/// consumer.**
///
/// `ingest_with` processes whatever is still inside the reorder horizon when its inbound
/// channel closes and emits a final snapshot before stopping. That guarantee is worth
/// nothing if the consumer can learn the pipeline has stopped *before* it takes that
/// snapshot: `LiveTrackingService` sets `pipeline_alive = false` on the disconnect and
/// reports `is_healthy() == false` from then on, so a truncation there would freeze the
/// picture one epoch short of the recording it replayed and say the pipeline was gone as
/// the reason.
///
/// The check is therefore on the *instant* the consumer learned it: at that point it
/// must already hold the flush snapshot. This is a real race -- the ingest task's return
/// drops the outbound sender while snapshots it sent may still be queued -- and loom
/// explores both sides of it.
///
/// **Verified able to fail**, twice, on 2026-09-08:
/// - keeping the first snapshot of a drain instead of the newest -- the mutation
///   recorded in [`poll`] -- leaves the consumer holding epoch 1 at the instant it
///   learns the pipeline has stopped;
/// - taking `crate::sync`'s modelled `try_recv` out of one critical section, so that the
///   queue is found empty under the lock and the sender count is read after releasing
///   it, turns the pair into a check-then-act and loses the flush snapshot. That is the
///   ordering `sync.rs` says is the whole contract, and this is the check that holds it
///   to it.
///
/// Both fail at every preemption bound from 0 to 3, so neither is evidence about the
/// breadth of the search; [`unbundled_publication_is_caught`] is what carries that.
#[test]
fn flush_snapshot_survives_the_producers_disconnect() {
    static EXECUTIONS: AtomicUsize = AtomicUsize::new(0);
    checked_model(
        "flush_snapshot_survives_the_producers_disconnect",
        &EXECUTIONS,
        || {
            let applied = drive();
            assert!(
                applied.saw_pipeline_gone,
                "the ingest task returned and dropped its sender, so the consumer's last \
             poll must have seen the channel disconnected"
            );
            assert_eq!(
                applied.epochs_when_gone,
                Some(FINAL_EPOCHS),
                "the consumer learned the pipeline had stopped while holding epoch {:?}, \
             with the flush snapshot still queued behind that news",
                applied.epochs_when_gone
            );
            let Some(last) = applied.last.as_ref() else {
                panic!("the flush snapshot must have been applied")
            };
            assert_eq!(applied.stats.initiated, FINAL_INITIATED);
            assert_eq!(last.tracks.len(), FINAL_TRACKS);
            assert_eq!(last.retained_bearings.len(), FINAL_RETAINED);
        },
    );
}

/// **The negative check: proof that [`assert_epoch_coherent`] can fail.**
///
/// A check that passes against a deliberately broken interleaving is worth nothing, and
/// this whole gap is that failure mode one level up. So the broken shape is kept, run,
/// and required to be caught -- permanently, rather than as something an author tried
/// once and wrote down.
///
/// The shape is this crate's own, before GAP-096: a publisher that sends `tracks` on one
/// channel and the retained bearings and counters on another, and a poller that drains
/// the two independently and combines whatever it last saw of each. Nothing about it is
/// synthetic -- it is the same `FusionPipeline` and the same three submissions -- and
/// nothing about the failure is deterministic. **Measured**: at
/// `LOOM_MAX_PREEMPTIONS=0` this check does not fire at all, because the one schedule
/// loom runs without preemption happens to be coherent; at 1, 2 and 3 it fires on
/// "2 live tracks against an initiated count of 1". The skew exists only where the
/// publisher is preempted between its two sends, so this is the check that shows the
/// search finding something rather than merely running the code.
///
/// If a future change makes `assert_epoch_coherent` unable to distinguish the two
/// halves, this check stops panicking and the test fails.
#[test]
#[should_panic(expected = "came from different instants")]
fn unbundled_publication_is_caught() {
    loom::model(|| {
        let (track_tx, track_rx) = unbounded::<Vec<TimedTrack>>();
        let (rest_tx, rest_rx) = unbounded::<(Vec<RetainedBearing>, PipelineStats)>();

        let publisher = loom::thread::spawn(move || {
            publish_over_two_channels(&track_tx, &rest_tx);
        });

        let mut tracks: Vec<TimedTrack> = Vec::new();
        let mut rest: (Vec<RetainedBearing>, PipelineStats) =
            (Vec::new(), PipelineStats::default());
        for _ in 0..CONCURRENT_POLLS {
            drain_latest(&track_rx, &mut tracks);
            drain_latest(&rest_rx, &mut rest);
            assert_epoch_coherent_parts(&tracks, &rest.0, &rest.1);
            loom::thread::yield_now();
        }
        assert!(publisher.join().is_ok(), "the publisher panicked");
        drain_latest(&track_rx, &mut tracks);
        drain_latest(&rest_rx, &mut rest);
        assert_epoch_coherent_parts(&tracks, &rest.0, &rest.1);
    });
}

/// `ingest_with`'s loop, with its one bundled send replaced by the two separate sends
/// this crate used before GAP-096 -- the same pipeline, the same submissions, the same
/// points at which a snapshot is published.
fn publish_over_two_channels(
    tracks: &Sender<Vec<TimedTrack>>,
    rest: &Sender<(Vec<RetainedBearing>, PipelineStats)>,
) {
    let mut pipeline = FusionPipeline::new(PipelineSettings::default());
    for submission in submissions() {
        let emit = match submission {
            Submission::Position(detection) => {
                let _ = pipeline.push(detection);
                pipeline.run_ready() > 0
            }
            Submission::Bearing(bearing) => {
                let _ = pipeline.offer_bearing(&bearing);
                pipeline.expire_bearings(bearing.timestamp_s);
                true
            }
        };
        if emit {
            let _ = tracks.send(pipeline.timed_snapshot());
            let _ = rest.send((pipeline.retained_bearings().to_vec(), pipeline.stats()));
        }
    }
    if pipeline.flush() > 0 {
        let _ = tracks.send(pipeline.timed_snapshot());
        let _ = rest.send((pipeline.retained_bearings().to_vec(), pipeline.stats()));
    }
}

/// Drain a channel to its newest message, keeping `held` unchanged when there is none --
/// the same drain-to-latest [`poll`] runs, per channel.
fn drain_latest<T>(rx: &Receiver<T>, held: &mut T) {
    // Both `Empty` and `Disconnected` end the drain here, unlike [`poll`], which has to
    // tell them apart because one of them is news about the pipeline's liveness.
    while let Ok(message) = rx.try_recv() {
        *held = message;
    }
}
