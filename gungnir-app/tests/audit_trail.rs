// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Every gated act leaves an audit row (GAP-059, contract C-04), attributed to the
//! verified operator or to nobody -- never to a role standing in for a person.

use gungnir_app::decisions;
use gungnir_app::requirements;
use gungnir_app::review;
use gungnir_app::state::AppState;
use gungnir_app::sustainment::{self, SustainmentState};
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::{ConfigBaseline, ResourceConfig, SensorConfig};
use gungnir_model::{EffectorLayer, MissionTime, PlanId, PlanView, SensorMode};
use gungnir_policy::PolicyVerdict;
use gungnir_security::{actions, AuditLog};
use gungnir_ui::panels::approval_queue::PendingId;
use gungnir_ui::panels::reports::ReviewAction;

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-audit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        sensors: vec![SensorConfig {
            id: 1,
            modality: "radar".into(),
            position: [0.0, 0.0, 10.0],
            max_range_m: 20_000.0,
            // A control endpoint makes the sensor commandable; nothing acknowledges,
            // which is a timeout later and an audit row now.
            control_endpoint: Some("udp://radar-1.example:7000".into()),
            maintenance: Vec::new(),
        }],
        resources: vec![ResourceConfig {
            handoff_endpoint: None,
            id: 1,
            position: [0.0, 0.0, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: None,
        }],
        // A requirement is stated against an asset (DN-11).
        assets: vec![gungnir_config::AssetConfig {
            id: 1,
            name: "the harbour".into(),
            position: [0.959_931, 0.209_440, 0.0],
            radius_m: Some(500.0),
            priority: "high".into(),
            warning_lead_time_s: None,
            warning_channel: None,
            warning_within_m: None,
            note: None,
        }],
        ..ConfigBaseline::default()
    };
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

fn actions_recorded(state: &AppState) -> Vec<String> {
    state
        .audit
        .entries()
        .iter()
        .map(|e| e.action.clone())
        .collect()
}

#[test]
fn a_decision_a_sensor_command_a_requirement_and_a_review_each_leave_a_row() {
    let (mut state, dir) = desktop("acts");
    let mut sustainment = SustainmentState::default();
    assert!(state.audit.entries().is_empty(), "nothing was done yet");

    // A decision.
    let pending = state
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(1),
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".into(),
        })
        .expect("queued");
    decisions::decide(&mut state, PendingId(pending.0), OperatorDecision::Accepted)
        .expect("decided");
    // A sensor command.
    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("commanded");
    // A requirement stated.
    let _ = requirements::state_requirement(
        &mut state,
        "identify the contact".into(),
        0,
        gungnir_model::AssetPriority::Medium,
        None,
    );
    // A review opened.
    review::apply(&mut state, &mut sustainment, ReviewAction::Open);

    let recorded = actions_recorded(&state);
    for expected in [
        actions::DECIDE_PLAN,
        actions::TASK_SENSOR,
        actions::REQUIREMENT,
        actions::REVIEW_CONDUCT,
    ] {
        assert!(
            recorded.iter().any(|a| a == expected),
            "{expected} left no audit row: {recorded:?}"
        );
    }
    // Nobody signed in, and no row pretends otherwise (DN-23 §5 rule 1).
    assert!(state.audit.entries().iter().all(|e| e.operator.is_none()));
    let _ = std::fs::remove_dir_all(dir);
}
