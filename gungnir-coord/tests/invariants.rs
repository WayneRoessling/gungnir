// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `proptest` invariants for the verification-capability-table.md §1 row
//! "Coordinate frame transforms (ECEF/ENU/NED/geodetic)", the gate that sits beside
//! the pymap3d differential test (`pymap3d_diff.rs`).
//!
//! These are properties the oracle cannot check for us: that the four transforms are
//! mutual inverses to within the row's 1e-6 m, that the tangent-plane rotation is an
//! isometry, and that no input in the declared domain produces a NaN or an infinity.
//! The domain deliberately runs to the poles and past the antimeridian, per
//! `docs/scenario-crate-narrative.md` Scenario 5.

use gungnir_coord::{
    enu_to_ned, geodetic_to_ned, ned_to_enu, CoordTransform, Ecef, Enu, Geodetic, Wgs84, WGS84_A,
};
use proptest::prelude::*;

/// Round-trip tolerance, metres: the same criterion the capability-table row states.
const TOL_M: f64 = 1e-6;

fn geodetic_anywhere() -> impl Strategy<Value = Geodetic> {
    let half_pi = std::f64::consts::FRAC_PI_2;
    (
        -half_pi..=half_pi,
        -std::f64::consts::PI..=std::f64::consts::PI,
        -11_000.0_f64..1_000_000.0,
    )
        .prop_map(|(lat_rad, lon_rad, alt_m)| Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
}

/// Local offsets at scenario scale: up to 500 km out and 100 km up or down.
fn enu_offset() -> impl Strategy<Value = Enu> {
    (
        -500_000.0_f64..500_000.0,
        -500_000.0_f64..500_000.0,
        -100_000.0_f64..100_000.0,
    )
        .prop_map(|(e_m, n_m, u_m)| Enu { e_m, n_m, u_m })
}

fn ecef_distance(a: Ecef, b: Ecef) -> f64 {
    let dx = a.x_m - b.x_m;
    let dy = a.y_m - b.y_m;
    let dz = a.z_m - b.z_m;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn norm(e: Ecef) -> f64 {
    (e.x_m * e.x_m + e.y_m * e.y_m + e.z_m * e.z_m).sqrt()
}

proptest! {
    /// Geodetic to ECEF and back is the identity to within the row's tolerance,
    /// measured as a position error rather than as an angle.
    #[test]
    fn geodetic_ecef_round_trip(g in geodetic_anywhere()) {
        let ecef = Wgs84::geodetic_to_ecef(g);
        let back = Wgs84::ecef_to_geodetic(ecef);
        let reprojected = Wgs84::geodetic_to_ecef(back);
        prop_assert!(
            ecef_distance(ecef, reprojected) < TOL_M,
            "round trip moved the point by {} m", ecef_distance(ecef, reprojected)
        );
    }

    /// ENU and ECEF are exact inverses about the same origin.
    #[test]
    fn enu_ecef_round_trip(origin in geodetic_anywhere(), v in enu_offset()) {
        let ecef = Wgs84::enu_to_ecef(v, origin);
        let back = Wgs84::ecef_to_enu(ecef, origin);
        let d = ((back.e_m - v.e_m).powi(2)
            + (back.n_m - v.n_m).powi(2)
            + (back.u_m - v.u_m).powi(2))
        .sqrt();
        prop_assert!(d < TOL_M, "ENU round trip moved the point by {d} m");
    }

    /// The tangent-plane rotation is an isometry: the ENU vector and the ECEF
    /// displacement it came from have the same length.
    #[test]
    fn tangent_plane_rotation_preserves_length(origin in geodetic_anywhere(), v in enu_offset()) {
        let o = Wgs84::geodetic_to_ecef(origin);
        let p = Wgs84::enu_to_ecef(v, origin);
        let displacement = Ecef { x_m: p.x_m - o.x_m, y_m: p.y_m - o.y_m, z_m: p.z_m - o.z_m };
        let enu_len = (v.e_m * v.e_m + v.n_m * v.n_m + v.u_m * v.u_m).sqrt();
        // Relative tolerance: at 500 km baselines an absolute 1e-6 m is below the
        // f64 resolution of the coordinates themselves.
        prop_assert!(
            (norm(displacement) - enu_len).abs() <= 1e-9 * enu_len.max(1.0),
            "rotation changed length: {} vs {}", norm(displacement), enu_len
        );
    }

    /// NED is a relabelling of ENU and round-trips exactly. Compared on the bit
    /// patterns, which is what "exactly" means for an f64 and what a tolerance
    /// comparison here would quietly weaken.
    #[test]
    fn ned_relabelling_is_exact(v in enu_offset()) {
        let back = ned_to_enu(enu_to_ned(v));
        prop_assert_eq!(back.e_m.to_bits(), v.e_m.to_bits());
        prop_assert_eq!(back.n_m.to_bits(), v.n_m.to_bits());
        prop_assert_eq!(back.u_m.to_bits(), v.u_m.to_bits());
    }

    /// A point placed by its ENU offset and read back as NED about the same origin
    /// returns the same offset, through the geodetic representation in between.
    #[test]
    fn geodetic_ned_round_trip(origin in geodetic_anywhere(), v in enu_offset()) {
        let g = Wgs84::ecef_to_geodetic(Wgs84::enu_to_ecef(v, origin));
        let ned = geodetic_to_ned(g, origin);
        let d = ((ned.n_m - v.n_m).powi(2)
            + (ned.e_m - v.e_m).powi(2)
            + (ned.d_m + v.u_m).powi(2))
        .sqrt();
        prop_assert!(d < TOL_M, "geodetic-NED round trip moved the point by {d} m");
    }

    /// No input in the declared domain produces a NaN or an infinity, in either
    /// direction. This is the zero-NaN/Inf half of the row's gate.
    #[test]
    fn transforms_are_finite(g in geodetic_anywhere(), origin in geodetic_anywhere(), v in enu_offset()) {
        let ecef = Wgs84::geodetic_to_ecef(g);
        prop_assert!(ecef.x_m.is_finite() && ecef.y_m.is_finite() && ecef.z_m.is_finite());

        let back = Wgs84::ecef_to_geodetic(ecef);
        prop_assert!(back.lat_rad.is_finite() && back.lon_rad.is_finite() && back.alt_m.is_finite());

        let enu = Wgs84::ecef_to_enu(ecef, origin);
        prop_assert!(enu.e_m.is_finite() && enu.n_m.is_finite() && enu.u_m.is_finite());

        let out = Wgs84::enu_to_ecef(v, origin);
        prop_assert!(out.x_m.is_finite() && out.y_m.is_finite() && out.z_m.is_finite());
    }

    /// Latitude stays inside its principal range and altitude stays physical for
    /// every point on or above the ellipsoid.
    #[test]
    fn latitude_is_principal(g in geodetic_anywhere()) {
        let back = Wgs84::ecef_to_geodetic(Wgs84::geodetic_to_ecef(g));
        prop_assert!(back.lat_rad.abs() <= std::f64::consts::FRAC_PI_2 + 1e-12);
        prop_assert!(back.lon_rad.abs() <= std::f64::consts::PI + 1e-12);
        prop_assert!(back.alt_m > -WGS84_A);
    }
}

/// The spin axis exactly: `p == 0` takes its own branch and must not divide by zero.
#[test]
fn exact_spin_axis_is_handled() {
    for z in [
        gungnir_coord::WGS84_B,
        -gungnir_coord::WGS84_B,
        gungnir_coord::WGS84_B + 1_000.0,
        1.0,
    ] {
        let g = Wgs84::ecef_to_geodetic(Ecef {
            x_m: 0.0,
            y_m: 0.0,
            z_m: z,
        });
        assert!(
            g.lat_rad.is_finite() && g.lon_rad.is_finite() && g.alt_m.is_finite(),
            "spin axis at z={z} produced a non-finite geodetic"
        );
        // Exactly a pole, not merely near one: the branch sets the constant.
        assert_eq!(
            g.lat_rad.abs().to_bits(),
            std::f64::consts::FRAC_PI_2.to_bits()
        );
    }
}

/// The geocentre is the one input with no geodetic answer; it must still not panic
/// or produce a NaN, because a corrupt detection can reach here.
#[test]
fn geocentre_does_not_panic() {
    let g = Wgs84::ecef_to_geodetic(Ecef {
        x_m: 0.0,
        y_m: 0.0,
        z_m: 0.0,
    });
    assert!(g.lat_rad.is_finite() && g.lon_rad.is_finite() && g.alt_m.is_finite());
}
