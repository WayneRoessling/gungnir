// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain on the desktop (GAP-023): the configured DEM loads off the render thread,
//! line of sight is masked against it and the coverage parameters say so; a file whose
//! own frame contradicts the baseline is refused by name; a missing file fails loudly.

use gungnir_app::state::AppState;
use gungnir_app::terrain::TerrainStatus;
use gungnir_app::{sustainment, update};
use gungnir_config::{ConfigBaseline, TerrainConfig};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/dem")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn desktop(name: &str, path: String) -> (AppState, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-terrain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([0.959_931, 0.209_44, 0.0]),
        terrain: Some(TerrainConfig {
            path,
            frame: "local-enu".into(),
        }),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

fn settle(state: &mut AppState) {
    for _ in 0..200 {
        update::tick(state);
        if !matches!(state.terrain, TerrainStatus::Loading { .. }) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the loader never answered: {:?}", state.terrain);
}

#[test]
fn an_ascii_grid_in_the_local_frame_loads_and_masks_line_of_sight() {
    let (mut state, dir) = desktop("asc", fixture("small.asc"));
    assert!(
        matches!(state.terrain, TerrainStatus::NotConfigured),
        "nothing until the first frame"
    );
    update::tick(&mut state);
    assert!(
        !matches!(state.terrain, TerrainStatus::NotConfigured),
        "the first frame starts the load: {:?}",
        state.terrain
    );
    settle(&mut state);
    assert!(
        matches!(
            state.terrain,
            TerrainStatus::Loaded {
                vertices: 20,
                triangles: 16,
                ..
            }
        ),
        "{:?}",
        state.terrain
    );
    assert_eq!(state.data.terrains.len(), 1);
    assert!(state.terrain.line().contains("masking line of sight"));
    // The placed surface sits at the grid's own coordinates, not at the loader's origin.
    let sw = state.data.terrains[0].positions[15];
    assert!((sw[0] - 500_015.0).abs() < 1.0, "{sw:?}");
    assert!(
        sustainment::coverage_report(&state).is_none(),
        "no approaches declared"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_geotiff_that_declares_utm_is_refused_by_name_and_line_of_sight_stays_flat() {
    let (mut state, dir) = desktop("utm", fixture("small.tif"));
    settle(&mut state);
    match &state.terrain {
        TerrainStatus::Failed { reason, .. } => {
            assert!(reason.contains("EPSG:32633"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert!(state.data.terrains.is_empty());
    assert!(!state.terrain.is_masking());
    assert!(
        state.alerts.iter().any(|a| a.contains("not placed")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_file_fails_loudly() {
    let (mut state, dir) = desktop("missing", fixture("absent.asc"));
    settle(&mut state);
    assert!(
        matches!(state.terrain, TerrainStatus::Failed { .. }),
        "{:?}",
        state.terrain
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("not loaded")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}
