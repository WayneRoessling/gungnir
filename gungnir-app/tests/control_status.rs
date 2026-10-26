// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-203: a live weapons-control-status change on a desktop that runs its own queue
//! (FL-08; `docs/design/DN-09-authority-and-control-status.md` §10; D-140).
//!
//! Every test runs round 1's committed baseline (`testdata/usability/round-1.json`),
//! which puts the point layer at `free` and the area layer at `hold`, and checks that a
//! change is:
//!
//! - made only by a signed-in supervisor or commander, with a reason, and refused for
//!   anyone else with nothing recorded;
//! - journaled as `PolicyEvent::ControlStatusSet` naming the person, and audited under
//!   `weapons.control_status`;
//! - applied to the settings in force, so the chain judges the next plan under it;
//! - what withdraws a queued item the new status refuses, as `CommandEvent::Withdrawn`
//!   and a record that is neither a decision nor an expiry;
//! - confirmed by a second person when the baseline's two-person rule is on (D-140);
//! - folded back over the baseline at a restart under the same revision, and superseded
//!   by a later revision.

use gungnir_app::control_status::{self, Confirmation, SetError, SetOutcome, SetRequest};
use gungnir_app::state::AppState;
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::{CommandEvent, PolicyEvent};
use gungnir_model::{
    EffectorLayer, InterceptSolutionView, MissionTime, PlanId, PlanKind, PlanView, TrackId,
    WeaponsControlStatus,
};
use gungnir_policy::PolicyVerdict;
use gungnir_security::{
    hash_passphrase, Account, AuditLog, InMemoryAccountStore, LocalAccountAuthority, OperatorId,
    Role,
};
use gungnir_time::ReplayClockAuthority;
use std::path::{Path, PathBuf};

const PASSPHRASE: &str = "correct horse battery staple";
const SUPERVISOR: OperatorId = OperatorId(51);
const COMMANDER: OperatorId = OperatorId(52);
const OPERATOR: OperatorId = OperatorId(53);

fn round_1() -> ConfigBaseline {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/usability/round-1.json");
    let text = std::fs::read_to_string(path).expect("round-1.json is committed");
    serde_json::from_str(&text).expect("round-1.json parses")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-control-status-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Round 1 at `revision`, journaling into `dir`, with three accounts. A second call with
/// the same `dir` is a restart.
fn desktop_with(dir: &Path, revision: u32, two_person: bool) -> AppState {
    let mut config = round_1();
    config.data_dir = dir.to_string_lossy().into_owned();
    config.revision = revision;
    config.policy.two_person.control_status = two_person;
    gungnir_config::validate(&config).expect("the round-1 baseline is valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    let account = |operator, role| Account {
        operator,
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    };
    state.set_session_authority(Box::new(LocalAccountAuthority::new(Box::new(
        InMemoryAccountStore::new(vec![
            account(SUPERVISOR, Role::Supervisor),
            account(COMMANDER, Role::Commander),
            account(OPERATOR, Role::Operator),
        ]),
    ))));
    state
}

fn desktop(dir: &Path) -> AppState {
    desktop_with(dir, 1, false)
}

fn sign_in(state: &mut AppState, who: OperatorId) {
    state.sign_out();
    state
        .sign_in(&LocalAccountAuthority::credential(who, PASSPHRASE))
        .expect("signed in");
}

fn request(layer: EffectorLayer, status: WeaponsControlStatus, reason: &str) -> SetRequest {
    SetRequest {
        layer,
        status,
        reason: reason.to_owned(),
        confirmation: None,
    }
}

fn policy_events(
    rx: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>,
) -> Vec<gungnir_model::ControlStatusChange> {
    rx.try_iter()
        .filter_map(|env| match env.event {
            Event::Policy(PolicyEvent::ControlStatusSet(c)) => Some(c),
            _ => None,
        })
        .collect()
}

fn audited(state: &AppState) -> Vec<(Option<OperatorId>, String)> {
    state
        .audit
        .entries()
        .iter()
        .filter(|e| e.action == gungnir_security::actions::SET_CONTROL_STATUS)
        .map(|e| (e.operator, e.detail.clone()))
        .collect()
}

fn in_force(state: &AppState, layer: EffectorLayer) -> WeaponsControlStatus {
    state.config.policy.control_status.for_layer(layer)
}

#[test]
fn a_supervisor_sets_a_layer_and_the_record_names_them() {
    let dir = scratch("who");
    let mut state = desktop(&dir);
    let events = state.events.subscribe();
    assert_eq!(
        in_force(&state, EffectorLayer::Point),
        WeaponsControlStatus::Free
    );

    sign_in(&mut state, SUPERVISOR);
    let outcome = control_status::set(
        &mut state,
        request(
            EffectorLayer::Point,
            WeaponsControlStatus::Hold,
            "  deconfliction  ",
        ),
    )
    .expect("a supervisor may set the status");
    let SetOutcome::Recorded { change, withdrawn } = outcome else {
        panic!("nothing is linked, so the change is recorded here: {outcome:?}");
    };
    assert_eq!(withdrawn, 0, "nothing was queued");
    assert_eq!(change.previous, WeaponsControlStatus::Free);
    assert_eq!(change.status, WeaponsControlStatus::Hold);
    assert_eq!(change.reason, "deconfliction", "the reason is trimmed");
    assert_eq!(change.operator, SUPERVISOR.0.to_string());
    assert_eq!(change.role, "Supervisor");
    assert_eq!(change.revision, 1);
    assert!(change.confirmed_by.is_none());
    assert!(change.origin.is_none(), "made here, not forwarded");

    // The setting in force, the ledger, the journal and the audit record all say it.
    assert_eq!(
        in_force(&state, EffectorLayer::Point),
        WeaponsControlStatus::Hold
    );
    assert_eq!(
        state
            .control_status_ledger
            .set_by
            .get(&EffectorLayer::Point)
            .map(|c| c.operator.as_str()),
        Some("51")
    );
    assert_eq!(state.control_status_ledger.this_session.len(), 1);
    let journaled = policy_events(&events);
    assert_eq!(journaled.len(), 1);
    assert_eq!(journaled[0], change);
    let audit = audited(&state);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].0, Some(SUPERVISOR));
    assert!(
        audit[0].1.contains("Point layer set to Hold (was Free)")
            && audit[0].1.contains("operator 51 (Supervisor)")
            && audit[0].1.contains("deconfliction"),
        "{}",
        audit[0].1
    );
}

#[test]
fn nobody_else_can_and_nothing_is_recorded() {
    let dir = scratch("refused");
    let mut state = desktop(&dir);
    let events = state.events.subscribe();

    // Nobody signed in: a desktop always has a selected role, and it is not enough.
    let refused = control_status::set(
        &mut state,
        request(EffectorLayer::Point, WeaponsControlStatus::Hold, "x"),
    );
    assert_eq!(refused, Err(SetError::NotSignedIn));

    sign_in(&mut state, OPERATOR);
    let refused = control_status::set(
        &mut state,
        request(EffectorLayer::Point, WeaponsControlStatus::Hold, "x"),
    );
    assert_eq!(
        refused,
        Err(SetError::NotPermitted {
            role: "Operator".into()
        })
    );

    sign_in(&mut state, SUPERVISOR);
    let refused = control_status::set(
        &mut state,
        request(EffectorLayer::Point, WeaponsControlStatus::Hold, "   "),
    );
    assert_eq!(refused, Err(SetError::NoReason));
    // Already there: a change that changes nothing is refused rather than recorded twice.
    let refused = control_status::set(
        &mut state,
        request(EffectorLayer::Area, WeaponsControlStatus::Hold, "x"),
    );
    assert_eq!(
        refused,
        Err(SetError::Unchanged {
            layer: EffectorLayer::Area,
            status: WeaponsControlStatus::Hold
        })
    );

    assert_eq!(
        in_force(&state, EffectorLayer::Point),
        WeaponsControlStatus::Free
    );
    assert!(policy_events(&events).is_empty());
    assert!(audited(&state).is_empty());
    assert!(state.control_status_ledger.this_session.is_empty());
}

/// A queued item on the layer being put at hold leaves the queue as a withdrawal: not a
/// decision, not an expiry, said on the stream and on the record (DN-09 §10).
#[test]
// One scenario checked at every layer it reaches: the queue, the record, the stream, the
// alert and the counter. Splitting it would check each against a queue set up twice.
#[allow(clippy::too_many_lines)]
fn a_tightening_withdraws_the_queued_items_it_refuses() {
    let dir = scratch("withdraw");
    let mut state = desktop(&dir);
    let events = state.events.subscribe();
    sign_in(&mut state, COMMANDER);
    // The desktop cannot produce a queued item without tracks, so two are submitted
    // directly, one per layer; everything after is the desktop's own path. The point
    // layer is `free` in round 1 and resource 1 is on it; resource 2 is on the area layer,
    // which is already at hold, so a plan tasking it could never have been queued: the
    // second item is on the point layer too, to show every refused item goes, not one.
    let point = state
        .resources
        .iter()
        .find(|r| r.layer == EffectorLayer::Point)
        .map(|r| r.id)
        .expect("round 1 sites a point-layer resource");
    for (n, plan_id) in [7u128, 8].into_iter().enumerate() {
        state
            .desk
            .approvals
            .submit_for_approval(Submission {
                plan: PlanView {
                    id: PlanId(plan_id),
                    kind: PlanKind::Intercept {
                        solutions: vec![InterceptSolutionView {
                            resource: point,
                            track: TrackId(900 + n as u64),
                            intercept_point: None,
                            time_to_intercept_s: None,
                        }],
                    },
                    ..PlanView::default()
                },
                verdict: PolicyVerdict::RequiresHumanApproval,
                submitted: MissionTime(100.0),
                layer: EffectorLayer::Point,
                priority: 0.0,
                role: "Commander".to_owned(),
            })
            .expect("submit");
    }
    assert_eq!(state.desk.approvals.queue().len(), 2);
    assert_eq!(
        control_status::would_withdraw(&state, EffectorLayer::Point, WeaponsControlStatus::Hold),
        2,
        "the dialog's sentence counts what the change would withdraw"
    );
    assert_eq!(
        control_status::would_withdraw(&state, EffectorLayer::Area, WeaponsControlStatus::Free),
        0,
        "nothing queued engages the area layer"
    );

    let outcome = control_status::set(
        &mut state,
        request(
            EffectorLayer::Point,
            WeaponsControlStatus::Hold,
            "raid over",
        ),
    )
    .expect("set");
    let SetOutcome::Recorded { withdrawn, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(withdrawn, 2);
    assert!(
        state.desk.approvals.queue().is_empty(),
        "both left the queue"
    );

    // The record: two withdrawals, actionable to nobody, naming no operator.
    let records: Vec<_> = state
        .desk
        .approvals
        .records()
        .iter()
        .filter(|r| r.is_withdrawal())
        .collect();
    assert_eq!(records.len(), 2);
    for r in &records {
        assert!(!r.is_actionable() && !r.is_expiry());
        assert!(r.operator_id.is_none() && r.role.is_none());
        assert!(matches!(
            r.decision,
            OperatorDecision::Withdrawn {
                layer: EffectorLayer::Point,
                status: WeaponsControlStatus::Hold,
                ..
            }
        ));
    }
    // The stream: the change first, then one `Withdrawn` per item, keyed to its item.
    let mut saw_change = false;
    let mut withdrawn_plans = Vec::new();
    for env in events.try_iter() {
        match env.event {
            Event::Policy(PolicyEvent::ControlStatusSet(_)) => {
                assert!(
                    withdrawn_plans.is_empty(),
                    "the change goes out before its effects"
                );
                saw_change = true;
            }
            Event::Command(CommandEvent::Withdrawn {
                plan,
                layer,
                status,
                ..
            }) => {
                assert_eq!(
                    (layer, status),
                    (EffectorLayer::Point, WeaponsControlStatus::Hold)
                );
                withdrawn_plans.push(plan);
            }
            _ => {}
        }
    }
    assert!(saw_change);
    withdrawn_plans.sort();
    assert_eq!(withdrawn_plans, [PlanId(7), PlanId(8)]);
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("withdrawn from the queue")),
        "{:?}",
        state.alerts
    );
    assert_eq!(state.desk.queue_outcomes.withdrawn, 2);
}

/// D-140: with the rule on, a second signed-in person who holds the action confirms; the
/// record names both; and every way of not being that person is refused by name.
#[test]
fn the_two_person_rule_needs_somebody_else_who_holds_the_action() {
    let dir = scratch("two-person");
    let mut state = desktop_with(&dir, 1, true);
    let events = state.events.subscribe();
    sign_in(&mut state, SUPERVISOR);
    let with = |confirmation: Option<Confirmation>| SetRequest {
        confirmation,
        ..request(
            EffectorLayer::Point,
            WeaponsControlStatus::Hold,
            "deconfliction",
        )
    };
    let confirm = |operator: OperatorId, passphrase: &str| {
        Some(Confirmation {
            operator: operator.0,
            passphrase: passphrase.to_owned(),
        })
    };

    assert_eq!(
        control_status::set(&mut state, with(None)),
        Err(SetError::TwoPersonRequired)
    );
    assert_eq!(
        control_status::set(&mut state, with(confirm(SUPERVISOR, PASSPHRASE))),
        Err(SetError::SameOperator)
    );
    assert_eq!(
        control_status::set(&mut state, with(confirm(COMMANDER, "wrong"))),
        Err(SetError::ConfirmationRejected)
    );
    assert_eq!(
        control_status::set(&mut state, with(confirm(OperatorId(99), PASSPHRASE))),
        Err(SetError::ConfirmationRejected),
        "an unknown operator and a wrong passphrase read the same (DN-23)"
    );
    assert_eq!(
        control_status::set(&mut state, with(confirm(OPERATOR, PASSPHRASE))),
        Err(SetError::ConfirmationNotPermitted {
            role: "Operator".into()
        })
    );
    assert_eq!(
        in_force(&state, EffectorLayer::Point),
        WeaponsControlStatus::Free
    );
    assert!(policy_events(&events).is_empty());
    assert!(audited(&state).is_empty());

    let outcome =
        control_status::set(&mut state, with(confirm(COMMANDER, PASSPHRASE))).expect("confirmed");
    let SetOutcome::Recorded { change, .. } = outcome else {
        panic!("{outcome:?}");
    };
    let confirmed = change
        .confirmed_by
        .expect("the record names the second person");
    assert_eq!(confirmed.operator, COMMANDER.0.to_string());
    assert_eq!(confirmed.role, "Commander");
    assert_eq!(change.operator, SUPERVISOR.0.to_string());
    assert_eq!(
        in_force(&state, EffectorLayer::Point),
        WeaponsControlStatus::Hold
    );
    let audit = audited(&state);
    assert_eq!(audit.len(), 1);
    assert!(
        audit[0].1.contains("confirmed by operator 52 (Commander)"),
        "{}",
        audit[0].1
    );
    // The supervisor is still the one signed in: confirming established no session.
    assert_eq!(state.signed_in().map(|s| s.operator), Some(SUPERVISOR));
}

/// The owner's choice of 2026-10-26: a change is folded from the journal at a restart,
/// over the baseline it was made under, and a later baseline revision supersedes it.
#[test]
fn the_status_survives_a_restart_under_the_same_revision_and_not_a_later_one() {
    let dir = scratch("restart");
    {
        let mut state = desktop(&dir);
        sign_in(&mut state, SUPERVISOR);
        control_status::set(
            &mut state,
            request(
                EffectorLayer::Point,
                WeaponsControlStatus::Hold,
                "deconfliction",
            ),
        )
        .expect("set");
        // And back to tight afterwards, so the fold has to take the latest, not the first.
        control_status::set(
            &mut state,
            request(EffectorLayer::Point, WeaponsControlStatus::Tight, "raid"),
        )
        .expect("set again");
        state.save_session().expect("the journal is on disk");
    }
    // The same revision: the latest change is in force before anyone signs in, and the
    // ledger says who set it.
    let restarted = desktop(&dir);
    assert_eq!(
        in_force(&restarted, EffectorLayer::Point),
        WeaponsControlStatus::Tight
    );
    assert_eq!(
        restarted
            .control_status_ledger
            .set_by
            .get(&EffectorLayer::Point)
            .map(|c| c.reason.as_str()),
        Some("raid")
    );
    assert!(
        restarted.control_status_ledger.this_session.is_empty(),
        "folded changes are the past session's, not this one's"
    );
    drop(restarted);
    // A later revision: the baseline's own value, as a change request under phase H.
    let superseded = desktop_with(&dir, 2, false);
    assert_eq!(
        in_force(&superseded, EffectorLayer::Point),
        WeaponsControlStatus::Free
    );
    assert!(superseded.control_status_ledger.set_by.is_empty());
}
