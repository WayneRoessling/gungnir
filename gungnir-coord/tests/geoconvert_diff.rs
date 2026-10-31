// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for DN-37 §6.1 row d: UTM, UPS and MGRS against `GeographicLib`'s
//! `GeoConvert` (`docs/design/DN-37-symbology-cursor-readout-and-declutter.md`, D-170).
//!
//! Criterion, as the design states it: the same MGRS string, and a UTM or UPS position
//! within a millimetre, across the zone and band edges, the Norway and Svalbard
//! exceptions, the antimeridian and both poles. The row is a draft until the owner enters
//! it in `docs/verification-capability-table.md`; this test is what it would run.
//!
//! The fixture is `testdata/oracles/coord/geoconvert.json`, written by
//! `testdata/oracles/tools/gen_geoconvert_fixtures.py`; `testdata/oracles/SOURCE.md` says
//! which release of the oracle wrote it.

use gungnir_coord::grid::{mgrs, utm_ups, GridError};
use std::collections::BTreeMap;

/// The row's tolerance on a grid position, metres.
const TOL_M: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    lat_deg: f64,
    lon_deg: f64,
    zone: u8,
    north: bool,
    easting_m: f64,
    northing_m: f64,
    mgrs: BTreeMap<String, String>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    oracle_version: String,
    cases: Vec<Case>,
}

fn load() -> Fixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/oracles/coord/geoconvert.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("oracle fixture present; see testdata/oracles/README.md");
    serde_json::from_str(&raw).expect("fixture parses")
}

#[test]
fn utm_and_ups_agree_with_geoconvert_to_a_millimetre() {
    let fx = load();
    assert_eq!(fx.oracle, "GeographicLib GeoConvert");
    let mut worst = (0.0_f64, String::new());
    for c in &fx.cases {
        let p = utm_ups(c.lat_deg, c.lon_deg).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        assert_eq!(p.zone, c.zone, "{}: zone", c.name);
        assert_eq!(p.north, c.north, "{}: hemisphere", c.name);
        let err = (p.easting_m - c.easting_m)
            .abs()
            .max((p.northing_m - c.northing_m).abs());
        if err > worst.0 {
            worst = (err, c.name.clone());
        }
        assert!(
            err < TOL_M,
            "{}: ({}, {}) off by {err} m: got {} {}, GeoConvert {} {}",
            c.name,
            c.lat_deg,
            c.lon_deg,
            p.easting_m,
            p.northing_m,
            c.easting_m,
            c.northing_m
        );
    }
    println!(
        "{} cases, GeoConvert {}: worst position error {:e} m at {}",
        fx.cases.len(),
        fx.oracle_version,
        worst.0,
        worst.1
    );
}

#[test]
fn mgrs_strings_are_identical_to_geoconvert_at_every_precision() {
    let fx = load();
    for c in &fx.cases {
        for (digits, want) in &c.mgrs {
            let d: u8 = digits.parse().expect("digits key");
            let got = mgrs(c.lat_deg, c.lon_deg, d).unwrap_or_else(|e| panic!("{}: {e}", c.name));
            assert_eq!(
                &got, want,
                "{}: ({}, {}) at {d} digits",
                c.name, c.lat_deg, c.lon_deg
            );
        }
    }
}

#[test]
fn the_fixture_covers_what_the_design_names() {
    let fx = load();
    let has = |prefix: &str| fx.cases.iter().any(|c| c.name.starts_with(prefix));
    for prefix in [
        "zone_edge_",
        "band_edge_",
        "norway_",
        "svalbard_",
        "north_cap_",
        "south_cap_",
        "equator_",
        "utm_sweep_",
    ] {
        assert!(has(prefix), "no {prefix} case");
    }
    let zones: std::collections::BTreeSet<u8> = fx.cases.iter().map(|c| c.zone).collect();
    assert!(zones.contains(&0), "UPS is covered");
    assert!(
        (1..=60).all(|z| zones.contains(&z)),
        "every UTM zone is covered"
    );
    let bands: std::collections::BTreeSet<char> = fx
        .cases
        .iter()
        .filter(|c| c.zone != 0)
        .filter_map(|c| c.mgrs["5"].chars().nth(2))
        .collect();
    assert_eq!(bands.len(), 20, "every latitude band is covered: {bands:?}");
    let ups: std::collections::BTreeSet<char> = fx
        .cases
        .iter()
        .filter(|c| c.zone == 0)
        .filter_map(|c| c.mgrs["5"].chars().next())
        .collect();
    assert_eq!(ups.len(), 4, "all four UPS bands are covered: {ups:?}");
}

#[test]
fn a_position_without_a_grid_is_refused_by_name() {
    assert_eq!(utm_ups(f64::NAN, 0.0), Err(GridError::NotFinite));
    assert_eq!(utm_ups(0.0, f64::INFINITY), Err(GridError::NotFinite));
    assert_eq!(utm_ups(90.5, 0.0), Err(GridError::LatitudeOutOfRange));
    assert_eq!(mgrs(45.0, 7.0, 6), Err(GridError::PrecisionOutOfRange));
}
