// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The LAS loader against its verification row (`verification-capability-table.md` §2,
//! `gungnir-data`): the fixture loads to exact counts and bounds, and a corrupt file is a
//! `DataError`, never a panic. Figures in `testdata/pointcloud/SOURCE.md`.

use std::path::{Path, PathBuf};

use gungnir_data::pointcloud;
use gungnir_data::{DataError, LoadRequest, LoadResult};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/pointcloud")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-las-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

#[test]
#[allow(clippy::float_cmp)]
fn the_fixture_loads_to_its_recorded_figures() {
    let cloud = pointcloud::load_las(&fixture("five-points.las")).expect("loads");
    assert_eq!(cloud.positions.len(), 5);
    assert_eq!(
        cloud.origin,
        [500_010.0, 6_000_019.0, 12.0],
        "the header's minimum"
    );
    let [min, max] = cloud.bounds().expect("five points have bounds");
    for (got, want) in min.iter().zip(&[500_010.0, 6_000_019.0, 12.0]) {
        assert!((got - want).abs() < 1e-6, "{min:?}");
    }
    for (got, want) in max.iter().zip(&[500_013.5, 6_000_022.0, 18.25]) {
        assert!((got - want).abs() < 1e-6, "{max:?}");
    }
    // Positions are relative to the origin, so a UTM coordinate survives the f32.
    assert!(
        (cloud.positions[1][0] - 1.25).abs() < 1e-6,
        "{:?}",
        cloud.positions[1]
    );
    assert_eq!(
        cloud.classification.as_deref(),
        Some(&[2u8, 2, 5, 6, 2][..])
    );
    let intensity = cloud.intensity.as_deref().expect("intensity");
    assert!((intensity[4] - 180.0).abs() < f32::EPSILON);
}

#[test]
fn corrupt_files_are_errors_never_panics() {
    let good = std::fs::read(fixture("five-points.las")).expect("fixture");
    let truncated_header = scratch("truncated-header.las");
    std::fs::write(&truncated_header, &good[..100]).expect("write");
    let truncated_points = scratch("truncated-points.las");
    std::fs::write(&truncated_points, &good[..good.len() - 7]).expect("write");
    let garbage = scratch("garbage.las");
    std::fs::write(
        &garbage,
        (0..400u32)
            .map(|i| u8::try_from((i * 37 + 11) & 0xff).unwrap_or(0))
            .collect::<Vec<_>>(),
    )
    .expect("write");
    let text = scratch("text.las");
    std::fs::write(&text, b"ncols 1\n").expect("write");
    for (path, what) in [
        (truncated_header, "a truncated header"),
        (truncated_points, "a truncated point record"),
        (garbage, "garbage"),
        (text, "text"),
    ] {
        match pointcloud::load_las(&path) {
            Err(DataError::Parse(message)) => {
                assert!(message.contains(".las"), "{what}: {message}");
            }
            other => panic!("{what}: expected a parse error, got {}", describe(&other)),
        }
    }
    assert!(matches!(
        pointcloud::load_las(&fixture("absent.las")),
        Err(DataError::Io(_))
    ));
}

/// A real COPC file, its own octree hierarchy queried through the happy path.
///
/// `testdata/pointcloud/autzen-classified.copc.laz` (`SOURCE.md`): a public,
/// CC-BY-4.0, real LIDAR capture -- not authored for this test, and not the `las`
/// crate's own doctest fixture either, so this is a second, independent confirmation
/// that `load_copc_bounded`'s query reaches a real hierarchy rather than one shaped to
/// match this function's own assumptions. The bounds are a round-number 100 x 100 m box
/// with a generous z range, chosen from the file's own reported bounds
/// (`testdata/pointcloud/SOURCE.md`) rather than from what the query happens to return,
/// so the count below is a real check and not a tautology.
#[test]
fn a_bounded_query_recovers_points_from_a_real_copc_hierarchy() {
    let cloud = pointcloud::load_copc_bounded(
        &fixture("autzen-classified.copc.laz"),
        [637_200.0, 851_100.0, 400.0, 637_300.0, 851_200.0, 620.0],
    )
    .expect("a real COPC file with points in this box");
    assert_eq!(cloud.positions.len(), 4767);
    assert!(
        (cloud.origin[0] - 635_577.79).abs() < 0.01
            && (cloud.origin[1] - 848_882.15).abs() < 0.01
            && (cloud.origin[2] - 406.14).abs() < 0.01,
        "origin is the file's own minimum bound, not the query box: {:?}",
        cloud.origin
    );
    let [min, max] = cloud.bounds().expect("4767 points have bounds");
    // The query box, not the file's: every returned point is inside what was asked
    // for, and the box was wide enough on x and y that the returned points do not
    // fill it exactly on z (the ground and the low vegetation above it stop well
    // short of 620 m).
    let query_box = [
        [637_200.0, 637_300.0],
        [851_100.0, 851_200.0],
        [400.0, 620.0],
    ];
    for axis in 0..3 {
        let (got_min, got_max) = (min[axis], max[axis]);
        let [box_min, box_max]: [f64; 2] = query_box[axis];
        assert!(
            got_min >= box_min - 0.01 && got_max <= box_max + 0.01,
            "axis {axis}: [{got_min}, {got_max}] outside the query box [{box_min}, {box_max}]"
        );
    }
    assert!(cloud.intensity.is_some());
    let classification = cloud.classification.as_deref().expect("classification");
    // Ground (2), high vegetation (5), overhead structure (19), and car (65): a real
    // classification, not four points that happen to be ground.
    let mut counts = std::collections::BTreeMap::new();
    for &c in classification {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    assert_eq!(
        counts,
        std::collections::BTreeMap::from([(0, 2), (2, 4499), (5, 114), (19, 8), (65, 144)]),
        "classification counts changed: {counts:?}"
    );
}

/// **What this proves, and what it does not.** `load_copc_bounded` is implemented
/// against `las::copc::CopcReader`, whose own doctest (`las-0.11.1/src/copc.rs`) reads
/// real points from a real COPC file through the same `query` call this function makes.
/// The test above is a second, independent confirmation against a different file. What
/// remains here is the refusal side: a plain LAZ/LAS file (no COPC info VLR, no
/// hierarchy EVLR) is refused by name rather than silently read as if it were bounded,
/// and malformed bounds are refused before the file is even opened.
#[test]
fn a_plain_laz_file_has_no_copc_hierarchy_and_is_refused_by_name() {
    match pointcloud::load_copc_bounded(&fixture("five-points.las"), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
    {
        Err(DataError::Parse(message)) => {
            assert!(
                message.to_lowercase().contains("copc"),
                "expected the refusal to name what was missing: {message}"
            );
        }
        other => panic!(
            "a file with no COPC hierarchy must be refused, not read: {}",
            describe(&other)
        ),
    }
}

#[test]
fn non_finite_or_inverted_bounds_are_refused_before_the_file_is_opened() {
    for bad in [
        [f32::NAN, 0.0, 0.0, 1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0, f32::INFINITY, 1.0, 1.0],
        // min_x (5.0) past max_x (1.0): inverted on one axis is still inverted.
        [5.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    ] {
        match pointcloud::load_copc_bounded(&fixture("absent.copc.laz"), bad) {
            Err(DataError::Parse(message)) => {
                assert!(message.contains(&format!("{bad:?}")), "{message}");
            }
            other => panic!("{bad:?}: expected a parse error, got {}", describe(&other)),
        }
    }
}

#[test]
fn a_missing_copc_file_is_an_io_error() {
    assert!(matches!(
        pointcloud::load_copc_bounded(&fixture("absent.copc.laz"), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
        Err(DataError::Io(_))
    ));
}

#[test]
fn the_loader_thread_dispatches_a_copc_bounded_request() {
    let (requests, results) = gungnir_data::spawn_loader();
    requests
        .send(LoadRequest::CopcBounded(
            fixture("five-points.las"),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ))
        .expect("loader alive");
    match results
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("a result")
    {
        // Dispatch is what this test proves; the file is deliberately not a COPC one,
        // so a named refusal (not a hang, not a panic) is the right outcome here.
        LoadResult::CopcBounded(Err(DataError::Parse(message))) => {
            assert!(message.to_lowercase().contains("copc"), "{message}");
        }
        LoadResult::CopcBounded(other) => panic!("expected a named refusal: {}", describe(&other)),
        _ => panic!("wrong result kind"),
    }
}

#[test]
fn the_loader_thread_dispatches_point_clouds() {
    let (requests, results) = gungnir_data::spawn_loader();
    requests
        .send(LoadRequest::PointCloud(fixture("five-points.las")))
        .expect("loader alive");
    match results
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("a result")
    {
        LoadResult::PointCloud(Ok(cloud)) => assert_eq!(cloud.positions.len(), 5),
        LoadResult::PointCloud(Err(e)) => panic!("{e}"),
        _ => panic!("wrong result kind"),
    }
}

fn describe(r: &Result<pointcloud::PointBuffer, DataError>) -> String {
    match r {
        Ok(c) => format!("ok with {} points", c.positions.len()),
        Err(e) => e.to_string(),
    }
}
