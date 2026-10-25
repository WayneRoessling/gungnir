// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **A linked desktop's gap acceptances are the node's** (GAP-194, D-131, D-134).
//!
//! The node is the real one as far as acceptances go: `gungnir_node::gap_acceptance::tick`
//! judging what the real `gungnir-api` route hands over, against a coverage answer the
//! loop publishes with its marks, and the bus drained into `NodeApi::publish_event` as
//! `gungnir-node/src/main.rs` drains it. What is stated rather than computed is the
//! coverage answer itself: one gap on the "east" approach, because the node's coverage is
//! `gungnir-analytics`' business and tested there. The desktop is a real `AppState` on
//! `BackendConfig::Remote`, signed in as a commander and ticked by the real `update::tick`.
//!
//! - **Linked**, PN-11 lists the node's gap; accepting it sends it to the node, which
//!   records it, and the desktop's list carries the node's mark. A gap the node cannot
//!   see is refused, and the refusal is said.
//! - **Cut off** (the node not yet serving), the desktop records the acceptance itself and
//!   forwards it when the node answers: one the node holds stands, named as the commander
//!   who accepted it; one it refuses re-opens on the desktop with the node's reason,
//!   journaled.

use gungnir_analytics::{
    accepted_gap, CoverageGap, CoverageParameters, CoverageReport, GapSeverity,
};
use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{ComputedCoverage, CoverageResponse, SnapshotResponse};
use gungnir_app::gap_acceptance::{self, Accepted};
use gungnir_app::state::AppState;
use gungnir_app::{session, update};
use gungnir_config::{
    ApproachConfig, AuthenticationConfig, AuthenticationProvider, BackendConfig, ConfigBaseline,
    SecurityConfig,
};
use gungnir_eventing::{Envelope, EventBus, InProcessBus, Receiver};
use gungnir_model::{MissionTime, ReopenedBecause, SystemHealth};
use gungnir_node::gap_acceptance::{BaselineHolds, NodeGapAcceptances, Tick};
use gungnir_security::{
    hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use gungnir_sensor_management::SensorRegistry;
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};
use std::sync::Arc;
use std::time::Instant;

const PASSPHRASE: &str = "correct horse battery staple";
const COMMANDER: u64 = 31;

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-linked-acceptance-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn baseline(dir: &std::path::Path) -> ConfigBaseline {
    ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        approaches: vec![ApproachConfig {
            name: "east".into(),
            points: vec![[0.9, 0.2, 100.0], [0.9, 0.201, 100.0]],
            corridor_half_width_m: None,
        }],
        ..ConfigBaseline::default()
    }
}

/// The node's coverage answer: one uncovered stretch of the "east" approach.
fn report() -> CoverageReport {
    CoverageReport {
        parameters: CoverageParameters {
            sample_spacing_m: 100.0,
            terrain_masking_applied: false,
        },
        gaps: vec![CoverageGap {
            approach: 0,
            from_m: 1_000.0,
            to_m: 1_400.0,
            samples: Vec::new(),
            severity: GapSeverity::Uncovered,
        }],
    }
}

fn account() -> Account {
    Account {
        operator: OperatorId(COMMANDER),
        role: Role::Commander,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }
}

/// A node serving on loopback with its acceptance loop running; stopped on drop.
struct Node {
    running: Arc<std::sync::atomic::AtomicBool>,
    _runtime: tokio::runtime::Runtime,
    dir: std::path::PathBuf,
}

impl Drop for Node {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Node {
    /// Serve `served` as the coverage answer, judging acceptances under `config`.
    fn serve(addr: std::net::SocketAddr, config: &ConfigBaseline, served: CoverageReport) -> Self {
        let issuer = TokenIssuer::new(vec![5u8; 32], 10_000.0).expect("issuer");
        let api = Arc::new(
            NodeApi::new(SnapshotResponse::new(
                Vec::new(),
                None,
                SystemHealth::default(),
                Vec::new(),
            ))
            .with_callers(Arc::new(AccountTokenAuthority::new(
                Box::new(InMemoryAccountStore::new(vec![account()])),
                issuer,
            ))),
        );
        api.set_now(100.0);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("node runtime");
        runtime.block_on(async {
            let listener = bind(addr).await.expect("bound");
            let api = Arc::clone(&api);
            tokio::spawn(async move {
                let _ = serve_on(listener, api).await;
            });
        });
        let dir = scratch("node");
        let holds = BaselineHolds::of(config);
        let bus = InProcessBus::new();
        let api_rx: Receiver<Envelope> = bus.subscribe();
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        // A plain thread, as `desktop_projection.rs`'s node loop is, so a failing assertion
        // reports rather than hangs.
        std::thread::spawn({
            let api = Arc::clone(&api);
            let running = Arc::clone(&running);
            move || {
                let mut ledger = NodeGapAcceptances::default();
                let mut audit = gungnir_security::InMemoryAuditLog::new();
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    let mut coverage =
                        CoverageResponse::Computed(ComputedCoverage::unaccepted(served.clone()));
                    let _ = gungnir_node::gap_acceptance::tick(
                        &mut ledger,
                        api.take_acceptances(),
                        &holds.judged(&coverage),
                        &mut Tick {
                            bus: &bus,
                            audit: &mut audit,
                            now: MissionTime(100.0),
                            session: gungnir_store::SessionId(1),
                        },
                    );
                    let marks = ledger.marks(&holds.judged(&coverage));
                    if let CoverageResponse::Computed(computed) = &mut coverage {
                        computed.acceptances = marks;
                    }
                    let _ = api.publish_coverage(coverage);
                    for envelope in api_rx.try_iter() {
                        let _ = api.publish_event(envelope);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        });
        Node {
            running,
            _runtime: runtime,
            dir,
        }
    }
}

/// A desktop on the connected profile, signed in as the commander.
fn desktop(endpoint: &str) -> (AppState, std::path::PathBuf) {
    let dir = scratch("desktop");
    let config = baseline(&dir);
    desktop_with(endpoint, config, dir)
}

/// [`desktop`], on `config`.
fn desktop_with(
    endpoint: &str,
    mut config: ConfigBaseline,
    dir: std::path::PathBuf,
) -> (AppState, std::path::PathBuf) {
    config.data_dir = dir.to_string_lossy().into_owned();
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&vec![account()]).expect("json"),
    )
    .expect("written");
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
    let mut state = AppState::with_config(config).expect("starts");
    let mut draft = SignInDraft {
        operator: COMMANDER.to_string(),
        passphrase: PASSPHRASE.into(),
        ..SignInDraft::default()
    };
    session::apply(&mut state, &mut draft, SessionAction::SignIn);
    assert_eq!(state.role(), Role::Commander, "{:?}", state.alerts);
    (state, dir)
}

fn until(state: &mut AppState, what: &str, mut check: impl FnMut(&AppState) -> bool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    loop {
        update::tick(state);
        if check(state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; alerts: {:?}",
            state.alerts
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn free_addr() -> std::net::SocketAddr {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
    probe.local_addr().expect("addr")
}

fn the_gap() -> gungnir_model::AcceptedGap {
    let r = report();
    accepted_gap(&r.gaps[0], "east", r.parameters)
}

/// **Linked**: PN-11 lists the node's gap, accepting it sends it to the node and records
/// nothing here, and the node's mark comes back; a gap the node cannot see is refused and
/// the refusal is said.
#[test]
fn a_linked_desktops_acceptance_is_the_nodes() {
    let addr = free_addr();
    let node = Node::serve(addr, &baseline(&scratch("node-baseline")), report());
    let (mut desk, dir) = desktop(&format!("http://{addr}"));
    until(&mut desk, "the node's coverage answer", |s| {
        gap_acceptance::node_gaps(s).is_some()
    });
    let (listed, marks) = gap_acceptance::node_gaps(&desk).expect("read");
    assert_eq!(listed.gaps.len(), 1);
    assert!(marks[0].is_none(), "nothing accepted yet");

    let sent = gap_acceptance::accept(&mut desk, &the_gap(), "watched by the patrol")
        .expect("a signed-in commander accepts");
    assert_eq!(sent, Accepted::SentToNode);
    assert!(
        desk.gap_acceptances.standing.is_empty(),
        "nothing is recorded here while linked"
    );
    until(&mut desk, "the node's mark", |s| {
        gap_acceptance::node_gaps(s).is_some_and(|(_, marks)| marks[0].is_some())
    });
    let (_, marks) = gap_acceptance::node_gaps(&desk).expect("read");
    let held = marks[0].as_ref().expect("marked");
    assert_eq!(
        (held.operator.as_str(), held.reason.as_str()),
        (COMMANDER.to_string().as_str(), "watched by the patrol")
    );
    until(&mut desk, "the node's answer", |s| {
        s.alerts.iter().any(|a| a.contains("recorded on the node"))
    });

    let mut elsewhere = the_gap();
    elsewhere.to_m += 400.0;
    gap_acceptance::accept(&mut desk, &elsewhere, "a different answer").expect("sent");
    until(&mut desk, "the node's refusal", |s| {
        s.alerts
            .iter()
            .any(|a| a.contains("not accepted by the node") && a.contains("not on the node's"))
    });
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

/// Round 1's baseline (`testdata/usability/round-1.json`): both radars searching leave the
/// outer end of the upper Vell approach uncovered on the desktop's own coverage report.
fn round_1() -> ConfigBaseline {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/usability/round-1.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("committed")).expect("parses")
}

/// A desktop on round 1, cut off (nothing serves `addr` yet), its radars searching, having
/// accepted the uncovered gap itself: recorded here and queued to forward.
fn cut_off_desktop_that_accepted(
    addr: std::net::SocketAddr,
) -> (
    AppState,
    std::path::PathBuf,
    gungnir_model::GapAcceptanceId,
    CoverageReport,
) {
    let (mut desk, dir) = desktop_with(&format!("http://{addr}"), round_1(), scratch("cut-off"));
    for id in [1, 2] {
        desk.sensors
            .set_mode(
                gungnir_model::SensorId(id),
                gungnir_model::SensorMode::Search,
            )
            .expect("search");
    }
    update::tick(&mut desk);
    assert!(
        desk.link.as_ref().is_some_and(|l| !l.connected()),
        "linked, and nothing answers yet"
    );
    let report = gungnir_app::sustainment::coverage_report(&desk).expect("round 1 has an origin");
    let names = gungnir_app::sustainment::approach_names(&desk);
    let gap = report
        .gaps
        .iter()
        .find(|g| g.severity == GapSeverity::Uncovered)
        .map(|g| accepted_gap(g, &names[g.approach], report.parameters))
        .expect("round 1 leaves the outer approach uncovered");
    let accepted = gap_acceptance::accept(&mut desk, &gap, "watched by the patrol")
        .expect("a signed-in commander accepts while cut off");
    let Accepted::Recorded(id) = accepted else {
        panic!("cut off, it is recorded here: {accepted:?}");
    };
    let queued = desk
        .link
        .as_ref()
        .map(gungnir_remote::link::NodeLink::acceptances_in_flight)
        .unwrap_or_default();
    assert_eq!(queued.len(), 1, "queued to forward");
    let offline = queued[0]
        .accepted_offline
        .as_ref()
        .expect("names who accepted it");
    assert_eq!(offline.operator, COMMANDER.to_string());
    (desk, dir, id, report)
}

/// **Cut off, then the node answers with the same coverage**: the forwarded acceptance is
/// held on the node, named as the commander who accepted it, and stands here too.
#[test]
fn a_cut_off_acceptance_is_forwarded_and_held_by_the_node() {
    let addr = free_addr();
    let (mut desk, dir, id, report) = cut_off_desktop_that_accepted(addr);
    let node = Node::serve(addr, &round_1(), report);
    until(
        &mut desk,
        "the node to hold the forwarded acceptance",
        |s| {
            gap_acceptance::node_gaps(s).is_some_and(|(_, marks)| marks.iter().any(Option::is_some))
        },
    );
    let (_, marks) = gap_acceptance::node_gaps(&desk).expect("read");
    let held = marks.iter().flatten().next().expect("marked");
    assert_eq!(held.operator, COMMANDER.to_string());
    until(&mut desk, "the forward's answer", |s| {
        s.alerts
            .iter()
            .any(|a| a.contains("made while cut off, forwarded"))
    });
    assert!(
        desk.gap_acceptances
            .standing
            .iter()
            .any(|(a, _)| a.id == id),
        "it stands here too"
    );
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

/// **Cut off, then the node answers with coverage that has no such gap**: the node refuses
/// the forward, and the desktop's own acceptance re-opens with the node's reason, journaled
/// and audited, so no console shows an acceptance the node does not hold (D-134).
#[test]
fn a_forward_the_node_refuses_reopens_here_with_its_reason() {
    let addr = free_addr();
    let (mut desk, dir, id, report) = cut_off_desktop_that_accepted(addr);
    let events = desk.events.subscribe();
    let covered = CoverageReport {
        gaps: Vec::new(),
        ..report
    };
    let node = Node::serve(addr, &round_1(), covered);
    until(&mut desk, "the re-opening", |s| {
        s.gap_acceptances
            .reopened
            .iter()
            .any(|r| r.acceptance.id == id)
    });
    let reopened = desk
        .gap_acceptances
        .reopened
        .iter()
        .find(|r| r.acceptance.id == id)
        .expect("re-opened");
    let ReopenedBecause::RefusedByNode { reason } = &reopened.because else {
        panic!("{:?}", reopened.because);
    };
    assert!(
        reason.contains("not on the node's coverage answer"),
        "{reason}"
    );
    assert!(!desk
        .gap_acceptances
        .standing
        .iter()
        .any(|(a, _)| a.id == id));
    let journaled = events.try_iter().any(|e| {
        matches!(
            e.event,
            gungnir_eventing::Event::Planning(gungnir_model::events::PlanningEvent::GapAcceptanceReopened {
                acceptance,
                because: ReopenedBecause::RefusedByNode { .. },
                ..
            }) if acceptance == id
        )
    });
    assert!(journaled, "the re-opening is on the record");
    assert!(
        gungnir_security::AuditLog::entries(&desk.audit)
            .iter()
            .any(|e| e.operator.is_none() && e.detail.contains("the node refused it")),
        "audited, attributed to nobody"
    );
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

/// Restart `desk` over the same journal, on round 1, signed in as the commander again.
fn restarted(mut desk: AppState, addr: std::net::SocketAddr, dir: std::path::PathBuf) -> AppState {
    update::tick(&mut desk);
    desk.save_session().expect("saved");
    drop(desk);
    let (mut desk, _) = desktop_with(&format!("http://{addr}"), round_1(), dir);
    // The radars come back in standby; searching again, as before the restart, the
    // desktop's own coverage is what the acceptance was made against, so DN-33 §5 leaves
    // it standing.
    for id in [1, 2] {
        desk.sensors
            .set_mode(
                gungnir_model::SensorId(id),
                gungnir_model::SensorMode::Search,
            )
            .expect("search");
    }
    desk
}

/// **GAP-202: a cut-off desktop restarts before its node answers.** The forward that was
/// waiting is gone with the process, but the acceptance is on the journal; the restarted
/// desktop owes it again, and when the node answers it holds it as the commander's.
#[test]
fn a_forward_owed_before_a_restart_is_sent_after_it() {
    let addr = free_addr();
    let (desk, dir, id, report) = cut_off_desktop_that_accepted(addr);
    let mut desk = restarted(desk, addr, dir.clone());
    assert_eq!(
        desk.gap_acceptances.owed(),
        &[id],
        "owed again after the restart"
    );
    update::tick(&mut desk);
    assert!(
        desk.link.as_ref().is_some_and(|l| l
            .acceptances_in_flight()
            .iter()
            .any(|o| o.local == Some(id))),
        "queued again for the node"
    );
    let node = Node::serve(addr, &round_1(), report);
    until(&mut desk, "the node to hold the recovered forward", |s| {
        gap_acceptance::node_gaps(s).is_some_and(|(_, marks)| marks.iter().any(Option::is_some))
    });
    until(&mut desk, "the forward to settle", |s| {
        s.gap_acceptances.owed().is_empty()
    });
    let (_, marks) = gap_acceptance::node_gaps(&desk).expect("read");
    let held = marks.iter().flatten().next().expect("marked");
    assert_eq!(
        held.operator,
        COMMANDER.to_string(),
        "the accepting commander's"
    );
    assert!(desk
        .gap_acceptances
        .standing
        .iter()
        .any(|(a, _)| a.id == id));
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

/// **GAP-202: a restart re-sends what the node already holds.** The node answers
/// `already-accepted`, which settles the forward: the acceptance stands here and nothing
/// re-opens.
#[test]
fn a_forward_the_node_already_holds_settles_without_reopening() {
    let addr = free_addr();
    let (mut desk, dir, id, report) = cut_off_desktop_that_accepted(addr);
    let node = Node::serve(addr, &round_1(), report);
    until(&mut desk, "the first forward to settle", |s| {
        s.gap_acceptances.owed().is_empty()
    });
    let mut desk = restarted(desk, addr, dir.clone());
    assert_eq!(
        desk.gap_acceptances.owed(),
        &[id],
        "owed again after the restart"
    );
    until(&mut desk, "the repeat to settle", |s| {
        s.gap_acceptances.owed().is_empty()
    });
    assert!(
        desk.gap_acceptances
            .standing
            .iter()
            .any(|(a, _)| a.id == id),
        "already held on the node is held: it stands"
    );
    assert!(
        desk.gap_acceptances.reopened.is_empty(),
        "nothing re-opened"
    );
    assert!(
        desk.alerts
            .iter()
            .any(|a| a.contains("already accepted on the node")),
        "{:?}",
        desk.alerts
    );
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}
