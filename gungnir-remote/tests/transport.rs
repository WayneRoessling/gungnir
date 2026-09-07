// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The v2 transport end to end (GAP-041).
//!
//! A real `axum` server on a real loopback socket, driven by the real `reqwest` and
//! `tokio-tungstenite` client. Nothing here is a stub: the point of this file is that
//! the two halves of the transport agree about the contract, which no unit test on
//! either side can establish.
//!
//! **These are the tests that stand behind "the connected profile can be exercised".**
//! Before GAP-041 that claim rested on nothing, because `connect` returned an error.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v2::SnapshotResponse;
use gungnir_eventing::{Envelope, Event};
use gungnir_intercept_service::InterceptService;
use gungnir_model::events::{InterceptEvent, TrackingEvent};
use gungnir_model::{
    Classification, MissionTime, PlanView, Provenance, Quality, Releasability, SystemHealth,
    TrackId, TrackStatus, TrackView,
};
use gungnir_remote::link::Credential;
use gungnir_remote::{connect, RemoteEndpoint, RemoteError};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use gungnir_tracking_service::TrackingService;
use std::sync::Arc;

#[allow(clippy::cast_precision_loss)]
fn track(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(id as f64),
        releasability: Releasability::default(),
    }
}

fn snapshot(tracks: Vec<TrackView>) -> SnapshotResponse {
    SnapshotResponse::new(tracks, None, SystemHealth::default(), Vec::new())
}

const PASSPHRASE: &str = "correct horse battery staple";

/// The credential the tests sign in with.
fn credential() -> Credential {
    Credential {
        operator: 7,
        passphrase: PASSPHRASE.into(),
    }
}

/// A node that can authenticate one operator (GAP-057, DN-23 §6).
///
/// Every route but `POST /v2/session` needs a token, so a test that did not sign in
/// would be testing the refusal rather than the contract.
fn authenticating(snapshot: SnapshotResponse) -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![3u8; 32], 300.0).expect("issuer");
    Arc::new(
        NodeApi::new(snapshot).with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        ))),
    )
}

/// Start a server on an ephemeral loopback port and return its base URL.
///
/// Binding before spawning is deliberate: asking the operating system for the port after
/// handing the listener to a task would race it.
async fn serve(api: Arc<NodeApi>) -> String {
    // On the ambient test runtime, which lives as long as the test. `serve_cuttable`
    // below makes its own runtime precisely so it can be taken away; returning its
    // runtime here and dropping it would panic, because a runtime cannot be dropped
    // from inside an async context.
    let listener = gungnir_api::transport::bind("127.0.0.1:0".parse().expect("addr"))
        .await
        .expect("bound");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    tokio::spawn(async move {
        let _ = gungnir_api::transport::serve_on(listener, api).await;
    });
    url
}

/// A node on **its own runtime**, so a test can take the whole thing away (GAP-056).
///
/// Aborting the `serve_on` task is not enough and the difference matters: axum hands each
/// established connection to its own task, so cancelling the acceptor stops new
/// connections and leaves the open websocket alive and silent. A desktop then waits out
/// `HEARTBEAT_TIMEOUT` before deciding the link is gone -- which is correct behaviour for
/// a node that went quiet, and is not what "the transport was cut" means. Shutting the
/// runtime down drops every task on it, which is a node going down.
async fn serve_cuttable(api: Arc<NodeApi>) -> (String, tokio::runtime::Runtime) {
    let listener = gungnir_api::transport::bind("127.0.0.1:0".parse().expect("addr"))
        .await
        .expect("bound");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let node = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("node runtime");
    node.spawn(async move {
        let _ = gungnir_api::transport::serve_on(listener, api).await;
    });
    (url, node)
}

/// Wait for a condition, or fail with what was seen. Polling beats a fixed sleep: the
/// link is asynchronous and a sleep long enough to be reliable would be long enough to
/// slow the suite.
///
/// **A deadlock guard, not a performance assertion**, and the bound is set on the same
/// reasoning as `gungnir-tracking-service/tests/sample_set_replay.rs`. At 200 iterations
/// this was five seconds of wall clock. Every one of these fourteen waits completes in
/// about a second on an unloaded machine, so five seconds looks generous -- until a
/// shared CI runner is doing something else, where `a_deleted_track_leaves_the_projection`
/// timed out on a branch that touched nothing in this crate. A correctness test failing
/// for want of CPU says nothing about the transport.
///
/// Two thousand four hundred iterations is a minute. What is being tested is unchanged:
/// the condition still has to become true. Only the patience for a loaded machine
/// changes, and if this ever fires now it is a hang and not a slow runner. The loop exits
/// the moment the condition holds, so a passing run costs no more than it did.
const PATIENCE: usize = 2_400;

async fn until(mut check: impl FnMut() -> bool, what: &str) {
    for _ in 0..PATIENCE {
        if check() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// The whole point: a desktop connects to a node and gets the node's picture.
#[tokio::test(flavor = "multi_thread")]
async fn a_desktop_receives_the_nodes_picture() {
    let api = authenticating(snapshot(vec![track(1), track(2)]));
    let url = serve(Arc::clone(&api)).await;

    let handle = tokio::runtime::Handle::current();
    let (mut tracking, _intercept) =
        connect(&RemoteEndpoint::plain(url), credential(), &handle).expect("the link starts");

    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.is_healthy()
        },
        "the link to report connected",
    )
    .await;

    tracking.poll(MissionTime(0.0));
    assert_eq!(
        tracking.tracks().len(),
        2,
        "the snapshot's tracks did not reach the desktop"
    );
}

/// A track initiated on the node after the desktop connected reaches it over the event
/// stream, without another snapshot request.
#[tokio::test(flavor = "multi_thread")]
async fn events_published_after_connecting_reach_the_desktop() {
    let api = authenticating(snapshot(Vec::new()));
    let url = serve(Arc::clone(&api)).await;

    let handle = tokio::runtime::Handle::current();
    let (mut tracking, mut intercept) =
        connect(&RemoteEndpoint::plain(url), credential(), &handle).expect("the link starts");

    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.is_healthy()
        },
        "the link to report connected",
    )
    .await;

    // **Connected is not subscribed, and this test used to assume it was.** The link
    // reports itself healthy once the snapshot is answered, which happens before the
    // event stream has subscribed; a subscription from sequence zero means "everything
    // from now" by the v2 contract, so an envelope published in that window reaches
    // nobody -- correctly, and silently. The test then failed under load and passed
    // alone, which is the shape of a race and not of a transport fault.
    //
    // So wait for the stream to actually follow, by publishing throwaway envelopes until
    // one is seen. `common::until_following` does the same for the suites that have it;
    // this file does not include that module, so the loop is here.
    for seq in 1_000..1_100u64 {
        api.publish_event(Envelope {
            seq,
            mission_time: MissionTime(0.0),
            event: Event::Tracking(TrackingEvent::TrackInitiated(track(9_999))),
        })
        .expect("published");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        tracking.poll(MissionTime(0.0));
        if !tracking.tracks().is_empty() {
            break;
        }
    }
    assert!(
        !tracking.tracks().is_empty(),
        "the event stream never began following, so this test would have proved nothing"
    );
    // Clear the sentinel so the assertions below count only what they publish.
    api.publish_event(Envelope {
        seq: 1_100,
        mission_time: MissionTime(0.5),
        event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(9_999))),
    })
    .expect("published");
    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.tracks().is_empty()
        },
        "the sentinel track to be withdrawn",
    )
    .await;

    api.publish_event(Envelope {
        seq: 1_101,
        mission_time: MissionTime(1.0),
        event: Event::Tracking(TrackingEvent::TrackInitiated(track(7))),
    })
    .expect("published");
    api.publish_event(Envelope {
        seq: 1_102,
        mission_time: MissionTime(2.0),
        event: Event::Intercept(InterceptEvent::PlanProposed(PlanView::default())),
    })
    .expect("published");

    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.tracks().len() == 1
        },
        "the initiated track to arrive over the stream",
    )
    .await;
    assert_eq!(tracking.tracks()[0].id, TrackId(7));

    // The plan comes from the node; the desktop does not compute one while connected.
    let outcome = intercept.plan(MissionTime(0.0), &[], &[]);
    assert!(
        outcome.is_fresh(),
        "a connected desktop reported a stale plan: {outcome:?}"
    );
    assert_eq!(outcome.plan(), Some(&PlanView::default()));
    assert!(intercept.is_healthy());
}

/// A deleted track leaves the desktop's picture. A projection that only ever grew would
/// show an operator tracks the node had dropped.
#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_track_leaves_the_projection() {
    let api = authenticating(snapshot(vec![track(1)]));
    let url = serve(Arc::clone(&api)).await;

    let handle = tokio::runtime::Handle::current();
    let (mut tracking, _intercept) =
        connect(&RemoteEndpoint::plain(url), credential(), &handle).expect("the link starts");

    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.tracks().len() == 1
        },
        "the snapshot to arrive",
    )
    .await;

    api.publish_event(Envelope {
        seq: 1,
        mission_time: MissionTime(1.0),
        event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(1))),
    })
    .expect("published");

    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.tracks().is_empty()
        },
        "the deleted track to leave the projection",
    )
    .await;
}

/// A node that stops answering makes the desktop unhealthy rather than leaving a stale
/// picture labelled live. The picture is kept -- stale and labelled beats empty -- but
/// `is_healthy` is what the status strip reads.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_to_nothing_never_reports_healthy() {
    let handle = tokio::runtime::Handle::current();
    // Port 1 on loopback: nothing listens there.
    let (mut tracking, _intercept) = connect(
        &RemoteEndpoint::plain("http://127.0.0.1:1"),
        credential(),
        &handle,
    )
    .expect("the link starts");

    for _ in 0..8 {
        tracking.poll(MissionTime(0.0));
        assert!(
            !tracking.is_healthy(),
            "a link that reached nothing reported healthy"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(tracking.tracks().is_empty());
}

/// Sign in over HTTP and return the token, as any client must before anything else.
async fn token(url: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!("{url}/v2/session"))
        .json(&serde_json::json!({ "operator": 7, "passphrase": PASSPHRASE }))
        .send()
        .await
        .expect("the session route answers");
    assert!(
        response.status().is_success(),
        "sign-in failed: {:?}",
        response.status()
    );
    let body: serde_json::Value = response.json().await.expect("a session response");
    // A node-issued session always expires; a token without an expiry would be a
    // permanent credential handed out over the network.
    assert!(
        body["expires_s"].as_f64().is_some_and(|e| e > 0.0),
        "the node issued a token with no expiry: {body}"
    );
    body["token"].as_str().expect("a token").to_owned()
}

/// **Every route but the session one refuses an unauthenticated caller.** This is the
/// property that makes the write paths safe to serve at all: the caller is whoever the
/// token says, and `ApprovalRequest`'s own `operator` field is not believed.
#[tokio::test(flavor = "multi_thread")]
async fn every_other_route_refuses_without_a_token() {
    let url = serve(authenticating(snapshot(Vec::new()))).await;
    let client = reqwest::Client::new();

    for path in ["/v2/snapshot", "/v2/health", "/v2/session"] {
        let status = client
            .get(format!("{url}{path}"))
            .send()
            .await
            .expect("the route exists")
            .status();
        assert_eq!(
            status.as_u16(),
            401,
            "{path} served an unauthenticated caller"
        );
    }
    for path in ["/v2/detections", "/v2/plans/1/decision"] {
        let status = client
            .post(format!("{url}{path}"))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("the route exists")
            .status();
        assert_eq!(
            status.as_u16(),
            401,
            "{path} served an unauthenticated caller"
        );
    }

    // A forged token is refused with the same status and message as a missing one.
    let forged = client
        .get(format!("{url}/v2/snapshot"))
        .bearer_auth("deadbeef.deadbeef")
        .send()
        .await
        .expect("the route exists");
    assert_eq!(forged.status().as_u16(), 401);
}

/// A detection submitted by an authenticated caller is **queued**, not accepted: the
/// ingest gateway authenticates and validates it on its next tick like any sensor feed.
/// `202` says taken, not believed.
#[tokio::test(flavor = "multi_thread")]
async fn an_authenticated_caller_may_submit_a_detection() {
    let api = authenticating(snapshot(Vec::new()));
    let url = serve(Arc::clone(&api)).await;
    let token = token(&url).await;

    let detection = gungnir_model::DetectionView {
        sensor: gungnir_model::SensorId(1),
        source_time: MissionTime(1.0),
        receipt_time: MissionTime(1.0),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(1.0, 0.0, 0.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: gungnir_model::Provenance::default(),
    };
    let status = reqwest::Client::new()
        .post(format!("{url}/v2/detections"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            // Stated, because the node refuses a caller that does not say what
            // canonical model it speaks. This test was written before that guard
            // existed and was refused by it the moment it did, which is the guard
            // working on a real caller rather than a hypothetical one.
            "schema_version": gungnir_model::SCHEMA_VERSION,
            "detection": detection
        }))
        .send()
        .await
        .expect("the route exists")
        .status();
    assert_eq!(
        status.as_u16(),
        202,
        "an authenticated submission was refused"
    );

    // It reached the queue the gateway polls, and nothing else claimed to accept it.
    let queued = api.take_submissions();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].sensor, gungnir_model::SensorId(1));
    assert!(
        api.take_submissions().is_empty(),
        "the queue was not drained"
    );
}

/// The decision route still refuses, and **for a different reason than before**: this
/// node runs no approval queue. Inventing one in a request handler would put the
/// recommend-versus-act boundary in the transport.
#[tokio::test(flavor = "multi_thread")]
async fn the_decision_route_refuses_because_a_node_runs_no_queue() {
    let url = serve(authenticating(snapshot(Vec::new()))).await;
    let token = token(&url).await;

    let response = reqwest::Client::new()
        .post(format!("{url}/v2/plans/1/decision"))
        .bearer_auth(&token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("the route exists");
    assert_eq!(response.status().as_u16(), 501);
    let body = response.text().await.expect("body");
    assert!(body.contains("no approval queue"), "{body}");
    // The old reason was that nobody could be authenticated. That is no longer true and
    // must not still be the message.
    assert!(!body.contains("authenticate"), "{body}");
}

/// A node with no account store authenticates nobody and says so, rather than serving
/// its picture to anyone who asks. That is the default deployment.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_with_no_account_store_serves_nobody() {
    let url = serve(Arc::new(NodeApi::new(snapshot(vec![track(1)])))).await;

    let response = reqwest::get(format!("{url}/v2/snapshot"))
        .await
        .expect("the route exists");
    assert_eq!(response.status().as_u16(), 503);
    let body = response.text().await.expect("body");
    assert!(body.contains("authenticates nobody"), "{body}");
}

/// Only loopback is served. A node asked to bind a routable address must fail rather
/// than listen in plaintext: there is no TLS, and this is a command-and-control surface.
#[tokio::test(flavor = "multi_thread")]
async fn a_non_loopback_bind_is_refused() {
    let err = gungnir_api::transport::bind("0.0.0.0:0".parse().expect("addr"))
        .await
        .expect_err("refused");
    let message = err.to_string();
    assert!(message.contains("loopback"), "{message}");
    assert!(message.contains("GAP-060"), "{message}");
}

/// An https endpoint is refused by the client rather than quietly downgraded to
/// plaintext, which would be worse than not connecting at all.
#[test]
fn the_client_refuses_an_https_endpoint() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = connect(
        &RemoteEndpoint::plain("https://node.local:7410"),
        credential(),
        runtime.handle(),
    );
    match outcome {
        Err(RemoteError::InvalidEndpoint(reason)) => {
            assert!(reason.contains("GAP-060"), "{reason}");
            assert!(reason.contains("trust roots"), "{reason}");
        }
        Err(other) => panic!("refused for the wrong reason: {other}"),
        Ok(_) => panic!("an https endpoint with no pinned roots was accepted"),
    }
}

/// The health route answers without the whole snapshot, which is what a load balancer
/// or an operator's curl asks for.
#[tokio::test(flavor = "multi_thread")]
async fn the_health_route_answers() {
    let url = serve(authenticating(snapshot(Vec::new()))).await;

    let token = token(&url).await;
    let health: SystemHealth = reqwest::Client::new()
        .get(format!("{url}/v2/health"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("decode");
    // The default is every flag false, which is what an unwired node truthfully reports.
    assert!(!health.tracking_healthy);
}

/// `GET /v2/coverage` carries the parameters that found the gaps, not only the gaps.
///
/// DN-12 §6 wrote the response as a bare `Vec<CoverageGap>`; §5 puts the sampling spacing
/// and whether terrain masking was applied **on the result**, so a coarse run cannot be
/// mistaken for a fine one. A response carrying only the gaps would discard exactly what
/// that rule preserves.
#[tokio::test(flavor = "multi_thread")]
async fn the_coverage_route_carries_the_parameters_that_found_the_gaps() {
    let api = authenticating(snapshot(Vec::new()));
    api.publish_coverage(gungnir_api::v2::CoverageResponse::Computed(
        gungnir_analytics::CoverageReport {
            parameters: gungnir_analytics::CoverageParameters {
                sample_spacing_m: 250.0,
                terrain_masking_applied: false,
            },
            gaps: Vec::new(),
        },
    ))
    .expect("published");
    let url = serve(Arc::clone(&api)).await;
    let token = token(&url).await;

    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{url}/v2/coverage"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("the route exists")
        .json()
        .await
        .expect("decode");

    assert_eq!(body["state"], "computed");
    assert_eq!(body["parameters"]["sample_spacing_m"], 250.0);
    // A flat-terrain answer is optimistic by construction, and saying so is the point.
    assert_eq!(body["parameters"]["terrain_masking_applied"], false);
}

/// **A node that computed nothing says so, rather than returning an empty gap list.**
/// "No gaps were found" and "no coverage was computed" are opposite claims about a
/// sector, and an empty list would read as the clean one.
#[tokio::test(flavor = "multi_thread")]
async fn an_uncomputed_coverage_answer_is_not_an_empty_one() {
    let api = authenticating(snapshot(Vec::new()));
    let url = serve(Arc::clone(&api)).await;
    let token = token(&url).await;

    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{url}/v2/coverage"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("the route exists")
        .json()
        .await
        .expect("decode");

    assert_eq!(body["state"], "not-computed");
    assert!(
        body["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "a node that computed nothing gave no reason: {body}"
    );
    assert!(
        body.get("gaps").is_none(),
        "an empty gap list was returned: {body}"
    );
}

/// The coverage route is a read path and needs the same token as every other.
#[tokio::test(flavor = "multi_thread")]
async fn the_coverage_route_refuses_an_unauthenticated_caller() {
    let url = serve(authenticating(snapshot(Vec::new()))).await;
    let status = reqwest::get(format!("{url}/v2/coverage"))
        .await
        .expect("the route exists")
        .status();
    assert_eq!(status.as_u16(), 401);
}

/// **The connectivity test `docs/performance-budgets.md` names** (GAP-056): a node and a
/// desktop in one process, the transport cut, and what the desktop does next.
///
/// The budget is "fallback to embedded after link loss, under 2 s from the last successful
/// heartbeat". This does not assert the 2 s -- `benches/README.md` keeps absolute numbers
/// out of tests, and the heartbeat interval is 10 s, so the honest thing to measure here is
/// the *behaviour*: does the desktop notice, does it stop claiming healthy, and does it
/// hold what it was given rather than throwing it away.
#[tokio::test(flavor = "multi_thread")]
async fn cutting_the_transport_leaves_the_desktop_detached_and_honest() {
    let api = authenticating(snapshot(vec![track(1), track(2)]));
    let (url, node) = serve_cuttable(Arc::clone(&api)).await;

    let handle = tokio::runtime::Handle::current();
    let (mut tracking, mut intercept) =
        connect(&RemoteEndpoint::plain(url), credential(), &handle).expect("the link starts");

    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.is_healthy()
        },
        "the link to report connected",
    )
    .await;
    tracking.poll(MissionTime(0.0));
    assert_eq!(tracking.tracks().len(), 2);
    assert!(
        intercept.plan(MissionTime(0.0), &[], &[]).is_fresh(),
        "a connected desktop reported a stale plan"
    );
    // D-23: a connected link has been heard from, and recently.
    let heard = tracking
        .last_heard_age()
        .expect("a connected link has been heard from");
    assert!(
        heard < gungnir_api::transport::HEARTBEAT_INTERVAL * 2,
        "{heard:?}"
    );

    // Cut it: the node goes down, connection handlers and all.
    node.shutdown_background();
    let cut_at = std::time::Instant::now();

    until(
        || {
            tracking.poll(MissionTime(1.0));
            !tracking.is_healthy()
        },
        "the desktop to notice the link is gone",
    )
    .await;

    // **The picture is kept, not discarded.** An operator whose link drops still needs to
    // see what they last had; what must not happen is the desktop claiming it is live.
    assert_eq!(
        tracking.tracks().len(),
        2,
        "the last picture was thrown away when the link dropped"
    );
    assert!(!tracking.is_healthy(), "a detached link reported healthy");

    // D-23: the age keeps growing after the cut, so the strip can say *how* stale the
    // picture is rather than only that the light went out.
    let age = tracking
        .last_heard_age()
        .expect("the last time the node was heard is kept after the cut");
    assert!(
        age >= cut_at
            .elapsed()
            .saturating_sub(std::time::Duration::from_millis(50))
    );

    // And the plan says it is old rather than passing for current (GAP-066).
    let outcome = intercept.plan(MissionTime(2.0), &[], &[]);
    assert!(
        !outcome.is_fresh(),
        "a detached desktop reported a fresh plan: {outcome:?}"
    );

    // Detections submitted while detached are queued, not dropped: that is the outbox the
    // store-and-forward budget is about.
    let before = tracking.outbox_len();
    tracking
        .submit_detection(gungnir_model::DetectionView {
            sensor: gungnir_model::SensorId(1),
            source_time: MissionTime(2.0),
            receipt_time: MissionTime(2.0),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::zeros(),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        })
        .expect("the outbox took it");
    assert_eq!(
        tracking.outbox_len(),
        before + 1,
        "a detection submitted while detached was not queued"
    );
}

/// **The connectivity budget, as amended by D-23, is met -- and the relationship is pinned
/// rather than the numbers** (GAP-056).
///
/// The budget used to read "fallback under 2 s from the last successful heartbeat" beside a
/// 10 s beat and an independent 35 s timeout: the case it existed for -- a node silent with
/// the socket open -- took 35 s. D-23 split the budget into what an operator can *see* and
/// what the desktop *declares*: the strip shows "heard N s ago" against a 2 s beat, so
/// staleness is visible within one beat; the desktop declares the link gone within four
/// beats, tolerating three misses so a single stall on an intermittent link does not cost a
/// full reconnect. The timeout is now derived from the beat, which is what makes this a
/// property of the design and not a coincidence of two constants.
#[test]
fn the_heartbeat_meets_the_amended_connectivity_budget() {
    use gungnir_api::transport::{
        HEARTBEAT_INTERVAL, HEARTBEAT_MISSES_TOLERATED, HEARTBEAT_TIMEOUT,
    };

    /// D-23: staleness visible within this of the last beat.
    const VISIBLE_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);
    /// D-23: declared gone within this many beats.
    const DECLARED_WITHIN_BEATS: u32 = 4;
    // An invariant of the design rather than a measurement, so it holds at compile time:
    // one tolerated miss would drop a healthy link on a single slow beat.
    const _: () = assert!(
        HEARTBEAT_MISSES_TOLERATED >= 2,
        "one missed beat would drop a healthy link and force a reconnect"
    );

    assert!(
        HEARTBEAT_INTERVAL <= VISIBLE_WITHIN,
        "a beat slower than the visibility budget"
    );
    assert!(
        HEARTBEAT_TIMEOUT <= HEARTBEAT_INTERVAL * DECLARED_WITHIN_BEATS,
        "declaration takes more than {DECLARED_WITHIN_BEATS} beats: {HEARTBEAT_TIMEOUT:?}"
    );
    assert!(HEARTBEAT_TIMEOUT > HEARTBEAT_INTERVAL);
}

/// `GET /v2/history` (GAP-050): the retained envelopes from a sequence, under the link's
/// token, and `410 Gone` past the window rather than a shorter list.
#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::cast_precision_loss)]
async fn the_history_route_serves_the_window_and_says_gone_past_it() {
    let api = authenticating(snapshot(Vec::new()));
    let url = serve(Arc::clone(&api)).await;
    for seq in 1..=5 {
        api.publish_event(Envelope {
            seq,
            mission_time: MissionTime(seq as f64),
            event: Event::Tracking(TrackingEvent::TrackInitiated(track(seq))),
        })
        .expect("published");
    }
    let token = token(&url).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint::plain(url.clone());
    let pending =
        gungnir_remote::link::fetch_history(&endpoint, &token, 3, &handle).expect("starts");
    let mut outcome = None;
    until(
        || {
            outcome = pending.poll();
            outcome.is_some()
        },
        "the history to arrive",
    )
    .await;
    match outcome {
        Some(gungnir_remote::link::HistoryOutcome::Complete(envelopes)) => {
            assert_eq!(
                envelopes.iter().map(|e| e.seq).collect::<Vec<_>>(),
                vec![3, 4, 5]
            );
        }
        other => panic!("{other:?}"),
    }

    // Past the window: gone, with the reason.
    for seq in 6..=(gungnir_api::transport::BACKLOG_CAPACITY as u64 + 10) {
        api.publish_event(Envelope {
            seq,
            mission_time: MissionTime(seq as f64),
            event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
        })
        .expect("published");
    }
    let pending =
        gungnir_remote::link::fetch_history(&endpoint, &token, 3, &handle).expect("starts");
    let mut outcome = None;
    until(
        || {
            outcome = pending.poll();
            outcome.is_some()
        },
        "the refusal to arrive",
    )
    .await;
    assert!(
        matches!(
            outcome,
            Some(gungnir_remote::link::HistoryOutcome::Gone { .. })
        ),
        "{outcome:?}"
    );

    // Without a token: refused like every other route.
    let pending = gungnir_remote::link::fetch_history(&endpoint, "", 3, &handle).expect("starts");
    let mut outcome = None;
    until(
        || {
            outcome = pending.poll();
            outcome.is_some()
        },
        "the refusal to arrive",
    )
    .await;
    assert!(
        matches!(
            outcome,
            Some(gungnir_remote::link::HistoryOutcome::Unreachable { .. })
        ),
        "{outcome:?}"
    );
}

/// Store-and-forward end to end (`ARCHITECTURE.md` §8.4, GAP-050): a detection
/// submitted to the linked service is posted to the node under the link's token, and
/// leaves the outbox only when the node has accepted it.
#[tokio::test(flavor = "multi_thread")]
async fn a_detection_submitted_while_linked_reaches_the_node() {
    let api = authenticating(snapshot(Vec::new()));
    let url = serve(Arc::clone(&api)).await;
    let handle = tokio::runtime::Handle::current();
    let (mut tracking, _intercept) =
        connect(&RemoteEndpoint::plain(url), credential(), &handle).expect("the link starts");
    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.is_healthy()
        },
        "the link to report connected",
    )
    .await;
    let detection = gungnir_model::DetectionView {
        sensor: gungnir_model::SensorId(3),
        source_time: MissionTime(2.0),
        receipt_time: MissionTime(2.0),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(4.0, 5.0, 6.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: gungnir_model::Provenance::default(),
    };
    tracking.submit_detection(detection).expect("queued");
    assert_eq!(tracking.outbox_len(), 1);
    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.forwarded() == 1
        },
        "the node to accept the detection",
    )
    .await;
    assert_eq!(tracking.outbox_len(), 0, "accepted, so no longer queued");
    let queued = api.take_submissions();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].sensor, gungnir_model::SensorId(3));
}
