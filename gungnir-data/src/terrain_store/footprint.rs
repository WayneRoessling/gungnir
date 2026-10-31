// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Where a store's tiles lie once placed (GAP-227,
//! `docs/design/DN-36-staged-terrain-and-map-tiles.md` amendment 4 item 1): the level-0
//! tile corners, placed into local ENU by the caller, from which every tile's box at every
//! level is read. Here rather than in `gungnir-viewport3d`, which drew it first, because
//! the node's line of sight reads the same boxes and has no edge to a UI crate (DN-36 §9).

use super::{StoreIndex, TileKey};

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
    /// The store's frame from local ENU, as the affine map through three of the placed
    /// extent's corners: `[x, y] = m * [e, n, 1]`.
    to_frame: [[f64; 3]; 2],
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
        let to_frame = affine_from(
            [
                lattice[0],
                lattice[last_i as usize],
                lattice[(last_j * columns) as usize],
            ],
            [at(0, 0), at(last_i, 0), at(0, last_j)],
        )
        .ok_or_else(|| "the store's placed corners do not span an area".to_string())?;
        Ok(Self {
            index,
            boxes,
            extent,
            to_frame,
            revision,
        })
    }

    /// A local ENU position in the store's own frame, by the affine map through three of
    /// the placed extent's corners. Exact in the local frame; across a few degrees of a
    /// geographic store it is good to the curvature the map leaves out, which is enough to
    /// say which one-degree tiles a view touches (D-174) and nothing finer.
    #[must_use]
    pub fn frame_of(&self, e: f64, n: f64) -> [f64; 2] {
        let [a, b] = self.to_frame;
        [a[0] * e + a[1] * n + a[2], b[0] * e + b[1] * n + b[2]]
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

/// The affine map taking each of `enu` to the matching `frame` point, or `None` when the
/// three ENU points are collinear.
fn affine_from(frame: [[f64; 2]; 3], enu: [[f64; 2]; 3]) -> Option<[[f64; 3]; 2]> {
    let [p0, p1, p2] = enu;
    let (u, v) = (
        [p1[0] - p0[0], p1[1] - p0[1]],
        [p2[0] - p0[0], p2[1] - p0[1]],
    );
    let det = u[0] * v[1] - u[1] * v[0];
    if det.abs() < f64::EPSILON || !det.is_finite() {
        return None;
    }
    let row = |k: usize| {
        let (d1, d2) = (frame[1][k] - frame[0][k], frame[2][k] - frame[0][k]);
        // Solve [d1, d2] = [a, b] . [u, v] for the linear part, then the offset.
        let a = (d1 * v[1] - d2 * u[1]) / det;
        let b = (d2 * u[0] - d1 * v[0]) / det;
        [a, b, frame[0][k] - a * p0[0] - b * p0[1]]
    };
    Some([row(0), row(1)])
}
