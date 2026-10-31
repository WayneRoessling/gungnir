// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! data/ layer per rust-3d-data-ecosystem-build-vs-adopt.md §1. Every `*_source`
//! module converts a third-party crate's types into an internal, dependency-free
//! type. Nothing outside this crate imports `pasture`/`vtkio`/`tiff` directly.
//!
//! What loads: every format a [`LoadRequest`] names (GAP-023). DEMs as ESRI ASCII grid or
//! `GeoTIFF` (`geospatial`); LAS and LAZ point clouds through `las`, and bounded reads of
//! COPC through `las::copc` (`pointcloud`); legacy VTK polydata through `vtkio`
//! (`scientific`); glTF assets through `gltf` (`assets`). Each module's own documentation
//! says what it reads and what it refuses by name. What this crate still answers with
//! `DataError::NotImplemented` is not listed here: `docs/unbuilt.md` is generated from the
//! code, and that is where the list is.

pub mod assets;
pub mod geoid;
pub mod geospatial;
pub mod nad83;
pub mod pointcloud;
pub mod scientific;
pub mod terrain_store;

use std::path::PathBuf;

pub enum LoadRequest {
    PointCloud(PathBuf),
    /// A COPC file, and the `[min_x, min_y, min_z, max_x, max_y, max_z]` bounds to read
    /// within it -- its own request shape, since a bounded read needs the bounds the
    /// other loaders have no equivalent of.
    CopcBounded(PathBuf, [f32; 6]),
    VtkMesh(PathBuf),
    Terrain(PathBuf),
    /// A staged terrain store's directory and the manifest digest the baseline pins
    /// (GAP-227, DN-36 §4.2): opened and verified off the render thread.
    TerrainStore(PathBuf, String),
    /// One tile of a store already opened, verified against its manifest entry and
    /// meshed (DN-36 §5.1).
    TerrainTile(
        std::sync::Arc<terrain_store::TerrainStore>,
        terrain_store::TileKey,
    ),
    GltfAsset(PathBuf),
}

pub enum LoadResult {
    PointCloud(Result<pointcloud::PointBuffer, DataError>),
    CopcBounded(Result<pointcloud::PointBuffer, DataError>),
    VtkMesh(Result<scientific::MeshData, DataError>),
    Terrain(Result<geospatial::TerrainMesh, DataError>),
    /// Boxed: a verified store carries its whole manifest.
    TerrainStore(Result<Box<terrain_store::TerrainStore>, terrain_store::StoreError>),
    TerrainTile(Result<geospatial::TerrainMesh, terrain_store::TileError>),
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
                LoadRequest::CopcBounded(path, bounds) => {
                    LoadResult::CopcBounded(pointcloud::load_copc_bounded(&path, bounds))
                }
                LoadRequest::VtkMesh(path) => LoadResult::VtkMesh(scientific::load_vtk(&path)),
                LoadRequest::Terrain(path) => LoadResult::Terrain(geospatial::load_dem(&path)),
                LoadRequest::TerrainStore(dir, sha256) => LoadResult::TerrainStore(
                    terrain_store::TerrainStore::open(&dir, &sha256).map(Box::new),
                ),
                LoadRequest::TerrainTile(store, key) => {
                    LoadResult::TerrainTile(store.read_tile(key).and_then(|grid| {
                        geospatial::TerrainMesh::from_grid(&grid).map_err(|e| {
                            terrain_store::TileError {
                                key,
                                reason: terrain_store::TileRefusal::NotItsEntry(e.to_string()),
                            }
                        })
                    }))
                }
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
