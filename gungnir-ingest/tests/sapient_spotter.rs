//! Row: "`gungnir-ingest` | The SAPIENT spotter adapter" in
//! `docs/verification-capability-table.md` §2.
//!
//! The spotter adapter run through the **real gateway**, over the fixtures in
//! `testdata/sapient/` (`SOURCE.md` records what is vendored from
//! `github.com/dstl/Apex-SAPIENT-Middleware` and what is constructed, and why the
//! bearing cases had to be constructed: the upstream protobuf-JSON set has no
//! bearing-only sample).
//!
//! Criterion: every message becomes a detection or a **named** count, and no bearing
//! becomes a position (`docs/design/DN-27-bearing-only-detections.md` §2 and §4).
//!
//! **The second half of that criterion is the point.** A test that only counted
//! detections would pass on a build that quietly gave every bearing a nominal range, so
//! the assertions below check the variant of every measurement and the name of every
//! refusal, not the totals.

use gungnir_ingest::adapters::sapient::{RecordedSapientSource, SpotterAdapter, SPOTTER_NODE_TYPE};
use gungnir_ingest::{IngestGateway, ProtocolAdapter, SourceAuthenticator};
use gungnir_model::events::IngestEvent;
use gungnir_model::{Geodetic, LocalFrame, Measurement, MissionTime, SensorId, TrackView};
use gungnir_tracking_service::{SubmitError, TrackingService};

fn testdata() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("testdata")
        .join("sapient")
}

/// The frame the constructed session's `Location` sits in: Salisbury Plain, which is
/// where the constructed coordinates were chosen near so the ENU offsets are small
/// enough to read.
fn frame() -> LocalFrame {
    LocalFrame::new(Geodetic {
        lat_rad: 51.0_f64.to_radians(),
        lon_rad: -1.0_f64.to_radians(),
        alt_m: 0.0,
    })
}

const SPOTTER: SensorId = SensorId(21);

fn adapter_over(path: &std::path::Path) -> SpotterAdapter<RecordedSapientSource> {
    let source = RecordedSapientSource::open(path).expect("the fixture is readable");
    SpotterAdapter::new("op-1", SPOTTER, frame(), [0.0, 0.0, 2.0], source)
}

/// The constructed spotter session: one registration, three reports that map, and three
/// messages that are counted by name.
#[test]
fn the_spotter_session_maps_what_it_can_and_names_everything_else() {
    let path = testdata().join("spotter-session.jsonl");
    let mut adapter = adapter_over(&path);
    // Mission time as Unix seconds, which is the live profile, so the session's own
    // timestamps are believed.
    let detections = adapter.poll(MissionTime(1_692_008_527.0)).expect("polls");

    assert_eq!(
        detections.len(),
        3,
        "three of the seven messages are detections: {detections:#?}"
    );

    // A bearing, with the elevation the report carried.
    match detections[0].measurement {
        Measurement::Bearing {
            azimuth_rad,
            elevation_rad,
            azimuth_variance_rad2,
            elevation_variance_rad2,
        } => {
            assert!((azimuth_rad - 2.0_f64.to_radians()).abs() < 1e-12);
            assert!(elevation_rad.is_some_and(|e| (e - 1.0_f64.to_radians()).abs() < 1e-12));
            let sigma = 1.0_f64.to_radians();
            assert!((azimuth_variance_rad2 - sigma * sigma).abs() < 1e-18);
            assert!(elevation_variance_rad2.is_some());
        }
        ref other => panic!("the first report is a bearing, not {other:?}"),
    }

    // A lased range keeps its polar shape (DN-27 §4 and §6).
    match detections[1].measurement {
        Measurement::RangeAzimuthElevation { range_m, .. } => {
            assert!((range_m - 3.0).abs() < 1e-9);
        }
        ref other => panic!("the second report is polar, not {other:?}"),
    }

    // The Cartesian report is a position, and it kept the report's own error rather
    // than being given the baseline's.
    match detections[2].measurement {
        Measurement::Position { variance_m2, .. } => {
            assert!((variance_m2[0] - 900.0).abs() < 1e-9, "{variance_m2:?}");
            assert!((variance_m2[2] - 2_500.0).abs() < 1e-9, "{variance_m2:?}");
        }
        ref other => panic!("the third report is a position, not {other:?}"),
    }
    assert!(
        detections[2].provenance.conversion_loss.is_none(),
        "the report stated its own error, so nothing was assumed"
    );

    // **Nothing became a position that was not one.** The criterion, stated directly.
    let placed = detections
        .iter()
        .filter(|d| d.measurement.position_enu().is_some())
        .count();
    assert_eq!(placed, 1, "only the Cartesian report is a place");

    let stats = adapter.stats();
    assert_eq!(stats.messages, 7);
    assert_eq!(stats.registered, 1);
    assert_eq!((stats.bearings, stats.ranged, stats.positions), (1, 1, 1));
    assert_eq!(stats.refused, 2);
    assert_eq!(stats.undecodable, 0);
    // Every message that did not become a detection is named.
    let named: Vec<(&str, u64)> = stats
        .unhandled
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        named,
        vec![
            ("bearing-without-a-stated-azimuth-error", 1),
            ("datum-magnetic-no-declination-model", 1),
            ("message-kind:statusReport", 1),
        ],
        "every unmapped message must be named: {named:?}"
    );
}

/// The vendored upstream samples, decoded as they actually are rather than as one might
/// hope. **Both are refused, and both refusals are correct**, which is why this test
/// exists rather than being folded into the one above: it is evidence about the
/// reference implementation's own fixtures and about what this adapter will not guess.
///
/// `registration_proto.json` carries no `nodeId` at all, and `detection_proto.json`
/// carries a `location` with `LOCATION_COORDINATE_SYSTEM_UNSPECIFIED`. Reading either
/// as usable would mean inventing a node identity or a unit.
#[test]
fn the_vendored_upstream_samples_are_refused_for_stated_reasons() {
    let registration = std::fs::read_to_string(testdata().join("registration_proto.json"))
        .expect("the vendored registration");
    let detection = std::fs::read_to_string(testdata().join("detection_proto.json"))
        .expect("the vendored detection");
    // Both are pretty-printed, so they are folded to one line each to make the feed's
    // line-per-message shape.
    let one_line = |text: &str| -> String {
        let value: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
        value.to_string()
    };
    let source = RecordedSapientSource::from_lines(
        vec![one_line(&registration), one_line(&detection)],
        "vendored".into(),
    );
    let mut adapter = SpotterAdapter::new("op-1", SPOTTER, frame(), [0.0, 0.0, 2.0], source);
    let detections = adapter.poll(MissionTime(1_692_008_527.0)).expect("polls");
    assert!(detections.is_empty(), "{detections:#?}");

    let stats = adapter.stats();
    assert_eq!(stats.messages, 2);
    assert_eq!(stats.registered, 0);
    // The upstream registration has no `nodeId`; the upstream detection therefore also
    // arrives from a node this adapter has never heard of.
    assert_eq!(stats.unhandled.get("message-without-node-id"), Some(&1));
    assert_eq!(
        stats.unhandled.get("detection-from-unregistered-node"),
        Some(&1)
    );
}

/// The whole path: the adapter inside the **real gateway**, authenticated, validated
/// and stamped as every other source is.
///
/// A bearing passes validation and is offered to the tracking service, which refuses it
/// with `SubmitError::NotAPosition` -- and **that refusal is the honest current state,
/// not a defect this test papers over**. The gateway's job is to decide whether an
/// observation is well formed, and a bearing now is; wiring one through to
/// `gungnir_fusion_async::FusionPipeline::offer_bearing` needs the reporting sensor's
/// position, which `DetectionView` does not carry and nothing resolves for the service
/// yet. The test pins both halves so the day the wiring lands, the change shows here.
#[test]
fn a_bearing_passes_the_real_gateway_and_the_tracker_names_what_it_cannot_take() {
    struct AllowSpotter;
    impl SourceAuthenticator for AllowSpotter {
        fn authenticate(&self, sensor: SensorId) -> Result<(), gungnir_ingest::IngestError> {
            if sensor == SPOTTER {
                Ok(())
            } else {
                Err(gungnir_ingest::IngestError::Unauthenticated(sensor))
            }
        }

        fn strength(&self) -> gungnir_model::SourceAuthentication {
            gungnir_model::SourceAuthentication::AllowList
        }
    }

    /// A sink that takes positions and names what it cannot take, which is exactly what
    /// `LiveTrackingService` does without needing a tokio runtime in this test.
    #[derive(Default)]
    struct RecordingSink {
        taken: Vec<gungnir_model::DetectionView>,
        refused: Vec<SubmitError>,
    }
    impl TrackingService for RecordingSink {
        fn submit_detection(
            &mut self,
            detection: gungnir_model::DetectionView,
        ) -> Result<(), SubmitError> {
            if detection.measurement.position_enu().is_some() {
                self.taken.push(detection);
                Ok(())
            } else {
                self.refused.push(SubmitError::NotAPosition);
                Err(SubmitError::NotAPosition)
            }
        }
        fn poll(&mut self, _now: MissionTime) {}
        fn tracks(&self) -> &[TrackView] {
            &[]
        }
        fn is_healthy(&self) -> bool {
            true
        }
    }

    let path = testdata().join("spotter-session.jsonl");
    let now = MissionTime(1_692_008_527.0);
    let mut gateway = IngestGateway::new(Box::new(AllowSpotter));
    gateway.add_adapter(Box::new(adapter_over(&path)));
    let mut sink = RecordingSink::default();
    let events = gateway.tick(now, &mut sink);

    // Nothing was quarantined: all three reports are well formed, the bearing included,
    // which is the thing that was impossible before DN-27 §4.
    let quarantined: Vec<&IngestEvent> = events
        .iter()
        .filter(|e| matches!(e, IngestEvent::Quarantined { .. }))
        .collect();
    assert!(quarantined.is_empty(), "{quarantined:#?}");

    let accepted: Vec<&IngestEvent> = events
        .iter()
        .filter(|e| matches!(e, IngestEvent::Accepted(_)))
        .collect();
    let not_accepted: Vec<&IngestEvent> = events
        .iter()
        .filter(|e| matches!(e, IngestEvent::NotAccepted { .. }))
        .collect();
    assert_eq!(accepted.len(), 1, "the Cartesian report is the only place");
    assert_eq!(
        not_accepted.len(),
        2,
        "the bearing and the polar report are named, not dropped: {events:#?}"
    );
    for event in not_accepted {
        let IngestEvent::NotAccepted { reason, sensor } = event else {
            unreachable!("filtered above")
        };
        assert_eq!(*sensor, SPOTTER);
        assert!(
            reason.contains("refused rather than converted"),
            "the refusal must say what it was, not merely that there was one: {reason}"
        );
    }
    // And the one thing that must never happen: nothing gave a bearing a range.
    assert!(sink
        .taken
        .iter()
        .all(|d| d.measurement.position_enu().is_some()));
    assert_eq!(sink.refused.len(), 2);
}

/// A node that is not a person is not a spotter, whatever else it is. Named by the type
/// it declared, so a deployment can see it has pointed an acoustic array at this
/// adapter -- which is the same message set and the next adapter, not this one.
#[test]
fn only_a_human_node_registers_with_the_spotter_adapter() {
    let text = std::fs::read_to_string(testdata().join("spotter-session.jsonl"))
        .expect("the fixture is readable");
    let swapped: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.replace(SPOTTER_NODE_TYPE, "NODE_TYPE_ACOUSTIC"))
        .collect();
    let source = RecordedSapientSource::from_lines(swapped, "swapped".into());
    let mut adapter = SpotterAdapter::new("op-1", SPOTTER, frame(), [0.0, 0.0, 2.0], source);
    let detections = adapter.poll(MissionTime(1_692_008_527.0)).expect("polls");
    assert!(detections.is_empty(), "{detections:#?}");
    let stats = adapter.stats();
    assert_eq!(stats.registered, 0);
    assert_eq!(
        stats.unhandled.get("node-type:NODE_TYPE_ACOUSTIC"),
        Some(&1)
    );
    assert_eq!(
        stats.unhandled.get("detection-from-unregistered-node"),
        Some(&5),
        "every report the acoustic node sent is refused and counted: {:?}",
        stats.unhandled
    );
}
