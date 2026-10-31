// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's DEM lent to its radar feeds as a ground elevation model (GAP-199, D-130).
//!
//! ASTERIX Category 129 may state a UAS's height only above the ground under it
//! (I129/100). The feed's adapter places such a UAS on the WGS-84 ellipsoid with the ground
//! height a host lends it through a [`gungnir_ingest::terrain::TerrainHandle`], the way it
//! corrects a height above mean sea level with the geoid this desktop lends
//! ([`crate::geoid::lend_to_feeds`]). [`lend_to_feeds`] lends the placed DEM once it has
//! loaded and the baseline states its vertical error (`terrain.vertical_sigma_m`), and
//! otherwise says why there is none: no DEM configured, still loading, refused, or its
//! error not stated. The node loads no terrain and lends none, so every such height on it
//! is unknown (D-130).
//!
//! **How the ground is read.** The placed DEM is a mesh of vertices at their local ENU
//! positions, one per grid cell, with two triangles per cell
//! (`gungnir_data::geospatial::TerrainMesh`). The ground under a latitude and longitude is
//! the mesh's surface there: the report's position goes into the frame, the triangle under
//! it in the east-north plane is found, and its up is interpolated across the triangle,
//! the same surface PN-02 draws and line of sight masks against. That point goes back out
//! of the frame for its ellipsoidal height, and the lookup is repeated once at that height,
//! since the frame's up tilts away from the local vertical with distance from its origin.
//! A cell with no data is a hole, and a position under one, or off the mesh, has no ground
//! height: an error, never a zero.
//!
//! **Finding the triangle** is `gungnir_analytics::MeshSurface`'s, since GAP-247 moved the
//! lookup down so the coverage analytics and the feeds read one surface
//! (`docs/design/DN-34-sensor-dead-spots.md` §3): a coarse subsample of the grid finds the
//! neighbourhood, and only the cells within a stride of it are searched. The model holds
//! its own copy of the surface, because the feed's adapter holds it on the gateway's side
//! and the desktop's copy belongs to the viewport.

use std::sync::Arc;

use gungnir_analytics::{MeshSurface, SurfaceMiss};
use gungnir_data::geospatial::TerrainMesh;
use gungnir_ingest::terrain::{GroundElevation, GroundHeight};
use gungnir_model::{Geodetic, LocalFrame};

use crate::state::AppState;
use crate::terrain::TerrainStatus;

/// The most vertices the coarse search scans for each report.
pub const COARSE_SAMPLES: usize = gungnir_analytics::terrain::COARSE_SAMPLES;

/// What every bound feed's handle says before the terrain has been read at all.
pub const NOT_YET_LENT: &str = "the terrain has not been read yet";

/// A placed DEM as a ground elevation model (module documentation).
pub struct PlacedGround {
    name: String,
    frame: LocalFrame,
    sigma_m: f64,
    surface: MeshSurface,
}

impl PlacedGround {
    /// The model for `mesh`, placed in `frame`, whose heights are good to `sigma_m`.
    ///
    /// # Errors
    ///
    /// When the mesh was not built from a grid, or is narrower than one cell either way.
    pub fn new(
        name: impl Into<String>,
        mesh: &TerrainMesh,
        frame: LocalFrame,
        sigma_m: f64,
    ) -> Result<Self, String> {
        let name = name.into();
        let surface = MeshSurface::new(mesh).map_err(|_| {
            format!(
                "{name} is not a grid of at least two rows and two columns, so it has no \
                 surface to read a ground height from"
            )
        })?;
        Ok(PlacedGround {
            name,
            frame,
            sigma_m,
            surface,
        })
    }

    /// The mesh's up at a frame east and north, interpolated across the triangle under it.
    ///
    /// # Errors
    ///
    /// When no triangle lies under the point, or the one that does touches a cell with no
    /// data.
    pub fn up_at(&self, east: f64, north: f64) -> Result<f64, String> {
        self.surface.up_at(east, north).map_err(|miss| match miss {
            SurfaceMiss::NoHeight => format!(
                "the position falls in a cell of {} with no height",
                self.name
            ),
            SurfaceMiss::Off => format!("the position lies off {}", self.name),
        })
    }
}

impl GroundElevation for PlacedGround {
    fn name(&self) -> &str {
        &self.name
    }

    fn ground(&self, lat_deg: f64, lon_deg: f64) -> Result<GroundHeight, String> {
        if !(lat_deg.is_finite() && lon_deg.is_finite()) {
            return Err(format!("the position {lat_deg}, {lon_deg} is not finite"));
        }
        let (lat_rad, lon_rad) = (lat_deg.to_radians(), lon_deg.to_radians());
        // Read at height 0, then again at the ground height found (module documentation).
        let mut ellipsoidal_m = 0.0;
        for _ in 0..2 {
            let [east, north, _] = self.frame.to_enu(Geodetic {
                lat_rad,
                lon_rad,
                alt_m: ellipsoidal_m,
            });
            let up = self.up_at(east, north)?;
            ellipsoidal_m = self.frame.to_geodetic([east, north, up]).alt_m;
        }
        Ok(GroundHeight {
            ellipsoidal_m,
            sigma_m: self.sigma_m,
        })
    }
}

/// Lend the placed DEM to every bound radar feed, or say why there is none (module
/// documentation). Runs every tick and acts only when the terrain's standing has changed,
/// so the lookup is built once per load.
pub fn lend_to_feeds(state: &mut AppState) {
    if state.terrain_lent.as_ref() == Some(&state.terrain) {
        return;
    }
    state.terrain_lent = Some(state.terrain.clone());
    match lendable(state) {
        Ok(ground) => state.terrain_feeds.set_available(Arc::new(ground)),
        Err(reason) => state.terrain_feeds.set_unavailable(reason),
    }
}

fn lendable(state: &AppState) -> Result<PlacedGround, String> {
    let path = match &state.terrain {
        TerrainStatus::NotConfigured => {
            return Err("no DEM is configured in this baseline (terrain)".to_owned())
        }
        TerrainStatus::Loading { path } => return Err(format!("{path} is still loading")),
        TerrainStatus::Failed { path, reason } => {
            return Err(format!("{path} did not load: {reason}"))
        }
        TerrainStatus::Loaded { path, .. } => path,
        status @ TerrainStatus::StoreVerified { .. } => return Err(status.line()),
    };
    let Some(sigma_m) = state
        .config
        .terrain
        .as_ref()
        .and_then(|t| t.vertical_sigma_m)
    else {
        return Err(format!(
            "terrain.vertical_sigma_m is not stated, so {path}'s vertical error is unknown"
        ));
    };
    let Some(frame) = state.config.origin.map(|[lat_rad, lon_rad, alt_m]| {
        LocalFrame::new(Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
    }) else {
        return Err("the deployment declares no local frame origin (config.origin)".to_owned());
    };
    let Some(mesh) = state.data.terrains.first() else {
        return Err(format!("{path} is loaded but no surface is held"));
    };
    PlacedGround::new(path.clone(), mesh, frame, sigma_m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_data::geospatial::{GridCrs, HeightGrid};

    fn frame() -> LocalFrame {
        LocalFrame::new(Geodetic {
            lat_rad: 0.9,
            lon_rad: 0.2,
            alt_m: 0.0,
        })
    }

    /// A 5-by-5 grid of 100 m cells centred on the frame origin, rising 1 m per metre
    /// east plus 100 m, with one hole, placed in the local frame as it stands.
    fn slope(hole: Option<(u32, u32)>) -> TerrainMesh {
        let (rows, columns) = (5u32, 5u32);
        let mut heights = Vec::new();
        for row in 0..rows {
            for column in 0..columns {
                let east = (f64::from(column) - 2.0) * 100.0;
                #[allow(clippy::cast_possible_truncation)] // a few hundred metres
                let h = (100.0 + east) as f32;
                heights.push(if hole == Some((row, column)) {
                    -9999.0
                } else {
                    h
                });
            }
        }
        let grid = HeightGrid {
            columns,
            rows,
            origin: [-250.0, -250.0],
            cell_size: [100.0, 100.0],
            nodata: Some(-9999.0),
            crs: GridCrs::Unstated,
            vertical: None,
            heights,
        };
        TerrainMesh::from_grid(&grid)
            .expect("a small grid")
            .placed()
    }

    /// Between vertices the surface is interpolated, not snapped to the nearest one: a
    /// plane is reproduced exactly anywhere inside the grid.
    #[test]
    fn the_ground_is_the_mesh_surface_between_its_vertices() {
        let ground = PlacedGround::new("slope", &slope(None), frame(), 4.0).expect("a grid");
        for (east, north) in [(0.0, 0.0), (37.0, -12.5), (-150.0, 180.0), (199.0, 0.0)] {
            let up = ground.up_at(east, north).expect("inside");
            assert!((up - (100.0 + east)).abs() < 1e-3, "{east},{north}: {up}");
        }
    }

    /// Off the grid, or in a hole, there is no ground height, and the reason says which.
    #[test]
    fn off_the_grid_or_in_a_hole_is_an_error_not_a_zero() {
        let ground =
            PlacedGround::new("slope", &slope(Some((2, 2))), frame(), 4.0).expect("a grid");
        let off = ground.up_at(1_000.0, 0.0).expect_err("off the grid");
        assert!(off.contains("lies off slope"), "{off}");
        let hole = ground.up_at(10.0, 10.0).expect_err("in the hole");
        assert!(hole.contains("no height"), "{hole}");
    }

    /// Through the frame: a position over the origin reads the ground there as an
    /// ellipsoidal height (the origin's own is 0), with the stated error.
    #[test]
    fn a_latitude_and_longitude_read_the_ellipsoidal_ground_height() {
        let ground = PlacedGround::new("slope", &slope(None), frame(), 4.0).expect("a grid");
        let at = ground
            .ground(0.9f64.to_degrees(), 0.2f64.to_degrees())
            .expect("over the origin");
        assert!((at.ellipsoidal_m - 100.0).abs() < 1e-3, "{at:?}");
        assert!((at.sigma_m - 4.0).abs() < f64::EPSILON);
        assert!(ground.ground(f64::NAN, 0.0).is_err());
        // 0.01 rad of latitude is about 64 km north: off a 500 m DEM.
        assert!(ground
            .ground(0.91f64.to_degrees(), 0.2f64.to_degrees())
            .expect_err("off")
            .contains("lies off"));
    }

    /// A mesh not built from a grid has no surface to read.
    #[test]
    fn a_mesh_with_no_grid_is_refused() {
        let reason = PlacedGround::new("hand", &TerrainMesh::default(), frame(), 4.0)
            .err()
            .expect("refused");
        assert!(reason.contains("not a grid"), "{reason}");
    }
}
