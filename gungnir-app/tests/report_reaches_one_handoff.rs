// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! DN-31 §9 row 1, the effector-report half (GAP-130; D-56): "an effector report reaches
//! exactly one handoff".
//!
//! Two desktops share one node. Each has its own planner and its own queue, and each
//! decides a plan and hands it off, so both handoffs exist at once. An effector reports on
//! one of them the way every report travels: `POST /v3/handoffs/{decision_id}/report` to
//! the node over the real transport; the node's report queue; the `HandoffEvent::Reported`
//! the node loop puts on its record for every desktop; each desktop's link inbox; and
//! `node_tasks::sweep`, which hands it to `handoffs::apply_report`. A report matches its
//! handoff by the decision alone (`gungnir_model::handoff::accept_report`), and with the
//! counters this replaced both desktops' first decision was decision 1: the report reached
//! both handoffs, and each desktop applied it to its own.
//!
//! Two steps are stood in for, and said so: the node loop's publication, which is three
//! lines of `gungnir-node`'s binary and is built here the way it builds it; and the event
//! stream's hop from node to desktop, whose envelopes crossing unchanged is
//! `gungnir-remote/tests/wire_conformance.rs`'s to show.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{EffectorReportRequest, SnapshotResponse};
use gungnir_app::state::AppState;
use gungnir_app::{decisions, node_tasks, update};
use gungnir_command::{ApprovalWorkflow, OperatorDecision};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::HandoffEvent;
use gungnir_model::handoff::{accept_report, EffectorReport};
use gungnir_model::{
    Classification, DecisionId, DetectionView, MissionTime, Provenance, Quality, Releasability,
    SystemHealth, TrackId, TrackStatus, TrackView,
};
use gungnir_remote::link::NodeLink;
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role};
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};
use gungnir_ui::panels::approval_queue::PendingId;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PASSPHRASE: &str = "correct horse battery staple";
/// The node's administrator, who holds `effector.report` and keys in what came over the
/// radio when an effector has no certificate of its own.
const ADMINISTRATOR: u64 = 1;

struct Picture(Vec<TrackView>);

impl TrackingService for Picture {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
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

fn hostile(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(9_000.0, 2_000.0, 150.0, -40.0, 0.0, 0.0),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

/// A desktop that proposes, queues and decides one plan with its own planner and its own
/// queue, and hands it off to `battery-2`. Returns the decision it minted.
fn a_desktop_that_decides(name: &str) -> (AppState, std::path::PathBuf, DecisionId) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-one-handoff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = serde_json::json!({
        "version": ConfigBaseline::default().version,
        "data_dir": dir.to_string_lossy(),
        "resources": [
            {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 4, "layer": "point",
             "handoff_endpoint": "battery-2"}
        ],
        "endpoints": [
            {"name": "battery-2", "kind": "handoff", "address": "https://battery-2.example/handoff"}
        ],
        "policy": {
            "control_status": {"by_layer": {"point": "free"}},
            "authority": {"rules": [
                {"action": "plan.decide", "role": "Operator", "layer": "point", "class": null,
                 "pre_delegated": false}
            ]}
        },
        "assessment": {"effect_window_s": {"point": 30.0}}
    })
    .to_string();
    let config: ConfigBaseline = serde_json::from_str(&text).expect("the baseline parses");
    gungnir_config::validate(&config).expect("the baseline is valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    state.tracking = Box::new(Picture(vec![hostile(7)]));
    update::tick(&mut state);
    let Some(item) = state.desk.approvals.queue().first().map(|p| p.id) else {
        panic!("{name} queued nothing: {:?}", state.alerts);
    };
    decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted).expect("decided");
    let decision = state
        .desk
        .handoffs
        .first()
        .expect("a handoff")
        .handoff
        .decision;
    (state, dir, decision)
}

/// A node that knows its administrator, on a runtime of its own.
fn node() -> (tokio::runtime::Runtime, Arc<NodeApi>, std::net::SocketAddr) {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(ADMINISTRATOR),
        role: Role::Administrator,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = gungnir_security::TokenIssuer::new(vec![6u8; 32], 300.0).expect("issuer");
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("node runtime");
    let served = Arc::clone(&api);
    let addr = runtime.block_on(async move {
        let listener = gungnir_api::transport::bind("127.0.0.1:0".parse().expect("address"))
            .await
            .expect("bound");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = gungnir_api::transport::serve_on(listener, served).await;
        });
        addr
    });
    (runtime, api, addr)
}

/// One HTTP/1.1 request to the node: the status and the body.
async fn http(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = token {
        use std::fmt::Write as _;
        let _ = write!(head, "Authorization: Bearer {token}\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write");
    stream.write_all(body.as_bytes()).await.expect("write body");
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

/// **DN-31 §9 row 1**: with two desktops' handoffs both present, a report on one of them
/// names exactly one handoff across both desktops, is applied by the desktop that issued
/// it, and is refused by the other as naming a decision it never handed off.
#[test]
fn an_effector_report_reaches_the_one_handoff_it_names_across_two_desktops() {
    let (mut desktop_a, dir_a, decision_a) = a_desktop_that_decides("a");
    let (mut desktop_b, dir_b, decision_b) = a_desktop_that_decides("b");
    assert_ne!(
        decision_a, decision_b,
        "two desktops minted one decision identifier"
    );
    let every_handoff: Vec<_> = desktop_a
        .desk
        .handoffs
        .iter()
        .chain(&desktop_b.desk.handoffs)
        .map(|record| record.handoff.clone())
        .collect();
    assert_eq!(every_handoff.len(), 2, "both handoffs exist at once");
    let report = EffectorReport::Acknowledged {
        at: MissionTime(3.0),
    };
    assert_eq!(
        every_handoff
            .iter()
            .filter(|h| h.decision == decision_a)
            .count(),
        1,
        "the report's decision names more than one handoff"
    );
    assert_eq!(
        accept_report(&every_handoff, decision_a, &report).map(|h| h.decision),
        Ok(decision_a)
    );

    // The effector's report reaches the node over the real transport, naming the decision
    // in the form the desktop wrote it.
    let (runtime, api, addr) = node();
    let token = runtime.block_on(async {
        let (status, body) = http(
            addr,
            "POST",
            "/v3/session",
            None,
            &format!("{{\"operator\":{ADMINISTRATOR},\"passphrase\":\"{PASSPHRASE}\"}}"),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<serde_json::Value>(&body).expect("json")["token"]
            .as_str()
            .expect("token")
            .to_owned()
    });
    let request = serde_json::to_string(&EffectorReportRequest {
        report: report.clone(),
    })
    .expect("json");
    let (status, body) = runtime.block_on(http(
        addr,
        "POST",
        &gungnir_api::path(&format!("/handoffs/{decision_a}/report")),
        Some(&token),
        &request,
    ));
    assert_eq!(status, 202, "{body}");
    let queued = api.take_effector_reports();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].decision, decision_a);

    // What the node loop puts on its record for every desktop (`gungnir-node`'s
    // `record_effector_reports`), and each desktop's link receiving it.
    let envelope = Envelope {
        seq: 1,
        mission_time: MissionTime(3.0),
        event: Event::Handoff(HandoffEvent::Reported {
            decision: queued[0].decision,
            endpoint: queued[0].endpoint.clone(),
            report: queued[0].report.clone(),
            at: MissionTime(3.0),
        }),
    };
    for desktop in [&mut desktop_a, &mut desktop_b] {
        let link = NodeLink::scripted();
        if let Some(mut projection) = link.read() {
            projection.inbox.push_back(envelope.clone());
        }
        desktop.link = Some(link);
        node_tasks::sweep(desktop);
    }

    assert_eq!(
        desktop_a.desk.handoffs[0].reports,
        vec![report],
        "the desktop that issued the handoff did not apply the report to it"
    );
    assert!(
        desktop_b.desk.handoffs[0].reports.is_empty(),
        "the report reached the other desktop's handoff"
    );
    assert!(
        desktop_b
            .alerts
            .iter()
            .any(|a| a.contains("rejected") && a.contains(&decision_a.short())),
        "the other desktop did not refuse a report on a decision it never handed off: {:?}",
        desktop_b.alerts
    );
    drop(runtime);
    let _ = std::fs::remove_dir_all(dir_a);
    let _ = std::fs::remove_dir_all(dir_b);
}
