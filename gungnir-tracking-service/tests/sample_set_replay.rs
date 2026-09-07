//! A committed plan-07 sample set through the live pipeline (GAP-076).
//!
//! The GAP-076 closing action asks for a sample fed "through the whole-pipeline replay
//! once the pipeline exists". It exists (GAP-011), so this is that case.
//!
//! # How this differs from `whole_pipeline_replay.rs`
//!
//! That test is the `docs/verification-capability-table.md` §2 row *Whole-pipeline
//! scenario replay*, whose data source the table names as "Scenario-crate generated":
//! it replays the five `gungnir-scenario` engineering scenarios, each shaped to force a
//! particular set of §1 rows to run. **This test is not that row and does not widen
//! it.** Its data is the other half of what the workspace has: the ten committed
//! sample sets of `docs/test-tracks/`, produced by the plan-07 reference generator and
//! reproduced byte for byte by `gungnir_scenario::tracks` (GAP-016). They are the data
//! `gungnir-ingest` already replays as far as the gateway
//! (`gungnir-ingest/tests/test_track_samples.rs`); what was missing was the rest of the
//! journey, and a set that reaches the gateway but is never tracked says nothing about
//! whether the pipeline can consume it.
//!
//! Two things the sample sets carry that the generated scenarios do not:
//!
//! - **A receipt time that is not the source time.** The five-scenario test hands the
//!   service a view whose `receipt_time` is copied from the source timestamp; the
//!   ordering it exercises is the generator's hand-out order. A sample set records the
//!   latency the plan-07 sensor models produce, up to the generator's 4.9 s cap
//!   (`docs/test-tracks/sensor-models.md`), and the file is written in **receipt**
//!   order. The submission order is therefore out of source order by up to several
//!   seconds of real recorded latency, which is what the reorder buffer is for and what
//!   [`inversion_s`] measures and asserts is non-zero -- so the comparison below cannot
//!   pass by the two orders happening to be the same.
//! - **Sensor geometry and detection density from the mission library** rather than from
//!   a scenario written to exercise an estimator: seven to nine sensors per set, tens of
//!   thousands of metres of extent, and false alarms mixed into the same stream.
//!
//! # What is compared, and why 1e-6
//!
//! The same comparison the §2 row makes, for the same reason: the offline batch is the
//! same [`run_batch`] pipeline over the same detections in source order, so a difference
//! between it and the live service is the ordering, the channels and the reorder buffer
//! rather than a second opinion about the mathematics. The tightest §1 tolerance the
//! pipeline exercises is the linear Kalman row's 1e-6, and that is what is asserted.
//!
//! # No detection dropped
//!
//! Every line is submitted and every submission is checked. The reorder horizon is set
//! from each set's own recorded latency spread rather than left at the default, because
//! the horizon is a deployment setting: a default too small for a sensor set is a
//! misconfiguration, and the detections it refuses would show up here as a disagreement
//! about the mathematics, which they are not.

use gungnir_fusion_async::{run_batch, PipelineSettings};
use gungnir_tracking_service::{
    to_core_detection, Detection, DetectionView, LiveTrackingService, MissionTime, TrackingService,
};
use std::path::{Path, PathBuf};

/// The tightest §1 tolerance the pipeline exercises: the linear Kalman row's.
const TOL: f64 = 1e-6;

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("testdata")
        .join("tracks")
        .join("samples")
}

/// The committed sample sets, sorted so a failure names them in a stable order.
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

/// One set's detections, in the order the file records them, which is receipt order.
fn detections(set: &Path) -> Vec<DetectionView> {
    let text = std::fs::read_to_string(set.join("detections.jsonl")).expect("detections readable");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str::<DetectionView>(line)
                .unwrap_or_else(|e| panic!("{} line {}: {e}", set.display(), i + 1))
        })
        .collect()
}

/// How far the submission order runs backwards in source time, seconds.
///
/// Zero would mean the recording happens to be in source order too, and the whole point
/// of replaying a recorded feed rather than a generated one would be lost silently.
fn inversion_s(views: &[DetectionView]) -> f64 {
    let mut latest = f64::NEG_INFINITY;
    let mut worst = 0.0f64;
    for view in views {
        if view.source_time.0 < latest {
            worst = worst.max(latest - view.source_time.0);
        }
        latest = latest.max(view.source_time.0);
    }
    worst
}

#[test]
fn every_committed_sample_set_replays_through_the_service_to_the_offline_result() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let sets = sample_sets();
    assert!(
        sets.len() >= 10,
        "only {} sample sets under testdata/tracks/samples; \
         the plan-07 library should hold ten",
        sets.len()
    );

    for set in sets {
        let name = set
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("a sample set directory has a name");
        let views = detections(&set);
        assert!(!views.is_empty(), "{name}: the set records no detections");

        let inversion = inversion_s(&views);
        assert!(
            inversion > 0.0,
            "{name}: the recording is already in source order, so this replay would not \
             exercise the reorder buffer at all"
        );
        let spread = views
            .iter()
            .map(|v| v.receipt_time.0 - v.source_time.0)
            .fold(0.0f64, f64::max);
        let settings = PipelineSettings {
            reorder_horizon_s: spread.mul_add(2.0, 1.0),
            ..PipelineSettings::default()
        };

        // The offline result: the same pipeline, the same settings, source order.
        // Every sample set is a position feed, so every view converts; DN-27 §4's
        // angular variants would return `None` here, and a set that produced one would
        // fail this assertion rather than being quietly shortened.
        let core: Vec<Detection> = views.iter().filter_map(to_core_detection).collect();
        assert_eq!(
            core.len(),
            views.len(),
            "{name}: a sample-set observation was not a position"
        );
        let offline = run_batch(settings.clone(), &core);
        assert!(
            !offline.is_empty(),
            "{name}: the offline run produced no tracks, so the comparison below would \
             hold vacuously"
        );

        // The service: submitted in the file's receipt order, which is the order a live
        // deployment sees and is not source order wherever a sensor is late.
        let mut service = LiveTrackingService::with_pipeline_settings(runtime.handle(), settings);
        for view in &views {
            service
                .submit_detection(view.clone())
                .unwrap_or_else(|e| panic!("{name}: a detection was dropped: {e}"));
        }
        // End the stream, so the reorder buffer is flushed rather than leaving the last
        // horizon of the recording unprocessed.
        service.finish();

        let last_source = views
            .iter()
            .map(|v| v.source_time.0)
            .fold(f64::NEG_INFINITY, f64::max);
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
            service.poll(MissionTime(last_source));
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
            "{name}: {} detections, {} tracks, latency spread {spread:.2} s, \
             source-order inversion {inversion:.2} s",
            views.len(),
            live.len()
        );
    }
}
