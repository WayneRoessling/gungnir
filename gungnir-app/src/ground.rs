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
//! loaded, or a staged store's ground once line of sight has read it (below), when the
//! baseline states its vertical error (`terrain.vertical_sigma_m`), and otherwise says why
//! there is none: no terrain configured, still loading or being read, refused, or its error
//! not stated. The node loads no terrain and lends none, so every such height on it
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
//!
//! **A staged store** (GAP-227, `docs/design/DN-36-staged-terrain-and-map-tiles.md`) is
//! read through line of sight's field, not a tile lookup of its own: the
//! [`gungnir_analytics::HeightField`] [`crate::terrain_tiles`] paints from the pinned store
//! alone, over the coverage extent, at the finest level that keeps it within its budget
//! (DN-36 §5.2). That extent is every declared sensor's range and approach with one
//! maximum range to spare, so a UAS the picture's sensors can see lies inside it, and one
//! reported past it is said to be, not placed. The field is read from the
//! manifest-verified tiles and never the fill cache (D-164); it is built once per extent on
//! a worker thread, so the feed reads a raster and never a tile file on the gateway's
//! thread; and the UAS sits on the ground line of sight masks against. It is immutable
//! once painted, so the model shares the desktop's `Arc` rather than copying it, and a
//! field painted anew is lent anew. Its ups are interpolated bilinearly between cell
//! centres, at the level the field was painted at, and the name the report carries says
//! which level that was. Where the field does not reach, the reason says which edge: past
//! the coverage extent, past the store's edge (read through the store's placed footprint),
//! or a cell inside the store with no height (a refused tile, or a hole in the source).
//! `terrain.vertical_sigma_m` is applied as for one DEM, unchanged.

use std::sync::Arc;

use gungnir_analytics::{HeightField, MeshSurface, SurfaceMiss};
use gungnir_data::geospatial::TerrainMesh;
use gungnir_ingest::terrain::{GroundElevation, GroundHeight};
use gungnir_model::{Geodetic, LocalFrame};
use gungnir_viewport3d::streaming::terrain::StoreFootprint;

use crate::state::AppState;
use crate::terrain::{SightState, TerrainStatus};

/// The most vertices the coarse search scans for each report.
pub const COARSE_SAMPLES: usize = gungnir_analytics::terrain::COARSE_SAMPLES;

/// What every bound feed's handle says before the terrain has been read at all.
pub const NOT_YET_LENT: &str = "the terrain has not been read yet";

/// A placed DEM, or a staged store's ground over the coverage extent, as a ground
/// elevation model (module documentation).
pub struct PlacedGround {
    name: String,
    frame: LocalFrame,
    sigma_m: f64,
    surface: Surface,
}

/// The surface a [`PlacedGround`] reads.
enum Surface {
    /// One placed DEM's mesh.
    Mesh(MeshSurface),
    /// Line of sight's field painted from a staged store, and the store's placed footprint,
    /// which says where its edge lies.
    Store {
        field: Arc<HeightField>,
        footprint: Box<StoreFootprint>,
    },
}

impl PlacedGround {
    /// The model for a staged store's ground: line of sight's `field`, painted from the
    /// pinned store whose placed footprint is `footprint`, in `frame`, its heights good to
    /// `sigma_m` (module documentation).
    #[must_use]
    pub fn on_store(
        name: impl Into<String>,
        field: Arc<HeightField>,
        footprint: StoreFootprint,
        frame: LocalFrame,
        sigma_m: f64,
    ) -> Self {
        PlacedGround {
            name: name.into(),
            frame,
            sigma_m,
            surface: Surface::Store {
                field,
                footprint: Box::new(footprint),
            },
        }
    }

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
            surface: Surface::Mesh(surface),
        })
    }

    /// The surface's up at a frame east and north: a mesh's interpolated across the
    /// triangle under it, a store's field's bilinearly between its cell centres.
    ///
    /// # Errors
    ///
    /// When the surface does not reach the point, or touches a cell with no data there; the
    /// reason says which edge, for a store (module documentation).
    pub fn up_at(&self, east: f64, north: f64) -> Result<f64, String> {
        let name = &self.name;
        match &self.surface {
            Surface::Mesh(surface) => surface.up_at(east, north).map_err(|miss| match miss {
                SurfaceMiss::NoHeight => {
                    format!("the position falls in a cell of {name} with no height")
                }
                SurfaceMiss::Off => format!("the position lies off {name}"),
            }),
            Surface::Store { field, footprint } => field.up_at(east, north).ok_or_else(|| {
                let ([e0, n0], [e1, n1]) = (field.origin_enu, field.far_corner());
                let [x, y] = footprint.frame_of(east, north);
                let [[x0, y0], [x1, y1]] = footprint.index().extent();
                if !(east >= e0 && east <= e1 && north >= n0 && north <= n1) {
                    format!(
                        "the position lies past the coverage extent line of sight reads \
                         {name} over"
                    )
                } else if !(x >= x0 && x <= x1 && y >= y0 && y <= y1) {
                    format!("the position lies past the edge of {name}")
                } else {
                    format!(
                        "the position falls in a cell of {name} with no height (a refused tile \
                         or a hole in the source)"
                    )
                }
            }),
        }
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

/// Lend the placed DEM, or a staged store's ground, to every bound radar feed, or say why
/// there is none (module documentation). Runs every tick and acts only when the terrain's
/// standing or line of sight's field has changed, so the lookup is built once per load or
/// field.
pub fn lend_to_feeds(state: &mut AppState) {
    // The field is compared by pointer: a field painted anew is a new `Arc`, and comparing
    // millions of cells every tick would cost what the check is there to save.
    if let Some((terrain, field)) = &state.terrain_lent {
        let same_field = match (field, &state.height_field) {
            (Some(lent), Some(now)) => Arc::ptr_eq(lent, now),
            (None, None) => true,
            _ => false,
        };
        if same_field && *terrain == state.terrain {
            return;
        }
    }
    state.terrain_lent = Some((state.terrain.clone(), state.height_field.clone()));
    match lendable(state) {
        Ok(ground) => state.terrain_feeds.set_available(Arc::new(ground)),
        Err(reason) => state.terrain_feeds.set_unavailable(reason),
    }
}

fn lendable(state: &AppState) -> Result<PlacedGround, String> {
    let (path, level) = match &state.terrain {
        TerrainStatus::NotConfigured => {
            return Err("no DEM is configured in this baseline (terrain)".to_owned())
        }
        TerrainStatus::Loading { path } => return Err(format!("{path} is still loading")),
        TerrainStatus::Failed { path, reason } => {
            return Err(format!("{path} did not load: {reason}"))
        }
        TerrainStatus::Loaded { path, .. } => (path, None),
        TerrainStatus::StoreVerified { store, sight, .. } => match sight {
            SightState::Building => {
                return Err(format!(
                    "{store}'s ground over the coverage extent is still being read"
                ))
            }
            SightState::NotBuilt(reason) => {
                return Err(format!(
                    "{store}'s ground over the coverage extent is not read: {reason}"
                ))
            }
            SightState::Built { level, .. } => (store, Some(*level)),
        },
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
    if let Some(level) = level {
        let (Some(field), Some(footprint)) = (&state.height_field, state.terrain_tiles.footprint())
        else {
            return Err(format!(
                "{path}'s ground over the coverage extent is read but not held"
            ));
        };
        return Ok(PlacedGround::on_store(
            format!("{path} at level {level}"),
            Arc::clone(field),
            footprint.clone(),
            frame,
            sigma_m,
        ));
    }
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
