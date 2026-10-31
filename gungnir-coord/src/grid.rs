// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! UTM, UPS and MGRS, for a position an operator reads out or passes on (GAP-227, DN-37
//! §4, D-170).
//!
//! Verified against `GeographicLib`'s `GeoConvert` on the committed vectors in
//! `testdata/oracles/coord/geoconvert.json`: the same MGRS string, and a UTM or UPS
//! position within a millimetre. `testdata/oracles/SOURCE.md` says which release wrote
//! them. Deterministic math: no logging here (agentic-coding-standards.md §2.8).
//!
//! # What is implemented
//!
//! Geodetic to grid, one way: [`utm_ups`] and [`mgrs`]. The readout needs nothing else,
//! so there is no inverse and no MGRS parser; an MGRS string is only ever written here.
//!
//! * **UTM** by the Krüger series (Karney 2011, "Transverse Mercator with an accuracy of
//!   a few nanometers") to sixth order in the third flattening. The series is accurate to
//!   about nine nanometres within 3900 km of the central meridian, far inside the
//!   millimetre the vectors hold it to.
//! * **UPS** by the conformal-latitude form of the polar stereographic projection.
//! * **MGRS lettering** from the grid position and the latitude band.
//!
//! # The rules the vectors pin down
//!
//! * UTM covers latitudes from -80 inclusive to 84 exclusive; UPS covers the rest.
//! * The zone is the six-degree strip of the longitude, with the Norway exception
//!   (latitudes 56 to 64, longitudes 3 to 12 are zone 32) and Svalbard's (latitudes 72 to
//!   84: zones 31, 33, 35 and 37 take the strips 0 to 9, 9 to 21, 21 to 33 and 33 to 42).
//!   A longitude of exactly 180 is in zone 1, as -180 is.
//! * The MGRS position is **truncated** to its precision, never rounded.
//! * A negative zero latitude is in the southern hemisphere, as `GeoConvert` reads it.
//! * UPS's band letter follows the side of the 2000 km easting, so the pole's own
//!   longitude is the east half.

use crate::{Geodetic, WGS84_A, WGS84_F};
use std::fmt::Write as _;

/// Scale on the central meridian of UTM.
const UTM_K0: f64 = 0.9996;
/// Scale at the pole of UPS.
const UPS_K0: f64 = 0.994;
/// False easting of UTM, metres.
const UTM_FALSE_EASTING_M: f64 = 500_000.0;
/// False northing of UTM's southern hemisphere, metres.
const UTM_FALSE_NORTHING_SOUTH_M: f64 = 10_000_000.0;
/// How far below the false northing a southern position on the equator is written in MGRS,
/// metres: far less than a digit of the finest precision, so it only chooses the square.
const SOUTH_EQUATOR_MARGIN_M: f64 = 1e-6;
/// False easting and northing of UPS, metres.
const UPS_FALSE_M: f64 = 2_000_000.0;
/// Side of an MGRS 100 km square, metres.
const SQUARE_M: f64 = 100_000.0;

/// The highest latitude UTM covers, exclusive; UPS begins here.
const UTM_NORTH_LIMIT_DEG: f64 = 84.0;
/// The lowest latitude UTM covers, inclusive.
const UTM_SOUTH_LIMIT_DEG: f64 = -80.0;

/// Orders of the Krüger series. Six is `GeographicLib`'s default.
const SERIES_ORDER: usize = 6;

/// MGRS latitude band letters from -80, eight degrees apiece; X is twelve degrees.
const LATITUDE_BANDS: &[u8; 20] = b"CDEFGHJKLMNPQRSTUVWX";
/// UTM's 100 km column letters, one run of eight for each of three zone sets.
const UTM_COLUMNS: [&[u8]; 3] = [b"ABCDEFGH", b"JKLMNPQR", b"STUVWXYZ"];
/// UTM's 100 km row letters, repeating every 2000 km.
const UTM_ROWS: &[u8; 20] = b"ABCDEFGHJKLMNPQRSTUV";
/// How far an even zone's row letters are shifted from an odd one's, in squares.
const UTM_EVEN_ZONE_ROW_SHIFT: usize = 5;
/// UPS's column letters for the west and east halves of the south then the north cap.
const UPS_COLUMNS: [&[u8]; 4] = [b"JKLPQRSTUXYZ", b"ABCFGHJKLPQR", b"RSTUXYZ", b"ABCFGHJ"];
/// UPS's first column, in squares from the false origin, for the same four halves.
const UPS_FIRST_COLUMN: [usize; 4] = [8, 20, 13, 20];
/// UPS's row letters for the south then the north cap.
const UPS_ROWS: [&[u8]; 2] = [b"ABCDEFGHJKLMNPQRSTUVWXYZ", b"ABCDEFGHJKLMNP"];
/// UPS's first row, in squares from the false origin, for the south then the north cap.
const UPS_FIRST_ROW: [usize; 2] = [8, 13];
/// UPS's four band letters: south west, south east, north west, north east.
const UPS_BANDS: &[u8; 4] = b"ABYZ";

/// Why a position has no grid reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GridError {
    /// A latitude or longitude that is NaN or infinite.
    #[error("the position is not a finite latitude and longitude")]
    NotFinite,
    /// A latitude beyond the poles.
    #[error("latitude is beyond 90 degrees")]
    LatitudeOutOfRange,
    /// More digits than a grid reference holds.
    #[error("an MGRS reference takes at most five digits for each axis")]
    PrecisionOutOfRange,
    /// A longitude so far from its zone's central meridian that the series does not hold.
    #[error("the position is too far from its zone's central meridian")]
    OutsideZone,
}

/// A position in UTM or UPS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UtmUps {
    /// The UTM zone, 1 to 60, or 0 for UPS.
    pub zone: u8,
    /// True north of the equator (UTM) or round the north pole (UPS).
    pub north: bool,
    /// Metres east of the false origin.
    pub easting_m: f64,
    /// Metres north of the false origin; a southern UTM position carries the 10 000 km
    /// false northing.
    pub northing_m: f64,
}

impl UtmUps {
    /// True when this is a polar stereographic position rather than a UTM zone's.
    #[must_use]
    pub fn is_ups(&self) -> bool {
        self.zone == 0
    }
}

/// `(sin, cos)` of an angle in degrees, exact at every multiple of 90 degrees.
///
/// Reducing by quadrant before converting to radians is what makes `sin(180°)` exactly zero
/// rather than 1.2e-16: a longitude on the antimeridian would otherwise land on one side
/// of UPS's east-west line by rounding.
fn sin_cos_deg(x_deg: f64) -> (f64, f64) {
    let mut r = x_deg % 360.0;
    // `round` is half away from zero, so an angle exactly between two quadrants is
    // reduced to the same remainder each way; both reductions agree to rounding.
    let q = (r / 90.0).round();
    r -= 90.0 * q;
    let (s, c) = r.to_radians().sin_cos();
    // `q` is in -4..=4, so the conversion cannot overflow.
    #[allow(clippy::cast_possible_truncation)]
    let (s, c) = match (q as i32).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    };
    // Adding zero turns a negative-zero cosine into a positive one; the sine keeps the
    // sign of the angle it came from at an exact multiple of 180 degrees.
    let s = if s == 0.0 { 0.0_f64.copysign(x_deg) } else { s };
    (s, c + 0.0)
}

/// `tan` of a latitude in degrees, exact at the equator and infinite at a pole.
fn tan_deg(lat_deg: f64) -> f64 {
    let (s, c) = sin_cos_deg(lat_deg);
    s / c
}

/// The third flattening, `f / (2 - f)`.
fn third_flattening() -> f64 {
    WGS84_F / (2.0 - WGS84_F)
}

/// First eccentricity of the WGS-84 ellipsoid.
fn eccentricity() -> f64 {
    (WGS84_F * (2.0 - WGS84_F)).sqrt()
}

/// `e atanh(e x)`, the term the conformal latitude adds to the sphere's.
fn e_atanh_e(x: f64) -> f64 {
    let e = eccentricity();
    e * (e * x).atanh()
}

/// The tangent of the conformal latitude, from the tangent of the geodetic one.
fn conformal_tau(tau: f64) -> f64 {
    let tau1 = tau.hypot(1.0);
    let sigma = e_atanh_e(tau / tau1).sinh();
    sigma.hypot(1.0) * tau - sigma * tau1
}

/// Krüger's `alpha` coefficients, orders 1 to 6, for the forward series.
fn krueger_alpha(n: f64) -> [f64; SERIES_ORDER] {
    let n2 = n * n;
    let n3 = n2 * n;
    let n4 = n2 * n2;
    let n5 = n4 * n;
    let n6 = n3 * n3;
    [
        n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
            + 7891.0 * n6 / 37800.0,
        13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
            - 1_983_433.0 * n6 / 1_935_360.0,
        61.0 * n3 / 240.0 - 103.0 * n4 / 140.0
            + 15061.0 * n5 / 26880.0
            + 167_603.0 * n6 / 181_440.0,
        49561.0 * n4 / 161_280.0 - 179.0 * n5 / 168.0 + 6_601_661.0 * n6 / 7_257_600.0,
        34729.0 * n5 / 80640.0 - 3_418_889.0 * n6 / 1_995_840.0,
        212_378_941.0 * n6 / 319_334_400.0,
    ]
}

/// The rectifying radius times the scale, in metres: `k0 A`, with `A` the meridian
/// quadrant's length over `pi / 2`.
fn rectifying_radius_m() -> f64 {
    let n = third_flattening();
    let n2 = n * n;
    WGS84_A / (1.0 + n) * (1.0 + n2 / 4.0 + n2 * n2 / 64.0 + n2 * n2 * n2 / 256.0)
}

/// Transverse Mercator on the WGS-84 ellipsoid: `(easting, northing)` in metres from the
/// central meridian's equator, for a latitude and a longitude offset from the central
/// meridian, both in degrees, at central scale `k0`.
///
/// Only the near side of the central meridian: an offset past 90 degrees is refused,
/// because the series is the wrong one there and UTM never reaches it.
fn transverse_mercator(lat_deg: f64, dlon_deg: f64, k0: f64) -> Result<(f64, f64), GridError> {
    if dlon_deg.abs() > 90.0 {
        return Err(GridError::OutsideZone);
    }
    // The projection is odd in each coordinate: work in the first quadrant and restore
    // the signs, as GeographicLib does, so the series never sees a negative argument.
    let lat_sign = if lat_deg.is_sign_negative() {
        -1.0
    } else {
        1.0
    };
    let lon_sign = if dlon_deg.is_sign_negative() {
        -1.0
    } else {
        1.0
    };
    let (lat, lon) = (lat_deg.abs(), dlon_deg.abs());
    let (sin_lon, cos_lon) = sin_cos_deg(lon);
    let taup = conformal_tau(tan_deg(lat));
    let xi0 = taup.atan2(cos_lon);
    let eta0 = (sin_lon / taup.hypot(cos_lon)).asinh();

    let alpha = krueger_alpha(third_flattening());
    let (mut xi, mut eta) = (xi0, eta0);
    for (j, a) in alpha.iter().enumerate() {
        // `j` is at most 5.
        #[allow(clippy::cast_precision_loss)]
        let m = 2.0 * (j + 1) as f64;
        xi += a * (m * xi0).sin() * (m * eta0).cosh();
        eta += a * (m * xi0).cos() * (m * eta0).sinh();
    }
    let scale = k0 * rectifying_radius_m();
    Ok((eta * scale * lon_sign, xi * scale * lat_sign))
}

/// Polar stereographic on the WGS-84 ellipsoid: `(x, y)` in metres from the pole, `x`
/// positive for longitudes 0 to 180 and `y` positive towards longitude 180 for the north
/// cap, towards longitude 0 for the south, at central scale `k0`.
fn polar_stereographic(north: bool, lat_deg: f64, lon_deg: f64, k0: f64) -> (f64, f64) {
    let lat = if north { lat_deg } else { -lat_deg };
    // The radius of the circle of latitude, over `2 k0 a / c`, in the conformal form
    // `sqrt(1 + t'^2) - t'`: written as a reciprocal when `t'` is positive, so it does
    // not subtract two nearly equal numbers near the pole.
    let rho_over = if (lat - 90.0).abs() == 0.0 {
        0.0
    } else {
        let taup = conformal_tau(tan_deg(lat));
        if taup >= 0.0 {
            1.0 / (taup.hypot(1.0) + taup)
        } else {
            taup.hypot(1.0) - taup
        }
    };
    let e = eccentricity();
    let c = (1.0 - WGS84_F) * (e * e.atanh()).exp();
    let rho = rho_over * 2.0 * k0 * WGS84_A / c;
    let (sin_lon, cos_lon) = sin_cos_deg(lon_deg);
    (
        rho * sin_lon,
        if north { -rho * cos_lon } else { rho * cos_lon },
    )
}

/// The standard UTM zone of a position, with the Norway and Svalbard exceptions; 0 for a
/// position UPS covers.
fn standard_zone(lat_deg: f64, lon_deg: f64) -> u8 {
    if !(UTM_SOUTH_LIMIT_DEG..UTM_NORTH_LIMIT_DEG).contains(&lat_deg) {
        return 0;
    }
    // `floor` of a longitude folded into [-180, 180); an exact 180 is -180.
    let mut ilon = lon_deg.floor();
    if ilon >= 180.0 {
        ilon -= 360.0;
    } else if ilon < -180.0 {
        ilon += 360.0;
    }
    // `ilon` is in [-180, 180).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut zone = ((ilon + 186.0) / 6.0).floor() as u8;
    if (56.0..64.0).contains(&lat_deg) && (3.0..12.0).contains(&ilon) {
        zone = 32;
    } else if (72.0..84.0).contains(&lat_deg) {
        if (0.0..9.0).contains(&ilon) {
            zone = 31;
        } else if (9.0..21.0).contains(&ilon) {
            zone = 33;
        } else if (21.0..33.0).contains(&ilon) {
            zone = 35;
        } else if (33.0..42.0).contains(&ilon) {
            zone = 37;
        }
    }
    zone
}

/// A longitude folded into [-180, 180], as an offset from `central_deg`.
fn offset_from_deg(lon_deg: f64, central_deg: f64) -> f64 {
    let mut d = (lon_deg - central_deg) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

/// A latitude and longitude in degrees as UTM, or as UPS outside UTM's latitudes.
///
/// # Errors
///
/// [`GridError::NotFinite`] for a NaN or an infinity, [`GridError::LatitudeOutOfRange`]
/// beyond the poles.
pub fn utm_ups(lat_deg: f64, lon_deg: f64) -> Result<UtmUps, GridError> {
    if !lat_deg.is_finite() || !lon_deg.is_finite() {
        return Err(GridError::NotFinite);
    }
    if lat_deg.abs() > 90.0 {
        return Err(GridError::LatitudeOutOfRange);
    }
    let zone = standard_zone(lat_deg, lon_deg);
    let north = !lat_deg.is_sign_negative();
    if zone == 0 {
        let (x, y) = polar_stereographic(north, lat_deg, lon_deg, UPS_K0);
        return Ok(UtmUps {
            zone,
            north,
            easting_m: x + UPS_FALSE_M,
            northing_m: y + UPS_FALSE_M,
        });
    }
    let central_deg = f64::from(zone) * 6.0 - 183.0;
    let (x, y) = transverse_mercator(lat_deg, offset_from_deg(lon_deg, central_deg), UTM_K0)?;
    Ok(UtmUps {
        zone,
        north,
        easting_m: x + UTM_FALSE_EASTING_M,
        northing_m: y + if north {
            0.0
        } else {
            UTM_FALSE_NORTHING_SOUTH_M
        },
    })
}

/// The MGRS latitude band letter of a latitude in UTM's range.
fn latitude_band(lat_deg: f64) -> u8 {
    // A negative zero is the southern hemisphere's, so its band is the one below the
    // equator, where `GeoConvert` puts it.
    let floor = lat_deg.floor();
    #[allow(clippy::cast_possible_truncation)]
    let mut ilat = floor as i32;
    if lat_deg.is_sign_negative() && ilat == 0 {
        ilat = -1;
    }
    // Integer division truncates toward zero; below the equator the band edge is eight
    // degrees from the one above it, so the shift by seven makes it floor.
    let band = (if ilat < 0 { ilat - 7 } else { ilat } / 8).clamp(-10, 9);
    // `band` is in -10..=9.
    #[allow(clippy::cast_sign_loss)]
    LATITUDE_BANDS[(band + 10) as usize]
}

/// A latitude and longitude in degrees as an MGRS reference, truncated to `digits` digits
/// for each axis: 5 is one metre, 4 is ten, down to 0 for the 100 km square alone.
///
/// The reference has no spaces: `10TDQ1234567890`.
///
/// # Errors
///
/// The errors of [`utm_ups`], and [`GridError::PrecisionOutOfRange`] for more than five
/// digits.
pub fn mgrs(lat_deg: f64, lon_deg: f64, digits: u8) -> Result<String, GridError> {
    if digits > 5 {
        return Err(GridError::PrecisionOutOfRange);
    }
    let p = utm_ups(lat_deg, lon_deg)?;
    // Truncated to a whole metre, then to the precision asked for, never rounded.
    let metres = |v: f64| v.floor();
    let divisor = 10_f64.powi(5 - i32::from(digits));
    // A southern position on the equator has the false northing exactly. MGRS keeps it in
    // the southern grid, in the last square before the northern one, as `GeoConvert` does.
    let northing_m = if p.zone != 0 && !p.north {
        p.northing_m
            .min(UTM_FALSE_NORTHING_SOUTH_M - SOUTH_EQUATOR_MARGIN_M)
    } else {
        p.northing_m
    };
    let (x, y) = (metres(p.easting_m), metres(northing_m));
    let square = |v: f64| -> usize {
        // Both coordinates are positive and below 10^8.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let s = (v / SQUARE_M).floor() as usize;
        s
    };
    let within = |v: f64| -> u64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let w = ((v % SQUARE_M) / divisor).floor() as u64;
        w
    };
    let mut out = String::with_capacity(15);
    let (col, row);
    if p.is_ups() {
        let half = usize::from(p.north) * 2 + usize::from(p.easting_m >= UPS_FALSE_M);
        out.push(char::from(UPS_BANDS[half]));
        let cap = usize::from(p.north);
        col = square(x)
            .checked_sub(UPS_FIRST_COLUMN[half])
            .and_then(|i| UPS_COLUMNS[half].get(i))
            .copied();
        row = square(y)
            .checked_sub(UPS_FIRST_ROW[cap])
            .and_then(|i| UPS_ROWS[cap].get(i))
            .copied();
    } else {
        let _ = write!(out, "{:02}", p.zone);
        out.push(char::from(latitude_band(lat_deg)));
        let zone_index = usize::from(p.zone) - 1;
        col = square(x)
            .checked_sub(1)
            .and_then(|i| UTM_COLUMNS[zone_index % 3].get(i))
            .copied();
        let shift = if p.zone % 2 == 0 {
            UTM_EVEN_ZONE_ROW_SHIFT
        } else {
            0
        };
        row = Some(UTM_ROWS[(square(y) + shift) % UTM_ROWS.len()]);
    }
    // A square outside the lettered grid is a position outside MGRS's limits, which only
    // a point at the very edge of a polar cap's grid could be.
    let (Some(col), Some(row)) = (col, row) else {
        return Err(GridError::OutsideZone);
    };
    out.push(char::from(col));
    out.push(char::from(row));
    if digits > 0 {
        let width = usize::from(digits);
        let _ = write!(out, "{:0width$}{:0width$}", within(x), within(y));
    }
    Ok(out)
}

/// A geodetic position as MGRS to `digits` digits for each axis; see [`mgrs`].
///
/// # Errors
///
/// The errors of [`mgrs`].
pub fn mgrs_of(position: Geodetic, digits: u8) -> Result<String, GridError> {
    mgrs(
        position.lat_rad.to_degrees(),
        position.lon_rad.to_degrees(),
        digits,
    )
}
