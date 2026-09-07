// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A set produced from Rust replays through the gateway (GAP-046, GAP-076): the
//! generator port (GAP-016) writes what the recorded adapter reads, so the plan-07
//! library feeds the ingest boundary without the Python reference in the loop.

use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::{AllowListAuthenticator, DetectionView, IngestGateway};
use gungnir_model::events::IngestEvent;
use gungnir_model::{MissionTime, SensorId, TrackView};
use gungnir_scenario::tracks::generate_sample;
use gungnir_scenario::TrackLibrary;
use gungnir_tracking_service::TrackingService;
use std::path::Path;

#[derive(Default)]
struct SinkSpy {
    received: usize,
}

impl TrackingService for SinkSpy {
    fn submit_detection(
        &mut self,
        _detection: DetectionView,
    ) -> Result<(), gungnir_tracking_service::SubmitError> {
        self.received += 1;
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

#[test]
fn a_set_generated_from_rust_replays_without_quarantine_under_its_own_allow_list() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/test-tracks");
    let lib = TrackLibrary::load(&docs).expect("the plan-07 library loads");
    let set = generate_sample(&lib, "TT-02").expect("TT-02 generates");
    let dir = std::env::temp_dir().join(format!("gungnir-generated-set-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    set.write_to(&dir).expect("written");

    // The allow-list is the set's own sensor list: the sensors.json the generator wrote.
    let sensors: Vec<SensorId> = set.sensors_json["sensors"]
        .as_array()
        .expect("sensors")
        .iter()
        .filter_map(|s| s["id"].as_u64())
        .map(|id| SensorId(u32::try_from(id).expect("sensor id fits")))
        .collect();
    assert!(!sensors.is_empty());

    let adapter = RecordedFeedAdapter::open(&dir.join("detections.jsonl")).expect("readable");
    let expected = adapter.remaining();
    assert!(expected > 0, "TT-02 produces detections");
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator { allowed: sensors }));
    gateway.add_adapter(Box::new(adapter));
    gateway.set_expected_adapters(1);
    let mut sink = SinkSpy::default();
    let mut accepted = 0usize;
    let mut quarantined = Vec::new();
    let mut now = 0.0f64;
    while now < 24.0 * 3600.0 {
        for event in gateway.tick(MissionTime(now), &mut sink) {
            match event {
                IngestEvent::Accepted(_) => accepted += 1,
                IngestEvent::Quarantined { sensor, reason } => {
                    quarantined.push(format!("sensor {}: {reason}", sensor.0));
                }
                IngestEvent::NotAccepted { .. } => {}
            }
        }
        now += 0.5;
    }
    assert!(quarantined.is_empty(), "{quarantined:?}");
    assert_eq!(accepted, expected);
    assert_eq!(sink.received, expected);
    let _ = std::fs::remove_dir_all(dir);
}
