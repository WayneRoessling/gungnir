// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Conformance of the Category 048 and 034 decoders against real radar output, and of
//! the Category 205 decoder against a hand-built fixture (no real capture exists for
//! it -- `testdata/asterix/SOURCE.md`'s Category 205 section says what was checked).
//!
//! The Category 048/034 fixtures under `testdata/asterix/` are public captures whose
//! origin, commit, and hashes are in `testdata/asterix/SOURCE.md`; their use was
//! decided 2026-09-06 (`docs/design/external-standards.md` §1.5). Those decoders are
//! built to editions 1.32 and 1.29; the edition the capture was produced under is not
//! recorded by its source, so those tests check that every block reads and that what
//! it says is physically plausible, not that any field equals a value known from
//! elsewhere. `cat205.raw` and `cat129.raw` are different in kind: each is synthesized
//! from its own specification's byte tables (GAP-100, GAP-101), so their tests below
//! check field values against the exact counts that built the files, which is the
//! "known-correct" those fixtures can honestly offer.

use gungnir_interop::asterix::cat034;
use gungnir_interop::asterix::cat048::{decode_block, decode_records, Mapped, Record, ReportType};
use gungnir_interop::asterix::cat129;
use gungnir_interop::asterix::cat205;
use gungnir_interop::asterix::data_blocks;
use gungnir_interop::{
    AsterixCat034Codec, AsterixCat048Codec, AsterixCat129Codec, AsterixCat205Codec, DetectionCodec,
    DfSite, InteropError, RadarSite, ServiceEvent, ServiceMessageCodec, UasIdentificationCodec,
    UasSite,
};
use gungnir_model::{MissionTime, SensorId};
use std::collections::BTreeSet;

const CAT048_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat048.raw");
const CAT034_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat034.raw");
const CAT205_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat205.raw");
const CAT129_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat129.raw");
const PCAP: &[u8] = include_bytes!("../../testdata/asterix/cat_034_048.pcap");

/// The UDP payloads of a little-endian libpcap capture over Ethernet and IPv4, with
/// the packet timestamps as seconds. Enough for this fixture; not a pcap library.
fn udp_payloads(pcap: &[u8]) -> Vec<(f64, Vec<u8>)> {
    assert_eq!(
        &pcap[..4],
        &[0xD4, 0xC3, 0xB2, 0xA1],
        "little-endian pcap magic"
    );
    let mut out = Vec::new();
    let mut off = 24;
    while off + 16 <= pcap.len() {
        let field = |i: usize| {
            u32::from_le_bytes([
                pcap[off + i],
                pcap[off + i + 1],
                pcap[off + i + 2],
                pcap[off + i + 3],
            ])
        };
        let (secs, usecs, incl) = (field(0), field(4), field(8) as usize);
        let pkt = &pcap[off + 16..off + 16 + incl];
        assert!(
            pkt.len() > 42,
            "packet shorter than Ethernet + IPv4 + UDP headers"
        );
        assert_eq!(pkt[12..14], [0x08, 0x00], "IPv4 ethertype");
        assert_eq!(pkt[23], 17, "UDP");
        // The UDP length (header included) bounds the payload: short frames are padded
        // to Ethernet's 60-octet minimum, and the padding is not ASTERIX.
        let udp_len = usize::from(u16::from_be_bytes([pkt[38], pkt[39]]));
        assert!(
            udp_len >= 8 && 34 + udp_len <= pkt.len(),
            "UDP length out of frame"
        );
        let ts = f64::from(secs) + f64::from(usecs) / 1e6;
        out.push((ts, pkt[42..34 + udp_len].to_vec()));
        off += 16 + incl;
    }
    out
}

fn sites_for(records: &[Record]) -> Vec<RadarSite> {
    records
        .iter()
        .filter_map(|r| r.data_source)
        .map(|s| (s.sac, s.sic))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(i, (sac, sic))| RadarSite {
            sac,
            sic,
            sensor: SensorId(u32::try_from(i).expect("few radars") + 1),
            origin_enu_m: [0.0, 0.0, 0.0],
        })
        .collect()
}

fn assert_plausible(r: &Record) {
    assert!(
        r.data_source.is_some(),
        "record at {} has no I048/010",
        r.offset
    );
    let tod = r.time_of_day_s.expect("every record carries I048/140");
    assert!(
        (0.0..86_400.0).contains(&tod),
        "time of day {tod} out of range"
    );
    if let Some(p) = r.polar {
        assert!(p.slant_range_nm < 256.0);
        assert!((0.0..360.0).contains(&p.azimuth_deg));
    }
    if let Some(v) = r.velocity {
        assert!(v.ground_speed_nm_s <= 4.0);
        assert!((0.0..360.0).contains(&v.heading_deg));
    }
    if let Some(fl) = r.flight_level {
        assert!(
            (-15.0..=1000.0).contains(&fl.level),
            "flight level {}",
            fl.level
        );
    }
}

#[test]
fn raw_block_decodes_to_one_full_record() {
    let recs = decode_records(CAT048_RAW).expect("cat048.raw decodes");
    assert_eq!(recs.len(), 1);
    let r = &recs[0];
    assert_plausible(r);
    assert!(r.polar.is_some(), "the sample plot has a measured position");
    assert_ne!(
        r.descriptor.as_ref().map(|d| d.report_type),
        Some(ReportType::NoDetection)
    );
    let codec = AsterixCat048Codec::new(sites_for(&recs));
    let dets = codec
        .decode(CAT048_RAW, MissionTime(1_462_433_756.5))
        .expect("maps");
    assert_eq!(dets.len(), 1);
    assert!(dets[0].measurement.is_finite());
}

#[test]
fn raw_category_034_block_decodes_with_its_compound_status() {
    let recs = cat034::decode_records(CAT034_RAW).expect("cat034.raw decodes");
    assert_eq!(recs.len(), 1);
    let r = &recs[0];
    assert_eq!(r.message_type, Some(cat034::MessageType::SectorCrossing));
    assert!(r.sector.is_some());
    let status = r.system_status.expect("the sample carries I034/050");
    assert!(status.common.is_some() && status.ssr.is_some());
    assert!(r.processing_mode.expect("and I034/060").common.is_some());
    let site = RadarSite {
        sac: r.data_source.expect("010").sac,
        sic: r.data_source.expect("010").sic,
        sensor: SensorId(9),
        origin_enu_m: [0.0; 3],
    };
    let reports = AsterixCat034Codec::new(vec![site])
        .decode(CAT034_RAW, MissionTime(1_462_433_756.5))
        .expect("maps");
    assert_eq!(reports.len(), 1);
    assert!(reports[0].status.is_some());
}

#[test]
fn every_category_034_block_in_the_capture_decodes_and_maps() {
    let packets = udp_payloads(PCAP);
    let mut blocks = 0usize;
    let mut sector_crossings = 0usize;
    let mut north_markers = 0usize;
    let mut records = Vec::new();
    for (_, payload) in &packets {
        for block in data_blocks("test", payload).expect("splits") {
            if block.category == 34 {
                blocks += 1;
                records.extend(cat034::decode_block(&block).expect("every 034 block decodes"));
            }
        }
    }
    assert_eq!(blocks, 34, "SOURCE.md records the block count");
    let radars: BTreeSet<_> = records.iter().filter_map(|r| r.data_source).collect();
    let sites: Vec<RadarSite> = radars
        .iter()
        .enumerate()
        .map(|(i, d)| RadarSite {
            sac: d.sac,
            sic: d.sic,
            sensor: SensorId(u32::try_from(i).expect("few") + 1),
            origin_enu_m: [0.0; 3],
        })
        .collect();
    let codec = AsterixCat034Codec::new(sites);
    for r in &records {
        let tod = r
            .time_of_day_s
            .expect("every service message carries I034/030");
        assert!((0.0..86_400.0).contains(&tod));
        match codec.map(r, MissionTime(1_462_433_756.5)).expect("maps") {
            gungnir_interop::RadarServiceReport {
                event: ServiceEvent::SectorCrossing(s),
                ..
            } => {
                sector_crossings += 1;
                assert!((0.0..360.0).contains(&s.azimuth_deg));
            }
            gungnir_interop::RadarServiceReport {
                event: ServiceEvent::NorthMarker,
                ..
            } => north_markers += 1,
            other => panic!("unexpected service message in the fixture: {other:?}"),
        }
    }
    eprintln!(
        "capture: {} service messages from radars {radars:?}: {sector_crossings} sector crossings, {north_markers} north markers",
        records.len()
    );
    assert_eq!(records.len(), sector_crossings + north_markers);
}

#[test]
fn category_034_block_is_refused_by_name() {
    assert!(matches!(
        decode_records(CAT034_RAW),
        Err(InteropError::WrongCategory {
            expected: 48,
            found: 34,
            ..
        })
    ));
}

#[test]
fn every_category_048_packet_in_the_capture_decodes() {
    let packets = udp_payloads(PCAP);
    assert_eq!(packets.len(), 100, "SOURCE.md records 100 packets");
    let mut cat048 = 0usize;
    let mut cat034 = 0usize;
    let mut mixed_datagrams = 0usize;
    let mut records = Vec::new();
    for (_, payload) in &packets {
        let blocks = data_blocks("test", payload).expect("every datagram splits into blocks");
        if blocks.iter().any(|b| b.category == 34) && blocks.iter().any(|b| b.category == 48) {
            mixed_datagrams += 1;
        }
        for block in &blocks {
            match block.category {
                48 => {
                    cat048 += 1;
                    records.extend(decode_block(block).expect("every 048 block decodes"));
                }
                34 => cat034 += 1,
                other => panic!("unexpected category {other} in the fixture"),
            }
        }
    }
    assert_eq!(
        (cat048, cat034),
        (86, 34),
        "SOURCE.md records the block counts"
    );
    assert!(
        mixed_datagrams > 0,
        "the feed interleaves categories inside datagrams"
    );
    assert!(records.len() >= cat048, "at least one record per block");
    for r in &records {
        assert_plausible(r);
    }
    // The capture is a multi-radar feed: SOURCE.md lists the seven SAC/SIC pairs.
    let radars: BTreeSet<_> = records.iter().filter_map(|r| r.data_source).collect();
    assert_eq!(radars.len(), 7, "radars in the capture: {radars:?}");
    eprintln!("capture: {} records from radars {radars:?}", records.len());

    // Report what the build does not interpret, so the list is visible in test output.
    let raw: BTreeSet<&str> = records
        .iter()
        .flat_map(|r| r.carried_raw.iter().map(|i| i.item))
        .collect();
    eprintln!("carried without interpretation in this capture: {raw:?}");
}

#[test]
fn capture_maps_to_detections_with_losses_stated() {
    let packets = udp_payloads(PCAP);
    let cat048_records = |payload: &[u8]| -> Vec<Record> {
        data_blocks("test", payload)
            .expect("splits")
            .iter()
            .filter(|b| b.category == 48)
            .flat_map(|b| decode_block(b).expect("decodes"))
            .collect()
    };
    let all: Vec<Record> = packets
        .iter()
        .flat_map(|(_, p)| cat048_records(p))
        .collect();
    let codec = AsterixCat048Codec::new(sites_for(&all));
    let mut detections = 0usize;
    let mut not_detections = 0usize;
    for (ts, payload) in &packets {
        let receipt = MissionTime(*ts);
        for r in cat048_records(payload) {
            match codec.map(&r, receipt).expect("maps") {
                Mapped::Detection(d) => {
                    detections += 1;
                    assert!(d.measurement.is_finite());
                    // Time of day placed on the capture's own day, within its scan.
                    assert!(
                        (d.source_time.0 - ts).abs() < 3600.0,
                        "source time far from receipt"
                    );
                    if r.height_3d_ft.is_none() {
                        assert!(
                            d.provenance.conversion_loss.is_some(),
                            "a plot without 3D height is lossy"
                        );
                    }
                }
                Mapped::NotADetection(_) => not_detections += 1,
            }
        }
    }
    assert!(detections > 0);
    eprintln!("capture: {detections} detections, {not_detections} records that are not detections");
}

/// The fuzz row's promise in miniature: no input derived from the capture panics.
/// Truncations and single-octet corruptions of every block either decode or return
/// an error. `gungnir-fuzz` carries the real corpus; this keeps the property in CI.
#[test]
fn truncated_and_corrupted_blocks_never_panic() {
    let packets = udp_payloads(PCAP);
    for (_, payload) in &packets {
        for n in 0..payload.len() {
            let _ = decode_records(&payload[..n]);
            let _ = cat034::decode_records(&payload[..n]);
        }
        for i in 0..payload.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut m = payload.clone();
                m[i] ^= flip;
                let _ = decode_records(&m);
                let _ = cat034::decode_records(&m);
            }
        }
    }
}

/// The Category 205 fixture (`testdata/asterix/SOURCE.md`'s Category 205 section):
/// field-for-field against the exact counts the fixture was built from, since it is a
/// hand-built record and not a real capture -- the "known-correct" values are simply
/// the specification's own decoding rule applied to the counts documented there.
#[test]
fn category_205_fixture_decodes_to_the_documented_values() {
    let recs = cat205::decode_records(CAT205_RAW).expect("cat205.raw decodes");
    assert_eq!(
        recs.len(),
        1,
        "one data block, one record (edition 1.0 §4.4)"
    );
    let r = &recs[0];
    assert_eq!(
        r.data_source,
        Some(gungnir_interop::asterix::cat048::DataSource { sac: 99, sic: 1 })
    );
    assert_eq!(r.message_type, Some(cat205::MessageType::SensorDataReport));
    assert!((r.time_of_day_s.expect("I205/030") - 43_200.0).abs() < 1e-9);
    assert_eq!(r.report_number, Some(1));
    assert_eq!(r.radio_channel_name.as_deref(), Some("121.500"));
    assert!((r.local_bearing_deg.expect("I205/070") - 45.0).abs() < 1e-9);
    assert!(
        r.system_bearing_deg.is_none(),
        "type 5 never carries I205/080"
    );
    assert!((r.signal_level_dbuv.expect("I205/180") - 55.0).abs() < 1e-9);
    assert_eq!(r.signal_quality, Some(200));
    assert!((r.signal_elevation_deg.expect("I205/200") - 12.5).abs() < 1e-9);
    assert!(
        r.carried_raw.is_empty(),
        "the fixture carries no implementation-dependent item"
    );
}

#[test]
fn category_205_fixture_maps_to_a_bearing_with_the_configured_site_accuracy() {
    let site = DfSite {
        sac: 99,
        sic: 1,
        sensor: SensorId(41),
        origin_enu_m: [0.0; 3],
        azimuth_sigma_rad: 3.0_f64.to_radians(),
    };
    let codec = AsterixCat205Codec::new(vec![site]);
    let receipt = MissionTime(20_500.0 * 86_400.0 + 43_205.0);
    let dets = codec.decode(CAT205_RAW, receipt).expect("maps");
    assert_eq!(dets.len(), 1);
    let d = &dets[0];
    assert_eq!(d.sensor, SensorId(41));
    assert!((d.source_time.0 - (20_500.0 * 86_400.0 + 43_200.0)).abs() < 1e-6);
    match d.measurement {
        gungnir_model::Measurement::Bearing {
            azimuth_rad,
            elevation_rad,
            azimuth_variance_rad2,
            ..
        } => {
            assert!((azimuth_rad - 45.0_f64.to_radians()).abs() < 1e-9);
            // I205/200 decodes (previous test) but has no stated error anywhere in
            // this category, so it never reaches the measurement.
            assert!(elevation_rad.is_none());
            let sigma = 3.0_f64.to_radians();
            assert!((azimuth_variance_rad2 - sigma * sigma).abs() < 1e-18);
        }
        ref other => panic!("expected a bearing, got {other:?}"),
    }
    assert!(d.measurement.is_finite());
    assert!(
        d.provenance
            .conversion_loss
            .as_deref()
            .is_some_and(|s| s.contains("elevation")),
        "the dropped elevation is recorded, not silently discarded"
    );
    assert_eq!(d.provenance.algorithm_version, "asterix.cat205/ed1.0");
}

#[test]
fn category_205_unconfigured_site_is_refused_not_guessed() {
    let codec = AsterixCat205Codec::default();
    assert!(matches!(
        codec.decode(CAT205_RAW, MissionTime(0.0)),
        Err(InteropError::UnknownRadar {
            sac: 99,
            sic: 1,
            ..
        })
    ));
}

/// The fuzz row's promise in miniature for the new category, the same property
/// `truncated_and_corrupted_blocks_never_panic` establishes for 048 and 034.
#[test]
fn category_205_truncated_and_corrupted_fixture_never_panics() {
    for n in 0..CAT205_RAW.len() {
        let _ = cat205::decode_records(&CAT205_RAW[..n]);
    }
    for i in 0..CAT205_RAW.len() {
        for flip in [0x01u8, 0x80, 0xFF] {
            let mut m = CAT205_RAW.to_vec();
            m[i] ^= flip;
            let _ = cat205::decode_records(&m);
        }
    }
}

/// The Category 129 fixture (`testdata/asterix/SOURCE.md`'s Category 129 section):
/// field-for-field against the exact counts the fixture was built from, since it is a
/// hand-built record and not a real capture -- the same "known-correct" discipline
/// `category_205_fixture_decodes_to_the_documented_values` applies.
#[test]
fn category_129_fixture_decodes_to_the_documented_values() {
    let recs = cat129::decode_records(CAT129_RAW).expect("cat129.raw decodes");
    assert_eq!(
        recs.len(),
        1,
        "one data block, one record (edition 1.2 §4.4)"
    );
    let r = &recs[0];
    assert_eq!(
        r.data_source,
        Some(gungnir_interop::asterix::cat048::DataSource { sac: 0, sic: 0 }),
        "the airborne-to-ground placeholder edition 1.2 §5.2.1 recommends"
    );
    assert_eq!(r.registration_country.as_deref(), Some("US"));
    assert!((r.time_of_day_s.expect("I129/070") - 43_200.0).abs() < 1e-9);
    let pos = r.position.expect("I129/080");
    assert!((pos.latitude_deg - 10.0).abs() < 0.001);
    assert!((pos.longitude_deg + 20.0).abs() < 0.001);
    assert!((r.altitude_amsl_m.expect("I129/090") - 500.0).abs() < 1e-9);
    assert!(
        r.altitude_agl_m.is_none(),
        "the fixture carries no I129/100"
    );
    assert!((r.gnss_signal_accuracy_m.expect("I129/110") - 12.0).abs() < 1e-9);
    assert!(
        r.manufacturer_id.is_none() && r.model_id.is_none() && r.serial_number.is_none(),
        "the fixture carries none of the optional identification items"
    );
    assert!(r.operational_risk.is_none());
    assert!(
        r.carried_raw.is_empty(),
        "the fixture carries no implementation-undocumented item"
    );
}

#[test]
fn category_129_fixture_maps_to_a_uas_identification_report() {
    let site = UasSite {
        sac: 0,
        sic: 0,
        sensor: SensorId(51),
    };
    let codec = AsterixCat129Codec::new(vec![site]);
    let receipt = MissionTime(20_500.0 * 86_400.0 + 43_205.0);
    let reports = codec.decode(CAT129_RAW, receipt).expect("maps");
    assert_eq!(reports.len(), 1);
    let rep = &reports[0];
    assert_eq!(rep.sensor, SensorId(51));
    assert!((rep.source_time.0 - (20_500.0 * 86_400.0 + 43_200.0)).abs() < 1e-6);
    assert_eq!(rep.registration_country, "US");
    assert!((rep.position.lat_rad.to_degrees() - 10.0).abs() < 0.001);
    assert!((rep.position.lon_rad.to_degrees() + 20.0).abs() < 0.001);
    assert!((rep.position.alt_m - 500.0).abs() < 1e-9);
    assert!(
        rep.conversion_loss
            .as_deref()
            .is_some_and(|s| s.contains("geoid")),
        "the AMSL-as-ellipsoidal approximation is recorded, not silently assumed exact"
    );
}

#[test]
fn category_129_unconfigured_site_is_refused_not_guessed() {
    let codec = AsterixCat129Codec::default();
    assert!(matches!(
        codec.decode(CAT129_RAW, MissionTime(0.0)),
        Err(InteropError::UnknownRadar { sac: 0, sic: 0, .. })
    ));
}

/// The fuzz row's promise in miniature for the new category, the same property
/// `category_205_truncated_and_corrupted_fixture_never_panics` establishes for 205.
#[test]
fn category_129_truncated_and_corrupted_fixture_never_panics() {
    for n in 0..CAT129_RAW.len() {
        let _ = cat129::decode_records(&CAT129_RAW[..n]);
    }
    for i in 0..CAT129_RAW.len() {
        for flip in [0x01u8, 0x80, 0xFF] {
            let mut m = CAT129_RAW.to_vec();
            m[i] ^= flip;
            let _ = cat129::decode_records(&m);
        }
    }
}

// ---------------------------------------------------------------------------
// GAP-116: the clauses of the `gungnir-interop` row that were unasserted or only
// partly asserted (the GAP-067 walk, 2026-09-16). Each test names its clause.
// ---------------------------------------------------------------------------

/// A receipt time on a plausible date, for mapping records whose day is not the point.
const RECEIPT: MissionTime = MissionTime(20_500.0 * 86_400.0 + 43_205.0);

/// The source time of a record timed 12:00:00 and received at [`RECEIPT`].
const NOON: f64 = 20_500.0 * 86_400.0 + 43_200.0;

/// A data block of `category` around `body` (FSPEC and items), its length patched in.
fn block(category: u8, body: &[u8]) -> Vec<u8> {
    let mut b = vec![category, 0, 0];
    b.extend_from_slice(body);
    let len = u16::try_from(b.len()).expect("a small block");
    b[1..3].copy_from_slice(&len.to_be_bytes());
    b
}

/// Every Category 034 record of the capture, in capture order.
fn capture_034_records() -> Vec<cat034::Record> {
    let mut records = Vec::new();
    for (_, payload) in udp_payloads(PCAP) {
        for b in data_blocks("test", &payload).expect("splits") {
            if b.category == 34 {
                records.extend(cat034::decode_block(&b).expect("every 034 block decodes"));
            }
        }
    }
    records
}

/// A Category 034 codec configured with every radar the records name.
fn codec_034_for(records: &[cat034::Record]) -> AsterixCat034Codec {
    let radars: BTreeSet<_> = records.iter().filter_map(|r| r.data_source).collect();
    AsterixCat034Codec::new(
        radars
            .iter()
            .enumerate()
            .map(|(i, d)| RadarSite {
                sac: d.sac,
                sic: d.sic,
                sensor: SensorId(u32::try_from(i).expect("few") + 1),
                origin_enu_m: [0.0; 3],
            })
            .collect(),
    )
}

/// GAP-116 clause 1: a service message carries an operational status exactly when its
/// record carries I034/050's common subfield, and **a status-less message carries
/// none** -- a consumer that read absence as "released" would be reading something the
/// radar never said.
///
/// Pinned to `testdata/asterix/SOURCE.md`'s own count, made when the capture was
/// copied: 10 of the 34 carry I034/050, every one released, not overloaded, on a valid
/// time source and not reset; the other 24 carry no I034/050 at all.
#[test]
fn a_category_034_report_carries_a_status_exactly_when_the_record_carries_i034_050_com() {
    let records = capture_034_records();
    assert_eq!(records.len(), 34, "SOURCE.md records 34 service messages");
    let codec = codec_034_for(&records);
    let (mut with_status, mut without_status) = (0usize, 0usize);
    for r in &records {
        let report = codec.map(r, RECEIPT).expect("maps");
        let common = r.system_status.and_then(|s| s.common);
        assert_eq!(
            report.status.is_some(),
            common.is_some(),
            "record at {}: a status on the report must mean I034/050 COM on the record",
            r.offset
        );
        if let Some(status) = report.status {
            with_status += 1;
            assert!(
                status.released_for_operational_use
                    && !status.overloaded
                    && !status.time_source_invalid
                    && !status.track_numbers_reset,
                "record at {}: SOURCE.md says every I034/050 in the capture is nominal: \
                 {status:?}",
                r.offset
            );
        } else {
            without_status += 1;
            assert!(
                r.system_status.is_none(),
                "record at {} carries I034/050 without its common subfield",
                r.offset
            );
        }
    }
    assert_eq!(
        (with_status, without_status),
        (10, 24),
        "SOURCE.md: 10 messages carry I034/050 and 24 do not"
    );
}

/// GAP-116 clause 4: a lossy Category 034 message says what it lost on the report, and
/// a message that lost nothing says nothing.
///
/// Two ways to lose something: a jamming strobe's polar window is decoded on the
/// record but has no place on the report, and a message with no I034/030 is timed at
/// receipt. **The second found a defect**: the shared time fold worded every
/// category's missing time as "no time of day (I048/140)", so a Category 034, 205 or
/// 129 report named an item its record cannot carry. Each category now names its own.
#[test]
fn a_lossy_category_034_message_carries_its_conversion_loss() {
    let site = RadarSite {
        sac: 25,
        sic: 11,
        sensor: SensorId(3),
        origin_enu_m: [0.0; 3],
    };
    let codec = AsterixCat034Codec::new(vec![site]);

    // A jamming strobe: FRN 1, 2, 3 and 9 (I034/100) -- FSPEC 0xE1 0x40.
    let mut body = vec![0xE1, 0x40, 25, 11, 4, 0x54, 0x60, 0x00];
    body.extend_from_slice(&[0x0A, 0x00, 0x14, 0x00, 0x20, 0x00, 0x40, 0x00]); // I034/100
    let strobe = block(34, &body);
    let reports = codec.decode(&strobe, RECEIPT).expect("maps");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].event, ServiceEvent::JammingStrobe);
    let loss = reports[0]
        .conversion_loss
        .as_deref()
        .expect("the polar window the report cannot carry is recorded");
    assert!(loss.contains("polar window"), "{loss}");

    // A north marker with no I034/030: FRN 1 and 2 only -- FSPEC 0xC0.
    let untimed = block(34, &[0xC0, 25, 11, 1]);
    let reports = codec.decode(&untimed, RECEIPT).expect("maps");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].source_time, RECEIPT, "timed at receipt");
    assert_eq!(
        reports[0].conversion_loss.as_deref(),
        Some(cat034::TIME_OF_DAY_ABSENT),
        "the loss names I034/030, the item this category's message lacks"
    );
    for (item, absent) in [
        ("I034/030", cat034::TIME_OF_DAY_ABSENT),
        ("I205/030", cat205::TIME_OF_DAY_ABSENT),
        ("I129/070", cat129::TIME_OF_DAY_ABSENT),
        (
            "I048/140",
            gungnir_interop::asterix::cat048::TIME_OF_DAY_ABSENT,
        ),
    ] {
        assert!(absent.contains(item), "{absent}");
    }

    // And a message that lost nothing says nothing: every one in the capture.
    let records = capture_034_records();
    let codec = codec_034_for(&records);
    for r in &records {
        let report = codec.map(r, RECEIPT).expect("maps");
        assert_eq!(
            report.conversion_loss, None,
            "record at {}: a timed sector crossing or north marker loses nothing",
            r.offset
        );
    }
}

/// GAP-116 clause 5: what the build carries without interpreting is reported by item
/// name, across the capture and on a hand-built record carrying I034/SP.
///
/// `SOURCE.md` records I048/230 as the one item the Category 048 capture carries that
/// this build does not interpret, and no Category 034 message of the capture carries
/// one. I048/230 is a fixed two-octet item (edition 1.32), so each one carried must be
/// two octets.
#[test]
fn carried_items_are_named_across_the_capture_and_on_a_034_sp_record() {
    let mut names_048 = BTreeSet::new();
    let mut carried_048 = 0usize;
    for (_, payload) in udp_payloads(PCAP) {
        for b in data_blocks("test", &payload).expect("splits") {
            if b.category != 48 {
                continue;
            }
            for r in decode_block(&b).expect("decodes") {
                for item in &r.carried_raw {
                    names_048.insert(item.item);
                    carried_048 += 1;
                    if item.item == "I048/230" {
                        assert_eq!(item.octets.len(), 2, "I048/230 is two octets");
                    }
                }
            }
        }
    }
    assert_eq!(
        names_048,
        BTreeSet::from(["I048/230"]),
        "SOURCE.md names I048/230 as the one item carried without interpretation"
    );
    assert!(carried_048 > 0);

    let names_034: BTreeSet<&str> = capture_034_records()
        .iter()
        .flat_map(|r| r.carried_raw.iter().map(|i| i.item))
        .collect();
    assert!(
        names_034.is_empty(),
        "no service message of the capture carries an uninterpreted item: {names_034:?}"
    );

    // FRN 1, 2 and 14 (SP): FSPEC 0xC1 0x02; SP counts its own length octet.
    let sp = block(34, &[0xC1, 0x02, 25, 11, 1, 0x03, 0xAB, 0xCD]);
    let records = cat034::decode_records(&sp).expect("an SP field decodes");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].carried_raw,
        vec![cat034::RawItem {
            item: "I034/SP",
            octets: vec![0xAB, 0xCD],
        }],
        "the SP field is carried under its name, its data without the length octet"
    );
}

/// Every truncation of `whole` (one data block), re-lengthed so the framing accepts it,
/// either decodes to the records that start before the cut or is refused by the
/// category's own parser -- never by the framing, and never by a panic.
///
/// `blocked` is whether the category allows several records in one block (048 and
/// 034 do; 205 and 129 do not, by each edition's §4.4).
fn every_truncation_reaches_the_parser<R: PartialEq + std::fmt::Debug>(
    what: &str,
    whole: &[u8],
    decode: impl Fn(&[u8]) -> Result<Vec<R>, InteropError>,
    offset: impl Fn(&R) -> usize,
    blocked: bool,
) {
    let full = decode(whole).unwrap_or_else(|e| panic!("{what}: the whole block decodes: {e}"));
    let starts: Vec<usize> = full.iter().map(&offset).collect();
    for n in 3..whole.len() {
        let mut cut = whole[..n].to_vec();
        let len = u16::try_from(n).expect("a block is under 64 KiB");
        cut[1..3].copy_from_slice(&len.to_be_bytes());
        let boundary = starts.iter().position(|&s| s == n);
        match decode(&cut) {
            // A cut on a record boundary of a blocked category leaves whole records.
            Ok(records) if blocked && boundary.is_some() => {
                let k = boundary.unwrap_or_default();
                assert_eq!(
                    records,
                    full[..k],
                    "{what}: cut at {n}, on the boundary of record {k}"
                );
            }
            Err(InteropError::Malformed { offset, reason, .. })
                if !(blocked && boundary.is_some()) =>
            {
                assert!(
                    (3..=n).contains(&offset),
                    "{what}: cut at {n} was refused at octet {offset}, outside the \
                     record the parser was reading: {reason}"
                );
                assert!(
                    !reason.contains("data block"),
                    "{what}: cut at {n} was refused by the framing, not the parser: {reason}"
                );
            }
            other => panic!("{what}: cut at {n} (record boundary {boundary:?}) gave {other:?}"),
        }
    }
}

/// GAP-116 clause 6: truncation reaches each category's own parser.
///
/// `truncated_and_corrupted_blocks_never_panic` cuts the datagram, so every cut fails
/// the framing's length check and no category parser ever sees a short record; the
/// criterion's "no truncation panics in any of the four decoders" was true of the
/// framing alone. Here every block of the capture, and each single-block fixture, is
/// cut at every length **and its length field rewritten to match**, so the cut
/// arrives at the parser, which must refuse it by name at an octet inside the record
/// or, at a record boundary, return exactly the records before the cut.
#[test]
fn truncated_blocks_relengthed_reach_their_own_parser() {
    let blocks_of = |category: u8| -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for (_, payload) in udp_payloads(PCAP) {
            for b in data_blocks("test", &payload).expect("splits") {
                if b.category == category {
                    out.push(payload[b.offset..b.offset + 3 + b.payload.len()].to_vec());
                }
            }
        }
        out
    };
    let mut cat048_blocks = blocks_of(48);
    let mut cat034_blocks = blocks_of(34);
    assert_eq!((cat048_blocks.len(), cat034_blocks.len()), (86, 34));
    cat048_blocks.push(CAT048_RAW.to_vec());
    cat034_blocks.push(CAT034_RAW.to_vec());
    for (i, whole) in cat048_blocks.iter().enumerate() {
        every_truncation_reaches_the_parser(
            &format!("048 block {i}"),
            whole,
            decode_records,
            |r: &Record| r.offset,
            true,
        );
    }
    for (i, whole) in cat034_blocks.iter().enumerate() {
        every_truncation_reaches_the_parser(
            &format!("034 block {i}"),
            whole,
            cat034::decode_records,
            |r: &cat034::Record| r.offset,
            true,
        );
    }
    every_truncation_reaches_the_parser(
        "cat205.raw",
        CAT205_RAW,
        cat205::decode_records,
        |r: &cat205::Record| r.offset,
        false,
    );
    every_truncation_reaches_the_parser(
        "cat129.raw",
        CAT129_RAW,
        cat129::decode_records,
        |r: &cat129::Record| r.offset,
        false,
    );
}

/// A direction finder configured for the hand-built Category 205 records below.
fn df_site() -> DfSite {
    DfSite {
        sac: 99,
        sic: 1,
        sensor: SensorId(41),
        origin_enu_m: [0.0; 3],
        azimuth_sigma_rad: 3.0_f64.to_radians(),
    }
}

/// Assert a mapped Category 205 record is a bearing at `azimuth_deg` from the site in
/// [`df_site`], with that site's configured variance, no elevation, and nothing lost.
fn assert_site_bearing(mapped: &cat205::Mapped, azimuth_deg: f64) {
    let cat205::Mapped::Detection(d) = mapped else {
        panic!("expected a detection, got {mapped:?}");
    };
    assert_eq!(d.sensor, SensorId(41));
    assert!((d.source_time.0 - NOON).abs() < 1e-6);
    match d.measurement {
        gungnir_model::Measurement::Bearing {
            azimuth_rad,
            elevation_rad,
            azimuth_variance_rad2,
            elevation_variance_rad2,
        } => {
            assert!((azimuth_rad - azimuth_deg.to_radians()).abs() < 1e-12);
            assert_eq!(elevation_rad, None);
            assert_eq!(elevation_variance_rad2, None);
            let sigma = df_site().azimuth_sigma_rad;
            assert!(
                (azimuth_variance_rad2 - sigma * sigma).abs() < 1e-18,
                "the variance is the site's configured accuracy squared, never the wire's"
            );
        }
        ref other => panic!("expected a bearing, got {other:?}"),
    }
    assert_eq!(
        d.provenance.conversion_loss, None,
        "a timed bearing with no elevation loses nothing"
    );
}

/// GAP-116 clause 2: a System Bearing Report (I205/000 = 2) maps to a bearing whether
/// it carries I205/080 (with the Cartesian I205/060, as Table 2 pairs them) or only
/// I205/070 (with the WGS-84 I205/050), each with the site's own variance.
///
/// The fixture and the earlier tests are type 5 only, so the codec's arm for type 2
/// had never run.
#[test]
fn a_system_bearing_report_maps_to_a_bearing_from_either_bearing_item() {
    let codec = AsterixCat205Codec::new(vec![df_site()]);

    // FRN 1, 3, 4 | FRN 8, 10: FSPEC 0xB1 0xA0. I205/060 X = +1000 m (2000 counts),
    // Y = -500 m (-1000 counts); I205/080 = 27 001 counts, 270.01 degrees.
    let mut body = vec![0xB1, 0xA0, 99, 1, 2, 0x54, 0x60, 0x00];
    body.extend_from_slice(&[0x00, 0x07, 0xD0, 0xFF, 0xFC, 0x18]); // I205/060
    body.extend_from_slice(&[0x69, 0x79]); // I205/080
    let with_080 = block(205, &body);
    let records = cat205::decode_records(&with_080).expect("decodes");
    let r = &records[0];
    assert_eq!(
        r.message_type,
        Some(cat205::MessageType::SystemBearingReport)
    );
    assert_eq!(r.local_bearing_deg, None);
    assert!((r.system_bearing_deg.expect("I205/080") - 270.01).abs() < 1e-9);
    let xy = r.position_cartesian.expect("I205/060");
    assert!((xy.x_m - 1000.0).abs() < 1e-9 && (xy.y_m + 500.0).abs() < 1e-9);
    assert_site_bearing(&codec.map(r, RECEIPT).expect("maps"), 270.01);

    // FRN 1, 3, 4, 7 | FRN 9: FSPEC 0xB3 0x40. I205/050 as the unit test's counts;
    // I205/070 = 12 345 counts, 123.45 degrees.
    let mut body = vec![0xB3, 0x40, 99, 1, 2, 0x54, 0x60, 0x00];
    body.extend_from_slice(&1_864_135_i32.to_be_bytes());
    body.extend_from_slice(&(-3_728_270_i32).to_be_bytes());
    body.extend_from_slice(&[0x30, 0x39]); // I205/070
    let only_070 = block(205, &body);
    let records = cat205::decode_records(&only_070).expect("decodes");
    let r = &records[0];
    assert_eq!(
        r.message_type,
        Some(cat205::MessageType::SystemBearingReport)
    );
    assert_eq!(r.system_bearing_deg, None);
    assert!((r.local_bearing_deg.expect("I205/070") - 123.45).abs() < 1e-9);
    assert_site_bearing(&codec.map(r, RECEIPT).expect("maps"), 123.45);

    // Through the detection boundary as well, which is what the ingest adapter calls.
    for (bytes, deg) in [(&with_080, 270.01), (&only_070, 123.45)] {
        let dets = codec.decode(bytes, RECEIPT).expect("maps");
        assert_eq!(dets.len(), 1);
        assert_site_bearing(&cat205::Mapped::Detection(dets[0].clone()), deg);
    }
}

/// GAP-116 clause 7: a System Position Report, and its conflicting-transmission
/// counterpart, decode with their data source and time and are named rather than
/// mapped -- the reason says which message type it was and which types do map.
#[test]
fn a_system_position_report_is_named_with_its_source_and_time() {
    let codec = AsterixCat205Codec::new(vec![DfSite {
        sac: 25,
        sic: 210,
        ..df_site()
    }]);
    for (code, expected) in [
        (1u8, cat205::MessageType::SystemPositionReport),
        (3, cat205::MessageType::ConflictingSystemPositionReport),
    ] {
        // FRN 1, 3, 4, 7: FSPEC 0xB2.
        let mut body = vec![0xB2, 25, 210, code, 0x54, 0x60, 0x00];
        body.extend_from_slice(&1_864_135_i32.to_be_bytes());
        body.extend_from_slice(&(-3_728_270_i32).to_be_bytes());
        let bytes = block(205, &body);
        let records = cat205::decode_records(&bytes).expect("decodes");
        let r = &records[0];
        assert_eq!(r.message_type, Some(expected));
        assert_eq!(
            r.data_source,
            Some(gungnir_interop::asterix::cat048::DataSource { sac: 25, sic: 210 })
        );
        assert!((r.time_of_day_s.expect("I205/030") - 43_200.0).abs() < 1e-9);
        assert!(
            r.position_wgs84.is_some(),
            "the position decodes losslessly"
        );
        match codec.map(r, RECEIPT).expect("a configured site maps") {
            cat205::Mapped::NotADetection(reason) => {
                assert!(reason.contains("System Position Report"), "{reason}");
                assert!(reason.contains("only message types 2 and 5"), "{reason}");
            }
            other @ cat205::Mapped::Detection(_) => {
                panic!("type {code} is a resolved position, not a bearing: {other:?}")
            }
        }
        assert!(
            codec.decode(&bytes, RECEIPT).expect("decodes").is_empty(),
            "the detection boundary yields nothing for a position report"
        );
    }
}
