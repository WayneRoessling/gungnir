// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The verification-capability-table.md §1 row
//! "`scenario` | Round-trip fidelity (generate→export→replay)".
//!
//! Method: generate a synthetic timeline in memory, export it to the recorded feed
//! format, replay it through the `RecordedFeedAdapter`, and compare against the
//! original in-memory stream. Pass criterion: **exact match on measurement content
//! and arrival order; timing within the export format's precision.**
//!
//! # Why this test lives here
//!
//! The row names the replay adapter, which belongs to `gungnir-ingest`, and the
//! export format is `gungnir_model::DetectionView`, which `gungnir-ingest` already
//! depends on. `gungnir-scenario` cannot host it: it deliberately does not depend on
//! `gungnir-model` (`ARCHITECTURE.md`, top-level graph), so it cannot construct the
//! exported type, and building a look-alike there would be the type duplication
//! `agentic-coding-standards.md` §1.2 forbids. `gungnir-scenario` is therefore a
//! **dev-dependency** of this crate; that adds no runtime edge and no cycle, and the
//! graph note in `ARCHITECTURE.md` lists it alongside `gungnir-oracle` and
//! `gungnir-mission`.
//!
//! # What "arrival order" means here
//!
//! The generator emits observations in *receipt* order, and the file is written in
//! that order (`docs/test-tracks/data-format.md` §3). `RecordedFeedAdapter::open`
//! then sorts by *source* time, because a replay releases each detection when mission
//! time reaches the moment the sensor observed it -- that is the adapter's documented
//! contract, not an accident of this test. So the comparison is against the original
//! stream in source-time order, and the test pins that the adapter really does
//! reorder rather than preserving file order, since a silent change there would make
//! every replay-driven row measure something else.

use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::ProtocolAdapter;
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId};
use gungnir_scenario::{GeneratedTimeline, Observation, Scenario, ScenarioGenerator};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::io::Write;

/// The generator's identity in the exported provenance, per `data-format.md` §3.
const ALGORITHM_VERSION: &str = "gungnir-scenario 0.1.0";

/// Times are written to millisecond precision and positions to 0.01 m
/// (`generation-method.md` §2), so a round trip may move a value by half of that.
const TIME_PRECISION_S: f64 = 1e-3;
const POSITION_PRECISION_M: f64 = 1e-2;
const TIME_DECIMALS: usize = 3;
const POSITION_DECIMALS: usize = 2;
/// The variance every exported observation carries, metres squared
/// (docs/design/DN-27-bearing-only-detections.md §8).
///
/// The tracking baseline's own default measurement noise
/// (`gungnir_fusion_async::PipelineSettings::measurement_noise_var`), which is what the
/// gate downstream assumed for every detection before `DetectionView` carried an error.
/// The generator states no per-sensor accuracy in the exported format, so this is the
/// only true thing to write here, and the round trip therefore compares exactly the
/// content it always did plus a constant.
const BASELINE_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// Quantize to the format's precision **through the decimal text**, not by
/// `(v / p).round() * p`.
///
/// The arithmetic form lands on a double that is up to an ulp away from the nearest
/// double to the printed decimal: `491.03` arrives back from JSON as one value and the
/// arithmetic rounding produces `491.03000000000003`. Going through the text makes the
/// in-memory value *be* the value the file holds, so the row's "exact match on
/// measurement content" is literally true after a round trip rather than true only to
/// within a tolerance invented here.
fn quantize(value: f64, decimals: usize) -> f64 {
    format!("{value:.decimals$}").parse().unwrap_or(value)
}

/// Export one generated observation as the `DetectionView` the recorded format holds.
fn to_detection_view(o: &Observation) -> DetectionView {
    DetectionView {
        sensor: SensorId(o.detection.sensor_id),
        source_time: MissionTime(quantize(o.detection.timestamp_s, TIME_DECIMALS)),
        receipt_time: MissionTime(quantize(o.receipt_time_s, TIME_DECIMALS)),
        // DN-27 §8: the scenario generator produces positions and keeps producing
        // them. The variance is the tracking baseline's default, which is what the
        // gate downstream assumed before `DetectionView` carried an error at all, so
        // the round trip compares the same content it always did plus a constant.
        measurement: gungnir_model::Measurement::Position {
            enu: o
                .detection
                .measurement
                .map(|v| quantize(v, POSITION_DECIMALS)),
            variance_m2: BASELINE_VARIANCE_M2,
        },
        provenance: Provenance {
            source_sensor_ids: vec![o.detection.sensor_id],
            calibration_baseline_version: o.calibration.clone(),
            algorithm_version: ALGORITHM_VERSION.to_owned(),
            peer: None,
            conversion_loss: None,
            authentication: gungnir_model::SourceAuthentication::default(),
        },
    }
}

/// Write a timeline to a JSON-lines file in receipt order, as `data-format.md` §3
/// specifies, and return the exported views in that same order.
fn export(timeline: &GeneratedTimeline, path: &std::path::Path) -> Vec<DetectionView> {
    let views: Vec<DetectionView> = timeline
        .observations
        .iter()
        .map(to_detection_view)
        .collect();
    let mut file = std::fs::File::create(path).expect("scratch file is writable");
    for view in &views {
        let line = serde_json::to_string(view).expect("DetectionView serializes");
        writeln!(file, "{line}").expect("scratch file is writable");
    }
    views
}

/// Drain a replay adapter completely by advancing mission time past the end.
fn drain(adapter: &mut RecordedFeedAdapter, end_s: f64) -> Vec<DetectionView> {
    let mut out = Vec::new();
    let mut now = 0.0_f64;
    // Step in one-second ticks so the poll boundary is exercised, then one final poll
    // well past the end to catch anything the loop's granularity left behind.
    while now <= end_s {
        out.extend(
            adapter
                .poll(MissionTime(now))
                .expect("replay poll succeeds"),
        );
        now += 1.0;
    }
    out.extend(
        adapter
            .poll(MissionTime(end_s + 3600.0))
            .expect("replay poll succeeds"),
    );
    out
}

fn generate(scenario: &Scenario, seed: u64) -> GeneratedTimeline {
    ScenarioGenerator::new(StdRng::seed_from_u64(seed)).generate(scenario)
}

/// The whole row, on the scenario built to make it hard: Scenario 3's three sensors
/// disagree about time, so file order and release order genuinely differ.
#[test]
fn round_trip_preserves_content_and_order() {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-scenario-round-trip-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch directory is creatable");
    let path = dir.join("urban-convoy.jsonl");

    let timeline = generate(
        &Scenario::UrbanConvoy {
            injected_bias_m: nalgebra::Vector3::new(40.0, -25.0, 5.0),
        },
        7,
    );
    let exported = export(&timeline, &path);
    assert!(!exported.is_empty(), "nothing was exported");

    let mut adapter = RecordedFeedAdapter::open(&path).expect("the exported file parses");
    assert_eq!(
        adapter.remaining(),
        exported.len(),
        "the adapter dropped or invented lines"
    );
    let replayed = drain(&mut adapter, timeline.duration_s + 10.0);

    // The expected stream: the exported views in source-time order, which is the
    // adapter's documented release order.
    let mut expected = exported.clone();
    expected.sort_by(|a, b| a.source_time.0.total_cmp(&b.source_time.0));

    assert_eq!(
        replayed.len(),
        expected.len(),
        "replayed {} of {} detections",
        replayed.len(),
        expected.len()
    );
    for (i, (got, want)) in replayed.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got.sensor, want.sensor, "sensor differs at index {i}");
        assert_eq!(
            got.measurement, want.measurement,
            "measurement differs at index {i}"
        );
        assert!(
            (got.source_time.0 - want.source_time.0).abs() <= TIME_PRECISION_S / 2.0,
            "source time differs at index {i} by more than the format's precision"
        );
        assert!(
            (got.receipt_time.0 - want.receipt_time.0).abs() <= TIME_PRECISION_S / 2.0,
            "receipt time differs at index {i} by more than the format's precision"
        );
        assert_eq!(
            got.provenance, want.provenance,
            "provenance differs at index {i}"
        );
    }

    // Release order is source-time order, and it is genuinely not file order: if these
    // ever coincide, Scenario 3 has stopped producing out-of-sequence arrivals and the
    // rows that depend on it are no longer being exercised.
    assert!(
        replayed
            .windows(2)
            .all(|w| w[0].source_time.0 <= w[1].source_time.0),
        "the adapter did not release in source-time order"
    );
    let file_order_differs = exported
        .iter()
        .zip(expected.iter())
        .any(|(a, b)| a.source_time != b.source_time);
    assert!(
        file_order_differs,
        "file order and release order are identical: no out-of-sequence data present"
    );

    let _ = std::fs::remove_file(&path);
}

/// Rounding to the format's precision must not lose a measurement: the criterion is
/// exact match on content, and the only permitted difference is the stated precision.
#[test]
fn export_precision_is_within_the_declared_format() {
    let timeline = generate(&Scenario::ManeuveringAircraft, 11);
    for observation in &timeline.observations {
        let view = to_detection_view(observation);
        for axis in 0..3 {
            let exported = view
                .measurement
                .position_enu()
                .expect("the generator produces positions");
            let moved = (exported[axis] - observation.detection.measurement[axis]).abs();
            assert!(
                moved <= POSITION_PRECISION_M / 2.0,
                "export moved a measurement by {moved} m, beyond the format's {POSITION_PRECISION_M} m"
            );
        }
        let moved_t = (view.source_time.0 - observation.detection.timestamp_s).abs();
        assert!(
            moved_t <= TIME_PRECISION_S / 2.0,
            "export moved a time by {moved_t} s"
        );
    }
}

/// Every exported line must survive the gateway that a live feed would face; an export
/// the gateway would quarantine is not a valid recorded feed.
#[test]
fn exported_lines_pass_the_gateway() {
    for scenario in [
        Scenario::ManeuveringAircraft,
        Scenario::MaritimeClutter {
            pd: 0.8,
            clutter_rate: 1.5,
        },
        Scenario::DenseSwarm { target_count: 30 },
    ] {
        let timeline = generate(&scenario, 23);
        for observation in &timeline.observations {
            let view = to_detection_view(observation);
            // The gateway checks against a "now" at or after receipt: a replay is fed
            // detections as mission time reaches them.
            gungnir_ingest::gateway::validate_detection(&view, view.receipt_time).unwrap_or_else(
                |e| {
                    panic!(
                        "scenario {} produced a detection the gateway rejects: {e}",
                        timeline.scenario_number
                    )
                },
            );
        }
    }
}
