// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **One scenario through both backends** (GAP-120): the `gungnir-app` backend-switching
//! row of `docs/verification-capability-table.md` §2, "same scenario through the embedded
//! and the remote backends; identical `AppState` projections; Scenario 1".
//!
//! One generated Scenario 1 timeline (`gungnir-scenario`, seed 1) is written as a
//! recorded feed and given to two desktops on the same baseline. Desktop A runs its own
//! services. Desktop B signs in to a node over mutual TLS and runs remote: its gateway
//! forwards every detection through the link to the node, the node tracks and plans, and
//! B draws what the node's stream and snapshot say. When both have drained, what their
//! panels read is compared field by field.
//!
//! # What is real here
//!
//! **Both desktops** are real `AppState`s built by `AppState::with_config`, fed by the
//! real recorded-feed adapter through their own ingest gateways, signed in through PN-20's
//! own path (`session::apply`) and ticked by the real `update::tick`.
//!
//! **The link** is the product's: B's `NodeLink`, presenting the identity B issued itself
//! at start (GAP-141, `gungnir_remote::identity::issue_desktop_identity`), against a node
//! whose own identity is issued from a key provider exactly as the node binary's
//! `spawn_tls_from_provider` issues one when the keystore is not in use (D-29), pinned in
//! B's baseline as `security.tls.trust_roots_pem`. The node pins B's self-signed
//! certificate as its client authority -- how a deployment pins its desktops -- and serves
//! through `gungnir_api::tls::acceptor_with_key` and `TlsListener`, the binary's own
//! serving path. Nothing here is signed by a test authority.
//!
//! **The node** is `gungnir-node`'s own code: its tracker, planner and gateway are built by
//! `gungnir_node::picture` -- the functions `gungnir-node/src/main.rs` calls -- its
//! approval steps are `gungnir_node::approval`'s, what it announces about its picture (the
//! track lifecycle, the plan, the health) is announced by `picture::Announcer`, and the
//! picture it publishes is `picture::publish_picture`. What is written here is the loop's *order*, the one
//! `main.rs` runs, as `desktop_projection.rs` writes it for the approval steps: a binary's
//! loop cannot be called from a test, and a node reading a wall clock could not be kept in
//! step with two replayed desktops. Not run here, because none of it reaches what a
//! linked desktop draws: the radar and peer feeds (none are configured), the sensor
//! registry's tasks and maintenance windows, the effector and warning reports, the
//! cross-session entity fold (whose identity events a desktop's link does not project),
//! and the coverage answer.
//!
//! # How both sides drain, and why no step waits on a pause
//!
//! The picture a pipeline reports is only complete once its stream has ended: the last
//! reorder horizon is processed on the flush (`TrackingService::finish`). GAP-136 was a
//! rehearsal that read the picture before its pipeline had reported, so every wait below
//! is on a condition the system itself reports, and a bounded deadline exists only to
//! turn a hang into a failure:
//!
//! 1. **Every detection has reached both trackers.** A's gateway and B's gateway have each
//!    accepted the whole feed, B's link outbox is empty, and the node's gateway has taken
//!    exactly what B's accepted.
//! 2. **Both pipelines have flushed.** Each tracker is finished, and each host is ticked
//!    until its tracker reports unhealthy -- which `LiveTrackingService::poll` does only
//!    after it has applied every snapshot the task sent, the flush included.
//! 3. **The node has said everything, and B has heard it.** The node is stepped until a
//!    step publishes nothing, and B is ticked until its link has applied the node's last
//!    envelope.
//!
//! # What is compared, and the one place exact equality is not the test
//!
//! Tracks, the retained bearings, PN-09's bearing counters, the health strip, the
//! withheld resources, the alerts raised by the scenario, and the what-if PN-05 shows for
//! a selected track are compared **exactly**. B's plan is compared exactly with the plan
//! the node holds.
//!
//! **A's plan and B's are compared by the recommendation they make, and the planners by a
//! common solve.** A plan keeps the geometry, the value and the time of the solve that
//! first made its pairing (GAP-097: "an unchanged one keeps the plan -- geometry included
//! -- exactly as it was"). Which solve that was depends on which frame first saw the
//! pipeline's report, and the pipeline runs on a task of its own on both backends, so two
//! runs of the *same* embedded desktop can hold the same pairing under different geometry.
//! That difference is the frame schedule's, not the backend's, and an exact comparison
//! of those three fields would fail or pass on the scheduler. So the pairing -- which
//! resource on which track, the thing an operator is asked to decide -- is compared
//! exactly, and the question the three fields would have answered, "does the node plan
//! as the desktop does", is asked where it has an exact answer: both backends' own
//! planners, freshly built by their own construction paths, solve the one final picture at
//! one time, and those plans must be equal in everything but their minted identifiers.
//! That is the comparison that found GAP-120's planner defect (the node's planner had no
//! local frame); the vintage comparison would have hidden it behind the scheduler.
//!
//! Plan identifiers are never compared across planners: each is a UUID v7 minted by the
//! planner that made it (D-56).

use gungnir_api::tls::{acceptor_with_key, TlsListener};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_app::state::AppState;
use gungnir_app::{session, update};
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, ResourceConfig,
    SecurityConfig, SensorConfig,
};
use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::IngestGateway;
use gungnir_intercept_service::{DpInterceptService, InterceptService, PlanOutcome};
use gungnir_model::{
    DetectionView, MissionTime, PlanView, Provenance, ResourceId, ResourceView, SensorId,
    SystemHealth, TrackId,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_node::picture;
use gungnir_scenario::{GeneratedTimeline, Scenario, ScenarioGenerator};
use gungnir_security::{hash_passphrase, Account, FileAccountStore, OperatorId, Role, TokenIssuer};
use gungnir_store::{EventJournal, FileEventJournal, SessionId};
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{LiveTrackingService, TrackingService};
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 7;
/// The seed `frame_budgets.rs` generates Scenario 1 with.
const SEED: u64 = 1;
/// The desktop's frame period: `main.rs`'s `REPAINT_INTERVAL`.
const FRAME_S: f64 = 1.0 / 30.0;
/// The per-axis variance the feed states for each position (DN-27 §4), the workspace's
/// default radar figures, as `frame_budgets.rs` states them.
const VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];
/// The node's token lifetime when the baseline names none (`gungnir-node/src/auth.rs`).
const NODE_TOKEN_LIFETIME_S: f64 = 900.0;
/// A deadlock guard on each wait, never a pacing device: every wait below leaves the
/// moment its condition holds. A minute is long enough that a loaded runner cannot fail
/// it and short enough that a real hang still does.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(60);

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-backend-parity-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

// ---------------------------------------------------------------------------------
// The scenario and the baseline both desktops and the node read
// ---------------------------------------------------------------------------------

fn timeline() -> GeneratedTimeline {
    ScenarioGenerator::new(StdRng::seed_from_u64(SEED)).generate(&Scenario::ManeuveringAircraft)
}

/// The timeline as the recorded-feed format of `docs/test-tracks/data-format.md` §3.
fn write_feed(timeline: &GeneratedTimeline, path: &Path) -> usize {
    let mut file = std::fs::File::create(path).expect("feed file");
    for o in &timeline.observations {
        let view = DetectionView {
            sensor: SensorId(o.detection.sensor_id),
            source_time: MissionTime(o.detection.timestamp_s),
            receipt_time: MissionTime(o.receipt_time_s),
            measurement: gungnir_model::Measurement::Position {
                enu: o.detection.measurement,
                variance_m2: VARIANCE_M2,
            },
            provenance: Provenance {
                source_sensor_ids: vec![o.detection.sensor_id],
                calibration_baseline_version: o.calibration.clone(),
                algorithm_version: "gungnir-scenario 0.1.0".to_owned(),
                peer: None,
                conversion_loss: None,
                authentication: gungnir_model::SourceAuthentication::default(),
            },
        };
        writeln!(
            file,
            "{}",
            serde_json::to_string(&view).expect("serializes")
        )
        .expect("feed file");
    }
    timeline.observations.len()
}

/// The one account every party knows: the desktops' account files and the node's.
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

/// The deployment: the scenario's radar, its origin -- so there is a local frame and a
/// planner can place an intercept -- and one point-defence resource at the origin.
fn baseline(timeline: &GeneratedTimeline, dir: &Path, backend: BackendConfig) -> ConfigBaseline {
    let origin = [
        timeline.origin.lat_rad,
        timeline.origin.lon_rad,
        timeline.origin.alt_m,
    ];
    let mut sensors: Vec<u32> = timeline.sensors.iter().map(|s| s.id).collect();
    sensors.sort_unstable();
    sensors.dedup();
    write_accounts(dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(origin),
        sensors: sensors
            .into_iter()
            .map(|id| SensorConfig {
                id,
                modality: "radar".to_owned(),
                position: origin,
                max_range_m: 200_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            })
            .collect(),
        resources: vec![ResourceConfig {
            handoff_endpoint: None,
            id: 1,
            position: origin,
            capacity: 1,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: Some(900.0),
        }],
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
    gungnir_config::validate(&config).expect("the baseline is valid");
    config
}

/// A desktop on `config`, fed the recorded timeline, on a replayed clock at zero.
fn desktop(config: ConfigBaseline, feed: &Path) -> AppState {
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.ingest.add_adapter(Box::new(
        RecordedFeedAdapter::open(feed).expect("the feed parses"),
    ));
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    state
}

fn set_clock(state: &mut AppState, now: MissionTime) {
    state.clock = Box::new(ReplayClockAuthority { current: now });
}

/// Sign in through PN-20's own path.
fn sign_in(state: &mut AppState) {
    let mut draft = SignInDraft {
        operator: OPERATOR.to_string(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(state, &mut draft, SessionAction::SignIn);
    assert_eq!(state.role(), Role::Supervisor, "{:?}", state.alerts);
}

// ---------------------------------------------------------------------------------
// The node
// ---------------------------------------------------------------------------------

/// A certificate as PEM, for the file the node reads its client authority from.
///
/// Written out here rather than taken from a crate: the desktop's identity exposes its
/// certificate as DER (`CertifiedKey::cert`), and the only other way to PEM would be a
/// dependency on a base64 crate for one test.
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

/// The node: its services, its loop state, and the transport serving it.
struct Node {
    /// Its own runtime, as the binary has: the transport and the pipeline task run here.
    /// Held so they live as long as the node; dropping it is the node going down.
    _runtime: tokio::runtime::Runtime,
    api: Arc<NodeApi>,
    config: ConfigBaseline,
    resources: Vec<ResourceView>,
    geo: gungnir_geo::InMemoryGeoService,
    bus: InProcessBus,
    journal_rx: Receiver<Envelope>,
    api_rx: Receiver<Envelope>,
    journal: FileEventJournal,
    gateway: IngestGateway,
    tracking: LiveTrackingService,
    intercept: DpInterceptService,
    announcer: picture::Announcer,
    approval: NodeApproval,
    /// The last sequence number offered to the transport.
    offered: u64,
}

/// What the node serves on and presents, before a desktop exists to be pinned.
struct NodeListener {
    runtime: tokio::runtime::Runtime,
    tcp: tokio::net::TcpListener,
    port: u16,
    identity: gungnir_remote::identity::HostIdentity,
}

impl NodeListener {
    /// Bind loopback and issue the node's serving identity from an ephemeral key
    /// provider over the names the binary issues it for: the bound address and
    /// `localhost` (`spawn_tls_from_provider`).
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
    /// Serve over mutual TLS, trusting exactly the desktop certificate `pinned`, and
    /// build the node's services as the binary does.
    fn start(listener: NodeListener, config: ConfigBaseline, pinned: &[u8]) -> Self {
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

        // The caller authority the binary builds (`auth::build_with_key`): the
        // baseline's account file, and the baseline's session lifetime or the node's
        // default.
        let store = FileAccountStore::open(&dir.join("accounts.json")).expect("account store");
        let issuer = TokenIssuer::new(
            vec![11u8; 32],
            config
                .security
                .authentication
                .session_lifetime_s
                .unwrap_or(NODE_TOKEN_LIFETIME_S),
        )
        .expect("issuer");
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

        let promoted = picture::promoted_baseline(&config);
        let tracking = picture::tracking_service(&config, runtime.handle(), promoted.as_ref());
        let intercept = picture::intercept_service(&config);
        let mut gateway = picture::gateway(&config);
        gateway.add_adapter(Box::new(picture::ApiSubmissionAdapter::new(Arc::clone(
            &api,
        ))));
        let bus = InProcessBus::new();
        let journal_rx = bus.subscribe();
        let api_rx = bus.subscribe();
        let journal = FileEventJournal::open(&dir).expect("the node's journal opens");
        Self {
            _runtime: runtime,
            api,
            resources: config.resource_views(),
            geo: gungnir_geo::InMemoryGeoService::new(Vec::new(), Vec::new()),
            approval: NodeApproval::new(&config),
            config,
            bus,
            journal_rx,
            api_rx,
            journal,
            gateway,
            tracking,
            intercept,
            announcer: picture::Announcer::new(),
            offered: 0,
        }
    }

    /// One tick of the node's loop at `now`, in `gungnir-node/src/main.rs`'s order.
    /// Returns how many envelopes it offered the transport.
    fn step(&mut self, now: MissionTime) -> usize {
        for event in self.gateway.tick(now, &mut self.tracking) {
            self.bus
                .publish(now, Event::Ingest(event))
                .expect("published");
        }
        self.tracking.poll(now);
        self.announcer
            .tracks(&self.bus, now, self.tracking.tracks())
            .expect("published");
        let outcome = self
            .intercept
            .plan(now, self.tracking.tracks(), &self.resources);
        let frame = Frame {
            now,
            config: &self.config,
            tracks: self.tracking.tracks(),
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
        approval::sweep(&mut self.approval, &frame);
        approval::answer_decisions(&mut self.approval, &frame).expect("answered");
        approval::answer_forwarded(&mut self.approval, &frame);
        approval::audit_refused_decisions(&mut self.approval, &frame);

        for envelope in self.journal_rx.try_iter() {
            self.journal
                .append(SessionId(1), &envelope)
                .expect("journaled");
        }
        let mut offered = 0;
        for envelope in self.api_rx.try_iter() {
            self.offered = self.offered.max(envelope.seq);
            self.api.publish_event(envelope).expect("offered");
            offered += 1;
        }

        let health = SystemHealth {
            tracking_healthy: self.tracking.is_healthy(),
            intercept_healthy: self.intercept.is_healthy(),
            ingest_healthy: self.gateway.is_healthy(),
        };
        self.announcer
            .health(&self.bus, now, health)
            .expect("published");
        self.api.set_now(now.0);
        let queue = self
            .approval
            .queue_view(&self.config, &self.resources, self.tracking.tracks());
        picture::publish_picture(
            &self.api,
            self.tracking.tracks(),
            self.tracking.bearing_rays(),
            self.tracking.pipeline_stats(),
            self.announcer.last_plan(),
            health,
            queue,
        );
        offered
    }
}

// ---------------------------------------------------------------------------------
// Waiting on what the system reports
// ---------------------------------------------------------------------------------

/// Repeat `attempt` until it reports its condition holds, failing with `what` and the
/// state it last described if [`PATIENCE`] runs out first.
///
/// `attempt` does one round of work -- a tick, a node step -- and answers `Ok` when the
/// condition holds or `Err` with what it saw. The loop leaves the moment it holds; the
/// short yield between rounds hands the network and the pipeline tasks the processor, and
/// decides nothing.
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
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The pairing a plan recommends: which resource on which track.
fn pairing(plan: &PlanView) -> BTreeSet<(ResourceId, TrackId)> {
    plan.solutions()
        .iter()
        .map(|s| (s.resource, s.track))
        .collect()
}

/// A plan with its minted identifier cleared, for comparing two planners' answers.
fn unminted(plan: &PlanView) -> PlanView {
    PlanView {
        id: gungnir_model::PlanId::default(),
        ..plan.clone()
    }
}

// ---------------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------------

#[test]
// One scenario told start to finish across three parties; splitting it would hide the
// order the drain depends on, which is the thing GAP-136 got wrong.
#[allow(clippy::too_many_lines)]
fn one_scenario_through_both_backends_leaves_both_desktops_the_same_picture() {
    let timeline = timeline();
    let feed_dir = scratch("feed");
    let feed = feed_dir.join("scenario-1.jsonl");
    let observations = write_feed(&timeline, &feed);
    assert!(observations > 100, "Scenario 1 generated {observations}");
    let end = MissionTime(
        timeline
            .observations
            .iter()
            .map(|o| o.receipt_time_s.max(o.detection.timestamp_s))
            .fold(timeline.duration_s, f64::max)
            + 1.0,
    );

    // The node's address and identity first: B's baseline pins the one and names the
    // other, and the node's client authority is B's own certificate.
    let listener = NodeListener::bind();
    let endpoint = format!("https://localhost:{}", listener.port);
    let node_pem = listener.identity.certificate_pem.clone();

    let a_dir = scratch("embedded");
    let mut a = desktop(baseline(&timeline, &a_dir, BackendConfig::Embedded), &feed);
    let b_dir = scratch("remote");
    let mut b_config = baseline(
        &timeline,
        &b_dir,
        BackendConfig::Remote {
            endpoint: endpoint.clone(),
        },
    );
    b_config.security.tls.trust_roots_pem = vec![node_pem];
    let mut b = desktop(b_config, &feed);
    let b_certificate = b
        .machine_identity
        .as_ref()
        .expect("the desktop issued its own identity (GAP-141)")
        .key
        .cert[0]
        .as_ref()
        .to_vec();

    let node_dir = scratch("node");
    let mut node = Node::start(
        listener,
        baseline(&timeline, &node_dir, BackendConfig::Embedded),
        &b_certificate,
    );

    // Both sign in as the same Supervisor; B's sign-in is what establishes its link
    // (DN-23 §5). Everything each desktop says up to the moment its backend is in place
    // is about the backend -- B's "needs an operator to sign in" and "connected" -- and
    // is set aside: what is compared is what the scenario made them say.
    sign_in(&mut a);
    sign_in(&mut b);
    assert!(
        matches!(b.backend, BackendConfig::Remote { .. }),
        "{:?}",
        b.alerts
    );
    until(
        "B's link to come up over mutual TLS and its stream to subscribe",
        || {
            node.step(MissionTime(0.0));
            update::tick(&mut b);
            let up = b
                .link
                .as_ref()
                .is_some_and(gungnir_remote::link::NodeLink::connected);
            if up && node.api.subscriber_count() >= 1 {
                return Ok(());
            }
            Err(format!(
                "link error {:?}; alerts {:?}",
                b.link
                    .as_ref()
                    .and_then(|l| l.read().and_then(|p| p.last_error.clone())),
                b.alerts
            ))
        },
    );
    let a_from = a.alerts.len();
    let b_from = b.alerts.len();

    // The scenario, frame by frame, on one clock for all three.
    let mut now = MissionTime(0.0);
    while now.0 < end.0 {
        now = MissionTime((now.0 + FRAME_S).min(end.0));
        set_clock(&mut a, now);
        set_clock(&mut b, now);
        update::tick(&mut a);
        update::tick(&mut b);
        node.step(now);
    }

    // Drain 1: every detection has reached both trackers.
    let ingested = a.ingest.stats();
    assert_eq!(
        ingested.accepted,
        u64::try_from(observations).expect("fits"),
        "A's gateway did not take the whole feed: {ingested:?}"
    );
    assert_eq!(
        b.ingest.stats(),
        ingested,
        "the two desktops' gateways disagree"
    );
    until(
        "every detection B forwarded to be taken by the node's gateway",
        || {
            node.step(end);
            update::tick(&mut b);
            let taken = node.gateway.stats();
            let outbox = b
                .link
                .as_ref()
                .map(gungnir_remote::link::NodeLink::outbox_len);
            if outbox == Some(0)
                && taken.accepted + taken.quarantined + taken.not_accepted >= ingested.accepted
            {
                return Ok(());
            }
            Err(format!("node gateway {taken:?}; B outbox {outbox:?}"))
        },
    );
    assert_eq!(
        node.gateway.stats().accepted,
        ingested.accepted,
        "the node refused detections B's gateway had accepted: {:?}",
        node.gateway.stats()
    );

    // Drain 2: both pipelines end their streams and flush; each host reads the flush.
    a.tracking.finish();
    until("A's pipeline to report its flush", || {
        update::tick(&mut a);
        if a.health.tracking_healthy {
            Err(format!("{:?}", a.tracking.pipeline_stats()))
        } else {
            Ok(())
        }
    });
    node.tracking.finish();
    until("the node's pipeline to report its flush", || {
        node.step(end);
        if node.tracking.is_healthy() {
            Err(format!("{:?}", node.tracking.pipeline_stats()))
        } else {
            Ok(())
        }
    });

    // Drain 3: the node says everything it has to say, and B hears all of it. Quiet is two
    // steps running that offer nothing: what a step publishes after its offer -- a health
    // change is published there -- is offered by the next step, so one empty step alone
    // could leave an envelope on the bus that B would never be waited for.
    let mut empty_steps = 0;
    until("the node to fall quiet", || {
        empty_steps = if node.step(end) == 0 {
            empty_steps + 1
        } else {
            0
        };
        if empty_steps >= 2 {
            Ok(())
        } else {
            Err(format!("last offered {}", node.offered))
        }
    });
    let last = node.offered;
    until("B's link to apply the node's last envelope", || {
        update::tick(&mut b);
        let heard = b
            .link
            .as_ref()
            .map(gungnir_remote::link::NodeLink::last_seq);
        if heard.is_some_and(|seq| seq >= last) {
            Ok(())
        } else {
            Err(format!("B at {heard:?}, the node at {last}"))
        }
    });
    // One more frame each, so every step of the tick reads the final picture.
    update::tick(&mut a);
    update::tick(&mut b);

    // ---- The tracks: PN-02, PN-03, PN-04 ----
    let a_tracks = a.tracking.tracks().to_vec();
    assert!(
        !a_tracks.is_empty(),
        "Scenario 1 formed no track on the embedded desktop"
    );
    assert_eq!(
        node.tracking.tracks(),
        a_tracks.as_slice(),
        "the node's tracker and the embedded tracker made different pictures of one scenario"
    );
    assert_eq!(
        b.tracking.tracks(),
        a_tracks.as_slice(),
        "the linked desktop draws a different picture from the embedded one"
    );
    assert_eq!(b.tracking.bearing_rays(), a.tracking.bearing_rays());
    assert_eq!(
        gungnir_app::sapient::bearing_pipeline_line(&b),
        gungnir_app::sapient::bearing_pipeline_line(&a),
        "PN-09's bearing counters"
    );
    assert_eq!(
        node.tracking.pipeline_stats(),
        a.tracking.pipeline_stats(),
        "the two pipelines counted one scenario differently"
    );

    // ---- The health strip and PN-09's indicators ----
    assert_eq!(
        b.health, a.health,
        "the linked desktop's health strip is not the embedded one's"
    );
    assert!(
        !b.health.tracking_healthy,
        "the node's tracker has stopped, and the linked desktop still showed it tracking"
    );

    // ---- The plan: PN-04, PN-05 ----
    assert_eq!(
        &b.last_plan,
        node.announcer.last_plan(),
        "the linked desktop's plan is not the node's"
    );
    assert!(
        !a.last_plan.solutions().is_empty(),
        "no plan was made against the scenario on the embedded desktop"
    );
    assert_eq!(
        pairing(&b.last_plan),
        pairing(&a.last_plan),
        "the two backends recommend different resources on different tracks"
    );
    assert_eq!(b.withheld_resources(), a.withheld_resources());
    // Both planners, as each backend constructs its own, solving the one final picture at
    // one time: equal in everything but the identifier each minted.
    let reference_dir = scratch("reference");
    let desktop_answer = {
        let mut reference =
            AppState::with_config(baseline(&timeline, &reference_dir, BackendConfig::Embedded))
                .expect("a reference desktop starts");
        reference
            .intercept
            .plan(end, &a_tracks, &reference.resources)
    };
    let mut node_planner = picture::intercept_service(&node.config);
    let node_answer = node_planner.plan(end, &a_tracks, &node.resources);
    match (&desktop_answer, &node_answer) {
        (PlanOutcome::Fresh(desktop_plan), PlanOutcome::Fresh(node_plan)) => {
            assert!(
                desktop_plan
                    .solutions()
                    .iter()
                    .all(|s| s.intercept_point.is_some()),
                "the scenario was meant to put the target inside the resource's reach: \
                 {desktop_plan:?}"
            );
            assert_eq!(
                unminted(node_plan),
                unminted(desktop_plan),
                "the node's planner and the embedded planner answer one picture differently"
            );
        }
        other => panic!("both planners were meant to answer fresh: {other:?}"),
    }

    // ---- The what-if PN-05 shows for a selected track ----
    let selected = a_tracks[0].id;
    a.select_track(selected);
    b.select_track(selected);
    update::tick(&mut a);
    update::tick(&mut b);
    let what_if = |state: &AppState| {
        state.what_if.as_ref().map(|course| {
            (
                unminted(&course.plan),
                course.policy_verdict,
                course.rationale.clone(),
            )
        })
    };
    assert!(what_if(&a).is_some(), "no what-if for the selected track");
    assert_eq!(what_if(&b), what_if(&a), "PN-05's what-if");

    // ---- The alerts the scenario raised ----
    assert_eq!(
        &b.alerts[b_from..],
        &a.alerts[a_from..],
        "the two desktops raised different alerts over one scenario"
    );

    // Dropped before the directories go: a journal still open on Windows makes the removal
    // fail silently.
    drop((a, b, node));
    for dir in [feed_dir, a_dir, b_dir, node_dir, reference_dir] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
