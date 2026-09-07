// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Prediction on the desktop (GAP-020, DN-02) and the warning it obliges (GAP-042,
//! DN-03): a track heading for an asset with an obligation is predicted to arrive, PN-04
//! gets an approach line naming the predictor, the viewport gets a dashed path, and a
//! warning is raised inside the lead time -- and, because no transport exists, is
//! `Failed` loudly rather than quietly not sent.

use gungnir_app::state::AppState;
use gungnir_app::{prediction, update, warnings};
use gungnir_config::{AssetConfig, ConfigBaseline, EndpointConfig};
use gungnir_eventing::Event;
use gungnir_model::events::WarningEvent;
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

/// A track at `e` metres east of the origin, flying west at `ve` (negative closes).
fn track(id: u64, e: f64, ve: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(e, 0.0, 50.0, ve, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 25.0,
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn desktop(name: &str, with_origin: bool) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-predict-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: with_origin.then_some([0.959_931, 0.209_44, 0.0]),
        assets: vec![AssetConfig {
            id: 1,
            name: "the harbour".into(),
            position: [0.959_931, 0.209_44, 0.0],
            radius_m: Some(100.0),
            priority: "high".into(),
            warning_lead_time_s: Some(120.0),
            warning_channel: Some("port-authority".into()),
            warning_within_m: None,
            note: None,
        }],
        endpoints: vec![EndpointConfig {
            name: "port-authority".into(),
            kind: "voice".into(),
            address: "vhf-16".into(),
        }],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

#[test]
fn a_closing_track_is_predicted_to_arrive_and_the_panel_names_the_predictor() {
    let (mut state, dir) = desktop("arrives", true);
    // 3000 m out at 50 m/s: the range is least at the centre, 60 s out (DN-02 §3), and
    // the distance to the boundary there is zero, which is what "arrives" means.
    state.tracking = Box::new(Picture(vec![track(1, 3000.0, -50.0)]));
    let outcome = prediction::predict(&state);
    assert!(outcome.reason().is_none(), "{outcome:?}");
    let approaches = prediction::approaches(&state, &outcome, TrackId(1));
    assert_eq!(approaches.len(), 1);
    assert_eq!(approaches[0].asset, "the harbour");
    assert!(approaches[0].arrives, "{approaches:?}");
    assert!(
        (approaches[0].time_ahead_s - 60.0).abs() < 1e-6,
        "{approaches:?}"
    );
    // GAP-020, 2026-09-06: the desktop predicts with the model its tracker runs, so
    // the name on the line is the filter's. The point of the assertion is unchanged:
    // every prediction says which predictor produced it, and an operator is never left
    // to assume.
    assert_eq!(
        prediction::predictor_name(approaches[0].predictor),
        "filter prediction"
    );
    let paths = prediction::paths(&outcome);
    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0].points.len(),
        state.config.assessment.prediction_horizons_s.len()
    );
    assert!(paths[0].uncertainty_drawable);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn without_an_origin_lines_are_drawn_and_approaches_are_not_claimed() {
    let (mut state, dir) = desktop("no-origin", false);
    state.tracking = Box::new(Picture(vec![track(1, 3000.0, -50.0)]));
    let outcome = prediction::predict(&state);
    assert_eq!(
        prediction::paths(&outcome).len(),
        1,
        "a line needs no origin"
    );
    assert!(prediction::approaches(&state, &outcome, TrackId(1)).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_warning_is_raised_inside_the_lead_time_and_fails_loudly_without_a_transport() {
    let (mut state, dir) = desktop("warns", true);
    let events = state.events.subscribe();
    // 20 km out at 50 m/s is 400 s away: outside the 120 s lead time, nothing owed.
    state.tracking = Box::new(Picture(vec![track(1, 20_000.0, -50.0)]));
    update::tick(&mut state);
    assert!(state.warnings.open().is_empty(), "outside the lead time");
    // 5 km out is 100 s away: owed, offered, refused, failed, alerted, journaled.
    state.tracking = Box::new(Picture(vec![track(1, 5_000.0, -50.0)]));
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(1.0),
    });
    update::tick(&mut state);
    assert_eq!(state.warnings.open().len(), 1);
    assert_eq!(
        state.warnings.failed_count(),
        1,
        "no transport, so failed and open"
    );
    let lines = warnings::lines(&state, Some(TrackId(1)));
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].state, "failed");
    assert!(lines[0].loud);
    // A voice channel has no transport: the failure names the kind and says to warn by
    // voice, which is the whole of what an operator can do with it.
    assert!(lines[0].detail.contains("voice"), "{}", lines[0].detail);
    assert!(
        state.alerts.iter().any(|a| a.contains("was not delivered")),
        "{:?}",
        state.alerts
    );
    let kinds: Vec<String> = events
        .try_iter()
        .filter_map(|env| match env.event {
            Event::Warning(WarningEvent::Raised { .. }) => Some("raised".to_string()),
            Event::Warning(WarningEvent::Failed { .. }) => Some("failed".to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["raised", "failed"]);
    // The same pair on the next frame raises nothing new and retries nothing.
    update::tick(&mut state);
    assert_eq!(state.warnings.open().len(), 1);
    // A person waives it, by name or by "nobody signed in".
    assert!(warnings::waive(
        &mut state,
        gungnir_model::AssetId(1),
        TrackId(1),
        "pilot launch".into()
    ));
    assert_eq!(warnings::lines(&state, None)[0].state, "waived");
    // The track turns away: the warning closes with its final state kept.
    state.tracking = Box::new(Picture(vec![track(1, 5_000.0, 50.0)]));
    update::tick(&mut state);
    assert!(state.warnings.open().is_empty());
    assert_eq!(state.warnings.closed().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
