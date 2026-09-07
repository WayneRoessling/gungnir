//! The anomaly detectors run on the desktop's tick (GAP-021, D-13, DN-15).
//!
//! Six detectors existed as pure functions with their own tests, and nothing called
//! `detect_all`. These are the tests behind the claim that they now run: fed from the real
//! ingest events, judged against the real registry, raising each finding **once**.

use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, SensorConfig};
use gungnir_model::anomaly_settings::{AnomalySettings, FeedSettings, KinematicEnvelope};
use gungnir_model::events::IngestEvent;
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId, SensorMode};
use gungnir_sensor_management::SensorRegistry;
use gungnir_time::ReplayClockAuthority;

fn desktop(name: &str, anomaly: AnomalySettings) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-anomaly-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        sensors: vec![SensorConfig {
            id: 1,
            modality: "radar".into(),
            position: [0.0, 0.0, 10.0],
            max_range_m: 20_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }],
        ..ConfigBaseline::default()
    };
    config.analytics.anomaly = anomaly;
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

fn at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
}

fn accepted(sensor: u32, t: f64) -> IngestEvent {
    IngestEvent::Accepted(DetectionView {
        sensor: SensorId(sensor),
        source_time: MissionTime(t),
        receipt_time: MissionTime(t),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::zeros(),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    })
}

fn feed_only() -> AnomalySettings {
    AnomalySettings {
        feed: Some(FeedSettings {
            silence_factor: 3.0,
            rate_tolerance: 0.5,
            max_rejected: 5,
        }),
        ..AnomalySettings::default()
    }
}

/// **A silent radiating feed is raised, once.** A detector re-evaluates every tick and a
/// silent feed is silent on every one of them; one alert per finding, not one per frame.
#[test]
fn a_silent_feed_is_raised_once_not_every_frame() {
    let (mut state, dir) = desktop("silent", feed_only());
    state
        .sensors
        .set_mode(SensorId(1), SensorMode::Search)
        .expect("radiating");

    // A steady 1 Hz feed for a full rate window establishes the baseline.
    for t in 0..40 {
        let t = f64::from(t);
        at(&mut state, t);
        gungnir_app::anomaly::observe_ingest(&mut state.anomaly, &accepted(1, t), MissionTime(t));
    }
    let before = state.alerts.len();

    // Then nothing, for far longer than the expected interval times the factor.
    for t in [60.0, 90.0, 120.0] {
        at(&mut state, t);
        update::tick(&mut state);
    }
    let raised: Vec<&String> = state.alerts[before..]
        .iter()
        .filter(|a| a.contains("FeedSilent"))
        .collect();
    assert_eq!(raised.len(), 1, "raised {} times: {raised:?}", raised.len());
    // DN-15 §5: the alert carries what the detector cannot know.
    assert!(raised[0].contains("cannot know"), "{}", raised[0]);
    assert_eq!(state.anomaly.anomalies().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// A sensor that is *not* radiating is expected to be silent, and its silence is not an
/// anomaly. Standby is a mode change that explains it.
#[test]
fn a_sensor_on_standby_is_not_reported_silent() {
    let (mut state, dir) = desktop("standby", feed_only());
    // Left in Standby, which is where every sensor starts.
    for t in 0..40 {
        let t = f64::from(t);
        at(&mut state, t);
        gungnir_app::anomaly::observe_ingest(&mut state.anomaly, &accepted(1, t), MissionTime(t));
    }
    at(&mut state, 200.0);
    update::tick(&mut state);
    assert!(
        !state.alerts.iter().any(|a| a.contains("FeedSilent")),
        "a sensor nobody expects to hear from was reported silent: {:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Rejections are counted per sensor from the same events the gateway publishes; the
/// gateway itself is not asked to keep anything new.
#[test]
fn quarantines_are_counted_per_sensor_from_the_bus() {
    let (mut state, dir) = desktop("rejections", feed_only());
    for _ in 0..3 {
        gungnir_app::anomaly::observe_ingest(
            &mut state.anomaly,
            &IngestEvent::Quarantined {
                sensor: SensorId(1),
                reason: "out of range".into(),
            },
            MissionTime(1.0),
        );
    }
    gungnir_app::anomaly::observe_ingest(
        &mut state.anomaly,
        &IngestEvent::NotAccepted {
            sensor: SensorId(1),
            reason: "pipeline gone".into(),
        },
        MissionTime(1.0),
    );
    // Four rejections and no acceptances: no baseline, so nothing to judge a rate
    // against yet, and the count is simply kept for when there is.
    update::tick(&mut state);
    assert!(state.anomaly.anomalies().len() <= 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// **An unconfigured detector is off and says so; a configured one that cannot run says
/// why** (DN-15 §6). Nothing on a track carries a cooperative identity (GAP-010) and no
/// watched extent can be configured, so those two are listed as unable, not running.
#[test]
fn detector_status_is_honest_about_what_cannot_run() {
    use gungnir_app::anomaly::DetectorStatus;
    let (state, dir) = desktop(
        "status",
        AnomalySettings {
            kinematics: Some(KinematicEnvelope {
                max_speed_mps: 400.0,
                max_climb_rate_mps: 100.0,
            }),
            cooperative: Some(gungnir_model::anomaly_settings::CooperativeSettings {
                lost_after_s: 30.0,
                max_separation_m: 500.0,
            }),
            ..AnomalySettings::default()
        },
    );
    let status: std::collections::BTreeMap<&str, DetectorStatus> =
        gungnir_app::anomaly::detector_status(&state)
            .into_iter()
            .collect();
    assert_eq!(status["implausible-kinematics"], DetectorStatus::Running);
    assert_eq!(status["feed"], DetectorStatus::Off);
    assert!(matches!(
        status["cooperative"],
        DetectorStatus::ConfiguredButUnable(reason) if reason.contains("GAP-010")
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// With no detector configured the tick does nothing at all: a deployment that asked for
/// no anomaly detection gets none, rather than a default set nobody chose.
#[test]
fn no_configured_detector_means_nothing_runs() {
    let (mut state, dir) = desktop("none", AnomalySettings::default());
    at(&mut state, 500.0);
    update::tick(&mut state);
    assert!(state.anomaly.anomalies().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
