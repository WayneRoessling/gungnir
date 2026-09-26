// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's audit record, act by act (GAP-111, D-87; the `gungnir-security` row of
//! `docs/verification-capability-table.md` §2, its third clause on a node).
//!
//! **Each act a caller can take against a node, over the real transport, grows the node's
//! audit log by exactly one entry naming that act** -- a sign-in and a refused one, every
//! refusal, and every act a role-gated route performs -- and a read that is served grows
//! it by none. Then the file the node wrote verifies as one intact chain holding exactly
//! those entries, and nothing in it is a passphrase.
//!
//! # What "the node" is here
//!
//! The transport is the real one on loopback, with tokens the node minted. The loop is a
//! thread calling the **same** `gungnir_node::approval` steps `main.rs` calls, in the same
//! order, ending with `audit_routes`, against a `NodeApproval` whose log is a
//! `FileAuditLog` in a scratch directory -- the log `main.rs` opens beside its journal.
//! It stands in for the sensor registry by answering sensor 1's tasks with a task id and
//! any other sensor's with a refusal, which is what `issue_api_tasks` hands back.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::InProcessBus;
use gungnir_geo::InMemoryGeoService;
use gungnir_model::{MissionTime, SensorTaskId, SystemHealth};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_security::audit::events;
use gungnir_security::{
    actions, hash_passphrase, verify_audit_dir, Account, AuditEntry, AuditSync, FileAuditLog,
    InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use std::sync::{Arc, Mutex};

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 21;
const SUPERVISOR: u64 = 22;
const ANALYST: u64 = 23;
const COMMANDER: u64 = 24;
const ADMINISTRATOR: u64 = 25;
const OFFICER: u64 = 26;

/// The node's clock for the whole test.
const NOW: f64 = 100.0;

struct Node {
    addr: std::net::SocketAddr,
    approval: Arc<Mutex<NodeApproval>>,
    dir: std::path::PathBuf,
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
        let dir =
            std::env::temp_dir().join(format!("gungnir-node-audit-{name}-{}", std::process::id()));
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
            account(ADMINISTRATOR, Role::Administrator),
            account(OFFICER, Role::SecurityOfficer),
        ]);
        let issuer = TokenIssuer::new(vec![7u8; 32], 10_000.0).expect("issuer");
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
        approval::claim_exchange_items(&api).expect("claimed");

        let log = FileAuditLog::open(&dir.join("audit"), AuditSync::OnFlush, NOW)
            .expect("the audit log opens");
        let approval = Arc::new(Mutex::new(NodeApproval::with_audit(&config, Box::new(log))));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let bus = Arc::new(InProcessBus::new());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        // A plain thread rather than `spawn_blocking`, for the reason
        // `approval_queue.rs` gives: `Drop` stops it however the test ends.
        std::thread::spawn({
            let api = api.clone();
            let approval = approval.clone();
            let running = running.clone();
            move || {
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    // The registry's answer, as `issue_api_tasks` gives it.
                    for task in api.take_tasks() {
                        let answer = if task.sensor.0 == 1 {
                            Ok(SensorTaskId(77))
                        } else {
                            Err(format!("sensor {} is not controllable", task.sensor.0))
                        };
                        let _ = task.reply.send(answer);
                    }
                    let _ = api.take_effector_reports();
                    let _ = api.take_warning_acknowledgements();
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
                            api: &api,
                        };
                        // The order `main.rs` runs them in.
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
            dir,
            running,
        }
    }

    fn entries(&self) -> Vec<AuditEntry> {
        self.approval
            .lock()
            .expect("the loop is running")
            .audit
            .entries()
            .to_vec()
    }

    /// The log once the loop has drained what the last request left: polled until it
    /// holds `before + want` entries, not slept for a fixed time and read too early, then
    /// given a few more ticks so an entry too many has the chance to show itself. For
    /// `want` of zero, the few ticks alone.
    async fn entries_after(&self, before: usize, want: usize) -> Vec<AuditEntry> {
        for _ in 0..500 {
            if self.entries().len() >= before + want {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        self.entries()
    }
}

/// `POST /v3/sensors/{sensor}/task`'s body, built from the contract's own type.
fn task_body() -> serde_json::Value {
    serde_json::to_value(gungnir_api::v3::SensorTaskRequest {
        command: gungnir_model::SensorCommand::SetMode {
            mode: gungnir_model::SensorMode::Search,
        },
        requirement: None,
    })
    .expect("json")
}

/// One request, with or without a token; the status.
async fn call(
    addr: std::net::SocketAddr,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> u16 {
    let mut request = reqwest::Client::new().request(method, format!("http://{addr}{path}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    request.send().await.expect("the route").status().as_u16()
}

async fn sign_in(addr: std::net::SocketAddr, operator: u64, passphrase: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v3/session"))
        .json(&serde_json::json!({ "operator": operator, "passphrase": passphrase }))
        .send()
        .await
        .expect("the session route");
    let status = response.status().as_u16();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    (
        status,
        body["token"].as_str().unwrap_or_default().to_owned(),
    )
}

/// What one act must leave: its status, and exactly one entry with this action,
/// attributed to this operator. `None` for `action` asserts that nothing was recorded.
struct Expect {
    status: u16,
    action: Option<&'static str>,
    operator: Option<u64>,
}

fn records(status: u16, action: &'static str, operator: Option<u64>) -> Expect {
    Expect {
        status,
        action: Some(action),
        operator,
    }
}

fn records_nothing(status: u16) -> Expect {
    Expect {
        status,
        action: None,
        operator: None,
    }
}

/// One request: what it is, the method, the path, the token, the body, and what it leaves.
type Act<'a> = (
    &'a str,
    reqwest::Method,
    String,
    Option<&'a str>,
    Option<serde_json::Value>,
    Expect,
);

/// Every act, one entry each; every served read, none; then the file.
#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)] // One act after another is the point: it reads as the list.
async fn each_act_on_a_node_grows_its_audit_log_by_exactly_one_entry_naming_it() {
    let node = Node::spawn("acts").await;
    let addr = node.addr;
    let mut expected_in_file = 0usize;

    // Sign-in, both ways. Checked by hand because the token is what the rest use.
    let mut tokens = std::collections::BTreeMap::new();
    for operator in [
        OPERATOR,
        SUPERVISOR,
        ANALYST,
        COMMANDER,
        ADMINISTRATOR,
        OFFICER,
    ] {
        let before = node.entries().len();
        let (status, token) = sign_in(addr, operator, PASSPHRASE).await;
        assert_eq!(status, 200);
        let after = node.entries_after(before, 1).await;
        assert_eq!(after.len(), before + 1, "sign-in of {operator}");
        let entry = &after[before];
        assert_eq!(entry.action, events::SIGN_IN);
        assert_eq!(entry.operator, Some(OperatorId(operator)));
        tokens.insert(operator, token);
        expected_in_file += 1;
    }
    let before = node.entries().len();
    let (status, _) = sign_in(addr, OPERATOR, "not the passphrase").await;
    assert_eq!(status, 401);
    let after = node.entries_after(before, 1).await;
    assert_eq!(after.len(), before + 1, "a refused sign-in");
    assert_eq!(after[before].action, events::SIGN_IN_REJECTED);
    assert_eq!(
        after[before].operator, None,
        "a claimed operator is not attributed"
    );
    assert!(after[before]
        .detail
        .contains(&format!("operator {OPERATOR} was tried")));
    expected_in_file += 1;

    let token = |operator: u64| tokens.get(&operator).map(String::as_str);
    let get = reqwest::Method::GET;
    let post = reqwest::Method::POST;
    let task = |sensor: u32| (format!("/v3/sensors/{sensor}/task"), task_body());
    let report = serde_json::to_value(gungnir_api::v3::EffectorReportRequest {
        report: gungnir_model::handoff::EffectorReport::Executing {
            at: MissionTime(NOW),
        },
    })
    .expect("json");
    let acknowledgement = serde_json::json!({ "at": NOW });
    let publish = serde_json::json!({ "products": [] });
    let decision = serde_json::json!({
        "request": "audit-1",
        "item": "1",
        "choice": "accept",
    });

    let acts: Vec<Act<'_>> = vec![
        // Reads: served and unrecorded, or refused and recorded.
        (
            "a served snapshot",
            get.clone(),
            "/v3/snapshot".into(),
            token(OPERATOR),
            None,
            records_nothing(200),
        ),
        (
            "a served queue",
            get.clone(),
            "/v3/queue".into(),
            token(ANALYST),
            None,
            records_nothing(200),
        ),
        (
            "health for the security officer",
            get.clone(),
            "/v3/health".into(),
            token(OFFICER),
            None,
            records_nothing(200),
        ),
        (
            "a snapshot with no token",
            get.clone(),
            "/v3/snapshot".into(),
            None,
            None,
            records(401, events::ACCESS_REFUSED, None),
        ),
        (
            "the session route with no token",
            get.clone(),
            "/v3/session".into(),
            None,
            None,
            records(401, events::ACCESS_REFUSED, None),
        ),
        (
            "the snapshot to the security officer",
            get.clone(),
            "/v3/snapshot".into(),
            token(OFFICER),
            None,
            records(403, actions::VIEW_PICTURE, Some(OFFICER)),
        ),
        (
            "the history to the security officer",
            get.clone(),
            "/v3/history?since_seq=0".into(),
            token(OFFICER),
            None,
            records(403, actions::VIEW_PICTURE, Some(OFFICER)),
        ),
        (
            "coverage to the security officer",
            get.clone(),
            "/v3/coverage".into(),
            token(OFFICER),
            None,
            records(403, actions::VIEW_PICTURE, Some(OFFICER)),
        ),
        (
            "the queue to the security officer",
            get.clone(),
            "/v3/queue".into(),
            token(OFFICER),
            None,
            records(403, actions::VIEW_PICTURE, Some(OFFICER)),
        ),
        (
            "exchanged warnings to the security officer",
            get.clone(),
            "/v3/exchange/warnings".into(),
            token(OFFICER),
            None,
            records(403, actions::VIEW_PICTURE, Some(OFFICER)),
        ),
        // Sensor tasking: refused, commanded, and refused by the registry.
        (
            "a task by an analyst",
            post.clone(),
            task(1).0,
            token(ANALYST),
            Some(task(1).1),
            records(403, actions::TASK_SENSOR, Some(ANALYST)),
        ),
        (
            "a task with no token",
            post.clone(),
            task(1).0,
            None,
            Some(task(1).1),
            records(401, events::ACCESS_REFUSED, None),
        ),
        (
            "a task the registry issues",
            post.clone(),
            task(1).0,
            token(SUPERVISOR),
            Some(task(1).1),
            records(202, actions::TASK_SENSOR, Some(SUPERVISOR)),
        ),
        (
            "a task the registry refuses",
            post.clone(),
            task(2).0,
            token(SUPERVISOR),
            Some(task(2).1),
            records(409, actions::TASK_SENSOR, Some(SUPERVISOR)),
        ),
        (
            "an undecodable task",
            post.clone(),
            task(1).0,
            token(SUPERVISOR),
            Some(serde_json::json!({"nonsense": 1})),
            records(400, actions::TASK_SENSOR, Some(SUPERVISOR)),
        ),
        // What an outside party said, keyed in by a person.
        (
            "an effector report by an operator",
            post.clone(),
            "/v3/handoffs/9/report".into(),
            token(OPERATOR),
            Some(report.clone()),
            records(403, actions::EFFECTOR_REPORT, Some(OPERATOR)),
        ),
        (
            "an effector report by the administrator",
            post.clone(),
            "/v3/handoffs/9/report".into(),
            token(ADMINISTRATOR),
            Some(report.clone()),
            records(202, actions::EFFECTOR_REPORT, Some(ADMINISTRATOR)),
        ),
        (
            "an acknowledgement by an operator",
            post.clone(),
            "/v3/warnings/1/7/acknowledge".into(),
            token(OPERATOR),
            Some(acknowledgement.clone()),
            records(403, actions::ACKNOWLEDGE_WARNING, Some(OPERATOR)),
        ),
        (
            "an acknowledgement by the administrator",
            post.clone(),
            "/v3/warnings/1/7/acknowledge".into(),
            token(ADMINISTRATOR),
            Some(acknowledgement.clone()),
            records(202, actions::ACKNOWLEDGE_WARNING, Some(ADMINISTRATOR)),
        ),
        // Publishing to exchange.
        (
            "a publish by an operator",
            post.clone(),
            "/v3/exchange/reports".into(),
            token(OPERATOR),
            Some(publish.clone()),
            records(403, actions::PUBLISH_EXCHANGE, Some(OPERATOR)),
        ),
        (
            "a publish by a commander",
            post.clone(),
            "/v3/exchange/reports".into(),
            token(COMMANDER),
            Some(publish.clone()),
            records(202, actions::PUBLISH_EXCHANGE, Some(COMMANDER)),
        ),
        // Decisions: the loop's own entry, one per refusal as DN-31 §9 row 4 has it.
        (
            "a decision with no token",
            post.clone(),
            "/v3/queue/1/decision".into(),
            None,
            Some(decision.clone()),
            records(401, actions::DECIDE_PLAN, None),
        ),
        (
            "a decision by an analyst",
            post.clone(),
            "/v3/queue/1/decision".into(),
            token(ANALYST),
            Some(decision.clone()),
            records(403, actions::DECIDE_PLAN, Some(ANALYST)),
        ),
        (
            "a decision by the administrator (D-88)",
            post.clone(),
            "/v3/queue/1/decision".into(),
            token(ADMINISTRATOR),
            Some(decision.clone()),
            records(403, actions::DECIDE_PLAN, Some(ADMINISTRATOR)),
        ),
        (
            "a decision on an item this node never issued",
            post.clone(),
            "/v3/queue/1/decision".into(),
            token(OPERATOR),
            Some(decision.clone()),
            records(404, actions::DECIDE_PLAN, Some(OPERATOR)),
        ),
        (
            "a forward by an analyst",
            post.clone(),
            "/v3/decisions/forwarded".into(),
            token(ANALYST),
            Some(serde_json::json!([])),
            records(403, actions::DECIDE_PLAN, Some(ANALYST)),
        ),
    ];

    for (what, method, path, bearer, body, expect) in acts {
        let before = node.entries().len();
        let status = call(addr, method, &path, bearer, body).await;
        assert_eq!(status, expect.status, "{what}: status");
        let after = node
            .entries_after(before, usize::from(expect.action.is_some()))
            .await;
        match expect.action {
            None => assert_eq!(
                after.len(),
                before,
                "{what}: a served read recorded {:?}",
                &after[before..]
            ),
            Some(action) => {
                assert_eq!(
                    after.len(),
                    before + 1,
                    "{what}: expected exactly one entry, got {:?}",
                    &after[before..]
                );
                let entry = &after[before];
                assert_eq!(entry.action, action, "{what}: {entry:?}");
                assert_eq!(
                    entry.operator,
                    expect.operator.map(OperatorId),
                    "{what}: {entry:?}"
                );
                expected_in_file += 1;
            }
        }
    }

    // The file: one intact chain, every entry the log holds, and no secret.
    let flushed = node.entries().len();
    assert_eq!(flushed, expected_in_file);
    let audit = node.dir.join("audit");
    let verified = verify_audit_dir(&audit).expect("the audit log reads");
    assert!(verified.intact(), "{:?}", verified.breaks);
    assert_eq!(
        verified.entries,
        u64::try_from(expected_in_file).expect("a count")
    );
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
    for bearer in tokens.values() {
        assert!(
            !text.contains(bearer.as_str()),
            "a token reached the audit log"
        );
    }
    let _ = std::fs::remove_dir_all(&node.dir);
}

/// A flood of refused sign-ins from nobody is bounded and counted, and does not push out
/// the entry a verified operator is owed (D-87).
#[tokio::test(flavor = "multi_thread")]
async fn a_flood_from_nobody_is_counted_not_recorded_and_does_not_crowd_out_a_person() {
    let node = Node::spawn("flood").await;
    let addr = node.addr;
    let (_, token) = sign_in(addr, SUPERVISOR, PASSPHRASE).await;
    let before = node.entries_after(0, 1).await.len();
    // Three times the unauthenticated burst, as fast as the transport takes them.
    let flood = gungnir_security::audit::UNATTRIBUTED_LIMIT.burst * 3;
    let mut handles = Vec::new();
    for _ in 0..flood {
        handles.push(tokio::spawn(call(
            addr,
            reqwest::Method::GET,
            "/v3/snapshot",
            None,
            None,
        )));
    }
    for handle in handles {
        assert_eq!(handle.await.expect("joined"), 401);
    }
    let status = call(
        addr,
        reqwest::Method::POST,
        "/v3/sensors/1/task",
        Some(&token),
        Some(task_body()),
    )
    .await;
    assert_eq!(status, 202);
    // At least the task and one overflow count; the rest is what the limit let through.
    let after = node.entries_after(before, 2).await;
    let new = &after[before..];
    let refused = new
        .iter()
        .filter(|e| e.action == events::ACCESS_REFUSED)
        .count();
    let overflow: Vec<&AuditEntry> = new
        .iter()
        .filter(|e| e.action == events::OVERFLOW)
        .collect();
    assert!(
        refused < flood as usize,
        "every one of {flood} refusals was recorded; the limit held nothing back"
    );
    assert!(
        !overflow.is_empty(),
        "what the limit held back was not counted"
    );
    let counted: u64 = overflow
        .iter()
        .filter_map(|e| e.detail.split_whitespace().next()?.parse::<u64>().ok())
        .sum();
    assert_eq!(
        refused as u64 + counted,
        u64::from(flood),
        "every refusal is either recorded or counted"
    );
    assert!(
        new.iter().any(|e| e.action == actions::TASK_SENSOR
            && e.operator == Some(OperatorId(SUPERVISOR))),
        "the supervisor's task was crowded out by the flood"
    );
    let _ = std::fs::remove_dir_all(&node.dir);
}
