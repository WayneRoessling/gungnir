// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Compact Position Reporting, checked against published worked examples and against
//! arithmetic (GAP-010, closing action point 3).
//!
//! **This file names no decoder as an oracle either.** CPR is a published algorithm and
//! the receivers' own test suites carry worked examples for it, so it is checkable
//! without asking another decoder what it thinks. That matters here more than anywhere
//! else in the ADS-B build: a position is the one field an operator acts on, and the
//! residual risk `docs/design/external-standards.md` §4 records — that `rs1090` and
//! `adsb_deku` are only partly independent — is exactly the risk a shared misreading of
//! the position algorithm would run.
//!
//! # Where the numbers come from, and what was and was not taken
//!
//! The input and expected-output tuples below are transcribed from the CPR test vectors
//! published in `flightaware/dump1090`'s `cprtests.c` (<https://github.com/flightaware/dump1090>),
//! which is GPL-2.0. **No code was copied and the file is not vendored**: what is here is
//! the numbers — CPR field values in, degrees out — which is the same use the owner
//! decided for the GPL-2.0 ASTERIX captures on 2026-09-06 (`external-standards.md` §1.5),
//! test data in a test target that no shipped binary links. The surface cases are the ones
//! that matter most, because they are the ones that need a reference position and so the
//! ones where a decoder can be confidently wrong by exactly ninety degrees.
//!
//! The NL boundary test takes nothing from anywhere: it inverts the same closed-form
//! expression `cpr::longitude_zones` evaluates and checks the two agree either side of
//! every transition, which is a statement about the function rather than about a table.

use gungnir_interop::adsb::cpr::{
    self, decode_global, decode_local, encode, CprError, CprFrame, CprKind, Position,
};

/// The tolerance `cprtests.c` states for its own expected values.
const TOLERANCE_DEG: f64 = 1e-6;

fn even(lat_cpr: u32, lon_cpr: u32) -> CprFrame {
    CprFrame {
        odd: false,
        lat_cpr,
        lon_cpr,
    }
}

fn odd(lat_cpr: u32, lon_cpr: u32) -> CprFrame {
    CprFrame {
        odd: true,
        lat_cpr,
        lon_cpr,
    }
}

fn assert_close(got: Position, latitude_deg: f64, longitude_deg: f64, what: &str) {
    assert!(
        (got.latitude_deg - latitude_deg).abs() < TOLERANCE_DEG,
        "{what}: latitude {} against {latitude_deg}",
        got.latitude_deg
    );
    assert!(
        (got.longitude_deg - longitude_deg).abs() < TOLERANCE_DEG,
        "{what}: longitude {} against {longitude_deg}",
        got.longitude_deg
    );
}

/// Global airborne decoding, both ways round: the same pair decoded with the even
/// message latest and with the odd message latest gives two positions a few metres
/// apart, which is the aircraft moving between the two transmissions.
#[test]
fn global_airborne_pairs_match_the_published_worked_examples() {
    // even lat, even lon, odd lat, odd lon, even-latest lat/lon, odd-latest lat/lon.
    let cases = [
        (
            80_536, 9_432, 61_720, 9_192, 51.686_646, 0.700_156, 51.686_763, 0.701_294,
        ),
        (
            80_534, 9_413, 61_714, 9_144, 51.686_554, 0.698_745, 51.686_484, 0.697_632,
        ),
    ];
    for (el, eo, ol, oo, e_lat, e_lon, o_lat, o_lon) in cases {
        let (e, o) = (even(el, eo), odd(ol, oo));
        let with_even = decode_global(e, o, false, CprKind::Airborne, None).expect("even latest");
        assert_close(with_even, e_lat, e_lon, "airborne, even latest");
        let with_odd = decode_global(e, o, true, CprKind::Airborne, None).expect("odd latest");
        assert_close(with_odd, o_lat, o_lon, "airborne, odd latest");
    }
}

/// Global surface decoding: one pair of frames, a reference position walked round the
/// world, and the answer that must come back is the Cambridge airport apron shifted into
/// whichever quadrant the reference selects. This is the case where a decoder can be
/// wrong by ninety or a hundred and eighty degrees and look entirely plausible.
// Long because it is a table of eighteen published cases, and a table read straight
// through is worth more here than three shorter functions.
#[allow(clippy::too_many_lines)]
#[test]
fn global_surface_pairs_choose_the_quadrant_the_reference_selects() {
    let (e, o) = (even(105_730, 9_259), odd(29_693, 8_997));
    // reference lat, reference lon, even-latest lat/lon, odd-latest lat/lon.
    let cases = [
        // Longitude quadrants.
        (
            52.0,
            -180.0,
            52.209_984,
            0.176_601 - 180.0,
            52.209_976,
            0.176_507 - 180.0,
        ),
        (
            52.0,
            -140.0,
            52.209_984,
            0.176_601 - 180.0,
            52.209_976,
            0.176_507 - 180.0,
        ),
        (
            52.0,
            -130.0,
            52.209_984,
            0.176_601 - 90.0,
            52.209_976,
            0.176_507 - 90.0,
        ),
        (
            52.0,
            -50.0,
            52.209_984,
            0.176_601 - 90.0,
            52.209_976,
            0.176_507 - 90.0,
        ),
        (52.0, -40.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (52.0, -10.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (52.0, 0.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (52.0, 10.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (52.0, 40.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (
            52.0,
            50.0,
            52.209_984,
            0.176_601 + 90.0,
            52.209_976,
            0.176_507 + 90.0,
        ),
        (
            52.0,
            130.0,
            52.209_984,
            0.176_601 + 90.0,
            52.209_976,
            0.176_507 + 90.0,
        ),
        (
            52.0,
            140.0,
            52.209_984,
            0.176_601 - 180.0,
            52.209_976,
            0.176_507 - 180.0,
        ),
        (
            52.0,
            180.0,
            52.209_984,
            0.176_601 - 180.0,
            52.209_976,
            0.176_507 - 180.0,
        ),
        // Latitude quadrants. The longitude changes with them, because the cell size does.
        (90.0, 0.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (8.0, 0.0, 52.209_984, 0.176_601, 52.209_976, 0.176_507),
        (
            7.0,
            0.0,
            52.209_984 - 90.0,
            0.135_269,
            52.209_976 - 90.0,
            0.134_299,
        ),
        (
            -52.0,
            0.0,
            52.209_984 - 90.0,
            0.135_269,
            52.209_976 - 90.0,
            0.134_299,
        ),
        (
            -90.0,
            0.0,
            52.209_984 - 90.0,
            0.135_269,
            52.209_976 - 90.0,
            0.134_299,
        ),
    ];
    for (ref_lat, ref_lon, e_lat, e_lon, o_lat, o_lon) in cases {
        let reference = Position {
            latitude_deg: ref_lat,
            longitude_deg: ref_lon,
        };
        let what = format!("surface from reference ({ref_lat}, {ref_lon})");
        let with_even = decode_global(e, o, false, CprKind::Surface, Some(reference))
            .unwrap_or_else(|err| panic!("{what}, even latest: {err}"));
        assert_close(with_even, e_lat, e_lon, &what);
        let with_odd = decode_global(e, o, true, CprKind::Surface, Some(reference))
            .unwrap_or_else(|err| panic!("{what}, odd latest: {err}"));
        assert_close(with_odd, o_lat, o_lon, &what);
    }
}

/// The pole and equator cases of the same published set: both CPR fields zero, and the
/// answer is a pole or the equator depending on which is nearer the reference. A
/// decoder that clamped or wrapped the wrong way lands ninety degrees out here.
#[test]
fn global_surface_at_the_poles_and_the_equator() {
    let (e, o) = (even(0, 0), odd(0, 0));
    let cases = [
        (-46.0, -180.0, -90.0, -180.0),
        (-44.0, -180.0, 0.0, -180.0),
        (44.0, -180.0, 0.0, -180.0),
        (46.0, -180.0, 90.0, -180.0),
    ];
    for (ref_lat, ref_lon, lat, lon) in cases {
        let reference = Position {
            latitude_deg: ref_lat,
            longitude_deg: ref_lon,
        };
        let what = format!("surface pole/equator from ({ref_lat}, {ref_lon})");
        for latest_is_odd in [false, true] {
            let got = decode_global(e, o, latest_is_odd, CprKind::Surface, Some(reference))
                .unwrap_or_else(|err| panic!("{what}: {err}"));
            assert_close(got, lat, lon, &what);
        }
    }
}

/// Local decoding against a reference, airborne and surface, with the reference moved
/// around inside the half-cell the algorithm allows.
#[test]
fn local_decoding_matches_the_published_worked_examples() {
    // reference lat, reference lon, frame, expected lat, expected lon, kind.
    let airborne = [
        (52.0, 0.0, even(80_536, 9_432), 51.686_646, 0.700_156),
        (52.0, 0.0, odd(61_720, 9_192), 51.686_763, 0.701_294),
        (52.0, 0.0, even(80_534, 9_413), 51.686_554, 0.698_745),
        (52.0, 0.0, odd(61_714, 9_144), 51.686_484, 0.697_632),
        // The receiver moved: latitude must be within about three degrees.
        (48.7, 0.0, even(80_536, 9_432), 51.686_646, 0.700_156),
        (54.6, 0.0, odd(61_720, 9_192), 51.686_763, 0.701_294),
        // Longitude must be within about 4.8 degrees at this latitude.
        (52.0, 5.4, even(80_536, 9_432), 51.686_646, 0.700_156),
        (52.0, -4.1, odd(61_720, 9_192), 51.686_763, 0.701_294),
    ];
    for (ref_lat, ref_lon, frame, lat, lon) in airborne {
        let reference = Position {
            latitude_deg: ref_lat,
            longitude_deg: ref_lon,
        };
        let what = format!("local airborne from ({ref_lat}, {ref_lon})");
        let got = decode_local(frame, CprKind::Airborne, reference)
            .unwrap_or_else(|err| panic!("{what}: {err}"));
        assert_close(got, lat, lon, &what);
    }

    let surface = [
        (52.0, 0.0, even(105_730, 9_259), 52.209_984, 0.176_601),
        (52.0, 0.0, odd(29_693, 8_997), 52.209_976, 0.176_507),
        // Latitude must be within about 0.75 degrees: the cell is 90/60.
        (51.46, 0.0, even(105_730, 9_259), 52.209_984, 0.176_601),
        (52.95, 0.0, odd(29_693, 8_997), 52.209_976, 0.176_507),
        // Longitude must be within about 1.25 degrees at this latitude.
        (52.0, 1.4, even(105_730, 9_259), 52.209_984, 0.176_601),
        (52.0, -1.05, odd(29_693, 8_997), 52.209_976, 0.176_507),
    ];
    for (ref_lat, ref_lon, frame, lat, lon) in surface {
        let reference = Position {
            latitude_deg: ref_lat,
            longitude_deg: ref_lon,
        };
        let what = format!("local surface from ({ref_lat}, {ref_lon})");
        let got = decode_local(frame, CprKind::Surface, reference)
            .unwrap_or_else(|err| panic!("{what}: {err}"));
        assert_close(got, lat, lon, &what);
    }
}

/// **A stale reference makes local decoding silently wrong, and this pins it rather
/// than letting a doc comment carry the claim.**
///
/// The published set walks the reference around inside the half-cell the algorithm
/// allows and stops there. Walk past it and the answer moves by a whole cell — six
/// degrees of latitude airborne — while every check the decoder can make still passes,
/// because the answer it chose is the cell nearest the reference and is therefore within
/// half a cell of it by construction. The half-cell rule is about the distance from the
/// **true** position, which a decoder does not have.
///
/// So this test asserts the wrong answer, on purpose. Keeping the reference fresh is the
/// caller's obligation; if this test ever starts failing because the decode refuses,
/// something has been added that can detect the case, and the module documentation and
/// the register entry both need changing with it.
#[test]
fn a_stale_reference_moves_a_local_decode_by_a_whole_cell_and_nothing_notices() {
    let frame = even(80_536, 9_432);
    let truth = 51.686_646;
    let reference = Position {
        latitude_deg: truth + 4.0,
        longitude_deg: 0.700_156,
    };
    let got = decode_local(frame, CprKind::Airborne, reference).expect("no check can fire");
    assert!(
        (got.latitude_deg - (truth + 6.0)).abs() < TOLERANCE_DEG,
        "a four-degree-stale reference puts the answer one six-degree cell out, at {}",
        got.latitude_deg
    );
}

/// The one refusal a local decode can still make, and the check that it is a real one
/// rather than another unreachable guard: a reference at the pole and a frame whose
/// cell falls past ninety degrees.
#[test]
fn a_local_decode_refuses_an_answer_off_the_planet() {
    let pole = Position {
        latitude_deg: 90.0,
        longitude_deg: 0.0,
    };
    assert!(
        matches!(
            decode_local(even(13_107, 0), CprKind::Airborne, pole),
            Err(CprError::LatitudeOutOfRange { .. })
        ),
        "a cell past the pole must not yield a position"
    );
}

/// `longitude_zones` is a closed form, and its inverse gives the latitude at which the
/// zone count falls by one. Evaluating the function either side of every one of the
/// fifty-eight transitions checks the whole function rather than sampling it — and it
/// takes nothing from any decoder's table, which is the point.
#[test]
fn the_longitude_zone_boundaries_agree_with_the_inverse_of_their_own_expression() {
    let nz = 15.0_f64;
    let a = 1.0 - (std::f64::consts::PI / (2.0 * nz)).cos();
    for zones in 2..=59u32 {
        let cos_squared = a / (1.0 - (2.0 * std::f64::consts::PI / f64::from(zones)).cos());
        let boundary = cos_squared.sqrt().acos().to_degrees();
        assert!(
            (0.0..=87.0).contains(&boundary),
            "boundary for {zones} zones is {boundary}"
        );
        assert_eq!(
            cpr::longitude_zones(boundary - 1e-6),
            zones,
            "just equatorward of the {zones}-zone boundary at {boundary}"
        );
        assert_eq!(
            cpr::longitude_zones(boundary + 1e-6),
            zones - 1,
            "just poleward of the {zones}-zone boundary at {boundary}"
        );
        // And the same on the southern side, because the function takes the magnitude.
        assert_eq!(cpr::longitude_zones(-(boundary - 1e-6)), zones);
    }
    assert_eq!(cpr::longitude_zones(0.0), 59);
    assert_eq!(cpr::longitude_zones(87.0), 1);
    assert_eq!(cpr::longitude_zones(90.0), 1);
}

/// Encode a place, decode the pair back, and land in the same place. Arithmetic against
/// arithmetic, over a grid rather than a handful of points, so a sign or a modulo that
/// happened to work at Cambridge is caught in the southern hemisphere.
#[test]
fn encoding_a_place_and_decoding_the_pair_returns_the_place() {
    let mut checked = 0usize;
    let mut latitude = -80.0;
    while latitude <= 80.0 {
        let mut longitude = -175.0;
        while longitude < 180.0 {
            let place = Position {
                latitude_deg: latitude,
                longitude_deg: longitude,
            };
            let e = encode(place, false, CprKind::Airborne);
            let o = encode(place, true, CprKind::Airborne);
            match decode_global(e, o, false, CprKind::Airborne, None) {
                Ok(got) => {
                    // One CPR count is 360/60/2^17 degrees of latitude, so the
                    // round trip is exact to a few metres and no better.
                    assert!(
                        (got.latitude_deg - latitude).abs() < 1e-4,
                        "latitude {latitude} came back as {}",
                        got.latitude_deg
                    );
                    assert!(
                        (got.longitude_deg - longitude).abs() < 1e-3,
                        "longitude {longitude} at latitude {latitude} came back as {}",
                        got.longitude_deg
                    );
                    checked += 1;
                }
                // A place on a latitude-zone boundary encodes to a pair that straddles
                // it, and refusing that pair is the correct behaviour, not a failure.
                Err(CprError::ZoneDisagreement { .. }) => {}
                Err(other) => panic!("({latitude}, {longitude}): {other}"),
            }
            longitude += 5.0;
        }
        latitude += 2.5;
    }
    assert!(checked > 4000, "only {checked} places round-tripped");
}

/// A pair whose two halves fall in different longitude-zone bands cannot be resolved,
/// and the decoder says which bands rather than returning a position from one of them.
#[test]
fn a_pair_that_straddles_a_zone_boundary_is_refused_by_name() {
    // 10.470471 degrees is the 59-to-58 zone boundary; a pair either side of it.
    let north = Position {
        latitude_deg: 10.471,
        longitude_deg: 0.0,
    };
    let south = Position {
        latitude_deg: 10.470,
        longitude_deg: 0.0,
    };
    let e = encode(south, false, CprKind::Airborne);
    let o = encode(north, true, CprKind::Airborne);
    match decode_global(e, o, true, CprKind::Airborne, None) {
        Err(CprError::ZoneDisagreement {
            even_zones,
            odd_zones,
        }) => assert_ne!(even_zones, odd_zones),
        // The two places are within one CPR count of each other, so the encoder may put
        // both in the same zone; then a position is the right answer and this case says
        // nothing either way.
        Ok(_) => {}
        Err(other) => panic!("unexpected refusal: {other}"),
    }
}
