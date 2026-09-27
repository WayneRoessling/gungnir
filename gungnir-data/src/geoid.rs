// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The vertical datum of a converted height, and the pinned geoid grids that turn a
//! height above a geoid into one above the ellipsoid (GAP-108 and D-121; GAP-197 and
//! D-125). The design is `docs/rust-3d-data-ecosystem-build-vs-adopt.md` §5.
//!
//! **What a converted height has to be.** Everything geodetic on the picture -- a track's
//! altitude, a radar's site, the deployment's own origin -- is a WGS-84 ellipsoidal
//! height, and `gungnir_coord`'s ENU transform reads it as one. A DEM or a point cloud
//! states its heights in its own vertical datum, and for most real data that is a
//! gravity-related one: a height above a geoid, which sits tens of metres off the
//! ellipsoid over much of the world (-23.3 m at the Autzen fixture in Oregon, +34.9 m at
//! the Baltic fixture). Before GAP-108 the file's number was used as it stood, so a
//! converted surface sat that far off in the vertical, consistently and silently.
//!
//! **What converts, and what is refused by name** (D-121, the owner's decision of
//! 2026-09-26; D-125, taken under his delegation the same day):
//!
//! * A height the file or the baseline states as **WGS-84 ellipsoidal** is already what
//!   the picture wants, and passes through unchanged.
//! * A height in a datum with a **pinned grid** ([`GeoidModel`]) gets that grid's
//!   undulation added, read through PROJ (`vgridshift`): an **EGM2008 height**
//!   (EPSG:3855) from NGA's 2.5-arc-minute EGM2008 grid; an **EGM96 height** (EPSG:5773)
//!   from NGA's 15-arc-minute EGM96 grid; a **NAVD88 height** (EPSG:5703, and its foot
//!   forms 6360 and 8228) from NOAA's GEOID18 grid, which covers the conterminous United
//!   States only.
//! * **Every other vertical datum is refused by name** -- a national levelling datum
//!   such as NN2000 or DHHN2016, PRVD02, and NAVD88 outside GEOID18's grid (Alaska and
//!   Hawaii, whose NOAA model is GEOID12B, not pinned) -- because this deployment
//!   carries no grid for it, and applying another datum's separation would be the silent
//!   ellipsoid/geoid mix this exists to end, only smaller. So is a height whose datum
//!   neither the file nor the baseline states. `deploy/README.md` ("Pinning a further
//!   geoid grid") is the procedure for admitting one more.
//!
//! **How accurate each chain is, stated rather than implied.** EGM2008 and EGM96 heights
//! are defined by their own models, and their grids take them to WGS-84 ellipsoidal
//! heights to within the grids' interpolation: centimetres, more in steep mountains for
//! the coarser EGM96 grid. **A NAVD88 height does not reach WGS 84 that well.** GEOID18
//! relates NAVD88 to **NAD83(2011)** ellipsoidal heights, at NOAA's stated centimetre
//! level; this workspace then reads that NAD83(2011) height as a WGS-84 one, with no
//! frame step, exactly as the horizontal conversion already reads a NAD83 latitude and
//! longitude as WGS-84 ones (PROJ's own choice for a 2D NAD83 source, and for the whole
//! NAVD88 chain when it runs it itself, is the null "NAD83(2011) to WGS 84 (1)"). The
//! two frames differ by about a metre or two horizontally and up to a metre vertically
//! across the United States: at the Autzen fixture, 1.4 m and -0.38 m at epoch 2010.0
//! (`pyproj`, "Inverse of ITRF2014 to NAD83(2011) (1)"). So a NAVD88 surface lands at
//! the metre level in WGS-84 terms -- tens of metres better than refusing it or reading
//! it as ellipsoidal, and not the centimetres the geoid model alone would suggest.
//!
//! **The grids are deployment artifacts, never repository files.** EGM2008's is about
//! 80 MB, GitHub refuses files over 100 MB and the repository should carry none of them
//! either way. Each is PROJ-data's own `GeoTIFF` from the PROJ CDN, pinned here by the
//! SHA-256 PROJ-data publishes for it; a [`GeoidGrid`] can only be made by hashing a file
//! against an expected digest, and the conversion takes a `GeoidGrid`, so an unverified
//! file never reaches PROJ. `deploy/README.md` says where a deployment puts them, and
//! `deploy/geoid/SHA256SUMS` carries the same digests for the install step and for CI; a
//! test holds the two equal.
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

/// The EGM2008 grid's file name, as PROJ-data distributes it and as a deployment
/// installs it.
pub const EGM2008_GRID_FILE: &str = "us_nga_egm08_25.tif";

/// Where PROJ-data distributes it: the official PROJ content delivery network.
pub const EGM2008_GRID_URL: &str = "https://cdn.proj.org/us_nga_egm08_25.tif";

/// The SHA-256 PROJ-data publishes for that file in its own index
/// (`https://cdn.proj.org/files.geojson`, the `sha256sum` of `us_nga_egm08_25.tif`), and
/// the digest of the copy downloaded for GAP-108 on 2026-09-26. `deploy/geoid/SHA256SUMS`
/// carries the same value; `the_pinned_digests_are_the_ones_the_deployment_manifest_carries`
/// holds the two equal.
pub const EGM2008_GRID_SHA256: &str =
    "4191d471eefebf24091b56dbc604353cb3b8cf8cc70e448bb9ae56a272bef17a";

/// The pinned EGM2008 file's size, in bytes, for the install step's own sanity check.
pub const EGM2008_GRID_BYTES: u64 = 80_585_622;

/// NGA's EGM96 geoid on a 15-arc-minute grid, as PROJ-data distributes it (GAP-197).
pub const EGM96_GRID_FILE: &str = "us_nga_egm96_15.tif";
/// Where PROJ-data distributes it.
pub const EGM96_GRID_URL: &str = "https://cdn.proj.org/us_nga_egm96_15.tif";
/// The SHA-256 PROJ-data's `files.geojson` publishes for it; the copy downloaded for
/// GAP-197 on 2026-09-26 matched it.
pub const EGM96_GRID_SHA256: &str =
    "db493027562c9b004d7220fa881f5603adada4e1c5029b933fa7de4547b0e78d";
/// Its size, in bytes.
pub const EGM96_GRID_BYTES: u64 = 2_710_815;

/// NOAA's GEOID18 for the conterminous United States, NAD83(2011) to NAVD88 height, as
/// PROJ-data distributes it (GAP-197). PROJ-data carries no GEOID18 file for Alaska or
/// Hawaii (NOAA's model there is GEOID12B), and its `us_noaa_g2018p0.tif` relates Puerto
/// Rico and the Virgin Islands to PRVD02, not to NAVD88.
pub const GEOID18_CONUS_GRID_FILE: &str = "us_noaa_g2018u0.tif";
/// Where PROJ-data distributes it.
pub const GEOID18_CONUS_GRID_URL: &str = "https://cdn.proj.org/us_noaa_g2018u0.tif";
/// The SHA-256 PROJ-data's `files.geojson` publishes for it; the copy downloaded for
/// GAP-197 on 2026-09-26 matched it.
pub const GEOID18_CONUS_GRID_SHA256: &str =
    "fa9a407ac7ee3f5a3694008e4bcd09ce9cc250452f0c3b11700a4960340abce2";
/// Its size, in bytes.
pub const GEOID18_CONUS_GRID_BYTES: u64 = 16_742_155;

/// EPSG:3855, "EGM2008 height": the vertical system the EGM2008 grid converts from.
pub const EPSG_EGM2008_HEIGHT: u32 = 3855;
/// EPSG:1027, the EGM2008 geoid, as a WKT `VERT_DATUM` names it.
pub const EPSG_EGM2008_GEOID: u32 = 1027;
/// EPSG:5773, "EGM96 height".
pub const EPSG_EGM96_HEIGHT: u32 = 5773;
/// EPSG:5171, the EGM96 geoid, as a WKT `VERT_DATUM` names it.
pub const EPSG_EGM96_GEOID: u32 = 5171;
/// EPSG:5703, "NAVD88 height", in metres.
pub const EPSG_NAVD88_HEIGHT: u32 = 5703;
/// EPSG:6360, "NAVD88 height (ftUS)": the Autzen fixture's own vertical system.
pub const EPSG_NAVD88_HEIGHT_FTUS: u32 = 6360;
/// EPSG:8228, "NAVD88 height (ft)", in international feet.
pub const EPSG_NAVD88_HEIGHT_FT: u32 = 8228;
/// EPSG:5103, the North American Vertical Datum 1988, as a WKT `VERT_DATUM` names it.
pub const EPSG_NAVD88_DATUM: u32 = 5103;
/// EPSG:4979, WGS 84 geographic 3D, whose third axis is the ellipsoidal height.
pub const EPSG_WGS84_3D: u32 = 4979;
/// `GeoTIFF` 1.0's `VertCS_WGS_84_ellipsoid` (OGC `GeoTIFF` 1.1, `VerticalGeoKey`'s
/// ellipsoid-based codes 5001-5033): heights above the WGS-84 ellipsoid.
pub const GEOTIFF_WGS84_ELLIPSOID: u32 = 5030;
/// `GeoTIFF`'s "user-defined" code: the file says it has a vertical system and not which.
pub const GEOTIFF_USER_DEFINED: u32 = 32_767;

/// The US survey foot, 1200/3937 m (EPSG unit 9003).
pub const US_SURVEY_FOOT_M: f64 = 1200.0 / 3937.0;
/// The international foot, 0.3048 m (EPSG unit 9002).
pub const INTERNATIONAL_FOOT_M: f64 = 0.3048;

/// How many metres one unit of a **vertical CRS**'s axis is, for the codes this module
/// knows, or `None` for any other.
///
/// A vertical CRS code fixes its unit: EPSG:6360 is NAVD88 *in US survey feet*, and a
/// `GeoTIFF` writer names that code and omits `VerticalUnitsGeoKey` (GDAL 3.11 does, for
/// EPSG:2992+6360), so the code is where the unit has to be read from. `None` is "not
/// known here", never "metres": a caller refuses rather than guesses.
#[must_use]
pub fn vertical_crs_unit_metres(code: u32) -> Option<f64> {
    match code {
        EPSG_EGM2008_HEIGHT
        | EPSG_EGM96_HEIGHT
        | EPSG_NAVD88_HEIGHT
        | EPSG_WGS84_3D
        | GEOTIFF_WGS84_ELLIPSOID => Some(1.0),
        EPSG_NAVD88_HEIGHT_FTUS => Some(US_SURVEY_FOOT_M),
        EPSG_NAVD88_HEIGHT_FT => Some(INTERNATIONAL_FOOT_M),
        _ => None,
    }
}

/// How many metres an EPSG **linear unit** code is (`VerticalUnitsGeoKey`,
/// `ProjLinearUnitsGeoKey`): the metre, the international foot and the US survey foot,
/// which between them are what real LIDAR is delivered in. `None` for any other, which a
/// caller refuses by name.
#[must_use]
pub fn linear_unit_metres(code: u16) -> Option<f64> {
    match code {
        9001 => Some(1.0),
        9002 => Some(INTERNATIONAL_FOOT_M),
        9003 => Some(US_SURVEY_FOOT_M),
        _ => None,
    }
}

/// A geoid model a deployment can carry a pinned grid for (D-121, D-125).
///
/// Adding one is a decision, not a code change alone: `deploy/README.md` ("Pinning a
/// further geoid grid") is the procedure, and each variant's grid is pinned by the
/// digest PROJ-data publishes, in `deploy/geoid/SHA256SUMS` and here alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeoidModel {
    /// NGA's EGM2008, 2.5 arc-minutes, global: EGM2008 heights (EPSG:3855).
    Egm2008,
    /// NGA's EGM96, 15 arc-minutes, global: EGM96 heights (EPSG:5773), which SRTM and
    /// most DTED-derived DEMs are in.
    Egm96,
    /// NOAA's GEOID18, 1 arc-minute, the conterminous United States only: NAVD88
    /// heights (EPSG:5703) to NAD83(2011) ellipsoidal heights.
    Geoid18Conus,
}

impl GeoidModel {
    /// Every pinned model, in the order PN-09 lists them.
    pub const ALL: [GeoidModel; 3] = [
        GeoidModel::Egm2008,
        GeoidModel::Egm96,
        GeoidModel::Geoid18Conus,
    ];

    /// The grid's file name, as PROJ-data distributes it and a deployment installs it.
    #[must_use]
    pub fn file(self) -> &'static str {
        match self {
            GeoidModel::Egm2008 => EGM2008_GRID_FILE,
            GeoidModel::Egm96 => EGM96_GRID_FILE,
            GeoidModel::Geoid18Conus => GEOID18_CONUS_GRID_FILE,
        }
    }

    /// Where PROJ-data distributes it.
    #[must_use]
    pub fn url(self) -> &'static str {
        match self {
            GeoidModel::Egm2008 => EGM2008_GRID_URL,
            GeoidModel::Egm96 => EGM96_GRID_URL,
            GeoidModel::Geoid18Conus => GEOID18_CONUS_GRID_URL,
        }
    }

    /// The pinned SHA-256, lowercase hexadecimal.
    #[must_use]
    pub fn sha256(self) -> &'static str {
        match self {
            GeoidModel::Egm2008 => EGM2008_GRID_SHA256,
            GeoidModel::Egm96 => EGM96_GRID_SHA256,
            GeoidModel::Geoid18Conus => GEOID18_CONUS_GRID_SHA256,
        }
    }

    /// The pinned file's size, in bytes.
    #[must_use]
    pub fn bytes(self) -> u64 {
        match self {
            GeoidModel::Egm2008 => EGM2008_GRID_BYTES,
            GeoidModel::Egm96 => EGM96_GRID_BYTES,
            GeoidModel::Geoid18Conus => GEOID18_CONUS_GRID_BYTES,
        }
    }

    /// The model's name as an operator reads it on PN-09.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            GeoidModel::Egm2008 => "EGM2008",
            GeoidModel::Egm96 => "EGM96",
            GeoidModel::Geoid18Conus => "GEOID18 (CONUS)",
        }
    }

    /// The heights it converts, in words a refusal quotes.
    #[must_use]
    pub fn heights(self) -> &'static str {
        match self {
            GeoidModel::Egm2008 => "EGM2008 heights (EPSG:3855)",
            GeoidModel::Egm96 => "EGM96 heights (EPSG:5773)",
            GeoidModel::Geoid18Conus => "NAVD88 heights (EPSG:5703)",
        }
    }

    /// Where the grid has values: a point outside it is refused by PROJ, never given a
    /// zero undulation.
    #[must_use]
    pub fn coverage(self) -> &'static str {
        match self {
            GeoidModel::Egm2008 | GeoidModel::Egm96 => "the whole globe",
            GeoidModel::Geoid18Conus => {
                "the conterminous United States (24 to 58 N, 130 to 60 W); NAVD88 in \
                 Alaska and Hawaii is GEOID12B's, which is not pinned"
            }
        }
    }
}

impl std::fmt::Display for GeoidModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What a file or a baseline says a height is measured from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerticalDatum {
    /// Above the WGS-84 ellipsoid: what the picture wants already.
    Ellipsoidal,
    /// EGM2008 height (EPSG:3855): above the EGM2008 geoid.
    Egm2008,
    /// EGM96 height (EPSG:5773): above the EGM96 geoid.
    Egm96,
    /// NAVD88 height (EPSG:5703, 6360, 8228): the North American Vertical Datum of 1988.
    Navd88,
    /// Anything else, kept by what it was declared as so a refusal can name it.
    Other { epsg: Option<u32>, name: String },
}

impl VerticalDatum {
    /// The datum a vertical CRS code (or a `GeoTIFF` `VerticalGeoKey` value) names.
    ///
    /// 4979 and 5030 are both "WGS-84 ellipsoidal height": the first is the geographic
    /// 3D CRS whose third axis it is, the second `GeoTIFF` 1.0's own code for it. The
    /// three NAVD88 codes differ only in unit, which [`vertical_crs_unit_metres`] reads.
    #[must_use]
    pub fn from_epsg(code: u32) -> Self {
        match code {
            EPSG_EGM2008_HEIGHT => VerticalDatum::Egm2008,
            EPSG_EGM96_HEIGHT => VerticalDatum::Egm96,
            EPSG_NAVD88_HEIGHT | EPSG_NAVD88_HEIGHT_FTUS | EPSG_NAVD88_HEIGHT_FT => {
                VerticalDatum::Navd88
            }
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

    /// The datum a WKT `VERT_DATUM`'s own EPSG authority names, for the geoid datums
    /// this module converts; `None` for any other, which the `VERT_CS`'s own code then
    /// decides.
    #[must_use]
    pub fn from_datum_epsg(code: u32) -> Option<Self> {
        match code {
            EPSG_EGM2008_GEOID => Some(VerticalDatum::Egm2008),
            EPSG_EGM96_GEOID => Some(VerticalDatum::Egm96),
            EPSG_NAVD88_DATUM => Some(VerticalDatum::Navd88),
            _ => None,
        }
    }

    /// The pinned grid that converts it; `None` for an ellipsoidal height, which needs
    /// none, and for a datum with no grid.
    #[must_use]
    pub fn model(&self) -> Option<GeoidModel> {
        match self {
            VerticalDatum::Egm2008 => Some(GeoidModel::Egm2008),
            VerticalDatum::Egm96 => Some(GeoidModel::Egm96),
            VerticalDatum::Navd88 => Some(GeoidModel::Geoid18Conus),
            VerticalDatum::Ellipsoidal | VerticalDatum::Other { .. } => None,
        }
    }

    /// Whether two declarations name the same datum. Two `Other`s agree when both carry
    /// the same code, or, lacking codes, the same name.
    #[must_use]
    pub fn agrees_with(&self, other: &VerticalDatum) -> bool {
        match (self, other) {
            (
                VerticalDatum::Other { epsg: Some(a), .. },
                VerticalDatum::Other { epsg: Some(b), .. },
            ) => a == b,
            (VerticalDatum::Other { name: a, .. }, VerticalDatum::Other { name: b, .. }) => a == b,
            (a, b) => a == b,
        }
    }
}

impl std::fmt::Display for VerticalDatum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerticalDatum::Ellipsoidal => f.write_str("WGS-84 ellipsoidal height"),
            VerticalDatum::Egm2008 => f.write_str("EGM2008 height (EPSG:3855)"),
            VerticalDatum::Egm96 => f.write_str("EGM96 height (EPSG:5773)"),
            VerticalDatum::Navd88 => f.write_str("NAVD88 height (EPSG:5703)"),
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

    /// `model`'s pinned grid in `dir`: [`GeoidModel::file`] there, verified against
    /// [`GeoidModel::sha256`].
    ///
    /// # Errors
    ///
    /// As [`GeoidGrid::verify`].
    pub fn verify_pinned_in(dir: &Path, model: GeoidModel) -> Result<GeoidGrid, GridError> {
        GeoidGrid::verify(&dir.join(model.file()), model.sha256())
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
    /// They are heights above `model`'s geoid; the undulation this verified grid gives
    /// is added. For [`GeoidModel::Geoid18Conus`] the sum is a NAD83(2011) ellipsoidal
    /// height, read as WGS 84's to the metre level the module documentation states.
    Geoid(GeoidModel, GeoidGrid),
}

/// Decide how a file's heights become ellipsoidal ones, or say why they cannot (D-121,
/// D-125).
///
/// `field` is the baseline field an operator would edit (`"terrain.vertical"`,
/// `"point_cloud.vertical"`), so a refusal names it. `file` is what the file itself
/// states; `baseline` is what that field declares. `grids` gives the deployment's
/// verified grid for a model, or the reason it has none, which a refusal carries.
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
    grids: &dyn Fn(GeoidModel) -> Result<GeoidGrid, String>,
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
                 declares none; declare \"ellipsoidal\" (WGS-84 ellipsoidal heights), \
                 \"epsg:3855\" (EGM2008), \"epsg:5773\" (EGM96) or \"epsg:5703\" (NAVD88) \
                 -- a height is never converted from a datum nobody stated"
            ))
        }
    };
    if *datum == VerticalDatum::Ellipsoidal {
        return Ok(HeightReference::Ellipsoidal);
    }
    let Some(model) = datum.model() else {
        return Err(format!(
            "the heights are in {datum}, and this deployment carries no geoid grid for \
             it; only WGS-84 ellipsoidal heights and EGM2008 (EPSG:3855), EGM96 \
             (EPSG:5773) and NAVD88 (EPSG:5703, GEOID18 over the conterminous United \
             States) heights are converted, so reproject the file's heights to one of \
             those, or pin a grid for this datum (deploy/README.md, \"Pinning a further \
             geoid grid\")"
        ));
    };
    match grids(model) {
        Ok(grid) => Ok(HeightReference::Geoid(model, grid)),
        Err(reason) => Err(format!(
            "the heights are {} and need the {model} geoid grid ({}), which this \
             deployment does not have verified: {reason}",
            model.heights(),
            model.file()
        )),
    }
}

/// `heights_m` (one per `lon_lat_deg`, in metres, `NaN` for a point with none) as WGS-84
/// ellipsoidal heights under `reference`.
///
/// # Errors
///
/// Under [`HeightReference::Geoid`]: whatever [`undulations`] refuses -- a point outside
/// the model's grid among it -- and, in a build without the `crs` feature,
/// `DataError::NotImplemented` naming that feature, since the grid is read through PROJ.
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
        HeightReference::Geoid(model, grid) => {
            geoid_to_ellipsoidal(*model, grid, lon_lat_deg, heights_m)
        }
    }
}

#[cfg(feature = "crs")]
fn geoid_to_ellipsoidal(
    model: GeoidModel,
    grid: &GeoidGrid,
    lon_lat_deg: &[[f64; 2]],
    heights_m: &[f64],
) -> Result<Vec<f64>, DataError> {
    let n = undulations(grid, lon_lat_deg).map_err(|e| match e {
        DataError::Parse(reason) => DataError::Parse(format!(
            "{model} (which covers {}): {reason}",
            model.coverage()
        )),
        other => other,
    })?;
    Ok(heights_m.iter().zip(n).map(|(h, n)| h + n).collect())
}

#[cfg(not(feature = "crs"))]
fn geoid_to_ellipsoidal(
    _model: GeoidModel,
    _grid: &GeoidGrid,
    _lon_lat_deg: &[[f64; 2]],
    _heights_m: &[f64],
) -> Result<Vec<f64>, DataError> {
    Err(DataError::NotImplemented {
        what: "adding a geoid undulation to a height",
        waiting_on: "a build with gungnir-data's `crs` feature, which reads the grid \
                     through libproj",
    })
}

/// The geoid undulation N, in metres, at each `[longitude, latitude]` (degrees), read
/// from `grid` by PROJ: a height H above that geoid is the ellipsoidal height H + N.
///
/// All three pinned grids are PROJ-data's `VERTICAL_OFFSET_GEOGRAPHIC_TO_VERTICAL`
/// files, which hold N in metres indexed by geographic position -- WGS 84 for EGM2008 and
/// EGM96, NAD83(2011) for GEOID18 -- so the one pipeline serves every model.
///
/// **One PROJ pipeline, stated in full rather than chosen by PROJ.** Asked for
/// "EGM2008 height to EPSG:4979", PROJ offers `vgridshift` with this grid *and*, when the
/// grid is not found, a "ballpark" no-op that returns the height unchanged -- exactly the
/// silent mix D-121 refuses. So the operation is written out: degrees to radians,
/// `vgridshift` on the verified file with `multiplier=1` (H + N, the direction PROJ's own
/// "Inverse of WGS 84 to EGM2008 height (1)" and "Inverse of NAD83(2011) to NAVD88 height
/// (3)" use), radians back to degrees. A missing or unreadable file then fails to build,
/// and a point off the grid fails to convert, rather than either quietly becoming zero.
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
/// falls outside the grid (GEOID18's, which covers the conterminous United States only,
/// or a clipped test grid), or comes back non-finite.
#[cfg(feature = "crs")]
pub fn undulations(grid: &GeoidGrid, lon_lat_deg: &[[f64; 2]]) -> Result<Vec<f64>, DataError> {
    let path = grid.path().display();
    let definition = format!(
        "+proj=pipeline \
         +step +proj=unitconvert +xy_in=deg +xy_out=rad \
         +step +proj=vgridshift +grids=\"{path}\" +multiplier=1 \
         +step +proj=unitconvert +xy_in=rad +xy_out=deg \
         +step +proj=axisswap +order=1,3,2"
    );
    let pipeline = proj::Proj::new(&definition).map_err(|e| {
        DataError::Parse(format!(
            "PROJ cannot read the geoid grid {path} for vgridshift: {e}"
        ))
    })?;
    lon_lat_deg
        .iter()
        .map(|&[lon, lat]| {
            if !(lon.is_finite() && lat.is_finite()) {
                return Err(DataError::Parse(format!(
                    "position ({lon}, {lat}) is not finite, so it has no geoid undulation"
                )));
            }
            let (_, n) = pipeline.convert((lon, lat)).map_err(|e| {
                DataError::Parse(format!(
                    "no geoid undulation at longitude {lon}, latitude {lat} in {path}: {e}"
                ))
            })?;
            if n.is_finite() {
                Ok(n)
            } else {
                Err(DataError::Parse(format!(
                    "no geoid undulation at longitude {lon}, latitude {lat} in {path}: \
                     PROJ returned {n}, which is outside the grid"
                )))
            }
        })
        .collect()
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

    /// Each digest is the one `deploy/geoid/SHA256SUMS` carries for the install step and
    /// CI, and the manifest pins nothing this module does not: two copies of a fact kept
    /// equal by a test rather than by care.
    #[test]
    fn the_pinned_digests_are_the_ones_the_deployment_manifest_carries() {
        let manifest = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../deploy/geoid/SHA256SUMS"),
        )
        .expect("deploy/geoid/SHA256SUMS is committed");
        let pinned: Vec<(&str, &str)> = manifest
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut fields = l.split_whitespace();
                (
                    fields.next().expect("a digest"),
                    fields.next().expect("a file name"),
                )
            })
            .collect();
        assert_eq!(pinned.len(), GeoidModel::ALL.len(), "{pinned:?}");
        for model in GeoidModel::ALL {
            let line = pinned
                .iter()
                .find(|(_, file)| *file == model.file())
                .unwrap_or_else(|| panic!("the manifest names {}", model.file()));
            assert_eq!(line.0, model.sha256(), "{model}");
            assert!(model.url().starts_with("https://cdn.proj.org/"), "{model}");
            assert!(model.url().ends_with(model.file()), "{model}");
        }
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
        for model in GeoidModel::ALL {
            let err = GeoidGrid::verify_pinned_in(&dir, model).expect_err("nothing there");
            assert!(matches!(err, GridError::Missing { .. }), "{err}");
            assert!(err.to_string().contains(model.file()), "{err}");
        }
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

    fn verified_stand_in(model: GeoidModel) -> GeoidGrid {
        GeoidGrid {
            path: PathBuf::from("/grids").join(model.file()),
            sha256: model.sha256().to_string(),
        }
    }

    /// Every model has its pinned grid. The `Result` is the signature `height_reference`
    /// takes a grid lookup by, not a way this one fails.
    #[allow(clippy::unnecessary_wraps)]
    fn all_installed(model: GeoidModel) -> Result<GeoidGrid, String> {
        Ok(verified_stand_in(model))
    }

    /// No model has one, for the reason given.
    fn none_installed(model: GeoidModel) -> Result<GeoidGrid, String> {
        Err(format!("/grids/{} is not there", model.file()))
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
            &none_installed,
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

    /// GAP-197 (D-125): each gravity-related datum with a pinned grid takes **its own**
    /// grid, never another's -- EGM96 is not approximated by EGM2008, nor NAVD88 by either.
    #[test]
    fn each_pinned_datum_takes_its_own_verified_grid() {
        for (code, model) in [
            (3855, GeoidModel::Egm2008),
            (5773, GeoidModel::Egm96),
            (5703, GeoidModel::Geoid18Conus),
            (6360, GeoidModel::Geoid18Conus),
            (8228, GeoidModel::Geoid18Conus),
        ] {
            let reference = height_reference(
                "terrain.vertical",
                None,
                Some(&VerticalDatum::from_epsg(code)),
                &all_installed,
            )
            .expect("a declared datum with a verified grid converts");
            assert_eq!(
                reference,
                HeightReference::Geoid(model, verified_stand_in(model)),
                "EPSG:{code}"
            );
        }
    }

    #[test]
    fn a_pinned_datum_without_its_verified_grid_is_refused_with_the_reason() {
        for (datum, file) in [
            (VerticalDatum::Egm2008, EGM2008_GRID_FILE),
            (VerticalDatum::Egm96, EGM96_GRID_FILE),
            (VerticalDatum::Navd88, GEOID18_CONUS_GRID_FILE),
        ] {
            let err = height_reference("point_cloud.vertical", Some(&datum), None, &none_installed)
                .expect_err("no grid, no conversion");
            assert!(err.contains(file), "{err}");
            assert!(err.contains("is not there"), "{err}");
        }
        // Only the grid the datum needs is asked for: EGM96 with only EGM2008 installed
        // is refused, not converted with the grid that is there.
        let only_egm2008 = |model: GeoidModel| match model {
            GeoidModel::Egm2008 => Ok(verified_stand_in(model)),
            other => none_installed(other),
        };
        let err = height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::Egm96),
            None,
            &only_egm2008,
        )
        .expect_err("EGM96 needs EGM96's grid");
        assert!(err.contains(EGM96_GRID_FILE), "{err}");
    }

    /// **The refusal for a vertical datum with no grid, by name**: a national levelling
    /// datum (NN2000, which a real Norwegian LAS 1.1 capture's geokeys state), a
    /// user-defined one, and one known only by its WKT name.
    #[test]
    fn a_vertical_datum_with_no_grid_is_refused_by_name() {
        for (datum, named) in [
            (VerticalDatum::from_epsg(5941), "EPSG:5941"),
            (
                VerticalDatum::from_epsg(GEOTIFF_USER_DEFINED),
                "user-defined vertical system",
            ),
            (
                VerticalDatum::Other {
                    epsg: None,
                    name: "Local datum".to_string(),
                },
                "Local datum",
            ),
        ] {
            let err = height_reference("point_cloud.vertical", Some(&datum), None, &all_installed)
                .expect_err("no grid for it");
            assert!(err.contains(named), "{err}");
            assert!(err.contains("no geoid grid"), "{err}");
            assert!(err.contains("Pinning a further geoid grid"), "{err}");
        }
    }

    #[test]
    fn a_height_nobody_states_is_refused_and_the_field_to_declare_is_named() {
        let err = height_reference("terrain.vertical", None, None, &all_installed)
            .expect_err("nobody stated the datum");
        assert!(err.contains("terrain.vertical"), "{err}");
        for code in ["epsg:3855", "epsg:5773", "epsg:5703"] {
            assert!(err.contains(code), "{err}");
        }
    }

    #[test]
    fn a_baseline_that_contradicts_the_file_is_refused_rather_than_obeyed() {
        let err = height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::Egm2008),
            Some(&VerticalDatum::Ellipsoidal),
            &all_installed,
        )
        .expect_err("the two disagree");
        assert!(err.contains("EGM2008"), "{err}");
        assert!(err.contains("ellipsoidal"), "{err}");
        // EGM96 and EGM2008 are different datums, and the difference is decimetres to
        // metres: a contradiction, not a near-agreement.
        assert!(height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::Egm96),
            Some(&VerticalDatum::Egm2008),
            &all_installed,
        )
        .is_err());
        // Agreement is not a contradiction, whichever NAVD88 unit either side names.
        assert!(height_reference(
            "terrain.vertical",
            Some(&VerticalDatum::from_epsg(6360)),
            Some(&VerticalDatum::from_epsg(5703)),
            &all_installed,
        )
        .is_ok());
    }

    #[test]
    fn epsg_codes_map_to_the_datums_they_name() {
        assert_eq!(VerticalDatum::from_epsg(3855), VerticalDatum::Egm2008);
        assert_eq!(VerticalDatum::from_epsg(5773), VerticalDatum::Egm96);
        for navd88 in [5703, 6360, 8228] {
            assert_eq!(VerticalDatum::from_epsg(navd88), VerticalDatum::Navd88);
        }
        assert_eq!(VerticalDatum::from_epsg(4979), VerticalDatum::Ellipsoidal);
        assert_eq!(VerticalDatum::from_epsg(5030), VerticalDatum::Ellipsoidal);
        assert!(matches!(
            VerticalDatum::from_epsg(5941),
            VerticalDatum::Other {
                epsg: Some(5941),
                ..
            }
        ));
        assert!(matches!(
            VerticalDatum::from_epsg(32_767),
            VerticalDatum::Other { epsg: None, .. }
        ));
        assert_eq!(
            VerticalDatum::from_datum_epsg(5103),
            Some(VerticalDatum::Navd88)
        );
        assert_eq!(
            VerticalDatum::from_datum_epsg(5171),
            Some(VerticalDatum::Egm96)
        );
        assert_eq!(
            VerticalDatum::from_datum_epsg(1027),
            Some(VerticalDatum::Egm2008)
        );
        assert_eq!(VerticalDatum::from_datum_epsg(5215), None);
    }

    /// The unit a vertical CRS code fixes: 6360 is NAVD88 in US survey feet, 8228 in
    /// international feet (`pyproj`'s `CRS.from_epsg(...).axis_info`), and a code this
    /// module does not know has no unit here rather than a metre.
    #[test]
    // Each factor is the defining constant itself, so equality is the property.
    #[allow(clippy::float_cmp)]
    fn a_vertical_code_fixes_its_unit_and_an_unknown_code_has_none() {
        assert_eq!(vertical_crs_unit_metres(5703), Some(1.0));
        assert_eq!(vertical_crs_unit_metres(3855), Some(1.0));
        assert_eq!(vertical_crs_unit_metres(5773), Some(1.0));
        assert_eq!(vertical_crs_unit_metres(6360), Some(1200.0 / 3937.0));
        assert_eq!(vertical_crs_unit_metres(8228), Some(0.3048));
        assert_eq!(vertical_crs_unit_metres(5941), None);
        assert_eq!(linear_unit_metres(9001), Some(1.0));
        assert_eq!(linear_unit_metres(9002), Some(0.3048));
        assert_eq!(linear_unit_metres(9003), Some(1200.0 / 3937.0));
        assert_eq!(linear_unit_metres(9036), None, "the kilometre is not read");
    }

    #[test]
    fn a_length_mismatch_is_an_error_rather_than_a_short_answer() {
        let err = ellipsoidal_heights(&HeightReference::Ellipsoidal, &[[0.0, 0.0]], &[])
            .expect_err("one position, no heights");
        assert!(err.to_string().contains("0 heights for 1"), "{err}");
    }
}
