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
//! **This used to be a defect and is now a design choice recorded for the same
//! reason.** `TrackView::mission_time` used to be the time the *service was polled*,
//! not the time of the estimate: `project_track` stamped `now` on every projection, so
//! scoring a final snapshot against truth at the poll time charged the estimator for the
//! gap between the last detection and the poll -- 970 ms of a 697 m/s aircraft in
//! scenario 1, which was most of the error. `mission_time` is now
//! `gungnir_fusion_async::TimedTrack::estimate_time_s`, the pipeline's own per-track
//! time, so it already carries the right instant. This file still computes its own
//! `comparison_time` rather than reading `mission_time` off each track, because a single
//! global instant lets `truth_at` be asked once per scenario instead of once per track,
//! and for a target still being hit at the end of the timeline the two answers coincide
//! anyway: the **last observation in the timeline**, which is when the picture was last
//! told anything.
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
//!   ambiguity is possible. **Measured on this seed: 169 m, run with `radar_medium`'s own
//!   measurement noise (DN-30); the figure this row carried before DN-30 (238 m) was run
//!   against `PipelineSettings::default()`'s generic noise, not this scenario's radar.**
//! * **Scenario 2, 300 m.** Six vessels at up to 12 m/s, one radar with one-sigma noise of
//!   15 m in range and 60 m across, scanning every 2.5 s. 300 m is five sigma of the worst
//!   measurement axis, and it is under a tenth of the 3.3 km separating the nearest two
//!   vessels at the comparison instant, so the track nearest a vessel cannot be another
//!   vessel's. **Measured on this seed: 83 m, run with `radar_coastal`'s own range and
//!   cross-range noise (DN-30); its height axis is left at the default's placeholder --
//!   see the test's own documentation for why an exact zero cannot be used here.**
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
//! * the pipeline also carries tracks that are not targets. **On this seed, run with each
//!   single-sensor scenario's own measurement noise (DN-30) rather than the generic
//!   figure these counts were first recorded against**: scenario 1 ends with two tracks
//!   for one aircraft, none confirmed (three before DN-30, over the same scenario and
//!   filter -- most of the fragmentation this row first blamed on the motion model was
//!   the noise mismatch DN-28 §7 found, not a defect DN-30 closes); scenario 2 is
//!   unchanged at ten tracks for six vessels, two confirmed, because its clutter and
//!   dropout rates -- not its measurement noise -- are what drive its extra tracks.
//!   The extras are track fragments and clutter-born tracks that the lifecycle has not
//!   deleted.
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

/// Each single-sensor scenario's own measurement-noise variance, `[sigma_range_m²,
/// sigma_cross_m², sigma_height_m²]` from `gungnir_scenario::sensor::SensorModel`,
/// carried into ENU as the pipeline's `measurement_noise_var` the same way DN-28 §7 did
/// for scenario 1 -- an approximation this module documentation names rather than
/// hides: the sigmas are in the sensor's line-of-sight frame (along/across the
/// boresight), rotated into ENU per detection by the actual bearing, and a fixed triple
/// cannot carry that rotation. DN-30 follows DN-28's own precedent rather than
/// widening scope to a per-detection frame-aware `R`, which is its own future increment.
///
/// Scenario 3 is not here: `plan_urban_convoy` runs three sensors of different kinds
/// (`radar_medium`, `isr_video`, `acoustic`) through one pipeline that has exactly one
/// `measurement_noise_var` for every detection regardless of source, so no single figure
/// is "the" correct one for it. Left at `PipelineSettings::default()` and named as its
/// own open question rather than answered with an invented number.
const RADAR_MEDIUM_MEASUREMENT_NOISE_VAR: [f64; 3] = [625.0, 3600.0, 22500.0];
const RADAR_COASTAL_MEASUREMENT_NOISE_VAR: [f64; 3] = [225.0, 3600.0, 0.0];

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
///
/// `tune` is given the chance to change the pipeline settings before the service is
/// built -- what [`the_imm_confirms_one_track_through_the_turn_where_constant_velocity_fragmented`]
/// uses to select `imm-cv-ct` (DN-28 §7), and what every test in this file now uses to
/// set its scenario's own measurement noise (DN-30) rather than leaving every one on
/// `PipelineSettings::default()`'s placeholder.
fn replay_with(
    scenario: &Scenario,
    tune: impl FnOnce(&mut PipelineSettings),
) -> (GeneratedTimeline, Vec<TrackView>) {
    let timeline = ScenarioGenerator::new(StdRng::seed_from_u64(SEED)).generate(scenario);
    let tracks = replay_timeline(&timeline, &timeline.observations, tune);
    (timeline, tracks)
}

/// [`replay_with`] over an explicit observation list rather than the whole timeline's
/// own, so a caller can drive the service through only a prefix of a scenario (DN-28's
/// finding recorded in the module documentation: scenario 1's comparison instant falls
/// inside a motion phase no six-dimensional filter can represent, and confirming that a
/// six-dimensional `imm-cv-ct` fixes fragmentation through the phase it *can* represent
/// needs scoring before the phase it cannot).
fn replay_timeline(
    timeline: &GeneratedTimeline,
    observations: &[gungnir_scenario::Observation],
    tune: impl FnOnce(&mut PipelineSettings),
) -> Vec<TrackView> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let spread = observations
        .iter()
        .map(|o| o.receipt_time_s - o.detection.timestamp_s)
        .fold(0.0f64, f64::max);
    let mut settings = PipelineSettings {
        reorder_horizon_s: spread.mul_add(2.0, 1.0),
        ..PipelineSettings::default()
    };
    tune(&mut settings);
    let mut service = LiveTrackingService::with_pipeline_settings(runtime.handle(), settings);
    for observation in observations {
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
    service.tracks().to_vec()
}

/// The mission time the comparison is made at: the last observation in the timeline.
///
/// See the module documentation for why this is computed independently rather than read
/// off a track's own (now-correct) `mission_time`.
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
///
/// Runs with `radar_medium`'s own measurement noise (DN-30), not
/// `PipelineSettings::default()`'s generic figure -- see this module's constant.
#[test]
fn the_maneuvering_aircraft_is_tracked_to_within_the_stated_bound() {
    let (timeline, tracks) = replay_with(&Scenario::ManeuveringAircraft, |settings| {
        settings.measurement_noise_var = RADAR_MEDIUM_MEASUREMENT_NOISE_VAR;
    });
    let worst = score("scenario 1", &timeline, &tracks, 500.0);
    assert!(
        worst > 0.0,
        "an error of exactly zero means the truth leaked into the estimator"
    );
}

/// **DN-28's acceptance criterion, isolated from two confounds this row found while
/// trying to state it plainly.**
///
/// The first draft of this row replayed the whole scenario and asked for one confirmed
/// track. It could not pass, for a reason that has nothing to do with `imm-cv-ct`:
/// `plan_maneuvering_aircraft` (`gungnir-scenario`) is a CV -> CT -> CA sequence, and
/// scenario 1's comparison instant (`comparison_time`, the last observation) falls in
/// the **third** phase, constant acceleration. `gungnir_core::ConstantAcceleration` is
/// `MotionModel<9>` -- position, velocity *and* acceleration -- and `imm-cv-ct`'s two
/// modes are both six-dimensional (DN-28 §2). No amount of process noise or gating makes
/// a six-dimensional filter represent a nine-dimensional dynamic; that is the same kind
/// of mismatch DN-28 §6 excludes EKF/UKF/particle for, found here rather than argued in
/// advance. **A three-mode CV/CT/CA IMM is not what DN-28 scoped or what the owner
/// signed**, so this row scores only the phase the signed scope actually covers:
/// replaying observations through the end of the coordinated-turn phase (source time
/// < 199 s, safely inside it, truth still six-dimensional).
///
/// **The second confound was `measurement_noise_var`.** `PipelineSettings::default()`
/// carries `[400.0, 400.0, 900.0]`, a generic figure no scenario's sensor was tuned
/// against; scenario 1's actual radar (`radar_medium`) reports at
/// `sigma_{range,cross,height}_m = [25, 60, 150]`, variance `[625, 3600, 22500]` --
/// twenty-five times the pipeline's assumed height variance. An `R` that
/// under-states real sensor noise makes every gate too tight, and it alone produces
/// most of the fragmentation the original defect blamed on the motion model: even
/// `kf-cv` stops *fragmenting* once `R` is corrected (one track, not three) -- it still
/// does not *confirm* the track through the turn, which is the residual `imm-cv-ct`
/// actually fixes and what this row isolates. This mismatch is not `imm-cv-ct`'s to fix
/// and is not fixed here; only this one test's own settings are corrected, and the
/// finding is flagged separately rather than silently changing `PipelineSettings::default()`
/// under every other gated row in this workspace.
#[test]
fn the_imm_confirms_one_track_through_the_turn_where_constant_velocity_fragmented() {
    /// Scenario 1's actual sensor noise (`gungnir_scenario::sensor::SensorModel::radar_medium`),
    /// not `PipelineSettings::default()`'s generic figure -- see this test's own
    /// documentation for why the difference matters here.
    const SCENARIO_1_MEASUREMENT_NOISE_VAR: [f64; 3] = [625.0, 3600.0, 22500.0];

    let full = ScenarioGenerator::new(StdRng::seed_from_u64(SEED))
        .generate(&Scenario::ManeuveringAircraft);
    let cutoff_s = 199.0;
    assert!(
        full.observations
            .iter()
            .any(|o| o.detection.timestamp_s >= 200.0),
        "the timeline does not reach the acceleration phase this row deliberately excludes"
    );
    let turn_only = GeneratedTimeline {
        observations: full
            .observations
            .iter()
            .filter(|o| o.detection.timestamp_s < cutoff_s)
            .cloned()
            .collect(),
        ..full
    };
    assert!(
        !turn_only.observations.is_empty(),
        "the cutoff left nothing to replay"
    );

    let cv_tracks = replay_timeline(&turn_only, &turn_only.observations, |settings| {
        settings.measurement_noise_var = SCENARIO_1_MEASUREMENT_NOISE_VAR;
    });
    let imm_tracks = replay_timeline(&turn_only, &turn_only.observations, |settings| {
        settings.measurement_noise_var = SCENARIO_1_MEASUREMENT_NOISE_VAR;
        settings.filter_selection = gungnir_fusion_async::FilterSelection::ImmCvCt;
        // The scenario's own coordinated-turn phase turns at this rate
        // (`gungnir-scenario::plan_maneuvering_aircraft`); a deployment reads its own
        // from the promoted baseline (DN-28 §5), and this test uses the number the
        // truth actually turns at rather than a generic placeholder.
        settings.imm_turn_rate_rad_s = 0.035;
    });

    println!(
        "scenario 1, turn phase only (kf-cv, corrected R): {} track(s), {} confirmed",
        cv_tracks.len(),
        cv_tracks
            .iter()
            .filter(|t| t.status == gungnir_model::TrackStatus::Confirmed)
            .count()
    );
    assert!(
        cv_tracks.len() > 1
            || cv_tracks
                .iter()
                .all(|t| t.status != gungnir_model::TrackStatus::Confirmed),
        "constant-velocity, even with the measurement noise corrected, must still show \
         some residual defect through the turn, or this row proves nothing: {cv_tracks:#?}"
    );

    let worst = score(
        "scenario 1, turn phase only (imm-cv-ct)",
        &turn_only,
        &imm_tracks,
        500.0,
    );
    assert!(
        worst > 0.0,
        "an error of exactly zero means the truth leaked into the estimator"
    );
    assert_eq!(
        imm_tracks.len(),
        1,
        "the imm must not fragment the one aircraft through the turn: {imm_tracks:#?}"
    );
    assert_eq!(
        imm_tracks[0].status,
        gungnir_model::TrackStatus::Confirmed,
        "the imm's one track must confirm through the turn, unlike the cv fragments"
    );
}

/// **Every vessel in scenario 2 is tracked to within 300 m of truth**, through 20 per cent
/// missed detections, two false alarms a scan and a land mask that hides one vessel
/// entirely for a while.
///
/// This is the multi-target case the bound can be stated for: the vessels are kilometres
/// apart, so a track within 300 m of one of them is that one's.
///
/// Runs with `radar_coastal`'s own range and cross-range noise (DN-30). Its height axis
/// is left at the default's placeholder rather than the real `sigma_height_m = 0.0`:
/// this pipeline seeds a freshly initiated track's covariance from the same
/// `measurement_noise_var` it builds `R` from (`FusionPipeline::initiate`), so an exact
/// zero there is a zero prior, not a stated noise, and is a substantively different
/// finding from the range/cross correction -- named rather than routed around with an
/// invented substitute.
#[test]
fn every_vessel_in_the_clutter_scenario_is_tracked_to_within_the_stated_bound() {
    let (timeline, tracks) = replay_with(
        &Scenario::MaritimeClutter {
            pd: 0.8,
            clutter_rate: 2.0,
        },
        |settings| {
            settings.measurement_noise_var[0] = RADAR_COASTAL_MEASUREMENT_NOISE_VAR[0];
            settings.measurement_noise_var[1] = RADAR_COASTAL_MEASUREMENT_NOISE_VAR[1];
        },
    );
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
///
/// Each single-sensor scenario runs with its own sensor's measurement noise (DN-30); the
/// three-sensor scenario 3 does not have one to use and stays at
/// `PipelineSettings::default()` -- see this module's constants.
#[test]
fn no_target_goes_untracked_in_any_scenario() {
    let no_correction: fn(&mut PipelineSettings) = |_| {};
    let radar_medium: fn(&mut PipelineSettings) = |settings| {
        settings.measurement_noise_var = RADAR_MEDIUM_MEASUREMENT_NOISE_VAR;
    };
    let radar_coastal_range_cross: fn(&mut PipelineSettings) = |settings| {
        settings.measurement_noise_var[0] = RADAR_COASTAL_MEASUREMENT_NOISE_VAR[0];
        settings.measurement_noise_var[1] = RADAR_COASTAL_MEASUREMENT_NOISE_VAR[1];
    };
    for (name, scenario, tune) in [
        ("scenario 1", Scenario::ManeuveringAircraft, radar_medium),
        (
            "scenario 2",
            Scenario::MaritimeClutter {
                pd: 0.8,
                clutter_rate: 2.0,
            },
            radar_coastal_range_cross,
        ),
        (
            "scenario 3",
            Scenario::UrbanConvoy {
                injected_bias_m: Vector3::new(40.0, -25.0, 5.0),
            },
            no_correction,
        ),
        (
            "scenario 4",
            Scenario::DenseSwarm { target_count: 40 },
            radar_medium,
        ),
    ] {
        let (timeline, tracks) = replay_with(&scenario, tune);
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
