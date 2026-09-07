// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Scenario replay through the live pipeline, scored against the scenario's own ground
//! truth (GAP-045).
//!
//! `whole_pipeline_replay.rs` next to this file answers a different question: it replays
//! the five scenarios through `LiveTrackingService` and compares the emitted stream
//! against the *same pipeline run offline*, which proves the service, its channels, its
//! task and its reorder buffer deliver the same picture as the batch. It cannot say
//! whether that picture is right, because both sides are the same estimator.
//!
//! This file asks whether the picture is right, and the only thing that can answer it is
//! truth the estimator never saw. `gungnir-scenario` generates both: noiseless truth from
//! the `gungnir-core` motion models, and the noisy detections a sensor model produced from
//! it. Replaying the detections and scoring the tracks against the truth is the half of
//! GAP-045 that does not need PN-16.
//!
//! # What this is not
//!
//! It is **not** the whole of GAP-045. The register's remaining work is reconstituting the
//! desktop's picture at a past moment, which is PN-16 (GAP-087) and is not built. Nothing
//! here scrubs a journal or rebuilds `AppState`; it drives detections through the live
//! service in receipt order, which is what a live system sees.
//!
//! # When the comparison is made, and why it has to be said
//!
//! `TrackView::mission_time` is the time the *service was polled*, not the time of the
//! estimate: `project_track` stamps `now`. So the tracks in a final snapshot are estimates
//! at their own last update, carrying the poll's timestamp. Scoring them against truth at
//! the poll time would charge the estimator for the gap between the last detection and the
//! poll -- 970 ms of a 697 m/s aircraft in scenario 1, which is most of the error. The
//! comparison time is therefore the **last observation in the timeline**, which is when
//! the picture was last told anything.
//!
//! # The bounds, and why each one
//!
//! Each bound is below the distance its scenario's targets are apart and above what the
//! sensor noise alone can explain, so "the nearest track to this target" is unambiguous
//! and the assertion is about estimation rather than about labelling.
//!
//! * **Scenario 1, 500 m.** One aircraft, one radar with one-sigma noise of 25 m in range,
//!   60 m across and 150 m in height, scanning once a second. At the comparison instant
//!   the aircraft is in its terminal acceleration phase at 697 m/s, so it moves 697 m
//!   between scans: a bound of 500 m asserts that the estimate is better than dead
//!   reckoning from the previous scan would be, and it is above the 450 m that three sigma
//!   of the worst measurement axis alone can explain. There is one target, so no
//!   ambiguity is possible. Measured on this seed: 238 m.
//! * **Scenario 2, 300 m.** Six vessels at up to 12 m/s, one radar with one-sigma noise of
//!   15 m in range and 60 m across, scanning every 2.5 s. 300 m is five sigma of the worst
//!   measurement axis, and it is under a tenth of the 3.3 km separating the nearest two
//!   vessels at the comparison instant, so the track nearest a vessel cannot be another
//!   vessel's. Measured on this seed: 85 m.
//!
//! Scenarios 3 and 4 are deliberately **not** scored this way. Their targets pass within
//! 120 m and 233 m of each other, which is inside the error the same measurement noise
//! produces, so a nearest-track match there says nothing about which target a track is of.
//! Scoring them needs an assignment against truth, which is the `gungnir-metrics` OSPA and
//! track-purity row rather than this one.
//!
//! # The count criterion, honestly
//!
//! The row asks for the track count to match the target count. **The first half holds and
//! the second does not**, and this file asserts the half that holds rather than widening
//! the criterion to cover the half that does not:
//!
//! * every target alive at the comparison time is accounted for by some track -- no target
//!   is missed, in any of the four scenarios;
//! * the pipeline also carries tracks that are not targets. On this seed scenario 1 ends
//!   with **three tracks for one aircraft, none of them confirmed**, and scenario 2 with
//!   ten tracks for six vessels, two confirmed. The extras are track fragments and
//!   clutter-born tracks that the lifecycle has not deleted.
//!
//! The second bullet is a finding about the pipeline, not about this test, and it is
//! reported rather than asserted: asserting today's counts would pin the behaviour in
//! place, and asserting one-track-per-target would be a test that fails for a defect it
//! does not own.

use gungnir_fusion_async::PipelineSettings;
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId, TrackView};
use gungnir_scenario::{truth, GeneratedTimeline, Scenario, ScenarioGenerator};
use gungnir_tracking_service::{LiveTrackingService, TrackingService};
use nalgebra::Vector3;
use rand::rngs::StdRng;
use rand::SeedableRng;

/// The seed the sibling replay test uses, so both rows read the same five timelines.
const SEED: u64 = 7;

fn view(detection: &gungnir_fusion_async::Detection) -> DetectionView {
    DetectionView {
        sensor: SensorId(detection.sensor_id),
        source_time: MissionTime(detection.timestamp_s),
        receipt_time: MissionTime(detection.timestamp_s),
        measurement: gungnir_model::Measurement::Position {
            enu: detection.measurement,
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

/// Generate a scenario and drive every observation through the live service in receipt
/// order, which is the order a live system sees and is not source order wherever a sensor
/// is late.
///
/// The reorder horizon is set from the timeline's own latency spread rather than left at
/// the default, for the reason the sibling test gives: the horizon is a deployment
/// setting, and one too small for a sensor set is a misconfiguration rather than a defect
/// in the pipeline. Every submission is asserted to be accepted, so a dropped detection
/// fails here rather than showing up later as an error nobody can attribute.
fn replay(scenario: &Scenario) -> (GeneratedTimeline, Vec<TrackView>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let timeline = ScenarioGenerator::new(StdRng::seed_from_u64(SEED)).generate(scenario);
    let spread = timeline
        .observations
        .iter()
        .map(|o| o.receipt_time_s - o.detection.timestamp_s)
        .fold(0.0f64, f64::max);
    let settings = PipelineSettings {
        reorder_horizon_s: spread.mul_add(2.0, 1.0),
        ..PipelineSettings::default()
    };
    let mut service = LiveTrackingService::with_pipeline_settings(runtime.handle(), settings);
    for observation in &timeline.observations {
        service
            .submit_detection(view(&observation.detection))
            .unwrap_or_else(|e| panic!("a detection was dropped: {e}"));
    }
    // End the stream, so the reorder buffer is flushed rather than leaving the last
    // horizon of the replay unprocessed.
    service.finish();

    let mut ended = false;
    // **A deadlock guard, not a performance assertion.** This is the same wait as
    // `sample_set_replay.rs`, on the same background pipeline task, and that one was
    // raised to thirty thousand iterations on the reasoning recorded beside it -- a
    // correctness test failing for want of CPU says nothing about the pipeline. This
    // sibling was left at four thousand and is brought to the same figure.
    for _ in 0..30_000 {
        service.poll(MissionTime(timeline.duration_s));
        if !service.is_healthy() {
            ended = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(ended, "the pipeline task did not finish");
    let tracks = service.tracks().to_vec();
    (timeline, tracks)
}

/// The mission time the comparison is made at: the last observation in the timeline.
///
/// See the module documentation -- a track's `mission_time` is the poll's, not the
/// estimate's, so the poll time would charge the estimator for time it was told nothing in.
fn comparison_time(timeline: &GeneratedTimeline) -> f64 {
    timeline
        .observations
        .iter()
        .map(|o| o.detection.timestamp_s)
        .fold(0.0f64, f64::max)
}

/// Every entity alive at `at`, with its exact ENU position.
///
/// Truth is noiseless and `state_at` marches only at phase boundaries, so this is the
/// entity's position at that instant rather than the nearest tick's.
fn truth_at(timeline: &GeneratedTimeline, at: f64) -> Vec<(&str, Vector3<f64>)> {
    timeline
        .entities
        .iter()
        .filter(|e| e.is_alive_at(at))
        .map(|e| (e.id.as_str(), truth::split(&truth::state_at(e, at)).0))
        .collect()
}

fn position_error(track: &TrackView, truth: Vector3<f64>) -> f64 {
    (Vector3::new(track.state[0], track.state[1], track.state[2]) - truth).norm()
}

/// Score a replay: every target must be accounted for by a track within `bound_m`.
///
/// Returns the worst error, so a caller can print how much headroom the bound has and a
/// reviewer can see the number rather than only that it passed.
fn score(name: &str, timeline: &GeneratedTimeline, tracks: &[TrackView], bound_m: f64) -> f64 {
    let at = comparison_time(timeline);
    let alive = truth_at(timeline, at);
    assert!(
        !alive.is_empty(),
        "{name}: no entity is alive at the comparison time, so nothing was scored"
    );
    assert!(
        !tracks.is_empty(),
        "{name}: the replay produced no tracks at all"
    );

    let mut worst = 0.0f64;
    let mut accounted = 0;
    for (id, position) in &alive {
        let nearest = tracks
            .iter()
            .map(|t| position_error(t, *position))
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest <= bound_m,
            "{name}: the nearest track to target {id} is {nearest:.1} m away at \
             t = {at:.2} s, over the stated bound of {bound_m:.0} m"
        );
        accounted += 1;
        worst = worst.max(nearest);
    }
    // The half of the count criterion that holds: the picture accounts for every target.
    assert_eq!(
        accounted,
        alive.len(),
        "{name}: the picture accounts for {accounted} of {} targets",
        alive.len()
    );
    let confirmed = tracks
        .iter()
        .filter(|t| t.status == gungnir_model::TrackStatus::Confirmed)
        .count();
    println!(
        "{name}: {} target(s) at t = {at:.2} s, all within {worst:.1} m of a track \
         (bound {bound_m:.0} m); the picture carries {} track(s), {confirmed} confirmed",
        alive.len(),
        tracks.len()
    );
    worst
}

/// **Scenario 1 is tracked to within 500 m of truth**: one manoeuvring aircraft, one
/// radar, and no ambiguity about which target a track belongs to.
///
/// The bound and its justification are in the module documentation. The aircraft is in its
/// terminal acceleration phase at the comparison instant, which is the hardest moment in
/// the scenario for a filter whose motion model is not the truth's, and is the moment
/// worth pinning for exactly that reason.
#[test]
fn the_maneuvering_aircraft_is_tracked_to_within_the_stated_bound() {
    let (timeline, tracks) = replay(&Scenario::ManeuveringAircraft);
    let worst = score("scenario 1", &timeline, &tracks, 500.0);
    assert!(
        worst > 0.0,
        "an error of exactly zero means the truth leaked into the estimator"
    );
}

/// **Every vessel in scenario 2 is tracked to within 300 m of truth**, through 20 per cent
/// missed detections, two false alarms a scan and a land mask that hides one vessel
/// entirely for a while.
///
/// This is the multi-target case the bound can be stated for: the vessels are kilometres
/// apart, so a track within 300 m of one of them is that one's.
#[test]
fn every_vessel_in_the_clutter_scenario_is_tracked_to_within_the_stated_bound() {
    let (timeline, tracks) = replay(&Scenario::MaritimeClutter {
        pd: 0.8,
        clutter_rate: 2.0,
    });
    let alive = truth_at(&timeline, comparison_time(&timeline));
    // The bound is only meaningful while the vessels stay far enough apart for a nearest
    // match to be unambiguous, so the separation is checked rather than assumed.
    let mut separation = f64::INFINITY;
    for (i, (_, a)) in alive.iter().enumerate() {
        for (_, b) in alive.iter().skip(i + 1) {
            separation = separation.min((a - b).norm());
        }
    }
    assert!(
        separation > 3_000.0,
        "the vessels are {separation:.0} m apart, so a nearest-track match no longer \
         identifies which vessel a track is of and the 300 m bound says nothing"
    );
    score("scenario 2", &timeline, &tracks, 300.0);
}

/// **No target is missed in any of the four scenarios.**
///
/// The weakest true statement of the count criterion, and the one worth having as a
/// regression guard across the scenarios whose targets are too close together for a
/// position bound to mean anything: whatever else the pipeline does with its track list,
/// every entity alive at the comparison time has a track somewhere near it. The distance
/// used here is deliberately generous -- a kilometre -- because the claim is about
/// coverage rather than accuracy, and the accuracy claims are the two tests above.
#[test]
fn no_target_goes_untracked_in_any_scenario() {
    for (name, scenario) in [
        ("scenario 1", Scenario::ManeuveringAircraft),
        (
            "scenario 2",
            Scenario::MaritimeClutter {
                pd: 0.8,
                clutter_rate: 2.0,
            },
        ),
        (
            "scenario 3",
            Scenario::UrbanConvoy {
                injected_bias_m: Vector3::new(40.0, -25.0, 5.0),
            },
        ),
        ("scenario 4", Scenario::DenseSwarm { target_count: 40 }),
    ] {
        let (timeline, tracks) = replay(&scenario);
        let at = comparison_time(&timeline);
        for (id, position) in truth_at(&timeline, at) {
            let nearest = tracks
                .iter()
                .map(|t| position_error(t, position))
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest <= 1_000.0,
                "{name}: target {id} is untracked -- the nearest track is {nearest:.1} m \
                 away at t = {at:.2} s"
            );
        }
        println!(
            "{name}: {} target(s), {} track(s) -- every target accounted for",
            timeline
                .entities
                .iter()
                .filter(|e| e.is_alive_at(at))
                .count(),
            tracks.len()
        );
    }
}
