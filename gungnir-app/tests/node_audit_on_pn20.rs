// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **A node's audit record and its verification on a linked desktop's PN-20** (GAP-179,
//! D-116; `docs/design/DN-23-operator-authentication.md` §15).
//!
//! A node whose earlier run's audit segment was cut starts, verifies its record against
//! the heads its journal holds, and finds the cut (GAP-163). A desktop linked to it over
//! mutual TLS then shows that on PN-20, beside its own record:
//!
//! 1. **an Administrator and a Commander read the node's record and its verification**,
//!    each read one `audit.read` entry on the node naming the operator and the desktop's
//!    machine, and nothing else;
//! 2. **the cut shows on PN-20** -- the summary in the warning colour, the segment `CUT`,
//!    a page of the segment read back, and a verification on request finding it again;
//! 3. **an Operator is refused**: PN-20 says the role lacks `audit.read` and sends nothing,
//!    and the route itself answers the Operator's session `403` over the same mutual TLS,
//!    with exactly one entry on the node's record saying so;
//! 4. **a node that cannot be reached is said to be**, and the earlier read stays on PN-20
//!    labelled with when it was read.
//!
//! # What is real here
//!
//! The link, the transport and the TLS are the product's own, as in
//! `linked_plan_standing.rs`, whose harness this follows: the node serves through
//! `gungnir_api::tls::acceptor_with_key` with an identity issued as the binary issues one,
//! trusting exactly the desktop's certificate; the desktop is a real `AppState` signed in
//! through PN-20's own path and ticked by `update::tick`. The node's audit log, journal and
//! verification are `gungnir-node`'s: a `FileAuditLog` and a `FileEventJournal` in a
//! scratch data directory, `NodeAuditRecord` verifying at start and answering reads, and
//! the loop's steps in `main.rs`'s order.

use gungnir_api::tls::{acceptor_with_key, TlsListener};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_app::state::AppState;
use gungnir_app::{session, update, workspace};
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline, SecurityConfig,
};
use gungnir_eventing::{Envelope, EventBus, InProcessBus, Receiver};
use gungnir_model::{MissionTime, ResourceView, SessionId, SystemHealth};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_node::audit_record::{self, NodeAuditRecord};
use gungnir_node::picture;
use gungnir_security::audit::events;
use gungnir_security::{
    actions, hash_passphrase, Account, AuditEntry, AuditLog, AuditSync, FileAccountStore,
    FileAuditLog, OperatorId, Role, TokenIssuer,
};
use gungnir_store::{EventJournal, FileEventJournal};
use gungnir_time::ReplayClockAuthority;
use gungnir_ui::harness::RenderProbe;
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 21;
const COMMANDER: u64 = 24;
const ADMINISTRATOR: u64 = 25;
const NODE_TOKEN_LIFETIME_S: f64 = 900.0;
/// The node's clock for the whole test.
const NODE_NOW: MissionTime = MissionTime(5.0);
/// How many entries the node's earlier run wrote, and how many were cut from its tail.
const WRITTEN: u64 = 6;
const CUT: usize = 3;
/// A deadlock guard on each wait, never a pacing device.
const PATIENCE: Duration = Duration::from_secs(60);

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-node-audit-pn20-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write_accounts(dir: &Path) {
    let account = |operator: u64, role: Role| Account {
        operator: OperatorId(operator),
        role,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    };
    let accounts = vec![
        account(OPERATOR, Role::Operator),
        account(COMMANDER, Role::Commander),
        account(ADMINISTRATOR, Role::Administrator),
    ];
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts).expect("json"),
    )
    .expect("accounts written");
}

fn baseline(dir: &Path, backend: BackendConfig) -> ConfigBaseline {
    write_accounts(dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
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

/// The node's earlier run: `WRITTEN` entries, anchored at close as the binary anchors
/// them, and then `CUT` lines taken off the segment's tail while the node was down.
/// Returns the segment's file name.
fn an_earlier_run_then_a_cut(dir: &Path) -> String {
    let mut journal = FileEventJournal::open(dir).expect("journal");
    let bus = InProcessBus::new();
    let rx = bus.subscribe();
    let audit_dir = dir.join(gungnir_security::AUDIT_DIR);
    let mut log = FileAuditLog::open(&audit_dir, AuditSync::OnFlush, 0.0).expect("audit log");
    for i in 0..WRITTEN {
        #[allow(clippy::cast_precision_loss)]
        log.record(AuditEntry::new(
            Some(OperatorId(3)),
            actions::TASK_SENSOR,
            i as f64,
            format!("task {i}"),
        ));
    }
    log.flush().expect("synced");
    let head = log.take_closing_anchor().expect("a closing head");
    let segment = head.segment.clone();
    audit_record::publish_anchor(&bus, head, true, MissionTime(1.0));
    for envelope in rx.try_iter() {
        journal.append(SessionId(1), &envelope).expect("journaled");
    }
    journal.sync().expect("synced");
    drop(log);
    let path = audit_dir.join(&segment);
    let text = std::fs::read_to_string(&path).expect("read");
    let mut lines: Vec<&str> = text.lines().collect();
    lines.truncate(lines.len() - CUT);
    std::fs::write(&path, lines.join("\n") + "\n").expect("cut");
    segment
}

/// The node: its record and log, its journal, and its transport over mutual TLS.
struct Node {
    runtime: Option<tokio::runtime::Runtime>,
    api: Arc<NodeApi>,
    config: ConfigBaseline,
    resources: Vec<ResourceView>,
    geo: gungnir_geo::InMemoryGeoService,
    bus: InProcessBus,
    api_rx: Receiver<Envelope>,
    journal_rx: Receiver<Envelope>,
    journal: FileEventJournal,
    approval: NodeApproval,
    record: NodeAuditRecord,
    announcer: picture::Announcer,
}

/// What the node serves on and presents, before a desktop exists to be pinned.
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
    /// This run: the journal and a new audit segment opened, the record verified at
    /// start as `main.rs` verifies it, and the transport served over mutual TLS trusting
    /// exactly the desktop certificate `pinned`.
    fn start(listener: NodeListener, config: ConfigBaseline, pinned: &[u8]) -> (Self, usize) {
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
        let issuer = TokenIssuer::new(vec![13u8; 32], NODE_TOKEN_LIFETIME_S).expect("issuer");
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
        api.set_now(NODE_NOW.0);
        approval::claim_exchange_items(&api).expect("claimed");
        let serving = Arc::clone(&api);
        runtime.spawn(async move {
            let _ = serve_on_listener(TlsListener::new(tcp, acceptor), serving).await;
        });

        let bus = InProcessBus::new();
        let api_rx = bus.subscribe();
        let journal_rx = bus.subscribe();
        let journal = FileEventJournal::open(&dir).expect("journal");
        let audit_dir = dir.join(gungnir_security::AUDIT_DIR);
        let mut log =
            FileAuditLog::open(&audit_dir, AuditSync::OnFlush, NODE_NOW.0).expect("audit log");
        // `main.rs`'s order: the record subscribes, then verifies at start.
        let mut record = NodeAuditRecord::new(&audit_dir, &bus);
        let problems = record
            .verify_at_start(&journal, &mut log, &bus, NODE_NOW)
            .expect("published");
        let node = Self {
            runtime: Some(runtime),
            api,
            resources: config.resource_views(),
            geo: gungnir_geo::InMemoryGeoService::new(Vec::new(), Vec::new()),
            approval: NodeApproval::with_audit(&config, Box::new(log)),
            config,
            bus,
            api_rx,
            journal_rx,
            journal,
            record,
            announcer: picture::Announcer::new(),
        };
        (node, problems)
    }

    /// One tick of the node's loop, in `main.rs`'s order for everything here.
    fn step(&mut self) {
        let frame = Frame {
            now: NODE_NOW,
            config: &self.config,
            tracks: &[],
            resources: &self.resources,
            geofences: &self.geo,
            bus: &self.bus,
            endpoint_client: None,
            api: &self.api,
        };
        approval::sweep(&mut self.approval, &frame);
        let _ = approval::answer_decisions(&mut self.approval, &frame);
        approval::answer_forwarded(&mut self.approval, &frame);
        approval::audit_refused_decisions(&mut self.approval, &frame);
        self.record
            .answer_reads(
                self.approval.audit.as_mut(),
                &self.journal,
                &self.api,
                &self.bus,
                NODE_NOW,
            )
            .expect("published");
        approval::audit_routes(&mut self.approval, &frame);
        for envelope in self.journal_rx.try_iter() {
            self.journal
                .append(SessionId(2), &envelope)
                .expect("journaled");
        }
        for envelope in self.api_rx.try_iter() {
            self.api.publish_event(envelope).expect("offered");
        }
        self.api.set_now(NODE_NOW.0);
        let health = SystemHealth {
            tracking_healthy: true,
            intercept_healthy: true,
            ingest_healthy: true,
        };
        picture::publish_picture(
            &self.api,
            &[],
            &[],
            gungnir_tracking_service::PipelineStats::default(),
            self.announcer.last_plan(),
            self.announcer.last_standing(),
            health,
            Vec::new(),
            Vec::new(),
        );
    }

    fn entries(&self) -> Vec<AuditEntry> {
        self.approval.audit.entries().to_vec()
    }

    /// The node gone: its transport stops answering.
    fn stop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(5));
        }
    }
}

/// Step the node and tick the desktop until `check` holds.
fn until(
    what: &str,
    node: &mut Node,
    desk: &mut AppState,
    mut check: impl FnMut(&Node, &AppState) -> bool,
) {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        node.step();
        update::tick(desk);
        if check(node, desk) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}: {:?}",
            desk.alerts
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Step both until the node's record has held still for a while: whatever signing in and
/// linking wrote is on it, so what a read adds is measured alone.
fn quiet(node: &mut Node, desk: &mut AppState) -> usize {
    let mut last = node.entries().len();
    let mut still = 0;
    let deadline = std::time::Instant::now() + PATIENCE;
    while still < 40 {
        node.step();
        update::tick(desk);
        std::thread::sleep(Duration::from_millis(5));
        let now = node.entries().len();
        if now == last {
            still += 1;
        } else {
            last = now;
            still = 0;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the node's record never settled"
        );
    }
    last
}

fn sign_in(node: &mut Node, desk: &mut AppState, operator: u64, role: Role) {
    let mut draft = SignInDraft {
        operator: operator.to_string(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(desk, &mut draft, SessionAction::SignIn);
    assert_eq!(desk.role(), role, "{:?}", desk.alerts);
    until("the link to sign in", node, desk, |_, d| {
        d.link
            .as_ref()
            .is_some_and(|l| l.connected() && l.token().is_some())
    });
}

fn sign_out(desk: &mut AppState) {
    session::apply(desk, &mut SignInDraft::default(), SessionAction::SignOut);
}

fn pn20(desk: &AppState) -> gungnir_ui::harness::DrawnFrame {
    let mut sustainment = gungnir_app::sustainment::SustainmentState::default();
    RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_audit(ui, desk, &mut sustainment);
        })
        .1
}

fn reads(node: &Node) -> Vec<AuditEntry> {
    node.entries()
        .into_iter()
        .filter(|e| e.action == actions::READ_AUDIT)
        .collect()
}

/// Press one of PN-20's node-record controls and wait for the node's answer.
fn press(node: &mut Node, desk: &mut AppState, action: SessionAction) {
    session::apply(desk, &mut SignInDraft::default(), action);
    until("the node's answer", node, desk, |_, d| {
        gungnir_app::node_audit::text(d).is_some_and(|t| !t.reading)
    });
}

#[test]
// One deployment told start to finish: the cut, three roles, and the node going away.
#[allow(clippy::too_many_lines)]
fn a_cut_on_the_node_reaches_a_linked_desktop_s_pn20_for_the_roles_that_hold_audit_read() {
    let listener = NodeListener::bind();
    let endpoint = format!("https://localhost:{}", listener.port);
    let node_pem = listener.identity.certificate_pem.clone();

    let desk_dir = scratch("desktop");
    let mut config = baseline(
        &desk_dir,
        BackendConfig::Remote {
            endpoint: endpoint.clone(),
        },
    );
    config.security.tls.trust_roots_pem = vec![node_pem];
    let mut desk = AppState::with_config(config).expect("the desktop starts");
    desk.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(10.0),
    });
    let certificate = desk
        .machine_identity
        .as_ref()
        .expect("the desktop issued its own identity (GAP-141)")
        .key
        .cert[0]
        .as_ref()
        .to_vec();

    let node_dir = scratch("node");
    let node_config = baseline(&node_dir, BackendConfig::Embedded);
    let segment = an_earlier_run_then_a_cut(&node_dir);
    let (mut node, problems) = Node::start(listener, node_config, &certificate);
    assert!(problems >= 1, "the node found the cut at start");

    // Nothing is read before a person asks, and nobody signed in may not.
    let frame = pn20(&desk);
    assert!(
        frame.says(&format!("The node's audit record ({endpoint})")),
        "{}",
        frame.joined()
    );
    assert!(frame.says("Nobody is signed in"), "{}", frame.joined());

    // 1 and 2: the Administrator reads the node's record and sees the cut.
    sign_in(&mut node, &mut desk, ADMINISTRATOR, Role::Administrator);
    let before = quiet(&mut node, &mut desk);
    press(&mut node, &mut desk, SessionAction::ReadNodeAuditRecord);
    let after = quiet(&mut node, &mut desk);
    assert_eq!(
        after,
        before + 1,
        "one read, one entry: {:?}",
        node.entries()
    );
    let read = reads(&node);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].operator, Some(OperatorId(ADMINISTRATOR)));
    assert!(
        read[0].party.is_some(),
        "the desktop's machine, verified over mutual TLS, is named: {:?}",
        read[0]
    );
    assert!(
        read[0]
            .detail
            .contains("read the audit record's verification and segment list"),
        "{:?}",
        read[0]
    );
    let frame = pn20(&desk);
    for said in [
        "The node's audit record is DAMAGED (verified at start at node T+5 s",
        &format!(
            "{segment}: {} entries, CUT: {CUT} entries missing",
            WRITTEN - CUT as u64
        ),
        &format!("{CUT} entries are missing"),
        "Each read is recorded on the node's own audit record.",
    ] {
        assert!(
            frame.says(said),
            "PN-20 did not say {said:?}: {}",
            frame.joined()
        );
    }
    assert!(
        desk.alerts
            .iter()
            .any(|a| a.contains("The node's audit record is damaged")),
        "{:?}",
        desk.alerts
    );

    // A page of the cut segment, read back on the node.
    let index = gungnir_app::node_audit::text(&desk)
        .expect("a node")
        .segments
        .iter()
        .position(|(d, _, _)| d.starts_with(&segment))
        .expect("the cut segment is listed");
    let before = quiet(&mut node, &mut desk);
    press(
        &mut node,
        &mut desk,
        SessionAction::ShowNodeAuditSegment(index),
    );
    assert_eq!(
        quiet(&mut node, &mut desk),
        before + 1,
        "one page, one entry"
    );
    let frame = pn20(&desk);
    assert!(
        frame.says(&format!("{segment} on the node (read only)")),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says(&format!(
            "Entries 1 to {} of {}",
            WRITTEN - CUT as u64,
            WRITTEN - CUT as u64
        )),
        "{}",
        frame.joined()
    );
    assert!(frame.says("task 0"), "{}", frame.joined());

    // A verification on request finds the cut again: the record carries it forward.
    let mismatches = |n: &Node| {
        n.entries()
            .iter()
            .filter(|e| e.action == events::ANCHOR_MISMATCH)
            .count()
    };
    let (before, found_before) = (quiet(&mut node, &mut desk), mismatches(&node));
    press(&mut node, &mut desk, SessionAction::VerifyNodeAuditRecord);
    // The read, and the verification's own finding, as a start would write it.
    assert_eq!(quiet(&mut node, &mut desk), before + 2);
    assert_eq!(mismatches(&node), found_before + 1);
    let frame = pn20(&desk);
    assert!(
        frame.says("The node's audit record is DAMAGED (verified on request at node T+5 s"),
        "{}",
        frame.joined()
    );

    // 3: an Operator's console says it may not, and sends nothing.
    sign_out(&mut desk);
    sign_in(&mut node, &mut desk, OPERATOR, Role::Operator);
    let before = quiet(&mut node, &mut desk);
    let frame = pn20(&desk);
    assert!(
        frame.says(&format!(
            "Reading the node's audit record needs audit.read, which the Administrator and the \
             Commander hold; operator {OPERATOR} is signed in as Operator."
        )),
        "{}",
        frame.joined()
    );
    session::apply(
        &mut desk,
        &mut SignInDraft::default(),
        SessionAction::ReadNodeAuditRecord,
    );
    assert_eq!(quiet(&mut node, &mut desk), before, "nothing was sent");

    // The route itself refuses the Operator's session over the same mutual TLS, and the
    // refusal is one entry on the node's record.
    let token = desk
        .link
        .as_ref()
        .and_then(gungnir_remote::link::NodeLink::token)
        .expect("the Operator's link session");
    let pending = gungnir_remote::link::fetch_audit_record(
        &gungnir_remote::RemoteEndpoint {
            url: endpoint.clone(),
            tls: session::link_tls(&desk),
        },
        &token,
        &gungnir_remote::link::AuditQuery::default(),
        desk.runtime.handle(),
    )
    .expect("the endpoint is valid");
    let mut outcome = None;
    until("the node's refusal", &mut node, &mut desk, |_, _| {
        outcome = pending.poll();
        outcome.is_some()
    });
    match outcome {
        Some(gungnir_remote::link::AuditRecordOutcome::Refused { status, reason }) => {
            assert_eq!(status, 403);
            assert!(reason.contains("audit.read"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        quiet(&mut node, &mut desk),
        before + 1,
        "one refusal, one entry"
    );
    let refusal = node.entries().last().cloned().expect("the refusal");
    assert_eq!(refusal.action, actions::READ_AUDIT);
    assert_eq!(refusal.operator, Some(OperatorId(OPERATOR)));
    assert!(refusal.detail.starts_with("refused:"), "{refusal:?}");
    assert!(refusal.party.is_some(), "{refusal:?}");

    // 1 again: the Commander reads it.
    sign_out(&mut desk);
    sign_in(&mut node, &mut desk, COMMANDER, Role::Commander);
    let before = quiet(&mut node, &mut desk);
    press(&mut node, &mut desk, SessionAction::ReadNodeAuditRecord);
    assert_eq!(quiet(&mut node, &mut desk), before + 1);
    let last = node.entries().last().cloned().expect("the read");
    assert_eq!(last.action, actions::READ_AUDIT);
    assert_eq!(last.operator, Some(OperatorId(COMMANDER)));
    assert!(pn20(&desk).says("CUT: 3 entries missing"));

    // 4: the node goes away. The read says so, and what was read stays, labelled.
    node.stop();
    press(&mut node, &mut desk, SessionAction::ReadNodeAuditRecord);
    let frame = pn20(&desk);
    assert!(
        frame.says("The node could not be reached"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("What is shown is the node's record as it was read at T+10 s."),
        "{}",
        frame.joined()
    );
    assert!(frame.says("CUT: 3 entries missing"), "{}", frame.joined());

    drop((desk, node));
    for dir in [node_dir, desk_dir] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
