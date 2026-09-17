// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The `gungnir-remote` Store-and-forward and honest connection state row of
//! `docs/verification-capability-table.md` §2: "Outbox holds submissions in order up to
//! `OUTBOX_CAPACITY`; `is_healthy` false until connected (tested)".
//!
//! **Order and capacity were never asserted before this file.** The outbox tests in
//! `src/lib.rs` and `tests/transport.rs` submit one or two detections, so an outbox that
//! dropped its newest detection instead of its oldest, reordered, or grew without bound
//! would pass every one of them. Here an outbox is filled past its capacity by [`K`]
//! detections carrying increasing source times, so a detection's source time says where in
//! the submission order it came from, and what is held is read back against that: the
//! newest `OUTBOX_CAPACITY`, oldest first, with exactly `K` counted as dropped.
//!
//! There are two outboxes and both are filled. A detached `RemoteTrackingService` holds its
//! own, which `drain_outbox` empties; a linked one hands every detection to its `NodeLink`,
//! whose outbox the link task forwards to the node. The linked outbox is filled while no
//! node answers, and then a node comes up and what it receives is read back the same way.
//!
//! # What forwarding a full outbox costs, and what every run forwards instead
//!
//! The link forwards on a fixed cadence: at most 64 detections per 250 ms tick
//! (`FORWARD_INTERVAL` and `flush_outbox` in `src/link.rs`), which is 256 a second however
//! fast the node answers. A full outbox of 100,000 therefore takes 390.6 s to reach the
//! node, and on 2026-09-16 it took 390.8 s in a debug build, every detection in order.
//! That run is `a_full_linked_outbox_reaches_the_node_whole_and_in_order`, `#[ignore]`d for
//! its running time and nothing else. Every run forwards [`FORWARDED_IN_EVERY_RUN`] instead, in
//! `a_linked_outbox_holds_the_newest_in_order_and_forwards_the_oldest_first`: the whole
//! outbox is still held and read back, and the forwarded part crosses the boundary between
//! two of the link's batches at least fifteen times.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_model::{DetectionView, Measurement, MissionTime, Provenance, SensorId, SystemHealth};
use gungnir_remote::link::{Credential, NodeLink};
use gungnir_remote::{connect_with_link, RemoteEndpoint, RemoteTrackingService, OUTBOX_CAPACITY};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use gungnir_tracking_service::TrackingService;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many detections past capacity each outbox is given: small, so the dropped count is
/// read at a glance, and more than one, so "the oldest is dropped" is shown to repeat.
const K: usize = 7;

/// How many detections every run has the node receive from a full linked outbox: sixteen
/// of the link's 64-detection batches, about four seconds at its cadence.
const FORWARDED_IN_EVERY_RUN: usize = 1_024;

/// Submission `i`: source and receipt time `i` seconds, and a position that says the same,
/// so its place in the submission order survives the wire and can be read back.
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

/// The first place `held` departs from the newest `OUTBOX_CAPACITY` submissions, oldest
/// first: the offset and the source time found there. `None` when it does not depart.
///
/// Every held detection is compared whole with the one submitted at its place, `K +
/// offset`, which is what "the newest, in order" means once the oldest `K` are gone.
/// Reported as the first departure rather than an `assert_eq!` over the whole sequence,
/// because a failure message a hundred thousand detections long says less than one line.
fn first_departure<'a>(
    held: impl IntoIterator<Item = &'a DetectionView>,
) -> Option<(usize, MissionTime)> {
    held.into_iter()
        .enumerate()
        .find(|(offset, d)| **d != detection(K + offset))
        .map(|(offset, d)| (offset, d.source_time))
}

/// The detached half: a service that has never connected holds what it is given, up to
/// `OUTBOX_CAPACITY`, and `drain_outbox` hands back the newest of it in submission order.
#[test]
fn a_detached_outbox_holds_the_newest_submissions_in_order_up_to_capacity() {
    let mut svc = RemoteTrackingService::detached(RemoteEndpoint::plain("http://node.local:7410"));
    for i in 0..OUTBOX_CAPACITY + K {
        svc.submit_detection(detection(i))
            .expect("the outbox takes every detection, dropping the oldest when full");
    }

    assert!(
        !svc.is_healthy(),
        "a service that has never connected reported healthy"
    );
    assert_eq!(
        svc.outbox_len(),
        OUTBOX_CAPACITY,
        "the outbox grew past its capacity, or lost more than the overflow"
    );
    assert_eq!(
        svc.dropped(),
        K as u64,
        "every detection past capacity is one drop, counted"
    );

    let drained = svc.drain_outbox();
    assert_eq!(drained.len(), OUTBOX_CAPACITY);
    assert_eq!(
        first_departure(&drained),
        None,
        "the drained outbox is not the newest {OUTBOX_CAPACITY} submissions in the order \
         they were submitted (offset, source time found there)"
    );
    assert_eq!(svc.outbox_len(), 0, "drained means empty");
    assert!(!svc.is_healthy(), "draining is not connecting");
}

const PASSPHRASE: &str = "correct horse battery staple";

/// The credential the link signs in with, as `tests/transport.rs` does.
fn credential() -> Credential {
    Credential {
        operator: 7,
        passphrase: PASSPHRASE.into(),
    }
}

/// A node that can authenticate that one operator (GAP-057, DN-23 §6), built the way
/// `tests/transport.rs` builds its own, served on `listener` from now on.
fn serve(listener: tokio::net::TcpListener) -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
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

/// A linked service whose node has not answered, its outbox filled past capacity and read
/// back: the listener its node is to be served on, the service, and its link.
///
/// **No node answers** because the node's address is bound and not yet served: a
/// connection is taken by the operating system and no response ever comes, so the link
/// cannot sign in, receives no snapshot, and stays unconnected. Binding first rather than
/// leaving the port closed is what lets the node come up on the address the link was given
/// without racing anything else on the machine for that port.
async fn a_full_link_whose_node_has_not_answered(
) -> (tokio::net::TcpListener, RemoteTrackingService, NodeLink) {
    let listener = gungnir_api::transport::bind("127.0.0.1:0".parse().expect("addr"))
        .await
        .expect("bound");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let (mut tracking, _intercept, link) = connect_with_link(
        &RemoteEndpoint::plain(url),
        credential(),
        &tokio::runtime::Handle::current(),
    )
    .expect("the link starts");

    for i in 0..OUTBOX_CAPACITY + K {
        tracking
            .submit_detection(detection(i))
            .expect("the link's outbox takes every detection, dropping the oldest when full");
    }
    tracking.poll(MissionTime(0.0));
    assert!(
        !tracking.is_healthy(),
        "a link no node has answered reported healthy"
    );
    assert_eq!(link.outbox_len(), OUTBOX_CAPACITY);
    assert_eq!(link.dropped(), K as u64);
    assert_eq!(
        link.forwarded(),
        0,
        "no node has answered, so nothing was forwarded"
    );
    // The service reports the link's outbox rather than one of its own.
    assert_eq!(tracking.outbox_len(), OUTBOX_CAPACITY);
    assert_eq!(tracking.dropped(), K as u64);
    {
        let projection = link.read().expect("the projection");
        assert_eq!(
            first_departure(&projection.outbox),
            None,
            "the link's outbox is not the newest {OUTBOX_CAPACITY} submissions in the order \
             they were submitted (offset, source time found there)"
        );
    }
    (listener, tracking, link)
}

/// Take what the node receives until it holds at least `at_least`, and fail rather than
/// wait past `patience`, which is a deadlock guard and not a performance assertion.
/// Progress is reported every thirty seconds, so a slow run says how far it got.
async fn receive(
    api: &NodeApi,
    link: &NodeLink,
    at_least: usize,
    patience: Duration,
) -> Vec<DetectionView> {
    let started = Instant::now();
    let mut last_report = started;
    let mut received = Vec::with_capacity(at_least);
    loop {
        received.extend(api.take_submissions());
        if received.len() >= at_least {
            eprintln!(
                "the node had received {} {:.1} s after it came up",
                received.len(),
                started.elapsed().as_secs_f64()
            );
            return received;
        }
        assert!(
            started.elapsed() < patience,
            "the node had received {} of {at_least} after {:.0} s (forwarded {}, still \
             queued {})",
            received.len(),
            started.elapsed().as_secs_f64(),
            link.forwarded(),
            link.outbox_len()
        );
        if last_report.elapsed() >= Duration::from_secs(30) {
            eprintln!(
                "the node has received {} of {at_least}, {:.0} s after it came up",
                received.len(),
                started.elapsed().as_secs_f64()
            );
            last_report = Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The linked half, as every run checks it: the link's own outbox holds the newest
/// `OUTBOX_CAPACITY` in order while no node answers, and once one does, what reaches it
/// first is the oldest of those, in order.
#[tokio::test(flavor = "multi_thread")]
async fn a_linked_outbox_holds_the_newest_in_order_and_forwards_the_oldest_first() {
    let (listener, mut tracking, link) = a_full_link_whose_node_has_not_answered().await;
    let api = serve(listener);

    let received = receive(&api, &link, FORWARDED_IN_EVERY_RUN, Duration::from_mins(5)).await;
    assert_eq!(
        first_departure(&received),
        None,
        "what reached the node first is not the oldest of the outbox in submission order \
         (offset, source time found there)"
    );
    tracking.poll(MissionTime(0.0));
    assert!(
        tracking.is_healthy(),
        "a link whose node has answered did not report connected"
    );
}

/// The linked half whole: every detection the outbox held reaches the node, in order, and
/// nothing more. **`#[ignore]`d for its running time and nothing else**; the module
/// documentation gives the cadence that sets it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "about 390 s: the link forwards 64 detections per 250 ms tick; run with --ignored"]
async fn a_full_linked_outbox_reaches_the_node_whole_and_in_order() {
    let (listener, mut tracking, link) = a_full_link_whose_node_has_not_answered().await;
    let api = serve(listener);

    let mut received = receive(&api, &link, OUTBOX_CAPACITY, Duration::from_mins(30)).await;
    // The last `202` may still be on its way back to the link, so wait for the link to have
    // counted every one before asking whether anything more arrived.
    let started = Instant::now();
    while link.forwarded() < OUTBOX_CAPACITY as u64 || link.outbox_len() > 0 {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the node holds the whole outbox and the link counted {} forwarded, {} still \
             queued",
            link.forwarded(),
            link.outbox_len()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    received.extend(api.take_submissions());

    assert_eq!(
        received.len(),
        OUTBOX_CAPACITY,
        "the node received more or fewer detections than the outbox held"
    );
    assert_eq!(
        first_departure(&received),
        None,
        "the node did not receive the newest {OUTBOX_CAPACITY} submissions in the order they \
         were submitted (offset, source time found there)"
    );
    assert_eq!(link.dropped(), K as u64, "forwarding dropped nothing more");
    tracking.poll(MissionTime(0.0));
    assert!(
        tracking.is_healthy(),
        "a link whose node answered every post did not report connected"
    );
}
