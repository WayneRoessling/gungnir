// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The dead-spot layer on the desktop (GAP-246, `docs/design/DN-34-sensor-dead-spots.md`
//! §11, the `gungnir-app` layer row, drafted there and not yet agreed in
//! `docs/verification-capability-table.md`).
//!
//! A DEM holding one ridge, loaded through the real loader and placed in the local frame;
//! one radar on the ground west of it, switched to search. The field is computed off the
//! tick; the dark cells are behind the ridge and nowhere else; the viewport says the
//! height, the age and that it is geometric line of sight, and holds a texture; PN-11
//! says the measure; a hidden layer is named; a change of inputs draws the old field as
//! recomputing; dropping the radar to standby takes its shadow with its ring; and with no
//! DEM the map says the dead spots are unknown and draws nothing dark.

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use gungnir_analytics::CellSight;
use gungnir_app::state::AppState;
use gungnir_app::terrain::TerrainStatus;
use gungnir_app::{dead_spots, sustainment, update};
use gungnir_config::{ConfigBaseline, SensorConfig, TerrainConfig};
use gungnir_model::SensorMode;
use gungnir_ui::harness::RenderProbe;
use gungnir_viewport3d::layers::{DeadSpotLayer, LayerInputs};

const ORIGIN: [f64; 3] = [0.959_931, 0.209_44, 0.0];
const CELL: f64 = 30.0;
/// The ridge's west face, east of the radar, metres.
const RIDGE_EAST: (f64, f64) = (990.0, 1_050.0);
const RIDGE_HEIGHT: f64 = 200.0;
const RADAR_UP: f64 = 20.0;

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("gungnir-dead-spots-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A 6 km square ESRI ASCII DEM in the local frame, centred on the origin: flat at 0 m
/// but for a ridge running north and south across the whole of it.
fn ridge_dem(dir: &std::path::Path) -> String {
    let n = 200u32;
    let path = dir.join("ridge.asc");
    let mut f = std::fs::File::create(&path).expect("writable");
    writeln!(f, "ncols {n}\nnrows {n}\nxllcorner -3000\nyllcorner -3000\ncellsize {CELL}\nNODATA_value -9999")
        .expect("header");
    for _row in 0..n {
        let line: Vec<String> = (0..n)
            .map(|c| {
                let east = -3000.0 + (f64::from(c) + 0.5) * CELL;
                if (RIDGE_EAST.0..=RIDGE_EAST.1).contains(&east) {
                    format!("{RIDGE_HEIGHT}")
                } else {
                    "0".to_owned()
                }
            })
            .collect();
        writeln!(f, "{}", line.join(" ")).expect("a row");
    }
    path.to_string_lossy().into_owned()
}

fn radar() -> SensorConfig {
    SensorConfig {
        id: 1,
        modality: "radar".into(),
        position: [ORIGIN[0], ORIGIN[1], RADAR_UP],
        max_range_m: 2_500.0,
        control_endpoint: None,
        maintenance: Vec::new(),
        detection_model: None,
        azimuth_sector: None,
        elevation_band: None,
    }
}

fn desktop(name: &str, terrain: bool) -> (AppState, PathBuf) {
    let dir = scratch(name);
    let terrain = terrain.then(|| TerrainConfig {
        path: ridge_dem(&dir),
        frame: "local-enu".into(),
        vertical: None,
        nad83_realisation: None,
        vertical_sigma_m: None,
    });
    // An approach due east at 100 m, across the ridge.
    let frame = gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
        lat_rad: ORIGIN[0],
        lon_rad: ORIGIN[1],
        alt_m: ORIGIN[2],
    });
    let at = |e: f64| {
        let g = frame.to_geodetic([e, 0.0, 100.0]);
        [g.lat_rad, g.lon_rad, g.alt_m]
    };
    let config = ConfigBaseline {
        data_dir: dir.join("data").to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        sensors: vec![radar()],
        approaches: vec![gungnir_config::ApproachConfig {
            name: "eastern valley".into(),
            points: vec![at(-2_000.0), at(2_400.0)],
            corridor_half_width_m: None,
        }],
        terrain,
        geoid_grid_dir: Some(dir.join("no-grid-here").to_string_lossy().into_owned()),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

/// Tick until `done` holds. **A deadlock guard, not a performance assertion**: it exits the
/// moment the condition holds.
fn settle(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
    for _ in 0..6_000 {
        update::tick(state);
        if done(state) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{what} never happened: {}", dead_spots::health_line(state));
}

/// The viewport, drawn once with this state's layers, and what it said.
fn draw_viewport(state: &mut AppState) -> gungnir_ui::harness::DrawnFrame {
    let probe = RenderProbe::new();
    let shown = dead_spots::shown_label(state);
    let (_, frame) = probe.draw(|ui| {
        let layer = dead_spots::layer(&state.dead_spots, &shown);
        gungnir_viewport3d::render(
            ui,
            &state.palette,
            &mut state.viewport,
            &[],
            &[],
            &gungnir_model::PlanView::default(),
            LayerInputs {
                coverage: gungnir_viewport3d::layers::CoverageLayer::Circles {
                    circles: &[],
                    nominal: None,
                },
                gaps: &[],
                hazards: &[],
                geofences: &[],
                predictions: &[],
                terrain: None,
                point_clouds: &[],
                laydown_preview: None,
                dense_group: None,
                dead_spots: layer,
            },
        );
    });
    frame
}

#[test]
#[allow(clippy::too_many_lines)]
fn the_ridge_casts_the_dead_spots_and_the_map_says_so() {
    let (mut state, dir) = desktop("ridge", true);
    settle(&mut state, "the DEM loading", |s| {
        !matches!(
            s.terrain,
            TerrainStatus::Loading { .. } | TerrainStatus::NotConfigured
        )
    });
    assert!(state.terrain.is_masking(), "{:?}", state.terrain);
    let field = state
        .height_field
        .as_deref()
        .expect("resampled with the load");
    assert!(
        (field.cell_m - CELL).abs() < 0.01,
        "the DEM's own cell: {}",
        field.cell_m
    );

    // A standby radar claims nothing, so the first field is clear.
    settle(&mut state, "the first field", |s| {
        s.dead_spots.computed.is_some()
    });
    assert!(state
        .dead_spots
        .shown_cells()
        .iter()
        .all(|c| *c == gungnir_viewport3d::layers::DeadSpotCell::Clear));

    sustainment::record_observed_mode(&mut state, 1, SensorMode::Search).expect("searching");
    // Off the tick: the first tick after the change hands the work to the worker and draws
    // the old field as recomputing.
    update::tick(&mut state);
    assert!(state.dead_spots.computing() || state.dead_spots.stale);
    let frame = draw_viewport(&mut state);
    assert!(frame.says("recomputing"), "{}", frame.joined());

    settle(&mut state, "the searching radar's field", |s| {
        s.dead_spots
            .computed
            .as_ref()
            .is_some_and(|c| !c.inputs.sensors.is_empty())
            && !s.dead_spots.stale
    });
    let computed = state.dead_spots.computed.as_ref().expect("a field");
    let field = &computed.field;
    assert!(
        (field.parameters.height_agl_m - 100.0).abs() < f64::EPSILON,
        "D-147's default"
    );
    let (mut behind, mut before) = (0usize, 0usize);
    for (i, cell) in field.cells.iter().enumerate() {
        let [e, n] = field.centre(i);
        let range = (e * e + n * n).sqrt();
        match cell {
            CellSight::Shadow { .. } => {
                assert!(
                    e > RIDGE_EAST.0 - CELL,
                    "a dead spot at {e:.0}, {n:.0} is not behind the ridge"
                );
                behind += 1;
            }
            CellSight::Single { .. } if e < RIDGE_EAST.0 - CELL && range < 2_400.0 => before += 1,
            CellSight::Unclaimed | CellSight::Single { .. } | CellSight::NoData => {}
            CellSight::Fused { .. } => panic!("one radar cannot fuse a cell: {cell:?}"),
        }
        if e < RIDGE_EAST.0 - 2.0 * CELL && range < 2_400.0 {
            assert!(
                matches!(cell, CellSight::Single { .. }),
                "{e:.0}, {n:.0} is in plain sight of the radar: {cell:?}"
            );
        }
    }
    assert!(
        behind > 500,
        "{behind} dead-spot cells behind a 200 m ridge"
    );
    assert!(before > 5_000, "{before} watched cells before it");

    // The map: the field's line, and a texture built once for it.
    let frame = draw_viewport(&mut state);
    for want in [
        "Terrain dead spots at 100 m above ground",
        "geometric line of sight",
        "s ago",
    ] {
        assert!(frame.says(want), "{want}: {}", frame.joined());
    }
    assert!(!frame.says("recomputing"), "{}", frame.joined());
    assert!(
        state.viewport.dead_spot_texture.is_some(),
        "the field's texture"
    );

    // PN-11: the measure, with the dark area in it.
    let probe = RenderProbe::new();
    let (_, panel) = probe.draw(|ui| {
        gungnir_app::workspace::render_coverage_layers(
            ui,
            &state,
            &mut gungnir_ui::panels::coverage_layers::AcceptanceDraft::default(),
        )
    });
    assert!(
        panel.says("dead spots at 100 m above ground"),
        "{}",
        panel.joined()
    );
    assert!(panel.says("km\u{b2} of"), "{}", panel.joined());
    assert!(panel.says("geometric line of sight"), "{}", panel.joined());

    // GAP-247 end to end: an approach flown at 100 m across the ridge is masked by the
    // resampled surface -- watched by the one radar up to the ridge, uncovered behind it --
    // and asked for twice it is the same report, from the cache.
    let report = sustainment::coverage_report(&state).expect("an origin and an approach");
    assert!(report.parameters.terrain_masking_applied);
    let behind_ridge = report
        .gaps
        .iter()
        .find(|g| g.severity == gungnir_model::GapSeverity::Uncovered)
        .expect("the approach is hidden behind the ridge");
    let first = behind_ridge.samples.first().expect("a sample");
    assert!(
        first[0] > RIDGE_EAST.0 - 250.0 && first[0] < RIDGE_EAST.1 + 250.0,
        "the hidden stretch begins at the ridge: {first:?}"
    );
    assert!(report
        .gaps
        .iter()
        .any(|g| g.severity == gungnir_model::GapSeverity::SingleSensor
            && g.samples.iter().all(|s| s[0] < RIDGE_EAST.0)));
    assert_eq!(sustainment::coverage_report(&state), Some(report.clone()));

    // One sensor's own shadow, chosen on PN-11: the radar's dead spots are the field's,
    // since it is the only one, and the line names it.
    let all = state.dead_spots.shown_cells().to_vec();
    dead_spots::apply(
        &mut state,
        gungnir_ui::panels::coverage_layers::LayerAction::DeadSpotsFor(Some(1)),
    );
    let mine = state.dead_spots.shown_cells().to_vec();
    assert_eq!(mine.len(), all.len());
    for (a, m) in all.iter().zip(&mine) {
        let shadow = gungnir_viewport3d::layers::DeadSpotCell::Shadow;
        assert_eq!(
            *a == shadow,
            *m == shadow,
            "one radar's shadow is the field's"
        );
    }
    let frame = draw_viewport(&mut state);
    assert!(frame.says("for S1 radar alone"), "{}", frame.joined());
    dead_spots::apply(
        &mut state,
        gungnir_ui::panels::coverage_layers::LayerAction::DeadSpotsFor(None),
    );

    // Hidden: named, and the field's line goes with it.
    state.viewport.layers.dead_spots = false;
    let frame = draw_viewport(&mut state);
    assert!(
        frame.says("layers hidden: dead spots"),
        "{}",
        frame.joined()
    );
    assert!(!frame.says("Terrain dead spots at"), "{}", frame.joined());
    state.viewport.layers.dead_spots = true;

    // To standby: the radar's ring goes, and its shadow with it.
    sustainment::record_observed_mode(&mut state, 1, SensorMode::Standby).expect("standby");
    settle(&mut state, "the standby field", |s| {
        s.dead_spots
            .computed
            .as_ref()
            .is_some_and(|c| c.inputs.sensors.is_empty())
            && !s.dead_spots.stale
    });
    assert!(
        state
            .dead_spots
            .shown_cells()
            .iter()
            .all(|c| *c == gungnir_viewport3d::layers::DeadSpotCell::Clear),
        "a standby radar left a shadow behind"
    );
    // And the cached report is not served for inputs it was not computed under.
    let standby = sustainment::coverage_report(&state).expect("still an approach");
    assert_ne!(standby, report);
    assert!(
        standby.gap_length_m(gungnir_model::GapSeverity::SingleSensor) < f64::EPSILON,
        "a standby radar watches nothing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn with_no_dem_the_dead_spots_are_unknown_and_nothing_is_dark() {
    let (mut state, dir) = desktop("no-dem", false);
    sustainment::record_observed_mode(&mut state, 1, SensorMode::Search).expect("searching");
    update::tick(&mut state);
    let reason = state.dead_spots.not_computed.clone().expect("not computed");
    assert!(reason.contains("no DEM is configured"), "{reason}");
    assert!(state.dead_spots.computed.is_none());
    assert!(matches!(
        dead_spots::layer(&state.dead_spots, ""),
        DeadSpotLayer::NotComputed { .. }
    ));
    let frame = draw_viewport(&mut state);
    assert!(
        frame.says("Terrain dead spots not computed"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("unknown"), "{}", frame.joined());
    assert!(
        state.viewport.dead_spot_texture.is_none(),
        "nothing dark is drawn"
    );
    assert!(dead_spots::health_line(&state).contains("not computed"));
    let _ = std::fs::remove_dir_all(dir);
}
