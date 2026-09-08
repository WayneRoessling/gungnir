// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `UasMetadataAdapter` run through the **real gateway** (GAP-099), the same
//! standard every other feed under GAP-001 is held to
//! (`docs/verification-capability-table.md` §2's shape, for a row this gap adds).
//!
//! Two things this checks that a unit test on the adapter alone cannot: that the
//! gateway's own `validate_detection` accepts what the adapter emits without any
//! special-casing, and that a checksum-invalid frame -- the vendored fixture's own
//! condition, `testdata/misb/SOURCE.md` -- reaches the gateway as *nothing at all*
//! rather than as a detection that then has to be caught downstream.

use gungnir_ingest::adapters::misb::{RecordedKlvSource, UasMetadataAdapter};
use gungnir_ingest::{AllowAllAuthenticator, IngestGateway};
use gungnir_interop::misb0601::{packet_checksum, UDS_KEY};
use gungnir_model::events::IngestEvent;
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId, TrackView};
use gungnir_tracking_service::{SubmitError, TrackingService};

const UAS_FEED: SensorId = SensorId(30);

/// Near the vendored fixture's own Sensor Latitude/Longitude (about 60.18N, 128.43E;
/// `testdata/misb/SOURCE.md`), so a placed detection has a small, readable ENU
/// magnitude rather than spanning a hemisphere.
fn frame() -> LocalFrame {
    LocalFrame::new(Geodetic {
        lat_rad: 60.0_f64.to_radians(),
        lon_rad: 128.0_f64.to_radians(),
        alt_m: 0.0,
    })
}

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("testdata")
        .join("misb")
        .join("DynamicConstantMISMMSPacketData.bin")
}

/// A well-formed frame with a real position and a checksum this function itself
/// computes with the real `packet_checksum` -- test scaffolding in the sense
/// `docs/design/external-standards.md` §1.8 already uses for ASTERIX's hand-built
/// records, proving the accept path independent of the vendored fixture's own
/// checksum finding.
///
/// The Sensor Latitude/Longitude raw bytes are copied verbatim from the vendored
/// fixture itself (`testdata/misb/SOURCE.md`: `55 95 B6 6D` / `5B 53 60 C4`, which
/// `misb0601_fixtures.rs` already pins as decoding to 60.176822966978335 /
/// 128.42675904204452) rather than encoded fresh here -- MISB's "mapped" encoding is
/// a linear map across the tag's full domain, not a fixed-point degree scale, and
/// reusing bytes this workspace has already independently verified avoids a second,
/// untested implementation of that map's inverse.
fn a_well_formed_frame_with_a_real_position() -> Vec<u8> {
    let items: [(u8, &[u8]); 3] = [
        (13, &[0x55, 0x95, 0xB6, 0x6D]),
        (14, &[0x5B, 0x53, 0x60, 0xC4]),
        (10, b"TestPlatform"),
    ];
    let mut value = Vec::new();
    for (tag, bytes) in items {
        value.push(tag);
        value.push(u8::try_from(bytes.len()).expect("short-form"));
        value.extend_from_slice(bytes);
    }
    value.push(1);
    value.push(2);
    let checksum_at = UDS_KEY.len() + 1 + value.len();
    value.push(0);
    value.push(0);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&UDS_KEY);
    bytes.push(u8::try_from(value.len()).expect("short-form"));
    bytes.extend_from_slice(&value);
    let cs = packet_checksum(&bytes).to_be_bytes();
    bytes[checksum_at] = cs[0];
    bytes[checksum_at + 1] = cs[1];
    bytes
}

/// A sink that takes a position and names what it cannot -- everything else this
/// adapter could ever emit, so a future change that starts sending something other
/// than a position is visible here rather than passing by accident.
#[derive(Default)]
struct RecordingSink {
    taken: Vec<gungnir_model::DetectionView>,
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

#[test]
fn a_well_formed_frame_is_accepted_by_the_real_gateway() {
    let bytes = a_well_formed_frame_with_a_real_position();
    let source = RecordedKlvSource::from_bytes(bytes, "test".into());
    let adapter = UasMetadataAdapter::new("uas-feed", UAS_FEED, frame(), source);

    let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = RecordingSink::default();
    let events = gateway.tick(MissionTime(1_000.0), &mut sink);

    assert_eq!(events.len(), 1, "{events:#?}");
    assert!(
        matches!(&events[0], IngestEvent::Accepted(d) if d.sensor == UAS_FEED),
        "{events:#?}"
    );
    assert_eq!(sink.taken.len(), 1);
    assert_eq!(gateway.stats().accepted, 1);
    assert_eq!(gateway.stats().quarantined, 0);
    assert!(gateway.is_healthy());
}

/// The vendored worked example's own checksum does not validate
/// (`testdata/misb/SOURCE.md`). Run through the real gateway, it must produce
/// **nothing**: not a detection, not a quarantine, not a `NotAccepted` -- the frame
/// is discarded at the adapter, per MISB ST 0601.8-08, before the gateway ever sees
/// it, exactly as this workspace separates "the codec decodes, the adapter and
/// gateway decide what to trust".
#[test]
fn the_fixtures_bad_checksum_never_reaches_the_gateway_as_a_detection() {
    let source = RecordedKlvSource::open(&fixture_path()).expect("the fixture reads");
    let adapter = UasMetadataAdapter::new("uas-feed", UAS_FEED, frame(), source);

    let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = RecordingSink::default();
    let events = gateway.tick(MissionTime(1_000.0), &mut sink);

    assert!(events.is_empty(), "{events:#?}");
    assert!(sink.taken.is_empty());
    assert_eq!(
        gateway.stats(),
        gungnir_ingest::IngestStats {
            accepted: 0,
            quarantined: 0,
            adapter_failures: 0,
            not_accepted: 0,
        }
    );
    // The gateway itself is unaffected by an adapter that decided its own input was
    // untrustworthy -- that is not the same thing as an adapter failure.
    assert!(gateway.is_healthy());
}
