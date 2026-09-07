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
