// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **DN-31 §9 row 7: two desktops, one queue** (GAP-133, D-55).
//!
//! The criterion, clause by clause:
//!
//! 1. both desktops show the same queue in the same order;
//! 2. a decision on one reaches the other's PN-06 as decided, naming who;
//! 3. **neither desktop queues, engages or issues a handoff for a node plan**;
//! 4. a plan proposed on the node is available to decide on a desktop in under 500 ms
//!    (MOP-07), measured over the loop, the journal append and the stream (DN-31 §6.9).
//!
//! All four are asserted since the owner confirmed the row on 2026-09-22 (D-16); clause 4,
//! being a timing figure, is asserted in the release profile, which `ci.yml` runs -- see
//! [`two_desktops_show_one_node_queue_and_neither_decides_it_itself`] at the measurement.
//!
//! # What is real here and what is not
//!
//! The node is the real one: `gungnir_node::approval`'s `propose`, `sweep`,
//! `answer_decisions`, `answer_forwarded` and `audit_refused_decisions`, called in the order
//! `gungnir-node/src/main.rs` calls them, against one `NodeApproval`, with the real
//! `gungnir-api` transport bound to loopback and the bus drained into
//! `NodeApi::publish_event` exactly as the binary drains it. The desktops are real
//! `AppState`s on `BackendConfig::Remote`, signed in over `POST /v3/session`, ticked by
//! the real `update::tick`.
//!
//! What is stated rather than computed is the *plan*: what these clauses are about is
//! what two desktops do with a queue, and the allocator's own choices are
//! `gungnir-intercept-service`'s business and are tested there.
//!
//! # Why MOP-07 is measured over spans and not with a stopwatch
//!
//! §6.9 makes the measure span the node loop, the journal append and the stream, so a
//! stopwatch wrapped around the test would also be timing the test's own polling. The
//! two ends are marked with tracing spans -- `mop07.plan_proposed` on the node the
//! instant `propose` is called, `mop07.approval_available` on the desktop the instant
//! PN-06 first carries a decidable row for that plan -- and [`Mop07`] records when each
//! opened. The number is printed, and compared in the release profile (the convention
//! `frame_budgets.rs` states, and why this file follows it, is at the measurement).

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{QueueItemView, SnapshotResponse};
use gungnir_app::state::AppState;
use gungnir_app::{projection, session, update};
use gungnir_command::ApprovalWorkflow;
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, ResourceConfig,
    SecurityConfig,
};
use gungnir_eventing::{Envelope, EventBus, InProcessBus, Receiver};
use gungnir_geo::InMemoryGeoService;
use gungnir_model::policy_settings::AuthorityRule;
use gungnir_model::{
    Classification, EffectorLayer, InterceptSolutionView, MissionTime, PendingApprovalId, PlanId,
    PlanKind, PlanView, Provenance, Quality, Releasability, ResourceId, ResourceView, SystemHealth,
    TrackId, TrackStatus, TrackView, WeaponsControlStatus,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_security::{
    actions, hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use gungnir_ui::panels::approval_queue::PendingId;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing_subscriber::layer::SubscriberExt;

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 21;
const SUPERVISOR: u64 = 22;
/// Mission time the node proposes at. Every deadline is relative to it.
const SUBMITTED: f64 = 100.0;
/// The point layer's decision window on this node, in seconds. Long enough that nothing
/// here expires on the way, and the number GAP-140's countdown is read against.
const POINT_EXPIRY_S: f64 = 600.0;

// ---------------------------------------------------------------------------------
// MOP-07, over the spans the path emits
// ---------------------------------------------------------------------------------

/// When each named span first opened.
///
/// A `tracing` layer rather than a pair of `Instant`s taken in the test body, so what is
/// measured is the path's own marks: the node's when it proposes, the desktop's when the
/// approval control becomes available. Nothing in between is timed by the harness.
#[derive(Clone, Default)]
struct Mop07(Arc<Mutex<Vec<(&'static str, Instant)>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Mop07 {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let name = attrs.metadata().name();
        if name.starts_with("mop07.") {
            if let Ok(mut marks) = self.0.lock() {
                marks.push((name, Instant::now()));
            }
        }
    }
}

impl Mop07 {
    /// The first time this span opened.
    fn at(&self, name: &str) -> Option<Instant> {
        self.0
            .lock()
            .ok()?
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, at)| *at)
    }
}

// ---------------------------------------------------------------------------------
// The baseline both sides read
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

/// The node's baseline.
///
/// **The authority matrix is what makes clause 1 worth asserting.** An Operator may
/// decide a point engagement against a hostile track; an area engagement is a
/// Supervisor's. So one item on the queue is the Operator's to take and another is not,
/// and both desktops have to show both -- which is the difference between showing the
/// node's queue and showing the part of it this console may act on.
fn node_baseline(dir: &std::path::Path) -> ConfigBaseline {
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![effector(0, "point"), effector(1, "area")],
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
    ];
    // Long enough that nothing in this test expires on the way; row 6 is where expiry is
    // the subject, and an expiry here would refuse a decision for the wrong reason.
    config
        .policy
        .decisions
        .expiry_s
        .insert(EffectorLayer::Point, POINT_EXPIRY_S);
    gungnir_config::validate(&config).expect("the node's baseline is valid");
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

struct Shared {
    approval: NodeApproval,
    now: MissionTime,
    tracks: Vec<TrackView>,
}

struct Node {
    api: Arc<NodeApi>,
    shared: Arc<Mutex<Shared>>,
    config: Arc<ConfigBaseline>,
    resources: Arc<Vec<ResourceView>>,
    geo: Arc<InMemoryGeoService>,
    bus: Arc<InProcessBus>,
    dir: std::path::PathBuf,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Node {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Node {
    /// A node serving on loopback with its approval loop running.
    ///
    /// The loop is a plain thread and not `spawn_blocking`, for the reason
    /// `gungnir-node/tests/approval_queue.rs` records: a tokio runtime waits for its
    /// blocking tasks, so a failing assertion would hang the test binary instead of
    /// reporting the failure. `Drop` stops it whether the test passes or panics.
    fn spawn(api: Arc<NodeApi>) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-projection-node-{}-{}",
            std::process::id(),
            scratch_id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Arc::new(node_baseline(&dir));
        let resources = Arc::new(config.resource_views());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        let bus = Arc::new(InProcessBus::new());
        let api_rx: Receiver<Envelope> = bus.subscribe();
        let shared = Arc::new(Mutex::new(Shared {
            approval: NodeApproval::new(&config),
            now: MissionTime(SUBMITTED),
            tracks: Vec::new(),
        }));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
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
                        let Ok(mut state) = shared.lock() else { return };
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
                        // `main.rs`'s order.
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
                    // The same envelopes reach every connected desktop, offered exactly
                    // as `main.rs` offers them.
                    for envelope in api_rx.try_iter() {
                        let _ = api.publish_event(envelope);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
        Self {
            api,
            shared,
            config,
            resources,
            geo,
            bus,
            dir,
            running,
        }
    }

    /// Propose a plan, as the node's tick does, and return the item it queued.
    ///
    /// Marked with `mop07.plan_proposed`: this is the instant §6.9's measure starts.
    fn propose(&self, tracks: &[TrackView], plan: PlanView) -> Option<PendingApprovalId> {
        let _span = tracing::info_span!("mop07.plan_proposed").entered();
        let mut state = self.shared.lock().expect("the loop is running");
        state.tracks = tracks.to_vec();
        let before: Vec<PendingApprovalId> = state
            .approval
            .desk
            .approvals
            .queue()
            .iter()
            .map(|p| p.id)
            .collect();
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
        state
            .approval
            .desk
            .approvals
            .queue()
            .iter()
            .map(|p| p.id)
            .find(|id| !before.contains(id))
    }

    fn queue(&self) -> Vec<QueueItemView> {
        self.api.queue()
    }

    fn records(&self) -> usize {
        self.shared
            .lock()
            .expect("the loop is running")
            .approval
            .desk
            .approvals
            .records()
            .len()
    }

    fn engagements(&self) -> usize {
        self.shared
            .lock()
            .expect("the loop is running")
            .approval
            .desk
            .engagements
            .len()
    }

    fn handoffs(&self) -> usize {
        self.shared
            .lock()
            .expect("the loop is running")
            .approval
            .desk
            .handoffs
            .len()
    }
}

/// A node that knows both accounts.
fn api_knowing_the_accounts() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        account(OPERATOR, Role::Operator),
        account(SUPERVISOR, Role::Supervisor),
    ]);
    let issuer = TokenIssuer::new(vec![5u8; 32], 10_000.0).expect("issuer");
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

fn account(operator: u64, role: Role) -> Account {
    Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

/// Serve on `addr` on a runtime of its own, as the failover end-to-end test does.
fn serve(addr: std::net::SocketAddr, api: Arc<NodeApi>) -> tokio::runtime::Runtime {
    let node = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("node runtime");
    node.block_on(async {
        let listener = bind(addr).await.expect("bound");
        tokio::spawn(async move {
            let _ = serve_on(listener, api).await;
        });
    });
    node
}

// ---------------------------------------------------------------------------------
// The desktops
// ---------------------------------------------------------------------------------

/// Makes every scratch directory in this binary its own.
///
/// **A process id is not enough**: two tests in this binary share a process and therefore
/// an id, so the second one's `remove_dir_all` wipes the first one's journal mid-run.
/// `gungnir-api/tests/exchange.rs` documents the same trap and the same counter.
static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch_id() -> u32 {
    SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A desktop on the connected profile, over a scratch journal directory.
fn desktop(name: &str, endpoint: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-projection-{name}-{}-{}",
        std::process::id(),
        scratch_id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    let accounts = vec![
        account(OPERATOR, Role::Operator),
        account(SUPERVISOR, Role::Supervisor),
    ];
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts).expect("json"),
    )
    .expect("written");
    let mut config = node_baseline(&dir);
    config.backend = BackendConfig::Remote {
        endpoint: endpoint.to_owned(),
    };
    config.security = SecurityConfig {
        authentication: AuthenticationConfig {
            provider: AuthenticationProvider::LocalAccounts {
                accounts_path: "accounts.json".into(),
            },
            ..AuthenticationConfig::default()
        },
        ..SecurityConfig::default()
    };
    gungnir_config::validate(&config).expect("valid");
    (AppState::with_config(config).expect("starts"), dir)
}

/// Sign in, which is what establishes the link on the connected profile (DN-23 §5).
fn sign_in(state: &mut AppState, operator: u64) {
    use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
    let mut draft = SignInDraft {
        operator: operator.to_string(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(state, &mut draft, SessionAction::SignIn);
    assert!(
        matches!(state.backend, BackendConfig::Remote { .. }),
        "operator {operator} did not reach the remote backend: {:?}",
        state.alerts
    );
}

/// Tick both desktops until `check` holds, or fail with what was seen.
fn until(
    a: &mut AppState,
    b: &mut AppState,
    what: &str,
    seconds: f64,
    mut check: impl FnMut(&AppState, &AppState) -> bool,
) {
    let deadline = Instant::now() + std::time::Duration::from_secs_f64(seconds);
    loop {
        update::tick(a);
        update::tick(b);
        if check(a, b) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; alerts: {:?} / {:?}",
            a.alerts,
            b.alerts
        );
        // 1 ms, not 10: this loop is also the clock MOP-07 is read against, and a
        // coarser poll would report the granularity of the test rather than the latency
        // of the path.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// The plans PN-06 is showing, in the order the panel would draw them.
fn shown(state: &AppState) -> Vec<PlanId> {
    projection::queue_rows(state)
        .iter()
        .map(|row| row.plan_id)
        .collect()
}

// ---------------------------------------------------------------------------------
// Row 7
// ---------------------------------------------------------------------------------

/// **DN-31 §9 row 7.** Two desktops linked to one node, each signed in as a different
/// role.
///
/// One test rather than four, because the four clauses are about one situation and
/// splitting them would mean standing two desktops and a node up four times to assert a
/// quarter of the criterion each.
#[test]
#[allow(clippy::too_many_lines)]
fn two_desktops_show_one_node_queue_and_neither_decides_it_itself() {
    let spans = Mop07::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(spans.clone()));

    let addr: std::net::SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    let api = api_knowing_the_accounts();
    let _runtime = serve(addr, Arc::clone(&api));
    let node = Node::spawn(Arc::clone(&api));

    let endpoint = format!("http://{addr}");
    let (mut console_a, dir_a) = desktop("a", &endpoint);
    let (mut console_b, dir_b) = desktop("b", &endpoint);
    sign_in(&mut console_a, OPERATOR);
    sign_in(&mut console_b, SUPERVISOR);
    assert_eq!(console_a.role(), Role::Operator);
    assert_eq!(console_b.role(), Role::Supervisor);

    // **Subscribed, not merely connected.** A `Queued` published into the window between
    // the two reaches no receiver and is not replayed (`NodeApi::subscriber_count`'s own
    // documentation, and the four CI failures `failover_e2e.rs` records).
    until(
        &mut console_a,
        &mut console_b,
        "both streams to subscribe",
        20.0,
        |_, _| api.subscriber_count() >= 2,
    );

    // ---- clause 4: MOP-07, and clause 1 with it ----
    let tracks = vec![track(0), track(1)];
    let point = node
        .propose(&tracks, plan(1, 0, 0))
        .expect("a point plan against a hostile track is the Operator's");
    until(
        &mut console_a,
        &mut console_b,
        "the node's item to reach both desktops",
        20.0,
        |a, b| !shown(a).is_empty() && !shown(b).is_empty(),
    );
    // Marked where the criterion puts it: the approval control is *available* when PN-06
    // carries a row this console may act on.
    {
        let _span = tracing::info_span!("mop07.approval_available").entered();
        let row = projection::queue_rows(&console_a)
            .into_iter()
            .find(|r| r.id == PendingId(point.0))
            .expect("the item is on PN-06");
        assert!(
            row.may_decide,
            "a point plan against a hostile track is the Operator's to decide"
        );
    }
    let proposed = spans.at("mop07.plan_proposed").expect("the node marked it");
    let available = spans
        .at("mop07.approval_available")
        .expect("the desktop marked it");
    let mop07 = available.duration_since(proposed);
    // **Gated 2026-09-22** (the owner's D-16 confirmation of row 7), and asserted in the
    // release profile alone, which is `frame_budgets.rs`'s convention for a timing figure
    // and the reason this clause stopped being asserted on 2026-09-17. It had compared
    // against 500 ms in a debug build on shared runners since GAP-133, measuring 3.4 to
    // 5.1 ms locally and passing on #140's, #143's and `main`'s runs, until #146's run
    // reported 647.1 ms on a change nowhere near the path: nothing on it waits on purpose,
    // and what varied was scheduling, with a node thread, the node's server, two desktops'
    // runtimes and a picture fetch sharing a four-core runner with every other test binary
    // nextest was running.
    //
    // `.github/workflows/ci.yml` runs this test in release beside the frame budgets, so
    // the gate is enforced rather than merely written down.
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "MOP-07: plan proposed on the node to approval control available on a desktop: \
         {:.1} ms (criterion: under 500 ms, {profile} profile)",
        mop07.as_secs_f64() * 1000.0
    );
    if cfg!(debug_assertions) {
        println!(
            "  not asserted in debug; the criterion is a release figure and ci.yml enforces \
             it with --release"
        );
    } else {
        assert!(
            mop07 < std::time::Duration::from_millis(500),
            "MOP-07: {:.1} ms from the node proposing to the control being available, over \
             the loop, the journal append and the stream (DN-31 §6.9)",
            mop07.as_secs_f64() * 1000.0
        );
    }

    // A second plan, which only the Supervisor may decide, so the two consoles differ in
    // what they may act on while showing the same queue.
    let area = node
        .propose(&tracks, plan(2, 1, 1))
        .expect("an area plan against a hostile track is the Supervisor's");
    until(
        &mut console_a,
        &mut console_b,
        "both items to reach both desktops",
        20.0,
        |a, b| shown(a).len() == 2 && shown(b).len() == 2,
    );

    // ---- clause 1: the same queue in the same order ----
    let on_a = shown(&console_a);
    let on_b = shown(&console_b);
    assert_eq!(on_a, on_b, "the two desktops show different queues");
    let node_order: Vec<PlanId> = node.queue().iter().map(|i| i.plan.id).collect();
    assert_eq!(
        on_a, node_order,
        "PN-06 is not showing the node's queue in the node's order"
    );

    // The offering is the node's, and both consoles read the same answer from it: each
    // sees both items, and each may act on the one its role was offered.
    let operator_rows = projection::queue_rows(&console_a);
    let supervisor_rows = projection::queue_rows(&console_b);
    let may = |rows: &[gungnir_ui::panels::approval_queue::QueueRow<'_>],
               item: PendingApprovalId| {
        rows.iter()
            .find(|r| r.id == PendingId(item.0))
            .expect("the item is shown")
            .may_decide
    };
    assert!(
        may(&operator_rows, point),
        "the Operator was offered the point item"
    );
    assert!(
        !may(&operator_rows, area),
        "the area item is beyond the Operator's authority and must not be actionable"
    );
    assert!(
        may(&supervisor_rows, area),
        "the Supervisor was offered the area item"
    );
    // An item this console may not decide names who may, rather than only refusing.
    let area_on_a = operator_rows
        .iter()
        .find(|r| r.id == PendingId(area.0))
        .expect("the area item is shown on the Operator's console");
    assert!(
        area_on_a.offered_to.iter().any(|r| r == "Supervisor"),
        "the row must name the role it is offered to, and names {:?}",
        area_on_a.offered_to
    );

    // ---- clause 2: a decision on one reaches the other, naming who ----
    projection::decide(
        &mut console_a,
        point,
        gungnir_remote::queue::DecisionChoice::Accept,
    )
    .expect("the Operator may decide the point item");
    // **The deciding console's own item goes first.** It holds the node's `201`, so the
    // item is no longer waiting on a person there whatever the stream has said yet; this
    // assertion is what caught it still sitting on PN-06 for a tick or two, where a
    // second click would have earned a `409` naming the operator's own decision.
    until(
        &mut console_a,
        &mut console_b,
        "the deciding console's item to leave PN-06",
        20.0,
        |a, _| !shown(a).contains(&PlanId(1)),
    );
    assert!(
        console_a.projection.queue.outcome_for(PlanId(1)).is_none()
            || console_b.projection.queue.outcome_for(PlanId(1)).is_some(),
        "a 201 says an item was decided, not who decided it; the record comes on the stream"
    );
    until(
        &mut console_a,
        &mut console_b,
        "the decision to reach the other console",
        20.0,
        |_, b| b.projection.queue.outcome_for(PlanId(1)).is_some(),
    );
    let seen = console_b
        .projection
        .queue
        .outcome_for(PlanId(1))
        .expect("the other console saw it end");
    match &seen.ended {
        gungnir_remote::queue::Ended::Decided {
            accepted,
            operator,
            role,
            ..
        } => {
            assert!(accepted, "the Operator accepted it");
            assert_eq!(
                operator.as_deref(),
                Some(OPERATOR.to_string()).as_deref(),
                "the other console must be told who decided"
            );
            assert_eq!(
                role.as_deref(),
                Some("Operator"),
                "and as which role (D-53)"
            );
        }
        gungnir_remote::queue::Ended::Expired { .. } => {
            panic!("a decision is not an expiry")
        }
    }
    // And it has left both queues, because it has left the node's.
    assert!(!shown(&console_a).contains(&PlanId(1)));
    assert!(!shown(&console_b).contains(&PlanId(1)));

    // ---- clause 3: neither desktop queued, engaged or handed off a node plan ----
    //
    // The whole run, not just after the decision: `update::tick` has run on both consoles
    // for the length of this test with the node proposing plans, and the desk is where a
    // desktop's own queue, engagements and handoffs would be.
    for (name, console) in [("a", &console_a), ("b", &console_b)] {
        assert!(
            console.desk.approvals.queue().is_empty(),
            "desktop {name} queued a node plan in its own queue"
        );
        assert!(
            console.desk.approvals.records().is_empty(),
            "desktop {name} recorded a decision on a node plan"
        );
        assert_eq!(
            console.desk.engagements.len(),
            0,
            "desktop {name} opened an engagement for a node plan"
        );
        assert_eq!(
            console.desk.handoffs.len(),
            0,
            "desktop {name} issued a handoff for a node plan"
        );
    }
    // The node did all four, which is what makes the four zeros above mean "the node is
    // the one issuer" rather than "nothing happened".
    assert_eq!(node.records(), 1, "the node holds the one decision");
    assert_eq!(node.engagements(), 1, "the node opened the engagement");
    assert_eq!(node.handoffs(), 1, "the node issued the handoff");

    let _ = std::fs::remove_dir_all(dir_a);
    let _ = std::fs::remove_dir_all(dir_b);
}
// ---------------------------------------------------------------------------------
// GAP-140: a node's deadline is in the node's time
// ---------------------------------------------------------------------------------

/// GAP-140: **PN-06 draws a node's deadline against the node's clock**, not this
/// console's, and PN-01 says when the two disagree.
///
/// `QueueItemView::expires_at` is the node's mission time. Both machines run a wall clock
/// -- seconds since the epoch on each -- so the two agree only as far as the machines do,
/// and nothing on the wire said what the node's was. A console a minute fast showed every
/// item a minute closer to expiry than it was; one a minute slow showed a window still
/// open after it had closed. The node refuses the late decision either way (`409
/// Expired`), so what was wrong was what the operator was told.
///
/// **No node loop, and deliberately so.** What is under test is the desktop's arithmetic
/// against a stated queue, so this serves one stated snapshot rather than running the
/// approval loop beside row 7's: a second loop in this binary would compete for the same
/// cores as the MOP-07 measurement the row above gates on, and `Node::propose` would
/// reach `mop07.plan_proposed` from a thread this test has no subscriber on -- which
/// `tracing` caches as "never interested" for the whole process.
///
/// The disagreement is the extreme case and it is not contrived: this node keeps a stated
/// mission clock at `SUBMITTED` while the desktop keeps the wall clock, so before this
/// change the row read as expired by about fifty-five years.
#[test]
fn a_node_s_deadline_is_drawn_against_the_node_s_clock() {
    let addr: std::net::SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    let api = api_knowing_the_accounts();
    // The node's clock, and one item on its queue with the window this baseline gives the
    // point layer. Both stated, because both are the node's to state.
    api.set_now(SUBMITTED);
    api.publish_snapshot(
        SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new())
            .with_queue(vec![waiting_item()]),
    )
    .expect("the snapshot publishes");
    let _runtime = serve(addr, Arc::clone(&api));

    let endpoint = format!("http://{addr}");
    let (mut console, dir) = desktop("clock", &endpoint);
    let (mut idle, idle_dir) = desktop("clock-idle", &endpoint);
    sign_in(&mut console, OPERATOR);
    until(
        &mut console,
        &mut idle,
        "the node's item to reach PN-06",
        20.0,
        |a, _| !shown(a).is_empty(),
    );

    let row = projection::queue_rows(&console)
        .into_iter()
        .next()
        .expect("the item is on PN-06");
    match row.time_remaining {
        gungnir_ui::panels::approval_queue::TimeRemaining::Seconds(left) => assert!(
            left > POINT_EXPIRY_S - 60.0 && left <= POINT_EXPIRY_S,
            "the countdown is {left} s, not the node's own window of about \
             {POINT_EXPIRY_S} s: it is being drawn against this console's clock"
        ),
        other => panic!("this item carries an expiry, so the row has one: {other:?}"),
    }

    // The other half: the two clocks disagree, and PN-01 is where that is said.
    let skew = projection::clock_skew_s(&console).expect("these two clocks disagree");
    assert!(
        skew < -f64::from(1_000_000),
        "the node keeps a stated mission clock and this console keeps the wall clock, so \
         the skew is large and negative; it was {skew}"
    );
    assert!(
        gungnir_ui::panels::status_strip::clock_skew_sentence(skew).contains("behind"),
        "a node whose clock is behind this console is not said to be behind it"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&idle_dir);
}

/// One item waiting on the node's queue, with the point layer's window from `SUBMITTED`.
fn waiting_item() -> gungnir_api::v3::QueueItemView {
    gungnir_api::v3::QueueItemView {
        item: PendingApprovalId(1),
        plan: plan(1, 0, 0),
        verdict: gungnir_model::events::VerdictSummary::RequiresHumanApproval,
        layer: EffectorLayer::Point,
        submitted: MissionTime(SUBMITTED),
        expires_at: Some(MissionTime(SUBMITTED + POINT_EXPIRY_S)),
        escalate_at: None,
        offered_to: vec!["Operator".into()],
        pre_delegated: false,
        priority: 0.0,
    }
}
