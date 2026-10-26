// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

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
//! | Zero-loss round-trip of the health payload | **Yes**, `GET /v3/health` as a machine caller under an agreement listing `Health` |
//! | Zero-loss crossing of the exchange products | **Yes**, for warnings, reports and handoffs: a real launch warning, a mission report generated from a real journal, and a handoff, each written by a signed-in desktop to its node and read by a partner over mutual TLS, compared as canonical JSON bytes and found verbatim in the served response |
//! | An unsupported version is refused with the catalogue's error | **Yes**: a node declaring a schema version this build does not speak is refused whole, and the link says which two versions disagreed |
//! | Public-specification samples decode partially, and nothing panics on the fuzz corpus | **No, and not applicable**: ASTERIX, AIS and STANAG are read from feeds and files, not from this transport, and stay covered in memory |
//!
//! # Which are still not covered, and why
//!
//! * **`gungnir.DetectionView`** has a wire door (`POST /v3/detections`) and no check
//!   here. It is a write path and an operator's, not a peer's; it is exercised for
//!   delivery by `transport.rs` and not for byte parity.
//! * **`gungnir.PlanView`** crosses no wire to a peer at all: a plan is a recommendation
//!   for this deployment's own effectors and `NodeApi::snapshot_for` withholds it from
//!   every party by design (DN-18 §5).
//! * **`gungnir.detections.arrow`, `gungnir.ml.classification-rows`,
//!   `gungnir.GlobalEntityId`, `asterix.cat048`, `asterix.cat034`, `ais.m1371`** have no
//!   producer on this transport.
//! * **`stanag.4676`** has no corpus and no decoder at all, and is descoped from this
//!   release by D-44.
//!
//! `gungnir-interop/tests/conformance.rs` states the same table entry by entry, so a new
//! catalogue entry fails there until somebody says which of these it is. This file
//! cannot walk the catalogue itself: `gungnir-remote` does not depend on
//! `gungnir-interop` and `ARCHITECTURE.md` §7.1 draws no such edge.

mod common;

use common::{until, until_following, Pki};
use gungnir_api::transport::{AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{ExchangeProduct, ExchangeResponse, SnapshotResponse};
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::TrackingEvent;
use gungnir_model::{
    Classification, DecisionAttribution, DecisionId, DetectionView, ExchangeAgreement,
    ExchangeFormat, ExchangeItem, ExchangeSet, Geodetic, Handoff, InterceptSolutionView,
    LaunchWarningReport, MissionTime, PlanId, PlanKind, Provenance, Quality, Releasability,
    ResourceId, SessionId, SystemHealth, TrackId, TrackStatus, TrackView, SCHEMA_VERSION,
};
use gungnir_remote::link::{Credential, ExchangeProductRecord};
use gungnir_remote::peer::PeerLink;
use gungnir_remote::{LinkTls, RemoteEndpoint};
use gungnir_reporting::{JournalReportGenerator, ReportGenerator, TrackingMetricsSummary};
use gungnir_security::{
    hash_passphrase, Account, InMemoryAccountStore, OperatorId, Role, TokenIssuer,
};
use gungnir_store::{EventJournal, FileEventJournal};
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

/// Criterion 1, health: the payload `GET /v3/health` serves is the one the node
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
        .get(format!("{url}/v3/health"))
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

/// The operator the desktop signs in as: a `Commander`, which holds `PUBLISH_EXCHANGE`.
const COMMANDER: u64 = 70;
const PASSPHRASE: &str = "correct horse battery staple";

// Every body below is written through `to_partner_value`, the function
// `ExchangeProduct::body_of` and the desktop's `product_body` both call, so a non-finite
// float is DN-18 §15's "value unavailable" object rather than a `null` (GAP-171, D-103).

/// A launch warning as `gungnir-app`'s `launch_warning::declare` publishes one, with text
/// a re-encoding transport could mangle: non-ASCII, and a dash a careless one would not.
fn launch_warning_record() -> ExchangeProductRecord {
    let report = LaunchWarningReport {
        id: "TT07-lw-0001".into(),
        what: "Shahed-136 \u{d7} 3 from the Vell launch site \u{2014} bearing 042\u{b0}".into(),
        at: MissionTime(1_234.567),
        releasability: Releasability::AllPeers,
    };
    ExchangeProductRecord {
        id: report.id.clone(),
        at: report.at,
        releasability: report.releasability.clone(),
        body: gungnir_eventing::nonfinite::to_partner_value(&report).expect("a product body"),
    }
}

/// A mission report generated from a real journal, as `sustainment::exchange_record`
/// publishes one, with a diverged tracker's metrics added: the NaN and the infinity are
/// what a replay with truth can produce, and the two values the body's encoding exists
/// for.
fn mission_report_record() -> ExchangeProductRecord {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-wire-report-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let mut journal = FileEventJournal::open(&dir).expect("a journal");
    let session = SessionId(41);
    for (seq, track) in (1u64..).zip(corpus().into_iter().take(12)) {
        journal
            .append(
                session,
                &Envelope {
                    seq,
                    mission_time: track.mission_time,
                    event: Event::Tracking(TrackingEvent::TrackInitiated(track)),
                },
            )
            .expect("appended");
    }
    let mut report = JournalReportGenerator {
        journal: &journal,
        metrics: None,
    }
    .generate(session)
    .expect("a report from the journal");
    report.metrics = Some(TrackingMetricsSummary {
        mota: f64::NAN,
        motp: f64::INFINITY,
        purity: 0.75,
        fragmentation: 0.125,
    });
    let _ = std::fs::remove_dir_all(&dir);
    // Every journaled track is releasable to all peers, so the report's combined marking
    // is too; anything less and the partner's gate would withhold it and this test would
    // measure the gate.
    assert_eq!(report.releasability, Releasability::AllPeers);
    ExchangeProductRecord {
        id: report.session.0.to_string(),
        at: MissionTime(1_250.0),
        releasability: report.releasability.clone(),
        body: gungnir_eventing::nonfinite::to_partner_value(&report).expect("a product body"),
    }
}

/// A handoff as the node's approval queue publishes one (`gungnir-node/src/approval.rs`):
/// a full-width decision identifier, an intercept solution with its geodetic point, and
/// the provenance of the track it names.
fn handoff_record() -> ExchangeProductRecord {
    let handoff = Handoff::from_decision(
        DecisionId(0x0192_f3a1_7c4e_7d2b_9a10_3e5f_6b7c_8d9e),
        PlanId(0x0192_f3a1_7c4e_7d2b_9a10_3e5f_6b7c_8d9f),
        PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(4),
                track: TrackId(7),
                intercept_point: Some(Geodetic {
                    lat_rad: 0.820_304_748_437_334_9,
                    lon_rad: 0.244_346_095_279_206_1,
                    alt_m: 2_150.25,
                }),
                time_to_intercept_s: Some(37.125),
            }],
        },
        DecisionAttribution {
            operator: "commander-70".into(),
            role: "commander".into(),
            at: MissionTime(1_300.0),
            authority_rule: Some("pre-delegated: air defence, hostile UAS".into()),
        },
        vec![(TrackId(7), Provenance::default(), Quality::default())],
        Releasability::AllPeers,
        MissionTime(1_301.5),
    );
    ExchangeProductRecord {
        id: format!("{:x}", handoff.decision.0),
        at: handoff.issued,
        releasability: handoff.releasability.clone(),
        body: gungnir_eventing::nonfinite::to_partner_value(&handoff).expect("a product body"),
    }
}

/// What a partner should be served for `record`: the product as the node holds it.
fn canonical_product(record: &ExchangeProductRecord) -> String {
    serde_json::to_string(&ExchangeProduct {
        id: record.id.clone(),
        at: record.at,
        releasability: record.releasability.clone(),
        body: record.body.clone(),
    })
    .expect("serialises")
}

/// A node holding one `Commander` account, and an agreement sending `partner` all three
/// exchange items.
fn exchange_node() -> Arc<NodeApi> {
    let accounts = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(COMMANDER),
        role: Role::Commander,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_exchange(ExchangeSet {
            agreements: vec![ExchangeAgreement {
                party: "partner".into(),
                inbound: Vec::new(),
                outbound: vec![
                    ExchangeItem::Warnings,
                    ExchangeItem::Reports,
                    ExchangeItem::Handoffs,
                ],
                format: ExchangeFormat::Canonical,
            }],
        })
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(accounts),
            TokenIssuer::new(vec![5u8; 32], 300.0).expect("issuer"),
        ))),
    )
}

/// Criterion 1, the exchange products (GAP-065): a real launch warning, a mission report
/// generated from a real journal, and a handoff each cross **both** hops -- a signed-in
/// desktop's write to its node over the link, and a partner's read from that node over
/// mutual TLS -- and arrive as the desktop wrote them, byte for byte.
///
/// Compared twice: as the canonical encoding of the product the partner decodes, and as
/// the body's own bytes found verbatim in the response the node served, so a transport
/// that re-encoded the body consistently on both sides would still be caught.
#[tokio::test(flavor = "multi_thread")]
async fn every_exchange_product_crosses_both_hops_with_zero_loss() {
    let pki = Pki::new("wire-exchange");
    let node = exchange_node();
    let url = pki.serve(Arc::clone(&node)).await;
    let handle = tokio::runtime::Handle::current();
    let desktop = gungnir_remote::link::start(
        &RemoteEndpoint {
            url: url.clone(),
            tls: pki.client("desktop-a"),
        },
        Credential {
            operator: COMMANDER,
            passphrase: PASSPHRASE.into(),
        },
        &handle,
    )
    .expect("the desktop's link starts");
    until(|| desktop.connected(), "the desktop's link to come up").await;

    let products = [
        (ExchangeItem::Warnings, "warnings", launch_warning_record()),
        (ExchangeItem::Reports, "reports", mission_report_record()),
        (ExchangeItem::Handoffs, "handoffs", handoff_record()),
    ];
    for (item, _, record) in &products {
        desktop.queue_exchange(*item, vec![record.clone()]);
    }
    for (item, _, _) in &products {
        until(
            || {
                matches!(
                    node.exchange_all(*item),
                    Some(ExchangeResponse::Held { ref products, .. }) if products.len() == 1
                )
            },
            "the node to hold what the desktop published",
        )
        .await;
    }

    let partner = http_client(&pki.client("partner"));
    for (item, route, record) in &products {
        let response = partner
            .get(format!("{url}/v3/exchange/{route}"))
            .send()
            .await
            .expect("the exchange route answers");
        assert!(
            response.status().is_success(),
            "{route}: {}",
            response.status()
        );
        let served = response.text().await.expect("a body");
        let body_bytes = serde_json::to_string(&record.body).expect("serialises");
        assert!(
            served.contains(&body_bytes),
            "{route}: the body the desktop wrote is not in the bytes the node served"
        );
        let ExchangeResponse::Held {
            item: held,
            products,
            withheld,
            ..
        } = serde_json::from_str(&served).expect("an exchange response")
        else {
            panic!("{route}: not held: {served}");
        };
        assert_eq!(held, *item);
        assert_eq!(
            withheld, 0,
            "{route}: the marking released it, so nothing is withheld"
        );
        assert_eq!(products.len(), 1, "{route}");
        assert_eq!(
            serde_json::to_string(&products[0]).expect("serialises"),
            canonical_product(record),
            "{route}: the product changed between the desktop and the partner"
        );
    }

    // The two non-finite metrics arrived as DN-18 §15's object, which is what made the
    // report's crossing worth checking: a `null` here would be a silent loss.
    let report = &products[1].2.body;
    for metric in ["mota", "motp"] {
        assert!(
            report["metrics"][metric].is_object(),
            "the report's {metric} must cross as the value-unavailable object: {}",
            report["metrics"][metric]
        );
    }
}
