// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The handoff record is built from the decision, and delivery is never claimed
//! (GAP-040, DN-07 §5, §8).

use gungnir_app::engagements;
use gungnir_app::state::AppState;
use gungnir_command::{DecisionRecord, OperatorDecision};
use gungnir_config::{ConfigBaseline, EndpointConfig, ResourceConfig};
use gungnir_eventing::Event;
use gungnir_model::events::HandoffEvent;
use gungnir_model::handoff::DeliveryState;
use gungnir_model::{
    DecisionId, EffectorLayer, InterceptSolutionView, MissionTime, PlanId, PlanKind, PlanView,
    ResourceId, TrackId,
};
use gungnir_policy::PolicyVerdict;

fn desktop(name: &str, endpoint: Option<&str>) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-handoffs-{name}-{pid}",
        pid = std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        endpoints: vec![EndpointConfig {
            name: "battery-2".into(),
            kind: "handoff".into(),
            address: "https://battery-2.example/handoff".into(),
        }],
        resources: vec![ResourceConfig {
            handoff_endpoint: endpoint.map(str::to_string),
            id: 1,
            position: [0.0, 0.0, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: None,
        }],
        ..ConfigBaseline::default()
    };
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 30.0);
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

fn accepted() -> DecisionRecord {
    DecisionRecord {
        id: DecisionId(1),
        plan: PlanView {
            id: PlanId(1),
            kind: PlanKind::Intercept {
                solutions: vec![InterceptSolutionView {
                    resource: ResourceId(1),
                    track: TrackId(7),
                    intercept_point: None,
                    time_to_intercept_s: Some(10.0),
                }],
            },
            ..PlanView::default()
        },
        verdict: PolicyVerdict::RequiresHumanApproval,
        decision: OperatorDecision::Accepted,
        operator_id: None,
        mission_time: MissionTime(0.0),
    }
}

/// **No endpoint: manual, said plainly** (DN-07 §5 case 4), with the attribution the
/// record has -- nobody signed in, in words, never a name that was not there.
#[test]
fn a_resource_with_no_endpoint_is_a_manual_handoff() {
    let (mut state, dir) = desktop("manual", None);
    let events = state.events.subscribe();
    engagements::open_for(&mut state, &accepted());
    assert_eq!(state.handoffs.len(), 1);
    let h = &state.handoffs[0];
    assert_eq!(h.delivery, DeliveryState::Manual);
    assert_eq!(h.handoff.decided_by.operator, "nobody signed in");
    assert!(!h.delivery.is_delivered());
    let kinds: Vec<&str> = events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Handoff(HandoffEvent::Issued { .. }) => Some("issued"),
            Event::Handoff(HandoffEvent::Manual { .. }) => Some("manual"),
            Event::Handoff(HandoffEvent::Undelivered { .. }) => Some("undelivered"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["issued", "manual"]);
    assert!(
        state.alerts.iter().any(|a| a.contains("manual")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **An endpoint with no transport: undelivered, never delivered** (DN-07 §5 case 3).
#[test]
fn an_endpoint_without_a_transport_is_undelivered_not_delivered() {
    let (mut state, dir) = desktop("undelivered", Some("battery-2"));
    engagements::open_for(&mut state, &accepted());
    let h = &state.handoffs[0];
    assert_eq!(h.endpoint.as_deref(), Some("battery-2"));
    assert!(matches!(h.delivery, DeliveryState::Undelivered { .. }));
    assert!(h.delivery.needs_attention());
    let _ = std::fs::remove_dir_all(dir);
}

/// A rejected decision issues nothing: the constructor is only ever reached from an
/// actionable record (C-01).
#[test]
fn a_rejected_decision_issues_no_handoff() {
    let (mut state, dir) = desktop("rejected", None);
    let mut record = accepted();
    record.decision = OperatorDecision::Rejected {
        reason: "not this one".into(),
    };
    engagements::open_for(&mut state, &record);
    assert!(state.handoffs.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// **PN-06's rule, end to end: the decision leaves the queue and the handoff does not
/// leave the screen** (GAP-040, DN-07 §5).
///
/// Rendered through the real dock behaviour rather than asserted on the view struct,
/// because the failure this guards against is a section drawn inside the queue's
/// non-empty branch: the view would be perfect and the pane would show nothing. The
/// queue here is genuinely empty, which is the state every decision leaves behind.
#[test]
fn a_decided_handoff_stays_on_pn06_until_it_is_delivered() {
    use gungnir_app::dock::PanelBehavior;
    use gungnir_app::sustainment::SustainmentState;
    use gungnir_ui::harness::RenderProbe;
    use gungnir_workflow::PanelId;

    let (mut state, dir) = desktop("visible", None);
    engagements::open_for(&mut state, &accepted());
    assert_eq!(state.handoffs[0].delivery, DeliveryState::Manual);
    assert!(
        !state.handoffs[0].delivery.needs_attention(),
        "a radio call is the arrangement, not an incident"
    );

    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        let mut behavior = PanelBehavior::new(&state, &mut sustainment);
        behavior.draw_panel(ui, PanelId::ApprovalQueue);
    });

    assert!(
        frame.says("Decided, not yet delivered"),
        "the decision left the queue and took the handoff with it: {}",
        frame.joined()
    );
    assert!(
        frame.says("Decision 1"),
        "the outstanding handoff named no decision: {}",
        frame.joined()
    );
    assert!(
        frame.says("not a fault"),
        "the radio call was not drawn as the arrangement it is: {}",
        frame.joined()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **PN-20's rows, end to end: what was handed off, to whom, when, and what came back**
/// (GAP-040, DN-07 §7).
///
/// The sequence is the point. An acknowledgement followed by an ineffective completion
/// is a different account from either one alone, and the delivery state cannot carry it:
/// both leave `delivery` untouched.
#[test]
fn pn20_draws_the_handoff_and_every_report_that_came_back() {
    use gungnir_app::dock::PanelBehavior;
    use gungnir_app::handoffs::apply_report;
    use gungnir_app::sustainment::SustainmentState;
    use gungnir_model::handoff::EffectorReport;
    use gungnir_ui::harness::RenderProbe;
    use gungnir_workflow::PanelId;

    let (mut state, dir) = desktop("reported", Some("battery-2"));
    engagements::open_for(&mut state, &accepted());
    apply_report(
        &mut state,
        DecisionId(1),
        "battery-2",
        &EffectorReport::Acknowledged {
            at: MissionTime(112.0),
        },
        MissionTime(112.0),
    );
    apply_report(
        &mut state,
        DecisionId(1),
        "battery-2",
        &EffectorReport::Completed {
            at: MissionTime(160.0),
            effective: false,
            detail: "round expended, target unaffected".into(),
        },
        MissionTime(160.0),
    );
    assert_eq!(
        state.handoffs[0].reports.len(),
        2,
        "the reports were folded away and the sequence lost"
    );

    let mut sustainment = SustainmentState::default();
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        let mut behavior = PanelBehavior::new(&state, &mut sustainment);
        behavior.draw_panel(ui, PanelId::Audit);
    });

    assert!(frame.says("battery-2"), "{}", frame.joined());
    assert!(
        frame.says("nobody signed in"),
        "an unattributed decision was tidied into a role: {}",
        frame.joined()
    );
    assert!(
        frame.says("Acknowledged at T+112 s"),
        "the first report was dropped: {}",
        frame.joined()
    );
    assert!(
        frame.says("ineffective"),
        "an ineffective engagement was not drawn as one: {}",
        frame.joined()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A report naming a decision this desktop never handed off is rejected, and nothing is
/// kept for PN-20 to draw: an effector report is an untrusted external input, and a
/// rejected one is not part of the after-action account.
#[test]
fn a_rejected_report_is_not_kept_on_any_record() {
    use gungnir_app::handoffs::apply_report;
    use gungnir_model::handoff::EffectorReport;

    let (mut state, dir) = desktop("rejected-report", Some("battery-2"));
    engagements::open_for(&mut state, &accepted());
    apply_report(
        &mut state,
        DecisionId(99),
        "battery-2",
        &EffectorReport::Acknowledged {
            at: MissionTime(112.0),
        },
        MissionTime(112.0),
    );
    assert!(
        state.handoffs.iter().all(|h| h.reports.is_empty()),
        "a report for an unknown decision was kept"
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("rejected")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}
