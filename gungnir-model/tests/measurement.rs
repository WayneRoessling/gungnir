//! Row: "`model` Bearing measurement" in `docs/verification-capability-table.md` §1
//! (docs/design/DN-27-bearing-only-detections.md §10, first row).
//!
//! Method: every variant round-trips; a bearing with no elevation is distinguishable
//! from one with zero elevation. Criterion: exact round-trip; the two are not equal.
//!
//! **The second half is the one worth writing carefully.** DN-27 §10 says so, and the
//! reason is concrete: an optional elevation that serialised indistinguishably from
//! zero would put every acoustic detection on the horizon. Nothing downstream could
//! tell such a detection from a real horizon-level one, and the picture would show a
//! consistent, confident lie. The tests below therefore check the serialised text as
//! well as the decoded value, because equality of the decoded values alone would still
//! pass if the encoder wrote `0.0` for `None` and the decoder read `0.0` back as
//! `Some(0.0)` on both sides of the comparison.

use gungnir_model::{DetectionView, Measurement, MissionTime, Provenance, SensorId};

fn round_trip(m: &Measurement) -> Measurement {
    let text = serde_json::to_string(m).expect("a measurement serialises");
    serde_json::from_str(&text).expect("and parses back")
}

/// Every variant survives the journal and the wire unchanged, its error included.
#[test]
fn every_variant_round_trips_exactly() {
    let cases = [
        Measurement::Position {
            enu: nalgebra::Vector3::new(-83_560.7, 79_563.94, 3_971.33),
            variance_m2: [400.0, 400.0, 900.0],
        },
        Measurement::RangeAzimuthElevation {
            range_m: 12_345.678,
            azimuth_rad: 0.643_501_108_793_284_4,
            elevation_rad: -0.017_453_292_519_943_295,
            variance: [225.0, 1.0e-6, 4.0e-6],
        },
        Measurement::Bearing {
            azimuth_rad: 2.356_194_490_192_345,
            elevation_rad: Some(0.261_799_387_799_149_4),
            azimuth_variance_rad2: 3.046_174_197_867_086e-4,
            elevation_variance_rad2: Some(1.218_469_679_146_834_4e-3),
        },
        Measurement::Bearing {
            azimuth_rad: -1.234,
            elevation_rad: None,
            azimuth_variance_rad2: 1.0e-4,
            elevation_variance_rad2: None,
        },
    ];
    for case in &cases {
        assert_eq!(&round_trip(case), case, "{case:?} did not round-trip");
    }
}

/// **A missing elevation is not a zero one: zero means the horizon** (DN-27 §4 and §10).
///
/// Three separate claims, because any one of them alone would let the bug through:
/// the two values are not equal; the two serialisations are not equal, so the
/// difference survives the journal and the wire rather than only the type; and the
/// absent one does not encode a zero, which is what would put an acoustic detection
/// on the horizon.
#[test]
fn a_missing_elevation_is_not_a_zero_one() {
    let absent = Measurement::Bearing {
        azimuth_rad: 0.6435,
        elevation_rad: None,
        azimuth_variance_rad2: 1.0e-4,
        elevation_variance_rad2: None,
    };
    let horizon = Measurement::Bearing {
        azimuth_rad: 0.6435,
        elevation_rad: Some(0.0),
        azimuth_variance_rad2: 1.0e-4,
        elevation_variance_rad2: Some(1.0e-4),
    };
    assert_ne!(absent, horizon, "an absent elevation equalled a zero one");

    let absent_text = serde_json::to_string(&absent).expect("serialises");
    let horizon_text = serde_json::to_string(&horizon).expect("serialises");
    assert_ne!(
        absent_text, horizon_text,
        "the two serialised identically, so the distinction dies on the wire"
    );
    assert!(
        absent_text.contains(r#""elevation_rad":null"#),
        "an absent elevation must encode as null, not as a number: {absent_text}"
    );
    assert!(
        !absent_text.contains(r#""elevation_rad":0"#),
        "an absent elevation encoded a zero, which is the horizon: {absent_text}"
    );
    assert!(
        horizon_text.contains(r#""elevation_rad":0"#),
        "a horizon elevation must encode as the number it is: {horizon_text}"
    );

    // And back the other way: each parses to itself and not to the other.
    assert_eq!(round_trip(&absent), absent);
    assert_eq!(round_trip(&horizon), horizon);
    assert_ne!(round_trip(&absent), horizon);
}

/// A bearing cannot be read as a position by anything downstream, whatever it does
/// with the value. `position_enu` is `None` for both angular variants and that is the
/// point rather than an omission (DN-27 §2).
#[test]
fn no_angular_measurement_yields_a_position() {
    let bearing = Measurement::Bearing {
        azimuth_rad: 0.5,
        elevation_rad: Some(0.1),
        azimuth_variance_rad2: 1.0e-4,
        elevation_variance_rad2: Some(1.0e-4),
    };
    let polar = Measurement::RangeAzimuthElevation {
        range_m: 5_000.0,
        azimuth_rad: 0.5,
        elevation_rad: 0.1,
        variance: [100.0, 1.0e-4, 1.0e-4],
    };
    assert!(bearing.position_enu().is_none());
    assert!(polar.position_enu().is_none());
    assert!(!bearing.localises());
    // A polar report does determine a place, given the sensor's own -- which the
    // measurement does not carry, so it still yields no position here.
    assert!(polar.localises());

    let position = Measurement::Position {
        enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
        variance_m2: [4.0, 5.0, 6.0],
    };
    assert_eq!(
        position.position_enu(),
        Some(nalgebra::Vector3::new(1.0, 2.0, 3.0))
    );
}

/// A whole `DetectionView` carrying a bearing round-trips, which is what the journal
/// and `gungnir-api` actually serialise.
#[test]
fn a_detection_view_carrying_a_bearing_round_trips() {
    let view = DetectionView {
        sensor: SensorId(7),
        source_time: MissionTime(101.25),
        receipt_time: MissionTime(101.4),
        measurement: Measurement::Bearing {
            azimuth_rad: 0.645_771_823_237_902_5,
            elevation_rad: None,
            azimuth_variance_rad2: 3.046_174_197_867_086e-4,
            elevation_variance_rad2: None,
        },
        provenance: Provenance::default(),
    };
    let text = serde_json::to_string(&view).expect("serialises");
    let back: DetectionView = serde_json::from_str(&text).expect("parses");
    assert_eq!(back, view);
}

/// Non-finite numbers are visible on every variant, error terms included, because the
/// gateway's first validation rule reads exactly this.
#[test]
fn is_finite_sees_a_broken_error_term_and_not_only_a_broken_value() {
    assert!(Measurement::Position {
        enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
        variance_m2: [400.0, 400.0, 900.0],
    }
    .is_finite());
    assert!(!Measurement::Position {
        enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
        variance_m2: [400.0, f64::NAN, 900.0],
    }
    .is_finite());
    assert!(!Measurement::Bearing {
        azimuth_rad: 0.5,
        elevation_rad: None,
        azimuth_variance_rad2: f64::INFINITY,
        elevation_variance_rad2: None,
    }
    .is_finite());
    assert!(!Measurement::Bearing {
        azimuth_rad: 0.5,
        elevation_rad: Some(f64::NAN),
        azimuth_variance_rad2: 1.0e-4,
        elevation_variance_rad2: Some(1.0e-4),
    }
    .is_finite());
    assert!(!Measurement::RangeAzimuthElevation {
        range_m: f64::NAN,
        azimuth_rad: 0.5,
        elevation_rad: 0.1,
        variance: [1.0, 1.0, 1.0],
    }
    .is_finite());
}
