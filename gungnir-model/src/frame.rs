// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The local ENU frame: where "the local ENU frame, metres" is anchored (GAP-007).
//!
//! `DetectionView::measurement` and `TrackView::state` are documented as being in "the
//! local ENU frame, meters", and sensors, resources and defended assets are all
//! geodetic. Nothing said where that local frame is anchored, and **nothing in the
//! workspace called `gungnir-coord`'s geodetic conversions at all** -- the functions
//! existed, tested against pymap3d, with no caller.
//!
//! That went unnoticed because nothing had yet needed to put a geodetic thing and an
//! ENU thing on the same screen. The tracking pipeline produces no tracks, so no
//! detection is ever converted; the intercept planner computes no intercept point, so
//! the plan is drawn as text beside a track rather than at a place. Drawing sensor
//! coverage is the first thing that needs both frames at once, and it is what forced
//! this.
//!
//! # Why the origin is optional and what that costs
//!
//! A deployment that has not declared its origin has no local frame, and there is no
//! sound default: guessing one -- the first sensor, the mean of the assets, the equator
//! -- would place every geodetic thing somewhere plausible and wrong, which on a map is
//! worse than placing it nowhere. So [`LocalFrame`] is constructed from a configured
//! origin or not at all, and callers that cannot get one say so rather than drawing.

use crate::Geodetic;
use serde::{Deserialize, Serialize};

/// The anchor of the local ENU frame every ENU coordinate in the system is relative to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LocalFrame {
    /// Geodetic position of the ENU origin.
    pub origin: Geodetic,
}

impl LocalFrame {
    #[must_use]
    pub fn new(origin: Geodetic) -> Self {
        Self { origin }
    }

    /// A geodetic position as `[e, n, u]` metres in this frame.
    ///
    /// Exact for the purposes it is used for: `gungnir-coord`'s conversion is gated
    /// against pymap3d to 1e-6 m, which is far finer than anything drawn on a map.
    #[must_use]
    pub fn to_enu(&self, position: Geodetic) -> [f64; 3] {
        let ned = gungnir_coord::geodetic_to_ned(position, self.origin);
        let enu = gungnir_coord::ned_to_enu(ned);
        [enu.e_m, enu.n_m, enu.u_m]
    }

    /// The inverse, for anything that has to report a local position geodetically.
    #[must_use]
    pub fn to_geodetic(&self, enu: [f64; 3]) -> Geodetic {
        let ned = gungnir_coord::enu_to_ned(gungnir_coord::Enu {
            e_m: enu[0],
            n_m: enu[1],
            u_m: enu[2],
        });
        gungnir_coord::ned_to_geodetic(ned, self.origin)
    }

    /// The bearing, in this frame, of true north at `position`: radians clockwise from the
    /// frame's `+n` axis, in `(-π, π]`.
    ///
    /// Zero at the origin and growing with distance east or west of it (the meridians
    /// converge), about 0.1 degree eleven kilometres east of an origin at 45 degrees north.
    /// A sensor's sector is surveyed against true north at the sensor, so a sector placed
    /// in this frame without this rotation would be off by that much
    /// (`docs/design/DN-12-coverage-and-gaps.md` amendment 1, D-84).
    ///
    /// Taken numerically from this frame's own conversion -- the frame direction of a step
    /// due north of `position` -- so it is exactly the frame the coverage is computed in.
    #[must_use]
    pub fn true_north_at(&self, position: Geodetic) -> f64 {
        // A step of 1e-6 rad is about 6.4 m; `to_enu` is gated to 1e-6 m, so the bearing
        // it gives is good to about 1e-7 rad. Stepped south instead at the pole, where
        // north is not a direction.
        const STEP_RAD: f64 = 1e-6;
        let (step, sign) = if position.lat_rad + STEP_RAD <= std::f64::consts::FRAC_PI_2 {
            (STEP_RAD, 1.0)
        } else {
            (-STEP_RAD, -1.0)
        };
        let here = self.to_enu(position);
        let there = self.to_enu(Geodetic {
            lat_rad: position.lat_rad + step,
            ..position
        });
        let (de, dn) = (sign * (there[0] - here[0]), sign * (there[1] - here[1]));
        if !(de.is_finite() && dn.is_finite()) || (de == 0.0 && dn == 0.0) {
            return 0.0;
        }
        de.atan2(dn)
    }

    /// A sector surveyed against true north at `position`, as bearings in this frame.
    #[must_use]
    pub fn sector_in_frame(&self, sector: AzimuthSector, position: Geodetic) -> AzimuthSector {
        sector.rotated(self.true_north_at(position))
    }
}

/// Bearing of the horizontal direction `(east, north)`: radians clockwise from north, in
/// `[0, 2π)`. `None` for the zero vector, which has no bearing, or a non-finite one.
#[must_use]
pub fn bearing_rad(east: f64, north: f64) -> Option<f64> {
    if !(east.is_finite() && north.is_finite()) || (east == 0.0 && north == 0.0) {
        return None;
    }
    Some(normalize_bearing(east.atan2(north)))
}

/// An angle as a bearing in `[0, 2π)`.
#[must_use]
pub fn normalize_bearing(angle_rad: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let b = angle_rad.rem_euclid(tau);
    // `rem_euclid` can return `tau` itself for a tiny negative input, by rounding.
    if b >= tau {
        0.0
    } else {
        b
    }
}

/// The horizontal bearings a sensor can see: a sector centred on its boresight
/// (GAP-118, D-84, `docs/design/DN-12-coverage-and-gaps.md` amendment 1).
///
/// Bearings are radians clockwise from north. In a baseline -- a sensor's declaration or
/// a laydown's placement -- north is **true north at the sensor**, because that is what a
/// sector is surveyed against; [`LocalFrame::sector_in_frame`] turns it into bearings in
/// the local frame, which is what a `CoverageVolume` holds.
///
/// **A sensor with no sector sees the full circle**, and that is spelled `None` wherever a
/// sector is optional, rather than a sector of width 2π by default. A 2π sector is also
/// legal and means the same thing, for a baseline that wants to say so.
///
/// A sector may straddle north: a boresight of 350 degrees and a width of 40 degrees
/// covers 330 degrees through north to 10 degrees, and [`AzimuthSector::contains`]
/// answers that across the wrap.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AzimuthSector {
    /// Centre of the sector, radians clockwise from north. Any finite value; it is read
    /// modulo 2π, so -10 degrees and 350 degrees are the same boresight.
    pub boresight_rad: f64,
    /// Full width of the sector, radians, in `(0, 2π]`.
    pub width_rad: f64,
}

/// Why a sector is refused.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum SectorError {
    #[error("the sector's boresight is not a finite number")]
    NonFiniteBoresight,
    /// A width of zero sees nothing and more than 2π is not a sector; a sensor that sees
    /// the full circle declares none, or states 2π.
    #[error("the sector's width is {0} rad; it must be greater than zero and at most 2π")]
    WidthOutOfRange(f64),
}

impl AzimuthSector {
    /// A sector, refused unless its boresight is finite and its width is in `(0, 2π]`.
    ///
    /// # Errors
    ///
    /// [`SectorError`] naming which of the two is wrong.
    pub fn new(boresight_rad: f64, width_rad: f64) -> Result<Self, SectorError> {
        let sector = Self {
            boresight_rad,
            width_rad,
        };
        sector.validate()?;
        Ok(sector)
    }

    /// The check [`AzimuthSector::new`] applies, for a sector that arrived by
    /// deserialization.
    ///
    /// # Errors
    ///
    /// [`SectorError`] naming which of the two is wrong.
    pub fn validate(&self) -> Result<(), SectorError> {
        if !self.boresight_rad.is_finite() {
            return Err(SectorError::NonFiniteBoresight);
        }
        if !(self.width_rad.is_finite()
            && self.width_rad > 0.0
            && self.width_rad <= std::f64::consts::TAU)
        {
            return Err(SectorError::WidthOutOfRange(self.width_rad));
        }
        Ok(())
    }

    /// True when the sector is the whole circle.
    #[must_use]
    pub fn is_full_circle(&self) -> bool {
        self.width_rad >= std::f64::consts::TAU
    }

    /// The boresight as a bearing in `[0, 2π)`.
    #[must_use]
    pub fn boresight(&self) -> f64 {
        normalize_bearing(self.boresight_rad)
    }

    /// The sector's anticlockwise edge -- where a sweep clockwise through it starts -- as a
    /// bearing in `[0, 2π)`.
    #[must_use]
    pub fn start_rad(&self) -> f64 {
        normalize_bearing(self.boresight_rad - self.width_rad / 2.0)
    }

    /// The sector's clockwise edge, as a bearing in `[0, 2π)`.
    #[must_use]
    pub fn end_rad(&self) -> f64 {
        normalize_bearing(self.boresight_rad + self.width_rad / 2.0)
    }

    /// Whether `bearing_rad` (clockwise from the same north) lies in the sector, edges
    /// included. Handles the wrap through north. A non-finite bearing is in no sector,
    /// and a sector that fails [`AzimuthSector::validate`] contains nothing.
    #[must_use]
    pub fn contains(&self, bearing_rad: f64) -> bool {
        if self.validate().is_err() || !bearing_rad.is_finite() {
            return false;
        }
        if self.is_full_circle() {
            return true;
        }
        let pi = std::f64::consts::PI;
        // Signed offset from the boresight in [-π, π).
        let offset = (bearing_rad - self.boresight_rad + pi).rem_euclid(std::f64::consts::TAU) - pi;
        // Edges included, to a nanoradian: a bearing stated on the edge in degrees and
        // converted differs from the computed edge by rounding, and must not fall out.
        offset.abs() <= self.width_rad / 2.0 + 1e-9
    }

    /// The same sector turned clockwise by `by_rad`.
    #[must_use]
    pub fn rotated(&self, by_rad: f64) -> Self {
        Self {
            boresight_rad: normalize_bearing(self.boresight_rad + by_rad),
            width_rad: self.width_rad,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Geodetic {
        Geodetic {
            lat_rad: 55.0_f64.to_radians(),
            lon_rad: 12.0_f64.to_radians(),
            alt_m: 0.0,
        }
    }

    /// The origin is the origin. A frame that put its own anchor anywhere but zero
    /// would offset every geodetic thing on the map by a constant nobody could see.
    #[test]
    fn the_origin_is_at_zero() {
        let frame = LocalFrame::new(origin());
        let enu = frame.to_enu(origin());
        for (axis, v) in ["e", "n", "u"].iter().zip(enu) {
            assert!(v.abs() < 1e-6, "the origin is offset on {axis}: {v}");
        }
    }

    /// East is +e and north is +n, which is the whole content of the frame's name and
    /// the thing that would silently mirror the map if it were wrong.
    #[test]
    fn east_is_positive_e_and_north_is_positive_n() {
        let frame = LocalFrame::new(origin());
        let east = frame.to_enu(Geodetic {
            lon_rad: 12.01_f64.to_radians(),
            ..origin()
        });
        assert!(east[0] > 0.0, "east was not +e: {east:?}");
        assert!(east[1].abs() < east[0] * 0.01, "east leaked into north");

        let north = frame.to_enu(Geodetic {
            lat_rad: 55.01_f64.to_radians(),
            ..origin()
        });
        assert!(north[1] > 0.0, "north was not +n: {north:?}");
        assert!(north[0].abs() < north[1] * 0.01, "north leaked into east");

        let up = frame.to_enu(Geodetic {
            alt_m: 500.0,
            ..origin()
        });
        assert!((up[2] - 500.0).abs() < 1e-3, "up was not +u: {up:?}");
    }

    /// The round trip closes, so anything reported back geodetically is the position it
    /// came from rather than an accumulated drift.
    #[test]
    fn the_round_trip_closes() {
        let frame = LocalFrame::new(origin());
        for position in [
            Geodetic {
                lat_rad: 55.2_f64.to_radians(),
                lon_rad: 12.3_f64.to_radians(),
                alt_m: 1_200.0,
            },
            Geodetic {
                lat_rad: 54.7_f64.to_radians(),
                lon_rad: 11.6_f64.to_radians(),
                alt_m: -30.0,
            },
        ] {
            let back = frame.to_geodetic(frame.to_enu(position));
            assert!(
                (back.lat_rad - position.lat_rad).abs() < 1e-12,
                "latitude drifted"
            );
            assert!(
                (back.lon_rad - position.lon_rad).abs() < 1e-12,
                "longitude drifted"
            );
            assert!(
                (back.alt_m - position.alt_m).abs() < 1e-6,
                "altitude drifted"
            );
        }
    }

    fn sector(boresight_deg: f64, width_deg: f64) -> AzimuthSector {
        AzimuthSector::new(boresight_deg.to_radians(), width_deg.to_radians())
            .expect("a legal sector")
    }

    /// A sector straddling north covers both sides of it and nothing opposite.
    #[test]
    fn a_sector_across_north_wraps() {
        let s = sector(350.0, 40.0);
        for inside in [330.0, 340.0, 359.9, 0.0, 5.0, 10.0] {
            assert!(
                s.contains(f64::to_radians(inside)),
                "{inside} deg is inside 330..10"
            );
        }
        for outside in [329.8, 10.2, 90.0, 170.0, 180.0, 270.0] {
            assert!(
                !s.contains(f64::to_radians(outside)),
                "{outside} deg is outside 330..10"
            );
        }
        // The same boresight written as a negative angle is the same sector.
        let negative = sector(-10.0, 40.0);
        assert!(negative.contains(5.0_f64.to_radians()));
        assert!(!negative.contains(15.0_f64.to_radians()));
        assert!((s.start_rad() - 330.0_f64.to_radians()).abs() < 1e-12);
        assert!((s.end_rad() - 10.0_f64.to_radians()).abs() < 1e-12);
    }

    /// A sector's width must be in (0, 2π]: zero sees nothing and cannot be what anyone
    /// meant, and more than the circle is not a sector. Full width is legal and total.
    #[test]
    fn the_width_is_refused_outside_zero_to_the_full_circle() {
        for bad in [0.0, -0.1, 7.0, f64::NAN, f64::INFINITY] {
            assert!(
                matches!(
                    AzimuthSector::new(0.0, bad),
                    Err(SectorError::WidthOutOfRange(_))
                ),
                "width {bad} was accepted"
            );
        }
        assert_eq!(
            AzimuthSector::new(f64::NAN, 1.0),
            Err(SectorError::NonFiniteBoresight)
        );
        let full = AzimuthSector::new(1.0, std::f64::consts::TAU).expect("the full circle");
        assert!(full.is_full_circle());
        for b in [0.0, 1.0, 3.0, 6.0] {
            assert!(full.contains(b));
        }
        // A sector built around the check contains nothing rather than something wrong.
        let forged = AzimuthSector {
            boresight_rad: 0.0,
            width_rad: 0.0,
        };
        assert!(!forged.contains(0.0));
        assert!(!sector(90.0, 10.0).contains(f64::NAN));
    }

    #[test]
    fn a_bearing_is_clockwise_from_north() {
        let deg = |e: f64, n: f64| bearing_rad(e, n).map(f64::to_degrees);
        assert!((deg(0.0, 1.0).expect("north") - 0.0).abs() < 1e-12);
        assert!((deg(1.0, 0.0).expect("east") - 90.0).abs() < 1e-12);
        assert!((deg(0.0, -1.0).expect("south") - 180.0).abs() < 1e-12);
        assert!((deg(-1.0, 0.0).expect("west") - 270.0).abs() < 1e-12);
        assert!(
            bearing_rad(0.0, 0.0).is_none(),
            "straight up has no bearing"
        );
    }

    /// True north at the origin is the frame's north. East of it the meridians lean
    /// toward the pole, so true north turns anticlockwise in the frame (a negative
    /// bearing) by the convergence, Δλ sin φ to first order; west, clockwise.
    #[test]
    fn true_north_departs_from_the_frame_by_the_meridian_convergence() {
        let frame = LocalFrame::new(origin());
        assert!(frame.true_north_at(origin()).abs() < 1e-6);
        let d_lon = 0.5_f64.to_radians();
        let expected = d_lon * origin().lat_rad.sin();
        let east = frame.true_north_at(Geodetic {
            lon_rad: origin().lon_rad + d_lon,
            ..origin()
        });
        let west = frame.true_north_at(Geodetic {
            lon_rad: origin().lon_rad - d_lon,
            ..origin()
        });
        assert!(
            (east + expected).abs() < 1e-4,
            "east: {east} rad, expected {}",
            -expected
        );
        assert!((west - expected).abs() < 1e-4, "west: {west} rad");
        // The sector follows: a sector surveyed on true north, placed east of the origin,
        // is rotated by the same amount in the frame.
        let placed = frame.sector_in_frame(
            sector(90.0, 30.0),
            Geodetic {
                lon_rad: origin().lon_rad + d_lon,
                ..origin()
            },
        );
        assert!((placed.boresight() - (90.0_f64.to_radians() + east)).abs() < 1e-12);
    }

    /// A sanity check against a distance anyone can verify: 0.01 degrees of latitude is
    /// about 1.11 km, whatever the longitude.
    #[test]
    fn a_tenth_of_a_degree_is_the_distance_it_should_be() {
        let frame = LocalFrame::new(origin());
        let north = frame.to_enu(Geodetic {
            lat_rad: 55.1_f64.to_radians(),
            ..origin()
        });
        // 0.1 degrees of latitude is 11.1 km to within the flattening.
        assert!(
            (north[1] - 11_120.0).abs() < 60.0,
            "0.1 degrees north came out as {} m",
            north[1]
        );
    }
}
