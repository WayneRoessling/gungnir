// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A linked desktop's health is the node's, not the link's (GAP-161).
//!
//! The two remote services answered `is_healthy` with whether the link was up, so a node
//! whose tracker had stopped was drawn on every linked desktop's status strip as
//! tracking. The node said otherwise twice over -- in its snapshot's `health` and in the
//! `HealthEvent::Changed` it publishes on every transition -- and nothing on the desktop
//! read either. Each check below is one the old answer fails.
//!
//! A real `axum` server on loopback and the real client, as `transport.rs` has them; kept
//! in a file of its own so the question it answers is not buried among the transport's.

use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_eventing::{Envelope, Event};
use gungnir_intercept_service::InterceptService;
use gungnir_model::events::HealthEvent;
use gungnir_model::{MissionTime, SystemHealth};
use gungnir_remote::link::Credential;
use gungnir_remote::{connect_with_link, RemoteEndpoint};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use gungnir_tracking_service::TrackingService;
use std::sync::Arc;

const PASSPHRASE: &str = "correct horse battery staple";

/// A node reporting `health`, able to authenticate operator 7.
fn node(health: SystemHealth) -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![3u8; 32], 300.0).expect("issuer");
    Arc::new(
        NodeApi::new(SnapshotResponse::new(Vec::new(), None, health, Vec::new())).with_callers(
            Arc::new(AccountTokenAuthority::new(Box::new(store), issuer)),
        ),
    )
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

/// A deadlock guard, not a performance assertion: the loop leaves the moment the
/// condition holds, and a minute is long enough that a loaded runner cannot fail it.
async fn until(mut check: impl FnMut() -> bool, what: &str) {
    for _ in 0..2_400 {
        if check() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

fn changed(tracking_healthy: bool, intercept_healthy: bool, seq: u64) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(0.0),
        event: Event::Health(HealthEvent::Changed {
            tracking_healthy,
            intercept_healthy,
            ingest_healthy: true,
            at: MissionTime(0.0),
        }),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_desktop_reports_the_nodes_services_not_the_link() {
    // The node's tracker is down from the start; its planner is up.
    let api = node(SystemHealth {
        tracking_healthy: false,
        intercept_healthy: true,
        ingest_healthy: true,
    });
    let url = serve(Arc::clone(&api)).await;
    let handle = tokio::runtime::Handle::current();
    let (mut tracking, mut intercept, link) = connect_with_link(
        &RemoteEndpoint::plain(url),
        Credential {
            operator: 7,
            passphrase: PASSPHRASE.into(),
        },
        &handle,
    )
    .expect("the link starts");

    // Connected, and the snapshot's word stands: the tracker is not shown working.
    until(
        || {
            tracking.poll(MissionTime(0.0));
            let _ = intercept.plan(MissionTime(0.0), &[], &[]);
            link.connected() && intercept.is_healthy()
        },
        "the link to come up with the node's planner healthy",
    )
    .await;
    assert!(
        !tracking.is_healthy(),
        "a node reporting its tracker down was shown tracking because the link was up"
    );

    // The stream, not only the snapshot: the node's tracker recovers, then its planner
    // fails. Published once the stream has subscribed, for the reason
    // `gungnir-app/tests/failover_e2e.rs` gives: an envelope published before then is
    // never delivered.
    until(|| api.subscriber_count() >= 1, "the stream to subscribe").await;
    api.publish_event(changed(true, true, 1))
        .expect("published");
    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.is_healthy()
        },
        "the node's recovered tracker to reach the desktop",
    )
    .await;
    api.publish_event(changed(true, false, 2))
        .expect("published");
    until(
        || {
            let _ = intercept.plan(MissionTime(0.0), &[], &[]);
            !intercept.is_healthy()
        },
        "the node's failed planner to reach the desktop",
    )
    .await;
    tracking.poll(MissionTime(0.0));
    assert!(
        tracking.is_healthy(),
        "{:?}",
        link.read().map(|p| p.node_health)
    );
}
