// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A fully populated `SnapshotResponse` loses nothing on the wire (GAP-112).
//!
//! `verification-capability-table.md` §2's `gungnir-api` "Contract compatibility and
//! authorization" row asks for a zero-loss round-trip of a fully populated snapshot,
//! checked so far only on sparse ones (`gungnir-api/src/v3/mod.rs`'s own tests build
//! `SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new())` every
//! time, and `party.rs`'s builds three tracks but no plan, requirement, bearing ray or
//! queue item). A field a serializer silently dropped from a real, populated snapshot
//! would pass every one of those.
//!
//! **`/v3`, not `/v2`.** The row's text still says "v2": GAP-130 moved the interface to
//! `/v3` whole when decision, plan and queue-item identifiers became UUID v7 strings
//! (`gungnir-api/tests/retired_v2.rs`), after the GAP-067 walk wrote that row. `/v2`
//! answers `410 Gone` now, not a snapshot, so a test that actually round-trips one has to
//! ask `/v3`; the row's wording is corrected in the same change as this file.
//!
//! Two round trips, because the gap names both: plain serde, and through an operator's
//! real `GET /v3/snapshot` with a bearer token, over the real transport `NodeApi` serves.
//! The HTTP half is hand-rolled over a plain `TcpStream` rather than pulling in a client
//! crate, the same choice every other `gungnir-api/tests/*.rs` file already makes (see
//! `party.rs`'s own comment); `bind`/`serve_on` are unencrypted loopback, so unlike the
//! TLS-carrying tests here this one needs no certificate authority.

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{QueueItemView, SnapshotResponse};
use gungnir_model::events::VerdictSummary;
use gungnir_model::requirements::{CollectionRequirement, Concurrence, RequirementState};
use gungnir_model::{
    AssetExtent, AssetPriority, BearingRayView, Classification, EffectorLayer, Geodetic,
    InterceptSolutionView, MissionTime, PendingApprovalId, PipelineStatsView, PlanId, PlanKind,
    PlanView, Provenance, Quality, Releasability, RequirementId, ResourceId, SensorId,
    SystemHealth, TrackId, TrackStatus, TrackView,
};
use gungnir_security::{
    hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use std::fmt::Write as _;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const OPERATOR: u64 = 41;
const PASSPHRASE: &str = "correct horse battery staple";

/// One track with every field away from its default: a non-identity state and
/// covariance, a classification other than `Unknown`, and a releasability other than
/// `Internal` -- three of the fields the gap's own words call out.
fn populated_track() -> TrackView {
    let mut state = nalgebra::SVector::<f64, 6>::zeros();
    state.copy_from_slice(&[1_200.5, -340.2, 610.0, 12.5, -3.25, 0.4]);
    let mut covariance = nalgebra::SMatrix::<f64, 6, 6>::identity();
    covariance *= 87.5;
    covariance[(0, 3)] = 4.5;
    covariance[(3, 0)] = 4.5;
    TrackView {
        id: TrackId(7),
        status: TrackStatus::Confirmed,
        state,
        covariance,
        classification: Classification::Hostile,
        provenance: Provenance {
            source_sensor_ids: vec![3, 5],
            calibration_baseline_version: Some("radar-medium-v3".into()),
            algorithm_version: "gungnir-filters 0.1.0".into(),
            ..Provenance::default()
        },
        quality: Quality {
            association_confidence: 0.91,
            latency_s: 0.4,
            is_stale: false,
        },
        mission_time: MissionTime(412.75),
        releasability: Releasability::parties(["sector-north", "sector-east"]),
    }
}

fn populated_plan() -> PlanView {
    PlanView {
        id: PlanId(9),
        mission_time: MissionTime(412.75),
        kind: PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(2),
                track: TrackId(7),
                intercept_point: Some(Geodetic {
                    lat_rad: 0.61,
                    lon_rad: -1.2,
                    alt_m: 850.0,
                }),
                time_to_intercept_s: Some(38.5),
            }],
        },
        policy_value: 0.73,
        releasability: Releasability::AllPeers,
        // GAP-156: not the default, so a wire form that dropped the label shows here.
        basis: gungnir_model::PlanBasis::OneStep,
    }
}

fn populated_requirement() -> CollectionRequirement {
    CollectionRequirement {
        id: RequirementId(3),
        title: "identify the contact off the harbour".into(),
        priority: AssetPriority::High,
        area: AssetExtent::Circle {
            center: Geodetic {
                lat_rad: 0.6,
                lon_rad: -1.19,
                alt_m: 0.0,
            },
            radius_m: 2_000.0,
        },
        needed_by: Some(MissionTime(500.0)),
        state: RequirementState::Tasked {
            by: Concurrence::Operator {
                id: "41".into(),
                role: "Operator".into(),
            },
        },
    }
}

fn populated_bearing_ray() -> BearingRayView {
    BearingRayView {
        sensor: SensorId(4),
        origin_enu: [10.0, 20.0, 3.0],
        azimuth_rad: 0.42,
        elevation_rad: Some(0.05),
        azimuth_one_sigma_rad: 0.012,
        valid_until: MissionTime(460.0),
    }
}

fn populated_pipeline_stats() -> PipelineStatsView {
    PipelineStatsView {
        accepted: 120,
        too_late: 3,
        reordered: 7,
        accepted_late: 0,
        not_finite: 1,
        epochs: 40,
        associated: 90,
        initiated: 5,
        bearings_offered: 12,
        bearings_updated: 4,
        bearings_retained: 2,
        bearings_expired: 1,
        bearings_refused: 0,
    }
}

fn populated_queue_item() -> QueueItemView {
    QueueItemView {
        item: PendingApprovalId(0x9f3a_61c2_dead_beef_0001_0002_0003_0004),
        plan: populated_plan(),
        verdict: VerdictSummary::RequiresHumanApproval,
        layer: EffectorLayer::Point,
        submitted: MissionTime(400.0),
        expires_at: Some(MissionTime(430.0)),
        escalate_at: Some(MissionTime(415.0)),
        offered_to: vec!["Operator".into(), "Supervisor".into()],
        pre_delegated: false,
        priority: 0.6,
    }
}

/// Every field the current `SnapshotResponse` carries, none of them at their default.
fn full_snapshot() -> SnapshotResponse {
    SnapshotResponse::new(
        vec![populated_track()],
        Some(populated_plan()),
        SystemHealth {
            tracking_healthy: true,
            intercept_healthy: false,
            ingest_healthy: true,
        },
        vec![populated_requirement()],
    )
    .with_bearing_data(vec![populated_bearing_ray()], populated_pipeline_stats())
    .with_queue(vec![populated_queue_item()])
    // GAP-157: the plan's standing, with every field of its richest variant set.
    .with_plan_standing(gungnir_model::PlanStandingView::Interim {
        value_at_least: 6.25,
        optimum_at_most: 7.5,
        reason: "the exact solver takes at most 16 tracks".into(),
    })
}

/// Plain serde: the shape of the existing tests in `v3/mod.rs`, over the fully
/// populated snapshot those never build.
#[test]
fn a_fully_populated_snapshot_survives_plain_serde() {
    let original = full_snapshot();
    let json = serde_json::to_string(&original).expect("encodes");
    let back: SnapshotResponse = serde_json::from_str(&json).expect("decodes");
    assert_eq!(back, original, "the wire form lost or changed a field");

    // Named individually too, so a failure here says which field went missing rather
    // than just "the structs differ" -- releasability, covariance and classification
    // are the three the gap's own words call out.
    assert_eq!(
        back.tracks[0].releasability,
        original.tracks[0].releasability
    );
    assert_eq!(back.tracks[0].covariance, original.tracks[0].covariance);
    assert_eq!(
        back.tracks[0].classification,
        original.tracks[0].classification
    );
    assert_eq!(back.plan, original.plan);
    assert_eq!(back.requirements, original.requirements);
    assert_eq!(back.bearing_rays, original.bearing_rays);
    assert_eq!(back.pipeline_stats, original.pipeline_stats);
    assert_eq!(back.queue, original.queue);
    assert_eq!(
        back.plan.as_ref().map(|p| p.basis),
        Some(gungnir_model::PlanBasis::OneStep)
    );
    assert_eq!(back.plan_standing, original.plan_standing);
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

/// One HTTP/1.1 request over plain loopback: the status and the body. No client crate,
/// matching `party.rs`'s reason for hand-rolling its own TLS one.
async fn request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        let _ = write!(head, "Authorization: Bearer {token}\r\n");
    }
    let body = json_body.unwrap_or_default();
    if json_body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
        let _ = write!(head, "Content-Length: {}\r\n", body.len());
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    stream.write_all(body.as_bytes()).await.expect("write body");
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let response_body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, response_body)
}

async fn sign_in(addr: std::net::SocketAddr) -> String {
    let body = serde_json::json!({ "operator": OPERATOR, "passphrase": PASSPHRASE }).to_string();
    let (status, body) = request(addr, "POST", "/v3/session", None, Some(&body)).await;
    assert_eq!(status, 200, "sign-in failed: {body}");
    let value: serde_json::Value = serde_json::from_str(&body).expect("a session");
    value["token"].as_str().expect("a token").to_owned()
}

/// Through an operator's real `GET /v3/snapshot`, bearer-token authenticated, over the
/// transport `NodeApi` serves -- the second half the gap's action asks for.
#[tokio::test(flavor = "multi_thread")]
async fn a_fully_populated_snapshot_survives_an_operators_get() {
    let original = full_snapshot();

    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(OPERATOR),
        role: Role::Operator,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
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
    api.publish_snapshot(original.clone())
        .expect("the snapshot publishes");

    api.set_now(412.75);
    let addr = serve(Arc::clone(&api)).await;
    let token = sign_in(addr).await;
    let (status, body) = request(addr, "GET", "/v3/snapshot", Some(&token), None).await;
    assert_eq!(status, 200, "the authenticated GET was refused: {body}");
    let mut back: SnapshotResponse = serde_json::from_str(&body).expect("a snapshot");

    // GAP-140: the one field the route fills in on the way out, and the only one -- this
    // node's clock as it answered, which is what a desktop draws the queue's deadlines
    // against. Asserted and then set aside, so everything else is still compared field
    // for field against what was published.
    assert_eq!(
        back.node_time,
        Some(gungnir_model::MissionTime(412.75)),
        "the route did not stamp the node's own clock on the snapshot it answered with"
    );
    back.node_time = original.node_time;

    assert_eq!(
        back, original,
        "the snapshot changed crossing the wire to an authenticated operator"
    );

    // `/v2` is retired to `410 Gone` (GAP-130; `retired_v2.rs`), not a route that still
    // answers a snapshot -- named here so a reader does not go looking for it.
    let (status, _) = request(addr, "GET", "/v2/snapshot", Some(&token), None).await;
    assert_eq!(status, 410, "v2 no longer answers 410 Gone");
}
