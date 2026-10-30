// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A live weapons-control-status change on the node, over the real transport (GAP-203;
//! `docs/design/DN-09-authority-and-control-status.md` §10; D-140).
//!
//! What is driven here is the node's own wiring -- `gungnir_node::control_status::tick`
//! beside the approval steps `main.rs` runs, in the order `main.rs` runs them, against one
//! `NodeApproval` and one `ConfigBaseline` the tick mutates -- over `gungnir_api`'s real
//! router on loopback, with real session tokens the node minted. The harness is
//! `approval_queue.rs`'s, with a journal beside the bus so the fold at start can be
//! driven against what this node actually wrote.
//!
//! The rows:
//!
//! - a supervisor puts the area layer at hold: `201` with the change; the stream carries
//!   `PolicyEvent::ControlStatusSet`; the snapshot's `control_status` shows hold; a queued
//!   item on that layer is withdrawn, the stream carries `CommandEvent::Withdrawn`,
//!   `GET /v3/queue` no longer lists it and a decision on it is `409 withdrawn`;
//! - the same request key again is `201` with the same change and records nothing; the
//!   same status under a new key is `409 unchanged`;
//! - an operator's token is `403`, audited;
//! - with the baseline's two-person rule on: no confirmation `409 two-person-required`, a
//!   wrong passphrase `403 confirmation-rejected`, the requester confirming themselves
//!   `403 same-operator`, a role without the permission `403 confirmation-not-permitted`,
//!   and a commander's confirmation `201` with `confirmed_by` filled and the audit detail
//!   naming both people;
//! - a restart on the same journal folds the change back over the baseline, and a
//!   baseline with a higher revision makes the fold skip it.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{
    ControlStatusRecorded, ControlStatusRefused, DecisionRefused, QueueItemView, SnapshotResponse,
};
use gungnir_command::ApprovalWorkflow;
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_geo::InMemoryGeoService;
use gungnir_model::events::{CommandEvent, PolicyEvent};
use gungnir_model::policy_settings::AuthorityRule;
use gungnir_model::{
    Classification, EffectorLayer, InterceptSolutionView, MissionTime, PendingApprovalId, PlanId,
    PlanKind, PlanView, Provenance, Quality, Releasability, ResourceId, ResourceView, SystemHealth,
    TrackId, TrackStatus, TrackView, WeaponsControlStatus,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_node::control_status::{self, NodeControlStatus, Tick};
use gungnir_security::{
    actions, hash_passphrase, Account, AuditEntry, InMemoryAccountStore, OperatorId, Role,
    TokenIssuer,
};
use gungnir_store::{EventJournal, FileEventJournal, SessionId};
use std::sync::{Arc, Mutex};

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 11;
const SUPERVISOR: u64 = 13;
const COMMANDER: u64 = 15;
const SUBMITTED: f64 = 100.0;
/// The session this harness journals under.
const SESSION: SessionId = SessionId(1);

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

fn rule(role: &str, layer: EffectorLayer) -> AuthorityRule {
    AuthorityRule {
        action: actions::DECIDE_PLAN.into(),
        role: role.into(),
        layer: Some(layer),
        class: Some("hostile".into()),
        pre_delegated: false,
    }
}

/// Weapons free at both layers, so a plan is queued rather than refused at the door; a
/// supervisor may decide an area engagement, which is the item a hold on the area layer
/// withdraws. `two_person` turns D-140's rule on in the baseline.
fn baseline(name: &str, two_person: bool) -> (ConfigBaseline, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-node-control-status-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    (baseline_in(&dir, two_person), dir)
}

/// The baseline as its file says it, in `dir`, touching nothing on disk: what a restart
/// on the same journal starts from.
fn baseline_in(dir: &std::path::Path, two_person: bool) -> ConfigBaseline {
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![effector(0, "point"), effector(1, "area")],
        revision: 3,
        ..ConfigBaseline::default()
    };
    for layer in [EffectorLayer::Point, EffectorLayer::Area] {
        config
            .policy
            .control_status
            .by_layer
            .insert(layer, WeaponsControlStatus::Free);
        config.assessment.effect_window_s.insert(layer, 60.0);
    }
    config.policy.authority.rules = vec![
        rule("Operator", EffectorLayer::Point),
        rule("Supervisor", EffectorLayer::Point),
        rule("Supervisor", EffectorLayer::Area),
        rule("Commander", EffectorLayer::Area),
    ];
    config.policy.two_person.control_status = two_person;
    gungnir_config::validate(&config).expect("the baseline is valid");
    config
}

fn track(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
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

/// What the ticker owns and a test reads. `config` is here, not shared read-only as in
/// `approval_queue.rs`, because the control-status tick mutates it: that is the point.
struct Shared {
    approval: NodeApproval,
    status: NodeControlStatus,
    config: ConfigBaseline,
    now: MissionTime,
    tracks: Vec<TrackView>,
    journal: FileEventJournal,
}

struct Node {
    addr: std::net::SocketAddr,
    api: Arc<NodeApi>,
    shared: Arc<Mutex<Shared>>,
    events: Receiver<Envelope>,
    resources: Arc<Vec<ResourceView>>,
    geo: Arc<InMemoryGeoService>,
    bus: Arc<InProcessBus>,
    dir: std::path::PathBuf,
    running: Arc<std::sync::atomic::AtomicBool>,
    ticks: Arc<std::sync::atomic::AtomicU64>,
}

fn account(operator: u64, role: Role) -> Account {
    Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

fn api_knowing_the_accounts() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        account(OPERATOR, Role::Operator),
        account(SUPERVISOR, Role::Supervisor),
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
    api.set_now(SUBMITTED);
    api
}

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
    /// A node serving on loopback with its loop running: the control-status tick, then
    /// the approval steps in `main.rs`'s order, then the journal append and the picture.
    // One loop body, in the order `main.rs` runs it; a split would hide the order.
    #[allow(clippy::too_many_lines)]
    async fn spawn(name: &str, two_person: bool) -> Self {
        let (config, dir) = baseline(name, two_person);
        let resources = Arc::new(config.resource_views());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        let bus = Arc::new(InProcessBus::new());
        let events = bus.subscribe();
        let journal_rx = bus.subscribe();
        let journal = FileEventJournal::open(dir.join("gungnir-journal")).expect("a journal");

        let api = api_knowing_the_accounts();
        let addr = serve(api.clone()).await;

        let shared = Arc::new(Mutex::new(Shared {
            approval: NodeApproval::new(&config),
            status: NodeControlStatus::default(),
            config,
            now: MissionTime(SUBMITTED),
            tracks: Vec::new(),
            journal,
        }));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
        std::thread::spawn({
            let api = api.clone();
            let shared = shared.clone();
            let resources = resources.clone();
            let geo = geo.clone();
            let bus = bus.clone();
            let running = running.clone();
            let ticks = ticks.clone();
            move || {
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    {
                        let Ok(mut guard) = shared.lock() else {
                            return;
                        };
                        let Shared {
                            approval,
                            status,
                            config,
                            now,
                            tracks,
                            journal,
                        } = &mut *guard;
                        let now = *now;
                        // GAP-203: before the frame, as in `main.rs`, because the tick
                        // mutates the baseline the frame borrows.
                        control_status::tick(
                            status,
                            approval,
                            api.take_control_status_requests(),
                            config,
                            &Tick {
                                now,
                                tracks,
                                resources: &resources,
                                geofences: &*geo,
                                bus: &bus,
                                endpoint_client: None,
                                api: &api,
                            },
                        );
                        let frame = Frame {
                            now,
                            config: &*config,
                            tracks,
                            resources: &resources,
                            geofences: &*geo,
                            bus: &bus,
                            endpoint_client: None,
                            effectors: None,
                            api: &api,
                        };
                        approval::sweep(approval, &frame);
                        let _ = approval::answer_decisions(approval, &frame);
                        approval::audit_refused_decisions(approval, &frame);
                        // The routes' own refusals reach the audit log here, as in
                        // `main.rs`.
                        approval::audit_routes(approval, &frame);
                        let queue = approval.queue_view(config, &resources, tracks);
                        api.set_now(now.0);
                        let offered: Vec<Envelope> = journal_rx.try_iter().collect();
                        for envelope in &offered {
                            journal
                                .append(SESSION, envelope)
                                .expect("the journal takes the envelope");
                        }
                        let _ = api.publish_snapshot(
                            SnapshotResponse::new(
                                Vec::new(),
                                None,
                                SystemHealth::default(),
                                Vec::new(),
                            )
                            .with_queue(queue)
                            .with_control_status(
                                status.in_force(config),
                                status.changes().to_vec(),
                            ),
                        );
                        for envelope in offered {
                            let _ = api.publish_event(envelope);
                        }
                    }
                    ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
        Self {
            addr,
            api,
            shared,
            events,
            resources,
            geo,
            bus,
            dir,
            running,
            ticks,
        }
    }

    /// Wait until the loop has run one whole tick after this call (see
    /// `approval_queue.rs`'s `settle` for why it is waited on rather than slept on).
    async fn settle(&self) {
        let from = self.ticks.load(std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.ticks.load(std::sync::atomic::Ordering::SeqCst) < from + 2 {
            assert!(
                std::time::Instant::now() < deadline,
                "the node's loop ran no tick in 30 s"
            );
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    /// Propose a plan against this picture, as the node's tick does, and return the item
    /// it queued.
    fn propose(&self, tracks: &[TrackView], plan: PlanView) -> Option<PendingApprovalId> {
        let mut state = self.shared.lock().expect("the loop is running");
        state.tracks = tracks.to_vec();
        let before: Vec<PendingApprovalId> = queued_ids(&state.approval);
        let Shared {
            approval, config, ..
        } = &mut *state;
        let frame = Frame {
            now: MissionTime(SUBMITTED),
            config: &*config,
            tracks,
            resources: &self.resources,
            geofences: &*self.geo,
            bus: &self.bus,
            endpoint_client: None,
            effectors: None,
            api: &self.api,
        };
        approval::propose(approval, &frame, plan);
        queued_ids(approval)
            .into_iter()
            .find(|id| !before.contains(id))
    }

    async fn queue(&self) -> Vec<QueueItemView> {
        self.settle().await;
        self.api.queue()
    }

    fn with_state<T>(&self, f: impl FnOnce(&Shared) -> T) -> T {
        f(&self.shared.lock().expect("the loop is running"))
    }

    fn with_state_mut<T>(&self, f: impl FnOnce(&mut Shared) -> T) -> T {
        f(&mut self.shared.lock().expect("the loop is running"))
    }

    fn audit_entries(&self) -> Vec<AuditEntry> {
        self.with_state(|s| s.approval.audit.entries().to_vec())
    }

    fn changes_held(&self) -> usize {
        self.with_state(|s| s.status.changes().len())
    }

    /// Every event published since the last call.
    fn events(&self) -> Vec<Event> {
        self.events.try_iter().map(|e| e.event).collect()
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for Node {
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

/// `POST /v3/policy/control-status`.
async fn set_status(
    addr: std::net::SocketAddr,
    token: &str,
    body: serde_json::Value,
) -> (u16, String) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v3/policy/control-status"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("the control-status route");
    let status = response.status().as_u16();
    (status, response.text().await.expect("a body"))
}

fn change(request: &str, layer: EffectorLayer, status: WeaponsControlStatus) -> serde_json::Value {
    serde_json::json!({
        "request": request,
        "layer": layer,
        "status": status,
        "reason": "deconfliction with friendly air",
    })
}

fn with_confirmation(
    mut body: serde_json::Value,
    operator: u64,
    passphrase: &str,
) -> serde_json::Value {
    body["confirmation"] = serde_json::json!({ "operator": operator, "passphrase": passphrase });
    body
}

fn recorded(body: &str) -> ControlStatusRecorded {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("a recorded change: {e}: {body}"))
}

fn refused(body: &str) -> ControlStatusRefused {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("a refusal: {e}: {body}"))
}

async fn decide(addr: std::net::SocketAddr, token: &str, item: PendingApprovalId) -> (u16, String) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v3/queue/{item}/decision"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "request": format!("decide/{item}"),
            "item": item.to_string(),
            "choice": "accept",
        }))
        .send()
        .await
        .expect("the decision route");
    let status = response.status().as_u16();
    (status, response.text().await.expect("a body"))
}

// ---------------------------------------------------------------------------------
// The rows
// ---------------------------------------------------------------------------------

/// A supervisor puts the area layer at hold: recorded, on the stream, in the snapshot,
/// and the queued area item withdrawn; a retry answers the same change and records
/// nothing; the same status again is `409 unchanged`.
// One change, followed through every door it reaches, in the order it reaches them.
#[allow(clippy::too_many_lines)]
#[tokio::test(flavor = "multi_thread")]
async fn a_supervisor_sets_a_layer_to_hold_and_the_queue_follows() {
    let node = Node::spawn("hold", false).await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let item = node
        .propose(&[track(0)], plan(1, 1, 0))
        .expect("an area plan against a hostile track is the Supervisor's");
    assert_eq!(node.queue().await.len(), 1, "the item is queued");
    let _ = node.events();
    let audit_before = node.audit_entries().len();

    let (status, body) = set_status(
        node.addr,
        &supervisor,
        change(
            "console-a/1",
            EffectorLayer::Area,
            WeaponsControlStatus::Hold,
        ),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let first = recorded(&body);
    assert_eq!(first.change.layer, EffectorLayer::Area);
    assert_eq!(first.change.status, WeaponsControlStatus::Hold);
    assert_eq!(first.change.previous, WeaponsControlStatus::Free);
    assert_eq!(first.change.operator, SUPERVISOR.to_string());
    assert_eq!(first.change.role, "Supervisor");
    assert_eq!(first.change.revision, 3, "the node's own baseline revision");
    assert!(first.change.confirmed_by.is_none());
    assert_eq!(
        first.change.request.as_ref().map(ToString::to_string),
        Some("console-a/1".to_owned())
    );
    assert_eq!(
        first.withdrawn.iter().map(|w| w.item).collect::<Vec<_>>(),
        vec![item],
        "the queued area item was withdrawn"
    );

    // The stream: the change, and the withdrawal it caused.
    node.settle().await;
    let events = node.events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Policy(PolicyEvent::ControlStatusSet(c))
                if c.layer == EffectorLayer::Area && c.status == WeaponsControlStatus::Hold
        )),
        "the stream carries the change: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Command(CommandEvent::Withdrawn { item: i, layer: EffectorLayer::Area, .. })
                if *i == item
        )),
        "the stream carries the withdrawal: {events:?}"
    );
    let offered = node.api.backlog_since(1).expect("the window holds the run");
    assert!(
        offered
            .iter()
            .any(|e| matches!(e.event, Event::Policy(PolicyEvent::ControlStatusSet(_)))),
        "the change was offered to the transport's stream"
    );

    // The picture: the status in force, the change, and no queued item.
    let snapshot = node.api.snapshot().expect("a snapshot");
    assert_eq!(
        snapshot.control_status.get(&EffectorLayer::Area),
        Some(&WeaponsControlStatus::Hold)
    );
    assert_eq!(snapshot.control_status_changes.len(), 1);
    assert!(node.queue().await.is_empty(), "the item left the queue");
    let (status, body) = decide(node.addr, &supervisor, item).await;
    assert_eq!(status, 409, "{body}");
    let refusal: DecisionRefused = serde_json::from_str(&body).expect("a refusal");
    assert!(
        matches!(
            refusal,
            DecisionRefused::Withdrawn {
                layer: EffectorLayer::Area,
                status: WeaponsControlStatus::Hold,
                ..
            }
        ),
        "{refusal:?}"
    );

    // The audit record: one entry for the change, naming who and why.
    let entries = node.audit_entries();
    let change_entries: Vec<&AuditEntry> = entries[audit_before..]
        .iter()
        .filter(|e| e.action == actions::SET_CONTROL_STATUS)
        .collect();
    assert_eq!(change_entries.len(), 1, "{change_entries:?}");
    assert_eq!(change_entries[0].operator, Some(OperatorId(SUPERVISOR)));
    assert!(
        change_entries[0]
            .detail
            .contains("Area layer set to Hold (was Free)")
            && change_entries[0]
                .detail
                .contains("operator 13 (Supervisor)")
            && change_entries[0]
                .detail
                .contains("deconfliction with friendly air"),
        "{}",
        change_entries[0].detail
    );

    // The retry: the same key, the same change, nothing recorded, nothing withdrawn
    // again. Asked after a publish and before one, so both the route's answer off the
    // snapshot and the loop's own answer are exercised.
    let (status, body) = set_status(
        node.addr,
        &supervisor,
        change(
            "console-a/1",
            EffectorLayer::Area,
            WeaponsControlStatus::Hold,
        ),
    )
    .await;
    assert_eq!(status, 201, "a retry is answered, not refused: {body}");
    let again = recorded(&body);
    assert_eq!(
        again.change, first.change,
        "a retry answers the first change"
    );
    assert!(
        again.withdrawn.is_empty(),
        "nothing is withdrawn a second time"
    );
    assert_eq!(node.changes_held(), 1, "a retry records nothing");
    node.settle().await;
    assert!(
        !node.events().iter().any(|e| matches!(e, Event::Policy(_))),
        "a retry publishes nothing"
    );

    // The same status under a new key: unchanged.
    let (status, body) = set_status(
        node.addr,
        &supervisor,
        change(
            "console-a/2",
            EffectorLayer::Area,
            WeaponsControlStatus::Hold,
        ),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert!(
        matches!(
            refused(&body),
            ControlStatusRefused::Unchanged {
                layer: EffectorLayer::Area,
                status: WeaponsControlStatus::Hold
            }
        ),
        "{body}"
    );
    assert_eq!(node.changes_held(), 1);
    node.cleanup();
}

/// A role without `weapons.control_status` is refused `403` and the refusal is audited
/// under the action naming them; a blank reason is `400`; nothing is recorded.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_is_refused_and_the_refusal_is_audited() {
    let node = Node::spawn("refused", false).await;
    let operator = sign_in(node.addr, OPERATOR).await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let before = node.audit_entries().len();

    let (status, body) = set_status(
        node.addr,
        &operator,
        change(
            "console-b/1",
            EffectorLayer::Point,
            WeaponsControlStatus::Hold,
        ),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains(actions::SET_CONTROL_STATUS), "{body}");

    let mut blank = change(
        "console-b/2",
        EffectorLayer::Point,
        WeaponsControlStatus::Hold,
    );
    blank["reason"] = serde_json::json!("   ");
    let (status, body) = set_status(node.addr, &supervisor, blank).await;
    assert_eq!(status, 400, "{body}");

    node.settle().await;
    let entries = node.audit_entries();
    let refusals: Vec<&AuditEntry> = entries[before..]
        .iter()
        .filter(|e| e.action == actions::SET_CONTROL_STATUS)
        .collect();
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    assert_eq!(refusals[0].operator, Some(OperatorId(OPERATOR)));
    assert!(
        refusals[0].detail.starts_with("refused:"),
        "{}",
        refusals[0].detail
    );
    assert_eq!(refusals[1].operator, Some(OperatorId(SUPERVISOR)));
    assert_eq!(node.changes_held(), 0, "nothing was recorded");
    assert_eq!(
        node.api
            .snapshot()
            .expect("a snapshot")
            .control_status
            .get(&EffectorLayer::Point),
        Some(&WeaponsControlStatus::Free),
        "the baseline's status stands"
    );
    node.cleanup();
}

/// D-140 on the node: with the baseline's rule on, the four refusals and the one
/// confirmation that carries.
#[tokio::test(flavor = "multi_thread")]
async fn the_two_person_rule_needs_a_second_person_who_may_set_the_status() {
    let node = Node::spawn("two-person", true).await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let before = node.audit_entries().len();
    let hold = || {
        change(
            "console-c/1",
            EffectorLayer::Area,
            WeaponsControlStatus::Hold,
        )
    };

    let (status, body) = set_status(node.addr, &supervisor, hold()).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        matches!(refused(&body), ControlStatusRefused::TwoPersonRequired),
        "{body}"
    );

    let (status, body) = set_status(
        node.addr,
        &supervisor,
        with_confirmation(hold(), COMMANDER, "not the passphrase"),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(
        matches!(refused(&body), ControlStatusRefused::ConfirmationRejected),
        "{body}"
    );
    // An unknown operator reads exactly as a wrong passphrase.
    let (status, unknown) = set_status(
        node.addr,
        &supervisor,
        with_confirmation(hold(), 999, PASSPHRASE),
    )
    .await;
    assert_eq!(status, 403, "{unknown}");
    assert_eq!(
        unknown, body,
        "an unknown operator and a wrong passphrase read the same"
    );

    let (status, body) = set_status(
        node.addr,
        &supervisor,
        with_confirmation(hold(), SUPERVISOR, PASSPHRASE),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(
        matches!(refused(&body), ControlStatusRefused::SameOperator),
        "{body}"
    );

    let (status, body) = set_status(
        node.addr,
        &supervisor,
        with_confirmation(hold(), OPERATOR, PASSPHRASE),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(
        matches!(refused(&body), ControlStatusRefused::ConfirmationNotPermitted { role } if role == "Operator"),
        "{body}"
    );
    assert_eq!(
        node.changes_held(),
        0,
        "nothing was recorded by any refusal"
    );

    let (status, body) = set_status(
        node.addr,
        &supervisor,
        with_confirmation(hold(), COMMANDER, PASSPHRASE),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let recorded = recorded(&body);
    assert_eq!(
        recorded.change.confirmed_by,
        Some(gungnir_model::ConfirmingOperator {
            operator: COMMANDER.to_string(),
            role: "Commander".into(),
        })
    );
    node.settle().await;
    assert_two_person_audit(&node.audit_entries(), before);
    node.cleanup();
}

/// The two-person test's audit trail after `before`: its five refusals under the
/// Supervisor, then the change naming both people, and no passphrase anywhere.
fn assert_two_person_audit(entries: &[AuditEntry], before: usize) {
    let under_action: Vec<&AuditEntry> = entries[before..]
        .iter()
        .filter(|e| e.action == actions::SET_CONTROL_STATUS)
        .collect();
    assert_eq!(
        under_action.len(),
        6,
        "five refusals and one change: {under_action:?}"
    );
    assert!(
        under_action[..5]
            .iter()
            .all(|e| e.detail.starts_with("refused:") && e.operator == Some(OperatorId(SUPERVISOR))),
        "{under_action:?}"
    );
    let change_entry = under_action[5];
    assert!(
        change_entry.detail.contains("operator 13 (Supervisor)")
            && change_entry
                .detail
                .contains("confirmed by operator 15 (Commander)"),
        "{}",
        change_entry.detail
    );
    assert!(
        !entries.iter().any(|e| e.detail.contains(PASSPHRASE)),
        "no passphrase reaches the record"
    );
}

/// A restart on the same journal folds the change back over the baseline; a baseline
/// with a higher revision supersedes it and the fold skips it.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_folds_the_change_unless_the_baseline_moved_on() {
    let node = Node::spawn("fold", false).await;
    let supervisor = sign_in(node.addr, SUPERVISOR).await;
    let (status, body) = set_status(
        node.addr,
        &supervisor,
        change(
            "console-d/1",
            EffectorLayer::Area,
            WeaponsControlStatus::Hold,
        ),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    node.settle().await;
    node.with_state_mut(|s| {
        s.journal.sync().expect("synced");
        assert!(
            s.journal
                .read_session(SESSION)
                .expect("reads")
                .iter()
                .any(|e| matches!(e.event, Event::Policy(_))),
            "the change is on the journal"
        );
    });

    // The same baseline, started again on that journal.
    let mut same = baseline_in(&node.dir, false);
    assert_eq!(
        same.policy.control_status.for_layer(EffectorLayer::Area),
        WeaponsControlStatus::Free,
        "the file says free"
    );
    let folded = node.with_state(|s| NodeControlStatus::recover(&s.journal, &mut same));
    assert!(folded.unreadable().is_none());
    assert_eq!(folded.changes().len(), 1);
    assert_eq!(
        same.policy.control_status.for_layer(EffectorLayer::Area),
        WeaponsControlStatus::Hold,
        "the fold put the layer back at hold"
    );

    // A later revision of the baseline: the change was made under revision 3 and is
    // superseded.
    let mut later = baseline_in(&node.dir, false);
    later.revision = 4;
    let folded = node.with_state(|s| NodeControlStatus::recover(&s.journal, &mut later));
    assert!(folded.changes().is_empty(), "superseded, not folded");
    assert_eq!(
        later.policy.control_status.for_layer(EffectorLayer::Area),
        WeaponsControlStatus::Free
    );
    node.cleanup();
}
