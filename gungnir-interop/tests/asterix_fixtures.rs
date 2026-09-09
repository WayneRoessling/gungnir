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
