// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **A linked desktop is told what the node's planner says about its plan** (GAP-157,
//! D-94; and GAP-156's interim plan over the link, D-93; DN-04 §11).
//!
//! A node whose planner cannot refresh its plan -- here, held past its budget by a
//! stepped clock -- answers stale; after its wait, interim; and when it can solve again,
//! fresh. A desktop signed in to it over mutual TLS must draw each of those on PN-05
//! exactly as an embedded desktop draws its own: the STALE line with the plan's age on
//! **the node's** clock, the INTERIM line with the node's bound, and nothing once the
//! node's plan is current again. The desktop's clock is set a hundred seconds ahead of
//! the node's, so an age taken across the two clocks instead of through the offset the
//! desktop measures per connection would be wrong by a hundred seconds and fail here.
//!
//! # What is real here
//!
//! The link, the transport, the node's planner, announcer, approval steps and published
//! picture are the product's own, as in `backend_parity.rs`, whose harness this follows:
//! the desktop is a real `AppState` signed in through PN-20's own path and ticked by the
//! real `update::tick`; the node serves through `gungnir_api::tls::acceptor_with_key` and
//! `TlsListener` with an identity issued as the binary issues one; its planner is built by
//! `gungnir_node::picture::intercept_service` and its loop is stepped in `main.rs`'s
//! order. What is not real is the picture the node plans against, which the test states
//! rather than tracks, because what is under test is what happens to a plan after the
//! planner answers, not how the picture is formed -- `backend_parity.rs` covers that.

use gungnir_api::tls::{acceptor_with_key, TlsListener};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_app::state::{AppState, PlanStanding};
use gungnir_app::{session, update, workspace};
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, ResourceConfig,
    SecurityConfig,
};
use gungnir_eventing::{Envelope, EventBus, InProcessBus, Receiver};
use gungnir_intercept_service::{DpInterceptService, InterceptService, SteppedClock};
use gungnir_model::policy_settings::{AuthorityRule, WeaponsControlStatus};
use gungnir_model::{
    Classification, EffectorLayer, MissionTime, PlanBasis, PlanView, Provenance, Quality,
    Releasability, ResourceView, SystemHealth, TrackId, TrackStatus, TrackView,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_node::picture;
use gungnir_security::{hash_passphrase, Account, FileAccountStore, OperatorId, Role, TokenIssuer};
use gungnir_time::ReplayClockAuthority;
use gungnir_ui::harness::RenderProbe;
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use gungnir_workflow::PanelId;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 7;
const NODE_TOKEN_LIFETIME_S: f64 = 900.0;
const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0];
/// How far the desktop's clock is ahead of the node's.
const DESKTOP_AHEAD_S: f64 = 100.0;
/// A deadlock guard on each wait, never a pacing device (`backend_parity.rs`).
const PATIENCE: Duration = Duration::from_secs(60);

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-linked-standing-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write_accounts(dir: &Path) {
    let accounts = vec![Account {
        operator: OperatorId(OPERATOR),
        role: Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }];
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts).expect("json"),
    )
    .expect("accounts written");
}

fn effector(id: u32) -> ResourceConfig {
    ResourceConfig {
        handoff_endpoint: None,
        id,
        position: ORIGIN,
        capacity: 1,
        layer: "point".into(),
        cost: None,
        rounds_available: None,
        reserve: None,
        intercept_speed_mps: Some(400.0),
    }
}

/// Three point effectors, a chain that clears a point plan, and the accounts.
fn baseline(dir: &Path, backend: BackendConfig) -> ConfigBaseline {
    write_accounts(dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        resources: vec![effector(40), effector(41), effector(42)],
        backend,
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
    config
        .policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
    config.policy.authority.rules.push(AuthorityRule {
        action: gungnir_app::decisions::DECISION_ACTION.into(),
        role: "Supervisor".into(),
        layer: Some(EffectorLayer::Point),
        class: None,
        pre_delegated: false,
    });
    gungnir_config::validate(&config).expect("the baseline is valid");
    config
}

fn track(id: u64, east_m: f64) -> TrackView {
    let mut t = TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    };
    t.state[0] = east_m;
    t.state[3] = -120.0;
    t
}

/// A certificate as PEM (`backend_parity.rs` says why it is written out here).
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

/// The node: its planner on a clock the test steps, its loop state, and the transport.
struct Node {
    _runtime: tokio::runtime::Runtime,
    api: Arc<NodeApi>,
    config: ConfigBaseline,
    resources: Vec<ResourceView>,
    geo: gungnir_geo::InMemoryGeoService,
    bus: InProcessBus,
    api_rx: Receiver<Envelope>,
    intercept: DpInterceptService,
    announcer: picture::Announcer,
    approval: NodeApproval,
}

/// What the node serves on and presents, before a desktop exists to be pinned
/// (`backend_parity.rs`'s `NodeListener`).
struct NodeListener {
    runtime: tokio::runtime::Runtime,
    tcp: tokio::net::TcpListener,
    port: u16,
    identity: gungnir_remote::identity::HostIdentity,
}

impl NodeListener {
    fn bind() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("node runtime");
        let tcp = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("bound");
        let port = tcp.local_addr().expect("addr").port();
        let mut provider = gungnir_security::P256KeyProvider::new();
        let key = provider.generate(gungnir_security::KeyPurpose::TransportIdentity);
        let spki = provider.public_key_der(&key).expect("public half");
        let identity = gungnir_remote::identity::issue(
            Arc::new(provider),
            key,
            spki,
            vec!["127.0.0.1".to_owned(), "localhost".to_owned()],
            "gungnir-node",
        )
        .expect("the node's identity is issued");
        Self {
            runtime,
            tcp,
            port,
            identity,
        }
    }
}

impl Node {
    /// Serve over mutual TLS, trusting exactly the desktop certificate `pinned`, with the
    /// node's planner built by the binary's own builder and measured on `clock`.
    fn start(
        listener: NodeListener,
        config: ConfigBaseline,
        pinned: &[u8],
        clock: Arc<SteppedClock>,
    ) -> Self {
        let NodeListener {
            runtime,
            tcp,
            identity,
            ..
        } = listener;
        let dir = PathBuf::from(&config.data_dir);
        let client_ca = dir.join("client-ca.pem");
        std::fs::write(&client_ca, pem(pinned)).expect("client authority written");
        let acceptor = acceptor_with_key(
            identity.certificate_der,
            identity.key,
            &client_ca.to_string_lossy(),
        )
        .expect("the acceptor builds");
        let store = FileAccountStore::open(&dir.join("accounts.json")).expect("account store");
        let issuer = TokenIssuer::new(vec![11u8; 32], NODE_TOKEN_LIFETIME_S).expect("issuer");
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
        approval::claim_exchange_items(&api).expect("claimed");
        let serving = Arc::clone(&api);
        runtime.spawn(async move {
            let _ = serve_on_listener(TlsListener::new(tcp, acceptor), serving).await;
        });
        let bus = InProcessBus::new();
        let api_rx = bus.subscribe();
        Self {
            api,
            resources: config.resource_views(),
            geo: gungnir_geo::InMemoryGeoService::new(Vec::new(), Vec::new()),
            approval: NodeApproval::new(&config),
            // The binary's own builder, measured on the test's clock.
            intercept: picture::intercept_service(&config).with_clock(clock),
            config,
            bus,
            api_rx,
            announcer: picture::Announcer::new(),
            _runtime: runtime,
        }
    }

    /// One tick of the node's loop at `now` against `tracks`, in `main.rs`'s order for
    /// everything a linked desktop's plan reads.
    fn step(&mut self, now: MissionTime, tracks: &[TrackView]) {
        let outcome = self.intercept.plan(now, tracks, &self.resources);
        let frame = Frame {
            now,
            config: &self.config,
            tracks,
            resources: &self.resources,
            geofences: &self.geo,
            bus: &self.bus,
            endpoint_client: None,
            api: &self.api,
        };
        if let Some(plan) = self
            .announcer
            .plan(&self.bus, now, &outcome)
            .expect("published")
        {
            let _ = approval::propose(&mut self.approval, &frame, plan);
        }
        self.announcer
            .standing(&self.bus, now, &outcome)
            .expect("published");
        approval::sweep(&mut self.approval, &frame);
        let health = SystemHealth {
            tracking_healthy: true,
            intercept_healthy: self.intercept.is_healthy(),
            ingest_healthy: true,
        };
        self.announcer
            .health(&self.bus, now, health)
            .expect("published");
        for envelope in self.api_rx.try_iter() {
            self.api.publish_event(envelope).expect("offered");
        }
        self.api.set_now(now.0);
        let queue = self
            .approval
            .queue_view(&self.config, &self.resources, tracks);
        picture::publish_picture(
            &self.api,
            tracks,
            &[],
            gungnir_tracking_service::PipelineStats::default(),
            self.announcer.last_plan(),
            self.announcer.last_standing(),
            health,
            queue,
        );
    }
}

fn until(what: &str, mut attempt: impl FnMut() -> Result<(), String>) {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        let seen = match attempt() {
            Ok(()) => return,
            Err(seen) => seen,
        };
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}: {seen}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn at(state: &mut AppState, node_time: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(node_time + DESKTOP_AHEAD_S),
    });
}

fn pn05(state: &AppState) -> gungnir_ui::harness::DrawnFrame {
    RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_panel(ui, PanelId::InterceptPanel, state);
        })
        .1
}

#[test]
#[allow(clippy::too_many_lines)] // one link told start to finish
fn a_linked_desktop_draws_the_nodes_plan_as_the_node_stands_it() {
    let listener = NodeListener::bind();
    let endpoint = format!("https://localhost:{}", listener.port);
    let node_pem = listener.identity.certificate_pem.clone();

    let desk_dir = scratch("desktop");
    let mut config = baseline(&desk_dir, BackendConfig::Remote { endpoint });
    config.security.tls.trust_roots_pem = vec![node_pem];
    let mut desk = AppState::with_config(config).expect("the desktop starts");
    let certificate = desk
        .machine_identity
        .as_ref()
        .expect("the desktop issued its own identity (GAP-141)")
        .key
        .cert[0]
        .as_ref()
        .to_vec();

    let node_dir = scratch("node");
    let clock = Arc::new(SteppedClock::new(Duration::ZERO));
    let mut node = Node::start(
        listener,
        baseline(&node_dir, BackendConfig::Embedded),
        &certificate,
        clock.clone(),
    );

    let three = [
        track(70, 30_000.0),
        track(71, 20_000.0),
        track(72, 10_000.0),
    ];
    let four = [
        track(70, 30_000.0),
        track(71, 20_000.0),
        track(72, 10_000.0),
        track(73, 40_000.0),
    ];

    // Sign in; the link comes up against the node's clock at 0, read on this desktop's
    // clock at 100 -- the offset the desktop measures for this connection.
    node.step(MissionTime(0.0), &[]);
    at(&mut desk, 0.0);
    let mut draft = SignInDraft {
        operator: OPERATOR.to_string(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(&mut desk, &mut draft, SessionAction::SignIn);
    assert_eq!(desk.role(), Role::Supervisor, "{:?}", desk.alerts);
    until("the link to come up and subscribe", || {
        update::tick(&mut desk);
        let up = desk
            .link
            .as_ref()
            .is_some_and(gungnir_remote::link::NodeLink::connected);
        if up && node.api.subscriber_count() >= 1 {
            Ok(())
        } else {
            Err(format!("alerts {:?}", desk.alerts))
        }
    });

    // Node t = 1: the node's planner answers; the desktop draws it current.
    node.step(MissionTime(1.0), &three);
    let first = node.announcer.last_plan().clone();
    assert!(
        !first.is_empty(),
        "three effectors and three tracks planned nothing"
    );
    at(&mut desk, 1.0);
    until("the node's first plan, current", || {
        update::tick(&mut desk);
        if desk.last_plan == first && desk.plan_standing == PlanStanding::Current {
            Ok(())
        } else {
            Err(format!("{:?} {:?}", desk.plan_standing, desk.last_plan.id))
        }
    });
    assert!(!pn05(&desk).says("STALE"));

    // Node t = 2: a fourth track, and the node's solve cannot advance. The node answers
    // stale; the desktop draws the node's last good plan under the STALE line, 1.5 s old
    // at node t = 3.5 -- on the node's clock, through the offset.
    clock.set_step(Duration::from_millis(10));
    node.step(MissionTime(2.0), &four);
    at(&mut desk, 2.3);
    until("the node's stale standing", || {
        node.step(MissionTime(2.0), &four);
        update::tick(&mut desk);
        match &desk.plan_standing {
            PlanStanding::Stale { .. } => Ok(()),
            other => Err(format!("{other:?}")),
        }
    });
    match &desk.plan_standing {
        PlanStanding::Stale {
            computed_at,
            asked_at,
            reason,
        } => {
            assert!(
                (asked_at.0 - computed_at.0 - 1.3).abs() < 1e-9,
                "the plan the node computed at its t = 1 is 1.3 s old at its t = 2.3, not \
                 {} s: computed {computed_at:?}, asked {asked_at:?}",
                asked_at.0 - computed_at.0
            );
            assert!(reason.starts_with("on the node, "), "{reason}");
            assert!(reason.contains("4 track(s)"), "{reason}");
            assert!(reason.contains("4 ms budget"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        desk.last_plan, first,
        "the node's last good plan is still drawn"
    );
    assert!(
        !desk.health().intercept_healthy,
        "the node's planner is behind and the strip said otherwise"
    );
    let panel = pn05(&desk);
    assert!(
        panel.says("STALE: this plan was computed at t = 101.0 s, 1.3 s before"),
        "{}",
        panel.joined()
    );
    assert!(panel.says("on the node"), "{}", panel.joined());

    // Node t = 2.6: past the node's wait, its one-step answer stands in. The desktop draws
    // the interim plan under the INTERIM line with the node's bound, and the node's queue
    // item for it says INTERIM on this desktop's PN-06.
    node.step(MissionTime(2.6), &four);
    let interim = node.announcer.last_plan().clone();
    assert_eq!(interim.basis, PlanBasis::OneStep);
    at(&mut desk, 2.7);
    until("the node's interim plan", || {
        update::tick(&mut desk);
        if desk.last_plan == interim && matches!(desk.plan_standing, PlanStanding::Interim { .. }) {
            Ok(())
        } else {
            Err(format!("{:?} {:?}", desk.plan_standing, desk.last_plan.id))
        }
    });
    match &desk.plan_standing {
        PlanStanding::Interim { share, reason } => {
            assert_eq!(share, "worth at least 100% of the best plan's value");
            assert!(reason.starts_with("on the node, "), "{reason}");
            assert!(reason.contains("has not finished 500 ms after"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    let panel = pn05(&desk);
    assert!(panel.says("INTERIM: this plan"), "{}", panel.joined());
    until("the node's queue item for the interim plan", || {
        update::tick(&mut desk);
        let rows = gungnir_app::projection::queue_rows(&desk);
        match rows.iter().find(|r| r.plan_id == interim.id) {
            Some(row) if row.basis == PlanBasis::OneStep => Ok(()),
            Some(row) => Err(format!("the item's basis read {:?}", row.basis)),
            None => Err(format!("{} rows", rows.len())),
        }
    });

    // Node t = 3: the node can solve again. The optimum, current; nothing labelled.
    clock.set_step(Duration::ZERO);
    node.step(MissionTime(3.0), &four);
    let exact: PlanView = node.announcer.last_plan().clone();
    assert_eq!(exact.basis, PlanBasis::Exact);
    at(&mut desk, 3.0);
    until("the node's optimum, current", || {
        update::tick(&mut desk);
        if desk.last_plan == exact && desk.plan_standing == PlanStanding::Current {
            Ok(())
        } else {
            Err(format!("{:?} {:?}", desk.plan_standing, desk.last_plan.id))
        }
    });
    let panel = pn05(&desk);
    assert!(
        !panel.says("STALE") && !panel.says("INTERIM"),
        "{}",
        panel.joined()
    );

    drop((desk, node));
    for dir in [node_dir, desk_dir] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
