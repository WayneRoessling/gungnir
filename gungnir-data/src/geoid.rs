// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The vertical datum of a converted height, and the EGM2008 geoid grid that turns a
//! height above the geoid into one above the WGS-84 ellipsoid (GAP-108, D-121). The
//! design is `docs/rust-3d-data-ecosystem-build-vs-adopt.md` §5.
//!
//! **What a converted height has to be.** Everything geodetic on the picture -- a track's
//! altitude, a radar's site, the deployment's own origin -- is a WGS-84 ellipsoidal
//! height, and `gungnir_coord`'s ENU transform reads it as one. A DEM or a point cloud
//! states its heights in its own vertical datum, and for most real data that is a
//! gravity-related one: a height above a geoid, which sits tens of metres off the
//! ellipsoid over much of the world (-22.6 m at the Autzen fixture in Oregon, +34.9 m at
//! the Baltic fixture). Before this module the file's number was used as it stood, so a
//! converted surface sat that far off in the vertical, consistently and silently.
//!
//! **What converts, and what is refused by name** (D-121, the owner's decision of
//! 2026-09-26):
//!
//! * A height the file or the baseline states as **WGS-84 ellipsoidal** is already what
//!   the picture wants, and passes through unchanged.
//! * A height stated as **EGM2008 height** (EPSG:3855) gets the EGM2008 undulation added,
//!   read from NGA's 2.5-arc-minute grid through PROJ (`vgridshift`), which is accurate to
//!   the centimetre level the grid itself is.
//! * **Every other vertical datum is refused by name** -- NAVD88, EGM96, a national
//!   levelling datum -- because this deployment carries no grid for it, and applying the
//!   EGM2008 separation to a height that is not an EGM2008 height would be the silent
//!   ellipsoid/geoid mix this exists to end, only smaller. So is a height whose datum
//!   neither the file nor the baseline states.
//!
//! **The grid is a deployment artifact, never a repository file.** It is about 80 MB,
//! GitHub refuses files over 100 MB and the repository should not carry either. It is
//! PROJ-data's own `GeoTIFF` of NGA's grid ([`EGM2008_GRID_URL`]), pinned here by the
//! SHA-256 PROJ-data publishes for it ([`EGM2008_GRID_SHA256`]); a [`GeoidGrid`] can only
//! be made by hashing a file against an expected digest, and the conversion takes a
//! `GeoidGrid`, so an unverified file never reaches PROJ. `deploy/README.md` says where
//! a deployment puts it, and `deploy/geoid/SHA256SUMS` carries the same digest for the
//! install step and for CI; a test holds the two equal.
//!
//! **Why the file is named by its absolute path rather than found on PROJ's search
//! path.** PROJ looks for a bare grid name along `PROJ_DATA` and its own defaults, first
//! match wins, so a stale copy earlier on that path would be read instead of the file
//! that was verified. Handing PROJ the verified file's absolute path is the only way the
//! bytes that were hashed are the bytes that are read.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::DataError;

/// The grid's file name, as PROJ-data distributes it and as a deployment installs it.
pub const EGM2008_GRID_FILE: &str = "us_nga_egm08_25.tif";

/// Where PROJ-data distributes it: the official PROJ content delivery network.
pub const EGM2008_GRID_URL: &str = "https://cdn.proj.org/us_nga_egm08_25.tif";

/// The SHA-256 PROJ-data publishes for that file in its own index
/// (`https://cdn.proj.org/files.geojson`, the `sha256sum` of `us_nga_egm08_25.tif`), and
/// the digest of the copy downloaded for GAP-108 on 2026-09-26. `deploy/geoid/SHA256SUMS`
/// carries the same value; `the_pinned_digest_is_the_one_the_deployment_manifest_carries`
/// holds the two equal.
pub const EGM2008_GRID_SHA256: &str =
    "4191d471eefebf24091b56dbc604353cb3b8cf8cc70e448bb9ae56a272bef17a";

/// The pinned file's size, in bytes, for the install step's own sanity check.
pub const EGM2008_GRID_BYTES: u64 = 80_585_622;

/// EPSG:3855, "EGM2008 height": the vertical system the grid converts from.
pub const EPSG_EGM2008_HEIGHT: u32 = 3855;
/// EPSG:1027, the EGM2008 geoid, as a WKT `VERT_DATUM` names it.
pub const EPSG_EGM2008_GEOID: u32 = 1027;
/// EPSG:4979, WGS 84 geographic 3D, whose third axis is the ellipsoidal height.
pub const EPSG_WGS84_3D: u32 = 4979;
/// `GeoTIFF` 1.0's `VertCS_WGS_84_ellipsoid` (OGC `GeoTIFF` 1.1, `VerticalGeoKey`'s
/// ellipsoid-based codes 5001-5033): heights above the WGS-84 ellipsoid.
pub const GEOTIFF_WGS84_ELLIPSOID: u32 = 5030;
/// `GeoTIFF`'s "user-defined" code: the file says it has a vertical system and not which.
const GEOTIFF_USER_DEFINED: u32 = 32_767;

/// What a file or a baseline says a height is measured from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerticalDatum {
    /// Above the WGS-84 ellipsoid: what the picture wants already.
    Ellipsoidal,
    /// EGM2008 height (EPSG:3855): above the EGM2008 geoid.
    Egm2008,
    /// Anything else, kept by what it was declared as so a refusal can name it.
    Other { epsg: Option<u32>, name: String },
}

impl VerticalDatum {
    /// The datum an EPSG code (or a `GeoTIFF` `VerticalGeoKey` value) names.
    ///
    /// 4979 and 5030 are both "WGS-84 ellipsoidal height": the first is the geographic
    /// 3D CRS whose third axis it is, the second `GeoTIFF` 1.0's own code for it.
    #[must_use]
    pub fn from_epsg(code: u32) -> Self {
        match code {
            EPSG_EGM2008_HEIGHT => VerticalDatum::Egm2008,
            EPSG_WGS84_3D | GEOTIFF_WGS84_ELLIPSOID => VerticalDatum::Ellipsoidal,
            GEOTIFF_USER_DEFINED => VerticalDatum::Other {
                epsg: None,
                name: "a user-defined vertical system (GeoTIFF code 32767)".to_string(),
            },
            code => VerticalDatum::Other {
                epsg: Some(code),
                name: format!("EPSG:{code}"),
            },
        }
    }

    /// Whether two declarations name the same datum. Two `Other`s agree when both carry
    /// the same code, or, lacking codes, the same name.
    #[must_use]
    pub fn agrees_with(&self, other: &VerticalDatum) -> bool {
        match (self, other) {
            (VerticalDatum::Ellipsoidal, VerticalDatum::Ellipsoidal)
            | (VerticalDatum::Egm2008, VerticalDatum::Egm2008) => true,
            (
                VerticalDatum::Other { epsg: Some(a), .. },
                VerticalDatum::Other { epsg: Some(b), .. },
            ) => a == b,
            (VerticalDatum::Other { name: a, .. }, VerticalDatum::Other { name: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Display for VerticalDatum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerticalDatum::Ellipsoidal => f.write_str("WGS-84 ellipsoidal height"),
            VerticalDatum::Egm2008 => f.write_str("EGM2008 height (EPSG:3855)"),
            VerticalDatum::Other {
                epsg: Some(code),
                name,
            } if !name.contains(&code.to_string()) => write!(f, "{name} (EPSG:{code})"),
            VerticalDatum::Other { name, .. } => f.write_str(name),
        }
    }
}

/// Why a grid file is not a [`GeoidGrid`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GridError {
    #[error("{} is not there", .path.display())]
    Missing { path: PathBuf },
    #[error(
        "{} is not the pinned grid: its SHA-256 is {found}, and the pinned one is {expected}",
        .path.display()
    )]
    Mismatch {
        path: PathBuf,
        found: String,
        expected: String,
    },
    #[error("{} could not be read: {reason}", .path.display())]
    Unreadable { path: PathBuf, reason: String },
    /// A path PROJ's grid list cannot carry: `vgridshift` splits its `grids` value on
    /// commas and the definition string is quoted with double quotes, so a path holding
    /// either would name a different file or none.
    #[error("{} cannot be handed to PROJ: {reason}", .path.display())]
    UnusablePath { path: PathBuf, reason: String },
}

/// A geoid grid file whose bytes were hashed and found to be the ones expected.
///
/// The fields are private and the only constructor is [`GeoidGrid::verify`], so holding
/// one is proof the check ran. Verified once, at start or when the file is named; the
/// conversion itself does not hash again, which would cost the whole file per load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoidGrid {
    path: PathBuf,
    sha256: String,
}

impl GeoidGrid {
    /// Hash `path` and accept it only if its SHA-256 is `expected_sha256` (lowercase or
    /// uppercase hexadecimal).
    ///
    /// # Errors
    ///
    /// [`GridError::Missing`] when there is no file, [`GridError::Unreadable`] when it
    /// cannot be read through, [`GridError::Mismatch`] when its digest differs, and
    /// [`GridError::UnusablePath`] when its absolute path could not be put in front of
    /// PROJ unchanged.
    pub fn verify(path: &Path, expected_sha256: &str) -> Result<GeoidGrid, GridError> {
        let absolute = std::path::absolute(path).map_err(|e| GridError::Unreadable {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        let Some(text) = absolute.to_str() else {
            return Err(GridError::UnusablePath {
                path: absolute,
                reason: "it is not valid UTF-8".to_string(),
            });
        };
        if let Some(c) = text.chars().find(|c| matches!(c, '"' | ',')) {
            return Err(GridError::UnusablePath {
                path: absolute.clone(),
                reason: format!("it contains {c:?}"),
            });
        }
        let file = match std::fs::File::open(&absolute) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(GridError::Missing { path: absolute });
            }
            Err(e) => {
                return Err(GridError::Unreadable {
                    path: absolute,
                    reason: e.to_string(),
                })
            }
        };
        let found = sha256_hex(file).map_err(|e| GridError::Unreadable {
            path: absolute.clone(),
            reason: e.to_string(),
        })?;
        if !found.eq_ignore_ascii_case(expected_sha256) {
            return Err(GridError::Mismatch {
                path: absolute,
                found,
                expected: expected_sha256.to_ascii_lowercase(),
            });
        }
        Ok(GeoidGrid {
            path: absolute,
            sha256: found,
        })
    }

    /// The pinned EGM2008 grid in `dir`: [`EGM2008_GRID_FILE`] there, verified against
    /// [`EGM2008_GRID_SHA256`].
    ///
    /// # Errors
    ///
    /// As [`GeoidGrid::verify`].
    pub fn verify_pinned_in(dir: &Path) -> Result<GeoidGrid, GridError> {
        GeoidGrid::verify(&dir.join(EGM2008_GRID_FILE), EGM2008_GRID_SHA256)
    }

    /// The verified file, absolute.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its SHA-256, lowercase hexadecimal.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// The SHA-256 of everything `reader` yields, lowercase hexadecimal, read in 1 MiB
/// pieces so an 80 MB grid is never held whole.
fn sha256_hex(mut reader: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing to a `String` cannot fail; the `Result` is `fmt`'s signature only.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// How the heights of one file become WGS-84 ellipsoidal heights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeightReference {
    /// They already are; nothing is added.
    Ellipsoidal,
    /// They are EGM2008 heights; the undulation this verified grid gives is added.
    Egm2008(GeoidGrid),
}

/// Decide how a file's heights become ellipsoidal ones, or say why they cannot (D-121).
///
/// `field` is the baseline field an operator would edit (`"terrain.vertical"`,
/// `"point_cloud.vertical"`), so a refusal names it. `file` is what the file itself
/// states; `baseline` is what that field declares. `grid` is the deployment's verified
/// EGM2008 grid, or the reason it has none, which a refusal carries.
///
/// **The file wins where both speak, and a disagreement is refused rather than
/// resolved**, the same rule a declared horizontal frame already follows: the file's own
/// tags are what its numbers actually are. A baseline declaration is what lets a file
/// that states nothing -- most `GeoTIFF` DEMs and many LAS files -- be converted at all.
///
/// # Errors
///
/// The refusal, in words an operator reads on PN-09 and in the alert.
pub fn height_reference(
    field: &str,
    file: Option<&VerticalDatum>,
    baseline: Option<&VerticalDatum>,
    grid: Result<&GeoidGrid, &str>,
) -> Result<HeightReference, String> {
    let datum = match (file, baseline) {
        (Some(file), Some(declared)) if !file.agrees_with(declared) => {
            return Err(format!(
                "{field} declares {declared} and the file states {file}; the file's own \
                 declaration is what its heights are, so this is a baseline to correct"
            ))
        }
        (Some(datum), _) | (None, Some(datum)) => datum,
        (None, None) => {
            return Err(format!(
                "the file does not state the vertical datum of its heights and {field} \
                 declares none; declare \"ellipsoidal\" (WGS-84 ellipsoidal heights) or \
                 \"epsg:3855\" (EGM2008 heights) -- a height is never converted from a \
                 datum nobody stated"
            ))
        }
    };
    match datum {
        VerticalDatum::Ellipsoidal => Ok(HeightReference::Ellipsoidal),
        VerticalDatum::Egm2008 => match grid {
            Ok(grid) => Ok(HeightReference::Egm2008(grid.clone())),
            Err(reason) => Err(format!(
                "the heights are EGM2008 heights and need the EGM2008 geoid grid \
                 ({EGM2008_GRID_FILE}), which this deployment does not have verified: \
                 {reason}"
            )),
        },
        other @ VerticalDatum::Other { .. } => Err(format!(
            "the heights are in {other}, and this deployment carries no geoid grid for \
             it; only WGS-84 ellipsoidal heights and EGM2008 heights (EPSG:3855) are \
             converted, so reproject the file's heights to one of those"
        )),
    }
}

/// `heights_m` (one per `lon_lat_deg`, in metres, `NaN` for a point with none) as WGS-84
/// ellipsoidal heights under `reference`.
///
/// # Errors
///
/// Under [`HeightReference::Egm2008`]: whatever [`undulations`] refuses, and, in a build
/// without the `crs` feature, `DataError::NotImplemented` naming that feature, since the
/// grid is read through PROJ.
pub fn ellipsoidal_heights(
    reference: &HeightReference,
    lon_lat_deg: &[[f64; 2]],
    heights_m: &[f64],
) -> Result<Vec<f64>, DataError> {
    if lon_lat_deg.len() != heights_m.len() {
        return Err(DataError::Parse(format!(
            "{} heights for {} positions",
            heights_m.len(),
            lon_lat_deg.len()
        )));
    }
    match reference {
        HeightReference::Ellipsoidal => Ok(heights_m.to_vec()),
        HeightReference::Egm2008(grid) => egm2008_to_ellipsoidal(grid, lon_lat_deg, heights_m),
    }
}

#[cfg(feature = "crs")]
fn egm2008_to_ellipsoidal(
    grid: &GeoidGrid,
    lon_lat_deg: &[[f64; 2]],
    heights_m: &[f64],
) -> Result<Vec<f64>, DataError> {
    let n = undulations(grid, lon_lat_deg)?;
    Ok(heights_m.iter().zip(n).map(|(h, n)| h + n).collect())
}

#[cfg(not(feature = "crs"))]
fn egm2008_to_ellipsoidal(
    _grid: &GeoidGrid,
    _lon_lat_deg: &[[f64; 2]],
    _heights_m: &[f64],
) -> Result<Vec<f64>, DataError> {
    Err(DataError::NotImplemented {
        what: "adding the EGM2008 geoid undulation to a height",
        waiting_on: "a build with gungnir-data's `crs` feature, which reads the grid \
                     through libproj",
    })
}

/// The EGM2008 geoid undulation N, in metres, at each `[longitude, latitude]` (degrees,
/// WGS 84), read from `grid` by PROJ: an EGM2008 height H is the ellipsoidal height
/// H + N.
///
/// **One PROJ pipeline, stated in full rather than chosen by PROJ.** Asked for
/// "EGM2008 height to EPSG:4979", PROJ offers `vgridshift` with this grid *and*, when the
/// grid is not found, a "ballpark" no-op that returns the height unchanged -- exactly the
/// silent mix D-121 refuses. So the operation is written out: degrees to radians,
/// `vgridshift` on the verified file with `multiplier=1` (H + N, the direction PROJ's own
/// "Inverse of WGS 84 to EGM2008 height (1)" uses), radians back to degrees. A missing or
/// unreadable file then fails to build, and a point off the grid fails to convert,
/// rather than either quietly becoming zero.
///
/// **Why the last step swaps two axes.** `proj` 0.31's `Proj::convert` hands PROJ a `z`
/// of zero and returns only `x` and `y` (the limitation `pointcloud::crs::to_local_enu`
/// already names), so the height a `vgridshift` produces would be discarded. Fed a zero
/// height, `vgridshift`'s output height *is* N, and `axisswap order=1,3,2` moves it into
/// the second slot, where `convert` returns it. This is PROJ doing the whole of the
/// geodesy -- the swap only routes its answer past the binding.
///
/// # Errors
///
/// `DataError::Parse` when PROJ cannot open the grid, or a position is not finite, or
/// falls outside the grid (possible only for a clipped test grid; the pinned one is
/// global), or comes back non-finite.
#[cfg(feature = "crs")]
pub fn undulations(grid: &GeoidGrid, lon_lat_deg: &[[f64; 2]]) -> Result<Vec<f64>, DataError> {
    let pipeline = egm2008_pipeline(grid.path())?;
    lon_lat_deg
        .iter()
        .map(|&[lon, lat]| undulation_at(&pipeline, grid.path(), lon, lat))
        .collect()
}

/// The written-out `vgridshift` pipeline over the verified file ([`undulations`] says
/// why each step is there).
#[cfg(feature = "crs")]
fn egm2008_pipeline(path: &Path) -> Result<proj::Proj, DataError> {
    let path = path.display();
    let definition = format!(
        "+proj=pipeline \
         +step +proj=unitconvert +xy_in=deg +xy_out=rad \
         +step +proj=vgridshift +grids=\"{path}\" +multiplier=1 \
         +step +proj=unitconvert +xy_in=rad +xy_out=deg \
         +step +proj=axisswap +order=1,3,2"
    );
    proj::Proj::new(&definition).map_err(|e| {
        DataError::Parse(format!(
            "PROJ cannot read the geoid grid {path} for vgridshift: {e}"
        ))
    })
}

/// N at one position through `pipeline`, refused rather than zero when there is none.
#[cfg(feature = "crs")]
fn undulation_at(pipeline: &proj::Proj, path: &Path, lon: f64, lat: f64) -> Result<f64, DataError> {
    let path = path.display();
    if !(lon.is_finite() && lat.is_finite()) {
        return Err(DataError::Parse(format!(
            "position ({lon}, {lat}) is not finite, so it has no geoid undulation"
        )));
    }
    let (_, n) = pipeline.convert((lon, lat)).map_err(|e| {
        DataError::Parse(format!(
            "no EGM2008 undulation at longitude {lon}, latitude {lat} in {path}: {e}"
        ))
    })?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(DataError::Parse(format!(
            "no EGM2008 undulation at longitude {lon}, latitude {lat} in {path}: \
             PROJ returned {n}"
        )))
    }
}

/// How long [`UndulationService::undulation`] waits for its answer before refusing. A
/// lookup is microseconds of PROJ; the wait only matters if the service thread is gone
/// or wedged, and then a refusal -- a height left flagged -- is the right outcome, not a
/// feed that stops.
pub const UNDULATION_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// One question to the service thread: N at a longitude and latitude, and where to
/// answer.
#[cfg(feature = "crs")]
struct Lookup {
    lon: f64,
    lat: f64,
    reply: crossbeam_channel::Sender<Result<f64, String>>,
}

/// The EGM2008 undulation at one position at a time, for a live feed (GAP-196, D-123):
/// a Category 129 report's height above mean sea level becomes an ellipsoidal one by
/// adding it.
///
/// **Why a thread of its own.** [`undulations`] builds its PROJ pipeline per call, which
/// opens the grid each time: right for a file converted once, wrong for a feed asking
/// once per report. So the pipeline is built once. But `proj` 0.31's `Proj` is neither
/// `Send` nor `Sync` (it holds PROJ's raw context pointers), and the ingest adapter
/// that asks is owned by the gateway and must be `Send`. So one thread owns the
/// pipeline for its whole life and answers lookups over a channel; the service is the
/// sending end, which is `Send + Sync` and cheap to clone. The thread ends when the last
/// clone is dropped.
///
/// Built only from a [`GeoidGrid`], so only a verified file is ever read, and through
/// the same written-out pipeline [`undulations`] uses: no ballpark fallback, and a
/// position off the grid is a refusal, never a zero.
#[derive(Debug, Clone)]
pub struct UndulationService {
    #[cfg(feature = "crs")]
    lookups: crossbeam_channel::Sender<Lookup>,
    /// A build without `crs` cannot make one at all: `start` refuses, and this field
    /// has no value to hold.
    #[cfg(not(feature = "crs"))]
    never: std::convert::Infallible,
    grid: GeoidGrid,
}

impl UndulationService {
    /// Build the pipeline over `grid` on a new thread, and return once PROJ has opened
    /// the file.
    ///
    /// # Errors
    ///
    /// `DataError::Parse` when PROJ cannot open the grid or the thread cannot start;
    /// in a build without the `crs` feature, `DataError::NotImplemented` naming it,
    /// since the grid is read through PROJ.
    #[cfg(feature = "crs")]
    pub fn start(grid: &GeoidGrid) -> Result<UndulationService, DataError> {
        let (lookups, questions) = crossbeam_channel::unbounded::<Lookup>();
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), DataError>>(1);
        let path = grid.path().to_path_buf();
        std::thread::Builder::new()
            .name("egm2008-undulation".to_string())
            .spawn(move || {
                let pipeline = match egm2008_pipeline(&path) {
                    Ok(pipeline) => {
                        let _ = ready_tx.send(Ok(()));
                        pipeline
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                for q in questions {
                    let answer =
                        undulation_at(&pipeline, &path, q.lon, q.lat).map_err(|e| e.to_string());
                    // The asker gave up (its timeout); nothing to tell.
                    let _ = q.reply.send(answer);
                }
            })
            .map_err(|e| {
                DataError::Parse(format!("the EGM2008 lookup thread did not start: {e}"))
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(UndulationService {
                lookups,
                grid: grid.clone(),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(DataError::Parse(
                "the EGM2008 lookup thread stopped before it opened the grid".to_string(),
            )),
        }
    }

    /// See the `crs` build's documentation: this build has no PROJ to read the grid.
    ///
    /// # Errors
    ///
    /// Always `DataError::NotImplemented`, naming the `crs` feature.
    #[cfg(not(feature = "crs"))]
    pub fn start(_grid: &GeoidGrid) -> Result<UndulationService, DataError> {
        Err(DataError::NotImplemented {
            what: "reading the EGM2008 geoid undulation for a live report's height",
            waiting_on: "a build with gungnir-data's `crs` feature, which reads the grid \
                         through libproj",
        })
    }

    /// The EGM2008 undulation N at `lon`, `lat` (degrees, WGS 84): a height H above the
    /// EGM2008 geoid is the ellipsoidal height H + N.
    ///
    /// # Errors
    ///
    /// `DataError::Parse` when the position is not finite or is off the grid, or the
    /// service thread did not answer within [`UNDULATION_LOOKUP_TIMEOUT`].
    #[cfg(feature = "crs")]
    pub fn undulation(&self, lon: f64, lat: f64) -> Result<f64, DataError> {
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.lookups
            .send(Lookup { lon, lat, reply })
            .map_err(|_| DataError::Parse("the EGM2008 lookup thread has stopped".to_string()))?;
        match answer.recv_timeout(UNDULATION_LOOKUP_TIMEOUT) {
            Ok(n) => n.map_err(DataError::Parse),
            Err(_) => Err(DataError::Parse(format!(
                "the EGM2008 lookup thread did not answer within {} ms",
                UNDULATION_LOOKUP_TIMEOUT.as_millis()
            ))),
        }
    }

    /// Uncallable in this build: no `UndulationService` can exist without `crs`.
    ///
    /// # Errors
    ///
    /// None; there is no value to call it on.
    #[cfg(not(feature = "crs"))]
    pub fn undulation(&self, _lon: f64, _lat: f64) -> Result<f64, DataError> {
        match self.never {}
    }

    /// The verified grid it reads.
    #[must_use]
    pub fn grid(&self) -> &GeoidGrid {
        &self.grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gungnir-geoid-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// The digest is the one `deploy/geoid/SHA256SUMS` carries for the install step and
    /// CI; two copies of a fact kept equal by a test rather than by care.
    #[test]
    fn the_pinned_digest_is_the_one_the_deployment_manifest_carries() {
        let manifest = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../deploy/geoid/SHA256SUMS"),
        )
        .expect("deploy/geoid/SHA256SUMS is committed");
        let line = manifest
            .lines()
            .find(|l| l.ends_with(EGM2008_GRID_FILE))
            .expect("the manifest names the grid");
        let digest = line.split_whitespace().next().expect("a digest");
        assert_eq!(digest, EGM2008_GRID_SHA256);
    }

    /// SHA-256 of "abc", FIPS 180-2 appendix B.1: the hasher is the standard one, read
    /// through the chunked loop.
    #[test]
    fn the_digest_is_standard_sha256() {
        assert_eq!(
            sha256_hex(&b"abc"[..]).expect("reads"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_file_with_the_expected_digest_is_a_grid_and_any_other_is_refused() {
        let dir = scratch("verify");
        let path = dir.join("grid.tif");
        std::fs::write(&path, b"abc").expect("writes");
        let grid = GeoidGrid::verify(
            &path,
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
        )
        .expect("the digest matches, whatever its case");
        assert!(grid.path().is_absolute());
        assert_eq!(
            grid.sha256(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        // One byte changed is a different file, and it is refused, naming both digests.
        std::fs::write(&path, b"abd").expect("writes");
        let err = GeoidGrid::verify(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .expect_err("a changed file is not the pinned one");
        assert!(matches!(err, GridError::Mismatch { .. }), "{err}");
        assert!(err.to_string().contains("ba7816bf"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_grid_is_refused_as_missing() {
        let dir = scratch("missing");
        let err = GeoidGrid::verify_pinned_in(&dir).expect_err("nothing there");
        assert!(matches!(err, GridError::Missing { .. }), "{err}");
        assert!(err.to_string().contains(EGM2008_GRID_FILE), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A comma would split PROJ's grid list and a double quote would end the quoted
    /// path, so either would have PROJ read something other than the verified file.
    #[test]
    fn a_path_proj_cannot_carry_is_refused_before_it_is_hashed() {
        for name in ["a,b.tif", "a\"b.tif"] {
            let err =
                GeoidGrid::verify(Path::new(name), EGM2008_GRID_SHA256).expect_err("unusable");
            assert!(
                matches!(err, GridError::UnusablePath { .. }),
                "{name}: {err}"
            );
        }
    }

    fn verified_stand_in() -> GeoidGrid {
        GeoidGrid {
            path: PathBuf::from("/grids/us_nga_egm08_25.tif"),
            sha256: EGM2008_GRID_SHA256.to_string(),
        }
    }

    /// **The ellipsoidal-already case passes through unchanged**, grid or none: there is
    /// nothing to add to a height that is already the picture's kind.
    #[test]
    // The heights are returned untouched, so equality is the property; an epsilon would
    // hide an addition.
    #[allow(clippy::float_cmp)]
    fn an_ellipsoidal_height_passes_through_unchanged() {
        let reference = height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::Ellipsoidal),
            None,
            Err("no grid is installed"),
        )
        .expect("an ellipsoidal height needs no grid");
        assert_eq!(reference, HeightReference::Ellipsoidal);
        let heights = [12.5, -3.25, f64::NAN];
        let out = ellipsoidal_heights(
            &reference,
            &[[15.0, 54.0], [15.1, 54.1], [15.2, 54.2]],
            &heights,
        )
        .expect("nothing to do");
        assert_eq!(out[..2], heights[..2]);
        assert!(out[2].is_nan(), "a point with no height keeps none");
    }

    #[test]
    fn an_egm2008_height_takes_the_verified_grid() {
        let grid = verified_stand_in();
        let reference = height_reference(
            "terrain.vertical",
            None,
            Some(&VerticalDatum::from_epsg(3855)),
            Ok(&grid),
        )
        .expect("a declared EGM2008 height with a verified grid converts");
        assert_eq!(reference, HeightReference::Egm2008(grid));
    }

    #[test]
    fn an_egm2008_height_without_a_verified_grid_is_refused_with_the_reason() {
        let err = height_reference(
            "point_cloud.vertical",
            Some(&VerticalDatum::Egm2008),
            None,
            Err("/grids/us_nga_egm08_25.tif is not there"),
        )
        .expect_err("no grid, no conversion");
        assert!(err.contains("EGM2008"), "{err}");
        assert!(err.contains("is not there"), "{err}");
    }

    /// **The refusal for a vertical datum with no grid, by name.** NAVD88 is the Autzen
    /// fixture's own; EGM96 is the one most likely to be mistaken for EGM2008, and is
    /// refused rather than approximated by it.
    #[test]
    fn a_vertical_datum_with_no_grid_is_refused_by_name() {
        let grid = verified_stand_in();
        for (datum, named) in [
            (
                VerticalDatum::Other {
                    epsg: Some(6360),
                    name: "NAVD88 height (ftUS)".to_string(),
                },
                "NAVD88 height (ftUS) (EPSG:6360)",
            ),
            (VerticalDatum::from_epsg(5773), "EPSG:5773"),
        ] {
            let err = height_reference("point_cloud.vertical", Some(&datum), None, Ok(&grid))
                .expect_err("no grid for it");
            assert!(err.contains(named), "{err}");
            assert!(err.contains("no geoid grid"), "{err}");
        }
    }

    #[test]
    fn a_height_nobody_states_is_refused_and_the_field_to_declare_is_named() {
        let err = height_reference("terrain.vertical", None, None, Err("unused"))
            .expect_err("nobody stated the datum");
        assert!(err.contains("terrain.vertical"), "{err}");
        assert!(err.contains("epsg:3855"), "{err}");
    }

    #[test]
    fn a_baseline_that_contradicts_the_file_is_refused_rather_than_obeyed() {
        let grid = verified_stand_in();
        let err = height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::Egm2008),
            Some(&VerticalDatum::Ellipsoidal),
            Ok(&grid),
        )
        .expect_err("the two disagree");
        assert!(err.contains("EGM2008"), "{err}");
        assert!(err.contains("ellipsoidal"), "{err}");
        // Agreement is not a contradiction.
        assert!(height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::Egm2008),
            Some(&VerticalDatum::from_epsg(3855)),
            Ok(&grid),
        )
        .is_ok());
    }

    #[test]
    fn epsg_codes_map_to_the_datums_they_name() {
        assert_eq!(VerticalDatum::from_epsg(3855), VerticalDatum::Egm2008);
        assert_eq!(VerticalDatum::from_epsg(4979), VerticalDatum::Ellipsoidal);
        assert_eq!(VerticalDatum::from_epsg(5030), VerticalDatum::Ellipsoidal);
        assert!(matches!(
            VerticalDatum::from_epsg(5703),
            VerticalDatum::Other {
                epsg: Some(5703),
                ..
            }
        ));
        assert!(matches!(
            VerticalDatum::from_epsg(32_767),
            VerticalDatum::Other { epsg: None, .. }
        ));
    }

    #[test]
    fn a_length_mismatch_is_an_error_rather_than_a_short_answer() {
        let err = ellipsoidal_heights(&HeightReference::Ellipsoidal, &[[0.0, 0.0]], &[])
            .expect_err("one position, no heights");
        assert!(err.to_string().contains("0 heights for 1"), "{err}");
    }
}
