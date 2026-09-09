// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain: digital elevation models (GAP-023).
//!
//! Two formats, decided by the owner on 2026-09-06 (`ARCHITECTURE.md` §10 item 84;
//! `docs/agentic-coding-standards.md` §2.9, "Terrain"):
//!
//! - **ESRI ASCII grid** (`.asc`): a six-line header and a matrix of numbers. No crate.
//! - **GeoTIFF** (`.tif`, `.tiff`): the raster is decoded by `tiff`; the georeferencing
//!   tags (`ModelPixelScale` 33550, `ModelTiepoint` 33922, `GeoKeyDirectory` 34735) and
//!   GDAL's no-data tag 42113 are interpreted **here**, per OGC GeoTIFF 1.1
//!   (OGC 19-008r4) §7. `tiff` names the tag numbers and nothing more.
//!
//! Nothing outside this crate sees a `tiff` type. Both loaders return a `HeightGrid`,
//! and `TerrainMesh::from_grid` turns one into vertices for the viewport; a corrupt file
//! is a `DataError`, never a panic (`verification-capability-table.md` §2, `gungnir-data`
//! row). Terrain classification (GAP-082) stays unwritten and says so.
//!
//! **Real-world CRS conversion** (signed off 2026-09-08, D-41): a file's `GridCrs` no
//! longer has to be `Unstated` or the deployment's own local frame. `crs::to_wgs84`
//! (behind the `crs` feature) converts a geographic or projected CRS with a
//! known EPSG code into WGS84 geographic via `proj`; `gungnir-app` carries the result
//! the rest of the way to local ENU, since that step needs `gungnir-coord`, which this
//! crate does not depend on. See the `crs` module below for the reasoning and the
//! feature's CI-feasibility caveat.

// "GeoTIFF" is a proper noun and not an identifier; the lint would have it in backticks.
#![allow(clippy::doc_markdown)]

use std::path::Path;

use crate::DataError;

/// How a grid's `origin` and `cell_size` are to be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GridCrs {
    /// Geographic (angular) coordinates; `epsg` is `GeographicTypeGeoKey` when present.
    Geographic { epsg: Option<u16> },
    /// Projected (linear) coordinates; `epsg` is `ProjectedCSTypeGeoKey` when present.
    Projected { epsg: Option<u16> },
    /// The file did not say. An ASCII grid never does; its `.prj` sidecar is not read.
    #[default]
    Unstated,
}

/// A regular grid of heights, one per cell, stored north row first, which is the order
/// both formats store them in.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    pub columns: u32,
    pub rows: u32,
    /// `[x, y]` of the outer corner of the south-west cell: ESRI's `xllcorner` and
    /// `yllcorner`, or a GeoTIFF tiepoint adjusted for its raster type.
    pub origin: [f64; 2],
    /// `[dx, dy]`, both positive.
    pub cell_size: [f64; 2],
    /// The value that marks a cell with no height, as the file wrote it.
    pub nodata: Option<f32>,
    pub crs: GridCrs,
    /// `rows * columns` values, row-major, north row first. No-data cells hold `nodata`
    /// exactly as written, so a caller can tell "no data" from "sea level".
    pub heights: Vec<f32>,
}

impl HeightGrid {
    /// `[[min_x, min_y], [max_x, max_y]]` of the grid's outer edges.
    #[must_use]
    pub fn bounds(&self) -> [[f64; 2]; 2] {
        let [x, y] = self.origin;
        [
            [x, y],
            [
                x + f64::from(self.columns) * self.cell_size[0],
                y + f64::from(self.rows) * self.cell_size[1],
            ],
        ]
    }

    /// The height at `(row, column)`, north row first; `None` outside the grid or where
    /// the cell holds the no-data value.
    // The no-data value is a sentinel the file wrote bit-for-bit; a cell is "no data"
    // when it holds exactly that, and a margin would turn a real height near it into a
    // hole.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn height_at(&self, row: u32, column: u32) -> Option<f32> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        let index = row as usize * self.columns as usize + column as usize;
        let h = *self.heights.get(index)?;
        match self.nodata {
            Some(nodata) if h == nodata => None,
            _ if h.is_nan() => None,
            _ => Some(h),
        }
    }

    /// Cells with a height.
    #[must_use]
    pub fn valid_count(&self) -> usize {
        (0..self.rows)
            .flat_map(|r| (0..self.columns).map(move |c| (r, c)))
            .filter(|&(r, c)| self.height_at(r, c).is_some())
            .count()
    }
}

/// A terrain surface for the viewport: one vertex per grid cell at the cell's centre,
/// positioned **relative to `origin`** so that a projected or geographic coordinate in
/// the hundreds of thousands survives the `f32`. Two triangles per cell whose four
/// corner vertices all have a height; cells touching a no-data cell leave a hole rather
/// than a wall.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerrainMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// What the `x` and `y` of every position are relative to, and in what frame.
    pub origin: [f64; 2],
    pub crs: GridCrs,
    /// The grid the vertices came from, north row first, `rows * columns` positions,
    /// so a viewport can decimate by stride rather than by triangle. Zero for a mesh
    /// built by hand rather than from a grid.
    pub rows: u32,
    pub columns: u32,
}

impl TerrainMesh {
    /// # Errors
    ///
    /// When the grid has more cells than a `u32` index can name.
    pub fn from_grid(grid: &HeightGrid) -> Result<Self, DataError> {
        let vertex_count = grid.columns.checked_mul(grid.rows).ok_or_else(|| {
            DataError::Parse(format!(
                "a {} by {} grid has more vertices than a u32 index can name",
                grid.columns, grid.rows
            ))
        })?;
        let mut positions = Vec::with_capacity(vertex_count as usize);
        for row in 0..grid.rows {
            for column in 0..grid.columns {
                // Cell centre, relative to the south-west corner; rows are stored
                // north first, so the last row is nearest the origin.
                let x = (f64::from(column) + 0.5) * grid.cell_size[0];
                let y = (f64::from(grid.rows - 1 - row) + 0.5) * grid.cell_size[1];
                let z = grid.height_at(row, column).unwrap_or(f32::NAN);
                positions.push([relative_to_f32(x), relative_to_f32(y), z]);
            }
        }
        let mut indices = Vec::new();
        for row in 0..grid.rows.saturating_sub(1) {
            for column in 0..grid.columns.saturating_sub(1) {
                let corners = [
                    (row, column),
                    (row, column + 1),
                    (row + 1, column),
                    (row + 1, column + 1),
                ];
                if corners.iter().any(|&(r, c)| grid.height_at(r, c).is_none()) {
                    continue;
                }
                let at = |r: u32, c: u32| r * grid.columns + c;
                let (nw, ne, sw, se) = (
                    at(row, column),
                    at(row, column + 1),
                    at(row + 1, column),
                    at(row + 1, column + 1),
                );
                indices.extend_from_slice(&[nw, sw, ne, ne, sw, se]);
            }
        }
        Ok(Self {
            positions,
            indices,
            origin: grid.origin,
            crs: grid.crs,
            rows: grid.rows,
            columns: grid.columns,
        })
    }

    /// Triangles in the surface.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The same surface with `origin` folded into every position, for a grid whose frame
    /// **is** the picture's (a DEM prepared in local ENU metres). The origin becomes zero.
    /// Meaningless for a geographic or projected grid, which the caller must refuse
    /// first; this method does not know and does not guess.
    #[must_use]
    pub fn placed(mut self) -> Self {
        let [ox, oy] = self.origin;
        for p in &mut self.positions {
            p[0] = relative_to_f32(f64::from(p[0]) + ox);
            p[1] = relative_to_f32(f64::from(p[1]) + oy);
        }
        self.origin = [0.0, 0.0];
        self
    }

    /// Every position's `[x, y]`, absolute in this mesh's own frame -- `origin` (kept in
    /// full `f64`) plus each position's small `f32` offset -- in the same order as
    /// `positions`. Reconstructing this way, rather than reading `positions` alone,
    /// never rounds a UTM- or WGS84-scale absolute coordinate to `f32`; only the small
    /// offset from a nearby origin ever was `f32` to begin with.
    ///
    /// Exists for a real-world-CRS conversion (GAP-023, D-41): a caller reprojects the
    /// result (`geospatial::crs::to_wgs84`, or a further conversion this crate does not
    /// hold, such as `gungnir_coord`'s geographic-to-local-ENU) and returns it through
    /// [`Self::with_xy`], which is the only place a coordinate re-enters `f32`.
    #[must_use]
    pub fn absolute_positions_xy(&self) -> Vec<[f64; 2]> {
        let [ox, oy] = self.origin;
        self.positions
            .iter()
            .map(|p| [ox + f64::from(p[0]), oy + f64::from(p[1])])
            .collect()
    }

    /// This mesh with its `[x, y]` replaced by `xy` (the same length and order as
    /// `positions` -- `absolute_positions_xy`'s own order) and `crs` set to `new_crs`.
    /// Height (`positions[_][2]`), `indices` and the grid shape (`rows`, `columns`) are
    /// unchanged: only where a vertex sits moved, never which ones exist or how they
    /// connect. `origin` becomes `[0.0, 0.0]`: `xy` is already absolute in whatever
    /// frame the caller placed it into, with nothing left to fold in later --
    /// `.placed()` on the result is a no-op, the same as it would be on any mesh whose
    /// origin is already zero.
    ///
    /// # Errors
    ///
    /// When `xy.len()` does not equal `self.positions.len()`.
    pub fn with_xy(&self, xy: &[[f64; 2]], new_crs: GridCrs) -> Result<Self, DataError> {
        if xy.len() != self.positions.len() {
            return Err(DataError::Parse(format!(
                "{} coordinates for {} vertices",
                xy.len(),
                self.positions.len()
            )));
        }
        let positions = self
            .positions
            .iter()
            .zip(xy)
            .map(|(p, &[x, y])| [relative_to_f32(x), relative_to_f32(y), p[2]])
            .collect();
        Ok(Self {
            positions,
            indices: self.indices.clone(),
            origin: [0.0, 0.0],
            crs: new_crs,
            rows: self.rows,
            columns: self.columns,
        })
    }
}

/// A cell offset within one grid is at most `columns * cell_size`, which is well inside
/// `f32`'s exact range for any DEM the viewport can hold; the `f64` origin carries the
/// large part.
#[allow(clippy::cast_possible_truncation)]
fn relative_to_f32(v: f64) -> f32 {
    v as f32
}

/// A raster sample as an `f32` height. Sixty-four-bit samples lose precision here,
/// which is a property of `TerrainMesh`'s `f32` positions and not of the loader.
#[allow(clippy::cast_possible_truncation)]
fn sample_to_f32(v: f64) -> f32 {
    v as f32
}

pub mod classification {
    /// # Errors
    ///
    /// Always: terrain classification is designed and not written (GAP-082).
    ///
    /// **Returns a `Result` rather than an empty `Vec`**, which would have read as
    /// "classified, and nothing was found".
    pub fn classify(_terrain: &super::TerrainMesh) -> Result<Vec<u8>, crate::DataError> {
        Err(crate::DataError::NotImplemented {
            what: "terrain classification",
            waiting_on: "GAP-082, a classification design over the loaded grid",
        })
    }
}

/// Real-world coordinate reference system conversion for a DEM (GAP-023, D-41): behind
/// the `crs` feature, off by default. `docs/agentic-coding-standards.md`
/// §2.9's D-41 entry says why: `proj-sys` links `libproj`, built from its own vendored
/// source (`bundled_proj`) with `cmake` and a C/C++ toolchain, a heavier and more
/// environment-dependent build than the rest of this crate pays for by default -- no
/// host this change could confirm build against had `cmake` or `libclang`, so this is
/// verified in CI (a dedicated workflow), not by this crate's ordinary `cargo test`.
///
/// Deliberately stops at WGS84 geographic (EPSG:4326) rather than going the rest of the
/// way to local ENU: this crate depends on no other workspace crate
/// (`ARCHITECTURE.md`'s dependency table), so it cannot reach `gungnir_coord::Wgs84`,
/// the oracle-verified tangent-plane transform every other geodetic thing in the system
/// already goes through. `gungnir-app` (which already depends on both `gungnir-data`
/// and `gungnir-coord`) carries a WGS84 result the rest of the way, using the
/// deployment's own declared origin (`ConfigBaseline::origin`) -- the same origin
/// everything else in the local picture is relative to, rather than a second one this
/// module would otherwise have to invent.
#[cfg(feature = "crs")]
pub mod crs {
    use crate::DataError;

    /// WGS84 geographic, EPSG:4326: what [`to_wgs84`] converts into.
    pub const WGS84_EPSG: u32 = 4326;

    /// Reproject `points`, each stated in `source_epsg`, into WGS84 geographic degrees
    /// (`[longitude, latitude]`) via PROJ.
    ///
    /// `Proj::new_known_crs` normalises **both** the input and the output coordinate
    /// order to Longitude/Latitude or Easting/Northing, regardless of a CRS's own
    /// authority-defined axis order (docs.rs, `proj::Proj::new_known_crs`: EPSG:4326's
    /// own definition is Latitude, Longitude, and the crate overrides that so a caller
    /// never has to remember to reverse it). That is what makes it correct to write
    /// `points` here as `[longitude, latitude]` for a geographic `source_epsg` and
    /// `[easting, northing]` for a projected one, with no separate case for either --
    /// otherwise the classic swapped-axis bug this function exists to not have.
    ///
    /// # Errors
    ///
    /// `DataError::Parse` when `source_epsg` is not a coordinate reference system PROJ
    /// recognises, when PROJ cannot otherwise build the transform, when a point is not
    /// finite, or when a point falls outside the transform's domain.
    pub fn to_wgs84(points: &[[f64; 2]], source_epsg: u32) -> Result<Vec<[f64; 2]>, DataError> {
        let from = format!("EPSG:{source_epsg}");
        let to = format!("EPSG:{WGS84_EPSG}");
        let transformer = proj::Proj::new_known_crs(&from, &to, None).map_err(|e| {
            DataError::Parse(format!(
                "{from} is not a coordinate reference system PROJ recognises: {e}"
            ))
        })?;
        points
            .iter()
            .map(|&[x, y]| {
                if !(x.is_finite() && y.is_finite()) {
                    return Err(DataError::Parse(format!(
                        "point ({x}, {y}) in {from} is not finite"
                    )));
                }
                let (lon, lat) = transformer.convert((x, y)).map_err(|e| {
                    DataError::Parse(format!(
                        "{from} point ({x}, {y}) did not convert to WGS84: {e}"
                    ))
                })?;
                Ok([lon, lat])
            })
            .collect()
    }
}

/// Independent of any Python or `pyproj` check (this crate's CI has no interpreter
/// installed for the feature this gates): a closed-form point derived from the
/// transverse Mercator projection's own construction, the same "hand-checked" spirit
/// `gungnir-data-fusion/src/point_to_plane.rs` and `normals.rs` document for their own
/// independent verification, just checkable from the projection's definition instead of
/// re-running it in another language.
// `#[cfg(test)]` on the outside and the feature gate within, rather than one combined
// `cfg(all(test, ...))`: `gungnir-app/tests/architecture_compliance.rs`'s unwrap-policy
// scanner strips `#[cfg(test)]`-gated modules by matching that literal attribute, so a
// combined form would leave these tests' `expect(...)` looking like production code.
#[cfg(test)]
mod crs_tests {
    #![cfg(feature = "crs")]

    use super::crs::to_wgs84;

    /// UTM zone 33N's central meridian is 15°E. On the central meridian, at every
    /// latitude, the transverse Mercator's projected x-coordinate is exactly zero by
    /// the projection's own left-right symmetry about that meridian; UTM's uniform
    /// scale factor (0.9996) maps zero to zero, so the 500,000 m false easting is the
    /// *only* contributor to easting there, for any ellipsoid the projection uses. The
    /// same symmetry makes the meridional arc length from the equator, measured along
    /// the central meridian, exactly zero at the equator itself, so northing there is
    /// the false northing (0 m, northern hemisphere) alone. EPSG:32633 (500000, 0) must
    /// therefore convert to WGS84 (15.0 deg E, 0.0 deg N) to within numerical precision
    /// -- a fact of UTM's definition, not of this crate or of `proj`'s implementation.
    #[test]
    fn utm_33n_central_meridian_at_the_equator_is_15_east_0_north() {
        let out = to_wgs84(&[[500_000.0, 0.0]], 32633).expect("PROJ knows EPSG:32633");
        let [lon, lat] = out[0];
        assert!((lon - 15.0).abs() < 1e-6, "longitude: {lon}");
        assert!(lat.abs() < 1e-6, "latitude: {lat}");
    }

    /// A point already in WGS84 converted to WGS84 is unchanged: the identity case,
    /// and a check that axis order is what the module documentation claims (longitude
    /// first), not silently swapped.
    #[test]
    fn wgs84_to_wgs84_is_the_identity() {
        let out = to_wgs84(&[[15.0, 47.0]], 4326).expect("EPSG:4326 is always known");
        let [lon, lat] = out[0];
        assert!((lon - 15.0).abs() < 1e-9, "longitude: {lon}");
        assert!((lat - 47.0).abs() < 1e-9, "latitude: {lat}");
    }

    #[test]
    fn an_unknown_epsg_code_is_refused_by_name() {
        let err = to_wgs84(&[[0.0, 0.0]], 999_999_999).expect_err("no such CRS");
        assert!(err.to_string().contains("999999999"), "{err}");
    }

    #[test]
    fn a_non_finite_point_is_refused_before_proj_sees_it() {
        let err = to_wgs84(&[[f64::NAN, 0.0]], 4326).expect_err("not finite");
        assert!(err.to_string().contains("not finite"), "{err}");
    }
}

/// Load a DEM as a height grid, choosing the format by extension: `.asc` is an ESRI
/// ASCII grid, `.tif` or `.tiff` a GeoTIFF.
///
/// # Errors
///
/// `DataError::Io` when the file cannot be read; `DataError::Parse` when the extension
/// is not one of the two, or the content is not what the extension claims. Never a
/// panic, whatever the bytes.
pub fn load_height_grid(path: &Path) -> Result<HeightGrid, DataError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("asc") => ascii_grid::load(path),
        Some("tif" | "tiff") => geotiff::load(path),
        _ => Err(DataError::Parse(format!(
            "{}: not a DEM this loader reads (expected .asc, .tif or .tiff)",
            path.display()
        ))),
    }
}

/// Load a DEM and build the viewport surface from it.
///
/// # Errors
///
/// As `load_height_grid`, plus a grid too large to index.
pub fn load_dem(path: &Path) -> Result<TerrainMesh, DataError> {
    let grid = load_height_grid(path)?;
    TerrainMesh::from_grid(&grid)
}

/// ESRI ASCII grid: `ncols`, `nrows`, `xllcorner` or `xllcenter`, `yllcorner` or
/// `yllcenter`, `cellsize`, an optional `NODATA_value`, then `nrows` rows of `ncols`
/// values, north row first.
pub mod ascii_grid {
    use std::path::Path;

    use super::{GridCrs, HeightGrid};
    use crate::DataError;

    /// # Errors
    ///
    /// `DataError::Io` when the file cannot be read; `DataError::Parse` for a missing
    /// header key, a non-numeric value, a non-positive size, or a count of heights that
    /// does not match the header.
    pub fn load(path: &Path) -> Result<HeightGrid, DataError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| DataError::Io(format!("{}: {e}", path.display())))?;
        parse(&text).map_err(|e| DataError::Parse(format!("{}: {e}", path.display())))
    }

    /// Parse the text of an ASCII grid.
    ///
    /// # Errors
    ///
    /// `DataError::Parse`, as `load`.
    pub fn parse(text: &str) -> Result<HeightGrid, DataError> {
        let mut tokens = text.split_whitespace().peekable();
        let mut header: Vec<(String, &str)> = Vec::new();
        while let Some(token) = tokens.peek() {
            if !token
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            {
                break;
            }
            let key = tokens.next().unwrap_or_default().to_ascii_lowercase();
            let value = tokens
                .next()
                .ok_or_else(|| DataError::Parse(format!("header key {key} has no value")))?;
            header.push((key, value));
        }
        let lookup = |name: &str| header.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let number = |name: &str| -> Result<f64, DataError> {
            let raw = lookup(name)
                .ok_or_else(|| DataError::Parse(format!("missing header key {name}")))?;
            raw.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    DataError::Parse(format!("header key {name} is not a number: {raw:?}"))
                })
        };
        let dimension = |name: &str| -> Result<u32, DataError> {
            let v = number(name)?;
            if v.fract() != 0.0 || v <= 0.0 || v > f64::from(u32::MAX) {
                return Err(DataError::Parse(format!(
                    "header key {name} must be a positive whole number, got {v}"
                )));
            }
            // Checked above: a positive whole number no larger than `u32::MAX`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let whole = v as u32;
            Ok(whole)
        };
        let columns = dimension("ncols")?;
        let rows = dimension("nrows")?;
        let cell_size = number("cellsize")?;
        if cell_size <= 0.0 {
            return Err(DataError::Parse(format!(
                "cellsize must be positive, got {cell_size}"
            )));
        }
        let x = match (lookup("xllcorner"), lookup("xllcenter")) {
            (Some(_), _) => number("xllcorner")?,
            (None, Some(_)) => number("xllcenter")? - cell_size / 2.0,
            (None, None) => return Err(DataError::Parse("missing header key xllcorner".into())),
        };
        let y = match (lookup("yllcorner"), lookup("yllcenter")) {
            (Some(_), _) => number("yllcorner")?,
            (None, Some(_)) => number("yllcenter")? - cell_size / 2.0,
            (None, None) => return Err(DataError::Parse("missing header key yllcorner".into())),
        };
        let nodata =
            match lookup("nodata_value") {
                Some(raw) => Some(raw.parse::<f32>().map_err(|_| {
                    DataError::Parse(format!("NODATA_value is not a number: {raw:?}"))
                })?),
                None => None,
            };
        let expected = columns
            .checked_mul(rows)
            .ok_or_else(|| DataError::Parse(format!("{columns} by {rows} cells overflow")))?
            as usize;
        let mut heights = Vec::with_capacity(expected.min(1 << 24));
        for (i, token) in tokens.enumerate() {
            if heights.len() == expected {
                return Err(DataError::Parse(format!(
                    "more than {expected} heights: extra value {token:?} at position {i}"
                )));
            }
            let h = token
                .parse::<f32>()
                .map_err(|_| DataError::Parse(format!("height {i} is not a number: {token:?}")))?;
            heights.push(h);
        }
        if heights.len() != expected {
            return Err(DataError::Parse(format!(
                "header promises {expected} heights ({columns} by {rows}); the file holds {}",
                heights.len()
            )));
        }
        Ok(HeightGrid {
            columns,
            rows,
            origin: [x, y],
            cell_size: [cell_size, cell_size],
            nodata,
            crs: GridCrs::Unstated,
            heights,
        })
    }
}

/// GeoTIFF: a single-band raster with `ModelPixelScale` and one `ModelTiepoint`
/// (OGC GeoTIFF 1.1 §7.1.3, the raster-to-model transformation by scale and tiepoint).
/// A file that georeferences by `ModelTransformation` instead is refused by name.
pub mod geotiff {
    use std::fs::File;
    use std::io::BufReader;
    use std::path::Path;

    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    use tiff::ColorType;

    use super::{sample_to_f32, GridCrs, HeightGrid};
    use crate::DataError;

    // GeoKey identifiers (OGC GeoTIFF 1.1 §6.2 and annex B).
    const GT_MODEL_TYPE: u16 = 1024;
    const GT_RASTER_TYPE: u16 = 1025;
    const GEOGRAPHIC_TYPE: u16 = 2048;
    const PROJECTED_CS_TYPE: u16 = 3072;
    const MODEL_TYPE_PROJECTED: u16 = 1;
    const MODEL_TYPE_GEOGRAPHIC: u16 = 2;
    const RASTER_PIXEL_IS_POINT: u16 = 2;

    /// # Errors
    ///
    /// `DataError::Io` when the file cannot be opened; `DataError::Parse` when it is not
    /// a TIFF, has no georeferencing this loader reads, has more than one band, or a
    /// sample type that is not a height.
    pub fn load(path: &Path) -> Result<HeightGrid, DataError> {
        let file =
            File::open(path).map_err(|e| DataError::Io(format!("{}: {e}", path.display())))?;
        let mut decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| DataError::Parse(format!("{}: {e}", path.display())))?;
        read(&mut decoder).map_err(|e| DataError::Parse(format!("{}: {e}", path.display())))
    }

    /// The georeferencing a GeoTIFF states, before the raster is read.
    struct Georeference {
        scale: [f64; 2],
        /// `[x, y]` of the north-west corner of the raster.
        top_left: [f64; 2],
        crs: GridCrs,
    }

    fn read<R: std::io::Read + std::io::Seek>(
        decoder: &mut Decoder<R>,
    ) -> Result<HeightGrid, String> {
        let (columns, rows) = decoder.dimensions().map_err(|e| e.to_string())?;
        if columns == 0 || rows == 0 {
            return Err(format!("the raster is {columns} by {rows}"));
        }
        let keys = geo_keys(decoder)?;
        let georeference = georeference(decoder, &keys)?;
        let nodata = nodata(decoder)?;
        match decoder.colortype().map_err(|e| e.to_string())? {
            ColorType::Gray(_) => {}
            other => return Err(format!("a DEM has one band; this file is {other:?}")),
        }
        let heights = samples(decoder)?;
        let expected = columns as usize * rows as usize;
        if heights.len() != expected {
            return Err(format!(
                "the raster is {columns} by {rows} but decoded to {} samples",
                heights.len()
            ));
        }
        let [dx, dy] = georeference.scale;
        Ok(HeightGrid {
            columns,
            rows,
            origin: [
                georeference.top_left[0],
                georeference.top_left[1] - f64::from(rows) * dy,
            ],
            cell_size: [dx, dy],
            nodata,
            crs: georeference.crs,
            heights,
        })
    }

    /// The `GeoKeyDirectory` as `(key, value)` pairs for the short-valued keys; keys
    /// whose value lives in the double or ASCII parameter tags are not needed here.
    fn geo_keys<R: std::io::Read + std::io::Seek>(
        decoder: &mut Decoder<R>,
    ) -> Result<Vec<(u16, u16)>, String> {
        let Some(_) = decoder
            .find_tag(Tag::GeoKeyDirectoryTag)
            .map_err(|e| e.to_string())?
        else {
            return Ok(Vec::new());
        };
        let directory = decoder
            .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
            .map_err(|e| e.to_string())?;
        let [version, _, _, count, entries @ ..] = directory.as_slice() else {
            return Err("GeoKeyDirectory is shorter than its four-short header".into());
        };
        if *version != 1 {
            return Err(format!(
                "GeoKeyDirectory version {version}; only 1 is defined"
            ));
        }
        let mut keys = Vec::new();
        for [key, location, _, value] in entries.as_chunks::<4>().0.iter().take(usize::from(*count))
        {
            if *location == 0 {
                keys.push((*key, *value));
            }
        }
        Ok(keys)
    }

    fn georeference<R: std::io::Read + std::io::Seek>(
        decoder: &mut Decoder<R>,
        keys: &[(u16, u16)],
    ) -> Result<Georeference, String> {
        let key = |id: u16| keys.iter().find(|(k, _)| *k == id).map(|(_, v)| *v);
        let has_transformation = decoder
            .find_tag(Tag::ModelTransformationTag)
            .map_err(|e| e.to_string())?
            .is_some();
        let scale = match decoder
            .find_tag(Tag::ModelPixelScaleTag)
            .map_err(|e| e.to_string())?
        {
            Some(_) => decoder
                .get_tag_f64_vec(Tag::ModelPixelScaleTag)
                .map_err(|e| e.to_string())?,
            None if has_transformation => {
                return Err(
                    "georeferenced by ModelTransformation, which this loader does not read".into(),
                )
            }
            None => return Err("not a GeoTIFF: no ModelPixelScale tag".into()),
        };
        let [dx, dy, ..] = scale.as_slice() else {
            return Err("ModelPixelScale holds fewer than two values".into());
        };
        if !(dx.is_finite() && *dx > 0.0 && dy.is_finite() && *dy > 0.0) {
            return Err(format!(
                "ModelPixelScale {dx} by {dy} is not positive and finite"
            ));
        }
        let tiepoint = decoder
            .get_tag_f64_vec(Tag::ModelTiepointTag)
            .map_err(|_| "not a GeoTIFF: no ModelTiepoint tag".to_string())?;
        let [i, j, _, x, y, _, ..] = tiepoint.as_slice() else {
            return Err("ModelTiepoint holds fewer than six values".into());
        };
        if ![i, j, x, y].iter().all(|v| v.is_finite()) {
            return Err("ModelTiepoint is not finite".into());
        }
        // The tiepoint names raster (I, J) at model (X, Y). PixelIsArea (the default)
        // means the pixel's corner; PixelIsPoint means its centre, half a cell inward.
        let half = if key(GT_RASTER_TYPE) == Some(RASTER_PIXEL_IS_POINT) {
            0.5
        } else {
            0.0
        };
        let top_left = [x - (i + half) * dx, y + (j + half) * dy];
        let crs = match key(GT_MODEL_TYPE) {
            Some(MODEL_TYPE_PROJECTED) => GridCrs::Projected {
                epsg: key(PROJECTED_CS_TYPE),
            },
            Some(MODEL_TYPE_GEOGRAPHIC) => GridCrs::Geographic {
                epsg: key(GEOGRAPHIC_TYPE),
            },
            _ => GridCrs::Unstated,
        };
        Ok(Georeference {
            scale: [*dx, *dy],
            top_left,
            crs,
        })
    }

    fn nodata<R: std::io::Read + std::io::Seek>(
        decoder: &mut Decoder<R>,
    ) -> Result<Option<f32>, String> {
        if decoder
            .find_tag(Tag::GdalNodata)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            return Ok(None);
        }
        let raw = decoder
            .get_tag_ascii_string(Tag::GdalNodata)
            .map_err(|e| e.to_string())?;
        let trimmed = raw.trim_end_matches('\0').trim();
        trimmed
            .parse::<f32>()
            .map(Some)
            .map_err(|_| format!("GDAL_NODATA is not a number: {trimmed:?}"))
    }

    fn samples<R: std::io::Read + std::io::Seek>(
        decoder: &mut Decoder<R>,
    ) -> Result<Vec<f32>, String> {
        Ok(match decoder.read_image().map_err(|e| e.to_string())? {
            DecodingResult::F32(v) => v,
            DecodingResult::F64(v) => v.into_iter().map(sample_to_f32).collect(),
            DecodingResult::I16(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::U16(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::I8(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::U8(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::I32(v) => v.into_iter().map(|s| sample_to_f32(f64::from(s))).collect(),
            DecodingResult::U32(v) => v.into_iter().map(|s| sample_to_f32(f64::from(s))).collect(),
            DecodingResult::F16(v) => v.into_iter().map(f32::from).collect(),
            other => {
                return Err(format!(
                    "sample type {} is not a height",
                    match other {
                        DecodingResult::I64(_) => "i64",
                        DecodingResult::U64(_) => "u64",
                        _ => "unknown",
                    }
                ))
            }
        })
    }
}
