// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A NAD83 position carried to WGS 84 by its realisation and the picture's epoch (GAP-200;
//! D-127 and D-128, the owner's decisions of 2026-10-23).
//!
//! **What this replaces.** A file in NAD83 -- most United States LIDAR -- was converted
//! with PROJ's null "NAD83(2011) to WGS 84" step, so it landed 1 to 2 m off horizontally
//! and up to a metre vertically in WGS 84 terms (1.40 m and -0.38 m at the Autzen fixture
//! at epoch 2010.0, 1.60 m and -0.36 m at 2026.8). D-125 accepted that and said so; D-127
//! and D-128 remove it.
//!
//! **The realisation (D-128).** NAD83 has been realised several times, and the
//! coordinates of one point differ between them by up to about a metre. A file's datum
//! says which: NAD83(2011) (EPSG datum 1116), NSRS2007 (6759), FBN (1211), HARN (6152),
//! CORS96 (1133), or the unqualified NAD83 (6269). Each older realisation is carried to
//! NAD83(2011) through NOAA's NADCON5 grids for the conterminous United States, the chain
//! PROJ itself uses: 1986 to HARN, HARN to FBN, FBN to NSRS2007, NSRS2007 to 2011
//! ([`Nadcon5Step`]). **CORS96 has no NADCON5 grid** and is read as NSRS2007, which was
//! adjusted holding the CORS at their CORS96 coordinates, so it takes the chain's last
//! step. **The unqualified NAD83 is ambiguous**: its code names the 1986 realisation, but
//! most modern surveys labelled with it are NAD83(2011). Such a file is converted only
//! once the baseline declares its realisation, and refused by name otherwise
//! ([`resolve`]); a file that states a realisation is taken at its word, and a
//! declaration that contradicts it is refused.
//!
//! **The frame step (D-127).** NAD83(2011) is fixed to the North American plate, so a
//! surveyed point's NAD83(2011) coordinates do not change with time, while its place in
//! ITRF2014 -- and so in WGS 84, read as ITRF2014 at the centimetre level -- moves by
//! about 2 cm a year. The tracks a surface is drawn against are measured now, so the
//! epoch is the picture's: the current date unless the baseline pins one
//! (`frame_epoch`). The transformation is EPSG's "ITRF2014 to NAD83(2011) (1)",
//! inverted, as PROJ defines it.
//!
//! **PROJ does the geodesy, and the epoch is exact.** `proj` 0.31's `Proj::convert`
//! hands PROJ a time of infinity and a height of zero, and PROJ's `helmert` has no
//! parameter that fixes the epoch in the definition. So the time-dependent Helmert is
//! written as the static one it is at the epoch: each parameter plus its rate times the
//! years from 2010.0, which is how PROJ evaluates it. Checked against PROJ 9.8.1's own
//! time-dependent operation at four epochs, six CONUS points and two heights: no
//! difference at all (`docs/record/2026-10-23/nad83-reaches-wgs-84-by-its-realisation.md`).
//!
//! **Heights.** The NADCON5 steps are applied horizontally (`+no_z_transform`, as PROJ's
//! own 2D operations do), so they never touch a height. A NAVD88 height is a NAD83(2011)
//! ellipsoidal height once GEOID18 has run, so it takes the Helmert's own height
//! difference at the point ([`nad83_2011_height_shift`]); an EGM2008, EGM96 or declared
//! ellipsoidal height is already WGS 84's and takes nothing.
//!
//! **The grids are pinned like the geoid grids** (D-121): PROJ-data's files, by the
//! SHA-256 PROJ-data publishes, never committed, verified before PROJ sees them, handed to
//! PROJ by absolute path in a written-out pipeline so PROJ's operation search -- and its
//! "ballpark" no-op when a grid is missing -- never stands in.

use crate::geoid::GeoidGrid;
#[cfg(feature = "crs")]
use crate::proj_lock::LockedProj;
#[cfg(feature = "crs")]
use crate::DataError;

/// PROJ-data's NADCON5 NAD83(1986) to NAD83(HARN) grid for the conterminous United States.
pub const NADCON5_1986_HARN_FILE: &str = "us_noaa_nadcon5_nad83_1986_nad83_harn_conus.tif";
/// Its SHA-256, as PROJ-data's `files.geojson` publishes it (read 2026-10-23).
pub const NADCON5_1986_HARN_SHA256: &str =
    "e12ac2e886edafe616e0994c610ad3daa01db17c00d0e9938145d956f249fc2f";
/// Its size in bytes.
pub const NADCON5_1986_HARN_BYTES: u64 = 3_522_682;
/// PROJ-data's NADCON5 NAD83(HARN) to NAD83(FBN) grid.
pub const NADCON5_HARN_FBN_FILE: &str = "us_noaa_nadcon5_nad83_harn_nad83_fbn_conus.tif";
/// Its SHA-256.
pub const NADCON5_HARN_FBN_SHA256: &str =
    "703434db8e87dd5c35579151410a9e5892d5769f4f2f9c6b0e2b700ae537e2ff";
/// Its size in bytes.
pub const NADCON5_HARN_FBN_BYTES: u64 = 9_111_570;
/// PROJ-data's NADCON5 NAD83(FBN) to NAD83(NSRS2007) grid.
pub const NADCON5_FBN_2007_FILE: &str = "us_noaa_nadcon5_nad83_fbn_nad83_2007_conus.tif";
/// Its SHA-256.
pub const NADCON5_FBN_2007_SHA256: &str =
    "73ea6609fcdbc971771dc0481be19f4d6aa5118ddbd3e0734ff1cf0facf0f40a";
/// Its size in bytes.
pub const NADCON5_FBN_2007_BYTES: u64 = 42_709_110;
/// PROJ-data's NADCON5 NAD83(NSRS2007) to NAD83(2011) grid.
pub const NADCON5_2007_2011_FILE: &str = "us_noaa_nadcon5_nad83_2007_nad83_2011_conus.tif";
/// Its SHA-256.
pub const NADCON5_2007_2011_SHA256: &str =
    "560d65e871afdcfed3d736074f5b3b82cb2fd74d25f988cc556824eceeafce96";
/// Its size in bytes.
pub const NADCON5_2007_2011_BYTES: u64 = 39_965_338;

/// One step of NOAA's NADCON5 chain for the conterminous United States, as PROJ-data
/// distributes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nadcon5Step {
    /// NAD83(1986) to NAD83(HARN).
    Nad83ToHarn,
    /// NAD83(HARN) to NAD83(FBN).
    HarnToFbn,
    /// NAD83(FBN) to NAD83(NSRS2007).
    FbnToNsrs2007,
    /// NAD83(NSRS2007) to NAD83(2011).
    Nsrs2007To2011,
}

impl Nadcon5Step {
    /// The chain, oldest realisation first.
    pub const ALL: [Nadcon5Step; 4] = [
        Nadcon5Step::Nad83ToHarn,
        Nadcon5Step::HarnToFbn,
        Nadcon5Step::FbnToNsrs2007,
        Nadcon5Step::Nsrs2007To2011,
    ];

    /// The grid's file name, as PROJ-data distributes it and a deployment installs it.
    #[must_use]
    pub fn file(self) -> &'static str {
        match self {
            Nadcon5Step::Nad83ToHarn => NADCON5_1986_HARN_FILE,
            Nadcon5Step::HarnToFbn => NADCON5_HARN_FBN_FILE,
            Nadcon5Step::FbnToNsrs2007 => NADCON5_FBN_2007_FILE,
            Nadcon5Step::Nsrs2007To2011 => NADCON5_2007_2011_FILE,
        }
    }

    /// The pinned SHA-256, lowercase hexadecimal.
    #[must_use]
    pub fn sha256(self) -> &'static str {
        match self {
            Nadcon5Step::Nad83ToHarn => NADCON5_1986_HARN_SHA256,
            Nadcon5Step::HarnToFbn => NADCON5_HARN_FBN_SHA256,
            Nadcon5Step::FbnToNsrs2007 => NADCON5_FBN_2007_SHA256,
            Nadcon5Step::Nsrs2007To2011 => NADCON5_2007_2011_SHA256,
        }
    }

    /// The pinned file's size, in bytes.
    #[must_use]
    pub fn bytes(self) -> u64 {
        match self {
            Nadcon5Step::Nad83ToHarn => NADCON5_1986_HARN_BYTES,
            Nadcon5Step::HarnToFbn => NADCON5_HARN_FBN_BYTES,
            Nadcon5Step::FbnToNsrs2007 => NADCON5_FBN_2007_BYTES,
            Nadcon5Step::Nsrs2007To2011 => NADCON5_2007_2011_BYTES,
        }
    }

    /// The step as an operator reads it on PN-09.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Nadcon5Step::Nad83ToHarn => "NADCON5 NAD83 to NAD83(HARN)",
            Nadcon5Step::HarnToFbn => "NADCON5 NAD83(HARN) to NAD83(FBN)",
            Nadcon5Step::FbnToNsrs2007 => "NADCON5 NAD83(FBN) to NAD83(NSRS2007)",
            Nadcon5Step::Nsrs2007To2011 => "NADCON5 NAD83(NSRS2007) to NAD83(2011)",
        }
    }
}

impl std::fmt::Display for Nadcon5Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A realisation of NAD83 this deployment converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Realisation {
    /// NAD83(1986), the original adjustment; EPSG datum 6269, the unqualified "NAD83".
    Nad83_1986,
    /// NAD83(HARN), the state High Accuracy Reference Networks; datum 6152.
    Harn,
    /// NAD83(FBN), the Federal Base Network; datum 1211.
    Fbn,
    /// NAD83(NSRS2007), the 2007 national readjustment; datum 6759.
    Nsrs2007,
    /// NAD83(CORS96), the CORS coordinates of 1996; datum 1133. No NADCON5 grid: read as
    /// NSRS2007 (D-128).
    Cors96,
    /// NAD83(2011), the 2011 readjustment; datum 1116.
    Nad83_2011,
}

impl Realisation {
    /// Every realisation, oldest first.
    pub const ALL: [Realisation; 6] = [
        Realisation::Nad83_1986,
        Realisation::Harn,
        Realisation::Fbn,
        Realisation::Nsrs2007,
        Realisation::Cors96,
        Realisation::Nad83_2011,
    ];

    /// The realisation's name, as EPSG gives it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Realisation::Nad83_1986 => "NAD83(1986)",
            Realisation::Harn => "NAD83(HARN)",
            Realisation::Fbn => "NAD83(FBN)",
            Realisation::Nsrs2007 => "NAD83(NSRS2007)",
            Realisation::Cors96 => "NAD83(CORS96)",
            Realisation::Nad83_2011 => "NAD83(2011)",
        }
    }

    /// The EPSG code of its geodetic datum, which a file's WKT carries.
    #[must_use]
    pub fn datum_epsg(self) -> u32 {
        match self {
            Realisation::Nad83_1986 => 6269,
            Realisation::Harn => 6152,
            Realisation::Fbn => 1211,
            Realisation::Nsrs2007 => 6759,
            Realisation::Cors96 => 1133,
            Realisation::Nad83_2011 => 1116,
        }
    }

    /// The EPSG code of its two-dimensional geographic system: what a baseline declares
    /// it by (`"epsg:6318"` for NAD83(2011)), and what a projected file is first
    /// unprojected to, in its own datum, before NADCON5 moves it.
    #[must_use]
    pub fn geographic_epsg(self) -> u32 {
        match self {
            Realisation::Nad83_1986 => 4269,
            Realisation::Harn => 4152,
            Realisation::Fbn => 8860,
            Realisation::Nsrs2007 => 4759,
            Realisation::Cors96 => 6783,
            Realisation::Nad83_2011 => 6318,
        }
    }

    /// The realisation a datum code names, `None` for a datum that is not NAD83's.
    #[must_use]
    pub fn from_datum_epsg(code: u32) -> Option<Realisation> {
        Realisation::ALL
            .into_iter()
            .find(|r| r.datum_epsg() == code)
    }

    /// The realisation a declaration's geographic code names, `None` otherwise.
    #[must_use]
    pub fn from_geographic_epsg(code: u32) -> Option<Realisation> {
        Realisation::ALL
            .into_iter()
            .find(|r| r.geographic_epsg() == code)
    }

    /// The NADCON5 steps that carry it to NAD83(2011), in order; none for NAD83(2011).
    #[must_use]
    pub fn chain(self) -> &'static [Nadcon5Step] {
        match self {
            Realisation::Nad83_1986 => &Nadcon5Step::ALL,
            Realisation::Harn => &Nadcon5Step::ALL[1..],
            Realisation::Fbn => &Nadcon5Step::ALL[2..],
            // CORS96 has no grid of its own and is read as NSRS2007 (D-128).
            Realisation::Nsrs2007 | Realisation::Cors96 => &Nadcon5Step::ALL[3..],
            Realisation::Nad83_2011 => &[],
        }
    }

    /// What the conversion from this realisation claims beyond the frame step, stated on
    /// the conversion rather than implied.
    #[must_use]
    pub fn residual(self) -> &'static str {
        match self {
            Realisation::Nad83_2011 => "",
            Realisation::Cors96 => {
                "; NAD83(CORS96) read as NAD83(NSRS2007), which held the CORS at their \
                 CORS96 coordinates, then NADCON5 to NAD83(2011): centimetre-level"
            }
            Realisation::Nad83_1986
            | Realisation::Harn
            | Realisation::Fbn
            | Realisation::Nsrs2007 => "; NADCON5 to NAD83(2011), at NOAA's stated accuracy",
        }
    }
}

impl std::fmt::Display for Realisation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What a file's horizontal system says its datum is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalDatum {
    /// Not NAD83: WGS 84, ETRS89, a local frame or anything else. PROJ's own conversion
    /// stands, unchanged by this module.
    NotNad83,
    /// A NAD83 realisation the file names.
    Nad83(Realisation),
    /// The unqualified NAD83 (datum 6269), which a baseline has to declare (D-128).
    Nad83Unqualified,
}

impl HorizontalDatum {
    /// The datum a WKT1 horizontal or compound system names.
    #[must_use]
    pub fn of_wkt1(wkt: &str) -> HorizontalDatum {
        match crate::pointcloud::crs::wkt1_datum_epsg(wkt) {
            Some(6269) => HorizontalDatum::Nad83Unqualified,
            Some(code) => Realisation::from_datum_epsg(code)
                .map_or(HorizontalDatum::NotNad83, HorizontalDatum::Nad83),
            None => HorizontalDatum::NotNad83,
        }
    }
}

/// The datum of `source` -- `"EPSG:<code>"` or a WKT, anything PROJ reads as a
/// coordinate reference system -- from PROJ's own WKT1 description of it.
///
/// # Errors
///
/// `DataError::Parse` when PROJ cannot read `source` as a coordinate reference system or
/// describe it. Two cases are refused by name before PROJ is asked for WKT1, which it has
/// no form of (GAP-262): a source PROJ reads as something other than a CRS -- a
/// coordinate operation, or a `+proj=` string without `+type=crs`, which PROJ reads as a
/// conversion -- and a geographic or projected 3D CRS such as `EPSG:4979`.
#[cfg(feature = "crs")]
pub fn horizontal_datum(source: &str) -> Result<HorizontalDatum, DataError> {
    let crs = LockedProj::new(source).map_err(|e| {
        DataError::Parse(format!(
            "{source:?} is not a coordinate reference system PROJ recognises: {e}"
        ))
    })?;
    let kind = crs
        .kind()
        .map_err(|e| DataError::Parse(format!("PROJ cannot say what {source:?} is: {e}")))?;
    if !kind.is_crs() {
        return Err(DataError::Parse(format!(
            "{source:?} is a {}, not a coordinate reference system, so it has no datum to \
             read",
            kind.kind
        )));
    }
    if kind.is_3d_geographic_or_projected() {
        return Err(DataError::Parse(format!(
            "{source:?} is a 3D {}, which PROJ cannot describe as WKT1, so its datum is not \
             read; declare the two-dimensional system and the heights' datum apart",
            kind.kind
        )));
    }
    let wkt = crs
        .as_wkt(proj::wkt::WktVersion::Wkt1_Gdal)
        .map_err(|e| DataError::Parse(format!("PROJ cannot describe {source:?} as WKT1: {e}")))?;
    Ok(HorizontalDatum::of_wkt1(&wkt))
}

/// The realisation a file's positions are in, or `None` when they are not NAD83's (D-128).
///
/// `field` is the baseline field an operator edits (`"terrain.nad83_realisation"`,
/// `"point_cloud.nad83_realisation"`), so a refusal names it. **The file wins where both
/// speak, and a disagreement is refused**, the rule `geoid::height_reference` follows for
/// a vertical datum: an unqualified NAD83 file takes the declaration; a file naming a
/// realisation is taken at its word, and a declaration naming another is refused; a
/// declaration on a file that is not NAD83 at all is refused too, because it says
/// something untrue of that file.
///
/// # Errors
///
/// The refusal, in words an operator reads on PN-09 and in the alert.
pub fn resolve(
    field: &str,
    file: HorizontalDatum,
    declared: Option<Realisation>,
) -> Result<Option<Realisation>, String> {
    match (file, declared) {
        (HorizontalDatum::NotNad83, None) => Ok(None),
        (HorizontalDatum::NotNad83, Some(declared)) => Err(format!(
            "{field} declares {declared}, and the file's horizontal datum is not NAD83; \
             remove the declaration, which is untrue of this file"
        )),
        (HorizontalDatum::Nad83(file), None) => Ok(Some(file)),
        (HorizontalDatum::Nad83(file), Some(declared)) if declared == file => Ok(Some(file)),
        (HorizontalDatum::Nad83(file), Some(declared)) => Err(format!(
            "{field} declares {declared} and the file states {file}; the file's own datum is \
             what its positions are, so this is a baseline to correct"
        )),
        (HorizontalDatum::Nad83Unqualified, Some(declared)) => Ok(Some(declared)),
        (HorizontalDatum::Nad83Unqualified, None) => Err(format!(
            "the file is in the unqualified NAD83 (EPSG datum 6269), which names the 1986 \
             realisation but labels most NAD83(2011) surveys too, up to a metre apart; \
             declare its realisation in {field} -- \"epsg:6318\" for NAD83(2011), \
             \"epsg:4759\" NSRS2007, \"epsg:8860\" FBN, \"epsg:4152\" HARN, \"epsg:6783\" \
             CORS96, \"epsg:4269\" the 1986 adjustment -- a position is never converted \
             from a realisation nobody stated"
        )),
    }
}

/// EPSG's "ITRF2014 to NAD83(2011) (1)", coordinate-frame convention, at its reference
/// epoch 2010.0: translations in metres, rotations in arc-seconds, scale in parts per
/// million, then each one's rate per year. PROJ 9.8.1's own definition, as `pyproj`
/// prints it for "Inverse of ITRF2014 to NAD83(2011) (1)".
const HELMERT_AT_2010: [(&str, f64, f64); 7] = [
    ("x", 1.0053, 0.00079),
    ("y", -1.90921, -0.0006),
    ("z", -0.54157, -0.00144),
    ("rx", 0.026_781_38, 6.667e-05),
    ("ry", -0.000_420_27, -0.000_757_44),
    ("rz", 0.010_932_06, -5.133e-05),
    ("s", 0.000_368_91, -7.201e-05),
];

/// The Helmert's reference epoch.
pub const HELMERT_REFERENCE_EPOCH: f64 = 2010.0;

/// The epochs a deployment may convert at: the transformation is defined at any epoch,
/// and these bounds only refuse a number that cannot be a date the picture is at.
pub const EPOCH_RANGE: std::ops::RangeInclusive<f64> = 1990.0..=2100.0;

/// PROJ's `helmert` step for "ITRF2014 to NAD83(2011) (1)" evaluated at `epoch`, as the
/// static seven-parameter transformation it is at that epoch: each parameter plus its
/// rate times the years from 2010.0, which is how PROJ evaluates the time-dependent form.
/// Not inverted here; the pipelines say `+inv`.
#[must_use]
pub fn helmert_at(epoch: f64) -> String {
    let years = epoch - HELMERT_REFERENCE_EPOCH;
    let params: Vec<String> = HELMERT_AT_2010
        .iter()
        .map(|(name, at_2010, rate)| format!("+{name}={}", at_2010 + rate * years))
        .collect();
    format!(
        "+proj=helmert {} +convention=coordinate_frame",
        params.join(" ")
    )
}

/// The decimal year of `seconds` since the Unix epoch, UTC: what the picture's epoch is
/// when the baseline pins none. Day resolution is far finer than the 2 cm a year the
/// epoch is worth.
// Whole days and calendar years are far inside the range an f64 holds exactly.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[must_use]
pub fn decimal_year(seconds: f64) -> f64 {
    let days = (seconds / 86_400.0).floor() as i64;
    let year = year_of(days);
    let start = days_from_civil(year, 1, 1);
    let length = days_from_civil(year + 1, 1, 1) - start;
    year as f64 + (days - start) as f64 / length as f64
}

/// The calendar year holding `days` since the Unix epoch (Hinnant's days-to-civil).
fn year_of(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    // The era's year runs from March, so January and February belong to the next one.
    yoe + era * 400 + i64::from(mp >= 10)
}

/// Days since the Unix epoch of a civil date (Hinnant's days-from-civil).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The decimal year now, UTC, from the system clock; 2010.0 should the clock read before
/// the Unix epoch, which only a broken clock does.
#[must_use]
pub fn current_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(HELMERT_REFERENCE_EPOCH, |d| decimal_year(d.as_secs_f64()))
}

/// A NAD83 realisation's positions carried to WGS 84 at an epoch: the NADCON5 steps it
/// needs, over verified grids, then the Helmert (D-127, D-128).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameStep {
    realisation: Realisation,
    grids: Vec<(Nadcon5Step, GeoidGrid)>,
    epoch: f64,
}

impl FrameStep {
    /// The step for `realisation` at `epoch`, each NADCON5 grid it needs from `grids` --
    /// the deployment's verified grid, or the reason it has none.
    ///
    /// # Errors
    ///
    /// The refusal: an epoch outside [`EPOCH_RANGE`] or not finite, or a grid the chain
    /// needs that the deployment does not have verified.
    pub fn new(
        realisation: Realisation,
        epoch: f64,
        grids: &dyn Fn(Nadcon5Step) -> Result<GeoidGrid, String>,
    ) -> Result<FrameStep, String> {
        check_epoch(epoch)?;
        let grids = realisation
            .chain()
            .iter()
            .map(|&step| {
                grids(step).map(|grid| (step, grid)).map_err(|reason| {
                    format!(
                        "the positions are {realisation} and need the {step} grid ({}), \
                         which this deployment does not have verified: {reason}",
                        step.file()
                    )
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(FrameStep {
            realisation,
            grids,
            epoch,
        })
    }

    /// The realisation the positions are in.
    #[must_use]
    pub fn realisation(&self) -> Realisation {
        self.realisation
    }

    /// The epoch the positions are carried to WGS 84 at.
    #[must_use]
    pub fn epoch(&self) -> f64 {
        self.epoch
    }

    /// What the conversion claims, for PN-09 and the conversion's own line.
    #[must_use]
    pub fn statement(&self) -> String {
        format!(
            "{} to WGS 84 at epoch {:.2}: EPSG's ITRF2014 to NAD83(2011) Helmert, WGS 84 \
             read as ITRF2014 to centimetres{}; plate-interior motion since the survey is \
             not modelled",
            self.realisation,
            self.epoch,
            self.realisation.residual()
        )
    }

    /// The written-out pipeline: degrees to radians, each NADCON5 `gridshift` on its
    /// verified file (horizontal only), geographic to geocentric on GRS80, the Helmert
    /// inverted (NAD83(2011) to ITRF2014), back to geographic, radians to degrees.
    #[cfg(feature = "crs")]
    fn pipeline(&self) -> Result<LockedProj, DataError> {
        let mut definition =
            String::from("+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad");
        for (_, grid) in &self.grids {
            definition.push_str(&format!(
                " +step +proj=gridshift +no_z_transform +grids=\"{}\"",
                grid.path().display()
            ));
        }
        definition.push_str(&format!(
            " +step +proj=cart +ellps=GRS80 +step +inv {} \
             +step +inv +proj=cart +ellps=GRS80 \
             +step +proj=unitconvert +xy_in=rad +xy_out=deg",
            helmert_at(self.epoch)
        ));
        LockedProj::new(&definition).map_err(|e| {
            DataError::Parse(format!(
                "PROJ cannot build the {} to WGS 84 pipeline: {e}",
                self.realisation
            ))
        })
    }

    /// Each `[longitude, latitude]` (degrees, in this step's realisation) as WGS 84
    /// longitude and latitude.
    ///
    /// # Errors
    ///
    /// `DataError::Parse` when PROJ cannot open a grid, or a position is not finite, or
    /// falls outside a NADCON5 grid (the conterminous United States, or a clipped test
    /// grid), naming the position.
    #[cfg(feature = "crs")]
    pub fn to_wgs84(&self, lon_lat_deg: &[[f64; 2]]) -> Result<Vec<[f64; 2]>, DataError> {
        let pipeline = self.pipeline()?;
        lon_lat_deg
            .iter()
            .map(|&[lon, lat]| {
                if !(lon.is_finite() && lat.is_finite()) {
                    return Err(DataError::Parse(format!(
                        "position ({lon}, {lat}) is not finite"
                    )));
                }
                let (x, y) = pipeline.convert((lon, lat)).map_err(|e| {
                    DataError::Parse(format!(
                        "{} position at longitude {lon}, latitude {lat} did not reach WGS 84 \
                         (the NADCON5 grids cover the conterminous United States): {e}",
                        self.realisation
                    ))
                })?;
                if x.is_finite() && y.is_finite() {
                    Ok([x, y])
                } else {
                    Err(DataError::Parse(format!(
                        "{} position at longitude {lon}, latitude {lat} is outside a NADCON5 \
                         grid: PROJ returned ({x}, {y})",
                        self.realisation
                    )))
                }
            })
            .collect()
    }
}

/// What the Helmert at `epoch` adds to a NAD83(2011) ellipsoidal height at each
/// `[longitude, latitude]` (degrees) to make it a WGS 84 one (D-127): the height a NAVD88
/// height has once GEOID18 has run.
///
/// Read through PROJ with the geoid module's route past the binding: fed a height of
/// zero, the Helmert's output height *is* the shift, and `axisswap order=1,3,2` moves it
/// where `Proj::convert` returns it. The shift for a height of zero and one of a
/// kilometre differ by under 0.01 mm (the rotations and scale acting on the extra
/// kilometre), which is why the shift is computed once per position, not per height.
///
/// # Errors
///
/// `DataError::Parse` when an epoch is outside [`EPOCH_RANGE`], a position is not
/// finite, or PROJ fails.
#[cfg(feature = "crs")]
pub fn nad83_2011_height_shift(
    epoch: f64,
    lon_lat_deg: &[[f64; 2]],
) -> Result<Vec<f64>, DataError> {
    check_epoch(epoch).map_err(DataError::Parse)?;
    let definition = format!(
        "+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad \
         +step +proj=cart +ellps=GRS80 +step +inv {} \
         +step +inv +proj=cart +ellps=GRS80 \
         +step +proj=unitconvert +xy_in=rad +xy_out=deg \
         +step +proj=axisswap +order=1,3,2",
        helmert_at(epoch)
    );
    let pipeline = LockedProj::new(&definition)
        .map_err(|e| DataError::Parse(format!("PROJ cannot build the Helmert height step: {e}")))?;
    lon_lat_deg
        .iter()
        .map(|&[lon, lat]| {
            if !(lon.is_finite() && lat.is_finite()) {
                return Err(DataError::Parse(format!(
                    "position ({lon}, {lat}) is not finite, so it has no height shift"
                )));
            }
            let (_, dh) = pipeline.convert((lon, lat)).map_err(|e| {
                DataError::Parse(format!(
                    "no NAD83(2011) height shift at longitude {lon}, latitude {lat}: {e}"
                ))
            })?;
            if dh.is_finite() {
                Ok(dh)
            } else {
                Err(DataError::Parse(format!(
                    "no NAD83(2011) height shift at longitude {lon}, latitude {lat}: PROJ \
                     returned {dh}"
                )))
            }
        })
        .collect()
}

/// A file's NAD83 positions on their way to WGS 84: the geographic system of the file's
/// own datum they are first unprojected to, and the step from there.
///
/// **Unprojected in the file's own datum, never the declared realisation's.** An
/// unqualified NAD83 file declared NAD83(2011) holds NAD83(2011) numbers under the 1986
/// label; asking PROJ for the declared realisation's geographic system would add a datum
/// step between the two labels, which is exactly what the declaration says is not there.
/// So the file is unprojected to its own datum's system (EPSG:4269 for the unqualified
/// NAD83), which PROJ does as the projection's inverse alone, and the numbers are then
/// read as the realisation the file or the declaration names.
#[derive(Debug, Clone, PartialEq)]
pub struct Nad83Source {
    unprojected_to: u32,
    step: FrameStep,
}

impl Nad83Source {
    /// The geographic system, in the file's own datum, positions are unprojected to.
    #[must_use]
    pub fn unprojected_to(&self) -> u32 {
        self.unprojected_to
    }

    /// The step from there to WGS 84.
    #[must_use]
    pub fn step(&self) -> &FrameStep {
        &self.step
    }
}

/// How a converted file's positions and heights reach WGS 84 beyond PROJ's own
/// conversion (GAP-200, D-127, D-128): a NAD83 file's positions by their realisation and
/// the epoch, and a NAVD88 height by the Helmert's height difference at the epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameConversion {
    nad83: Option<Nad83Source>,
    epoch: f64,
}

impl FrameConversion {
    /// A file whose positions are not NAD83's: PROJ's own conversion stands. The epoch
    /// still applies to a NAVD88 height, which is NAD83(2011)'s once GEOID18 has run.
    ///
    /// # Errors
    ///
    /// The refusal of an epoch outside [`EPOCH_RANGE`].
    pub fn not_nad83(epoch: f64) -> Result<FrameConversion, String> {
        check_epoch(epoch)?;
        Ok(FrameConversion { nad83: None, epoch })
    }

    /// The conversion for a file whose horizontal datum is `file`, with what `field`
    /// declares (D-128, [`resolve`]), at `epoch`, over the deployment's verified NADCON5
    /// grids.
    ///
    /// # Errors
    ///
    /// The refusal: [`resolve`]'s, an epoch outside [`EPOCH_RANGE`], or a NADCON5 grid the
    /// chain needs and the deployment does not have verified.
    pub fn resolve(
        field: &str,
        file: HorizontalDatum,
        declared: Option<Realisation>,
        epoch: f64,
        grids: &dyn Fn(Nadcon5Step) -> Result<GeoidGrid, String>,
    ) -> Result<FrameConversion, String> {
        check_epoch(epoch)?;
        let Some(realisation) = resolve(field, file, declared)? else {
            return Ok(FrameConversion { nad83: None, epoch });
        };
        let unprojected_to = match file {
            HorizontalDatum::Nad83(own) => own.geographic_epsg(),
            HorizontalDatum::Nad83Unqualified | HorizontalDatum::NotNad83 => {
                Realisation::Nad83_1986.geographic_epsg()
            }
        };
        Ok(FrameConversion {
            nad83: Some(Nad83Source {
                unprojected_to,
                step: FrameStep::new(realisation, epoch, grids)?,
            }),
            epoch,
        })
    }

    /// The NAD83 step, when the file's positions are NAD83's.
    #[must_use]
    pub fn nad83(&self) -> Option<&Nad83Source> {
        self.nad83.as_ref()
    }

    /// The epoch the picture is at.
    #[must_use]
    pub fn epoch(&self) -> f64 {
        self.epoch
    }
}

fn check_epoch(epoch: f64) -> Result<(), String> {
    if EPOCH_RANGE.contains(&epoch) {
        Ok(())
    } else {
        Err(format!(
            "the frame epoch {epoch} is not a decimal year between {} and {}",
            EPOCH_RANGE.start(),
            EPOCH_RANGE.end()
        ))
    }
}

/// `xy` (each point in `source`, `"EPSG:<code>"` or a WKT) as WGS 84 `[longitude,
/// latitude]` degrees under `frame`: PROJ's own conversion when the positions are not
/// NAD83's, and otherwise unprojected in the file's own datum, then the NADCON5 steps
/// and the Helmert ([`Nad83Source`]).
///
/// # Errors
///
/// `DataError::Parse` when PROJ cannot build a transformation from `source`, or a point
/// does not convert, naming it.
#[cfg(feature = "crs")]
pub fn positions_to_wgs84(
    source: &str,
    frame: &FrameConversion,
    xy: &[[f64; 2]],
) -> Result<Vec<[f64; 2]>, DataError> {
    // EPSG:4979, WGS 84 three-dimensional, is what a compound source converts into
    // cleanly; the pair is the same as EPSG:4326's. `new_known_crs` normalises the axis
    // order, so the pair that comes back is (longitude, latitude) either way.
    let target = frame.nad83().map_or_else(
        || "EPSG:4979".to_string(),
        |n| format!("EPSG:{}", n.unprojected_to()),
    );
    let transform = LockedProj::new_known_crs(source, &target).map_err(|e| {
        DataError::Parse(format!(
            "cannot build a transformation from {source:?} to {target}: {e}"
        ))
    })?;
    let converted = xy
        .iter()
        .map(|&[x, y]| {
            if !(x.is_finite() && y.is_finite()) {
                return Err(DataError::Parse(format!(
                    "{source:?}: point ({x}, {y}) is not finite"
                )));
            }
            let (lon, lat) = transform.convert((x, y)).map_err(|e| {
                DataError::Parse(format!(
                    "{source:?}: point ({x}, {y}) does not convert: {e}"
                ))
            })?;
            Ok([lon, lat])
        })
        .collect::<Result<Vec<_>, DataError>>()?;
    match frame.nad83() {
        None => Ok(converted),
        Some(n) => n.step().to_wgs84(&converted),
    }
}

/// `heights_m` as WGS 84 ellipsoidal heights: [`crate::geoid::ellipsoidal_heights`]
/// under `heights`, and, for a NAVD88 height -- a NAD83(2011) ellipsoidal one once
/// GEOID18 has run -- the Helmert's height difference at `frame`'s epoch added
/// ([`nad83_2011_height_shift`]). A height with no value stays without one.
///
/// Both are read at `lon_lat_deg`, the converted WGS 84 position. GEOID18 is indexed by
/// NAD83(2011) position, a metre or two away; over that distance its undulation changes
/// by hundredths of a millimetre (0.02 mm at the Autzen fixture), which is not worth a
/// second set of positions carried through the conversion.
///
/// # Errors
///
/// Whatever the geoid step or the height shift refuses.
#[cfg(feature = "crs")]
pub fn wgs84_heights(
    heights: &crate::geoid::HeightReference,
    frame: &FrameConversion,
    lon_lat_deg: &[[f64; 2]],
    heights_m: &[f64],
) -> Result<Vec<f64>, DataError> {
    let mut ellipsoidal = crate::geoid::ellipsoidal_heights(heights, lon_lat_deg, heights_m)?;
    if matches!(
        heights,
        crate::geoid::HeightReference::Geoid(crate::geoid::GeoidModel::Geoid18Conus, _)
    ) {
        let shift = nad83_2011_height_shift(frame.epoch(), lon_lat_deg)?;
        for (h, dh) in ellipsoidal.iter_mut().zip(shift) {
            *h += dh;
        }
    }
    Ok(ellipsoidal)
}

/// Without the `crs` feature, the geoid step alone: a NAVD88 height already needs PROJ
/// for GEOID18, and [`crate::geoid::ellipsoidal_heights`] refuses it by name.
///
/// # Errors
///
/// Whatever the geoid step refuses.
#[cfg(not(feature = "crs"))]
pub fn wgs84_heights(
    heights: &crate::geoid::HeightReference,
    _frame: &FrameConversion,
    lon_lat_deg: &[[f64; 2]],
    heights_m: &[f64],
) -> Result<Vec<f64>, crate::DataError> {
    crate::geoid::ellipsoidal_heights(heights, lon_lat_deg, heights_m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WKT1 system's datum is read from the `DATUM` node's own authority, never the
    /// spheroid's nested inside it or the projected system's above it.
    #[test]
    fn the_datum_is_read_from_the_datum_node() {
        let autzen = r#"COMPD_CS["NAD83 / Oregon GIC Lambert (ft) + NAVD88 height (ftUS)",PROJCS["NAD83 / Oregon GIC Lambert (ft)",GEOGCS["NAD83",DATUM["North_American_Datum_1983",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6269"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4269"]],PROJECTION["Lambert_Conformal_Conic_2SP"],UNIT["foot",0.3048,AUTHORITY["EPSG","9002"]],AUTHORITY["EPSG","2992"]],VERT_CS["NAVD88 height (ftUS)",VERT_DATUM["North American Vertical Datum 1988",2005,AUTHORITY["EPSG","5103"]],UNIT["US survey foot",0.304800609601219,AUTHORITY["EPSG","9003"]],AUTHORITY["EPSG","6360"]]]"#;
        assert_eq!(
            HorizontalDatum::of_wkt1(autzen),
            HorizontalDatum::Nad83Unqualified
        );
        let harn = r#"GEOGCS["NAD83(HARN)",DATUM["NAD83_High_Accuracy_Reference_Network",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6152"]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4152"]]"#;
        assert_eq!(
            HorizontalDatum::of_wkt1(harn),
            HorizontalDatum::Nad83(Realisation::Harn)
        );
        let wgs84 = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],AUTHORITY["EPSG","4326"]]"#;
        assert_eq!(HorizontalDatum::of_wkt1(wgs84), HorizontalDatum::NotNad83);
        let no_authority = r#"GEOGCS["x",DATUM["d",SPHEROID["GRS 1980",6378137,298.257222101]]]"#;
        assert_eq!(
            HorizontalDatum::of_wkt1(no_authority),
            HorizontalDatum::NotNad83
        );
    }

    /// GAP-262: every source PROJ reads but has no WKT1 form of is refused by name, where
    /// each of these once panicked inside `proj`, and a CRS still has its datum read.
    #[cfg(feature = "crs")]
    #[test]
    fn a_source_with_no_wkt1_form_is_refused_by_name_rather_than_panicking() {
        let refused = |source: &str, named: &str| match horizontal_datum(source) {
            Err(DataError::Parse(reason)) => {
                assert!(reason.contains(source), "{reason}");
                assert!(reason.contains(named), "{reason}");
            }
            other => panic!("{source}: {other:?}"),
        };
        // A transformation: what `EPSG:1671` names, though PROJ reads `EPSG:` as a CRS.
        refused(
            "urn:ogc:def:coordinateOperation:EPSG::1671",
            "a Transformation, not a coordinate reference system",
        );
        // A bare PROJ string is a conversion to PROJ.
        refused(
            "+proj=merc",
            "a Conversion, not a coordinate reference system",
        );
        refused(
            "+proj=utm +zone=10 +datum=WGS84",
            "a Conversion, not a coordinate reference system",
        );
        // WGS 84 and ITRF2014, geographic 3D.
        refused("EPSG:4979", "3D GeographicCRS");
        refused("EPSG:7912", "3D GeographicCRS");

        assert_eq!(
            horizontal_datum("EPSG:6318").expect("NAD83(2011)"),
            HorizontalDatum::Nad83(Realisation::Nad83_2011)
        );
        assert_eq!(
            horizontal_datum("EPSG:4269").expect("NAD83"),
            HorizontalDatum::Nad83Unqualified
        );
        // Three axes, but geocentric: PROJ writes it as a WKT1 `GEOCCS`.
        assert_eq!(
            horizontal_datum("EPSG:4978").expect("WGS 84 geocentric"),
            HorizontalDatum::NotNad83
        );
        assert_eq!(
            horizontal_datum("+proj=utm +zone=10 +datum=NAD83 +type=crs").expect("a CRS"),
            HorizontalDatum::Nad83Unqualified
        );
    }

    /// D-128: the file wins, an unqualified file takes the declaration or is refused by
    /// name, and a declaration that contradicts the file, or names NAD83 for a file that
    /// is not, is refused.
    #[test]
    fn a_realisation_is_the_files_or_the_declarations_and_never_a_guess() {
        let f = "terrain.nad83_realisation";
        let r2011 = Realisation::Nad83_2011;
        assert_eq!(resolve(f, HorizontalDatum::NotNad83, None), Ok(None));
        assert_eq!(
            resolve(f, HorizontalDatum::Nad83(r2011), None),
            Ok(Some(r2011))
        );
        assert_eq!(
            resolve(f, HorizontalDatum::Nad83(r2011), Some(r2011)),
            Ok(Some(r2011))
        );
        assert_eq!(
            resolve(
                f,
                HorizontalDatum::Nad83Unqualified,
                Some(Realisation::Harn)
            ),
            Ok(Some(Realisation::Harn))
        );
        let refused = resolve(f, HorizontalDatum::Nad83Unqualified, None).expect_err("refused");
        assert!(
            refused.contains(f) && refused.contains("epsg:6318"),
            "{refused}"
        );
        let contradicts = resolve(f, HorizontalDatum::Nad83(r2011), Some(Realisation::Harn))
            .expect_err("refused");
        assert!(contradicts.contains("NAD83(HARN)") && contradicts.contains("NAD83(2011)"));
        assert!(resolve(f, HorizontalDatum::NotNad83, Some(r2011)).is_err());
    }

    /// Each realisation takes the chain from itself to NAD83(2011), and CORS96 the last
    /// step only, as NSRS2007 (D-128).
    #[test]
    fn each_realisation_takes_its_own_part_of_the_chain() {
        assert_eq!(Realisation::Nad83_1986.chain().len(), 4);
        assert_eq!(Realisation::Harn.chain()[0], Nadcon5Step::HarnToFbn);
        assert_eq!(Realisation::Fbn.chain()[0], Nadcon5Step::FbnToNsrs2007);
        assert_eq!(Realisation::Nsrs2007.chain(), [Nadcon5Step::Nsrs2007To2011]);
        assert_eq!(Realisation::Cors96.chain(), Realisation::Nsrs2007.chain());
        assert!(Realisation::Nad83_2011.chain().is_empty());
        for r in Realisation::ALL {
            assert_eq!(Realisation::from_datum_epsg(r.datum_epsg()), Some(r));
            assert_eq!(
                Realisation::from_geographic_epsg(r.geographic_epsg()),
                Some(r)
            );
        }
    }

    /// The decimal year by the calendar: 1 January is the whole year, a leap year's
    /// length is 366 days, and the Unix epoch is 1970.0.
    #[test]
    fn a_date_is_a_decimal_year_by_the_calendar() {
        assert!((decimal_year(0.0) - 1970.0).abs() < 1e-12);
        let jan_2024 = 1_704_067_200.0; // 2024-01-01T00:00:00Z
        assert!((decimal_year(jan_2024) - 2024.0).abs() < 1e-12);
        // 2024-07-02 is day 183 (0-based) of a 366-day year.
        let july_2024 = jan_2024 + 183.0 * 86_400.0;
        assert!((decimal_year(july_2024) - (2024.0 + 183.0 / 366.0)).abs() < 1e-12);
        let mar_2023 = 1_677_628_800.0; // 2023-03-01
        assert!((decimal_year(mar_2023) - (2023.0 + 59.0 / 365.0)).abs() < 1e-12);
    }

    /// The static Helmert at an epoch is the reference parameters moved by their rates,
    /// and an epoch out of range is refused rather than extrapolated.
    #[test]
    fn the_helmert_at_an_epoch_is_its_parameters_moved_by_their_rates() {
        assert!(helmert_at(2010.0).contains("+x=1.0053 "));
        assert!(helmert_at(2020.0).contains(&format!("+x={} ", 1.0053 + 0.00079 * 10.0)));
        let no_grid = |_: Nadcon5Step| Err::<GeoidGrid, String>("none".into());
        assert!(FrameStep::new(Realisation::Nad83_2011, 2026.5, &no_grid).is_ok());
        assert!(FrameStep::new(Realisation::Nad83_2011, f64::NAN, &no_grid).is_err());
        assert!(FrameStep::new(Realisation::Nad83_2011, 1900.0, &no_grid).is_err());
        let refused = FrameStep::new(Realisation::Harn, 2026.5, &no_grid).expect_err("refused");
        assert!(refused.contains(NADCON5_HARN_FBN_FILE), "{refused}");
    }
}
