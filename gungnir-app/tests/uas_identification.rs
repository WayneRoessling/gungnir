// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cooperative identity from ASTERIX Category 129 on the desktop (GAP-101): a UAS
//! gateway named in the baseline gets a sink this binary drains, the reports on it are
//! associated with the nearest track, the identification engine holds the evidence and
//! PN-04 names where it came from, a UAS broadcasting every second does not accumulate
//! into certainty, and a report with no track near it is counted rather than invented
//! into one.
//!
//! The reports are pushed onto the bound feed's own sink rather than sent over the
//! socket, because what is under test here is the desktop's consumer: the wire half --
//! datagram to block to record to `UasIdentificationReport` -- is gated in
//! `gungnir-interop` and `gungnir-ingest` against real Category 129 frames, and
//! repeating it here would test the codec a third time and this module not at all.

use gungnir_app::state::AppState;
use gungnir_app::{cooperative, uas, update};
use gungnir_config::{ConfigBaseline, RadarFeedConfig, SensorConfig, UasSiteConfig};
use gungnir_model::{
    Classification, DetectionView, Geodetic, MissionTime, Provenance, Quality, Releasability,
    SensorId, TrackId, TrackStatus, TrackView, UasIdentificationReport,
};
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};

struct Picture(Vec<TrackView>);
impl TrackingService for Picture {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &self.0
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn track(id: u64, e: f64, n: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(e, n, 0.0, 3.0, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 25.0,
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

const ORIGIN: [f64; 3] = [0.830_477_1, -2.135_337_6, 0.0];

/// A report from a UAS sitting over the frame origin, so its ENU position is (0, 0) and
/// a track's distance from it is that track's own coordinates.
fn report(serial: Option<[u8; 12]>) -> UasIdentificationReport {
    UasIdentificationReport {
        sensor: SensorId(11),
        source_time: MissionTime(99.0),
        receipt_time: MissionTime(99.5),
        position: Geodetic {
            lat_rad: ORIGIN[0],
            lon_rad: ORIGIN[1],
            alt_m: 60.0,
        },
        altitude_amsl_m: Some(60.0),
        altitude_agl_m: Some(58.0),
        gnss_signal_accuracy_m: Some(2.5),
        manufacturer_id: Some("ACM".into()),
        model_id: Some("X1M".into()),
        serial_number: serial,
        registration_country: "NL".into(),
        operational_risk: None,
        horizontal_velocity_enu_m_s: Some([12.0, -3.0]),
        vertical_velocity_m_s: Some(0.5),
        conversion_loss: None,
    }
}

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-uas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        sensors: vec![SensorConfig {
            id: 11,
            modality: "uas-gateway".into(),
            position: ORIGIN,
            max_range_m: 20_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
            azimuth_sector: None,
        }],
        radar_feeds: vec![RadarFeedConfig {
            name: "uas-gateway".into(),
            // Port 0: the socket is bound because a feed is bound, and nothing is sent
            // over it in this test, so no fixed port is claimed and two runs of this
            // suite cannot collide on one.
            bind_addr: "127.0.0.1:0".into(),
            multicast: None,
            radars: Vec::new(),
            df_sites: Vec::new(),
            uas_sites: vec![UasSiteConfig {
                sensor_id: 11,
                sac: 0,
                sic: 0,
            }],
        }],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    (state, dir)
}

#[test]
fn a_configured_uas_gateway_is_bound_with_a_sink_this_binary_drains() {
    let (state, dir) = desktop("bound");
    assert_eq!(state.uas_sinks.len(), 1, "{:?}", state.alerts);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_report_becomes_evidence_on_the_nearest_track_and_only_once() {
    let (mut state, dir) = desktop("evidence");
    state.tracking = Box::new(Picture(vec![
        track(1, 40.0, -30.0),
        track(2, 18_000.0, 0.0),
    ]));
    let serial = Some([0x01, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0f]);
    state.uas_sinks[0]
        .lock()
        .expect("sink")
        .push_back(report(serial));
    update::tick(&mut state);

    let lines = cooperative::evidence_lines(&state, TrackId(1));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].kind, "cooperative identity (ASTERIX Category 129)");
    assert_eq!(lines[0].supports, Classification::Neutral);
    assert_eq!(
        lines[0].source,
        "UAS NL ACM/X1M serial 01020000000000000000000f"
    );
    assert!(cooperative::evidence_lines(&state, TrackId(2)).is_empty());

    let last = state.uas.by_track.get(&TrackId(1)).expect("associated");
    assert_eq!(last.registration_country, "NL");
    assert!(last.separation_m < 100.0, "{}", last.separation_m);
    assert_eq!((state.uas.matched, state.uas.unmatched), (1, 0));

    // The same UAS again: associated again, and not submitted again -- a report every
    // second must not accumulate into certainty.
    state.uas_sinks[0]
        .lock()
        .expect("sink")
        .push_back(report(serial));
    update::tick(&mut state);
    assert_eq!(cooperative::evidence_lines(&state, TrackId(1)).len(), 1);
    assert_eq!((state.uas.matched, state.uas.unmatched), (2, 0));

    // No threshold is configured for Neutral, so the engine asks for a person rather
    // than declaring (DN-08 §5), the same as it does for an AIS claim.
    let sentence = cooperative::decision_sentence(&state, TrackId(1));
    assert!(sentence.contains("needs an operator"), "{sentence}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_report_with_no_track_near_it_is_counted_and_invents_nothing() {
    let (mut state, dir) = desktop("unmatched");
    state.tracking = Box::new(Picture(vec![track(7, 30_000.0, 0.0)]));
    state.uas_sinks[0]
        .lock()
        .expect("sink")
        .push_back(report(None));
    update::tick(&mut state);
    assert_eq!((state.uas.matched, state.uas.unmatched), (0, 1));
    assert!(state.uas.by_track.is_empty());
    assert!(cooperative::evidence_lines(&state, TrackId(7)).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Two UAS over one crowded piece of sky, each broadcasting its own identity: the
/// nearest track takes each claim, and the claims are told apart -- which is what the
/// identity half of the deduplication key is for, since Category 129 has no per-UAS
/// identifier to key on the way AIS has an MMSI.
#[test]
fn two_different_uas_on_one_track_are_two_claims() {
    let (mut state, dir) = desktop("two");
    state.tracking = Box::new(Picture(vec![track(3, 10.0, 10.0)]));
    let mut second = report(None);
    second.manufacturer_id = Some("BQD".into());
    second.model_id = Some("R2".into());
    {
        let mut sink = state.uas_sinks[0].lock().expect("sink");
        sink.push_back(report(None));
        sink.push_back(second);
    }
    update::tick(&mut state);

    let mut sources: Vec<String> = cooperative::evidence_lines(&state, TrackId(3))
        .into_iter()
        .map(|e| e.source)
        .collect();
    sources.sort();
    assert_eq!(sources, vec!["UAS NL ACM/X1M", "UAS NL BQD/R2"]);
    // The record keeps the most recent claim, not both.
    assert_eq!(
        state
            .uas
            .by_track
            .get(&TrackId(3))
            .expect("associated")
            .claim,
        uas::claim_of(&{
            let mut r = report(None);
            r.manufacturer_id = Some("BQD".into());
            r.model_id = Some("R2".into());
            r
        })
    );
    let _ = std::fs::remove_dir_all(dir);
}
