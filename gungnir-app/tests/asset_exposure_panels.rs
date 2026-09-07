// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The asset ranking reaches the panels (GAP-026, DN-01 §7).
//!
//! `AssetListAssessor` scored every frame and nothing drew it. These are the wiring's
//! tests: PN-17's lines name the asset and its priority from the baseline, PN-04's factors
//! carry the priority's weight, and with no assets declared both say so.

use gungnir_app::state::AppState;
use gungnir_app::sustainment;
use gungnir_config::{AssetConfig, ConfigBaseline};
use gungnir_model::{
    Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
    TrackView,
};
use gungnir_tracking_service::{SubmitError, TrackingService};

struct Picture(Vec<TrackView>);
impl TrackingService for Picture {
    fn submit_detection(&mut self, _: gungnir_model::DetectionView) -> Result<(), SubmitError> {
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

fn desktop(name: &str, assets: Vec<AssetConfig>) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-exposure-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([0.959_931, 0.209_440, 0.0]),
        assets,
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    // One track 2 km east of the origin, closing westward at 100 m/s.
    let mut track = TrackView {
        id: TrackId(42),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    };
    track.state[0] = 2_000.0;
    track.state[3] = -100.0;
    state.tracking = Box::new(Picture(vec![track]));
    (state, dir)
}

fn harbour() -> AssetConfig {
    AssetConfig {
        id: 1,
        name: "the harbour".into(),
        position: [0.959_931, 0.209_440, 0.0],
        radius_m: Some(500.0),
        priority: "high".into(),
        warning_lead_time_s: None,
        warning_channel: None,
        warning_within_m: None,
        note: None,
    }
}

#[test]
fn the_exposure_lines_name_the_asset_and_its_priority() {
    let (state, dir) = desktop("lines", vec![harbour()]);
    let ranking = sustainment::asset_exposure(&state);
    assert!(ranking.reason().is_none(), "{:?}", ranking.reason());
    let lines = sustainment::exposure_lines(&state, &ranking);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].track, 42);
    assert_eq!(lines[0].asset, "the harbour");
    assert_eq!(lines[0].priority, "high");
    assert!(lines[0].score > 0.0);

    let factors = sustainment::score_factors(&state, &ranking, TrackId(42));
    assert!(
        factors.iter().any(|(n, _)| n.contains("the harbour")),
        "{factors:?}"
    );
    let weight = factors
        .iter()
        .find(|(n, _)| n.starts_with("priority high"))
        .map(|(_, w)| *w)
        .expect("a priority factor");
    assert!(
        (f64::from(weight) - gungnir_model::AssetPriority::High.weight()).abs() < 1e-6,
        "the priority's contribution is its weight"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// With no assets declared, the panels say so; an empty list would read as "nothing is
/// threatened".
#[test]
fn no_assets_is_a_reason_not_an_empty_ranking() {
    let (state, dir) = desktop("none", Vec::new());
    let ranking = sustainment::asset_exposure(&state);
    assert!(ranking
        .reason()
        .is_some_and(|r| r.contains("no defended assets")));
    assert!(sustainment::exposure_lines(&state, &ranking).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
