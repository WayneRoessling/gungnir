//! The `gungnir-tracking-service` row of `docs/verification-capability-table.md` §2:
//! "Whole-pipeline scenario replay".
//!
//! Method, verbatim: *replay each of the five scenarios through `TrackingService` and
//! compare the emitted track stream against the offline per-crate oracle results*.
//! Criterion: **track states within the tolerance of the tightest §1 row exercised; no
//! detection dropped**.
//!
//! # What the offline result is, and why it is the right comparison
//!
//! The pipeline exercises four §1 rows: the linear Kalman filter (state < 1e-6), the
//! Jonker-Volgenant assignment (exact), chi-square gating (exact membership), and the
//! track lifecycle (exact on step index). The tightest of those is **1e-6**, and that
//! is the tolerance asserted here rather than the looser 1e-4 the `fusion-async` row
//! carries; each of those four is separately gated against its own external oracle, so
//! the offline batch is a composition of already-verified parts rather than a second
//! opinion about the mathematics.
//!
//! What this row adds on top of them is the whole: that the service, its channels, its
//! task and its reorder buffer deliver the same picture as the same pipeline run
//! offline over the same detections in source order. The generator hands out
//! observations **in receipt order**, which for scenario 3 is genuinely out of order,
//! so the comparison is not trivially true.
//!
//! # No detection dropped
//!
//! Every observation is submitted and every submission is asserted to be accepted. The
//! pipeline's own refusal counter is not visible through the service, so what stands in
//! for it is the agreement itself: a refused detection moves the estimate that would
//! have used it, by hundreds of metres in the first case this test caught, and no
//! tolerance of 1e-6 survives that. The counter is asserted directly one layer down, in
//! `gungnir-fusion-async/tests/oos_convergence.rs`.
//!
//! The reorder horizon is set from each scenario's own latency spread rather than left
//! at the default, because the horizon is a deployment setting and a default too small
//! for a sensor set is a misconfiguration rather than a defect in the pipeline. What the
//! spread has to be is printed, so a deployment can see what its sensors demand.

use gungnir_fusion_async::{run_batch, PipelineSettings};
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId};
use gungnir_scenario::{Scenario, ScenarioGenerator};
use gungnir_tracking_service::{LiveTrackingService, TrackingService};
use rand::rngs::StdRng;
use rand::SeedableRng;

/// The tightest §1 tolerance the pipeline exercises: the linear Kalman row's.
const TOL: f64 = 1e-6;

/// The five scenarios `docs/test-tracks/` and `gungnir-scenario` define.
fn scenarios() -> Vec<(&'static str, Scenario)> {
    vec![
        ("1 maneuvering aircraft", Scenario::ManeuveringAircraft),
        (
            "2 maritime clutter",
            Scenario::MaritimeClutter {
                pd: 0.8,
                clutter_rate: 2.0,
            },
        ),
        (
            "3 urban convoy",
            Scenario::UrbanConvoy {
                injected_bias_m: nalgebra::Vector3::new(40.0, -25.0, 5.0),
            },
        ),
        ("4 dense swarm", Scenario::DenseSwarm { target_count: 40 }),
        (
            "5 adversarial geometry",
            Scenario::AdversarialGeometrySoak { cycles: 200 },
        ),
    ]
}

fn view(detection: &gungnir_fusion_async::Detection) -> DetectionView {
    DetectionView {
        sensor: SensorId(detection.sensor_id),
        source_time: MissionTime(detection.timestamp_s),
        receipt_time: MissionTime(detection.timestamp_s),
        measurement: gungnir_model::Measurement::Position {
            enu: detection.measurement,
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

#[test]
fn every_scenario_replays_through_the_service_to_the_offline_result() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    for (name, scenario) in scenarios() {
        let timeline = ScenarioGenerator::new(StdRng::seed_from_u64(7)).generate(&scenario);
        let detections: Vec<gungnir_fusion_async::Detection> = timeline
            .observations
            .iter()
            .map(|o| o.detection.clone())
            .collect();
        assert!(
            !detections.is_empty(),
            "{name}: the scenario produced no observations"
        );

        // The reorder horizon is a deployment setting, and it has to cover the
        // latency spread of the sensors it is deployed with or late measurements are
        // refused (which is the pipeline being honest, not the pipeline being wrong).
        // The scenario says what its spread is, so the test configures for it rather
        // than accepting the default and calling the resulting drops a disagreement.
        let spread = timeline
            .observations
            .iter()
            .map(|o| o.receipt_time_s - o.detection.timestamp_s)
            .fold(0.0f64, f64::max);
        let settings = PipelineSettings {
            reorder_horizon_s: spread.mul_add(2.0, 1.0),
            ..PipelineSettings::default()
        };

        // The offline result: the same pipeline, the same settings, source order.
        let offline = run_batch(settings.clone(), &detections);

        // The service: submitted in the generator's receipt order, which is the order a
        // live system sees and is not source order wherever a sensor is late.
        let mut service = LiveTrackingService::with_pipeline_settings(runtime.handle(), settings);
        for detection in &detections {
            service
                .submit_detection(view(detection))
                .unwrap_or_else(|e| panic!("{name}: a detection was dropped: {e}"));
        }
        // End the stream, so the reorder buffer is flushed rather than leaving the last
        // horizon of the replay unprocessed.
        service.finish();

        let mut ended = false;
        // **A deadlock guard, not a performance assertion.** This waits for the
        // background pipeline task to drain and report itself finished. At 2,000
        // iterations it was four seconds of wall clock, which the dense-swarm case
        // approaches in a debug build, so the suite failed whenever cargo ran several
        // test binaries at once -- a correctness test failing for want of CPU, which
        // says nothing about the pipeline and cost three people an afternoon between
        // them. Thirty thousand iterations is a minute: a real deadlock still fails,
        // and load no longer does. If this ever fires, it is a hang and not a slow
        // machine.
        for _ in 0..30_000 {
            service.poll(MissionTime(timeline.duration_s));
            if !service.is_healthy() {
                ended = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ended, "{name}: the pipeline task did not finish");

        let live = service.tracks();
        assert_eq!(
            live.len(),
            offline.len(),
            "{name}: the service reported {} tracks, the offline run {}",
            live.len(),
            offline.len()
        );
        for expected in &offline {
            let found = live
                .iter()
                .find(|t| t.id == expected.id)
                .unwrap_or_else(|| panic!("{name}: the service has no track {:?}", expected.id));
            assert_eq!(
                found.status, expected.status,
                "{name}: status of {:?}",
                expected.id
            );
            let worst = (found.state - expected.state).abs().max();
            assert!(
                worst < TOL,
                "{name}: track {:?} differs by {worst}, over the tightest §1 tolerance \
                 {TOL}",
                expected.id
            );
        }
        println!(
            "{name}: {} detections, {} tracks, latency spread {spread:.2} s",
            detections.len(),
            live.len()
        );
    }
}
