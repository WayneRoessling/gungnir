// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain on the desktop (GAP-023): the baseline names a DEM, the loader thread reads
//! it, and the coverage analytics mask line of sight against it.
//!
//! **Placement used to be the honest problem, and now has a real answer for it.** A DEM
//! arrives in its own frame (a projected CRS, or geographic degrees) and the picture is
//! local ENU metres; converting between them needed a projection library the approved
//! stack did not hold, so the baseline could only ever state that the DEM was already
//! prepared in the local frame (`frame: "local-enu"`), and a file whose own tags said
//! otherwise was refused by name.
//!
//! D-41 (2026-09-08) closed that: `frame` also accepts `"epsg:<code>"`
//! ([`gungnir_config::Frame`]), and [`place`] reconciles it against the file's own
//! tags (`GridCrs`) and converts through `gungnir_data::geospatial::crs::to_wgs84`
//! (behind the `crs-projection` feature; PROJ) and then `gungnir_coord::Wgs84` (always
//! available -- geographic-to-local-ENU needs no native dependency) using the
//! deployment's own declared origin. A file whose tags still contradict what `frame`
//! declares, or a real-world CRS this build was not compiled to convert, is refused by
//! name exactly as before.

use gungnir_config::Frame;
use gungnir_coord::CoordTransform;
use gungnir_data::geospatial::{GridCrs, TerrainMesh};
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
            // `start` only ever sends a `LoadRequest::Terrain` when `config.terrain` is
            // `Some` (its own early return above), so this is reachable only with one
            // configured; the fallback is defensive rather than reachable in practice,
            // and "local-enu" is the same as `mesh.placed()`'s original behaviour.
            let declared_frame = state
                .config
                .terrain
                .as_ref()
                .map_or("local-enu", |t| t.frame.as_str());
            match place(mesh, declared_frame, state.config.origin) {
                Ok(placed) => {
                    state.terrain = TerrainStatus::Loaded {
                        path,
                        vertices: placed.positions.len(),
                        triangles: placed.triangle_count(),
                    };
                    state.data.terrains.clear();
                    state.data.terrains.push(placed);
                }
                Err(reason) => {
                    state.terrain = TerrainStatus::Failed {
                        path: path.clone(),
                        reason: reason.clone(),
                    };
                    state
                        .alerts
                        .push(format!("terrain {path} not placed: {reason}"));
                }
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

/// WGS84 geographic, EPSG:4326 -- `gungnir_data::geospatial::crs::WGS84_EPSG` restated
/// so this module's unconditional code does not need the `crs-projection` feature just
/// to name the constant.
const WGS84_EPSG: u32 = 4326;

/// Turn a loaded mesh into one placed in the deployment's local ENU frame, or say why
/// not (GAP-023, D-41). `declared_frame` is `state.config.terrain`'s own `frame` string;
/// `origin` is `state.config.origin`, the same geodetic anchor every other local-ENU
/// thing in the picture (`DetectionView::measurement`, `TrackView::state`, ...) is
/// relative to, reused rather than inventing a second one for terrain alone.
///
/// The file's own tags (`mesh.crs`) are read regardless of what `declared_frame` says:
/// a `"local-enu"` declaration contradicted by a file that states a real CRS is refused,
/// exactly as before D-41; a declared real-world CRS is cross-checked against the
/// file's own tags when it has any (a `GeoTIFF`) and trusted outright when it does not
/// (an ESRI ASCII grid, which never carries CRS tags at all -- `frame` is the only place
/// its CRS could ever come from).
fn place(
    mesh: TerrainMesh,
    declared_frame: &str,
    origin: Option<[f64; 3]>,
) -> Result<TerrainMesh, String> {
    let frame: Frame = declared_frame
        .parse()
        .map_err(|reason| format!("terrain.frame {reason}"))?;
    let Some(source_epsg) = source_epsg(frame, mesh.crs)? else {
        return Ok(mesh.placed());
    };
    let Some(origin) = origin else {
        return Err(format!(
            "the terrain declares a real-world CRS (EPSG:{source_epsg}) but the deployment has \
             no declared local ENU origin (config.origin) to place it against"
        ));
    };
    let absolute = mesh.absolute_positions_xy();
    let lon_lat_deg = if source_epsg == WGS84_EPSG {
        // Already geographic WGS84: what a GeoTIFF's own [x, y] already means for a
        // `GridCrs::Geographic` file (longitude, latitude, degrees), so there is
        // nothing for PROJ to do, and this path needs no `crs-projection` feature.
        absolute
    } else {
        to_wgs84(&absolute, source_epsg)?
    };
    let enu = to_local_enu(&mesh, &lon_lat_deg, origin);
    // 32767 is GeoTIFF's "user-defined" projected code, the existing convention this
    // codebase already uses for "trust it as local" (see `source_epsg` below); reused
    // here as the marker for "already placed", since a mesh in this state has exactly
    // that property and `.placed()` on it is correctly a no-op (`origin` is `[0, 0]`).
    mesh.with_xy(&enu, GridCrs::Projected { epsg: Some(32_767) })
        .map_err(|e| e.to_string())
}

/// What a grid's `GridCrs` actually asserts, collapsing `GridCrs`'s three variants (two
/// of which carry an `Option<u16>`) down to the three things `source_epsg` needs to
/// distinguish.
enum FileCrs {
    /// `Unstated`, or `Projected` with no code or `GeoTIFF`'s "user-defined" placeholder
    /// (32767, what a local projection carries): nothing here contradicts a
    /// `"local-enu"` declaration.
    NotStated,
    /// A real-world CRS with a known EPSG code, geographic or projected.
    Known(u32),
    /// The file says geographic but names no code: neither "trust it as local" nor
    /// "here is what it is".
    Ambiguous,
}

fn file_crs(crs: GridCrs) -> FileCrs {
    match crs {
        GridCrs::Unstated
        | GridCrs::Projected {
            epsg: None | Some(32_767),
        } => FileCrs::NotStated,
        GridCrs::Geographic { epsg: Some(code) } | GridCrs::Projected { epsg: Some(code) } => {
            FileCrs::Known(u32::from(code))
        }
        GridCrs::Geographic { epsg: None } => FileCrs::Ambiguous,
    }
}

/// What EPSG code (if any) a loaded mesh should be reprojected from, reconciling
/// `frame`'s declaration against the file's own tags. `Ok(None)` means the file is
/// already in the deployment's local frame (nothing to convert); `Ok(Some(code))` names
/// the real-world CRS to convert from; `Err` names a contradiction between what the
/// baseline declares and what the file actually states, refused by name rather than
/// guessed at.
fn source_epsg(frame: Frame, crs: GridCrs) -> Result<Option<u32>, String> {
    match (frame, file_crs(crs)) {
        (Frame::LocalEnu, FileCrs::NotStated) => Ok(None),
        (Frame::LocalEnu, FileCrs::Known(code)) => Err(format!(
            "the file declares a real-world CRS (EPSG:{code}), not the local frame the baseline \
             claims; declare terrain.frame \"epsg:{code}\" instead if that is correct"
        )),
        (Frame::LocalEnu, FileCrs::Ambiguous) => Err(
            "the file declares a geographic frame with no EPSG code, not local metres".to_string(),
        ),
        (Frame::Epsg(declared), FileCrs::NotStated) => Ok(Some(declared)),
        (Frame::Epsg(declared), FileCrs::Known(code)) if code == declared => Ok(Some(declared)),
        (Frame::Epsg(declared), FileCrs::Known(code)) => Err(format!(
            "terrain.frame declares EPSG:{declared} but the file's own tags state EPSG:{code}"
        )),
        (Frame::Epsg(declared), FileCrs::Ambiguous) => Err(format!(
            "terrain.frame declares EPSG:{declared} but the file's tags name a geographic frame \
             with no EPSG code to check it against"
        )),
    }
}

/// Reproject `points` (`[longitude, latitude]` or `[easting, northing]`, matching
/// `source_epsg`) into WGS84 geographic degrees.
///
/// Behind the `crs-projection` feature (PROJ, GAP-023, D-41): the feature-off build
/// below refuses by name instead, so this function's signature -- and every caller --
/// is the same regardless of whether the feature is compiled in.
#[cfg(feature = "crs-projection")]
fn to_wgs84(points: &[[f64; 2]], source_epsg: u32) -> Result<Vec<[f64; 2]>, String> {
    gungnir_data::geospatial::crs::to_wgs84(points, source_epsg).map_err(|e| e.to_string())
}

#[cfg(not(feature = "crs-projection"))]
fn to_wgs84(_points: &[[f64; 2]], source_epsg: u32) -> Result<Vec<[f64; 2]>, String> {
    Err(format!(
        "terrain.frame declares EPSG:{source_epsg}; converting a real-world coordinate \
         reference system other than WGS84 geographic needs the crs-projection feature, which \
         this build was not compiled with"
    ))
}

/// `lon_lat_deg` (the same length and order as `mesh.positions`) placed into the local
/// ENU frame anchored at `origin`, via `gungnir_coord::Wgs84` -- the same
/// oracle-verified tangent-plane transform every other geodetic thing in the system
/// already goes through, rather than a second implementation of the same math here.
///
/// Each vertex's own height (`mesh.positions[_][2]`, `f32::NAN` for a no-data cell)
/// feeds the conversion as that point's altitude, which is the geometrically correct
/// input for *where the point sits horizontally* relative to an origin at a different
/// altitude, however small the effect; a no-data cell's non-finite height is read as
/// `0.0` for this purpose only; the output `z` stays untouched (`TerrainMesh::with_xy`
/// carries it through unchanged, `NaN` included) since this is a horizontal conversion,
/// not a vertical datum reconciliation the DEM's own height does or does not need.
fn to_local_enu(mesh: &TerrainMesh, lon_lat_deg: &[[f64; 2]], origin: [f64; 3]) -> Vec<[f64; 2]> {
    let origin = gungnir_coord::Geodetic {
        lat_rad: origin[0],
        lon_rad: origin[1],
        alt_m: origin[2],
    };
    mesh.positions
        .iter()
        .zip(lon_lat_deg)
        .map(|(p, &[lon_deg, lat_deg])| {
            let alt_m = if p[2].is_finite() {
                f64::from(p[2])
            } else {
                0.0
            };
            let point = gungnir_coord::Geodetic {
                lat_rad: lat_deg.to_radians(),
                lon_rad: lon_deg.to_radians(),
                alt_m,
            };
            let enu = gungnir_coord::Wgs84::ecef_to_enu(
                gungnir_coord::Wgs84::geodetic_to_ecef(point),
                origin,
            );
            [enu.e_m, enu.n_m]
        })
        .collect()
}
