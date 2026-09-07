// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A peer's launch warning across a real wire (GAP-009, `docs/design/DN-16-peer-sources.md`
//! §5).
//!
//! DN-16 §5 makes a launch warning **a distinct message rather than a track**, "because a
//! warning is a statement about the future with no kinematic state", and it "never creates
//! a track: a track we have not observed is a track we cannot maintain". DN-16 §8's pass
//! criterion for CAP-1.6 asks that "a launch warning creates an alert and no track", to be
//! shown with "two nodes in one test process, one feeding the other".
//!
//! That is what this file is. One node publishes a launch warning on its v2 event stream;
//! a second host takes it over mutual TLS through the machine link every peer uses, and
//! what it receives is a launch warning and not a track. The alert half is not here: it
//! belongs to the hosts, which have no `NodeApi`, and is checked where the stamping
//! happens (`gungnir_ingest::adapters::peer`) and where the alert list is
//! (`gungnir-app/src/peers.rs`).
//!
//! What is **not** shown: nothing in this workspace issues a launch warning of its own, so
//! the publishing node here stands in for a peer that does. That is the honest state of
//! GAP-009's outbound half and no producer was invented to hide it.

mod common;

use common::{until, until_following, Pki};
use gungnir_api::transport::NodeApi;
use gungnir_api::v2::SnapshotResponse;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::{LaunchWarningEvent, TrackingEvent};
use gungnir_model::{
    Classification, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet,
    LaunchWarningReport, MissionTime, PeerLaunchWarning, Provenance, Quality, Releasability,
    SystemHealth, TrackId, TrackStatus, TrackView,
};
use gungnir_remote::peer::PeerLink;
use gungnir_remote::RemoteEndpoint;
use std::sync::Arc;

fn track(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::AllPeers,
    }
}

fn warning() -> LaunchWarningReport {
    LaunchWarningReport {
        id: "LW-2291".into(),
        what: "ballistic launch, bearing 040, from the northern range".into(),
        at: MissionTime(120.0),
        releasability: Releasability::AllPeers,
    }
}

/// A node serving `partner` under an agreement listing `outbound`.
fn api(outbound: Vec<ExchangeItem>) -> Arc<NodeApi> {
    let snapshot = SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new());
    Arc::new(NodeApi::new(snapshot).with_exchange(ExchangeSet {
        agreements: vec![ExchangeAgreement {
            party: "partner".into(),
            inbound: Vec::new(),
            outbound,
            format: ExchangeFormat::Canonical,
        }],
    }))
}

fn envelope(seq: u64, event: Event) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(120.0),
        event,
    }
}

/// The sentinel every test here follows the stream with: a track the agreement always
/// releases, republished until one comes back.
fn sentinel(seq: u64) -> Envelope {
    envelope(
        seq,
        Event::Tracking(TrackingEvent::TrackInitiated(track(1))),
    )
}

/// A linked partner whose stream is known to be following, and the next free sequence.
async fn linked(pki: &Pki, node: &Arc<NodeApi>) -> (PeerLink, u64) {
    let url = pki.serve(Arc::clone(node)).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("partner"),
    };
    let peer = PeerLink::connect(&endpoint, &handle).expect("the peer link starts");
    until(|| peer.connected(), "the partner's link to come up").await;
    let next = until_following(node, sentinel, || !peer.take_tracks().is_empty()).await;
    (peer, next)
}

/// The criterion DN-16 §8 states: a launch warning arrives, and no track comes with it.
///
/// The partner's link holds tracks and launch warnings in two queues and hands them over
/// through two calls, so "no track appeared" is not a matter of reading the right field
/// out of a shared list -- there is no shared list.
#[tokio::test(flavor = "multi_thread")]
async fn a_launch_warning_crosses_the_wire_as_a_distinct_message_and_makes_no_track() {
    let pki = Pki::new("launch-warning");
    let node = api(vec![ExchangeItem::Tracks, ExchangeItem::Warnings]);
    let (peer, seq) = linked(&pki, &node).await;

    node.publish_event(envelope(
        seq,
        Event::LaunchWarning(LaunchWarningEvent::Issued(warning())),
    ))
    .expect("the node publishes the warning");

    let mut warnings = Vec::new();
    until(
        || {
            warnings.extend(peer.take_launch_warnings());
            !warnings.is_empty()
        },
        "the launch warning to reach the partner",
    )
    .await;
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0], warning(), "carried across the wire unchanged");
    assert_eq!(
        peer.launch_warnings_dropped(),
        0,
        "nothing was dropped from the queue"
    );
    assert!(
        peer.take_tracks().is_empty(),
        "a launch warning must not create a track: DN-16 §5"
    );
    assert!(
        peer.take_launch_warnings().is_empty(),
        "handed over once, not on every poll"
    );
}

/// The agreement gates a launch warning like everything else (DN-18 §5): a partner whose
/// agreement does not list warnings gets tracks and never a warning. The track published
/// after it proves the stream was alive, so the absence is the gate and not a race.
#[tokio::test(flavor = "multi_thread")]
async fn a_partner_whose_agreement_omits_warnings_never_receives_one() {
    let pki = Pki::new("launch-warning-gate");
    let node = api(vec![ExchangeItem::Tracks]);
    let (peer, seq) = linked(&pki, &node).await;

    node.publish_event(envelope(
        seq,
        Event::LaunchWarning(LaunchWarningEvent::Issued(warning())),
    ))
    .expect("published");
    node.publish_event(envelope(
        seq + 1,
        Event::Tracking(TrackingEvent::TrackInitiated(track(2))),
    ))
    .expect("published");

    let mut later = Vec::new();
    until(
        || {
            later.extend(peer.take_tracks());
            later.iter().any(|t| t.id == TrackId(2))
        },
        "the track published after the warning to reach the partner",
    )
    .await;
    assert!(
        peer.take_launch_warnings().is_empty(),
        "the agreement does not send warnings, so none crossed"
    );
}

/// An unmarked launch warning does not leave the deployment, even under an agreement
/// that lists warnings. DN-18 §5's two gates with the restrictive one deciding: the
/// marking defaults to `Internal`, and an agreement does not override it.
#[tokio::test(flavor = "multi_thread")]
async fn an_internally_marked_launch_warning_is_withheld_from_a_permitted_partner() {
    let pki = Pki::new("launch-warning-marking");
    let node = api(vec![ExchangeItem::Tracks, ExchangeItem::Warnings]);
    let (peer, seq) = linked(&pki, &node).await;

    node.publish_event(envelope(
        seq,
        Event::LaunchWarning(LaunchWarningEvent::Issued(LaunchWarningReport {
            releasability: Releasability::Internal,
            ..warning()
        })),
    ))
    .expect("published");
    node.publish_event(envelope(
        seq + 1,
        Event::Tracking(TrackingEvent::TrackInitiated(track(2))),
    ))
    .expect("published");

    until(
        || peer.take_tracks().iter().any(|t| t.id == TrackId(2)),
        "the track published after the warning to reach the partner",
    )
    .await;
    assert!(
        peer.take_launch_warnings().is_empty(),
        "an agreement permitting warnings does not override an internal marking"
    );
}

/// This deployment's record of what *its* peers told it is not forwarded onward.
///
/// `Received` and `Refused` name our peers and our judgement of them; a partner reading
/// those off our stream would learn who we listen to. Only `Issued` -- what this
/// deployment says itself -- may cross.
#[tokio::test(flavor = "multi_thread")]
async fn a_partner_does_not_read_our_record_of_other_peers() {
    let pki = Pki::new("launch-warning-record");
    let node = api(vec![ExchangeItem::Tracks, ExchangeItem::Warnings]);
    let (peer, seq) = linked(&pki, &node).await;

    node.publish_event(envelope(
        seq,
        Event::LaunchWarning(LaunchWarningEvent::Received(PeerLaunchWarning {
            peer: "some-other-cell".into(),
            report: warning(),
            receipt_time: MissionTime(121.0),
        })),
    ))
    .expect("published");
    node.publish_event(envelope(
        seq + 1,
        Event::LaunchWarning(LaunchWarningEvent::Refused {
            peer: "some-other-cell".into(),
            reason: "the launch warning says nothing".into(),
            at: MissionTime(121.0),
        }),
    ))
    .expect("published");
    node.publish_event(envelope(
        seq + 2,
        Event::Tracking(TrackingEvent::TrackInitiated(track(2))),
    ))
    .expect("published");

    until(
        || peer.take_tracks().iter().any(|t| t.id == TrackId(2)),
        "the track published after the two records to reach the partner",
    )
    .await;
    assert!(
        peer.take_launch_warnings().is_empty(),
        "our record of another peer is ours, not the partner's"
    );
}
