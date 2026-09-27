// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain on the desktop (GAP-023): the configured DEM loads off the render thread,
//! line of sight is masked against it and the coverage parameters say so; a file whose
//! own frame contradicts the baseline is refused by name; a missing file fails loudly.
//!
//! `a_geotiff_that_declares_utm_is_refused_by_name...` below covers the default build
//! (no `crs` feature, D-41's own build-feasibility caveat --
//! `docs/agentic-coding-standards.md` §2.9): declaring `frame: "local-enu"` against a
//! file that states EPSG:32633 is still refused, exactly as before D-41.
//! `a_geotiff_that_declares_utm_converts...`, behind that feature, is the same fixture
//! with `frame: "epsg:32633"` instead, now converting and loading.
//!
//! GAP-108 (D-121, D-122): a converted DEM's heights are made WGS-84 ellipsoidal ones and
//! land at their local up. The refusals -- a height nobody states, a datum with no grid,
//! a baseline contradicting the file, an EGM2008 file with no verified grid -- are decided
//! before any projection runs and are checked in every build; the conversions through the
//! grid are behind the `crs` feature. The expected ups are an independent computation:
//! `pyproj` for the horizontal, the geoid and geodetic-to-ECEF, and a hand rotation into
//! the origin's tangent plane, nothing of `gungnir_coord`'s (recorded in
//! `docs/record/2026-09-26/`'s GAP-108 item).

use gungnir_app::state::AppState;
use gungnir_app::terrain::TerrainStatus;
use gungnir_app::{sustainment, update};
use gungnir_config::{ConfigBaseline, TerrainConfig};
use std::path::{Path, PathBuf};

/// `testdata/geoid/SOURCE.md` records this digest for the committed EGM2008 clip.
#[cfg(feature = "crs")]
const CLIP_SHA256: &str = "60a16af44ca47724fd6cbb58565104a010dd2ef8c2d5ec1c666552052fa83e10";

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/dem")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn desktop(name: &str, path: String) -> (AppState, PathBuf) {
    desktop_with_frame(name, path, "local-enu")
}

/// As `desktop`, but naming the terrain's declared frame (GAP-023, D-41) instead of
/// always `"local-enu"`.
fn desktop_with_frame(name: &str, path: String, frame: &str) -> (AppState, PathBuf) {
    desktop_at(name, path, frame, [0.959_931, 0.209_44, 0.0])
}

/// The `small.tif` fixture's own south-west corner as a WGS84 origin in radians.
///
/// The corner is EPSG:32633 easting 500 000, northing 6 000 000 (`testdata/dem/
/// SOURCE.md`). Longitude is **exactly** 15 deg E: 500 000 m is UTM's false easting, so
/// the corner lies on zone 33N's central meridian, where the transverse Mercator's
/// easting is zero by the projection's own left-right symmetry -- the same fact
/// `gungnir-data`'s `utm_33n_central_meridian_at_the_equator_is_15_east_0_north` makes
/// a test of. Latitude is the footpoint latitude of the meridional arc
/// 6 000 000 / 0.9996 = 6 002 401 m, which the standard rectifying-latitude series puts
/// at 54.148 104 104 deg N; derived from the series rather than read off PROJ, and
/// agreeing with PROJ's own inverse to within 0.2 micrometres of latitude.
///
/// Nothing here is fitted: the spacing assertion moves by 0.2 mm for a kilometre of
/// error in this anchor, against a tolerance of a millimetre.
#[cfg(feature = "crs")]
fn fixture_corner_origin() -> [f64; 3] {
    [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 0.0]
}

/// As `desktop_with_frame`, but placing the deployment's local ENU origin
/// (`[lat_rad, lon_rad, alt_m]`) rather than taking the default one.
fn desktop_at(name: &str, path: String, frame: &str, origin: [f64; 3]) -> (AppState, PathBuf) {
    desktop_vertical(name, path, frame, None, origin)
}

/// As `desktop_at`, also declaring `terrain.vertical` (GAP-108), and naming the scratch
/// directory as `geoid_grid_dir` -- which holds no grid, so every test starts from a
/// known "no grid" whatever `PROJ_DATA` says on the machine running it. A test that needs
/// a grid installs one it verified itself.
fn desktop_vertical(
    name: &str,
    path: String,
    frame: &str,
    vertical: Option<&str>,
    origin: [f64; 3],
) -> (AppState, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-terrain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(origin),
        terrain: Some(TerrainConfig {
            path,
            frame: frame.into(),
            vertical: vertical.map(str::to_string),
        }),
        geoid_grid_dir: Some(dir.join("no-grid-here").to_string_lossy().into_owned()),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

/// The failure reason, or a panic naming the state it was in instead.
fn refusal(state: &AppState) -> String {
    match &state.terrain {
        TerrainStatus::Failed { reason, .. } => reason.clone(),
        other => panic!("expected a refusal: {other:?}"),
    }
}

/// Tick until the terrain loader thread answers.
///
/// **A deadlock guard, not a performance assertion** (the reasoning is
/// `gungnir-tracking-service/tests/sample_set_replay.rs`'s): a real hang still fails,
/// and a loaded machine no longer does. The loop exits the moment the condition holds,
/// so a passing run costs what it always did. Every caller configures a terrain, so a
/// status still `NotConfigured` means the load has not started -- it waits for the
/// geoid grid check to settle (GAP-108) -- and is ticked through like `Loading`.
fn settle(state: &mut AppState) {
    for _ in 0..6_000 {
        update::tick(state);
        if !matches!(
            state.terrain,
            TerrainStatus::Loading { .. } | TerrainStatus::NotConfigured
        ) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the loader never answered: {:?}", state.terrain);
}

#[test]
fn an_ascii_grid_in_the_local_frame_loads_and_masks_line_of_sight() {
    let (mut state, dir) = desktop("asc", fixture("small.asc"));
    assert!(
        matches!(state.terrain, TerrainStatus::NotConfigured),
        "nothing until the first frame"
    );
    update::tick(&mut state);
    assert!(
        !matches!(state.terrain, TerrainStatus::NotConfigured),
        "the first frame starts the load: {:?}",
        state.terrain
    );
    settle(&mut state);
    assert!(
        matches!(
            state.terrain,
            TerrainStatus::Loaded {
                vertices: 20,
                triangles: 16,
                ..
            }
        ),
        "{:?}",
        state.terrain
    );
    assert_eq!(state.data.terrains.len(), 1);
    assert!(state.terrain.line().contains("masking line of sight"));
    // The placed surface sits at the grid's own coordinates, not at the loader's origin.
    let sw = state.data.terrains[0].positions[15];
    assert!((sw[0] - 500_015.0).abs() < 1.0, "{sw:?}");
    assert!(
        sustainment::coverage_report(&state).is_none(),
        "no approaches declared"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_geotiff_that_declares_utm_is_refused_by_name_and_line_of_sight_stays_flat() {
    let (mut state, dir) = desktop("utm", fixture("small.tif"));
    settle(&mut state);
    match &state.terrain {
        TerrainStatus::Failed { reason, .. } => {
            assert!(reason.contains("EPSG:32633"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert!(state.data.terrains.is_empty());
    assert!(!state.terrain.is_masking());
    assert!(
        state.alerts.iter().any(|a| a.contains("not placed")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-023, D-41: the same UTM fixture as
/// `a_geotiff_that_declares_utm_is_refused_by_name_and_line_of_sight_stays_flat`, but
/// with `terrain.frame` declaring the CRS the file actually states instead of
/// contradicting it, and the `crs` feature compiled in. Not run by default:
/// `proj-sys` links `libproj`, which needs `cmake`, `SQLite3` and a C/C++ toolchain to
/// build from source and cannot build at all on Windows MSVC
/// (`docs/agentic-coding-standards.md` §2.9's D-41 entry); `ci.yml`'s `proj-crs` job
/// installs those on a Linux runner and is what actually compiles and runs this,
/// alongside the point-cloud half of the same feature.
#[cfg(feature = "crs")]
#[test]
fn a_geotiff_that_declares_utm_converts_and_masks_line_of_sight_when_the_frame_matches() {
    // The origin is put beside the fixture rather than at `desktop`'s default one, and
    // that is load-bearing rather than cosmetic. `small.tif` spans easting
    // 500000..500150 -- so it straddles zone 33N's central meridian, 15 deg E, 500000
    // being the false easting -- and northing 6000000..6000120, which the meridian-arc
    // series puts at 54.1481 deg N (the footpoint latitude of 6000000 / 0.9996 m).
    //
    // `desktop`'s default origin is 55 deg N, 12 deg E, which is 216 km away, and at
    // 216 km three things that have nothing to do with the reprojection each swamp the
    // 12 mm the reprojection itself contributes (see the spacing assertion below):
    // a `TerrainMesh` position's `f32` step is 15.6 mm out there; the pair's local
    // horizontal is tilted 0.0339 rad out of the origin's tangent plane, shortening
    // their separation by 13.5 mm when it is projected onto it; and because
    // `to_local_enu` correctly feeds each cell's own height as that point's altitude,
    // the 1 m height difference between these two cells contributes 1 m * sin(0.0339)
    // = 30.1 mm of purely horizontal ENU offset. A quantity cannot be checked to a
    // millimetre through 15.6 mm of quantisation, so the assertion below would be
    // measuring the anchoring, not the conversion.
    //
    // A DEM tile 216 km from its own deployment's origin is also not what this feature
    // is for: `TerrainMesh` keeps an `f64` origin and small `f32` offsets precisely so
    // that a DEM near the origin stays exact.
    // GAP-108: a real-world DEM's heights are converted only from a stated datum;
    // `small.tif` states none, so the baseline says they are ellipsoidal.
    let (mut state, dir) = desktop_vertical(
        "utm-converted",
        fixture("small.tif"),
        "epsg:32633",
        Some("ellipsoidal"),
        fixture_corner_origin(),
    );
    settle(&mut state);
    match &state.terrain {
        TerrainStatus::Loaded {
            vertices,
            triangles,
            ..
        } => {
            // Same underlying 5 by 4 grid, one no-data cell, as
            // `an_ascii_grid_in_the_local_frame_loads_and_masks_line_of_sight`'s ASCII
            // fixture (`testdata/dem/SOURCE.md`: both encode the identical grid).
            // Reprojection moves where a vertex sits, never which ones exist or how
            // they triangulate, so the topology should be unchanged.
            assert_eq!(*vertices, 20, "{:?}", state.terrain);
            assert_eq!(*triangles, 16, "{:?}", state.terrain);
        }
        other => panic!("expected the UTM fixture to convert and load: {other:?}"),
    }
    assert!(state.terrain.line().contains("masking line of sight"));
    assert_eq!(state.data.terrains.len(), 1);
    let mesh = &state.data.terrains[0];
    assert!(
        mesh.positions
            .iter()
            .all(|p| p[0].is_finite() && p[1].is_finite()),
        "every converted vertex's [e, n] must be finite: {:?}",
        mesh.positions
    );
    // The one no-data cell's height stays NaN through the conversion (`TerrainMesh::
    // with_enu` keeps a hole a hole); every other vertex has a real height.
    assert_eq!(
        mesh.positions.iter().filter(|p| p[2].is_nan()).count(),
        1,
        "{:?}",
        mesh.positions
    );
    // `positions` is row-major with the north row first (`TerrainMesh::from_grid`;
    // `gungnir-data/tests/dem.rs` pins `positions[15] == [15.0, 15.0, 40.0]`, which is
    // what fixes that layout), so [0] and [1] are the north row's first two cell
    // centres: easting 500015 and 500045, northing 6000105, heights 10 m and 11 m.
    //
    // **Thirty metres of grid is not thirty metres of ground, and the difference is the
    // whole point of this assertion.** UTM shrinks its grid deliberately: the scale
    // factor on the central meridian is k0 = 0.9996 exactly, and both cells sit 15 m
    // and 45 m from zone 33N's central meridian, where the point scale factor departs
    // from k0 by parts in 10^12 -- under a nanometre over 30 m. So the ground distance
    // is 30 / 0.9996 = 30.012 004 802 m, and that is what a correct conversion into a
    // local ENU frame anchored beside the fixture has to produce.
    //
    // Two residuals sit inside the tolerance, both derived rather than fitted:
    //   +0.054 mm   each cell's own height is fed to the conversion as that point's
    //               altitude (`terrain::to_local_enu` documents why), so the 10 m and
    //               11 m heights lift the two cells radially away from an origin ~110 m
    //               off, which lengthens their east separation by that much.
    //   +0.0004 mm  `TerrainMesh::with_enu` returning the result to `f32`, whose step is
    //               about 1e-6 m at these ranges.
    // One millimetre of tolerance is eighteen times their sum.
    //
    // **This assertion previously read `30.0 +/- 0.01`, and that was satisfied by
    // exactly the bug it named itself a tripwire for**: a pipeline that dropped k0
    // lands on 30.000, inside the old band, while a correct one lands on 30.012,
    // outside it. No correct implementation could ever have passed. The band is now ten
    // times tighter and centred on the derived value, so dropping k0 misses by 12 mm --
    // twelve times the tolerance -- a swapped easting/northing reads about 13.3 m, and
    // degrees fed in as radians reads about 1894 m.
    let adjacent_spacing_m = {
        let a = mesh.positions[0];
        let b = mesh.positions[1];
        (f64::from(a[0] - b[0]).powi(2) + f64::from(a[1] - b[1]).powi(2)).sqrt()
    };
    let expected_m = 30.0 / 0.9996;
    assert!(
        (adjacent_spacing_m - expected_m).abs() < 0.001,
        "adjacent cell spacing: {adjacent_spacing_m} m, expected {expected_m} m \
         (30 m of UTM grid with k0 = 0.9996 undone)"
    );
    // GAP-108 (D-122): the height is the vertex's local up now, not the file's number.
    // Vertex 0 is 10 m high and 106 m from the origin, so the Earth curves 0.88 mm away
    // beneath it: `pyproj` plus a hand rotation put its up at 9.999117320760021 m. The
    // old behaviour -- the file's 10.0 carried through -- misses by nine times the
    // tolerance.
    let up = f64::from(mesh.positions[0][2]);
    assert!(
        (up - 9.999_117_320_760_021).abs() < 1e-4,
        "vertex 0's up: {up} m"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-108 (D-121): `small.tif` states no vertical datum, and a baseline declaring none
/// for it is refused by name -- before GAP-108 its heights were used as they stood.
#[test]
fn a_real_world_dem_whose_heights_nobody_states_is_refused_by_name() {
    let (mut state, dir) = desktop_vertical(
        "unstated-vertical",
        fixture("small.tif"),
        "epsg:32633",
        None,
        [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 0.0],
    );
    settle(&mut state);
    let reason = refusal(&state);
    assert!(reason.contains("terrain.vertical"), "{reason}");
    assert!(!state.terrain.is_masking());
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-108: a vertical datum this deployment carries no grid for is refused by name,
/// whichever side states it -- here the baseline says NAVD88 (EPSG:5703).
#[test]
fn a_dem_in_a_vertical_datum_with_no_grid_is_refused_by_name() {
    let (mut state, dir) = desktop_vertical(
        "navd88",
        fixture("small.tif"),
        "epsg:32633",
        Some("epsg:5703"),
        [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 0.0],
    );
    settle(&mut state);
    let reason = refusal(&state);
    assert!(reason.contains("EPSG:5703"), "{reason}");
    assert!(reason.contains("no geoid grid"), "{reason}");
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-108: `small-egm2008.tif` states EGM2008 heights itself (`VerticalGeoKey` 3855,
/// written by GDAL), and a baseline claiming they are ellipsoidal contradicts the file,
/// so it is refused rather than obeyed.
#[test]
fn a_baseline_that_contradicts_the_dems_own_vertical_system_is_refused() {
    let (mut state, dir) = desktop_vertical(
        "contradiction",
        fixture("small-egm2008.tif"),
        "epsg:32633",
        Some("ellipsoidal"),
        [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 0.0],
    );
    settle(&mut state);
    let reason = refusal(&state);
    assert!(reason.contains("EGM2008"), "{reason}");
    assert!(reason.contains("the file states"), "{reason}");
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-108: a DEM stating EGM2008 heights where the named grid directory holds no grid
/// is refused, naming the grid; PN-09's geoid line says the same.
#[test]
fn an_egm2008_dem_without_the_grid_is_refused_and_pn_09_says_why() {
    let (mut state, dir) = desktop_vertical(
        "no-grid",
        fixture("small-egm2008.tif"),
        "epsg:32633",
        None,
        [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 0.0],
    );
    settle(&mut state);
    let reason = refusal(&state);
    assert!(reason.contains("us_nga_egm08_25.tif"), "{reason}");
    assert!(reason.contains("is not there"), "{reason}");
    assert!(!state.geoid.is_verified());
    let line = state.geoid.line();
    assert!(line.contains("refused"), "{line}");
    assert!(line.contains("the baseline's geoid_grid_dir"), "{line}");
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("EGM2008 geoid grid")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **GAP-108 end to end through the grid**: `small-egm2008.tif` states EGM2008 heights,
/// the committed clip of the pinned grid is installed verified, and the DEM lands at the
/// up its ellipsoidal height gives. The origin is at the fixture's corner, 30 m up.
///
/// Independently: `pyproj` over the full pinned grid takes vertex 0 (easting 500015,
/// northing 6000105, 10 m EGM2008 height) to an ellipsoidal height of 44.92379767439437 m
/// (the undulation there is 34.924 m), and to ECEF, and a hand rotation into the
/// origin's tangent plane puts it 44.922914990572664 m up from an origin at 0 m -- so
/// 14.922914990572664 m from one 30 m up, the origin's altitude coming straight off `u`.
/// Dropping the geoid misses by 35 m, and the old file-height-as-up by 5 m.
#[test]
#[cfg(feature = "crs")]
fn an_egm2008_dem_lands_at_the_up_its_ellipsoidal_height_gives() {
    let (mut state, dir) = desktop_vertical(
        "egm2008-clip",
        fixture("small-egm2008.tif"),
        "epsg:32633",
        None,
        [54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 30.0],
    );
    let clip = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/geoid/egm08_25_clip_53n56n_13e17e.tif");
    state.geoid = gungnir_app::geoid::GeoidStatus::Verified {
        grid: gungnir_data::geoid::GeoidGrid::verify(&clip, CLIP_SHA256).expect("the clip"),
        source: gungnir_app::geoid::GridSource::Baseline,
    };
    settle(&mut state);
    assert!(state.terrain.is_masking(), "{:?}", state.terrain);
    let mesh = &state.data.terrains[0];
    let up = f64::from(mesh.positions[0][2]);
    assert!(
        (up - 14.922_914_990_572_664).abs() < 1e-4,
        "vertex 0's up: {up} m"
    );
    assert_eq!(
        mesh.positions.iter().filter(|p| p[2].is_nan()).count(),
        1,
        "the no-data cell is still a hole"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The same, through the start-up check against the **full pinned grid**
/// (`GUNGNIR_EGM2008_GRID_DIR`, which `ci.yml`'s `proj-crs` job fetches and checks): the
/// baseline names the directory, the desktop hashes the 80 MB file off the render thread,
/// PN-09 says it is verified, and the DEM lands at the same up as through the clip.
#[test]
#[ignore = "needs the pinned 80 MB grid; ci.yml's proj-crs job fetches it and runs this"]
#[cfg(feature = "crs")]
fn the_pinned_grid_is_verified_at_start_and_places_an_egm2008_dem() {
    let grid_dir = std::env::var("GUNGNIR_EGM2008_GRID_DIR")
        .expect("GUNGNIR_EGM2008_GRID_DIR names the directory holding the pinned grid");
    let dir = std::env::temp_dir().join(format!("gungnir-terrain-pinned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([54.148_104_104_f64.to_radians(), 15.0_f64.to_radians(), 30.0]),
        terrain: Some(TerrainConfig {
            path: fixture("small-egm2008.tif"),
            frame: "epsg:32633".into(),
            vertical: None,
        }),
        geoid_grid_dir: Some(grid_dir),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    settle(&mut state);
    assert!(state.geoid.is_verified(), "{}", state.geoid.line());
    assert!(
        state.geoid.line().contains("verified"),
        "{}",
        state.geoid.line()
    );
    assert!(state.terrain.is_masking(), "{:?}", state.terrain);
    let up = f64::from(state.data.terrains[0].positions[0][2]);
    assert!(
        (up - 14.922_914_990_572_664).abs() < 1e-4,
        "vertex 0's up: {up} m"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_file_fails_loudly() {
    let (mut state, dir) = desktop("missing", fixture("absent.asc"));
    settle(&mut state);
    assert!(
        matches!(state.terrain, TerrainStatus::Failed { .. }),
        "{:?}",
        state.terrain
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("not loaded")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}
