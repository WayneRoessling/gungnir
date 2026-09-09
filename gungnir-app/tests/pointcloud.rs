// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point clouds on the desktop (GAP-098): a configured source and target both load off
//! the render thread into `DataStore.point_clouds`, in that order; either failing
//! leaves neither behind, since a lone cloud is not the pair registration needs
//! (`gungnir-data-fusion::CpuIcp`, GAP-024, independent of this).
//!
//! **These tests force the CPU registration backend (GAP-024).** Once a pair
//! completes loading, `update::tick` reaches `crate::pointcloud::register`, which
//! calls `FusionBackend::engine_for` -- and this crate's own hard rule is that no
//! test may let that resolve for real, since resolution asks a real `wgpu::Instance`
//! for an adapter (`gungnir-app/src/fusion.rs`'s own doc comment has the full history
//! of why). These tests are about the *load*, not the registration, so `desktop`
//! pins the backend to `FusionBackend::Cpu` before the first tick, the same bypass
//! `crate::fusion`'s own tests and `crate::pointcloud`'s own tests use.

use gungnir_app::fusion::FusionBackend;
use gungnir_app::pointcloud::PointCloudStatus;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, PointCloudConfig, PointCloudFileConfig};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/pointcloud")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn desktop(name: &str, point_cloud: PointCloudConfig) -> (AppState, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-pointcloud-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        point_cloud: Some(point_cloud),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    // GAP-024: never let a real tick resolve a real `wgpu` device from a test that is
    // only exercising the load (see this file's own module doc comment).
    state.fusion = FusionBackend::Cpu {
        reason: "test: forced CPU path (gungnir-app/tests/pointcloud.rs)".into(),
    };
    (state, dir)
}

fn plain(path: String) -> PointCloudFileConfig {
    PointCloudFileConfig {
        path,
        copc_bounds: None,
    }
}

/// Tick until the point-cloud loader thread answers, the same deadlock guard
/// `gungnir-app/tests/terrain.rs::settle` uses: a real hang still fails this, and a
/// loaded machine no longer does.
fn settle(state: &mut AppState) {
    for _ in 0..6_000 {
        update::tick(state);
        if !matches!(state.point_cloud, PointCloudStatus::Loading { .. }) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the loader never answered: {:?}", state.point_cloud);
}

/// **Rewritten by GAP-102 (D-41), and the rewrite is the point rather than a
/// concession.** This test used to assert that this pair *loaded*: a five-point LAS as
/// source and the bounded Autzen COPC hierarchy as target, both kept, with
/// `frame: "local-enu"` unopposed. GAP-102 taught the loader to read a LAS file's own
/// CRS VLRs, and the Autzen capture declares one -- NAD83 / Oregon GIC Lambert (ft) with
/// NAVD88 heights -- so the baseline's claim that this file was already in the
/// deployment's local metres is now contradicted by the file itself and the pair is
/// refused by name.
///
/// That is the behaviour change GAP-102 exists to make. The old expectation was not
/// wrong about the mechanics, which `a_pair_of_undeclared_files_still_loads_in_order`
/// below still covers; it was wrong about the geography, and silently so. The two
/// fixtures were never in one frame -- the five-point file's coordinates are a synthetic
/// UTM-shaped pair and Autzen's are Oregon Lambert feet -- and nothing in the desktop
/// could say so until a point cloud carried a CRS to check.
///
/// The bounded COPC read is still exercised end to end here: `load_copc_bounded` runs,
/// returns its 4767 points, and the refusal happens after it, in `pointcloud::place`.
#[test]
fn a_copc_target_that_declares_a_real_crs_is_refused_against_a_local_enu_baseline() {
    let (mut state, dir) = desktop(
        "pair",
        PointCloudConfig {
            source: plain(fixture("five-points.las")),
            target: PointCloudFileConfig {
                path: fixture("autzen-classified.copc.laz"),
                // Same box `gungnir-data/tests/pointcloud.rs`'s own bounded-query test
                // uses, chosen there from the fixture's recorded bounds rather than
                // from what the query returns; 4767 points is that test's pinned count.
                copc_bounds: Some([637_200.0, 851_100.0, 400.0, 637_300.0, 851_200.0, 620.0]),
            },
            frame: "local-enu".into(),
        },
    );
    assert!(
        matches!(state.point_cloud, PointCloudStatus::NotConfigured),
        "nothing until the first frame"
    );
    assert!(state.data.point_clouds.is_empty());
    update::tick(&mut state);
    assert!(
        matches!(state.point_cloud, PointCloudStatus::Loading { .. }),
        "the first frame starts the load: {:?}",
        state.point_cloud
    );
    settle(&mut state);
    match &state.point_cloud {
        PointCloudStatus::Failed { path, reason } => {
            assert!(path.contains("autzen"), "the target is what refused: {path}");
            assert!(
                reason.contains("NAD83 / Oregon GIC Lambert (ft)"),
                "the refusal names what the file actually declares: {reason}"
            );
            assert!(
                reason.contains("epsg:<code>"),
                "and says what to set instead: {reason}"
            );
        }
        other => panic!("{other:?}"),
    }
    // All-or-nothing is unchanged: the source had already loaded and is discarded, so
    // nothing half-placed reaches the viewport.
    assert!(state.data.point_clouds.is_empty());
    assert!(gungnir_app::pointcloud::layers(&state.point_cloud, &state.data).is_empty());
    assert!(
        state.alerts.iter().any(|a| a.contains("not loaded")),
        "the operator is told: {:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The pair mechanics GAP-098 built, kept under test with two files that declare no CRS
/// at all -- which is what leaves a baseline's `frame: "local-enu"` unopposed, and so is
/// the case where a pair still loads and is drawn.
#[test]
fn a_pair_of_undeclared_files_still_loads_in_order() {
    let (mut state, dir) = desktop(
        "undeclared-pair",
        PointCloudConfig {
            source: plain(fixture("five-points.las")),
            target: plain(fixture("five-points.las")),
            frame: "local-enu".into(),
        },
    );
    update::tick(&mut state);
    settle(&mut state);
    match &state.point_cloud {
        PointCloudStatus::Loaded {
            source_points,
            target_points,
            ..
        } => {
            assert_eq!(*source_points, 5);
            assert_eq!(*target_points, 5);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(state.data.point_clouds.len(), 2, "source, then target");
    // Positions are unchanged by the placement step: an undeclared file is drawn exactly
    // as it was before GAP-102, relative to its own minimum bound.
    assert_eq!(state.data.point_clouds[0].origin, [500_010.0, 6_000_019.0, 12.0]);
    assert_eq!(state.data.point_clouds[0].crs, None);
    // The viewport layer only ever reflects a complete pair (GAP-098's display piece).
    let layers = gungnir_app::pointcloud::layers(&state.point_cloud, &state.data);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].positions.len(), 5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_source_fails_the_whole_pair_even_though_the_target_would_load() {
    let (mut state, dir) = desktop(
        "missing-source",
        PointCloudConfig {
            source: plain(fixture("absent.las")),
            target: plain(fixture("five-points.las")),
            frame: "local-enu".into(),
        },
    );
    settle(&mut state);
    match &state.point_cloud {
        PointCloudStatus::Failed { path, reason } => {
            assert!(path.contains("absent.las"), "{path}");
            assert!(!reason.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert!(
        state.data.point_clouds.is_empty(),
        "a lone cloud must not be kept when its partner never arrived: {:?}",
        state.data.point_clouds.len()
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("not loaded")),
        "{:?}",
        state.alerts
    );
    assert!(gungnir_app::pointcloud::layers(&state.point_cloud, &state.data).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_target_fails_the_pair_and_discards_the_source_that_already_loaded() {
    let (mut state, dir) = desktop(
        "missing-target",
        PointCloudConfig {
            source: plain(fixture("five-points.las")),
            target: plain(fixture("absent.las")),
            frame: "local-enu".into(),
        },
    );
    settle(&mut state);
    match &state.point_cloud {
        PointCloudStatus::Failed { path, reason } => {
            assert!(path.contains("absent.las"), "{path}");
            assert!(!reason.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert!(
        state.data.point_clouds.is_empty(),
        "the source that loaded first must be discarded once its partner fails: {:?}",
        state.data.point_clouds.len()
    );
    let _ = std::fs::remove_dir_all(dir);
}
