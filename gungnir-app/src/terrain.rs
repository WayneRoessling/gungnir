// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain on the desktop (GAP-023): the baseline names a DEM, the loader thread reads
//! it, and the coverage analytics mask line of sight against it.
//!
//! **Placement used to be the honest problem, and now has a real answer for it.** A DEM
//! arrives in its own frame (a projected CRS, or geographic degrees) and the picture is
//! local ENU metres; converting between them needed a projection library the approved
//! stack did not hold, so the baseline could only ever state that the DEM was already
//! prepared in the local frame (`frame: "local-enu"`), and a file whose own tags said
//! otherwise was refused by name.
//!
//! D-41 (2026-09-08) closed that: `frame` also accepts `"epsg:<code>"`
//! ([`gungnir_config::Frame`]), and [`place`] reconciles it against the file's own
//! tags (`GridCrs`) and converts through `gungnir_data::geospatial::crs::to_wgs84`
//! (behind the `crs` feature; PROJ) and then `gungnir_coord::Wgs84` (always
//! available -- geographic-to-local-ENU needs no native dependency) using the
//! deployment's own declared origin. A file whose tags still contradict what `frame`
//! declares, or a real-world CRS this build was not compiled to convert, is refused by
//! name exactly as before.
//!
//! **A converted height is a real height since GAP-108** (D-121, D-122). The DEM's own
//! height is made a WGS-84 ellipsoidal one -- unchanged when the file or `terrain.vertical`
//! says it already is, with its datum's geoid undulation added when it is an EGM2008, EGM96
//! or NAVD88 height (GAP-197, D-125), and refused by name in any other datum or when
//! nobody states one -- and each vertex's
//! height becomes its local ENU up, the quantity line of sight compares a sensor's own up
//! against. Before, the file's height was used as the up unchanged.
//!
//! **The conversion itself is `gungnir_analytics::placement`'s since GAP-227** (DN-36 §9):
//! the node's line of sight places a staged store's tiles by the same path, so it moved
//! there rather than being copied. This module reads the baseline's strings into it.

use gungnir_config::Frame;
use gungnir_data::geospatial::TerrainMesh;
use gungnir_data::{LoadRequest, LoadResult};

use crate::state::AppState;

/// Where the terrain stands, for PN-09 and PN-11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerrainStatus {
    /// The baseline names no terrain: line of sight is flat, and the coverage report
    /// says so on its parameters.
    NotConfigured,
    Loading {
        path: String,
    },
    Loaded {
        path: String,
        vertices: usize,
        triangles: usize,
        /// Which placement this is, so the viewport builds the surface it draws once per
        /// load (GAP-252, DN-35 §6). Unique within the process: [`next_revision`].
        revision: u64,
    },
    /// A staged store (GAP-227, `docs/design/DN-36-staged-terrain-and-map-tiles.md`)
    /// whose manifest matched the baseline's digest and agrees with its frame. Its tiles
    /// are drawn by view ([`crate::terrain_tiles`]), and line of sight reads it over the
    /// coverage extent, never the view (DN-36 §5.2), once `sight` is built.
    StoreVerified {
        store: String,
        tiles: usize,
        levels: u8,
        sight: SightState,
    },
    /// The file could not be read or placed; the reason is on screen, and line of
    /// sight stays flat rather than masked against something that is not there.
    Failed {
        path: String,
        reason: String,
    },
}

impl TerrainStatus {
    /// One line for the health panel.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            TerrainStatus::NotConfigured => {
                "no terrain configured; line of sight is flat (optimistic)".to_string()
            }
            TerrainStatus::Loading { path } => format!("loading {path}"),
            TerrainStatus::Loaded {
                path,
                vertices,
                triangles,
                ..
            } => {
                format!("{path}: {vertices} vertices, {triangles} triangles; masking line of sight")
            }
            TerrainStatus::StoreVerified {
                store,
                tiles,
                levels,
                sight,
            } => format!(
                "staged store {store}: manifest verified, {tiles} tiles over {levels} levels, \
                 drawn by view; {}",
                sight.line()
            ),
            TerrainStatus::Failed { path, reason } => {
                format!("{path} not loaded: {reason}; line of sight is flat (optimistic)")
            }
        }
    }

    #[must_use]
    pub fn is_masking(&self) -> bool {
        matches!(
            self,
            TerrainStatus::Loaded { .. }
                | TerrainStatus::StoreVerified {
                    sight: SightState::Built { .. },
                    ..
                }
        )
    }
}

/// Line of sight over a staged store (GAP-227, DN-36 §5.2 and amendment 2): built from the
/// pinned store alone, over the coverage extent, at the finest level that keeps the field
/// within its budget ([`crate::terrain_tiles`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SightState {
    /// The store's footprint is still being placed, or the field is being painted.
    Building,
    /// The field is built: the level it was read at, the tiles painted into it, those
    /// refused (flat where they lie), and whether the extent reaches past the store
    /// (flat there too).
    Built {
        level: u8,
        tiles: usize,
        refused: usize,
        past_edge: bool,
    },
    /// Why there is none: no local frame, nothing declared to cover, or a field that could
    /// not be made.
    NotBuilt(String),
}

impl SightState {
    /// What line of sight reads, for the terrain line.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            SightState::Building => "building line of sight from the store over the coverage \
                                     extent; line of sight is flat (optimistic) until it is built"
                .to_string(),
            SightState::Built {
                level,
                tiles,
                refused,
                past_edge,
            } => {
                let mut line = format!(
                    "line of sight reads the pinned store at level {level} over the coverage \
                     extent, never the view ({tiles} tiles"
                );
                if *refused > 0 {
                    use std::fmt::Write as _;
                    let _ = write!(line, ", {refused} refused and flat");
                }
                line.push(')');
                if *past_edge {
                    line.push_str(
                        "; the extent reaches past the store's edge, and line of sight is flat \
                         there (optimistic)",
                    );
                }
                line
            }
            SightState::NotBuilt(reason) => format!(
                "line of sight over the store is not built: {reason}; line of sight is flat \
                 (optimistic)"
            ),
        }
    }
}

/// The placed terrain for the viewport, once it is loaded (PN-02 draws it under the
/// picture). `None` while nothing is placed, which draws nothing and claims nothing.
///
/// Takes the two fields rather than the state, so the viewport can borrow its own state
/// mutably in the same call.
#[must_use]
pub fn layer<'a>(
    status: &TerrainStatus,
    data: &'a gungnir_data::DataStore,
) -> Option<gungnir_viewport3d::layers::TerrainLayer<'a>> {
    let TerrainStatus::Loaded { revision, .. } = status else {
        return None;
    };
    data.terrains
        .first()
        .map(|t| gungnir_viewport3d::layers::TerrainLayer {
            positions: &t.positions,
            rows: t.rows,
            columns: t.columns,
            revision: *revision,
        })
}

/// A revision no earlier placement in this process has had. Process-wide rather than per
/// `AppState`, so two placements can never share one, whichever state made them.
#[must_use]
pub fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Start loading the configured terrain off the render thread. Idempotent: the first
/// tick calls it, and a load already started or finished is left alone.
pub fn start(state: &mut AppState) {
    if state.loader.is_some() || !matches!(state.terrain, TerrainStatus::NotConfigured) {
        return;
    }
    // GAP-108, GAP-197: a DEM stating geoid heights needs its verified grid when it is
    // placed, so the load waits for the start-up checks to settle (`crate::geoid`).
    if !state.geoid.is_settled() {
        return;
    }
    let Some(terrain) = state.config.terrain.clone() else {
        return;
    };
    // Validation makes it exactly one of the two, and a store always pinned; a baseline
    // that skipped validation is refused here rather than guessed at.
    let (path, request) = match (&terrain.path, &terrain.store, &terrain.manifest_sha256) {
        (Some(path), None, _) => (
            path.clone(),
            LoadRequest::Terrain(std::path::PathBuf::from(path)),
        ),
        (None, Some(store), Some(digest)) => (
            store.clone(),
            LoadRequest::TerrainStore(std::path::PathBuf::from(store), digest.clone()),
        ),
        _ => {
            state.terrain = TerrainStatus::Failed {
                path: "terrain".to_string(),
                reason: "it names neither one DEM file nor one pinned store".to_string(),
            };
            return;
        }
    };
    let (requests, results) = gungnir_data::spawn_loader();
    match requests.send(request) {
        Ok(()) => {
            state.terrain = TerrainStatus::Loading { path };
            state.loader = Some((requests, results));
        }
        Err(err) => {
            state.terrain = TerrainStatus::Failed {
                path,
                reason: format!("the loader thread is gone: {err}"),
            };
        }
    }
}

/// Poll the loader; on the tick, so a slow file never stalls a frame. Starts the load
/// on the first call.
pub fn poll(state: &mut AppState) {
    start(state);
    let Some((_, results)) = state.loader.as_ref() else {
        return;
    };
    let Ok(result) = results.try_recv() else {
        return;
    };
    let path = match &state.terrain {
        TerrainStatus::Loading { path } => path.clone(),
        _ => return,
    };
    let loaded = match result {
        LoadResult::Terrain(loaded) => loaded,
        LoadResult::TerrainStore(opened) => {
            store_opened(state, path, opened);
            state.loader = None;
            return;
        }
        _ => return,
    };
    match loaded {
        Ok(mesh) => {
            // `start` only ever sends a `LoadRequest::Terrain` when `config.terrain` is
            // `Some` (its own early return above), so this is reachable only with one
            // configured; the fallback is defensive rather than reachable in practice,
            // and "local-enu" is the same as `mesh.placed()`'s original behaviour.
            let declared = state
                .config
                .terrain
                .as_ref()
                .map_or(Declared::default(), |t| Declared {
                    frame: t.frame.as_str(),
                    vertical: t.vertical.as_deref(),
                    nad83_realisation: t.nad83_realisation.as_deref(),
                });
            let epoch = crate::geoid::frame_epoch(&state.config);
            let placed = place(mesh, declared, state.config.origin, epoch, &state.geoid).and_then(
                |placed| {
                    let field = height_field_of(&placed)?;
                    Ok((placed, field))
                },
            );
            match placed {
                Ok((placed, field)) => {
                    state.terrain = TerrainStatus::Loaded {
                        path,
                        vertices: placed.positions.len(),
                        triangles: placed.triangle_count(),
                        revision: next_revision(),
                    };
                    state.data.terrains.clear();
                    state.data.terrains.push(placed);
                    state.height_field = Some(std::sync::Arc::new(field));
                }
                Err(reason) => {
                    state.height_field = None;
                    state.terrain = TerrainStatus::Failed {
                        path: path.clone(),
                        reason: reason.clone(),
                    };
                    state
                        .alerts
                        .push(format!("terrain {path} not placed: {reason}"));
                }
            }
        }
        Err(err) => {
            let reason = err.to_string();
            state.height_field = None;
            state.terrain = TerrainStatus::Failed {
                path: path.clone(),
                reason: reason.clone(),
            };
            state
                .alerts
                .push(format!("terrain {path} not loaded: {reason}"));
        }
    }
    state.loader = None;
}

/// A staged store came back from the loader thread: verified against the baseline's
/// digest, or refused by name. Its manifest's frame has to be the baseline's, and its
/// vertical system and NAD83 realisation may not contradict the baseline's
/// ([`store_declaration`]).
fn store_opened(
    state: &mut AppState,
    path: String,
    opened: Result<
        Box<gungnir_data::terrain_store::TerrainStore>,
        gungnir_data::terrain_store::StoreError,
    >,
) {
    let checked = opened.map_err(|e| e.to_string()).and_then(|store| {
        let terrain = state
            .config
            .terrain
            .as_ref()
            .ok_or_else(|| "the baseline no longer names terrain".to_string())?;
        let declaration = store_declaration(terrain, store.index().manifest())?;
        Ok((store, declaration))
    });
    state.height_field = None;
    match checked {
        Ok((store, declaration)) => {
            state.terrain = TerrainStatus::StoreVerified {
                store: path,
                tiles: store.index().len(),
                levels: store.index().levels(),
                sight: SightState::Building,
            };
            let store: std::sync::Arc<_> = std::sync::Arc::from(store);
            crate::terrain_tiles::start(state, std::sync::Arc::clone(&store), declaration);
            state.terrain_store = Some(store);
        }
        Err(reason) => {
            state.terrain_store = None;
            state
                .alerts
                .push(format!("terrain store {path} not opened: {reason}"));
            state.terrain = TerrainStatus::Failed { path, reason };
        }
    }
}

/// The frame, vertical system and NAD83 realisation every tile of a store is placed
/// with (DN-36 §4.2): the baseline's, which the manifest has to agree with, by the rule
/// `gungnir-config` holds for the desktop and the node alike
/// ([`gungnir_config::TerrainConfig::store_declaration`]).
///
/// # Errors
///
/// A frame that differs, or a vertical system or realisation the two state differently,
/// by name.
pub fn store_declaration(
    terrain: &gungnir_config::TerrainConfig,
    manifest: &gungnir_data::terrain_store::Manifest,
) -> Result<StoreDeclaration, String> {
    terrain.store_declaration(
        &manifest.frame,
        manifest.vertical.as_deref(),
        manifest.nad83_realisation.as_deref(),
    )
}

/// What every tile of a store is placed with, once the baseline and the manifest agree.
pub use gungnir_config::StoreDeclaration;

/// Place one store tile, as read and meshed by the loader thread, through the path the
/// single DEM takes ([`place`]): the tile's own tags reconciled with the declaration, its
/// heights made WGS 84 ellipsoidal through the verified geoid grids, then local ENU.
///
/// # Errors
///
/// As the single DEM's placement, by name.
pub fn place_tile(
    mesh: TerrainMesh,
    declaration: &StoreDeclaration,
    origin: Option<[f64; 3]>,
    epoch: f64,
    geoid: &crate::geoid::GeoidGrids,
) -> Result<TerrainMesh, String> {
    place(
        mesh,
        Declared {
            frame: &declaration.frame,
            vertical: declaration.vertical.as_deref(),
            nad83_realisation: declaration.nad83_realisation.as_deref(),
        },
        origin,
        epoch,
        geoid,
    )
}

/// The placed surface resampled once for line of sight (GAP-247,
/// `docs/design/DN-34-sensor-dead-spots.md` §6.1): at the DEM's own spacing, made coarser
/// only when the raster would be over its budget. A surface that cannot be resampled is
/// refused with the terrain, by name: drawing a DEM the coverage analytics cannot mask
/// against would show ground the line of sight ignores.
///
/// # Errors
///
/// When the placed mesh is not a grid, or its surface cannot be resampled.
pub fn height_field_of(placed: &TerrainMesh) -> Result<gungnir_analytics::HeightField, String> {
    let refused =
        |why: String| format!("its surface could not be resampled for line of sight: {why}");
    let native = gungnir_analytics::HeightField::native_cell_m(placed)
        .ok_or_else(|| refused("it is not a grid of at least two rows and columns".into()))?;
    let (cell, _) = gungnir_analytics::HeightField::fitting_cell_m(
        placed,
        native,
        gungnir_analytics::HEIGHT_FIELD_CELL_BUDGET,
    );
    gungnir_analytics::HeightField::resample(placed, cell).map_err(|e| refused(e.to_string()))
}

/// One line for PN-09 about the resampled surface, or `None` while there is none.
#[must_use]
pub fn height_field_line(field: Option<&gungnir_analytics::HeightField>) -> Option<String> {
    let field = field?;
    Some(format!(
        "line of sight reads the surface resampled to {:.0} m cells, {} by {} ({} cells)",
        field.cell_m,
        field.columns,
        field.rows,
        field.up_m.len()
    ))
}

/// Turn a loaded mesh into one placed in the deployment's local ENU frame, or say why
/// not (GAP-023, D-41; GAP-108, GAP-197, GAP-200). `declared` is `state.config.terrain`'s
/// own strings; `origin` is `state.config.origin`, the same geodetic anchor every other
/// local-ENU thing in the picture (`DetectionView::measurement`, `TrackView::state`, ...) is
/// relative to.
///
/// The strings are read here, where the baseline's types are, and placed by
/// [`gungnir_analytics::placement::place`], the path the node's line of sight takes too
/// (DN-36 §9): a frame that does not parse is refused first, and a vertical system or a
/// realisation only when the frame is a real-world CRS that reads them.
fn place(
    mesh: TerrainMesh,
    declared: Declared<'_>,
    origin: Option<[f64; 3]>,
    epoch: f64,
    geoid: &crate::geoid::GeoidGrids,
) -> Result<TerrainMesh, String> {
    let read = read_declared(
        declared.frame,
        declared.vertical,
        declared.nad83_realisation,
    )?;
    gungnir_analytics::placement::place(mesh, &read, origin, epoch, &|pinned| geoid.grid(pinned))
}

/// The baseline's `terrain` strings read into what [`gungnir_analytics::placement`] places
/// with: the frame, refused by name when it does not parse, and the vertical system and the
/// realisation as read, a refusal kept for the placement that needs them.
///
/// # Errors
///
/// A frame that does not parse.
pub fn read_declared(
    frame: &str,
    vertical: Option<&str>,
    nad83_realisation: Option<&str>,
) -> Result<gungnir_analytics::placement::Declared, String> {
    let frame: Frame = frame
        .parse()
        .map_err(|reason| format!("terrain.frame {reason}"))?;
    Ok(gungnir_analytics::placement::Declared {
        frame_epsg: match frame {
            Frame::LocalEnu => None,
            Frame::Epsg(code) => Some(code),
        },
        vertical: crate::geoid::declared_datum("terrain.vertical", vertical),
        nad83_realisation: crate::geoid::declared_realisation(
            "terrain.nad83_realisation",
            nad83_realisation,
        ),
    })
}

/// What `terrain` declares about the DEM, borrowed from the baseline for one placement.
#[derive(Debug, Clone, Copy)]
struct Declared<'a> {
    frame: &'a str,
    vertical: Option<&'a str>,
    nad83_realisation: Option<&'a str>,
}

impl Default for Declared<'_> {
    /// `"local-enu"` and nothing else: what `mesh.placed()` always did.
    fn default() -> Self {
        Declared {
            frame: "local-enu",
            vertical: None,
            nad83_realisation: None,
        }
    }
}
