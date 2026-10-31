// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The ground the node's line of sight reads (GAP-227,
//! `docs/design/DN-36-staged-terrain-and-map-tiles.md` §9): the same pinned store the
//! desktop reads, opened and verified against the baseline's `terrain.manifest_sha256`, and
//! line of sight's height field built from it over the coverage extent.
//!
//! **The desktop's path, not a copy of it.** The store is verified by
//! `gungnir_data::terrain_store::TerrainStore::open`, agreed with the baseline by
//! `gungnir_config::TerrainConfig::store_declaration`, its tile corners placed by
//! `gungnir_analytics::placement::place_footprint`, and the field painted by
//! `gungnir_analytics::placement::store_sight`, each tile placed by `place_store_tile`: the
//! functions the desktop's `terrain_tiles` calls. So a desktop and a node over one baseline
//! mask the same ground, and a commander's acceptance of a gap drawn on one is judged
//! against the same answer on the other (GAP-194).
//!
//! **The pinned store alone, over the coverage extent** (DN-36 §5.2, D-164). The node has
//! no view, and it reads no fill tile: `terrain.fill_source` is ground a desktop draws for
//! context, and it never enters line of sight. Where the extent reaches past the store, or
//! over a tile refused by its digest, line of sight is flat, and the line says so.
//!
//! **What it reads is said, and so is why it reads nothing.** [`NodeTerrain::line`] goes on
//! the node's health line, as PN-09's terrain line does on the desktop: no terrain named,
//! one DEM named (which only the desktop reads), a store refused, a field still building,
//! one not built and why, or the level and tiles it was built from. The coverage answer
//! carries `terrain_masking_applied` only while a field is held.
//!
//! **Off the loop.** The footprint and the field are made on a thread of their own, never
//! on the node's tick, and the field is asked for again only when the coverage extent moves.
//! The thread is joined when the [`NodeTerrain`] is dropped, because a placement through
//! PROJ still running into the process's exit is the abort GAP-260 found.
//!
//! **A real-world CRS needs the `crs` feature**, which turns on `gungnir-analytics`' and
//! `gungnir-data`'s own: a store in a projected frame is refused by name without it. A store
//! in WGS84 geographic degrees with ellipsoidal heights needs no PROJ and is placed by any
//! build. A height in a geoid datum is made ellipsoidal through the pinned grid, found where
//! the desktop finds it (`geoid_grid_dir`, else `PROJ_DATA`) and verified against its digest
//! the first time a tile needs it.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;

use gungnir_analytics::placement::{self, Declared, StoreSight};
use gungnir_analytics::HeightField;
use gungnir_config::ConfigBaseline;
use gungnir_data::geoid::{GeoidGrid, PinnedGrid};
use gungnir_data::terrain_store::{StoreFootprint, TerrainStore};

/// Where the node's terrain stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerrainStatus {
    /// The baseline names no terrain.
    NotConfigured,
    /// The baseline names one DEM file (`terrain.path`), which only the desktop reads.
    SingleDem { path: String },
    /// The store, or the baseline's declaration of it, was refused.
    Refused { store: String, reason: String },
    /// The store is verified; what line of sight has made of it.
    StoreVerified {
        store: String,
        tiles: usize,
        levels: u8,
        sight: Sight,
    },
}

/// Line of sight over a verified store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sight {
    /// The footprint is being placed, or the field painted.
    Building,
    /// The field is built: the level it was read at, the tiles painted into it, those
    /// refused (flat where they lie), and whether the extent reaches past the store (flat
    /// there too).
    Built {
        level: u8,
        tiles: usize,
        refused: usize,
        past_edge: bool,
    },
    /// Why there is none: no local frame, nothing declared to cover, a footprint that
    /// could not be placed, or no staged tile under the extent.
    NotBuilt(String),
}

const FLAT: &str = "line of sight is flat (optimistic)";

impl TerrainStatus {
    /// One line for the node's health log.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            TerrainStatus::NotConfigured => format!("no terrain configured; {FLAT}"),
            TerrainStatus::SingleDem { path } => format!(
                "terrain.path names one DEM ({path}), which only the desktop reads; the node's \
                 line of sight reads a pinned store (terrain.store), so {FLAT}"
            ),
            TerrainStatus::Refused { store, reason } => {
                format!("staged store {store} not opened: {reason}; {FLAT}")
            }
            TerrainStatus::StoreVerified {
                store,
                tiles,
                levels,
                sight,
            } => {
                let read = match sight {
                    Sight::Building => format!(
                        "building line of sight over the coverage extent; {FLAT} until it is \
                         built"
                    ),
                    Sight::Built {
                        level,
                        tiles,
                        refused,
                        past_edge,
                    } => {
                        let mut line = format!(
                            "line of sight reads the pinned store at level {level} over the \
                             coverage extent ({tiles} tiles"
                        );
                        if *refused > 0 {
                            use std::fmt::Write as _;
                            // Writing into a `String` cannot fail.
                            let _ = write!(line, ", {refused} refused and flat");
                        }
                        line.push(')');
                        if *past_edge {
                            line.push_str(
                                "; the extent reaches past the store's edge, and line of sight \
                                 is flat there (optimistic)",
                            );
                        }
                        line
                    }
                    Sight::NotBuilt(reason) => {
                        format!("line of sight over the store is not built: {reason}; {FLAT}")
                    }
                };
                format!(
                    "staged store {store}: manifest verified, {tiles} tiles over {levels} \
                     levels; {read}"
                )
            }
        }
    }
}

/// The node's terrain: its status, the field line of sight reads once one is built, and
/// the thread that builds it.
pub struct NodeTerrain {
    status: TerrainStatus,
    field: Option<Arc<HeightField>>,
    /// Whether the baseline names a fill source, which the node never reads (D-164).
    fill_named: bool,
    worker: Option<Worker>,
    /// The extent last asked for, so a field painted over an older one is dropped.
    asked: Option<[[f64; 2]; 2]>,
}

impl std::fmt::Debug for NodeTerrain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeTerrain")
            .field("status", &self.status)
            .field("field", &self.field.is_some())
            .field("asked", &self.asked)
            .finish_non_exhaustive()
    }
}

/// What the worker sends back: the extent it painted over, and the field or why not.
type Painted = ([[f64; 2]; 2], Result<StoreSight, String>);

/// The thread placing the footprint and painting fields. Dropping it closes its inbox and
/// joins it, so no PROJ work outlives the node (GAP-260).
struct Worker {
    /// `None` only inside `drop`, which closes it before joining.
    jobs: Option<mpsc::Sender<[[f64; 2]; 2]>>,
    done: mpsc::Receiver<Painted>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            // A thread that panicked has nothing left to tear down.
            let _ = thread.join();
        }
    }
}

impl NodeTerrain {
    /// Read what the baseline names: nothing, one DEM the node does not read, or a store,
    /// which is opened and verified here and handed to a worker that places its footprint.
    /// Call [`Self::poll`] on the tick to ask for the field and take it.
    #[must_use]
    pub fn start(config: &ConfigBaseline) -> NodeTerrain {
        let mut terrain = NodeTerrain {
            status: TerrainStatus::NotConfigured,
            field: None,
            fill_named: false,
            worker: None,
            asked: None,
        };
        let Some(declared) = config.terrain.as_ref() else {
            return terrain;
        };
        terrain.fill_named = declared.fill_source.is_some();
        // Validation makes it exactly one of the two, and a store always pinned; a baseline
        // that skipped validation is refused here rather than guessed at.
        let (store, digest) = match (&declared.path, &declared.store, &declared.manifest_sha256) {
            (Some(path), None, _) => {
                terrain.status = TerrainStatus::SingleDem { path: path.clone() };
                return terrain;
            }
            (None, Some(store), Some(digest)) => (store.clone(), digest.clone()),
            _ => {
                terrain.status = TerrainStatus::Refused {
                    store: "terrain".to_string(),
                    reason: "it names neither one DEM file nor one pinned store".to_string(),
                };
                return terrain;
            }
        };
        let opened = TerrainStore::open(std::path::Path::new(&store), &digest)
            .map_err(|e| e.to_string())
            .and_then(|opened| {
                let manifest = opened.index().manifest();
                let declaration = declared.store_declaration(
                    &manifest.frame,
                    manifest.vertical.as_deref(),
                    manifest.nad83_realisation.as_deref(),
                )?;
                let read = read_declared(
                    &declaration.frame,
                    declaration.vertical.as_deref(),
                    declaration.nad83_realisation.as_deref(),
                )?;
                Ok((opened, read))
            });
        match opened {
            Ok((opened, read)) => {
                terrain.status = TerrainStatus::StoreVerified {
                    store,
                    tiles: opened.index().len(),
                    levels: opened.index().levels(),
                    sight: Sight::Building,
                };
                terrain.worker = Some(Worker::start(
                    opened,
                    Placing {
                        declared: read,
                        origin: config.origin,
                        epoch: config
                            .frame_epoch
                            .unwrap_or_else(gungnir_data::nad83::current_epoch),
                        geoid_dir: config.geoid_grid_dir.clone(),
                    },
                ));
            }
            Err(reason) => terrain.status = TerrainStatus::Refused { store, reason },
        }
        terrain
    }

    /// On the tick: take a field the worker finished for the extent last asked for, and ask
    /// again when the coverage extent moved (DN-36 §5.2). A field painted over an extent
    /// since moved is dropped, the newer one on its way.
    pub fn poll(&mut self, config: &ConfigBaseline) {
        let TerrainStatus::StoreVerified { sight, .. } = &mut self.status else {
            return;
        };
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        while let Ok((extent, built)) = worker.done.try_recv() {
            if Some(extent) != self.asked {
                continue;
            }
            match built {
                Ok(built) => {
                    *sight = Sight::Built {
                        level: built.level,
                        tiles: built.tiles,
                        refused: built.refused,
                        past_edge: built.past_edge,
                    };
                    self.field = Some(Arc::new(built.field));
                }
                Err(reason) => {
                    *sight = Sight::NotBuilt(reason);
                    self.field = None;
                }
            }
        }
        match config.coverage_extent() {
            Ok(extent) if self.asked != Some(extent) => {
                let sent = worker.jobs.as_ref().is_some_and(|j| j.send(extent).is_ok());
                if sent {
                    self.asked = Some(extent);
                    *sight = Sight::Building;
                } else {
                    *sight = Sight::NotBuilt("the terrain worker is gone".to_string());
                }
                self.field = None;
            }
            Ok(_) => {}
            Err(reason) => {
                self.asked = None;
                self.field = None;
                *sight = Sight::NotBuilt(reason);
            }
        }
    }

    /// The field line of sight reads, once one is built over the current extent.
    #[must_use]
    pub fn field(&self) -> Option<&HeightField> {
        self.field.as_deref()
    }

    #[must_use]
    pub fn status(&self) -> &TerrainStatus {
        &self.status
    }

    /// What the node's line of sight reads, or why it reads nothing, for the health line.
    #[must_use]
    pub fn line(&self) -> String {
        let mut line = self.status.line();
        if self.fill_named {
            line.push_str(
                "; terrain.fill_source is ground a desktop draws for context, never read here \
                 (D-164)",
            );
        }
        line
    }
}

/// What every tile is placed with, moved onto the worker.
struct Placing {
    declared: Declared,
    origin: Option<[f64; 3]>,
    epoch: f64,
    geoid_dir: Option<String>,
}

impl Worker {
    fn start(store: TerrainStore, placing: Placing) -> Worker {
        let (jobs, inbox) = mpsc::channel::<[[f64; 2]; 2]>();
        let (outbox, done) = mpsc::channel::<Painted>();
        let thread = std::thread::Builder::new()
            .name("node-terrain".to_string())
            .spawn(move || {
                let grids = verified_grids(placing.geoid_dir.as_deref());
                let footprint: Result<StoreFootprint, String> = placement::place_footprint(
                    store.index(),
                    &placing.declared,
                    placing.origin,
                    placing.epoch,
                    &grids,
                    1,
                );
                for extent in inbox {
                    let built = footprint
                        .as_ref()
                        .map_err(Clone::clone)
                        .and_then(|footprint| {
                            placement::store_sight(footprint, extent, &|key| {
                                placement::place_store_tile(
                                    &store,
                                    key,
                                    &placing.declared,
                                    placing.origin,
                                    placing.epoch,
                                    &grids,
                                )
                            })
                        });
                    // The node has dropped its terrain; nothing to report to.
                    if outbox.send((extent, built)).is_err() {
                        break;
                    }
                }
            });
        match thread {
            Ok(thread) => Worker {
                jobs: Some(jobs),
                done,
                thread: Some(thread),
            },
            // No thread: the inbox is dropped with the closure, so a send fails and the
            // line says the worker is gone.
            Err(_) => Worker {
                jobs: None,
                done,
                thread: None,
            },
        }
    }
}

/// The baseline's `terrain` strings read into what `gungnir_analytics::placement` places
/// with, as the desktop's `terrain::read_declared` reads them: the frame refused by name
/// when it does not parse, the vertical system and the realisation kept as read, a refusal
/// included, for the placement that needs them.
fn read_declared(
    frame: &str,
    vertical: Option<&str>,
    nad83_realisation: Option<&str>,
) -> Result<Declared, String> {
    use gungnir_config::{Frame, Nad83Realisation, VerticalFrame};
    use gungnir_data::geoid::VerticalDatum;
    let frame: Frame = frame
        .parse()
        .map_err(|reason| format!("terrain.frame {reason}"))?;
    let vertical = vertical
        .map(|v| {
            v.parse::<VerticalFrame>()
                .map(|frame| match frame {
                    VerticalFrame::Ellipsoidal => VerticalDatum::Ellipsoidal,
                    VerticalFrame::Epsg(code) => VerticalDatum::from_epsg(code),
                })
                .map_err(|reason| format!("terrain.vertical {reason}"))
        })
        .transpose();
    let nad83_realisation = nad83_realisation
        .map(|d| {
            const FIELD: &str = "terrain.nad83_realisation";
            let Nad83Realisation(code) = d
                .parse::<Nad83Realisation>()
                .map_err(|reason| format!("{FIELD} {reason}"))?;
            gungnir_data::nad83::Realisation::from_geographic_epsg(code).ok_or_else(|| {
                format!("{FIELD} names EPSG:{code}, which gungnir-data has no realisation for")
            })
        })
        .transpose();
    Ok(Declared {
        frame_epsg: match frame {
            Frame::LocalEnu => None,
            Frame::Epsg(code) => Some(code),
        },
        vertical,
        nad83_realisation,
    })
}

/// Each pinned grid a placement asks for, found where the desktop finds it
/// (`gungnir_data::geoid::locate`: `geoid_grid_dir`, final when named, else `PROJ_DATA`)
/// and verified against its digest the first time it is asked for, then held.
fn verified_grids(dir: Option<&str>) -> impl Fn(PinnedGrid) -> Result<GeoidGrid, String> {
    let dir = dir.map(str::to_string);
    let held = std::cell::RefCell::new(HashMap::<PinnedGrid, Result<GeoidGrid, String>>::new());
    move |pinned| {
        held.borrow_mut()
            .entry(pinned)
            .or_insert_with(|| {
                let Some((path, named)) =
                    gungnir_data::geoid::locate(dir.as_deref(), pinned.file())
                else {
                    return Err(format!(
                        "no {} grid is installed ({} is in no geoid_grid_dir or PROJ_DATA \
                         directory)",
                        pinned.name(),
                        pinned.file()
                    ));
                };
                let from = if named { "geoid_grid_dir" } else { "PROJ_DATA" };
                GeoidGrid::verify(&path, pinned.sha256())
                    .map_err(|e| format!("{} (from {from}) refused: {e}", path.display()))
            })
            .clone()
    }
}
