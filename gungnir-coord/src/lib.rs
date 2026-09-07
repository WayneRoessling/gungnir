//! Verifies against verification-capability-table.md: "Coordinate frame transforms
//! (ECEF/ENU/NED/geodetic)". Position error < 1e-6 m vs. pymap3d/ecef2enu/geodetic2ecef.
//! Deliberately stress-tested with an adversarial pole/antimeridian scenario -- see
//! scenario-crate-narrative.md, Scenario 5. Deterministic math: no logging here
//! (agentic-coding-standards.md §2.8).
//!
//! The position types derive `serde` because `gungnir-model` embeds `Geodetic` in
//! its canonical views, which cross the journal and API boundaries.
//!
//! # Implementation notes
//!
//! [`Wgs84::geodetic_to_ecef`] is the textbook closed form. [`Wgs84::ecef_to_geodetic`]
//! is the Heikkinen/Ferrari closed-form solution of the quartic rather than a Bowring
//! iteration: it is exact at every altitude and, unlike `h = p / cos(lat)`, does not
//! lose precision at the poles, which the Scenario 5 adversarial case drives through.
//! The local-tangent-plane rotation is built once per call from the origin's sine and
//! cosine pairs; `ecef_to_enu` and `enu_to_ecef` use the same matrix transposed, so
//! they are exact inverses of each other up to rounding.
//!
//! The oracle fixtures these are differentially tested against are checked in under
//! `testdata/oracles/coord/`; `testdata/oracles/README.md` says how to regenerate them
//! and against which pymap3d version.

pub mod bearing;

pub use bearing::{
    cross_bearings, BearingCrossing, BearingRay, CrossingError, CrossingSettings,
    DEFAULT_MINIMUM_CROSSING_ANGLE_RAD,
};

/// WGS-84 semi-major axis, metres.
pub const WGS84_A: f64 = 6_378_137.0;

/// WGS-84 flattening (reciprocal of 298.257223563).
pub const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// WGS-84 semi-minor axis, metres: `a (1 - f)`.
pub const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F);

/// WGS-84 first eccentricity squared: `f (2 - f)`.
pub const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

/// WGS-84 second eccentricity squared: `(a² - b²) / b²`.
pub const WGS84_EP2: f64 = (WGS84_A * WGS84_A - WGS84_B * WGS84_B) / (WGS84_B * WGS84_B);

/// Geodetic position: latitude/longitude in radians, altitude in meters (WGS-84).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Geodetic {
    pub lat_rad: f64,
    pub lon_rad: f64,
    pub alt_m: f64,
}

/// Earth-Centered-Earth-Fixed position, meters.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ecef {
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
}

/// East-North-Up position relative to a local tangent-plane origin, meters.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Enu {
    pub e_m: f64,
    pub n_m: f64,
    pub u_m: f64,
}

/// North-East-Down position relative to a local tangent-plane origin, meters.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ned {
    pub n_m: f64,
    pub e_m: f64,
    pub d_m: f64,
}

/// The oracle-comparable surface for coordinate transforms (agentic-coding-standards.md §1.3).
pub trait CoordTransform {
    fn geodetic_to_ecef(g: Geodetic) -> Ecef;
    fn ecef_to_geodetic(e: Ecef) -> Geodetic;
    fn ecef_to_enu(e: Ecef, origin: Geodetic) -> Enu;
    fn enu_to_ecef(v: Enu, origin: Geodetic) -> Ecef;
}

/// WGS-84 implementation of [`CoordTransform`]. Singular at the poles by construction;
/// callers near a pole should expect degraded numerical conditioning, not a panic.
pub struct Wgs84;

/// Rows of the ECEF-to-ENU rotation for a tangent plane at `origin`.
///
/// Returned as three basis vectors rather than an `nalgebra` matrix so that both
/// directions can share it without allocating or transposing a heap type.
#[inline]
fn enu_basis(origin: Geodetic) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let (sin_lat, cos_lat) = origin.lat_rad.sin_cos();
    let (sin_lon, cos_lon) = origin.lon_rad.sin_cos();
    let east = [-sin_lon, cos_lon, 0.0];
    let north = [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat];
    let up = [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat];
    (east, north, up)
}

impl CoordTransform for Wgs84 {
    /// Closed form: `N = a / sqrt(1 - e² sin²φ)`, then the standard projection.
    fn geodetic_to_ecef(g: Geodetic) -> Ecef {
        let (sin_lat, cos_lat) = g.lat_rad.sin_cos();
        let (sin_lon, cos_lon) = g.lon_rad.sin_cos();
        let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
        Ecef {
            x_m: (n + g.alt_m) * cos_lat * cos_lon,
            y_m: (n + g.alt_m) * cos_lat * sin_lon,
            z_m: (n * (1.0 - WGS84_E2) + g.alt_m) * sin_lat,
        }
    }

    /// Heikkinen's closed-form (Ferrari) solution. Exact at all altitudes and stable
    /// on the polar axis, where the `p / cos(lat)` altitude form used by iterative
    /// methods degenerates. The polar axis (`p == 0`) is handled as its own branch
    /// because the quartic's `c = e⁴ F p² / G³` term carries no information there.
    fn ecef_to_geodetic(e: Ecef) -> Geodetic {
        let p = (e.x_m * e.x_m + e.y_m * e.y_m).sqrt();
        let lon_rad = e.y_m.atan2(e.x_m);

        if p == 0.0 {
            // On the spin axis: latitude is exactly a pole, longitude is conventionally
            // zero (pymap3d returns atan2(0, 0) == 0 here as well), altitude is the
            // distance from the pole along the axis.
            let lat_rad = if e.z_m >= 0.0 {
                std::f64::consts::FRAC_PI_2
            } else {
                -std::f64::consts::FRAC_PI_2
            };
            return Geodetic {
                lat_rad,
                lon_rad,
                alt_m: e.z_m.abs() - WGS84_B,
            };
        }

        let z2 = e.z_m * e.z_m;
        let a2 = WGS84_A * WGS84_A;
        let b2 = WGS84_B * WGS84_B;
        let e4 = WGS84_E2 * WGS84_E2;

        let f = 54.0 * b2 * z2;
        let g = p * p + (1.0 - WGS84_E2) * z2 - WGS84_E2 * (a2 - b2);
        let c = e4 * f * p * p / (g * g * g);
        let s = (1.0 + c + (c * c + 2.0 * c).sqrt()).cbrt();
        let k = s + 1.0 + 1.0 / s;
        let pp = f / (3.0 * k * k * g * g);
        let q = (1.0 + 2.0 * e4 * pp).sqrt();

        // The radicand is non-negative for every physically meaningful input; clamping
        // at zero keeps a rounding-induced -1e-30 from producing a NaN latitude rather
        // than masking a real error, which would show up as a metre-scale disagreement.
        let radicand = 0.5 * a2 * (1.0 + 1.0 / q)
            - pp * (1.0 - WGS84_E2) * z2 / (q * (1.0 + q))
            - 0.5 * pp * p * p;
        let r0 = -(pp * WGS84_E2 * p) / (1.0 + q) + radicand.max(0.0).sqrt();

        let t = p - WGS84_E2 * r0;
        let u = (t * t + z2).sqrt();
        let v = (t * t + (1.0 - WGS84_E2) * z2).sqrt();
        let z0 = b2 * e.z_m / (WGS84_A * v);

        Geodetic {
            lat_rad: ((e.z_m + WGS84_EP2 * z0) / p).atan(),
            lon_rad,
            alt_m: u * (1.0 - b2 / (WGS84_A * v)),
        }
    }

    fn ecef_to_enu(e: Ecef, origin: Geodetic) -> Enu {
        let o = Self::geodetic_to_ecef(origin);
        let d = [e.x_m - o.x_m, e.y_m - o.y_m, e.z_m - o.z_m];
        let (east, north, up) = enu_basis(origin);
        Enu {
            e_m: east[0] * d[0] + east[1] * d[1] + east[2] * d[2],
            n_m: north[0] * d[0] + north[1] * d[1] + north[2] * d[2],
            u_m: up[0] * d[0] + up[1] * d[1] + up[2] * d[2],
        }
    }

    fn enu_to_ecef(v: Enu, origin: Geodetic) -> Ecef {
        let o = Self::geodetic_to_ecef(origin);
        let (east, north, up) = enu_basis(origin);
        Ecef {
            x_m: o.x_m + east[0] * v.e_m + north[0] * v.n_m + up[0] * v.u_m,
            y_m: o.y_m + east[1] * v.e_m + north[1] * v.n_m + up[1] * v.u_m,
            z_m: o.z_m + east[2] * v.e_m + north[2] * v.n_m + up[2] * v.u_m,
        }
    }
}

/// Re-express an ENU offset as NED about the same origin. A relabelling, exact by
/// construction; kept a free function so the oracle-comparable [`CoordTransform`]
/// surface stays exactly the four transforms the capability-table row names an
/// oracle for.
#[must_use]
pub fn enu_to_ned(v: Enu) -> Ned {
    Ned {
        n_m: v.n_m,
        e_m: v.e_m,
        d_m: -v.u_m,
    }
}

/// Inverse of [`enu_to_ned`].
#[must_use]
pub fn ned_to_enu(v: Ned) -> Enu {
    Enu {
        e_m: v.e_m,
        n_m: v.n_m,
        u_m: -v.d_m,
    }
}

/// Geodetic position as a NED offset from a local tangent-plane origin.
#[must_use]
pub fn geodetic_to_ned(g: Geodetic, origin: Geodetic) -> Ned {
    enu_to_ned(Wgs84::ecef_to_enu(Wgs84::geodetic_to_ecef(g), origin))
}

/// Inverse of [`geodetic_to_ned`].
#[must_use]
pub fn ned_to_geodetic(v: Ned, origin: Geodetic) -> Geodetic {
    Wgs84::ecef_to_geodetic(Wgs84::enu_to_ecef(ned_to_enu(v), origin))
}
