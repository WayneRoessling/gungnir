// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-205: a desktop on a GNSS-denied site, its clock disciplined from a named radar
//! (plan 12 tranche A).
//!
//! The case the gap exists for: the laptop's clock is behind the GPS-disciplined radar by
//! more than the gateway's one-second tolerance, so every radar report is quarantined as
//! "source time is ahead of receipt time" and none ever reaches the tracker. The desktop
//! hears the radar anyway, through the stamps a clock-rule quarantine carries, locks to
//! the radar's time, and from the next tick the radar's reports are accepted. The record
//! says waiting, then locked; PN-09's line says the same; and the skew estimate is
//! measured against the corrected clock rather than the old one.

use gungnir_app::state::AppState;
use gungnir_config::{ClockDisciplineConfig, ConfigBaseline, SensorConfig};
use gungnir_eventing::Event;
use gungnir_model::events::{HealthEvent, IngestEvent};
use gungnir_model::{ClockDisciplineState as S, DetectionView, MissionTime, Provenance, SensorId};
use gungnir_time::{DisciplinedClockAuthority, HostClock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const RADAR: u32 = 4;
/// How far the radar's GNSS time is ahead of this laptop's free-running clock.
const HOST_BEHIND_S: f64 = 40.0;

fn baseline(dir: &std::path::Path) -> ConfigBaseline {
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    config.sensors.push(SensorConfig {
        id: RADAR,
        modality: "radar".into(),
        position: [0.9, 0.2, 10.0],
        max_range_m: 20_000.0,
        control_endpoint: None,
        maintenance: Vec::new(),
        detection_model: None,
        azimuth_sector: None,
        elevation_band: None,
    });
    config.time.discipline = Some(ClockDisciplineConfig { sensor: RADAR });
    gungnir_config::validate(&config).expect("valid");
    config
}

/// A host clock the test steps, in milliseconds.
fn host_at(start_s: f64) -> (Arc<AtomicU64>, HostClock) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ms = Arc::new(AtomicU64::new((start_s * 1000.0) as u64));
    let read = ms.clone();
    #[allow(clippy::cast_precision_loss)]
    let host: HostClock = Arc::new(move || read.load(Ordering::SeqCst) as f64 / 1000.0);
    (ms, host)
}

/// A radar report emitted at the radar's (true) time `true_s`; the adapter stamps
/// receipt itself, as a live adapter does, from the gateway's `now`.
fn radar_report(true_s: f64) -> DetectionView {
    DetectionView {
        sensor: SensorId(RADAR),
        source_time: MissionTime(true_s),
        // Replaced by the stamping adapter below.
        receipt_time: MissionTime(0.0),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(1_000.0, 2_000.0, 300.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

#[test]
fn a_host_behind_its_radar_hears_it_through_the_quarantine_and_locks() {
    let dir = std::env::temp_dir().join(format!("gungnir-clock-discipline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut state = AppState::with_config(baseline(&dir)).expect("the desktop starts");
    // The desktop built a disciplined clock from the baseline; replace its host clock
    // with one the test steps, keeping the same discipline semantics.
    assert!(
        state.clock_discipline.is_some(),
        "the baseline names a discipline"
    );
    assert_eq!(state.clock.source(), gungnir_time::ClockSource::Disciplined);
    let (ms, host) = host_at(10_000.0);
    let (clock, handle) =
        DisciplinedClockAuthority::with_host(state.config.time.late_data, RADAR, host);
    state.clock = Box::new(clock);
    state.clock_discipline = Some(handle);
    let events = state.events.subscribe();
    let reports = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let feed = reports.clone();
    state.ingest.add_adapter(Box::new(Shared { feed }));

    // Tick 1: the radar's report is 40 s ahead of receipt, so the gateway quarantines it
    // under its clock rule, and the stamps it carries lock the clock.
    reports
        .lock()
        .expect("lock")
        .push_back(radar_report(10_000.0 + HOST_BEHIND_S - 0.02));
    gungnir_app::update::tick(&mut state);
    let first: Vec<_> = events.try_iter().map(|e| e.event).collect();
    assert!(
        first.iter().any(|e| matches!(
            e,
            Event::Ingest(IngestEvent::Quarantined {
                stamps: Some(_),
                ..
            })
        )),
        "the clock rule refused it, and said with what stamps: {first:?}"
    );
    let health: Vec<&str> = first
        .iter()
        .filter_map(|e| match e {
            Event::Health(HealthEvent::ClockDiscipline { state, .. }) => Some(state.kind()),
            _ => None,
        })
        .collect();
    assert_eq!(
        health,
        ["locked"],
        "locked on the first report it heard: {first:?}"
    );
    assert!(matches!(
        state
            .clock_discipline
            .as_ref()
            .map(gungnir_time::DisciplineHandle::state),
        Some(S::Locked { .. })
    ));
    assert!(
        (state.clock.now().0 - (10_000.0 + HOST_BEHIND_S - 0.02)).abs() < 1e-3,
        "mission time is the radar's"
    );

    // Tick 2, a second later on the host: the radar's report is accepted now.
    ms.store(10_001_000, Ordering::SeqCst);
    reports
        .lock()
        .expect("lock")
        .push_back(radar_report(10_001.0 + HOST_BEHIND_S - 0.02));
    gungnir_app::update::tick(&mut state);
    let second: Vec<_> = events.try_iter().map(|e| e.event).collect();
    assert!(
        second
            .iter()
            .any(|e| matches!(e, Event::Ingest(IngestEvent::Accepted(_)))),
        "accepted once the clock follows the radar: {second:?}"
    );
    // The skew estimate is measured against the corrected clock: near zero, not 40 s.
    let skew = state
        .clock_skew
        .skew_s(RADAR)
        .expect("heard since the lock");
    assert!(skew.abs() < 0.1, "{skew}");
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("Clock disciplined by sensor 4: locked")),
        "{:?}",
        state.alerts
    );
}

/// An adapter fed through a shared queue that, like a live one, stamps each report's
/// receipt with the gateway's `now` at the tick it is polled.
struct Shared {
    feed: Arc<std::sync::Mutex<std::collections::VecDeque<DetectionView>>>,
}

impl gungnir_ingest::ProtocolAdapter for Shared {
    fn name(&self) -> &'static str {
        "radar"
    }
    fn poll(
        &mut self,
        now: MissionTime,
    ) -> Result<Vec<DetectionView>, gungnir_ingest::IngestError> {
        let drained: Vec<DetectionView> = self
            .feed
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
