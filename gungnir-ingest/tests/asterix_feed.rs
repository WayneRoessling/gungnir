// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The public radar capture through the real gateway: the adapter, authentication,
//! validation, and the tracking-service sink, end to end (GAP-001, radar half).
//!
//! `testdata/asterix/SOURCE.md` records what the capture holds: 100 datagrams, 86
//! Category 048 blocks with 128 records of which 126 are detections, 34 Category 034
//! blocks, seven radars. The radar positions are not in the capture, so every radar is
//! bound at the frame origin here; that anchors the plots, it does not check them.

use gungnir_ingest::adapters::asterix::{
    AsterixFeedAdapter, DfBinding, RadarBinding, ReplayDatagramSource, UasBinding,
    UasIdentificationSink,
};
use gungnir_ingest::{AllowListAuthenticator, DetectionView, IngestGateway, IngestStats, SensorId};
use gungnir_interop::ServiceEvent;
use gungnir_model::events::IngestEvent;
use gungnir_model::{Geodetic, LocalFrame, Measurement, MissionTime, TrackView};
use gungnir_tracking_service::{SubmitError, TrackingService};

const PCAP: &[u8] = include_bytes!("../../testdata/asterix/cat_034_048.pcap");
/// Hand-built, not captured: `testdata/asterix/SOURCE.md` says so and gives every field.
const CAT205: &[u8] = include_bytes!("../../testdata/asterix/cat205.raw");
/// Hand-built, not captured, for the same reason as `CAT205`.
const CAT129: &[u8] = include_bytes!("../../testdata/asterix/cat129.raw");

/// UDP payloads of the little-endian libpcap capture, honouring the UDP length so
/// Ethernet padding is not fed to the adapter, with the capture timestamps.
fn datagrams(pcap: &[u8]) -> Vec<(f64, Vec<u8>)> {
    assert_eq!(&pcap[..4], &[0xD4, 0xC3, 0xB2, 0xA1]);
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
        let udp_len = usize::from(u16::from_be_bytes([pkt[38], pkt[39]]));
        out.push((
            f64::from(secs) + f64::from(usecs) / 1e6,
            pkt[42..34 + udp_len].to_vec(),
        ));
        off += 16 + incl;
    }
    out
}

fn bindings(origin: Geodetic) -> Vec<RadarBinding> {
    [11u8, 12, 13, 14, 201, 204, 205]
        .iter()
        .enumerate()
        .map(|(i, sic)| RadarBinding {
            sac: 25,
            sic: *sic,
            sensor: SensorId(u32::try_from(i).expect("few") + 1),
            position: origin,
        })
        .collect()
}

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

#[test]
fn the_capture_flows_through_the_gateway_to_the_tracking_service() {
    let packets = datagrams(PCAP);
    let last_ts = packets.last().expect("packets").0;
    let origin = Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 100.0,
    };
    let frame = LocalFrame::new(origin);
    let source = ReplayDatagramSource::new(packets.iter().map(|(_, d)| d.clone()));
    let adapter = AsterixFeedAdapter::new("capture", source, &frame, &bindings(origin));

    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: (1..=7).map(SensorId).collect(),
    }));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = SinkSpy::default();

    // One tick at the capture's own time: the radar clocks run up to 20 s behind the
    // capture clock, never ahead, so every detection passes the source-time rule.
    let events = gateway.tick(MissionTime(last_ts), &mut sink);

    assert_eq!(sink.received.len(), 126, "SOURCE.md: 126 detections");
    assert!(events.iter().all(|e| matches!(e, IngestEvent::Accepted(_))));
    let stats = gateway.stats();
    assert_eq!(
        (stats.accepted, stats.quarantined, stats.not_accepted),
        (126, 0, 0)
    );
    assert_eq!(stats.adapter_failures, 0);
    assert!(gateway.is_healthy());

    // Every accepted detection is anchored at the radar's site in the local frame and
    // names the codec edition; the ones without a 3D height say what the mapping lost.
    let origin_enu = frame.to_enu(origin);
    let mut lossless = 0usize;
    for d in &sink.received {
        assert!(d.measurement.is_finite());
        let enu = d
            .measurement
            .position_enu()
            .expect("a radar plot is a position");
        assert!((enu[2] - origin_enu[2]).abs() < 20_000.0);
        assert_eq!(d.provenance.algorithm_version, "asterix.cat048/ed1.32");
        if d.provenance.conversion_loss.is_none() {
            lossless += 1;
        }
    }
    eprintln!(
        "capture: {lossless} of {} detections carried a 3D height and mapped without loss",
        sink.received.len()
    );
}

#[test]
fn service_messages_are_kept_apart_from_detections() {
    let packets = datagrams(PCAP);
    let origin = Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    };
    let frame = LocalFrame::new(origin);
    let source = ReplayDatagramSource::new(packets.iter().map(|(_, d)| d.clone()));
    let mut adapter = AsterixFeedAdapter::new("capture", source, &frame, &bindings(origin));

    let detections: Vec<DetectionView> = packets
        .iter()
        .flat_map(|(ts, d)| adapter.handle_datagram(d, MissionTime(*ts)))
        .collect();
    let reports = adapter.drain_service_reports();
    let stats = adapter.stats();

    assert_eq!(detections.len(), 126);
    assert_eq!(reports.len(), 34);
    assert_eq!((stats.blocks_cat048, stats.blocks_cat034), (86, 34));
    assert_eq!(stats.not_detections, 2);
    assert_eq!(
        stats.malformed_datagrams
            + stats.malformed_blocks
            + stats.unknown_radar
            + stats.unsupported_category_blocks,
        0
    );
    let sector_crossings = reports
        .iter()
        .filter(|r| matches!(r.event, ServiceEvent::SectorCrossing(_)))
        .count();
    let north_markers = reports
        .iter()
        .filter(|r| r.event == ServiceEvent::NorthMarker)
        .count();
    assert_eq!((sector_crossings, north_markers), (32, 2));
    // A status is claimed only where I034/050 was sent, and then it says what it says.
    let with_status: Vec<_> = reports.iter().filter_map(|r| r.status).collect();
    eprintln!(
        "capture: {} of {} service messages carried a system status: {with_status:?}",
        with_status.len(),
        reports.len()
    );
    assert!(
        with_status.len() < reports.len(),
        "the sector crossings seen on copy carried no I034/050"
    );
}

#[test]
fn an_unbound_radar_never_reaches_the_gateway() {
    let packets = datagrams(PCAP);
    let origin = Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    };
    let frame = LocalFrame::new(origin);
    let source = ReplayDatagramSource::new(packets.iter().map(|(_, d)| d.clone()));
    // Only SIC 11 is bound; the other six radars are strangers.
    let one = bindings(origin).into_iter().take(1).collect::<Vec<_>>();
    let adapter = AsterixFeedAdapter::new("capture", source, &frame, &one);
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: vec![SensorId(1)],
    }));
    gateway.add_adapter(Box::new(adapter));
    let mut sink = SinkSpy::default();
    let events = gateway.tick(MissionTime(packets.last().expect("packets").0), &mut sink);
    assert!(!sink.received.is_empty());
    assert!(sink.received.len() < 126);
    assert!(sink.received.iter().all(|d| d.sensor == SensorId(1)));
    assert_eq!(
        events.len(),
        sink.received.len(),
        "nothing quarantined: strangers never became detections"
    );
}

/// The hand-built Category 205 and Category 129 records through the same adapter and the
/// real gateway, for the "`gungnir-ingest` Validation and quarantine; the ASTERIX radar,
/// direction-finder and UAS-identification adapter" row of
/// `docs/verification-capability-table.md` §2. The adapter's own tests stop at the adapter;
/// here the direction finder's bearing and the UAS gateway's position are each authenticated,
/// validated and accepted, the bearing reaches the tracking service with no position on it,
/// and the UAS report is queued beside its detection (GAP-100, GAP-101).
#[test]
fn the_hand_built_bearing_and_uas_position_are_accepted_by_the_gateway() {
    // 10 N, 20 W at sea level: close to where the Category 129 record puts the UAS
    // (SOURCE.md), so its offset from the origin can be worked by hand below.
    let origin = Geodetic {
        lat_rad: 10.0_f64.to_radians(),
        lon_rad: (-20.0_f64).to_radians(),
        alt_m: 0.0,
    };
    let frame = LocalFrame::new(origin);
    // A stand-in accuracy: no direction finder's Interface Control Document stands behind a
    // hand-built record.
    let sigma = 1.5_f64.to_radians();
    // The SAC/SIC pairs the two records carry (SOURCE.md): 99/1 and 0/0.
    let direction_finder = DfBinding {
        sac: 99,
        sic: 1,
        sensor: SensorId(8),
        position: origin,
        azimuth_sigma_rad: sigma,
    };
    let uas_gateway = UasBinding {
        sac: 0,
        sic: 0,
        sensor: SensorId(9),
    };
    let uas_reports = UasIdentificationSink::default();
    let source = ReplayDatagramSource::new(vec![CAT205.to_vec(), CAT129.to_vec()]);
    let adapter = AsterixFeedAdapter::new("hand-built", source, &frame, &[])
        .with_df_sites(&[direction_finder], &frame)
        .with_uas_sites(&[uas_gateway])
        .with_uas_report_sink(uas_reports.clone());

    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: vec![SensorId(8), SensorId(9)],
    }));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = SinkSpy::default();

    // Both records say 12:00:00 (I205/030 and I129/070 in SOURCE.md) and carry no date, which
    // the codecs take from the receipt time. Received half a second after noon on day 20 000
    // of the Unix epoch, both are stamped half a second before receipt: inside the gateway's
    // timestamp rules, so validation is decided by the measurements.
    let noon = 20_000.0 * 86_400.0 + 43_200.0;
    let events = gateway.tick(MissionTime(noon + 0.5), &mut sink);

    assert_eq!(events.len(), 2, "{events:#?}");
    assert!(
        events.iter().all(|e| matches!(e, IngestEvent::Accepted(_))),
        "{events:#?}"
    );
    assert_eq!(
        gateway.stats(),
        IngestStats {
            accepted: 2,
            quarantined: 0,
            adapter_failures: 0,
            not_accepted: 0,
            rehearsal_refused: 0,
        }
    );
    assert_eq!(sink.received.len(), 2, "{:#?}", sink.received);

    // The bearing: 4500 counts at 0.01 degrees (SOURCE.md) and the variance this binding
    // states. The record's signal elevation has no stated error anywhere in the category, so
    // the codec drops it rather than handing the gateway an elevation without one.
    let bearing = &sink.received[0];
    assert_eq!(bearing.sensor, SensorId(8));
    assert!((bearing.source_time.0 - noon).abs() < 1e-6);
    match bearing.measurement {
        Measurement::Bearing {
            azimuth_rad,
            elevation_rad,
            azimuth_variance_rad2,
            elevation_variance_rad2,
        } => {
            assert!((azimuth_rad - 45.0_f64.to_radians()).abs() < 1e-12);
            assert!((azimuth_variance_rad2 - sigma * sigma).abs() < 1e-18);
            assert!(elevation_rad.is_none() && elevation_variance_rad2.is_none());
        }
        ref other => panic!("a direction finder reports a bearing, not {other:?}"),
    }
    assert!(bearing.measurement.position_enu().is_none());

    // The UAS's own position, worked by hand. I129/080's wire counts (SOURCE.md) are
    // 0x038E38E3 = 59 652 323 and 0xF8E38E39 = -119 304 647 (two's complement) at 180/2^30
    // degrees each: 9.3132e-8 degrees south of 10 N and 1.8626e-8 degrees east of 20 W. On
    // WGS-84 at 10 N and 500 m up, a degree of latitude is (M + h) pi/180 = 110 616 m, with
    // the meridian radius M = a(1 - e^2)/(1 - e^2 sin^2 10deg)^1.5 = 6 337 358 m, and a degree
    // of longitude is (N + h) cos 10deg pi/180 = 109 648 m, with N = a/(1 - e^2 sin^2 10deg)^0.5
    // = 6 378 781 m. So the fix is 10.302 mm south and 2.042 mm east of the origin, and
    // 500.0 m above it (I129/090: 5000 counts at 0.1 m).
    let uas = &sink.received[1];
    assert_eq!(uas.sensor, SensorId(9));
    assert!((uas.source_time.0 - noon).abs() < 1e-6);
    let enu = uas
        .measurement
        .position_enu()
        .expect("a UAS's own reported position is a position");
    assert!((enu[0] - 0.002_042).abs() < 1e-5, "{enu:?}");
    assert!((enu[1] + 0.010_302).abs() < 1e-5, "{enu:?}");
    assert!((enu[2] - 500.0).abs() < 1e-6, "{enu:?}");

    let reports: Vec<_> = uas_reports.lock().expect("sink").drain(..).collect();
    assert_eq!(reports.len(), 1, "{reports:#?}");
    assert_eq!(reports[0].sensor, SensorId(9));
    assert_eq!(reports[0].registration_country, "US");
}
