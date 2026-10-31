// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A staged store's tiles on the desktop (GAP-227,
//! `docs/design/DN-36-staged-terrain-and-map-tiles.md` §5.1 and §5.3): the tiles the view
//! wants are read, verified and placed on a worker thread, never the render thread, and
//! kept for the viewport to draw.
//!
//! **Placed off the render thread.** A tile is read and checked against its manifest
//! digest (`gungnir_data::terrain_store::TerrainStore::read_tile`), meshed, and placed
//! through [`crate::terrain::place_tile`], the single DEM's path, on a thread of its own.
//! So is the footprint: the level-0 tile corners, placed once per store so the viewport
//! knows where every tile lies before any is read. The tick only moves results across.
//!
//! **Bounded.** At most [`RESIDENT_TILES`] placed tiles are kept, the least recently
//! wanted dropped first, and at most [`IN_FLIGHT`] are asked for at once, so a fast pan
//! queues no more than that. A refused tile is remembered and not asked for again.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use gungnir_data::geospatial::{GridCrs, TerrainMesh};
use gungnir_data::terrain_store::fill::{self, OneDegree};
use gungnir_data::terrain_store::{TerrainStore, TileKey};
use gungnir_viewport3d::streaming::terrain::{
    FillLayer, PlacedTile, StoreFootprint, TerrainStoreLayer, WantedTile,
};

use crate::state::AppState;
use crate::terrain::StoreDeclaration;

/// The most placed tiles the desktop keeps. A 512-cell tile is about four megabytes
/// placed, so this is about a hundred.
pub const RESIDENT_TILES: usize = 24;

/// The most tiles asked of the worker at once.
pub const IN_FLIGHT: usize = 8;

enum Job {
    Footprint,
    Tile(TileKey),
    /// Open or fetch a one-degree fill tile's sub-store (D-174).
    Fill {
        tile: OneDegree,
        template: String,
        cache_dir: std::path::PathBuf,
    },
    /// Place one tile of a fill sub-store.
    FillTile(OneDegree, TileKey),
    /// Paint line of sight's field over `extent` from the pinned store (DN-36 §5.2).
    Sight {
        extent: [[f64; 2]; 2],
        footprint: Box<StoreFootprint>,
    },
}

/// A fill sub-store opened, with its placed footprint.
type FillOpened = Result<Box<(Arc<TerrainStore>, StoreFootprint)>, String>;

enum Done {
    /// The pinned store's footprint, and whether it is in a geographic frame.
    Footprint(Result<Box<StoreFootprint>, String>, Option<bool>),
    Tile(TileKey, Result<PlacedTile, String>),
    Fill(OneDegree, FillOpened),
    FillTile(OneDegree, TileKey, Result<PlacedTile, String>),
    Sight([[f64; 2]; 2], Result<Box<Sight>, String>),
}

/// Line of sight's field painted from the pinned store, and what went into it.
struct Sight {
    field: gungnir_analytics::HeightField,
    level: u8,
    tiles: usize,
    refused: usize,
    past_edge: bool,
}

/// What a tile needs to be placed, moved onto the worker thread.
struct Placement {
    store: Arc<TerrainStore>,
    declaration: StoreDeclaration,
    origin: Option<[f64; 3]>,
    epoch: f64,
    geoid: crate::geoid::GeoidGrids,
}

impl Placement {
    fn place(&self, mesh: TerrainMesh) -> Result<TerrainMesh, String> {
        crate::terrain::place_tile(
            mesh,
            &self.declaration,
            self.origin,
            self.epoch,
            &self.geoid,
        )
    }

    /// The level-0 tile corners placed into local ENU (DN-36 §5.1). The corners carry no
    /// height, so their vertical system is declared ellipsoidal for this placement only:
    /// a missing height is placed horizontally and stays missing.
    fn footprint(&self, store: &TerrainStore, revision: u64) -> Result<StoreFootprint, String> {
        let index = store.index().clone();
        let (columns, rows, lattice) = StoreFootprint::lattice(&index);
        let origin = lattice.first().copied().unwrap_or([0.0, 0.0]);
        let mesh = TerrainMesh {
            positions: lattice
                .iter()
                .map(|p| {
                    #[allow(clippy::cast_possible_truncation)]
                    let offset = |v: f64, o: f64| (v - o) as f32;
                    [offset(p[0], origin[0]), offset(p[1], origin[1]), f32::NAN]
                })
                .collect(),
            indices: Vec::new(),
            origin,
            crs: GridCrs::Unstated,
            vertical: None,
            rows,
            columns,
        };
        let declaration = StoreDeclaration {
            vertical: Some("ellipsoidal".to_string()),
            ..self.declaration.clone()
        };
        let placed =
            crate::terrain::place_tile(mesh, &declaration, self.origin, self.epoch, &self.geoid)
                .map_err(|reason| format!("its tile corners could not be placed: {reason}"))?;
        let corners: Vec<[f64; 2]> = placed
            .positions
            .iter()
            .map(|p| [f64::from(p[0]), f64::from(p[1])])
            .collect();
        StoreFootprint::new(index, &corners, revision)
    }

    /// A tile read, verified against its manifest entry, meshed and placed, with the
    /// file's own heights in the mesh's order.
    fn placed_from(
        &self,
        store: &TerrainStore,
        key: TileKey,
    ) -> Result<(TerrainMesh, Vec<f32>), String> {
        let grid = store.read_tile(key).map_err(|e| e.reason.to_string())?;
        let mesh = TerrainMesh::from_grid(&grid).map_err(|e| e.to_string())?;
        let heights = mesh.positions.iter().map(|p| p[2]).collect();
        let placed = self
            .place(mesh)
            .map_err(|reason| format!("not placed: {reason}"))?;
        Ok((placed, heights))
    }

    /// A tile of the pinned store, placed.
    fn placed(&self, key: TileKey) -> Result<(TerrainMesh, Vec<f32>), String> {
        self.placed_from(&self.store, key)
    }

    /// Whether the pinned store is in a geographic frame, read from its coarsest tile's own
    /// tags: only such a store can be filled (D-174). `None` when no tile reads.
    fn geographic(&self) -> Option<bool> {
        let index = self.store.index();
        let coarsest = index.levels().checked_sub(1)?;
        let tile = index.tiles().find(|t| t.level == coarsest)?;
        let grid = self.store.read_tile(tile.key()).ok()?;
        Some(matches!(grid.crs, GridCrs::Geographic { .. }))
    }

    /// A one-degree fill tile's sub-store: reopened from the cache against the digest
    /// recorded when it was fetched, or fetched and staged now, and its footprint placed.
    fn fill(
        &self,
        tile: OneDegree,
        template: &str,
        cache_dir: &std::path::Path,
    ) -> Result<(Arc<TerrainStore>, StoreFootprint), String> {
        let store = match fill::open_cached(cache_dir, tile)? {
            Some(store) => store,
            None => {
                fill::fetch_and_stage(template, tile, cache_dir, self.store.index().manifest())?
            }
        };
        let footprint = self.footprint(&store, crate::terrain::next_revision())?;
        Ok((Arc::new(store), footprint))
    }

    fn tile_of(&self, store: &TerrainStore, key: TileKey) -> Result<PlacedTile, String> {
        let (placed, heights) = self.placed_from(store, key)?;
        Ok(PlacedTile {
            rows: placed.rows,
            columns: placed.columns,
            positions: placed.positions,
            heights,
        })
    }

    /// Line of sight's field over `extent` (DN-36 §5.2, amendment 2): from the pinned store
    /// alone, at the finest level whose cells keep the field within
    /// `HEIGHT_FIELD_CELL_BUDGET`, each tile of that level under the extent painted in and
    /// dropped. A refused tile, and the extent past the store's edge, stay no data: flat.
    fn sight(&self, footprint: &StoreFootprint, extent: [[f64; 2]; 2]) -> Result<Sight, String> {
        let index = footprint.index();
        let [[e0, n0], [e1, n1]] = extent;
        let overlaps =
            |q: [[f64; 2]; 2]| q[0][0] < e1 && q[1][0] > e0 && q[0][1] < n1 && q[1][1] > n0;
        let held = |level: u8| -> Vec<TileKey> {
            let [across, down] = index.level_tiles(level).unwrap_or([0, 0]);
            (0..down)
                .flat_map(|row| (0..across).map(move |column| TileKey { level, column, row }))
                .filter(|&k| index.tile(k).is_some() && footprint.tile_box(k).is_some_and(overlaps))
                .collect()
        };
        // A level's cell in the frame: the smallest of its tiles' boxes over their cells.
        let cell_of = |level: u8| {
            held(level)
                .iter()
                .filter_map(|&k| {
                    let q = footprint.tile_box(k)?;
                    let t = index.tile(k)?;
                    Some(
                        ((q[1][0] - q[0][0]) / f64::from(t.columns))
                            .min((q[1][1] - q[0][1]) / f64::from(t.rows)),
                    )
                })
                .fold(f64::INFINITY, f64::min)
        };
        let budget = gungnir_analytics::HEIGHT_FIELD_CELL_BUDGET;
        let fits = |cell: f64| {
            let cells = ((e1 - e0) / cell).ceil() * ((n1 - n0) / cell).ceil();
            #[allow(clippy::cast_precision_loss)]
            let budget = budget as f64;
            cell.is_finite() && cells <= budget
        };
        let chosen = (0..index.levels()).find_map(|level| {
            let cell = cell_of(level);
            fits(cell).then_some((level, cell))
        });
        // No level fits: the coarsest, at a cell made coarser still to fit.
        let (level, cell) = chosen.unwrap_or_else(|| {
            let level = index.levels() - 1;
            let (cell, _) =
                gungnir_analytics::terrain::fit_cell(e1 - e0, n1 - n0, cell_of(level), budget);
            (level, cell)
        });
        if !cell.is_finite() {
            return Err("no staged tile lies under the coverage extent".to_string());
        }
        let mut field =
            gungnir_analytics::HeightField::blank(extent, cell).map_err(|e| e.to_string())?;
        let (mut tiles, mut refused) = (0, 0);
        for key in held(level) {
            match self.placed(key) {
                Ok((mesh, _)) if field.paint(&mesh).is_ok() => tiles += 1,
                _ => refused += 1,
            }
        }
        let [[s0, t0], [s1, t1]] = footprint.extent();
        let past_edge = e0 < s0 || n0 < t0 || e1 > s1 || n1 > t1;
        Ok(Sight {
            field,
            level,
            tiles,
            refused,
            past_edge,
        })
    }
}

/// The ground line of sight has to read (DN-36 §5.2): the union of every declared
/// sensor's range and every approach point, in the deployment's local ENU frame, with a
/// margin of one maximum range. It moves only when the baseline's sensors or approaches
/// do, never with the view.
///
/// # Errors
///
/// Why there is none: no local frame origin, or nothing declared to cover.
pub fn coverage_extent(config: &gungnir_config::ConfigBaseline) -> Result<[[f64; 2]; 2], String> {
    let Some(frame) = config.local_frame() else {
        return Err("this deployment declares no local frame origin (origin)".to_string());
    };
    let enu = |p: &[f64; 3]| {
        frame.to_enu(gungnir_model::Geodetic {
            lat_rad: p[0],
            lon_rad: p[1],
            alt_m: p[2],
        })
    };
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut take = |e: f64, n: f64, reach: f64| {
        lo = [lo[0].min(e - reach), lo[1].min(n - reach)];
        hi = [hi[0].max(e + reach), hi[1].max(n + reach)];
    };
    let mut margin = 0.0_f64;
    for sensor in &config.sensors {
        let p = enu(&sensor.position);
        let range = if sensor.max_range_m.is_finite() {
            sensor.max_range_m.max(0.0)
        } else {
            0.0
        };
        take(p[0], p[1], range);
        margin = margin.max(range);
    }
    for approach in &config.approaches {
        for point in &approach.points {
            let p = enu(point);
            take(p[0], p[1], 0.0);
        }
    }
    if !(lo[0].is_finite() && lo[1].is_finite() && hi[0] > lo[0] && hi[1] > lo[1]) {
        return Err("no sensor with a range or approach is declared for it to cover".to_string());
    }
    Ok([
        [lo[0] - margin, lo[1] - margin],
        [hi[0] + margin, hi[1] + margin],
    ])
}

/// A staged store's tiles as the desktop holds them.
#[derive(Default)]
pub struct TileStreaming {
    worker: Option<(
        crossbeam_channel::Sender<Job>,
        crossbeam_channel::Receiver<Done>,
    )>,
    footprint: Option<StoreFootprint>,
    /// Why the footprint could not be placed, when it could not.
    footprint_failed: Option<String>,
    resident: BTreeMap<TileKey, Arc<PlacedTile>>,
    refused: BTreeMap<TileKey, String>,
    pending: BTreeSet<TileKey>,
    /// When each resident tile was last wanted, in ticks.
    wanted_at: BTreeMap<TileKey, u64>,
    tick: u64,
    generation: u64,
    /// The extent line of sight's field was last asked for over.
    sight_asked: Option<[[f64; 2]; 2]>,
    /// Whether the pinned store is in a geographic frame, once the worker has read it.
    geographic: Option<bool>,
    /// The fill cache (D-160, D-174), empty without a fill source.
    fill: FillCache,
}

/// The fill cache as the desktop holds it: one sub-store per one-degree tile fetched or
/// reopened, never the pinned store (DN-36 amendment 1, D-174).
#[derive(Default)]
struct FillCache {
    stores: Vec<FillStore>,
    /// Every one-degree tile asked for, opened, opening or failed: none is asked twice.
    asked: BTreeSet<OneDegree>,
    opening: Option<OneDegree>,
    failed: BTreeMap<OneDegree, String>,
    /// What the fill cache says, for the map and PN-09; empty to say nothing.
    status: String,
}

impl FillCache {
    /// What the fill cache says: the tile it is fetching, the first that failed, or
    /// nothing.
    fn status_now(&self) -> String {
        match (self.opening, self.failed.iter().next()) {
            (Some(tile), _) => format!(
                "fetching fill {} for ground past the store's edge",
                tile.name()
            ),
            (None, Some((tile, reason))) => {
                let more = self.failed.len() - 1;
                let more = if more == 0 {
                    String::new()
                } else {
                    format!(" (and {more} more)")
                };
                format!("fill {} not fetched: {reason}{more}", tile.name())
            }
            (None, None) => String::new(),
        }
    }
}

/// The next one-degree tile to fetch for a view whose ENU box is `visible`: of those the
/// view touches, the first not wholly inside the pinned store and not asked for already,
/// among the first [`FILL_TILES_PER_VIEW`]. The view is mapped to degrees through the
/// store's placed corners (`StoreFootprint::frame_of`).
fn next_fill_tile(
    footprint: &StoreFootprint,
    visible: [[f64; 2]; 2],
    asked: &BTreeSet<OneDegree>,
) -> Option<OneDegree> {
    let corners = [
        footprint.frame_of(visible[0][0], visible[0][1]),
        footprint.frame_of(visible[1][0], visible[0][1]),
        footprint.frame_of(visible[0][0], visible[1][1]),
        footprint.frame_of(visible[1][0], visible[1][1]),
    ];
    let lon = corners.iter().map(|c| c[0]);
    let lat = corners.iter().map(|c| c[1]);
    let [[x0, y0], [x1, y1]] = footprint.index().extent();
    let inside_store = |t: &OneDegree| {
        let (west, north) = (f64::from(t.west), f64::from(t.north));
        west >= x0 && west + 1.0 <= x1 && north - 1.0 >= y0 && north <= y1
    };
    OneDegree::covering(
        lon.clone().fold(f64::INFINITY, f64::min),
        lat.clone().fold(f64::INFINITY, f64::min),
        lon.fold(f64::NEG_INFINITY, f64::max),
        lat.fold(f64::NEG_INFINITY, f64::max),
    )
    .into_iter()
    .filter(|t| !inside_store(t))
    .take(FILL_TILES_PER_VIEW)
    .find(|t| !asked.contains(t))
}

/// One fill sub-store and its tiles placed so far.
struct FillStore {
    tile: OneDegree,
    footprint: StoreFootprint,
    resident: BTreeMap<TileKey, Arc<PlacedTile>>,
    pending: BTreeSet<TileKey>,
}

/// The most one-degree tiles fetched for one view.
pub const FILL_TILES_PER_VIEW: usize = 4;

impl std::fmt::Debug for TileStreaming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileStreaming")
            .field("running", &self.worker.is_some())
            .field("footprint", &self.footprint.is_some())
            .field("resident", &self.resident.len())
            .field("refused", &self.refused.len())
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl TileStreaming {
    /// The fill sub-stores as the viewport draws them; the store's layer borrows them.
    #[must_use]
    pub fn fill_layers(&self) -> Vec<FillLayer<'_>> {
        self.fill
            .stores
            .iter()
            .map(|s| FillLayer {
                footprint: &s.footprint,
                resident: &s.resident,
            })
            .collect()
    }

    /// The store as the viewport draws it, once its footprint is placed, with `fills`
    /// from [`Self::fill_layers`].
    #[must_use]
    pub fn layer<'a>(&'a self, fills: &'a [FillLayer<'a>]) -> Option<TerrainStoreLayer<'a>> {
        Some(TerrainStoreLayer {
            footprint: self.footprint.as_ref()?,
            resident: &self.resident,
            refused: &self.refused,
            fills,
            fill_status: &self.fill.status,
            generation: self.generation,
        })
    }

    /// Fill tiles placed and held, over every fill sub-store.
    #[must_use]
    pub fn fill_resident(&self) -> usize {
        self.fill.stores.iter().map(|s| s.resident.len()).sum()
    }

    /// The one-degree tiles fetched or reopened, by name.
    #[must_use]
    pub fn fill_tiles(&self) -> Vec<String> {
        self.fill.stores.iter().map(|s| s.tile.name()).collect()
    }

    /// What the fill cache says, empty to say nothing.
    #[must_use]
    pub fn fill_status(&self) -> &str {
        &self.fill.status
    }

    /// Tiles placed and held.
    #[must_use]
    pub fn resident(&self) -> usize {
        self.resident.len()
    }

    /// Tiles asked for and not yet back.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Tiles refused, by key, with why.
    #[must_use]
    pub fn refused(&self) -> &BTreeMap<TileKey, String> {
        &self.refused
    }

    /// Why the store's footprint could not be placed, when it could not.
    #[must_use]
    pub fn footprint_failed(&self) -> Option<&str> {
        self.footprint_failed.as_deref()
    }

    /// Whether the footprint is placed, so the viewport can choose tiles.
    #[must_use]
    pub fn has_footprint(&self) -> bool {
        self.footprint.is_some()
    }

    /// The pinned store's placed footprint, once it is placed: where its edge lies in the
    /// frame, for the UAS ground model ([`crate::ground`]).
    #[must_use]
    pub fn footprint(&self) -> Option<&StoreFootprint> {
        self.footprint.as_ref()
    }
}

/// Start the worker for a store just verified, and ask it for the footprint.
pub fn start(state: &mut AppState, store: Arc<TerrainStore>, declaration: StoreDeclaration) {
    let placement = Placement {
        store,
        declaration,
        origin: state.config.origin,
        epoch: crate::geoid::frame_epoch(&state.config),
        geoid: state.geoid.clone(),
    };
    let revision = crate::terrain::next_revision();
    let (jobs, inbox) = crossbeam_channel::unbounded::<Job>();
    let (outbox, results) = crossbeam_channel::unbounded::<Done>();
    std::thread::spawn(move || {
        // The fill sub-stores this worker has opened, for their tiles.
        let mut fills: BTreeMap<OneDegree, Arc<TerrainStore>> = BTreeMap::new();
        for job in &inbox {
            let done = match job {
                Job::Footprint => Done::Footprint(
                    placement
                        .footprint(&placement.store, revision)
                        .map(Box::new),
                    placement.geographic(),
                ),
                Job::Tile(key) => Done::Tile(key, placement.tile_of(&placement.store, key)),
                Job::Fill {
                    tile,
                    template,
                    cache_dir,
                } => {
                    let opened = placement.fill(tile, &template, &cache_dir);
                    if let Ok((store, _)) = &opened {
                        fills.insert(tile, Arc::clone(store));
                    }
                    Done::Fill(tile, opened.map(Box::new))
                }
                Job::FillTile(tile, key) => Done::FillTile(
                    tile,
                    key,
                    fills
                        .get(&tile)
                        .ok_or_else(|| format!("fill {} is not open", tile.name()))
                        .and_then(|store| placement.tile_of(store, key)),
                ),
                Job::Sight { extent, footprint } => {
                    Done::Sight(extent, placement.sight(&footprint, extent).map(Box::new))
                }
            };
            // The desktop has dropped the store; nothing to report to.
            if outbox.send(done).is_err() {
                break;
            }
        }
    });
    state.terrain_tiles = TileStreaming::default();
    if jobs.send(Job::Footprint).is_ok() {
        state.terrain_tiles.worker = Some((jobs, results));
    } else {
        state.terrain_tiles.footprint_failed = Some("the tile worker did not start".to_string());
    }
}

/// On the tick: take what the worker finished, ask for what the last frame wanted and
/// the desktop does not hold, drop what has not been wanted longest beyond
/// [`RESIDENT_TILES`], and ask for line of sight's field again when the coverage extent
/// moved (DN-36 §5.2).
pub fn poll(state: &mut AppState) {
    let wanted: Vec<WantedTile> = state.viewport.terrain_tiles.wanted().to_vec();
    let fill_wanted = state.viewport.terrain_tiles.fill_wanted().to_vec();
    let past_edge = state
        .viewport
        .terrain_tiles
        .visible()
        .filter(|_| state.viewport.terrain_tiles.report().unstaged);
    let extent = coverage_extent(&state.config);
    let fill_source = state
        .config
        .terrain
        .as_ref()
        .and_then(|t| t.fill_source.clone());
    let cache_dir = std::path::Path::new(&state.config.data_dir).join("terrain-fill");
    let tiles = &mut state.terrain_tiles;
    if tiles.worker.is_none() {
        return;
    }
    tiles.tick += 1;
    let (mut changed, mut sight) = tiles.take_results();
    if let Some(asked) = tiles.ask_for_sight(extent) {
        sight = Some(asked);
    }
    tiles.ask_for_tiles(&wanted);
    changed |= tiles.ask_for_fill(fill_source.as_deref(), &cache_dir, past_edge, &fill_wanted);
    changed |= tiles.evict(&wanted);
    if changed {
        tiles.generation += 1;
    }
    let Some((sight, field)) = sight else {
        return;
    };
    state.height_field = field.map(Arc::new);
    if let crate::terrain::TerrainStatus::StoreVerified { sight: held, .. } = &mut state.terrain {
        if *held != sight {
            *held = sight;
        }
    }
}

/// Line of sight's state, and the field to hold with it: `None` for none.
type SightUpdate = (
    crate::terrain::SightState,
    Option<gungnir_analytics::HeightField>,
);

impl TileStreaming {
    /// Move what the worker finished into place. Returns whether the resident or refused
    /// tiles changed, and line of sight's new state when a field for the extent last asked
    /// for came back.
    fn take_results(&mut self) -> (bool, Option<SightUpdate>) {
        let Some((_, results)) = self.worker.as_ref() else {
            return (false, None);
        };
        let mut changed = false;
        let mut sight = None;
        while let Ok(done) = results.try_recv() {
            match done {
                Done::Footprint(Ok(footprint), geographic) => {
                    self.footprint = Some(*footprint);
                    self.geographic = geographic;
                    changed = true;
                }
                Done::Footprint(Err(reason), _) => self.footprint_failed = Some(reason),
                Done::Fill(tile, opened) => {
                    self.fill.opening = None;
                    match opened {
                        Ok(opened) => {
                            let (_, footprint) = *opened;
                            self.fill.stores.push(FillStore {
                                tile,
                                footprint,
                                resident: BTreeMap::new(),
                                pending: BTreeSet::new(),
                            });
                        }
                        Err(reason) => {
                            self.fill.failed.insert(tile, reason);
                        }
                    }
                    changed = true;
                }
                Done::FillTile(tile, key, placed) => {
                    if let Some(store) = self.fill.stores.iter_mut().find(|s| s.tile == tile) {
                        store.pending.remove(&key);
                        // A fill tile that does not place is left undrawn; the pinned
                        // store's refusals are the ones PN-09 names.
                        if let Ok(placed) = placed {
                            store.resident.insert(key, Arc::new(placed));
                        }
                    }
                    changed = true;
                }
                Done::Tile(key, placed) => {
                    self.pending.remove(&key);
                    match placed {
                        Ok(tile) => {
                            self.resident.insert(key, Arc::new(tile));
                            self.wanted_at.insert(key, self.tick);
                        }
                        Err(reason) => {
                            self.refused.insert(key, reason);
                        }
                    }
                    changed = true;
                }
                Done::Sight(asked, built) if Some(asked) == self.sight_asked => {
                    sight = Some(match built {
                        Ok(built) => {
                            let Sight {
                                field,
                                level,
                                tiles,
                                refused,
                                past_edge,
                            } = *built;
                            let state = crate::terrain::SightState::Built {
                                level,
                                tiles,
                                refused,
                                past_edge,
                            };
                            (state, Some(field))
                        }
                        Err(reason) => (crate::terrain::SightState::NotBuilt(reason), None),
                    });
                }
                // Painted over an extent since moved: the newer one is on its way.
                Done::Sight(..) => {}
            }
        }
        (changed, sight)
    }

    /// Ask for line of sight's field when the footprint is placed and the coverage extent
    /// is not the one last asked for (DN-36 §5.2); say why there is none when there is no
    /// extent.
    fn ask_for_sight(&mut self, extent: Result<[[f64; 2]; 2], String>) -> Option<SightUpdate> {
        let footprint = self.footprint.as_ref()?;
        let (jobs, _) = self.worker.as_ref()?;
        match extent {
            Ok(extent) if self.sight_asked != Some(extent) => {
                let job = Job::Sight {
                    extent,
                    footprint: Box::new(footprint.clone()),
                };
                jobs.send(job).ok()?;
                self.sight_asked = Some(extent);
                Some((crate::terrain::SightState::Building, None))
            }
            Ok(_) => None,
            Err(reason) => {
                self.sight_asked = None;
                Some((crate::terrain::SightState::NotBuilt(reason), None))
            }
        }
    }

    /// Ask the worker for the wanted tiles not held, refused or on their way, at most
    /// [`IN_FLIGHT`] at once, and mark the held ones wanted now.
    fn ask_for_tiles(&mut self, wanted: &[WantedTile]) {
        let Some((jobs, _)) = self.worker.as_ref() else {
            return;
        };
        for w in wanted {
            if self.resident.contains_key(&w.key) {
                self.wanted_at.insert(w.key, self.tick);
                continue;
            }
            if self.refused.contains_key(&w.key) || self.pending.contains(&w.key) {
                continue;
            }
            if self.pending.len() >= IN_FLIGHT {
                break;
            }
            if jobs.send(Job::Tile(w.key)).is_ok() {
                self.pending.insert(w.key);
            }
        }
    }

    /// The fill cache's tick (D-160, D-174): with a fill source, a geographic store, and a
    /// view reaching past the store's edge, open or fetch the one-degree tiles the view
    /// touches outside the store, one at a time and at most [`FILL_TILES_PER_VIEW`] per
    /// view; place the fill tiles the last frame wanted; and say what it is doing. Whether
    /// anything the viewport draws changed.
    fn ask_for_fill(
        &mut self,
        template: Option<&str>,
        cache_dir: &std::path::Path,
        past_edge: Option<[[f64; 2]; 2]>,
        wanted: &[(u64, WantedTile)],
    ) -> bool {
        let before = self.fill.status.clone();
        let (Some(template), Some(footprint), Some((jobs, _))) =
            (template, self.footprint.as_ref(), self.worker.as_ref())
        else {
            self.fill.status.clear();
            return self.fill.status != before;
        };
        if self.geographic == Some(false) {
            self.fill.status = "terrain.fill_source is named, and the pinned store is not in a \
                                geographic frame; only a geographic store is filled (D-174)"
                .to_string();
            return self.fill.status != before;
        }
        if let (Some(visible), None, Some(true)) = (past_edge, self.fill.opening, self.geographic) {
            if let Some(tile) = next_fill_tile(footprint, visible, &self.fill.asked) {
                let job = Job::Fill {
                    tile,
                    template: template.to_string(),
                    cache_dir: cache_dir.to_path_buf(),
                };
                if jobs.send(job).is_ok() {
                    self.fill.asked.insert(tile);
                    self.fill.opening = Some(tile);
                }
            }
        }
        let mut in_flight = self.pending.len()
            + self
                .fill
                .stores
                .iter()
                .map(|s| s.pending.len())
                .sum::<usize>();
        for (revision, w) in wanted {
            if in_flight >= IN_FLIGHT {
                break;
            }
            let Some(store) = self
                .fill
                .stores
                .iter_mut()
                .find(|s| s.footprint.revision() == *revision)
            else {
                continue;
            };
            if store.resident.contains_key(&w.key) || store.pending.contains(&w.key) {
                continue;
            }
            if jobs.send(Job::FillTile(store.tile, w.key)).is_ok() {
                store.pending.insert(w.key);
                in_flight += 1;
            }
        }
        // Fill tiles the view no longer wants go first once the fill holds too many.
        let total: usize = self.fill.stores.iter().map(|s| s.resident.len()).sum();
        let mut dropped = false;
        if total > RESIDENT_TILES {
            for store in &mut self.fill.stores {
                let revision = store.footprint.revision();
                let before = store.resident.len();
                store
                    .resident
                    .retain(|k, _| wanted.iter().any(|(r, w)| *r == revision && w.key == *k));
                dropped |= store.resident.len() != before;
            }
        }
        self.fill.status = self.fill.status_now();
        dropped || self.fill.status != before
    }

    /// Drop the tiles not wanted longest beyond [`RESIDENT_TILES`]; whether any went.
    fn evict(&mut self, wanted: &[WantedTile]) -> bool {
        let wanted_now: BTreeSet<TileKey> = wanted.iter().map(|w| w.key).collect();
        let mut dropped = false;
        while self.resident.len() > RESIDENT_TILES {
            let Some(oldest) = self
                .wanted_at
                .iter()
                .filter(|(k, _)| !wanted_now.contains(k))
                .min_by_key(|(_, at)| **at)
                .map(|(k, _)| *k)
            else {
                break;
            };
            self.resident.remove(&oldest);
            self.wanted_at.remove(&oldest);
            dropped = true;
        }
        dropped
    }
}
/// One line for PN-09 counting the store's tiles (DN-36 §5.3), or `None` without a store.
#[must_use]
pub fn line(state: &AppState) -> Option<String> {
    use std::fmt::Write as _;
    let tiles = &state.terrain_tiles;
    tiles.worker.as_ref()?;
    if let Some(reason) = tiles.footprint_failed() {
        return Some(format!("terrain tiles not drawn: {reason}"));
    }
    if !tiles.has_footprint() {
        return Some("terrain tiles: placing the store's footprint".to_string());
    }
    let report = state.viewport.terrain_tiles.report();
    let mut line = format!(
        "terrain tiles: pinned {} placed, {} loading, {} refused; {} of {} wanted drawn",
        tiles.resident(),
        tiles.pending(),
        tiles.refused().len(),
        report.drawn.saturating_sub(report.standing_in),
        report.wanted
    );
    // DN-36 amendment 1 item 1: fetched ground is counted apart from the pinned store.
    let fills = tiles.fill_tiles();
    let named = state
        .config
        .terrain
        .as_ref()
        .is_some_and(|t| t.fill_source.is_some());
    if named || !fills.is_empty() {
        let _ = write!(
            line,
            "; fetched {} placed from {} one-degree tile{}{}, outside line of sight",
            tiles.fill_resident(),
            fills.len(),
            if fills.len() == 1 { "" } else { "s" },
            if fills.is_empty() {
                String::new()
            } else {
                format!(" ({})", fills.join(", "))
            }
        );
    }
    if report.says_anything() {
        line.push_str("; ");
        report.write(&mut line);
    }
    Some(line)
}
