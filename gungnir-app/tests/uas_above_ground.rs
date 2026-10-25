// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A UAS reporting only its height above ground, through the desktop's own wiring
//! (GAP-199, D-130): the configured DEM loads, `gungnir_app::ground::lend_to_feeds` lends
//! it to the bound feeds when the baseline states its vertical error, and a Category 129
//! record with I129/100 and no I129/090 lands at the ground's height plus its own. Without
//! the error the height is unknown, with that reason, and PN-03 marks the track.
//!
//! The DEM is written here: a 5-by-5 grid of 30 m cells in the local frame, centred on the
//! origin, rising 0.1 m per metre east from 100 m at the origin. The record is the
//! ingest adapter's own fixture, placed at the I129/080 counts nearest the origin
//! (0.9 rad N, 0.2 rad E), with I129/100 = 120.0 m.

use gungnir_app::state::AppState;
use gungnir_app::terrain::TerrainStatus;
use gungnir_app::uas::{self, LastUasIdentification};
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, TerrainConfig};
use gungnir_ingest::adapters::asterix::{bind_feed, FeedSinks, FeedSpec, UasBinding};
use gungnir_model::{
    Geodetic, LocalFrame, Measurement, MissionTime, SensorId, TrackId, UasAltitudeReference,
};
use std::path::PathBuf;

const ORIGIN: [f64; 3] = [0.9, 0.2, 0.0];

/// SAC 10 SIC 20, country "DE", 12:00:00, the counts nearest the origin, I129/100 =
/// 120.0 m (`0x00_04B0`); FSPEC flags FRN 1, 6, 7 and FRN 8, 10.
const AGL_RECORD: [u8; 23] = [
    0x81, 0x00, 0x17, 0x87, 0xA0, 10, 20, b'D', b'E', 0x54, 0x60, 0x00, 0x12, 0x55, 0xAB, 0x96,
    0x04, 0x13, 0x09, 0xB0, 0x00, 0x04, 0xB0,
];

fn desktop(name: &str, vertical_sigma_m: Option<f64>) -> (AppState, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-agl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let row = "94 97 100 103 106\n";
    let dem = dir.join("plane.asc");
    std::fs::write(
        &dem,
        format!(
            "ncols 5\nnrows 5\nxllcorner -75.0\nyllcorner -75.0\ncellsize 30.0\n\
             NODATA_value -9999\n{}",
            row.repeat(5)
        ),
    )
    .expect("writes the DEM");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        terrain: Some(TerrainConfig {
            path: dem.to_string_lossy().into_owned(),
            frame: "local-enu".into(),
            vertical: None,
            nad83_realisation: None,
            vertical_sigma_m,
        }),
        geoid_grid_dir: Some(dir.join("no-grid-here").to_string_lossy().into_owned()),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

/// Tick until the terrain has loaded; a deadlock guard, as `tests/terrain.rs`'s `settle`.
fn settle(state: &mut AppState) {
    for _ in 0..6_000 {
        update::tick(state);
        if matches!(state.terrain, TerrainStatus::Loaded { .. }) {
            // One more tick lends what the last one loaded.
            update::tick(state);
            return;
        }
        assert!(
            !matches!(state.terrain, TerrainStatus::Failed { .. }),
            "{:?}",
            state.terrain
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the loader never answered: {:?}", state.terrain);
}

fn report_through_the_desktops_terrain(
    state: &AppState,
) -> (gungnir_model::UasIdentificationReport, [f64; 3]) {
    let spec = FeedSpec {
        name: "uas-gateway".into(),
        bind_addr: "127.0.0.1:0".parse().expect("loopback, any port"),
        multicast: None,
        radars: Vec::new(),
        df_sites: Vec::new(),
        uas_sites: vec![UasBinding {
            sac: 10,
            sic: 20,
            sensor: SensorId(61),
        }],
    };
    let sinks = FeedSinks {
        terrain: state.terrain_feeds.clone(),
        ..FeedSinks::default()
    };
    let frame = LocalFrame::new(Geodetic {
        lat_rad: ORIGIN[0],
        lon_rad: ORIGIN[1],
        alt_m: ORIGIN[2],
    });
    let mut adapter = bind_feed(&spec, &frame, &sinks).expect("a loopback socket always binds");
    let detections = adapter.handle_datagram(&AGL_RECORD, MissionTime(43_205.0));
    assert_eq!(detections.len(), 1);
    let Measurement::Position { variance_m2, .. } = detections[0].measurement else {
        panic!("a position");
    };
    let report = adapter.drain_uas_reports().pop().expect("one report");
    (report, variance_m2)
}

/// Put `altitude` on track 5's record, as `uas::tick` would after association.
fn associated(state: &mut AppState, altitude: UasAltitudeReference) -> TrackId {
    state.uas.by_track.insert(
        TrackId(5),
        LastUasIdentification {
            claim: "DE".into(),
            registration_country: "DE".into(),
            at: MissionTime(43_205.0),
            separation_m: 0.0,
            altitude_amsl_m: None,
            altitude_agl_m: Some(120.0),
            altitude,
        },
    );
    TrackId(5)
}

/// **The placement (D-130).** The DEM is lent once it loads; the UAS lands at the
/// ground under it, 100 m, plus its 120 m, with the stated error in its up variance; PN-04
/// says it is ellipsoidal and PN-03 marks nothing.
#[test]
fn a_height_above_ground_lands_on_the_loaded_dem() {
    let (mut state, dir) = desktop("placed", Some(2.5));
    settle(&mut state);
    assert!(
        state.terrain_feeds.is_available(),
        "{:?}",
        state.terrain_feeds
    );

    let (report, variance) = report_through_the_desktops_terrain(&state);
    let UasAltitudeReference::TerrainPlaced {
        terrain,
        ground_m,
        ground_sigma_m,
    } = &report.altitude_reference
    else {
        panic!("{:?}", report.altitude_reference);
    };
    assert!(terrain.ends_with("plane.asc"), "{terrain}");
    // The record sits within centimetres of the origin, where the plane is 100 m; the
    // tolerance is far above that offset and the Earth's curvature over it, and far below
    // the 100 m a dropped ground height would miss by.
    assert!((ground_m - 100.0).abs() < 0.01, "{ground_m}");
    assert!((ground_sigma_m - 2.5).abs() < f64::EPSILON);
    assert!(
        (report.position.alt_m - 220.0).abs() < 0.01,
        "{}",
        report.position.alt_m
    );
    assert!((variance[2] - (900.0 + 6.25)).abs() < 1e-9, "{variance:?}");

    let track = associated(&mut state, report.altitude_reference.clone());
    let (line, ellipsoidal) = uas::height_line(&state, track).expect("associated");
    assert!(ellipsoidal, "{line}");
    assert!(line.contains("120.0 m above ground level"), "{line}");
    assert!(uas::unknown_height_tracks(&state).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// **No stated error, no placement.** The same DEM loads, but its vertical error is not
/// stated, so nothing is lent and the reason names the field; the height is unknown at 0,
/// weighed as unknown, and PN-03 marks the track.
#[test]
fn without_the_dems_error_a_height_above_ground_is_unknown_and_marked() {
    let (mut state, dir) = desktop("no-sigma", None);
    settle(&mut state);
    let reason = state
        .terrain_feeds
        .current()
        .err()
        .expect("nothing is lent");
    assert!(reason.contains("terrain.vertical_sigma_m"), "{reason}");

    let (report, variance) = report_through_the_desktops_terrain(&state);
    let UasAltitudeReference::NoAbsoluteHeight { reason } = &report.altitude_reference else {
        panic!("{:?}", report.altitude_reference);
    };
    assert!(reason.contains("terrain.vertical_sigma_m"), "{reason}");
    assert!(report.position.alt_m.abs() < 1e-12);
    let unknown = 900.0 + gungnir_model::UNKNOWN_UAS_HEIGHT_BOUND_M.powi(2);
    assert!((variance[2] - unknown).abs() < 1e-6, "{variance:?}");

    let track = associated(&mut state, report.altitude_reference.clone());
    let (line, ellipsoidal) = uas::height_line(&state, track).expect("associated");
    assert!(!ellipsoidal && line.contains("height unknown"), "{line}");
    assert_eq!(uas::unknown_height_tracks(&state), vec![track]);
    let _ = std::fs::remove_dir_all(dir);
}
