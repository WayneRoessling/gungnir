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
            rehearsal: None,
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

/// The `gungnir-tracking-service` Sensor-position resolution for angular measurements row
/// of `docs/verification-capability-table.md` §2: "a polar report places to within 1e-9
/// of the closed form", with no angle on an axis.
///
/// **The three cases above cannot catch every wrong term.** Every angle in them is zero or
/// a right angle, so wherever elevation matters to east, `sin(azimuth)` is zero, and a
/// placement that dropped `cos(elevation)` from the east term passes all three. Here range
/// 500 m, azimuth 0.6 rad and elevation 0.3 rad give every term weight, and the closed
/// form is sensor + (r cos(el) sin(az), r cos(el) cos(az), r sin(el)).
///
/// The expected figures are that closed form evaluated to 50 significant digits with
/// mpmath (2026-09-16), outside the arithmetic under test, and written at the shortest
/// length that reads back as the same `f64`. Dropping `cos(el)` from the east term would
/// put east at 1282.32 m, twelve metres from where it belongs.
#[test]
fn an_off_axis_polar_report_is_placed_at_the_closed_form() {
    let enu = place_polar([1000.0, 0.0, 20.0], 500.0, 0.6, 0.3);
    assert!(
        (enu[0] - 1_269.711_779_072_205_7).abs() < 1e-9,
        "east is 1000 + r cos(el) sin(az): {enu:?}"
    );
    assert!(
        (enu[1] - 394.236_614_349_067_6).abs() < 1e-9,
        "north is r cos(el) cos(az): {enu:?}"
    );
    assert!(
        (enu[2] - 167.760_103_330_669_78).abs() < 1e-9,
        "up is 20 + r sin(el): {enu:?}"
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

/// The `gungnir-tracking-service` Sensor-position resolution for angular measurements row
/// of `docs/verification-capability-table.md` §2, through the service rather than beside
/// it: a polar report handed to `LiveTrackingService::submit_detection` is placed exactly
/// where [`place_polar`] puts it.
///
/// The test above shows such a report is accepted and says nothing about where it went.
/// **The observable is the track it starts.** A position detection that matches nothing
/// initiates a tentative track whose state is that position with zero velocity
/// (`FusionPipeline::initiate` copies the measurement in, and it runs after everything
/// else in its epoch has touched the tracks), and ending the stream flushes the reorder
/// buffer, so a single detection is processed rather than held waiting for a later one.
/// The track the service reports afterwards therefore carries, in its first three state
/// components, the position the service handed the pipeline, and that is compared with
/// `place_polar`'s answer bit for bit. No production code is opened up for it.
///
/// Off-axis angles on purpose, the same as
/// `an_off_axis_polar_report_is_placed_at_the_closed_form`, so that an azimuth and an
/// elevation handed over in each other's places land somewhere else. At zero azimuth and
/// zero elevation, which is what the acceptance test above submits, that swap is
/// invisible.
#[test]
fn a_polar_report_submitted_through_the_service_is_placed_where_place_polar_puts_it() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut svc = service(&runtime, positions());
    svc.submit_detection(view(
        4,
        Measurement::RangeAzimuthElevation {
            range_m: 500.0,
            azimuth_rad: 0.6,
            elevation_rad: 0.3,
            variance: [25.0, 25.0, 100.0],
        },
    ))
    .expect("a polar report from a declared sensor is accepted");
    svc.finish();

    // A deadlock guard, not a performance assertion, on the same reasoning as the drain
    // in `whole_pipeline_replay.rs`: health turns false when the pipeline task has flushed
    // and stopped, and a task that never stops fails here instead of hanging.
    let mut ended = false;
    for _ in 0..30_000 {
        svc.poll(MissionTime(10.0));
        if !svc.is_healthy() {
            ended = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(ended, "the pipeline task did not finish");

    let tracks = svc.tracks();
    assert_eq!(
        tracks.len(),
        1,
        "one report, one tentative track: {tracks:?}"
    );
    let state = tracks[0].state;
    let expected = place_polar([1000.0, 0.0, 20.0], 500.0, 0.6, 0.3);
    for (axis, want) in expected.iter().enumerate() {
        assert_eq!(
            state[axis].to_bits(),
            want.to_bits(),
            "state[{axis}] is {}, and place_polar puts it at {want}: {state:?}",
            state[axis]
        );
    }
    assert!(
        (3..6).all(|axis| state[axis] == 0.0),
        "a track started from one position knows no velocity, so this is not the \
         initiation the comparison relies on: {state:?}"
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

/// GAP-104: a sensor's declared geodetic position reaches the map as **metres in the
/// local ENU frame**, not as the radians it was written in.
///
/// `SensorConfig::position` is `[lat_rad, lon_rad, alt_m]` and this map is ENU metres.
/// Both are three `f64`, so passing the one for the other compiles, and both binaries did
/// from 2026-09-07 until this closed: a latitude near 55 degrees is about 0.96 radians,
/// so every sensor landed about a metre from the ENU origin. That is the failure this
/// pins -- a metre versus the sixteen hundred the deployment actually declared.
#[test]
fn a_geodetic_sensor_position_becomes_enu_metres_not_radians() {
    let frame = gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
        lat_rad: 55.0_f64.to_radians(),
        lon_rad: 12.0_f64.to_radians(),
        alt_m: 0.0,
    });
    // A hundredth of a degree north and two hundredths east of the origin: about 1.11 km
    // and 1.28 km respectively at this latitude.
    let p = SensorPositions::from_geodetic(
        &frame,
        [(
            4_u32,
            gungnir_model::Geodetic {
                lat_rad: 55.01_f64.to_radians(),
                lon_rad: 12.02_f64.to_radians(),
                alt_m: 0.0,
            },
        )],
    );
    let enu = p.get(4).expect("the sensor is stored");
    // The WGS84 reference, derived outside `gungnir-coord` two ways (2026-09-16). First,
    // each position to earth-centred coordinates by the closed form -- N = a / sqrt(1 -
    // e^2 sin^2(lat)), x = (N + h) cos(lat) cos(lon), y = (N + h) cos(lat) sin(lon),
    // z = (N (1 - e^2) + h) sin(lat), with a = 6378137 m, f = 1/298.257223563 and
    // e^2 = f (2 - f) -- then the difference rotated onto the origin's east, north and up,
    // all evaluated to 50 significant digits with mpmath. Second, PROJ 9.8.1's own `cart`
    // and `topocentric` conversions, through pyproj. Both give (1279.564224, 1113.419155,
    // -0.225243) m, so the east and north figures below, written to the millimetre, sit
    // within a quarter of a millimetre of it, and up is about 0.225 m below the origin's
    // tangent plane: the earth curving away over 1.7 km.
    assert!(
        (enu[0] - 1279.564).abs() < 0.5,
        "east is the converted metres: {enu:?}"
    );
    assert!(
        (enu[1] - 1113.419).abs() < 0.5,
        "north is the converted metres: {enu:?}"
    );
    // Within 0.5 m, which is the row's figure. At this geometry that bound does not tell
    // the reference from the tangent plane's zero, or from its own sign flipped, so it
    // holds `up` to the reference without testing the curvature itself: the §1 `coord`
    // Coordinate frame transforms row holds the conversion to 1e-6 m against pymap3d.
    let up_reference_m = -0.225;
    assert!(
        (enu[2] - up_reference_m).abs() < 0.5,
        "up is the WGS84 reference, {up_reference_m} m, below the origin's tangent plane: \
         {enu:?}"
    );
    // The claim that fails loudly under the defect, stated on its own so a future reader
    // sees what was actually wrong: the sensor is a kilometre and a half away, not one
    // metre. Unconverted, this sensor sat at (0.9601, 0.2098, 0.0).
    let from_origin = enu[0].hypot(enu[1]);
    assert!(
        from_origin > 1_000.0,
        "a sensor 1.7 km from the origin must not be placed beside it ({from_origin} m); \
         that is geodetic radians being read as ENU metres"
    );
}

/// The frame's origin is the ENU origin, and the altitude axis survives the conversion:
/// a sensor on a 250 m mast at the origin is at (0, 0, 250), not at the origin's radians.
#[test]
fn a_sensor_at_the_origin_is_at_zero_with_its_height_kept() {
    let origin = gungnir_model::Geodetic {
        lat_rad: 55.0_f64.to_radians(),
        lon_rad: 12.0_f64.to_radians(),
        alt_m: 0.0,
    };
    let frame = gungnir_model::LocalFrame::new(origin);
    let p = SensorPositions::from_geodetic(
        &frame,
        [(
            7_u32,
            gungnir_model::Geodetic {
                alt_m: 250.0,
                ..origin
            },
        )],
    );
    let enu = p.get(7).expect("the sensor is stored");
    assert!(
        enu[0].abs() < 1e-6 && enu[1].abs() < 1e-6,
        "the origin is the origin: {enu:?}"
    );
    assert!((enu[2] - 250.0).abs() < 1e-6, "up is the mast: {enu:?}");
}

/// And the converted position is what a polar report is actually placed from, which is
/// the whole point of the map: the defect did not merely store a wrong number, it drew
/// every range-azimuth-elevation detection beside the ENU origin.
#[test]
fn a_polar_report_is_placed_from_the_converted_position() {
    let frame = gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
        lat_rad: 55.0_f64.to_radians(),
        lon_rad: 12.0_f64.to_radians(),
        alt_m: 0.0,
    });
    let p = SensorPositions::from_geodetic(
        &frame,
        [(
            4_u32,
            gungnir_model::Geodetic {
                lat_rad: 55.01_f64.to_radians(),
                lon_rad: 12.02_f64.to_radians(),
                alt_m: 0.0,
            },
        )],
    );
    let sensor = p.get(4).expect("the sensor is stored");
    // 500 m due north of the sensor, level.
    let enu = place_polar(sensor, 500.0, 0.0, 0.0);
    assert!(
        (enu[1] - (sensor[1] + 500.0)).abs() < 1e-9,
        "the range is added to the sensor's own northing, not to the origin's: {enu:?}"
    );
    assert!(
        (enu[0] - 1279.564).abs() < 0.5,
        "and it is still east of the origin by the sensor's own easting: {enu:?}"
    );
}
