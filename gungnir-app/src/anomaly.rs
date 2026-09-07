// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The anomaly detectors on the tick (GAP-021, D-13, DN-15).
//!
//! `gungnir-analytics` holds six detectors as pure functions over snapshots, tested and,
//! until this module, called by nothing. D-13 says the app and node tick call them and
//! raise alerts; this is the app's half.
//!
//! # What is real here, and what cannot be yet
//!
//! **Kinematics and feed detectors run on real input.** Speed and climb rate come from the
//! track state; feed rate, silence and rejections come from per-sensor statistics this
//! module keeps from the ingest events on the bus -- the gateway, which is human-owned, is
//! not asked to do anything new. **Loitering cannot fire**: DN-15 keys it on watched
//! extents and `LoiteringSettings` carries none, so `in_watched_area` is false for every
//! track and the detector is silent by its own rule 4. **Cooperative detectors cannot
//! fire**: no cooperative decoder exists (GAP-010), so no track has ever carried a
//! cooperative identity to lose. PN-09 lists which detectors are configured; these two
//! are listed as configured-but-unable, which is the state they are in.
//!
//! # One alert per anomaly, not one per frame
//!
//! A detector re-evaluates every tick, and a track that is too fast is too fast on every
//! one of them. An anomaly is raised once per (kind, subject) for the session; DN-15's
//! rule 2 -- every anomaly names its detector -- is what lets a reader group them.

use crate::state::AppState;
use gungnir_analytics::anomaly::{
    detect_all, Anomaly, AnomalySubject, SensorHealthSnapshot, TrackSnapshot,
};
use gungnir_model::events::IngestEvent;
use gungnir_model::{MissionTime, SensorId, TrackId};
use gungnir_sensor_management::SensorRegistry;
use std::collections::{BTreeMap, HashSet, VecDeque};

/// The window a feed's message rate is measured over, seconds.
const RATE_WINDOW_S: f64 = 30.0;

/// What this module remembers between frames.
#[derive(Debug, Default)]
pub struct AnomalyState {
    feeds: BTreeMap<u32, FeedStats>,
    /// Mission time at which each track was first seen below the loitering speed; cleared
    /// when it speeds up. Feeds `TrackSnapshot::slow_for_s`.
    slow_since: BTreeMap<TrackId, MissionTime>,
    /// Anomalies already raised this session, so each is said once.
    raised: HashSet<(gungnir_analytics::anomaly::AnomalyKind, AnomalySubjectKey)>,
    /// Everything raised, for PN-04 and the report.
    anomalies: Vec<Anomaly>,
}

/// `AnomalySubject` is not `Hash`; this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AnomalySubjectKey {
    Track(u64),
    Sensor(u32),
}

impl From<AnomalySubject> for AnomalySubjectKey {
    fn from(s: AnomalySubject) -> Self {
        match s {
            AnomalySubject::Track(t) => Self::Track(t),
            AnomalySubject::Sensor(s) => Self::Sensor(s),
        }
    }
}

/// Per-sensor receipt statistics, kept from ingest events.
#[derive(Debug, Default)]
struct FeedStats {
    receipts: VecDeque<MissionTime>,
    last_receipt: Option<MissionTime>,
    /// The first full window's rate, kept as the feed's own baseline (DN-15 rule 3: the
    /// snapshot carries a rate rather than the detector remembering one).
    baseline_rate_hz: Option<f64>,
    rejected_recently: u32,
}

impl AnomalyState {
    /// Everything raised this session.
    #[must_use]
    pub fn anomalies(&self) -> &[Anomaly] {
        &self.anomalies
    }

    /// Anomalies raised against one track, for PN-04.
    #[must_use]
    pub fn for_track(&self, track: TrackId) -> Vec<&Anomaly> {
        self.anomalies
            .iter()
            .filter(|a| a.subject == AnomalySubject::Track(track.0))
            .collect()
    }
}

/// Receipts in the rate window as a rate. The window holds at most a few thousand
/// receipts, so the conversion is exact.
fn rate_hz(receipts: usize) -> f64 {
    f64::from(u32::try_from(receipts).unwrap_or(u32::MAX)) / RATE_WINDOW_S
}

/// Feed one ingest event into the per-sensor statistics. Called from the tick as events
/// are published, so no second reader of the bus is needed.
pub fn observe_ingest(state: &mut AnomalyState, event: &IngestEvent, now: MissionTime) {
    match event {
        IngestEvent::Accepted(d) => {
            let feed = state.feeds.entry(d.sensor.0).or_default();
            feed.receipts.push_back(now);
            feed.last_receipt = Some(now);
            while feed
                .receipts
                .front()
                .is_some_and(|t| now.seconds_since(*t) > RATE_WINDOW_S)
            {
                feed.receipts.pop_front();
            }
            if feed.baseline_rate_hz.is_none() {
                if let Some(first) = feed.receipts.front() {
                    if now.seconds_since(*first) >= RATE_WINDOW_S {
                        feed.baseline_rate_hz = Some(rate_hz(feed.receipts.len()));
                    }
                }
            }
        }
        IngestEvent::Quarantined { sensor, .. } | IngestEvent::NotAccepted { sensor, .. } => {
            state.feeds.entry(sensor.0).or_default().rejected_recently += 1;
        }
    }
}

/// Run every configured detector over this frame's picture and raise what is new.
pub fn tick(state: &mut AppState) {
    let settings = state.config.analytics.anomaly;
    if settings.enabled().is_empty() {
        return;
    }
    let now = state.clock.now();
    let tracks = track_snapshots(state, now);
    let feeds = feed_snapshots(state, now);
    let found = detect_all(&tracks, &feeds, now.0, &settings);
    for anomaly in found {
        let key = (anomaly.kind, AnomalySubjectKey::from(anomaly.subject));
        if !state.anomaly.raised.insert(key) {
            continue;
        }
        // DN-15 §5: the limit of the inference goes on the alert, so the operator sees
        // it at the moment they act.
        state.alerts.push(format!(
            "{:?} on {:?}: {} ({}; what this cannot know: {})",
            anomaly.kind, anomaly.subject, anomaly.detail, anomaly.detector, anomaly.limit
        ));
        state.anomaly.anomalies.push(anomaly);
    }
}

fn track_snapshots(state: &mut AppState, now: MissionTime) -> Vec<TrackSnapshot> {
    let loiter_speed = state
        .config
        .analytics
        .anomaly
        .loitering
        .map(|l| l.max_speed_mps);
    let mut out = Vec::new();
    for t in state.tracking.tracks() {
        let v = &t.state;
        let speed = (v[3] * v[3] + v[4] * v[4] + v[5] * v[5]).sqrt();
        let slow_for_s = match loiter_speed {
            Some(limit) if speed < limit => {
                let since = *state.anomaly.slow_since.entry(t.id).or_insert(now);
                now.seconds_since(since)
            }
            _ => {
                state.anomaly.slow_since.remove(&t.id);
                0.0
            }
        };
        out.push(TrackSnapshot {
            track: t.id.0,
            mission_time_s: t.mission_time.0,
            speed_mps: speed,
            climb_rate_mps: v[5],
            is_stale: t.quality.is_stale,
            slow_for_s,
            // GAP-010: the last cooperative report associated with this track, if any.
            since_cooperative_s: state
                .cooperative
                .by_track
                .get(&t.id)
                .map(|c| now.seconds_since(c.at)),
            // No watched extents are configurable (DN-15 §6 names them; the settings
            // carry none), so no track is in one. The detector is silent by its rule 4.
            in_watched_area: false,
            cooperative_disagrees: state
                .cooperative
                .by_track
                .get(&t.id)
                .is_some_and(|c| c.disagrees),
            cooperative_separation_m: state
                .cooperative
                .by_track
                .get(&t.id)
                .map(|c| c.separation_m),
        });
    }
    out
}

fn feed_snapshots(state: &AppState, now: MissionTime) -> Vec<SensorHealthSnapshot> {
    state
        .sensors
        .sensors()
        .iter()
        .map(|record| {
            let feed = state.anomaly.feeds.get(&record.id.0);
            let last = feed.and_then(|s| s.last_receipt);
            let rate = feed.map_or(0.0, |s| rate_hz(s.receipts.len()));
            let baseline = feed.and_then(|s| s.baseline_rate_hz).unwrap_or(rate);
            SensorHealthSnapshot {
                sensor: record.id.0,
                since_last_message_s: last.map_or(0.0, |t| now.seconds_since(t)),
                // Expected interval is the inverse of the feed's own baseline rate; a feed
                // with no baseline yet has no expectation to be silent against.
                expected_interval_s: if baseline > 0.0 {
                    1.0 / baseline
                } else {
                    f64::INFINITY
                },
                message_rate_hz: rate,
                baseline_rate_hz: baseline,
                rejected_recently: feed.map_or(0, |s| s.rejected_recently),
                // A sensor that is not radiating is expected to be silent.
                mode_change_explains_silence: !matches!(
                    record.mode,
                    gungnir_model::SensorMode::Search | gungnir_model::SensorMode::Track
                ) || record.is_in_maintenance(now),
            }
        })
        .collect()
}

/// Which detectors are configured, and which of those cannot fire in this build, for
/// PN-09 (DN-15 §6: an unconfigured detector is visibly absent rather than silently
/// missing).
#[must_use]
pub fn detector_status(state: &AppState) -> Vec<(&'static str, DetectorStatus)> {
    let a = &state.config.analytics.anomaly;
    vec![
        (
            "loitering",
            if a.loitering.is_some() {
                DetectorStatus::ConfiguredButUnable("no watched extents can be configured yet")
            } else {
                DetectorStatus::Off
            },
        ),
        (
            "implausible-kinematics",
            if a.kinematics.is_some() {
                DetectorStatus::Running
            } else {
                DetectorStatus::Off
            },
        ),
        (
            "feed",
            if a.feed.is_some() {
                DetectorStatus::Running
            } else {
                DetectorStatus::Off
            },
        ),
        (
            "cooperative",
            if a.cooperative.is_some() && state.ais_sinks.is_empty() {
                DetectorStatus::ConfiguredButUnable("no AIS feed is bound (GAP-010)")
            } else {
                DetectorStatus::Off
            },
        ),
    ]
}

/// What PN-09 says about one detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorStatus {
    Off,
    Running,
    /// Configured, and unable to evaluate in this build; the reason is shown.
    ConfiguredButUnable(&'static str),
}

/// Placeholder to keep `SensorId` in scope for the doc-links above.
#[allow(dead_code)]
fn _sensor_id_is_used(_: SensorId) {}
