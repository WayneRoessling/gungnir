// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The endpoint transport (GAP-040, GAP-042): a handoff posted to a configured HTTP
//! endpoint is `Delivered` when the endpoint accepts, `Refused` with the body when it does
//! not, and retried, never dropped, when it is unreachable; a warning is `Sent` when the
//! endpoint accepts and `Failed` loudly when it refuses. Against a real socket: a stub
//! server on the loopback that answers what the test tells it to.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use gungnir_app::state::AppState;
use gungnir_app::{decisions, update, warnings};
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::{AssetConfig, ConfigBaseline, EndpointConfig, ResourceConfig};
use gungnir_model::handoff::DeliveryState;
use gungnir_model::{
    Classification, DetectionView, EffectorLayer, InterceptSolutionView, MissionTime, PlanId,
    PlanView, Provenance, Quality, Releasability, ResourceId, TrackId, TrackStatus, TrackView,
};
use gungnir_policy::PolicyVerdict;
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};
use gungnir_ui::panels::approval_queue::PendingId;

/// A one-thread HTTP server that answers every request with `status` and records how
/// many it saw.
fn stub(status: Arc<AtomicU16>, hits: Arc<AtomicU16>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 65536];
            let mut read = 0;
            // Read until the headers and the declared body are in.
            while let Ok(n) = stream.read(&mut buf[read..]) {
                if n == 0 {
                    break;
                }
                read += n;
                let text = String::from_utf8_lossy(&buf[..read]);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .or_else(|| l.strip_prefix("Content-Length: "))
                        })
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if read >= end + 4 + length {
                        break;
                    }
                }
            }
            hits.fetch_add(1, Ordering::SeqCst);
            let code = status.load(Ordering::SeqCst);
            let body = if code >= 400 { "no such effector" } else { "" };
            let _ = write!(
                stream,
                "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.flush();
        }
    });
    format!("http://{addr}/messages")
}

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

fn track(id: u64, e: f64, ve: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(e, 0.0, 50.0, ve, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 25.0,
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn desktop(name: &str, url: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-endpoint-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([0.959_931, 0.209_44, 0.0]),
        endpoints: vec![
            EndpointConfig {
                name: "battery".into(),
                kind: "http".into(),
                address: url.into(),
            },
            EndpointConfig {
                name: "port-authority".into(),
                kind: "http".into(),
                address: url.into(),
            },
        ],
        resources: vec![ResourceConfig {
            handoff_endpoint: Some("battery".into()),
            id: 1,
            position: [0.959_931, 0.209_44, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: None,
        }],
        assets: vec![AssetConfig {
            id: 1,
            name: "the harbour".into(),
            position: [0.959_931, 0.209_44, 0.0],
            radius_m: Some(100.0),
            priority: "high".into(),
            warning_lead_time_s: Some(120.0),
            warning_channel: Some("port-authority".into()),
            warning_within_m: None,
            note: None,
        }],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    assert!(state.endpoint_client.is_some(), "{:?}", state.alerts);
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

fn at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
    update::tick(state);
}

/// Tick until the predicate holds.
///
/// **A deadlock guard, not a performance assertion** (the reasoning is
/// `gungnir-tracking-service/tests/sample_set_replay.rs`'s): a real hang still fails,
/// and a loaded machine no longer does. The loop exits the moment the condition holds,
/// so a passing run costs what it always did.
fn settle(state: &mut AppState, t: f64, done: impl Fn(&AppState) -> bool) {
    for _ in 0..6_000 {
        at(state, t);
        if done(state) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never settled: {:?}", state.alerts);
}

fn decide(state: &mut AppState) -> gungnir_model::DecisionId {
    let pending = state
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView::intercept(
                PlanId(7),
                MissionTime(0.0),
                vec![InterceptSolutionView {
                    resource: ResourceId(1),
                    track: TrackId(1),
                    intercept_point: None,
                    time_to_intercept_s: None,
                }],
                0.0,
            ),
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".into(),
        })
        .expect("queued");
    decisions::decide(state, PendingId(pending.0), OperatorDecision::Accepted).expect("decided");
    state
        .handoffs
        .last()
        .expect("an accepted decision issues a handoff")
        .handoff
        .decision
}

#[test]
fn a_handoff_to_an_accepting_endpoint_is_delivered_and_a_refusal_is_recorded() {
    let status = Arc::new(AtomicU16::new(200));
    let hits = Arc::new(AtomicU16::new(0));
    let url = stub(status.clone(), hits.clone());
    let (mut state, dir) = desktop("handoff", &url);
    state.tracking = Box::new(Picture(vec![track(1, 3000.0, 0.0)]));
    let decision = decide(&mut state);
    let record = |s: &AppState| {
        s.handoffs
            .iter()
            .find(|h| h.handoff.decision == decision)
            .map(|h| h.delivery.clone())
    };
    assert!(
        matches!(record(&state), Some(DeliveryState::Undelivered { .. })),
        "posted, not yet answered"
    );
    settle(&mut state, 1.0, |s| {
        matches!(record(s), Some(DeliveryState::Delivered { .. }))
    });
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("delivered to battery")),
        "{:?}",
        state.alerts
    );

    // A second decision against a refusing endpoint.
    status.store(503, Ordering::SeqCst);
    let pending = state
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView::intercept(
                PlanId(8),
                MissionTime(2.0),
                vec![InterceptSolutionView {
                    resource: ResourceId(1),
                    track: TrackId(1),
                    intercept_point: None,
                    time_to_intercept_s: None,
                }],
                0.0,
            ),
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(2.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".into(),
        })
        .expect("queued");
    decisions::decide(&mut state, PendingId(pending.0), OperatorDecision::Accepted)
        .expect("decided");
    let second = state
        .handoffs
        .last()
        .expect("an accepted decision issues a handoff")
        .handoff
        .decision;
    settle(&mut state, 3.0, |s| {
        s.handoffs.iter().any(|h| {
            h.handoff.decision == second && matches!(h.delivery, DeliveryState::Refused { .. })
        })
    });
    let refused = state
        .handoffs
        .iter()
        .find(|h| h.handoff.decision == second)
        .expect("record");
    match &refused.delivery {
        DeliveryState::Refused { reason, .. } => assert!(
            reason.contains("503") && reason.contains("no such effector"),
            "{reason}"
        ),
        other => panic!("{other:?}"),
    }
    assert!(
        state.pending_handoffs.is_empty(),
        "a refusal is not retried"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unreachable_endpoint_is_retried_and_never_dropped() {
    // Nothing listens on this port.
    let (mut state, dir) = desktop("unreachable", "http://127.0.0.1:9/messages");
    state.tracking = Box::new(Picture(vec![track(1, 3000.0, 0.0)]));
    let decision = decide(&mut state);
    settle(&mut state, 1.0, |s| {
        s.alerts
            .iter()
            .any(|a| a.contains("undelivered (attempt 1)"))
    });
    assert!(matches!(
        state
            .handoffs
            .iter()
            .find(|h| h.handoff.decision == decision)
            .map(|h| &h.delivery),
        Some(DeliveryState::Undelivered { .. })
    ));
    assert_eq!(state.pending_handoffs.len(), 1, "kept for the next attempt");
    assert_eq!(state.pending_handoffs[0].attempts, 1);
    // Before the retry interval nothing is posted; after it, the second attempt goes out.
    at(&mut state, 10.0);
    assert_eq!(state.pending_handoffs[0].attempts, 1);
    settle(&mut state, 40.0, |s| {
        s.alerts
            .iter()
            .any(|a| a.contains("undelivered (attempt 2)"))
    });
    assert_eq!(state.pending_handoffs.len(), 1, "still never dropped");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_warning_is_sent_through_the_endpoint_and_fails_loudly_when_it_refuses() {
    let status = Arc::new(AtomicU16::new(200));
    let hits = Arc::new(AtomicU16::new(0));
    let url = stub(status.clone(), hits.clone());
    let (mut state, dir) = desktop("warning", &url);
    // 5 km out at 50 m/s: inside the 120 s lead time.
    state.tracking = Box::new(Picture(vec![track(1, 5_000.0, -50.0)]));
    at(&mut state, 0.0);
    assert_eq!(
        warnings::lines(&state, None)[0].state,
        "sent",
        "handed to the transport"
    );
    settle(&mut state, 1.0, |s| s.pending_warnings.is_empty());
    assert_eq!(
        warnings::lines(&state, None)[0].state,
        "sent",
        "the endpoint accepted; sent stands"
    );
    assert_eq!(state.warnings.failed_count(), 0);

    // The track turns away and back with the endpoint now refusing: a fresh warning
    // that fails loudly.
    state.tracking = Box::new(Picture(vec![track(1, 5_000.0, 50.0)]));
    at(&mut state, 2.0);
    assert!(state.warnings.open().is_empty());
    status.store(500, Ordering::SeqCst);
    state.tracking = Box::new(Picture(vec![track(1, 5_000.0, -50.0)]));
    at(&mut state, 3.0);
    settle(&mut state, 4.0, |s| s.warnings.failed_count() == 1);
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("was not delivered") && a.contains("500")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}
