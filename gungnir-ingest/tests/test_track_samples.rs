// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The committed test-track sample sets (`testdata/tracks/samples/*/detections.jsonl`,
//! plan 07) must replay through the recorded-feed adapter and the ingest gateway
//! without a single quarantine: every line parses as a `DetectionView`, passes the
//! gateway's rules at the mission time the adapter releases it, and reaches the
//! tracking service. This is the CI use of the sample sets that
//! `docs/test-tracks/README.md` promises; the generator's latency cap exists so that
//! this holds (`docs/test-tracks/sensor-models.md`).

use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::{AllowAllAuthenticator, DetectionView, IngestGateway};
use gungnir_model::events::IngestEvent;
use gungnir_model::{MissionTime, TrackView};
use gungnir_tracking_service::TrackingService;
use std::path::{Path, PathBuf};

/// Records what reaches the tracking service.
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

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("testdata")
        .join("tracks")
        .join("samples")
}

fn sample_sets() -> Vec<PathBuf> {
    let mut sets: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("testdata/tracks/samples exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("detections.jsonl").is_file())
        .collect();
    sets.sort();
    sets
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .expect("read detections")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

#[test]
fn every_sample_set_replays_without_quarantine() {
    let sets = sample_sets();
    assert!(
        !sets.is_empty(),
        "no sample sets under testdata/tracks/samples; run docs/test-tracks/tools/gen_tracks.py"
    );
    for set in sets {
        let file = set.join("detections.jsonl");
        let expected = line_count(&file);
        let adapter = RecordedFeedAdapter::open(&file).expect("sample parses as a recorded feed");
        assert_eq!(
            adapter.remaining(),
            expected,
            "{}: adapter loaded every line",
            set.display()
        );

        let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
        gateway.add_adapter(Box::new(adapter));
        gateway.set_expected_adapters(1);
        let mut sink = SinkSpy::default();

        // Advance mission time in half-second ticks well past the last detection.
        let mut accepted = 0usize;
        let mut quarantined = Vec::new();
        let mut now = 0.0f64;
        while now < 24.0 * 3600.0 {
            for event in gateway.tick(MissionTime(now), &mut sink) {
                match event {
                    IngestEvent::Accepted(_) => accepted += 1,
                    IngestEvent::Quarantined { sensor, reason } => {
                        quarantined.push(format!("sensor {} at t={now}: {reason}", sensor.0));
                    }
                    // The spy always accepts, so this cannot happen here. Named rather
                    // than caught by a wildcard: a sample set that started being refused
                    // by the sink must fail this test loudly (GAP-066).
                    IngestEvent::NotAccepted { sensor, reason } => {
                        panic!(
                            "the sink refused a detection from sensor {}: {reason}",
                            sensor.0
                        )
                    }
                }
            }
            if accepted + quarantined.len() >= expected {
                break;
            }
            now += 0.5;
        }
        assert!(
            quarantined.is_empty(),
            "{}: {} quarantined, first: {:?}",
            set.display(),
            quarantined.len(),
            quarantined.iter().take(3).collect::<Vec<_>>()
        );
        assert_eq!(accepted, expected, "{}: every line accepted", set.display());
        assert_eq!(
            sink.received,
            expected,
            "{}: every line reached the tracking service",
            set.display()
        );
        assert!(
            gateway.is_healthy(),
            "{}: gateway healthy after replay",
            set.display()
        );
    }
}
