// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The inbound half of GAP-004's outbound round trip: a `TaskAck` read off a SAPIENT
//! feed reaches `AppState::sensors` through `crate::sapient::tick`.
//!
//! The wire `taskId` a real ack would carry is not hand-encoded here: it is produced by
//! `SapientTaskAdapter::issue`, the same encoder `gungnir-node` and a future desktop
//! wiring would use to send the task in the first place, called directly (standalone,
//! not attached to the registry) against the exact `SensorTask` `command_sensor_mode`
//! issues. Anything else would be testing this crate's decode against its own guess at
//! the encoding rather than against the real one.

use std::sync::{Arc, Mutex};

use gungnir_app::state::AppState;
use gungnir_app::{sapient, sustainment, update};
use gungnir_config::{
    ConfigBaseline, SapientFeedConfig, SapientNodeType, SapientSource, SensorConfig,
};
use gungnir_ingest::adapters::sapient::{TaskAckReport, TaskAckSink, TaskAckStatus};
use gungnir_model::{MissionTime, SensorId, SensorMode};
use gungnir_sensor_management::sapient_task::{SapientDestinations, SapientTaskAdapter};
use gungnir_sensor_management::{SensorCommand, SensorControl, SensorControlAdapter, SensorTask};

const ORIGIN: [f64; 3] = [51.0_f64.to_radians(), -1.0_f64.to_radians(), 0.0];

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-sapient-ack-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        sensors: vec![SensorConfig {
            id: 30,
            modality: "sapient".into(),
            position: ORIGIN,
            max_range_m: 2_000.0,
            control_endpoint: Some("spotter-30-control".into()),
            maintenance: Vec::new(),
        }],
        endpoints: vec![gungnir_config::EndpointConfig {
            name: "spotter-30-control".into(),
            kind: "sensor-control".into(),
            address: "tcp://127.0.0.1:9200".into(),
        }],
        sapient_feeds: vec![SapientFeedConfig {
            name: "spotter-30".into(),
            sensor_id: 30,
            node_type: SapientNodeType::Spotter,
            source: SapientSource::File {
                path: dir
                    .join("no-detections.jsonl")
                    .to_string_lossy()
                    .into_owned(),
            },
            destination_id: None,
        }],
        sensor_task_ack_window_s: 10.0,
        ..ConfigBaseline::default()
    };
    std::fs::write(dir.join("no-detections.jsonl"), "").expect("empty feed file");
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("desktop state"), dir)
}

/// The wire `taskId` a real acknowledgement of `local` would name, produced by the same
/// encoder a real outbound wiring would use.
fn wire_task_id(local: gungnir_sensor_management::SensorTaskId, sensor: SensorId) -> String {
    let captured: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink_handle = captured.clone();
    let adapter = SapientTaskAdapter::new(
        "desktop-under-test",
        SapientDestinations::from_sensors([(
            sensor.0,
            "b5546692-a0cc-4846-a36c-4b7098eae08e".into(),
        )]),
        move |message: String| sink_handle.lock().expect("not poisoned").push(message),
    );
    let task = SensorTask {
        id: local,
        sensor,
        command: SensorCommand::SetMode {
            mode: SensorMode::Search,
        },
        requirement: None,
        issued: MissionTime(0.0),
        state: gungnir_sensor_management::TaskState::Issued,
    };
    adapter
        .issue(&task)
        .expect("a destination is configured for this sensor");
    let messages = captured.lock().expect("not poisoned");
    let wire: serde_json::Value = serde_json::from_str(&messages[0]).expect("issue() emits JSON");
    wire["task"]["taskId"]
        .as_str()
        .expect("issue() always names a taskId")
        .to_owned()
}

fn push_ack(state: &mut AppState, task_id: &str, status: TaskAckStatus, reasons: &[&str]) {
    let sink = TaskAckSink::default();
    sink.lock().expect("not poisoned").push_back(TaskAckReport {
        task_id: task_id.to_owned(),
        status,
        reasons: reasons.iter().map(|s| (*s).to_owned()).collect(),
    });
    state.sapient_task_acks.push(sink);
}

#[test]
fn an_accepted_ack_acknowledges_the_real_issued_task() {
    let (mut state, dir) = desktop("accepted");
    sustainment::command_sensor_mode(&mut state, 30, SensorMode::Search).expect("issued");
    let local = state.sensors.tasks()[0].id;
    let wire_id = wire_task_id(local, SensorId(30));

    push_ack(&mut state, &wire_id, TaskAckStatus::Accepted, &[]);
    update::tick(&mut state);

    assert!(
        matches!(
            state.sensors.tasks()[0].state,
            gungnir_sensor_management::TaskState::Acknowledged { .. }
        ),
        "{:?}",
        state.sensors.tasks()[0].state
    );
    assert_eq!(
        state.sensors.mode_status(SensorId(30)).map(|m| m.confirmed),
        Some(SensorMode::Search),
        "acknowledging is the only path that moves the confirmed mode"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_rejected_ack_fails_the_task_and_keeps_its_reason() {
    let (mut state, dir) = desktop("rejected");
    sustainment::command_sensor_mode(&mut state, 30, SensorMode::Search).expect("issued");
    let local = state.sensors.tasks()[0].id;
    let wire_id = wire_task_id(local, SensorId(30));

    push_ack(
        &mut state,
        &wire_id,
        TaskAckStatus::Rejected,
        &["no line of sight to the cued point"],
    );
    update::tick(&mut state);

    match &state.sensors.tasks()[0].state {
        gungnir_sensor_management::TaskState::Failed { reason } => {
            assert!(reason.contains("no line of sight"), "{reason}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(
        state.sensors.mode_status(SensorId(30)).map(|m| m.confirmed),
        Some(SensorMode::Standby),
        "a rejected command must not move the confirmed mode"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// `Completed` and `Failed`-after-acceptance have nowhere in `SensorControl`'s task
/// lifecycle to go once a task is already `Acknowledged` -- recorded as an alert, and
/// the task record itself is left exactly as the acceptance left it.
#[test]
fn a_completed_ack_after_acceptance_is_alerted_rather_than_forced_through_a_second_transition() {
    let (mut state, dir) = desktop("completed");
    sustainment::command_sensor_mode(&mut state, 30, SensorMode::Search).expect("issued");
    let local = state.sensors.tasks()[0].id;
    let wire_id = wire_task_id(local, SensorId(30));

    push_ack(&mut state, &wire_id, TaskAckStatus::Accepted, &[]);
    update::tick(&mut state);
    push_ack(&mut state, &wire_id, TaskAckStatus::Completed, &[]);
    update::tick(&mut state);

    assert!(
        matches!(
            state.sensors.tasks()[0].state,
            gungnir_sensor_management::TaskState::Acknowledged { .. }
        ),
        "a Completed ack changed the task record: {:?}",
        state.sensors.tasks()[0].state
    );
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("completed") && a.contains(&local.0.to_string())),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A `task_id` this desktop never minted decodes to *something*, but not to a task in
/// the record -- alerted, not silently ignored and not applied to an unrelated task.
#[test]
fn an_unrecognised_task_id_is_alerted_and_changes_nothing() {
    let (mut state, dir) = desktop("unrecognised");
    sustainment::command_sensor_mode(&mut state, 30, SensorMode::Search).expect("issued");

    push_ack(
        &mut state,
        "not-a-real-ulid-at-all-000",
        TaskAckStatus::Accepted,
        &[],
    );
    update::tick(&mut state);

    assert!(
        matches!(
            state.sensors.tasks()[0].state,
            gungnir_sensor_management::TaskState::Issued
        ),
        "an unrelated TaskAck moved the real task's state: {:?}",
        state.sensors.tasks()[0].state
    );
    assert!(!state.alerts.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// `sapient::tick` with no `TaskAck` sinks attached does nothing and does not panic --
/// the state before any SAPIENT feed is bound.
#[test]
fn tick_with_no_sinks_is_a_no_op() {
    let (mut state, dir) = desktop("no-sinks");
    state.sapient_task_acks.clear();
    sapient::tick(&mut state);
    assert!(state.sensors.tasks().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
