// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The DEM loaders against their verification row (`verification-capability-table.md`
//! §2, `gungnir-data`): a fixture loads to exact counts and bounds, and a corrupt file
//! is a `DataError`, never a panic. The fixtures and their figures are described in
//! `testdata/dem/SOURCE.md`.

// The row asks for exact bounds, and the fixture's figures are whole numbers a float
// carries exactly; a margin would be a widened criterion.
#![allow(clippy::float_cmp)]

use std::path::{Path, PathBuf};

use gungnir_data::geospatial::{self, GridCrs, HeightGrid, TerrainMesh};
use gungnir_data::{DataError, LoadRequest, LoadResult};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/dem")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-dem-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

/// What SOURCE.md promises of both fixtures.
fn assert_is_the_small_grid(grid: &HeightGrid) {
    assert_eq!((grid.columns, grid.rows), (5, 4));
    assert_eq!(
        grid.bounds(),
        [[500_000.0, 6_000_000.0], [500_150.0, 6_000_120.0]]
    );
    assert_eq!(grid.cell_size, [30.0, 30.0]);
    assert_eq!(grid.nodata, Some(-9999.0));
    assert_eq!(grid.heights.len(), 20);
    assert_eq!(grid.height_at(0, 0), Some(10.0));
    assert_eq!(grid.height_at(3, 4), Some(44.0));
    assert_eq!(
        grid.height_at(1, 1),
        None,
        "the no-data cell is not a height"
    );
    assert_eq!(grid.height_at(4, 0), None, "outside the grid");
    assert_eq!(grid.valid_count(), 19);
    let mesh = TerrainMesh::from_grid(grid).expect("indexable");
    assert_eq!(mesh.positions.len(), 20, "one vertex per cell");
    assert_eq!(mesh.triangle_count(), 16, "eight blocks clear of the hole");
    assert_eq!(mesh.origin, [500_000.0, 6_000_000.0]);
    // The south-west vertex sits half a cell in from the origin.
    assert_eq!(mesh.positions[15], [15.0, 15.0, 40.0]);
}

#[test]
fn the_ascii_grid_fixture_loads_to_its_recorded_figures() {
    let grid = geospatial::load_height_grid(&fixture("small.asc")).expect("loads");
    assert_is_the_small_grid(&grid);
    assert_eq!(grid.crs, GridCrs::Unstated, "an ASCII grid names no frame");
}

#[test]
fn the_geotiff_fixture_loads_to_the_same_figures_and_names_its_frame() {
    let grid = geospatial::load_height_grid(&fixture("small.tif")).expect("loads");
    assert_is_the_small_grid(&grid);
    assert_eq!(grid.crs, GridCrs::Projected { epsg: Some(32633) });
    let ascii = geospatial::load_height_grid(&fixture("small.asc")).expect("loads");
    assert_eq!(grid.heights, ascii.heights, "the two fixtures are one grid");
}

#[test]
fn a_centre_referenced_ascii_grid_is_shifted_to_its_corner() {
    let text = "ncols 2\nnrows 1\nxllcenter 115\nyllcenter 15\ncellsize 30\n1 2\n";
    let grid = geospatial::ascii_grid::parse(text).expect("parses");
    assert_eq!(grid.origin, [100.0, 0.0]);
    assert_eq!(grid.nodata, None);
}

#[test]
fn corrupt_ascii_grids_are_parse_errors_never_panics() {
    let cases = [
        ("", "empty"),
        (
            "ncols 2\nxllcorner 0\nyllcorner 0\ncellsize 1\n1 2\n",
            "missing nrows",
        ),
        (
            "ncols 2\nnrows 2\nxllcorner 0\nyllcorner 0\ncellsize 1\n1 2 3\n",
            "too few heights",
        ),
        (
            "ncols 1\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize 1\n1 2\n",
            "too many heights",
        ),
        (
            "ncols 1\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize 1\nabc\n",
            "non-numeric height",
        ),
        (
            "ncols 0\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize 1\n",
            "zero columns",
        ),
        (
            "ncols 1\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize -1\n1\n",
            "negative cell",
        ),
        (
            "ncols 1\nnrows 1\nxllcorner x\nyllcorner 0\ncellsize 1\n1\n",
            "non-numeric origin",
        ),
        ("ncols 1\nnrows 1\ncellsize 1\n1\n", "no origin at all"),
        (
            "ncols 1\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize 1\nNODATA_value\n",
            "key without value",
        ),
        (
            "ncols 99999999999\nnrows 99999999999\nxllcorner 0\nyllcorner 0\ncellsize 1\n1\n",
            "overflow",
        ),
    ];
    for (text, what) in cases {
        match geospatial::ascii_grid::parse(text) {
            Err(DataError::Parse(_)) => {}
            other => panic!("{what}: expected a parse error, got {other:?}"),
        }
    }
}

#[test]
fn corrupt_tiffs_are_errors_never_panics() {
    let good = std::fs::read(fixture("small.tif")).expect("fixture");
    let truncated = scratch("truncated.tif");
    std::fs::write(&truncated, &good[..40]).expect("write");
    let garbage = scratch("garbage.tif");
    std::fs::write(
        &garbage,
        (0..512u32)
            .map(|i| u8::try_from((i.wrapping_mul(2_654_435_761) >> 13) & 0xff).unwrap_or(0))
            .collect::<Vec<_>>(),
    )
    .expect("write");
    let text = scratch("text.tiff");
    std::fs::write(&text, b"ncols 1\nnrows 1\n").expect("write");
    let bad_scale = scratch("bad-scale.tif");
    // Flip the sign byte of the first ModelPixelScale double: the tag is intact, its
    // value is not a positive scale.
    let mut flipped = good.clone();
    let scale_bytes = 30.0f64.to_le_bytes();
    let at = flipped
        .windows(8)
        .position(|w| w == scale_bytes)
        .expect("the fixture carries its scale");
    flipped[at + 7] |= 0x80;
    std::fs::write(&bad_scale, &flipped).expect("write");

    for (path, what) in [
        (truncated, "truncated"),
        (garbage, "garbage"),
        (text, "text in a .tiff"),
        (fixture("plain.tif"), "a TIFF with no georeferencing"),
        (bad_scale, "a negative pixel scale"),
    ] {
        match geospatial::load_height_grid(&path) {
            Err(DataError::Parse(message)) => {
                assert!(
                    message.contains(path.file_name().and_then(|n| n.to_str()).unwrap_or("")),
                    "{what}: {message}"
                );
            }
            other => panic!("{what}: expected a parse error, got {other:?}"),
        }
    }
    assert!(
        matches!(geospatial::load_height_grid(&fixture("plain.tif")), Err(DataError::Parse(m)) if m.contains("not a GeoTIFF")),
        "a plain TIFF is refused by name"
    );
}

#[test]
fn a_missing_file_and_an_unknown_extension_are_named_errors() {
    assert!(matches!(
        geospatial::load_height_grid(&fixture("absent.asc")),
        Err(DataError::Io(_))
    ));
    assert!(matches!(
        geospatial::load_height_grid(&fixture("SOURCE.md")),
        Err(DataError::Parse(m)) if m.contains("expected .asc, .tif or .tiff")
    ));
}

#[test]
fn the_loader_thread_dispatches_terrain_and_a_wrong_format_is_a_named_error() {
    let (requests, results) = gungnir_data::spawn_loader();
    requests
        .send(LoadRequest::Terrain(fixture("small.tif")))
        .expect("loader alive");
    requests
        .send(LoadRequest::VtkMesh(fixture("small.tif")))
        .expect("loader alive");
    let timeout = std::time::Duration::from_secs(10);
    match results.recv_timeout(timeout).expect("a terrain result") {
        LoadResult::Terrain(Ok(mesh)) => assert_eq!(mesh.triangle_count(), 16),
        other => panic!("expected terrain, got {}", describe(&other)),
    }
    // A GeoTIFF handed to the VTK loader is refused by the reader, not read as a mesh
    // (the VTK loader is real since GAP-023's second half, 2026-09-06).
    match results.recv_timeout(timeout).expect("a mesh result") {
        LoadResult::VtkMesh(Err(DataError::Parse(_) | DataError::Io(_))) => {}
        other => panic!("expected a refusal, got {}", describe(&other)),
    }
}

fn state<T>(r: &Result<T, DataError>) -> String {
    match r {
        Ok(_) => "ok".into(),
        Err(e) => e.to_string(),
    }
}

fn describe(result: &LoadResult) -> String {
    match result {
        LoadResult::PointCloud(r) => format!("point cloud: {}", state(r)),
        LoadResult::VtkMesh(r) => format!("vtk: {}", state(r)),
        LoadResult::Terrain(r) => format!("terrain: {}", state(r)),
        LoadResult::GltfAsset(r) => format!("gltf: {}", state(r)),
    }
}
