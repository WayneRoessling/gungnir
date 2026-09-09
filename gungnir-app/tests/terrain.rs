// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain on the desktop (GAP-023): the configured DEM loads off the render thread,
//! line of sight is masked against it and the coverage parameters say so; a file whose
//! own frame contradicts the baseline is refused by name; a missing file fails loudly.
//!
//! `a_geotiff_that_declares_utm_is_refused_by_name...` below covers the default build
//! (no `crs-projection` feature, D-41's own build-feasibility caveat --
//! `docs/agentic-coding-standards.md` §2.9): declaring `frame: "local-enu"` against a
//! file that states EPSG:32633 is still refused, exactly as before D-41.
//! `a_geotiff_that_declares_utm_converts...`, behind that feature, is the same fixture
//! with `frame: "epsg:32633"` instead, now converting and loading.

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
    desktop_with_frame(name, path, "local-enu")
}

/// As `desktop`, but naming the terrain's declared frame (GAP-023, D-41) instead of
/// always `"local-enu"`.
fn desktop_with_frame(name: &str, path: String, frame: &str) -> (AppState, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-terrain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([0.959_931, 0.209_44, 0.0]),
        terrain: Some(TerrainConfig {
            path,
            frame: frame.into(),
        }),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

/// Tick until the terrain loader thread answers.
///
/// **A deadlock guard, not a performance assertion** (the reasoning is
/// `gungnir-tracking-service/tests/sample_set_replay.rs`'s): a real hang still fails,
/// and a loaded machine no longer does. The loop exits the moment the condition holds,
/// so a passing run costs what it always did.
fn settle(state: &mut AppState) {
    for _ in 0..6_000 {
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

/// GAP-023, D-41: the same UTM fixture as
/// `a_geotiff_that_declares_utm_is_refused_by_name_and_line_of_sight_stays_flat`, but
/// with `terrain.frame` declaring the CRS the file actually states instead of
/// contradicting it, and the `crs-projection` feature compiled in. Not run by default:
/// `proj-sys` links `libproj`, and no host this change could confirm build against had
/// the toolchain for it (`docs/agentic-coding-standards.md` §2.9's D-41 entry); a
/// dedicated CI workflow (`.github/workflows/crs-projection.yml`) is what actually
/// compiles and runs this, on a Linux runner with `cmake` installed.
#[cfg(feature = "crs-projection")]
#[test]
fn a_geotiff_that_declares_utm_converts_and_masks_line_of_sight_when_the_frame_matches() {
    let (mut state, dir) = desktop_with_frame("utm-converted", fixture("small.tif"), "epsg:32633");
    settle(&mut state);
    match &state.terrain {
        TerrainStatus::Loaded {
            vertices,
            triangles,
            ..
        } => {
            // Same underlying 5 by 4 grid, one no-data cell, as
            // `an_ascii_grid_in_the_local_frame_loads_and_masks_line_of_sight`'s ASCII
            // fixture (`testdata/dem/SOURCE.md`: both encode the identical grid).
            // Reprojection moves where a vertex sits, never which ones exist or how
            // they triangulate, so the topology should be unchanged.
            assert_eq!(*vertices, 20, "{:?}", state.terrain);
            assert_eq!(*triangles, 16, "{:?}", state.terrain);
        }
        other => panic!("expected the UTM fixture to convert and load: {other:?}"),
    }
    assert!(state.terrain.line().contains("masking line of sight"));
    assert_eq!(state.data.terrains.len(), 1);
    let mesh = &state.data.terrains[0];
    assert!(
        mesh.positions
            .iter()
            .all(|p| p[0].is_finite() && p[1].is_finite()),
        "every converted vertex's [e, n] must be finite: {:?}",
        mesh.positions
    );
    // The one no-data cell's height stays NaN through the conversion (`TerrainMesh::
    // with_xy` carries `z` through unchanged); every other vertex keeps a real height.
    assert_eq!(
        mesh.positions.iter().filter(|p| p[2].is_nan()).count(),
        1,
        "{:?}",
        mesh.positions
    );
    // Two adjacent cell centres in the fixture's own frame are exactly 30 m apart
    // (SOURCE.md). Over this fixture's 150 m by 120 m extent, UTM zone 33N's distortion
    // from true scale is many orders of magnitude below a centimetre (its 0.9996
    // central-meridian scale factor barely moves across 150 m), so the same two
    // points' Euclidean distance after conversion to local ENU should still read as
    // 30 m to well within a centimetre. A reprojection that silently swapped an axis,
    // mixed up degrees and radians, or dropped a scale factor would miss this by
    // orders of magnitude, not by noise -- this is a coarse but effective tripwire for
    // exactly those bugs, independent of knowing this fixture's absolute position.
    let adjacent_spacing_m = {
        let a = mesh.positions[0];
        let b = mesh.positions[1];
        (f64::from(a[0] - b[0]).powi(2) + f64::from(a[1] - b[1]).powi(2)).sqrt()
    };
    assert!(
        (adjacent_spacing_m - 30.0).abs() < 0.01,
        "adjacent cell spacing: {adjacent_spacing_m} m"
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
