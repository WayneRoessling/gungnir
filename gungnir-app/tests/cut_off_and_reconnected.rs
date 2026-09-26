// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **DN-31 §9 row 8: cut off and reconnected** (GAP-134; D-15, D-53, D-55, D-58).
//!
//! A desktop loses its node mid-queue, decides, and reconnects, while the node's queue
//! keeps running for a second desktop. The criterion, clause by clause, each written so it
//! cannot pass on nothing:
//!
//! 1. **every offline decision reaches the node's record exactly once** (MOE-11 = 1.0) and
//!    none is lost or duplicated (MT-10) -- five decisions, none on the node before the
//!    forwarding and each once after it, with the node's engagements unchanged by it;
//! 2. **forwarding twice records nothing new** -- the batch reaches the node twice, once
//!    because the link retried it after a real `504` and once because it was sent again,
//!    and the record holds five;
//! 3. **D-15's delegations lapse after `disconnected_lapse_s`** -- a delegated item is the
//!    Operator's one second before the interval and not one second after, while an item the
//!    Operator holds on its own account stays the Operator's across it;
//! 4. **plan conflicts are resolved by D-53's rule or a person** -- one of each, both on
//!    the node's record afterwards, and nothing is forwarded until both are settled;
//! 5. **two engagements of one track across the outage raise `BothActed` for a person
//!    whatever the verdict** -- two tracks engaged on both sides, one with no plan
//!    conflict at all and one whose conflict the rule settled against the node's side, and
//!    two tracks engaged on one side only that raise nothing.
//!
//! # What is real here and what is stated
//!
//! The node is the real one: `gungnir_node::approval`'s loop steps in the order
//! `gungnir-node/src/main.rs` calls them, including `answer_forwarded`, against one
//! `NodeApproval`, on the real `gungnir-api` transport bound to loopback. Both desktops are
//! real `AppState`s on `BackendConfig::Remote`, signed in over `POST /v3/session`, ticked
//! by `update::tick`. **The outage is synthetic and the transport is not**: desktop A
//! reaches the node through a TCP proxy the test can cut, so A loses its node exactly as a
//! network would take it away -- its sockets close, its link hears nothing past
//! `HEARTBEAT_TIMEOUT`, it falls back -- while desktop B, connected directly, goes on
//! deciding on the node's queue.
//!
//! What is stated is the plans and the cut-off desktop's picture, as rows 7 and 9 state
//! them: what this row is about is what an outage's decisions become, and the allocator's
//! choices are tested where it lives.
//!
//! **One plan is on both queues because the test puts it there**, and that is a finding
//! rather than a shortcut. Since GAP-130 a plan's identifier is minted where it is
//! proposed, and since GAP-133 a linked desktop queues no node plan, so a cut-off desktop's
//! plans and the node's cannot share an identifier: pairing decisions by plan finds no
//! real conflict across an outage in the system as built, which is exactly why D-58 added
//! the comparison by track. Clause 4 still has to be shown to settle a conflict, so the
//! cut-off desktop is handed two of the node's plans -- what a desktop did before GAP-133
//! -- and decides them the other way. The node's side of each conflict is real: a decision
//! the second desktop took on the node's queue over the route.
//!
//! **Desktop A never signs in again while it is cut off.** A sign-in during an outage ends
//! it without a person switching back and discards its reconciliation (GAP-143), so the
//! decision with no recorded role is taken with nobody signed in -- DN-23 §5 rule 5's
//! role-selected fallback -- and PN-18's person acts in the selected role.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, ExchangeProducer, NodeApi};
use gungnir_api::v3::{Settlement, SnapshotResponse};
use gungnir_app::failover::{self, Forwarding, ReconciliationView};
use gungnir_app::state::AppState;
use gungnir_app::{decisions, projection, session, update};
use gungnir_command::{ApprovalWorkflow, DecisionRecord, OperatorDecision};
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, ResourceConfig,
    SecurityConfig,
};
use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_geo::InMemoryGeoService;
use gungnir_intercept_service::{InterceptService, PlanOutcome};
use gungnir_model::arbitration::ArbitrationGround;
use gungnir_model::events::{CommandEvent, LinkEvent};
use gungnir_model::policy_settings::AuthorityRule;
use gungnir_model::{
    Classification, EffectorLayer, InterceptSolutionView, MissionTime, PendingApprovalId, PlanId,
    PlanKind, PlanView, Provenance, Quality, Releasability, ResourceId, ResourceView, SystemHealth,
    TrackId, TrackStatus, TrackView, WeaponsControlStatus,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_policy::{Delegations, PolicyVerdict};
use gungnir_remote::link::HEARTBEAT_TIMEOUT;
use gungnir_remote::queue::DecisionChoice;
use gungnir_security::{
    actions, hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};
use gungnir_ui::panels::approval_queue::PendingId;
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PASSPHRASE: &str = "correct horse battery staple";
/// Desktop A's operator: cut off, deciding on its own queue.
const A_OPERATOR: u64 = 61;
/// Desktop B's operator: linked throughout, deciding on the node's queue.
const B_OPERATOR: u64 = 62;
/// Both are Operators, so D-15's delegation is what lets either take a hostile point
/// engagement, and D-03's rule sees equal ranks and falls to the earlier decision.
const ROLE: Role = Role::Operator;
/// An account that may publish to the coalition exchange, which `ROLE` may not
/// (`PUBLISH_EXCHANGE` is granted to Commander, IntelligenceAnalyst and Supervisor:
/// DN-18 §5 amendment 2). GAP-145's test signs in as this one, because a handoff only
/// reaches the node's register when the console that issued it may send it.
const SUPERVISOR: u64 = 63;
/// `policy.delegation.disconnected_lapse_s`.
const LAPSE_S: f64 = 30.0;
/// The mission time at which A is cut off, which is when its interval starts.
const CUT: f64 = 110.0;
/// The mission time at which A's node answers again.
const RESTORED: f64 = 150.0;

// Plans. Node-only, cut-off-desktop-only, and the two the test puts on both queues.
const NODE_N: u128 = 1001; // track 42, decided on the node
const NODE_M: u128 = 1002; // track 46, decided on the node only: the node-side zero
const SHARED_C: u128 = 2001; // track 43, both queues; the rule settles it
const SHARED_D: u128 = 2002; // track 44, both queues; a person settles it
const A_A: u128 = 3001; // track 42, accepted on A: 42 is engaged on both sides
const A_B: u128 = 3002; // track 43, accepted on A: 43 is engaged on both sides
const A_E: u128 = 3003; // track 45, accepted on A only: the desktop-side zero
const A_L: u128 = 3004; // track 47, delegated, left waiting: withdrawn at the lapse
const A_U: u128 = 3005; // track 50, the Operator's own case, left waiting: kept
const A_X: u128 = 3006; // track 48, delegated, proposed after the lapse: the Supervisor's

// ---------------------------------------------------------------------------------
// The baseline every machine reads
// ---------------------------------------------------------------------------------

fn rule(role: &str, class: &str, pre_delegated: bool) -> AuthorityRule {
    AuthorityRule {
        action: actions::DECIDE_PLAN.into(),
        role: role.into(),
        layer: Some(EffectorLayer::Point),
        class: Some(class.into()),
        pre_delegated,
    }
}

/// The policy the node and both desktops share.
///
/// **The matrix is what gives clause 3 something to withdraw and something to keep.** An
/// Operator takes a hostile point engagement only because a Supervisor delegated it
/// (D-15), holds an unknown-class one on its own account, and a Supervisor holds the
/// hostile case on its own account -- so a lapsed delegation has somewhere to send the
/// item rather than nowhere.
fn baseline(dir: &std::path::Path) -> ConfigBaseline {
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![ResourceConfig {
            handoff_endpoint: None,
            id: 0,
            position: [0.0, 0.0, 0.0],
            capacity: 16,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: Some(400.0),
        }],
        ..ConfigBaseline::default()
    };
    config
        .policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
    // An engagement opens only where a window says when its effect is expected (DN-06),
    // and clause 5 is about engagements.
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 600.0);
    config.policy.authority.rules = vec![
        rule("Operator", "hostile", true),
        rule("Supervisor", "hostile", false),
        rule("Operator", "unknown", false),
    ];
    // Long enough that nothing expires or escalates on the way: an expiry here would end
    // an item for a reason that is not this row's.
    config
        .policy
        .decisions
        .expiry_s
        .insert(EffectorLayer::Point, 3600.0);
    config
        .policy
        .decisions
        .escalate_after_s
        .insert(EffectorLayer::Point, 1800.0);
    config.policy.delegation.disconnected_lapse_s = Some(LAPSE_S);
    gungnir_config::validate(&config).expect("the baseline is valid");
    config
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
        mission_time: MissionTime(100.0),
        releasability: Releasability::default(),
    }
}

/// The picture, the same on every machine: hostile tracks, and one of unknown class.
fn picture() -> Vec<TrackView> {
    let mut tracks: Vec<TrackView> = (42..=48)
        .map(|id| track(id, Classification::Hostile))
        .collect();
    tracks.push(track(50, Classification::Unknown));
    tracks
}

fn plan(id: u128, track: u64) -> PlanView {
    PlanView {
        id: PlanId(id),
        mission_time: MissionTime(100.0),
        kind: PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(0),
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
    addr: SocketAddr,
    api: Arc<NodeApi>,
    shared: Arc<Mutex<Shared>>,
    config: Arc<ConfigBaseline>,
    resources: Arc<Vec<ResourceView>>,
    geo: Arc<InMemoryGeoService>,
    bus: Arc<InProcessBus>,
    /// Everything the node's bus carried, which is what its journal appends.
    journal: Mutex<Vec<Envelope>>,
    journal_rx: Receiver<Envelope>,
    /// While set, the loop does not take forwarded batches, so the route's reply window
    /// closes on one and the link has to send it again (clause 2).
    forwarding_held: Arc<AtomicBool>,
    dir: std::path::PathBuf,
    running: Arc<AtomicBool>,
    /// The node's runtime: the transport, and since GAP-150 a partner listener, run here.
    server: tokio::runtime::Runtime,
}

impl Drop for Node {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Node {
    /// A node serving on loopback with its approval loop running, on a plain thread for the
    /// reason `gungnir-node/tests/approval_queue.rs` gives.
    fn spawn() -> Self {
        Self::spawn_with(api_knowing_both_operators())
    }

    /// [`Self::spawn`] over a transport the test built, so a node can carry an exchange
    /// agreement with a partner (GAP-150).
    fn spawn_with(api: Arc<NodeApi>) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-row8-node-{}-{}",
            std::process::id(),
            scratch_id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Arc::new(baseline(&dir));
        let resources = Arc::new(config.resource_views());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        let bus = Arc::new(InProcessBus::new());
        let api_rx: Receiver<Envelope> = bus.subscribe();
        let journal_rx: Receiver<Envelope> = bus.subscribe();
        let shared = Arc::new(Mutex::new(Shared {
            approval: NodeApproval::new(&config),
            now: MissionTime(100.0),
            tracks: picture(),
        }));
        let running = Arc::new(AtomicBool::new(true));
        let forwarding_held = Arc::new(AtomicBool::new(false));
        std::thread::spawn({
            let api = api.clone();
            let shared = shared.clone();
            let config = config.clone();
            let resources = resources.clone();
            let geo = geo.clone();
            let bus = bus.clone();
            let running = running.clone();
            let forwarding_held = forwarding_held.clone();
            move || {
                while running.load(Ordering::Relaxed) {
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
                        if !forwarding_held.load(Ordering::Relaxed) {
                            approval::answer_forwarded(&mut state.approval, &frame);
                        }
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
                    for envelope in api_rx.try_iter() {
                        let _ = api.publish_event(envelope);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        });
        let addr: SocketAddr = {
            let probe = TcpListener::bind("127.0.0.1:0").expect("probe");
            probe.local_addr().expect("addr")
        };
        let server = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("node runtime");
        server.block_on(async {
            let listener = bind(addr).await.expect("bound");
            let api = api.clone();
            tokio::spawn(async move {
                let _ = serve_on(listener, api).await;
            });
        });
        Self {
            addr,
            api,
            shared,
            config,
            resources,
            geo,
            bus,
            journal: Mutex::new(Vec::new()),
            journal_rx,
            forwarding_held,
            dir,
            running,
            server,
        }
    }

    /// Move the node's clock.
    fn at(&self, t: f64) {
        self.shared.lock().expect("the loop is running").now = MissionTime(t);
    }

    /// Propose a plan as the node's tick does, and return the item it queued.
    fn propose(&self, plan: PlanView) -> PendingApprovalId {
        let mut state = self.shared.lock().expect("the loop is running");
        let before: Vec<PendingApprovalId> = state
            .approval
            .desk
            .approvals
            .queue()
            .iter()
            .map(|p| p.id)
            .collect();
        let tracks = state.tracks.clone();
        let frame = Frame {
            now: state.now,
            config: &self.config,
            tracks: &tracks,
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
            .expect("the node queued the plan")
    }

    fn records(&self) -> Vec<DecisionRecord> {
        self.shared
            .lock()
            .expect("the loop is running")
            .approval
            .desk
            .approvals
            .records()
            .to_vec()
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

    fn audit_details(&self) -> Vec<String> {
        self.shared
            .lock()
            .expect("the loop is running")
            .approval
            .audit
            .entries()
            .iter()
            .map(|e| e.detail.clone())
            .collect()
    }

    /// Everything the node's bus has carried so far.
    fn journal(&self) -> Vec<Envelope> {
        let mut journal = self.journal.lock().expect("journal");
        journal.extend(self.journal_rx.try_iter());
        journal.clone()
    }

    fn hold_forwarding(&self, held: bool) {
        self.forwarding_held.store(held, Ordering::Relaxed);
    }
}

fn account(operator: u64) -> Account {
    account_as(operator, ROLE)
}

fn account_as(operator: u64, role: Role) -> Account {
    Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

fn api_knowing_both_operators() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        account(A_OPERATOR),
        account(B_OPERATOR),
        account_as(SUPERVISOR, Role::Supervisor),
    ]);
    let issuer = TokenIssuer::new(vec![8u8; 32], 10_000.0).expect("issuer");
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
    api.set_now(100.0);
    api
}

// ---------------------------------------------------------------------------------
// The cut: a TCP proxy between desktop A and the node
// ---------------------------------------------------------------------------------

/// A loopback TCP proxy that can be cut and restored.
///
/// Plain threads and `std::net`, deliberately: it carries HTTP and the WebSocket alike
/// because it knows nothing about either, and a cut is what a network does to both at
/// once -- every open socket closed, every new one refused -- while the node on the other
/// side goes on serving everybody else.
struct Proxy {
    addr: SocketAddr,
    /// Where connections are sent: a node, or after [`Proxy::retarget`] the node that
    /// replaced it at the same address as far as a desktop can tell (GAP-150).
    upstream: Arc<Mutex<SocketAddr>>,
    open: Arc<AtomicBool>,
    live: Arc<Mutex<Vec<TcpStream>>>,
    stop: Arc<AtomicBool>,
}

impl Proxy {
    fn start(upstream: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy bound");
        let addr = listener.local_addr().expect("proxy addr");
        let open = Arc::new(AtomicBool::new(true));
        let live: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let upstream = Arc::new(Mutex::new(upstream));
        std::thread::spawn({
            let upstream = upstream.clone();
            let open = open.clone();
            let live = live.clone();
            let stop = stop.clone();
            move || {
                for inbound in listener.incoming() {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let Ok(inbound) = inbound else { continue };
                    let Ok(target) = upstream.lock().map(|a| *a) else {
                        continue;
                    };
                    let Ok(outbound) = TcpStream::connect(target) else {
                        let _ = inbound.shutdown(std::net::Shutdown::Both);
                        continue;
                    };
                    // **Admitted and registered under the lock `cut` holds** (GAP-167).
                    // This checked `open`, connected upstream, and only then registered
                    // the connection, so a cut landing in between drained the list without
                    // it and the connection was piped anyway: one live socket through a
                    // cut proxy. The link's WebSocket is opened just after the link reports
                    // connected, which is exactly when these tests cut, so that socket went
                    // on carrying the node's heartbeat, the desktop never heard silence,
                    // and "the desktop to fall back" waited out its 30 s.
                    let Ok(mut held) = live.lock() else { continue };
                    if !open.load(Ordering::SeqCst) {
                        let _ = inbound.shutdown(std::net::Shutdown::Both);
                        let _ = outbound.shutdown(std::net::Shutdown::Both);
                        continue;
                    }
                    let (Ok(i), Ok(o)) = (inbound.try_clone(), outbound.try_clone()) else {
                        continue;
                    };
                    held.push(i);
                    held.push(o);
                    drop(held);
                    pipe(&inbound, &outbound);
                    pipe(&outbound, &inbound);
                }
            }
        });
        Self {
            addr,
            upstream,
            open,
            live,
            stop,
        }
    }

    /// Send every later connection to `upstream` instead: a node restarted where the old
    /// one was, as a desktop sees it (GAP-150). Connections already piped are the cut's.
    fn retarget(&self, upstream: SocketAddr) {
        if let Ok(mut target) = self.upstream.lock() {
            *target = upstream;
        }
    }

    /// Close every connection through the proxy and refuse new ones.
    ///
    /// Under the registration lock, so every connection is either registered before the
    /// cut and closed by it, or admitted after it and refused (GAP-167).
    fn cut(&self) {
        if let Ok(mut held) = self.live.lock() {
            self.open.store(false, Ordering::SeqCst);
            for stream in held.drain(..) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    fn restore(&self) {
        if let Ok(_held) = self.live.lock() {
            self.open.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.cut();
        // Wake the accept loop so it sees `stop`.
        let _ = TcpStream::connect(self.addr);
    }
}

/// Copy one direction until either side closes, then close both.
fn pipe(from: &TcpStream, to: &TcpStream) {
    let (Ok(mut from), Ok(mut to)) = (from.try_clone(), to.try_clone()) else {
        return;
    };
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut from, &mut to);
        let _ = from.shutdown(std::net::Shutdown::Both);
        let _ = to.shutdown(std::net::Shutdown::Both);
    });
}

// ---------------------------------------------------------------------------------
// The desktops
// ---------------------------------------------------------------------------------

/// The picture a cut-off desktop runs on. Its embedded tracker has no sensors behind it
/// here, so the tracks are stated, exactly as the engagements tests state them.
struct Picture(Vec<TrackView>);

impl TrackingService for Picture {
    fn submit_detection(&mut self, _: gungnir_model::DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &self.0
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

/// A planner that proposes nothing, so the only plans on the cut-off desktop's queue are
/// the ones the test states and every count below is the test's own.
struct StatedPlans;

impl InterceptService for StatedPlans {
    fn plan(&mut self, _: MissionTime, _: &[TrackView], _: &[ResourceView]) -> PlanOutcome {
        PlanOutcome::NoPlan {
            reason: "this test states its plans".into(),
            progress: None,
        }
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

/// Makes every scratch directory in this binary its own.
///
/// **A process id is not enough**: two tests in this binary share a process and therefore
/// an id, so the second one's `remove_dir_all` wipes the first one's journal mid-run.
/// `gungnir-api/tests/exchange.rs` documents the same trap and the same counter.
static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch_id() -> u32 {
    SCRATCH.fetch_add(1, Ordering::Relaxed)
}

fn desktop(name: &str, endpoint: SocketAddr) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-row8-{name}-{}-{}",
        std::process::id(),
        scratch_id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&vec![
            account(A_OPERATOR),
            account(B_OPERATOR),
            account_as(SUPERVISOR, Role::Supervisor),
        ])
        .expect("json"),
    )
    .expect("written");
    let mut config = baseline(&dir);
    config.backend = BackendConfig::Remote {
        endpoint: format!("http://{endpoint}"),
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
    let mut state = AppState::with_config(config).expect("starts");
    set_clock(&mut state, 100.0);
    (state, dir)
}

/// The same desktop again, over the journal it already has (GAP-142).
///
/// **Not `desktop`**, which wipes the directory: what is under test is a process that
/// starts where the last one stopped, so the accounts, the baseline and above all the
/// journal are the ones the previous process wrote.
fn desktop_again(dir: &std::path::Path, endpoint: SocketAddr) -> AppState {
    let mut config = baseline(dir);
    config.backend = BackendConfig::Remote {
        endpoint: format!("http://{endpoint}"),
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
    let mut state = AppState::with_config(config).expect("starts");
    set_clock(&mut state, 100.0);
    state
}

fn set_clock(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
}

fn sign_in(state: &mut AppState, operator: u64) {
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

fn sign_out(state: &mut AppState) {
    session::apply(state, &mut SignInDraft::default(), SessionAction::SignOut);
}

/// Tick both desktops until `check` holds, or fail with what was seen.
fn until(
    a: &mut AppState,
    b: &mut AppState,
    what: &str,
    seconds: f64,
    mut check: impl FnMut(&AppState, &AppState) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        update::tick(a);
        update::tick(b);
        if check(a, b) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; A: {:?} / B: {:?}",
            a.alerts,
            b.alerts
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Queue a stated plan on the cut-off desktop's own queue, through the desktop's own chain.
fn submit(state: &mut AppState, plan: PlanView) -> PendingApprovalId {
    let id = plan.id;
    assert_eq!(
        decisions::submit(state, plan),
        decisions::Submitted::Evaluated(PolicyVerdict::RequiresHumanApproval),
        "plan {id} was not queued: {:?}",
        state.alerts
    );
    state
        .desk
        .approvals
        .queue()
        .iter()
        .find(|p| p.plan.id == id)
        .map(|p| p.id)
        .expect("queued")
}

/// Decide an item on the cut-off desktop's own queue, and return the record it left.
fn decide(
    state: &mut AppState,
    item: PendingApprovalId,
    decision: OperatorDecision,
) -> DecisionRecord {
    decisions::decide(state, PendingId(item.0), decision).expect("decided");
    state
        .desk
        .approvals
        .records()
        .last()
        .cloned()
        .expect("recorded")
}

fn offered_to(state: &AppState, plan: u128) -> Vec<String> {
    state
        .desk
        .approvals
        .queue()
        .iter()
        .find(|p| p.plan.id == PlanId(plan))
        .map(|p| p.offered_to.clone())
        .expect("still queued")
}

fn reconciled(state: &AppState) -> (failover::Reconciliation, bool, Forwarding) {
    match failover::reconciliation_view(state) {
        ReconciliationView::Due {
            reconciliation: Some(Ok(r)),
            can_switch_back,
            forwarding,
            ..
        } => (r, can_switch_back, forwarding),
        other => panic!("no reconciliation computed: {other:?}"),
    }
}

fn forwarding(state: &AppState) -> Forwarding {
    state
        .fallback
        .as_ref()
        .map(|f| f.forwarding.clone())
        .expect("still in the outage")
}

// ---------------------------------------------------------------------------------
// Row 8
// ---------------------------------------------------------------------------------

/// **DN-31 §9 row 8.** One outage, told in the order it happens, because the clauses are
/// about one situation and their order -- the lapse before the reconnection, the merge
/// before the forwarding, the forwarding before the switch -- is part of what is tested.
// The row's five clauses are one story, and splitting it would hide the order the record
// is built in, which the failover end-to-end test's own allowance says of the same kind of
// test.
#[allow(clippy::too_many_lines)]
#[test]
fn an_outage_decided_offline_reaches_the_node_once_and_what_both_sides_did_is_faced() {
    let node = Node::spawn();
    let proxy = Proxy::start(node.addr);
    let (mut a, a_dir) = desktop("a", proxy.addr);
    let (mut b, b_dir) = desktop("b", node.addr);
    let a_bus = a.events.subscribe();
    // GAP-141: what desktop A calls itself, which is what its forwarded batch must say it
    // is. Read from A rather than stated here, so the test cannot pass against an origin
    // no desktop would send. It differs from B's, which is the point of the gap.
    let a_origin = session::origin_of(&a);
    assert_ne!(
        a_origin,
        session::origin_of(&b),
        "two desktops named themselves the same thing"
    );

    // Linked: both signed in, both following the stream.
    sign_in(&mut a, A_OPERATOR);
    sign_in(&mut b, B_OPERATOR);
    until(&mut a, &mut b, "both desktops to link", 15.0, |a, b| {
        [a, b].iter().all(|s| {
            s.link
                .as_ref()
                .is_some_and(gungnir_remote::link::NodeLink::connected)
        })
    });
    until(&mut a, &mut b, "both streams to subscribe", 15.0, |_, _| {
        node.api.subscriber_count() >= 2
    });

    // --- The cut ------------------------------------------------------------------
    let at = |a: &mut AppState, b: &mut AppState, t: f64| {
        set_clock(a, t);
        set_clock(b, t);
        node.at(t);
    };
    at(&mut a, &mut b, CUT);
    proxy.cut();
    until(
        &mut a,
        &mut b,
        "desktop A to lose its node and fall back",
        HEARTBEAT_TIMEOUT.as_secs_f64() + 15.0,
        |a, _| a.fallback.is_some(),
    );
    assert_eq!(
        a.fallback.as_ref().map(|f| f.since),
        Some(MissionTime(CUT)),
        "the interval runs from the moment the node went silent"
    );
    assert!(
        b.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected),
        "the cut took the node away from desktop B too; the row needs it running for B"
    );
    // The cut-off desktop's picture and planner, stated (see the module documentation).
    a.tracking = Box::new(Picture(picture()));
    a.intercept = Box::new(StatedPlans);
    assert_eq!(
        failover::delegations(&a),
        Delegations::AsConfigured,
        "a delegation in force at the cut stays in force for the interval (D-15)"
    );

    // --- Meanwhile, the node's queue keeps running for desktop B --------------------
    let node_decides = |a: &mut AppState, b: &mut AppState, t: f64, plan: PlanView| {
        at(a, b, t);
        let item = node.propose(plan);
        let before = node.records().len();
        projection::decide(b, item, DecisionChoice::Accept).expect("posted");
        until(a, b, "the node to record B's decision", 10.0, |_, _| {
            node.records().len() > before
        });
    };

    // --- The outage, in mission-time order -----------------------------------------
    node_decides(&mut a, &mut b, 115.0, plan(NODE_N, 42));

    at(&mut a, &mut b, 116.0);
    let item = submit(&mut a, plan(A_A, 42));
    let rec_a = decide(&mut a, item, OperatorDecision::Accepted);

    at(&mut a, &mut b, 117.0);
    let item = submit(&mut a, plan(A_B, 43));
    let rec_b = decide(&mut a, item, OperatorDecision::Accepted);

    // One of the node's plans, put on A's queue as well (the module documentation says
    // why), and rejected here before the node's side accepts it: equal ranks, so D-03's
    // rule keeps the earlier decision, which is this desktop's.
    at(&mut a, &mut b, 118.0);
    let item = submit(&mut a, plan(SHARED_C, 43));
    let rec_c = decide(
        &mut a,
        item,
        OperatorDecision::Rejected {
            reason: "the same track is already engaged from here".into(),
        },
    );

    at(&mut a, &mut b, 119.0);
    let item = submit(&mut a, plan(A_E, 45));
    let rec_e = decide(&mut a, item, OperatorDecision::Accepted);

    node_decides(&mut a, &mut b, 120.0, plan(SHARED_C, 43));

    // The second shared plan, decided here with nobody signed in, so its record carries no
    // role and the rule cannot rank it: a person settles this one.
    at(&mut a, &mut b, 121.0);
    let item = submit(&mut a, plan(SHARED_D, 44));
    sign_out(&mut a);
    assert!(
        a.link.is_some(),
        "signing out while cut off dropped the link that will forward the outage"
    );
    let rec_d = decide(
        &mut a,
        item,
        OperatorDecision::Rejected {
            reason: "nobody here can see track 44 well enough".into(),
        },
    );
    assert_eq!(
        (rec_d.operator_id.as_deref(), rec_d.role.as_deref()),
        (None, None)
    );

    node_decides(&mut a, &mut b, 122.0, plan(SHARED_D, 44));
    node_decides(&mut a, &mut b, 123.0, plan(NODE_M, 46));

    // Two items left waiting: one the Operator may take only by delegation, one the
    // Operator holds on its own account.
    at(&mut a, &mut b, 125.0);
    submit(&mut a, plan(A_L, 47));
    at(&mut a, &mut b, 126.0);
    submit(&mut a, plan(A_U, 50));

    // --- Clause 3: D-15's delegations lapse after `disconnected_lapse_s` -------------
    let lapses = |events: &[Envelope]| -> Vec<(MissionTime, Vec<PlanId>, Option<f64>)> {
        events
            .iter()
            .filter_map(|env| match &env.event {
                Event::Link(LinkEvent::DelegationsLapsed {
                    withdrawn,
                    lapse_s,
                    at,
                    ..
                }) => Some((*at, withdrawn.clone(), *lapse_s)),
                _ => None,
            })
            .collect()
    };
    let mut a_events: Vec<Envelope> = Vec::new();

    // One second before the interval: still the Operator's, on the queue and on PN-06.
    at(&mut a, &mut b, CUT + LAPSE_S - 1.0);
    update::tick(&mut a);
    a_events.extend(a_bus.try_iter());
    assert_eq!(failover::delegations(&a), Delegations::AsConfigured);
    assert!(lapses(&a_events).is_empty(), "a delegation lapsed early");
    assert_eq!(offered_to(&a, A_L), ["Operator"]);
    let row = |state: &AppState, plan: u128| {
        decisions::queue_rows(state)
            .into_iter()
            .find(|r| r.plan_id == PlanId(plan))
            .map(|r| (r.may_decide, r.pre_delegated))
            .expect("drawn")
    };
    assert_eq!(
        row(&a, A_L),
        (true, true),
        "before the interval the delegated item is the Operator's to decide; if it is not \
         here, its withdrawal below proves nothing"
    );

    // One second after: the delegated item has gone to the Supervisor, the Operator's own
    // case has not moved, and the lapse is on the record naming exactly what it withdrew.
    at(&mut a, &mut b, CUT + LAPSE_S + 1.0);
    update::tick(&mut a);
    a_events.extend(a_bus.try_iter());
    assert_eq!(failover::delegations(&a), Delegations::Lapsed);
    assert_eq!(
        lapses(&a_events),
        vec![(
            MissionTime(CUT + LAPSE_S + 1.0),
            vec![PlanId(A_L)],
            Some(LAPSE_S)
        )],
        "the lapse is on the record once, naming the one item it withdrew"
    );
    assert_eq!(
        offered_to(&a, A_L),
        ["Supervisor"],
        "a lapsed delegation left the item actionable by the role that lost it"
    );
    assert_eq!(
        row(&a, A_L),
        (false, false),
        "PN-06 still offers the Operator an item whose delegation lapsed"
    );
    assert_eq!(
        offered_to(&a, A_U),
        ["Operator"],
        "the lapse withdrew authority the Operator holds on its own account"
    );
    assert_eq!(row(&a, A_U), (true, false));
    // And no new delegation is exercised while cut off: the same case, proposed now, is
    // not this Operator's to take on their own account.
    //
    // **What became of it moved on 2026-09-23** (GAP-113, DN-09 §7). It used to be denied
    // by authority and dropped, which is what the desktop did with every plan the role at
    // the console could not accept; it is now offered to a role that may, and the queue
    // says which. The lapse withdraws the delegation, not the plan, and nothing here is
    // delegated: a Supervisor has to decide it, and until one does it is not actionable
    // at this console. The clause this row gates -- D-15's delegations lapsing -- is
    // asserted by the two lines above and by `row(&a, A_X)` below.
    assert_eq!(
        decisions::submit(&mut a, plan(A_X, 48)),
        decisions::Submitted::Evaluated(PolicyVerdict::RequiresHumanApproval)
    );
    assert_eq!(
        offered_to(&a, A_X),
        ["Supervisor"],
        "an Operator whose delegation has lapsed may not take this, and a Supervisor may"
    );
    assert_eq!(
        row(&a, A_X),
        (false, false),
        "a plan only a lapsed delegation could take is neither this Operator's to decide          nor delegated to them"
    );

    // --- The node answers again ----------------------------------------------------
    let offline = [&rec_a, &rec_b, &rec_c, &rec_e, &rec_d];
    let forwarded_on_node = |node: &Node| -> Vec<DecisionRecord> {
        node.records()
            .into_iter()
            .filter(|r| r.origin.is_some())
            .collect()
    };
    let engagements_on_node = node.engagements();
    assert_eq!(
        engagements_on_node, 4,
        "desktop B's four acceptances each opened an engagement on the node"
    );

    at(&mut a, &mut b, RESTORED);
    proxy.restore();
    until(
        &mut a,
        &mut b,
        "the reconciliation to be computed",
        30.0,
        |a, _| {
            matches!(
                failover::reconciliation_view(a),
                ReconciliationView::Due {
                    reconciliation: Some(Ok(_)),
                    ..
                }
            )
        },
    );
    a_events.extend(a_bus.try_iter());
    let (r, can_switch_back, forwarding_now) = reconciled(&a);

    // --- Clause 5: both acted, whatever the verdict (D-58) ---------------------------
    let both: Vec<TrackId> = r.both_acted.iter().map(|i| i.track).collect();
    assert_eq!(
        both,
        vec![TrackId(42), TrackId(43)],
        "44 and 46 were engaged on the node only and 45 here only; none of them is an \
         incident: {r:?}"
    );
    assert_eq!(
        (r.both_acted[0].local.plan, r.both_acted[0].remote.plan),
        (PlanId(A_A), PlanId(NODE_N)),
        "track 42 was engaged through two different plans, so no plan conflict could ever \
         have found it"
    );
    assert_eq!(
        (r.both_acted[1].local.plan, r.both_acted[1].remote.plan),
        (PlanId(A_B), PlanId(SHARED_C)),
    );
    let published: Vec<TrackId> = a_events
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::BothActed { track, .. }) => Some(*track),
            _ => None,
        })
        .collect();
    assert_eq!(published, vec![TrackId(42), TrackId(43)], "on the record");
    let first_verdict = a_events
        .iter()
        .position(|env| matches!(env.event, Event::Link(LinkEvent::ConflictArbitrated { .. })))
        .expect("the rule settled a conflict");
    let last_incident = a_events
        .iter()
        .rposition(|env| matches!(env.event, Event::Link(LinkEvent::BothActed { .. })))
        .expect("both-acted incidents");
    assert!(
        last_incident < first_verdict,
        "both-acted incidents must be on the record before the rule's verdicts, so no verdict \
         can be read as having settled them"
    );
    for track in [42, 43] {
        assert!(
            a.alerts
                .iter()
                .any(|m| m.contains(&format!("BOTH MAY HAVE ACTED on track {track}"))),
            "no person was alerted about track {track}: {:?}",
            a.alerts
        );
    }
    // PN-18: the incidents above everything else, and no keep button for them.
    let probe = gungnir_ui::harness::RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        gungnir_app::workspace::render_panel(ui, gungnir_workflow::PanelId::Reconciliation, &a)
    });
    let drawn = frame.joined();
    let incident = frame
        .position_of("BOTH MAY HAVE ACTED on track 42")
        .unwrap_or_else(|| panic!("PN-18 does not draw the incident: {drawn}"));
    for later in [
        "Merged:",
        "answers again",
        "resolved by the arbitration rule",
        "cannot rank",
    ] {
        let position = frame
            .position_of(later)
            .unwrap_or_else(|| panic!("PN-18 does not draw {later:?}: {drawn}"));
        assert!(
            incident < position,
            "PN-18 draws {later:?} above the both-acted incident: {drawn}"
        );
    }
    assert_eq!(
        frame
            .texts
            .iter()
            .filter(|t| t.as_str() == "keep this desktop's")
            .count(),
        1,
        "only the one conflict left to a person carries keep buttons: {drawn}"
    );

    // --- Clause 4: the rule settles what it can rank, and a person the rest (D-53) ----
    let settled_by_rule: Vec<(PlanId, bool, ArbitrationGround)> = r
        .arbitrated
        .iter()
        .map(|x| (x.conflict.plan, x.kept_local, x.ground))
        .collect();
    assert_eq!(
        settled_by_rule,
        vec![(
            PlanId(SHARED_C),
            true,
            ArbitrationGround::EarlierOnEqualRank
        )],
        "equal ranks, and this desktop's rejection at T+118 s came before the node's \
         acceptance at T+120 s -- which the node's engagement on track 43 does not undo, and \
         clause 5 raised it anyway"
    );
    assert_eq!(
        r.conflicts.iter().map(|c| c.plan).collect::<Vec<_>>(),
        vec![PlanId(SHARED_D)],
        "the conflict whose desktop side carries no role waits for a person"
    );
    assert!(!can_switch_back, "not while a person's conflict is open");
    assert!(failover::switch_back(&mut a).is_err());
    assert_eq!(
        forwarding_now,
        Forwarding::Waiting,
        "nothing goes to the node while a conflict in the outage is unsettled"
    );
    assert!(
        forwarded_on_node(&node).is_empty(),
        "the node holds part of the outage before it was settled"
    );

    // --- The person settles the last conflict, and the outage goes, whole --------------
    // The node's loop does not take it at first, so the route's window closes on the batch
    // and the link sends it again: the batch reaches the node twice (clause 2).
    node.hold_forwarding(true);
    failover::resolve_conflict(&mut a, PlanId(SHARED_D), false).expect("a person resolves it");
    assert_eq!(
        forwarding(&a),
        Forwarding::Sent { decisions: 5 },
        "the last settlement is what sends the outage"
    );
    let link = a.link.clone().expect("the link");
    until(
        &mut a,
        &mut b,
        "the link to meet an unanswered post and send the batch again",
        20.0,
        |_, _| {
            link.forwards_in_flight()
                .first()
                .is_some_and(|f| f.attempts >= 1)
        },
    );
    let batch = link.forwards_in_flight()[0].decisions.clone();
    assert_eq!(batch.len(), 5);
    node.hold_forwarding(false);
    until(
        &mut a,
        &mut b,
        "the node to answer the outage",
        20.0,
        |a, _| matches!(forwarding(a), Forwarding::Accepted { .. }),
    );

    // --- Clause 1: every offline decision on the node's record, exactly once ----------
    let on_node = forwarded_on_node(&node);
    let mut reached = 0usize;
    for record in offline {
        let copies: Vec<&DecisionRecord> = on_node.iter().filter(|r| r.id == record.id).collect();
        assert_eq!(
            copies.len(),
            1,
            "decision {} reached the node's record {} times",
            record.id,
            copies.len()
        );
        let expected = DecisionRecord {
            // GAP-141: desktop A's own name, derived from the key its certificate
            // carries, rather than the one name every desktop used to share.
            origin: Some(a_origin.clone()),
            ..record.clone()
        };
        assert_eq!(
            *copies[0], expected,
            "the node's record of a forwarded decision is not the desktop's record"
        );
        reached += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let moe_11 = reached as f64 / offline.len() as f64;
    assert!(
        (moe_11 - 1.0).abs() < f64::EPSILON,
        "MOE-11 is {moe_11}, not 1.0"
    );
    assert_eq!(
        on_node.len(),
        offline.len(),
        "the node holds a decision this desktop did not take while cut off, or one twice"
    );
    assert_eq!(
        node.engagements(),
        engagements_on_node,
        "a forwarded decision opened an engagement on the node: the desktop already acted \
         on it, and acting again is the double engagement D-58 reports"
    );
    let forwarded_events = node
        .journal()
        .iter()
        .filter(|env| {
            matches!(
                &env.event,
                Event::Command(CommandEvent::Decided { origin: Some(o), .. })
                    if *o == a_origin
            )
        })
        .count();
    assert_eq!(
        forwarded_events, 5,
        "one `Decided` per forwarded decision on the node's journal"
    );
    // The node's queue ran for B throughout, and its record says so.
    let b_decisions: Vec<PlanId> = node
        .records()
        .iter()
        .filter(|r| r.origin.is_none() && r.operator_id.as_deref() == Some("62"))
        .map(|r| r.plan.id)
        .collect();
    assert_eq!(
        b_decisions,
        vec![
            PlanId(NODE_N),
            PlanId(SHARED_C),
            PlanId(SHARED_D),
            PlanId(NODE_M)
        ]
    );

    // --- Clause 2: forwarding twice records nothing new -------------------------------
    assert_eq!(
        forwarding(&a),
        Forwarding::Accepted {
            recorded: 0,
            already_held: 5,
            settled: 0
        },
        "the answer the link received is to its second post: the first had already put \
         all five on the record, and this one recorded nothing"
    );
    // And once more, on purpose: the same batch sent again by the real client.
    let held_before = node.records().len();
    let answers_before = a
        .alerts
        .iter()
        .filter(|m| m.contains("the node holds this desktop's outage"))
        .count();
    link.queue_forward(batch);
    until(
        &mut a,
        &mut b,
        "the node to answer the batch again",
        20.0,
        |a, _| {
            a.alerts
                .iter()
                .filter(|m| m.contains("the node holds this desktop's outage"))
                .count()
                > answers_before
        },
    );
    assert_eq!(
        forwarding(&a),
        Forwarding::Accepted {
            recorded: 0,
            already_held: 5,
            settled: 0
        }
    );
    assert_eq!(
        node.records().len(),
        held_before,
        "a repeat recorded something"
    );
    assert_eq!(
        node.audit_details()
            .iter()
            .filter(|d| d.starts_with("forwarded from"))
            .count(),
        5,
        "one audit entry per forwarded decision, however many times it was sent"
    );

    // --- Clause 4, on the node: its record says what stands (MT-10 step 5) ------------
    let journal = node.journal();
    let arbitrated: Vec<(PlanId, bool, ArbitrationGround)> = journal
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::ConflictArbitrated {
                plan,
                kept_local,
                ground,
                ..
            }) => Some((*plan, *kept_local, *ground)),
            _ => None,
        })
        .collect();
    assert_eq!(
        arbitrated,
        vec![(
            PlanId(SHARED_C),
            true,
            ArbitrationGround::EarlierOnEqualRank
        )],
        "the rule's verdict is on the node's record once"
    );
    let resolved: Vec<(PlanId, bool, Option<String>)> = journal
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::ConflictResolved {
                plan,
                kept_local,
                operator,
                ..
            }) => Some((*plan, *kept_local, operator.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        resolved,
        vec![(PlanId(SHARED_D), false, None)],
        "the person's resolution is on the node's record once, naming whoever the desktop's \
         record names -- nobody signed in"
    );
    assert!(
        settlements_held(&node),
        "the settlements the node holds are the ones the desktop sent"
    );

    // --- The switch back ---------------------------------------------------------------
    failover::switch_back(&mut a).expect("every conflict settled and the node holds the outage");
    assert!(matches!(a.backend, BackendConfig::Remote { .. }));

    drop(proxy);
    drop(node);
    let _ = std::fs::remove_dir_all(a_dir);
    let _ = std::fs::remove_dir_all(b_dir);
}

/// The settlements on the node are one rule verdict and one person's resolution, keyed by
/// the plans the desktop settled.
fn settlements_held(node: &Node) -> bool {
    let held = node
        .shared
        .lock()
        .expect("the loop is running")
        .approval
        .settlements
        .clone();
    matches!(
        held.get(&PlanId(SHARED_C)),
        Some(Settlement::Rule {
            kept_local: true,
            ground: ArbitrationGround::EarlierOnEqualRank,
            ..
        })
    ) && matches!(
        held.get(&PlanId(SHARED_D)),
        Some(Settlement::Person {
            kept_local: false,
            operator: None
        })
    ) && held.len() == 2
}
// ---------------------------------------------------------------------------------
// GAP-145: the node's exchange register outlives neither a restart nor a silence
// ---------------------------------------------------------------------------------

/// GAP-145, DN-18 §12: **a desktop publishes its whole handoff set again when its link
/// comes back**, so a node holding none of it is repaired without waiting for the next
/// decision.
///
/// The register lives in the node's memory and a desktop publishes only when it issues
/// (GAP-137). Between a node restart and a console's next handoff, a partner reading
/// `GET /v3/exchange/handoffs` was served an empty deployment while the consoles held
/// engagements they believed were published -- and nothing said the list was short.
///
/// **The node forgetting is stated, not staged**: emptying this desktop's set through
/// `publish_exchange` leaves the node in the state a restart leaves it in, as far as this
/// desktop can tell, without a second node on a second port. What the test drives for
/// real is the half this change owns -- the cut, the reconnection, and what the desktop
/// does about it with no new handoff to prompt it.
#[test]
fn a_desktop_publishes_its_handoffs_again_when_its_link_comes_back() {
    let node = Node::spawn();
    let proxy = Proxy::start(node.addr);
    let (mut a, a_dir) = desktop("e", proxy.addr);
    // `until` ticks two desktops; this one only keeps it company.
    let (mut idle, idle_dir) = desktop("f", node.addr);

    sign_in(&mut a, SUPERVISOR);
    until(&mut a, &mut idle, "the desktop to link", 15.0, |a, _| {
        a.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected)
    });

    // A handoff this desktop issued, published to the node as `issue_for` does.
    a.tracking = Box::new(Picture(picture()));
    a.intercept = Box::new(StatedPlans);
    // A hostile track, which is the class this baseline's Supervisor rule names.
    let item = submit(&mut a, plan(9001, 42));
    let issued = decide(&mut a, item, OperatorDecision::Accepted)
        .id
        .to_string();
    until(
        &mut a,
        &mut idle,
        "the node to hold the handoff this desktop issued",
        15.0,
        |_, _| handoff_ids(&node).contains(&issued),
    );

    // The node forgets: what it holds for this desktop is gone, as after a restart.
    node.api
        .publish_exchange(
            ExchangeProducer::Unidentified,
            gungnir_model::ExchangeItem::Handoffs,
            Vec::new(),
        )
        .expect("the node's register was emptied");
    assert!(
        handoff_ids(&node).is_empty(),
        "the register was not emptied, so the rest of this proves nothing"
    );

    // The link goes and comes back. No decision is taken in between: the only thing that
    // can put the set back is the reconnection itself.
    proxy.cut();
    until(&mut a, &mut idle, "the link to drop", 30.0, |a, _| {
        !a.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected)
    });
    proxy.restore();
    until(
        &mut a,
        &mut idle,
        "the desktop to publish its whole set again",
        60.0,
        |_, _| handoff_ids(&node).contains(&issued),
    );
    assert_eq!(
        a.desk.approvals.records().len(),
        1,
        "the set came back because a second decision was taken, not because of the link"
    );

    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&idle_dir);
}

/// Every handoff id the node holds for exchange, whatever producer wrote it.
fn handoff_ids(node: &Node) -> Vec<String> {
    match node.api.exchange_all(gungnir_model::ExchangeItem::Handoffs) {
        Some(gungnir_api::v3::ExchangeResponse::Held { products, .. }) => {
            products.into_iter().map(|p| p.id).collect()
        }
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------
// GAP-146: a console that may not publish says so once, and holds a bounded outbox
// ---------------------------------------------------------------------------------

/// An accepted decision as the engagement path hands one to `handoffs::issue_for`, with
/// its own identifier so every handoff is a new one.
fn accepted_record(id: u128) -> DecisionRecord {
    DecisionRecord {
        id: gungnir_model::DecisionId(id),
        item: None,
        plan: plan(id, 42),
        verdict: PolicyVerdict::RequiresHumanApproval,
        decision: OperatorDecision::Accepted,
        operator_id: Some(A_OPERATOR.to_string()),
        role: Some("Operator".into()),
        request: None,
        origin: None,
        mission_time: MissionTime(100.0),
    }
}

/// PN-09 as this desktop draws it, as text.
fn pn09(state: &AppState) -> String {
    let probe = gungnir_ui::harness::RenderProbe::new();
    let mut sustainment = gungnir_app::sustainment::SustainmentState::default();
    let (_, frame) = probe.draw(|ui| {
        let mut behavior = gungnir_app::dock::PanelBehavior::new(state, &mut sustainment);
        behavior.draw_panel(ui, gungnir_workflow::PanelId::SystemHealth);
    });
    frame.joined()
}

/// **GAP-146.** A console signed in as an Operator -- the ordinary case on a watch floor --
/// issues handoffs, and the node refuses every publish `403`, because `PUBLISH_EXCHANGE` is
/// not an Operator's (DN-18 §5 amendment 2). Before this the link kept the refused batch,
/// retried it every forward tick and queued one more per handoff, for as long as the
/// console ran, and nothing said so anywhere.
///
/// Now: PN-09 says it **once**, in the node's own words and with the roles that could
/// publish; the link made **one** request for the whole run; the outbox holds **one** set,
/// the newest, with every replacement counted; and the node holds nothing. Then a
/// Supervisor signs in on the same console and the whole set reaches the node with no
/// further handoff issued.
// One console's run told in order -- refused, held, then the sign-in that changes it --
// for the reason row 8's own test gives for not splitting a story.
#[allow(clippy::too_many_lines)]
#[test]
fn an_operator_s_console_says_once_on_pn09_that_it_may_not_publish() {
    let node = Node::spawn();
    let (mut a, a_dir) = desktop("publish-refused", node.addr);
    let (mut idle, idle_dir) = desktop("publish-refused-idle", node.addr);
    sign_in(&mut a, A_OPERATOR);
    // **The desktop has seen its link come up, not only the link task** (GAP-170). The
    // tick that first reads the link as connected republishes this console's exchange
    // set (`failover::republish_exchange_on_reconnect`); the link task sets `connected`
    // on its own thread, so reading it here can find it up after a tick that read it
    // down. Leaving then made the first tick after the first handoff the one that saw
    // the link come up, which republished the set a twenty-first time and replaced 20
    // older sets rather than 19 -- once in about a hundred runs under load.
    until(
        &mut a,
        &mut idle,
        "the desktop to see its link",
        15.0,
        |a, _| {
            a.link_was_connected
                && a.link
                    .as_ref()
                    .is_some_and(gungnir_remote::link::NodeLink::connected)
        },
    );
    assert!(
        !pn09(&a).contains("Not publishing"),
        "nothing refused yet: {}",
        pn09(&a)
    );

    for n in 0..20u128 {
        gungnir_app::handoffs::issue_for(&mut a, &accepted_record(0x4600 + n));
        update::tick(&mut a);
    }
    assert_eq!(a.desk.handoffs.len(), 20);
    until(
        &mut a,
        &mut idle,
        "PN-09 to say the node refused this console",
        15.0,
        |a, _| {
            gungnir_app::exchange::exchange_line(a).is_some_and(|l| {
                l.standing == gungnir_ui::panels::sensor_health::ExchangeStanding::Refused
            })
        },
    );
    // Two seconds of frames: eight forward ticks, each of which used to post again.
    let settle = Instant::now() + Duration::from_secs(2);
    while Instant::now() < settle {
        update::tick(&mut a);
        update::tick(&mut idle);
        std::thread::sleep(Duration::from_millis(10));
    }

    let text = pn09(&a);
    assert_eq!(
        text.matches("Not publishing to coalition exchange").count(),
        1,
        "the refusal is said once on PN-09: {text}"
    );
    let roles = format!(
        "may publish ({})",
        gungnir_app::exchange::roles_that_may_publish()
    );
    for words in [
        "role Operator may not publish to exchange",
        "This console's handoffs are held here",
        roles.as_str(),
        "19 older sets were replaced",
    ] {
        assert!(text.contains(words), "{words:?} is not on PN-09: {text}");
    }
    assert!(roles.contains("Supervisor"), "{roles}");
    assert!(
        !a.alerts
            .iter()
            .any(|alert| alert.contains("coalition exchange")),
        "the refusal is PN-09's to say, not a stream of alerts: {:?}",
        a.alerts
    );
    let standing = a
        .link
        .as_ref()
        .and_then(gungnir_remote::link::NodeLink::exchange_standing)
        .expect("the link is readable");
    assert_eq!(
        standing.publishing.posts, 1,
        "one request for the whole run, not one per tick: {standing:?}"
    );
    assert_eq!(
        standing.waiting,
        vec![gungnir_model::ExchangeItem::Handoffs],
        "one set held, not one per handoff"
    );
    assert_eq!(standing.publishing.superseded, 19);
    assert!(handoff_ids(&node).is_empty(), "the node took nothing");

    // A Supervisor signs in on this console. A sign-in on a linked desktop builds a new
    // link, and its connection is the edge that publishes the whole set again.
    sign_in(&mut a, SUPERVISOR);
    until(
        &mut a,
        &mut idle,
        "the Supervisor's link to publish the whole set",
        30.0,
        |_, _| handoff_ids(&node).len() == 20,
    );
    until(
        &mut a,
        &mut idle,
        "PN-09 to stop saying the console is refused",
        15.0,
        |a, _| {
            gungnir_app::exchange::exchange_line(a).is_some_and(|l| {
                l.standing == gungnir_ui::panels::sensor_health::ExchangeStanding::Current
                    && l.text.contains("published from this console")
            })
        },
    );
    assert!(!pn09(&a).contains("Not publishing"), "{}", pn09(&a));

    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&idle_dir);
}

// ---------------------------------------------------------------------------------
// GAP-142: an outage outlives the process that fell into it
// ---------------------------------------------------------------------------------

/// GAP-142: **a desktop that restarts while cut off comes up cut off**, with the outage's
/// bounds, the decisions it took and the session its record is in.
///
/// The fallback lived in memory alone. A desktop that restarted mid-outage started with
/// no outage: what it had decided offline stayed in its own journal, never reached the
/// node's record, was never compared with the node's engagements by track, and MOE-11
/// fell below 1.0 with nothing saying so.
///
/// The restart here is a real one -- a second `AppState` over the same data directory,
/// reading the journal the first one wrote -- because what is under test is exactly what
/// a process learns from a record it did not write.
#[test]
fn an_outage_outlives_the_desktop_that_fell_into_it() {
    let node = Node::spawn();
    let proxy = Proxy::start(node.addr);
    let (mut a, a_dir) = desktop("restart", proxy.addr);
    let (mut idle, idle_dir) = desktop("restart-idle", node.addr);
    sign_in(&mut a, A_OPERATOR);
    until(&mut a, &mut idle, "the desktop to link", 15.0, |a, _| {
        a.link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected)
    });

    // Cut off, and deciding on its own queue.
    set_clock(&mut a, CUT);
    proxy.cut();
    until(
        &mut a,
        &mut idle,
        "the desktop to fall back",
        30.0,
        |a, _| a.fallback.is_some(),
    );
    a.tracking = Box::new(Picture(picture()));
    a.intercept = Box::new(StatedPlans);
    set_clock(&mut a, CUT + 1.0);
    // The plan on the record, as `update::tick` puts it there the moment the planner's
    // plan changes: a decision names a plan, and the journal has to hold the plan for a
    // decision to be read back off it whole. This harness states its plans rather than
    // solving for them (see the module documentation), so the event it would have
    // published is published here.
    a.events
        .publish(
            MissionTime(CUT + 1.0),
            Event::Intercept(gungnir_model::events::InterceptEvent::PlanProposed(plan(
                7001, 42,
            ))),
        )
        .expect("the desktop's own bus");
    let item = submit(&mut a, plan(7001, 42));
    let offline = decide(&mut a, item, OperatorDecision::Accepted);
    update::journal_pending(&mut a);
    let since = a.fallback.as_ref().map(|f| f.since).expect("in the outage");

    // The process stops here. Everything the next one knows, it reads off the journal.
    drop(a);
    let restarted = desktop_again(&a_dir, proxy.addr);
    let recovered = restarted
        .fallback
        .as_ref()
        .expect("the outage was not recovered from the journal");
    assert_eq!(
        recovered.since, since,
        "the outage's bounds did not survive"
    );
    assert!(
        recovered.endpoint.contains(&proxy.addr.to_string()),
        "the recovered outage names another node: {}",
        recovered.endpoint
    );
    assert!(
        recovered.session.is_some(),
        "the merge would read the session in progress, which is not the one the outage \
         was journaled into"
    );
    let decisions = recovered
        .recovered_decisions
        .as_ref()
        .expect("a recovered outage carries its own decisions");
    assert_eq!(
        decisions.iter().map(|d| d.decision).collect::<Vec<_>>(),
        vec![offline.id],
        "the decision taken while cut off is not in the batch the node will be sent"
    );
    assert_eq!(
        recovered.forwarding,
        failover::Forwarding::Waiting,
        "every decision was rebuilt, so the batch is owed rather than held back"
    );
    assert!(
        restarted
            .alerts
            .iter()
            .any(|a| a.contains("Recovered an unfinished outage")),
        "the strip says nothing about an outage this desktop is still in: {:?}",
        restarted.alerts
    );

    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&idle_dir);
}

/// GAP-142: **a desktop whose node never answers falls back**, rather than sitting
/// neither linked nor cut off.
///
/// `last_heard` is `None` until a snapshot lands, and a link that has never been heard is
/// not a link that has gone quiet -- so a desktop that signed in to an unreachable node
/// stayed on a remote backend for ever, with a queue it could not read and a decision
/// path it could not use. Silence is measured from the link's own start until there is
/// something later to measure it from.
#[test]
fn a_node_that_never_answers_is_silent_rather_than_pending() {
    let node = Node::spawn();
    let proxy = Proxy::start(node.addr);
    // Cut before anybody signs in: this link will never hear anything at all.
    proxy.cut();
    let (mut a, a_dir) = desktop("never", proxy.addr);
    let (mut idle, idle_dir) = desktop("never-idle", node.addr);
    sign_in(&mut a, A_OPERATOR);
    until(
        &mut a,
        &mut idle,
        "a node that never answered to be judged silent",
        HEARTBEAT_TIMEOUT.as_secs_f64() + 20.0,
        |a, _| a.fallback.is_some(),
    );
    assert!(
        a.alerts
            .iter()
            .any(|m| m.contains("has not answered since this desktop signed in")),
        "the desktop fell back without saying this node was never reached: {:?}",
        a.alerts
    );
    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&idle_dir);
}

// ---------------------------------------------------------------------------------
// GAP-150: the mission report is published again when the link comes back
// ---------------------------------------------------------------------------------

/// The coalition partner the node's agreement sends mission reports to.
const PARTNER: &str = "sector-north";

/// A node that knows the three accounts and has an agreement sending `PARTNER` this
/// deployment's mission reports (DN-18 §5).
fn api_with_a_partner() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        account(A_OPERATOR),
        account(B_OPERATOR),
        account_as(SUPERVISOR, Role::Supervisor),
    ]);
    // The same key as `api_knowing_both_operators`, so a node restarted here verifies
    // what the one before it issued, as a node with its key in custody would.
    let issuer = TokenIssuer::new(vec![8u8; 32], 10_000.0).expect("issuer");
    let api = Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_exchange(gungnir_model::ExchangeSet {
            agreements: vec![gungnir_model::ExchangeAgreement {
                party: PARTNER.into(),
                inbound: Vec::new(),
                outbound: vec![gungnir_model::ExchangeItem::Reports],
                format: gungnir_model::ExchangeFormat::Canonical,
            }],
        })
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        ))),
    );
    api.set_now(100.0);
    api
}

/// A certificate as PEM, for the file a node reads its client authority from and the
/// roots a partner trusts. Written out for the reason `backend_parity.rs`'s copy gives:
/// the only other way to PEM would be a base64 dependency for one test.
fn pem(der: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in der.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                encoded.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ascii"));
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

/// The two machine identities of the partner route: the node's serving certificate and
/// the partner's, each issued as the product issues a peer's (`issue_for_client`) and
/// each pinned by the other side, as a deployment pins its partners.
struct Partnership {
    node: Arc<tokio_rustls::rustls::sign::CertifiedKey>,
    partner: Arc<tokio_rustls::rustls::sign::CertifiedKey>,
    client_ca: std::path::PathBuf,
}

impl Partnership {
    fn new(dir: &std::path::Path) -> Self {
        let node = gungnir_remote::identity::issue_for_client("gungnir-node")
            .expect("the node's identity");
        let partner =
            gungnir_remote::identity::issue_for_client(PARTNER).expect("the partner's identity");
        std::fs::create_dir_all(dir).expect("dir");
        let client_ca = dir.join("partners.pem");
        std::fs::write(&client_ca, pem(partner.cert[0].as_ref())).expect("written");
        Self {
            node,
            partner,
            client_ca,
        }
    }
}

impl Node {
    /// Serve partners over mutual TLS beside the desktops' plain loopback, on the node's
    /// own runtime and the same transport: `gungnir_api::tls::acceptor_with_key` and
    /// `serve_on_listener`, the binary's own serving path. Dies with the node.
    fn serve_partners(&self, partnership: &Partnership) -> SocketAddr {
        let acceptor = gungnir_api::tls::acceptor_with_key(
            partnership.node.cert[0].as_ref().to_vec(),
            Arc::clone(&partnership.node.key),
            &partnership.client_ca.to_string_lossy(),
        )
        .expect("the acceptor builds");
        let api = Arc::clone(&self.api);
        self.server.block_on(async move {
            let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bound");
            let addr = tcp.local_addr().expect("addr");
            tokio::spawn(async move {
                let _ = gungnir_api::transport::serve_on_listener(
                    gungnir_api::tls::TlsListener::new(tcp, acceptor),
                    api,
                )
                .await;
            });
            addr
        })
    }
}

/// The partner's `GET /v3/exchange/reports`, over mutual TLS as `PARTNER`: the ids and
/// times it is served, or the status and body when it is not served a list.
fn partner_reads_reports(
    runtime: &tokio::runtime::Runtime,
    addr: SocketAddr,
    partnership: &Partnership,
) -> Vec<(String, MissionTime)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let tls = gungnir_remote::LinkTls {
        trust_roots_pem: vec![pem(partnership.node.cert[0].as_ref())],
        issued: Some(Arc::clone(&partnership.partner)),
        identity_pem: None,
    };
    let config = gungnir_remote::client_config(&tls).expect("the partner's client");
    let (status, body) = runtime.block_on(async move {
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let tcp = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let name = tokio_rustls::rustls::pki_types::ServerName::try_from("localhost")
            .expect("name");
        let mut stream = connector.connect(name, tcp).await.expect("handshake");
        stream
            .write_all(
                b"GET /v3/exchange/reports HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("write");
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw).await;
        let text = String::from_utf8_lossy(&raw).into_owned();
        let status: u16 = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .expect("status line");
        let body = text
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_owned())
            .unwrap_or_default();
        (status, body)
    });
    assert_eq!(status, 200, "the partner was not served: {body}");
    match serde_json::from_str::<gungnir_api::v3::ExchangeResponse>(&body) {
        Ok(gungnir_api::v3::ExchangeResponse::Held { products, .. }) => {
            products.into_iter().map(|p| (p.id, p.at)).collect()
        }
        Ok(gungnir_api::v3::ExchangeResponse::NotHeld { .. }) => Vec::new(),
        Err(err) => panic!("the partner's answer does not decode ({err}): {body}"),
    }
}

/// Tick one desktop until `check` holds, or fail with what it said.
fn until_desktop(
    state: &mut AppState,
    what: &str,
    seconds: f64,
    mut check: impl FnMut(&AppState) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        update::tick(state);
        if check(state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:?}",
            state.alerts
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn linked(state: &AppState) -> bool {
    state
        .link
        .as_ref()
        .is_some_and(gungnir_remote::link::NodeLink::connected)
}

/// **GAP-150, D-97: a console's mission report reaches its partner again when the link
/// comes back, with the time it was generated, and nobody generates it again.**
///
/// The report is generated once, at T+120, by a console whose Operator may not publish
/// (GAP-146), so the node refuses it and the partner is served nothing. PN-13 is then
/// closed -- its state dropped, so nothing on the desktop's window side holds the report
/// any more -- and two things happen that each build or meet a new link:
///
/// 1. **a sign-in**: a Supervisor signs in on the same console at T+150; the new link's
///    connection is the edge, and the partner is served the report, stamped T+120;
/// 2. **a node restart**: the node goes down and a new one, with an empty register, comes
///    up where it was (the proxy sends the desktop to it) while the console's clock
///    reads T+200; on the reconnection the new node's partner is served the report again,
///    still stamped T+120.
///
/// Before GAP-150 the report lived in PN-13's window state, which the tick cannot reach:
/// both steps served the partner nothing until somebody generated a report again. The
/// partner's read is the real route over mutual TLS, as the partner would make it; the
/// node is real in both lives; the restart is a new node process in all but the process.
// One console's run told in order, for the reason row 8's own test gives.
#[allow(clippy::too_many_lines)]
#[test]
fn a_console_s_mission_report_reaches_its_partner_again_when_its_link_comes_back() {
    let scratch = std::env::temp_dir().join(format!(
        "gungnir-gap150-{}-{}",
        std::process::id(),
        scratch_id()
    ));
    let partnership = Partnership::new(&scratch);
    let reader = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the partner's runtime");

    let first = Node::spawn_with(api_with_a_partner());
    let first_partners = first.serve_partners(&partnership);
    let proxy = Proxy::start(first.addr);
    let (mut a, a_dir) = desktop("report-republished", proxy.addr);

    // Something releasable to partners on this console's record, so the report is marked
    // for them rather than `Internal`, which a partner would be withheld.
    let mut marked = track(42, Classification::Hostile);
    marked.releasability = Releasability::AllPeers;
    let now = a.clock.now();
    a.events
        .publish(
            now,
            gungnir_eventing::Event::Tracking(
                gungnir_model::events::TrackingEvent::TrackInitiated(marked),
            ),
        )
        .expect("published");
    update::tick(&mut a);

    // An Operator's console: linked, and refused as a publisher.
    sign_in(&mut a, A_OPERATOR);
    until_desktop(&mut a, "the desktop to link", 15.0, linked);

    // PN-13 generates the report at T+120.
    set_clock(&mut a, 120.0);
    let mut pn13 = gungnir_app::sustainment::SustainmentState::default();
    pn13.reports
        .generate(&mut a)
        .expect("the journal folds into a report");
    let generated = a
        .exchange_report
        .clone()
        .expect("the report is mission state");
    assert_eq!(generated.at, MissionTime(120.0));
    assert_eq!(
        generated.releasability,
        Releasability::AllPeers,
        "the report is not marked for partners, so this test would prove nothing"
    );
    until_desktop(
        &mut a,
        "the node to refuse the Operator's publish",
        15.0,
        |a| {
            a.link
                .as_ref()
                .and_then(gungnir_remote::link::NodeLink::exchange_standing)
                .is_some_and(|s| s.publishing.refused.is_some())
        },
    );
    assert!(
        partner_reads_reports(&reader, first_partners, &partnership).is_empty(),
        "the node took a report from a console that may not publish"
    );

    // PN-13 is closed. Nothing on the window side holds the report from here on.
    drop(pn13);
    let expected = vec![(generated.id.clone(), MissionTime(120.0))];

    // 1. A sign-in builds a new link.
    set_clock(&mut a, 150.0);
    sign_in(&mut a, SUPERVISOR);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        update::tick(&mut a);
        if partner_reads_reports(&reader, first_partners, &partnership) == expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the Supervisor's link did not publish the report again: {:?}",
            a.alerts
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // 2. The node restarts: down, and a new one with an empty register where it was.
    proxy.cut();
    drop(first);
    until_desktop(&mut a, "the link to drop", 30.0, |a| !linked(a));
    let second = Node::spawn_with(api_with_a_partner());
    let second_partners = second.serve_partners(&partnership);
    assert!(
        partner_reads_reports(&reader, second_partners, &partnership).is_empty(),
        "a new node's register is not empty, so the rest of this proves nothing"
    );
    set_clock(&mut a, 200.0);
    proxy.retarget(second.addr);
    proxy.restore();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        update::tick(&mut a);
        if partner_reads_reports(&reader, second_partners, &partnership) == expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the restarted node was not given the report again: {:?}",
            a.alerts
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        a.exchange_report.as_ref().map(|r| r.at),
        Some(MissionTime(120.0)),
        "the report was generated again rather than republished"
    );

    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&scratch);
}
