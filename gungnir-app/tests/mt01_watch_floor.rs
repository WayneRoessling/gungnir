// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **DN-31 §9 row 9: MT-01 with a watch floor** (GAP-133, D-55).
//!
//! MT-01 step 6 is the supervisor managing the queue, and its failure is *a queue that
//! outruns the authority*. This replays that against one node with three consoles --
//! two Operators and a Supervisor -- and asserts the four clauses of the criterion:
//!
//! 1. every queued item ends decided once, expired, or escalated;
//! 2. no track is engaged twice by the same shift;
//! 3. every escalation is visible to the Supervisor;
//! 4. every decision is on the node's record.
//!
//! # Where the saturation comes from
//!
//! TT-01's committed fixture, `testdata/tracks/samples/TT-01-sample/truth.jsonl`: its
//! twelve entities, in the order they first appear, one track and one plan each. The set
//! and its arrival order are the scenario's, not this test's -- what the test supplies is
//! the *watch*, which is what these clauses are about. The tracker's own job of turning
//! TT-01's 1,408 detections into those entities is `gungnir-app/tests/laydown_rehearsal.rs`'s
//! and is not repeated here.
//!
//! **All twelve are proposed at once**, which is the saturation: the node's queue is
//! immediately longer than one Operator can work, so some items escalate on the node's
//! clock and some windows close with nobody deciding. A test that fed them in slowly
//! would be testing a calm queue.
//!
//! # The node's clock is the wall clock, and why that matters here
//!
//! A deadline is a mission time, and a desktop draws the time remaining on a node's item
//! as that deadline minus **its own** clock. In a real deployment both are
//! `WallClockAuthority`, which is seconds since the Unix epoch, so the two agree. This
//! harness therefore drives the node's mission time from the same wall clock rather than
//! from a counter starting at zero: a node on a private clock would make every countdown
//! on every console meaningless, and the test would not be replaying anything real.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_app::state::AppState;
use gungnir_app::{projection, session, update};
use gungnir_command::{ApprovalWorkflow, OperatorDecision};
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
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR_A: u64 = 31;
const OPERATOR_B: u64 = 32;
const SUPERVISOR: u64 = 33;

/// Seconds after submission at which an item is offered one rank higher.
const ESCALATE_AFTER_S: f64 = 1.5;
/// Seconds after submission at which a window closes.
const EXPIRY_S: f64 = 5.0;

/// Seconds since the Unix epoch, which is what `WallClockAuthority` calls mission time.
fn wall_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ---------------------------------------------------------------------------------
// TT-01's entities
// ---------------------------------------------------------------------------------

/// The scenario's entities, in the order they first appear in its truth file.
///
/// Read from the committed fixture rather than listed here, so this test replays TT-01
/// and does not merely claim to: a change to the scenario changes what is replayed.
fn tt01_entities() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/tracks/samples/TT-01-sample/truth.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("TT-01's truth file is committed at {}: {e}", path.display()));
    let mut first_seen: BTreeMap<String, f64> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).expect("a truth row is json");
        let (Some(entity), Some(t)) = (row["entity"].as_str(), row["t"].as_f64()) else {
            continue;
        };
        let at = first_seen.entry(entity.to_owned()).or_insert(t);
        *at = at.min(t);
    }
    let mut ordered: Vec<(String, f64)> = first_seen.into_iter().collect();
    ordered.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let entities: Vec<String> = ordered.into_iter().map(|(name, _)| name).collect();
    assert!(
        entities.len() >= 8,
        "TT-01 declares twelve entities; {} were read, so the fixture or this reader has \
         changed and the saturation is no longer the scenario's",
        entities.len()
    );
    entities
}

// ---------------------------------------------------------------------------------
// The baseline
// ---------------------------------------------------------------------------------

fn effector(id: u32) -> ResourceConfig {
    ResourceConfig {
        handoff_endpoint: None,
        id,
        position: [0.0, 0.0, 0.0],
        // Capacity for the whole raid, so nothing is refused for a reason this row is
        // not about.
        capacity: 32,
        layer: "point".into(),
        cost: None,
        rounds_available: None,
        reserve: None,
        intercept_speed_mps: Some(400.0),
    }
}

/// The deployment the watch works under.
///
/// A point layer only, and an Operator holds it, so every item starts as an Operator's
/// and every escalation adds the Supervisor -- which is what clause 3 is about.
fn baseline(dir: &std::path::Path) -> ConfigBaseline {
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![effector(0)],
        ..ConfigBaseline::default()
    };
    config
        .policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 60.0);
    config.policy.authority.rules = vec![
        AuthorityRule {
            action: actions::DECIDE_PLAN.into(),
            role: "Operator".into(),
            layer: Some(EffectorLayer::Point),
            class: Some("hostile".into()),
            pre_delegated: false,
        },
        AuthorityRule {
            action: actions::DECIDE_PLAN.into(),
            role: "Supervisor".into(),
            layer: Some(EffectorLayer::Point),
            class: Some("hostile".into()),
            pre_delegated: false,
        },
    ];
    config
        .policy
        .decisions
        .escalate_after_s
        .insert(EffectorLayer::Point, ESCALATE_AFTER_S);
    config
        .policy
        .decisions
        .expiry_s
        .insert(EffectorLayer::Point, EXPIRY_S);
    gungnir_config::validate(&config).expect("the baseline is valid");
    config
}

fn track(id: u64, at: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(at),
        releasability: Releasability::default(),
    }
}

fn plan(id: u128, track: u64, at: f64) -> PlanView {
    PlanView {
        id: PlanId(id),
        mission_time: MissionTime(at),
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
    shared: Arc<Mutex<Shared>>,
    config: Arc<ConfigBaseline>,
    resources: Arc<Vec<ResourceView>>,
    geo: Arc<InMemoryGeoService>,
    bus: Arc<InProcessBus>,
    /// A third subscriber to the node's own bus, beside the journal's and the
    /// transport's, so the test can read what the node published.
    ///
    /// **Clause 3's set of escalations has to come from here and not from the queue.**
    /// An escalation is not an ending: an item that is offered upward and then expires
    /// leaves the queue with nothing on it to say it was ever escalated, so counting
    /// escalations by looking at what is still waiting undercounts every one that ran
    /// out of time -- and under saturation that is most of them. This read that way
    /// first and reported zero escalations on a run that had several.
    events: Receiver<Envelope>,
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
    fn spawn(api: &Arc<NodeApi>) -> Self {
        let dir = std::env::temp_dir().join(format!("gungnir-mt01-node-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Arc::new(baseline(&dir));
        let resources = Arc::new(config.resource_views());
        let geo = Arc::new(InMemoryGeoService::new(Vec::new(), Vec::new()));
        let bus = Arc::new(InProcessBus::new());
        let api_rx: Receiver<Envelope> = bus.subscribe();
        let events: Receiver<Envelope> = bus.subscribe();
        let shared = Arc::new(Mutex::new(Shared {
            approval: NodeApproval::new(&config),
            now: MissionTime(wall_now()),
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
                        // The node's clock is the wall clock, as the binary's is, so its
                        // deadlines mean the same thing on every console.
                        state.now = MissionTime(wall_now());
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
                        };
                        approval::sweep(&mut state.approval, &frame);
                        let _ = approval::answer_decisions(&mut state.approval, &frame, &api);
                        approval::answer_forwarded(&mut state.approval, &frame, &api);
                        approval::audit_refused_decisions(&mut state.approval, &frame, &api);
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
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
        Self {
            shared,
            config,
            resources,
            geo,
            bus,
            events,
            dir,
            running,
        }
    }

    /// Propose the whole raid in one tick: the saturation MT-01 step 6 is about.
    fn propose_all(&self, tracks: &[TrackView], plans: Vec<PlanView>) -> Vec<PendingApprovalId> {
        let mut state = self.shared.lock().expect("the loop is running");
        state.tracks = tracks.to_vec();
        let mut queued = Vec::new();
        for plan in plans {
            let before: BTreeSet<PendingApprovalId> = state
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
            };
            approval::propose(&mut state.approval, &frame, plan);
            if let Some(id) = state
                .approval
                .desk
                .approvals
                .queue()
                .iter()
                .map(|p| p.id)
                .find(|id| !before.contains(id))
            {
                queued.push(id);
            }
        }
        queued
    }

    /// The plans the node has escalated since the last call, and to which role.
    ///
    /// Drained from the node's own bus: `CommandEvent::Escalated` is the node saying it
    /// has asked a higher role, and it is the only record of an escalation that survives
    /// the item expiring afterwards.
    fn escalations(&self) -> Vec<(PlanId, String)> {
        self.events
            .try_iter()
            .filter_map(|envelope| match envelope.event {
                gungnir_eventing::Event::Command(
                    gungnir_model::events::CommandEvent::Escalated { plan, to_role, .. },
                ) => Some((plan, to_role)),
                _ => None,
            })
            .collect()
    }

    fn with_state<T>(&self, f: impl FnOnce(&Shared) -> T) -> T {
        f(&self.shared.lock().expect("the loop is running"))
    }
}

fn account(operator: u64, role: Role) -> Account {
    Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

fn accounts() -> Vec<Account> {
    vec![
        account(OPERATOR_A, Role::Operator),
        account(OPERATOR_B, Role::Operator),
        account(SUPERVISOR, Role::Supervisor),
    ]
}

fn api_knowing_the_watch() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(accounts());
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
    api.set_now(wall_now());
    api
}

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
// The consoles
// ---------------------------------------------------------------------------------

fn desktop(name: &str, endpoint: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-mt01-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts()).expect("json"),
    )
    .expect("written");
    let mut config = baseline(&dir);
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

// ---------------------------------------------------------------------------------
// Row 9
// ---------------------------------------------------------------------------------

/// **DN-31 §9 row 9.** MT-01's saturation with two Operators and a Supervisor on three
/// desktops against one node.
#[test]
#[allow(clippy::too_many_lines)]
fn a_saturated_node_queue_worked_by_three_consoles_ends_every_item_once() {
    let entities = tt01_entities();
    let addr: std::net::SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    let api = api_knowing_the_watch();
    let _runtime = serve(addr, Arc::clone(&api));
    let node = Node::spawn(&api);
    let endpoint = format!("http://{addr}");

    let (mut a, dir_a) = desktop("a", &endpoint);
    let (mut b, dir_b) = desktop("b", &endpoint);
    let (mut sup, dir_sup) = desktop("sup", &endpoint);
    sign_in(&mut a, OPERATOR_A);
    sign_in(&mut b, OPERATOR_B);
    sign_in(&mut sup, SUPERVISOR);

    // All three subscribed before anything is queued; an envelope published into the
    // window between connecting and subscribing reaches nobody and is not replayed.
    let deadline = Instant::now() + std::time::Duration::from_secs(20);
    while api.subscriber_count() < 3 {
        update::tick(&mut a);
        update::tick(&mut b);
        update::tick(&mut sup);
        assert!(
            Instant::now() < deadline,
            "the three streams did not subscribe"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // The raid: one track and one plan per TT-01 entity, all queued at once.
    let at = wall_now();
    let tracks: Vec<TrackView> = (0..entities.len()).map(|i| track(i as u64, at)).collect();
    let plans: Vec<PlanView> = (0..entities.len())
        .map(|i| plan(i as u128 + 1, i as u64, at))
        .collect();
    let queued = node.propose_all(&tracks, plans);
    assert_eq!(
        queued.len(),
        entities.len(),
        "every one of TT-01's entities should have reached the queue"
    );

    // The watch works the queue for the length of one expiry window plus a margin, so
    // that what is not decided escalates and then expires on the node's own clock.
    //
    // **The Operators deliberately cannot keep up**, which is MT-01 step 6's whole
    // point: each takes at most a third of the raid, the Supervisor takes what escalates
    // up to a cap, and the rest runs out of time.
    let operator_cap = entities.len() / 3;
    let supervisor_cap = 2usize;
    let mut taken_a = 0usize;
    let mut taken_b = 0usize;
    let mut taken_sup = 0usize;
    // Every item a console posted a decision for, so clause 4 can ask the node's record
    // about each one by name rather than only about totals.
    let mut posted: Vec<PendingApprovalId> = Vec::new();
    let mut escalations_seen_by_the_supervisor: BTreeSet<PlanId> = BTreeSet::new();
    let mut escalated_on_the_node: BTreeMap<PlanId, String> = BTreeMap::new();
    let run_until = Instant::now() + std::time::Duration::from_secs_f64(EXPIRY_S + 4.0);
    while Instant::now() < run_until {
        update::tick(&mut a);
        update::tick(&mut b);
        update::tick(&mut sup);

        // What the node says it escalated, drained as it says it: an escalation that is
        // followed by an expiry leaves nothing on the queue to find afterwards.
        for (plan, to_role) in node.escalations() {
            // **The first rung, kept.** An item nobody takes climbs the whole ladder,
            // one rank per `escalate_after_s` (DN-10 §5), so under saturation the last
            // role a plan was offered to is the top of it. What clause 3 is about is
            // that the *next* role up was asked, which is the first escalation.
            escalated_on_the_node.entry(plan).or_insert(to_role);
        }
        // And what the Supervisor can see of them, recorded as that console sees it:
        // clause 3 is about what reaches the higher role, not about what the node holds.
        for row in projection::queue_rows(&sup) {
            if row.escalated_from.is_some() && row.may_decide {
                escalations_seen_by_the_supervisor.insert(row.plan_id);
            }
        }

        for (state, taken, cap) in [
            (&mut a, &mut taken_a, operator_cap),
            (&mut b, &mut taken_b, operator_cap),
            (&mut sup, &mut taken_sup, supervisor_cap),
        ] {
            if *taken >= cap || !state.projection.in_flight.is_empty() {
                continue;
            }
            let next = projection::queue_rows(state)
                .into_iter()
                .find(|row| row.may_decide)
                .map(|row| row.id);
            if let Some(PendingId(item)) = next {
                if projection::decide(
                    state,
                    PendingApprovalId(item),
                    gungnir_remote::queue::DecisionChoice::Accept,
                )
                .is_ok()
                {
                    *taken += 1;
                    posted.push(PendingApprovalId(item));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Let the last posts settle and the last windows close.
    let settle = Instant::now() + std::time::Duration::from_secs(3);
    while Instant::now() < settle {
        update::tick(&mut a);
        update::tick(&mut b);
        update::tick(&mut sup);
        for (plan, to_role) in node.escalations() {
            // **The first rung, kept.** An item nobody takes climbs the whole ladder,
            // one rank per `escalate_after_s` (DN-10 §5), so under saturation the last
            // role a plan was offered to is the top of it. What clause 3 is about is
            // that the *next* role up was asked, which is the first escalation.
            escalated_on_the_node.entry(plan).or_insert(to_role);
        }
        for row in projection::queue_rows(&sup) {
            if row.escalated_from.is_some() && row.may_decide {
                escalations_seen_by_the_supervisor.insert(row.plan_id);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // ---- clause 1: every queued item ends decided once, expired, or escalated ----
    let (records, still_waiting, engaged) = node.with_state(|s| {
        let records = s.approval.desk.approvals.records().to_vec();
        let waiting: Vec<(PendingApprovalId, PlanId, usize)> = s
            .approval
            .desk
            .approvals
            .queue()
            .iter()
            .map(|p| (p.id, p.plan.id, p.offered_to.len()))
            .collect();
        let engaged: Vec<TrackId> = s
            .approval
            .desk
            .engagements
            .iter()
            .map(|e| e.track)
            .collect();
        (records, waiting, engaged)
    });

    let mut ended_once: BTreeMap<PlanId, usize> = BTreeMap::new();
    for record in &records {
        *ended_once.entry(record.plan.id).or_default() += 1;
    }
    for (plan, count) in &ended_once {
        assert_eq!(
            *count, 1,
            "plan {plan} ended {count} times; one queue means one ending"
        );
    }
    let waiting_plans: BTreeSet<PlanId> = still_waiting.iter().map(|(_, plan, _)| *plan).collect();
    for (i, _) in entities.iter().enumerate() {
        let plan = PlanId(i as u128 + 1);
        let ended = ended_once.contains_key(&plan);
        let escalated = escalated_on_the_node.contains_key(&plan);
        assert!(
            ended || escalated,
            "plan {plan} neither ended nor escalated: it is sitting on the queue with \
             nobody asked above the role that was first offered it, which is the queue \
             outrunning the authority MT-01 step 6 exists to catch (waiting: {}, records: {})",
            waiting_plans.contains(&plan),
            ended_once.len()
        );
    }
    // The saturation was real: some were decided, and some ran out of time.
    let decided: Vec<_> = records.iter().filter(|r| r.is_actionable()).collect();
    let expired: Vec<_> = records
        .iter()
        .filter(|r| matches!(r.decision, OperatorDecision::Expired { .. }))
        .collect();
    assert!(
        !decided.is_empty(),
        "the watch decided nothing, so nothing about a saturated queue was exercised"
    );
    assert!(
        !expired.is_empty(),
        "nothing expired, so the queue never outran the authority and the saturation was \
         not saturation"
    );
    println!(
        "MT-01 saturation: {} items queued, {} decided, {} expired, {} still waiting, \
         {} escalated",
        queued.len(),
        decided.len(),
        expired.len(),
        still_waiting.len(),
        escalated_on_the_node.len()
    );

    // ---- clause 2: no track is engaged twice by the same shift ----
    let mut seen: BTreeSet<TrackId> = BTreeSet::new();
    for track in &engaged {
        assert!(
            seen.insert(*track),
            "track {track:?} was engaged twice by one shift"
        );
    }
    assert_eq!(
        engaged.len(),
        decided.len(),
        "one engagement per actionable decision, and no more"
    );

    // ---- clause 3: every escalation is visible to the Supervisor ----
    assert!(
        !escalated_on_the_node.is_empty(),
        "nothing escalated, so clause 3 would pass by having nothing to check"
    );
    for (plan, to_role) in &escalated_on_the_node {
        assert_eq!(
            to_role, "Supervisor",
            "an item first offered to an Operator escalates to the Supervisor"
        );
        assert!(
            escalations_seen_by_the_supervisor.contains(plan),
            "plan {plan} escalated on the node and never appeared as escalated on the \
             Supervisor's PN-06; seen: {escalations_seen_by_the_supervisor:?}"
        );
    }

    // ---- clause 4: every decision is on the node's record ----
    //
    // **Every item a console decided is on the record, and nothing else is.** The record
    // holds exactly one entry per queued item -- asserted above -- so the posts the node
    // refused recorded nothing: under saturation several arrive after their window has
    // closed and are answered `409 Expired`, which is the route working, not a decision
    // going missing.
    let posted_by_the_watch = taken_a + taken_b + taken_sup;
    assert_eq!(
        records.len(),
        queued.len(),
        "one record per queued item and no more: {} records for {} items",
        records.len(),
        queued.len()
    );
    for item in &posted {
        assert!(
            records.iter().any(|r| r.item == Some(*item)),
            "console posted a decision on item {item} and the node's record has no entry \
             for it at all"
        );
    }
    assert!(
        decided.len() <= posted_by_the_watch,
        "the node recorded {} decisions and the watch asked for {posted_by_the_watch}",
        decided.len()
    );
    for record in &decided {
        // **Named, both of them.** Only a decision taken with nobody signed in records
        // neither (DN-23 §5 rule 1, D-53), and the node's route needs a token, so a
        // decision on this record naming nobody would be one nobody authorized.
        assert!(
            record
                .operator_id
                .as_deref()
                .is_some_and(|id| id.parse::<u64>().is_ok()),
            "a decision on the node names the operator whose token took it, and this one \
             says {:?}",
            record.operator_id
        );
        assert!(
            matches!(record.role.as_deref(), Some("Operator" | "Supervisor")),
            "a decision on the node carries the role its token held, and this one says \
             {:?}",
            record.role
        );
    }
    // And no console recorded a decision of its own.
    for (name, console) in [("a", &a), ("b", &b), ("sup", &sup)] {
        assert!(
            console.desk.approvals.records().is_empty(),
            "console {name} recorded a decision on a node plan"
        );
        assert_eq!(
            console.desk.engagements.len(),
            0,
            "console {name} opened an engagement for a node plan"
        );
    }

    let _ = std::fs::remove_dir_all(dir_a);
    let _ = std::fs::remove_dir_all(dir_b);
    let _ = std::fs::remove_dir_all(dir_sup);
}
