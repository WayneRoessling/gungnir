// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! data/ layer per rust-3d-data-ecosystem-build-vs-adopt.md §1. Every `*_source`
//! module converts a third-party crate's types into an internal, dependency-free
//! type. Nothing outside this crate imports `pasture`/`vtkio`/`tiff` directly.
//!
//! What loads (2026-09-06): DEMs, as ESRI ASCII grid or `GeoTIFF` (`geospatial`, GAP-023).
//! Point clouds, VTK meshes and glTF assets are each a `DataError::NotImplemented`
//! naming what they wait on, and the loader thread returns that rather than nothing.

pub mod assets;
pub mod geospatial;
pub mod pointcloud;
pub mod scientific;

use std::path::PathBuf;

pub enum LoadRequest {
    PointCloud(PathBuf),
    VtkMesh(PathBuf),
    Terrain(PathBuf),
    GltfAsset(PathBuf),
}

pub enum LoadResult {
    PointCloud(Result<pointcloud::PointBuffer, DataError>),
    VtkMesh(Result<scientific::MeshData, DataError>),
    Terrain(Result<geospatial::TerrainMesh, DataError>),
    GltfAsset(Result<assets::StaticMesh, DataError>),
}

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("file I/O failed: {0}")]
    Io(String),
    #[error("parse failed: {0}")]
    Parse(String),
    /// The format is designed and the loader is not written.
    ///
    /// Named rather than panicked (GAP-082): every one of these was a `todo!()` in a
    /// public function, and a caller reaching one would have taken the desktop down
    /// rather than been told the file cannot be read yet. The crate this waits on is
    /// named, because "not implemented" without one is untraceable.
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
}

/// Aggregate, single source of truth for loaded data -- what `gungnir-app::AppState`
/// actually holds a `DataStore` field of.
#[derive(Default)]
pub struct DataStore {
    pub point_clouds: Vec<pointcloud::PointBuffer>,
    pub meshes: Vec<scientific::MeshData>,
    pub terrains: Vec<geospatial::TerrainMesh>,
    pub assets: Vec<assets::StaticMesh>,
}

/// Background thread pulls `LoadRequest`s from a channel, does the file-I/O-bound
/// work, sends `LoadResult`s back. `update()` polls the result channel non-blockingly.
pub fn spawn_loader() -> (
    crossbeam_channel::Sender<LoadRequest>,
    crossbeam_channel::Receiver<LoadResult>,
) {
    let (req_tx, req_rx) = crossbeam_channel::unbounded::<LoadRequest>();
    let (res_tx, res_rx) = crossbeam_channel::unbounded::<LoadResult>();
    std::thread::spawn(move || {
        for request in &req_rx {
            let result = match request {
                LoadRequest::PointCloud(path) => {
                    LoadResult::PointCloud(pointcloud::load_las(&path))
                }
                LoadRequest::VtkMesh(path) => LoadResult::VtkMesh(scientific::load_vtk(&path)),
                LoadRequest::Terrain(path) => LoadResult::Terrain(geospatial::load_dem(&path)),
                LoadRequest::GltfAsset(path) => LoadResult::GltfAsset(assets::load_gltf(&path)),
            };
            // The receiver is gone when the desktop has closed; nothing to report to.
            if res_tx.send(result).is_err() {
                break;
            }
        }
    });
    (req_tx, res_rx)
}
