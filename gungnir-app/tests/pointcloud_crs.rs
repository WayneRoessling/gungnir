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
        placement(
            None,
            Some(&PointCloudCrs::Geokeys(GridCrs::Unstated.into())),
            true
        ),
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
    let geokeys = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) }.into());
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
    let geokeys = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) }.into());
    let Placement::Refused(reason) = placement(Some(32610), Some(&geokeys), true) else {
        panic!("no readable vertical unit must refuse");
    };
    assert!(reason.contains("heights"), "{reason}");
}

// -- The conversion itself, through a real desktop tick -----------------------------

/// A desktop over `pair` with `origin`, CPU registration forced (never a real `wgpu`
/// device in a test: `gungnir-app/tests/pointcloud.rs`'s module doc comment has the
/// history), and a `geoid_grid_dir` that holds no grid, so the state of the grid is the
/// test's to set rather than `PROJ_DATA`'s.
fn desktop(
    name: &str,
    pair: gungnir_config::PointCloudConfig,
    origin: [f64; 3],
) -> (gungnir_app::state::AppState, std::path::PathBuf) {
    use gungnir_app::fusion::FusionBackend;
    use gungnir_config::ConfigBaseline;

    let dir = std::env::temp_dir().join(format!(
        "gungnir-pointcloud-crs-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(origin),
        point_cloud: Some(pair),
        geoid_grid_dir: Some(dir.join("no-grid-here").to_string_lossy().into_owned()),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("an epsg frame is valid since GAP-102");
    let mut state = gungnir_app::state::AppState::with_config(config).expect("starts");
    state.fusion = FusionBackend::Cpu {
        reason: "test: forced CPU path (gungnir-app/tests/pointcloud_crs.rs)".into(),
    };
    (state, dir)
}

/// Tick until the pair has loaded or failed. A deadlock guard, not a performance
/// assertion: the loop exits the moment the pair settles.
fn settle(state: &mut gungnir_app::state::AppState) {
    use gungnir_app::pointcloud::PointCloudStatus;
    for _ in 0..6_000 {
        gungnir_app::update::tick(state);
        if !matches!(
            state.point_cloud,
            PointCloudStatus::Loading { .. } | PointCloudStatus::NotConfigured
        ) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the pair never settled: {:?}", state.point_cloud);
}

fn fixture(name: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/pointcloud")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// The Autzen query box, the same one `gungnir-data`'s own tests read.
fn autzen_bounded(path: &str) -> gungnir_config::PointCloudFileConfig {
    gungnir_config::PointCloudFileConfig {
        path: path.to_string(),
        copc_bounds: Some([637_200.0, 851_100.0, 400.0, 637_300.0, 851_200.0, 620.0]),
    }
}

/// The deployment origin the Autzen tests place onto: the query box's centre, 100 m
/// above the ellipsoid.
fn autzen_origin() -> [f64; 3] {
    [
        44.056_081_952_041_154_f64.to_radians(),
        (-123.068_898_230_983_63_f64).to_radians(),
        100.0,
    ]
}

/// **GAP-197 (D-125) on the real capture, through a real tick, without the grid.**
/// NAVD88 takes GEOID18 and no other model, so a deployment that has not installed
/// GEOID18 refuses the pair, naming the grid it needs -- decided before PROJ runs, so it
/// holds in every build. (Before GAP-108 these NAVD88 heights were drawn as though
/// ellipsoidal, 23 m high; between GAP-108 and GAP-197 they were refused outright.)
#[test]
fn the_real_fixtures_navd88_heights_are_refused_without_geoid18_naming_it() {
    use gungnir_app::pointcloud::PointCloudStatus;
    use gungnir_config::PointCloudConfig;

    let autzen = fixture("autzen-classified.copc.laz");
    let (mut state, dir) = desktop(
        "navd88-no-grid",
        PointCloudConfig {
            source: autzen_bounded(&autzen),
            target: autzen_bounded(&autzen),
            frame: "epsg:2992".into(),
            vertical: None,
        },
        autzen_origin(),
    );
    settle(&mut state);
    let PointCloudStatus::Failed { reason, .. } = &state.point_cloud else {
        panic!("NAVD88 without GEOID18 is refused: {:?}", state.point_cloud);
    };
    assert!(reason.contains("us_noaa_g2018u0.tif"), "{reason}");
    assert!(reason.contains("NAVD88"), "{reason}");
    assert!(state.data.point_clouds.is_empty(), "nothing is drawn");
    let _ = std::fs::remove_dir_all(dir);
}

/// The committed GEOID18 clip, verified, as the start-up check would have installed it
/// (`testdata/geoid/SOURCE.md`).
#[cfg(feature = "crs")]
fn install_geoid18_clip(state: &mut gungnir_app::state::AppState) {
    let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/geoid/g2018u0_clip_43n45n_124w122w.tif");
    state.geoid.set(
        gungnir_data::geoid::GeoidModel::Geoid18Conus,
        gungnir_app::geoid::GeoidStatus::Verified {
            grid: gungnir_data::geoid::GeoidGrid::verify(
                &clip,
                "a93c8ca6a47cc3d5a58ffeeac469aa9c8c9ca2400a0113a1976eb0a4bc9b16eb",
            )
            .expect("the committed GEOID18 clip"),
            source: gungnir_app::geoid::GridSource::Baseline,
        },
    );
}

/// The absolute ENU of point `i` of the first loaded cloud: its `f64` corner plus its
/// `f32` offset.
#[cfg(feature = "crs")]
fn enu_of(state: &gungnir_app::state::AppState, i: usize) -> [f64; 3] {
    let cloud = &state.data.point_clouds[0];
    let p = cloud.positions[i];
    [
        f64::from(p[0]) + cloud.origin[0],
        f64::from(p[1]) + cloud.origin[1],
        f64::from(p[2]) + cloud.origin[2],
    ]
}

/// **GAP-197 (D-125): the real capture converts again, now through GEOID18, end to end
/// through a real tick** -- PROJ for the Lambert inverse, the grid for the NAVD88 height,
/// `gungnir_model::LocalFrame` for ENU. This is the test GAP-108 took away and the one
/// GAP-102 was built around.
///
/// Independently, with nothing of this workspace's: `pyproj` over the full pinned GEOID18
/// grid took `Transformer.from_crs(<the fixture's WKT>, "EPSG:4979")` for three of the
/// box's points, in the loader's own order, to ECEF, and a hand rotation into the tangent
/// plane at [`autzen_origin`] put them at the values below
/// (`gungnir-data/tests/pointcloud_crs.rs` has the geodetic ones). Dropping GEOID18 would
/// miss every up by 23.3 m; the tolerance is a tenth of a millimetre.
#[test]
#[cfg(feature = "crs")]
fn the_real_capture_lands_through_geoid18_where_an_independent_computation_puts_it() {
    use gungnir_app::pointcloud::PointCloudStatus;
    use gungnir_config::PointCloudConfig;

    let autzen = fixture("autzen-classified.copc.laz");
    let (mut state, dir) = desktop(
        "navd88-geoid18",
        PointCloudConfig {
            source: autzen_bounded(&autzen),
            target: autzen_bounded(&autzen),
            frame: "epsg:2992".into(),
            vertical: None,
        },
        autzen_origin(),
    );
    install_geoid18_clip(&mut state);
    settle(&mut state);
    assert!(
        matches!(state.point_cloud, PointCloudStatus::Loaded { .. }),
        "{:?}",
        state.point_cloud
    );
    assert_eq!(state.data.point_clouds[0].positions.len(), 4767);
    for (i, want) in [
        (
            0,
            [
                -15.632_399_400_289_087,
                14.322_704_328_325_154,
                5.001_896_268_387_805,
            ],
        ),
        (
            2383,
            [
                4.199_937_042_242_675,
                14.821_449_285_954_024,
                4.611_462_284_825_729,
            ],
        ),
        (
            4766,
            [
                -7.645_210_074_340_254,
                -9.606_629_005_367_767,
                4.642_099_538_056_842,
            ],
        ),
    ] {
        let got = enu_of(&state, i);
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-4,
                "point {i} axis {axis}: {} against {}",
                got[axis],
                want[axis]
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// **GAP-102 item (2) and GAP-197 through a real tick: a LAS 1.2 file whose geokeys
/// state NAVD88 in US survey feet** (`testdata/pointcloud/autzen-geokeys.las`, the key
/// directory GDAL writes for EPSG:2992+6360), placed through GEOID18. `pyproj` took
/// `EPSG:2992+6360` to `EPSG:4979` for its five points, then the same hand rotation at
/// [`autzen_origin`]; the first point is the box centre, 29.063 m up because the origin
/// sits 100 m above the ellipsoid and the point 129.063 m. The cloud spans 1.25 km, where
/// the loader's `f32` offsets in feet and the placed `f32` offsets in metres each carry
/// tens of micrometres, so the tolerance is two tenths of a millimetre.
#[test]
#[cfg(feature = "crs")]
fn a_geokey_file_in_navd88_feet_lands_through_geoid18_where_pyproj_puts_it() {
    use gungnir_app::pointcloud::PointCloudStatus;
    use gungnir_config::{PointCloudConfig, PointCloudFileConfig};

    let plain = |path: String| PointCloudFileConfig {
        path,
        copc_bounds: None,
    };
    let (mut state, dir) = desktop(
        "geokeys-geoid18",
        PointCloudConfig {
            source: plain(fixture("autzen-geokeys.las")),
            target: plain(fixture("autzen-geokeys.las")),
            frame: "epsg:2992".into(),
            vertical: None,
        },
        autzen_origin(),
    );
    install_geoid18_clip(&mut state);
    settle(&mut state);
    assert!(
        matches!(state.point_cloud, PointCloudStatus::Loaded { .. }),
        "{:?}",
        state.point_cloud
    );
    for (i, want) in [
        [0.0, 0.0, 29.062_630_753_229_662],
        [
            -487.936_104_413_653_54,
            -707.021_701_265_043_4,
            0.403_309_564_943_924_67,
        ],
        [
            511.640_178_016_742_8,
            744.316_031_152_460_3,
            64.123_472_020_214_25,
        ],
        [
            -14.759_457_977_780_002,
            -15.713_213_344_444_414,
            13.975_171_759_190_776,
        ],
        [
            220.437_830_523_481_95,
            266.170_492_937_138_7,
            4.742_427_778_033_175,
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let got = enu_of(&state, i);
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 2e-4,
                "point {} axis {axis}: {} against {}",
                i + 1,
                got[axis],
                want[axis]
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// **The composed pipeline, end to end, through the geoid**: `five-points.las` states no
/// CRS, the baseline says it is EPSG:32633 with EGM2008 heights, the committed clip of
/// the pinned grid is installed verified, and a real tick converts the pair through PROJ
/// (horizontal), the grid (vertical) and `gungnir_model::LocalFrame` (ENU).
///
/// Independently, with nothing of this workspace's: `pyproj` over the full pinned grid
/// takes point 1 (easting 500010, northing 6000020, 12.5 m EGM2008 height) to longitude
/// 15.00015310098077, latitude 54.14828385766182 and an ellipsoidal height of
/// 47.42449178478446 m, then to ECEF; a hand rotation into the tangent plane at the
/// origin below puts it at east 10.004075820331082, north 20.0081374609536, up
/// 47.42445256905888 m. Point 4 (500012, 6000022, 18.25) lands at east
/// 12.004901783352192, north 22.00897250261202, up 53.174404143393154. Dropping the
/// geoid misses the up by 34.9 m; the tolerance is a tenth of a millimetre.
#[test]
#[cfg(feature = "crs")]
fn a_pair_with_egm2008_heights_lands_where_an_independent_computation_puts_it() {
    use gungnir_app::pointcloud::PointCloudStatus;
    use gungnir_config::{PointCloudConfig, PointCloudFileConfig};

    let plain = |path: String| PointCloudFileConfig {
        path,
        copc_bounds: None,
    };
    let (mut state, dir) = desktop(
        "egm2008",
        PointCloudConfig {
            source: plain(fixture("five-points.las")),
            target: plain(fixture("five-points.las")),
            frame: "epsg:32633".into(),
            vertical: Some("epsg:3855".into()),
        },
        [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 0.0],
    );
    let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/geoid/egm08_25_clip_53n56n_13e17e.tif");
    state.geoid.set(
        gungnir_data::geoid::GeoidModel::Egm2008,
        gungnir_app::geoid::GeoidStatus::Verified {
            grid: gungnir_data::geoid::GeoidGrid::verify(
                &clip,
                "60a16af44ca47724fd6cbb58565104a010dd2ef8c2d5ec1c666552052fa83e10",
            )
            .expect("the committed clip, testdata/geoid/SOURCE.md"),
            source: gungnir_app::geoid::GridSource::Baseline,
        },
    );
    settle(&mut state);
    assert!(
        matches!(state.point_cloud, PointCloudStatus::Loaded { .. }),
        "{:?}",
        state.point_cloud
    );
    let cloud = &state.data.point_clouds[0];
    let enu = |i: usize| {
        let p = cloud.positions[i];
        [
            f64::from(p[0]) + cloud.origin[0],
            f64::from(p[1]) + cloud.origin[1],
            f64::from(p[2]) + cloud.origin[2],
        ]
    };
    for (i, want) in [
        (
            0,
            [
                10.004_075_820_331_082,
                20.008_137_460_953_6,
                47.424_452_569_058_88,
            ],
        ),
        (
            3,
            [
                12.004_901_783_352_192,
                22.008_972_502_612_02,
                53.174_404_143_393_154,
            ],
        ),
    ] {
        let got = enu(i);
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-4,
                "point {} axis {axis}: {} against {}",
                i + 1,
                got[axis],
                want[axis]
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
