// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-207: a desktop with no node hands an engagement to a SAPIENT effector, and learns
//! the outcome by data (plan 12 tranche A; D-144).
//!
//! Before GAP-207 a handoff from a disconnected desktop was a generic HTTP post or a radio
//! call, and no effector outcome ever came back: the report route lives on the node, and
//! the desktop binds no listener. Here the resource's endpoint is a SAPIENT effector behind
//! a middleware on the loopback. The decision goes out as a `Task` asking it to engage at
//! the intercept point, the effector's `TaskAck`s come back on the connection the desktop
//! opened, and the handoff record PN-06 and PN-20 draw holds what it said.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use gungnir_app::state::AppState;
use gungnir_app::{decisions, update};
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::{
    ConfigBaseline, EndpointConfig, ResourceConfig, SapientEffectorConfig, SapientEffectorType,
};
use gungnir_model::handoff::{DeliveryState, EffectorReport};
use gungnir_model::{
    Classification, DetectionView, EffectorLayer, Geodetic, InterceptSolutionView, MissionTime,
    PlanId, PlanView, Provenance, Quality, Releasability, ResourceId, TrackId, TrackStatus,
    TrackView,
};
use gungnir_policy::PolicyVerdict;
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};
use gungnir_ui::panels::approval_queue::PendingId;

#[path = "support/healthy.rs"]
mod healthy;

const OURS: &str = "a1b2c3d4-0000-0000-0000-000000000000";
const EFFECTOR: &str = "7c0e6a2b-1d3f-4e5a-9b8c-0d1e2f3a4b5c";

/// The intercept point the plan solved.
const POINT: Geodetic = Geodetic {
    lat_rad: 0.959_95,
    lon_rad: 0.209_5,
    alt_m: 120.0,
};

/// A middleware with a jammer behind it. It registers the jammer, then answers each task
/// with the statuses `answers` lists, and keeps every task it was sent.
fn effector(answers: &'static [&'static str], tasks: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut writer = stream.try_clone().expect("clone");
        let _ = writeln!(
            writer,
            r#"{{"nodeId":"{EFFECTOR}","registration":{{"nodeDefinition":[{{"nodeType":"NODE_TYPE_JAMMER"}}],"icdVersion":"BSI Flex 335 v2.0","name":"Jammer"}}}}"#
        );
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            let task: serde_json::Value = serde_json::from_str(&line).expect("a task line");
            let id = task["task"]["taskId"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if let Ok(mut kept) = tasks.lock() {
                kept.push(line);
            }
            for status in answers {
                let reason = if status.ends_with("REJECTED") {
                    r#","reason":["target outside the jammer's arc"]"#
                } else {
                    ""
                };
                let _ = writeln!(
                    writer,
                    r#"{{"nodeId":"{EFFECTOR}","taskAck":{{"taskId":"{id}","taskStatus":"{status}"{reason}}}}}"#
                );
            }
            let _ = writer.flush();
        }
    });
    addr.to_string()
}

struct Picture(Vec<TrackView>);
impl TrackingService for Picture {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &self.0
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn track() -> TrackView {
    TrackView {
        id: TrackId(1),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(3_000.0, 0.0, 50.0, -20.0, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 25.0,
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn desktop(name: &str, address: &str) -> AppState {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-sapient-effector-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([0.959_931, 0.209_44, 0.0]),
        sapient_node_id: Some(OURS.into()),
        endpoints: vec![EndpointConfig {
            name: "jammer".into(),
            kind: "sapient".into(),
            address: address.into(),
        }],
        sapient_effectors: vec![SapientEffectorConfig {
            endpoint: "jammer".into(),
            destination_id: EFFECTOR.into(),
            node_type: SapientEffectorType::Jammer,
        }],
        resources: vec![ResourceConfig {
            handoff_endpoint: Some("jammer".into()),
            id: 1,
            position: [0.959_931, 0.209_44, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: None,
        }],
        ..ConfigBaseline::default()
    };
    // An engagement opens only where the layer says when an effect is expected by.
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 600.0);
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    state.tracking = Box::new(Picture(vec![track()]));
    state
}

/// Decide a plan engaging track 1 with the jammer, at `point`.
fn decide(state: &mut AppState, point: Option<Geodetic>) -> gungnir_model::DecisionId {
    let pending = state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView::intercept(
                PlanId(7),
                MissionTime(0.0),
                vec![InterceptSolutionView {
                    resource: ResourceId(1),
                    track: TrackId(1),
                    intercept_point: point,
                    time_to_intercept_s: Some(14.0),
                }],
                0.0,
            ),
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".into(),
        })
        .expect("queued");
    healthy::healthy(state);
    decisions::decide(state, PendingId(pending.0), OperatorDecision::Accepted).expect("decided");
    state
        .desk
        .handoffs
        .last()
        .expect("an accepted decision issues a handoff")
        .handoff
        .decision
}

/// Tick until `done` holds; a deadlock guard, left the moment it does.
fn settle(state: &mut AppState, done: impl Fn(&AppState) -> bool) {
    for _ in 0..3_000 {
        update::tick(state);
        if done(state) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never settled: {:?}", state.alerts);
}

fn record(
    state: &AppState,
    decision: gungnir_model::DecisionId,
) -> &gungnir_approval::HandoffRecord {
    state
        .desk
        .handoffs
        .iter()
        .find(|h| h.handoff.decision == decision)
        .expect("the handoff record")
}

/// The case GAP-207 exists for: delivered on the effector's own word, its acceptance and
/// its completion on the record, and the engagement left open because SAPIENT says
/// nothing about effect.
#[test]
fn a_sapient_effector_takes_the_task_and_its_outcome_comes_back() {
    let tasks = Arc::new(Mutex::new(Vec::new()));
    let address = effector(
        &["TASK_STATUS_ACCEPTED", "TASK_STATUS_COMPLETED"],
        Arc::clone(&tasks),
    );
    let mut state = desktop("outcome", &address);
    let decision = decide(&mut state, Some(POINT));
    assert!(matches!(
        record(&state, decision).delivery,
        DeliveryState::Undelivered { .. }
    ));
    settle(&mut state, |s| record(s, decision).reports.len() == 2);

    let r = record(&state, decision);
    assert!(
        matches!(r.delivery, DeliveryState::Delivered { .. }),
        "delivered on the effector's TaskAck: {:?}",
        r.delivery
    );
    assert!(matches!(r.reports[0], EffectorReport::Acknowledged { .. }));
    assert!(matches!(r.reports[1], EffectorReport::Finished { .. }));
    let said = gungnir_ui::panels::handoff::report_sentence(&r.reports[1]);
    assert!(
        said.contains("does not say whether it was effective"),
        "{said}"
    );
    assert!(
        state
            .desk
            .engagements
            .iter()
            .find(|e| e.decision == decision)
            .is_some_and(|e| e.state.is_open()),
        "a finished task is not a stated effect; the track's evidence closes the engagement: {:?} {:?}", state.desk.engagements, state.alerts
    );

    // What the effector was handed: a request to engage at the intercept point, from this
    // deployment, naming the decision.
    let sent = tasks.lock().expect("tasks").clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    let task: serde_json::Value = serde_json::from_str(&sent[0]).expect("JSON");
    assert_eq!(task["nodeId"], OURS);
    assert_eq!(task["destinationId"], EFFECTOR);
    assert_eq!(task["task"]["command"]["request"], "engage");
    let point = &task["task"]["region"][0]["regionArea"]["locationList"]["locations"][0];
    assert!((point["y"].as_f64().expect("lat") - POINT.lat_rad).abs() < 1e-12);
    assert!(task["task"]["taskDescription"]
        .as_str()
        .is_some_and(|d| d.contains(&decision.to_string())));
}

/// The effector refuses: delivered, because it has the task, and refused, because it will
/// not act, with its own reason on the record.
#[test]
fn an_effector_that_rejects_the_task_is_recorded_as_refusing_it() {
    let tasks = Arc::new(Mutex::new(Vec::new()));
    let address = effector(&["TASK_STATUS_REJECTED"], tasks);
    let mut state = desktop("rejected", &address);
    let decision = decide(&mut state, Some(POINT));
    settle(&mut state, |s| !record(s, decision).reports.is_empty());
    let r = record(&state, decision);
    assert!(
        matches!(&r.delivery, DeliveryState::Refused { reason, .. } if reason.contains("outside the jammer's arc")),
        "{:?}",
        r.delivery
    );
}

/// A plan with no intercept point cannot say where, so nothing is sent and the operator is
/// told to reach the effector another way.
#[test]
fn a_handoff_with_no_intercept_point_is_never_sent() {
    let tasks = Arc::new(Mutex::new(Vec::new()));
    let address = effector(&["TASK_STATUS_ACCEPTED"], Arc::clone(&tasks));
    let mut state = desktop("no-point", &address);
    let decision = decide(&mut state, None);
    update::tick(&mut state);
    assert!(matches!(
        record(&state, decision).delivery,
        DeliveryState::Undelivered { .. }
    ));
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("undelivered") && a.contains("intercept_speed_mps")),
        "{:?}",
        state.alerts
    );
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(tasks.lock().expect("tasks").is_empty(), "nothing was sent");
}
