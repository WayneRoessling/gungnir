// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-102 (D-41): the coordinate reference system a LAS file declares for itself, and
//! the conversion out of it into a local ENU frame.
//!
//! Split from `pointcloud.rs` rather than appended to it because half of what is here
//! only compiles under the `crs` feature, and a reader should be able to see at a glance
//! which tests a default `cargo test` actually ran.
//!
//! **What runs where.** The reading half needs no projection library and runs
//! everywhere. The conversion half is `#[cfg(feature = "crs")]` and runs in `ci.yml`'s
//! `proj-crs` job, which installs the native PROJ build dependencies; it does not run on
//! a stock Windows developer machine, because `proj-sys` 0.27 cannot build libproj
//! there at all (see `gungnir-data/Cargo.toml`'s `crs` feature for the specifics).

use std::path::{Path, PathBuf};

use gungnir_data::pointcloud;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/pointcloud")
        .join(name)
}

/// The bounded query `pointcloud.rs`'s own happy-path test uses: a round-number
/// 100 x 100 box in the file's own units, chosen from the fixture's recorded bounds
/// rather than from what the query returns. Reused so the two tests describe the same
/// 4767 points.
const QUERY_BOX: [f32; 6] = [637_200.0, 851_100.0, 400.0, 637_300.0, 851_200.0, 620.0];

/// The EPSG code the fixture's own WKT names for its horizontal system: NAD83 / Oregon
/// GIC Lambert (ft). Recorded in `testdata/pointcloud/SOURCE.md` with the rest of the
/// fixture's provenance.
const AUTZEN_HORIZONTAL_EPSG: u32 = 2992;

fn autzen() -> pointcloud::PointBuffer {
    pointcloud::load_copc_bounded(&fixture("autzen-classified.copc.laz"), QUERY_BOX)
        .expect("a real COPC file with points in this box")
}

/// **The fixture that made this gap testable without authoring one.** The Autzen COPC
/// capture is a real survey and declares a real compound CRS in its own record-2112 WKT
/// VLR -- EPSG:2992 horizontally, NAVD88 height in US survey feet vertically -- so the
/// claim "the loader now reads a LAS file's CRS" is checked against a file this
/// workspace did not write and could not have shaped to agree with it.
///
/// Before GAP-102 the loader read none of this: `PointBuffer` had no `crs` field at all,
/// and a baseline declaring `frame: "local-enu"` for this file was contradicted by
/// nothing, which is exactly what the gap register said was missing.
#[test]
fn a_real_capture_declares_its_own_crs_and_the_loader_now_reads_it() {
    let cloud = autzen();

    let Some(pointcloud::crs::PointCloudCrs::Wkt(wkt)) = cloud.crs.as_ref() else {
        panic!("a LAS 1.4 file declares WKT, not geokeys: {:?}", cloud.crs);
    };
    assert!(
        wkt.starts_with("COMPD_CS["),
        "a compound (horizontal + vertical) system, got: {}",
        &wkt[..wkt.len().min(40)]
    );
    assert!(wkt.contains("NAD83 / Oregon GIC Lambert (ft)"), "{wkt}");
    assert!(wkt.contains("NAVD88 height (ftUS)"), "{wkt}");

    let declared = cloud.crs.as_ref().expect("just matched");
    assert!(
        declared.agrees_with_epsg(AUTZEN_HORIZONTAL_EPSG),
        "the WKT names EPSG:{AUTZEN_HORIZONTAL_EPSG}"
    );
    assert!(
        !declared.agrees_with_epsg(32_610),
        "and does not name an unrelated UTM zone"
    );

    // The vertical unit is the US survey foot (1200/3937 m), NOT the international foot
    // (0.3048 m) the *horizontal* axes of this same file use. The two differ by about
    // two parts per million, and reading the wrong one would put every height out by
    // that much -- small, and exactly the kind of small wrongness that is never found
    // later. The scanner walks to the `VERT_CS` node for this reason.
    let vertical = declared
        .vertical_unit_metres()
        .expect("the WKT declares a VERT_CS");
    assert!(
        (vertical - 1200.0 / 3937.0).abs() < 1e-15,
        "expected the US survey foot, got {vertical}"
    );
    assert!(
        (vertical - 0.3048).abs() > 1e-9,
        "the international foot is this file's horizontal unit and must not be read as \
         its vertical one"
    );
}

/// The other fixture declares nothing, and that is not an error. A LAS file written by a
/// tool that did not know its own frame is the common case, and `None` is what lets a
/// baseline's `frame: "local-enu"` stand unopposed for it.
#[test]
fn a_file_that_declares_no_crs_reads_as_none_rather_than_failing() {
    let cloud = pointcloud::load_las(&fixture("five-points.las")).expect("loads");
    assert_eq!(cloud.crs, None);
}

/// The geodetic-to-ENU half is a closure the caller supplies, so these tests can stub it
/// out and check PROJ's half on its own. It hands the triple back in degrees, unchanged
/// but for the radian-to-degree turn, which makes the expected values below directly
/// comparable to what `pyproj` prints.
///
/// `gungnir-data` depends on no other crate in this workspace (`ARCHITECTURE.md` §7.1),
/// so `gungnir_model::LocalFrame` is not reachable from here. The composed pipeline --
/// PROJ, then a real `LocalFrame` -- is checked in `gungnir-app/tests/pointcloud_crs.rs`.
#[cfg(feature = "crs")]
fn passthrough(g: [f64; 3]) -> [f64; 3] {
    [g[0].to_degrees(), g[1].to_degrees(), g[2]]
}

/// **Independently checked against `pyproj`, and the check is recorded here rather than
/// committed as a script** -- the same discipline `gungnir-data-fusion/src/normals.rs`
/// and `point_to_plane.rs` apply to their own Python oracles.
///
/// **What was run.** `pyproj` 3.8.0, which bundles PROJ 9.8.1 -- a different PROJ build
/// from the 9.6.2 `proj-sys` links, so this is a second implementation and not the same
/// library asked twice. It built `Transformer.from_crs(<the fixture's own WKT, read out
/// of its record-2112 VLR>, "EPSG:4979", always_xy=True)` and transformed the centre of
/// `QUERY_BOX`, X=637250, Y=851150, Z=500, giving
///
/// ```text
/// lon = -123.06889823098363
/// lat =   44.056081952041154
/// h   =  152.4003048006096
/// ```
///
/// **What agreed, and to what.** The three values asserted below are that run's,
/// transcribed. The tolerance is 1e-9 degrees, about 0.1 mm on the ground -- far tighter
/// than any difference a datum realisation could contribute, so this is a real check on
/// the Lambert Conformal Conic inverse and not a loose one.
///
/// **Two findings from running it, recorded because neither is obvious.** First, the
/// same transform via `EPSG:2992 -> EPSG:4269` (NAD83 to NAD83, no datum step at all)
/// returns the identical longitude and latitude to all seventeen digits, so the
/// NAD83-to-WGS84 step PROJ selects here contributes nothing at this precision: the
/// number above tests the projection inverse, not a datum model. Second, `pyproj`'s
/// height for the compound CRS is exactly `Z * 1200/3937`, the US survey foot's
/// definition -- so PROJ itself, with no vertical-datum grid available, applies the unit
/// conversion and no geoid separation. That is precisely what
/// `pointcloud::crs::to_local_enu` does by hand, for the reason its own documentation
/// gives (`proj` 0.31's high-level API zeroes every `z` it passes to `proj_trans`), which
/// is why this test can hold the height to the same figure rather than to a looser one.
///
/// The script is not committed: it is four lines of `pyproj`, and the two numbers it
/// produced are above.
#[test]
#[cfg(feature = "crs")]
fn the_conversion_out_of_the_fixtures_own_crs_matches_an_independent_pyproj_run() {
    let declared = autzen().crs.clone().expect("the fixture declares a CRS");
    let source = declared.proj_definition().expect("a WKT is a definition");
    let vertical = declared.vertical_unit_metres().expect("a VERT_CS");

    // One point, at the centre of `QUERY_BOX`, with the large part of the coordinate in
    // `origin` exactly as the loader itself carries it.
    let one = pointcloud::PointBuffer {
        positions: vec![[0.0, 0.0, 0.0]],
        origin: [637_250.0, 851_150.0, 500.0],
        ..pointcloud::PointBuffer::default()
    };
    let out = pointcloud::crs::to_local_enu(&one, &source, vertical, &passthrough)
        .expect("the fixture's own WKT is a CRS PROJ can transform from");

    assert_eq!(out.positions.len(), 1);
    // A single point is its own minimum corner, so the whole answer is in `origin` and
    // the position is zero.
    assert_eq!(out.positions[0], [0.0, 0.0, 0.0]);
    let [lat_deg, lon_deg, h_m] = out.origin;
    assert!(
        (lat_deg - 44.056_081_952_041_154).abs() < 1e-9,
        "latitude {lat_deg} disagrees with pyproj"
    );
    assert!(
        (lon_deg + 123.068_898_230_983_63).abs() < 1e-9,
        "longitude {lon_deg} disagrees with pyproj"
    );
    assert!(
        (h_m - 152.400_304_800_609_6).abs() < 1e-9,
        "height {h_m} disagrees with pyproj"
    );
    // A swapped axis order is the failure this catches loudest, and it is worth catching
    // separately: Oregon is at latitude 44 and longitude -123, so the two cannot be
    // exchanged by accident without one of these signs going wrong.
    assert!(lat_deg > 0.0, "latitude should be north");
    assert!(lon_deg < 0.0, "longitude should be west");
}

/// The whole bounded cloud through the conversion: no point is dropped, the attributes
/// ride along, a normal does not, and the re-basing keeps `positions` small enough for
/// an `f32` to carry them exactly.
#[test]
#[cfg(feature = "crs")]
fn every_point_of_the_real_cloud_converts_and_lands_inside_the_fixtures_own_bounds() {
    let cloud = autzen();
    let declared = cloud.crs.clone().expect("the fixture declares a CRS");
    let source = declared.proj_definition().expect("a WKT is a definition");
    let vertical = declared.vertical_unit_metres().expect("a VERT_CS");

    let out = pointcloud::crs::to_local_enu(&cloud, &source, vertical, &passthrough)
        .expect("converts");

    assert_eq!(out.positions.len(), 4767, "no point is dropped");
    assert_eq!(out.intensity.as_ref().map(Vec::len), Some(4767));
    assert_eq!(
        out.classification, cloud.classification,
        "attributes ride along unchanged"
    );
    assert_eq!(
        out.crs, None,
        "a converted cloud no longer carries the file's own claim, which is no longer \
         true of its numbers"
    );

    // `QUERY_BOX` is 100 x 100 in the file's own feet, near longitude -123, latitude 44,
    // so its whole extent is a small fraction of a degree. With `passthrough` standing
    // in for the ENU step the re-based positions are therefore in degrees, and must be
    // tiny -- which is the point of re-basing and what keeps the `f32` exact.
    for p in &out.positions {
        assert!(
            p[0].abs() < 0.001 && p[1].abs() < 0.001,
            "a re-based position should be a fraction of a degree: {p:?}"
        );
    }
    // The corner they are relative to is inside the file's own recorded bounds, put
    // through the same conversion. `testdata/pointcloud/SOURCE.md` records
    // x [635577.79, 639003.73] and y [848882.15, 853537.66]; the same pyproj run above
    // maps those corners to longitude [-123.07498674, -123.06251260], latitude
    // [44.04971882, 44.06278031] and height [123.79171958, 187.53162306].
    let [lat_deg, lon_deg, h_m] = out.origin;
    assert!(
        (44.049_718..=44.062_781).contains(&lat_deg),
        "latitude {lat_deg} is outside the fixture's own converted bounds"
    );
    assert!(
        (-123.074_987..=-123.062_512).contains(&lon_deg),
        "longitude {lon_deg} is outside the fixture's own converted bounds"
    );
    assert!(
        (123.791_719..=187.531_624).contains(&h_m),
        "height {h_m} is outside the fixture's own converted bounds"
    );
}

/// An EPSG code no register knows is refused by name, at the point where it can be --
/// the loader, which has PROJ, rather than the validator, which does not.
#[test]
#[cfg(feature = "crs")]
fn an_unknown_crs_is_refused_by_name_rather_than_placed_somewhere_plausible() {
    let one = pointcloud::PointBuffer {
        positions: vec![[0.0, 0.0, 0.0]],
        origin: [1.0, 2.0, 3.0],
        ..pointcloud::PointBuffer::default()
    };
    let err = pointcloud::crs::to_local_enu(&one, "EPSG:999999", 1.0, &passthrough)
        .expect_err("999999 is not an EPSG code");
    let text = err.to_string();
    assert!(text.contains("999999"), "the refusal names the code: {text}");
}
