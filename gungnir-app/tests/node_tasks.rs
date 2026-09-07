// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Sensor tasks through the node (GAP-004, DN-11 §5) and effector reports applied
//! (GAP-040): a command issued on a linked desktop is delivered by the link, mapped to
//! the node's task id, and closed only by what the node streams back; a report on a
//! handoff this desktop issued changes the record, and one on a decision it did not is
//! rejected. The warned party's acknowledgement (GAP-042, DN-03 §5 rule 2) travels the
//! same way and is applied here for the same reason: the node holds neither record.

use gungnir_app::state::AppState;
use gungnir_app::{handoffs, node_tasks, sustainment, update};
use gungnir_config::{ConfigBaseline, EndpointConfig, SensorConfig};
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::SensorTaskEvent;
use gungnir_model::handoff::{DecisionAttribution, DeliveryState, EffectorReport, Handoff};
use gungnir_model::{
    DecisionId, MissionTime, PlanId, PlanKind, Releasability, SensorId, SensorMode, SensorTaskId,
};
use gungnir_remote::link::{NodeLink, TaskOutcome};
use gungnir_security::AuditLog;
use gungnir_sensor_management::tasking::TaskState;
use gungnir_sensor_management::{SensorControl, SensorRegistry};
use gungnir_time::ReplayClockAuthority;

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-node-tasks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        sensors: vec![SensorConfig {
            id: 1,
            modality: "radar".into(),
            position: [0.0, 0.0, 0.0],
            max_range_m: 50_000.0,
            control_endpoint: Some("radar-1-control".into()),
            maintenance: Vec::new(),
        }],
        endpoints: vec![EndpointConfig {
            name: "radar-1-control".into(),
            kind: "sensor-control".into(),
            address: "tcp://127.0.0.1:9100".into(),
        }],
        sensor_task_ack_window_s: 10.0,
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("desktop state");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    (state, dir)
}

fn mode_of(state: &AppState) -> SensorMode {
    state
        .sensors
        .sensors()
        .iter()
        .find(|s| s.id == SensorId(1))
        .expect("sensor 1")
        .mode
}

fn task_state(state: &AppState, id: SensorTaskId) -> TaskState {
    state
        .sensors
        .tasks()
        .iter()
        .find(|t| t.id == id)
        .expect("the task is recorded")
        .state
        .clone()
}

/// The whole path on a scripted link: issued locally, queued on the link, mapped to
/// the node's id, and confirmed only by the node's acknowledgement.
#[test]
fn a_command_travels_through_the_link_and_is_confirmed_by_the_node() {
    let (mut state, dir) = desktop("acknowledged");
    let link = NodeLink::scripted();
    state.link = Some(link.clone());
    node_tasks::attach(&mut state, link.clone());
    let before = mode_of(&state);

    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("issued");
    let local = state.sensors.tasks()[0].id;
    assert_eq!(task_state(&state, local), TaskState::Sent);
    assert_eq!(mode_of(&state), before, "nothing is confirmed by issuing");
    {
        let p = link.read().expect("projection");
        assert_eq!(p.task_outbox.len(), 1);
        assert_eq!(p.task_outbox[0].local, local);
        assert_eq!(p.task_outbox[0].sensor, SensorId(1));
    }

    // The node answered with its own id.
    if let Some(mut p) = link.read() {
        p.task_outbox.clear();
        p.task_outcomes.push(TaskOutcome {
            local,
            outcome: Ok(SensorTaskId(77)),
        });
    }
    update::tick(&mut state);
    assert_eq!(state.node_task_map.get(&SensorTaskId(77)), Some(&local));
    assert_eq!(task_state(&state, local), TaskState::Sent);

    // The sensor acknowledged, naming the node's id.
    if let Some(mut p) = link.read() {
        p.inbox.push_back(Envelope {
            seq: 5,
            mission_time: MissionTime(101.0),
            event: Event::SensorTask(SensorTaskEvent::Acknowledged {
                task: SensorTaskId(77),
                sensor: SensorId(1),
                at: MissionTime(101.0),
            }),
        });
    }
    update::tick(&mut state);
    assert!(matches!(
        task_state(&state, local),
        TaskState::Acknowledged { .. }
    ));
    assert_eq!(mode_of(&state), SensorMode::Search);
    let _ = std::fs::remove_dir_all(dir);
}

/// The node refused, or the sensor did: the task fails with the reason and the mode
/// stays where it was.
#[test]
fn a_refusal_from_the_node_fails_the_task_with_its_words() {
    let (mut state, dir) = desktop("refused");
    let link = NodeLink::scripted();
    state.link = Some(link.clone());
    node_tasks::attach(&mut state, link.clone());
    let before = mode_of(&state);
    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("issued");
    let local = state.sensors.tasks()[0].id;
    if let Some(mut p) = link.read() {
        p.task_outbox.clear();
        p.task_outcomes.push(TaskOutcome {
            local,
            outcome: Err("the node answered 409: sensor 1 has no control adapter".into()),
        });
    }
    update::tick(&mut state);
    match task_state(&state, local) {
        TaskState::Failed { reason } => assert!(reason.contains("409"), "{reason}"),
        other => panic!("expected failed, got {other:?}"),
    }
    assert_eq!(mode_of(&state), before);
    let _ = std::fs::remove_dir_all(dir);
}

/// With the link gone the adapter refuses at the door, with the reason, and the task is
/// recorded failed rather than left to time out.
#[test]
fn without_a_link_the_command_fails_at_the_door() {
    let (mut state, dir) = desktop("no-link");
    let link = NodeLink::scripted();
    node_tasks::attach(&mut state, link);
    node_tasks::detach(&mut state);
    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("recorded");
    match &state.sensors.tasks()[0].state {
        TaskState::Failed { reason } => assert!(reason.contains("no node link"), "{reason}"),
        other => panic!("expected failed, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn handoff(decision: u64) -> Handoff {
    Handoff::from_decision(
        DecisionId(decision),
        PlanId(decision),
        PlanKind::Intercept {
            solutions: Vec::new(),
        },
        DecisionAttribution {
            operator: "7".into(),
            role: "Supervisor".into(),
            at: MissionTime(90.0),
            authority_rule: None,
        },
        Vec::new(),
        Releasability::Internal,
        MissionTime(90.0),
    )
}

/// An effector's report on a handoff this desktop issued changes the record; one on a
/// decision it did not is rejected and said.
#[test]
fn an_effector_report_is_applied_to_a_known_handoff_and_rejected_otherwise() {
    let (mut state, dir) = desktop("report");
    state.handoffs.push(handoffs::HandoffRecord {
        handoff: handoff(9),
        endpoint: Some("battery".into()),
        delivery: DeliveryState::Delivered {
            at: MissionTime(91.0),
        },
        reports: Vec::new(),
    });
    handoffs::apply_report(
        &mut state,
        DecisionId(9),
        "battery",
        &EffectorReport::Refused {
            at: MissionTime(95.0),
            reason: "out of range".into(),
        },
        MissionTime(95.0),
    );
    assert!(matches!(
        state.handoffs[0].delivery,
        DeliveryState::Refused { .. }
    ));
    assert!(state
        .alerts
        .iter()
        .any(|a| a.contains("refused the handoff")));
    assert!(state
        .audit
        .entries()
        .iter()
        .any(|e| e.action == gungnir_security::actions::EFFECTOR_REPORT));

    let alerts_before = state.alerts.len();
    handoffs::apply_report(
        &mut state,
        DecisionId(404),
        "battery",
        &EffectorReport::Acknowledged {
            at: MissionTime(96.0),
        },
        MissionTime(96.0),
    );
    assert!(state.alerts[alerts_before..]
        .iter()
        .any(|a| a.contains("rejected") && a.contains("404")));
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-042, DN-03 §5 rule 2: the warned party's acknowledgement, arriving through the node
/// that holds no ledger, discharges the warning on the desktop that raised it. One naming
/// a pair this desktop never raised is rejected and said, as an untrusted external input
/// is -- the node could not know, and discarding it silently would leave the party
/// believing it had answered.
#[test]
fn an_acknowledgement_discharges_a_sent_warning_and_an_unknown_pair_is_rejected() {
    use gungnir_assessment::AssetExposure;
    use gungnir_model::{
        AssetExtent, AssetId, AssetPriority, DefendedAsset, Geodetic, TrackId, WarningObligation,
    };
    use gungnir_workflow::warning::{Warning, WarningDelivery, WarningState};

    struct Takes;
    impl WarningDelivery for Takes {
        fn deliver(&self, _: &Warning) -> Result<(), String> {
            Ok(())
        }
    }

    let (mut state, dir) = desktop("acknowledge");
    let assets = vec![DefendedAsset {
        id: AssetId(1),
        name: "the harbour".into(),
        extent: AssetExtent::Point {
            position: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
        },
        priority: AssetPriority::High,
        warning: Some(WarningObligation {
            lead_time_s: 120.0,
            channel: "port-authority".into(),
            within_m: None,
        }),
        note: None,
    }];
    let exposures = vec![(
        TrackId(7),
        AssetExposure {
            asset: AssetId(1),
            range_m: 1_000.0,
            time_to_impact_s: Some(60.0),
            closest_approach_m: None,
            time_to_closest_approach_s: None,
        },
    )];
    state
        .warnings
        .evaluate(MissionTime(100.0), &assets, &exposures, &Takes);
    assert!(
        matches!(state.warnings.open()[0].state, WarningState::Sent { .. }),
        "{:?}",
        state.warnings.open()[0].state
    );

    gungnir_app::warnings::apply_acknowledgement(
        &mut state,
        AssetId(1),
        TrackId(7),
        "port-authority",
        MissionTime(105.0),
    );
    assert_eq!(
        state.warnings.open()[0].state,
        WarningState::Acknowledged {
            at: MissionTime(105.0)
        },
        "the party's own claimed time is what the record carries"
    );
    assert!(state
        .alerts
        .iter()
        .any(|a| a.contains("acknowledged by port-authority")));
    assert!(state
        .audit
        .entries()
        .iter()
        .any(|e| e.action == gungnir_security::actions::ACKNOWLEDGE_WARNING));

    let alerts_before = state.alerts.len();
    gungnir_app::warnings::apply_acknowledgement(
        &mut state,
        AssetId(1),
        TrackId(404),
        "port-authority",
        MissionTime(106.0),
    );
    assert!(state.alerts[alerts_before..]
        .iter()
        .any(|a| a.contains("no warning open for that pair")));
    let _ = std::fs::remove_dir_all(dir);
}
