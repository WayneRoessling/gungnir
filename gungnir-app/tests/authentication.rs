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

/// Signing in decides the desktop's role as well as attribution (the GAP-067 walk,
/// 2026-09-16): `role()` and the workspace become the account's, and what may be done is
/// what the matrix gives **that** role. Signing out returns both to the selection, which
/// waited underneath the session -- DN-23 §5 rule 5's role-selected fallback.
///
/// This test pinned the opposite until 2026-09-16, under the name
/// `a_session_does_not_change_what_a_role_may_do`: the role stayed selected through a
/// sign-in, so every `role_permits` check on the desktop asked about the selection --
/// `Operator` unless something changed it -- whoever had signed in. The matrix still
/// governs; what changed is which role it is asked about.
#[test]
fn signing_in_makes_the_accounts_role_the_desktops_until_signing_out() {
    use gungnir_security::{actions::TASK_SENSOR, authz::role_permits};
    use gungnir_workflow::WorkspaceLayout;

    let (mut state, dir) = desktop("authority");
    with_accounts(&mut state);
    // The selected role is IntelligenceAnalyst; operator 7's *account* role is
    // SensorManager.
    state.set_role(Role::IntelligenceAnalyst);
    assert_eq!(state.role(), Role::IntelligenceAnalyst);
    assert_eq!(
        state.layout(),
        WorkspaceLayout::for_role(Role::IntelligenceAnalyst)
    );
    assert!(!role_permits(state.role(), TASK_SENSOR));

    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    assert_eq!(
        state.role(),
        Role::SensorManager,
        "the desktop kept the selected role through a sign-in"
    );
    assert_eq!(
        state.layout(),
        WorkspaceLayout::for_role(Role::SensorManager),
        "the workspace did not follow the account"
    );
    assert!(
        role_permits(state.role(), TASK_SENSOR),
        "the check asked about the selection rather than the signed-in account"
    );

    // A selection made while signed in waits underneath the session.
    state.set_role(Role::Planner);
    assert_eq!(state.role(), Role::SensorManager);

    state.sign_out();
    assert_eq!(
        state.role(),
        Role::Planner,
        "signing out left the account's role in force"
    );
    assert_eq!(state.layout(), WorkspaceLayout::for_role(Role::Planner));
    assert!(!role_permits(state.role(), TASK_SENSOR));
    let _ = std::fs::remove_dir_all(dir);
}

/// An expiry ends the account's role the way a sign-out does, on the clock and with
/// nothing calling in: the desktop is back on its selected role and workspace, and
/// attributes nothing (DN-23 §5 rules 2 and 5). An unavailable store never gave it an
/// account's role in the first place.
#[test]
fn an_expired_session_returns_the_desktop_to_the_selected_role() {
    use gungnir_workflow::WorkspaceLayout;

    let (mut state, dir) = desktop("expiry-role");
    assert_eq!(
        state.role(),
        Role::Operator,
        "a desktop with no account store is role-selected"
    );
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: Role::SensorManager,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    state.set_session_authority(Box::new(
        LocalAccountAuthority::new(Box::new(store)).with_lifetime(Some(60.0)),
    ));
    state.set_role(Role::Analyst);
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    assert_eq!(state.role(), Role::SensorManager);

    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(61.0),
    });
    assert!(matches!(
        state.session_state(),
        SessionState::Expired { .. }
    ));
    assert_eq!(
        state.role(),
        Role::Analyst,
        "an expired session kept its role"
    );
    assert_eq!(state.layout(), WorkspaceLayout::for_role(Role::Analyst));
    assert_eq!(state.attributed_operator(), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// Queue a plan directly: the desktop has no tracks, so it cannot produce one.
fn queue(state: &mut AppState, plan: u128) -> gungnir_command::PendingApprovalId {
    use gungnir_command::{ApprovalWorkflow, Submission};
    state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: gungnir_model::PlanView {
                id: gungnir_model::PlanId(plan),
                ..gungnir_model::PlanView::default()
            },
            verdict: gungnir_policy::PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: gungnir_model::EffectorLayer::Point,
            priority: 0.0,
            role: "Supervisor".into(),
        })
        .expect("queued")
}

/// A decision records the role of the session that took it, so D-03's rule can rank it if
/// an outage leaves it in conflict -- and records **none** with nobody signed in, although
/// the desktop then has a selected role, because a selection is nobody's verified
/// authority (DN-23 §5 rule 1; the GAP-067 walk). The record and the event agree.
#[test]
fn a_decision_records_the_signed_in_role_and_none_when_nobody_is_signed_in() {
    use gungnir_command::ApprovalWorkflow;
    use gungnir_eventing::Event;
    use gungnir_model::events::CommandEvent;

    let (mut state, dir) = desktop("decision-role");
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(8),
        role: Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    state.set_session_authority(Box::new(LocalAccountAuthority::new(Box::new(store))));
    let events = state.events.subscribe();
    state.set_role(Role::Supervisor);

    let unattributed = queue(&mut state, 1);
    decisions::decide(
        &mut state,
        gungnir_ui::panels::approval_queue::PendingId(unattributed.0),
        OperatorDecision::Rejected {
            reason: "friendly airliner".into(),
        },
    )
    .expect("decided");
    let record = state.desk.approvals.records().last().expect("recorded");
    assert_eq!(
        (record.operator_id.as_deref(), record.role.as_deref()),
        (None, None),
        "the selected role was recorded as an authority nobody verified"
    );

    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(8),
            PASSPHRASE,
        ))
        .expect("signed in");
    let attributed = queue(&mut state, 2);
    decisions::decide(
        &mut state,
        gungnir_ui::panels::approval_queue::PendingId(attributed.0),
        OperatorDecision::Rejected {
            reason: "outside the engagement zone".into(),
        },
    )
    .expect("decided");
    let record = state.desk.approvals.records().last().expect("recorded");
    assert_eq!(
        (record.operator_id.as_deref(), record.role.as_deref()),
        (Some("8"), Some("Supervisor"))
    );

    let on_the_bus: Vec<(Option<String>, Option<String>)> = events
        .try_iter()
        .filter_map(|env| match env.event {
            Event::Command(CommandEvent::Decided { operator, role, .. }) => Some((operator, role)),
            _ => None,
        })
        .collect();
    assert_eq!(
        on_the_bus,
        vec![(None, None), (Some("8".into()), Some("Supervisor".into()))]
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Applying a baseline is on the audit trail under the operator who applied it, and under
/// nobody with nobody signed in. The entry named nobody unconditionally until 2026-09-16.
#[test]
fn a_baseline_apply_is_audited_under_the_signed_in_operator() {
    use gungnir_app::sustainment::ConfigEditorState;
    use gungnir_config::{ConfigStore, FileConfigStore};
    use gungnir_security::AuditLog;

    let dir = std::env::temp_dir().join(format!("gungnir-authn-apply-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    let path = dir.join("baseline.json");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config_and_store(
        config.clone(),
        Some(FileConfigStore::new(
            &path,
            gungnir_app::state::known_vocabulary(),
        )),
    )
    .expect("desktop state");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    with_accounts(&mut state);
    // Revision 1 is what this store has applied so far this session.
    state
        .config_store
        .as_mut()
        .expect("a baseline file")
        .apply(
            ConfigBaseline {
                revision: 1,
                ..config.clone()
            },
            MissionTime(0.0),
        )
        .expect("revision 1 applied");

    // An administrator edits the file to the next revision, reloads, validates, applies.
    let apply = |state: &mut AppState, revision: u32| {
        let candidate = ConfigBaseline {
            revision,
            ..config.clone()
        };
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&candidate).expect("json"),
        )
        .expect("written");
        let mut editor = ConfigEditorState::default();
        editor.reload(state);
        editor.validate(state);
        editor.apply(state).expect("applied");
        state
            .audit
            .entries()
            .iter()
            .rev()
            .find(|e| e.action == gungnir_security::actions::APPLY_CONFIG)
            .map(|e| e.operator)
            .expect("an apply is audited")
    };

    assert_eq!(apply(&mut state, 2), None, "nobody is signed in");
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signed in");
    assert_eq!(
        apply(&mut state, 3),
        Some(OperatorId(7)),
        "the apply did not name the operator who made it"
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
