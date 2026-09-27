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
//!
//! **GAP-196: the height.** The last three tests are the exception, because what they
//! test is the desktop's own wiring: the geoid model it lends every bound feed once its
//! grid check settles (`gungnir_app::geoid::lend_to_feeds`). They build the adapter
//! `bind_feed` builds, holding the desktop's own handle, and put real datagrams through
//! it -- the committed `cat129.raw` fixture (10 N, 20 W) and a record over the Baltic,
//! inside the committed EGM2008 clip (`testdata/geoid/`) -- then hand the report to the
//! desktop's sink and read PN-03's and PN-04's flags. With the clip (a `crs` build) the
//! Baltic height gains N; off the clip, with no grid, or without `crs`, it stays above
//! mean sea level and is said to.

use gungnir_app::state::AppState;
use gungnir_app::{cooperative, uas, update};
use gungnir_config::{ConfigBaseline, RadarFeedConfig, SensorConfig, UasSiteConfig};
use gungnir_ingest::adapters::asterix::{
    bind_feed, AsterixFeedAdapter, FeedSinks, FeedSpec, UasBinding, UdpDatagramSource,
};
use gungnir_model::{
    Classification, DetectionView, Geodetic, LocalFrame, MissionTime, Provenance, Quality,
    Releasability, SensorId, TrackId, TrackStatus, TrackView, UasAltitudeReference,
    UasIdentificationReport,
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
        altitude_reference: UasAltitudeReference::GeoidCorrected {
            model: "EGM2008".into(),
            separation_m: -22.6,
        },
        altitude_amsl_m: Some(82.6),
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
    desktop_with_grid_dir(name, None)
}

/// `grid_dir` is the baseline's `geoid_grid_dir`; `None` leaves the desktop to look in
/// `PROJ_DATA`, as the tests above that do not read the height do.
fn desktop_with_grid_dir(name: &str, grid_dir: Option<&str>) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-uas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let config = ConfigBaseline {
        geoid_grid_dir: grid_dir.map(|g| dir.join(g).to_string_lossy().into_owned()),
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        sensors: vec![SensorConfig {
            id: 11,
            modality: "uas-gateway".into(),
            position: ORIGIN,
            max_range_m: 20_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
            detection_model: None,
            azimuth_sector: None,
            elevation_band: None,
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

// ---------------------------------------------------------------------------
// GAP-196: the height a report states above mean sea level
// ---------------------------------------------------------------------------

/// The committed fixture: 10 N, 20 W, I129/090 = 500.0 m (`testdata/asterix/SOURCE.md`).
const CAT129_RAW: &[u8] = include_bytes!("../../testdata/asterix/cat129.raw");

/// `testdata/geoid/SOURCE.md` records this digest for the committed EGM2008 clip.
#[cfg(feature = "crs")]
const CLIP_SHA256: &str = "60a16af44ca47724fd6cbb58565104a010dd2ef8c2d5ec1c666552052fa83e10";

/// A record over the Baltic, inside the clip: SAC 0 SIC 0, "PL", 12:00:00, I129/080 the
/// counts nearest 54.5 N and 15.0 E (`0x1360_B60B`, `0x0555_5555`: 54.499 999 937 N,
/// 14.999 999 944 E), I129/090 = 120.0 m (`0x00_04B0`). FSPEC `0x87, 0xC0` flags FRN 1,
/// 6, 7, 8, 9. 54.5 N, 15.0 E is a node of the grid, where N is the node's own value,
/// 34.678050994873047 m (`gungnir-data/tests/geoid.rs`'s `PYPROJ`, from an independent
/// `pyproj` run over the full pinned grid); the counts sit 7e-8 degrees off it, which
/// moves N by well under a micrometre at this grid's gradients.
fn baltic_datagram() -> Vec<u8> {
    vec![
        0x81, 0x00, 0x17, 0x87, 0xC0, 0, 0, b'P', b'L', 0x54, 0x60, 0x00, 0x13, 0x60, 0xB6, 0x0B,
        0x05, 0x55, 0x55, 0x55, 0x00, 0x04, 0xB0,
    ]
}

/// The adapter `bind_feed` builds for the baseline's UAS gateway, holding the desktop's
/// own geoid handle -- the one `gungnir_app::geoid::lend_to_feeds` sets.
fn feed_holding_the_desktops_geoid(state: &AppState) -> AsterixFeedAdapter<UdpDatagramSource> {
    let spec = FeedSpec {
        name: "uas-gateway".into(),
        bind_addr: "127.0.0.1:0".parse().expect("loopback, any port"),
        multicast: None,
        radars: Vec::new(),
        df_sites: Vec::new(),
        uas_sites: vec![UasBinding {
            sac: 0,
            sic: 0,
            sensor: SensorId(11),
        }],
    };
    let sinks = FeedSinks {
        geoid: state.geoid_feeds.clone(),
        ..FeedSinks::default()
    };
    bind_feed(&spec, &frame(), &sinks).expect("a loopback socket always binds")
}

fn frame() -> LocalFrame {
    LocalFrame::new(Geodetic {
        lat_rad: ORIGIN[0],
        lon_rad: ORIGIN[1],
        alt_m: ORIGIN[2],
    })
}

fn one_report(
    adapter: &mut AsterixFeedAdapter<UdpDatagramSource>,
    bytes: &[u8],
) -> UasIdentificationReport {
    let detections = adapter.handle_datagram(bytes, MissionTime(43_205.0));
    assert_eq!(detections.len(), 1, "one record, one detection");
    let mut reports = adapter.drain_uas_reports();
    assert_eq!(reports.len(), 1);
    reports.remove(0)
}

/// Associate `report` with a track standing exactly where it is placed, and tick.
fn associate(state: &mut AppState, report: UasIdentificationReport) -> TrackId {
    let enu = frame().to_enu(report.position);
    state.tracking = Box::new(Picture(vec![track(5, enu[0], enu[1])]));
    state.uas_sinks[0].lock().expect("sink").push_back(report);
    update::tick(state);
    TrackId(5)
}

/// **The no-grid path (D-124)**, through the desktop's own wiring: a baseline naming a
/// grid directory that holds none. Every bound feed holds the reason; a Baltic report's
/// height stays 120.0 m above mean sea level, flagged with that reason; PN-04 says so in
/// the warning sense and PN-03 marks the track -- never an ellipsoidal height.
#[test]
fn with_no_grid_a_uas_height_stays_above_mean_sea_level_and_says_so() {
    let (mut state, dir) = desktop_with_grid_dir("no-grid", Some("no-grid-here"));
    update::tick(&mut state);
    let reason = state.geoid_feeds.current().err().expect("no grid is lent");
    assert!(
        reason.contains("no verified EGM2008 geoid grid"),
        "{reason}"
    );

    let mut adapter = feed_holding_the_desktops_geoid(&state);
    let report = one_report(&mut adapter, &baltic_datagram());
    let UasAltitudeReference::MeanSeaLevelUncorrected { reason } = &report.altitude_reference
    else {
        panic!("{:?}", report.altitude_reference);
    };
    assert!(reason.contains("is not there"), "{reason}");
    assert!((report.position.alt_m - 120.0).abs() < 1e-9);
    assert_eq!(adapter.stats().uas_heights_msl_uncorrected, 1);

    let track = associate(&mut state, report);
    let (line, ellipsoidal) = uas::height_line(&state, track).expect("a report is associated");
    assert!(!ellipsoidal);
    assert!(
        line.contains("120.0 m above mean sea level")
            && line.contains("NOT corrected to the ellipsoid"),
        "{line}"
    );
    assert_eq!(uas::msl_height_tracks(&state), vec![track]);
    let _ = std::fs::remove_dir_all(dir);
}

/// **The correction (D-123)** with the committed clip installed as the verified grid (a
/// test installs a status it verified itself, as `tests/terrain.rs` does): the desktop
/// lends it to its feeds on the next tick, the Baltic report's height becomes
/// 120.0 m + N as a WGS-84 ellipsoidal one, and PN-03 marks nothing. The fixture's own
/// position, 10 N 20 W, is off the clip: that report stays above mean sea level, with
/// PROJ's refusal as the reason -- never a zero separation.
#[test]
#[cfg(feature = "crs")]
fn with_the_verified_grid_a_uas_height_reaches_the_ellipsoid() {
    let (mut state, dir) = desktop_with_grid_dir("clip", Some("no-grid-here"));
    let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/geoid/egm08_25_clip_53n56n_13e17e.tif");
    state.geoid = gungnir_app::geoid::GeoidStatus::Verified {
        grid: gungnir_data::geoid::GeoidGrid::verify(&clip, CLIP_SHA256).expect("the clip"),
        source: gungnir_app::geoid::GridSource::Baseline,
    };
    update::tick(&mut state);
    assert!(state.geoid_feeds.is_available(), "{:?}", state.geoid_feeds);

    let mut adapter = feed_holding_the_desktops_geoid(&state);
    let report = one_report(&mut adapter, &baltic_datagram());
    let UasAltitudeReference::GeoidCorrected {
        model,
        separation_m,
    } = &report.altitude_reference
    else {
        panic!("{:?}", report.altitude_reference);
    };
    assert_eq!(model, "EGM2008");
    // The node value, from the independent pyproj run; the tolerance is a tenth of a
    // millimetre, far above the point's 7e-8 degree offset from the node and far below
    // the 34.7 m a dropped correction would miss by.
    assert!(
        (separation_m - 34.678_050_994_873_05).abs() < 1e-4,
        "{separation_m}"
    );
    assert!((report.position.alt_m - (120.0 + 34.678_050_994_873_05)).abs() < 1e-4);

    let off_grid = one_report(&mut adapter, CAT129_RAW);
    let UasAltitudeReference::MeanSeaLevelUncorrected { reason } = &off_grid.altitude_reference
    else {
        panic!("{:?}", off_grid.altitude_reference);
    };
    assert!(
        reason.contains("EGM2008 gives no separation here"),
        "{reason}"
    );
    assert!((off_grid.position.alt_m - 500.0).abs() < 1e-9);
    let s = adapter.stats();
    assert_eq!(
        (s.uas_heights_geoid_corrected, s.uas_heights_msl_uncorrected),
        (1, 1)
    );

    let track = associate(&mut state, report);
    let (line, ellipsoidal) = uas::height_line(&state, track).expect("a report is associated");
    assert!(ellipsoidal, "{line}");
    assert!(line.contains("WGS-84 ellipsoidal"), "{line}");
    assert!(uas::msl_height_tracks(&state).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A build without `crs` cannot read even a verified grid, so its feeds hold that
/// reason, naming the feature, and the height stays flagged; the build's own PN-09 line
/// already says it reads no grid, so no alert is added for it.
#[test]
#[cfg(not(feature = "crs"))]
fn without_crs_a_verified_grid_is_not_lent_and_the_reason_names_the_feature() {
    let (mut state, dir) = desktop_with_grid_dir("no-crs", Some("no-grid-here"));
    // Any verified file stands in for the grid here: nothing reads it without PROJ.
    let stand_in = dir.join("stand-in.tif");
    std::fs::write(&stand_in, b"abc").expect("writes");
    state.geoid = gungnir_app::geoid::GeoidStatus::Verified {
        grid: gungnir_data::geoid::GeoidGrid::verify(
            &stand_in,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .expect("SHA-256 of abc"),
        source: gungnir_app::geoid::GridSource::Baseline,
    };
    let alerts_before = state.alerts.len();
    update::tick(&mut state);
    let reason = state.geoid_feeds.current().err().expect("nothing is lent");
    assert!(reason.contains("crs"), "{reason}");
    assert!(
        !state.alerts[alerts_before..]
            .iter()
            .any(|a| a.contains("mean sea level")),
        "{:?}",
        state.alerts
    );

    let mut adapter = feed_holding_the_desktops_geoid(&state);
    let report = one_report(&mut adapter, CAT129_RAW);
    assert!(report.altitude_reference.is_msl_uncorrected());
    assert!((report.position.alt_m - 500.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(dir);
}
