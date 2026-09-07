//! A peer as a source, through the gateway (GAP-009, `docs/design/DN-16-peer-sources.md`
//! §5).
//!
//! DN-16 §5's first rule is that "a peer is a source, so it goes through the gateway:
//! validation, authentication, and quarantine apply exactly as they do to a radar". This
//! file holds that to the same gateway both binaries run, with the peer admitted under
//! its own source id from the baseline's allow-list.
//!
//! # What this proves about fusion, and what it does not
//!
//! GAP-009's remaining action reads "the peer's tracks on the node fused once GAP-011
//! gives it a pipeline". GAP-011 is closed and `gungnir_fusion_async::PIPELINE_IMPLEMENTED`
//! is `true`, and **the node needed no change for that**: `gungnir-node` already binds a
//! `PeerSourceAdapter` into the gateway and calls `gateway.tick(now, &mut tracking)` with
//! a real `LiveTrackingService`, so a peer's detections take the identical path a radar's
//! do. The edge the node would have needed -- `gungnir-node` to `gungnir-tracking-service`
//! -- has been in its manifest from the start.
//!
//! What is checked here is the half this crate owns: that a peer's track is admitted
//! rather than quarantined, reaches `TrackingService::submit_detection` with the peer's
//! provenance intact, and that a launch warning on the same link reaches it never. The
//! step from an accepted detection to a fused track belongs to `gungnir-fusion-async` and
//! is checked there; asserting it here would need a tokio runtime to spawn the pipeline
//! on, which is a dependency this crate does not carry.

use gungnir_ingest::adapters::peer::{
    LaunchWarningOutcome, LaunchWarningSink, PeerSourceAdapter, QueuedPeerStream,
};
use gungnir_ingest::{AllowListAuthenticator, DetectionView, IngestGateway, SensorId};
use gungnir_model::events::IngestEvent;
use gungnir_model::{
    Classification, LaunchWarningReport, MissionTime, Provenance, Quality, Releasability, TrackId,
    TrackStatus, TrackView,
};
use gungnir_tracking_service::TrackingService;

/// The source id the baseline admits this peer under: no sensor's.
const PEER_SOURCE: SensorId = SensorId(900);

/// Records what reaches the tracking service, which is the boundary DN-16 §5 cares
/// about: past this point a detection is in the picture.
#[derive(Default)]
struct SinkSpy {
    received: Vec<DetectionView>,
}

impl TrackingService for SinkSpy {
    fn submit_detection(
        &mut self,
        detection: DetectionView,
    ) -> Result<(), gungnir_tracking_service::SubmitError> {
        self.received.push(detection);
        Ok(())
    }
    fn poll(&mut self, _now: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &[]
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn track(id: u64, at: f64, position: [f64; 3]) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(
            position[0],
            position[1],
            position[2],
            10.0,
            0.0,
            0.0,
        ),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(at),
        releasability: Releasability::AllPeers,
    }
}

fn warning(id: &str) -> LaunchWarningReport {
    LaunchWarningReport {
        id: id.into(),
        what: "ballistic launch, northern sector".into(),
        at: MissionTime(99.0),
        releasability: Releasability::AllPeers,
    }
}

/// The gateway both binaries build, admitting exactly this peer.
fn gateway_with(stream: QueuedPeerStream, sink: &LaunchWarningSink) -> IngestGateway {
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: vec![PEER_SOURCE],
    }));
    gateway.set_expected_adapters(1);
    gateway.add_adapter(Box::new(
        PeerSourceAdapter::new("kal-cell", PEER_SOURCE, 0.6, 30.0, stream)
            .with_launch_warning_sink(sink.clone()),
    ));
    gateway
}

/// A peer's track is admitted under the peer's own source id and reaches the tracking
/// service with the peer, both timestamps and our assigned quality on the provenance.
///
/// The provenance is the assertion that matters: a peer track that arrived with the
/// peer stripped off it would be indistinguishable from something this deployment
/// observed, which is exactly the lie DN-16 exists to prevent.
#[test]
fn a_peer_track_is_admitted_and_reaches_the_tracking_service_with_its_provenance() {
    let sink = LaunchWarningSink::default();
    let mut gateway = gateway_with(
        QueuedPeerStream {
            tracks: vec![track(41, 99.0, [1_000.0, 2_000.0, 300.0])],
            ..QueuedPeerStream::default()
        },
        &sink,
    );
    let mut tracking = SinkSpy::default();

    let events = gateway.tick(MissionTime(100.0), &mut tracking);

    assert_eq!(gateway.stats().accepted, 1);
    assert_eq!(gateway.stats().quarantined, 0);
    assert!(
        events.iter().any(|e| matches!(e, IngestEvent::Accepted(_))),
        "the peer's track was not recorded as accepted: {events:?}"
    );
    assert_eq!(tracking.received.len(), 1);
    let detection = &tracking.received[0];
    assert_eq!(detection.sensor, PEER_SOURCE);
    let origin = detection
        .provenance
        .peer
        .as_ref()
        .expect("the peer origin reached the tracking service");
    assert_eq!(origin.peer, "kal-cell");
    assert!(
        (origin.assigned_quality - 0.6).abs() < f32::EPSILON,
        "the quality is ours, not the peer's"
    );
    assert!((origin.age_s() - 1.0).abs() < 1e-9, "the age is visible");
    assert_eq!(
        detection.provenance.authentication,
        gungnir_model::SourceAuthentication::AllowList,
        "the gateway stamps how the peer was admitted, like any other source"
    );
}

/// A source the baseline does not name is refused even when it calls itself a peer.
/// The peer's own id is what the allow-list grants, and nothing else rides in on it.
#[test]
fn a_peer_speaking_for_a_source_the_baseline_does_not_admit_is_refused() {
    let sink = LaunchWarningSink::default();
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: vec![PEER_SOURCE],
    }));
    gateway.set_expected_adapters(1);
    gateway.add_adapter(Box::new(
        PeerSourceAdapter::new(
            "kal-cell",
            SensorId(901),
            0.6,
            30.0,
            QueuedPeerStream {
                tracks: vec![track(41, 99.0, [1_000.0, 2_000.0, 300.0])],
                ..QueuedPeerStream::default()
            },
        )
        .with_launch_warning_sink(sink.clone()),
    ));
    let mut tracking = SinkSpy::default();

    gateway.tick(MissionTime(100.0), &mut tracking);

    assert!(tracking.received.is_empty());
    assert_eq!(gateway.stats().quarantined, 1);
}

/// DN-16 §8's criterion, at the gateway: a launch warning creates an alert and no track.
///
/// The warning and a track arrive on one poll of one peer. The track becomes a detection
/// and the warning goes to the host's sink; nothing the tracking service receives came
/// from the warning, so no track can be built from it.
#[test]
fn a_launch_warning_reaches_the_host_and_never_the_tracking_service() {
    let sink = LaunchWarningSink::default();
    let mut gateway = gateway_with(
        QueuedPeerStream {
            tracks: vec![track(41, 99.0, [1_000.0, 2_000.0, 300.0])],
            launch_warnings: vec![warning("LW-1")],
        },
        &sink,
    );
    let mut tracking = SinkSpy::default();

    gateway.tick(MissionTime(100.0), &mut tracking);

    assert_eq!(
        tracking.received.len(),
        1,
        "only the track became a detection"
    );
    assert_eq!(gateway.stats().accepted, 1);
    let outcomes: Vec<LaunchWarningOutcome> = sink
        .lock()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    assert_eq!(outcomes.len(), 1);
    let LaunchWarningOutcome::Admitted(admitted) = &outcomes[0] else {
        panic!("the launch warning was not admitted: {:?}", outcomes[0]);
    };
    assert!(admitted.alert_summary().contains("kal-cell"));
}

/// A peer that sends only launch warnings produces nothing for the picture, and the
/// gateway does not report having accepted anything.
///
/// The counters have to stay apart: a health line reading "1 accepted" for a peer that
/// sent a warning and no track would claim a picture contribution that does not exist.
#[test]
fn a_peer_that_only_warns_contributes_nothing_to_the_picture() {
    let sink = LaunchWarningSink::default();
    let mut gateway = gateway_with(
        QueuedPeerStream {
            launch_warnings: vec![warning("LW-1"), warning("LW-2")],
            ..QueuedPeerStream::default()
        },
        &sink,
    );
    let mut tracking = SinkSpy::default();

    let events = gateway.tick(MissionTime(100.0), &mut tracking);

    assert!(tracking.received.is_empty());
    assert!(events.is_empty());
    assert_eq!(gateway.stats().accepted, 0);
    assert_eq!(gateway.stats().quarantined, 0);
    assert_eq!(
        sink.lock().map(|q| q.len()).unwrap_or_default(),
        2,
        "both warnings reached the host"
    );
}
