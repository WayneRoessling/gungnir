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
use gungnir_config::ConfigBaseline;
use gungnir_model::{PlanId, PlanView};
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
        state.approvals.records().is_empty(),
        "a decision was recorded with nobody deciding"
    );
    assert!(
        !state
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
    assert!(state.approvals.pending().is_empty());

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
    assert!(state.approvals.pending().is_empty());
    assert_eq!(state.denials.count, 1);
    assert!(
        state.denials.last_reason.is_some(),
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
    state.approvals = gungnir_command::InMemoryApprovalWorkflow::with_settings({
        let mut settings = gungnir_model::DecisionSettings::default();
        settings.expiry_s.insert(EffectorLayer::Point, 30.0);
        settings
    });
    state
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
    assert_eq!(state.approvals.queue().len(), 1);

    // The clock is a wall clock, so it is already far past the deadline; one sweep is
    // enough. This is the same call `update::tick` makes.
    decisions::sweep(&mut state);

    assert!(
        state.approvals.queue().is_empty(),
        "the item did not expire"
    );
    assert_eq!(decisions::expired_count(&state), 1);
    let record = state.approvals.records().last().expect("a record");
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
        state.approvals.queue().len(),
        1,
        "an item nobody configured an expiry for was discarded"
    );
    assert_eq!(decisions::expired_count(&state), 0);
    assert!(state.approvals.queue()[0].no_expiry_reason().is_some());
}
