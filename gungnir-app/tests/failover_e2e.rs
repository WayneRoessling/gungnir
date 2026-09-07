//! The disconnected-reconciliation row against a real transport (GAP-050, the
//! cross-layer row of `docs/verification-capability-table.md` §2): a desktop signed in
//! to a real node over the real client, the node goes down, the desktop falls back and
//! decides on its own, the node comes back on the same address, and the reconciliation
//! runs over the node's real `GET /v2/history`, is resolved by a person, and the switch
//! back is theirs.
//!
//! Wall-clock by necessity: the link judges silence by when it last heard the node, so
//! the outage is a real `HEARTBEAT_TIMEOUT` of silence. One test, a dozen seconds.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v2::SnapshotResponse;
use gungnir_app::failover::{self, ReconciliationView};
use gungnir_app::state::AppState;
use gungnir_app::{session, update};
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, SecurityConfig,
};
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::{CommandEvent, VerdictSummary};
use gungnir_model::{DecisionId, MissionTime, PlanId, SystemHealth};
use gungnir_remote::link::HEARTBEAT_TIMEOUT;
use gungnir_security::{
    hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use std::sync::Arc;

const PASSPHRASE: &str = "correct horse battery staple";

fn decided(plan: u64, accepted: bool) -> Event {
    Event::Command(CommandEvent::Decided {
        plan: PlanId(plan),
        decision: DecisionId(plan),
        accepted,
        operator: Some("7".into()),
        verdict: VerdictSummary::RequiresHumanApproval,
        rationale: (!accepted).then(|| "no".to_string()),
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

fn desktop(endpoint: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-failover-e2e-{}", std::process::id()));
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
fn an_outage_against_a_real_node_is_reconciled_over_the_real_history_route() {
    // A fixed loopback port, so the node can come back on the address the desktop has.
    let addr: std::net::SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    let api = node_api();
    let node = serve(addr, Arc::clone(&api));
    let (mut state, dir) = desktop(&format!("http://{addr}"));
    state.set_role(Role::Supervisor);

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
    until(&mut state, "the link to come up", 10.0, |s| {
        s.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected)
    });

    // The node's record has one envelope before the outage, which the stream carries.
    api.publish_event(Envelope {
        seq: 1,
        mission_time: MissionTime(50.0),
        event: decided(5, true),
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

    // A decision taken here during the outage, the other way.
    let now = state.clock.now();
    update::publish(&mut state, now, decided(1, false));
    // Meanwhile the node's record moved on without this desktop: another desktop's
    // decision on the same plan reached it. The api outlives its transport, which is
    // what the system of record does.
    api.publish_event(Envelope {
        seq: 2,
        mission_time: MissionTime(now.0),
        event: decided(1, true),
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
            assert!(
                r.conflicts.iter().any(|c| c.plan == PlanId(1)),
                "the two decisions on plan 1 conflict: {r:?}"
            );
            assert!(!can_switch_back, "not until every conflict is resolved");
        }
        other => panic!("{other:?}"),
    }
    failover::resolve_conflict(&mut state, PlanId(1), true).expect("resolved by a person");
    failover::switch_back(&mut state).expect("switched back");
    assert!(matches!(state.backend, BackendConfig::Remote { .. }));
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}
