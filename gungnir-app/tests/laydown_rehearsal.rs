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
use gungnir_coord::{CoordTransform, Enu, Geodetic, Wgs84};
use gungnir_model::laydown::{Laydown, LaydownId, ResourcePlacement, SensorPlacement};
use gungnir_model::{MissionTime, ResourceId, SensorId, SensorMode, TestTrackNumber};
use gungnir_ui::panels::planning::{
    ApproachEngagement, RehearsalFirstEngagement, RehearsalSection, RowFirstEngagement,
    RowRehearsal, VersusCurrent,
};

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
            elevation_band: None,
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
        MissionTime(0.0),
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
        MissionTime(0.0),
    )
    .expect("runs");
    let second = run(
        &testdata_root(),
        TestTrackNumber(1),
        &l,
        &sensors,
        &base_resources(),
        MissionTime(0.0),
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
        MissionTime(0.0),
    )
    .expect("runs");
    let far = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown("far", [-250_000.0, 250_000.0, 450.0]),
        &sensors,
        &base_resources(),
        MissionTime(0.0),
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
        MissionTime(0.0),
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
        MissionTime(0.0),
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
        MissionTime(0.0),
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
        MissionTime(0.0),
    )
    .expect_err("TT-99 does not exist");
    assert!(err.to_string().contains("metadata.json"), "{err}");
}

/// **Round 1 laydown `c`** (DN-32 §10): rehearse `current` and `c` over the round-1
/// scenario. `c` re-sites S2 and keeps S1 where it was, so S2's per-sensor detections
/// differ and S1's are identical detection for detection -- D-74's per-sensor-and-target
/// streams at work -- and the comparison names S2 alone, as moved. `b`, which moves a
/// battery and no sensor, changes no sensor's detections at all.
///
/// The round-1 scenario is TT-11, the committed recording of a raid down round 1's own
/// declared approach (GAP-147, D-112), generated by both generators and validated like
/// every other sample set, rehearsed with round 1's own baseline, laydowns and detection
/// models from `testdata/usability/round-1.json` -- exactly what a US-15 session runs.
/// The ten plan-07 recordings bring no target within 25 km of round 1's radars before
/// they end, which is why TT-11 exists: [`no_plan_07_recording_reaches_round_1s_radars`].
#[test]
fn round_1s_forward_radar_changes_its_own_detections_and_nothing_else() {
    let path = testdata_root().join("usability/round-1.json");
    let text = std::fs::read_to_string(&path).expect("round-1.json is committed");
    let config: ConfigBaseline = serde_json::from_str(&text).expect("round-1.json parses");
    gungnir_config::validate(&config).expect("the round-1 baseline is valid");
    let root = testdata_root();
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
            MissionTime(0.0),
        )
        .unwrap_or_else(|e| panic!("laydown {id} rehearses: {e}"))
    };
    let current = rehearse("current");
    let b = rehearse("b");
    let c = rehearse("c");

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
            ran_at: MissionTime(0.0),
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

    // GAP-020, D-45: the upper Vell approach's first engagement, per laydown -- the
    // comparison US-15 makes, now over the committed TT-11 rather than a raid this test
    // wrote (GAP-147, D-112). Each run's worst case is over the raid's six drones, and over
    // nothing else: round 1's short-range radars form tracks from their false alarms beside
    // the harbour and the planner pairs them, and those are clutter, counted and never
    // measured (D-107), or every laydown's worst case would be a false alarm a few metres
    // from a radar. `c` sites S2 10 km up the approach, so it first engages farther out.
    // `b` moves the area-layer battery toward the approach: its sensors see exactly what
    // `current`'s do, but an effector that stands elsewhere predicts its intercepts
    // elsewhere, so its worst case may differ, and the row says by how much.
    let upper_vell = |id: &str| match &rows
        .iter()
        .find(|r| r.id.0 == id)
        .unwrap_or_else(|| panic!("a row for {id}"))
        .first_engagement
    {
        RowFirstEngagement::PerApproach(cells) => match cells.as_slice() {
            [ApproachEngagement::WorstCase {
                range_m,
                predictions,
                target,
                versus_current_m,
                ..
            }] => (*range_m, *predictions, target.clone(), *versus_current_m),
            other => panic!("{id}: {other:?}"),
        },
        other => panic!("{id}'s first engagement was not computed: {other:?}"),
    };
    for r in [&current, &b, &c] {
        assert!(
            r.clutter_pairings > 0,
            "{}: the harbour radars' false alarms form tracks the planner pairs",
            r.laydown
        );
    }
    let (current_m, current_n, current_target, _) = upper_vell("current");
    let (b_m, b_n, b_target, b_versus) = upper_vell("b");
    let (c_m, c_n, _, c_versus) = upper_vell("c");
    println!("upper Vell: current {current_m:.0} m, b {b_m:.0} m, c {c_m:.0} m");
    assert_eq!(
        (current_n, b_n, c_n),
        (6, 6, 6),
        "one prediction per recorded drone"
    );
    assert!(current_target.starts_with("TT11-raid-"), "{current_target}");
    assert!(b_target.starts_with("TT11-raid-"), "{b_target}");
    assert!(
        c_m > current_m,
        "S2 forward-sited first engages farther out: {c_m} vs {current_m}"
    );
    assert!((c_versus.expect("comparable") - (c_m - current_m)).abs() < 1e-9);
    assert!((b_versus.expect("comparable") - (b_m - current_m)).abs() < 1e-9);

    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

/// TT-01's inbound axis from the east-south-east: its south and north streams launch
/// near (120 km, -80 km) and fly toward the origin (`scenarios.yaml`). Outer end first,
/// inner end last, as DN-02 §9 reads an approach.
const TT01_EAST_AXIS: [[f64; 3]; 2] = [[125_000.0, -83_000.0, 1_000.0], [0.0, 0.0, 1_000.0]];

/// TT-01's sea stream's axis, from about (-90 km, 20 km). The ridge radar re-observes
/// nothing of it in this excerpt, so no track comes down it.
const TT01_SEA_AXIS: [[f64; 3]; 2] = [[-95_000.0, 21_000.0, 1_000.0], [0.0, 0.0, 1_000.0]];

/// A deployment declaring TT-01's two approach axes, a long-range radar, one effector
/// with a closing speed, and two laydowns: `current`, the effector at home, and
/// `forward`, the same effector 72 km up the eastern axis. The deployment's origin is
/// round 1's, not the recording's, so the test also holds DN-32 §5.5's frame: a laydown
/// and its approaches are read as an arrangement about the recording's origin.
fn first_engagement_deployment(dir: &std::path::Path) -> ConfigBaseline {
    let origin = Geodetic {
        lat_rad: 0.959_931,
        lon_rad: 0.209_44,
        alt_m: 0.0,
    };
    let geodetic = |p: [f64; 3]| {
        let g = Wgs84::ecef_to_geodetic(Wgs84::enu_to_ecef(
            Enu {
                e_m: p[0],
                n_m: p[1],
                u_m: p[2],
            },
            origin,
        ));
        [g.lat_rad, g.lon_rad, g.alt_m]
    };
    let approach = |name: &str, axis: [[f64; 3]; 2]| gungnir_config::ApproachConfig {
        name: name.into(),
        points: axis.iter().map(|p| geodetic(*p)).collect(),
        corridor_half_width_m: Some(15_000.0),
    };
    let placed = |id: &str, resource_enu: [f64; 3], current: bool| Laydown {
        id: LaydownId(id.into()),
        intent: "a first-engagement fixture".into(),
        sensors: vec![SensorPlacement {
            sensor: SensorId(1),
            position_enu: RIDGE,
            mode: SensorMode::Search,
            azimuth_sector: None,
            elevation_band: None,
        }],
        resources: vec![ResourcePlacement {
            resource: ResourceId(1),
            position_enu: resource_enu,
        }],
        current,
    };
    let config = ConfigBaseline {
        origin: Some([origin.lat_rad, origin.lon_rad, origin.alt_m]),
        sensors: vec![sensor(1, Some("radar.long"))],
        resources: serde_json::from_value(serde_json::json!([{
            "id": 1, "position": [origin.lat_rad, origin.lon_rad, 0.0], "capacity": 2,
            "layer": "point", "intercept_speed_mps": 250.0
        }]))
        .expect("the fixture resource parses"),
        approaches: vec![
            approach("eastern approach", TT01_EAST_AXIS),
            approach("sea approach", TT01_SEA_AXIS),
        ],
        laydowns: vec![
            placed("current", [1_000.0, 500.0, 0.0], true),
            placed("forward", [60_000.0, -40_000.0, 0.0], false),
        ],
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("the first-engagement fixture is a valid baseline");
    config
}

/// **GAP-020, D-45: each approach's first-engagement range is the worst case over a
/// rehearsal of a committed recording, per laydown, with its count and provenance, and
/// an approach nothing came down is not computable rather than zero.**
///
/// TT-01 re-observed with a ridge radar: the planner pairs tracks of the raid's targets
/// with the one effector as they come in from the east, and each target's first pairing
/// predicts an intercept point (D-107). Moving the effector 72 km up the eastern axis moves
/// the first engagements outward, so `forward`'s worst case is farther out than
/// `current`'s, and the row says by how much in words. The sea approach, down which the
/// radar sees nothing in this excerpt, says so on both rows.
#[test]
fn first_engagement_is_the_worst_case_over_a_committed_recording_per_laydown() {
    let dir = std::env::temp_dir().join(format!("gungnir-first-engagement-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = first_engagement_deployment(&dir);
    let laydown = |id: &str| {
        config
            .laydowns
            .iter()
            .find(|l| l.id.0 == id)
            .unwrap_or_else(|| panic!("laydown {id}"))
            .clone()
    };
    let rehearse = |id: &str, ran_at: f64| {
        run(
            &testdata_root(),
            TestTrackNumber(1),
            &laydown(id),
            &config.sensors,
            &config.resources,
            MissionTime(ran_at),
        )
        .unwrap_or_else(|e| panic!("laydown {id} rehearses: {e}"))
    };
    let current = rehearse("current", 100.0);
    let forward = rehearse("forward", 200.0);

    // A measurement of the recording, not of the machine (D-109): the same laydown
    // twice proposes the same plans, so the same first pairings.
    assert_eq!(current, rehearse("current", 100.0));
    assert!(
        current
            .first_pairings
            .iter()
            .any(|p| p.engagement.is_some()),
        "the planner predicted intercepts for TT-01's raid: {:?}",
        current.first_pairings
    );
    assert_eq!(current.ran_at, MissionTime(100.0));

    let mut state =
        gungnir_app::state::AppState::with_config(config.clone()).expect("the desktop starts");
    for r in [&current, &forward] {
        state.rehearsal_records.insert(r.laydown.clone(), r.clone());
    }
    let rows = match gungnir_app::sustainment::planning_rows(&state) {
        gungnir_app::sustainment::PlanningRows::Rows(rows) => rows,
        gungnir_app::sustainment::PlanningRows::Empty { reason } => panic!("{reason}"),
    };
    let cells = |id: &str| match &rows
        .iter()
        .find(|r| r.id.0 == id)
        .unwrap_or_else(|| panic!("a row for {id}"))
        .first_engagement
    {
        RowFirstEngagement::PerApproach(cells) => cells.clone(),
        other => panic!("{id}'s first engagement was not computed: {other:?}"),
    };
    let worst = |e: &ApproachEngagement| match e {
        ApproachEngagement::WorstCase {
            range_m,
            predictions,
            versus_current_m,
            ..
        } => (*range_m, *predictions, *versus_current_m),
        ApproachEngagement::NotComputable { reason } => panic!("not computable: {reason}"),
    };
    let (current_m, current_n, current_versus) = worst(&cells("current")[0]);
    let (forward_m, forward_n, forward_versus) = worst(&cells("forward")[0]);
    println!(
        "eastern approach: current worst {current_m:.0} m over {current_n}, forward worst \
         {forward_m:.0} m over {forward_n}"
    );

    // The worst case, labelled with its count: a count is never zero and never more
    // than the tracks first paired.
    assert!(current_m.is_finite() && current_m > 0.0);
    assert!((1..=current.first_pairings.len()).contains(&current_n));
    assert!((1..=forward.first_pairings.len()).contains(&forward_n));
    // The comparison US-15 makes: the forward effector first engages farther out, and
    // the row says by how much against the current laydown, which has no difference of
    // its own.
    assert!(
        forward_m > current_m,
        "the forward effector should first engage farther out: {forward_m} vs {current_m}"
    );
    assert_eq!(current_versus, None);
    let delta = forward_versus.expect("both rehearsed TT-01, so comparable");
    assert!((delta - (forward_m - current_m)).abs() < 1e-9);

    // The worst case is the minimum over the run, not a mean: no prediction on the
    // approach is nearer in than it.
    let summary = gungnir_app::sustainment::rehearsal_first_engagement(&state, &current)
        .expect("approaches declared, origin declared");
    assert_eq!(summary.approaches[0].range_m(), Some(current_m));

    // An approach nothing came down is not computable, with the reason, on every row.
    for id in ["current", "forward"] {
        match &cells(id)[1] {
            ApproachEngagement::NotComputable { reason } => {
                assert!(
                    reason.contains("no recorded target on this approach"),
                    "{reason}"
                );
            }
            e @ ApproachEngagement::WorstCase { .. } => {
                panic!("the sea approach had nothing come down it, yet {id} shows {e:?}")
            }
        }
    }

    // The rehearsal section carries the provenance: when the run was made, and the
    // track and time behind the worst case.
    state.select_laydown(LaydownId("forward".into()));
    let RehearsalSection::Ran(summary) = state.rehearsal_section() else {
        panic!("forward was rehearsed");
    };
    assert_eq!(summary.ran_at, MissionTime(200.0));
    assert_eq!(summary.scenario, TestTrackNumber(1));
    assert_eq!(summary.seed, 1701);
    let RehearsalFirstEngagement::PerApproach { lines, .. } = &summary.first_engagement else {
        panic!("approaches are declared: {:?}", summary.first_engagement);
    };
    assert_eq!(lines[0].approach, "eastern approach");
    assert_eq!(lines[0].corridor_half_width_m, Some(15_000.0));
    let ApproachEngagement::WorstCase {
        track, proposed_at, ..
    } = &lines[0].engagement
    else {
        panic!("{:?}", lines[0].engagement);
    };
    let behind = forward
        .first_pairings
        .iter()
        .find(|p| p.track == *track)
        .expect("the worst case names a track the run paired");
    assert_eq!(behind.proposed_at, *proposed_at);
    assert!(behind.engagement.is_some());

    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Round 1's committed baseline against a plan-07 recording: not computable, and
/// why** (GAP-020). None of the ten plan-07 recordings brings a target within reach of
/// round 1's short-range radars -- TT-11, round 1's own, is the one that does, and
/// [`round_1s_forward_radar_changes_its_own_detections_and_nothing_else`] computes the
/// column over it (GAP-147, D-112) -- so over TT-01 the only tracks the run forms are the
/// radars' own false alarms beside the harbour. The planner pairs them; they are clutter, counted and
/// never measured (D-107), and the upper Vell approach's first engagement reads not
/// computable on every row -- never a range of a few metres from a radar, and never zero.
#[test]
fn round_1_against_a_plan_07_recording_is_not_computable_and_says_why() {
    let path = testdata_root().join("usability/round-1.json");
    let text = std::fs::read_to_string(&path).expect("round-1.json is committed");
    let mut config: ConfigBaseline = serde_json::from_str(&text).expect("round-1.json parses");
    let dir = std::env::temp_dir().join(format!("gungnir-round-1-fe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    config.data_dir = dir.to_string_lossy().into_owned();
    let current = config
        .laydowns
        .iter()
        .find(|l| l.current)
        .expect("round 1 marks a current laydown")
        .clone();
    let record = run(
        &testdata_root(),
        TestTrackNumber(1),
        &current,
        &config.sensors,
        &config.resources,
        MissionTime(0.0),
    )
    .expect("round 1's current laydown rehearses against TT-01");
    assert!(
        record.first_pairings.is_empty(),
        "{:?}",
        record.first_pairings
    );
    assert!(
        record.clutter_pairings > 0,
        "the harbour radars' false alarms form tracks the planner pairs"
    );

    let mut state = gungnir_app::state::AppState::with_config(config).expect("starts");
    state
        .rehearsal_records
        .insert(current.id.clone(), record.clone());
    let rows = match gungnir_app::sustainment::planning_rows(&state) {
        gungnir_app::sustainment::PlanningRows::Rows(rows) => rows,
        gungnir_app::sustainment::PlanningRows::Empty { reason } => panic!("{reason}"),
    };
    for row in &rows {
        let expected = if row.id == current.id {
            RowFirstEngagement::PerApproach(vec![ApproachEngagement::NotComputable {
                reason: "no recorded target on this approach was engaged in the run".into(),
            }])
        } else {
            RowFirstEngagement::NotRehearsed
        };
        assert_eq!(row.first_engagement, expected, "{}", row.id);
    }
    state.select_laydown(current.id.clone());
    let RehearsalSection::Ran(summary) = state.rehearsal_section() else {
        panic!("current was rehearsed");
    };
    assert!(matches!(
        summary.first_engagement,
        RehearsalFirstEngagement::PerApproach { clutter_pairings, .. }
            if clutter_pairings == record.clutter_pairings
    ));
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

/// GAP-147: why round 1 needs TT-11. Rehearsing round 1's `current` laydown over each of
/// the ten plan-07 recordings, both radars re-observe nothing of a recorded target -- the
/// nearest any of them brings a target to round 1's harbour is about 25 km -- and over
/// TT-11 both see the raid. A session's rehearsal therefore has something to compare only
/// when it picks TT-11, which US-15's card names.
#[test]
fn no_plan_07_recording_reaches_round_1s_radars() {
    let text = std::fs::read_to_string(testdata_root().join("usability/round-1.json"))
        .expect("round-1.json is committed");
    let config: ConfigBaseline = serde_json::from_str(&text).expect("round-1.json parses");
    let current = config
        .laydowns
        .iter()
        .find(|l| l.current)
        .expect("round 1 marks a current laydown")
        .clone();
    let detections = |scenario: TestTrackNumber| -> usize {
        run(
            &testdata_root(),
            scenario,
            &current,
            &config.sensors,
            &config.resources,
            MissionTime(0.0),
        )
        .unwrap_or_else(|e| panic!("{} rehearses: {e}", scenario.label()))
        .sensors
        .iter()
        .map(|s| s.detections)
        .sum()
    };
    for n in 1..=10 {
        let scenario = TestTrackNumber(n);
        assert_eq!(
            detections(scenario),
            0,
            "{} reaches round 1's radars; GAP-147's premise no longer holds",
            scenario.label()
        );
    }
    assert!(detections(TestTrackNumber(11)) > 0, "TT-11 reaches them");
}
