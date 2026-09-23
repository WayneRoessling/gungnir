// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The approval gate, end to end through the desktop tick (GAP-038).
//!
//! `gungnir-command` and `gungnir-policy` each test their own rules. What is not
//! testable inside either is the property the *wiring* has to hold: that the desktop
//! actually routes plans through the gate, that nothing arrives at an actionable state
//! without a recorded decision, and -- the one this build most needs -- that the
//! permanently empty queue explains itself rather than reading as a calm screen.
//!
//! These run against a real `AppState` over a scratch data directory, so they exercise
//! the same `update::tick` the binary runs.

use gungnir_app::decisions;
use gungnir_app::decisions::Submitted;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_command::{ApprovalWorkflow, OperatorDecision};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_model::policy_settings::AuthorityRule;
use gungnir_model::{
    EffectorLayer, InterceptSolutionView, PlanId, PlanKind, PlanView, ResourceId, TrackId,
    WeaponsControlStatus,
};
use gungnir_policy::PolicyVerdict;
use gungnir_ui::panels::approval_queue::EmptyBecause;

/// A desktop over a scratch journal directory, so a test never writes into the
/// developer's `./gungnir-journal`.
fn desktop(name: &str) -> AppState {
    let dir = std::env::temp_dir().join(format!("gungnir-approval-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    AppState::with_config(config).expect("desktop state")
}

/// A desktop whose baseline has an effector at each layer and an authority matrix that
/// gives the area layer to a Supervisor alone (GAP-113).
///
/// The same shape `gungnir-app/tests/desktop_projection.rs` gives its node, because the
/// question here is the desktop's half of it: an Operator at this console may take a
/// point engagement and may not take an area one.
fn desktop_with_a_ladder(name: &str) -> AppState {
    let dir = std::env::temp_dir().join(format!("gungnir-approval-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![effector(0, "point"), effector(1, "area")],
        ..ConfigBaseline::default()
    };
    for layer in [EffectorLayer::Point, EffectorLayer::Area] {
        config
            .policy
            .control_status
            .by_layer
            .insert(layer, WeaponsControlStatus::Free);
        config.assessment.effect_window_s.insert(layer, 60.0);
    }
    config.policy.authority.rules = vec![
        rule("Operator", EffectorLayer::Point),
        rule("Supervisor", EffectorLayer::Point),
        rule("Supervisor", EffectorLayer::Area),
    ];
    AppState::with_config(config).expect("desktop state")
}

fn effector(id: u32, layer: &str) -> ResourceConfig {
    ResourceConfig {
        handoff_endpoint: None,
        id,
        position: [0.0, 0.0, 0.0],
        capacity: 4,
        layer: layer.into(),
        cost: None,
        rounds_available: None,
        reserve: None,
        intercept_speed_mps: Some(400.0),
    }
}

/// One rule of the matrix. No class, so it is about the layer alone: this desktop states
/// no picture, and a plan naming a track nobody is tracking carries no classification to
/// key on.
fn rule(role: &str, layer: EffectorLayer) -> AuthorityRule {
    AuthorityRule {
        action: gungnir_security::actions::DECIDE_PLAN.into(),
        role: role.into(),
        layer: Some(layer),
        class: None,
        pre_delegated: false,
    }
}

/// A plan tasking one effector against one track.
fn plan_on(id: u128, resource: u32) -> PlanView {
    PlanView {
        id: PlanId(id),
        kind: PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(resource),
                track: TrackId(1),
                intercept_point: None,
                time_to_intercept_s: Some(10.0),
            }],
        },
        ..PlanView::default()
    }
}

/// What the queue is offering this plan to, if it is queued at all.
fn offered_to(state: &AppState, plan: u128) -> Option<Vec<String>> {
    use gungnir_command::ApprovalWorkflow;
    state
        .desk
        .approvals
        .queue()
        .iter()
        .find(|item| item.plan.id == PlanId(plan))
        .map(|item| item.offered_to.clone())
}

/// GAP-113, DN-09 §7: **a plan the role at this console may not accept is offered to a
/// role that may**, rather than counted as a denial and dropped.
///
/// The desktop ran the chain for the role at the console and stopped there, so an
/// area-layer plan asked by an Operator reached nobody with the authority to take it --
/// the queue stayed empty and a counter went up. The node has offered such a plan to the
/// lowest role that may take it since GAP-132; this is the same rule on the desktop's own
/// queue, which is what a cut-off desktop decides from (DN-31 §6.7).
#[test]
fn a_plan_this_console_may_not_accept_is_offered_to_a_role_that_may() {
    let mut state = desktop_with_a_ladder("gap113-area");
    assert_eq!(
        state.role(),
        gungnir_security::Role::Operator,
        "this test is about what an Operator at the console cannot take"
    );

    let outcome = decisions::submit(&mut state, plan_on(1, 1));
    assert!(
        matches!(
            outcome,
            Submitted::Evaluated(PolicyVerdict::RequiresHumanApproval)
        ),
        "an area plan a Supervisor may take was not queued for one: {outcome:?}"
    );
    assert_eq!(
        offered_to(&state, 1).as_deref(),
        Some(&["Supervisor".to_string()][..]),
        "the area plan is the Supervisor's to take, and the queue has to say so"
    );
    assert_eq!(
        state.desk.denials.count, 0,
        "the denial was counted instead of being offered to a role that may accept"
    );
}

/// The other half, unchanged: a plan this console's own role may accept is queued for
/// that role and not handed past it.
#[test]
fn a_plan_this_console_may_accept_stays_with_this_console() {
    let mut state = desktop_with_a_ladder("gap113-point");
    let outcome = decisions::submit(&mut state, plan_on(2, 0));
    assert!(
        matches!(
            outcome,
            Submitted::Evaluated(PolicyVerdict::RequiresHumanApproval)
        ),
        "a point plan an Operator may take was not queued: {outcome:?}"
    );
    assert_eq!(
        offered_to(&state, 2).as_deref(),
        Some(&["Operator".to_string()][..]),
        "the point plan is this console's to take"
    );
}

/// Contract C-01 at the wiring level: after ticking the desktop, nothing has become
/// actionable, because nothing has been decided.
///
/// This is the assertion `PIPELINE_IMPLEMENTED` will change the meaning of, not the
/// outcome of: when tracks start flowing, plans will reach the queue and this must
/// still hold until a person decides one.
#[test]
fn ticking_the_desktop_never_produces_an_actionable_plan() {
    let mut state = desktop("c01");
    for _ in 0..20 {
        update::tick(&mut state);
    }
    assert!(
        state.desk.approvals.records().is_empty(),
        "a decision was recorded with nobody deciding"
    );
    assert!(
        !state
            .desk
            .approvals
            .records()
            .iter()
            .any(gungnir_command::DecisionRecord::is_actionable),
        "a plan became actionable without a human decision"
    );
}

/// The queue is empty and says why. In this build the reason is always that no plan is
/// being produced, and it must never report the reassuring variant.
///
/// **The reason moved on 2026-09-06** and the test moved with it. The pipeline used to
/// be the answer; it produces tracks now (GAP-011), so what stops a plan reaching the
/// queue is the allocator (GAP-029). The assertion that matters is unchanged: an empty
/// queue never claims that nothing needs deciding.
#[test]
fn the_empty_queue_says_the_allocator_is_the_reason() {
    let mut state = desktop("empty");
    for _ in 0..5 {
        update::tick(&mut state);
    }
    assert!(state.desk.approvals.pending().is_empty());

    let reason = decisions::queue_empty_reason(&state);
    assert!(
        !reason.means_nothing_to_decide(),
        "the queue reported that nothing needs deciding while no plan is being \
         produced at all; that is the misreading this panel exists to prevent"
    );
    match reason {
        EmptyBecause::NoPlanProduced { because } => {
            assert!(
                because.contains("allocator"),
                "the reason named something other than the stage that is missing: {because}"
            );
        }
        other => panic!("expected NoPlanProduced, got {other:?}"),
    }
}

/// A plan the policy chain denies is recorded as denied and never enters the queue.
/// The empty plan is the case this build actually produces, so it is the one worth
/// pinning: an empty plan is not a plan an operator should be asked about.
#[test]
fn a_denied_plan_never_reaches_the_queue() {
    let mut state = desktop("denied");
    let outcome = decisions::submit(
        &mut state,
        PlanView {
            id: PlanId(1),
            ..PlanView::default()
        },
    );
    assert!(
        matches!(outcome, Submitted::Evaluated(PolicyVerdict::Denied { .. })),
        "an empty plan cleared the chain: {outcome:?}"
    );
    assert!(state.desk.approvals.pending().is_empty());
    assert_eq!(state.desk.denials.count, 1);
    assert!(
        state.desk.denials.last_reason.is_some(),
        "a denial with no recorded reason cannot be explained on the queue"
    );
}

/// A decision on an item that is not there fails loudly rather than being swallowed.
/// `main.rs` turns this into an alert; a silent `Ok` here would let the dialog close as
/// though a decision had been recorded when none was.
#[test]
fn deciding_a_missing_item_is_an_error() {
    let mut state = desktop("missing");
    let err = decisions::decide(
        &mut state,
        gungnir_ui::panels::approval_queue::PendingId(404),
        OperatorDecision::Accepted,
    )
    .expect_err("deciding a missing item must fail");
    assert!(err.to_string().contains("404"));
}

/// The escalation ladder follows the authorization rather than a list written beside
/// it: every role on it may decide, they are in rank order, and no role that may decide
/// is missing. A ladder that disagreed with `role_permits` would offer an item to
/// somebody who cannot take it, or skip somebody who can.
#[test]
fn the_escalation_ladder_follows_the_authorisation() {
    use gungnir_security::authz::role_permits;
    use gungnir_security::Role;

    let ladder = decisions::escalation_ladder();
    assert!(!ladder.is_empty());

    let named: Vec<Role> = ladder
        .iter()
        .map(|name| {
            *Role::ALL
                .iter()
                .find(|r| format!("{r:?}") == *name)
                .expect("every rung names a real role")
        })
        .collect();

    for role in &named {
        assert!(
            role_permits(*role, decisions::DECISION_ACTION),
            "{role:?} is on the ladder and may not decide"
        );
    }
    for role in Role::ALL {
        if role_permits(*role, decisions::DECISION_ACTION) {
            assert!(
                named.contains(role),
                "{role:?} may decide and would never be offered an item"
            );
        }
    }
    let ranks: Vec<u8> = named.iter().map(|r| r.rank()).collect();
    let mut sorted = ranks.clone();
    sorted.sort_unstable();
    assert_eq!(ranks, sorted, "the ladder must climb");
}

/// A queued item that nobody decides expires, leaves a record that is not a rejection,
/// and is counted as an expiry rather than as a decision.
///
/// This is the whole of GAP-034 exercised through the desktop's own state: the tick
/// sweeps, the outcome reaches the alert list, and PN-17's counts come out right.
#[test]
fn an_undecided_item_expires_through_the_desktop_tick() {
    use gungnir_command::{ApprovalWorkflow, Submission};
    use gungnir_model::{EffectorLayer, MissionTime};

    let mut state = desktop("expiry");
    // The desktop cannot produce a queued item -- no tracks, so no plan -- so the item
    // is submitted directly. Everything after this line is the desktop's own path.
    state.desk.approvals = gungnir_command::InMemoryApprovalWorkflow::with_settings({
        let mut settings = gungnir_model::DecisionSettings::default();
        settings.expiry_s.insert(EffectorLayer::Point, 30.0);
        settings
    });
    state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(7),
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".to_owned(),
        })
        .expect("submit");
    assert_eq!(state.desk.approvals.queue().len(), 1);

    // The clock is a wall clock, so it is already far past the deadline; one sweep is
    // enough. This is the same call `update::tick` makes.
    decisions::sweep(&mut state);

    assert!(
        state.desk.approvals.queue().is_empty(),
        "the item did not expire"
    );
    assert_eq!(decisions::expired_count(&state), 1);
    let record = state.desk.approvals.records().last().expect("a record");
    assert!(!record.is_actionable(), "an expiry became actionable");
    assert!(record.operator_id.is_none());
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("expired") && a.contains("not a rejection")),
        "the operator was not told the difference between an expiry and a rejection"
    );
}

/// An item with no configured expiry is preserved rather than dropped, which is DN-10's
/// deliberate asymmetry: silence about authority denies, silence about expiry preserves.
#[test]
fn an_item_with_no_configured_expiry_is_preserved() {
    use gungnir_command::{ApprovalWorkflow, Submission};
    use gungnir_model::{EffectorLayer, MissionTime};

    let mut state = desktop("preserved");
    state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(8),
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".to_owned(),
        })
        .expect("submit");

    for _ in 0..5 {
        update::tick(&mut state);
    }
    assert_eq!(
        state.desk.approvals.queue().len(),
        1,
        "an item nobody configured an expiry for was discarded"
    );
    assert_eq!(decisions::expired_count(&state), 0);
    assert!(state.desk.approvals.queue()[0].no_expiry_reason().is_some());
}

/// GAP-127: the desktop's `decide` authorizes the call itself rather than trusting that
/// PN-06 hid the control. Each refusal names why, and records nothing: no decision in the
/// history, nothing published, no audit row, and the item still waiting.
mod decide_is_authorized_where_it_acts {
    use super::desktop;
    use gungnir_app::decisions;
    use gungnir_command::{ApprovalWorkflow, CommandError, OperatorDecision, Submission};
    use gungnir_model::{EffectorLayer, MissionTime, PendingApprovalId, PlanId, PlanView};
    use gungnir_policy::PolicyVerdict;
    use gungnir_security::{AuditLog, Role};
    use gungnir_ui::panels::approval_queue::PendingId;

    /// A queued item, offered to `role` only.
    fn queued(state: &mut gungnir_app::state::AppState, role: &str) -> PendingApprovalId {
        state
            .desk
            .approvals
            .submit_for_approval(Submission {
                plan: PlanView {
                    id: PlanId(7),
                    ..PlanView::default()
                },
                verdict: PolicyVerdict::RequiresHumanApproval,
                submitted: MissionTime(0.0),
                layer: EffectorLayer::Point,
                priority: 0.0,
                role: role.into(),
            })
            .expect("queued")
    }

    /// Nothing happened: the item still waits, and nothing reached the history, the bus
    /// or the audit trail.
    fn nothing_recorded(state: &gungnir_app::state::AppState, item: PendingApprovalId) {
        assert!(
            state.desk.approvals.records().is_empty(),
            "a refused decision was recorded"
        );
        assert!(
            state.desk.approvals.queue().iter().any(|i| i.id == item),
            "a refused decision took the item out of the queue"
        );
        assert!(
            state.audit.entries().is_empty(),
            "a refused decision left an audit row: {:?}",
            state.audit.entries()
        );
    }

    #[test]
    fn a_role_without_plan_decide_is_refused_and_nothing_is_recorded() {
        let mut state = desktop("gap127-analyst");
        let item = queued(&mut state, "Operator");
        state.set_role(Role::Analyst);
        let err = decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted)
            .expect_err("an Analyst may not decide a plan");
        assert!(
            matches!(&err, CommandError::NotPermitted { action, .. } if *action == "plan.decide"),
            "refused for some other reason: {err}"
        );
        assert!(
            err.to_string().contains("Analyst"),
            "the refusal did not name the role: {err}"
        );
        nothing_recorded(&state, item);
    }

    /// Operator holds `plan.decide` and not `plan.override`. Before GAP-127 the desktop
    /// recorded an Operator's override that the node's route refuses with a 403.
    #[test]
    fn an_operator_override_needs_plan_override_and_is_refused() {
        let mut state = desktop("gap127-override");
        let item = queued(&mut state, "Operator");
        state.set_role(Role::Operator);
        let err = decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Overridden)
            .expect_err("an Operator may not override");
        assert!(
            matches!(&err, CommandError::NotPermitted { action, .. } if *action == "plan.override"),
            "refused for some other reason: {err}"
        );
        nothing_recorded(&state, item);
    }

    /// A Supervisor holds `plan.decide`, but an item offered to the Operator and never
    /// escalated is not theirs to take (DN-10 §5), as the node's route already says.
    #[test]
    fn a_role_the_item_was_not_offered_to_is_refused() {
        let mut state = desktop("gap127-offer");
        let item = queued(&mut state, "Operator");
        state.set_role(Role::Supervisor);
        let err = decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted)
            .expect_err("the item was offered to the Operator only");
        let CommandError::NotOffered {
            offered_to, role, ..
        } = &err
        else {
            panic!("refused for some other reason: {err}");
        };
        assert_eq!(role, "Supervisor");
        assert_eq!(offered_to, &vec!["Operator".to_string()]);
        nothing_recorded(&state, item);
    }

    /// The control: the role the item was offered to, holding the permission, decides.
    #[test]
    fn the_offered_role_with_the_permission_decides() {
        let mut state = desktop("gap127-allowed");
        let item = queued(&mut state, "Operator");
        state.set_role(Role::Operator);
        decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted)
            .expect("the Operator may accept an item offered to the Operator");
        assert_eq!(state.desk.approvals.records().len(), 1);
    }
}
