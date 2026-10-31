// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Ground placed in the deployment's local ENU frame, and line of sight's field built from a
//! staged store (GAP-023, D-41; GAP-108, GAP-197, GAP-200; GAP-227,
//! `docs/design/DN-36-staged-terrain-and-map-tiles.md` §5.2 and §9).
//!
//! **One path for both binaries.** This was `gungnir-app`'s own until the node came to read
//! the same pinned store for its line of sight (DN-36 §9). It moved here, unchanged, rather
//! than being copied, so a desktop and a node over one baseline place one tile the same way
//! and mask the same ground. What the baseline declares arrives already read into
//! `gungnir-data`'s types ([`Declared`]), because this crate has no edge to
//! `gungnir-config`; each binary reads its own baseline's strings into it.
//!
//! **Placement.** A DEM arrives in its own frame (a projected CRS, or geographic degrees).
//! [`place`] reconciles the declared frame against the file's own tags (`GridCrs`) and
//! converts through `gungnir_data::nad83::positions_to_wgs84` (behind the `crs` feature;
//! PROJ) and then `gungnir_coord::Wgs84` (always available) using the deployment's declared
//! origin. A file whose tags contradict the declaration, or a real-world CRS this build was
//! not compiled to convert, is refused by name. A converted height is a real height
//! (D-121, D-122): the DEM's own height is made a WGS-84 ellipsoidal one through the
//! verified geoid grids and becomes each vertex's local ENU up.
//!
//! **Line of sight over a store** ([`store_sight`]): read from the pinned store alone, over
//! the coverage extent, never a view and never a fill tile (DN-36 §5.2, D-164).

use gungnir_coord::CoordTransform;
use gungnir_data::geoid::{GeoidGrid, PinnedGrid, VerticalDatum};
use gungnir_data::geospatial::{GridCrs, TerrainMesh};
use gungnir_data::nad83::Realisation;
use gungnir_data::terrain_store::{StoreFootprint, StoreIndex, TerrainStore, TileKey};

use crate::terrain::HeightField;

/// The verified grid for a pinned geoid model or NADCON5 step, or why there is none: the
/// desktop's start-up checks, or the node's own.
pub type Grids<'a> = &'a dyn Fn(PinnedGrid) -> Result<GeoidGrid, String>;

/// What the baseline declares about a DEM or a store's tiles, read into `gungnir-data`'s
/// types. The vertical system and the realisation are kept as read, a refusal included,
/// because a `"local-enu"` DEM never reads them and so is never refused for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// `terrain.frame`: `None` for the deployment's local frame, or the EPSG code it names.
    pub frame_epsg: Option<u32>,
    /// `terrain.vertical`, or why it does not read.
    pub vertical: Result<Option<VerticalDatum>, String>,
    /// `terrain.nad83_realisation`, or why it does not read.
    pub nad83_realisation: Result<Option<Realisation>, String>,
}

impl Default for Declared {
    /// `"local-enu"` and nothing else: what `TerrainMesh::placed` always did.
    fn default() -> Self {
        Declared {
            frame_epsg: None,
            vertical: Ok(None),
            nad83_realisation: Ok(None),
        }
    }
}

/// WGS84 geographic, EPSG:4326 -- `gungnir_data::geospatial::crs::WGS84_EPSG` restated
/// so this module's unconditional code does not need the `crs` feature just
/// to name the constant.
const WGS84_EPSG: u32 = 4326;

/// Turn a loaded mesh into one placed in the deployment's local ENU frame, or say why
/// not (GAP-023, D-41). `origin` is the baseline's `origin`, the same geodetic anchor every
/// other local-ENU thing in the picture is relative to.
///
/// The file's own tags (`mesh.crs`) are read regardless of what `declared` says: a
/// `"local-enu"` declaration contradicted by a file that states a real CRS is refused; a
/// declared real-world CRS is cross-checked against the file's own tags when it has any
/// (a `GeoTIFF`) and trusted outright when it does not (an ESRI ASCII grid, which never
/// carries CRS tags at all -- `frame` is the only place its CRS could ever come from).
///
/// **The height, since GAP-108** (D-121, D-122): `declared.vertical` and the file's own
/// `VerticalGeoKey` decide how the DEM's heights become WGS-84 ellipsoidal ones
/// (`gungnir_data::geoid::height_reference`), refusing by name what cannot be converted.
/// That is decided before any projection runs, so a refusal needs no PROJ. Each vertex then
/// lands at its full local ENU position, up included. A `"local-enu"` DEM is untouched: its
/// heights are already the deployment's own up.
///
/// **And the frame, since GAP-200** (D-127, D-128): a NAD83 DEM is carried to WGS 84 by
/// its realisation -- the file's, or `declared.nad83_realisation` for an unqualified NAD83
/// file -- and `epoch`, and a NAVD88 height takes the Helmert's height difference at
/// `epoch` ([`gungnir_data::nad83::FrameConversion`]).
///
/// # Errors
///
/// Every refusal above, by name.
pub fn place(
    mesh: TerrainMesh,
    declared: &Declared,
    origin: Option<[f64; 3]>,
    epoch: f64,
    grids: Grids<'_>,
) -> Result<TerrainMesh, String> {
    let Some(source_epsg) = source_epsg(declared.frame_epsg, mesh.crs)? else {
        return Ok(mesh.placed());
    };
    let Some(origin) = origin else {
        return Err(format!(
            "the terrain declares a real-world CRS (EPSG:{source_epsg}) but the deployment has \
             no declared local ENU origin (config.origin) to place it against"
        ));
    };
    let baseline_vertical = declared.vertical.clone()?;
    let heights = gungnir_data::geoid::height_reference(
        "terrain.vertical",
        mesh.vertical_datum().as_ref(),
        baseline_vertical.as_ref(),
        &|model| grids(model.into()),
    )?;
    let conversion = frame_conversion(
        source_epsg,
        declared.nad83_realisation.clone()?,
        epoch,
        grids,
    )?;
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

/// One tile of `store` read, verified against its manifest entry, meshed and placed
/// ([`place`]).
///
/// # Errors
///
/// The tile's refusal by its digest or its decoding, or its placement's, by name.
pub fn place_store_tile(
    store: &TerrainStore,
    key: TileKey,
    declared: &Declared,
    origin: Option<[f64; 3]>,
    epoch: f64,
    grids: Grids<'_>,
) -> Result<TerrainMesh, String> {
    let grid = store.read_tile(key).map_err(|e| e.reason.to_string())?;
    let mesh = TerrainMesh::from_grid(&grid).map_err(|e| e.to_string())?;
    place(mesh, declared, origin, epoch, grids).map_err(|reason| format!("not placed: {reason}"))
}

/// A store's level-0 tile corners placed into local ENU (DN-36 amendment 4 item 1). The
/// corners carry no height, so their vertical system is declared ellipsoidal for this
/// placement only: a missing height is placed horizontally and stays missing.
///
/// # Errors
///
/// When the corners cannot be placed, or do not span an area.
pub fn place_footprint(
    index: &StoreIndex,
    declared: &Declared,
    origin: Option<[f64; 3]>,
    epoch: f64,
    grids: Grids<'_>,
    revision: u64,
) -> Result<StoreFootprint, String> {
    let (columns, rows, lattice) = StoreFootprint::lattice(index);
    let first = lattice.first().copied().unwrap_or([0.0, 0.0]);
    let mesh = TerrainMesh {
        positions: lattice
            .iter()
            .map(|p| {
                #[allow(clippy::cast_possible_truncation)]
                let offset = |v: f64, o: f64| (v - o) as f32;
                [offset(p[0], first[0]), offset(p[1], first[1]), f32::NAN]
            })
            .collect(),
        indices: Vec::new(),
        origin: first,
        crs: GridCrs::Unstated,
        vertical: None,
        rows,
        columns,
    };
    let corners_declared = Declared {
        vertical: Ok(Some(VerticalDatum::Ellipsoidal)),
        ..declared.clone()
    };
    let placed = place(mesh, &corners_declared, origin, epoch, grids)
        .map_err(|reason| format!("its tile corners could not be placed: {reason}"))?;
    let corners: Vec<[f64; 2]> = placed
        .positions
        .iter()
        .map(|p| [f64::from(p[0]), f64::from(p[1])])
        .collect();
    StoreFootprint::new(index.clone(), &corners, revision)
}

/// Line of sight's field painted from a pinned store, and what went into it.
#[derive(Debug, Clone)]
pub struct StoreSight {
    pub field: HeightField,
    /// The level it was read at.
    pub level: u8,
    /// The tiles painted into it.
    pub tiles: usize,
    /// The tiles refused, by their digest or their placement: flat where they lie.
    pub refused: usize,
    /// Whether the extent reaches past the store's edge: flat there too.
    pub past_edge: bool,
}

/// Line of sight's field over `extent` (DN-36 §5.2, amendment 2): from the pinned store
/// alone, at the finest level whose cells keep the field within
/// [`crate::HEIGHT_FIELD_CELL_BUDGET`], each tile of that level under the extent placed by
/// `place_tile`, painted in and dropped. A refused tile, and the extent past the store's
/// edge, stay no data: flat.
///
/// # Errors
///
/// When no staged tile lies under the extent, or the field cannot be made.
pub fn store_sight(
    footprint: &StoreFootprint,
    extent: [[f64; 2]; 2],
    place_tile: &dyn Fn(TileKey) -> Result<TerrainMesh, String>,
) -> Result<StoreSight, String> {
    let index = footprint.index();
    let [[e0, n0], [e1, n1]] = extent;
    let overlaps = |q: [[f64; 2]; 2]| q[0][0] < e1 && q[1][0] > e0 && q[0][1] < n1 && q[1][1] > n0;
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
    let budget = crate::HEIGHT_FIELD_CELL_BUDGET;
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
        let (cell, _) = crate::terrain::fit_cell(e1 - e0, n1 - n0, cell_of(level), budget);
        (level, cell)
    });
    if !cell.is_finite() {
        return Err("no staged tile lies under the coverage extent".to_string());
    }
    let mut field = HeightField::blank(extent, cell).map_err(|e| e.to_string())?;
    let (mut tiles, mut refused) = (0, 0);
    for key in held(level) {
        match place_tile(key) {
            Ok(mesh) if field.paint(&mesh).is_ok() => tiles += 1,
            _ => refused += 1,
        }
    }
    let [[s0, t0], [s1, t1]] = footprint.extent();
    let past_edge = e0 < s0 || n0 < t0 || e1 > s1 || n1 > t1;
    Ok(StoreSight {
        field,
        level,
        tiles,
        refused,
        past_edge,
    })
}

/// How the DEM's positions and heights reach WGS 84 beyond PROJ's own conversion
/// (GAP-200): a WGS 84 geographic file is not NAD83's and needs no PROJ to say so; any
/// other has its datum read through PROJ, and a build without `crs` refuses it by name,
/// as it refuses converting it.
fn frame_conversion(
    source_epsg: u32,
    declared: Option<Realisation>,
    epoch: f64,
    grids: Grids<'_>,
) -> Result<gungnir_data::nad83::FrameConversion, String> {
    const FIELD: &str = "terrain.nad83_realisation";
    let file = if source_epsg == WGS84_EPSG {
        gungnir_data::nad83::HorizontalDatum::NotNad83
    } else if cfg!(feature = "crs") {
        file_datum(&format!("EPSG:{source_epsg}"))?
    } else {
        // This build cannot convert the file at all, and `to_wgs84` says so by name; a
        // datum it could not read anyway is not the refusal to lead with.
        return gungnir_data::nad83::FrameConversion::not_nad83(epoch);
    };
    gungnir_data::nad83::FrameConversion::resolve(FIELD, file, declared, epoch, &|step| {
        grids(step.into())
    })
}

/// What a file's horizontal system says its datum is, read through PROJ from `source`
/// (`"EPSG:<code>"` or a WKT).
///
/// # Errors
///
/// PROJ cannot read `source`.
#[cfg(feature = "crs")]
pub fn file_datum(source: &str) -> Result<gungnir_data::nad83::HorizontalDatum, String> {
    gungnir_data::nad83::horizontal_datum(source).map_err(|e| e.to_string())
}

/// The same in a build without the `crs` feature: refused by name, since reading a
/// datum needs PROJ, and so does converting a file that has one.
///
/// # Errors
///
/// Always, naming the feature.
#[cfg(not(feature = "crs"))]
pub fn file_datum(source: &str) -> Result<gungnir_data::nad83::HorizontalDatum, String> {
    Err(format!(
        "reading the horizontal datum of {source} and converting from it needs the crs \
         feature, which this build was not compiled with"
    ))
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

/// What EPSG code (if any) a loaded mesh should be reprojected from, reconciling the
/// declared frame (`None` for the local frame) against the file's own tags. `Ok(None)`
/// means the file is already in the deployment's local frame (nothing to convert);
/// `Ok(Some(code))` names the real-world CRS to convert from; `Err` names a contradiction
/// between what the baseline declares and what the file actually states, refused by name
/// rather than guessed at.
fn source_epsg(frame_epsg: Option<u32>, crs: GridCrs) -> Result<Option<u32>, String> {
    match (frame_epsg, file_crs(crs)) {
        (None, FileCrs::NotStated) => Ok(None),
        (None, FileCrs::Known(code)) => Err(format!(
            "the file declares a real-world CRS (EPSG:{code}), not the local frame the baseline \
             claims; declare terrain.frame \"epsg:{code}\" instead if that is correct"
        )),
        (None, FileCrs::Ambiguous) => Err(
            "the file declares a geographic frame with no EPSG code, not local metres".to_string(),
        ),
        (Some(declared), FileCrs::NotStated) => Ok(Some(declared)),
        (Some(declared), FileCrs::Known(code)) if code == declared => Ok(Some(declared)),
        (Some(declared), FileCrs::Known(code)) => Err(format!(
            "terrain.frame declares EPSG:{declared} but the file's own tags state EPSG:{code}"
        )),
        (Some(declared), FileCrs::Ambiguous) => Err(format!(
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
