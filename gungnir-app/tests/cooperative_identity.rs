// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cooperative identity on the desktop (GAP-010): an AIS feed named in the baseline is
//! bound, its reports are associated with the nearest track, the identification engine
//! holds the evidence and PN-04 shows it, the anomaly detectors see the association, and
//! a report with no track near it is counted rather than invented into one.

use gungnir_app::state::AppState;
use gungnir_app::{cooperative, update};
use gungnir_config::{AisFeedConfig, AisSource, ConfigBaseline, SensorConfig};
use gungnir_model::{
    Classification, DetectionView, MissionTime, Provenance, Quality, Releasability, TrackId,
    TrackStatus, TrackView,
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

fn track(id: u64, e: f64, n: f64, class: Classification) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(e, n, 0.0, 3.0, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 25.0,
        classification: class,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

/// The type 1 report in the recording places its vessel at 47.5828 N, 122.3458 W; the
/// origin is put there, so the vessel is at the frame's origin.
const ORIGIN: [f64; 3] = [0.830_477_1, -2.135_337_6, 0.0];

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-ais-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let recording = dir.join("ais.log");
    std::fs::write(
        &recording,
        "!AIVDM,2,1,9,B,55R3Vn82=ILTQ3KKS>1<D60Dq@E918U<F222221J1`?164vc03S1CCAD,0*2A\n\
         !AIVDM,2,2,9,B,`88888888888880,2*76\n\
         $GPRMC,213950.00,A,5250.53669,N,00542.34920,E,0.020,,070420,,,A*7D\n\
         !AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5C\n",
    )
    .expect("recording");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        sensors: vec![SensorConfig {
            id: 10,
            modality: "ais".into(),
            position: ORIGIN,
            max_range_m: 60_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }],
        ais_feeds: vec![AisFeedConfig {
            name: "harbour".into(),
            sensor_id: 10,
            source: AisSource::File {
                path: recording.to_string_lossy().into_owned(),
            },
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
fn a_recorded_ais_feed_becomes_evidence_on_the_nearest_track() {
    let (mut state, dir) = desktop("evidence");
    assert_eq!(state.ais_sinks.len(), 1, "{:?}", state.alerts);
    // One track on the vessel, one far away, one near but already declared hostile.
    state.tracking = Box::new(Picture(vec![
        track(1, 40.0, -30.0, Classification::Unknown),
        track(2, 20_000.0, 0.0, Classification::Unknown),
    ]));
    // The gateway polls the adapter on one tick and the association runs on the same
    // tick or the next; two ticks cover both orders.
    update::tick(&mut state);
    update::tick(&mut state);

    let feeds = cooperative::feed_lines(&state);
    assert_eq!(feeds.len(), 1);
    assert_eq!(feeds[0].name, "harbour");
    assert_eq!(
        (
            feeds[0].sentences,
            feeds[0].positions,
            feeds[0].static_reports
        ),
        (4, 1, 1)
    );

    let lines = cooperative::evidence_lines(&state, TrackId(1));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].source.contains("477553000"), "{}", lines[0].source);
    assert_eq!(lines[0].supports, Classification::Neutral);
    assert!(cooperative::evidence_lines(&state, TrackId(2)).is_empty());
    let last = state
        .cooperative
        .by_track
        .get(&TrackId(1))
        .expect("associated");
    assert_eq!(last.mmsi, 477_553_000);
    assert!(last.separation_m < 100.0, "{}", last.separation_m);
    assert!(!last.disagrees);
    assert_eq!(
        (state.cooperative.matched, state.cooperative.unmatched),
        (1, 0)
    );
    // No threshold is configured for Neutral, so the engine asks for a person rather
    // than declaring (DN-08 §5).
    let sentence = cooperative::decision_sentence(&state, TrackId(1));
    assert!(sentence.contains("needs an operator"), "{sentence}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_report_with_no_track_near_it_is_counted_and_a_hostile_track_disagrees() {
    let (mut state, dir) = desktop("unmatched");
    state.tracking = Box::new(Picture(vec![track(
        7,
        30_000.0,
        0.0,
        Classification::Unknown,
    )]));
    update::tick(&mut state);
    update::tick(&mut state);
    assert_eq!(
        (state.cooperative.matched, state.cooperative.unmatched),
        (0, 1)
    );
    assert!(state.cooperative.by_track.is_empty());

    let (mut state, dir2) = desktop("hostile");
    state.tracking = Box::new(Picture(vec![track(9, 10.0, 10.0, Classification::Hostile)]));
    update::tick(&mut state);
    update::tick(&mut state);
    let last = state
        .cooperative
        .by_track
        .get(&TrackId(9))
        .expect("associated");
    assert!(
        last.disagrees,
        "a self-declared civil vessel on a hostile track disagrees"
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
}
