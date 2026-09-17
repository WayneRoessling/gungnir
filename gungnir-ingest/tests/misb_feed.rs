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

use gungnir_ingest::adapters::misb::{
    MisbFeedStats, MisbStatsSink, PlatformReportSink, RecordedKlvSource, UasMetadataAdapter,
};
use gungnir_ingest::{AllowAllAuthenticator, IngestGateway};
use gungnir_interop::misb0601::{packet_checksum, UDS_KEY};
use gungnir_model::events::IngestEvent;
use gungnir_model::{
    EnuPoint, Geodetic, LocalFrame, MissionTime, SensorId, TrackView, UasPlatformReport,
};
use gungnir_tracking_service::{SubmitError, TrackingService};

const UAS_FEED: SensorId = SensorId(30);

/// `frame()`'s origin, degrees, at 0 m.
const FRAME_LAT_DEG: f64 = 60.0;
const FRAME_LON_DEG: f64 = 128.0;

/// Near the vendored fixture's own Sensor Latitude/Longitude (about 60.18N, 128.43E;
/// `testdata/misb/SOURCE.md`), so a placed detection has a small, readable ENU
/// magnitude rather than spanning a hemisphere.
fn frame() -> LocalFrame {
    LocalFrame::new(Geodetic {
        lat_rad: FRAME_LAT_DEG.to_radians(),
        lon_rad: FRAME_LON_DEG.to_radians(),
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

// ---------------------------------------------------------------------------------------
// Row: "`gungnir-ingest` | The UAS KLV metadata adapter" in
// `docs/verification-capability-table.md` §2, its report clause: every checksum-valid
// frame's full report reaches the side-channel sink.
//
// The vendored worked example is the only MISB ST 0601 frame this workspace has a third
// party's reading of, field by field (`gungnir-interop/tests/misb0601_fixtures.rs`,
// transcribed from klvdata), and its stated checksum is the one thing known to be wrong with
// it. So the frame below is that fixture with the checksum it computes put in place of the
// one it states; every other byte is the vendored one, and every expected value is klvdata's.
// ---------------------------------------------------------------------------------------

/// klvdata's reading of the fixture's Tag 2, `2009-01-12 22:08:22+00:00`
/// (`misb0601_fixtures.rs`), in whole seconds since the Unix epoch.
const FIXTURE_TIME_S: f64 = 1_231_798_102.0;

/// When the frames below arrive: a quarter second after the fixture's own time stamp, inside
/// the gateway's timestamp rules, so a placed fix is judged on its position.
const RECEIVED: MissionTime = MissionTime(FIXTURE_TIME_S + 0.25);

/// The checksum `testdata/misb/SOURCE.md` records klvdata computing over the fixture's first
/// 226 bytes, and re-derived by hand there. The fixture itself states `0xAA43`.
const FIXTURE_COMPUTED_CHECKSUM: [u8; 2] = [0x3E, 0x1E];

/// The largest angular disagreement this file allows with klvdata: the 1e-9 degrees
/// `misb0601_fixtures.rs` holds the decoder to, in radians.
fn angle_tolerance_rad() -> f64 {
    1e-9_f64.to_radians()
}

/// The largest placement disagreement this file allows, metres. A latitude or longitude
/// 1e-9 degrees from klvdata's moves a point by at most 0.11 mm anywhere on Earth, so a
/// millimetre leaves that margin and still catches any mistake in which tag goes where.
const PLACEMENT_TOLERANCE_M: f64 = 1e-3;

fn fixture_bytes() -> Vec<u8> {
    std::fs::read(fixture_path()).expect("the fixture reads")
}

/// The UAS Datalink LS checksum, worked a byte at a time: the low sixteen bits of a running
/// sum in which a byte at an even offset counts as the high half of a sixteen-bit word and a
/// byte at an odd offset as the low half. That is the sum of big-endian words
/// `gungnir_interop::misb0601::packet_checksum` documents, written out another way on purpose,
/// so this file never checks the codec's checksum against itself. Anchored on the fixture: it
/// reproduces SOURCE.md's 0x3E1E (asserted where it is used).
fn checksum_by_bytes(summed: &[u8]) -> u16 {
    summed.iter().enumerate().fold(0u16, |sum, (i, byte)| {
        let shift = if i % 2 == 0 { 8 } else { 0 };
        sum.wrapping_add(u16::from(*byte) << shift)
    })
}

/// The vendored fixture with its stated checksum replaced by the one its own bytes compute,
/// and nothing else changed.
fn the_fixture_with_its_checksum_corrected() -> Vec<u8> {
    let mut bytes = fixture_bytes();
    let n = bytes.len();
    // SOURCE.md: 228 bytes, closing with Tag 1 (Checksum), length 2, stated value 0xAA43.
    assert_eq!(n, 228);
    assert_eq!(bytes[n - 4..], [0x01, 0x02, 0xAA, 0x43]);
    assert_eq!(
        checksum_by_bytes(&bytes[..n - 2]).to_be_bytes(),
        FIXTURE_COMPUTED_CHECKSUM,
        "this file's checksum must reproduce the one SOURCE.md records before it is trusted"
    );
    bytes[n - 2..].copy_from_slice(&FIXTURE_COMPUTED_CHECKSUM);
    bytes
}

/// The fixture's local-set items in wire order, as (tag, value), without the closing checksum
/// item. After the 16-byte key, `0x81 0xD2` is a one-octet long-form BER length of 210
/// (SOURCE.md), and every item inside has a one-octet tag and a short-form length: the
/// longest, Tag 94, is 34 octets (`misb0601_fixtures.rs`).
fn fixture_items() -> Vec<(u8, Vec<u8>)> {
    let bytes = fixture_bytes();
    assert_eq!(bytes[16..18], [0x81, 0xD2]);
    let value = &bytes[18..];
    assert_eq!(value.len(), 210);
    let mut items = Vec::new();
    let mut at = 0;
    while at < value.len() {
        let (tag, len) = (value[at], usize::from(value[at + 1]));
        assert!(tag < 0x80 && len < 0x80, "not a short-form item at {at}");
        items.push((tag, value[at + 2..at + 2 + len].to_vec()));
        at += 2 + len;
    }
    assert_eq!(
        items.pop().map(|(tag, _)| tag),
        Some(1),
        "the checksum item closes the set"
    );
    // The 18 tags `misb0601_fixtures.rs` checks by value and the 6 it checks as carried raw.
    assert_eq!(items.len(), 24, "24 items before the checksum item");
    items
}

/// A frame of `items` under the fixture's own key, closed with a checksum item whose value
/// `checksum_by_bytes` computes.
fn frame_of(items: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut value = Vec::new();
    for (tag, bytes) in items {
        value.push(*tag);
        value.push(u8::try_from(bytes.len()).expect("short-form item"));
        value.extend_from_slice(bytes);
    }
    // Tag 1, length 2, and the value, which is filled in once the rest is known.
    value.extend_from_slice(&[1, 2, 0, 0]);
    let mut frame = fixture_bytes()[..16].to_vec();
    // BER: short form below 128 octets, else one long-form length octet.
    let len = u8::try_from(value.len()).expect("a frame this file builds stays under 256 octets");
    if len < 0x80 {
        frame.push(len);
    } else {
        frame.extend_from_slice(&[0x81, len]);
    }
    frame.extend_from_slice(&value);
    let n = frame.len();
    let checksum = checksum_by_bytes(&frame[..n - 2]).to_be_bytes();
    frame[n - 2..].copy_from_slice(&checksum);
    frame
}

/// Where a latitude, longitude and height land in `frame()`, worked from WGS-84's defining
/// constants (semi-major axis 6 378 137 m, inverse flattening 298.257 223 563) rather than
/// through `LocalFrame`, which is part of the path under test: the point and the origin to
/// Earth-centred coordinates, and their difference turned onto the origin's east, north and
/// up.
fn placed_by_hand(lat_deg: f64, lon_deg: f64, height_m: f64) -> [f64; 3] {
    const A: f64 = 6_378_137.0;
    const F: f64 = 1.0 / 298.257_223_563;
    let e2 = F * (2.0 - F);
    let earth_centred = |lat_deg: f64, lon_deg: f64, h: f64| {
        let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians());
        let n = A / (1.0 - e2 * lat.sin() * lat.sin()).sqrt();
        [
            (n + h) * lat.cos() * lon.cos(),
            (n + h) * lat.cos() * lon.sin(),
            (n * (1.0 - e2) + h) * lat.sin(),
        ]
    };
    let p = earth_centred(lat_deg, lon_deg, height_m);
    let o = earth_centred(FRAME_LAT_DEG, FRAME_LON_DEG, 0.0);
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let (lat0, lon0) = (FRAME_LAT_DEG.to_radians(), FRAME_LON_DEG.to_radians());
    [
        -lon0.sin() * d[0] + lon0.cos() * d[1],
        -lat0.sin() * lon0.cos() * d[0] - lat0.sin() * lon0.sin() * d[1] + lat0.cos() * d[2],
        lat0.cos() * lon0.cos() * d[0] + lat0.cos() * lon0.sin() * d[1] + lat0.sin() * d[2],
    ]
}

/// klvdata's Sensor Latitude, Longitude and True Altitude (Tags 13, 14, 15), placed by hand.
fn platform_placed_by_hand() -> [f64; 3] {
    placed_by_hand(
        60.176_822_966_978_335,
        128.426_759_042_044_52,
        14_190.719_462_882_427,
    )
}

fn assert_placed(got: [f64; 3], expected: [f64; 3], what: &str) {
    assert!(
        got.iter()
            .zip(&expected)
            .all(|(g, e)| (g - e).abs() < PLACEMENT_TOLERANCE_M),
        "{what}: placed at {got:?}, worked by hand as {expected:?}"
    );
}

fn assert_angle(got: Option<f64>, klvdata_deg: f64, what: &str) {
    let got = got.unwrap_or_else(|| panic!("{what} is absent"));
    assert!(
        (got - klvdata_deg.to_radians()).abs() < angle_tolerance_rad(),
        "{what}: {got} rad, klvdata reads {klvdata_deg} degrees"
    );
}

/// Every field of `report` against klvdata's reading of the fixture, except the platform
/// position, which the caller states because one frame below leaves it out.
fn assert_the_fixtures_report(report: &UasPlatformReport, platform_position: Option<[f64; 3]>) {
    // Destructured with no `..`, so a field added to the report and not checked here stops
    // this file compiling instead of passing unexamined.
    let UasPlatformReport {
        sensor,
        platform_position: position,
        platform_heading_rad,
        platform_pitch_rad,
        platform_roll_rad,
        sensor_relative_azimuth_rad,
        sensor_relative_elevation_rad,
        sensor_relative_roll_rad,
        slant_range_m,
        frame_center,
        platform_designation,
        platform_tail_number,
        mission_id,
        image_source_sensor,
        uas_lds_version,
        source_time,
        receipt_time,
    } = report;

    // The feed's identity, not anything the platform broadcast.
    assert_eq!(*sensor, UAS_FEED);
    match (position, platform_position) {
        (
            Some(EnuPoint {
                enu,
                elevation_reported,
            }),
            Some(expected),
        ) => {
            assert!(*elevation_reported, "Tag 15 is in the frame");
            assert_placed(*enu, expected, "platform_position");
        }
        (None, None) => {}
        (got, expected) => {
            panic!("platform_position is {got:?}; a placement of {expected:?} was expected")
        }
    }
    assert_angle(
        *platform_heading_rad,
        159.974_364_843_213_55,
        "platform_heading_rad",
    );
    assert_angle(
        *platform_pitch_rad,
        -0.431_531_723_990_598_7,
        "platform_pitch_rad",
    );
    assert_angle(
        *platform_roll_rad,
        3.405_865_657_521_289_3,
        "platform_roll_rad",
    );
    assert_angle(
        *sensor_relative_azimuth_rad,
        160.719_211_436_975_57,
        "sensor_relative_azimuth_rad",
    );
    assert_angle(
        *sensor_relative_elevation_rad,
        -168.792_324_833_940_85,
        "sensor_relative_elevation_rad",
    );
    assert_angle(
        *sensor_relative_roll_rad,
        176.865_437_649_391_94,
        "sensor_relative_roll_rad",
    );
    let slant = slant_range_m.expect("Tag 21 is in the frame");
    assert!(
        (slant - 68_590.983_298_744_77).abs() < 1e-9,
        "slant_range_m: {slant}"
    );
    // In this worked example the frame centre is some 10 000 km from the platform, so its
    // placement is checked at that scale rather than as a small offset from the origin.
    let center = frame_center.expect("Tags 23 and 24 are in the frame");
    assert!(center.elevation_reported, "Tag 25 is in the frame");
    assert_placed(
        center.enu,
        placed_by_hand(
            -10.542_388_633_146_132,
            29.157_890_122_923_02,
            3_216.037_232_013_427_5,
        ),
        "frame_center",
    );
    assert_eq!(platform_designation.as_deref(), Some("Predator"));
    // No Tag 4 in this worked example.
    assert_eq!(platform_tail_number.as_deref(), None);
    assert_eq!(mission_id.as_deref(), Some("Mission 12"));
    assert_eq!(image_source_sensor.as_deref(), Some("EO Nose"));
    assert_eq!(*uas_lds_version, Some(6));
    // Tag 2's own time, not the receipt time: the two differ by the quarter second `RECEIVED`
    // adds.
    assert!(
        (source_time.0 - FIXTURE_TIME_S).abs() < 1e-6,
        "source_time: {source_time:?}"
    );
    assert_eq!(*receipt_time, RECEIVED);
}

/// What one frame through the adapter and the real gateway produced.
struct Delivered {
    events: Vec<IngestEvent>,
    taken: Vec<gungnir_model::DetectionView>,
    reports: Vec<UasPlatformReport>,
    stats: MisbFeedStats,
}

/// `bytes` through `UasMetadataAdapter`, with a `PlatformReportSink` and the stats sink a host
/// reads, inside the real gateway, in one tick at `RECEIVED`.
fn through_the_gateway(bytes: Vec<u8>) -> Delivered {
    let reports = PlatformReportSink::default();
    let stats = MisbStatsSink::default();
    let source = RecordedKlvSource::from_bytes(bytes, "test".into());
    let adapter = UasMetadataAdapter::new("uas-feed", UAS_FEED, frame(), source)
        .with_report_sink(reports.clone())
        .with_stats_sink(stats.clone());
    let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = RecordingSink::default();
    let events = gateway.tick(RECEIVED, &mut sink);
    let reports = reports.lock().expect("report sink").drain(..).collect();
    let stats = *stats.lock().expect("stats sink");
    Delivered {
        events,
        taken: sink.taken,
        reports,
        stats,
    }
}

/// The vendored fixture with a checksum that validates: one report, carrying every field
/// klvdata reads from it, and the platform's fix accepted by the gateway as a detection.
#[test]
fn a_checksum_valid_fixture_frame_delivers_its_whole_report_to_the_sink() {
    let delivered = through_the_gateway(the_fixture_with_its_checksum_corrected());

    let stats = delivered.stats;
    assert_eq!(
        (
            stats.frames_decoded,
            stats.checksum_mismatches,
            stats.positions_placed,
            stats.positions_not_placed
        ),
        (1, 0, 1, 0),
        "{stats:?}"
    );

    assert_eq!(delivered.reports.len(), 1, "{:#?}", delivered.reports);
    let platform = platform_placed_by_hand();
    assert_the_fixtures_report(&delivered.reports[0], Some(platform));

    assert_eq!(delivered.events.len(), 1, "{:#?}", delivered.events);
    assert!(
        matches!(&delivered.events[0], IngestEvent::Accepted(d) if d.sensor == UAS_FEED),
        "{:#?}",
        delivered.events
    );
    assert_eq!(delivered.taken.len(), 1);
    let fix = delivered.taken[0]
        .measurement
        .position_enu()
        .expect("the platform's fix is a position");
    assert_placed([fix[0], fix[1], fix[2]], platform, "the detection");
}

/// A checksum-valid frame with no Sensor Latitude or Longitude (Tags 13 and 14 removed from
/// the fixture's items) has no fix to place, and its report still reaches the sink: with no
/// platform position, and with everything else the frame carried.
#[test]
fn a_checksum_valid_frame_with_no_platform_position_still_delivers_its_report() {
    // The builder first reproduces the corrected fixture byte for byte from its own items, so
    // the frame below is a frame already shown valid less two items, with its length and
    // checksum worked the same way.
    assert_eq!(
        frame_of(&fixture_items()),
        the_fixture_with_its_checksum_corrected()
    );
    let items: Vec<(u8, Vec<u8>)> = fixture_items()
        .into_iter()
        .filter(|(tag, _)| *tag != 13 && *tag != 14)
        .collect();
    assert_eq!(items.len(), 22);

    let delivered = through_the_gateway(frame_of(&items));

    let stats = delivered.stats;
    assert_eq!(
        (stats.frames_decoded, stats.checksum_mismatches),
        (1, 0),
        "{stats:?}"
    );
    assert_eq!(stats.positions_not_placed, 1, "{stats:?}");
    assert_eq!(stats.positions_placed, 0, "{stats:?}");

    assert_eq!(delivered.reports.len(), 1, "{:#?}", delivered.reports);
    assert_the_fixtures_report(&delivered.reports[0], None);

    // No fix, so no detection: nothing reached the gateway to accept or refuse.
    assert!(delivered.events.is_empty(), "{:#?}", delivered.events);
    assert!(delivered.taken.is_empty());
}
