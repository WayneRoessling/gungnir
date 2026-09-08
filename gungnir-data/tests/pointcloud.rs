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

/// **What this proves, and what it does not.** `load_copc_bounded` is implemented
/// against `las::copc::CopcReader`, whose own doctest (`las-0.11.1/src/copc.rs`) reads
/// real points from a real COPC file through the same `query` call this function makes.
/// This workspace holds no COPC fixture of its own yet -- vendoring one needs the same
/// licence and provenance check every other fixture here has had, which this change did
/// not do -- so what these tests can prove without one is the refusal side: a plain
/// LAZ/LAS file (no COPC info VLR, no hierarchy EVLR) is refused by name rather than
/// silently read as if it were bounded, and malformed bounds are refused before the
/// file is even opened. The happy path -- bounded points recovered from a real COPC
/// file -- is the next test to add, not a claim this one makes.
#[test]
fn a_plain_laz_file_has_no_copc_hierarchy_and_is_refused_by_name() {
    match pointcloud::load_copc_bounded(
        &fixture("five-points.las"),
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    ) {
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
