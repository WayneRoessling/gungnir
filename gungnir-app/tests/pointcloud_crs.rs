// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-102 (D-41) on the desktop: reconciling the frame a baseline claims with the one a
//! point-cloud file declares for itself, and converting between them.
//!
//! **Two halves, and only one of them runs everywhere.** `placement` is a pure function
//! and every branch of it is checked below under a plain `cargo test`. The conversion
//! that follows a `Placement::Convert` links `libproj` and is therefore
//! `#[cfg(feature = "crs")]`; `ci.yml`'s `proj-crs` job is where it runs, and
//! `gungnir-data/Cargo.toml`'s `crs` feature explains why it cannot run on a stock
//! Windows developer machine.

use gungnir_app::pointcloud::{placement, Placement};
use gungnir_data::geospatial::GridCrs;
use gungnir_data::pointcloud::crs::PointCloudCrs;

/// The real WKT the vendored Autzen COPC fixture carries, read from its own record-2112
/// VLR. Kept whole rather than abbreviated: the point of these tests is what the real
/// declaration does, and a trimmed one would test a string this workspace wrote.
const AUTZEN_WKT: &str = r#"COMPD_CS["NAD83 / Oregon GIC Lambert (ft) + NAVD88 height (ftUS)",PROJCS["NAD83 / Oregon GIC Lambert (ft)",GEOGCS["NAD83",DATUM["North_American_Datum_1983",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6269"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4269"]],PROJECTION["Lambert_Conformal_Conic_2SP"],PARAMETER["latitude_of_origin",41.75],PARAMETER["central_meridian",-120.5],PARAMETER["standard_parallel_1",43],PARAMETER["standard_parallel_2",45.5],PARAMETER["false_easting",1312335.958],PARAMETER["false_northing",0],UNIT["foot",0.3048,AUTHORITY["EPSG","9002"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","2992"]],VERT_CS["NAVD88 height (ftUS)",VERT_DATUM["North American Vertical Datum 1988",2005,AUTHORITY["EPSG","5103"]],UNIT["US survey foot",0.304800609601219,AUTHORITY["EPSG","9003"]],AXIS["Gravity-related height",UP],AUTHORITY["EPSG","6360"]]]"#;

fn autzen() -> PointCloudCrs {
    PointCloudCrs::Wkt(AUTZEN_WKT.to_string())
}

/// The case that existed before this gap and still does: nobody declared anything, so
/// the baseline's word stands and the cloud is drawn exactly as it was.
#[test]
fn an_undeclared_file_under_a_local_enu_baseline_is_drawn_as_loaded() {
    assert_eq!(placement(None, None, false), Placement::AsLoaded);
    assert_eq!(placement(None, None, true), Placement::AsLoaded);
    // A geokey directory that names no code declares nothing either.
    assert_eq!(
        placement(None, Some(&PointCloudCrs::Geokeys(GridCrs::Unstated)), true),
        Placement::AsLoaded
    );
}

/// **The safety property this gap adds.** A baseline claiming the local frame for a file
/// whose own tags name a real-world system is refused by name -- the same rule
/// `terrain.rs`'s `placement_refusal` has always applied to a DEM, and the thing the gap
/// register said a point cloud had no way to do.
#[test]
fn a_local_enu_baseline_is_refused_when_the_file_declares_a_real_system() {
    let Placement::Refused(reason) = placement(None, Some(&autzen()), true) else {
        panic!("a declared CRS must contradict a local-enu claim");
    };
    assert!(
        reason.contains("NAD83 / Oregon GIC Lambert (ft)"),
        "{reason}"
    );
    assert!(reason.contains("epsg:<code>"), "{reason}");

    // The geokey form contradicts it just as well.
    let geokeys = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) });
    assert!(matches!(
        placement(None, Some(&geokeys), true),
        Placement::Refused(_)
    ));
}

/// A conversion has to land somewhere, and a deployment that declared no origin has no
/// local frame to land it on. Refused rather than defaulted -- the same rule
/// `ConfigBaseline::origin`'s own documentation states for every other geodetic thing on
/// the picture.
#[test]
fn a_conversion_without_a_declared_origin_is_refused_rather_than_anchored_somewhere() {
    let Placement::Refused(reason) = placement(Some(2992), Some(&autzen()), false) else {
        panic!("no origin means no frame to convert onto");
    };
    assert!(reason.contains("origin"), "{reason}");
    assert!(reason.contains("2992"), "{reason}");
}

/// The baseline and the file disagreeing is an operator error worth naming, not
/// something to resolve silently in either direction.
#[test]
fn a_baseline_that_names_a_different_code_than_the_file_is_refused() {
    let Placement::Refused(reason) = placement(Some(32610), Some(&autzen()), true) else {
        panic!("EPSG:32610 is not what this file declares");
    };
    assert!(reason.contains("32610"), "{reason}");
    assert!(
        reason.contains("NAD83 / Oregon GIC Lambert (ft)"),
        "{reason}"
    );
}

/// The happy path's *plan*: the file's own WKT is what PROJ is given, not the baseline's
/// bare code, and the vertical unit comes from the file's `VERT_CS`.
#[test]
fn a_matching_baseline_converts_from_the_files_own_definition() {
    let Placement::Convert {
        source,
        vertical_metres,
    } = placement(Some(2992), Some(&autzen()), true)
    else {
        panic!("a matching code should convert");
    };
    assert_eq!(
        source, AUTZEN_WKT,
        "the whole WKT, since a bare EPSG:2992 would drop the vertical system"
    );
    assert!(
        (vertical_metres - 1200.0 / 3937.0).abs() < 1e-15,
        "the US survey foot the file's VERT_CS declares, got {vertical_metres}"
    );
}

/// A file that declares nothing at all takes the baseline's code and is read as metres.
/// This is the one assumption in the path; `placement`'s own documentation names it.
#[test]
fn an_undeclared_file_under_an_epsg_baseline_takes_the_baselines_code() {
    let Placement::Convert {
        source,
        vertical_metres,
    } = placement(Some(32610), None, true)
    else {
        panic!("an undeclared file is taken at the baseline's word");
    };
    assert_eq!(source, "EPSG:32610");
    assert!((vertical_metres - 1.0).abs() < f64::EPSILON);
}

/// A file that declares a system but no unit this build can read for its heights is
/// refused, because a wrong vertical unit is a silent factor-of-three error rather than
/// a visible failure. The geokey form is exactly that case today
/// (`PointCloudCrs::vertical_unit_metres` says why it is left unread).
#[test]
fn a_declared_system_with_no_readable_vertical_unit_is_refused() {
    let geokeys = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) });
    let Placement::Refused(reason) = placement(Some(32610), Some(&geokeys), true) else {
        panic!("no readable vertical unit must refuse");
    };
    assert!(reason.contains("heights"), "{reason}");
}

// -- The conversion itself, through a real desktop tick -----------------------------

/// **The composed pipeline, end to end, on the real fixture**: the loader reads the
/// Autzen file's own CRS, `placement` plans a conversion from it, `libproj` does the
/// horizontal half, and `gungnir_model::LocalFrame` -- the same type every radar site,
/// asset and geofence on this picture is placed through -- does the ENU half.
///
/// The deployment origin is set to the geodetic centre of the very box the query asks
/// for, which the independent `pyproj` run recorded in
/// `gungnir-data/tests/pointcloud_crs.rs` gives as latitude 44.056081952041154,
/// longitude -123.06889823098363, height 152.4003048006096. The box is 100 x 100 in the
/// file's own feet, about 30 m square, so **every converted point must land within a few
/// tens of metres of the origin**. That is a real check rather than a loose one: a
/// feet-for-metres slip would put the cloud out by a factor of 3.28, a wrong projection
/// by kilometres, and a swapped axis order by thousands of kilometres. Any of the three
/// fails this bound by orders of magnitude.
#[test]
#[cfg(feature = "crs")]
fn the_real_fixture_converts_through_a_real_tick_onto_the_deployments_own_frame() {
    use gungnir_app::fusion::FusionBackend;
    use gungnir_app::pointcloud::PointCloudStatus;
    use gungnir_app::state::AppState;
    use gungnir_app::update;
    use gungnir_config::{ConfigBaseline, PointCloudConfig, PointCloudFileConfig};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/pointcloud/autzen-classified.copc.laz")
        .to_string_lossy()
        .into_owned();
    let bounded = |path: &str| PointCloudFileConfig {
        path: path.to_string(),
        copc_bounds: Some([637_200.0, 851_100.0, 400.0, 637_300.0, 851_200.0, 620.0]),
    };
    let dir = std::env::temp_dir().join(format!("gungnir-pointcloud-crs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([
            44.056_081_952_041_154_f64.to_radians(),
            (-123.068_898_230_983_63_f64).to_radians(),
            152.400_304_800_609_6,
        ]),
        point_cloud: Some(PointCloudConfig {
            // The same file on both halves: this test is about the conversion, and a
            // cloud registered onto itself is the one pair whose correct answer is known
            // without doing any registration at all.
            source: bounded(&fixture),
            target: bounded(&fixture),
            frame: "epsg:2992".into(),
        }),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("an epsg frame is valid since GAP-102");
    let mut state = AppState::with_config(config).expect("starts");
    // Never let a real tick resolve a real `wgpu` device (this crate's hard rule; see
    // `gungnir-app/tests/pointcloud.rs`'s module doc comment for the history).
    state.fusion = FusionBackend::Cpu {
        reason: "test: forced CPU path (gungnir-app/tests/pointcloud_crs.rs)".into(),
    };

    for _ in 0..6_000 {
        update::tick(&mut state);
        if !matches!(
            state.point_cloud,
            PointCloudStatus::Loading { .. } | PointCloudStatus::NotConfigured
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    match &state.point_cloud {
        PointCloudStatus::Loaded {
            source_points,
            target_points,
            ..
        } => {
            assert_eq!(*source_points, 4767);
            assert_eq!(*target_points, 4767);
        }
        other => panic!("{other:?}"),
    }

    let cloud = &state.data.point_clouds[0];
    assert_eq!(
        cloud.crs, None,
        "a converted cloud no longer carries the file's own claim"
    );
    for p in &cloud.positions {
        let enu = [
            f64::from(p[0]) + cloud.origin[0],
            f64::from(p[1]) + cloud.origin[1],
            f64::from(p[2]) + cloud.origin[2],
        ];
        assert!(
            enu[0].abs() < 60.0 && enu[1].abs() < 60.0,
            "a 30 m box centred on the origin must land within tens of metres of it, \
             got east {} north {}",
            enu[0],
            enu[1]
        );
        assert!(
            enu[2].abs() < 120.0,
            "the box's own height range is about 64 m, got up {}",
            enu[2]
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}
