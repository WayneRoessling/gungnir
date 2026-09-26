// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-16's data path (GAP-087): the current laydown gets no difference against itself,
//! an alternative's difference is computed against it, and a deployment with no
//! declared laydowns says so rather than drawing an empty table.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::{laydown_preview, planning_rows, PlanningRows};
use gungnir_config::ConfigBaseline;
use gungnir_model::laydown::{Laydown, LaydownId, ResourcePlacement, SensorPlacement};
use gungnir_model::{ResourceId, SensorId, SensorMode};
use gungnir_ui::panels::planning::LaydownCoverage;

fn placed(sensor: u32, position_enu: [f64; 3]) -> SensorPlacement {
    SensorPlacement {
        sensor: SensorId(sensor),
        position_enu,
        mode: SensorMode::Search,
        azimuth_sector: None,
    }
}

fn resourced(resource: u32) -> ResourcePlacement {
    ResourcePlacement {
        resource: ResourceId(resource),
        position_enu: [0.0, 0.0, 0.0],
    }
}

fn laydown(id: &str, current: bool, sensors: Vec<SensorPlacement>) -> Laydown {
    Laydown {
        id: LaydownId(id.into()),
        intent: "cover the eastern approach".into(),
        sensors,
        resources: vec![resourced(1)],
        current,
    }
}

/// A deployment with an origin, one approach, one declared sensor, and one resource --
/// everything `planning_rows` needs before it can compute anything. Built through JSON
/// rather than a struct literal so the many optional fields these two config types
/// carry take their own `#[serde(default)]`, the way a real configuration file would.
fn base_config() -> ConfigBaseline {
    let text = serde_json::json!({
        "version": ConfigBaseline::default().version,
        "origin": [0.0, 0.0, 0.0],
        "sensors": [
            {"id": 1, "modality": "radar", "position": [0.0, 0.0, 0.0], "max_range_m": 50_000.0}
        ],
        "resources": [
            {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 1, "layer": "point"}
        ],
        "approaches": [
            {"name": "east", "points": [[0.0, 0.0, 100.0], [0.0, 0.01, 100.0]]}
        ],
    })
    .to_string();
    serde_json::from_str(&text).expect("the fixture baseline parses")
}

#[test]
fn no_laydowns_declared_is_an_honest_empty_state() {
    let config = base_config();
    gungnir_config::validate(&config).expect("valid with no laydowns");
    let state = AppState::with_config(config).expect("starts");
    match planning_rows(&state) {
        PlanningRows::Empty { reason } => {
            assert!(reason.contains("no laydown"), "{reason}");
        }
        PlanningRows::Rows(_) => panic!("a baseline with no laydowns must not show a table"),
    }
}

/// The PN-16 laydown options table row of `docs/verification-capability-table.md` §2:
/// the current laydown carries no difference against itself, and an alternative's
/// difference is computed against it.
#[test]
// The difference is defined as one subtraction of the two uncovered lengths the rows
// themselves carry (`LaydownCoverage::Computed::delta_uncovered_m`), so the check is
// equality with that subtraction; a tolerance would admit a difference taken against
// something other than the current row.
#[allow(clippy::float_cmp)]
fn the_current_laydown_has_no_difference_against_itself_and_an_alternative_does() {
    let mut config = base_config();
    config.laydowns = vec![
        laydown("current", true, vec![placed(1, [0.0, 0.0, 10.0])]),
        // Moved far enough that it cannot see the approach at all: this laydown's
        // sensor sits on the opposite side of the earth from the approach, so its
        // gap should be worse than the current laydown's, and the delta must say so.
        laydown("moved-away", false, vec![placed(1, [-1.0e7, 0.0, 10.0])]),
    ];
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("starts");

    let rows = match planning_rows(&state) {
        PlanningRows::Rows(rows) => rows,
        PlanningRows::Empty { reason } => panic!("expected rows, got: {reason}"),
    };
    assert_eq!(rows.len(), 2);

    let current = rows
        .iter()
        .find(|r| r.id == LaydownId("current".into()))
        .expect("current row");
    assert!(current.current);
    let current_uncovered_m = match &current.coverage {
        LaydownCoverage::Computed {
            uncovered_m,
            delta_uncovered_m,
            ..
        } => {
            assert_eq!(
                *delta_uncovered_m, None,
                "the current laydown has no difference against itself"
            );
            *uncovered_m
        }
        LaydownCoverage::NotComputed { reason } => panic!("expected computed: {reason}"),
    };

    let moved = rows
        .iter()
        .find(|r| r.id == LaydownId("moved-away".into()))
        .expect("alternative row");
    assert!(!moved.current);
    match &moved.coverage {
        LaydownCoverage::Computed {
            uncovered_m,
            delta_uncovered_m,
            ..
        } => {
            let delta = delta_uncovered_m.expect("an alternative gets a delta against current");
            // Its value, not only its sign: this row's uncovered length less the current
            // row's, as the two rows report them.
            assert_eq!(
                delta,
                uncovered_m - current_uncovered_m,
                "the difference must be {uncovered_m} m (this row) less {current_uncovered_m} m \
                 (the current row)"
            );
            assert!(
                delta > 0.0,
                "a sensor moved off the map must cover less, not more: delta was {delta}"
            );
        }
        LaydownCoverage::NotComputed { reason } => panic!("expected computed: {reason}"),
    }
}

/// Selecting an option previews exactly its own sensors and resources (GAP-087's own
/// remaining item) -- not the current laydown's, and not a mix of the two.
#[test]
fn selecting_a_laydown_previews_its_own_sensors_and_resources() {
    let mut config = base_config();
    config.laydowns = vec![
        laydown("current", true, vec![placed(1, [0.0, 0.0, 10.0])]),
        laydown("moved-away", false, vec![placed(1, [-1.0e7, 0.0, 10.0])]),
    ];
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");

    assert!(laydown_preview(&state).is_none(), "nothing is selected yet");

    state.select_laydown(LaydownId("moved-away".into()));
    let preview = laydown_preview(&state).expect("a declared laydown is selected");
    assert_eq!(preview.intent, "cover the eastern approach");
    assert_eq!(preview.sensor_positions, vec![[-1.0e7, 0.0, 10.0]]);
    assert_eq!(preview.resource_positions, vec![[0.0, 0.0, 0.0]]);
}

/// Re-selecting the previewed option clears it, the same toggle-by-reclick rule PN-03's
/// track selection uses.
#[test]
fn reclicking_the_selected_laydown_clears_the_preview() {
    let mut config = base_config();
    config.laydowns = vec![laydown("current", true, vec![placed(1, [0.0, 0.0, 10.0])])];
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");

    state.select_laydown(LaydownId("current".into()));
    assert!(laydown_preview(&state).is_some());
    state.select_laydown(LaydownId("current".into()));
    assert!(
        laydown_preview(&state).is_none(),
        "re-clicking the selected option must clear it"
    );
}

/// A selection naming a laydown this baseline no longer declares -- a reload could
/// remove one mid-session -- previews nothing rather than a stale placement.
#[test]
fn a_selection_naming_no_declared_laydown_previews_nothing() {
    let mut config = base_config();
    config.laydowns = vec![laydown("current", true, vec![placed(1, [0.0, 0.0, 10.0])])];
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");

    state.select_laydown(LaydownId("never-declared".into()));
    assert!(
        laydown_preview(&state).is_none(),
        "a selection naming no declared laydown must not preview a stale one"
    );
}

/// GAP-118, D-84: PN-16 counts a sensor's coverage only inside its azimuth sector. The
/// approach runs east; a laydown that re-aims the radar west leaves all of it uncovered,
/// one that aims it east covers what the full circle did, and a sector declared on the
/// sensor itself is what a placement without its own inherits.
#[test]
fn a_laydown_counts_coverage_only_inside_the_sensor_s_sector() {
    let sector = |boresight_deg: f64, width_deg: f64| {
        gungnir_model::AzimuthSector::new(boresight_deg.to_radians(), width_deg.to_radians())
            .expect("legal")
    };
    let aimed = |id: &str, s: Option<gungnir_model::AzimuthSector>| {
        let mut p = placed(1, [0.0, 0.0, 10.0]);
        p.azimuth_sector = s;
        laydown(id, id == "current", vec![p])
    };
    let uncovered = |config: ConfigBaseline| -> std::collections::HashMap<String, f64> {
        gungnir_config::validate(&config).expect("valid");
        let state = AppState::with_config(config).expect("starts");
        match planning_rows(&state) {
            PlanningRows::Rows(rows) => rows
                .into_iter()
                .map(|r| match r.coverage {
                    LaydownCoverage::Computed { uncovered_m, .. } => (r.id.0, uncovered_m),
                    LaydownCoverage::NotComputed { reason } => panic!("{reason}"),
                })
                .collect(),
            PlanningRows::Empty { reason } => panic!("{reason}"),
        }
    };

    let mut config = base_config();
    config.laydowns = vec![
        aimed("current", None),
        aimed("aimed-west", Some(sector(270.0, 90.0))),
        aimed("aimed-east", Some(sector(90.0, 60.0))),
    ];
    let rows = uncovered(config);
    let (full, west, east) = (rows["current"], rows["aimed-west"], rows["aimed-east"]);
    // The approach is 0.01 rad of longitude, 63.8 km; sampled at the default 250 m, its
    // last sample is at 63.5 km. The full circle covers its near stretch (below the
    // curvature of the earth, flat line of sight loses it further out).
    assert!(
        full < 60_000.0,
        "the full circle covers some of it: {full} m"
    );
    assert!(
        west >= 63_000.0,
        "aimed away, the radar covers none of the approach: {west} m vs {full} m"
    );
    assert!(
        (east - full).abs() < 1.0,
        "aimed along it, it covers what the full circle did: {east} m vs {full} m"
    );

    // Declared on the sensor: every placement without its own sector inherits it.
    let mut config = base_config();
    config.sensors[0].azimuth_sector = Some(sector(270.0, 90.0));
    config.laydowns = vec![
        aimed("current", None),
        aimed("aimed-east", Some(sector(90.0, 60.0))),
    ];
    let rows = uncovered(config);
    assert!((rows["current"] - west).abs() < 1.0, "{rows:?}");
    assert!((rows["aimed-east"] - full).abs() < 1.0, "{rows:?}");
}

#[test]
fn a_baseline_with_no_origin_reports_why_rather_than_a_zero() {
    let mut config = base_config();
    config.origin = None;
    config.laydowns = vec![laydown("current", true, vec![placed(1, [0.0, 0.0, 10.0])])];
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("starts");

    let rows = match planning_rows(&state) {
        PlanningRows::Rows(rows) => rows,
        PlanningRows::Empty { reason } => panic!(
            "laydowns exist; rows must still be drawn, one per laydown, with a reason: {reason}"
        ),
    };
    assert_eq!(rows.len(), 1);
    match &rows[0].coverage {
        LaydownCoverage::NotComputed { reason } => {
            assert!(reason.contains("frame"), "{reason}");
        }
        LaydownCoverage::Computed { .. } => panic!("no origin means no ENU frame to compute in"),
    }
}
