// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-206: a tracker-produced track crosses the wire to a partner node, and one the
//! baseline keeps `Internal` does not (DN-17 §5 and §6; plan 12 tranche A).
//!
//! Until GAP-206 both projections stamped `Releasability::Internal` on every track and no
//! baseline field could say otherwise, so the exchange gate withheld everything and the
//! only tracks that ever crossed a peer link were ones a test built as `AllPeers`. Here
//! node A runs its own tracker, gateway and picture publication from a baseline that
//! releases sensor 1 to node B and leaves sensor 2 at the default, and node B links to it
//! as a machine over mutual TLS, under its own peer identity, exactly as `bind_peers` does.
//!
//! Then node B's half: what it heard from A is a peer source, and a track a peer fed stays
//! `Internal` on B whatever B's default says, so it can never be released back to A as a
//! new source (the owner's choice of 2026-10-27, until GAP-218's re-ingest guard).

use gungnir_api::tls::{self, TlsListener};
use gungnir_api::transport::{serve_on_listener, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_config::{
    ConfigBaseline, PeerConfig, ReleasabilityConfig, SensorConfig, SensorReleasabilityConfig,
};
use gungnir_ingest::adapters::peer::{PeerSourceAdapter, QueuedPeerStream};
use gungnir_ingest::{IngestGateway, ProtocolAdapter};
use gungnir_model::{
    DetectionView, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet, Measurement,
    MissionTime, PlanView, Provenance, Releasability, SensorId, SystemHealth, TrackView,
};
use gungnir_remote::peer::PeerLink;
use gungnir_remote::{LinkTls, RemoteEndpoint};
use gungnir_tracking_service::{LiveTrackingService, TrackingService};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Released to node B.
const RELEASED: u32 = 1;
/// Left at the deployment's default, `Internal`.
const HELD: u32 = 2;
/// The source id node B admits node A's tracks under.
const PEER_SOURCE: u32 = 900;

/// A deadlock guard, as in `gungnir-remote/tests/tls_link.rs`: a minute, left the moment
/// the condition holds.
const PATIENCE: usize = 2_400;

fn sensor(id: u32) -> SensorConfig {
    SensorConfig {
        id,
        modality: "radar".into(),
        position: [0.9, 0.2, 10.0],
        max_range_m: 50_000.0,
        control_endpoint: None,
        maintenance: Vec::new(),
        detection_model: None,
        azimuth_sector: None,
        elevation_band: None,
    }
}

/// Node A's baseline: two radars, one released to `partner` by name and one left at the
/// default, the partner as an endpoint, and the agreement that lets it receive tracks.
fn node_a(partner: &str) -> ConfigBaseline {
    let config = ConfigBaseline {
        sensors: vec![sensor(RELEASED), sensor(HELD)],
        // An exchange party must be a peer or an endpoint this baseline knows.
        endpoints: vec![gungnir_config::EndpointConfig {
            name: partner.to_owned(),
            kind: "peer".into(),
            address: "https://node-b.local:7410".into(),
        }],
        exchange: vec![ExchangeAgreement {
            party: partner.to_owned(),
            inbound: Vec::new(),
            outbound: vec![ExchangeItem::Tracks],
            format: ExchangeFormat::Canonical,
        }],
        releasability: ReleasabilityConfig {
            sensors: vec![SensorReleasabilityConfig {
                sensor: RELEASED,
                marking: Releasability::parties([partner]),
            }],
            ..ReleasabilityConfig::default()
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("node A's baseline is valid");
    config
}

/// Node B's baseline: node A as a peer, and a default that would release everything of
/// B's own -- which a track A fed must not inherit.
fn node_b() -> ConfigBaseline {
    let config = ConfigBaseline {
        peers: vec![PeerConfig {
            name: "node-a".into(),
            endpoint: "node-a".into(),
            source_id: PEER_SOURCE,
            assigned_quality: 0.6,
            max_age_s: 30.0,
        }],
        endpoints: vec![gungnir_config::EndpointConfig {
            name: "node-a".into(),
            kind: "peer".into(),
            address: "https://localhost:7410".into(),
        }],
        releasability: ReleasabilityConfig {
            default: Releasability::AllPeers,
            ..ReleasabilityConfig::default()
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("node B's baseline is valid");
    config
}

/// A feed the test fills, stamping receipt with the gateway's `now` as a live one does.
struct Feed(Arc<Mutex<VecDeque<DetectionView>>>);

impl ProtocolAdapter for Feed {
    fn name(&self) -> &'static str {
        "radar"
    }

    fn poll(
        &mut self,
        now: MissionTime,
    ) -> Result<Vec<DetectionView>, gungnir_ingest::IngestError> {
        let drained: Vec<DetectionView> = self
            .0
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        Ok(drained
            .into_iter()
            .map(|mut d| {
                d.receipt_time = now;
                d
            })
            .collect())
    }
}

fn report(sensor: u32, at: f64, enu: [f64; 3]) -> DetectionView {
    DetectionView {
        sensor: SensorId(sensor),
        source_time: MissionTime(at - 0.05),
        receipt_time: MissionTime(0.0),
        measurement: Measurement::Position {
            enu: nalgebra::Vector3::new(enu[0], enu[1], enu[2]),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

/// A feed's queue, and what to put on it at each tick's time.
type Scan<'a> = (
    &'a Arc<Mutex<VecDeque<DetectionView>>>,
    &'a dyn Fn(f64) -> Vec<DetectionView>,
);

/// Tick the gateway and the tracker until the tracker holds `want` tracks.
async fn ticked_until(
    gateway: &mut IngestGateway,
    tracking: &mut LiveTrackingService,
    feed: Option<Scan<'_>>,
    want: usize,
    what: &str,
) -> Vec<TrackView> {
    for k in 0..PATIENCE {
        #[allow(clippy::cast_precision_loss)]
        let now = 1_000.0 + k as f64;
        if let Some((queue, make)) = feed {
            queue.lock().expect("feed").extend(make(now));
        }
        gateway.tick(MissionTime(now), tracking);
        tracking.poll(MissionTime(now));
        if tracking.tracks().len() == want {
            return tracking.tracks().to_vec();
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Node A serving under its own serving identity, trusting exactly `pinned` as a client:
/// the way a deployment pins a partner node's peer identity (D-135).
async fn serve_node_a(api: Arc<NodeApi>, pinned: &str) -> (String, String) {
    let dir = std::env::temp_dir().join(format!("gungnir-releasable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    // A file where the keystore's directory would go, so the serving identity comes from
    // the ephemeral fallback and the test touches no operating-system keystore.
    let blocked = dir.join("no-keystore");
    std::fs::write(&blocked, b"not a directory").expect("written");
    let serving =
        gungnir_remote::identity::issue_node_serving_identity(&blocked, vec!["localhost".into()])
            .expect("issued");
    let client_ca = dir.join("pinned-peers.pem");
    std::fs::write(&client_ca, pinned).expect("pinned");
    let acceptor = tls::acceptor_with_key(
        serving.certificate_der.clone(),
        Arc::clone(&serving.key),
        &client_ca.to_string_lossy(),
    )
    .expect("acceptor");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = tcp.local_addr().expect("addr").port();
    tokio::spawn(async move {
        let _ = serve_on_listener(TlsListener::new(tcp, acceptor), api).await;
    });
    (format!("https://localhost:{port}"), serving.certificate_pem)
}

// One scene from end to end: node A's picture, the wire, and node B's own reading of what
// arrived. Split, the second half would have to rebuild the first.
#[allow(clippy::too_many_lines)]
#[tokio::test(flavor = "multi_thread")]
async fn a_released_track_crosses_to_the_partner_and_an_internal_one_does_not() {
    let handle = tokio::runtime::Handle::current();
    // Node B's peer identity first: node A's baseline names it as the party.
    let b_identity =
        gungnir_remote::identity::issue_node_peer_identity_ephemeral().expect("issued");
    let partner = b_identity.name.clone();

    // ---- Node A: its own tracker, gateway and picture, from its baseline. ----
    let config_a = node_a(&partner);
    let mut tracking_a = gungnir_node::picture::tracking_service(&config_a, &handle, None);
    let mut gateway_a = gungnir_node::picture::gateway(&config_a);
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    gateway_a.add_adapter(Box::new(Feed(Arc::clone(&queue))));
    // Two targets far apart, one seen only by each radar.
    let scan = |now: f64| {
        let t = now - 1_000.0;
        vec![
            report(RELEASED, now, [1_000.0 + 50.0 * t, 2_000.0, 300.0]),
            report(HELD, now, [-20_000.0, 15_000.0 + 40.0 * t, 500.0]),
        ]
    };
    let tracks = ticked_until(
        &mut gateway_a,
        &mut tracking_a,
        Some((&queue, &scan)),
        2,
        "node A's two tracks",
    )
    .await;
    let marking_of = |sensor: u32| {
        tracks
            .iter()
            .find(|t| t.provenance.source_sensor_ids == [sensor])
            .map(|t| t.releasability.clone())
    };
    assert_eq!(
        marking_of(RELEASED),
        Some(Releasability::parties([partner.as_str()])),
        "the released radar's track is marked as the baseline says: {tracks:?}"
    );
    assert_eq!(
        marking_of(HELD),
        Some(Releasability::Internal),
        "the other radar's track stays in: {tracks:?}"
    );

    let healthy = SystemHealth {
        tracking_healthy: true,
        intercept_healthy: true,
        ingest_healthy: true,
    };
    let api = Arc::new(
        NodeApi::new(SnapshotResponse::new(Vec::new(), None, healthy, Vec::new())).with_exchange(
            ExchangeSet {
                agreements: config_a.exchange.clone(),
            },
        ),
    );
    gungnir_node::picture::publish_picture(
        &api,
        &tracks,
        &[],
        tracking_a.pipeline_stats(),
        None,
        &PlanView::default(),
        None,
        healthy,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let (url, serving_pem) = serve_node_a(Arc::clone(&api), &b_identity.certificate_pem).await;

    // ---- Over the wire: node B's machine link, under its own pinned identity. ----
    let link = PeerLink::connect(
        &RemoteEndpoint {
            url,
            tls: LinkTls {
                trust_roots_pem: vec![serving_pem],
                issued: Some(b_identity.key),
                identity_pem: None,
            },
        },
        &handle,
    )
    .expect("the peer link starts");
    let mut heard = Vec::new();
    for _ in 0..PATIENCE {
        heard.extend(link.take_tracks());
        if !heard.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(
        heard.len(),
        1,
        "exactly the released track crosses; the internal one does not: {heard:?}"
    );
    assert_eq!(heard[0].provenance.source_sensor_ids, [RELEASED]);
    let filtered = api.snapshot_for(&partner).expect("a snapshot is published");
    assert!(
        filtered.withheld >= 1,
        "the partner is told its picture is partial, not handed a shorter list in silence"
    );

    // ---- Node B: what it heard is a peer source, and stays in on B. ----
    let config_b = node_b();
    let mut tracking_b = gungnir_node::picture::tracking_service(&config_b, &handle, None);
    let mut gateway_b = gungnir_node::picture::gateway(&config_b);
    gateway_b.add_adapter(Box::new(Replayed {
        peer: config_b.peers[0].clone(),
        heard,
    }));
    let on_b = ticked_until(&mut gateway_b, &mut tracking_b, None, 1, "node B's track").await;
    assert_eq!(on_b[0].provenance.source_sensor_ids, [PEER_SOURCE]);
    assert_eq!(
        on_b[0].releasability,
        Releasability::Internal,
        "a track a peer fed stays in on B, under B's all-peers default, so it cannot go \
         back to A as a new source"
    );
}

/// Node B's peer adapter, built from B's own peer row as `bind_peers` builds it, and handed
/// what the link heard again on every poll at B's clock: the two nodes' mission times are
/// unrelated, and a live link keeps a partner's track current rather than replaying one
/// instant of it.
struct Replayed {
    peer: PeerConfig,
    heard: Vec<TrackView>,
}

impl ProtocolAdapter for Replayed {
    fn name(&self) -> &'static str {
        "peer:node-a"
    }

    fn poll(
        &mut self,
        now: MissionTime,
    ) -> Result<Vec<DetectionView>, gungnir_ingest::IngestError> {
        let tracks: Vec<TrackView> = self
            .heard
            .iter()
            .cloned()
            .map(|mut t| {
                t.mission_time = MissionTime(now.0 - 0.05);
                t
            })
            .collect();
        PeerSourceAdapter::new(
            self.peer.name.clone(),
            SensorId(self.peer.source_id),
            self.peer.assigned_quality,
            self.peer.max_age_s,
            QueuedPeerStream {
                tracks,
                launch_warnings: Vec::new(),
            },
        )
        .poll(now)
    }
}
