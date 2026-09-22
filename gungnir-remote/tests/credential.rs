// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A link's credential replaced while its node is silent (GAP-143).
//!
//! A sign-in on a desktop during an outage must not replace the link the outage is
//! carrying, because that link holds what the desktop queued for the node while it was cut
//! off. So a sign-in replaces only who the link signs in as
//! ([`NodeLink::replace_credential`]), and this file shows two things about it against a
//! real node: the replacement is the credential the next sign-in uses, and what the link
//! was holding survives it.
//!
//! **Why the node knows only the new operator.** A node that accepted both would let the
//! link connect whichever credential it used, and the test would pass on a link that never
//! took the replacement. Knowing only operator 8 means the link gets in if and only if it
//! signs in as 8 -- and the first half of the test shows it being refused while it still
//! holds operator 7's credential, delivering nothing, which is what makes the second half's
//! delivery mean something.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_model::{DetectionView, Measurement, MissionTime, Provenance, SensorId, SystemHealth};
use gungnir_remote::link::{Credential, NodeLink};
use gungnir_remote::{connect_with_link, RemoteEndpoint};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use gungnir_tracking_service::TrackingService;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PASSPHRASE: &str = "correct horse battery staple";

fn credential(operator: u64) -> Credential {
    Credential {
        operator,
        passphrase: PASSPHRASE.into(),
    }
}

/// Submission `i`, carrying its place in the order as its source time.
#[allow(clippy::cast_precision_loss)]
fn detection(i: usize) -> DetectionView {
    let t = i as f64;
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

/// A node that authenticates exactly one operator, served on `listener` from now on.
fn serve_knowing(listener: tokio::net::TcpListener, operator: u64) -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(operator),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![3u8; 32], 300.0).expect("issuer");
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
    let node = Arc::clone(&api);
    tokio::spawn(async move {
        let _ = gungnir_api::transport::serve_on(listener, node).await;
    });
    api
}

/// The link's last recorded error, if it has one.
fn last_error(link: &NodeLink) -> Option<String> {
    link.read().and_then(|p| p.last_error.clone())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replaced_credential_is_the_next_sign_in_and_what_the_link_held_survives_it() {
    let listener = gungnir_api::transport::bind("127.0.0.1:0".parse().expect("addr"))
        .await
        .expect("bound");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let (mut tracking, _intercept, link) = connect_with_link(
        &RemoteEndpoint::plain(url),
        credential(7),
        &tokio::runtime::Handle::current(),
    )
    .expect("the link starts");

    // What a desktop queues for its node while cut off: nothing is serving the address yet.
    for i in 0..3 {
        tracking
            .submit_detection(detection(i))
            .expect("the outbox takes it");
    }
    assert_eq!(link.outbox_len(), 3);
    assert_eq!(link.signs_in_as(), Some(7));

    // The node answers, knowing only operator 8, and the link -- still operator 7's -- is
    // refused and delivers nothing. A deadlock guard, not a performance assertion.
    let api = serve_knowing(listener, 8);
    let started = Instant::now();
    while !last_error(&link).is_some_and(|e| e.contains("refused the sign-in")) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the link was never refused as operator 7; last error: {:?}",
            last_error(&link)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !link.connected(),
        "a node that knows only operator 8 let operator 7 in"
    );
    assert!(
        api.take_submissions().is_empty(),
        "the node received what the link held before anyone it knows signed in"
    );
    assert_eq!(link.outbox_len(), 3, "a refused sign-in cost the queue");

    // The desktop's operator changes during the outage: the link signs in as 8 from its
    // next attempt, and nothing it held is lost on the way.
    link.replace_credential(credential(8))
        .expect("an operator's link takes a new credential");
    assert_eq!(link.signs_in_as(), Some(8));
    let started = Instant::now();
    let mut received = Vec::new();
    while received.len() < 3 {
        received.extend(api.take_submissions());
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the node received {} of 3 after the credential was replaced; last error: {:?}",
            received.len(),
            last_error(&link)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(link.connected(), "delivered without being connected");
    let order: Vec<f64> = received.iter().map(|d| d.source_time.0).collect();
    assert_eq!(
        order,
        vec![0.0, 1.0, 2.0],
        "what the link held arrived out of order or incomplete"
    );
}
