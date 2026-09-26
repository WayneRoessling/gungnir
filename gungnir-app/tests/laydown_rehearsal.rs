// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A laydown rehearsed against a real, committed test-track recording (GAP-045, GAP-105):
//! the laydown's own sensors re-observe the recording's truth where the laydown puts
//! them (`docs/design/DN-32-re-observation-for-a-laydown.md`), and what they would have
//! detected runs through the live pipeline on a throwaway desktop.
//!
//! Three of DN-32 §10's rows have their tests here, each a Draft row of
//! `docs/verification-capability-table.md` §2:
//!
//! - **A laydown sensor with no model is refused** -- by name, and no rehearsal runs:
//!   [`a_laydown_sensor_with_no_detection_model_is_refused_by_name_and_nothing_runs`].
//! - **Round 1 laydown `c`** -- rehearse `current` and `c` over the round-1 scenario;
//!   S2's per-sensor detection counts differ, and the record says which sensor the
//!   difference came from: [`round_1s_forward_radar_changes_its_own_detections_and_nothing_else`].
//! - **Geometry moves detections**, end to end rather than in the model alone:
//!   [`moving_a_sensor_out_of_range_of_the_raid_empties_its_detections`] (the property
//!   test is `gungnir-sensor-sim/tests/geometry.rs`).
//!
//! **A count that used to wander, and why it no longer does.** A run once read its
//! picture at whatever point the pipeline's own task had reached, so the number was
//! partly a measurement of the machine; it now ends its detection stream and waits for
//! the pipeline's end-of-stream flush before reading anything, which is deterministic by
//! construction (GAP-045, `laydown_rehearsal.rs`'s module doc).

// Detection counts are small and non-negative, converted to i64 to take a difference and
// back only where the test itself computed a sum of them; the two round-1 tests read as
// one scenario each rather than several fragments sharing a set-up.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]

use gungnir_app::laydown_rehearsal::{run, sensors_that_differ, RehearsalError};
use gungnir_config::{ConfigBaseline, ResourceConfig, SensorConfig};
use gungnir_coord::{CoordTransform, Geodetic, Wgs84};
use gungnir_model::laydown::{Laydown, LaydownId, ResourcePlacement, SensorPlacement};
use gungnir_model::{ResourceId, SensorId, SensorMode, TestTrackNumber};
use gungnir_ui::panels::planning::{RehearsalSection, RowRehearsal, VersusCurrent};

fn testdata_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata")
}

fn base_resources() -> Vec<ResourceConfig> {
    let text = serde_json::json!([
        {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 2, "layer": "point"}
    ])
    .to_string();
    serde_json::from_str(&text).expect("the fixture resource config parses")
}

fn sensor(id: u32, model: Option<&str>) -> SensorConfig {
    let mut v = serde_json::json!({
        "id": id, "modality": "radar", "position": [0.0, 0.0, 0.0], "max_range_m": 15000.0
    });
    if let Some(m) = model {
        v["detection_model"] = m.into();
    }
    serde_json::from_value(v).expect("the fixture sensor config parses")
}

/// The ridge site TT-01's own long-range radar stands on (`scenarios.yaml`'s
/// `RIDGE_RADAR`), from which a long-range radar sees TT-01's raid inbound.
const RIDGE: [f64; 3] = [8000.0, 3000.0, 450.0];

/// A laydown placing the deployment's sensor 1 at `sensor_enu`, in search.
fn laydown(id: &str, sensor_enu: [f64; 3]) -> Laydown {
    Laydown {
        id: LaydownId(id.into()),
        intent: "a candidate placement for the rehearsal test".into(),
        sensors: vec![SensorPlacement {
            sensor: SensorId(1),
            position_enu: sensor_enu,
            mode: SensorMode::Search,
            azimuth_sector: None,
        }],
        resources: vec![ResourcePlacement {
            resource: ResourceId(1),
            position_enu: [1000.0, 500.0, 0.0],
        }],
        current: true,
    }
}

#[test]
fn a_rehearsal_re_observes_a_real_recording_and_reports_the_queue_honestly() {
    let record = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown("rehearsed", RIDGE),
        &[sensor(1, Some("radar.long"))],
        &base_resources(),
    )
    .expect("TT-01's recording runs");

    assert_eq!(record.scenario, TestTrackNumber(1));
    assert_eq!(record.laydown, LaydownId("rehearsed".into()));
    assert_eq!(record.seed, 1701, "the recording's own seed");
    assert_eq!(record.sensors.len(), 1);
    assert_eq!(
        record.sensors[0].detection_model.as_deref(),
        Some("radar.long")
    );
    assert!(
        record.sensors[0].detections > 0,
        "a long-range radar on the ridge sees TT-01's raid inbound"
    );
    assert!(
        record.tracks_formed > 0,
        "the re-observed detections form tracks in the live pipeline"
    );
    // Never a claim beyond what the run actually measured: an expired decision is a
    // subset of raised ones, never more.
    assert!(record.decisions_expired <= record.decisions_raised);
}

/// The regression test for a rehearsal's numbers being the recording's, not the
/// runner's: the same laydown twice, back to back in one process, is the same record.
#[test]
fn two_runs_of_one_laydown_report_the_same_thing() {
    let l = laydown("twice", RIDGE);
    let sensors = [sensor(1, Some("radar.long"))];
    let first = run(
        &testdata_root(),
        TestTrackNumber(1),
        &l,
        &sensors,
        &base_resources(),
    )
    .expect("runs");
    let second = run(
        &testdata_root(),
        TestTrackNumber(1),
        &l,
        &sensors,
        &base_resources(),
    )
    .expect("runs a second time");
    assert_eq!(
        first, second,
        "the same laydown against the same recording measured differently twice"
    );
}

/// **Geometry moves detections**, end to end: the same radar moved to the far side of
/// the sector, beyond its longest band from every target, re-observes nothing.
#[test]
fn moving_a_sensor_out_of_range_of_the_raid_empties_its_detections() {
    let sensors = [sensor(1, Some("radar.long"))];
    let near = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown("near", RIDGE),
        &sensors,
        &base_resources(),
    )
    .expect("runs");
    let far = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown("far", [-250_000.0, 250_000.0, 450.0]),
        &sensors,
        &base_resources(),
    )
    .expect("runs");
    assert!(near.sensors[0].detections > 0);
    assert_eq!(
        far.sensors[0].detections, 0,
        "nothing of the raid is in range"
    );
    let differ = sensors_that_differ(&near, &far).expect("one recording, comparable");
    assert_eq!(differ.len(), 1);
    assert_eq!(differ[0].sensor, SensorId(1));
    assert!(differ[0].moved);
}

/// **A laydown sensor with no model is refused**: by name, with nothing run -- the error
/// comes back before a throwaway desktop, a journal or a pipeline exists -- and never by
/// borrowing TT-01's own sensor 1, a long-range ridge radar that happens to share the
/// identifier (DN-32 §5.4).
#[test]
fn a_laydown_sensor_with_no_detection_model_is_refused_by_name_and_nothing_runs() {
    let scratch_before = rehearsal_scratch_dirs();
    let err = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown("unmodelled", [500.0, 300.0, 20.0]),
        &[sensor(1, None)],
        &base_resources(),
    )
    .expect_err("a sensor that names no detection model is refused");
    assert!(
        matches!(err, RehearsalError::NoDetectionModel { sensor: 1, .. }),
        "{err}"
    );
    let text = err.to_string();
    assert!(
        text.contains("sensor 1") && text.contains("unmodelled"),
        "{text}"
    );
    assert!(text.contains("does not borrow"), "{text}");
    assert_eq!(
        rehearsal_scratch_dirs(),
        scratch_before,
        "a refused rehearsal made a scratch desktop"
    );

    let err = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown("unknown", [500.0, 300.0, 20.0]),
        &[sensor(1, Some("radar.imaginary"))],
        &base_resources(),
    )
    .expect_err("a model the catalogue does not hold is refused");
    assert!(err.to_string().contains("radar.imaginary"), "{err}");

    // A sensor placed in standby needs no model, and a laydown with nothing observing
    // is refused rather than reported as a rehearsal that saw nothing.
    let mut standby = laydown("standby", [500.0, 300.0, 20.0]);
    standby.sensors[0].mode = SensorMode::Standby;
    let err = run(
        &testdata_root(),
        TestTrackNumber(1),
        &standby,
        &[sensor(1, None)],
        &base_resources(),
    )
    .expect_err("nothing observes");
    assert!(
        matches!(err, RehearsalError::NothingObserves { .. }),
        "{err}"
    );
}

/// The scratch directories rehearsals of this process have left, by name.
fn rehearsal_scratch_dirs() -> Vec<String> {
    let pid = std::process::id().to_string();
    let mut names: Vec<String> = std::fs::read_dir(std::env::temp_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with("gungnir-rehearsal-TT-01-unmodelled") && n.contains(&pid))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn an_unknown_scenario_number_is_a_clear_error_not_a_panic() {
    let err = run(
        &testdata_root(),
        TestTrackNumber(99),
        &laydown("any", [0.0, 0.0, 0.0]),
        &[sensor(1, Some("radar.short"))],
        &base_resources(),
    )
    .expect_err("TT-99 does not exist");
    assert!(err.to_string().contains("metadata.json"), "{err}");
}

/// A raid down round 1's own upper Vell approach, written as a recording a rehearsal can
/// read, into a scratch testdata root beside the committed sensor catalogue.
///
/// **Why the test writes it.** No committed recording is a round-1 scenario: round 1's
/// harbour and plant sit at the recordings' origin under DN-32 §5.5's frame, and none of
/// the ten sample sets brings a target within 25 km of it before its excerpt ends, so
/// round 1's two short-range radars re-observe nothing of any of them (GAP-147). Three
/// one-way drones flying `round-1.json`'s declared approach, in round 1's own local
/// frame, at its declared 300 m, are the smallest recording that asks the row's question.
/// It is written by this test, never committed as a sample and never read by the
/// desktop.
fn write_round_1_recording(config: &ConfigBaseline) -> std::path::PathBuf {
    const TICK_S: f64 = 2.0;
    const DURATION_S: f64 = 900.0;
    const SPEED_MPS: f64 = 45.0;

    let root =
        std::env::temp_dir().join(format!("gungnir-round-1-recording-{}", std::process::id()));
    let set = root.join("tracks/samples/TT-11-sample");
    std::fs::create_dir_all(&set).expect("scratch recording directory");
    std::fs::copy(
        testdata_root().join("tracks/sensor-models.json"),
        root.join("tracks/sensor-models.json"),
    )
    .expect("the committed catalogue export copies");

    let origin = config.origin.expect("round 1 declares an origin");
    let origin = Geodetic {
        lat_rad: origin[0],
        lon_rad: origin[1],
        alt_m: origin[2],
    };
    let approach = &config
        .approaches
        .first()
        .expect("round 1 declares the upper Vell approach")
        .points;
    // Outer end first, then along the axis, then over the harbour.
    let mut path: Vec<[f64; 3]> = approach
        .iter()
        .map(|p| {
            let e = Wgs84::ecef_to_enu(
                Wgs84::geodetic_to_ecef(Geodetic {
                    lat_rad: p[0],
                    lon_rad: p[1],
                    alt_m: p[2],
                }),
                origin,
            );
            [e.e_m, e.n_m, 300.0]
        })
        .collect();
    path.push([0.0, 0.0, 300.0]);

    let mut truth = String::new();
    let mut entities = Vec::new();
    for (n, spawn) in [0.0f64, 90.0, 180.0].iter().enumerate() {
        let id = format!("R1-{:03}", n + 1);
        entities.push(serde_json::json!({
            "id": id, "spawn_s": spawn, "occluded_window_s": null,
            "signature": {"rcs": "small", "ir": null, "acoustic": null, "emission": "none"},
            "decoy": false, "adsb_intermittent": null, "ais_spoof_offset_m": null,
            "surface": false, "destroyed_at_s": null,
        }));
        let (mut leg, mut pos) = (1usize, path[0]);
        let mut t = 0.0f64;
        while t <= DURATION_S + 1e-9 {
            if t >= *spawn {
                let target = path[leg];
                let to = [target[0] - pos[0], target[1] - pos[1]];
                let dist = to[0].hypot(to[1]);
                let step = SPEED_MPS * TICK_S;
                let vel = if dist > 0.0 {
                    [SPEED_MPS * to[0] / dist, SPEED_MPS * to[1] / dist, 0.0]
                } else {
                    [0.0; 3]
                };
                if dist <= step {
                    pos = target;
                    leg += 1;
                } else {
                    pos = [pos[0] + vel[0] * TICK_S, pos[1] + vel[1] * TICK_S, 300.0];
                }
                let alive = leg < path.len();
                truth.push_str(
                    &serde_json::json!({
                        "t": (t * 1000.0).round() / 1000.0, "entity": id,
                        "pos": pos, "vel": vel, "alive": alive,
                    })
                    .to_string(),
                );
                truth.push('\n');
                if !alive {
                    break;
                }
            }
            t += TICK_S;
        }
    }
    let write = |name: &str, text: String| {
        std::fs::write(set.join(name), text).unwrap_or_else(|e| panic!("{name}: {e}"));
    };
    write("truth.jsonl", truth);
    write(
        "entities.json",
        serde_json::json!({"format": 1, "scenario": "TT-11", "entities": entities}).to_string(),
    );
    write(
        "environment.json",
        serde_json::json!({"format": 1, "scenario": "TT-11", "events": []}).to_string(),
    );
    write(
        "metadata.json",
        serde_json::json!({
            "scenario": "TT-11", "seed": 1111, "duration_s": DURATION_S,
            "truth_tick_s": TICK_S,
            "origin": {"lat": origin.lat_rad.to_degrees(), "lon": origin.lon_rad.to_degrees(), "alt_m": 0.0},
        })
        .to_string(),
    );
    root
}

/// **Round 1 laydown `c`** (DN-32 §10): rehearse `current` and `c` over the round-1
/// scenario. `c` re-sites S2 and keeps S1 where it was, so S2's per-sensor detections
/// differ and S1's are identical detection for detection -- D-74's per-sensor-and-target
/// streams at work -- and the comparison names S2 alone, as moved. `b`, which moves a
/// battery and no sensor, changes no sensor's detections at all.
///
/// The round-1 scenario is a raid down round 1's own declared approach
/// ([`write_round_1_recording`] says why the test writes it), rehearsed with round 1's
/// own baseline, laydowns and detection models from `testdata/usability/round-1.json`.
#[test]
fn round_1s_forward_radar_changes_its_own_detections_and_nothing_else() {
    let path = testdata_root().join("usability/round-1.json");
    let text = std::fs::read_to_string(&path).expect("round-1.json is committed");
    let config: ConfigBaseline = serde_json::from_str(&text).expect("round-1.json parses");
    gungnir_config::validate(&config).expect("the round-1 baseline is valid");
    let root = write_round_1_recording(&config);
    let laydown = |id: &str| {
        config
            .laydowns
            .iter()
            .find(|l| l.id.0 == id)
            .unwrap_or_else(|| panic!("round-1.json declares laydown {id}"))
            .clone()
    };
    let rehearse = |id: &str| {
        run(
            &root,
            TestTrackNumber(11),
            &laydown(id),
            &config.sensors,
            &config.resources,
        )
        .unwrap_or_else(|e| panic!("laydown {id} rehearses: {e}"))
    };
    let current = rehearse("current");
    let b = rehearse("b");
    let c = rehearse("c");
    let _ = std::fs::remove_dir_all(&root);

    let s = |r: &gungnir_app::laydown_rehearsal::RehearsalRecord, id: u32| {
        r.sensors
            .iter()
            .find(|x| x.sensor == SensorId(id))
            .unwrap_or_else(|| panic!("S{id} is placed"))
            .clone()
    };
    for r in [&current, &b, &c] {
        for id in [1, 2] {
            assert_eq!(
                s(r, id).detection_model.as_deref(),
                Some("radar.short"),
                "round 1 names each radar's detection model"
            );
        }
    }
    assert!(
        s(&current, 1).detections > 0,
        "S1 at the harbour sees the raid arrive"
    );
    assert!(
        s(&current, 2).detections > 0 && s(&c, 2).detections > 0,
        "S2 sees the raid from both sites: current {:?}, c {:?}",
        current.sensors,
        c.sensors
    );

    for (name, r) in [("current", &current), ("b", &b), ("c", &c)] {
        println!(
            "{name}: S1 {} detection(s), S2 {} detection(s), {} track(s)",
            s(r, 1).detections,
            s(r, 2).detections,
            r.tracks_formed
        );
    }
    let differ = sensors_that_differ(&current, &c).expect("one recording, comparable");
    assert_eq!(
        differ.iter().map(|d| d.sensor).collect::<Vec<_>>(),
        vec![SensorId(2)],
        "c re-sites S2 only, so S2's detections are the only ones that differ: \
         current {:?}, c {:?}",
        current.sensors,
        c.sensors
    );
    assert!(differ[0].moved, "and the comparison says S2 moved");
    assert_ne!(differ[0].detections.0, differ[0].detections.1);
    assert_eq!(
        s(&current, 1),
        s(&c, 1),
        "S1 stands in the same place under both, so its detections are identical"
    );
    assert_eq!(
        sensors_that_differ(&current, &b).expect("comparable"),
        Vec::new(),
        "b moves a battery and no sensor"
    );

    // PN-16's table reads the run, not the declared placements (GAP-105's action): the
    // three records on a round-1 desktop, and the row for `c` says how many more or
    // fewer detections it made than `current` and that they came from S2.
    let dir = std::env::temp_dir().join(format!("gungnir-round-1-table-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut desk_config = config.clone();
    desk_config.data_dir = dir.to_string_lossy().into_owned();
    let mut state = gungnir_app::state::AppState::with_config(desk_config).expect("starts");
    for r in [&current, &b, &c] {
        state.rehearsal_records.insert(r.laydown.clone(), r.clone());
    }
    let rows = match gungnir_app::sustainment::planning_rows(&state) {
        gungnir_app::sustainment::PlanningRows::Rows(rows) => rows,
        gungnir_app::sustainment::PlanningRows::Empty { reason } => {
            panic!("round 1 declares laydowns: {reason}")
        }
    };
    let row = |id: &str| {
        rows.iter()
            .find(|r| r.id.0 == id)
            .unwrap_or_else(|| panic!("a row for {id}"))
            .rehearsal
            .clone()
    };
    let total = |r: &gungnir_app::laydown_rehearsal::RehearsalRecord| -> i64 {
        r.sensors.iter().map(|s| s.detections as i64).sum()
    };
    assert!(matches!(
        row("current"),
        RowRehearsal::Rehearsed {
            versus_current: VersusCurrent::IsCurrent,
            ..
        }
    ));
    assert_eq!(
        row("c"),
        RowRehearsal::Rehearsed {
            scenario: TestTrackNumber(11),
            detections: total(&c) as usize,
            tracks_formed: c.tracks_formed,
            versus_current: VersusCurrent::Difference {
                detections: total(&c) - total(&current),
                sensors: vec![2],
            },
        }
    );
    assert!(matches!(
        row("b"),
        RowRehearsal::Rehearsed {
            versus_current: VersusCurrent::Difference { detections: 0, ref sensors },
            ..
        } if sensors.is_empty()
    ));
    state.select_laydown(LaydownId("c".into()));
    let RehearsalSection::Ran(summary) = state.rehearsal_section() else {
        panic!("c was rehearsed");
    };
    let delta = |id: u32| {
        summary
            .sensors
            .iter()
            .find(|s| s.sensor == id)
            .and_then(|s| s.delta_from_current)
    };
    assert_eq!(delta(1), Some(0), "S1 did not move");
    assert_eq!(
        delta(2),
        Some(s(&c, 2).detections as i64 - s(&current, 2).detections as i64)
    );
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}
