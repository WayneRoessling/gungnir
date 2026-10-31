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
    desktop_with(name, terrain, Vec::new())
}

/// The radar searching where `position_enu` puts it, as a laydown places it.
fn placed(id: &str, intent: &str, current: bool, position_enu: [f64; 3]) -> gungnir_model::Laydown {
    gungnir_model::Laydown {
        id: gungnir_model::LaydownId(id.into()),
        intent: intent.into(),
        sensors: vec![gungnir_model::laydown::SensorPlacement {
            sensor: gungnir_model::SensorId(1),
            position_enu,
            mode: SensorMode::Search,
            azimuth_sector: None,
            elevation_band: None,
        }],
        resources: Vec::new(),
        current,
    }
}

fn desktop_with(
    name: &str,
    terrain: bool,
    laydowns: Vec<gungnir_model::Laydown>,
) -> (AppState, PathBuf) {
    let dir = scratch(name);
    let terrain = terrain.then(|| TerrainConfig {
        path: Some(ridge_dem(&dir)),
        store: None,
        manifest_sha256: None,
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
        laydowns,
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

/// GAP-248 (DN-34 §7 item 5): a laydown option's dead spots are computed when it is
/// selected on PN-16, from its own placements and not the live registry, one option at a
/// time. The radar west of the ridge leaves the valley behind it dark; the same radar on
/// the ridge top sees over it. Unselecting drops the field; with no DEM a selected option
/// says why it has none.
#[test]
fn a_selected_options_dead_spots_come_from_its_own_placements() {
    use gungnir_ui::panels::planning::RowDeadSpots;
    let laydowns = vec![
        placed(
            "west",
            "the radar where it stands",
            true,
            [0.0, 0.0, RADAR_UP],
        ),
        placed(
            "ridge",
            "the radar on the ridge top",
            false,
            [1_020.0, 0.0, RIDGE_HEIGHT + RADAR_UP],
        ),
    ];
    let (mut state, dir) = desktop_with("options", true, laydowns.clone());
    settle(&mut state, "the DEM loading", |s| {
        !matches!(
            s.terrain,
            TerrainStatus::Loading { .. } | TerrainStatus::NotConfigured
        )
    });
    let row = |state: &AppState, id: &str| {
        sustainment::row_dead_spots(state, &gungnir_model::LaydownId(id.into()))
    };
    assert_eq!(row(&state, "west"), RowDeadSpots::NotSelected);
    assert_eq!(row(&state, "ridge"), RowDeadSpots::NotSelected);

    let settled = |state: &mut AppState, id: &str| -> f64 {
        settle(state, &format!("option {id}'s field"), |s| {
            matches!(row(s, id), RowDeadSpots::Computed { stale: false, .. })
        });
        match row(state, id) {
            RowDeadSpots::Computed {
                shadow_km2,
                claimed_km2,
                height_agl_m,
                ..
            } => {
                assert!((height_agl_m - 100.0).abs() < f64::EPSILON);
                assert!(claimed_km2 > 10.0, "{id}: {claimed_km2} km² claimed");
                shadow_km2
            }
            other => panic!("{id}: {other:?}"),
        }
    };

    state.select_laydown(gungnir_model::LaydownId("ridge".into()));
    let on_the_ridge = settled(&mut state, "ridge");
    assert_eq!(
        row(&state, "west"),
        RowDeadSpots::NotSelected,
        "one option at a time"
    );
    state.select_laydown(gungnir_model::LaydownId("west".into()));
    let behind_it = settled(&mut state, "west");
    assert_eq!(row(&state, "ridge"), RowDeadSpots::NotSelected);
    assert!(
        behind_it > 1.0 && behind_it > on_the_ridge + 1.0,
        "behind the ridge {behind_it} km², on it {on_the_ridge} km²"
    );
    // From the option's placements: the live radar is at standby, so the live field is empty.
    assert!(state
        .dead_spots
        .computed
        .as_ref()
        .is_some_and(|c| c.inputs.sensors.is_empty()));

    // Selecting the selected option again clears it, and its field with it.
    state.select_laydown(gungnir_model::LaydownId("west".into()));
    update::tick(&mut state);
    assert_eq!(row(&state, "west"), RowDeadSpots::NotSelected);
    assert!(state.dead_spots.option.laydown.is_none());
    let _ = std::fs::remove_dir_all(dir);

    let (mut flat, dir) = desktop_with("options-no-dem", false, laydowns);
    flat.select_laydown(gungnir_model::LaydownId("ridge".into()));
    update::tick(&mut flat);
    match row(&flat, "ridge") {
        RowDeadSpots::NotComputed { reason } => {
            assert!(reason.contains("no DEM is configured"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
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

/// GAP-249 (DN-34 amendment 4, D-149 to D-152): a sensor change tried against the dead
/// spots from PN-11. Nobody signed in is refused and nothing is recorded. Signed in, the
/// standby radar switched on where it stands leaves the valley behind the ridge newly dark
/// and clears nothing; the live radar with its mast raised over the ridge clears that
/// valley; each run is recorded once, with who ran it, even when a live change recomputes
/// it; the map draws it as a what-if over the live dead spots; and clearing it puts the
/// live field back with nothing more recorded.
#[test]
#[allow(clippy::too_many_lines)]
fn a_what_if_shows_what_a_sensor_change_would_clear_and_is_recorded() {
    use gungnir_security::{
        hash_passphrase, Account, InMemoryAccountStore, LocalAccountAuthority, OperatorId, Role,
    };
    use gungnir_ui::panels::coverage_layers::{LayerAction, WhatIfDraft};
    use gungnir_viewport3d::layers::DeadSpotCell;

    const MANAGER: OperatorId = OperatorId(7);
    const PASSPHRASE: &str = "correct horse battery staple";
    let (mut state, dir) = desktop("what-if", true);
    state.set_session_authority(Box::new(LocalAccountAuthority::new(Box::new(
        InMemoryAccountStore::new(vec![Account {
            operator: MANAGER,
            role: Role::SensorManager,
            phc: hash_passphrase(PASSPHRASE).expect("hashed"),
        }]),
    ))));
    let events = state.events.subscribe();
    let what_ifs = || -> Vec<gungnir_model::DeadSpotWhatIf> {
        events
            .try_iter()
            .filter_map(|env| match env.event {
                gungnir_eventing::Event::Planning(
                    gungnir_model::events::PlanningEvent::DeadSpotWhatIf(w),
                ) => Some(w),
                _ => None,
            })
            .collect()
    };
    settle(&mut state, "the DEM loading", |s| {
        !matches!(
            s.terrain,
            TerrainStatus::Loading { .. } | TerrainStatus::NotConfigured
        )
    });
    let set = |state: &mut AppState, draft: WhatIfDraft| {
        dead_spots::apply(state, LayerAction::EditWhatIf(draft));
        dead_spots::apply(state, LayerAction::RunWhatIf);
    };
    let answered = |state: &mut AppState, what: &str| {
        settle(state, what, |s| {
            s.dead_spots.what_if.answer.is_some() && !s.dead_spots.what_if.stale
        });
    };

    // Nobody signed in: refused in words, nothing run, nothing recorded.
    set(
        &mut state,
        WhatIfDraft {
            sensor: Some(1),
            ..WhatIfDraft::default()
        },
    );
    let refused = state.dead_spots.what_if.refused.clone().expect("refused");
    assert!(refused.contains("nobody is signed in"), "{refused}");
    assert!(state.dead_spots.what_if.describes().is_none());
    update::tick(&mut state);
    assert!(what_ifs().is_empty(), "a refusal was recorded");

    // Signed in: the standby radar, switched on where it stands.
    state
        .sign_in(&LocalAccountAuthority::credential(MANAGER, PASSPHRASE))
        .expect("signed in");
    set(
        &mut state,
        WhatIfDraft {
            sensor: Some(1),
            ..WhatIfDraft::default()
        },
    );
    assert!(state.dead_spots.what_if.refused.is_none());
    answered(&mut state, "the switched-on what-if");
    let answer = state.dead_spots.what_if.answer.as_ref().expect("an answer");
    assert!(
        answer.comparison.darkened_m2 > 1.0e6,
        "switching the radar on leaves the valley behind the ridge dark: {} m²",
        answer.comparison.darkened_m2
    );
    assert!(
        answer.comparison.cleared_m2 < f64::EPSILON,
        "nothing was dark to clear"
    );
    assert!(answer.comparison.watched_after_m2 > 5.0e6);
    assert!(answer.codes.contains(&DeadSpotCell::WouldDarken));
    // From the live picture's own sensors: the live field is still the standby one's.
    assert!(state
        .dead_spots
        .computed
        .as_ref()
        .is_some_and(|c| c.inputs.sensors.is_empty()));
    let recorded = what_ifs();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let first = &recorded[0];
    assert_eq!(first.operator, "7");
    assert_eq!(first.role, "SensorManager");
    assert_eq!(first.change.sensor, gungnir_model::SensorId(1));
    assert!((first.after.watched_m2 - answer.comparison.watched_after_m2).abs() < 1.0);
    assert!((first.height_agl_m - 100.0).abs() < f64::EPSILON);
    let line = state
        .dead_spots
        .what_if
        .describes()
        .expect("running")
        .to_owned();
    assert!(line.contains("switched on from standby"), "{line}");

    // The map: the what-if over the live dead spots, under a line saying what it is.
    let frame = draw_viewport(&mut state);
    assert!(
        frame.says("WHAT-IF, not the live picture"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("switched on from standby"), "{}", frame.joined());

    // The live radar searching, and the same radar's mast raised over the ridge.
    sustainment::record_observed_mode(&mut state, 1, SensorMode::Search).expect("searching");
    set(
        &mut state,
        WhatIfDraft {
            sensor: Some(1),
            mast_m: 300.0,
            ..WhatIfDraft::default()
        },
    );
    answered(&mut state, "the raised-mast what-if");
    let answer = state.dead_spots.what_if.answer.as_ref().expect("an answer");
    assert!(
        answer.comparison.cleared_m2 > 1.0e6,
        "a mast 300 m up sees over a 200 m ridge: {} m² cleared",
        answer.comparison.cleared_m2
    );
    assert!(answer.after_measure.shadow_m2 < answer.before_measure.shadow_m2);
    assert!(answer.codes.contains(&DeadSpotCell::WouldClear));
    let recorded = what_ifs();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert!((recorded[0].change.mast_m - 300.0).abs() < f64::EPSILON);

    // A live change recomputes the run; it is the same run, and is not recorded again.
    sustainment::record_observed_mode(&mut state, 1, SensorMode::Standby).expect("standby");
    answered(&mut state, "the recomputed what-if");
    assert!(state
        .dead_spots
        .what_if
        .answer
        .as_ref()
        .is_some_and(|a| a.inputs.before.is_empty()));
    assert!(
        what_ifs().is_empty(),
        "a recompute was recorded as a new run"
    );

    // Cleared: the live field is back on the map, and nothing more is recorded.
    dead_spots::apply(&mut state, LayerAction::ClearWhatIf);
    update::tick(&mut state);
    assert!(state.dead_spots.what_if.describes().is_none());
    let frame = draw_viewport(&mut state);
    assert!(!frame.says("WHAT-IF"), "{}", frame.joined());
    assert!(what_ifs().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
