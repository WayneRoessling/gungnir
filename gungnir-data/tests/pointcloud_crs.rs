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

/// **These tests check the horizontal conversion and the vertical unit, and hold the
/// vertical datum still on purpose.** Handing `to_local_enu` `Ellipsoidal` tells it to
/// add nothing, so the height that comes back is the file's own scaled to metres --
/// exactly what the `pyproj` run below gives for this compound CRS with no NAVD88 grid on
/// its path -- and the Lambert inverse and the US-survey-foot scale stay checked against
/// a real capture on their own. The same capture through GEOID18, the datum step
/// included, is `the_real_capture_converts_through_geoid18_to_the_pyproj_heights` below
/// (GAP-197).
#[cfg(feature = "crs")]
const HORIZONTAL_ONLY: gungnir_data::geoid::HeightReference =
    gungnir_data::geoid::HeightReference::Ellipsoidal;

/// The epoch every frame step in this file is taken at (GAP-200, D-127): pinned, so the
/// expected values hold every year rather than drifting 2 cm a year with the date.
#[cfg(feature = "crs")]
const EPOCH: f64 = 2020.0;

/// **PROJ's own horizontal conversion, held still on purpose**, as [`HORIZONTAL_ONLY`]
/// holds the vertical datum still: the Autzen capture is NAD83, which a deployment now
/// carries to WGS 84 by its realisation (`nad83_frame_step` below), but these tests check
/// the Lambert inverse and the US survey foot on their own, against `pyproj`'s
/// projection-only figures. A NAVD88 height through GEOID18 still takes the Helmert's
/// height difference at [`EPOCH`] under it (D-127).
#[cfg(feature = "crs")]
fn not_nad83() -> gungnir_data::nad83::FrameConversion {
    gungnir_data::nad83::FrameConversion::not_nad83(EPOCH).expect("an epoch in range")
}

/// The committed GEOID18 clip (`testdata/geoid/SOURCE.md`).
const GEOID18_CLIP_SHA256: &str =
    "a93c8ca6a47cc3d5a58ffeeac469aa9c8c9ca2400a0113a1976eb0a4bc9b16eb";

fn geoid18_clip() -> gungnir_data::geoid::GeoidGrid {
    gungnir_data::geoid::GeoidGrid::verify(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../testdata/geoid/g2018u0_clip_43n45n_124w122w.tif"),
        GEOID18_CLIP_SHA256,
    )
    .expect("the committed GEOID18 clip is the recorded one")
}

/// GAP-197 (D-125): the real capture's heights are NAVD88 in US survey feet -- the WKT's
/// `VERT_DATUM` authority is EPSG:5103 -- and NAVD88 takes the pinned GEOID18 grid,
/// never another model's: with GEOID18 verified it converts, and without it the refusal
/// names GEOID18's file and why it is missing. Before GAP-108 these heights were used as
/// though ellipsoidal, 23 m high; between GAP-108 and GAP-197 they were refused by name.
#[test]
fn the_real_fixtures_navd88_heights_take_the_geoid18_grid_and_no_other() {
    use gungnir_data::geoid::{GeoidModel, HeightReference, VerticalDatum};
    let declared = autzen().crs.expect("the fixture declares a CRS");
    let datum = declared.vertical_datum().expect("a VERT_CS");
    assert_eq!(datum, VerticalDatum::Navd88);
    let err = gungnir_data::geoid::height_reference(
        "point_cloud.vertical",
        Some(&datum),
        None,
        &|model| Err(format!("{} is not there", model.file())),
    )
    .expect_err("no grid, no conversion");
    assert!(err.contains("us_noaa_g2018u0.tif"), "{err}");
    assert!(err.contains("NAVD88"), "{err}");
    let clip = geoid18_clip();
    let reference = gungnir_data::geoid::height_reference(
        "point_cloud.vertical",
        Some(&datum),
        None,
        &|model| match model {
            GeoidModel::Geoid18Conus => Ok(clip.clone()),
            other => Err(format!("{} is not asked for", other.file())),
        },
    )
    .expect("GEOID18 converts NAVD88");
    assert_eq!(
        reference,
        HeightReference::Geoid(GeoidModel::Geoid18Conus, clip)
    );
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
/// conversion and no geoid separation -- falling back to a "ballpark" operation
/// silently, which is the behaviour GAP-108 (D-121) refuses in a deployment and this
/// test reproduces deliberately through [`HORIZONTAL_ONLY`], so the height can be held
/// to the same figure rather than a looser one.
///
/// The script is not committed: it is four lines of `pyproj`, and the two numbers it
/// produced are above.
#[test]
#[cfg(feature = "crs")]
// A single point is its own minimum corner, so its re-based position is exactly zero by
// construction; an epsilon would only hide a re-basing that had drifted.
#[allow(clippy::float_cmp)]
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
    let out = pointcloud::crs::to_local_enu(
        &one,
        &source,
        vertical,
        &HORIZONTAL_ONLY,
        &not_nad83(),
        &passthrough,
    )
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

    let out = pointcloud::crs::to_local_enu(
        &cloud,
        &source,
        vertical,
        &HORIZONTAL_ONLY,
        &not_nad83(),
        &passthrough,
    )
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
    let err = pointcloud::crs::to_local_enu(
        &one,
        "EPSG:999999",
        1.0,
        &HORIZONTAL_ONLY,
        &not_nad83(),
        &passthrough,
    )
    .expect_err("999999 is not an EPSG code");
    let text = err.to_string();
    assert!(
        text.contains("999999"),
        "the refusal names the code: {text}"
    );
}

// -- GAP-197 (D-125): the Autzen capture through GEOID18, end to end -----------------
//
// **The independent values.** `pyproj` 3.8.0 (PROJ 9.8.1), network off, with the full
// pinned `us_noaa_g2018u0.tif` on its data path, ran `Transformer.from_crs(<the
// fixture's own WKT>, "EPSG:4979", always_xy=True)`. PROJ chose, by itself, "Inverse of
// Oregon GIC Lambert (international foot) + NAD83 to WGS 84 (1) + Inverse of NAD83(2011)
// to WGS 84 (1) + Conversion from NAVD88 height (ftUS) to NAVD88 height + Inverse of
// NAD83(2011) to NAVD88 height (3) + NAD83(2011) to WGS 84 (1)": the Lambert inverse,
// the US survey foot, `vgridshift` on GEOID18 with `multiplier=1`, and null steps
// between NAD83, NAD83(2011) and WGS 84 -- exactly the chain this workspace runs, which
// is the point: the metre-level NAD83(2011)-to-WGS-84 difference is unmodelled by PROJ's
// own choice too (`gungnir_data::geoid`'s module documentation). The figures below are
// that run's, transcribed; the script is not committed (the GAP-102 precedent).
//
// **Since GAP-200 (D-127) each height also carries the Helmert's height difference.** A
// NAVD88 height is a NAD83(2011) ellipsoidal one once GEOID18 has run, and the frame step
// to WGS 84 changes it by -0.3701 m here at epoch 2020.0 (between -0.37010 and -0.37014 m
// across the fixture). The same `pyproj` gave that difference from PROJ's own
// "Inverse of ITRF2014 to NAD83(2011) (1)" at each point, and each height below is the
// run above plus it. The positions stay PROJ's own (`not_nad83`), so the projection and
// the geoid step stay checked on their own.

/// The centre of `QUERY_BOX` (X=637250, Y=851150, Z=500 ftUS) converts to longitude
/// -123.06889823098363, latitude 44.056081952041154 and an ellipsoidal height of
/// 128.6925129932065 m: 152.4003048006096 m of NAVD88 height plus GEOID18's
/// -23.33767404713973 m there, plus the Helmert's -0.3701177602633834 m at 2020.0. The horizontal pair is unchanged from the horizontal-only
/// test above, to all seventeen digits.
#[test]
#[cfg(feature = "crs")]
// A single point is its own minimum corner, so its re-based position is exactly zero.
#[allow(clippy::float_cmp)]
fn the_real_capture_converts_through_geoid18_to_the_pyproj_heights() {
    use gungnir_data::geoid::{GeoidModel, HeightReference};
    let declared = autzen().crs.clone().expect("the fixture declares a CRS");
    let source = declared.proj_definition().expect("a WKT is a definition");
    let vertical = declared.vertical_unit_metres().expect("a VERT_CS");
    let one = pointcloud::PointBuffer {
        positions: vec![[0.0, 0.0, 0.0]],
        origin: [637_250.0, 851_150.0, 500.0],
        ..pointcloud::PointBuffer::default()
    };
    let out = pointcloud::crs::to_local_enu(
        &one,
        &source,
        vertical,
        &HeightReference::Geoid(GeoidModel::Geoid18Conus, geoid18_clip()),
        &not_nad83(),
        &passthrough,
    )
    .expect("inside the clip");
    assert_eq!(out.positions[0], [0.0, 0.0, 0.0]);
    let [lat_deg, lon_deg, h_m] = out.origin;
    assert!((lat_deg - 44.056_081_952_041_154).abs() < 1e-9, "{lat_deg}");
    assert!((lon_deg + 123.068_898_230_983_63).abs() < 1e-9, "{lon_deg}");
    assert!(
        (h_m - 128.692_512_993_206_5).abs() < 1e-6,
        "height {h_m}, pyproj 128.6925129932065"
    );
}

/// The whole bounded cloud through GEOID18: every one of the 4767 points converts, and
/// three of them, read from the loader in its own order (the COPC hierarchy's, which is
/// the file's and not this workspace's), land where `pyproj` puts them. Their absolute
/// coordinates are what the loader hands the conversion -- the file's minimum corner plus
/// each `f32` offset, in feet -- so the comparison is exact to rounding.
#[test]
#[cfg(feature = "crs")]
fn every_point_of_the_real_cloud_converts_through_geoid18_where_pyproj_puts_it() {
    use gungnir_data::geoid::{GeoidModel, HeightReference};
    let cloud = autzen();
    let declared = cloud.crs.clone().expect("the fixture declares a CRS");
    let source = declared.proj_definition().expect("a WKT is a definition");
    let vertical = declared.vertical_unit_metres().expect("a VERT_CS");
    let out = pointcloud::crs::to_local_enu(
        &cloud,
        &source,
        vertical,
        &HeightReference::Geoid(GeoidModel::Geoid18Conus, geoid18_clip()),
        &not_nad83(),
        &passthrough,
    )
    .expect("every point is inside the clip");
    assert_eq!(out.positions.len(), 4767, "no point is dropped");
    for (i, [lon, lat, h]) in [
        (
            0usize,
            [
                -123.069_093_314_703,
                44.056_210_851_503_74,
                104.631_820_927_891_1,
            ],
        ),
        (
            2383,
            [
                -123.068_845_818_079_5,
                44.056_215_340_207_004,
                104.241_364_547_468_85,
            ],
        ),
        (
            4766,
            [
                -123.068_993_638_646_72,
                44.055_995_495_492_74,
                104.271_994_162_844_75,
            ],
        ),
    ] {
        let p = out.positions[i];
        let got = [
            out.origin[0] + f64::from(p[0]),
            out.origin[1] + f64::from(p[1]),
            out.origin[2] + f64::from(p[2]),
        ];
        // Re-based offsets here are ten-thousandths of a degree and a few metres, where
        // an f32 carries about 1e-11 degree and a micrometre.
        assert!(
            (got[0] - lat).abs() < 1e-9,
            "point {i}: latitude {}",
            got[0]
        );
        assert!(
            (got[1] - lon).abs() < 1e-9,
            "point {i}: longitude {}",
            got[1]
        );
        assert!((got[2] - h).abs() < 1e-5, "point {i}: height {}", got[2]);
    }
}

// -- GAP-102 item (2), GAP-197: a LAS 1.0-1.3 file's geokey vertical system ----------

/// **A LAS 1.2 file whose `GeoTIFF` keys state NAVD88 in US survey feet.** Generated for
/// GAP-197 (`testdata/pointcloud/SOURCE.md`): five points at the Autzen capture's own
/// query-box centre, bounds and two points between, and a key directory copied byte for
/// byte from the one GDAL 3.11.3 writes for `-a_srs EPSG:2992+6360` -- `VerticalGeoKey`
/// 6360 and no unit key, because the code fixes the unit. So the unit and the datum here
/// are read from keys a real writer lays out, not from a layout this workspace chose.
#[test]
fn a_geokey_file_states_its_vertical_system_and_its_unit_through_the_code() {
    use gungnir_data::geoid::VerticalDatum;
    use gungnir_data::geospatial::GridCrs;
    use pointcloud::crs::{GeokeyCrs, PointCloudCrs};
    let cloud = pointcloud::load_las(&fixture("autzen-geokeys.las")).expect("loads");
    assert_eq!(cloud.positions.len(), 5);
    let Some(PointCloudCrs::Geokeys(keys)) = cloud.crs.as_ref() else {
        panic!("a LAS 1.2 file declares geokeys: {:?}", cloud.crs);
    };
    assert_eq!(
        *keys,
        GeokeyCrs {
            horizontal: GridCrs::Projected { epsg: Some(2992) },
            vertical: Some(6360),
            vertical_units: None,
            linear_units: None,
        }
    );
    let declared = cloud.crs.as_ref().expect("just matched");
    assert!(declared.agrees_with_epsg(AUTZEN_HORIZONTAL_EPSG));
    assert_eq!(declared.vertical_datum(), Some(VerticalDatum::Navd88));
    let unit = declared
        .vertical_unit()
        .expect("6360 fixes the US survey foot");
    assert!((unit - 1200.0 / 3937.0).abs() < 1e-15, "{unit}");
    assert_eq!(declared.proj_definition().as_deref(), Some("EPSG:2992"));
}

/// The same file through PROJ and GEOID18: `pyproj`, with the full pinned grid, took
/// `Transformer.from_crs("EPSG:2992+6360", "EPSG:4979", always_xy=True)` -- PROJ's own
/// choice, the same chain as the WKT's -- over the five points to the values below. The
/// first equals the WKT fixture's box centre, as it must: the same place, the same datum,
/// declared the other way LAS allows.
#[test]
#[cfg(feature = "crs")]
fn a_geokey_file_in_navd88_feet_converts_through_geoid18_where_pyproj_puts_it() {
    use gungnir_data::geoid::{GeoidModel, HeightReference};
    let cloud = pointcloud::load_las(&fixture("autzen-geokeys.las")).expect("loads");
    let declared = cloud.crs.clone().expect("declares geokeys");
    let source = declared.proj_definition().expect("an EPSG code");
    let vertical = declared.vertical_unit().expect("a unit");
    let out = pointcloud::crs::to_local_enu(
        &cloud,
        &source,
        vertical,
        &HeightReference::Geoid(GeoidModel::Geoid18Conus, geoid18_clip()),
        &not_nad83(),
        &passthrough,
    )
    .expect("inside the clip");
    for (i, [lon, lat, h]) in [
        [
            -123.068_898_230_983_63,
            44.056_081_952_041_154,
            128.692_512_993_206_5,
        ],
        [
            -123.074_986_743_530_63,
            44.049_718_818_802_12,
            100.091_103_403_470_61,
        ],
        [
            -123.062_512_596_976_29,
            44.062_780_307_017_22,
            163.817_331_800_242_44,
        ],
        [
            -123.069_082_419_779_08,
            44.055_940_538_335_6,
            113.605_092_029_014_39,
        ],
        [
            -123.066_147_183_440_44,
            44.058_477_365_450_614,
            104.381_659_087_480_27,
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let p = out.positions[i];
        let got = [
            out.origin[0] + f64::from(p[0]),
            out.origin[1] + f64::from(p[1]),
            out.origin[2] + f64::from(p[2]),
        ];
        // The loader carries each point as an f32 offset from the file's corner in feet
        // (3425.94 ft at most here, a 0.24 mft step), and the result as an f32 offset
        // from the converted corner; together up to about 1e-9 degree (0.1 mm) and a few
        // micrometres, so the tolerances are twice that and ten micrometres.
        assert!(
            (got[0] - lat).abs() < 2e-9,
            "point {i}: latitude {}",
            got[0]
        );
        assert!(
            (got[1] - lon).abs() < 2e-9,
            "point {i}: longitude {}",
            got[1]
        );
        assert!((got[2] - h).abs() < 1e-5, "point {i}: height {}", got[2]);
    }
}

// -- GAP-200 (D-127, D-128): the Autzen capture carried to WGS 84 by its realisation ----
//
// **The independent values.** `pyproj` 3.8.0 (PROJ 9.8.1), network off, with the four
// full pinned NADCON5 grids on its data path, took the query-box centre from EPSG:2992 to
// EPSG:4269 (the Lambert inverse alone, in the file's own datum), then from each
// realisation's geographic system to NAD83(2011) by PROJ's own choice of operation (none
// for NAD83(2011); NADCON5's grids for the others), then "Inverse of ITRF2014 to
// NAD83(2011) (1)" at epoch 2020.0. The height is the NAVD88 height plus GEOID18 plus
// that operation's height difference. The figures are transcribed; the clips agree with
// the full grids to 0.6 mm at most over this area (`testdata/nadcon5/SOURCE.md`), so the
// horizontal tolerance is 1e-8 degree (about a millimetre).

/// The four NADCON5 clips (`testdata/nadcon5/SOURCE.md`), as the verified grids a
/// deployment would hold.
#[cfg(feature = "crs")]
fn nadcon5_clip(
    step: gungnir_data::nad83::Nadcon5Step,
) -> Result<gungnir_data::geoid::GeoidGrid, String> {
    use gungnir_data::nad83::Nadcon5Step;
    let (file, sha256) = match step {
        Nadcon5Step::Nad83ToHarn => (
            "nadcon5_1986_harn_clip_43n45n_124w122w.tif",
            "2c1de773467589732394ca55fba70e194969b23cbcfb78c9b0655125804c1828",
        ),
        Nadcon5Step::HarnToFbn => (
            "nadcon5_harn_fbn_clip_43n45n_124w122w.tif",
            "efd6c9de5569ab7e6451be9a5deda1cf079c75ffee8a74ac10c5b80ab29bb639",
        ),
        Nadcon5Step::FbnToNsrs2007 => (
            "nadcon5_fbn_2007_clip_43n45n_124w122w.tif",
            "2cf42b29211aaf3a6b0ac20ccbfcc6bb623c1badb580770a1c263edae20f8a80",
        ),
        Nadcon5Step::Nsrs2007To2011 => (
            "nadcon5_2007_2011_clip_43n45n_124w122w.tif",
            "a460457996126ebef3a71232aa7f25e50bef8207516ab0e81648d7189e1eebe5",
        ),
    };
    gungnir_data::geoid::GeoidGrid::verify(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../testdata/nadcon5")
            .join(file),
        sha256,
    )
    .map_err(|e| e.to_string())
}

/// D-128 on the real capture: its datum, read by PROJ from its own WKT, is the
/// unqualified NAD83, refused without a declaration and taken as the declared realisation
/// with one; a realisation whose grids are missing is refused naming the grid.
#[test]
#[cfg(feature = "crs")]
fn the_real_capture_is_unqualified_nad83_and_needs_its_realisation_declared() {
    use gungnir_data::nad83::{FrameConversion, HorizontalDatum, Realisation};
    let declared = autzen().crs.clone().expect("the fixture declares a CRS");
    let source = declared.proj_definition().expect("a WKT is a definition");
    let file = gungnir_data::nad83::horizontal_datum(&source).expect("PROJ reads the WKT");
    assert_eq!(file, HorizontalDatum::Nad83Unqualified);
    assert_eq!(
        gungnir_data::nad83::horizontal_datum("EPSG:2992").expect("a code"),
        HorizontalDatum::Nad83Unqualified
    );
    assert_eq!(
        gungnir_data::nad83::horizontal_datum("EPSG:6339").expect("UTM 10N on NAD83(2011)"),
        HorizontalDatum::Nad83(Realisation::Nad83_2011)
    );
    assert_eq!(
        gungnir_data::nad83::horizontal_datum("EPSG:32610").expect("UTM 10N on WGS 84"),
        HorizontalDatum::NotNad83
    );
    let field = "point_cloud.nad83_realisation";
    let refused = FrameConversion::resolve(field, file, None, EPOCH, &nadcon5_clip)
        .expect_err("an unqualified NAD83 file needs its realisation declared");
    assert!(refused.contains(field), "{refused}");
    let missing = FrameConversion::resolve(field, file, Some(Realisation::Harn), EPOCH, &|step| {
        Err(format!("{} is not there", step.file()))
    })
    .expect_err("HARN needs three NADCON5 grids");
    assert!(
        missing.contains("us_noaa_nadcon5_nad83_harn_nad83_fbn_conus.tif"),
        "{missing}"
    );
    let conversion =
        FrameConversion::resolve(field, file, Some(Realisation::Harn), EPOCH, &nadcon5_clip)
            .expect("declared, with the clips");
    let nad83 = conversion.nad83().expect("a NAD83 step");
    assert_eq!(
        nad83.unprojected_to(),
        4269,
        "unprojected in the file's own datum"
    );
    assert_eq!(nad83.step().realisation(), Realisation::Harn);
}

/// D-127 and D-128 end to end: the query-box centre, declared each of three
/// realisations, lands where PROJ's own operations put it at epoch 2020.0 -- each a
/// different place, up to 0.8 m from the null step's -- with the NAVD88 height through
/// GEOID18 and the Helmert's height difference.
#[test]
#[cfg(feature = "crs")]
fn the_real_capture_reaches_wgs_84_by_its_realisation_where_pyproj_puts_it() {
    use gungnir_data::geoid::{GeoidModel, HeightReference};
    use gungnir_data::nad83::{FrameConversion, Realisation};
    let declared = autzen().crs.clone().expect("the fixture declares a CRS");
    let source = declared.proj_definition().expect("a WKT is a definition");
    let vertical = declared.vertical_unit_metres().expect("a VERT_CS");
    let file = gungnir_data::nad83::horizontal_datum(&source).expect("PROJ reads the WKT");
    let one = pointcloud::PointBuffer {
        positions: vec![[0.0, 0.0, 0.0]],
        origin: [637_250.0, 851_150.0, 500.0],
        ..pointcloud::PointBuffer::default()
    };
    for (realisation, [lon, lat, h]) in [
        (
            Realisation::Nad83_2011,
            [
                -123.068_916_764_793_5,
                44.056_084_529_291_53,
                128.692_513_484_944_8,
            ],
        ),
        (
            Realisation::Harn,
            [
                -123.068_916_311_826_33,
                44.056_085_968_351_85,
                128.692_513_504_502_59,
            ],
        ),
        (
            Realisation::Nad83_1986,
            [
                -123.068_908_997_868_28,
                44.056_081_759_368_89,
                128.692_513_246_526_23,
            ],
        ),
    ] {
        let conversion = FrameConversion::resolve(
            "point_cloud.nad83_realisation",
            file,
            Some(realisation),
            EPOCH,
            &nadcon5_clip,
        )
        .expect("declared, with the clips");
        let out = pointcloud::crs::to_local_enu(
            &one,
            &source,
            vertical,
            &HeightReference::Geoid(GeoidModel::Geoid18Conus, geoid18_clip()),
            &conversion,
            &passthrough,
        )
        .expect("inside every clip");
        let [got_lat, got_lon, got_h] = out.origin;
        assert!(
            (got_lat - lat).abs() < 1e-8,
            "{realisation}: latitude {got_lat}, pyproj {lat}"
        );
        assert!(
            (got_lon - lon).abs() < 1e-8,
            "{realisation}: longitude {got_lon}, pyproj {lon}"
        );
        // GEOID18 is read at the converted WGS 84 position, about 1.5 m from the NAD83(2011)
        // one it is indexed by and pyproj read it at: 0.02 mm here, so a tenth of a mm.
        assert!(
            (got_h - h).abs() < 1e-4,
            "{realisation}: height {got_h}, pyproj {h}"
        );
    }
}

/// The Helmert's height difference agrees with PROJ's own operation, and a position off a
/// NADCON5 clip is refused rather than placed by a grid that does not reach it.
#[test]
#[cfg(feature = "crs")]
fn the_height_difference_is_projs_and_a_position_off_the_grid_is_refused() {
    use gungnir_data::nad83::{FrameStep, Realisation};
    let dh = gungnir_data::nad83::nad83_2011_height_shift(
        EPOCH,
        &[[-123.068_898_230_983_63, 44.056_081_952_041_154]],
    )
    .expect("defined everywhere");
    assert!((dh[0] + 0.370_117_760_263_383_4).abs() < 1e-9, "{dh:?}");
    let step = FrameStep::new(Realisation::Harn, EPOCH, &nadcon5_clip).expect("the clips");
    let refused = step
        .to_wgs84(&[[-124.2, 44.0]])
        .expect_err("124.2 W is west of every clip");
    assert!(refused.to_string().contains("NAD83(HARN)"), "{refused}");
}
