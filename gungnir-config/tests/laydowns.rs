// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The laydown table's five refusals (`docs/design/DN-26-laydown-options.md` §4, §10).
//!
//! The verification row DN-26 §10 asks for is "each of §4's five refusals, and a valid
//! multi-laydown baseline; every invalid baseline is refused naming the laydown and
//! field; the valid one loads".
//!
//! **Why each of these is a refusal rather than a default** is the thing worth testing:
//! every quiet alternative produces a comparison that still looks like an answer. A
//! baseline with no current laydown could pick the first; one placing an unknown sensor
//! could drop it; one where options place different sensors could take the union. Each
//! of those yields a coverage number a planner would read as a property of the laydowns
//! rather than of the configuration, which is the failure DN-26 §5 exists to prevent.

use gungnir_config::ConfigBaseline;
use gungnir_model::laydown::{Laydown, LaydownId, ResourcePlacement, SensorPlacement};
use gungnir_model::{ResourceId, SensorId, SensorMode};

/// A baseline that declares two sensors and one resource for laydowns to place.
fn baseline_with_registries() -> ConfigBaseline {
    let text = serde_json::json!({
        // The configuration schema version, which is not the model schema version.
        "version": ConfigBaseline::default().version,
        "sensors": [
            {"id": 1, "modality": "radar", "position": [0.0, 0.0, 10.0], "max_range_m": 20000.0},
            {"id": 2, "modality": "radar", "position": [500.0, 0.0, 10.0], "max_range_m": 20000.0}
        ],
        "resources": [
            {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 2, "layer": "point"}
        ]
    })
    .to_string();
    serde_json::from_str(&text).expect("the fixture baseline parses")
}

fn placed(sensor: u32, position: [f64; 3]) -> SensorPlacement {
    SensorPlacement {
        sensor: SensorId(sensor),
        position_enu: position,
        mode: SensorMode::Search,
    }
}

fn laydown(id: &str, current: bool, sensors: Vec<SensorPlacement>) -> Laydown {
    Laydown {
        id: LaydownId(id.into()),
        intent: "cover the western approach".into(),
        sensors,
        resources: vec![ResourcePlacement {
            resource: ResourceId(1),
            position_enu: [0.0, 0.0, 0.0],
        }],
        current,
    }
}

/// Validate a baseline the way loading does, and return the refusal text.
fn refusal(baseline: &ConfigBaseline) -> String {
    match gungnir_config::validate(baseline) {
        Ok(()) => panic!("this baseline should have been refused"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_valid_multi_laydown_baseline_loads() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![
        laydown(
            "current",
            true,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
        laydown(
            "west",
            false,
            vec![
                placed(1, [-800.0, 0.0, 12.0]),
                placed(2, [0.0, 400.0, 10.0]),
            ],
        ),
    ];
    gungnir_config::validate(&b).expect("a complete, well-formed laydown table loads");
}

#[test]
fn a_baseline_that_mentions_no_laydowns_is_valid_and_means_none_offered() {
    // DN-26 §8: every configuration written before this field existed stays valid, and
    // an empty table is the honest reading of a file that does not mention laydowns.
    let b = baseline_with_registries();
    assert!(b.laydowns.is_empty());
    gungnir_config::validate(&b).expect("no laydowns declared is a valid deployment");
}

#[test]
fn refusal_1a_no_laydown_is_marked_current() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![
        laydown(
            "a",
            false,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
        laydown(
            "b",
            false,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
    ];
    let why = refusal(&b);
    assert!(
        why.contains("none is marked current"),
        "the refusal must say what is missing: {why}"
    );
    assert!(
        why.contains("compared against"),
        "and why guessing is not an option: {why}"
    );
}

#[test]
fn refusal_1b_more_than_one_laydown_is_marked_current() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![
        laydown(
            "a",
            true,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
        laydown(
            "b",
            true,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
    ];
    let why = refusal(&b);
    assert!(why.contains('a') && why.contains('b'), "name them: {why}");
    assert!(why.contains("exactly one"), "{why}");
}

#[test]
fn refusal_2a_a_laydown_places_a_sensor_no_registry_declares() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![laydown(
        "current",
        true,
        vec![
            placed(1, [0.0, 0.0, 10.0]),
            placed(2, [500.0, 0.0, 10.0]),
            placed(9, [1.0, 1.0, 1.0]),
        ],
    )];
    let why = refusal(&b);
    assert!(
        why.contains("laydown current places sensor 9"),
        "the refusal must name the laydown and the field: {why}"
    );
}

#[test]
fn refusal_2b_a_laydown_places_a_resource_no_registry_declares() {
    let mut b = baseline_with_registries();
    let mut l = laydown(
        "current",
        true,
        vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
    );
    l.resources.push(ResourcePlacement {
        resource: ResourceId(7),
        position_enu: [0.0, 0.0, 0.0],
    });
    b.laydowns = vec![l];
    let why = refusal(&b);
    assert!(
        why.contains("laydown current places resource 7"),
        "the refusal must name the laydown and the field: {why}"
    );
}

#[test]
fn refusal_3_two_laydowns_share_an_identifier() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![
        laydown(
            "west",
            true,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
        laydown(
            "west",
            false,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
    ];
    let why = refusal(&b);
    assert!(why.contains("share the identifier"), "{why}");
    assert!(why.contains("west"), "name it: {why}");
}

#[test]
fn refusal_4_one_laydown_places_a_sensor_another_omits() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![
        laydown(
            "current",
            true,
            vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
        ),
        // Sensor 2 is simply not mentioned. Without this refusal its absence would be
        // read as a sensor contributing nothing, and the comparison would report the
        // option as having worse coverage rather than as being incompletely described.
        laydown("partial", false, vec![placed(1, [-800.0, 0.0, 12.0])]),
    ];
    let why = refusal(&b);
    assert!(
        why.contains("current") && why.contains("partial"),
        "the refusal must name both laydowns: {why}"
    );
    assert!(why.contains("complete placement"), "{why}");
}

#[test]
fn refusal_5_a_coordinate_is_not_a_finite_number() {
    let mut b = baseline_with_registries();
    b.laydowns = vec![laydown(
        "current",
        true,
        vec![
            placed(1, [0.0, f64::NAN, 10.0]),
            placed(2, [500.0, 0.0, 10.0]),
        ],
    )];
    let why = refusal(&b);
    assert!(
        why.contains("laydown current has a non-finite coordinate"),
        "the refusal must name the laydown: {why}"
    );
}

#[test]
fn the_refusals_are_checked_before_the_table_is_used_rather_than_at_first_comparison() {
    // DN-26 §6 rule 1: validation happens on load. A baseline that would produce a
    // meaningless comparison must not be in force at all, because by the time a panel
    // asked for a coverage answer the deployment would already be running on it.
    let mut b = baseline_with_registries();
    b.laydowns = vec![laydown(
        "only",
        false,
        vec![placed(1, [0.0, 0.0, 10.0]), placed(2, [500.0, 0.0, 10.0])],
    )];
    assert!(
        gungnir_config::validate(&b).is_err(),
        "the baseline itself is refused, not the first comparison drawn from it"
    );
}
