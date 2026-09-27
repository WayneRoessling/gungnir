// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-108 (D-121): the EGM2008 geoid, read through PROJ, against an independent
//! `pyproj` run over the same grid; and GAP-197 (D-125): the EGM96 and GEOID18 grids,
//! checked the same way (the sections at the end of this file).
//!
//! **Which grid.** The pinned grid is 80 MB and is a deployment artifact, never a
//! repository file. What is committed is a 97 x 73 node clip of it around the DEM and
//! point-cloud fixtures (53-56 N, 13-17 E), cut by GDAL from the pinned file and recorded
//! with its own SHA-256 in `testdata/geoid/SOURCE.md`. Every node in it is the pinned
//! file's own value, so an undulation interpolated inside it is the pinned grid's.
//!
//! **What was run, independently** (the GAP-102 precedent: `pyproj`, recorded and not
//! committed). `pyproj` 3.8.0 bundling PROJ 9.8.1 -- a different build from the 9.6.2
//! `proj-sys` compiles, so a second implementation -- with the **full pinned grid** on
//! its data path and the network off. For each point below it gave the same undulation
//! three ways: `Transformer.from_crs("EPSG:4326+3855", "EPSG:4979")`, where PROJ chose
//! the operation itself ("Inverse of WGS 84 to EGM2008 height (1)", `vgridshift` on
//! `us_nga_egm08_25.tif`, `multiplier=1`); the explicit `vgridshift` pipeline on the
//! full file; and the same pipeline on the clip. Full and clip agreed to 3e-14 m. A hand
//! bilinear interpolation of the clip's four surrounding nodes, as GDAL prints them,
//! agreed to 3e-11 m, which is the ten-decimal printing and not a disagreement. The
//! values asserted here are that run's, transcribed.
//!
//! **The tolerance is a micrometre**, a thousandth of the centimetre the grid is good
//! for: both sides interpolate the same float32 nodes bilinearly in double precision, so
//! any real difference -- a different grid, a different node, the wrong sign -- is
//! metres, and anything under a micrometre is rounding.
//!
//! **What runs where.** Verifying the clip's own digest runs everywhere. Everything that
//! reads the grid goes through PROJ, so it is `#[cfg(feature = "crs")]` and runs in
//! `ci.yml`'s `proj-crs` job. The tests over the full pinned grids are `#[ignore]`d and
//! run by that job after it has fetched and checked the grids, from the directory
//! `GUNGNIR_GEOID_GRID_DIR` names.

use std::path::{Path, PathBuf};

#[cfg(feature = "crs")]
use gungnir_data::geoid::VerticalDatum;
use gungnir_data::geoid::{GeoidGrid, GeoidModel};

/// `testdata/geoid/SOURCE.md` records this digest for the clip.
const CLIP_SHA256: &str = "60a16af44ca47724fd6cbb58565104a010dd2ef8c2d5ec1c666552052fa83e10";

fn clip_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/geoid/egm08_25_clip_53n56n_13e17e.tif")
}

fn clip() -> GeoidGrid {
    GeoidGrid::verify(&clip_path(), CLIP_SHA256).expect("the committed clip is the recorded one")
}

/// `[longitude, latitude, N]`, N from the independent `pyproj` run over the full grid.
/// The first is the DEM fixture's own south-west corner (`testdata/dem/SOURCE.md`); the
/// last sits exactly on a node, where interpolation has nothing to do and N is the node
/// value GDAL reads out of the file (34.678050994873047, float32).
#[cfg(feature = "crs")]
const PYPROJ: [[f64; 3]; 5] = [
    [15.0, 54.148_104_104, 34.924_758_738_286_63],
    [15.001_23, 54.148_30, 34.923_654_345_700_44],
    [13.37, 55.61, 35.881_427_008_056_63],
    [16.83, 53.27, 33.399_890_936_279_29],
    [15.0, 54.5, 34.678_050_994_873_05],
];

#[test]
fn the_committed_clip_is_the_one_its_source_records() {
    let grid = clip();
    assert_eq!(grid.sha256(), CLIP_SHA256);
    assert!(grid.path().is_absolute());
    // And it is not the pinned grid, so no deployment could be handed it as one.
    let err = GeoidGrid::verify(&clip_path(), gungnir_data::geoid::EGM2008_GRID_SHA256)
        .expect_err("a clip is not the pinned grid");
    assert!(err.to_string().contains(CLIP_SHA256), "{err}");
}

/// The undulation PROJ reads from the clip agrees with `pyproj` over the full pinned
/// grid, to a micrometre, at five points: one on a node and four between nodes.
#[test]
#[cfg(feature = "crs")]
fn the_undulation_agrees_with_an_independent_pyproj_run_over_the_full_grid() {
    let grid = clip();
    let points: Vec<[f64; 2]> = PYPROJ.iter().map(|p| [p[0], p[1]]).collect();
    let n = gungnir_data::geoid::undulations(&grid, &points).expect("inside the clip");
    for (got, [lon, lat, want]) in n.iter().zip(PYPROJ) {
        assert!(
            (got - want).abs() < 1e-6,
            "at ({lon}, {lat}): PROJ read {got} m, pyproj {want} m"
        );
    }
    // The sign is the geodesy, not a convention: the geoid is 35 m *above* the WGS-84
    // ellipsoid over the Baltic, so an EGM2008 height is 35 m less than the
    // ellipsoidal height of the same point.
    assert!(n.iter().all(|n| *n > 33.0 && *n < 36.0), "{n:?}");
}

/// An EGM2008 height becomes an ellipsoidal one by adding N, and a point with no height
/// keeps none.
#[test]
#[cfg(feature = "crs")]
fn an_egm2008_height_gains_the_undulation_and_a_hole_stays_a_hole() {
    use gungnir_data::geoid::{ellipsoidal_heights, HeightReference};
    let reference = HeightReference::Geoid(GeoidModel::Egm2008, clip());
    let out = ellipsoidal_heights(
        &reference,
        &[[PYPROJ[0][0], PYPROJ[0][1]], [PYPROJ[4][0], PYPROJ[4][1]]],
        &[10.0, f64::NAN],
    )
    .expect("inside the clip");
    assert!((out[0] - (10.0 + PYPROJ[0][2])).abs() < 1e-6, "{out:?}");
    assert!(out[1].is_nan(), "{out:?}");
}

/// Off the grid is a refusal naming the point, never a zero: `pyproj` over the full grid
/// gives 39.771907806396484 m at (10 E, 54 N), and the clip, which stops at 13 E, has no
/// value there at all.
#[test]
#[cfg(feature = "crs")]
fn a_point_off_the_grid_is_refused_rather_than_given_a_zero() {
    let err = gungnir_data::geoid::undulations(&clip(), &[[10.0, 54.0]])
        .expect_err("the clip stops at 13 E");
    let text = err.to_string();
    assert!(text.contains("longitude 10"), "{text}");
}

/// PROJ is handed the verified file by its absolute path, double-quoted, so a directory
/// with a space in it -- `C:\Program Files\...` on a Windows desktop -- still names the
/// one file that was hashed.
#[test]
#[cfg(feature = "crs")]
fn a_grid_in_a_directory_with_a_space_is_still_read() {
    let dir = std::env::temp_dir().join(format!("gungnir geoid spaced {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let copy = dir.join("clip.tif");
    std::fs::copy(clip_path(), &copy).expect("copies");
    let grid = GeoidGrid::verify(&copy, CLIP_SHA256).expect("same bytes");
    let n = gungnir_data::geoid::undulations(&grid, &[[PYPROJ[4][0], PYPROJ[4][1]]])
        .expect("read through the quoted path");
    assert!((n[0] - PYPROJ[4][2]).abs() < 1e-6, "{n:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// **No ballpark.** A grid verified and then taken away is a refusal from PROJ when the
/// pipeline is built, not the height unchanged -- which is what PROJ's own operation
/// search would fall back to.
#[test]
#[cfg(feature = "crs")]
fn a_grid_that_disappears_after_verification_is_refused_not_ballparked() {
    let dir = std::env::temp_dir().join(format!("gungnir-geoid-gone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let copy = dir.join("clip.tif");
    std::fs::copy(clip_path(), &copy).expect("copies");
    let grid = GeoidGrid::verify(&copy, CLIP_SHA256).expect("same bytes");
    std::fs::remove_file(&copy).expect("removes");
    let err =
        gungnir_data::geoid::undulations(&grid, &[[15.0, 54.5]]).expect_err("the file is gone");
    assert!(
        err.to_string().contains("cannot read the geoid grid"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-196 (D-123): the live lookup a Category 129 feed asks once per report gives the
/// same undulation as `pyproj` over the full grid, from the thread that started it and
/// from another one -- the ingest adapter asks from the gateway's thread, and the
/// service is shared by every feed.
#[test]
#[cfg(feature = "crs")]
fn the_live_lookup_agrees_with_pyproj_from_any_thread() {
    use gungnir_data::geoid::UndulationService;
    let service = UndulationService::start(&clip()).expect("PROJ opens the clip");
    assert_eq!(service.grid().sha256(), CLIP_SHA256);
    for [lon, lat, want] in PYPROJ {
        let got = service.undulation(lon, lat).expect("inside the clip");
        assert!(
            (got - want).abs() < 1e-6,
            "at ({lon}, {lat}): {got} vs {want}"
        );
    }
    let shared = service.clone();
    let from_elsewhere = std::thread::spawn(move || shared.undulation(PYPROJ[2][0], PYPROJ[2][1]))
        .join()
        .expect("the asking thread finishes")
        .expect("inside the clip");
    assert!((from_elsewhere - PYPROJ[2][2]).abs() < 1e-6);
}

/// Off the grid is a refusal naming the point, never a zero -- through the live lookup
/// as through [`gungnir_data::geoid::undulations`]. The Category 129 fixture's own
/// position (10 N, 20 W) is one such point for the clip.
#[test]
#[cfg(feature = "crs")]
fn the_live_lookup_refuses_a_point_off_the_grid() {
    let service = gungnir_data::geoid::UndulationService::start(&clip()).expect("opens");
    let err = service
        .undulation(-20.0, 10.0)
        .expect_err("the clip is the Baltic");
    assert!(err.to_string().contains("longitude -20"), "{err}");
    let err = service.undulation(f64::NAN, 54.0).expect_err("not finite");
    assert!(err.to_string().contains("not finite"), "{err}");
}

/// A grid verified and then taken away is a refusal when the lookup starts, not a
/// service that answers zero.
#[test]
#[cfg(feature = "crs")]
fn the_live_lookup_refuses_a_grid_that_disappeared_after_verification() {
    let dir = std::env::temp_dir().join(format!("gungnir-geoid-live-gone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let copy = dir.join("clip.tif");
    std::fs::copy(clip_path(), &copy).expect("copies");
    let grid = GeoidGrid::verify(&copy, CLIP_SHA256).expect("same bytes");
    std::fs::remove_file(&copy).expect("removes");
    let err = gungnir_data::geoid::UndulationService::start(&grid).expect_err("the file is gone");
    assert!(
        err.to_string().contains("cannot read the geoid grid"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Without `crs` there is no PROJ to read the grid with, and the live lookup says so by
/// naming the feature, rather than answering anything.
#[test]
#[cfg(not(feature = "crs"))]
fn without_crs_the_live_lookup_is_refused_by_naming_the_feature() {
    let err = gungnir_data::geoid::UndulationService::start(&clip()).expect_err("no PROJ");
    assert!(
        matches!(err, gungnir_data::DataError::NotImplemented { .. }),
        "{err}"
    );
    assert!(err.to_string().contains("crs"), "{err}");
}

/// The whole point-cloud conversion for a file that states its heights as EGM2008:
/// `pyproj`, over the full grid, took `Transformer.from_crs(<this WKT>, "EPSG:4979",
/// always_xy=True)` for five-points.las's own first and fifth points (easting 500010,
/// northing 6000020, height 12.5; and 500013.5, 6000019, 12.0) to
///
/// ```text
/// lon 15.00015310098077  lat 54.14828385766182  h 47.42449178478446
/// lon 15.00020668627928  lat 54.14827486988743  h 46.92445825608703
/// ```
///
/// -- the same longitude and latitude as the horizontal-only transform, and a height
/// 34.924 m above the file's, the undulation there.
#[test]
#[cfg(feature = "crs")]
fn a_cloud_in_utm_with_egm2008_heights_converts_to_the_pyproj_ellipsoidal_height() {
    use gungnir_data::geoid::HeightReference;
    use gungnir_data::pointcloud::{self, crs::PointCloudCrs};

    let crs = PointCloudCrs::Wkt(UTM33_EGM2008_WKT.to_string());
    assert_eq!(crs.vertical_datum(), Some(VerticalDatum::Egm2008));
    let source = crs.proj_definition().expect("a WKT");
    let vertical = crs.vertical_unit_metres().expect("metres");
    let cloud = pointcloud::PointBuffer {
        positions: vec![[0.0, 1.0, 0.5], [3.5, 0.0, 0.0]],
        origin: [500_010.0, 6_000_019.0, 12.0],
        crs: Some(crs),
        ..pointcloud::PointBuffer::default()
    };
    let passthrough = |g: [f64; 3]| [g[0].to_degrees(), g[1].to_degrees(), g[2]];
    let out = pointcloud::crs::to_local_enu(
        &cloud,
        &source,
        vertical,
        &HeightReference::Geoid(GeoidModel::Egm2008, clip()),
        &passthrough,
    )
    .expect("converts");
    let absolute = |i: usize| {
        let p = out.positions[i];
        [
            out.origin[0] + f64::from(p[0]),
            out.origin[1] + f64::from(p[1]),
            out.origin[2] + f64::from(p[2]),
        ]
    };
    for (i, [lat, lon, h]) in [
        [
            54.148_283_857_661_82,
            15.000_153_100_980_77,
            47.424_491_784_784_46,
        ],
        [
            54.148_274_869_887_43,
            15.000_206_686_279_28,
            46.924_458_256_087_03,
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let [got_lat, got_lon, got_h] = absolute(i);
        // Re-basing puts each point through an f32 offset from the cloud's own corner;
        // the offsets here are thousandths of a degree and fractions of a metre, where
        // an f32 carries a few parts in 1e10 of a degree and a few micrometres.
        assert!(
            (got_lat - lat).abs() < 1e-9,
            "point {i}: latitude {got_lat}, pyproj {lat}"
        );
        assert!(
            (got_lon - lon).abs() < 1e-9,
            "point {i}: longitude {got_lon}, pyproj {lon}"
        );
        assert!(
            (got_h - h).abs() < 1e-6,
            "point {i}: height {got_h}, pyproj {h}"
        );
    }
}

/// **The ellipsoidal-already case passes through unchanged**, through the real PROJ
/// conversion: the same two points, stated as WGS-84 ellipsoidal heights, keep them.
#[test]
#[cfg(feature = "crs")]
fn a_cloud_whose_heights_are_already_ellipsoidal_keeps_them() {
    use gungnir_data::geoid::HeightReference;
    use gungnir_data::pointcloud::{self, crs::PointCloudCrs};

    let wkt = UTM33_EGM2008_WKT.replace(
        r#"VERT_DATUM["EGM2008 geoid",2005,AUTHORITY["EPSG","1027"]]"#,
        r#"VERT_DATUM["Ellipsoid",2002]"#,
    );
    let crs = PointCloudCrs::Wkt(wkt);
    assert_eq!(crs.vertical_datum(), Some(VerticalDatum::Ellipsoidal));
    let cloud = pointcloud::PointBuffer {
        positions: vec![[0.0, 1.0, 0.5], [3.5, 0.0, 0.0]],
        origin: [500_010.0, 6_000_019.0, 12.0],
        ..pointcloud::PointBuffer::default()
    };
    let source = crs.proj_definition().expect("a WKT");
    let out =
        pointcloud::crs::to_local_enu(&cloud, &source, 1.0, &HeightReference::Ellipsoidal, &|g| {
            [g[0].to_degrees(), g[1].to_degrees(), g[2]]
        })
        .expect("converts");
    // The corner's height is the lower of the two, 12.0, and the other sits 0.5 above.
    assert!((out.origin[2] - 12.0).abs() < 1e-9, "{:?}", out.origin);
    assert!((f64::from(out.positions[0][2]) - 0.5).abs() < 1e-6);
    assert!(f64::from(out.positions[1][2]).abs() < 1e-6);
}

/// The full pinned grid, when the CI job has fetched and checked it
/// (`GUNGNIR_GEOID_GRID_DIR`): it verifies against the pinned digest, gives the clip's
/// answers inside the clip, and answers outside it too -- 39.771907806396484 m at
/// (10 E, 54 N) per the same `pyproj` run.
#[test]
#[ignore = "needs the pinned 80 MB grid; ci.yml's proj-crs job fetches it and runs this"]
#[cfg(feature = "crs")]
fn the_full_pinned_grid_verifies_and_agrees_with_pyproj() {
    let grid =
        GeoidGrid::verify_pinned_in(&pinned_dir(), GeoidModel::Egm2008).expect("the pinned grid");
    let mut points: Vec<[f64; 2]> = PYPROJ.iter().map(|p| [p[0], p[1]]).collect();
    points.push([10.0, 54.0]);
    let n = gungnir_data::geoid::undulations(&grid, &points).expect("global");
    for (got, [lon, lat, want]) in n.iter().zip(PYPROJ) {
        assert!((got - want).abs() < 1e-6, "({lon}, {lat}): {got} vs {want}");
    }
    assert!((n[5] - 39.771_907_806_396_484).abs() < 1e-6, "{n:?}");
}

/// The compound WKT GDAL writes for EPSG:32633+3855 (`pyproj`'s
/// `CRS("EPSG:32633+3855").to_wkt("WKT1_GDAL")`, transcribed), the form a LAS 1.4
/// writer built on GDAL or PDAL puts in its record-2112 VLR.
#[cfg(feature = "crs")]
const UTM33_EGM2008_WKT: &str = r#"COMPD_CS["WGS 84 / UTM zone 33N + EGM2008 height",PROJCS["WGS 84 / UTM zone 33N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",15],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","32633"]],VERT_CS["EGM2008 height",VERT_DATUM["EGM2008 geoid",2005,AUTHORITY["EPSG","1027"]],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Gravity-related height",UP],AUTHORITY["EPSG","3855"]]]"#;

/// The directory `ci.yml`'s `proj-crs` job fetched and checked the pinned grids into.
#[cfg(feature = "crs")]
fn pinned_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("GUNGNIR_GEOID_GRID_DIR")
            .expect("GUNGNIR_GEOID_GRID_DIR names the directory holding the pinned grids"),
    )
}

// -- GAP-197 (D-125): EGM96 ----------------------------------------------------------
//
// The committed clip is 17 x 13 nodes of the pinned 15-arc-minute grid over the same
// 53-56 N, 13-17 E as the EGM2008 clip, cut by GDAL by node offsets
// (`testdata/geoid/SOURCE.md`). The same `pyproj` run, with the full pinned
// `us_nga_egm96_15.tif` on its path, gave each undulation below three ways -- PROJ's own
// choice for `EPSG:4326+5773` to `EPSG:4979` ("Inverse of WGS 84 to EGM96 height (1)",
// `vgridshift` on that file, `multiplier=1`), the written-out pipeline on the full file,
// and on the clip -- agreeing to 7e-14 m.

/// `testdata/geoid/SOURCE.md` records this digest for the EGM96 clip.
const EGM96_CLIP_SHA256: &str = "9b8e9b6c7811e88b72af9191e118e9c43ed03630b84ea2b3789647e925300b19";

fn egm96_clip_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/geoid/egm96_15_clip_53n56n_13e17e.tif")
}

fn egm96_clip() -> GeoidGrid {
    GeoidGrid::verify(&egm96_clip_path(), EGM96_CLIP_SHA256)
        .expect("the committed EGM96 clip is the recorded one")
}

/// `[longitude, latitude, N]` over the full pinned EGM96 grid, at the same five points
/// as [`PYPROJ`]; the last is on a node (33.99778747558594, float32).
#[cfg(feature = "crs")]
const PYPROJ_EGM96: [[f64; 3]; 5] = [
    [15.0, 54.148_104_104, 34.538_621_030_444_58],
    [15.001_23, 54.148_30, 34.537_525_273_543_78],
    [13.37, 55.61, 35.408_993_579_101_58],
    [16.83, 53.27, 33.244_335_424_804_746],
    [15.0, 54.5, 33.997_787_475_585_94],
];

#[test]
fn the_committed_egm96_clip_is_the_one_its_source_records_and_not_the_pinned_grid() {
    let grid = egm96_clip();
    assert_eq!(grid.sha256(), EGM96_CLIP_SHA256);
    let err = GeoidGrid::verify(&egm96_clip_path(), GeoidModel::Egm96.sha256())
        .expect_err("a clip is not the pinned grid");
    assert!(err.to_string().contains(EGM96_CLIP_SHA256), "{err}");
}

/// EGM96's undulation, through PROJ, agrees with `pyproj` over the full pinned grid to a
/// micrometre. **It is not EGM2008's**: over the Baltic the two models differ by 0.39 m
/// at the DEM corner and 0.68 m on the node at 54.5 N -- the "smaller silent mix" D-121
/// refused to accept by approximating one with the other, and why each has its own grid.
#[test]
#[cfg(feature = "crs")]
fn the_egm96_undulation_agrees_with_pyproj_and_differs_from_egm2008() {
    let points: Vec<[f64; 2]> = PYPROJ_EGM96.iter().map(|p| [p[0], p[1]]).collect();
    let n = gungnir_data::geoid::undulations(&egm96_clip(), &points).expect("inside the clip");
    for (got, [lon, lat, want]) in n.iter().zip(PYPROJ_EGM96) {
        assert!(
            (got - want).abs() < 1e-6,
            "at ({lon}, {lat}): PROJ read {got} m, pyproj {want} m"
        );
    }
    let egm2008 = gungnir_data::geoid::undulations(&clip(), &points).expect("inside");
    for (a, b) in n.iter().zip(&egm2008) {
        assert!((a - b).abs() > 0.1, "EGM96 {a} against EGM2008 {b}");
    }
}

/// The whole point-cloud conversion for EGM96 heights: `pyproj`, over the full pinned
/// grid, took `Transformer.from_crs("EPSG:32633+5773", "EPSG:4979", always_xy=True)` for
/// five-points.las's first and fifth points to heights 47.03827116141956 and
/// 46.53824749931015 m -- the same longitude and latitude as the EGM2008 test above, and
/// heights 0.386 m below the EGM2008 ones, the difference between the two models there.
#[test]
#[cfg(feature = "crs")]
fn a_cloud_in_utm_with_egm96_heights_converts_to_the_pyproj_ellipsoidal_height() {
    use gungnir_data::geoid::HeightReference;
    use gungnir_data::pointcloud::{self, crs::PointCloudCrs};

    let wkt = UTM33_EGM2008_WKT
        .replace("EGM2008 height", "EGM96 height")
        .replace(
            r#"VERT_DATUM["EGM2008 geoid",2005,AUTHORITY["EPSG","1027"]]"#,
            r#"VERT_DATUM["EGM96 geoid",2005,AUTHORITY["EPSG","5171"]]"#,
        )
        .replace(r#"AUTHORITY["EPSG","3855"]"#, r#"AUTHORITY["EPSG","5773"]"#);
    let crs = PointCloudCrs::Wkt(wkt);
    assert_eq!(crs.vertical_datum(), Some(VerticalDatum::Egm96));
    let source = crs.proj_definition().expect("a WKT");
    let cloud = pointcloud::PointBuffer {
        positions: vec![[0.0, 1.0, 0.5], [3.5, 0.0, 0.0]],
        origin: [500_010.0, 6_000_019.0, 12.0],
        crs: Some(crs),
        ..pointcloud::PointBuffer::default()
    };
    let out = pointcloud::crs::to_local_enu(
        &cloud,
        &source,
        1.0,
        &HeightReference::Geoid(GeoidModel::Egm96, egm96_clip()),
        &|g| [g[0].to_degrees(), g[1].to_degrees(), g[2]],
    )
    .expect("converts");
    for (i, h) in [47.038_271_161_419_56, 46.538_247_499_310_15]
        .into_iter()
        .enumerate()
    {
        let got = out.origin[2] + f64::from(out.positions[i][2]);
        assert!(
            (got - h).abs() < 1e-6,
            "point {i}: height {got}, pyproj {h}"
        );
    }
}

/// Off the clip is a refusal, never a zero: the full grid gives 39.505645751953125 m at
/// (10 E, 54 N), which the clip does not reach.
#[test]
#[cfg(feature = "crs")]
fn a_point_off_the_egm96_clip_is_refused_rather_than_given_a_zero() {
    let err = gungnir_data::geoid::undulations(&egm96_clip(), &[[10.0, 54.0]])
        .expect_err("the clip stops at 13 E");
    assert!(err.to_string().contains("longitude 10"), "{err}");
}

/// The full pinned EGM96 grid, fetched and checked by the CI job: it verifies against the
/// pinned digest, agrees with the clip's answers inside the clip, and answers outside it.
#[test]
#[ignore = "needs the pinned EGM96 grid; ci.yml's proj-crs job fetches it and runs this"]
#[cfg(feature = "crs")]
fn the_full_pinned_egm96_grid_verifies_and_agrees_with_pyproj() {
    let grid =
        GeoidGrid::verify_pinned_in(&pinned_dir(), GeoidModel::Egm96).expect("the pinned grid");
    let mut points: Vec<[f64; 2]> = PYPROJ_EGM96.iter().map(|p| [p[0], p[1]]).collect();
    points.push([10.0, 54.0]);
    let n = gungnir_data::geoid::undulations(&grid, &points).expect("global");
    for (got, [lon, lat, want]) in n.iter().zip(PYPROJ_EGM96) {
        assert!((got - want).abs() < 1e-6, "({lon}, {lat}): {got} vs {want}");
    }
    assert!((n[5] - 39.505_645_751_953_125).abs() < 1e-6, "{n:?}");
}

// -- GAP-197 (D-125): GEOID18, for NAVD88 --------------------------------------------
//
// The committed clip is 61 x 61 nodes of the pinned 1-arc-minute CONUS grid, 43.5-44.5 N
// and 123.5-122.5 W around the Autzen fixture, cut by GDAL by node offsets
// (`testdata/geoid/SOURCE.md`; the grid stores longitude as 236.5-237.5 E, which PROJ
// reads either way). The same `pyproj` run, with the full pinned `us_noaa_g2018u0.tif`
// on its path, gave each undulation three ways -- PROJ's own choice for
// `EPSG:6318+5703` to `EPSG:6319` ("Inverse of NAD83(2011) to NAVD88 height (3)",
// `vgridshift` on that file, `multiplier=1`), the written-out pipeline on the full file,
// and on the clip -- agreeing to 3e-14 m. N here is GEOID18's height of the geoid above
// the NAD83(2011) ellipsoid, not WGS 84's: `gungnir_data::geoid`'s module documentation
// states what that leaves.

/// `testdata/geoid/SOURCE.md` records this digest for the GEOID18 clip.
const GEOID18_CLIP_SHA256: &str =
    "a93c8ca6a47cc3d5a58ffeeac469aa9c8c9ca2400a0113a1976eb0a4bc9b16eb";

fn geoid18_clip_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/geoid/g2018u0_clip_43n45n_124w122w.tif")
}

fn geoid18_clip() -> GeoidGrid {
    GeoidGrid::verify(&geoid18_clip_path(), GEOID18_CLIP_SHA256)
        .expect("the committed GEOID18 clip is the recorded one")
}

/// `[longitude, latitude, N]` over the full pinned GEOID18 grid. The first is the centre
/// of the Autzen query box, converted (`gungnir-data/tests/pointcloud_crs.rs`); the
/// second the capture's own south-west corner; the last is on a node.
#[cfg(feature = "crs")]
const PYPROJ_GEOID18: [[f64; 3]; 5] = [
    [
        -123.068_898_230_983_63,
        44.056_081_952_041_154,
        -23.337_674_047_139_73,
    ],
    [-123.074_986_74, 44.049_718_82, -23.330_517_586_705_497],
    [-122.7, 43.8, -22.754_699_707_022_173],
    [-123.4, 44.4, -22.498_300_552_383_512],
    [-123.0, 44.0, -23.439_199_447_637_318],
];

#[test]
fn the_committed_geoid18_clip_is_the_one_its_source_records_and_not_the_pinned_grid() {
    let grid = geoid18_clip();
    assert_eq!(grid.sha256(), GEOID18_CLIP_SHA256);
    let err = GeoidGrid::verify(&geoid18_clip_path(), GeoidModel::Geoid18Conus.sha256())
        .expect_err("a clip is not the pinned grid");
    assert!(err.to_string().contains(GEOID18_CLIP_SHA256), "{err}");
}

/// GEOID18's undulation, through PROJ, agrees with `pyproj` over the full pinned grid to
/// a micrometre. The sign is the geodesy: the geoid sits 23 m **below** the ellipsoid in
/// Oregon, so a NAVD88 height is 23 m more than the ellipsoidal height of the same point.
#[test]
#[cfg(feature = "crs")]
fn the_geoid18_undulation_agrees_with_an_independent_pyproj_run() {
    let points: Vec<[f64; 2]> = PYPROJ_GEOID18.iter().map(|p| [p[0], p[1]]).collect();
    let n = gungnir_data::geoid::undulations(&geoid18_clip(), &points).expect("inside the clip");
    for (got, [lon, lat, want]) in n.iter().zip(PYPROJ_GEOID18) {
        assert!(
            (got - want).abs() < 1e-6,
            "at ({lon}, {lat}): PROJ read {got} m, pyproj {want} m"
        );
    }
    assert!(n.iter().all(|n| *n > -24.0 && *n < -22.0), "{n:?}");
}

/// **GEOID18 is a CONUS grid, and a NAVD88 height outside it is refused, never given a
/// zero.** Honolulu and Anchorage are outside the pinned grid's 24-58 N, 130-60 W
/// (`pyproj` over the full grid returns `inf` at both), and 121 W is outside this clip.
/// The refusal names the model's coverage, so an operator reads why, and that NAVD88
/// there is GEOID12B's.
#[test]
#[cfg(feature = "crs")]
fn a_navd88_height_outside_geoid18s_grid_is_refused_naming_the_coverage() {
    use gungnir_data::geoid::{ellipsoidal_heights, HeightReference};
    let reference = HeightReference::Geoid(GeoidModel::Geoid18Conus, geoid18_clip());
    for [lon, lat] in [[-157.9, 21.3], [-150.0, 61.0], [-121.0, 44.0]] {
        let err =
            ellipsoidal_heights(&reference, &[[lon, lat]], &[10.0]).expect_err("outside the grid");
        let text = err.to_string();
        assert!(text.contains("conterminous United States"), "{text}");
        assert!(text.contains("GEOID12B"), "{text}");
    }
}

/// The full pinned GEOID18 grid, fetched and checked by the CI job: it verifies, agrees
/// with the clip's answers, answers outside the clip (-20.65489959715147 m at 121 W,
/// 44 N), and still refuses Honolulu and Anchorage, which are outside the grid itself.
#[test]
#[ignore = "needs the pinned GEOID18 grid; ci.yml's proj-crs job fetches it and runs this"]
#[cfg(feature = "crs")]
fn the_full_pinned_geoid18_grid_verifies_and_agrees_with_pyproj() {
    let grid = GeoidGrid::verify_pinned_in(&pinned_dir(), GeoidModel::Geoid18Conus)
        .expect("the pinned grid");
    let mut points: Vec<[f64; 2]> = PYPROJ_GEOID18.iter().map(|p| [p[0], p[1]]).collect();
    points.push([-121.0, 44.0]);
    let n = gungnir_data::geoid::undulations(&grid, &points).expect("inside CONUS");
    for (got, [lon, lat, want]) in n.iter().zip(PYPROJ_GEOID18) {
        assert!((got - want).abs() < 1e-6, "({lon}, {lat}): {got} vs {want}");
    }
    assert!((n[5] + 20.654_899_597_151_47).abs() < 1e-6, "{n:?}");
    for outside in [[-157.9, 21.3], [-150.0, 61.0]] {
        assert!(
            gungnir_data::geoid::undulations(&grid, &[outside]).is_err(),
            "{outside:?} is outside GEOID18's grid"
        );
    }
}
