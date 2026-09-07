//! Conformance of the Category 048 decoder against real radar output.
//!
//! The fixtures under `testdata/asterix/` are public captures whose origin, commit,
//! and hashes are in `testdata/asterix/SOURCE.md`; their use was decided 2026-09-06
//! (`docs/design/external-standards.md` §1.5). The decoder is built to edition 1.32;
//! the edition the capture was produced under is not recorded by its source, so
//! these tests check that every block reads and that what it says is physically
//! plausible, not that any field equals a value known from elsewhere.

use gungnir_interop::asterix::cat034;
use gungnir_interop::asterix::cat048::{decode_block, decode_records, Mapped, Record, ReportType};
use gungnir_interop::asterix::data_blocks;
use gungnir_interop::{
    AsterixCat034Codec, AsterixCat048Codec, DetectionCodec, InteropError, RadarSite, ServiceEvent,
    ServiceMessageCodec,
};
use gungnir_model::{MissionTime, SensorId};
use std::collections::BTreeSet;

const CAT048_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat048.raw");
const CAT034_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat034.raw");
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
