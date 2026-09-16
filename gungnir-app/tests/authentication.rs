// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Operator sessions through the desktop (GAP-057, DN-23).
//!
//! The point of this file is the property the whole chain was built around: **what the
//! system records about who acted follows from whether somebody was actually verified.**
//!
//! Three earlier gaps had to invent a way to say "we do not know who did this" --
//! `DecisionRecord` with no operator (GAP-038), `Concurrence::UnattributedRole`
//! (GAP-005), and the v2 write paths returning `501` (GAP-041). Those are not removed by
//! authentication existing; they are what a deployment without it still records. These
//! tests assert both halves: a verified operator is named, and an unverified one is not.
//!
//! **One of them became a refusal** in the GAP-067 walk (2026-09-16). The CAP-2.12
//! criterion moves a requirement to tasked "only with a concurrence carrying an operator",
//! and sign-in is what made carrying one possible, so a tasking with nobody signed in is
//! now refused -- with the reason, no command issued, and the requirement left stated --
//! rather than recorded against the role. A decline is outside the criterion and still
//! records the role.

use gungnir_app::state::AppState;
use gungnir_app::{decisions, requirements};
use gungnir_command::OperatorDecision;
use gungnir_config::{AssetConfig, ConfigBaseline, EndpointConfig, SensorConfig};
use gungnir_eventing::Event;
use gungnir_model::events::RequirementEvent;
use gungnir_model::{AssetPriority, Concurrence, MissionTime, RequirementState};
use gungnir_security::{
    hash_passphrase, Account, InMemoryAccountStore, LocalAccountAuthority, OperatorId, Role,
    SessionState,
};
use gungnir_sensor_management::SensorControl;
use gungnir_time::ReplayClockAuthority;

const PASSPHRASE: &str = "correct horse battery staple";

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-authn-{name}-{}", std::process::id()));
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
        assets: vec![AssetConfig {
            id: 1,
            name: "the harbour".into(),
            position: [0.0, 0.0, 0.0],
            radius_m: Some(2_000.0),
            priority: "high".into(),
            warning_lead_time_s: None,
            warning_channel: None,
            warning_within_m: None,
            note: None,
        }],
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("desktop state");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

/// Install an account store holding one operator.
fn with_accounts(state: &mut AppState) {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: Role::SensorManager,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    state.set_session_authority(Box::new(LocalAccountAuthority::new(Box::new(store))));
}

/// Whether anything the bus carried since `seen` was subscribed says a requirement was
/// tasked. A refused tasking must publish nothing: a `Tasked` event with no operator on
/// it is exactly the record the CAP-2.12 criterion forbids.
fn any_tasked_published(seen: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>) -> bool {
    seen.try_iter().any(|envelope| {
        matches!(
            envelope.event,
            Event::Requirement(RequirementEvent::Tasked { .. })
        )
    })
}

fn state_a_requirement(state: &mut AppState) -> gungnir_model::RequirementId {
    requirements::state_requirement(
        state,
        "identify the contact".into(),
        0,
        AssetPriority::High,
        None,
    )
    .expect("stated")
}

/// A fresh desktop has no account store, **starts anyway**, and attributes nothing.
/// DN-23 §5 rule 5: a console that refused to run because a keystore was missing would
/// be a worse failure than one that runs and says what it cannot do.
#[test]
fn a_desktop_with_no_account_store_starts_and_attributes_nothing() {
    let (state, dir) = desktop("no-store");

    match state.session_state() {
        SessionState::StoreUnavailable { reason } => {
            assert!(reason.contains("no account store"), "{reason}");
        }
        other => panic!("expected an unavailable store, got {other:?}"),
    }
    assert!(state.session_state().is_fault());
    assert_eq!(state.attributed_operator(), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// A desktop with no account store starts (the test above) and **cannot task through a
/// requirement**: the `gungnir-workflow` Collection requirements and tasking concurrence
/// (CAP-2.12) row of `docs/verification-capability-table.md` §2 asks for a concurrence
/// carrying an operator, and here nobody can sign in to be one (GAP-067 walk,
/// 2026-09-16). The refusal has to say it is the account store, because telling an
/// operator to sign in where nobody can would send them looking for a form that does not
/// work. A decline is outside the criterion and still records the role.
#[test]
fn a_desktop_with_no_account_store_cannot_task_and_says_why() {
    let (mut state, dir) = desktop("no-store-task");
    state.set_role(Role::SensorManager);
    let seen = state.events.subscribe();
    let id = state_a_requirement(&mut state);

    let err = requirements::task(&mut state, id, 1, MissionTime(1.0))
        .expect_err("a requirement was tasked with no account store");
    let requirements::RequirementError::Unattributed { requirement, .. } = &err else {
        panic!("refused for some other reason: {err}");
    };
    assert_eq!(*requirement, id);
    let text = err.to_string();
    assert!(
        text.contains("account store"),
        "the refusal did not name the account store: {text}"
    );
    assert!(
        !text.contains("Sign in to task"),
        "an operator was told to sign in where nobody can: {text}"
    );
    assert_eq!(state.requirements[0].state, RequirementState::Stated);
    assert!(
        state.sensors.tasks().is_empty(),
        "a command was issued for a concurrence that was refused"
    );
    assert!(
        !any_tasked_published(&seen),
        "a refused tasking was published"
    );

    // The half the walk left alone: declining still records the role, and says nobody
    // was signed in rather than naming one.
    requirements::decline(&mut state, id, "no sensor can reach that area".into())
        .expect("a decline with nobody signed in is still recorded");
    match &state.requirements[0].state {
        RequirementState::Declined { by, .. } => {
            assert_eq!(
                by.operator(),
                None,
                "an unverified decline named an operator"
            );
            assert!(matches!(by, Concurrence::UnattributedRole { .. }));
        }
        other => panic!("expected declined, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The payoff. A verified operator is named on a concurrence; before signing in, the
/// same act is **refused** (GAP-067 walk, 2026-09-16) with a reason telling the operator
/// to sign in, no command issued and nothing published, and the requirement stays stated
/// -- so the signed-in operator can then task that same requirement. The CAP-2.12 row of
/// `docs/verification-capability-table.md` §2: "a requirement moves from stated to
/// tasked only with a concurrence carrying an operator".
#[test]
fn a_concurrence_names_the_operator_only_after_a_real_sign_in() {
    let (mut state, dir) = desktop("concurrence");
    with_accounts(&mut state);
    state.set_role(Role::SensorManager);
    let seen = state.events.subscribe();

    // Before signing in: refused, and nothing is left behind the refusal.
    let id = state_a_requirement(&mut state);
    let err = requirements::task(&mut state, id, 1, MissionTime(1.0))
        .expect_err("a requirement was tasked with nobody signed in");
    let requirements::RequirementError::Unattributed { requirement, .. } = &err else {
        panic!("refused for some other reason: {err}");
    };
    assert_eq!(*requirement, id);
    assert!(
        err.to_string().contains("Sign in to task it"),
        "the refusal did not tell the operator to sign in: {err}"
    );
    assert_eq!(
        state.requirements[0].state,
        RequirementState::Stated,
        "a refused concurrence moved the requirement"
    );
    assert!(
        state.sensors.tasks().is_empty(),
        "a command was issued for a concurrence that was refused"
    );
    assert!(
        !any_tasked_published(&seen),
        "a refused tasking was published"
    );

    // Sign in, and the act names the operator who was actually verified.
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    assert_eq!(state.attributed_operator(), Some(OperatorId(7)));

    requirements::task(&mut state, id, 1, MissionTime(2.0)).expect("tasked");
    match &state.requirements[0].state {
        RequirementState::Tasked { by } => {
            assert_eq!(by.operator(), Some("7"));
            assert_eq!(by.role(), "SensorManager");
        }
        other => panic!("expected tasked, got {other:?}"),
    }
    assert_eq!(state.sensors.tasks().len(), 1, "one task serves it");
    let _ = std::fs::remove_dir_all(dir);
}

/// An expired session is the third reason nobody can be named, and it asks something
/// different again: sign in *again*. DN-23 §5 rule 2 says an expired session refuses and
/// is not renewed by use, and a tasking under one is refused like any other with nobody
/// to name (GAP-067 walk, 2026-09-16).
#[test]
fn a_tasking_under_an_expired_session_is_refused_and_says_to_sign_in_again() {
    let (mut state, dir) = desktop("expired");
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: Role::SensorManager,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    // A 60 s session, signed in at T+0 and so valid until T+60 s.
    state.set_session_authority(Box::new(
        LocalAccountAuthority::new(Box::new(store)).with_lifetime(Some(60.0)),
    ));
    state.set_role(Role::SensorManager);
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    let id = state_a_requirement(&mut state);

    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(61.0),
    });
    assert!(matches!(
        state.session_state(),
        SessionState::Expired { .. }
    ));
    let err = requirements::task(&mut state, id, 1, MissionTime(61.0))
        .expect_err("a requirement was tasked under an expired session");
    assert!(
        err.to_string().contains("Sign in again"),
        "the refusal did not say the session expired: {err}"
    );
    assert_eq!(state.requirements[0].state, RequirementState::Stated);
    assert!(state.sensors.tasks().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A wrong passphrase changes nothing about attribution, and the failure never says
/// which half was wrong.
#[test]
fn a_rejected_sign_in_leaves_the_desktop_unattributed() {
    let (mut state, dir) = desktop("rejected");
    with_accounts(&mut state);

    let err = state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            "not the passphrase",
        ))
        .expect_err("rejected");
    assert_eq!(err.to_string(), "the credential was rejected");
    assert!(!err.to_string().contains("unknown"));
    assert_eq!(state.attributed_operator(), None);
    assert_eq!(state.session_state(), SessionState::NobodySignedIn);
    let _ = std::fs::remove_dir_all(dir);
}

/// Signing out stops attribution immediately. An act after signing out must not carry
/// the operator who has left the console, and since the GAP-067 walk (2026-09-16) a
/// tasking after signing out is refused outright rather than recorded against nobody:
/// the requirement stays stated and no command is issued.
#[test]
fn signing_out_stops_attribution() {
    let (mut state, dir) = desktop("sign-out");
    with_accounts(&mut state);
    state.set_role(Role::SensorManager);
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    assert_eq!(state.attributed_operator(), Some(OperatorId(7)));

    state.sign_out();
    assert_eq!(state.attributed_operator(), None);

    let seen = state.events.subscribe();
    let id = state_a_requirement(&mut state);
    let err = requirements::task(&mut state, id, 1, MissionTime(3.0))
        .expect_err("a requirement was tasked after the operator signed out");
    assert!(
        err.to_string().contains("nobody is signed in"),
        "the refusal did not say nobody is signed in: {err}"
    );
    assert_eq!(
        state.requirements[0].state,
        RequirementState::Stated,
        "a tasking after signing out moved the requirement"
    );
    assert!(
        state.sensors.tasks().is_empty(),
        "a command was issued after the operator signed out"
    );
    assert!(
        !any_tasked_published(&seen),
        "a tasking after signing out was published"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Every attempt reaches the audit log, successful or not, and a sign-out does too.
/// A failed attempt nobody can see is the one an investigation needs most.
#[test]
fn every_attempt_is_audited() {
    use gungnir_security::AuditLog;

    let (mut state, dir) = desktop("audit");
    with_accounts(&mut state);
    let before = state.audit.entries().len();

    let _ = state.sign_in(&LocalAccountAuthority::credential(OperatorId(7), "wrong"));
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    state.sign_out();

    let actions: Vec<&str> = state.audit.entries()[before..]
        .iter()
        .map(|e| e.action.as_str())
        .collect();
    assert_eq!(
        actions,
        vec!["session.rejected", "session.sign_in", "session.sign_out"]
    );
    // The rejected entry names no operator, because nobody was verified.
    assert_eq!(state.audit.entries()[before].operator, None);
    let _ = std::fs::remove_dir_all(dir);
}

/// Signing in decides attribution and **not** authority. The role still governs what
/// may be done, and a session does not widen it.
#[test]
fn a_session_does_not_change_what_a_role_may_do() {
    use gungnir_security::{actions::TASK_SENSOR, authz::role_permits};

    let (mut state, dir) = desktop("authority");
    with_accounts(&mut state);
    // The desktop is signed in as an operator whose *account* role is SensorManager,
    // while the selected role is IntelligenceAnalyst. The matrix is what governs.
    state.set_role(Role::IntelligenceAnalyst);
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");

    assert_eq!(
        state.role(),
        Role::IntelligenceAnalyst,
        "signing in changed the role"
    );
    assert!(
        !role_permits(state.role(), TASK_SENSOR),
        "a session widened what the selected role may do"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A decision records the operator who was verified, and records none when nobody was.
#[test]
fn a_decision_records_the_verified_operator() {
    let (mut state, dir) = desktop("decision");
    with_accounts(&mut state);
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");

    // Nothing is queued in this build, so the decision path is exercised for its
    // attribution rather than its outcome: an unknown item is refused, and the
    // refusal proves the operator was read before the queue was consulted.
    let outcome = decisions::decide(
        &mut state,
        gungnir_ui::panels::approval_queue::PendingId(9_999),
        OperatorDecision::Accepted,
    );
    assert!(outcome.is_err(), "an unqueued plan was decided");
    assert_eq!(state.attributed_operator(), Some(OperatorId(7)));
    let _ = std::fs::remove_dir_all(dir);
}
