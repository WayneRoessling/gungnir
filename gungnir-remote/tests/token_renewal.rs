// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A live link keeps its session with the node (GAP-165).
//!
//! A node-issued token always expires (DN-23 §5): after the baseline's session lifetime,
//! or the node's own 900 s where the baseline names none. The stream is authenticated
//! once, when it subscribes, so a link could stay up long after its token lapsed, and
//! until GAP-165 nothing asked for a new one: every write it made from then on was
//! refused `401`, and the desktop's detections waited in the outbox for as long as the
//! stream happened to stay open.
//!
//! Both halves of the fix are here, on the node's own clock rather than a long wait:
//!
//! - **The node refuses the token.** The node's clock (`NodeApi::set_now`) is moved past
//!   the token's expiry in one step; the next detection is refused, the link signs in
//!   again, and the same detection is delivered under the new token.
//! - **The link renews ahead of expiry.** A node whose tokens last two seconds of its own
//!   time: the link renews at three quarters of that, before the node has refused
//!   anything, and keeps doing so while it stays up.
//!
//! A real `axum` server on loopback and the real client, as `transport.rs` has them.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_model::{DetectionView, Measurement, MissionTime, Provenance, SensorId, SystemHealth};
use gungnir_remote::link::{Credential, NodeLink};
use gungnir_remote::{connect_with_link, RemoteEndpoint};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use std::sync::Arc;

const PASSPHRASE: &str = "correct horse battery staple";

/// A deadlock guard, never a pacing device: every wait leaves the moment its condition
/// holds, and a minute is long enough that a loaded runner cannot fail it.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(60);

/// A node whose tokens last `lifetime_s` of its own mission time, starting at zero.
fn node(lifetime_s: f64) -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![5u8; 32], lifetime_s).expect("issuer");
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
    api.set_now(0.0);
    api
}

async fn serve(api: Arc<NodeApi>) -> String {
    let listener = gungnir_api::transport::bind("127.0.0.1:0".parse().expect("addr"))
        .await
        .expect("bound");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    tokio::spawn(async move {
        let _ = gungnir_api::transport::serve_on(listener, api).await;
    });
    url
}

async fn link_to(url: String) -> NodeLink {
    let (_tracking, _intercept, link) = connect_with_link(
        &RemoteEndpoint::plain(url),
        Credential {
            operator: 7,
            passphrase: PASSPHRASE.into(),
        },
        &tokio::runtime::Handle::current(),
    )
    .expect("the link starts");
    until("the link to come up", || link.connected()).await;
    link
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + PATIENCE;
    while !check() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

fn detection(t: f64) -> DetectionView {
    DetectionView {
        sensor: SensorId(1),
        source_time: MissionTime(t),
        receipt_time: MissionTime(t),
        measurement: Measurement::Position {
            enu: nalgebra::Vector3::new(t, 0.0, 0.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

/// The node's clock passes the token's expiry; the link signs in again when the node
/// refuses it, and nothing it was carrying is lost.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_token_is_renewed_and_the_write_goes_through() {
    // An hour of node time: nothing renews ahead of expiry during this test.
    let api = node(3_600.0);
    let link = link_to(serve(Arc::clone(&api)).await).await;
    let first = link.token().expect("signed in");

    link.queue_outbound(detection(1.0));
    until("the first detection to reach the node", || {
        link.forwarded() == 1
    })
    .await;
    assert_eq!(api.take_submissions().len(), 1);

    // The node's clock moves past the token's expiry in one step.
    api.set_now(7_200.0);
    link.queue_outbound(detection(2.0));
    until(
        "the detection to reach the node under a renewed session",
        || link.forwarded() == 2,
    )
    .await;
    assert_eq!(
        api.take_submissions(),
        vec![detection(2.0)],
        "the detection refused under the lapsed token was not the one delivered"
    );
    assert_eq!(link.session_renewals(), 1, "renewed once, on the refusal");
    assert_ne!(
        link.token().expect("signed in"),
        first,
        "the link is still offering the token the node refused"
    );
    assert!(link.connected(), "the renewal took the link down");
    assert_eq!(link.outbox_len(), 0);
}

/// A node whose tokens last two seconds of its own time: the link renews ahead of the
/// expiry, before any request is refused, and keeps doing so.
#[tokio::test(flavor = "multi_thread")]
async fn a_live_link_renews_ahead_of_expiry() {
    let api = node(2.0);
    let link = link_to(serve(Arc::clone(&api)).await).await;
    until("the link to renew twice ahead of expiry", || {
        link.session_renewals() >= 2
    })
    .await;
    assert!(link.connected(), "the renewals took the link down");
    // And what it carries still goes through: every token it held was current.
    link.queue_outbound(detection(1.0));
    until("the detection to reach the node", || link.forwarded() == 1).await;
    assert_eq!(api.take_submissions(), vec![detection(1.0)]);
}

/// The renewal point is three quarters of the lifetime the node gave, on the node's own
/// clock, and there is none where the node says nothing a lifetime can be read from.
#[test]
fn the_renewal_point_is_read_off_the_nodes_clock() {
    use gungnir_remote::link::renew_after;
    assert_eq!(
        renew_after(1_000.0, Some(MissionTime(100.0))),
        Some(std::time::Duration::from_secs(675))
    );
    assert_eq!(
        renew_after(1_000.0, None),
        None,
        "a node that sends no time"
    );
    assert_eq!(
        renew_after(100.0, Some(MissionTime(100.0))),
        None,
        "a token already at its expiry has no lifetime to divide"
    );
    assert_eq!(renew_after(f64::NAN, Some(MissionTime(0.0))), None);
}
