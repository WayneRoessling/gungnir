// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Row: "`gungnir-ingest` | Validation and quarantine; the ASTERIX radar, direction-finder
//! and UAS-identification adapter" in `docs/verification-capability-table.md` §2, its first
//! clause: zero malformed or unauthenticated payloads reach `TrackingService`.
//!
//! One payload for every refusal rule `gungnir_ingest::gateway::validate_detection` states,
//! enumerated from the function in the order it checks them, and one from a sensor the
//! allow-list does not name, all through `IngestGateway::tick` the way a host calls it. Each
//! must be quarantined under its own reason, and the tracking service must receive none of
//! them.
//!
//! **Every payload is a one-field copy of a detection the same tick accepts.** The rules run
//! in order and the first to fail names the reason, so a payload that broke two rules would
//! show only the earlier one working. Each row here breaks exactly one, and the four
//! well-formed detections the rows are copied from go through the same tick and do reach the
//! sink: a tracking service that received nothing because nothing was ever submitted to it
//! would otherwise pass a test that only looked at what it received.

use std::collections::BTreeSet;

use gungnir_ingest::adapters::simulated::SimulatedAdapter;
use gungnir_ingest::{AllowListAuthenticator, IngestGateway, IngestStats};
use gungnir_model::events::IngestEvent;
use gungnir_model::{DetectionView, Measurement, MissionTime, Provenance, SensorId, TrackView};
use gungnir_tracking_service::{SubmitError, TrackingService};

/// The gateway's clock for the one tick each test runs.
const NOW: MissionTime = MissionTime(100.0);

/// The sensor no allow-list entry names.
const STRANGER: u32 = 999;

/// Records every detection offered to the tracking service, and takes it.
#[derive(Default)]
struct SinkSpy {
    received: Vec<DetectionView>,
}

impl TrackingService for SinkSpy {
    fn submit_detection(&mut self, detection: DetectionView) -> Result<(), SubmitError> {
        self.received.push(detection);
        Ok(())
    }
    fn poll(&mut self, _now: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &[]
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

/// A detection from `sensor`, observed half a second before it was received at `NOW`: inside
/// both timestamp tolerances, so the measurement alone decides.
fn detection(sensor: u32, measurement: Measurement) -> DetectionView {
    DetectionView {
        sensor: SensorId(sensor),
        source_time: MissionTime(99.5),
        receipt_time: NOW,
        measurement,
        provenance: Provenance::default(),
    }
}

/// The detections the refused rows are copied from: each of `Measurement`'s three variants,
/// and a bearing both without and with an elevation, since the bearing rules turn on which.
fn well_formed() -> Vec<DetectionView> {
    vec![
        detection(
            1,
            Measurement::Position {
                enu: nalgebra::Vector3::new(1_000.0, 2_000.0, 300.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
        ),
        detection(
            2,
            Measurement::RangeAzimuthElevation {
                range_m: 5_000.0,
                azimuth_rad: 0.5,
                elevation_rad: 0.1,
                variance: [25.0, 1.0e-4, 1.0e-4],
            },
        ),
        detection(
            3,
            Measurement::Bearing {
                azimuth_rad: 1.0,
                elevation_rad: None,
                azimuth_variance_rad2: 1.0e-4,
                elevation_variance_rad2: None,
            },
        ),
        detection(
            4,
            Measurement::Bearing {
                azimuth_rad: 1.0,
                elevation_rad: Some(0.2),
                azimuth_variance_rad2: 1.0e-4,
                elevation_variance_rad2: Some(4.0e-4),
            },
        ),
    ]
}

/// What the gateway has to say about a payload it refuses.
enum Refusal {
    /// `validate_detection` refused it: the rule's reason, verbatim.
    Invalid(&'static str),
    /// The authenticator refused the sensor, before validation ran.
    Unauthenticated,
}

impl Refusal {
    /// The reason an `IngestEvent::Quarantined` carries, which is the `IngestError` as text.
    fn event_reason(&self, sensor: SensorId) -> String {
        match self {
            Self::Invalid(rule) => format!("message from {sensor:?} quarantined: {rule}"),
            Self::Unauthenticated => format!("source {sensor:?} is not authenticated"),
        }
    }
}

/// One payload per refusal rule, in the order `validate_detection` checks them, then the one
/// from a stranger. Each differs from the well-formed detection of the same shape in the one
/// field named in its comment.
#[allow(clippy::too_many_lines)] // a table: one entry per rule reads better unbroken
fn refused() -> Vec<(DetectionView, Refusal)> {
    use Refusal::{Invalid, Unauthenticated};
    vec![
        // 1. A number in the measurement that is not finite (sensor 3's azimuth). NaN is the
        // case this rule exists for: every comparison with it is false, so none of the rules
        // after this one would refuse it.
        (
            detection(
                101,
                Measurement::Bearing {
                    azimuth_rad: f64::NAN,
                    elevation_rad: None,
                    azimuth_variance_rad2: 1.0e-4,
                    elevation_variance_rad2: None,
                },
            ),
            Invalid("non-finite measurement"),
        ),
        // 2. A position 20 000 km from the frame origin (sensor 1's east axis): half again the
        // Earth's own diameter, 12 756 km, and twice the gateway's stated bound of 1.0e7 m.
        (
            detection(
                102,
                Measurement::Position {
                    enu: nalgebra::Vector3::new(2.0e7, 0.0, 0.0),
                    variance_m2: [400.0, 400.0, 900.0],
                },
            ),
            Invalid("measurement magnitude out of range"),
        ),
        // 3. A position axis with a variance of zero (sensor 1's north variance).
        (
            detection(
                103,
                Measurement::Position {
                    enu: nalgebra::Vector3::new(1_000.0, 2_000.0, 300.0),
                    variance_m2: [400.0, 0.0, 900.0],
                },
            ),
            Invalid("a position axis states a variance of zero or less"),
        ),
        // 4. A negative range (sensor 2's range).
        (
            detection(
                104,
                Measurement::RangeAzimuthElevation {
                    range_m: -5.0,
                    azimuth_rad: 0.5,
                    elevation_rad: 0.1,
                    variance: [25.0, 1.0e-4, 1.0e-4],
                },
            ),
            Invalid("range out of range"),
        ),
        // 5. A polar component with a negative variance (sensor 2's azimuth variance).
        (
            detection(
                105,
                Measurement::RangeAzimuthElevation {
                    range_m: 5_000.0,
                    azimuth_rad: 0.5,
                    elevation_rad: 0.1,
                    variance: [25.0, -1.0e-4, 1.0e-4],
                },
            ),
            Invalid("a polar component states a variance of zero or less"),
        ),
        // 6. A bearing with no azimuth error (sensor 3's azimuth variance).
        (
            detection(
                106,
                Measurement::Bearing {
                    azimuth_rad: 1.0,
                    elevation_rad: None,
                    azimuth_variance_rad2: 0.0,
                    elevation_variance_rad2: None,
                },
            ),
            Invalid(
                "a bearing states an azimuth variance of zero or less, and the angular error \
                 is the only thing a bearing measures well",
            ),
        ),
        // 7. A bearing whose elevation carries an error of zero (sensor 4's elevation
        // variance).
        (
            detection(
                107,
                Measurement::Bearing {
                    azimuth_rad: 1.0,
                    elevation_rad: Some(0.2),
                    azimuth_variance_rad2: 1.0e-4,
                    elevation_variance_rad2: Some(0.0),
                },
            ),
            Invalid("a bearing states an elevation variance of zero or less"),
        ),
        // 8. A bearing with an elevation and no error for it (sensor 4's elevation variance,
        // removed).
        (
            detection(
                108,
                Measurement::Bearing {
                    azimuth_rad: 1.0,
                    elevation_rad: Some(0.2),
                    azimuth_variance_rad2: 1.0e-4,
                    elevation_variance_rad2: None,
                },
            ),
            Invalid(
                "a bearing carries an elevation without its error, or an error without an \
                 elevation",
            ),
        ),
        // 9. A source time that is not a number (sensor 3's source time). NaN again, for the
        // same reason as rule 1: neither timestamp rule after this one would refuse it.
        (
            DetectionView {
                source_time: MissionTime(f64::NAN),
                ..detection(
                    109,
                    Measurement::Bearing {
                        azimuth_rad: 1.0,
                        elevation_rad: None,
                        azimuth_variance_rad2: 1.0e-4,
                        elevation_variance_rad2: None,
                    },
                )
            },
            Invalid("non-finite timestamp"),
        ),
        // 10. A source time ten seconds after its own receipt (sensor 2's source time); a
        // sensor clock one second ahead of ours is the most tolerated.
        (
            DetectionView {
                source_time: MissionTime(NOW.0 + 10.0),
                ..detection(
                    110,
                    Measurement::RangeAzimuthElevation {
                        range_m: 5_000.0,
                        azimuth_rad: 0.5,
                        elevation_rad: 0.1,
                        variance: [25.0, 1.0e-4, 1.0e-4],
                    },
                )
            },
            Invalid("source time is ahead of receipt time"),
        ),
        // 11. A receipt stamped a minute after the gateway's own clock (sensor 1's receipt
        // time); five seconds is the most tolerated.
        (
            DetectionView {
                receipt_time: MissionTime(NOW.0 + 60.0),
                ..detection(
                    111,
                    Measurement::Position {
                        enu: nalgebra::Vector3::new(1_000.0, 2_000.0, 300.0),
                        variance_m2: [400.0, 400.0, 900.0],
                    },
                )
            },
            Invalid("receipt time is in the future"),
        ),
        // And sensor 1's detection, word for word, from a sensor the allow-list does not name.
        (
            detection(
                STRANGER,
                Measurement::Position {
                    enu: nalgebra::Vector3::new(1_000.0, 2_000.0, 300.0),
                    variance_m2: [400.0, 400.0, 900.0],
                },
            ),
            Unauthenticated,
        ),
    ]
}

/// The row's first clause, rule by rule, through the real `tick`.
#[test]
fn every_refusal_rule_quarantines_its_payload_under_its_own_reason_and_none_reaches_the_tracker() {
    let well_formed = well_formed();
    let refused = refused();

    let mut adapter = SimulatedAdapter::new("refusal-rules");
    for d in well_formed.iter().chain(refused.iter().map(|(d, _)| d)) {
        adapter.push(d.clone());
    }
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: (1..=4).chain(101..=111).map(SensorId).collect(),
    }));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = SinkSpy::default();
    let events = gateway.tick(NOW, &mut sink);

    // The tracking service received the four well-formed detections, in order, and nothing
    // else: none of the twelve refused payloads was submitted to it.
    let reached: Vec<SensorId> = sink.received.iter().map(|d| d.sensor).collect();
    let expected: Vec<SensorId> = well_formed.iter().map(|d| d.sensor).collect();
    assert_eq!(reached, expected, "{:#?}", sink.received);

    assert_eq!(
        events.len(),
        well_formed.len() + refused.len(),
        "{events:#?}"
    );
    let (accepted, quarantined) = events.split_at(well_formed.len());
    for (event, d) in accepted.iter().zip(&well_formed) {
        assert!(
            matches!(event, IngestEvent::Accepted(a) if a.sensor == d.sensor),
            "sensor {} was well formed and not accepted: {event:?}",
            d.sensor.0
        );
    }
    for (event, (payload, refusal)) in quarantined.iter().zip(&refused) {
        match event {
            IngestEvent::Quarantined { sensor, reason } => {
                assert_eq!(*sensor, payload.sensor);
                assert_eq!(
                    *reason,
                    refusal.event_reason(payload.sensor),
                    "sensor {} was quarantined under another rule's reason",
                    payload.sensor.0
                );
            }
            other => panic!(
                "sensor {}'s payload was not quarantined: {other:?}",
                payload.sensor.0
            ),
        }
    }

    // Eleven rules, eleven different reasons: an operator reading the quarantine log can
    // tell every rule from every other.
    let rules: BTreeSet<&str> = refused
        .iter()
        .filter_map(|(_, r)| match r {
            Refusal::Invalid(rule) => Some(*rule),
            Refusal::Unauthenticated => None,
        })
        .collect();
    assert_eq!(rules.len(), 11, "{rules:#?}");

    assert_eq!(
        gateway.stats(),
        IngestStats {
            accepted: 4,
            quarantined: 12,
            adapter_failures: 0,
            not_accepted: 0,
        }
    );
    // Quarantine is the gateway doing its job, not the gateway failing.
    assert!(gateway.is_healthy());
}

/// The table has to grow when the gateway does. `validate_detection` names every refusal
/// through its one `quarantine` closure, so the closure's call sites are its rules: a rule
/// added without a row above fails here, rather than leaving the new rule asserted nowhere.
#[test]
fn the_table_has_a_row_for_every_rule_validate_detection_states() {
    let source = include_str!("../src/gateway.rs");
    let start = source
        .find("pub fn validate_detection")
        .expect("validate_detection is in src/gateway.rs");
    let len = source[start..]
        .find("pub fn decode_json_line")
        .expect("decode_json_line follows validate_detection in src/gateway.rs");
    let rules = source[start..start + len]
        .matches("Err(quarantine(")
        .count();
    let rows = refused()
        .iter()
        .filter(|(_, r)| matches!(r, Refusal::Invalid(_)))
        .count();
    assert_eq!(
        rows, rules,
        "validate_detection states {rules} refusal rules and the table has {rows} rows"
    );
}
