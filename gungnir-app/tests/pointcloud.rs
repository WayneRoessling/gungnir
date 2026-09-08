// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point clouds on the desktop (GAP-098): a configured source and target both load off
//! the render thread into `DataStore.point_clouds`, in that order; either failing
//! leaves neither behind, since a lone cloud is not the pair registration needs
//! (`gungnir-data-fusion::CpuIcp`, GAP-024, independent of this).

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
    (AppState::with_config(config).expect("starts"), dir)
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

#[test]
fn a_las_source_and_a_bounded_copc_target_both_load_in_order() {
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
        PointCloudStatus::Loaded {
            source_points,
            target_points,
            ..
        } => {
            assert_eq!(*source_points, 5);
            assert_eq!(*target_points, 4767);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(state.data.point_clouds.len(), 2, "source, then target");
    assert_eq!(state.data.point_clouds[0].positions.len(), 5);
    assert_eq!(state.data.point_clouds[1].positions.len(), 4767);
    // The viewport layer only ever reflects a complete pair (GAP-098's display piece).
    let layers = gungnir_app::pointcloud::layers(&state.point_cloud, &state.data);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].positions.len(), 5);
    assert_eq!(layers[1].positions.len(), 4767);
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
