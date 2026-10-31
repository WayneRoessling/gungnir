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
use gungnir_data::terrain_store::{TerrainStore, TileKey};
use gungnir_viewport3d::streaming::terrain::{
    PlacedTile, StoreFootprint, TerrainStoreLayer, WantedTile,
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
}

enum Done {
    Footprint(Result<Box<StoreFootprint>, String>),
    Tile(TileKey, Result<PlacedTile, String>),
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
    fn footprint(&self, revision: u64) -> Result<StoreFootprint, String> {
        let index = self.store.index().clone();
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

    fn tile(&self, key: TileKey) -> Result<PlacedTile, String> {
        let grid = self
            .store
            .read_tile(key)
            .map_err(|e| e.reason.to_string())?;
        let mesh = TerrainMesh::from_grid(&grid).map_err(|e| e.to_string())?;
        let heights = mesh.positions.iter().map(|p| p[2]).collect();
        let (rows, columns) = (mesh.rows, mesh.columns);
        let placed = self
            .place(mesh)
            .map_err(|reason| format!("not placed: {reason}"))?;
        Ok(PlacedTile {
            positions: placed.positions,
            heights,
            rows,
            columns,
        })
    }
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
}

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
    /// The store as the viewport draws it, once its footprint is placed.
    #[must_use]
    pub fn layer(&self) -> Option<TerrainStoreLayer<'_>> {
        Some(TerrainStoreLayer {
            footprint: self.footprint.as_ref()?,
            resident: &self.resident,
            refused: &self.refused,
            generation: self.generation,
        })
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
        for job in &inbox {
            let done = match job {
                Job::Footprint => Done::Footprint(placement.footprint(revision).map(Box::new)),
                Job::Tile(key) => Done::Tile(key, placement.tile(key)),
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
/// the desktop does not hold, and drop what has not been wanted longest beyond
/// [`RESIDENT_TILES`].
pub fn poll(state: &mut AppState) {
    let wanted: Vec<WantedTile> = state.viewport.terrain_tiles.wanted().to_vec();
    let tiles = &mut state.terrain_tiles;
    let Some((jobs, results)) = tiles.worker.as_ref() else {
        return;
    };
    tiles.tick += 1;
    let mut changed = false;
    while let Ok(done) = results.try_recv() {
        match done {
            Done::Footprint(Ok(footprint)) => {
                tiles.footprint = Some(*footprint);
                changed = true;
            }
            Done::Footprint(Err(reason)) => {
                tiles.footprint_failed = Some(reason);
            }
            Done::Tile(key, placed) => {
                tiles.pending.remove(&key);
                match placed {
                    Ok(tile) => {
                        tiles.resident.insert(key, Arc::new(tile));
                        tiles.wanted_at.insert(key, tiles.tick);
                    }
                    Err(reason) => {
                        tiles.refused.insert(key, reason);
                    }
                }
                changed = true;
            }
        }
    }
    for w in &wanted {
        if tiles.resident.contains_key(&w.key) {
            tiles.wanted_at.insert(w.key, tiles.tick);
            continue;
        }
        if tiles.refused.contains_key(&w.key) || tiles.pending.contains(&w.key) {
            continue;
        }
        if tiles.pending.len() >= IN_FLIGHT {
            break;
        }
        if jobs.send(Job::Tile(w.key)).is_ok() {
            tiles.pending.insert(w.key);
        }
    }
    while tiles.resident.len() > RESIDENT_TILES {
        let wanted_now: BTreeSet<TileKey> = wanted.iter().map(|w| w.key).collect();
        let Some(oldest) = tiles
            .wanted_at
            .iter()
            .filter(|(k, _)| !wanted_now.contains(k))
            .min_by_key(|(_, at)| **at)
            .map(|(k, _)| *k)
        else {
            break;
        };
        tiles.resident.remove(&oldest);
        tiles.wanted_at.remove(&oldest);
        changed = true;
    }
    if changed {
        tiles.generation += 1;
    }
}

/// One line for PN-09 counting the store's tiles (DN-36 §5.3), or `None` without a store.
#[must_use]
pub fn line(state: &AppState) -> Option<String> {
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
        "terrain tiles: {} placed, {} loading, {} refused; {} of {} wanted drawn",
        tiles.resident(),
        tiles.pending(),
        tiles.refused().len(),
        report.drawn.saturating_sub(report.standing_in),
        report.wanted
    );
    if report.says_anything() {
        line.push_str("; ");
        report.write(&mut line);
    }
    Some(line)
}
