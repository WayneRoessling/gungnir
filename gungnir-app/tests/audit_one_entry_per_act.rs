// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Every audited act on the desktop grows its audit log by **exactly one** entry naming
//! that act (GAP-111; the `gungnir-security` row of `docs/verification-capability-table.md`
//! §2, its third clause). `audit_trail.rs` (GAP-059) asks that each act leave *a* row; this
//! asks that it leave one and only one, act by act, and then that the file on disk holds
//! the same entries as one intact chain.
//!
//! **The acts are every call site that writes an entry**, and
//! [`every_audit_site_is_an_act_this_file_or_another_performs`] reads the sources to keep it
//! so: a site added without a row here fails that test by name.
//!
//! One site is performed elsewhere, because its setup is an outage and a reconnection:
//! resolving a reconciliation conflict (`failover.rs`), checked for exactly one entry in
//! `gungnir-app/tests/failover.rs`. The desk's forwarded-decision site is the node's, and
//! `gungnir-node/tests/approval_queue.rs` counts it.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::{self, ConfigEditorState, SustainmentState};
use gungnir_app::{decisions, handoffs, launch_warning, requirements, review, session, warnings};
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::{
    AssetConfig, AuthenticationConfig, AuthenticationProvider, ConfigBaseline, EndpointConfig,
    FileConfigStore, ResourceConfig, SecurityConfig, SensorConfig,
};
use gungnir_model::handoff::{DecisionAttribution, DeliveryState, EffectorReport, Handoff};
use gungnir_model::{
    AssetId, AssetPriority, DecisionId, EffectorLayer, MissionTime, PlanId, PlanKind, PlanView,
    Releasability, SensorMode, TrackId,
};
use gungnir_policy::PolicyVerdict;
use gungnir_security::audit::events;
use gungnir_security::{
    actions, hash_passphrase, verify_audit_dir, Account, AuditLog, OperatorId, Role,
};
use gungnir_ui::panels::approval_queue::PendingId;
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use gungnir_ui::panels::reports::{FindingKindView, ReviewAction};

const PASSPHRASE: &str = "correct horse battery staple";
const SUPERVISOR: u64 = 11;
const SENSOR_MANAGER: u64 = 12;
const ADMINISTRATOR: u64 = 13;
const ANALYST: u64 = 14;

fn baseline(dir: &std::path::Path, revision: u32) -> ConfigBaseline {
    ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        revision,
        security: SecurityConfig {
            authentication: AuthenticationConfig {
                provider: AuthenticationProvider::LocalAccounts {
                    accounts_path: "accounts.json".into(),
                },
                session_lifetime_s: None,
            },
            ..SecurityConfig::default()
        },
        sensors: vec![SensorConfig {
            id: 1,
            modality: "radar".into(),
            position: [0.0, 0.0, 10.0],
            max_range_m: 20_000.0,
            azimuth_sector: None,
            // Commandable, and nothing acknowledges: a timeout later, an entry now.
            control_endpoint: Some("radar-1-control".into()),
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
        assets: vec![AssetConfig {
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
        endpoints: vec![EndpointConfig {
            name: "radar-1-control".into(),
            kind: "sensor-control".into(),
            address: "tcp://127.0.0.1:9100".into(),
        }],
        ..ConfigBaseline::default()
    }
}

/// A desktop started from a baseline file, with four accounts to sign in as.
fn desktop() -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-audit-per-act-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    let accounts: Vec<Account> = [
        (SUPERVISOR, Role::Supervisor),
        (SENSOR_MANAGER, Role::SensorManager),
        (ADMINISTRATOR, Role::Administrator),
        (ANALYST, Role::Analyst),
    ]
    .into_iter()
    .map(|(operator, role)| Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    })
    .collect();
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts).expect("json"),
    )
    .expect("accounts written");
    let config = baseline(&dir, 1);
    gungnir_config::validate(&config).expect("valid");
    let path = dir.join("baseline.json");
    std::fs::write(&path, serde_json::to_string_pretty(&config).expect("json"))
        .expect("baseline written");
    let state = AppState::with_config_and_store(
        config,
        Some(FileConfigStore::new(
            &path,
            gungnir_app::state::known_vocabulary(),
        )),
    )
    .expect("the desktop starts");
    (state, dir)
}

/// Perform `act` and assert the log grew by exactly one entry, naming `action`, and
/// attributed to `operator`.
fn exactly_one(
    state: &mut AppState,
    what: &str,
    action: &str,
    operator: Option<u64>,
    act: impl FnOnce(&mut AppState),
) {
    let before = state.audit.entries().len();
    act(state);
    let after = state.audit.entries();
    assert_eq!(
        after.len(),
        before + 1,
        "{what}: expected exactly one entry, got {:?}; alerts {:?}",
        &after[before..],
        state.alerts
    );
    let entry = &after[before];
    assert_eq!(entry.action, action, "{what}: {entry:?}");
    assert_eq!(
        entry.operator,
        operator.map(OperatorId),
        "{what}: {entry:?}"
    );
}

fn sign_in(state: &mut AppState, operator: u64, passphrase: &str) {
    let mut draft = SignInDraft {
        operator: operator.to_string(),
        passphrase: passphrase.into(),
        ..SignInDraft::default()
    };
    session::apply(state, &mut draft, SessionAction::SignIn);
}

fn sign_out(state: &mut AppState) {
    session::apply(state, &mut SignInDraft::default(), SessionAction::SignOut);
}

fn handoff(decision: u128) -> Handoff {
    Handoff::from_decision(
        DecisionId(decision),
        PlanId(decision),
        PlanKind::Intercept {
            solutions: Vec::new(),
        },
        DecisionAttribution {
            operator: "11".into(),
            role: "Supervisor".into(),
            at: MissionTime(90.0),
            authority_rule: None,
        },
        Vec::new(),
        Releasability::Internal,
        MissionTime(90.0),
    )
}

#[test]
#[allow(clippy::too_many_lines)] // One act after another is the point: it reads as the list.
fn every_audited_act_on_the_desktop_leaves_exactly_one_entry_naming_it() {
    let (mut state, dir) = desktop();
    let mut sustainment = SustainmentState::default();
    let s = &mut state;

    // Sessions.
    exactly_one(
        s,
        "a refused sign-in",
        events::SIGN_IN_REJECTED,
        None,
        |s| {
            sign_in(s, 99, "not the passphrase");
        },
    );
    exactly_one(s, "a sign-in", events::SIGN_IN, Some(SUPERVISOR), |s| {
        sign_in(s, SUPERVISOR, PASSPHRASE);
    });

    // A decision.
    exactly_one(
        s,
        "a decision",
        actions::DECIDE_PLAN,
        Some(SUPERVISOR),
        |s| {
            let pending = s
                .desk
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
                    role: "Supervisor".into(),
                })
                .expect("queued");
            decisions::decide(s, PendingId(pending.0), OperatorDecision::Accepted)
                .expect("decided");
        },
    );
    // A sensor command.
    exactly_one(
        s,
        "a sensor command",
        actions::TASK_SENSOR,
        Some(SUPERVISOR),
        |s| {
            sustainment::command_sensor_mode(s, 1, SensorMode::Search).expect("commanded");
        },
    );
    // A launch warning, which is a product released.
    exactly_one(
        s,
        "a launch warning",
        actions::RELEASE_PRODUCT,
        Some(SUPERVISOR),
        |s| {
            launch_warning::declare(s, "a launch seen from the ridge", Releasability::Internal)
                .expect("declared");
        },
    );
    // A warned party's acknowledgement, keyed in.
    exactly_one(
        s,
        "an acknowledgement",
        actions::ACKNOWLEDGE_WARNING,
        Some(SUPERVISOR),
        |s| {
            let now = s.clock.now();
            warnings::apply_acknowledgement(s, AssetId(1), TrackId(7), "the harbour master", now);
        },
    );
    // An effector's report on a handoff this desktop issued.
    exactly_one(
        s,
        "an effector report",
        actions::EFFECTOR_REPORT,
        Some(SUPERVISOR),
        |s| {
            s.desk.handoffs.push(handoffs::HandoffRecord {
                handoff: handoff(9),
                endpoint: Some("battery".into()),
                delivery: DeliveryState::Delivered {
                    at: MissionTime(91.0),
                },
                reports: Vec::new(),
            });
            handoffs::apply_report(
                s,
                DecisionId(9),
                "battery",
                &EffectorReport::Refused {
                    at: MissionTime(95.0),
                    reason: "out of range".into(),
                },
                MissionTime(95.0),
            );
        },
    );
    // A baseline applied: the file is edited to revision 2, then validated and applied.
    exactly_one(
        s,
        "a baseline applied",
        actions::APPLY_CONFIG,
        Some(SUPERVISOR),
        |s| {
            std::fs::write(
                dir.join("baseline.json"),
                serde_json::to_string_pretty(&baseline(&dir, 2)).expect("json"),
            )
            .expect("edited");
            let mut editor = ConfigEditorState::default();
            editor.reload(s);
            editor.validate(s);
            editor.apply(s).expect("applied");
        },
    );
    // Requirements: stated, then tasked and declined by the sensor manager, satisfied.
    let mut stated = Vec::new();
    for title in ["identify the contact", "watch the approach"] {
        exactly_one(
            s,
            "a requirement stated",
            actions::REQUIREMENT,
            Some(SUPERVISOR),
            |s| {
                stated.push(
                    requirements::state_requirement(
                        s,
                        title.into(),
                        0,
                        AssetPriority::Medium,
                        None,
                    )
                    .expect("stated"),
                );
            },
        );
    }
    exactly_one(
        s,
        "a sign-out",
        events::SIGN_OUT,
        Some(SUPERVISOR),
        sign_out,
    );
    exactly_one(s, "a sign-in", events::SIGN_IN, Some(SENSOR_MANAGER), |s| {
        sign_in(s, SENSOR_MANAGER, PASSPHRASE);
    });
    exactly_one(
        s,
        "a requirement tasked",
        actions::TASK_SENSOR,
        Some(SENSOR_MANAGER),
        |s| {
            let now = s.clock.now();
            requirements::task(s, stated[0], 1, now).expect("tasked");
        },
    );
    exactly_one(
        s,
        "a requirement declined",
        actions::REQUIREMENT,
        Some(SENSOR_MANAGER),
        |s| {
            requirements::decline(s, stated[1], "no sensor covers it".into()).expect("declined");
        },
    );
    exactly_one(
        s,
        "a requirement satisfied",
        actions::REQUIREMENT,
        Some(SENSOR_MANAGER),
        |s| {
            requirements::satisfy(s, stated[0], "track 7 identified".into()).expect("satisfied");
        },
    );
    sign_out(s);

    // An after-action review, each of its five acts.
    sign_in(s, ANALYST, PASSPHRASE);
    for (what, action) in [
        ("a review opened", ReviewAction::Open),
        (
            "a finding recorded",
            ReviewAction::RecordFinding {
                summary: "the queue got behind".into(),
                kind: FindingKindView::SystemBehaviour,
            },
        ),
        (
            "a finding promoted",
            ReviewAction::Promote {
                finding: 1,
                gap: "GAP-034".into(),
            },
        ),
        ("a review concluded", ReviewAction::Conclude),
        ("a review closed", ReviewAction::Close),
    ] {
        exactly_one(s, what, actions::REVIEW_CONDUCT, Some(ANALYST), |s| {
            review::apply(s, &mut sustainment, action);
        });
    }
    sign_out(s);

    // A role assigned to an account.
    sign_in(s, ADMINISTRATOR, PASSPHRASE);
    exactly_one(
        s,
        "a role assigned",
        actions::ASSIGN_ROLE,
        Some(ADMINISTRATOR),
        |s| {
            session::assign_role(s, ANALYST, "Supervisor");
        },
    );

    // The file holds what the log holds, as one chain, and no passphrase.
    let held = state.audit.entries().len();
    let audit = dir.join(gungnir_security::AUDIT_DIR);
    let verified = verify_audit_dir(&audit).expect("the audit log reads");
    assert!(verified.intact(), "{:?}", verified.breaks);
    assert_eq!(verified.entries, u64::try_from(held).expect("a count"));
    let mut text = String::new();
    for entry in std::fs::read_dir(&audit).expect("the audit directory") {
        text.push_str(&std::fs::read_to_string(entry.expect("entry").path()).expect("read"));
    }
    assert!(
        !text.contains(PASSPHRASE),
        "a passphrase reached the audit log"
    );
    assert!(
        !text.contains("not the passphrase"),
        "a tried passphrase reached it"
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

/// The audit log outlives the window (GAP-111, D-87): a desktop started again on the same
/// data directory continues the chain rather than starting a record of its own.
#[test]
fn the_desktop_audit_log_survives_a_restart_as_one_chain() {
    let dir = std::env::temp_dir().join(format!("gungnir-audit-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    for _ in 0..2 {
        let mut state = AppState::with_config(config.clone()).expect("the desktop starts");
        review::apply(
            &mut state,
            &mut SustainmentState::default(),
            ReviewAction::Open,
        );
        assert_eq!(state.audit.entries().len(), 1, "{:?}", state.alerts);
    }
    let verified = verify_audit_dir(&dir.join(gungnir_security::AUDIT_DIR)).expect("read");
    assert!(verified.intact(), "{:?}", verified.breaks);
    assert_eq!((verified.segments, verified.entries), (2, 2));
    let _ = std::fs::remove_dir_all(dir);
}

/// Every call site that writes an audit entry, per file, and where its act is performed.
///
/// Counted from the sources, so a new site fails here until it has an act above (or a
/// named test elsewhere) and a line below.
#[test]
#[allow(clippy::too_many_lines)] // The list of sites is the test; splitting it would hide one.
fn every_audit_site_is_an_act_this_file_or_another_performs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let sites: &[(&str, &str, usize, &str)] = &[
        (
            "gungnir-app/src/state.rs",
            "gungnir_security::audit_attempt(",
            1,
            "a sign-in and a refused one",
        ),
        (
            "gungnir-app/src/state.rs",
            "gungnir_security::audit_sign_out(",
            1,
            "a sign-out",
        ),
        (
            "gungnir-app/src/launch_warning.rs",
            "crate::audit::record(",
            1,
            "a launch warning",
        ),
        (
            "gungnir-app/src/warnings.rs",
            "crate::audit::record(",
            1,
            "an acknowledgement",
        ),
        (
            "gungnir-app/src/sustainment.rs",
            "crate::audit::record(",
            2,
            "a baseline applied; a sensor command",
        ),
        (
            "gungnir-app/src/requirements.rs",
            "crate::audit::record(",
            4,
            "stated, tasked, declined, satisfied",
        ),
        (
            "gungnir-app/src/review.rs",
            "crate::audit::record(",
            5,
            "opened, finding, concluded, closed, promoted",
        ),
        (
            "gungnir-app/src/session.rs",
            "crate::audit::record(",
            1,
            "a role assigned",
        ),
        (
            "gungnir-app/src/failover.rs",
            "crate::audit::record(",
            1,
            "tests/failover.rs: a conflict resolved",
        ),
        (
            "gungnir-approval/src/queue.rs",
            "host.audit(",
            2,
            "a decision; a forwarded decision (gungnir-node/tests/approval_queue.rs)",
        ),
        (
            "gungnir-approval/src/handoffs.rs",
            "host.audit(",
            1,
            "an effector report",
        ),
    ];
    for (file, call, expected, act) in sites {
        let text =
            std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(
            text.matches(call).count(),
            *expected,
            "{file} has a different number of `{call}` sites than this list, whose acts are: \
             {act}. Perform the new act in this file and count it here"
        );
    }
    // No file outside the list writes one.
    for dir in ["gungnir-app/src", "gungnir-approval/src"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("a source directory") {
            let path = entry.expect("entry").path();
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let name = format!(
                "{dir}/{}",
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
            );
            let writes = [
                "crate::audit::record(",
                "host.audit(",
                "audit_attempt(",
                "audit_sign_out(",
            ]
            .iter()
            .any(|call| text.contains(call));
            if writes && name != "gungnir-app/src/audit.rs" {
                assert!(
                    sites.iter().any(|(file, ..)| *file == name),
                    "{name} writes audit entries and is not in this list"
                );
            }
        }
    }
}
