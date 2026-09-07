//! Differential test for the verification-capability-table.md §1 row
//! "Coordinate frame transforms (ECEF/ENU/NED/geodetic)".
//!
//! Oracle: pymap3d. Pass criterion: position error < 1e-6 m. Every assertion below is
//! therefore expressed in metres, never in radians:
//!
//! - forward transforms are compared as a Euclidean ECEF distance;
//! - `ecef_to_geodetic` is compared as altitude error, a north-south arc error
//!   (`|Δlat| · a`), and an east-west arc error (`|Δlon| · a · cos φ`). The `cos φ`
//!   factor is what makes the criterion meaningful at the poles, where longitude is
//!   degenerate and pymap3d's own answer is an arbitrary member of the meridian
//!   family: a metre of east-west error is a metre wherever it is measured.
//!
//! The fixture deserializes straight into the crate's own [`Geodetic`], [`Ecef`], and
//! [`Enu`] types -- the generator writes their field names -- so there is no second
//! definition of a coordinate here to drift from the first.
//!
//! The fixture is checked in at `testdata/oracles/coord/pymap3d.json` and regenerated
//! by `testdata/oracles/tools/gen_coord_fixtures.py`; `testdata/oracles/README.md`
//! records the oracle version and how to rebuild it. MATLAB has no column here: the
//! capability-table row names pymap3d for this capability.

use gungnir_coord::{CoordTransform, Ecef, Enu, Geodetic, Wgs84, WGS84_A};

/// The capability-table tolerance for this row, metres.
const TOL_M: f64 = 1e-6;

#[derive(serde::Deserialize)]
struct ForwardCase {
    name: String,
    geodetic: Geodetic,
    ecef: Ecef,
}

#[derive(serde::Deserialize)]
struct ReverseCase {
    name: String,
    ecef: Ecef,
    geodetic: Geodetic,
}

#[derive(serde::Deserialize)]
struct EnuCase {
    name: String,
    origin: Geodetic,
    ecef: Ecef,
    enu: Enu,
    enu_to_ecef: Ecef,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    oracle_version: String,
    geodetic_to_ecef: Vec<ForwardCase>,
    ecef_to_geodetic: Vec<ReverseCase>,
    enu: Vec<EnuCase>,
}

fn load() -> Fixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/oracles/coord/pymap3d.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("oracle fixture present; see testdata/oracles/README.md");
    serde_json::from_str(&raw).expect("oracle fixture parses")
}

fn ecef_distance(a: Ecef, b: Ecef) -> f64 {
    let dx = a.x_m - b.x_m;
    let dy = a.y_m - b.y_m;
    let dz = a.z_m - b.z_m;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn enu_distance(a: Enu, b: Enu) -> f64 {
    let de = a.e_m - b.e_m;
    let dn = a.n_m - b.n_m;
    let du = a.u_m - b.u_m;
    (de * de + dn * dn + du * du).sqrt()
}

/// Smallest signed difference between two longitudes, radians; ±π are the same meridian.
fn wrap_pi(d: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    let mut x = d % two_pi;
    if x > std::f64::consts::PI {
        x -= two_pi;
    } else if x < -std::f64::consts::PI {
        x += two_pi;
    }
    x
}

/// Report the worst case of a run so a tightening tolerance shows where the margin is.
fn report(transform: &str, worst: &(f64, String)) {
    println!("{transform} worst case: {} at {:e} m", worst.1, worst.0);
}

#[test]
fn fixture_is_the_expected_oracle() {
    let f = load();
    assert_eq!(f.oracle, "pymap3d");
    assert!(!f.oracle_version.is_empty());
    assert!(!f.geodetic_to_ecef.is_empty());
    assert!(!f.ecef_to_geodetic.is_empty());
    assert!(!f.enu.is_empty());
}

#[test]
fn geodetic_to_ecef_matches_pymap3d() {
    let f = load();
    let mut worst = (0.0_f64, String::new());
    for case in &f.geodetic_to_ecef {
        let got = Wgs84::geodetic_to_ecef(case.geodetic);
        let d = ecef_distance(got, case.ecef);
        assert!(
            d.is_finite(),
            "{}: non-finite ECEF from geodetic_to_ecef",
            case.name
        );
        if d > worst.0 {
            worst = (d, case.name.clone());
        }
        assert!(
            d < TOL_M,
            "{}: position error {d:e} m exceeds {TOL_M:e} m",
            case.name
        );
    }
    report("geodetic_to_ecef", &worst);
}

#[test]
fn ecef_to_geodetic_matches_pymap3d() {
    let f = load();
    let mut worst = (0.0_f64, String::new());
    for case in &f.ecef_to_geodetic {
        let got = Wgs84::ecef_to_geodetic(case.ecef);
        assert!(
            got.lat_rad.is_finite() && got.lon_rad.is_finite() && got.alt_m.is_finite(),
            "{}: non-finite geodetic from ecef_to_geodetic",
            case.name
        );

        let alt_err = (got.alt_m - case.geodetic.alt_m).abs();
        let lat_err = (got.lat_rad - case.geodetic.lat_rad).abs() * WGS84_A;
        // East-west arc error: degenerate longitude at a pole costs no distance.
        let lon_err =
            wrap_pi(got.lon_rad - case.geodetic.lon_rad).abs() * WGS84_A * got.lat_rad.cos().abs();

        let d = alt_err.max(lat_err).max(lon_err);
        if d > worst.0 {
            worst = (d, case.name.clone());
        }
        assert!(
            alt_err < TOL_M,
            "{}: altitude error {alt_err:e} m exceeds {TOL_M:e} m",
            case.name
        );
        assert!(
            lat_err < TOL_M,
            "{}: north-south arc error {lat_err:e} m exceeds {TOL_M:e} m",
            case.name
        );
        assert!(
            lon_err < TOL_M,
            "{}: east-west arc error {lon_err:e} m exceeds {TOL_M:e} m",
            case.name
        );
    }
    report("ecef_to_geodetic", &worst);
}

#[test]
fn ecef_to_enu_matches_pymap3d() {
    let f = load();
    let mut worst = (0.0_f64, String::new());
    for case in &f.enu {
        let got = Wgs84::ecef_to_enu(case.ecef, case.origin);
        let d = enu_distance(got, case.enu);
        assert!(d.is_finite(), "{}: non-finite ENU", case.name);
        if d > worst.0 {
            worst = (d, case.name.clone());
        }
        assert!(
            d < TOL_M,
            "{}: ENU position error {d:e} m exceeds {TOL_M:e} m",
            case.name
        );
    }
    report("ecef_to_enu", &worst);
}

#[test]
fn enu_to_ecef_matches_pymap3d() {
    let f = load();
    let mut worst = (0.0_f64, String::new());
    for case in &f.enu {
        let got = Wgs84::enu_to_ecef(case.enu, case.origin);
        let d = ecef_distance(got, case.enu_to_ecef);
        assert!(d.is_finite(), "{}: non-finite ECEF", case.name);
        if d > worst.0 {
            worst = (d, case.name.clone());
        }
        assert!(
            d < TOL_M,
            "{}: position error {d:e} m exceeds {TOL_M:e} m",
            case.name
        );
    }
    report("enu_to_ecef", &worst);
}
