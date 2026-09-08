// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What each role's workspace actually draws (GAP-055, GAP-071, GAP-075).
//!
//! Everything else about the dock tree is tested as data: the arrangement round-trips,
//! the default names every docked panel, the detachable lists agree. None of that puts
//! a pane on screen. These render the real tree, with the real `Behavior`, through the
//! headless probe -- no window, no GPU, no display -- and assert on the text that came
//! out.
//!
//! This is the gap that was open when GAP-075 closed: the tree and the panels in it were
//! covered by the compiler and by tests over their inputs, and by nobody looking at
//! them. It does not replace looking at them -- whether a warning is *noticeable* is a
//! person's judgement, and that is GAP-074 -- but "every role's workspace draws every
//! panel it is supposed to, without panicking" is a gate, and it was not one before.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::SustainmentState;
use gungnir_app::{dock, update};
use gungnir_config::ConfigBaseline;
use gungnir_security::Role;
use gungnir_ui::harness::RenderProbe;
use gungnir_workflow::{PanelId, WorkspaceLayout};

fn desktop(name: &str) -> AppState {
    let dir = std::env::temp_dir().join(format!("gungnir-rendered-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    AppState::with_config(config).expect("desktop state")
}

/// Every panel draws something. A pane that silently drew nothing would leave a hole in
/// a role's screen with nothing on it to say why -- which is the failure the placeholder
/// panels exist to prevent, and this checks it for the built ones too.
///
/// Every `PanelId`, not only the built ones: an unbuilt panel must draw its placeholder.
#[test]
fn every_panel_draws_something() {
    let mut state = desktop("panels");
    update::tick(&mut state);
    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();

    for panel in PanelId::ALL {
        if *panel == PanelId::Viewport3d {
            continue; // the central area, drawn by `gungnir-viewport3d`
        }
        let (_, frame) = probe.draw(|ui| {
            let mut behavior = dock::PanelBehavior::new(&state, &mut sustainment);
            behavior.draw_panel(ui, *panel);
        });
        assert!(
            !frame.texts.is_empty(),
            "{} ({}) drew nothing",
            panel.pn(),
            panel.title()
        );
    }
}

/// Every role's default workspace renders as a tree without panicking, and draws at
/// least as many panes as its layout has.
///
/// Counted by panes rather than by matching titles: a panel's own heading is its own
/// wording (PN-13's is "Mission report", not "Reports"), and a test that matched
/// `PanelId::title()` would be asserting a coincidence rather than a requirement.
#[test]
fn every_role_workspace_renders_as_a_tree() {
    let mut state = desktop("roles");
    update::tick(&mut state);
    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();

    for role in Role::ALL {
        state.set_role(*role);
        let layout = WorkspaceLayout::for_role(*role);
        let arrangement = dock::default_arrangement(&layout);
        let expected = arrangement.panels().len();
        let mut tree = dock::tree_from(&arrangement);

        let mut panes_drawn = 0_usize;
        let (_, frame) = probe.draw(|ui| {
            let mut behavior = dock::PanelBehavior::new(&state, &mut sustainment);
            tree.ui(&mut behavior, ui);
            panes_drawn = tree
                .tiles
                .iter()
                .filter(|(_, t)| matches!(t, egui_tiles::Tile::Pane(_)))
                .count();
        });

        assert!(
            !frame.texts.is_empty(),
            "{role:?}'s workspace drew nothing at all"
        );
        assert_eq!(
            panes_drawn, expected,
            "{role:?}'s tree holds {panes_drawn} panes for {expected} panels"
        );
    }
}

/// An unbuilt panel names the gap that will build it, on screen. Drawing an empty frame
/// instead would tell an operator there are no requirements or no coverage gaps.
#[test]
fn an_unbuilt_panel_draws_its_gap() {
    let state = desktop("unbuilt");
    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();

    // The assistant is the panel this list is down to.
    // PN-15 left this list when GAP-005 built it, PN-11 when GAP-007 did, PN-18 when
    // GAP-050's failover gave it a state to draw (2026-09-06), and PN-16 when GAP-087
    // built the options table (2026-09-07).
    let panel = PanelId::Assistant;
    let (_, frame) = probe.draw(|ui| {
        let mut behavior = dock::PanelBehavior::new(&state, &mut sustainment);
        behavior.draw_panel(ui, panel);
    });
    assert!(
        frame.says("GAP-"),
        "{} drew no gap reference: {}",
        panel.pn(),
        frame.joined()
    );
    assert!(
        frame.says(panel.pn()),
        "{} did not name itself: {}",
        panel.pn(),
        frame.joined()
    );
}

/// The approval queue in a real desktop draws the reason it is empty, and that reason
/// is the pipeline rather than the reassuring one. This is the end-to-end version of
/// the panel-level test: the view is built from `AppState`, not handed in.
#[test]
fn the_desktop_queue_draws_the_pipeline_as_its_reason() {
    let mut state = desktop("queue");
    for _ in 0..3 {
        update::tick(&mut state);
    }
    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();

    let (_, frame) = probe.draw(|ui| {
        let mut behavior = dock::PanelBehavior::new(&state, &mut sustainment);
        behavior.draw_panel(ui, PanelId::ApprovalQueue);
    });

    assert!(
        frame.says("allocator"),
        "the desktop's own queue did not say why it is empty: {}",
        frame.joined()
    );
    assert!(
        !frame.says("Nothing is waiting on a decision"),
        "the queue drew the reassuring sentence while the pipeline is not implemented: \
         {}",
        frame.joined()
    );
}

/// A configured arrangement is what gets drawn, and a detached panel is drawn once --
/// in its own window, not also in the tree.
#[test]
fn a_configured_arrangement_is_what_reaches_the_screen() {
    use gungnir_model::{LayoutNode, RoleLayout, UiSettings};
    use std::collections::BTreeMap;

    let dir =
        std::env::temp_dir().join(format!("gungnir-rendered-arranged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ui: UiSettings {
            scene_3d: false,
            theme: "day".to_owned(),
            layouts: BTreeMap::from([(
                "Operator".to_owned(),
                RoleLayout {
                    main: LayoutNode::stack(&["PN-06", "PN-03", "PN-09"]),
                    detached: vec!["PN-06".to_owned()],
                },
            )]),
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("the arrangement is valid");
    let state = AppState::with_config(config).expect("desktop state");
    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();

    let arrangement = state
        .config
        .ui
        .for_role("Operator")
        .expect("the operator has an arrangement");
    let pruned = arrangement
        .main
        .without(&["PN-06"])
        .expect("two panels remain");
    let mut tree = dock::tree_from(&pruned);

    let (_, frame) = probe.draw(|ui| {
        let mut behavior = dock::PanelBehavior::new(&state, &mut sustainment);
        tree.ui(&mut behavior, ui);
    });

    assert!(
        frame.says(PanelId::TrackTable.title()),
        "the configured track table was not drawn: {}",
        frame.joined()
    );
    assert!(
        frame.says(PanelId::SystemHealth.title()),
        "the configured health panel was not drawn: {}",
        frame.joined()
    );
    assert!(
        !frame.says("Approval queue"),
        "the detached queue was drawn in the main tree as well as its own window: {}",
        frame.joined()
    );
}

/// GAP-007: coverage that cannot be placed says so on the map, rather than the map
/// simply having no rings on it.
///
/// The two situations are opposite in what they tell a sensor manager. "Nothing is
/// covered" is a fact about the sector. "The rings cannot be placed" is a fact about
/// this deployment's configuration, and the sector may be perfectly well covered.
#[test]
#[allow(clippy::too_many_lines)]
fn coverage_that_cannot_be_placed_says_so_on_the_map() {
    use gungnir_app::sustainment;
    use gungnir_config::SensorConfig;
    use gungnir_ui::harness::RenderProbe;

    let dir = std::env::temp_dir().join(format!("gungnir-coverage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let sensors = vec![SensorConfig {
        id: 1,
        modality: "radar".into(),
        position: [55.0_f64.to_radians(), 12.0_f64.to_radians(), 0.0],
        max_range_m: 50_000.0,
        control_endpoint: None,
        maintenance: Vec::new(),
    }];

    // No origin declared: the sensor is configured, and its ring is unplaceable.
    let mut state = AppState::with_config(ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        sensors: sensors.clone(),
        ..ConfigBaseline::default()
    })
    .expect("desktop state");
    assert!(
        sustainment::coverage_circles(&state).is_empty(),
        "a circle was placed without a local frame"
    );

    let probe = RenderProbe::new();
    let circles = sustainment::coverage_circles(&state);
    let layer = sustainment::coverage_layer(&state, &circles);
    let (_, frame) = probe.draw(|ui| {
        gungnir_viewport3d::render(
            ui,
            &state.palette,
            &mut state.viewport,
            &[],
            &gungnir_model::PlanView::default(),
            gungnir_viewport3d::layers::LayerInputs {
                coverage: layer,
                gaps: &[],
                hazards: &[],
                geofences: &[],
                predictions: &[],
                terrain: None,
                point_clouds: &[],
                laydown_preview: None,
            },
        );
    });
    assert!(
        frame.says("unplaceable"),
        "the map did not say the rings cannot be placed: {}",
        frame.joined()
    );
    assert!(
        !frame.says("nothing is covered"),
        "an unplaceable layer was drawn as an uncovered sector: {}",
        frame.joined()
    );

    // With an origin, the sensor's ring is placed -- and the map says it is the
    // configured range rather than observed coverage.
    let mut placed = AppState::with_config(ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        sensors,
        origin: Some([55.0_f64.to_radians(), 12.0_f64.to_radians(), 0.0]),
        ..ConfigBaseline::default()
    })
    .expect("desktop state");
    // Every sensor starts at Standby (GAP-003), so nothing is covered until somebody
    // switches one on. That is the honest default and the reason PN-10 exists.
    assert!(
        sustainment::coverage_circles(&placed).is_empty(),
        "a standby sensor was credited with coverage"
    );
    sustainment::record_observed_mode(&mut placed, 1, gungnir_model::SensorMode::Search)
        .expect("standby to search is a permitted transition");

    let circles = sustainment::coverage_circles(&placed);
    assert_eq!(circles.len(), 1);
    for axis in circles[0].center {
        assert!(
            axis.abs() < 1.0,
            "the sensor at the origin should be at the origin: {:?}",
            circles[0].center
        );
    }

    let layer = sustainment::coverage_layer(&placed, &circles);
    let (_, frame) = probe.draw(|ui| {
        gungnir_viewport3d::render(
            ui,
            &placed.palette,
            &mut placed.viewport,
            &[],
            &gungnir_model::PlanView::default(),
            gungnir_viewport3d::layers::LayerInputs {
                coverage: layer,
                gaps: &[],
                hazards: &[],
                geofences: &[],
                predictions: &[],
                terrain: None,
                point_clouds: &[],
                laydown_preview: None,
            },
        );
    });
    // Since GAP-003 the rings are observed rather than nominal: they come from
    // `SensorRegistry::coverage`, which reports only what is searching or tracking. The
    // caveat that used to be drawn would now be a false one.
    assert!(
        !frame.says("configured range"),
        "an observed coverage ring still carried the nominal caveat: {}",
        frame.joined()
    );
    assert!(
        !frame.says("unplaceable"),
        "a placed ring was reported as unplaceable: {}",
        frame.joined()
    );
}

/// GAP-006 through the desktop: a declared approach that runs out past the sensors is
/// reported as an uncovered stretch, and the strip counts it.
///
/// The three coverage states the strip can be in are opposite claims about a sector, and
/// the one that must never be shown as full coverage is "nothing was measured".
#[test]
fn coverage_gaps_are_reported_along_the_declared_approaches() {
    use gungnir_app::sustainment;
    use gungnir_config::{ApproachConfig, SensorConfig};
    use gungnir_ui::panels::status_strip::CoverageStatus;

    let origin = [55.0_f64.to_radians(), 12.0_f64.to_radians(), 0.0];
    let dir = std::env::temp_dir().join(format!("gungnir-gaps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // One sensor at the origin with a 20 km range, and an approach running due north
    // from the origin to 0.5 degrees away -- about 55 km, so most of it is uncovered.
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(origin),
        sensors: vec![SensorConfig {
            id: 1,
            modality: "radar".into(),
            position: origin,
            max_range_m: 20_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }],
        // At altitude, because an air-defence approach axis is a flight corridor and
        // because of what a ground-level one would run into: in the ENU tangent plane a
        // point at constant altitude drops below the plane with distance -- about 240 m
        // at 55 km -- and `FlatTerrainLineOfSight` treats anything below the plane as
        // blocked. That is a crude horizon and roughly the right direction, but it means
        // a ground-level approach reads as invisible past a few kilometres. See
        // `ARCHITECTURE.md` §10 item 55.
        approaches: vec![ApproachConfig {
            name: "northern axis".into(),
            points: vec![
                [origin[0], origin[1], 3_000.0],
                [55.5_f64.to_radians(), 12.0_f64.to_radians(), 3_000.0],
            ],
        }],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("the approach is valid");
    let mut state = AppState::with_config(config).expect("desktop state");

    // A registry that has just come up has every sensor at Standby, so the whole
    // approach is uncovered -- correctly, and this is the state PN-10 changes.
    let standby = sustainment::coverage_report(&state).expect("an origin and an approach");
    assert!(
        standby
            .gap_length_m(gungnir_analytics::GapSeverity::SingleSensor)
            .abs()
            < f64::EPSILON,
        "a standby sensor contributed coverage"
    );
    sustainment::record_observed_mode(&mut state, 1, gungnir_model::SensorMode::Track)
        .expect("standby to track is a permitted transition");

    let report = sustainment::coverage_report(&state).expect("an origin and an approach");
    assert!(
        !report.parameters.terrain_masking_applied,
        "no terrain is loaded, so the answer is the optimistic one and must say so"
    );
    assert!(
        report.gap_length_m(gungnir_analytics::GapSeverity::Uncovered) > 20_000.0,
        "an approach running well past a 20 km sensor should be largely uncovered: {:?}",
        report.gaps
    );
    // One sensor never gives redundancy, so the covered part is single-sensor rather
    // than covered -- DN-12's rule that detectable is not fusible.
    assert!(report.gap_length_m(gungnir_analytics::GapSeverity::SingleSensor) > 0.0);

    match sustainment::coverage_status(&state, Some(&report)) {
        CoverageStatus::Measured {
            uncovered_segments,
            terrain_masking,
            ..
        } => {
            assert!(uncovered_segments > 0);
            assert!(!terrain_masking);
        }
        other => panic!("expected a measured coverage status, got {other:?}"),
    }

    // The gaps reach the map, named by their approach rather than by an index.
    let names = sustainment::approach_names(&state);
    let gaps = sustainment::gap_polylines(&names, &report);
    assert!(gaps.iter().any(|g| g.approach == "northern axis"));
    assert!(gaps.iter().any(|g| g.uncovered));

    let probe = gungnir_ui::harness::RenderProbe::new();
    let circles = sustainment::coverage_circles(&state);
    let layer = sustainment::coverage_layer(&state, &circles);
    let (_, frame) = probe.draw(|ui| {
        gungnir_viewport3d::render(
            ui,
            &state.palette,
            &mut state.viewport,
            &[],
            &gungnir_model::PlanView::default(),
            gungnir_viewport3d::layers::LayerInputs {
                coverage: layer,
                gaps: &gaps,
                hazards: &[],
                geofences: &[],
                predictions: &[],
                terrain: None,
                point_clouds: &[],
                laydown_preview: None,
            },
        );
    });
    assert!(!frame.texts.is_empty(), "the viewport drew nothing");
}

/// A deployment with no declared approaches has measured nothing, which the strip must
/// not show as full coverage.
#[test]
fn no_declared_approaches_is_not_full_coverage() {
    use gungnir_app::sustainment;
    use gungnir_ui::panels::status_strip::CoverageStatus;

    let dir = std::env::temp_dir().join(format!("gungnir-no-approaches-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = AppState::with_config(ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([55.0_f64.to_radians(), 12.0_f64.to_radians(), 0.0]),
        ..ConfigBaseline::default()
    })
    .expect("desktop state");

    assert!(sustainment::coverage_report(&state).is_none());
    assert_eq!(
        sustainment::coverage_status(&state, None),
        CoverageStatus::NoApproaches,
        "an unmeasured sector was reported as measured"
    );

    // And with approaches but no origin, they cannot be placed -- a third distinct state.
    let dir = std::env::temp_dir().join(format!(
        "gungnir-no-origin-approaches-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let state = AppState::with_config(ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        approaches: vec![gungnir_config::ApproachConfig {
            name: "northern axis".into(),
            points: vec![
                [55.0_f64.to_radians(), 12.0_f64.to_radians(), 0.0],
                [55.5_f64.to_radians(), 12.0_f64.to_radians(), 0.0],
            ],
        }],
        ..ConfigBaseline::default()
    })
    .expect("desktop state");
    assert!(sustainment::coverage_report(&state).is_none());
    assert!(matches!(
        sustainment::coverage_status(&state, None),
        CoverageStatus::NotPlaceable { .. }
    ));
}
