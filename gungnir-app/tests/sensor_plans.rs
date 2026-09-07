//! Sensor re-tasking recommendations reach PN-10 from the real registry (GAP-037, DN-13).
//!
//! `SensorPlanner` existed with its own tests and nothing enumerated candidates for it. These
//! are the tests behind the claim that the desktop now does: candidates come from the
//! registry's transition table, the score comes from the same coverage inputs PN-11 draws,
//! and "nothing to recommend" is told apart from "could not evaluate".

use gungnir_app::state::AppState;
use gungnir_app::sustainment::{self, NoSensorPlan};
use gungnir_config::{ApproachConfig, ConfigBaseline, SensorConfig};
use gungnir_model::{SensorId, SensorMode};
use gungnir_sensor_management::SensorRegistry;

const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0]; // 55°N 12°E in radians

fn desktop(name: &str, approaches: Vec<ApproachConfig>) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-sensor-plans-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let sensor = |id| SensorConfig {
        id,
        modality: "radar".into(),
        position: ORIGIN,
        max_range_m: 20_000.0,
        control_endpoint: None,
        maintenance: Vec::new(),
    };
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        sensors: vec![sensor(1), sensor(2)],
        approaches,
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("the baseline is valid");
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

fn northern_axis() -> ApproachConfig {
    // Due north from the origin, at altitude so the flat-terrain horizon does not hide it
    // (see `ARCHITECTURE.md` §10 item 55).
    ApproachConfig {
        name: "northern axis".into(),
        points: vec![
            [ORIGIN[0], ORIGIN[1], 3_000.0],
            [55.5_f64.to_radians(), ORIGIN[1], 3_000.0],
        ],
    }
}

/// **A standby sensor that could close a gap is recommended to radiate**, and the
/// recommendation says how much of the approach it closes -- the rationale DN-13 §5 rule 2
/// requires. The number is the same one PN-11 would draw, because both come from the same
/// coverage computation.
#[test]
fn a_standby_sensor_that_closes_a_gap_is_recommended() {
    let (state, dir) = desktop("closes", vec![northern_axis()]);
    let plans = sustainment::sensor_plans(&state).expect("an origin, an approach, sensors");
    assert!(
        !plans.is_empty(),
        "two standby sensors and a bare approach: nothing recommended"
    );

    let best = &plans[0];
    assert_eq!(
        best.changes.len(),
        1,
        "one change per plan: {:?}",
        best.changes
    );
    assert_eq!(best.changes[0].from, "Standby");
    assert!(
        best.changes[0].to == "Search" || best.changes[0].to == "Track",
        "the recommended mode does not radiate: {:?}",
        best.changes[0]
    );
    assert!(
        best.cost.delta_uncovered_m < -10_000.0,
        "a 20 km radar switched on should close well over 10 km of a bare approach, \
         closed {} m",
        -best.cost.delta_uncovered_m
    );
    let before = sustainment::coverage_report(&state).expect("the same inputs");
    assert!(
        (best
            .gaps_before
            .gap_length_m(gungnir_analytics::GapSeverity::Uncovered)
            - before.gap_length_m(gungnir_analytics::GapSeverity::Uncovered))
        .abs()
            < f64::EPSILON,
        "the plan's before-picture disagrees with PN-11's"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **A change the registry would refuse is never proposed** (DN-13 §5 rule 1). Offline is
/// not a mode a recommendation can leave; the candidate list is built from the registry's
/// own transition table.
#[test]
fn an_offline_sensor_is_never_asked_to_change() {
    let (mut state, dir) = desktop("offline", vec![northern_axis()]);
    for id in [1, 2] {
        state
            .sensors
            .set_mode(SensorId(id), SensorMode::Offline)
            .expect("standby to offline is permitted");
    }
    assert_eq!(
        sustainment::sensor_plans(&state),
        Err(NoSensorPlan::NothingCanChange)
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **"No change helps" and "could not evaluate" are different answers.** With no approach
/// there is nothing to measure coverage along; that is a reason, not an empty list that
/// PN-10 would draw as "no mode change improves coverage".
#[test]
fn no_approaches_is_a_reason_not_an_empty_recommendation() {
    let (state, dir) = desktop("no-approaches", Vec::new());
    assert_eq!(
        sustainment::sensor_plans(&state),
        Err(NoSensorPlan::NoApproaches)
    );
    assert_ne!(NoSensorPlan::NoApproaches.sentence(), "");
    let _ = std::fs::remove_dir_all(dir);
}

/// Once every sensor that can see the approach is radiating, the honest answer is that no
/// further change improves coverage: an empty `Ok`, which PN-10 draws as exactly that.
#[test]
fn a_fully_radiating_laydown_gets_no_recommendation() {
    let (mut state, dir) = desktop("nothing-to-do", vec![northern_axis()]);
    for id in [1, 2] {
        state
            .sensors
            .set_mode(SensorId(id), SensorMode::Search)
            .expect("standby to search is permitted");
    }
    let plans = sustainment::sensor_plans(&state).expect("evaluable");
    assert!(
        plans.is_empty(),
        "both sensors already radiate at the same spot; nothing should improve: {:?}",
        plans.iter().map(|p| &p.changes).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(dir);
}
