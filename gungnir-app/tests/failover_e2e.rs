// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The disconnected-reconciliation row against a real transport (GAP-050, the
//! cross-layer row of `docs/verification-capability-table.md` §2): a desktop signed in
//! to a real node over the real client, the node goes down, the desktop falls back and
//! decides on its own, the node comes back on the same address, and the reconciliation
//! runs over the node's real `GET /v3/history`, drops the duplicate the two records share,
//! and settles the conflicting decision by D-03's arbitration rule with no person asked
//! (the GAP-067 walk, 2026-09-16), after which the switch back needs nobody's resolution.
//!
//! A second test (GAP-143) signs in again during the outage and shows the outage survives
//! it: only a person switching back ends one (D-15).
//!
//! Wall-clock by necessity: the link judges silence by when it last heard the node, so
//! each outage is a real `HEARTBEAT_TIMEOUT` of silence. Two tests, a dozen seconds each.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_app::failover::{self, ReconciliationView};
use gungnir_app::state::AppState;
use gungnir_app::{decisions, session, update};
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, SecurityConfig,
};
use gungnir_eventing::{Envelope, Event};
use gungnir_model::arbitration::ArbitrationGround;
use gungnir_model::events::{CommandEvent, LinkEvent, VerdictSummary};
use gungnir_model::{DecisionId, EffectorLayer, MissionTime, PlanId, PlanView, SystemHealth};
use gungnir_remote::link::HEARTBEAT_TIMEOUT;
use gungnir_security::{
    hash_passphrase, Account, AuditLog, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use std::sync::Arc;

const PASSPHRASE: &str = "correct horse battery staple";

/// A decision as a record holds it, with the operator and the role the deciding session
/// carried.
fn decided(plan: u128, accepted: bool, operator: &str, role: &str) -> Event {
    Event::Command(CommandEvent::Decided {
        plan: PlanId(plan),
        decision: DecisionId(plan),
        accepted,
        operator: Some(operator.into()),
        role: Some(role.into()),
        verdict: VerdictSummary::RequiresHumanApproval,
        rationale: (!accepted).then(|| "no".to_string()),
        request: None,
        origin: None,
    })
}

/// A node that authenticates operator 7.
fn node_api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![3u8; 32], 300.0).expect("issuer");
    Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        ))),
    )
}

/// Serve `api` on `addr` on a runtime of its own; dropping the runtime is the node
/// going down, every connection included.
fn serve(addr: std::net::SocketAddr, api: Arc<NodeApi>) -> tokio::runtime::Runtime {
    let node = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("node runtime");
    node.block_on(async {
        let listener = gungnir_api::transport::bind(addr).await.expect("bound");
        tokio::spawn(async move {
            let _ = gungnir_api::transport::serve_on(listener, api).await;
        });
    });
    node
}

/// A desktop configured for the node at `endpoint`, in a data directory of its own:
/// `name` keeps two tests in this binary, which run as threads of one process, from
/// sharing a journal.
fn desktop(endpoint: &str, name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-failover-e2e-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    let accounts = vec![Account {
        operator: OperatorId(7),
        role: Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }];
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts).expect("json"),
    )
    .expect("written");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        backend: BackendConfig::Remote {
            endpoint: endpoint.to_owned(),
        },
        security: SecurityConfig {
            authentication: AuthenticationConfig {
                provider: AuthenticationProvider::LocalAccounts {
                    accounts_path: "accounts.json".into(),
                },
                ..AuthenticationConfig::default()
            },
            ..SecurityConfig::default()
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

/// Tick until `check` holds, or fail with what was seen.
fn until(state: &mut AppState, what: &str, seconds: f64, mut check: impl FnMut(&AppState) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
    loop {
        update::tick(state);
        if check(state) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}; alerts: {:?}",
            state.alerts
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
// One outage told start to finish against a real node: splitting it would hide the order
// the record is built in, which is what the test is about.
#[allow(clippy::too_many_lines)]
fn an_outage_against_a_real_node_is_reconciled_over_the_real_history_route() {
    // A fixed loopback port, so the node can come back on the address the desktop has.
    let addr: std::net::SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    let api = node_api();
    let node = serve(addr, Arc::clone(&api));
    let (mut state, dir) = desktop(&format!("http://{addr}"), "reconciled");
    let events = state.events.subscribe();

    // Sign in: DN-23 §5, the connected profile's link is established by the sign-in.
    let mut draft = SignInDraft {
        operator: "7".into(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(&mut state, &mut draft, SessionAction::SignIn);
    assert!(
        matches!(state.backend, BackendConfig::Remote { .. }),
        "{:?}",
        state.alerts
    );
    // The desktop acts in the account's role; nothing had to select it.
    assert_eq!(state.role(), Role::Supervisor);
    until(&mut state, "the link to come up", 10.0, |s| {
        s.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected)
    });

    // **Connected is not subscribed, and this test used to assume it was.** The wait
    // above is satisfied as soon as the link's *snapshot* has been answered over HTTP,
    // which `NodeLink` does before it opens its WebSocket and sends the subscribe frame
    // (`gungnir-remote/src/link.rs`: `p.connected = true` precedes `open_stream`). An
    // envelope published into that window reaches no live receiver, and it is not
    // recovered afterwards either: the subscription carries `from_seq` 0, which means
    // "everything from now" by the contract, so `backlog_since` returns an empty
    // backlog rather than replaying it. The envelope is simply gone, and the test then
    // waits out its whole deadline with the link showing connected.
    //
    // That is what actually failed here -- four times now: three on 2026-09-08 (once on
    // `main` itself, Actions run 34232995914) and again on 2026-09-09 in run
    // 34348951731. Every one of them printed the link already connected. It was read as
    // slowness at the time and the deadline was raised from 5 s to 10 s, which could not
    // have helped: no deadline recovers an envelope that was never delivered, and the
    // 10 s duly failed the same way. The deadline is back to 5 s because the wait is
    // once again a liveness bound on an immediate operation -- a broadcast fan-out to an
    // already-subscribed socket -- and a failure at 5 s would now be real evidence of
    // something new rather than this race again.
    //
    // Neither number was ever a performance criterion, and the tightening does not make
    // one: nothing in `docs/verification-capability-table.md` names a delivery deadline,
    // and the budget that does bound this path -- detection to event-stream publish, p99
    // under 150 ms on-prem (`docs/performance-budgets.md`) -- is thirty times tighter
    // than even 5 s. A regression to seconds is that budget's row to catch.
    //
    // `gungnir-remote/tests/common/mod.rs::until_following` documents the same hazard
    // and works around it with sentinel envelopes. This waits on the node's own
    // subscriber count instead, which is the condition itself rather than a probe for
    // it, and leaves the record clean -- the reconciliation below reads that record.
    until(&mut state, "the event stream to subscribe", 10.0, |_| {
        api.subscriber_count() >= 1
    });

    // The node's record has one envelope before the outage, which the stream carries.
    api.publish_event(Envelope {
        seq: 1,
        mission_time: MissionTime(50.0),
        event: decided(5, true, "7", "Supervisor"),
    })
    .expect("published");
    until(&mut state, "the node's envelope to arrive", 5.0, |s| {
        s.link.as_ref().is_some_and(|l| l.last_seq() >= 1)
    });

    // The node goes down: every connection with it.
    node.shutdown_background();
    until(
        &mut state,
        "the desktop to fall back",
        HEARTBEAT_TIMEOUT.as_secs_f64() + 10.0,
        |s| matches!(s.backend, BackendConfig::Embedded) && s.fallback.is_some(),
    );
    assert!(state.link.is_some(), "the link keeps retrying");

    // A decision taken here during the outage, the other way, through the desktop's own
    // workflow: the record names the signed-in operator and the role their session
    // carries, which is what lets the rule rank it below.
    let submitted = state.clock.now();
    let pending = state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(1),
                ..PlanView::default()
            },
            verdict: gungnir_policy::PolicyVerdict::RequiresHumanApproval,
            submitted,
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Supervisor".into(),
        })
        .expect("queued");
    decisions::decide(
        &mut state,
        gungnir_ui::panels::approval_queue::PendingId(pending.0),
        OperatorDecision::Rejected {
            reason: "friendly airliner".into(),
        },
    )
    .expect("decided");
    let local = state
        .desk
        .approvals
        .records()
        .last()
        .expect("recorded")
        .clone();
    assert_eq!(
        (local.operator_id.as_deref(), local.role.as_deref()),
        (Some("7"), Some("Supervisor"))
    );
    // Meanwhile the node's record moved on without this desktop: another desktop's
    // operator decided the same plan and it reached the node. **A node runs no approval
    // queue** (`POST /v3/plans/{plan_id}/decision` refuses, `gungnir-api`'s transport), so
    // a decision reaches a node's record only as another desktop recorded it -- with the
    // role that desktop's signed-in session carried, here an Operator's. The api outlives
    // its transport, which is what the system of record does.
    api.publish_event(Envelope {
        seq: 2,
        mission_time: local.mission_time,
        event: decided(1, true, "9", "Operator"),
    })
    .expect("published");
    // And one envelope both records hold at the same moment, as the same decision does
    // when it reached both: the merge keeps it once.
    let shared_at = state.clock.now();
    let shared = decided(3, true, "7", "Supervisor");
    update::publish(&mut state, shared_at, shared.clone());
    api.publish_event(Envelope {
        seq: 3,
        mission_time: shared_at,
        event: shared,
    })
    .expect("published");

    // The node comes back on the same address, with its record intact.
    let node = serve(addr, Arc::clone(&api));
    until(&mut state, "the reconciliation report", 15.0, |s| {
        matches!(
            failover::reconciliation_view(s),
            ReconciliationView::Due {
                reconciliation: Some(Ok(_)),
                ..
            }
        )
    });
    match failover::reconciliation_view(&state) {
        ReconciliationView::Due {
            reconciliation: Some(Ok(r)),
            can_switch_back,
            ..
        } => {
            assert_eq!(r.remote, 2, "the node's two envelopes of the outage: {r:?}");
            assert_eq!(r.duplicates_dropped, 1, "{r:?}");
            assert_eq!(r.merged, r.local + r.remote - 1, "{r:?}");
            assert!(r.conflicts.is_empty(), "left to a person: {r:?}");
            let settled: Vec<(PlanId, bool, ArbitrationGround)> = r
                .arbitrated
                .iter()
                .map(|a| (a.conflict.plan, a.kept_local, a.ground))
                .collect();
            assert_eq!(
                settled,
                vec![(PlanId(1), true, ArbitrationGround::HigherRole)],
                "this desktop's Supervisor outranks the node's Operator: {r:?}"
            );
            assert!(can_switch_back, "the rule left nothing for a person");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        !state
            .audit
            .entries()
            .iter()
            .any(|e| e.detail.contains("reconciliation")),
        "the rule's verdict was audited as somebody's decision"
    );
    failover::switch_back(&mut state).expect("switched back with no person resolving");
    assert!(matches!(state.backend, BackendConfig::Remote { .. }));
    let bus: Vec<Envelope> = events.try_iter().collect();
    let verdicts: Vec<(PlanId, bool)> = bus
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::ConflictArbitrated {
                plan, kept_local, ..
            }) => Some((*plan, *kept_local)),
            _ => None,
        })
        .collect();
    assert_eq!(
        verdicts,
        vec![(PlanId(1), true)],
        "the verdict is on the record"
    );
    assert!(
        !bus.iter()
            .any(|env| matches!(env.event, Event::Link(LinkEvent::ConflictResolved { .. }))),
        "the rule's verdict was recorded as a person's"
    );
    assert!(bus.iter().any(|env| matches!(
        env.event,
        Event::Link(LinkEvent::SwitchedBack { conflicts: 1, .. })
    )));
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

/// **A sign-in during an outage leaves the outage for a person to end** (GAP-143, D-15).
///
/// A sign-in on a desktop configured for a node used to build a new link, put the remote
/// services back and clear the fallback: the outage ended with nobody switching back, its
/// reconciliation was discarded, and what the old link held for the node went with it. The
/// operator here signs out and in again while the node is down -- what a session that
/// expired mid-outage invites -- and everything the old path threw away has to still be
/// there: the fallback, the embedded services, the link, and, once the node answers, a
/// reconciliation computed over the node's history, which only that link could have
/// fetched. Then the outage ends when a person switches back, and not before.
///
/// Each check after the sign-in is one the old path fails: it left no fallback, a remote
/// backend, and a reconciliation that was never computed.
#[test]
fn a_sign_in_during_an_outage_leaves_it_for_a_person_to_end() {
    let addr: std::net::SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    let api = node_api();
    let node = serve(addr, Arc::clone(&api));
    let (mut state, dir) = desktop(&format!("http://{addr}"), "sign-in-mid-outage");
    let mut draft = SignInDraft {
        operator: "7".into(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(&mut state, &mut draft, SessionAction::SignIn);
    until(&mut state, "the link to come up", 10.0, |s| {
        s.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected)
    });

    // The node goes down, and the desktop falls back.
    node.shutdown_background();
    until(
        &mut state,
        "the desktop to fall back",
        HEARTBEAT_TIMEOUT.as_secs_f64() + 10.0,
        |s| matches!(s.backend, BackendConfig::Embedded) && s.fallback.is_some(),
    );

    // The operator signs out and in again while the node is down.
    session::apply(&mut state, &mut SignInDraft::default(), SessionAction::SignOut);
    assert!(state.fallback.is_some(), "a sign-out ended the outage");
    let mut again = SignInDraft {
        operator: "7".into(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(&mut state, &mut again, SessionAction::SignIn);
    assert!(
        state.fallback.is_some(),
        "a sign-in ended the outage with nobody switching back: {:?}",
        state.alerts
    );
    assert!(
        matches!(state.backend, BackendConfig::Embedded),
        "a sign-in put the remote services back mid-outage: {:?}",
        state.alerts
    );
    assert_eq!(
        state
            .link
            .as_ref()
            .and_then(gungnir_remote::link::NodeLink::signs_in_as),
        Some(7),
        "the outage's link was not kept, or does not sign in as the operator who did"
    );
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("signed in during an outage")),
        "the operator was not told the outage continues: {:?}",
        state.alerts
    );

    // The node comes back on the same address: the kept link reaches it, and the
    // reconciliation is computed over the node's history.
    let node = serve(addr, Arc::clone(&api));
    until(&mut state, "the reconciliation report", 15.0, |s| {
        matches!(
            failover::reconciliation_view(s),
            ReconciliationView::Due {
                reconciliation: Some(Ok(_)),
                ..
            }
        )
    });
    assert!(
        matches!(state.backend, BackendConfig::Embedded),
        "the outage ended before a person switched back"
    );

    // A person switches back, and that is what ends it.
    failover::switch_back(&mut state).expect("a person switches back");
    assert!(matches!(state.backend, BackendConfig::Remote { .. }));
    assert!(state.fallback.is_none());
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}
