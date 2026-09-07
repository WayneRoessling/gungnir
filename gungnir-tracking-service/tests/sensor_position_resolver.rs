// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The sensor-position resolver, and what it does and does not permit
//! (GAP-001's closing action; `docs/design/DN-27-bearing-only-detections.md` §2, §4, §5).
//!
//! Three of the six feed types GAP-001 lists report angles rather than positions, and
//! until this resolver existed `LiveTrackingService` refused every one of them: a
//! `DetectionView` carries a `SensorId` and not a position, and nothing joined the two.
//!
//! **The tests that matter here are the negative ones.** DN-27 §2 exists to forbid
//! turning a bearing into a position by assuming a range, and the tempting failure is not
//! a wrong formula -- it is a plausible default. An unknown sensor placed at the origin,
//! or a bearing given a nominal range, produces a detection that enters the tracker
//! without complaint, initiates a track, and is drawn as a symbol at a place nothing is.

use gungnir_model::{DetectionView, Measurement, MissionTime, SensorId};
use gungnir_tracking_service::{
    place_polar, LiveTrackingService, SensorPositions, SubmitError, TrackingService,
};

/// The sensor at (1000, 0, 20): a thousand metres east of the origin, twenty up.
fn positions() -> SensorPositions {
    SensorPositions::from_sensors([(4_u32, [1000.0, 0.0, 20.0])])
}

fn service(runtime: &tokio::runtime::Runtime, positions: SensorPositions) -> LiveTrackingService {
    LiveTrackingService::new(runtime.handle()).with_sensor_positions(positions)
}

fn view(sensor: u32, measurement: Measurement) -> DetectionView {
    DetectionView {
        sensor: SensorId(sensor),
        source_time: MissionTime(10.0),
        measurement,
        receipt_time: MissionTime(10.0),
        provenance: gungnir_model::Provenance {
            source_sensor_ids: vec![sensor],
            calibration_baseline_version: None,
            algorithm_version: String::new(),
            peer: None,
            conversion_loss: None,
            authentication: gungnir_model::SourceAuthentication::default(),
        },
    }
}

fn bearing(azimuth_rad: f64) -> Measurement {
    Measurement::Bearing {
        azimuth_rad,
        elevation_rad: None,
        azimuth_variance_rad2: 0.001,
        elevation_variance_rad2: None,
    }
}

#[test]
fn a_range_azimuth_elevation_report_is_placed_from_the_sensor_that_made_it() {
    // Due north of the sensor, level: 500 m along +north from (1000, 0, 20).
    let enu = place_polar([1000.0, 0.0, 20.0], 500.0, 0.0, 0.0);
    assert!((enu[0] - 1000.0).abs() < 1e-9, "east is unchanged: {enu:?}");
    assert!(
        (enu[1] - 500.0).abs() < 1e-9,
        "north advances by the range: {enu:?}"
    );
    assert!(
        (enu[2] - 20.0).abs() < 1e-9,
        "level means the sensor's height: {enu:?}"
    );

    // Due east, and the azimuth convention is atan2(east, north): 90 degrees is east.
    let enu = place_polar([1000.0, 0.0, 20.0], 500.0, std::f64::consts::FRAC_PI_2, 0.0);
    assert!((enu[0] - 1500.0).abs() < 1e-9, "east advances: {enu:?}");
    assert!((enu[1]).abs() < 1e-9, "north is unchanged: {enu:?}");

    // Straight up: elevation carries the whole range into the vertical.
    let enu = place_polar([1000.0, 0.0, 20.0], 500.0, 0.0, std::f64::consts::FRAC_PI_2);
    assert!(
        (enu[2] - 520.0).abs() < 1e-9,
        "up advances by the range: {enu:?}"
    );
    assert!(
        (enu[0] - 1000.0).abs() < 1e-9 && enu[1].abs() < 1e-9,
        "and nothing else moves: {enu:?}"
    );
}

#[test]
fn a_polar_report_reaches_the_pipeline_where_it_used_to_be_refused() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, positions());
    let d = view(
        4,
        Measurement::RangeAzimuthElevation {
            range_m: 500.0,
            azimuth_rad: 0.0,
            elevation_rad: 0.0,
            variance: [25.0, 25.0, 100.0],
        },
    );
    assert!(
        svc.submit_detection(d).is_ok(),
        "a range from a known point is a position; refusing it was the missing resolver"
    );
}

#[test]
fn a_bearing_reaches_the_tracker_as_a_bearing() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, positions());
    assert!(
        svc.submit_detection(view(4, bearing(0.6))).is_ok(),
        "a bearing from a sensor with a known position is submittable"
    );
    // What it may then do is DN-27 §5's business and the pipeline's: it may refine a
    // track, it may not initiate one, and if it matches nothing it is retained. This
    // test's claim is only that it is no longer refused at the door for want of a
    // position, which is what GAP-001's closing action names.
}

#[test]
fn without_a_resolver_an_angular_report_is_refused_and_not_placed_at_the_origin() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, SensorPositions::default());
    assert_eq!(
        svc.submit_detection(view(4, bearing(0.6))),
        Err(SubmitError::NotAPosition),
        "with no sensor positions at all the service must refuse, not assume an origin"
    );
    assert_eq!(
        svc.submit_detection(view(
            4,
            Measurement::RangeAzimuthElevation {
                range_m: 500.0,
                azimuth_rad: 0.0,
                elevation_rad: 0.0,
                variance: [25.0, 25.0, 100.0],
            }
        )),
        Err(SubmitError::NotAPosition),
        "and the same for a polar report"
    );
}

#[test]
fn a_sensor_the_deployment_never_declared_is_refused_by_name() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, positions());
    assert_eq!(
        svc.submit_detection(view(9, bearing(0.6))),
        Err(SubmitError::UnknownSensorPosition(9)),
        "a feed reporting under an undeclared sensor id is a configuration fault with a \
         name, and must not be collapsed into the no-resolver case"
    );
}

#[test]
fn a_sensor_with_a_non_finite_position_is_not_stored_and_its_reports_are_refused() {
    // A non-finite coordinate could only place a detection at a non-finite position, so
    // the resolver drops it at construction and the refusal that follows names the sensor
    // rather than letting a NaN travel into the tracker.
    let p = SensorPositions::from_sensors([
        (1_u32, [0.0, 0.0, 0.0]),
        (2_u32, [f64::NAN, 0.0, 0.0]),
        (3_u32, [0.0, f64::INFINITY, 0.0]),
    ]);
    assert_eq!(p.len(), 1, "only the finite one is kept");
    assert!(p.get(1).is_some());
    assert!(p.get(2).is_none());
    assert!(p.get(3).is_none());

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, p);
    assert_eq!(
        svc.submit_detection(view(2, bearing(0.6))),
        Err(SubmitError::UnknownSensorPosition(2))
    );
}

#[test]
fn a_position_report_still_goes_straight_through() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, positions());
    let d = view(
        4,
        Measurement::Position {
            enu: nalgebra::Vector3::new(10.0, 20.0, 30.0),
            variance_m2: [4.0, 4.0, 9.0],
        },
    );
    assert!(
        svc.submit_detection(d).is_ok(),
        "the existing path is unchanged"
    );
}

#[test]
fn a_polar_report_that_would_place_a_detection_nowhere_is_refused() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, positions());
    let d = view(
        4,
        Measurement::RangeAzimuthElevation {
            range_m: f64::NAN,
            azimuth_rad: 0.0,
            elevation_rad: 0.0,
            variance: [25.0, 25.0, 100.0],
        },
    );
    assert_eq!(
        svc.submit_detection(d),
        Err(SubmitError::NotAPosition),
        "a non-finite range must not produce a non-finite position inside the tracker"
    );
}
