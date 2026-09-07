// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The VTK and glTF loaders against the hand-written fixtures (GAP-023): exact counts,
//! and a corrupt file that yields `DataError` and never a panic (verification table §2,
//! `gungnir-data` row).

// The fixtures hold exactly representable values, and exact is what a loader test wants.
#![allow(clippy::float_cmp)]

use std::path::PathBuf;

use gungnir_data::{assets, scientific, DataError, LoadRequest, LoadResult};

fn fixture(dir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata")
        .join(dir)
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-data-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

#[test]
fn the_two_triangle_polydata_loads_with_its_scalar() {
    let mesh = scientific::load_vtk(&fixture("scientific", "two-triangles.vtk")).expect("loads");
    assert_eq!(mesh.positions.len(), 4);
    assert_eq!(mesh.triangle_count(), 2);
    assert_eq!(mesh.indices, vec![0, 1, 2, 0, 2, 3]);
    let field = mesh.scalar_field.expect("height scalar");
    assert_eq!(field, vec![0.0, 0.0, 0.5, 0.5]);
    assert_eq!(mesh.positions[2], [1.0, 1.0, 0.5]);
}

#[test]
fn a_vtk_file_that_is_not_polydata_or_not_vtk_is_refused() {
    let grid = scratch("grid.vtk");
    std::fs::write(
        &grid,
        "# vtk DataFile Version 3.0\nimage\nASCII\nDATASET STRUCTURED_POINTS\nDIMENSIONS 2 2 1\nORIGIN 0 0 0\nSPACING 1 1 1\n",
    )
    .expect("write");
    match scientific::load_vtk(&grid) {
        Err(DataError::Parse(reason)) => assert!(reason.contains("POLYDATA"), "{reason}"),
        other => panic!("{other:?}"),
    }
    let garbage = scratch("garbage.vtk");
    std::fs::write(&garbage, b"\x00\x01\x02 not a vtk file").expect("write");
    assert!(matches!(
        scientific::load_vtk(&garbage),
        Err(DataError::Parse(_) | DataError::Io(_))
    ));
    // A polygon that names a vertex the file does not have.
    let bad = scratch("bad-index.vtk");
    std::fs::write(
        &bad,
        "# vtk DataFile Version 3.0\nbad\nASCII\nDATASET POLYDATA\nPOINTS 3 float\n0 0 0\n1 0 0\n0 1 0\nPOLYGONS 1 4\n3 0 1 7\n",
    )
    .expect("write");
    match scientific::load_vtk(&bad) {
        Err(DataError::Parse(reason)) => assert!(reason.contains("vertex 7"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        scientific::load_vtk(&fixture("scientific", "missing.vtk")),
        Err(DataError::Io(_))
    ));
}

#[test]
fn the_triangle_asset_loads_with_its_normals_and_indices() {
    let mesh = assets::load_gltf(&fixture("assets", "triangle.gltf")).expect("loads");
    assert_eq!(mesh.positions.len(), 3);
    assert_eq!(mesh.normals.len(), 3);
    assert_eq!(mesh.indices, vec![0, 1, 2]);
    assert_eq!(mesh.normals[0], [0.0, 0.0, 1.0]);
    assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
}

#[test]
fn a_gltf_file_that_is_not_gltf_is_refused() {
    let garbage = scratch("garbage.gltf");
    std::fs::write(&garbage, b"{\"asset\": {\"version\": \"2.0\"}, \"meshes\": [{\"primitives\": [{\"attributes\": {\"POSITION\": 9}}]}]}")
        .expect("write");
    assert!(matches!(
        assets::load_gltf(&garbage),
        Err(DataError::Parse(_) | DataError::Io(_))
    ));
    let empty = scratch("empty.gltf");
    std::fs::write(&empty, b"{\"asset\": {\"version\": \"2.0\"}}").expect("write");
    match assets::load_gltf(&empty) {
        Err(DataError::Parse(reason)) => assert!(reason.contains("no triangle"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        assets::load_gltf(&fixture("assets", "missing.gltf")),
        Err(DataError::Io(_))
    ));
}

/// The loader thread dispatches both formats.
#[test]
fn the_loader_thread_serves_both_formats() {
    let (requests, results) = gungnir_data::spawn_loader();
    requests
        .send(LoadRequest::VtkMesh(fixture(
            "scientific",
            "two-triangles.vtk",
        )))
        .expect("send");
    requests
        .send(LoadRequest::GltfAsset(fixture("assets", "triangle.gltf")))
        .expect("send");
    let mut vtk = None;
    let mut gltf = None;
    for _ in 0..2 {
        match results
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a result")
        {
            LoadResult::VtkMesh(r) => vtk = Some(r.expect("vtk")),
            LoadResult::GltfAsset(r) => gltf = Some(r.expect("gltf")),
            _ => panic!("unexpected result"),
        }
    }
    assert_eq!(vtk.expect("vtk").triangle_count(), 2);
    assert_eq!(gltf.expect("gltf").triangle_count(), 1);
}
