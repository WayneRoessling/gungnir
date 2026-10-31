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

use gungnir_config::Frame;
use gungnir_coord::CoordTransform;
use gungnir_data::geospatial::{GridCrs, TerrainMesh};
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
/// with (DN-36 §4.2): the baseline's, which the manifest has to agree with. The frame
/// must be the same frame; a vertical system or a realisation the baseline leaves out is
/// the manifest's, pinned by the same digest, and one stated in both must be the same.
///
/// # Errors
///
/// A frame that differs, or a vertical system or realisation the two state differently,
/// by name.
pub fn store_declaration(
    terrain: &gungnir_config::TerrainConfig,
    manifest: &gungnir_data::terrain_store::Manifest,
) -> Result<StoreDeclaration, String> {
    let parse_frame = |field: &str, text: &str| {
        text.parse::<Frame>()
            .map_err(|reason| format!("{field} {reason}"))
    };
    let baseline = parse_frame("terrain.frame", &terrain.frame)?;
    let staged = parse_frame("the store manifest's frame", &manifest.frame)?;
    if baseline != staged {
        return Err(format!(
            "terrain.frame declares {baseline} and the store was staged in {staged}"
        ));
    }
    let agree = |field: &str,
                 ours: Option<&String>,
                 theirs: Option<&String>,
                 same: &dyn Fn(&str, &str) -> bool|
     -> Result<Option<String>, String> {
        match (ours, theirs) {
            (Some(a), Some(b)) if !same(a, b) => Err(format!(
                "terrain.{field} declares {a:?} and the store manifest states {b:?}"
            )),
            (Some(a), _) => Ok(Some(a.clone())),
            (None, b) => Ok(b.cloned()),
        }
    };
    let vertical = agree(
        "vertical",
        terrain.vertical.as_ref(),
        manifest.vertical.as_ref(),
        &|a, b| {
            a.parse::<gungnir_config::VerticalFrame>().ok()
                == b.parse::<gungnir_config::VerticalFrame>().ok()
        },
    )?;
    let nad83_realisation = agree(
        "nad83_realisation",
        terrain.nad83_realisation.as_ref(),
        manifest.nad83_realisation.as_ref(),
        &|a, b| {
            a.parse::<gungnir_config::Nad83Realisation>().ok()
                == b.parse::<gungnir_config::Nad83Realisation>().ok()
        },
    )?;
    Ok(StoreDeclaration {
        frame: terrain.frame.clone(),
        vertical,
        nad83_realisation,
    })
}

/// What every tile of a store is placed with, once the baseline and the manifest agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDeclaration {
    pub frame: String,
    pub vertical: Option<String>,
    pub nad83_realisation: Option<String>,
}

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

/// WGS84 geographic, EPSG:4326 -- `gungnir_data::geospatial::crs::WGS84_EPSG` restated
/// so this module's unconditional code does not need the `crs` feature just
/// to name the constant.
const WGS84_EPSG: u32 = 4326;

/// Turn a loaded mesh into one placed in the deployment's local ENU frame, or say why
/// not (GAP-023, D-41). `declared_frame` is `state.config.terrain`'s own `frame` string;
/// `origin` is `state.config.origin`, the same geodetic anchor every other local-ENU
/// thing in the picture (`DetectionView::measurement`, `TrackView::state`, ...) is
/// relative to, reused rather than inventing a second one for terrain alone.
///
/// The file's own tags (`mesh.crs`) are read regardless of what `declared_frame` says:
/// a `"local-enu"` declaration contradicted by a file that states a real CRS is refused,
/// exactly as before D-41; a declared real-world CRS is cross-checked against the
/// file's own tags when it has any (a `GeoTIFF`) and trusted outright when it does not
/// (an ESRI ASCII grid, which never carries CRS tags at all -- `frame` is the only place
/// its CRS could ever come from).
///
/// **The height, since GAP-108** (D-121, D-122): `declared_vertical` is
/// `terrain.vertical`, and with the file's own `VerticalGeoKey` it decides how the DEM's
/// heights become WGS-84 ellipsoidal ones (`gungnir_data::geoid::height_reference`),
/// refusing by name what cannot be converted. That is decided before any projection runs,
/// so a refusal needs no PROJ. Each vertex then lands at its full local ENU position, up
/// included. A `"local-enu"` DEM is untouched: its heights are already the deployment's
/// own up.
///
/// **And the frame, since GAP-200** (D-127, D-128): a NAD83 DEM is carried to WGS 84 by
/// its realisation -- the file's, or `terrain.nad83_realisation`'s for an unqualified
/// NAD83 file -- and `epoch`, and a NAVD88 height takes the Helmert's height difference
/// at `epoch` ([`gungnir_data::nad83::FrameConversion`]).
fn place(
    mesh: TerrainMesh,
    declared: Declared<'_>,
    origin: Option<[f64; 3]>,
    epoch: f64,
    geoid: &crate::geoid::GeoidGrids,
) -> Result<TerrainMesh, String> {
    let frame: Frame = declared
        .frame
        .parse()
        .map_err(|reason| format!("terrain.frame {reason}"))?;
    let Some(source_epsg) = source_epsg(frame, mesh.crs)? else {
        return Ok(mesh.placed());
    };
    let Some(origin) = origin else {
        return Err(format!(
            "the terrain declares a real-world CRS (EPSG:{source_epsg}) but the deployment has \
             no declared local ENU origin (config.origin) to place it against"
        ));
    };
    let baseline_vertical = crate::geoid::declared_datum("terrain.vertical", declared.vertical)?;
    let heights = gungnir_data::geoid::height_reference(
        "terrain.vertical",
        mesh.vertical_datum().as_ref(),
        baseline_vertical.as_ref(),
        &|model| geoid.grid(model),
    )?;
    let conversion = frame_conversion(source_epsg, declared.nad83_realisation, epoch, geoid)?;
    let absolute = mesh.absolute_positions_xy();
    let lon_lat_deg = if source_epsg == WGS84_EPSG {
        // Already geographic WGS84: what a GeoTIFF's own [x, y] already means for a
        // `GridCrs::Geographic` file (longitude, latitude, degrees), so there is
        // nothing for PROJ to do, and this path needs no `crs` feature.
        absolute
    } else {
        to_wgs84(&absolute, source_epsg, &conversion)?
    };
    let file_heights: Vec<f64> = mesh.positions.iter().map(|p| f64::from(p[2])).collect();
    let ellipsoidal =
        gungnir_data::nad83::wgs84_heights(&heights, &conversion, &lon_lat_deg, &file_heights)
            .map_err(|e| e.to_string())?;
    let enu = to_local_enu(&lon_lat_deg, &ellipsoidal, origin);
    // 32767 is GeoTIFF's "user-defined" projected code, the existing convention this
    // codebase already uses for "trust it as local" (see `source_epsg` below); reused
    // here as the marker for "already placed", since a mesh in this state has exactly
    // that property and `.placed()` on it is correctly a no-op (`origin` is `[0, 0]`).
    mesh.with_enu(&enu, GridCrs::Projected { epsg: Some(32_767) })
        .map_err(|e| e.to_string())
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

/// How the DEM's positions and heights reach WGS 84 beyond PROJ's own conversion
/// (GAP-200): a WGS 84 geographic file is not NAD83's and needs no PROJ to say so; any
/// other has its datum read through PROJ, and a build without `crs` refuses it by name,
/// as it refuses converting it.
fn frame_conversion(
    source_epsg: u32,
    declared: Option<&str>,
    epoch: f64,
    geoid: &crate::geoid::GeoidGrids,
) -> Result<gungnir_data::nad83::FrameConversion, String> {
    const FIELD: &str = "terrain.nad83_realisation";
    let declared = crate::geoid::declared_realisation(FIELD, declared)?;
    let file = if source_epsg == WGS84_EPSG {
        gungnir_data::nad83::HorizontalDatum::NotNad83
    } else if cfg!(feature = "crs") {
        crate::geoid::file_datum(&format!("EPSG:{source_epsg}"))?
    } else {
        // This build cannot convert the file at all, and `to_wgs84` says so by name; a
        // datum it could not read anyway is not the refusal to lead with.
        return gungnir_data::nad83::FrameConversion::not_nad83(epoch);
    };
    gungnir_data::nad83::FrameConversion::resolve(FIELD, file, declared, epoch, &|step| {
        geoid.grid(step)
    })
}

/// What a grid's `GridCrs` actually asserts, collapsing `GridCrs`'s three variants (two
/// of which carry an `Option<u16>`) down to the three things `source_epsg` needs to
/// distinguish.
enum FileCrs {
    /// `Unstated`, or `Projected` with no code or `GeoTIFF`'s "user-defined" placeholder
    /// (32767, what a local projection carries): nothing here contradicts a
    /// `"local-enu"` declaration.
    NotStated,
    /// A real-world CRS with a known EPSG code, geographic or projected.
    Known(u32),
    /// The file says geographic but names no code: neither "trust it as local" nor
    /// "here is what it is".
    Ambiguous,
}

fn file_crs(crs: GridCrs) -> FileCrs {
    match crs {
        GridCrs::Unstated
        | GridCrs::Projected {
            epsg: None | Some(32_767),
        } => FileCrs::NotStated,
        GridCrs::Geographic { epsg: Some(code) } | GridCrs::Projected { epsg: Some(code) } => {
            FileCrs::Known(u32::from(code))
        }
        GridCrs::Geographic { epsg: None } => FileCrs::Ambiguous,
    }
}

/// What EPSG code (if any) a loaded mesh should be reprojected from, reconciling
/// `frame`'s declaration against the file's own tags. `Ok(None)` means the file is
/// already in the deployment's local frame (nothing to convert); `Ok(Some(code))` names
/// the real-world CRS to convert from; `Err` names a contradiction between what the
/// baseline declares and what the file actually states, refused by name rather than
/// guessed at.
fn source_epsg(frame: Frame, crs: GridCrs) -> Result<Option<u32>, String> {
    match (frame, file_crs(crs)) {
        (Frame::LocalEnu, FileCrs::NotStated) => Ok(None),
        (Frame::LocalEnu, FileCrs::Known(code)) => Err(format!(
            "the file declares a real-world CRS (EPSG:{code}), not the local frame the baseline \
             claims; declare terrain.frame \"epsg:{code}\" instead if that is correct"
        )),
        (Frame::LocalEnu, FileCrs::Ambiguous) => Err(
            "the file declares a geographic frame with no EPSG code, not local metres".to_string(),
        ),
        (Frame::Epsg(declared), FileCrs::NotStated) => Ok(Some(declared)),
        (Frame::Epsg(declared), FileCrs::Known(code)) if code == declared => Ok(Some(declared)),
        (Frame::Epsg(declared), FileCrs::Known(code)) => Err(format!(
            "terrain.frame declares EPSG:{declared} but the file's own tags state EPSG:{code}"
        )),
        (Frame::Epsg(declared), FileCrs::Ambiguous) => Err(format!(
            "terrain.frame declares EPSG:{declared} but the file's tags name a geographic frame \
             with no EPSG code to check it against"
        )),
    }
}

/// Reproject `points` (`[longitude, latitude]` or `[easting, northing]`, matching
/// `source_epsg`) into WGS84 geographic degrees.
///
/// Behind the `crs` feature (PROJ, GAP-023, D-41): the feature-off build
/// below refuses by name instead, so this function's signature -- and every caller --
/// is the same regardless of whether the feature is compiled in.
#[cfg(feature = "crs")]
fn to_wgs84(
    points: &[[f64; 2]],
    source_epsg: u32,
    conversion: &gungnir_data::nad83::FrameConversion,
) -> Result<Vec<[f64; 2]>, String> {
    gungnir_data::nad83::positions_to_wgs84(&format!("EPSG:{source_epsg}"), conversion, points)
        .map_err(|e| e.to_string())
}

#[cfg(not(feature = "crs"))]
fn to_wgs84(
    _points: &[[f64; 2]],
    source_epsg: u32,
    _conversion: &gungnir_data::nad83::FrameConversion,
) -> Result<Vec<[f64; 2]>, String> {
    Err(format!(
        "terrain.frame declares EPSG:{source_epsg}; converting a real-world coordinate \
         reference system other than WGS84 geographic needs the crs feature, which \
         this build was not compiled with"
    ))
}

/// `lon_lat_deg` and `ellipsoidal_m` (the same length and order as the mesh's
/// positions) placed into the local ENU frame anchored at `origin`, via
/// `gungnir_coord::Wgs84` -- the same oracle-verified tangent-plane transform every other
/// geodetic thing in the system already goes through, rather than a second implementation
/// of the same math here.
///
/// **All three components are kept** (GAP-108, D-122): the up is what the vertex's height
/// is in the picture, and it differs from the file's height by the origin's own altitude,
/// the geoid separation when the file's was an EGM2008 height, and the Earth's curvature
/// away from the origin. A no-data vertex's non-finite height is read as `0.0` for placing
/// it horizontally only; `TerrainMesh::with_enu` keeps its up `NaN`, so the hole stays.
fn to_local_enu(
    lon_lat_deg: &[[f64; 2]],
    ellipsoidal_m: &[f64],
    origin: [f64; 3],
) -> Vec<[f64; 3]> {
    let origin = gungnir_coord::Geodetic {
        lat_rad: origin[0],
        lon_rad: origin[1],
        alt_m: origin[2],
    };
    lon_lat_deg
        .iter()
        .zip(ellipsoidal_m)
        .map(|(&[lon_deg, lat_deg], &h)| {
            let alt_m = if h.is_finite() { h } else { 0.0 };
            let point = gungnir_coord::Geodetic {
                lat_rad: lat_deg.to_radians(),
                lon_rad: lon_deg.to_radians(),
                alt_m,
            };
            let enu = gungnir_coord::Wgs84::ecef_to_enu(
                gungnir_coord::Wgs84::geodetic_to_ecef(point),
                origin,
            );
            [enu.e_m, enu.n_m, enu.u_m]
        })
        .collect()
}
