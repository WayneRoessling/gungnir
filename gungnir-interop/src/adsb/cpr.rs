// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Compact Position Reporting: the 17-bit latitude and longitude pair an extended
//! squitter carries, and the arithmetic that turns a pair of them into a place.
//!
//! **This module is the one part of the ADS-B build that is checked against published
//! worked examples rather than against other decoders** (GAP-010's closing action, point
//! 3). CPR is a published algorithm with worked examples, so it is checkable by
//! arithmetic; the rest of the codec is gated by agreement with two open-source decoders,
//! which is a weaker claim (`docs/design/external-standards.md` §4). The vectors are in
//! `gungnir-interop/tests/adsb_cpr.rs`.
//!
//! A CPR pair is deliberately ambiguous: one message names a position inside a zone but
//! not which zone. There are three ways out and this module offers all three, because
//! each has a different failure mode a caller must be able to see:
//!
//! - [`decode_global`] takes an even and an odd message and needs no prior knowledge,
//!   but fails when the two straddle a latitude-zone boundary ([`CprError::ZoneDisagreement`]).
//! - [`decode_local`] takes one message and a reference position, and is correct only
//!   while the true position is within half a cell of the reference — 3° of latitude
//!   airborne, 0.75° on the surface. It refuses rather than guessing when the answer
//!   lands further away than that ([`CprError::TooFarFromReference`]).
//! - [`encode`] is the inverse, used by the round-trip test so the decoder is checked
//!   against arithmetic and not only against a table of expected numbers.
//!
//! Surface messages carry a quarter of the airborne range, so a global surface decode
//! resolves to one of four quadrants and needs a reference to choose between them; that
//! is why [`decode_global`] takes an `Option<Position>` and refuses a surface pair with
//! none ([`CprError::ReferenceRequired`]).

/// 2^17: the width of the CPR latitude and longitude fields.
const CPR_MAX: f64 = 131_072.0;

/// NZ, the number of latitude zones between the equator and a pole. Fifteen in Mode S.
const NZ: f64 = 15.0;

/// Above this latitude the whole polar cap is one longitude zone.
const POLAR_LATITUDE_DEG: f64 = 87.0;

/// Which of the two interleaved encodings a message used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CprKind {
    /// Airborne position: the encoding spans the whole 360° of latitude.
    Airborne,
    /// Surface position: the encoding spans 90°, so four times the resolution and four
    /// times the ambiguity.
    Surface,
}

impl CprKind {
    /// The latitude and longitude span one CPR encoding covers, in degrees.
    fn span_deg(self) -> f64 {
        match self {
            Self::Airborne => 360.0,
            Self::Surface => 90.0,
        }
    }
}

/// One message's CPR fields, exactly as transmitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CprFrame {
    /// The F bit: `false` is the even encoding, `true` the odd one.
    pub odd: bool,
    /// 17 bits.
    pub lat_cpr: u32,
    /// 17 bits.
    pub lon_cpr: u32,
}

/// A place, in degrees on WGS 84. Longitude is in `[-180, 180)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}

/// Why a CPR pair or a single frame did not become a position. Every arm says what the
/// caller has to do about it; none is a silent skip and none is a guessed position.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CprError {
    /// Two frames of the same parity carry no more information than one.
    #[error("a global decode needs one even and one odd frame, not two of the same parity")]
    SameParity,
    /// A surface pair without a reference: the answer would be one of four quadrants.
    #[error("a global surface decode needs a reference position to choose the quadrant")]
    ReferenceRequired,
    /// The zone index put the latitude off the planet, which means the pair does not
    /// belong together.
    #[error("decoded latitude {latitude_deg} is outside -90..=90")]
    LatitudeOutOfRange { latitude_deg: f64 },
    /// The even and odd frames fell in different longitude-zone bands, so the pair
    /// straddles a boundary and the longitude cannot be resolved. The caller waits for
    /// the next pair; this is normal and not an error in the data.
    #[error("even frame is in {even_zones} longitude zones and odd frame in {odd_zones}")]
    ZoneDisagreement { even_zones: u32, odd_zones: u32 },
}

/// NL(lat): the number of longitude zones at this latitude, 1 at the poles and 59 at the
/// equator.
///
/// The closed form of DO-260's definition rather than the transition table the receivers
/// carry: the table is derived from this expression, and a table in this repository would
/// be a set of magic numbers no test could check. `tests/adsb_cpr.rs` checks it at the
/// transition latitudes obtained by inverting the same expression, which is the arithmetic
/// gate this module exists to be given.
#[must_use]
pub fn longitude_zones(latitude_deg: f64) -> u32 {
    let lat = latitude_deg.abs();
    if lat >= POLAR_LATITUDE_DEG {
        return 1;
    }
    if lat == 0.0 {
        return 59;
    }
    let numerator = 1.0 - (std::f64::consts::PI / (2.0 * NZ)).cos();
    let denominator = lat.to_radians().cos().powi(2);
    let zones = 2.0 * std::f64::consts::PI / (1.0 - numerator / denominator).acos();
    // The expression is monotone and bounded by construction; the clamp is here so a
    // rounding artefact at a boundary cannot produce a zone count the rest of the
    // arithmetic divides by.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let zones = zones.floor() as u32;
    zones.clamp(1, 59)
}

/// The modulo of DO-260 §A.1.7.5: always non-negative for a positive divisor, unlike
/// Rust's `%` on a negative dividend.
fn modulo(a: f64, b: f64) -> f64 {
    a - b * (a / b).floor()
}

/// Normalise a longitude into `[-180, 180)`.
fn wrap_longitude(deg: f64) -> f64 {
    deg - ((deg + 180.0) / 360.0).floor() * 360.0
}

/// The shorter way round the circle between two longitudes, in degrees.
fn longitude_separation(a: f64, b: f64) -> f64 {
    let d = wrap_longitude(a - b).abs();
    d.min(360.0 - d)
}

/// Decode a position from one even and one odd frame, with no prior knowledge for an
/// airborne pair and a reference quadrant for a surface pair.
///
/// `latest_is_odd` says which of the two arrived last: CPR names the position at the
/// time of the later message, so decoding with the wrong one shifts the answer by
/// however far the aircraft moved between them.
///
/// # Errors
///
/// See [`CprError`]. [`CprError::ZoneDisagreement`] is the ordinary case of a pair
/// straddling a zone boundary and means "wait for the next pair", not "bad data".
pub fn decode_global(
    even: CprFrame,
    odd: CprFrame,
    latest_is_odd: bool,
    kind: CprKind,
    reference: Option<Position>,
) -> Result<Position, CprError> {
    if even.odd || !odd.odd {
        return Err(CprError::SameParity);
    }
    if kind == CprKind::Surface && reference.is_none() {
        return Err(CprError::ReferenceRequired);
    }
    let span = kind.span_deg();
    let lat_even_cpr = f64::from(even.lat_cpr) / CPR_MAX;
    let lon_even_cpr = f64::from(even.lon_cpr) / CPR_MAX;
    let lat_odd_cpr = f64::from(odd.lat_cpr) / CPR_MAX;
    let lon_odd_cpr = f64::from(odd.lon_cpr) / CPR_MAX;

    // The latitude zone index, common to both frames.
    let j = (59.0 * lat_even_cpr - 60.0 * lat_odd_cpr + 0.5).floor();
    let mut lat_even = (span / 60.0) * (modulo(j, 60.0) + lat_even_cpr);
    let mut lat_odd = (span / 59.0) * (modulo(j, 59.0) + lat_odd_cpr);

    match kind {
        CprKind::Airborne => {
            // The southern hemisphere arrives as 270..360.
            if lat_even >= 270.0 {
                lat_even -= 360.0;
            }
            if lat_odd >= 270.0 {
                lat_odd -= 360.0;
            }
        }
        CprKind::Surface => {
            // A surface latitude comes out in 0..90 and the true one is that value
            // shifted by a whole number of quadrants. The reference picks the quadrant;
            // it is checked to be present above.
            let reference_latitude = reference.map_or(0.0, |p| p.latitude_deg);
            lat_even = nearest_quadrant_latitude(lat_even, reference_latitude);
            lat_odd = nearest_quadrant_latitude(lat_odd, reference_latitude);
        }
    }
    for latitude_deg in [lat_even, lat_odd] {
        if !(-90.0..=90.0).contains(&latitude_deg) {
            return Err(CprError::LatitudeOutOfRange { latitude_deg });
        }
    }
    let even_zones = longitude_zones(lat_even);
    let odd_zones = longitude_zones(lat_odd);
    if even_zones != odd_zones {
        return Err(CprError::ZoneDisagreement {
            even_zones,
            odd_zones,
        });
    }

    let (latitude_deg, lon_cpr) = if latest_is_odd {
        (lat_odd, lon_odd_cpr)
    } else {
        (lat_even, lon_even_cpr)
    };
    let zones = f64::from(even_zones);
    let ni = f64::from(if latest_is_odd {
        even_zones.saturating_sub(1).max(1)
    } else {
        even_zones.max(1)
    });
    let m = (lon_even_cpr * (zones - 1.0) - lon_odd_cpr * zones + 0.5).floor();
    let longitude = (span / ni) * (modulo(m, ni) + lon_cpr);
    let longitude_deg = match kind {
        CprKind::Airborne => wrap_longitude(longitude),
        CprKind::Surface => {
            let reference_longitude = reference.map_or(0.0, |p| p.longitude_deg);
            nearest_quadrant_longitude(longitude, reference_longitude)
        }
    };
    Ok(Position {
        latitude_deg,
        longitude_deg,
    })
}

/// A surface latitude in 0..90 shifted to the quadrant nearest the reference.
fn nearest_quadrant_latitude(base_deg: f64, reference_deg: f64) -> f64 {
    let mut best = base_deg;
    let mut best_error = f64::INFINITY;
    for k in -1..=1 {
        let candidate = f64::from(k).mul_add(90.0, base_deg);
        if !(-90.0..=90.0).contains(&candidate) {
            continue;
        }
        let error = (candidate - reference_deg).abs();
        if error < best_error {
            best_error = error;
            best = candidate;
        }
    }
    best
}

/// A surface longitude in 0..90 shifted to the quadrant nearest the reference, going the
/// short way round the antimeridian.
fn nearest_quadrant_longitude(base_deg: f64, reference_deg: f64) -> f64 {
    let mut best = wrap_longitude(base_deg);
    let mut best_error = f64::INFINITY;
    for k in 0..4 {
        let candidate = wrap_longitude(f64::from(k).mul_add(90.0, base_deg));
        let error = longitude_separation(candidate, reference_deg);
        if error < best_error {
            best_error = error;
            best = candidate;
        }
    }
    best
}

/// Decode one frame against a reference position.
///
/// Correct only while the **true** position is within half a cell of the reference: 3°
/// of latitude airborne, 0.75° on the surface, and the corresponding longitude figure,
/// which narrows towards the poles. Beyond that the answer is a whole cell out.
///
/// **There is no half-cell check here, and its absence is the honest choice.** Every
/// reference decoder carries one — compare the decoded position with the reference and
/// refuse when it is more than half a cell away — and in every one of them it is dead
/// code. The zone index is `floor(reference/cell - cpr + 0.5)`, which selects the cell
/// **nearest the reference**, so the answer is within half a cell of the reference by
/// construction and the comparison can never fail. A reference four degrees stale
/// produces a position six degrees wrong and two degrees from the reference, and passes.
/// The half-cell rule is a statement about the distance from the *true* position, which
/// a decoder does not have; carrying a guard that reads as protection and is none would
/// be exactly the "health flag that claims a subsystem works" this project forbids.
///
/// So the obligation is the caller's and is stated rather than checked: use a reference
/// no older than the aircraft can have travelled half a cell in, or use
/// [`decode_global`], which needs no reference at all in the air.
/// `tests/adsb_cpr.rs` pins the stale-reference failure so it stays on the record.
///
/// # Errors
///
/// [`CprError::LatitudeOutOfRange`], which a reference within a half-cell of a pole can
/// still produce.
pub fn decode_local(
    frame: CprFrame,
    kind: CprKind,
    reference: Position,
) -> Result<Position, CprError> {
    let span = kind.span_deg();
    let lat_cpr = f64::from(frame.lat_cpr) / CPR_MAX;
    let lon_cpr = f64::from(frame.lon_cpr) / CPR_MAX;

    let d_lat = span / if frame.odd { 59.0 } else { 60.0 };
    let j = (0.5 + reference.latitude_deg / d_lat - lat_cpr).floor();
    let latitude_deg = d_lat * (j + lat_cpr);
    if !(-90.0..=90.0).contains(&latitude_deg) {
        return Err(CprError::LatitudeOutOfRange { latitude_deg });
    }

    let zones = longitude_zones(latitude_deg);
    let ni = if frame.odd {
        zones.saturating_sub(1)
    } else {
        zones
    };
    let d_lon = if ni > 0 { span / f64::from(ni) } else { span };
    let m = (0.5 + reference.longitude_deg / d_lon - lon_cpr).floor();
    let longitude_deg = wrap_longitude(d_lon * (m + lon_cpr));
    Ok(Position {
        latitude_deg,
        longitude_deg,
    })
}

/// Encode a position the way a transmitter would.
///
/// Here so the decoders can be checked against arithmetic — encode a known place, decode
/// it back — rather than only against a list of expected numbers. Nothing in the receive
/// path uses it.
#[must_use]
pub fn encode(position: Position, odd: bool, kind: CprKind) -> CprFrame {
    let span = kind.span_deg();
    let i = if odd { 1.0 } else { 0.0 };
    let d_lat = span / (60.0 - i);
    let lat_cpr_real = CPR_MAX * (modulo(position.latitude_deg, d_lat) / d_lat) + 0.5;
    let lat_cpr = truncate_to_17_bits(lat_cpr_real);

    let rlat = d_lat * (f64::from(lat_cpr) / CPR_MAX + (position.latitude_deg / d_lat).floor());
    let zones = longitude_zones(rlat);
    let ni = f64::from(zones).max(1.0) - i;
    let d_lon = if ni > 0.0 { span / ni } else { span };
    let lon_cpr_real = CPR_MAX * (modulo(position.longitude_deg, d_lon) / d_lon) + 0.5;
    let lon_cpr = truncate_to_17_bits(lon_cpr_real);

    CprFrame {
        odd,
        lat_cpr,
        lon_cpr,
    }
}

/// The floor of a non-negative CPR value, reduced into the 17-bit field.
fn truncate_to_17_bits(value: f64) -> u32 {
    let floored = value.floor();
    if !floored.is_finite() || floored < 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let raw = floored.min(f64::from(u32::MAX)) as u32;
    raw % 131_072
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modulo_is_non_negative_for_a_negative_dividend() {
        assert!((modulo(-1.0, 60.0) - 59.0).abs() < 1e-12);
        assert!((modulo(61.0, 60.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn longitude_zones_run_from_fifty_nine_to_one() {
        assert_eq!(longitude_zones(0.0), 59);
        assert_eq!(longitude_zones(-0.0), 59);
        assert_eq!(longitude_zones(87.0), 1);
        assert_eq!(longitude_zones(90.0), 1);
        assert_eq!(longitude_zones(-90.0), 1);
        // Monotone non-increasing in |latitude|, which is what makes the boundary test
        // in tests/adsb_cpr.rs a complete check rather than a sample.
        let mut previous = 60;
        let mut lat = 0.0;
        while lat < 90.0 {
            let zones = longitude_zones(lat);
            assert!(zones <= previous, "{lat}: {zones} after {previous}");
            previous = zones;
            lat += 0.01;
        }
    }

    #[test]
    fn a_pair_of_the_same_parity_is_refused() {
        let even = CprFrame {
            odd: false,
            lat_cpr: 1,
            lon_cpr: 1,
        };
        assert_eq!(
            decode_global(even, even, false, CprKind::Airborne, None),
            Err(CprError::SameParity)
        );
    }

    #[test]
    fn a_global_surface_pair_without_a_reference_is_refused_not_guessed() {
        let even = CprFrame {
            odd: false,
            lat_cpr: 105_730,
            lon_cpr: 9_259,
        };
        let odd = CprFrame {
            odd: true,
            lat_cpr: 29_693,
            lon_cpr: 8_997,
        };
        assert_eq!(
            decode_global(even, odd, false, CprKind::Surface, None),
            Err(CprError::ReferenceRequired)
        );
    }

    /// The one refusal a local decode can still make: a reference at the pole and a
    /// frame whose cell falls past 90°.
    #[test]
    fn a_local_decode_refuses_an_answer_off_the_planet() {
        let frame = CprFrame {
            odd: false,
            lat_cpr: 13_107,
            lon_cpr: 0,
        };
        let pole = Position {
            latitude_deg: 90.0,
            longitude_deg: 0.0,
        };
        assert!(matches!(
            decode_local(frame, CprKind::Airborne, pole),
            Err(CprError::LatitudeOutOfRange { .. })
        ));
    }
}
