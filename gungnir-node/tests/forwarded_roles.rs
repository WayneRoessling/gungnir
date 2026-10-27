// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-209: the node ranks a forwarded outage by its own accounts, not by the roles the
//! desktop's journal states (D-03; DN-31 §6.8 and §16; plan 12 tranche B).
//!
//! A disconnected desktop's journal is unlocked by its own operator, so a role in it is a
//! claim nobody else vouches for, and until GAP-209 the node recorded it as given and
//! published a rule settlement ranked on it. Here every batch goes over the real route to a
//! loop running `gungnir_node::approval`'s own steps, against an account store that knows
//! four operators, and the node:
//!
//! - refuses a decision whose role its account here does not carry, on the audit record;
//! - records an honest one as it is, and one by an operator it has no account for with no
//!   role at all;
//! - refuses a rule settlement its own ranks do not give, and takes one they do;
//! - refuses a settlement a person made whose account here may not decide.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{self, SnapshotResponse};
use gungnir_command::{ApprovalWorkflow, DecisionRecord};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::InProcessBus;
use gungnir_geo::InMemoryGeoService;
use gungnir_model::arbitration::{ArbitrationGround, ConflictSide, SideOutcome};
use gungnir_model::events::VerdictSummary;
use gungnir_model::{DecisionId, MissionTime, PlanId, PlanView, SystemHealth};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_security::{
    hash_passphrase, Account, AuditEntry, AuditSync, FileAuditLog, InMemoryAccountStore,
    OperatorId, Role, TokenIssuer,
};
use std::sync::{Arc, Mutex};

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 21;
const SUPERVISOR: u64 = 22;
const ANALYST: u64 = 23;
const COMMANDER: u64 = 24;
/// Has no account on this node.
const STRANGER: u64 = 99;
const NOW: f64 = 100.0;

struct Node {
    addr: std::net::SocketAddr,
    approval: Arc<Mutex<NodeApproval>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Node {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

fn account(operator: u64, role: Role) -> Account {
    Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

impl Node {
    async fn spawn(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-forwarded-roles-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Arc::new(ConfigBaseline {
            data_dir: dir.to_string_lossy().into_owned(),
            ..ConfigBaseline::default()
        });
        let store = InMemoryAccountStore::new(vec![
            account(OPERATOR, Role::Operator),
            account(SUPERVISOR, Role::Supervisor),
            account(ANALYST, Role::Analyst),
            account(COMMANDER, Role::Commander),
        ]);
        let issuer = TokenIssuer::new(vec![9u8; 32], 10_000.0).expect("issuer");
        let api = Arc::new(
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
        );
        api.set_now(NOW);
        let log = FileAuditLog::open(&dir.join("audit"), AuditSync::OnFlush, NOW)
            .expect("the audit log opens");
        let approval = Arc::new(Mutex::new(NodeApproval::with_audit(&config, Box::new(log))));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let bus = Arc::new(InProcessBus::new());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        std::thread::spawn({
            let api = api.clone();
            let approval = approval.clone();
            let running = running.clone();
            move || {
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    {
                        let Ok(mut state) = approval.lock() else {
                            return;
                        };
                        let frame = Frame {
                            now: MissionTime(NOW),
                            config: &config,
                            tracks: &[],
                            resources: &[],
                            geofences: &*geo,
                            bus: &bus,
                            endpoint_client: None,
                            effectors: None,
                            api: &api,
                        };
                        approval::sweep(&mut state, &frame);
                        let _ = approval::answer_decisions(&mut state, &frame);
                        approval::answer_forwarded(&mut state, &frame);
                        approval::audit_refused_decisions(&mut state, &frame);
                        approval::audit_routes(&mut state, &frame);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
        let listener = bind("127.0.0.1:0".parse().expect("address"))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = serve_on(listener, api).await;
        });
        Self {
            addr,
            approval,
            running,
        }
    }

    fn records(&self) -> Vec<DecisionRecord> {
        self.approval
            .lock()
            .expect("the loop")
            .desk
            .approvals
            .records()
            .to_vec()
    }

    fn entries(&self) -> Vec<AuditEntry> {
        self.approval
            .lock()
            .expect("the loop")
            .audit
            .entries()
            .to_vec()
    }

    /// Forward `batch` as the supervisor's session; the status and the body.
    async fn forward(&self, batch: &[v3::ForwardedDecision]) -> (u16, String) {
        let session = reqwest::Client::new()
            .post(format!("http://{}/v3/session", self.addr))
            .json(&serde_json::json!({ "operator": SUPERVISOR, "passphrase": PASSPHRASE }))
            .send()
            .await
            .expect("the session route");
        let token: serde_json::Value = session.json().await.expect("a session");
        let response = reqwest::Client::new()
            .post(format!("http://{}/v3/decisions/forwarded", self.addr))
            .bearer_auth(token["token"].as_str().expect("a token"))
            .json(&serde_json::to_value(batch).expect("json"))
            .send()
            .await
            .expect("the forwarding route");
        let status = response.status().as_u16();
        (status, response.text().await.unwrap_or_default())
    }
}

fn decision(id: u128, plan: u128, operator: u64, role: &str) -> v3::ForwardedDecision {
    v3::ForwardedDecision {
        record: v3::DecisionRecordView {
            decision: DecisionId(id),
            item: None,
            plan: PlanView {
                id: PlanId(plan),
                ..PlanView::default()
            },
            verdict: VerdictSummary::RequiresHumanApproval,
            choice: v3::DecisionChoice::Accept,
            operator: Some(operator.to_string()),
            role: Some(role.into()),
            request: None,
            at: MissionTime(80.0),
            acknowledged: Vec::new(),
        },
        origin: "desktop-a".into(),
        settled: None,
    }
}

fn side(operator: u64, role: &str, at: f64) -> ConflictSide {
    ConflictSide {
        outcome: SideOutcome::Accepted,
        operator: Some(operator.to_string()),
        role: Some(role.into()),
        at: MissionTime(at),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_role_the_nodes_account_does_not_carry_is_refused_on_the_record() {
    let node = Node::spawn("mismatch").await;
    // Operator 21 is an Operator here; the desktop's journal says Commander.
    let (status, body) = node
        .forward(&[decision(1, 11, OPERATOR, "Commander")])
        .await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body.contains("role-mismatch") && body.contains("Commander"),
        "{body}"
    );
    assert!(
        node.records().is_empty(),
        "nothing of the batch was applied"
    );
    assert!(
        node.entries()
            .iter()
            .any(|e| e.detail.contains("RoleMismatch")),
        "{:?}",
        node.entries()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_honest_role_is_recorded_and_an_unknown_operator_has_none() {
    let node = Node::spawn("honest").await;
    let (status, body) = node
        .forward(&[
            decision(2, 12, SUPERVISOR, "Supervisor"),
            decision(3, 13, STRANGER, "Commander"),
        ])
        .await;
    assert_eq!(status, 202, "{body}");
    let records = node.records();
    let role_of = |id: u128| {
        records
            .iter()
            .find(|r| r.id == DecisionId(id))
            .map(|r| r.role.clone())
    };
    assert_eq!(role_of(2), Some(Some("Supervisor".into())));
    assert_eq!(
        role_of(3),
        Some(None),
        "a role only the desktop vouches for ranks nothing here"
    );
    // The same batch again is the same records, held: the derivation is deterministic.
    let (status, body) = node
        .forward(&[
            decision(2, 12, SUPERVISOR, "Supervisor"),
            decision(3, 13, STRANGER, "Commander"),
        ])
        .await;
    assert_eq!(status, 202, "{body}");
    assert!(body.contains("\"already_held\":2"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_settlement_the_nodes_ranks_do_not_give_is_refused_and_one_they_do_is_taken() {
    let node = Node::spawn("settlement").await;
    // The desktop says its Supervisor outranked the other side's Operator. The other side
    // was operator 24, a Commander here, so the rule on the node's ranks keeps it instead.
    let mut forged = decision(4, 14, SUPERVISOR, "Supervisor");
    forged.settled = Some(v3::Settlement::Rule {
        kept_local: true,
        ground: ArbitrationGround::HigherRole,
        local: side(SUPERVISOR, "Supervisor", 80.0),
        remote: side(COMMANDER, "Operator", 70.0),
    });
    let (status, body) = node.forward(&[forged]).await;
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("arbitrated-otherwise"), "{body}");
    assert!(node.records().is_empty());

    // The same claim against operator 21, an Operator here: the node's ranks agree.
    let mut honest = decision(5, 15, SUPERVISOR, "Supervisor");
    honest.settled = Some(v3::Settlement::Rule {
        kept_local: true,
        ground: ArbitrationGround::HigherRole,
        local: side(SUPERVISOR, "Supervisor", 80.0),
        remote: side(OPERATOR, "Operator", 70.0),
    });
    let (status, body) = node.forward(&[honest]).await;
    assert_eq!(status, 202, "{body}");
    assert!(body.contains("\"settled\":1"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_conflict_settled_by_someone_who_may_not_decide_is_refused() {
    let node = Node::spawn("resolver").await;
    let mut settled = decision(6, 16, SUPERVISOR, "Supervisor");
    settled.settled = Some(v3::Settlement::Person {
        kept_local: true,
        operator: Some(ANALYST.to_string()),
    });
    let (status, body) = node.forward(&[settled]).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body.contains("resolver-not-permitted") && body.contains("Analyst"),
        "{body}"
    );
    assert!(node.records().is_empty());
}
