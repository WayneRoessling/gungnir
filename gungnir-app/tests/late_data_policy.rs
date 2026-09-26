// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-114: the baseline's late-data policy governs the desktop's tracker.
//!
//! `gungnir-fusion-async/tests/late_data_policy.rs` shows each policy doing what it says
//! inside the pipeline. What these add is the wiring: the value an operator writes in
//! `time.late_data` is the one the desktop's pipeline runs -- at start, and on every path
//! that puts a running desktop back on its own services -- and PN-09 names it beside the
//! counters it produced.
//!
//! The same stream goes through each desktop: in order to t = 3, then a detection from
//! t = 2.5, then t = 4. Half a second late, it is dropped by a desktop that rejects late
//! data and reordered by one that buffers for a second, which is the whole observable
//! difference between the two baselines.

use gungnir_app::state::AppState;
use gungnir_config::{ConfigBaseline, TimeConfig};
use gungnir_model::{DetectionView, LateDataPolicy, MissionTime, Provenance, SensorId};
use gungnir_tracking_service::{PipelineStats, TrackingService};

fn view(t: f64) -> DetectionView {
    DetectionView {
        sensor: SensorId(1),
        source_time: MissionTime(t),
        receipt_time: MissionTime(t),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(100.0 * t, 0.0, 500.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

const STREAM: [f64; 6] = [0.0, 1.0, 2.0, 3.0, 2.5, 4.0];

/// Submit [`STREAM`] and poll until the pipeline has reported on every detection in it.
///
/// The bound is a deadlock guard, not a timing assertion: the pipeline runs on its own
/// task and reports when it has processed something, and nothing here depends on how fast.
fn deliver(tracking: &mut dyn TrackingService) -> PipelineStats {
    for t in STREAM {
        tracking
            .submit_detection(view(t))
            .expect("the pipeline is running");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        tracking.poll(MissionTime(4.0));
        let counted = tracking.pipeline_stats();
        if counted.accepted + counted.too_late >= 6 {
            return counted;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the pipeline never reported the whole stream: {counted:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn config(name: &str, late_data: LateDataPolicy) -> (ConfigBaseline, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-late-data-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (
        ConfigBaseline {
            data_dir: dir.to_string_lossy().into_owned(),
            time: TimeConfig { late_data },
            ..ConfigBaseline::default()
        },
        dir,
    )
}

#[test]
fn a_desktop_whose_baseline_rejects_late_data_drops_it_and_says_so() {
    let (config, dir) = config("reject", LateDataPolicy::Reject);
    let mut state = AppState::with_config(config).expect("the desktop starts");
    let counted = deliver(state.tracking.as_mut());
    assert_eq!((counted.too_late, counted.reordered), (1, 0), "{counted:?}");

    let line = gungnir_app::status::late_data_line(&state);
    assert_eq!(line.policy, Some(LateDataPolicy::Reject));
    assert_eq!(line.too_late, 1, "PN-09 reads the pipeline's own count");
    assert_eq!(
        state.clock.late_data_policy(),
        LateDataPolicy::Reject,
        "the clock judges skew against the policy the pipeline drops by"
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_desktop_with_no_policy_named_buffers_for_a_second_and_reorders() {
    let (config, dir) = config("default", LateDataPolicy::default());
    let mut state = AppState::with_config(config).expect("the desktop starts");
    let counted = deliver(state.tracking.as_mut());
    assert_eq!((counted.too_late, counted.reordered), (0, 1), "{counted:?}");
    assert_eq!(
        gungnir_app::status::late_data_line(&state).policy,
        Some(LateDataPolicy::BufferAndReorder {
            max_lateness_s: 1.0
        })
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

/// Falling back from a silent node, signing in to an outage and signing out of a link
/// all rebuild the tracker through `embedded_tracker`. Before GAP-114 they built it with
/// the defaults, so a desktop that rejected late data started buffering it the moment it
/// fell back.
#[test]
fn the_tracker_a_desktop_falls_back_to_runs_the_baselines_policy() {
    let (config, dir) = config("fallback", LateDataPolicy::Reject);
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let governance = gungnir_app::governance::Governance::from_config(&config);
    let mut tracker = gungnir_app::state::embedded_tracker(runtime.handle(), &config, &governance);
    let counted = deliver(&mut tracker);
    assert_eq!((counted.too_late, counted.reordered), (1, 0), "{counted:?}");
    let _ = std::fs::remove_dir_all(dir);
}
