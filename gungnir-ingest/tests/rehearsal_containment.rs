// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **Containment, the gateway half** (`docs/design/DN-32-re-observation-for-a-laydown.md`
//! §6 mechanism 2; the *Containment* Draft row of `docs/verification-capability-table.md`
//! §2): a live gateway fed a detection carrying `Provenance::rehearsal` rejects it and
//! counts it, under its own name and as a quarantine; only a gateway built with
//! `IngestGateway::for_rehearsal` admits one. The gateway is human-owned
//! (`docs/agentic-workflow.md`); what the owner has reviewed of it is in
//! `docs/signatures.md`.
//!
//! Fed three ways, because a marked detection could reach a live gateway three ways: from
//! an in-memory adapter, from a recorded file whose lines carry the mark, and mixed in
//! with real detections from the same sensor, which must still pass.

use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::adapters::simulated::SimulatedAdapter;
use gungnir_ingest::{
    AllowAllAuthenticator, AllowListAuthenticator, IngestError, IngestGateway, IngestStats,
};
use gungnir_model::events::IngestEvent;
use gungnir_model::{
    DetectionView, LaydownId, MissionTime, Provenance, RehearsalOrigin, SensorId, TestTrackNumber,
    TrackView,
};
use gungnir_tracking_service::{SubmitError, TrackingService};

#[derive(Default)]
struct Sink {
    received: Vec<DetectionView>,
}

impl TrackingService for Sink {
    fn submit_detection(&mut self, detection: DetectionView) -> Result<(), SubmitError> {
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

fn origin() -> RehearsalOrigin {
    RehearsalOrigin {
        scenario: TestTrackNumber(1),
        laydown: LaydownId("c".into()),
        seed: 1701,
    }
}

fn detection(sensor: u32, t: f64, rehearsal: Option<RehearsalOrigin>) -> DetectionView {
    DetectionView {
        sensor: SensorId(sensor),
        source_time: MissionTime(t),
        receipt_time: MissionTime(t + 0.2),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(1000.0, 2000.0, 300.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance {
            source_sensor_ids: vec![sensor],
            algorithm_version: "gungnir-sensor-sim 0.1.0".into(),
            rehearsal,
            ..Provenance::default()
        },
    }
}

#[test]
fn a_live_gateway_rejects_and_counts_a_rehearsals_detection() {
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: vec![SensorId(2)],
    }));
    assert!(!gateway.admits_rehearsal());
    let mut adapter = SimulatedAdapter::new("sim");
    adapter.push(detection(2, 9.0, Some(origin())));
    adapter.push(detection(2, 9.5, None));
    gateway.add_adapter(Box::new(adapter));
    let mut sink = Sink::default();
    let events = gateway.tick(MissionTime(10.0), &mut sink);

    assert_eq!(
        sink.received.len(),
        1,
        "only the sensor's own detection reaches the sink"
    );
    assert!(sink.received[0].provenance.rehearsal.is_none());
    assert_eq!(
        gateway.stats(),
        IngestStats {
            accepted: 1,
            quarantined: 1,
            adapter_failures: 0,
            not_accepted: 0,
            rehearsal_refused: 1,
        }
    );
    let reason = events
        .iter()
        .find_map(|e| match e {
            IngestEvent::Quarantined { sensor, reason } if *sensor == SensorId(2) => {
                Some(reason.clone())
            }
            _ => None,
        })
        .expect("the refusal is published as a quarantine");
    assert_eq!(
        reason,
        IngestError::RehearsalRefused {
            sensor: SensorId(2),
            origin: origin().to_string(),
        }
        .to_string()
    );
    assert!(
        reason.contains("re-observed from TT-01 under laydown c"),
        "{reason}"
    );
}

/// A recorded file is JSON, and `rehearsal` is a field a line may carry: a file
/// written out of a rehearsal and replayed into a live gateway is refused line by line.
#[test]
fn a_recorded_file_carrying_the_mark_is_refused_line_by_line() {
    let path = std::env::temp_dir().join(format!(
        "gungnir-rehearsal-containment-{}.jsonl",
        std::process::id()
    ));
    let lines: Vec<String> = [
        detection(4, 1.0, Some(origin())),
        detection(4, 2.0, Some(origin())),
    ]
    .iter()
    .map(|d| serde_json::to_string(d).expect("encodes"))
    .collect();
    std::fs::write(&path, lines.join("\n")).expect("written");
    let adapter = RecordedFeedAdapter::open(&path).expect("a well-formed file opens");
    let _ = std::fs::remove_file(&path);

    let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
    gateway.add_adapter(Box::new(adapter));
    let mut sink = Sink::default();
    let _ = gateway.tick(MissionTime(5.0), &mut sink);
    assert!(sink.received.is_empty());
    assert_eq!(gateway.stats().rehearsal_refused, 2);
    assert_eq!(gateway.stats().quarantined, 2);
    assert_eq!(gateway.stats().accepted, 0);
}

/// The one construction that admits a marked detection, and it still authenticates and
/// validates it exactly as a live gateway would.
#[test]
fn a_rehearsals_own_gateway_admits_it_and_still_authenticates_and_validates() {
    let mut gateway = IngestGateway::for_rehearsal(Box::new(AllowListAuthenticator {
        allowed: vec![SensorId(2)],
    }));
    assert!(gateway.admits_rehearsal());
    let mut bad = detection(2, 9.0, Some(origin()));
    bad.receipt_time = MissionTime(100.0);
    gateway.add_adapter(Box::new(RecordedFeedAdapter::from_views(
        "rehearsal",
        vec![
            detection(2, 9.0, Some(origin())),
            detection(3, 9.0, Some(origin())),
            bad,
        ],
    )));
    let mut sink = Sink::default();
    let _ = gateway.tick(MissionTime(10.0), &mut sink);
    assert_eq!(sink.received.len(), 1);
    assert_eq!(sink.received[0].provenance.rehearsal, Some(origin()));
    let stats = gateway.stats();
    assert_eq!(stats.accepted, 1);
    assert_eq!(
        stats.quarantined, 2,
        "an unknown sensor and a future receipt"
    );
    assert_eq!(stats.rehearsal_refused, 0);
}
