// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The staged terrain store (GAP-227, `docs/design/DN-36-staged-terrain-and-map-tiles.md`
//! §4): a directory holding `manifest.json` and a pyramid of `GeoTIFF` DEM tiles,
//! `<level>/<column>_<row>.tif`, staged before deployment and never written by the
//! desktop or the node (D-160).
//!
//! **Integrity is the point of the format.** The baseline pins the manifest's SHA-256
//! (`terrain.manifest_sha256`), and [`TerrainStore::open`] hashes the manifest's bytes and
//! refuses it by name when they differ; the manifest pins each tile's. A tile is read once
//! into memory, hashed, and decoded from those same bytes ([`TerrainStore::read_tile`]), so
//! what is decoded is what was verified: a file replaced between a check and a read has no
//! gap to fall through. A tile whose digest differs is refused by its key (`tile 1/3_2
//! refused: ...`) and the rest of the store stays usable (§4.2, §5.3).
//!
//! **The geometry is the manifest's, checked rather than trusted.** Level 0 holds the
//! source's own cells; a level-`k` cell is `2^k` level-0 cells on a side (§4.1). Tiles are
//! `tile_cells` on a side, counted from the extent's north-west corner, column east and row
//! south, and a level's last column and row of tiles hold what is left. Each tile's file
//! carries `apron_cells` more on every side, shared with its neighbour, for the hillshade's
//! normals and seams (§6), and not drawn. [`StoreIndex::from_manifest`] derives every
//! tile's shape and bounds from those rules and refuses a manifest whose entries disagree,
//! so a reader that chooses tiles by view ([`StoreIndex`]) works from geometry that holds.
//!
//! Placement is not here: a tile is a [`HeightGrid`] in the source's own CRS and vertical
//! system, which `gungnir-analytics` places through the same path as a single DEM, for the
//! desktop and the node alike. [`StoreFootprint`] holds the placed tile corners.

pub mod cli;
pub mod fill;
mod footprint;

pub use footprint::StoreFootprint;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::geospatial::HeightGrid;

/// The manifest's file name in a store directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The manifest format this reader understands. A store staged in another is refused by
/// name rather than read under rules it was not written to.
pub const FORMAT: u32 = 1;

/// The most levels a store may have: level 23's cell is about 8.4 million level-0 cells
/// on a side, past any extent a deployment stages.
pub const MAX_LEVELS: u8 = 24;

/// A manifest larger than this is refused before it is parsed. A store of 100 000 tiles
/// writes about 20 MB.
const MAX_MANIFEST_BYTES: u64 = 64 << 20;

/// Where a tile sits in the pyramid. Shown as `level/column_row`, the form the store's
/// own file names take and DN-36 §5.3's sentences use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub level: u8,
    pub column: u32,
    pub row: u32,
}

impl TileKey {
    /// The tile's file, relative to the store directory: `<level>/<column>_<row>.tif`.
    /// Built from three numbers, so no manifest can name a path outside the store.
    #[must_use]
    pub fn relative_path(self) -> PathBuf {
        PathBuf::from(self.level.to_string()).join(format!("{}_{}.tif", self.column, self.row))
    }
}

impl std::fmt::Display for TileKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}_{}", self.level, self.column, self.row)
    }
}

/// Where the staged heights came from, as `testdata/dem/SOURCE.md` records it for the
/// committed clip.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSource {
    pub url: String,
    /// The retrieval date, as the staging run recorded it.
    pub retrieved: String,
    pub licence: String,
}

/// One tile's entry in the manifest.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestTile {
    pub level: u8,
    pub column: u32,
    pub row: u32,
    /// The tile's own cells, apron excluded: `tile_cells` except in a level's last column
    /// or row of tiles.
    pub columns: u32,
    pub rows: u32,
    /// `[[min_x, min_y], [max_x, max_y]]` of the tile's own cells, apron excluded, in the
    /// store's frame.
    pub bounds: [[f64; 2]; 2],
    /// The tile file's SHA-256, lowercase hexadecimal.
    pub sha256: String,
}

impl ManifestTile {
    #[must_use]
    pub fn key(&self) -> TileKey {
        TileKey {
            level: self.level,
            column: self.column,
            row: self.row,
        }
    }
}

/// `manifest.json`: everything a tile does not say about itself (§4.2).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// [`FORMAT`].
    pub format: u32,
    /// What staged the store, with its version.
    pub staged_by: String,
    pub source: ManifestSource,
    /// The tiles' horizontal frame, in `terrain.frame`'s grammar (`"epsg:4269"`). Every
    /// tile is in it; the baseline's own declaration has to agree.
    pub frame: String,
    /// The tiles' vertical system in `terrain.vertical`'s grammar, when the source states
    /// one or the staging run was told it.
    #[serde(default)]
    pub vertical: Option<String>,
    /// The NAD83 realisation in `terrain.nad83_realisation`'s grammar, for a NAD83 source.
    #[serde(default)]
    pub nad83_realisation: Option<String>,
    /// A tile's side in cells, apron excluded.
    pub tile_cells: u32,
    /// Cells of apron on every side of every tile file.
    pub apron_cells: u32,
    /// `[dx, dy]` of a level-0 cell, in the frame's units.
    pub cell_size: [f64; 2],
    /// How many levels: `0` to `levels - 1`.
    pub levels: u8,
    /// `[[min_x, min_y], [max_x, max_y]]` of level 0, a whole number of its cells.
    pub extent: [[f64; 2]; 2],
    /// The lowest and highest height over the whole store, so every tile is coloured on
    /// one ramp (§6 item 1).
    pub height_range_m: [f32; 2],
    pub tiles: Vec<ManifestTile>,
}

/// A manifest that held up: its geometry checked, its tiles indexed by key. Pure, so a
/// choice of tiles by view is tested without a file.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreIndex {
    manifest: Manifest,
    /// Level-0 cells across and down the extent.
    cells0: [u64; 2],
    /// Index into `manifest.tiles` by key.
    tiles: BTreeMap<TileKey, usize>,
}

impl StoreIndex {
    /// Check `manifest`'s geometry against the rules in this module's documentation and
    /// index its tiles.
    ///
    /// # Errors
    ///
    /// The first rule the manifest breaks, in words: a format this reader does not know, a
    /// size or extent that is not finite and positive, an extent that is not a whole
    /// number of cells, a height range that is not finite and ordered, or a tile whose
    /// level, position, shape, bounds or digest is not what the rules make it, or that
    /// appears twice.
    pub fn from_manifest(manifest: Manifest) -> Result<StoreIndex, String> {
        if manifest.format != FORMAT {
            return Err(format!(
                "manifest format {} is not {FORMAT}, the one this build reads",
                manifest.format
            ));
        }
        if manifest.tile_cells < 2 || manifest.tile_cells > 4096 {
            return Err(format!(
                "tile_cells {} is not between 2 and 4096",
                manifest.tile_cells
            ));
        }
        if manifest.apron_cells >= manifest.tile_cells {
            return Err(format!(
                "apron_cells {} is not less than tile_cells {}",
                manifest.apron_cells, manifest.tile_cells
            ));
        }
        let [dx, dy] = manifest.cell_size;
        if !(dx.is_finite() && dx > 0.0 && dy.is_finite() && dy > 0.0) {
            return Err(format!("cell_size {dx} by {dy} is not finite and positive"));
        }
        if manifest.levels == 0 || manifest.levels > MAX_LEVELS {
            return Err(format!(
                "levels {} is not between 1 and {MAX_LEVELS}",
                manifest.levels
            ));
        }
        let [[x0, y0], [x1, y1]] = manifest.extent;
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) || x1 <= x0 || y1 <= y0 {
            return Err(format!(
                "extent [{x0}, {y0}] to [{x1}, {y1}] is not finite with its maximum past its \
                 minimum"
            ));
        }
        let cells0 = [
            whole_cells(x1 - x0, dx).ok_or_else(|| {
                format!(
                    "the extent's width {} is not a whole number of {dx} cells",
                    x1 - x0
                )
            })?,
            whole_cells(y1 - y0, dy).ok_or_else(|| {
                format!(
                    "the extent's height {} is not a whole number of {dy} cells",
                    y1 - y0
                )
            })?,
        ];
        let [low, high] = manifest.height_range_m;
        if !(low.is_finite() && high.is_finite() && low <= high) {
            return Err(format!(
                "height_range_m [{low}, {high}] is not finite with its lowest first"
            ));
        }
        let mut index = StoreIndex {
            manifest,
            cells0,
            tiles: BTreeMap::new(),
        };
        for (i, tile) in index.manifest.tiles.iter().enumerate() {
            let key = tile.key();
            let refused = |why: String| format!("tile {key}: {why}");
            if !is_sha256_hex(&tile.sha256) {
                return Err(refused(format!(
                    "sha256 {:?} is not 64 hexadecimal digits",
                    tile.sha256
                )));
            }
            let (columns, rows) = index
                .tile_shape(key)
                .ok_or_else(|| refused("is outside the store's levels and extent".into()))?;
            if (tile.columns, tile.rows) != (columns, rows) {
                return Err(refused(format!(
                    "is {} by {} cells; at its place it is {columns} by {rows}",
                    tile.columns, tile.rows
                )));
            }
            let expected = index
                .tile_bounds(key)
                .ok_or_else(|| refused("is outside the store's levels and extent".into()))?;
            if !bounds_agree(tile.bounds, expected, index.cell_size(key.level)) {
                return Err(refused(format!(
                    "bounds {:?}; at its place they are {expected:?}",
                    tile.bounds
                )));
            }
            if index.tiles.insert(key, i).is_some() {
                return Err(refused("appears twice".into()));
            }
        }
        Ok(index)
    }

    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    #[must_use]
    pub fn levels(&self) -> u8 {
        self.manifest.levels
    }

    #[must_use]
    pub fn tile_cells(&self) -> u32 {
        self.manifest.tile_cells
    }

    #[must_use]
    pub fn apron_cells(&self) -> u32 {
        self.manifest.apron_cells
    }

    /// The store's level-0 extent, `[[min_x, min_y], [max_x, max_y]]`.
    #[must_use]
    pub fn extent(&self) -> [[f64; 2]; 2] {
        self.manifest.extent
    }

    /// The lowest and highest height over the whole store.
    #[must_use]
    pub fn height_range_m(&self) -> [f32; 2] {
        self.manifest.height_range_m
    }

    /// `[dx, dy]` of one of `level`'s cells: `2^level` level-0 cells.
    #[must_use]
    pub fn cell_size(&self, level: u8) -> [f64; 2] {
        let scale = f64::from(1_u32 << level.min(MAX_LEVELS - 1));
        let [dx, dy] = self.manifest.cell_size;
        [dx * scale, dy * scale]
    }

    /// `level`'s cells across and down: level 0's, halved `level` times and rounded up,
    /// since a coarse cell at the edge averages what is left. `None` past the last level.
    #[must_use]
    pub fn level_cells(&self, level: u8) -> Option<[u64; 2]> {
        (level < self.manifest.levels).then(|| {
            let scale = 1_u64 << level;
            [
                self.cells0[0].div_ceil(scale),
                self.cells0[1].div_ceil(scale),
            ]
        })
    }

    /// `level`'s tiles across and down. `None` past the last level.
    #[must_use]
    pub fn level_tiles(&self, level: u8) -> Option<[u32; 2]> {
        let [across, down] = self.level_cells(level)?;
        let size = u64::from(self.manifest.tile_cells);
        Some([
            u32::try_from(across.div_ceil(size)).ok()?,
            u32::try_from(down.div_ceil(size)).ok()?,
        ])
    }

    /// The cells a tile at `key` holds, apron excluded, whether or not the store has it.
    /// `None` outside the levels and the extent.
    #[must_use]
    pub fn tile_shape(&self, key: TileKey) -> Option<(u32, u32)> {
        let [across, down] = self.level_cells(key.level)?;
        let size = u64::from(self.manifest.tile_cells);
        let first = [u64::from(key.column) * size, u64::from(key.row) * size];
        if first[0] >= across || first[1] >= down {
            return None;
        }
        Some((
            u32::try_from((across - first[0]).min(size)).ok()?,
            u32::try_from((down - first[1]).min(size)).ok()?,
        ))
    }

    /// `[[min_x, min_y], [max_x, max_y]]` of a tile at `key`, apron excluded, whether or
    /// not the store has it. `None` outside the levels and the extent. A level's last
    /// tiles may reach past the extent by less than one of that level's cells.
    #[must_use]
    pub fn tile_bounds(&self, key: TileKey) -> Option<[[f64; 2]; 2]> {
        let (columns, rows) = self.tile_shape(key)?;
        let [dx, dy] = self.cell_size(key.level);
        let size = f64::from(self.manifest.tile_cells);
        let [[x0, _], [_, y1]] = self.manifest.extent;
        let west = x0 + f64::from(key.column) * size * dx;
        let north = y1 - f64::from(key.row) * size * dy;
        Some([
            [west, north - f64::from(rows) * dy],
            [west + f64::from(columns) * dx, north],
        ])
    }

    /// The manifest's entry for `key`, or `None` when the store does not hold it.
    #[must_use]
    pub fn tile(&self, key: TileKey) -> Option<&ManifestTile> {
        self.tiles
            .get(&key)
            .and_then(|&i| self.manifest.tiles.get(i))
    }

    /// Every tile the store holds, by key.
    pub fn tiles(&self) -> impl Iterator<Item = &ManifestTile> + '_ {
        self.tiles
            .values()
            .filter_map(|&i| self.manifest.tiles.get(i))
    }

    /// How many tiles the store holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

/// `length / cell` when it is a whole number to within a millionth of a cell.
fn whole_cells(length: f64, cell: f64) -> Option<u64> {
    let cells = length / cell;
    let rounded = cells.round();
    if rounded < 1.0 || (cells - rounded).abs() > 1e-6 || rounded > 2f64.powi(52) {
        return None;
    }
    // Checked above: a whole number between 1 and 2^52.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = rounded as u64;
    Some(whole)
}

/// Two bounds agree when every edge is within a millionth of a cell.
fn bounds_agree(a: [[f64; 2]; 2], b: [[f64; 2]; 2], cell: [f64; 2]) -> bool {
    let close = |p: f64, q: f64, c: f64| (p - q).abs() <= 1e-6 * c;
    close(a[0][0], b[0][0], cell[0])
        && close(a[1][0], b[1][0], cell[0])
        && close(a[0][1], b[0][1], cell[1])
        && close(a[1][1], b[1][1], cell[1])
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Why a store could not be opened. Every variant names the manifest's path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("{} is not there", .path.display())]
    Missing { path: PathBuf },
    #[error("{} could not be read: {reason}", .path.display())]
    Unreadable { path: PathBuf, reason: String },
    #[error(
        "{} is not the pinned manifest: its SHA-256 is {found}, and the baseline pins {expected}",
        .path.display()
    )]
    Digest {
        path: PathBuf,
        found: String,
        expected: String,
    },
    #[error("{} is not a terrain store manifest this build reads: {reason}", .path.display())]
    Invalid { path: PathBuf, reason: String },
}

/// Why one tile was refused. The rest of the store is unaffected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("tile {key} refused: {reason}")]
pub struct TileError {
    pub key: TileKey,
    pub reason: TileRefusal,
}

/// What was wrong with a tile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TileRefusal {
    #[error("the store's manifest does not list it")]
    NotInManifest,
    #[error("{} is not there", .0.display())]
    Missing(PathBuf),
    #[error("{} could not be read: {1}", .0.display())]
    Unreadable(PathBuf, String),
    #[error("digest: {} has SHA-256 {found}, and the manifest pins {expected}", .path.display())]
    Digest {
        path: PathBuf,
        found: String,
        expected: String,
    },
    /// The file is the pinned one and is still not what its entry says: the store was
    /// staged wrong.
    #[error("{0}")]
    NotItsEntry(String),
}

/// A store directory whose manifest matched the baseline's digest and held up.
///
/// The fields are private and the only constructor is [`TerrainStore::open`], so holding
/// one is proof the manifest was verified, as holding a `geoid::GeoidGrid` is for a grid.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainStore {
    dir: PathBuf,
    manifest_sha256: String,
    index: StoreIndex,
}

impl TerrainStore {
    /// Read `dir`'s manifest, accept it only if its SHA-256 is `manifest_sha256`
    /// (hexadecimal, either case), and check it.
    ///
    /// # Errors
    ///
    /// [`StoreError::Missing`] with no manifest, [`StoreError::Unreadable`] when it cannot
    /// be read or is over 64 MiB, [`StoreError::Digest`] when its digest differs, and
    /// [`StoreError::Invalid`] when it is not JSON of this format or its geometry does not
    /// hold ([`StoreIndex::from_manifest`]).
    pub fn open(dir: &Path, manifest_sha256: &str) -> Result<TerrainStore, StoreError> {
        let path = dir.join(MANIFEST_FILE);
        let bytes = read_capped(&path, MAX_MANIFEST_BYTES).map_err(|e| match e {
            ReadError::Missing => StoreError::Missing { path: path.clone() },
            ReadError::Unreadable(reason) => StoreError::Unreadable {
                path: path.clone(),
                reason,
            },
        })?;
        let found =
            crate::geoid::sha256_hex(bytes.as_slice()).map_err(|e| StoreError::Unreadable {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        if !found.eq_ignore_ascii_case(manifest_sha256) {
            return Err(StoreError::Digest {
                path,
                found,
                expected: manifest_sha256.to_ascii_lowercase(),
            });
        }
        let invalid = |reason: String| StoreError::Invalid {
            path: path.clone(),
            reason,
        };
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        let index = StoreIndex::from_manifest(manifest).map_err(invalid)?;
        Ok(TerrainStore {
            dir: dir.to_path_buf(),
            manifest_sha256: found,
            index,
        })
    }

    /// The store directory, as the baseline named it.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The verified manifest's SHA-256, lowercase hexadecimal.
    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    #[must_use]
    pub fn index(&self) -> &StoreIndex {
        &self.index
    }

    /// Read the tile at `key`, verify it against its manifest entry, and decode it from the
    /// bytes that were verified. The grid includes the apron: `columns + 2 * apron` by
    /// `rows + 2 * apron` cells.
    ///
    /// # Errors
    ///
    /// A [`TileError`] naming `key`: not in the manifest, missing, unreadable or larger
    /// than a tile can be, a digest that differs from the manifest's, or a pinned file
    /// whose shape, cells or bounds are not its entry's.
    pub fn read_tile(&self, key: TileKey) -> Result<HeightGrid, TileError> {
        let refused = |reason: TileRefusal| TileError { key, reason };
        let entry = self
            .index
            .tile(key)
            .ok_or_else(|| refused(TileRefusal::NotInManifest))?;
        let path = self.dir.join(key.relative_path());
        let apron = self.index.apron_cells();
        let side = u64::from(self.index.tile_cells() + 2 * apron);
        // Uncompressed 64-bit samples and a generous allowance for the directory: a file
        // larger than this is not a tile, and is refused before it is hashed.
        let cap = side * side * 8 + (1 << 20);
        let bytes = read_capped(&path, cap).map_err(|e| {
            refused(match e {
                ReadError::Missing => TileRefusal::Missing(path.clone()),
                ReadError::Unreadable(reason) => TileRefusal::Unreadable(path.clone(), reason),
            })
        })?;
        let found = crate::geoid::sha256_hex(bytes.as_slice())
            .map_err(|e| refused(TileRefusal::Unreadable(path.clone(), e.to_string())))?;
        if !found.eq_ignore_ascii_case(&entry.sha256) {
            return Err(refused(TileRefusal::Digest {
                path,
                found,
                expected: entry.sha256.to_ascii_lowercase(),
            }));
        }
        let not_its_entry = |why: String| refused(TileRefusal::NotItsEntry(why));
        let grid = crate::geospatial::geotiff::parse(&bytes)
            .map_err(|e| not_its_entry(format!("{} does not decode: {e}", path.display())))?;
        let want = (entry.columns + 2 * apron, entry.rows + 2 * apron);
        if (grid.columns, grid.rows) != want {
            return Err(not_its_entry(format!(
                "the file is {} by {} cells; with its apron the entry is {} by {}",
                grid.columns, grid.rows, want.0, want.1
            )));
        }
        let cell = self.index.cell_size(key.level);
        let close = |p: f64, q: f64| (p - q).abs() <= 1e-6 * q;
        if !(close(grid.cell_size[0], cell[0]) && close(grid.cell_size[1], cell[1])) {
            return Err(not_its_entry(format!(
                "its cells are {} by {}; level {} has {} by {}",
                grid.cell_size[0], grid.cell_size[1], key.level, cell[0], cell[1]
            )));
        }
        let pad = f64::from(apron);
        let [[x0, y0], [x1, y1]] = entry.bounds;
        let expected = [
            [x0 - pad * cell[0], y0 - pad * cell[1]],
            [x1 + pad * cell[0], y1 + pad * cell[1]],
        ];
        if !bounds_agree(grid.bounds(), expected, cell) {
            return Err(not_its_entry(format!(
                "the file covers {:?}; with its apron the entry covers {expected:?}",
                grid.bounds()
            )));
        }
        Ok(grid)
    }
}

/// What a staging run is told that its source grid does not say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageOptions {
    pub source: ManifestSource,
    pub staged_by: String,
    /// [`Manifest::frame`], [`Manifest::vertical`], [`Manifest::nad83_realisation`].
    pub frame: String,
    pub vertical: Option<String>,
    pub nad83_realisation: Option<String>,
    pub tile_cells: u32,
    pub apron_cells: u32,
    /// How many levels; `None` for as many as it takes to reach one tile.
    pub levels: Option<u8>,
}

/// Cut `source` into a store in `dir` (DN-36 §4.1, amendment 1 item 2): level 0 its own
/// cells, each coarser level by averaging, the tiles written as `GeoTIFF` files with their
/// apron, and the manifest last. Returns the manifest and its SHA-256, the digest a
/// baseline pins.
///
/// **A level-`k` cell is the mean of every level-0 cell under it** that has a height,
/// not the mean of four level-`(k-1)` means: each level carries the sum and the count, so
/// a no-data cell beside a block weighs nothing at any level. A cell with none is no data.
/// An apron cell past the extent, and a tile cell past it at a coarse level's edge, is no
/// data too. A tile whose own cells hold no height is not written, and the manifest does
/// not list it: its area is missing ground, as past the store's edge.
///
/// `dir` must not hold a manifest already: a staging run writes a new store and never
/// writes into one a baseline may pin.
///
/// # Errors
///
/// `DataError::Parse` when the options or the source cannot make a store
/// ([`StoreIndex::from_manifest`]'s rules), or the source has no height at all;
/// `DataError::Io` when `dir` holds a manifest or a file cannot be written.
pub fn stage(
    source: &HeightGrid,
    dir: &Path,
    options: &StageOptions,
) -> Result<(Manifest, String), crate::DataError> {
    use crate::DataError;
    let manifest_path = dir.join(MANIFEST_FILE);
    if manifest_path.exists() {
        return Err(DataError::Io(format!(
            "{} already exists; a staging run writes a new store, never into one",
            manifest_path.display()
        )));
    }
    let [low, high] = height_range(source)
        .ok_or_else(|| DataError::Parse("the source holds no height".to_string()))?;
    let mut manifest = Manifest {
        format: FORMAT,
        staged_by: options.staged_by.clone(),
        source: options.source.clone(),
        frame: options.frame.clone(),
        vertical: options.vertical.clone(),
        nad83_realisation: options.nad83_realisation.clone(),
        tile_cells: options.tile_cells,
        apron_cells: options.apron_cells,
        cell_size: source.cell_size,
        levels: MAX_LEVELS,
        extent: source.bounds(),
        height_range_m: [low, high],
        tiles: Vec::new(),
    };
    manifest.levels = if let Some(levels) = options.levels {
        levels
    } else {
        let probe = StoreIndex::from_manifest(manifest.clone()).map_err(DataError::Parse)?;
        (0..MAX_LEVELS)
            .find(|&k| probe.level_tiles(k) == Some([1, 1]))
            .map_or(MAX_LEVELS, |k| k + 1)
    };
    let geometry = StoreIndex::from_manifest(manifest.clone()).map_err(DataError::Parse)?;
    let mut level = Level::from_grid(source);
    for k in 0..manifest.levels {
        if k > 0 {
            level = level.halved();
        }
        let [tiles_across, tiles_down] = geometry
            .level_tiles(k)
            .ok_or_else(|| DataError::Parse(format!("level {k} has no tiles")))?;
        for row in 0..tiles_down {
            for column in 0..tiles_across {
                let key = TileKey {
                    level: k,
                    column,
                    row,
                };
                let Some((mut entry, tile)) = cut_tile(&geometry, &level, key, source) else {
                    continue;
                };
                entry.sha256 = write_tile(dir, key, &tile)?;
                manifest.tiles.push(entry);
            }
        }
    }
    // Checked once more as a reader will check it, so a store this writes is one it reads.
    StoreIndex::from_manifest(manifest.clone()).map_err(DataError::Parse)?;
    let text = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| DataError::Parse(format!("the manifest did not serialise: {e}")))?;
    std::fs::write(&manifest_path, &text)
        .map_err(|e| DataError::Io(format!("{}: {e}", manifest_path.display())))?;
    let digest =
        crate::geoid::sha256_hex(text.as_slice()).map_err(|e| DataError::Io(e.to_string()))?;
    Ok((manifest, digest))
}

/// The lowest and highest height in `grid`, or `None` when it holds none.
fn height_range(grid: &HeightGrid) -> Option<[f32; 2]> {
    let mut range: Option<[f32; 2]> = None;
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            if let Some(h) = grid.height_at(row, column) {
                range = Some(range.map_or([h, h], |[low, high]| [low.min(h), high.max(h)]));
            }
        }
    }
    range
}

/// The tile at `key` cut from `level` with its apron, and its manifest entry with the
/// digest still to fill; `None` when the key is outside the store or the tile's own cells
/// hold no height.
fn cut_tile(
    geometry: &StoreIndex,
    level: &Level,
    key: TileKey,
    source: &HeightGrid,
) -> Option<(ManifestTile, HeightGrid)> {
    let (columns, rows) = geometry.tile_shape(key)?;
    let bounds = geometry.tile_bounds(key)?;
    let apron = geometry.apron_cells();
    let size = i64::from(geometry.tile_cells());
    let first = [
        i64::from(key.column) * size - i64::from(apron),
        i64::from(key.row) * size - i64::from(apron),
    ];
    let shape = [columns + 2 * apron, rows + 2 * apron];
    let (heights, any) = level.window(first, shape, [i64::from(apron); 2], [columns, rows]);
    if !any {
        return None;
    }
    let cell = geometry.cell_size(key.level);
    let tile = HeightGrid {
        columns: shape[0],
        rows: shape[1],
        origin: [
            bounds[0][0] - f64::from(apron) * cell[0],
            bounds[0][1] - f64::from(apron) * cell[1],
        ],
        cell_size: cell,
        nodata: Some(f32::NAN),
        crs: source.crs,
        vertical: source.vertical,
        heights,
    };
    let entry = ManifestTile {
        level: key.level,
        column: key.column,
        row: key.row,
        columns,
        rows,
        bounds,
        sha256: String::new(),
    };
    Some((entry, tile))
}

/// Write `tile` at `key`'s path under `dir` and return the written bytes' SHA-256.
fn write_tile(dir: &Path, key: TileKey, tile: &HeightGrid) -> Result<String, crate::DataError> {
    use crate::DataError;
    let mut bytes = std::io::Cursor::new(Vec::new());
    crate::geospatial::geotiff::write(&mut bytes, tile)?;
    let bytes = bytes.into_inner();
    let path = dir.join(key.relative_path());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DataError::Io(format!("{}: {e}", parent.display())))?;
    }
    std::fs::write(&path, &bytes).map_err(|e| DataError::Io(format!("{}: {e}", path.display())))?;
    crate::geoid::sha256_hex(bytes.as_slice()).map_err(|e| DataError::Io(e.to_string()))
}

/// One level of the pyramid while staging: per cell, the sum of the level-0 heights under
/// it and how many there are, north row first.
struct Level {
    columns: u64,
    rows: u64,
    sum: Vec<f64>,
    count: Vec<u64>,
}

impl Level {
    fn from_grid(grid: &HeightGrid) -> Level {
        let mut sum = Vec::with_capacity(grid.heights.len());
        let mut count = Vec::with_capacity(grid.heights.len());
        for row in 0..grid.rows {
            for column in 0..grid.columns {
                let h = grid.height_at(row, column);
                sum.push(h.map_or(0.0, f64::from));
                count.push(u64::from(h.is_some()));
            }
        }
        Level {
            columns: u64::from(grid.columns),
            rows: u64::from(grid.rows),
            sum,
            count,
        }
    }

    /// The next level: each cell the two-by-two block under it, a block at the east or
    /// south edge holding what is left.
    fn halved(&self) -> Level {
        let columns = self.columns.div_ceil(2);
        let rows = self.rows.div_ceil(2);
        let mut sum = vec![0.0; to_usize(columns * rows)];
        let mut count = vec![0; to_usize(columns * rows)];
        for row in 0..self.rows {
            for column in 0..self.columns {
                let from = to_usize(row * self.columns + column);
                let to = to_usize((row / 2) * columns + column / 2);
                sum[to] += self.sum[from];
                count[to] += self.count[from];
            }
        }
        Level {
            columns,
            rows,
            sum,
            count,
        }
    }

    /// The `shape` cells from `first` (column, row), as heights: the mean where a cell has
    /// any, NaN where it has none or lies outside the level. Also whether any cell of the
    /// tile's own, `own` cells from `inset`, has a height.
    // A mean of `f32` heights fits an `f32`; the `f64` sum exists so a large block does
    // not lose its low digits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn window(
        &self,
        first: [i64; 2],
        shape: [u32; 2],
        inset: [i64; 2],
        own: [u32; 2],
    ) -> (Vec<f32>, bool) {
        let mut heights = Vec::with_capacity(shape[0] as usize * shape[1] as usize);
        let mut any = false;
        for dr in 0..i64::from(shape[1]) {
            for dc in 0..i64::from(shape[0]) {
                let (column, row) = (first[0] + dc, first[1] + dr);
                let inside = column >= 0
                    && row >= 0
                    && column.unsigned_abs() < self.columns
                    && row.unsigned_abs() < self.rows;
                let mean = if inside {
                    let i = to_usize(row.unsigned_abs() * self.columns + column.unsigned_abs());
                    (self.count[i] > 0).then(|| (self.sum[i] / self.count[i] as f64) as f32)
                } else {
                    None
                };
                let own_cell = (inset[0]..inset[0] + i64::from(own[0])).contains(&dc)
                    && (inset[1]..inset[1] + i64::from(own[1])).contains(&dr);
                any |= own_cell && mean.is_some();
                heights.push(mean.unwrap_or(f32::NAN));
            }
        }
        (heights, any)
    }
}

/// A count of cells this process already holds in memory fits a `usize`.
#[allow(clippy::cast_possible_truncation)]
fn to_usize(n: u64) -> usize {
    n as usize
}

enum ReadError {
    Missing,
    Unreadable(String),
}

/// The whole of `path`, or a refusal when it is missing, unreadable, or over `cap` bytes.
fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, ReadError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ReadError::Missing),
        Err(e) => return Err(ReadError::Unreadable(e.to_string())),
    };
    let mut bytes = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ReadError::Unreadable(e.to_string()))?;
    if bytes.len() as u64 > cap {
        return Err(ReadError::Unreadable(format!(
            "it is larger than {cap} bytes, the most this file can be"
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        // A 5 by 3 extent of 10 m cells in 2-cell tiles: level 0 is 3 by 2 tiles, the last
        // column and row a cell wide; level 1 is 3 by 2 cells, so 2 by 1 tiles.
        Manifest {
            format: FORMAT,
            staged_by: "test".into(),
            source: ManifestSource {
                url: "none".into(),
                retrieved: "2026-10-30".into(),
                licence: "test".into(),
            },
            frame: "local-enu".into(),
            vertical: None,
            nad83_realisation: None,
            tile_cells: 2,
            apron_cells: 1,
            cell_size: [10.0, 10.0],
            levels: 2,
            extent: [[1000.0, 2000.0], [1050.0, 2030.0]],
            height_range_m: [0.0, 1.0],
            tiles: Vec::new(),
        }
    }

    fn entry(index: &StoreIndex, key: TileKey) -> ManifestTile {
        let (columns, rows) = index.tile_shape(key).expect("inside");
        ManifestTile {
            level: key.level,
            column: key.column,
            row: key.row,
            columns,
            rows,
            bounds: index.tile_bounds(key).expect("inside"),
            sha256: "0".repeat(64),
        }
    }

    const fn key(level: u8, column: u32, row: u32) -> TileKey {
        TileKey { level, column, row }
    }

    #[test]
    fn the_geometry_follows_the_rules() {
        let index = StoreIndex::from_manifest(manifest()).expect("valid");
        assert_eq!(index.level_cells(0), Some([5, 3]));
        assert_eq!(index.level_cells(1), Some([3, 2]));
        assert_eq!(index.level_cells(2), None);
        assert_eq!(index.level_tiles(0), Some([3, 2]));
        assert_eq!(index.level_tiles(1), Some([2, 1]));
        assert_eq!(index.tile_shape(key(0, 2, 1)), Some((1, 1)));
        assert_eq!(index.tile_shape(key(0, 3, 0)), None);
        assert_eq!(
            index.tile_bounds(key(0, 0, 0)),
            Some([[1000.0, 2010.0], [1020.0, 2030.0]])
        );
        assert_eq!(
            index.tile_bounds(key(0, 2, 1)),
            Some([[1040.0, 2000.0], [1050.0, 2010.0]])
        );
        // Level 1's last column is one 20 m cell, reaching 10 m past the extent.
        assert_eq!(
            index.tile_bounds(key(1, 1, 0)),
            Some([[1040.0, 1990.0], [1060.0, 2030.0]])
        );
        assert_eq!(key(1, 3, 2).to_string(), "1/3_2");
        assert_eq!(
            key(1, 3, 2).relative_path(),
            PathBuf::from("1").join("3_2.tif")
        );
    }

    #[test]
    fn a_tile_that_disagrees_with_its_place_is_refused_by_key() {
        let index = StoreIndex::from_manifest(manifest()).expect("valid");
        let mut m = manifest();
        let mut wrong = entry(&index, key(0, 1, 0));
        wrong.bounds[0][0] += 1.0;
        m.tiles.push(wrong);
        let err = StoreIndex::from_manifest(m).expect_err("bounds moved");
        assert!(
            err.contains("tile 0/1_0") && err.contains("bounds"),
            "{err}"
        );

        let mut m = manifest();
        let mut wrong = entry(&index, key(0, 2, 0));
        wrong.columns = 2;
        m.tiles.push(wrong);
        let err = StoreIndex::from_manifest(m).expect_err("too wide");
        assert!(
            err.contains("tile 0/2_0") && err.contains("1 by 2"),
            "{err}"
        );

        let mut m = manifest();
        m.tiles.push(entry(&index, key(0, 0, 0)));
        m.tiles.push(entry(&index, key(0, 0, 0)));
        let err = StoreIndex::from_manifest(m).expect_err("twice");
        assert!(err.contains("appears twice"), "{err}");

        let mut m = manifest();
        let mut wrong = entry(&index, key(0, 0, 0));
        wrong.level = 2;
        m.tiles.push(wrong);
        let err = StoreIndex::from_manifest(m).expect_err("no level 2");
        assert!(err.contains("outside"), "{err}");

        let mut m = manifest();
        let mut wrong = entry(&index, key(0, 0, 0));
        wrong.sha256 = "not hex".into();
        m.tiles.push(wrong);
        let err = StoreIndex::from_manifest(m).expect_err("digest");
        assert!(err.contains("64 hexadecimal"), "{err}");
    }

    #[test]
    fn a_manifest_that_does_not_hold_is_refused_in_words() {
        type Case = (fn(&mut Manifest), &'static str);
        let cases: [Case; 6] = [
            (|m| m.format = 2, "format 2"),
            (|m| m.tile_cells = 1, "tile_cells"),
            (|m| m.apron_cells = 2, "apron_cells"),
            (|m| m.extent[1][0] = 1055.0, "whole number"),
            (|m| m.levels = 0, "levels"),
            (|m| m.height_range_m = [1.0, 0.0], "height_range_m"),
        ];
        for (break_it, said) in cases {
            let mut m = manifest();
            break_it(&mut m);
            let err = StoreIndex::from_manifest(m).expect_err(said);
            assert!(err.contains(said), "{said}: {err}");
        }
    }
}
