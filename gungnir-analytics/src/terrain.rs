// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The placed terrain as a surface a line of sight can afford to read (GAP-247,
//! `docs/design/DN-34-sensor-dead-spots.md` §5 and §6.1).
//!
//! [`crate::TerrainMaskLineOfSight`] takes the nearest vertex's height at every sample of
//! every ray by scanning the whole mesh, which is correct and stays as the reference, and
//! which costs a million distance computations a sample on a 1 000 by 1 000 DEM. Nothing
//! that runs every frame can use it. This module gives the same surface two cheaper reads:
//!
//! - [`MeshSurface`] interpolates the mesh's up across the triangle under a point, found by
//!   a coarse search of the grid. It is the lookup `gungnir-app`'s ground elevation model
//!   held (GAP-199); it lives here now so the analytics and the feeds read one surface.
//! - [`HeightField`] resamples that surface once per load onto a regular ENU raster, after
//!   which every lookup is an index and a bilinear mix, and [`HeightFieldLineOfSight`]
//!   samples a segment against it at the raster's own cell size.
//!
//! **A hole stays a hole.** A cell of the DEM with no height is `NaN` on the mesh, `NaN`
//! on the raster, and no height at all to a lookup: it neither blocks a ray nor reads as
//! sea level.

use crate::LineOfSight;
use gungnir_data::geospatial::TerrainMesh;

/// The most vertices the coarse search scans for each lookup.
pub const COARSE_SAMPLES: usize = 65_536;

/// The most cells a [`HeightField`] holds: sixteen megabytes of `f32`, a 2 000 by 2 000
/// raster. A DEM larger than that is resampled at a coarser cell ([`HeightField::fitting_cell_m`]).
pub const HEIGHT_FIELD_CELL_BUDGET: usize = 4_000_000;

/// Why a surface could not be built from a mesh, or a raster from a surface.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HeightFieldError {
    /// The mesh was not built from a grid, or is narrower than one cell either way.
    #[error("the mesh is not a grid of at least two rows and two columns")]
    NotAGrid,
    /// The cell size asked for is not a length.
    #[error("the cell size {0} is not finite and positive")]
    BadCell(f64),
    /// The raster would hold more cells than [`HEIGHT_FIELD_CELL_BUDGET`].
    #[error("a raster of {cells} cells is over the budget of {budget}")]
    OverBudget { cells: usize, budget: usize },
    /// No vertex of the mesh has a finite position.
    #[error("the mesh has no vertex with a finite position")]
    NoExtent,
}

/// Why a point has no ground height on a [`MeshSurface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMiss {
    /// No triangle lies under the point.
    Off,
    /// The triangle under it touches a cell with no data.
    NoHeight,
}

/// The placed mesh as a surface: the up interpolated across the triangle under a frame
/// east and north (module documentation).
///
/// **Finding the triangle.** The mesh's vertices keep their grid order, and a placed grid
/// is still a grid locally, only bent by its projection. So a coarse subsample, every
/// `stride`-th vertex in each direction with at most [`COARSE_SAMPLES`] of them, finds the
/// nearest grid neighbourhood, and only the cells within a stride of it are searched.
#[derive(Debug, Clone)]
pub struct MeshSurface {
    /// Every vertex's position relative to `origin`, metres, grid order, north row first.
    positions: Vec<[f32; 3]>,
    origin: [f64; 2],
    rows: usize,
    columns: usize,
    stride: usize,
    /// `(row, column, east, north)` of every `stride`-th vertex, and of the last row and
    /// column.
    coarse: Vec<(usize, usize, f64, f64)>,
}

impl MeshSurface {
    /// The surface of `mesh`.
    ///
    /// # Errors
    ///
    /// When the mesh was not built from a grid, or is narrower than one cell either way.
    pub fn new(mesh: &TerrainMesh) -> Result<Self, HeightFieldError> {
        let (rows, columns) = (mesh.rows as usize, mesh.columns as usize);
        if rows < 2 || columns < 2 || mesh.positions.len() != rows * columns {
            return Err(HeightFieldError::NotAGrid);
        }
        let cells = rows * columns;
        let mut stride = 1;
        while cells.div_ceil(stride * stride) > COARSE_SAMPLES {
            stride += 1;
        }
        let along = |len: usize| {
            let mut at: Vec<usize> = (0..len).step_by(stride).collect();
            if at.last() != Some(&(len - 1)) {
                at.push(len - 1);
            }
            at
        };
        let origin = mesh.origin;
        let mut coarse = Vec::new();
        for row in along(rows) {
            for column in along(columns) {
                let p = mesh.positions[row * columns + column];
                coarse.push((
                    row,
                    column,
                    origin[0] + f64::from(p[0]),
                    origin[1] + f64::from(p[1]),
                ));
            }
        }
        Ok(Self {
            positions: mesh.positions.clone(),
            origin,
            rows,
            columns,
            stride,
            coarse,
        })
    }

    /// Vertex `(row, column)` as a frame east, north and up.
    fn vertex(&self, row: usize, column: usize) -> [f64; 3] {
        vertex_of(&self.positions, self.origin, self.columns, row, column)
    }

    /// The mesh's up at a frame east and north, interpolated across the triangle under it.
    ///
    /// # Errors
    ///
    /// [`SurfaceMiss::Off`] when no triangle lies under the point, and
    /// [`SurfaceMiss::NoHeight`] when the one that does touches a cell with no data.
    pub fn up_at(&self, east: f64, north: f64) -> Result<f64, SurfaceMiss> {
        let Some(&(row, column, _, _)) = self.coarse.iter().min_by(|a, b| {
            let da = (a.2 - east).powi(2) + (a.3 - north).powi(2);
            let db = (b.2 - east).powi(2) + (b.3 - north).powi(2);
            da.total_cmp(&db)
        }) else {
            return Err(SurfaceMiss::Off);
        };
        let reach = self.stride + 1;
        let rows = row.saturating_sub(reach)..=(row + reach).min(self.rows - 2);
        for r in rows {
            for c in column.saturating_sub(reach)..=(column + reach).min(self.columns - 2) {
                let (nw, ne, sw, se) = (
                    self.vertex(r, c),
                    self.vertex(r, c + 1),
                    self.vertex(r + 1, c),
                    self.vertex(r + 1, c + 1),
                );
                // The same two triangles per cell `TerrainMesh::from_grid` draws.
                for triangle in [[nw, sw, ne], [ne, sw, se]] {
                    let Some(weights) = barycentric(triangle, east, north) else {
                        continue;
                    };
                    let up: f64 = triangle.iter().zip(weights).map(|(v, w)| v[2] * w).sum();
                    return if up.is_finite() {
                        Ok(up)
                    } else {
                        Err(SurfaceMiss::NoHeight)
                    };
                }
            }
        }
        Err(SurfaceMiss::Off)
    }
}

fn vertex_of(
    positions: &[[f32; 3]],
    origin: [f64; 2],
    columns: usize,
    row: usize,
    column: usize,
) -> [f64; 3] {
    let p = positions[row * columns + column];
    [
        origin[0] + f64::from(p[0]),
        origin[1] + f64::from(p[1]),
        f64::from(p[2]),
    ]
}

/// The weights that place `(east, north)` in `triangle`'s east-north projection, or `None`
/// when it lies outside. A point on an edge is inside, so a point on the line two cells
/// share is not lost between them.
fn barycentric(triangle: [[f64; 3]; 3], east: f64, north: f64) -> Option<[f64; 3]> {
    const EDGE: f64 = -1e-9;
    let [a, b, c] = triangle;
    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if det.abs() < f64::EPSILON || !det.is_finite() {
        return None;
    }
    let wa = ((b[1] - c[1]) * (east - c[0]) + (c[0] - b[0]) * (north - c[1])) / det;
    let wb = ((c[1] - a[1]) * (east - c[0]) + (a[0] - c[0]) * (north - c[1])) / det;
    let wc = 1.0 - wa - wb;
    (wa >= EDGE && wb >= EDGE && wc >= EDGE).then_some([wa, wb, wc])
}

/// The placed terrain resampled onto a regular ENU raster: one up per cell, `NaN` where
/// the mesh has no height, so a lookup costs nothing per vertex (DN-34 §5).
#[derive(Debug, Clone, PartialEq)]
pub struct HeightField {
    /// ENU east and north of the south-west corner, metres.
    pub origin_enu: [f64; 2],
    pub cell_m: f64,
    pub rows: u32,
    pub columns: u32,
    /// `rows * columns` ups, metres, north row first; `NaN` is no data.
    pub up_m: Vec<f32>,
}

impl HeightField {
    /// The east and north extent of `mesh`'s vertices with a finite position:
    /// `[[min_e, min_n], [max_e, max_n]]`.
    fn extent(mesh: &TerrainMesh) -> Option<[[f64; 2]; 2]> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in &mesh.positions {
            let (e, n) = (
                mesh.origin[0] + f64::from(p[0]),
                mesh.origin[1] + f64::from(p[1]),
            );
            if e.is_finite() && n.is_finite() {
                lo = [lo[0].min(e), lo[1].min(n)];
                hi = [hi[0].max(e), hi[1].max(n)];
            }
        }
        (lo[0] < hi[0] && lo[1] < hi[1]).then_some([lo, hi])
    }

    /// The DEM's own spacing in the frame: the median distance between neighbouring
    /// vertices along a row and down a column, whichever is smaller. `None` for a mesh
    /// that is not a grid.
    #[must_use]
    pub fn native_cell_m(mesh: &TerrainMesh) -> Option<f64> {
        let (rows, columns) = (mesh.rows as usize, mesh.columns as usize);
        if rows < 2 || columns < 2 || mesh.positions.len() != rows * columns {
            return None;
        }
        let at = |r: usize, c: usize| vertex_of(&mesh.positions, mesh.origin, columns, r, c);
        let distance =
            |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let median = |mut v: Vec<f64>| {
            v.retain(|d| d.is_finite() && *d > 0.0);
            if v.is_empty() {
                return None;
            }
            v.sort_by(f64::total_cmp);
            Some(v[v.len() / 2])
        };
        // A few hundred pairs spread over the grid say what its spacing is; a placed grid's
        // spacing varies by parts in a thousand across a DEM, which the median ignores.
        let row_step = (rows / 16).max(1);
        let column_step = (columns / 16).max(1);
        let mut along = Vec::new();
        let mut down = Vec::new();
        for r in (0..rows - 1).step_by(row_step) {
            for c in (0..columns - 1).step_by(column_step) {
                along.push(distance(at(r, c), at(r, c + 1)));
                down.push(distance(at(r, c), at(r + 1, c)));
            }
        }
        match (median(along), median(down)) {
            (Some(a), Some(d)) => Some(a.min(d)),
            (one, other) => one.or(other),
        }
    }

    /// The cell `wanted` would be, made coarser if a raster over `mesh`'s extent at that
    /// cell would hold more than `budget` cells. The second value says whether the budget
    /// made it coarser.
    #[must_use]
    pub fn fitting_cell_m(mesh: &TerrainMesh, wanted: f64, budget: usize) -> (f64, bool) {
        let Some([lo, hi]) = Self::extent(mesh) else {
            return (wanted, false);
        };
        fit_cell(hi[0] - lo[0], hi[1] - lo[1], wanted, budget)
    }

    /// Resample a placed mesh at `cell_m`, by the same triangle interpolation
    /// [`MeshSurface::up_at`] does, once per load: each triangle of the mesh writes the
    /// cells whose centres it covers, so the cost is the triangles plus the cells, never
    /// their product.
    ///
    /// # Errors
    ///
    /// When the mesh is not a grid, the cell size is not finite and positive, or the raster
    /// would be over [`HEIGHT_FIELD_CELL_BUDGET`].
    // Cell indices are bounded by the budget, far inside every integer type here.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn resample(mesh: &TerrainMesh, cell_m: f64) -> Result<Self, HeightFieldError> {
        if !(cell_m.is_finite() && cell_m > 0.0) {
            return Err(HeightFieldError::BadCell(cell_m));
        }
        let (rows_in, columns_in) = (mesh.rows as usize, mesh.columns as usize);
        if rows_in < 2 || columns_in < 2 || mesh.positions.len() != rows_in * columns_in {
            return Err(HeightFieldError::NotAGrid);
        }
        let [first, last] = Self::extent(mesh).ok_or(HeightFieldError::NoExtent)?;
        // The raster's first cell is centred on the mesh's first vertex, so at the DEM's
        // own spacing every cell centre is a vertex and the surface is copied, not smoothed
        // a second time between two grids half a cell apart.
        let lo = [first[0] - cell_m / 2.0, first[1] - cell_m / 2.0];
        let count = |span: f64| (((span / cell_m) - 1e-9).ceil().max(0.0) as usize + 1).max(2);
        let columns = count(last[0] - first[0]);
        let rows = count(last[1] - first[1]);
        let mut field = Self::empty(lo, rows, columns, cell_m)?;
        field.paint(mesh)?;
        Ok(field)
    }

    /// A raster over `extent`, `[[min_e, min_n], [max_e, max_n]]`, at `cell_m`, with no
    /// height anywhere: what a staged store's tiles are painted into one by one
    /// ([`Self::paint`]; GAP-227, `docs/design/DN-36-staged-terrain-and-map-tiles.md`
    /// §5.2). Its south-west corner is the extent's, and it reaches at least to the
    /// extent's north-east corner.
    ///
    /// # Errors
    ///
    /// When the cell size is not finite and positive, the extent is not finite with its
    /// maximum past its minimum, or the raster would be over [`HEIGHT_FIELD_CELL_BUDGET`].
    // Cell counts are checked against the budget before they are used.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn blank(extent: [[f64; 2]; 2], cell_m: f64) -> Result<Self, HeightFieldError> {
        if !(cell_m.is_finite() && cell_m > 0.0) {
            return Err(HeightFieldError::BadCell(cell_m));
        }
        let [lo, hi] = extent;
        if ![lo[0], lo[1], hi[0], hi[1]].iter().all(|v| v.is_finite())
            || hi[0] <= lo[0]
            || hi[1] <= lo[1]
        {
            return Err(HeightFieldError::NoExtent);
        }
        let count = |span: f64| {
            let cells = (span / cell_m).ceil();
            if cells > HEIGHT_FIELD_CELL_BUDGET as f64 {
                HEIGHT_FIELD_CELL_BUDGET + 1
            } else {
                (cells as usize).max(2)
            }
        };
        Self::empty(lo, count(hi[1] - lo[1]), count(hi[0] - lo[0]), cell_m)
    }

    /// A raster of `rows` by `columns` cells of `cell_m` from `lo`, all no data.
    // Rows and columns are within the budget, so they fit a `u32`.
    #[allow(clippy::cast_possible_truncation)]
    fn empty(
        lo: [f64; 2],
        rows: usize,
        columns: usize,
        cell_m: f64,
    ) -> Result<Self, HeightFieldError> {
        let cells = rows.saturating_mul(columns);
        if cells > HEIGHT_FIELD_CELL_BUDGET {
            return Err(HeightFieldError::OverBudget {
                cells,
                budget: HEIGHT_FIELD_CELL_BUDGET,
            });
        }
        Ok(Self {
            origin_enu: lo,
            cell_m,
            rows: rows as u32,
            columns: columns as u32,
            up_m: vec![f32::NAN; cells],
        })
    }

    /// Write `mesh`'s surface into the cells whose centres its triangles cover, by the
    /// same triangle interpolation [`MeshSurface::up_at`] does. A finite up wins over a
    /// hole on a shared edge, so a cell is no data only when every triangle over it touches
    /// a hole; a cell no triangle covers keeps what it held. [`Self::resample`] is one
    /// paint into an empty raster over the mesh's own extent.
    ///
    /// # Errors
    ///
    /// When the mesh is not a grid.
    // Cell indices are bounded by the budget, far inside every integer type here.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn paint(&mut self, mesh: &TerrainMesh) -> Result<(), HeightFieldError> {
        let (rows_in, columns_in) = (mesh.rows as usize, mesh.columns as usize);
        if rows_in < 2 || columns_in < 2 || mesh.positions.len() != rows_in * columns_in {
            return Err(HeightFieldError::NotAGrid);
        }
        let (lo, cell_m) = (self.origin_enu, self.cell_m);
        let (rows, columns) = (self.rows as usize, self.columns as usize);
        let up_m = &mut self.up_m;
        let at = |r: usize, c: usize| vertex_of(&mesh.positions, mesh.origin, columns_in, r, c);
        // Field column and south-first row index whose centre lies at or after `x` along
        // an axis starting at `start`, and the last at or before `y`.
        let span = |start: f64, a: f64, b: f64, len: usize| -> Option<(usize, usize)> {
            let first = ((a - start) / cell_m - 0.5).ceil().max(0.0);
            let last = ((b - start) / cell_m - 0.5).floor().min(len as f64 - 1.0);
            (first.is_finite() && last.is_finite() && first <= last)
                .then_some((first as usize, last as usize))
        };
        for r in 0..rows_in - 1 {
            for c in 0..columns_in - 1 {
                let (nw, ne, sw, se) = (at(r, c), at(r, c + 1), at(r + 1, c), at(r + 1, c + 1));
                for triangle in [[nw, sw, ne], [ne, sw, se]] {
                    let min_e = triangle.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min);
                    let max_e = triangle
                        .iter()
                        .map(|v| v[0])
                        .fold(f64::NEG_INFINITY, f64::max);
                    let min_n = triangle.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
                    let max_n = triangle
                        .iter()
                        .map(|v| v[1])
                        .fold(f64::NEG_INFINITY, f64::max);
                    let (Some((c0, c1)), Some((s0, s1))) = (
                        span(lo[0], min_e, max_e, columns),
                        span(lo[1], min_n, max_n, rows),
                    ) else {
                        continue;
                    };
                    for s in s0..=s1 {
                        let north = lo[1] + (s as f64 + 0.5) * cell_m;
                        for column in c0..=c1 {
                            let east = lo[0] + (column as f64 + 0.5) * cell_m;
                            let Some(w) = barycentric(triangle, east, north) else {
                                continue;
                            };
                            let up: f64 = triangle.iter().zip(w).map(|(v, w)| v[2] * w).sum();
                            // A finite up wins over a hole on a shared edge, so a cell is
                            // no data only when every triangle over it touches a hole.
                            if up.is_finite() {
                                up_m[(rows - 1 - s) * columns + column] = up as f32;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The up of cell `(row, column)`, north row first; `None` off the raster or no data.
    #[must_use]
    pub fn cell_up(&self, row: u32, column: u32) -> Option<f64> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        let up = self.up_m[row as usize * self.columns as usize + column as usize];
        up.is_finite().then(|| f64::from(up))
    }

    /// Bilinear up at an east and north; `None` off the field or touching a no-data cell.
    #[must_use]
    // The indices are floors of values already checked to lie inside the raster.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn up_at(&self, east: f64, north: f64) -> Option<f64> {
        let (columns, rows) = (f64::from(self.columns), f64::from(self.rows));
        // Continuous index on the cell centres, the row counted from the south.
        let x = (east - self.origin_enu[0]) / self.cell_m - 0.5;
        let y = (north - self.origin_enu[1]) / self.cell_m - 0.5;
        if !(x >= -0.5 && x <= columns - 0.5 && y >= -0.5 && y <= rows - 0.5) {
            return None;
        }
        let x = x.clamp(0.0, columns - 1.0);
        let y = y.clamp(0.0, rows - 1.0);
        let c0 = (x.floor() as u32).min(self.columns - 2);
        let s0 = (y.floor() as u32).min(self.rows - 2);
        let (tx, ty) = (x - f64::from(c0), y - f64::from(s0));
        let row_of = |s: u32| self.rows - 1 - s;
        let sw = self.cell_up(row_of(s0), c0)?;
        let se = self.cell_up(row_of(s0), c0 + 1)?;
        let nw = self.cell_up(row_of(s0 + 1), c0)?;
        let ne = self.cell_up(row_of(s0 + 1), c0 + 1)?;
        let south = sw + (se - sw) * tx;
        let north = nw + (ne - nw) * tx;
        Some(south + (north - south) * ty)
    }

    /// The steepest slope between neighbouring cells, rise over run: a bound on how much
    /// the ground can differ between two points a given distance apart, which is what the
    /// shadow sweep's error bound is made of. Zero for a field with no two neighbours.
    #[must_use]
    pub fn max_gradient(&self) -> f64 {
        let (rows, columns) = (self.rows as usize, self.columns as usize);
        let mut steepest = 0.0_f64;
        for r in 0..rows {
            for c in 0..columns {
                let here = self.up_m[r * columns + c];
                for there in [
                    (c + 1 < columns).then(|| self.up_m[r * columns + c + 1]),
                    (r + 1 < rows).then(|| self.up_m[(r + 1) * columns + c]),
                ]
                .into_iter()
                .flatten()
                {
                    let rise = f64::from((there - here).abs());
                    if rise.is_finite() {
                        steepest = steepest.max(rise);
                    }
                }
            }
        }
        steepest / self.cell_m
    }

    /// The ENU east and north of the raster's far, north-east corner.
    #[must_use]
    pub fn far_corner(&self) -> [f64; 2] {
        [
            self.origin_enu[0] + f64::from(self.columns) * self.cell_m,
            self.origin_enu[1] + f64::from(self.rows) * self.cell_m,
        ]
    }
}

/// The cell `wanted` would be over an `east_m` by `north_m` extent, made coarser if the
/// raster would hold more than `budget` cells; and whether it was.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn fit_cell(east_m: f64, north_m: f64, wanted: f64, budget: usize) -> (f64, bool) {
    let area = (east_m.max(0.0)) * (north_m.max(0.0));
    let budget = budget.max(1) as f64;
    if !(wanted.is_finite() && wanted > 0.0) || area <= 0.0 {
        return (wanted, false);
    }
    if area / (wanted * wanted) <= budget {
        return (wanted, false);
    }
    // One per cent over the square root, so the ceilings of both sides still fit.
    ((area / budget).sqrt() * 1.01, true)
}

/// Where a ray across a [`HeightField`] crosses a line through cell centres: the places
/// the bilinear surface can bend, in order of distance, up to a reach.
#[derive(Debug, Clone)]
pub struct Crossings {
    /// The next crossing's distance along each axis's family of lines, and the distance
    /// between successive crossings of that family.
    next: [f64; 2],
    step: [f64; 2],
    reach: f64,
}

impl Iterator for Crossings {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        let axis = usize::from(self.next[1] < self.next[0]);
        let d = self.next[axis];
        // Infinite for an axis the ray never crosses, so the reach ends it.
        if d.is_nan() || d > self.reach {
            return None;
        }
        self.next[axis] += self.step[axis];
        Some(d)
    }
}

impl HeightField {
    /// The distances along the ray from `start` in the unit direction `direction` at which
    /// it crosses a line of cell centres, in order, up to `reach`, excluding the start.
    #[must_use]
    pub fn crossings(&self, start: [f64; 2], direction: [f64; 2], reach: f64) -> Crossings {
        let axis = |start: f64, dir: f64, origin: f64| -> (f64, f64) {
            if dir.abs() < 1e-12 {
                return (f64::INFINITY, f64::INFINITY);
            }
            // The continuous index of `start` on the centre lines, and the first line
            // strictly ahead of it.
            let at = (start - origin) / self.cell_m - 0.5;
            let k = if dir > 0.0 {
                at.floor() + 1.0
            } else {
                at.ceil() - 1.0
            };
            let line = origin + (k + 0.5) * self.cell_m;
            ((line - start) / dir, self.cell_m / dir.abs())
        };
        let (e0, de) = axis(start[0], direction[0], self.origin_enu[0]);
        let (n0, dn) = axis(start[1], direction[1], self.origin_enu[1]);
        Crossings {
            next: [e0, n0],
            step: [de, dn],
            reach,
        }
    }
}

/// Line of sight against a height field, **exactly**: the segment is clear when no point of
/// it lies below the bilinear surface. O(length / cell) per ray (DN-34 §5).
///
/// The surface bends only where the segment crosses a line through cell centres, and
/// between two crossings its height along the segment is a parabola, so the test checks
/// every crossing, both ends, and the top of each piece's parabola where it has one inside
/// the piece. It does not sample: a sharp peak sits on a crossing, and a ray stepping at a
/// fixed spacing reads it low (DN-34 amendment 1 item 1, found by a cone).
///
/// The same rule as [`crate::TerrainMaskLineOfSight`], which stays a reference over the
/// mesh: every point, both ends included, must be at or above the ground under it, and a
/// point over no data or off the field is not blocked by ground nobody measured.
#[derive(Debug, Clone, Copy)]
pub struct HeightFieldLineOfSight<'a> {
    pub field: &'a HeightField,
}

impl LineOfSight for HeightFieldLineOfSight<'_> {
    fn visible(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        // How far the ground stands above the segment at parameter `t`; positive is blocked.
        let above = |t: f64| {
            self.field
                .up_at(from[0] + d[0] * t, from[1] + d[1] * t)
                .map(|h| h - (from[2] + d[2] * t))
        };
        let blocked = |t: f64| above(t).is_some_and(|g| g > 0.0);
        if blocked(0.0) || blocked(1.0) {
            return false;
        }
        let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if length <= f64::EPSILON {
            return true;
        }
        let direction = [d[0] / length, d[1] / length];
        let mut previous = 0.0;
        for distance in self
            .field
            .crossings([from[0], from[1]], direction, length)
            .chain(std::iter::once(length))
        {
            let (t0, t1) = (previous / length, distance / length);
            previous = distance;
            if t1 <= t0 {
                continue;
            }
            if blocked(t1) {
                return false;
            }
            // The piece lies in one bilinear patch, so the height above the segment is a
            // quadratic in t; through its two ends and its middle, its top is exact.
            let middle = f64::midpoint(t0, t1);
            match (above(t0), above(middle), above(t1)) {
                (Some(a), Some(m), Some(b)) => {
                    let (c1, c2) = (4.0 * m - 3.0 * a - b, 2.0 * a + 2.0 * b - 4.0 * m);
                    if c2 < 0.0 {
                        let u = -c1 / (2.0 * c2);
                        if u > 0.0 && u < 1.0 && blocked(t0 + u * (t1 - t0)) {
                            return false;
                        }
                    }
                }
                _ => {
                    if blocked(middle) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_data::geospatial::{GridCrs, HeightGrid};

    /// A `rows` by `columns` grid of `cell` m cells in the local frame, with the height
    /// `h(east, north)` at each cell centre and `NaN` where it returns `None`.
    pub(crate) fn grid(
        rows: u32,
        columns: u32,
        cell: f64,
        h: impl Fn(f64, f64) -> Option<f64>,
    ) -> TerrainMesh {
        let mut heights = Vec::new();
        for row in 0..rows {
            for column in 0..columns {
                let east = (f64::from(column) + 0.5) * cell;
                let north = (f64::from(rows - 1 - row) + 0.5) * cell;
                #[allow(clippy::cast_possible_truncation)]
                heights.push(h(east, north).map_or(-9999.0, |v| v as f32));
            }
        }
        TerrainMesh::from_grid(&HeightGrid {
            columns,
            rows,
            origin: [0.0, 0.0],
            cell_size: [cell, cell],
            nodata: Some(-9999.0),
            crs: GridCrs::Unstated,
            vertical: None,
            heights,
        })
        .expect("a small grid")
        .placed()
    }

    /// A plane is a plane through every read: the mesh surface, the resampled raster, and
    /// the raster's bilinear lookup reproduce it to rounding anywhere inside the grid.
    #[test]
    fn a_plane_survives_resampling_exactly() {
        let plane = |e: f64, n: f64| 100.0 + 0.5 * e - 0.25 * n;
        let mesh = grid(21, 31, 30.0, |e, n| Some(plane(e, n)));
        let surface = MeshSurface::new(&mesh).expect("a grid");
        let field = HeightField::resample(&mesh, 30.0).expect("resampled");
        for (e, n) in [(100.0, 100.0), (437.0, 222.5), (800.0, 550.0), (46.0, 31.0)] {
            let s = surface.up_at(e, n).expect("inside");
            assert!((s - plane(e, n)).abs() < 1e-3, "surface {e},{n}: {s}");
            let f = field.up_at(e, n).expect("inside");
            assert!((f - plane(e, n)).abs() < 1e-2, "field {e},{n}: {f}");
        }
    }

    /// GAP-227 (DN-36 §5.2): a surface painted in two overlapping pieces, as a store's
    /// tiles are with their shared apron row, is the surface resampled whole, cell for
    /// cell; and a blank raster reaches past its extent's north-east corner and holds no
    /// height until something is painted.
    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn painting_in_pieces_is_resampling_whole() {
        let ridge = |e: f64, n: f64| Some(50.0 + (e / 90.0).sin() * 30.0 + n * 0.1);
        let whole = grid(20, 25, 30.0, ridge);
        let field = HeightField::resample(&whole, 30.0).expect("resampled");
        let piece = |first_row: u32, rows: u32| {
            let mut mesh = grid(rows, 25, 30.0, |e, n| {
                ridge(e, n + 30.0 * f64::from(20 - rows - first_row))
            });
            for p in &mut mesh.positions {
                p[1] += 30.0 * (20 - rows - first_row) as f32;
            }
            mesh
        };
        let mut painted = HeightField {
            up_m: vec![f32::NAN; field.up_m.len()],
            ..field.clone()
        };
        painted.paint(&piece(0, 11)).expect("north half");
        painted.paint(&piece(10, 10)).expect("south half");
        assert_eq!(painted.up_m.len(), field.up_m.len());
        for (i, (a, b)) in painted.up_m.iter().zip(&field.up_m).enumerate() {
            assert!(
                a.to_bits() == b.to_bits() || (a - b).abs() < 1e-4,
                "cell {i}: {a} painted, {b} resampled"
            );
        }
        let blank = HeightField::blank([[0.0, 0.0], [95.0, 31.0]], 30.0).expect("blank");
        assert_eq!((blank.columns, blank.rows), (4, 2));
        assert!(blank.up_m.iter().all(|h| h.is_nan()));
        assert!(matches!(
            HeightField::blank([[0.0, 0.0], [0.0, 10.0]], 30.0),
            Err(HeightFieldError::NoExtent)
        ));
    }

    /// The DEM's own spacing is read back from the placed vertices, and a cell that would
    /// overflow the budget is made coarser and says so.
    #[test]
    fn the_native_cell_is_the_grid_spacing_and_the_budget_coarsens_it() {
        let mesh = grid(10, 12, 30.0, |_, _| Some(0.0));
        let native = HeightField::native_cell_m(&mesh).expect("a grid");
        assert!((native - 30.0).abs() < 1e-3, "{native}");
        let (kept, clamped) = HeightField::fitting_cell_m(&mesh, 30.0, 1_000);
        assert!(!clamped && (kept - 30.0).abs() < 1e-9);
        let (coarse, clamped) = HeightField::fitting_cell_m(&mesh, 1.0, 1_000);
        assert!(clamped && coarse > 1.0, "{coarse}");
        let field = HeightField::resample(&mesh, coarse).expect("fits");
        assert!(field.up_m.len() <= 1_100, "{}", field.up_m.len());
    }

    /// A hole stays a hole: no height on the raster, no height to a lookup touching it,
    /// and no wall to a ray across it.
    #[test]
    fn a_hole_is_no_data_and_blocks_nothing() {
        let mesh = grid(9, 9, 10.0, |e, n| {
            let in_hole = (e - 45.0).abs() < 1.0 && (n - 45.0).abs() < 1.0;
            (!in_hole).then_some(0.0)
        });
        let field = HeightField::resample(&mesh, 10.0).expect("resampled");
        assert!(field.up_at(45.0, 45.0).is_none(), "the hole");
        assert!(field.up_at(10.0, 10.0).is_some(), "away from it");
        assert!(field.up_at(-500.0, 10.0).is_none(), "off the field");
        let los = HeightFieldLineOfSight { field: &field };
        assert!(los.visible([10.0, 45.0, 1.0], [80.0, 45.0, 1.0]));
    }

    /// The ridge fixture `TerrainMaskLineOfSight`'s own test uses, as a grid: blocked low,
    /// clear above it, and the same answers the reference gives.
    #[test]
    fn a_ridge_masks_the_far_side_as_the_reference_does() {
        let mesh = grid(5, 41, 5.0, |e, _| {
            Some(if (e - 102.5).abs() < 6.0 { 30.0 } else { 0.0 })
        });
        let field = HeightField::resample(&mesh, 5.0).expect("resampled");
        let fast = HeightFieldLineOfSight { field: &field };
        let reference = crate::TerrainMaskLineOfSight {
            terrain: &mesh,
            sample_spacing_m: 5.0,
        };
        for (from, to) in [
            ([5.0, 12.5, 5.0], [200.0, 12.5, 5.0]),
            ([5.0, 12.5, 40.0], [200.0, 12.5, 40.0]),
            ([5.0, 12.5, 5.0], [60.0, 12.5, 5.0]),
        ] {
            assert_eq!(
                fast.visible(from, to),
                reference.visible(from, to),
                "{from:?} to {to:?}"
            );
        }
        assert!(!fast.visible([5.0, 12.5, 5.0], [200.0, 12.5, 5.0]));
        assert!(fast.visible([5.0, 12.5, 40.0], [200.0, 12.5, 40.0]));
    }

    /// Not a grid, a bad cell, or a raster over the budget is refused by name.
    #[test]
    fn what_cannot_be_resampled_is_refused() {
        assert_eq!(
            HeightField::resample(&TerrainMesh::default(), 10.0),
            Err(HeightFieldError::NotAGrid)
        );
        let mesh = grid(4, 4, 10.0, |_, _| Some(0.0));
        assert!(matches!(
            HeightField::resample(&mesh, f64::NAN),
            Err(HeightFieldError::BadCell(_))
        ));
        assert!(matches!(
            HeightField::resample(&mesh, 0.001),
            Err(HeightFieldError::OverBudget { .. })
        ));
        assert!(MeshSurface::new(&TerrainMesh::default()).is_err());
    }
}
