//! The interface conformance suite across a real wire (GAP-063;
//! `docs/verification-capability-table.md` §2, the cross-layer interop row).
//!
//! `gungnir-interop/tests/conformance.rs` walks the schema catalogue entry by entry and
//! checks each **in memory**, against corpora in the repository. That is the whole suite
//! for a format read from a file or a socket the deployment owns. It is not the whole
//! suite for a schema two deployments speak to each other: a type can round-trip
//! perfectly through `serde_json` in one process and still lose something crossing a
//! transport that re-encodes, filters, or truncates it. GAP-063's remaining action asks
//! for the suite "against a peer over the wire once GAP-065 has one", and GAP-065 now
//! has the mutual-TLS machine link.
//!
//! # Which criteria are now covered over the wire
//!
//! The §2 row asks four things. What this file adds, and only for the exchange items
//! that have a producer today:
//!
//! | Criterion | Over the wire here |
//! |---|---|
//! | Zero-loss round-trip of a catalogued schema | **Yes, for `gungnir.TrackView` and `gungnir.Envelope`**, both doors: the snapshot and the event stream, against the committed test-track corpus, compared as canonical JSON bytes and not only as values |
//! | Zero-loss round-trip of the health payload | **Yes**, `GET /v2/health` as a machine caller under an agreement listing `Health` |
//! | An unsupported version is refused with the catalogue's error | **Yes**: a node declaring a schema version this build does not speak is refused whole, and the link says which two versions disagreed |
//! | Public-specification samples decode partially, and nothing panics on the fuzz corpus | **No, and not applicable**: ASTERIX, AIS and STANAG are read from feeds and files, not from this transport, and stay covered in memory |
//!
//! # Which are still not covered, and why
//!
//! * **`gungnir.DetectionView`** has a wire door (`POST /v2/detections`) and no check
//!   here. It is a write path and an operator's, not a peer's; it is exercised for
//!   delivery by `transport.rs` and not for byte parity.
//! * **`gungnir.PlanView`** crosses no wire to a peer at all: a plan is a recommendation
//!   for this deployment's own effectors and `NodeApi::snapshot_for` withholds it from
//!   every party by design (DN-18 §5).
//! * **`gungnir.detections.arrow`, `gungnir.ml.classification-rows`,
//!   `gungnir.GlobalEntityId`, `asterix.cat048`, `asterix.cat034`, `ais.m1371`** have no
//!   producer on this transport.
//! * **`stanag.4676`** has no corpus and no decoder at all (D-09), which is the other
//!   half of GAP-063 and is still blocked on an owner decision.
//!
//! `gungnir-interop/tests/conformance.rs` states the same table entry by entry, so a new
//! catalogue entry fails there until somebody says which of these it is. This file
//! cannot walk the catalogue itself: `gungnir-remote` does not depend on
//! `gungnir-interop` and `ARCHITECTURE.md` §7.1 draws no such edge.

mod common;

use common::{until, until_following, Pki};
use gungnir_api::transport::NodeApi;
use gungnir_api::v2::SnapshotResponse;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::TrackingEvent;
use gungnir_model::{
    Classification, DetectionView, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet,
    MissionTime, Quality, Releasability, SystemHealth, TrackId, TrackStatus, TrackView,
    SCHEMA_VERSION,
};
use gungnir_remote::peer::PeerLink;
use gungnir_remote::{LinkTls, RemoteEndpoint};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How many corpus tracks cross the wire.
///
/// The in-memory suite uses the whole corpus, which is over a thousand detections; this
/// one is bounded because every track here is serialised, pushed through TLS and a
/// WebSocket, and compared. The value of the wire check is that the transport is real,
/// not that the corpus is large -- and the corpus is walked in full where that is cheap.
const WIRE_SAMPLE: usize = 120;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Tracks built from the committed test-track detections
/// (`testdata/tracks/samples/*/detections.jsonl`), which is the same corpus the
/// in-memory suite reads.
///
/// Built from real detections rather than written by hand, so the provenance, the
/// timestamps and the measurement values are the ones a generator produced and not the
/// handful a test author would have thought to vary.
fn corpus() -> Vec<TrackView> {
    let mut out = Vec::new();
    let samples = std::fs::read_dir(root().join("testdata/tracks/samples")).expect("samples");
    for entry in samples {
        let dir = entry.expect("entry").path();
        let Ok(text) = std::fs::read_to_string(dir.join("detections.jsonl")) else {
            continue;
        };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let d: DetectionView = serde_json::from_str(line).expect("a detection line");
            let id = u64::try_from(out.len()).expect("the corpus is smaller than u64::MAX") + 1;
            // The sample sets are position feeds (docs/test-tracks/data-format.md §3);
            // a line that was not would fail here rather than be silently skipped.
            let enu = d
                .measurement
                .position_enu()
                .expect("a sample-set observation is a position");
            out.push(TrackView {
                id: TrackId(id),
                status: TrackStatus::Confirmed,
                state: nalgebra::SVector::<f64, 6>::new(enu[0], enu[1], enu[2], 1.0, -2.0, 0.5),
                covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 4.0,
                classification: Classification::Unknown,
                provenance: d.provenance.clone(),
                quality: Quality::default(),
                mission_time: d.source_time,
                // Releasable, or the agreement's filter would withhold it and the test
                // would be measuring the gate rather than the schema.
                releasability: Releasability::AllPeers,
            });
            if out.len() == WIRE_SAMPLE {
                return out;
            }
        }
    }
    out
}

/// A node serving `partner` the items it should have for this check.
fn api(snapshot: SnapshotResponse, outbound: Vec<ExchangeItem>) -> Arc<NodeApi> {
    Arc::new(NodeApi::new(snapshot).with_exchange(ExchangeSet {
        agreements: vec![ExchangeAgreement {
            party: "partner".into(),
            inbound: Vec::new(),
            outbound,
            format: ExchangeFormat::Canonical,
        }],
    }))
}

/// The canonical encoding of a value, which is what "zero loss" is measured against.
///
/// Comparing the bytes and not only the values: two `TrackView`s can compare equal while
/// one lost a field that happens to equal its default, and a partner reading the
/// serialised form would see the difference even though `PartialEq` did not.
fn canonical_track(track: &TrackView) -> String {
    serde_json::to_string(track).expect("serialises")
}

/// The same, for the health payload. Two concrete functions rather than one generic
/// one because `serde` is not a dependency of this crate and only `serde_json` is, so
/// there is no `Serialize` bound to write here.
fn canonical_health(health: SystemHealth) -> String {
    serde_json::to_string(&health).expect("serialises")
}

/// A client of the same TLS material the link uses, for the routes that are plain HTTP.
fn http_client(tls: &LinkTls) -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    for pem in &tls.trust_roots_pem {
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_pem(pem.as_bytes()).expect("a trust root"),
        );
    }
    let identity = tls
        .identity_pem
        .as_ref()
        .expect("a machine caller holds an identity");
    builder
        .identity(reqwest::Identity::from_pem(identity.as_bytes()).expect("an identity"))
        .build()
        .expect("client")
}

/// Criterion 1, tracks, first door: every track of the corpus crosses the snapshot
/// unchanged, byte for byte.
#[tokio::test(flavor = "multi_thread")]
async fn the_track_corpus_crosses_the_snapshot_with_zero_loss() {
    let tracks = corpus();
    assert!(
        tracks.len() >= 100,
        "the corpus is {} tracks; run docs/test-tracks/tools/gen_tracks.py",
        tracks.len()
    );
    let pki = Pki::new("wire-snapshot");
    let node = api(
        SnapshotResponse::new(tracks.clone(), None, SystemHealth::default(), Vec::new()),
        vec![ExchangeItem::Tracks],
    );
    let url = pki.serve(Arc::clone(&node)).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("partner"),
    };
    let peer = PeerLink::connect(&endpoint, &handle).expect("the peer link starts");

    let mut received: Vec<TrackView> = Vec::new();
    until(
        || {
            received.extend(peer.take_tracks());
            received.len() >= tracks.len()
        },
        "the whole corpus to reach the partner",
    )
    .await;
    assert_eq!(received.len(), tracks.len());
    for (sent, back) in tracks.iter().zip(&received) {
        assert_eq!(
            canonical_track(sent),
            canonical_track(back),
            "track {} did not survive the snapshot unchanged",
            sent.id.0
        );
    }
}

/// Criterion 1, tracks, second door: the same corpus through the event stream, which is
/// a different encoder on a different connection and therefore a second place loss could
/// happen. `gungnir.Envelope` is the schema being checked, `gungnir.TrackView` its
/// payload.
#[tokio::test(flavor = "multi_thread")]
async fn the_track_corpus_crosses_the_event_stream_with_zero_loss() {
    let tracks = corpus();
    let pki = Pki::new("wire-stream");
    let node = api(
        SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new()),
        vec![ExchangeItem::Tracks],
    );
    let url = pki.serve(Arc::clone(&node)).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("partner"),
    };
    let peer = PeerLink::connect(&endpoint, &handle).expect("the peer link starts");
    until(|| peer.connected(), "the partner's link to come up").await;

    // The stream is followed before the corpus goes down it, or the envelopes published
    // in the window before the subscription would reach nobody and prove nothing.
    let sentinel = tracks[0].clone();
    let mut seq = until_following(
        &node,
        |seq| Envelope {
            seq,
            mission_time: MissionTime(0.0),
            event: Event::Tracking(TrackingEvent::TrackInitiated(sentinel.clone())),
        },
        || !peer.take_tracks().is_empty(),
    )
    .await;

    for track in tracks.iter().skip(1) {
        node.publish_event(Envelope {
            seq,
            mission_time: track.mission_time,
            event: Event::Tracking(TrackingEvent::TrackUpdated(track.clone())),
        })
        .expect("published");
        seq += 1;
    }

    let mut received: Vec<TrackView> = Vec::new();
    until(
        || {
            received.extend(peer.take_tracks());
            received.len() >= tracks.len() - 1
        },
        "the corpus to arrive over the event stream",
    )
    .await;
    for (sent, back) in tracks.iter().skip(1).zip(&received) {
        assert_eq!(
            canonical_track(sent),
            canonical_track(back),
            "track {} did not survive the event stream unchanged",
            sent.id.0
        );
    }
}

/// Criterion 1, health: the payload `GET /v2/health` serves is the one the node
/// published, byte for byte, to a machine caller whose agreement lists `Health`.
#[tokio::test(flavor = "multi_thread")]
async fn the_health_payload_crosses_the_wire_with_zero_loss() {
    let health = SystemHealth {
        tracking_healthy: true,
        intercept_healthy: false,
        ingest_healthy: true,
    };
    let pki = Pki::new("wire-health");
    let node = api(
        SnapshotResponse::new(Vec::new(), None, health, Vec::new()),
        vec![ExchangeItem::Health],
    );
    let url = pki.serve(Arc::clone(&node)).await;
    let tls = pki.client("partner");

    let response = http_client(&tls)
        .get(format!("{url}/v2/health"))
        .send()
        .await
        .expect("the health route answers");
    assert!(response.status().is_success(), "{}", response.status());
    let body = response.text().await.expect("a body");
    assert_eq!(
        body,
        canonical_health(health),
        "the health payload changed crossing the wire"
    );
    let back: SystemHealth = serde_json::from_str(&body).expect("decodes");
    assert_eq!(back, health);
}

/// Criterion 2 over the wire: the catalogue's rule is an exact version match, and a node
/// speaking another version is refused **whole** rather than read in part.
///
/// The client says which two versions disagreed, because "this node speaks 3, I speak 2"
/// is actionable and "could not connect" is not. Nothing is projected: a picture read
/// from another schema would be shown as though it were this one's, which is the failure
/// the rule exists to prevent.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_speaking_another_schema_version_is_refused_whole() {
    let pki = Pki::new("wire-version");
    let mut snapshot = SnapshotResponse::new(corpus(), None, SystemHealth::default(), Vec::new());
    snapshot.schema_version = SCHEMA_VERSION + 1;
    let node = api(snapshot, vec![ExchangeItem::Tracks]);
    let url = pki.serve(Arc::clone(&node)).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("partner"),
    };
    let peer = PeerLink::connect(&endpoint, &handle).expect("the peer link starts");

    until(
        || peer.last_error().is_some(),
        "the link to refuse the incompatible node",
    )
    .await;
    let reason = peer.last_error().unwrap_or_default();
    assert!(
        reason.contains(&(SCHEMA_VERSION + 1).to_string())
            && reason.contains(&SCHEMA_VERSION.to_string()),
        "the refusal must name both versions: {reason}"
    );
    assert!(!peer.connected());
    assert!(
        peer.take_tracks().is_empty(),
        "nothing from an incompatible schema may enter the picture"
    );
}
