// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's approval queue: DN-31 §9 rows 3 to 6 (GAP-132, D-55, D-57, D-59).
//!
//! | Row | What it holds |
//! |---|---|
//! | 3 | One decision per item: 100 randomized races between two operator clients over the real transport, plus retries with the same request key |
//! | 4 | Authorization on the node: each role against each item state, each choice, and an expired token; every refusal records, publishes and engages nothing, and every decision and every refusal writes exactly one audit entry |
//! | 5 | Authority and offering: plans per layer and class against the authority matrix, and `Denied { Authority }` for a plan nobody may accept |
//! | 6 | Expiry and escalation on the node's clock, with a second client signed in as the higher role, and a pre-delegated item that still expires and escalates (D-59) |
//!
//! **And what leaves this node when a row has run** (GAP-137): a handoff the queue issued
//! reaches a coalition partner that has an agreement for it, which is the one thing the
//! rows above never asked -- they end at the record.
//!
//! # What "an in-process node on the real transport" is here
//!
//! The transport is the real one: `gungnir_api::transport::bind` and `serve_on` over
//! loopback TCP, with real clients holding real session tokens the node minted. The loop
//! is [`Node::spawn`]'s ticker, which calls the **same** `gungnir_node::approval`
//! functions `main.rs` calls -- `propose`, `sweep`, `answer_decisions`, `answer_forwarded`
//! (since GAP-134), `audit_refused_decisions` -- in the same order, against one
//! `NodeApproval`. That is why
//! those functions are a library module (`gungnir-node/src/lib.rs`): a test that drove a
//! copy of the loop would prove the copy.
//!
//! The clock is the test's, because rows 4 and 6 are about a window closing rather than
//! about how long a machine took: [`Shared::now`] is the mission time the loop reads, and a
//! test advances it. The plans are stated rather than solved for, because what these rows
//! are about is what the queue does with a plan; the allocator's own choices are
//! `gungnir-intercept-service`'s business and are tested there.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, ExchangeProducer, NodeApi};
use gungnir_api::v3::{
    DecisionRefused, ExchangeProduct, ExchangeResponse, QueueItemView, SnapshotResponse,
};
use gungnir_command::{ApprovalWorkflow, DecisionRecord};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_geo::InMemoryGeoService;
use gungnir_model::events::{CommandEvent, InterceptEvent};
use gungnir_model::policy_settings::AuthorityRule;
use gungnir_model::{
    Classification, EffectorLayer, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet,
    InterceptSolutionView, MissionTime, PendingApprovalId, PlanId, PlanKind, PlanView, Provenance,
    Quality, Releasability, ResourceId, ResourceView, SystemHealth, TrackId, TrackStatus,
    TrackView, WeaponsControlStatus,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_security::{
    actions, hash_passphrase, Account, AuditEntry, AuditLog, InMemoryAccountStore, OperatorId,
    OperatorSession, Role, TokenIssuer,
};
use std::sync::{Arc, Mutex};

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 11;
const SECOND_OPERATOR: u64 = 12;
const SUPERVISOR: u64 = 13;
/// A role that holds no `plan.decide` at all: it may read the picture and export a report
/// and decide nothing (`gungnir_security::authz::role_permits`).
const ANALYST: u64 = 14;

/// The coalition partner these rows' node has an exchange agreement with (GAP-137).
const PARTNER: &str = "sector-north";

/// Mission time the queue is submitted at. Every deadline below is relative to it.
const SUBMITTED: f64 = 100.0;
/// Seconds after submission at which a point item is offered one rank higher.
const ESCALATE_AFTER_S: f64 = 10.0;
/// Seconds after submission at which a point item's window closes.
const EXPIRY_S: f64 = 30.0;

// ---------------------------------------------------------------------------------
// The baseline
// ---------------------------------------------------------------------------------

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

fn rule(
    role: &str,
    layer: EffectorLayer,
    class: Option<&str>,
    pre_delegated: bool,
) -> AuthorityRule {
    AuthorityRule {
        action: actions::DECIDE_PLAN.into(),
        role: role.into(),
        layer: Some(layer),
        class: class.map(str::to_owned),
        pre_delegated,
    }
}

/// The deployment every row below judges against.
///
/// **The authority matrix is the point of the fixture.** An Operator may decide a point
/// engagement against a hostile track and nothing else; a Supervisor may decide an area
/// engagement as well. So one plan is the Operator's, another has to go up, and a third --
/// against a neutral track -- is nobody's, which is what row 5 needs to see denied.
///
/// `pre_delegated` sets D-15's flag on the Operator's rule, for the row 6 case that has to
/// expire and escalate anyway.
fn baseline(name: &str, pre_delegated: bool) -> (ConfigBaseline, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-node-queue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![effector(0, "point"), effector(1, "area")],
        ..ConfigBaseline::default()
    };
    // Weapons free at both layers, so what the chain returns is a judgment about the plan
    // rather than a standing refusal that would be the same whatever was proposed.
    for layer in [EffectorLayer::Point, EffectorLayer::Area] {
        config
            .policy
            .control_status
            .by_layer
            .insert(layer, WeaponsControlStatus::Free);
    }
    config.policy.authority.rules = vec![
        rule(
            "Operator",
            EffectorLayer::Point,
            Some("hostile"),
            pre_delegated,
        ),
        rule("Supervisor", EffectorLayer::Point, Some("hostile"), false),
        rule("Supervisor", EffectorLayer::Area, Some("hostile"), false),
        rule("Commander", EffectorLayer::Area, Some("hostile"), false),
    ];
    // A point item escalates then expires; an area item has neither deadline, so row 6 can
    // tell a window that closes from one that does not.
    config
        .policy
        .decisions
        .expiry_s
        .insert(EffectorLayer::Point, EXPIRY_S);
    config
        .policy
        .decisions
        .escalate_after_s
        .insert(EffectorLayer::Point, ESCALATE_AFTER_S);
    // An engagement needs a window for an effect to be expected by, or none opens and the
    // desk says so rather than opening one on a guess.
    for layer in [EffectorLayer::Point, EffectorLayer::Area] {
        config.assessment.effect_window_s.insert(layer, 60.0);
    }
    gungnir_config::validate(&config).expect("the baseline is valid");
    (config, dir)
}

fn track(id: u64, classification: Classification) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(SUBMITTED),
        releasability: Releasability::default(),
    }
}

/// A plan tasking `resource` against `track`.
fn plan(id: u128, resource: u32, track: u64) -> PlanView {
    PlanView {
        id: PlanId(id),
        mission_time: MissionTime(SUBMITTED),
        kind: PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(resource),
                track: TrackId(track),
                intercept_point: None,
                time_to_intercept_s: Some(10.0),
            }],
        },
        ..PlanView::default()
    }
}

// ---------------------------------------------------------------------------------
// The node
// ---------------------------------------------------------------------------------

/// What the ticker owns and a test reads.
///
/// Behind one lock, taken and released inside each tick: the loop is still one loop taking
/// one request at a time, which is the property rows 3 and 6 turn on. The lock is how a
/// test looks at the record, not how the node works.
struct Shared {
    approval: NodeApproval,
    now: MissionTime,
    tracks: Vec<TrackView>,
}

struct Node {
    addr: std::net::SocketAddr,
    api: Arc<NodeApi>,
    shared: Arc<Mutex<Shared>>,
    events: Receiver<Envelope>,
    config: Arc<ConfigBaseline>,
    resources: Arc<Vec<ResourceView>>,
    geo: Arc<InMemoryGeoService>,
    bus: Arc<InProcessBus>,
    dir: std::path::PathBuf,
    running: Arc<std::sync::atomic::AtomicBool>,
}

/// A node that knows the four accounts these rows sign in as.
///
/// The token lifetime is long, because rows 4 and 6 advance mission time far past
/// submission and a session that lapsed on the way would refuse for the wrong reason. The
/// one test that wants an expired token mints its own.
fn api_knowing_the_accounts() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        account(OPERATOR, Role::Operator),
        account(SECOND_OPERATOR, Role::Operator),
        account(SUPERVISOR, Role::Supervisor),
        account(ANALYST, Role::Analyst),
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
        )))
        // One partner with an agreement for handoffs (GAP-137). The agreement gate and
        // the marking gate are `gungnir-api`'s and tested there; what it is here for is
        // that a partner reading this node has something to read.
        .with_exchange(ExchangeSet {
            agreements: vec![ExchangeAgreement {
                party: PARTNER.into(),
                inbound: Vec::new(),
                outbound: vec![ExchangeItem::Handoffs],
                format: ExchangeFormat::Canonical,
            }],
        }),
    );
    api.set_now(SUBMITTED);
    api
}

/// Serve it on loopback, the way `transport::serve` does.
async fn serve(api: Arc<NodeApi>) -> std::net::SocketAddr {
    let listener = bind("127.0.0.1:0".parse().expect("address"))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = serve_on(listener, api).await;
    });
    addr
}

impl Node {
    async fn spawn(name: &str) -> Self {
        Self::spawn_with(name, false).await
    }

    /// A node serving on loopback with its loop running.
    async fn spawn_with(name: &str, pre_delegated: bool) -> Self {
        let (config, dir) = baseline(name, pre_delegated);
        let config = Arc::new(config);
        let resources = Arc::new(config.resource_views());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        let bus = Arc::new(InProcessBus::new());
        let events = bus.subscribe();

        let api = api_knowing_the_accounts();
        let addr = serve(api.clone()).await;

        let shared = Arc::new(Mutex::new(Shared {
            approval: NodeApproval::new(&config),
            now: MissionTime(SUBMITTED),
            tracks: Vec::new(),
        }));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        // **A plain thread, not `spawn_blocking`.** A tokio runtime waits for its blocking
        // tasks when it shuts down, so a loop that runs until it is told to stop would
        // hang the test binary for ever the first time an assertion failed before
        // `cleanup`. `Node`'s `Drop` stops this one whether the test passes or panics.
        std::thread::spawn({
            let api = api.clone();
            let shared = shared.clone();
            let config = config.clone();
            let resources = resources.clone();
            let geo = geo.clone();
            let bus = bus.clone();
            let running = running.clone();
            move || {
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    {
                        let Ok(mut state) = shared.lock() else {
                            return;
                        };
                        let now = state.now;
                        let tracks = std::mem::take(&mut state.tracks);
                        let frame = Frame {
                            now,
                            config: &config,
                            tracks: &tracks,
                            resources: &resources,
                            geofences: &*geo,
                            bus: &bus,
                            endpoint_client: None,
                            api: &api,
                        };
                        // The order `main.rs` runs them in: the sweep on the node's clock,
                        // then the decisions the routes accepted, then the outages desktops
                        // forwarded (GAP-134), then the audit entries the refusals owe.
                        approval::sweep(&mut state.approval, &frame);
                        let _ = approval::answer_decisions(&mut state.approval, &frame);
                        approval::answer_forwarded(&mut state.approval, &frame);
                        approval::audit_refused_decisions(&mut state.approval, &frame);
                        let queue = state.approval.queue_view(&config, &resources, &tracks);
                        state.tracks = tracks;
                        api.set_now(now.0);
                        let _ = api.publish_snapshot(
                            SnapshotResponse::new(
                                Vec::new(),
                                None,
                                SystemHealth::default(),
                                Vec::new(),
                            )
                            .with_queue(queue),
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
        Self {
            addr,
            api,
            shared,
            events,
            config,
            resources,
            geo,
            bus,
            dir,
            running,
        }
    }

    /// Propose a plan against this picture, as the node's tick does, and return the item
    /// it queued -- or `None` when policy denied it and it was never queued.
    ///
    /// Runs `approval::propose` under the lock the ticker uses, against the same config,
    /// resources, geofences and bus, so a proposal cannot land halfway through a tick.
    fn propose(&self, tracks: &[TrackView], plan: PlanView) -> Option<PendingApprovalId> {
        let mut state = self.shared.lock().expect("the loop is running");
        state.tracks = tracks.to_vec();
        let before: Vec<PendingApprovalId> = queued_ids(&state.approval);
        let frame = Frame {
            now: state.now,
            config: &self.config,
            tracks,
            resources: &self.resources,
            geofences: &*self.geo,
            bus: &self.bus,
            endpoint_client: None,
            api: &self.api,
        };
        approval::propose(&mut state.approval, &frame, plan);
        queued_ids(&state.approval)
            .into_iter()
            .find(|id| !before.contains(id))
    }

    /// Advance the node's clock, and let its loop read it.
    async fn advance_to(&self, seconds: f64) {
        {
            let mut state = self.shared.lock().expect("the loop is running");
            state.now = MissionTime(seconds);
        }
        settle().await;
    }

    async fn queue(&self) -> Vec<QueueItemView> {
        settle().await;
        self.api.queue()
    }

    fn records(&self) -> Vec<DecisionRecord> {
        self.with_state(|s| s.approval.desk.approvals.records().to_vec())
    }

    fn audit_entries(&self) -> Vec<AuditEntry> {
        self.with_state(|s| s.approval.audit.entries().to_vec())
    }

    fn engagements(&self) -> usize {
        self.with_state(|s| s.approval.desk.engagements.len())
    }

    fn handoffs(&self) -> usize {
        self.with_state(|s| s.approval.desk.handoffs.len())
    }

    fn with_state<T>(&self, f: impl FnOnce(&Shared) -> T) -> T {
        f(&self.shared.lock().expect("the loop is running"))
    }

    /// Every command event published since the last call.
    fn command_events(&self) -> Vec<CommandEvent> {
        self.events
            .try_iter()
            .filter_map(|e| match e.event {
                Event::Command(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for Node {
    /// Stop the loop however the test ended. A failed assertion leaves the thread running
    /// otherwise, and a test binary that cannot exit reports nothing at all.
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

fn queued_ids(approval: &NodeApproval) -> Vec<PendingApprovalId> {
    approval
        .desk
        .approvals
        .queue()
        .iter()
        .map(|p| p.id)
        .collect()
}

fn account(operator: u64, role: Role) -> Account {
    Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

/// Let the loop run a few ticks. It sleeps 2 ms; ten times that is enough for a decision
/// to be taken and published without making a test's pass depend on timing.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
}

// ---------------------------------------------------------------------------------
// Clients
// ---------------------------------------------------------------------------------

async fn sign_in(addr: std::net::SocketAddr, operator: u64) -> String {
    let body = serde_json::json!({ "operator": operator, "passphrase": PASSPHRASE });
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v3/session"))
        .json(&body)
        .send()
        .await
        .expect("the session route");
    assert_eq!(response.status().as_u16(), 200, "sign-in failed");
    let value: serde_json::Value = response.json().await.expect("a session");
    value["token"].as_str().expect("a token").to_owned()
}

/// `POST /v3/queue/{item}/decision` with a well-formed body.
async fn decide(
    addr: std::net::SocketAddr,
    token: &str,
    item: PendingApprovalId,
    request: &str,
    choice: serde_json::Value,
) -> (u16, String) {
    post_decision(
        addr,
        Some(token),
        item,
        serde_json::json!({ "request": request, "item": item.to_string(), "choice": choice }),
    )
    .await
}

async fn post_decision(
    addr: std::net::SocketAddr,
    token: Option<&str>,
    item: PendingApprovalId,
    body: serde_json::Value,
) -> (u16, String) {
    let mut request = reqwest::Client::new()
        .post(format!("http://{addr}/v3/queue/{item}/decision"))
        .json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("the decision route");
    let status = response.status().as_u16();
    (status, response.text().await.expect("a body"))
}

fn accept() -> serde_json::Value {
    serde_json::json!("accept")
}

/// The decision identifier a `201` names.
fn recorded(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body).expect("json")["decision"]
        .as_str()
        .expect("the decision it recorded")
        .to_owned()
}

// ---------------------------------------------------------------------------------
// Row 3: one decision per item
// ---------------------------------------------------------------------------------

/// **DN-31 §9 row 3.** A hundred randomized races, two operator clients deciding the same
/// item over the real transport, and retries with the same request key.
///
/// The criterion, clause by clause: exactly one `DecisionRecord` per item; the first valid
/// decision stands and every later one is refused `409` naming it; a retry answers the
/// first outcome and records nothing; one engagement per solution and one handoff per
/// actionable decision.
///
/// **Why a hundred rather than one.** A single race would pass on a build where the loop
/// happened to see one request first every time. Each round starts both posts together and
/// the order they arrive in is the transport's, so over a hundred rounds each client wins
/// some; what is asserted is invariant to which. The test fails if either client wins them
/// all, because then nothing was raced.
#[tokio::test(flavor = "multi_thread")]
async fn two_clients_racing_one_item_produce_exactly_one_decision() {
    let node = Node::spawn("race").await;
    let first = sign_in(node.addr, OPERATOR).await;
    let second = sign_in(node.addr, SECOND_OPERATOR).await;

    let mut wins = (0usize, 0usize);
    for round in 0..100u32 {
        let item = node
            .propose(
                &[track(0, Classification::Hostile)],
                plan(u128::from(round) + 1, 0, 0),
            )
            .expect("a point plan against a hostile track is the Operator's");
        node.queue().await;

        let key_a = format!("client-a/{round}");
        let key_b = format!("client-b/{round}");
        let (a, b) = tokio::join!(
            decide(node.addr, &first, item, &key_a, accept()),
            decide(node.addr, &second, item, &key_b, accept()),
        );
        let (winner, loser, winning_key, winning_token) = if a.0 == 201 {
            wins.0 += 1;
            (a, b, key_a, &first)
        } else {
            wins.1 += 1;
            (b, a, key_b, &second)
        };
        assert_eq!(winner.0, 201, "one of the two must have been recorded");
        assert_eq!(
            loser.0, 409,
            "the later decision must be refused, not taken: {}",
            loser.1
        );
        let decision = recorded(&winner.1);
        let refused: DecisionRefused = serde_json::from_str(&loser.1).expect("a refusal");
        match refused {
            DecisionRefused::AlreadyDecided {
                decision: stands, ..
            } => assert_eq!(
                stands.to_string(),
                decision,
                "the refusal must name the decision that stands"
            ),
            DecisionRefused::Expired { .. } => panic!("nothing expired in this round"),
        }

        // The retry: the same request key, answered with the same decision, recording
        // nothing. Asked of the client that won, because the loser's key produced none.
        let before = node.records().len();
        let (status, body) = decide(node.addr, winning_token, item, &winning_key, accept()).await;
        assert_eq!(status, 201, "a retry is answered, not refused: {body}");
        assert_eq!(
            recorded(&body),
            decision,
            "a retry must answer the first outcome"
        );
        assert_eq!(
            node.records().len(),
            before,
            "a retry must record nothing new"
        );
    }

    let records = node.records();
    assert_eq!(records.len(), 100, "exactly one decision per item");
    let mut items: Vec<_> = records.iter().map(|r| r.item.expect("the item")).collect();
    items.sort_unstable();
    items.dedup();
    assert_eq!(items.len(), 100, "no item was decided twice");
    assert_eq!(
        node.engagements(),
        100,
        "one engagement per solution, and every plan had one solution"
    );
    assert_eq!(
        node.handoffs(),
        100,
        "one handoff per actionable decision, and every decision was an acceptance"
    );
    assert!(
        wins.0 > 0 && wins.1 > 0,
        "both clients must win some rounds or the race is not a race: {wins:?}"
    );
    node.cleanup();
}

// ---------------------------------------------------------------------------------
// Row 4: authorization on the node
// ---------------------------------------------------------------------------------

/// The two refusals that never reach a session: an expired token, and no token at all.
///
/// Returns how many refusals it made, so the caller's count of audit entries stays one
/// number. **A decision under an expired session is not recorded at all** (DN-23 §5 rule
/// 2), and both are refused before the queue is consulted, so neither can touch the item.
async fn refused_without_a_session(node: &Node, item: PendingApprovalId) -> usize {
    let expired = TokenIssuer::new(vec![9u8; 32], 1.0)
        .expect("issuer")
        .mint(
            &OperatorSession {
                operator: OperatorId(OPERATOR),
                role: Role::Operator,
                established: 0.0,
                expires: Some(1.0),
            },
            0.0,
        )
        .expect("a token");
    for (label, token) in [
        ("expired-session", Some(expired.as_str())),
        ("no-session", None),
    ] {
        let (status, body) = post_decision(
            node.addr,
            token,
            item,
            serde_json::json!({
                "request": label,
                "item": item.to_string(),
                "choice": "accept",
            }),
        )
        .await;
        assert_eq!(status, 401, "{label}: {body}");
    }
    2
}

/// **DN-31 §9 row 4.** Each role against each item state, each choice, and an expired
/// token.
///
/// The criterion: a role without `plan.decide` (or `plan.override` for an override), or
/// not offered the item, is refused `403`; an expired token `401`; a rejection without a
/// reason `400`; **in every refusal nothing is recorded, published or engaged**; and every
/// decision and every refusal writes exactly one audit entry on the node.
#[tokio::test(flavor = "multi_thread")]
async fn every_refusal_records_nothing_and_writes_one_audit_entry() {
    let node = Node::spawn("authorization").await;
    let operator = sign_in(node.addr, OPERATOR).await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;

    let item = node
        .propose(&[track(0, Classification::Hostile)], plan(1, 0, 0))
        .expect("a point plan against a hostile track is the Operator's");
    node.queue().await;
    // The proposal's own events are not what this test is about.
    let _ = node.command_events();

    let before = node.audit_entries().len();
    let mut refusals = refused_without_a_session(&node, item).await;

    // **A role holding no `plan.decide` at all.** An Analyst may read the picture and
    // export a report; the queue is not its to end, whatever it is offered.
    let analyst = sign_in(node.addr, ANALYST).await;
    let (status, body) = decide(node.addr, &analyst, item, "analyst-accept", accept()).await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains(actions::DECIDE_PLAN), "{body}");
    refusals += 1;

    // An override needs `plan.override`, which an Operator does not hold.
    let (status, body) = decide(
        node.addr,
        &operator,
        item,
        "override-by-operator",
        serde_json::json!("override"),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains(actions::OVERRIDE_PLAN), "{body}");
    refusals += 1;

    // A Supervisor holds `plan.override` and is still refused: this item is offered to the
    // Operator and has not escalated. **A permission is not an offer**, which is the check
    // DN-31 §6.3's table puts third.
    let (status, body) = decide(
        node.addr,
        &supervisor,
        item,
        "override-by-supervisor",
        serde_json::json!("override"),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("offered to"), "{body}");
    refusals += 1;

    // The same role, the same item, accepting rather than overriding: still not offered.
    let (status, body) = decide(
        node.addr,
        &supervisor,
        item,
        "accept-by-supervisor",
        accept(),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    refusals += 1;

    // A rejection with no reason: MOE-01 tells a considered rejection from an abandoned
    // decision by the reason alone (DN-10 §3).
    let (status, body) = decide(
        node.addr,
        &operator,
        item,
        "blank-rejection",
        serde_json::json!({ "reject": { "reason": "   " } }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    refusals += 1;

    settle().await;
    assert!(
        node.records().is_empty(),
        "a refusal records nothing: {:?}",
        node.records()
    );
    assert_eq!(node.engagements(), 0, "a refusal engages nothing");
    assert_eq!(node.handoffs(), 0, "a refusal hands nothing off");
    assert!(
        node.command_events().is_empty(),
        "a refusal publishes nothing"
    );
    assert_eq!(
        node.queue().await.len(),
        1,
        "a refused item is still waiting for a person"
    );
    assert_eq!(
        node.audit_entries().len() - before,
        refusals,
        "exactly one audit entry per refusal, and this is {refusals} refusals"
    );

    // And the decision the item was offered for: one more entry, and one only.
    let before = node.audit_entries().len();
    let (status, body) = decide(node.addr, &operator, item, "the-decision", accept()).await;
    assert_eq!(status, 201, "{body}");
    settle().await;
    assert_eq!(
        node.audit_entries().len() - before,
        1,
        "exactly one audit entry per decision"
    );
    assert_eq!(node.records().len(), 1);
    assert_eq!(node.engagements(), 1, "one engagement per solution");
    assert_eq!(node.handoffs(), 1, "one handoff per actionable decision");

    assert_each_entry_is_attributed(&node);
    node.cleanup();
}

/// GAP-137, DN-18 §5 amendment 3: **a partner with an agreement receives a handoff this
/// node issued.**
///
/// The node's own handoffs reached no partner until now. `republish_handoffs` was a
/// documented no-op because the register held one set per item, so the node's set and a
/// desktop's would each have erased the other; a partner was told about an engagement a
/// desktop decided and not about one the node decided, with nothing saying the list was
/// partial.
///
/// **Three claims, in the order they become true.** Before the queue has issued anything
/// the node claims an empty set rather than withholding -- it keeps handoffs now, and
/// says so. After a decision the partner's read holds exactly that decision. And a
/// desktop publishing its own set beside it leaves the node's where it was, which is the
/// producer key doing the only job it has.
///
/// The read is `exchange_for`, in process, because this harness serves plaintext and a
/// partner is named by its certificate. Both of §5's gates are in that call, and the
/// route that answers a partner over TLS is `gungnir-api/tests/exchange.rs`.
#[tokio::test(flavor = "multi_thread")]
async fn a_partner_with_an_agreement_receives_a_handoff_this_node_issued() {
    let node = Node::spawn("exchange").await;
    approval::claim_exchange_items(&node.api).expect("the node's opening claim");
    assert_eq!(
        partner_handoffs(&node),
        Vec::<String>::new(),
        "a node that keeps handoffs claims an empty set before it has issued one"
    );

    let operator = sign_in(node.addr, OPERATOR).await;
    let item = node
        .propose(&[releasable_track(0)], plan(1, 0, 0))
        .expect("a point plan against a hostile track is the Operator's");
    node.queue().await;
    let (status, body) = decide(node.addr, &operator, item, "the-decision", accept()).await;
    assert_eq!(status, 201, "{body}");
    let decision = recorded(&body);
    settle().await;
    assert_eq!(node.handoffs(), 1, "the decision issued one handoff");
    assert_eq!(
        partner_handoffs(&node),
        vec![decision.clone()],
        "the partner was not served the handoff this node issued"
    );

    // A desktop publishes its own set under its own producer, as the write path does for
    // a desktop that holds handoffs of its own.
    node.api
        .publish_exchange(
            ExchangeProducer::Party("desktop-aaaa".into()),
            ExchangeItem::Handoffs,
            vec![ExchangeProduct {
                id: "a-desktop-s-own".into(),
                at: MissionTime(SUBMITTED),
                releasability: Releasability::AllPeers,
                body: serde_json::Value::Null,
            }],
        )
        .expect("the desktop published");
    assert_eq!(
        partner_handoffs(&node),
        vec![decision, "a-desktop-s-own".to_string()],
        "a desktop's publish erased what this node issued"
    );
    node.cleanup();
}

/// What the partner may read of this node's handoffs, in producer order.
fn partner_handoffs(node: &Node) -> Vec<String> {
    match node.api.exchange_for(PARTNER, ExchangeItem::Handoffs) {
        Some(ExchangeResponse::Held { products, .. }) => {
            products.into_iter().map(|p| p.id).collect()
        }
        other => panic!("expected a held set, got {other:?}"),
    }
}

/// A hostile track marked for every peer, so the handoff its decision issues carries a
/// marking the partner's own agreement lets through: `issue_for` combines the tracks'
/// markings, and the default one keeps everything home.
fn releasable_track(id: u64) -> TrackView {
    TrackView {
        releasability: Releasability::AllPeers,
        ..track(id, Classification::Hostile)
    }
}

/// Every audit entry is filed under the action a person would search for, and attributed
/// to whoever was refused or decided (DN-23 §5 rule 1).
fn assert_each_entry_is_attributed(node: &Node) {
    for entry in node.audit_entries() {
        assert_eq!(entry.action, actions::DECIDE_PLAN, "{entry:?}");
    }
    let by = |who: Option<u64>| {
        node.audit_entries()
            .iter()
            .filter(|e| e.operator == who.map(OperatorId))
            .count()
    };
    assert_eq!(
        by(Some(OPERATOR)),
        3,
        "the operator's two refusals and its decision: {:?}",
        node.audit_entries()
    );
    assert_eq!(
        by(Some(SUPERVISOR)),
        2,
        "the supervisor's two refusals, attributed to the supervisor and not to the item's \
         own role"
    );
    assert_eq!(by(Some(ANALYST)), 1, "the analyst's one refusal");
    assert_eq!(
        by(None),
        2,
        "**and the two refusals nobody was authenticated for name nobody**: attribution is \
         never invented (DN-23 §5 rule 1), so a refused prober does not appear in the \
         record as whoever it claimed to be"
    );
}

/// **Row 4, the read side.** A role may see a queue it may not decide.
///
/// `GET /v3/queue` is `picture.view`: a role that holds no authority for an item still has
/// to see that somebody must decide it, which is what DN-09 §7's mark is for, and PN-06
/// disables its control from `offered_to` rather than hiding the row (DN-31 §6.6).
#[tokio::test(flavor = "multi_thread")]
async fn a_role_the_item_is_not_offered_to_may_still_read_the_queue() {
    let node = Node::spawn("read").await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let item = node
        .propose(&[track(0, Classification::Hostile)], plan(1, 0, 0))
        .expect("queued");
    node.queue().await;

    let response = reqwest::Client::new()
        .get(format!("http://{}/v3/queue", node.addr))
        .bearer_auth(&supervisor)
        .send()
        .await
        .expect("the queue route");
    assert_eq!(response.status().as_u16(), 200);
    let rows: Vec<QueueItemView> = response.json().await.expect("a queue");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].item, item);
    assert_eq!(
        rows[0].offered_to,
        vec!["Operator".to_string()],
        "a reader is told who it is offered to"
    );
    node.cleanup();
}

// ---------------------------------------------------------------------------------
// Row 5: authority and offering
// ---------------------------------------------------------------------------------

/// **DN-31 §9 row 5.** Plans per layer and class against the authority matrix.
///
/// The criterion: each item is offered to the lowest role holding authority for every
/// solution; a plan no role on the ladder may accept is `Denied { Authority }` and never
/// queued; an item beyond the Operator's authority is actionable by a role that holds it
/// (GAP-113).
#[tokio::test(flavor = "multi_thread")]
async fn each_item_is_offered_to_the_lowest_role_that_may_take_it() {
    let node = Node::spawn("offering").await;

    // A point engagement against a hostile track: the Operator holds it, and is the lowest
    // rung that does.
    let point = node
        .propose(&[track(0, Classification::Hostile)], plan(1, 0, 0))
        .expect("queued");
    // An area engagement against a hostile track: the Operator does not hold it and the
    // Supervisor does, so it is offered to the Supervisor rather than counted as a denial.
    // **This is GAP-113's case**, which the desktop counts and never queues.
    let area = node
        .propose(&[track(1, Classification::Hostile)], plan(2, 1, 1))
        .expect("an area plan goes to the role that may take it, not to nobody");
    let rows = node.queue().await;
    let offered = |item: PendingApprovalId| {
        rows.iter()
            .find(|r| r.item == item)
            .map(|r| r.offered_to.clone())
            .expect("the item is queued")
    };
    assert_eq!(offered(point), vec!["Operator".to_string()]);
    assert_eq!(
        offered(area),
        vec!["Supervisor".to_string()],
        "the lowest role holding area authority, not the lowest role on the ladder"
    );

    // A point engagement against a *neutral* track: every rule in this baseline names the
    // hostile class, so no role on the ladder may accept it.
    let denied = node.propose(&[track(2, Classification::Neutral)], plan(3, 0, 2));
    assert!(denied.is_none(), "a plan nobody may accept is never queued");
    assert_eq!(
        node.queue().await.len(),
        2,
        "and nothing else left or entered the queue"
    );

    // The supervisor can actually take the area item, which is the half of GAP-113 that
    // "reaches a role that may take it" has to mean.
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let (status, body) = decide(node.addr, &supervisor, area, "area-1", accept()).await;
    assert_eq!(
        status, 201,
        "the role the item was offered to must be able to decide it: {body}"
    );
    // And the Operator still cannot, which is what makes the offer mean something.
    let operator = sign_in(node.addr, OPERATOR).await;
    let second = node
        .propose(&[track(1, Classification::Hostile)], plan(4, 1, 1))
        .expect("queued");
    node.queue().await;
    let (status, body) = decide(node.addr, &operator, second, "area-2", accept()).await;
    assert_eq!(status, 403, "{body}");
    node.cleanup();
}

/// **Row 5, the published verdict.** The denial reaches the record with the engines that
/// ran, rather than only a counter (GAP-028).
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_no_role_may_accept_is_denied_by_authority_on_the_record() {
    let node = Node::spawn("denial").await;
    // Drain what the node published before this proposal.
    let events = node.bus.subscribe();
    let denied = node.propose(&[track(2, Classification::Neutral)], plan(9, 0, 2));
    assert!(denied.is_none());

    let evaluated: Vec<_> = events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Intercept(InterceptEvent::PlanEvaluated {
                verdict, engines, ..
            }) => Some((verdict, engines)),
            _ => None,
        })
        .collect();
    assert_eq!(evaluated.len(), 1, "one evaluation per proposal");
    let summary = format!("{:?}", evaluated[0].0);
    assert!(
        summary.contains("Denied") && summary.contains("Authority"),
        "a plan no role on the ladder may accept is `Denied {{ Authority }}`, and the \
         record carries the reason rather than a bare refusal: {summary}"
    );
    assert_eq!(
        evaluated[0].1,
        gungnir_approval::CHAIN_ENGINES
            .iter()
            .map(|e| (*e).to_string())
            .collect::<Vec<_>>(),
        "the whole chain ran, and the record says which engines -- including authority, \
         which a node ran for no role at all before GAP-132"
    );
    node.cleanup();
}

// ---------------------------------------------------------------------------------
// Row 6: expiry and escalation on the node's clock
// ---------------------------------------------------------------------------------

/// **DN-31 §9 row 6.** Items past `escalate_at`, with a second client signed in as the
/// higher role.
///
/// The criterion's escalation half: escalation adds the next role without removing the
/// first, at most once per rank step, and the higher role's client can decide it.
#[tokio::test(flavor = "multi_thread")]
async fn an_item_escalates_without_losing_its_first_role_and_the_higher_role_decides_it() {
    let node = Node::spawn("escalation").await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let item = node
        .propose(&[track(0, Classification::Hostile)], plan(1, 0, 0))
        .expect("queued");
    assert_eq!(
        node.queue().await[0].offered_to,
        vec!["Operator".to_string()]
    );
    let _ = node.command_events();

    node.advance_to(SUBMITTED + ESCALATE_AFTER_S + 1.0).await;
    assert_eq!(
        node.queue().await[0].offered_to,
        vec!["Operator".to_string(), "Supervisor".to_string()],
        "escalation adds the next role and does not remove the first (DN-10 §5)"
    );
    let escalations = node
        .command_events()
        .into_iter()
        .filter(|e| matches!(e, CommandEvent::Escalated { .. }))
        .count();
    assert_eq!(escalations, 1, "published once, not once per tick");

    // The escalation clock restarts, which is what bounds it at once per rank step. Two
    // more seconds is inside the next step and must add nobody.
    node.advance_to(SUBMITTED + ESCALATE_AFTER_S + 3.0).await;
    assert_eq!(
        node.queue().await[0].offered_to.len(),
        2,
        "at most once per rank step"
    );
    assert!(
        node.command_events()
            .iter()
            .all(|e| !matches!(e, CommandEvent::Escalated { .. })),
        "and nothing more is published inside the step"
    );

    // And the higher role can now decide it.
    let (status, body) = decide(node.addr, &supervisor, item, "escalated-1", accept()).await;
    assert_eq!(status, 201, "{body}");
    let records = node.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].role.as_deref(),
        Some("Supervisor"),
        "the record names the role of the session that decided, not the role it was first \
         offered to"
    );
    assert_eq!(
        records[0].operator_id.as_deref(),
        Some(SUPERVISOR.to_string().as_str())
    );
    node.cleanup();
}

/// **Row 6, the expiry half.** A decision on an expired item is refused, and the record
/// says the window closed rather than that anybody accepted.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_item_is_refused_and_recorded_as_expired_never_accepted() {
    let node = Node::spawn("expiry").await;
    let operator = sign_in(node.addr, OPERATOR).await;
    let item = node
        .propose(&[track(0, Classification::Hostile)], plan(1, 0, 0))
        .expect("queued");
    node.queue().await;
    let _ = node.command_events();

    node.advance_to(SUBMITTED + EXPIRY_S + 1.0).await;
    assert!(
        node.queue().await.is_empty(),
        "an expired item leaves the queue"
    );
    let records = node.records();
    assert_eq!(records.len(), 1, "an expiry leaves a record");
    assert!(
        records[0].is_expiry(),
        "and the record says so: {records:?}"
    );
    assert!(
        !records[0].is_actionable(),
        "**no configuration makes an expiry accept** (C-01, DN-10 §3)"
    );
    assert_eq!(records[0].operator_id, None, "nobody decided");
    assert_eq!(
        records[0].item,
        Some(item),
        "and it names the item it ended"
    );
    assert_eq!(node.engagements(), 0, "an expiry engages nothing");
    assert_eq!(node.handoffs(), 0, "an expiry hands nothing off");
    assert!(
        node.command_events()
            .iter()
            .any(|e| matches!(e, CommandEvent::Expired { .. })),
        "and it is published"
    );

    let (status, body) = decide(node.addr, &operator, item, "too-late", accept()).await;
    assert_eq!(status, 409, "{body}");
    let refused: DecisionRefused = serde_json::from_str(&body).expect("a refusal");
    assert!(
        matches!(refused, DecisionRefused::Expired { .. }),
        "an expired item is refused as expired, not as already decided: {refused:?}"
    );
    assert_eq!(
        node.records().len(),
        1,
        "and the refusal recorded nothing new"
    );
    node.cleanup();
}

/// **Row 6, D-59.** A pre-delegated item is actionable from submission and **still expires
/// and escalates**.
///
/// DN-10 §5 says so and GAP-035's closing note said the opposite; the owner settled it as
/// D-59, and this is the test that holds the settled rule. The baseline here marks the
/// Operator's point rule pre-delegated, so the queue row says `pre_delegated`, and the two
/// deadlines still run.
#[tokio::test(flavor = "multi_thread")]
async fn a_pre_delegated_item_still_expires_and_escalates() {
    let node = Node::spawn_with("delegated", true).await;
    let item = node
        .propose(&[track(0, Classification::Hostile)], plan(1, 0, 0))
        .expect("queued");
    let rows = node.queue().await;
    assert!(
        rows[0].pre_delegated,
        "the fixture's point rule is pre-delegated, or this test proves nothing"
    );

    node.advance_to(SUBMITTED + ESCALATE_AFTER_S + 1.0).await;
    let rows = node.queue().await;
    assert_eq!(
        rows[0].offered_to,
        vec!["Operator".to_string(), "Supervisor".to_string()],
        "a delegated item escalates like any other (D-59)"
    );
    assert!(
        rows[0].pre_delegated,
        "and is still shown as delegated after escalating"
    );

    node.advance_to(SUBMITTED + EXPIRY_S + 1.0).await;
    assert!(
        node.queue().await.is_empty(),
        "and it expires like any other (D-59)"
    );
    let records = node.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].is_expiry());
    assert_eq!(records[0].item, Some(item));
    node.cleanup();
}
