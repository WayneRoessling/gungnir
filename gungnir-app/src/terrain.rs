//! Terrain on the desktop (GAP-023): the baseline names a DEM, the loader thread reads
//! it, and the coverage analytics mask line of sight against it.
//!
//! **Placement is the honest problem.** A DEM arrives in its own frame (a projected CRS,
//! or geographic degrees) and the picture is local ENU metres; converting between them
//! needs a projection library the approved stack does not hold. So the baseline states
//! that the DEM was prepared in the local frame (`frame: "local-enu"`: `gdalwarp` to a
//! transverse Mercator centred on `origin` gives metres that are ENU to well within a
//! cell), and a file whose own tags say otherwise -- geographic, or a projected CRS with
//! a known EPSG code -- is refused by name rather than drawn in the wrong place.

use gungnir_data::geospatial::GridCrs;
use gungnir_data::{LoadRequest, LoadResult};

use crate::state::AppState;

/// Where the terrain stands, for PN-09 and PN-11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerrainStatus {
    /// The baseline names no terrain: line of sight is flat, and the coverage report
    /// says so on its parameters.
    NotConfigured,
    Loading {
        path: String,
    },
    Loaded {
        path: String,
        vertices: usize,
        triangles: usize,
    },
    /// The file could not be read or placed; the reason is on screen, and line of
    /// sight stays flat rather than masked against something that is not there.
    Failed {
        path: String,
        reason: String,
    },
}

impl TerrainStatus {
    /// One line for the health panel.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            TerrainStatus::NotConfigured => {
                "no terrain configured; line of sight is flat (optimistic)".to_string()
            }
            TerrainStatus::Loading { path } => format!("loading {path}"),
            TerrainStatus::Loaded {
                path,
                vertices,
                triangles,
            } => {
                format!("{path}: {vertices} vertices, {triangles} triangles; masking line of sight")
            }
            TerrainStatus::Failed { path, reason } => {
                format!("{path} not loaded: {reason}; line of sight is flat (optimistic)")
            }
        }
    }

    #[must_use]
    pub fn is_masking(&self) -> bool {
        matches!(self, TerrainStatus::Loaded { .. })
    }
}

/// The placed terrain for the viewport, once it is loaded (PN-02 draws it under the
/// picture). `None` while nothing is placed, which draws nothing and claims nothing.
///
/// Takes the two fields rather than the state, so the viewport can borrow its own state
/// mutably in the same call.
#[must_use]
pub fn layer<'a>(
    status: &TerrainStatus,
    data: &'a gungnir_data::DataStore,
) -> Option<gungnir_viewport3d::layers::TerrainLayer<'a>> {
    if !status.is_masking() {
        return None;
    }
    data.terrains
        .first()
        .map(|t| gungnir_viewport3d::layers::TerrainLayer {
            positions: &t.positions,
            rows: t.rows,
            columns: t.columns,
        })
}

/// Start loading the configured terrain off the render thread. Idempotent: the first
/// tick calls it, and a load already started or finished is left alone.
pub fn start(state: &mut AppState) {
    if state.loader.is_some() || !matches!(state.terrain, TerrainStatus::NotConfigured) {
        return;
    }
    let Some(terrain) = state.config.terrain.clone() else {
        return;
    };
    let (requests, results) = gungnir_data::spawn_loader();
    match requests.send(LoadRequest::Terrain(std::path::PathBuf::from(
        &terrain.path,
    ))) {
        Ok(()) => {
            state.terrain = TerrainStatus::Loading {
                path: terrain.path.clone(),
            };
            state.loader = Some((requests, results));
        }
        Err(err) => {
            state.terrain = TerrainStatus::Failed {
                path: terrain.path.clone(),
                reason: format!("the loader thread is gone: {err}"),
            };
        }
    }
}

/// Poll the loader; on the tick, so a slow file never stalls a frame. Starts the load
/// on the first call.
pub fn poll(state: &mut AppState) {
    start(state);
    let Some((_, results)) = state.loader.as_ref() else {
        return;
    };
    let Ok(result) = results.try_recv() else {
        return;
    };
    let path = match &state.terrain {
        TerrainStatus::Loading { path } => path.clone(),
        _ => return,
    };
    let LoadResult::Terrain(loaded) = result else {
        return;
    };
    match loaded {
        Ok(mesh) => {
            if let Some(reason) = placement_refusal(mesh.crs) {
                state.terrain = TerrainStatus::Failed {
                    path: path.clone(),
                    reason: reason.clone(),
                };
                state
                    .alerts
                    .push(format!("terrain {path} not placed: {reason}"));
            } else {
                let placed = mesh.placed();
                state.terrain = TerrainStatus::Loaded {
                    path,
                    vertices: placed.positions.len(),
                    triangles: placed.triangle_count(),
                };
                state.data.terrains.clear();
                state.data.terrains.push(placed);
            }
        }
        Err(err) => {
            let reason = err.to_string();
            state.terrain = TerrainStatus::Failed {
                path: path.clone(),
                reason: reason.clone(),
            };
            state
                .alerts
                .push(format!("terrain {path} not loaded: {reason}"));
        }
    }
    state.loader = None;
}

/// Why a grid's own frame contradicts `frame: "local-enu"`, when it does. A grid that
/// states no frame is taken at the baseline's word; one that names a geographic frame
/// or a known projected code is not in local metres, whatever the baseline says.
fn placement_refusal(crs: GridCrs) -> Option<String> {
    match crs {
        // 32767 is GeoTIFF's "user-defined", which is what a local projection carries.
        GridCrs::Unstated
        | GridCrs::Projected {
            epsg: None | Some(32767),
        } => None,
        GridCrs::Projected { epsg: Some(code) } => Some(format!(
            "the file declares projected CRS EPSG:{code}, not the local frame the baseline claims; \
             reproject it to a local transverse Mercator on the origin"
        )),
        GridCrs::Geographic { epsg } => Some(format!(
            "the file declares a geographic frame ({}), not local metres",
            epsg.map_or_else(|| "no code".to_string(), |c| format!("EPSG:{c}"))
        )),
    }
}
