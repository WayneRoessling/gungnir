// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Staged terrain tiles in the picture (GAP-227,
//! `docs/design/DN-36-staged-terrain-and-map-tiles.md` §5.1, §5.3 and §6): which tiles a
//! view wants, the surface each is drawn as, and the one surface the two renderers draw.
//!
//! **Tiles by view, under one budget for the frame.** [`tiles_in_view`] is §2.1's
//! screen-space error in its closed form for the straight-down camera: a cell's size on
//! screen is its size in metres over `meters_per_px`. It picks one level and one stride
//! for the frame, so that every tile it wants together keeps within
//! [`TERRAIN_VERTEX_BUDGET`], the single DEM's budget, and so a zoomed-out view never draws
//! more than the one file did. Among the choices that fit, it takes the finest whose drawn
//! cells are still at least [`MIN_CELL_POINTS`] across. It is pure, and tested headless.
//!
//! **One surface, composed.** Each resident tile is built into a [`TerrainSurface`] once
//! per stride and kept ([`SurfaceCache`]). The tiles a frame draws are then joined into
//! one surface with its own revision, which is the surface the projection's mesh and the
//! scene's ground pass already draw (DN-35 §6). It is built again only when the set of
//! tiles drawn changes, never per frame, and it is under the budget, so the scene uploads
//! at most that many vertices on such a frame. DN-36 §5.1 placed one GL mesh per tile in
//! `tile_cache::TileCache`; one upload of the joined surface needs neither the cache nor a
//! draw call per tile (DN-36 amendment 4).
//!
//! **Seams (§6).** Every tile is coloured over the store's own height range, from the
//! file's heights, so a height takes one colour in every tile. A tile is drawn out to its
//! east and south apron, which is the neighbour's first cell, so adjacent tiles at one
//! level share that edge's vertices exactly. At those shared vertices, and at a tile's
//! first row and column, the hillshade's difference is taken toward the north and west,
//! which both tiles hold; elsewhere it is central, one cell each way. So a shared vertex
//! has the same position, colour and shade in both tiles, whatever the stride.
//!
//! **Missing ground (§5.3).** A tile not yet resident is stood in for by its nearest
//! resident ancestor, which then replaces its wanted descendants so nothing is drawn
//! twice; with none, nothing is drawn there. [`TileReport`] says what is missing and why,
//! for the sentence on the map and for PN-09.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use gungnir_data::terrain_store::{StoreIndex, TileKey};

use crate::interaction::TopDownView;
use crate::layers::{TerrainSurface, TerrainSurfaceSlot, TERRAIN_VERTEX_BUDGET};
use crate::theme;

/// The smallest a drawn cell is worth being on screen, in points. Finer than this, a
/// coarser level shows the same ground for fewer vertices.
pub const MIN_CELL_POINTS: f64 = 4.0;

/// How many tile surfaces the viewport keeps. Each is under the frame's budget, so this
/// holds at most a few megabytes.
pub const SURFACE_CACHE_TILES: usize = 128;

/// A composed surface's revision has this bit set, so it never equals a single DEM's
/// revision (`gungnir-app`'s counter starts at one and counts up), and the scene's ground
/// pass never takes one for the other.
const COMPOSED: u64 = 1 << 63;

/// One store tile placed in the picture, as the caller hands it over: the tile's whole
/// grid, apron included, north row first.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedTile {
    /// Local ENU metres; a no-data cell's up is `NaN`.
    pub positions: Vec<[f32; 3]>,
    /// The file's own heights, the same order: what the colour ramp reads, over the
    /// store's range (§6 item 1).
    pub heights: Vec<f32>,
    pub rows: u32,
    pub columns: u32,
}

/// A tile at the stride the frame draws it at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WantedTile {
    pub key: TileKey,
    pub stride: u32,
}

/// Where each of a store's tiles lies in the picture: its manifest's geometry, and the
/// level-0 tile corners placed into local ENU by the caller (through the same placement
/// the tiles take), from which every tile's box at every level is read.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreFootprint {
    index: StoreIndex,
    /// Per level, each tile's ENU box, row-major over the level's tiles.
    boxes: Vec<Vec<[[f64; 2]; 2]>>,
    /// The whole store's ENU box.
    extent: [[f64; 2]; 2],
    revision: u64,
}

impl StoreFootprint {
    /// The level-0 tile corners in the store's own frame, north row first: `columns` by
    /// `rows` points. The caller places them into local ENU and hands them to [`Self::new`].
    #[must_use]
    pub fn lattice(index: &StoreIndex) -> (u32, u32, Vec<[f64; 2]>) {
        let [across, down] = index.level_tiles(0).unwrap_or([0, 0]);
        let size = f64::from(index.tile_cells());
        let [dx, dy] = index.cell_size(0);
        let [[x0, y0], [x1, y1]] = index.extent();
        let mut points = Vec::with_capacity((across as usize + 1) * (down as usize + 1));
        for j in 0..=down {
            for i in 0..=across {
                points.push([
                    (x0 + f64::from(i) * size * dx).min(x1),
                    (y1 - f64::from(j) * size * dy).max(y0),
                ]);
            }
        }
        (across + 1, down + 1, points)
    }

    /// The footprint from `placed`, [`Self::lattice`]'s points in local ENU in the same
    /// order. `revision` names this placement of this store; a new one clears every
    /// surface built from the old.
    ///
    /// # Errors
    ///
    /// When `placed` is not the lattice's length or holds a point that is not finite.
    pub fn new(index: StoreIndex, placed: &[[f64; 2]], revision: u64) -> Result<Self, String> {
        let (columns, rows, lattice) = Self::lattice(&index);
        if placed.len() != lattice.len() {
            return Err(format!(
                "{} placed corners for a lattice of {}",
                placed.len(),
                lattice.len()
            ));
        }
        if placed.iter().flatten().any(|v| !v.is_finite()) {
            return Err("a tile corner did not place to a finite position".to_string());
        }
        let at = |i: u32, j: u32| placed[(j * columns + i) as usize];
        let border = |i0: u32, i1: u32, j0: u32, j1: u32| {
            let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
            let mut take = |p: [f64; 2]| {
                b[0] = [b[0][0].min(p[0]), b[0][1].min(p[1])];
                b[1] = [b[1][0].max(p[0]), b[1][1].max(p[1])];
            };
            for i in i0..=i1 {
                take(at(i, j0));
                take(at(i, j1));
            }
            for j in j0..=j1 {
                take(at(i0, j));
                take(at(i1, j));
            }
            b
        };
        let (last_i, last_j) = (columns - 1, rows - 1);
        let mut boxes = Vec::new();
        for level in 0..index.levels() {
            let [across, down] = index.level_tiles(level).unwrap_or([0, 0]);
            let span = 1_u64 << level;
            let mut level_boxes = Vec::with_capacity(across as usize * down as usize);
            for r in 0..down {
                for c in 0..across {
                    let clamp =
                        |v: u64, last: u32| u32::try_from(v.min(u64::from(last))).unwrap_or(last);
                    let (i0, i1) = (
                        clamp(u64::from(c) * span, last_i),
                        clamp((u64::from(c) + 1) * span, last_i),
                    );
                    let (j0, j1) = (
                        clamp(u64::from(r) * span, last_j),
                        clamp((u64::from(r) + 1) * span, last_j),
                    );
                    level_boxes.push(border(i0, i1, j0, j1));
                }
            }
            boxes.push(level_boxes);
        }
        let extent = border(0, last_i, 0, last_j);
        Ok(Self {
            index,
            boxes,
            extent,
            revision,
        })
    }

    #[must_use]
    pub fn index(&self) -> &StoreIndex {
        &self.index
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The whole store's box in local ENU.
    #[must_use]
    pub fn extent(&self) -> [[f64; 2]; 2] {
        self.extent
    }

    /// A tile's box in local ENU, whether or not the store holds it; `None` outside the
    /// levels and the extent.
    #[must_use]
    pub fn tile_box(&self, key: TileKey) -> Option<[[f64; 2]; 2]> {
        let [across, _] = self.index.level_tiles(key.level)?;
        if key.column >= across {
            return None;
        }
        let i = key.row as usize * across as usize + key.column as usize;
        self.boxes.get(usize::from(key.level))?.get(i).copied()
    }
}

/// The grid indices a tile is drawn at, along one axis of `cells` own cells with `apron`
/// on each side, every `stride`th from the first own cell out to the far apron, which is
/// the neighbour's first cell. Where the stride does not reach the far apron exactly (a
/// tile at the store's edge), the last own cell and the apron are added, so the ground
/// still reaches the edge.
#[must_use]
pub fn kept_indices(cells: u32, stride: u32, apron: u32) -> Vec<u32> {
    let stride = stride.max(1);
    let first = apron;
    let last_own = apron + cells.saturating_sub(1);
    let seam = (apron > 0).then_some(last_own + 1);
    let end = seam.unwrap_or(last_own);
    let mut kept: Vec<u32> = (first..=end).step_by(stride as usize).collect();
    if kept.last() != Some(&end) {
        if kept.last().is_some_and(|&l| l < last_own) {
            kept.push(last_own);
        }
        kept.extend(seam);
    }
    kept
}

/// How many vertices a tile of `columns` by `rows` own cells is drawn with at `stride`.
#[must_use]
pub fn tile_vertices(columns: u32, rows: u32, stride: u32, apron: u32) -> usize {
    kept_indices(columns, stride, apron).len() * kept_indices(rows, stride, apron).len()
}

/// What a view wants of a store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewTiles {
    /// The tiles to draw, at one level and one stride.
    pub wanted: Vec<WantedTile>,
    /// Part of the view has no staged ground: past the store's edge, or where it holds
    /// no tile.
    pub unstaged: bool,
}

/// The tiles to draw for this view (DN-36 §5.1): one level and one stride for the whole
/// frame, the tiles of that level the view touches, together within
/// [`TERRAIN_VERTEX_BUDGET`]. Among the levels and strides that fit, the finest whose drawn
/// cells are at least [`MIN_CELL_POINTS`] across on screen; when none is that coarse, the
/// coarsest that fits. Pure, so the choice is tested headless.
#[must_use]
pub fn tiles_in_view(
    footprint: &StoreFootprint,
    view: &TopDownView,
    rect: egui::Rect,
) -> ViewTiles {
    let a = view.unproject(rect.left_top(), rect);
    let b = view.unproject(rect.right_bottom(), rect);
    let visible = [
        [a[0].min(b[0]), a[1].min(b[1])],
        [a[0].max(b[0]), a[1].max(b[1])],
    ];
    let overlaps = |q: [[f64; 2]; 2]| {
        q[0][0] < visible[1][0]
            && q[1][0] > visible[0][0]
            && q[0][1] < visible[1][1]
            && q[1][1] > visible[0][1]
    };
    let inside = |q: [[f64; 2]; 2]| {
        visible[0][0] >= q[0][0]
            && visible[1][0] <= q[1][0]
            && visible[0][1] >= q[0][1]
            && visible[1][1] <= q[1][1]
    };
    let index = footprint.index();
    let apron = index.apron_cells();
    // (level, stride, drawn cell in points, the tiles held, whether any position is not held)
    let mut choices: Vec<(u8, u32, f64, Vec<TileKey>, bool)> = Vec::new();
    for level in 0..index.levels() {
        let Some([across, down]) = index.level_tiles(level) else {
            continue;
        };
        let mut held = Vec::new();
        let mut touched = 0_usize;
        for row in 0..down {
            for column in 0..across {
                let key = TileKey { level, column, row };
                if footprint.tile_box(key).is_some_and(overlaps) {
                    touched += 1;
                    if index.tile(key).is_some() {
                        held.push(key);
                    }
                }
            }
        }
        if held.is_empty() {
            continue;
        }
        let shape = |key: TileKey| index.tile(key).map_or((0, 0), |t| (t.columns, t.rows));
        let mut stride = 1_u32;
        let fits = loop {
            let vertices: usize = held
                .iter()
                .map(|&k| {
                    let (c, r) = shape(k);
                    tile_vertices(c, r, stride, apron)
                })
                .sum();
            if vertices <= TERRAIN_VERTEX_BUDGET {
                break true;
            }
            if stride >= index.tile_cells() {
                break false;
            }
            stride *= 2;
        };
        if !fits {
            continue;
        }
        let cell_m = held
            .iter()
            .filter_map(|&k| {
                let q = footprint.tile_box(k)?;
                let (c, r) = shape(k);
                Some(
                    ((q[1][0] - q[0][0]) / f64::from(c.max(1)))
                        .min((q[1][1] - q[0][1]) / f64::from(r.max(1))),
                )
            })
            .fold(f64::INFINITY, f64::min);
        let drawn = f64::from(stride) * cell_m / view.meters_per_px;
        let missing = touched > held.len();
        choices.push((level, stride, drawn, held, missing));
    }
    let best = choices
        .iter()
        .filter(|c| c.2 >= MIN_CELL_POINTS)
        .min_by(|x, y| x.2.total_cmp(&y.2).then(y.0.cmp(&x.0)))
        .or_else(|| choices.iter().max_by(|x, y| x.2.total_cmp(&y.2)));
    let Some((_, stride, _, held, missing)) = best else {
        return ViewTiles {
            wanted: Vec::new(),
            unstaged: true,
        };
    };
    ViewTiles {
        wanted: held
            .iter()
            .map(|&key| WantedTile {
                key,
                stride: *stride,
            })
            .collect(),
        unstaged: *missing || !inside(footprint.extent()),
    }
}

/// The surface one tile is drawn as at `stride` (§6): its kept vertices out to the far
/// apron, coloured over the store's `range` from the file's heights, and hillshaded from
/// one-cell differences that two adjacent tiles take alike at their shared edge.
///
/// # Errors
///
/// [`crate::ViewportError::MalformedMesh`] when the grid does not fill its shape or is no
/// larger than its apron, and [`crate::ViewportError::EmptyMesh`] when no drawn cell has
/// four corners with a height.
pub fn tile_surface(
    palette: &theme::Palette,
    tile: &PlacedTile,
    stride: u32,
    apron: u32,
    range: [f32; 2],
) -> Result<TerrainSurface, crate::ViewportError> {
    let (rows, columns) = (tile.rows as usize, tile.columns as usize);
    let cells = rows.saturating_mul(columns);
    if tile.positions.len() != cells
        || tile.heights.len() != cells
        || tile.rows <= 2 * apron
        || tile.columns <= 2 * apron
    {
        return Err(crate::ViewportError::MalformedMesh {
            reason: format!(
                "{} positions and {} heights for a {} by {} tile with {apron} cells of apron",
                tile.positions.len(),
                tile.heights.len(),
                tile.rows,
                tile.columns
            ),
        });
    }
    let own = |n: u32| n - 2 * apron;
    let kept_rows = kept_indices(own(tile.rows), stride, apron);
    let kept_columns = kept_indices(own(tile.columns), stride, apron);
    let [lo, hi] = range;
    let span = if hi > lo { hi - lo } else { 1.0 };
    let at = |r: usize, c: usize| {
        let p = tile.positions[r * columns + c];
        p[2].is_finite().then(|| p.map(f64::from))
    };
    // Along one axis: the indices the difference at `t` is taken between (§6 item 2).
    let pair =
        |t: usize, first: usize, seam: usize, len: usize| -> (Option<usize>, Option<usize>) {
            if t == first || t == seam {
                (t.checked_sub(1), None)
            } else {
                (t.checked_sub(1), (t + 1 < len).then_some(t + 1))
            }
        };
    let difference = |here: [f64; 3], before: Option<[f64; 3]>, after: Option<[f64; 3]>| {
        let (from, to) = match (before, after) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, here),
            (None, Some(b)) => (here, b),
            (None, None) => return None,
        };
        Some([to[0] - from[0], to[1] - from[1], to[2] - from[2]])
    };
    let first = apron as usize;
    let seam_row = first + own(tile.rows) as usize;
    let seam_column = first + own(tile.columns) as usize;
    let width = kept_columns.len();
    let mut positions = Vec::with_capacity(kept_rows.len() * width);
    let mut colors = Vec::with_capacity(kept_rows.len() * width);
    let mut ids: Vec<Option<u32>> = Vec::with_capacity(kept_rows.len() * width);
    for &r in &kept_rows {
        for &c in &kept_columns {
            let (r, c) = (r as usize, c as usize);
            let Some(here) = at(r, c) else {
                ids.push(None);
                continue;
            };
            let (w, e) = pair(c, first, seam_column, columns);
            let (n, s) = pair(r, first, seam_row, rows);
            let across = difference(here, w.and_then(|c| at(r, c)), e.and_then(|c| at(r, c)));
            let down = difference(here, n.and_then(|r| at(r, c)), s.and_then(|r| at(r, c)));
            let (de, dn) = crate::layers::rise(across, down);
            let height = tile.heights[r * columns + c];
            #[allow(clippy::cast_possible_truncation)]
            ids.push(Some(positions.len() as u32));
            positions.push(tile.positions[r * columns + c]);
            colors.push(crate::layers::shaded(
                palette,
                (height - lo) / span,
                crate::layers::hillshade(de, dn),
            ));
        }
    }
    let mut indices = Vec::new();
    for i in 0..kept_rows.len().saturating_sub(1) {
        for j in 0..width.saturating_sub(1) {
            if let (Some(nw), Some(ne), Some(sw), Some(se)) = (
                ids[i * width + j],
                ids[i * width + j + 1],
                ids[(i + 1) * width + j],
                ids[(i + 1) * width + j + 1],
            ) {
                indices.extend_from_slice(&[nw, sw, ne, ne, sw, se]);
            }
        }
    }
    if indices.is_empty() {
        return Err(crate::ViewportError::EmptyMesh);
    }
    Ok(TerrainSurface {
        revision: 0,
        positions,
        colors,
        indices,
    })
}

/// Tile surfaces by tile and stride, kept across frames and dropped least recently drawn
/// first beyond [`SURFACE_CACHE_TILES`]. A tile that could not be built is kept as such, so
/// it is refused once and not once a frame.
#[derive(Debug, Default)]
pub struct SurfaceCache {
    surfaces: BTreeMap<WantedTile, (Option<Arc<TerrainSurface>>, u64)>,
    clock: u64,
}

impl SurfaceCache {
    /// The surface for `wanted`, built from `tile` if it is not held.
    fn get(
        &mut self,
        wanted: WantedTile,
        build: impl FnOnce() -> Option<Arc<TerrainSurface>>,
    ) -> Option<Arc<TerrainSurface>> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self
            .surfaces
            .entry(wanted)
            .or_insert_with(|| (build(), clock));
        entry.1 = clock;
        let surface = entry.0.clone();
        if self.surfaces.len() > SURFACE_CACHE_TILES {
            if let Some(oldest) = self
                .surfaces
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k)
            {
                self.surfaces.remove(&oldest);
            }
        }
        surface
    }

    /// How many surfaces are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }
}

/// Join tile surfaces into one, under `revision`.
#[must_use]
pub fn compose(surfaces: &[Arc<TerrainSurface>], revision: u64) -> TerrainSurface {
    let vertices = surfaces.iter().map(|s| s.positions.len()).sum();
    let mut joined = TerrainSurface {
        revision,
        positions: Vec::with_capacity(vertices),
        colors: Vec::with_capacity(vertices),
        indices: Vec::with_capacity(surfaces.iter().map(|s| s.indices.len()).sum()),
    };
    for surface in surfaces {
        #[allow(clippy::cast_possible_truncation)]
        let offset = joined.positions.len() as u32;
        joined.positions.extend_from_slice(&surface.positions);
        joined.colors.extend_from_slice(&surface.colors);
        joined
            .indices
            .extend(surface.indices.iter().map(|i| i + offset));
    }
    joined
}

/// A staged store as a frame draws it, borrowed from the caller (GAP-227).
#[derive(Debug, Clone, Copy)]
pub struct TerrainStoreLayer<'a> {
    pub footprint: &'a StoreFootprint,
    /// The tiles placed so far.
    pub resident: &'a BTreeMap<TileKey, Arc<PlacedTile>>,
    /// The tiles refused, with why.
    pub refused: &'a BTreeMap<TileKey, String>,
    /// Changes whenever `resident` or `refused` does.
    pub generation: u64,
}

/// What the store's ground is missing this frame, and why (§5.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TileReport {
    /// Tiles the view wants.
    pub wanted: usize,
    /// Tiles drawn, a stand-in counted once.
    pub drawn: usize,
    /// Coarser tiles drawn in place of wanted ones not yet resident.
    pub standing_in: usize,
    /// Wanted tiles not resident and not refused.
    pub loading: usize,
    /// Wanted tiles refused, with why.
    pub refused: Vec<(TileKey, String)>,
    /// Part of the view has no staged ground.
    pub unstaged: bool,
}

impl TileReport {
    /// Whether anything is missing.
    #[must_use]
    pub fn says_anything(&self) -> bool {
        self.unstaged || self.loading > 0 || !self.refused.is_empty()
    }

    /// The sentence the map draws (§5.3): empty when nothing is missing.
    pub fn write(&self, text: &mut String) {
        use std::fmt::Write as _;
        let mut parts: Vec<String> = Vec::new();
        if self.unstaged {
            parts.push("no staged terrain past the store's edge".to_string());
        }
        if let Some((key, reason)) = self.refused.first() {
            let more = self.refused.len() - 1;
            parts.push(if more == 0 {
                format!("tile {key} refused: {reason}")
            } else {
                format!("tile {key} refused: {reason} (and {more} more)")
            });
        }
        if self.loading > 0 {
            let stand = if self.standing_in > 0 {
                ", coarser ground stands in"
            } else {
                ""
            };
            parts.push(format!(
                "{} terrain tile{} loading{stand}",
                self.loading,
                if self.loading == 1 { "" } else { "s" }
            ));
        }
        let _ = write!(text, "{}", parts.join("; "));
    }
}

/// The viewport's staged-terrain state across frames.
#[derive(Debug, Default)]
pub struct TerrainTiles {
    view_key: Option<(TopDownView, egui::Rect, u64)>,
    view_tiles: ViewTiles,
    cache: SurfaceCache,
    footprint_revision: Option<u64>,
    generation: Option<u64>,
    drawn: Vec<WantedTile>,
    composed: u64,
    report: TileReport,
}

impl TerrainTiles {
    /// The tiles the last frame wanted: the caller loads the ones it does not hold.
    #[must_use]
    pub fn wanted(&self) -> &[WantedTile] {
        &self.view_tiles.wanted
    }

    /// What the last frame drew.
    #[must_use]
    pub fn drawn(&self) -> &[WantedTile] {
        &self.drawn
    }

    #[must_use]
    pub fn report(&self) -> &TileReport {
        &self.report
    }

    /// The surfaces held.
    #[must_use]
    pub fn cache(&self) -> &SurfaceCache {
        &self.cache
    }

    /// Bring `slot` up to date for a frame at `view` into `rect` with `layer`: choose the
    /// tiles when the view moved, and join a new surface only when the tiles drawn
    /// changed. Costs a comparison on a frame where neither the view nor the resident
    /// tiles changed. Returns whether it joined.
    pub fn sync(
        &mut self,
        palette: &theme::Palette,
        layer: TerrainStoreLayer<'_>,
        view: &TopDownView,
        rect: egui::Rect,
        slot: &mut Option<TerrainSurfaceSlot>,
    ) -> bool {
        let footprint = layer.footprint;
        if self.footprint_revision != Some(footprint.revision()) {
            *self = Self {
                footprint_revision: Some(footprint.revision()),
                composed: self.composed,
                ..Self::default()
            };
        }
        let key = (*view, rect, footprint.revision());
        let moved = self.view_key != Some(key);
        if moved {
            self.view_tiles = tiles_in_view(footprint, view, rect);
            self.view_key = Some(key);
        }
        if !moved && self.generation == Some(layer.generation) {
            return false;
        }
        self.generation = Some(layer.generation);
        let (drawn, report) = choose_drawn(&self.view_tiles, layer);
        self.report = report;
        let holds_this = slot.as_ref().is_some_and(|s| {
            s.revision & COMPOSED != 0 && s.revision == (COMPOSED | self.composed)
        });
        if drawn == self.drawn && (holds_this || drawn.is_empty() && slot.is_none()) {
            return false;
        }
        let range = footprint.index().height_range_m();
        let apron = footprint.index().apron_cells();
        let surfaces: Vec<Arc<TerrainSurface>> = drawn
            .iter()
            .filter_map(|&w| {
                let tile = layer.resident.get(&w.key)?;
                self.cache.get(w, || {
                    tile_surface(palette, tile, w.stride, apron, range)
                        .ok()
                        .map(Arc::new)
                })
            })
            .collect();
        self.drawn = drawn;
        if surfaces.is_empty() {
            *slot = None;
            return true;
        }
        self.composed = self.composed.wrapping_add(1) & !COMPOSED;
        let revision = COMPOSED | self.composed;
        *slot = Some(TerrainSurfaceSlot {
            revision,
            surface: Ok(Arc::new(compose(&surfaces, revision))),
        });
        true
    }
}

/// The tiles to draw from what the view wants and what is resident: each wanted tile that
/// is resident, or its nearest resident ancestor in place of it and of every wanted tile
/// under that ancestor, so nothing is drawn twice (§5.3).
fn choose_drawn(view: &ViewTiles, layer: TerrainStoreLayer<'_>) -> (Vec<WantedTile>, TileReport) {
    let levels = layer.footprint.index().levels();
    let parent = |k: TileKey| {
        (k.level + 1 < levels).then(|| TileKey {
            level: k.level + 1,
            column: k.column / 2,
            row: k.row / 2,
        })
    };
    let mut report = TileReport {
        wanted: view.wanted.len(),
        unstaged: view.unstaged,
        ..TileReport::default()
    };
    let mut stand_ins: BTreeSet<WantedTile> = BTreeSet::new();
    for w in &view.wanted {
        if layer.resident.contains_key(&w.key) {
            continue;
        }
        if let Some(reason) = layer.refused.get(&w.key) {
            report.refused.push((w.key, reason.clone()));
        } else {
            report.loading += 1;
        }
        let mut up = parent(w.key);
        while let Some(k) = up {
            if layer.resident.contains_key(&k) {
                stand_ins.insert(WantedTile {
                    key: k,
                    stride: w.stride,
                });
                break;
            }
            up = parent(k);
        }
    }
    let covered = |k: TileKey| {
        let mut up = parent(k);
        while let Some(a) = up {
            if stand_ins.iter().any(|s| s.key == a) {
                return true;
            }
            up = parent(a);
        }
        false
    };
    let mut drawn: Vec<WantedTile> = view
        .wanted
        .iter()
        .filter(|w| layer.resident.contains_key(&w.key) && !covered(w.key))
        .copied()
        .collect();
    report.standing_in = stand_ins.len();
    drawn.extend(stand_ins);
    drawn.sort_unstable();
    report.drawn = drawn.len();
    (drawn, report)
}

#[cfg(test)]
// A shared vertex is the same vertex only if its position is bit for bit the same, and the
// test heights are made in `f64` and stored as the `f32` a grid holds.
#[allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;
    use gungnir_data::geospatial::{GridCrs, HeightGrid, TerrainMesh};
    use gungnir_data::terrain_store::{stage, ManifestSource, StageOptions, TerrainStore};

    /// A store staged in a fresh directory from a `columns` by `rows` grid of 10 m cells in
    /// the local frame, heights a gentle ridge, and its footprint (the local frame places a
    /// corner where it stands).
    fn store(
        name: &str,
        columns: u32,
        rows: u32,
        tile_cells: u32,
    ) -> (std::path::PathBuf, TerrainStore, StoreFootprint) {
        let dir =
            std::env::temp_dir().join(format!("gungnir-vp-tiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut heights = Vec::new();
        for r in 0..rows {
            for c in 0..columns {
                heights.push(100.0 + (f64::from(c) * 0.13).sin() as f32 * 20.0 + r as f32 * 0.5);
            }
        }
        let grid = HeightGrid {
            columns,
            rows,
            origin: [0.0, 0.0],
            cell_size: [10.0, 10.0],
            nodata: None,
            crs: GridCrs::Unstated,
            vertical: None,
            heights,
        };
        let options = StageOptions {
            source: ManifestSource {
                url: "synthetic".into(),
                retrieved: "2026-10-30".into(),
                licence: "none".into(),
            },
            staged_by: "viewport tests".into(),
            frame: "local-enu".into(),
            vertical: None,
            nad83_realisation: None,
            tile_cells,
            apron_cells: 1,
            levels: None,
        };
        let (_, digest) = stage(&grid, &dir, &options).expect("staged");
        let opened = TerrainStore::open(&dir, &digest).expect("opens");
        let (_, _, lattice) = StoreFootprint::lattice(opened.index());
        let footprint =
            StoreFootprint::new(opened.index().clone(), &lattice, 1).expect("footprint");
        (dir, opened, footprint)
    }

    fn placed(store: &TerrainStore, key: TileKey) -> PlacedTile {
        let grid = store.read_tile(key).expect("reads");
        let mesh = TerrainMesh::from_grid(&grid).expect("meshed").placed();
        PlacedTile {
            heights: mesh.positions.iter().map(|p| p[2]).collect(),
            positions: mesh.positions,
            rows: mesh.rows,
            columns: mesh.columns,
        }
    }

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))
    }

    #[test]
    fn kept_indices_reach_the_far_apron() {
        assert_eq!(kept_indices(4, 1, 1), vec![1, 2, 3, 4, 5]);
        assert_eq!(kept_indices(8, 4, 1), vec![1, 5, 9]);
        // A tile at the store's edge: the stride misses the apron, so the last own cell
        // and the apron are added.
        assert_eq!(kept_indices(7, 4, 1), vec![1, 5, 7, 8]);
        assert_eq!(kept_indices(5, 4, 1), vec![1, 5, 6]);
        assert_eq!(kept_indices(3, 2, 0), vec![0, 2]);
        assert_eq!(tile_vertices(8, 8, 4, 1), 9);
    }

    /// DN-36 §8.1 row c (a draft, not a gate): across zooms from the store's finest cell
    /// to past its whole extent, the chosen tiles cover the visible part of the store, no
    /// two overlap, and together they keep within the budget.
    #[test]
    fn tiles_in_view_cover_the_view_once_within_the_budget() {
        let (dir, _store, footprint) = store("view", 600, 400, 64);
        let [[x0, y0], [x1, y1]] = footprint.extent();
        let apron = footprint.index().apron_cells();
        let mut levels_seen = BTreeSet::new();
        for mpp in [1.25, 2.5, 5.0, 10.0, 20.0, 40.0, 80.0, 160.0] {
            for centre in [[3000.0, 2000.0], [500.0, 300.0], [5900.0, 3950.0]] {
                let view = TopDownView {
                    center_e: centre[0],
                    center_n: centre[1],
                    meters_per_px: mpp,
                };
                let chosen = tiles_in_view(&footprint, &view, rect());
                assert!(!chosen.wanted.is_empty(), "{mpp} at {centre:?}");
                let level = chosen.wanted[0].key.level;
                levels_seen.insert(level);
                assert!(chosen
                    .wanted
                    .iter()
                    .all(|w| w.key.level == level && w.stride == chosen.wanted[0].stride));
                let vertices: usize = chosen
                    .wanted
                    .iter()
                    .map(|w| {
                        let t = footprint.index().tile(w.key).expect("held");
                        tile_vertices(t.columns, t.rows, w.stride, apron)
                    })
                    .sum();
                assert!(vertices <= TERRAIN_VERTEX_BUDGET, "{vertices} at {mpp}");
                let boxes: Vec<_> = chosen
                    .wanted
                    .iter()
                    .map(|w| footprint.tile_box(w.key).expect("box"))
                    .collect();
                for (i, a) in boxes.iter().enumerate() {
                    for b in &boxes[i + 1..] {
                        let overlap_x = a[1][0].min(b[1][0]) - a[0][0].max(b[0][0]);
                        let overlap_y = a[1][1].min(b[1][1]) - a[0][1].max(b[0][1]);
                        assert!(
                            overlap_x <= 0.0 || overlap_y <= 0.0,
                            "{a:?} and {b:?} overlap"
                        );
                    }
                }
                let lo = view.unproject(rect().left_bottom(), rect());
                let hi = view.unproject(rect().right_top(), rect());
                for i in 0..=20 {
                    for j in 0..=20 {
                        let e = lo[0] + (hi[0] - lo[0]) * f64::from(i) / 20.0;
                        let n = lo[1] + (hi[1] - lo[1]) * f64::from(j) / 20.0;
                        if e <= x0 || e >= x1 || n <= y0 || n >= y1 {
                            continue;
                        }
                        assert!(
                            boxes.iter().any(|b| e >= b[0][0]
                                && e <= b[1][0]
                                && n >= b[0][1]
                                && n <= b[1][1]),
                            "({e}, {n}) is in the store and in view but in no chosen tile at {mpp}"
                        );
                    }
                }
                let past_edge = lo[0] < x0 || lo[1] < y0 || hi[0] > x1 || hi[1] > y1;
                assert_eq!(chosen.unstaged, past_edge, "{mpp} at {centre:?}");
            }
        }
        assert!(
            levels_seen.len() > 1,
            "zooming out moves to coarser levels: {levels_seen:?}"
        );
        let away = TopDownView {
            center_e: -50_000.0,
            center_n: 0.0,
            meters_per_px: 1.0,
        };
        let none = tiles_in_view(&footprint, &away, rect());
        assert!(none.wanted.is_empty() && none.unstaged);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// DN-36 §8.1 row d (a draft): a vertex on the edge two tiles share has the same
    /// position, colour and shade in both, at every stride.
    #[test]
    fn a_shared_edge_is_one_edge_in_both_tiles() {
        let (dir, store, footprint) = store("seam", 64, 40, 16);
        let palette = crate::theme::Palette::day();
        let range = footprint.index().height_range_m();
        let west = placed(
            &store,
            TileKey {
                level: 0,
                column: 0,
                row: 0,
            },
        );
        let east = placed(
            &store,
            TileKey {
                level: 0,
                column: 1,
                row: 0,
            },
        );
        let south = placed(
            &store,
            TileKey {
                level: 0,
                column: 0,
                row: 1,
            },
        );
        for stride in [1, 2, 4, 8] {
            let a = tile_surface(&palette, &west, stride, 1, range).expect("west");
            for (other, side) in [(&east, "east"), (&south, "south")] {
                let b = tile_surface(&palette, other, stride, 1, range).expect("neighbour");
                let mut shared = 0;
                for (p, colour) in a.positions.iter().zip(&a.colors) {
                    if let Some(i) = b.positions.iter().position(|q| q == p) {
                        assert_eq!(b.colors[i], *colour, "{side} at stride {stride}: {p:?}");
                        shared += 1;
                    }
                }
                let expected = 16 / stride as usize + 1;
                assert_eq!(shared, expected, "{side} at stride {stride}");
            }
        }
        // One ramp: a tile is coloured over the store's range, not its own.
        let one = tile_surface(&palette, &west, 1, 1, range).expect("ramp");
        let own = {
            let finite = west.heights.iter().copied().filter(|h| h.is_finite());
            let lo = finite.clone().fold(f32::INFINITY, f32::min);
            let hi = finite.fold(f32::NEG_INFINITY, f32::max);
            tile_surface(&palette, &west, 1, 1, [lo, hi]).expect("own range")
        };
        assert_ne!(
            one.colors, own.colors,
            "the store's range is not the tile's own"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tile not yet resident is stood in for by its parent, which replaces its
    /// siblings; the report says what is loading and what was refused; and the joined
    /// surface is built again only when what is drawn changes.
    #[test]
    fn a_missing_tile_is_stood_in_for_and_said() {
        let (dir, store, footprint) = store("stand-in", 64, 64, 16);
        let palette = crate::theme::Palette::day();
        let view = TopDownView {
            center_e: 320.0,
            center_n: 320.0,
            meters_per_px: 1.0,
        };
        let chosen = tiles_in_view(&footprint, &view, rect());
        assert!(chosen.wanted.iter().all(|w| w.key.level == 0), "{chosen:?}");
        let mut resident: BTreeMap<TileKey, Arc<PlacedTile>> = chosen
            .wanted
            .iter()
            .map(|w| (w.key, Arc::new(placed(&store, w.key))))
            .collect();
        let missing = chosen.wanted[0].key;
        resident.remove(&missing);
        let parent = TileKey {
            level: 1,
            column: missing.column / 2,
            row: missing.row / 2,
        };
        resident.insert(parent, Arc::new(placed(&store, parent)));
        let refused = BTreeMap::new();
        let mut tiles = TerrainTiles::default();
        let mut slot = None;
        let layer = TerrainStoreLayer {
            footprint: &footprint,
            resident: &resident,
            refused: &refused,
            generation: 1,
        };
        assert!(tiles.sync(&palette, layer, &view, rect(), &mut slot));
        let report = tiles.report().clone();
        assert_eq!((report.loading, report.standing_in), (1, 1), "{report:?}");
        assert!(tiles.drawn().iter().any(|w| w.key == parent));
        assert!(
            tiles.drawn().iter().all(|w| w.key == parent
                || w.key.column / 2 != parent.column
                || w.key.row / 2 != parent.row),
            "the parent's children are not drawn under it: {:?}",
            tiles.drawn()
        );
        let mut sentence = String::new();
        report.write(&mut sentence);
        assert!(
            sentence.contains("1 terrain tile loading, coarser ground stands in"),
            "{sentence}"
        );
        let revision = slot.as_ref().map(|s| s.revision);
        assert!(revision.is_some_and(|r| r & COMPOSED != 0));
        // Nothing changed: nothing is joined again.
        assert!(!tiles.sync(&palette, layer, &view, rect(), &mut slot));
        assert_eq!(slot.as_ref().map(|s| s.revision), revision);
        // The tile arrives: the parent steps aside.
        resident.insert(missing, Arc::new(placed(&store, missing)));
        let layer = TerrainStoreLayer {
            footprint: &footprint,
            resident: &resident,
            refused: &refused,
            generation: 2,
        };
        assert!(tiles.sync(&palette, layer, &view, rect(), &mut slot));
        assert!(tiles.drawn().iter().all(|w| w.key.level == 0));
        assert!(!tiles.report().says_anything() || tiles.report().unstaged);
        // A refusal is said by key.
        resident.remove(&missing);
        let refused: BTreeMap<TileKey, String> = [(missing, "digest: ...".to_string())].into();
        let layer = TerrainStoreLayer {
            footprint: &footprint,
            resident: &resident,
            refused: &refused,
            generation: 3,
        };
        tiles.sync(&palette, layer, &view, rect(), &mut slot);
        let mut sentence = String::new();
        tiles.report().write(&mut sentence);
        assert!(
            sentence.contains(&format!("tile {missing} refused: digest")),
            "{sentence}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
