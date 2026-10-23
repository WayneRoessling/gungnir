// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A decision acting on a plan while the picture is degraded acknowledges each degraded
//! condition in force, and the acknowledgement is on the record (GAP-195, D-132).
//!
//! Until GAP-195 PN-07's tick gated accept and nothing recorded it: the decision record
//! and the audit entry said nothing of the conditions a plan was accepted under, and an
//! override was not gated at all. Now `decisions::decide` asks, for an accept and an
//! override alike, against the set in force at the moment of the decision; a rejection
//! asks nothing. What was ticked goes onto `DecisionRecord::acknowledged`,
//! `CommandEvent::Decided` and the decision's audit entry, one acknowledgement per
//! condition, subject `degraded <subsystem>` and statement as PN-07 drew it.
//!
//! The desktop starts healthy -- every feed bound and the services reported
//! (`support/healthy.rs`) -- and then its journal fails, so exactly one condition is in
//! force and the test says which.

use gungnir_app::state::AppState;
use gungnir_app::{decisions, update, workspace};
use gungnir_command::{ApprovalWorkflow, CommandError, OperatorDecision, Submission};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::CommandEvent;
use gungnir_model::{
    Acknowledgement, EffectorLayer, MissionTime, PendingApprovalId, PlanId, PlanView,
};
use gungnir_policy::PolicyVerdict;
use gungnir_security::{AuditLog, Role};
use gungnir_time::ReplayClockAuthority;
use gungnir_ui::panels::approval_queue::PendingId;

#[path = "support/healthy.rs"]
mod healthy;

/// A healthy desktop with no laydown, so no rehearsal statement is asked (D-119), acting
/// as a supervisor, who may accept and override -- and then its journal fails.
fn desktop(name: &str) -> AppState {
    let dir = std::env::temp_dir().join(format!("gungnir-degraded-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    state.set_role(Role::Supervisor);
    healthy::healthy(&mut state);
    state.journal_failed = true;
    state
}

fn queued(state: &mut AppState, plan: u128) -> PendingId {
    let id = state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(plan),
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(100.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Supervisor".into(),
        })
        .expect("queued");
    PendingId(id.0)
}

/// Tick PN-07's acknowledgement against `subsystems`, as the dialog does when drawn.
fn tick(state: &mut AppState, id: PendingId, subsystems: &[&str]) {
    state.select_approval(id);
    state.dialog.degraded_acknowledged = true;
    state.dialog.acknowledged_for = subsystems.iter().map(|s| (*s).to_owned()).collect();
}

fn told(state: &AppState, id: PendingId) -> Vec<Acknowledgement> {
    workspace::decision_conditions(state, id)
        .into_iter()
        .map(|(subsystem, detail)| Acknowledgement {
            subject: format!("degraded {subsystem}"),
            statement: detail,
        })
        .collect()
}

/// D-132: accept and override are refused without the tick, and with a tick given
/// against another set; a rejection asks nothing; with the tick, each condition goes
/// onto the record, the event and the audit entry.
#[test]
// One walk through the decision path; split, each part would repeat the set-up.
#[allow(clippy::too_many_lines)]
fn a_decision_on_a_degraded_picture_records_what_was_acknowledged() {
    let mut state = desktop("walk");
    let events = state.events.subscribe();
    let item = queued(&mut state, 1);
    let conditions = workspace::decision_conditions(&state, item);
    let subsystems: Vec<&str> = conditions.iter().map(|(s, _)| *s).collect();
    assert_eq!(
        subsystems,
        ["journal"],
        "a healthy desktop whose journal failed"
    );

    // Refused without the tick, for an accept and for an override alike; the refusal
    // names the first condition, and nothing is recorded.
    state.select_approval(item);
    for decision in [OperatorDecision::Accepted, OperatorDecision::Overridden] {
        let refused = decisions::decide(&mut state, item, decision.clone())
            .expect_err("an unacknowledged degraded picture refuses a decision acting on a plan");
        assert!(
            matches!(&refused, CommandError::Unacknowledged { subject, .. }
                if *subject == format!("degraded {}", subsystems[0])),
            "{decision:?}: {refused}"
        );
    }
    assert!(state.desk.approvals.records().is_empty());

    // A tick given against a set without the journal does not cover the journal.
    let without: Vec<&str> = subsystems
        .iter()
        .copied()
        .filter(|s| *s != "journal")
        .collect();
    tick(&mut state, item, &without);
    let refused = decisions::decide(&mut state, item, OperatorDecision::Accepted)
        .expect_err("a tick for another set does not count");
    assert!(
        matches!(&refused, CommandError::Unacknowledged { subject, statement }
            if subject == "degraded journal" && statement.starts_with("writes are failing")),
        "{refused}"
    );
    assert!(state.desk.approvals.records().is_empty());

    // A rejection acts on nothing and asks nothing.
    let rejected = queued(&mut state, 2);
    state.select_approval(rejected);
    decisions::decide(
        &mut state,
        rejected,
        OperatorDecision::Rejected {
            reason: "not on this picture".into(),
        },
    )
    .expect("a rejection needs no acknowledgement");
    let records = state.desk.approvals.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].acknowledged.is_empty());

    // With the tick against the set in force, the accept is recorded with each condition.
    let expected = told(&state, item);
    tick(&mut state, item, &subsystems);
    decisions::decide(&mut state, item, OperatorDecision::Accepted).expect("acknowledged");
    let accepted = state
        .desk
        .approvals
        .records()
        .iter()
        .find(|r| r.item == Some(PendingApprovalId(item.0)))
        .expect("recorded")
        .clone();
    assert_eq!(accepted.acknowledged, expected);

    // So is an override, on its own item.
    let overridden = queued(&mut state, 3);
    tick(&mut state, overridden, &subsystems);
    decisions::decide(&mut state, overridden, OperatorDecision::Overridden).expect("acknowledged");
    let record = state
        .desk
        .approvals
        .records()
        .iter()
        .find(|r| r.item == Some(PendingApprovalId(overridden.0)))
        .expect("recorded")
        .clone();
    assert_eq!(record.acknowledged, told(&state, overridden));

    // The event carries it, and so does the audit entry: the rejection's empty, the
    // accept's and the override's each the conditions in force.
    update::tick(&mut state);
    let decided: Vec<Vec<Acknowledgement>> = events
        .try_iter()
        .filter_map(|env| match env.event {
            Event::Command(CommandEvent::Decided { acknowledged, .. }) => Some(acknowledged),
            _ => None,
        })
        .collect();
    assert_eq!(decided.len(), 3, "{decided:?}");
    assert!(decided[0].is_empty());
    assert_eq!(decided[1], expected);
    let entries = state.audit.entries();
    let journal_line = format!(
        "acknowledged degraded journal: {}",
        expected
            .iter()
            .find(|a| a.subject == "degraded journal")
            .expect("the journal is in force")
            .statement
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.action == gungnir_security::actions::DECIDE_PLAN
                && e.detail.contains(&journal_line))
            .count(),
        2,
        "{entries:?}"
    );
}
