//! Row: "`fusion-async` Bearing update" in `docs/verification-capability-table.md` §1
//! (docs/design/DN-27-bearing-only-detections.md §10, third row).
//!
//! Method: a bearing offered to an empty pipeline, and to a pipeline holding a track it
//! gates into. Criterion: **initiates nothing** in the first case; refines the estimate
//! in the second, with the cross-range error scaling with range.
//!
//! # Why the first half is the important half
//!
//! DN-27 §5 rule 1 is a prohibition: a bearing may update a track and may **not**
//! initiate one. A single fixed sensor cannot localise from bearings at all -- the
//! problem is unobservable -- and a filter given a sequence of them converges to a
//! confident answer at the wrong range. **That failure is silent and it looks exactly
//! like success**, which is why the rule is a prohibition and not a tuning parameter,
//! and why the test below offers a long sequence of bearings rather than one: a build
//! that initiated on the tenth bearing and not the first would pass a one-shot test.
//!
//! The second half measures §6: the same angular error implies a cross-range error that
//! grows with range, so the same bearing offered to a distant track moves it further
//! and leaves it with more cross-range variance than one offered to a near track.

use gungnir_fusion_async::{
    run_batch, BearingDetection, BearingOutcome, BearingRefusal, Detection, FusionPipeline,
    PipelineSettings,
};
use gungnir_track::TrackStatus;

/// One degree of azimuth error, which is a direction finder's order of accuracy.
const ONE_DEGREE_VARIANCE_RAD2: f64 =
    (std::f64::consts::PI / 180.0) * (std::f64::consts::PI / 180.0);

fn bearing(t: f64, sensor_enu: [f64; 3], azimuth_rad: f64) -> BearingDetection {
    BearingDetection {
        sensor_id: 9,
        timestamp_s: t,
        sensor_enu,
        azimuth_rad,
        elevation_rad: None,
        azimuth_variance_rad2: ONE_DEGREE_VARIANCE_RAD2,
        elevation_variance_rad2: None,
    }
}

fn position(t: f64, enu: [f64; 3]) -> Detection {
    Detection {
        sensor_id: 1,
        timestamp_s: t,
        measurement: nalgebra::SVector::<f64, 3>::new(enu[0], enu[1], enu[2]),
    }
}

/// The azimuth from a sensor to a place, in this system's convention: `atan2(east, north)`.
fn azimuth_to(sensor_enu: [f64; 3], target_enu: [f64; 3]) -> f64 {
    (target_enu[0] - sensor_enu[0]).atan2(target_enu[1] - sensor_enu[1])
}

/// **Rule 1.** A hundred bearings of a real, moving, consistent target offered to an
/// empty pipeline start nothing at all.
///
/// The sequence is the case that matters: it is exactly the input a filter would
/// converge on -- one fixed sensor, a target crossing its field of view, a clean
/// bearing every tenth of a second -- and the answer it would converge to would be a
/// confident position at the wrong range. Nothing here produces one.
#[test]
fn a_sequence_of_bearings_from_one_sensor_initiates_nothing() {
    let mut pipeline = FusionPipeline::new(PipelineSettings::default());
    let sensor = [0.0, 0.0, 0.0];
    for k in 0..100 {
        let t = f64::from(k) * 0.1;
        // A target at 8 km moving east at 100 m/s: the textbook unobservable case.
        let target = [4_000.0 + 100.0 * t, 8_000.0, 1_000.0];
        let outcome = pipeline.offer_bearing(&bearing(t, sensor, azimuth_to(sensor, target)));
        assert!(
            matches!(outcome, BearingOutcome::Retained { .. }),
            "bearing {k} did something other than being retained: {outcome:?}"
        );
    }
    assert!(
        pipeline.snapshot().is_empty(),
        "a hundred bearings from one sensor initiated {} track(s); DN-27 §5 rule 1 \
         forbids initiating from a bearing at all",
        pipeline.snapshot().len()
    );
    let stats = pipeline.stats();
    assert_eq!(stats.initiated, 0, "nothing may be initiated by a bearing");
    assert_eq!(stats.accepted, 0, "no bearing entered the position buffer");
    assert_eq!(stats.bearings_offered, 100);
    assert_eq!(stats.bearings_updated, 0, "there was no track to update");
    assert_eq!(stats.bearings_retained, 100);
}

/// **Rule 3.** A bearing that matches nothing is retained for the stated lifetime and
/// shown, not dropped -- and then it expires, rather than accumulating for ever.
#[test]
fn an_unmatched_bearing_is_retained_for_its_lifetime_and_then_expires() {
    let settings = PipelineSettings {
        bearing_retention_s: 5.0,
        ..PipelineSettings::default()
    };
    let mut pipeline = FusionPipeline::new(settings);
    let sensor = [0.0, 0.0, 0.0];
    match pipeline.offer_bearing(&bearing(10.0, sensor, 0.5)) {
        BearingOutcome::Retained { until_s } => assert!((until_s - 15.0).abs() < 1e-12),
        other => panic!("an unmatched bearing must be retained: {other:?}"),
    }
    assert_eq!(pipeline.retained_bearings().len(), 1);

    // Still inside the lifetime.
    pipeline.expire_bearings(14.9);
    assert_eq!(pipeline.retained_bearings().len(), 1);
    assert_eq!(pipeline.stats().bearings_expired, 0);

    // Past it.
    pipeline.expire_bearings(15.1);
    assert!(pipeline.retained_bearings().is_empty());
    assert_eq!(pipeline.stats().bearings_expired, 1);
}

/// **Rule 1's permitted half.** A bearing offered to a pipeline holding a track it
/// gates into refines that track's estimate.
///
/// The track is initiated from positions, as it must be; the bearing is then offered
/// from a sensor off to one side, pointing at where the target really is while the
/// track's estimate has drifted away from it. The estimate moves towards the truth and
/// the cross-track uncertainty shrinks.
#[test]
fn a_bearing_refines_a_track_it_gates_into() {
    let settings = PipelineSettings::default();
    let mut pipeline = FusionPipeline::new(settings);
    // Six positions of a target sitting still at (5000, 5000, 0), which give a track
    // whose estimate is there.
    for k in 0..6 {
        let t = f64::from(k);
        let _ = pipeline.push(position(t, [5_000.0, 5_000.0, 0.0]));
    }
    pipeline.flush();
    let before = pipeline.snapshot();
    assert_eq!(before.len(), 1, "one target, one track: {before:?}");
    assert_eq!(before[0].status, TrackStatus::Confirmed);

    // A direction finder due south of the target, bearing at a place 40 m east of
    // where the track thinks the target is. Forty metres at 5 km is 0.46 degrees,
    // which is inside a one-degree gate.
    let sensor = [5_000.0, 0.0, 0.0];
    let truth = [5_040.0, 5_000.0, 0.0];
    let outcome = pipeline.offer_bearing(&bearing(5.0, sensor, azimuth_to(sensor, truth)));
    let BearingOutcome::Updated(id) = outcome else {
        panic!("the bearing should have refined the track: {outcome:?}");
    };
    assert_eq!(id, before[0].id);

    let after = pipeline.snapshot();
    assert_eq!(after.len(), 1, "a bearing must not have added a track");
    assert_eq!(pipeline.stats().initiated, 1, "the positions initiated it");
    assert_eq!(pipeline.stats().bearings_updated, 1);
    assert!(pipeline.retained_bearings().is_empty(), "it matched");

    // The estimate moved towards the truth, along the direction the bearing
    // constrains (east here, since the sensor is due south).
    let moved = after[0].state[0] - before[0].state[0];
    assert!(
        moved > 0.0 && moved < 40.0,
        "the estimate should move east towards the bearing but not past it: {moved} m"
    );
    // And the cross-range direction is better known than it was.
    assert!(
        after[0].covariance[(0, 0)] < before[0].covariance[(0, 0)],
        "an update must reduce the variance it informs: {} then {}",
        before[0].covariance[(0, 0)],
        after[0].covariance[(0, 0)]
    );
    // The along-range direction the bearing says nothing about is not improved by it.
    assert!(
        after[0].covariance[(1, 1)] >= before[0].covariance[(1, 1)] - 1e-6,
        "a bearing must not sharpen the range it did not measure"
    );
}

/// **§6, measured.** The same angular error is a larger cross-range error further away,
/// so the same one-degree bearing leaves a distant track with more cross-range variance
/// than a near one -- and by the square of the range, which is what `r σ` implies.
#[test]
fn the_cross_range_error_scales_with_range() {
    let residual_variance_at = |range_m: f64| {
        let settings = PipelineSettings::default();
        let mut pipeline = FusionPipeline::new(settings);
        let target = [0.0, range_m, 0.0];
        for k in 0..6 {
            let _ = pipeline.push(position(f64::from(k), target));
        }
        pipeline.flush();
        let before = pipeline.snapshot();
        assert_eq!(before.len(), 1);
        // A sensor at the origin looking straight up the north axis at the target: the
        // bearing constrains east, which is the cross-range direction.
        let sensor = [0.0, 0.0, 0.0];
        let outcome = pipeline.offer_bearing(&bearing(5.0, sensor, azimuth_to(sensor, target)));
        assert!(
            matches!(outcome, BearingOutcome::Updated(_)),
            "a bearing straight at the track must gate in: {outcome:?}"
        );
        let after = pipeline.snapshot();
        // What the bearing alone says about east, read off the update: the prior and
        // the posterior give the measurement's own variance by the scalar Kalman
        // identity 1/P_post = 1/P_prior + 1/R_effective.
        let prior = before[0].covariance[(0, 0)];
        let post = after[0].covariance[(0, 0)];
        1.0 / (1.0 / post - 1.0 / prior)
    };

    let near = residual_variance_at(2_000.0);
    let far = residual_variance_at(20_000.0);
    let ratio = far / near;
    assert!(
        (ratio - 100.0).abs() < 0.5,
        "ten times the range should be a hundred times the cross-range variance, \
         got {ratio} ({near} then {far})"
    );
    // And the absolute value is `(r sigma)^2` at each range, which is DN-27 §6's
    // `sigma_cross ~ r sigma_azimuth` squared.
    for (range_m, measured) in [(2_000.0, near), (20_000.0, far)] {
        let want = range_m * range_m * ONE_DEGREE_VARIANCE_RAD2;
        assert!(
            (measured - want).abs() < 0.01 * want,
            "at {range_m} m the bearing should carry a cross-range variance of {want}, \
             measured {measured}"
        );
    }
}

/// A bearing with no stated error is refused, not given a default one. For a bearing
/// the error *is* the information (DN-27 §4), and a default would be an invention.
#[test]
fn a_bearing_with_no_stated_error_is_refused_and_counted() {
    let mut pipeline = FusionPipeline::new(PipelineSettings::default());
    let mut b = bearing(1.0, [0.0, 0.0, 0.0], 0.5);
    b.azimuth_variance_rad2 = 0.0;
    assert_eq!(
        pipeline.offer_bearing(&b),
        BearingOutcome::Refused(BearingRefusal::NoStatedError)
    );
    b.azimuth_variance_rad2 = f64::NAN;
    assert_eq!(
        pipeline.offer_bearing(&b),
        BearingOutcome::Refused(BearingRefusal::NoStatedError)
    );
    b.azimuth_variance_rad2 = 1e-4;
    b.azimuth_rad = f64::NAN;
    assert_eq!(
        pipeline.offer_bearing(&b),
        BearingOutcome::Refused(BearingRefusal::NotFinite)
    );
    assert_eq!(pipeline.stats().bearings_refused, 3);
    assert_eq!(pipeline.stats().bearings_retained, 0, "nothing was kept");
    assert!(pipeline.snapshot().is_empty());
}

/// A bearing far from the pipeline's processed cursor is refused rather than folded
/// into an estimate that has already moved past it -- the same rule the position path
/// applies with `PushError::TooLate`, stated for bearings.
#[test]
fn a_bearing_outside_the_reorder_horizon_is_refused() {
    let settings = PipelineSettings::default();
    let horizon = settings.reorder_horizon_s;
    let mut pipeline = FusionPipeline::new(settings);
    for k in 0..6 {
        let _ = pipeline.push(position(f64::from(k), [1_000.0, 1_000.0, 0.0]));
    }
    pipeline.flush();
    let sensor = [0.0, 0.0, 0.0];
    let azimuth = azimuth_to(sensor, [1_000.0, 1_000.0, 0.0]);
    // The cursor is at t = 5 after the flush.
    assert_eq!(
        pipeline.offer_bearing(&bearing(5.0 + horizon * 2.0, sensor, azimuth)),
        BearingOutcome::Refused(BearingRefusal::OutsideHorizon)
    );
    assert!(matches!(
        pipeline.offer_bearing(&bearing(5.0, sensor, azimuth)),
        BearingOutcome::Updated(_)
    ));
}

/// The position path is untouched by any of this: the same timeline through
/// `run_batch` gives the same tracks it always did, so the bearing entry point added
/// nothing to the ordinary flow.
#[test]
fn the_position_path_is_unchanged() {
    let detections: Vec<Detection> = (0..12)
        .map(|k| {
            let t = f64::from(k);
            position(t, [100.0 * t, 50.0 * t, 1_000.0])
        })
        .collect();
    let tracks = run_batch(PipelineSettings::default(), &detections);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].status, TrackStatus::Confirmed);
    assert!((tracks[0].state[3] - 100.0).abs() < 5.0);
}
