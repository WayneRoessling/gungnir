// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Track and feed anomaly detectors.
//!
//! Design: docs/design/DN-15-anomaly-detectors.md. Capability CAP-2.9; mission
//! threads MT-05 (a vessel that switched its transponder off, a report that does not
//! match its track, a craft loitering) and MT-07 (a sensor emitting implausible
//! data).
//!
//! **Decision D-13 governs the shape and this note does not reopen it.** Each
//! detector is a pure function over a snapshot; the binaries call them on the tick
//! and raise alerts through `gungnir-observability`. No new dependency edge.
//!
//! **Correction found during implementation, 2026-09-05.** DN-15 §3 typed the track
//! detectors as `fn(&[TrackView], ...)`, which would require
//! `gungnir-analytics` to depend on `gungnir-model` -- exactly the edge D-13
//! forbids, and one `ARCHITECTURE.md` does not draw. So [`TrackSnapshot`] carries
//! primitives on both sides of the boundary and the binary projects the model types
//! into it, the same way [`SensorHealthSnapshot`] already worked. Identifiers are
//! plain integers here and are mapped back to `TrackId` and `SensorId` by the
//! caller that raises the alert.
//!
//! Two rules hold across every detector and are tested rather than assumed:
//! **no detector alters a track, a feed, or a health flag** (they raise alerts; the
//! ingest gateway is what quarantines), and **every anomaly names the detector and
//! version that raised it**, so a noisy rule can be found and tuned.

/// What a detector found. Never a verdict: an anomaly is a reason to look, and the
/// operator decides what it means.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anomaly {
    pub kind: AnomalyKind,
    pub subject: AnomalySubject,
    /// Mission time, seconds, as the caller supplied it.
    pub observed_at_s: f64,
    /// Why the detector fired, in the words the alert shows. Never empty.
    pub detail: String,
    /// What the detector cannot know, shown with the alert so the operator sees the
    /// limit of the inference at the moment they act on it.
    pub limit: &'static str,
    /// Detector name and version, so an alert traces to the rule that raised it.
    /// A string rather than an enum because plan 09's learned detector puts a model
    /// name and version here (docs/ml/use-cases.md ML-04).
    pub detector: String,
}

/// Identifiers are primitives at this boundary; the caller maps them back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnomalySubject {
    Track(u64),
    Sensor(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnomalyKind {
    /// A cooperative identity that was present and stopped.
    CooperativeIdentityLost,
    /// A cooperative report that disagrees with the track.
    CooperativeMismatch,
    /// Persistent slow motion inside a watched area.
    Loitering,
    /// Kinematics outside the envelope of any known class.
    ImplausibleKinematics,
    /// A feed whose rate, timing, or values are outside its own history.
    FeedImplausible,
    /// A feed that stopped without a mode change explaining it.
    FeedSilent,
}

/// Everything a track detector needs, assembled by the binary on the tick.
///
/// Stateless detectors take whatever history they need from here rather than
/// remembering it, which is what makes them testable and replayable.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackSnapshot {
    pub track: u64,
    pub mission_time_s: f64,
    pub speed_mps: f64,
    /// Vertical rate, m/s; the sign is preserved so a dive reads as a dive.
    pub climb_rate_mps: f64,
    pub is_stale: bool,
    /// Seconds the track has been below the loitering speed.
    pub slow_for_s: f64,
    /// Seconds since a cooperative report was last associated; `None` when it never
    /// had one, which is not an anomaly but an uncooperative track.
    pub since_cooperative_s: Option<f64>,
    pub in_watched_area: bool,
    /// True when a cooperative report's declared identity disagrees with the
    /// picture's. The caller compares, because the classification type belongs to
    /// `gungnir-model`; the detector owns what to do about it.
    pub cooperative_disagrees: bool,
    /// Distance between the cooperative report and the fused track, metres;
    /// `None` when there is no report to compare.
    pub cooperative_separation_m: Option<f64>,
}

/// Everything a feed detector needs, assembled by the binary on the tick.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorHealthSnapshot {
    pub sensor: u32,
    /// Seconds since the last message from this sensor.
    pub since_last_message_s: f64,
    /// The interval this feed is expected to report at, seconds.
    pub expected_interval_s: f64,
    /// Messages per second over the recent window.
    pub message_rate_hz: f64,
    /// The rate this feed has been running at, for comparison with the current one.
    pub baseline_rate_hz: f64,
    /// Rejections by the ingest gateway over the recent window.
    pub rejected_recently: u32,
    /// True when a mode change explains the silence, so it is not anomalous.
    pub mode_change_explains_silence: bool,
}

/// Thresholds per detector.
///
/// A detector with no settings is **off**, and [`AnomalySettings::enabled`] lists
/// which are running, so an unconfigured detector is visibly absent rather than
/// silently missing.
// The settings live in `gungnir-model::anomaly_settings` (GAP-021): the baseline has to
// carry them and `gungnir-config` cannot depend on this crate. Re-exported, not
// redefined, per the standing rule about types the model owns.
pub use gungnir_model::anomaly_settings::{
    AnomalySettings, CooperativeSettings, FeedSettings, KinematicEnvelope, LoiteringSettings,
};

fn anomaly(
    kind: AnomalyKind,
    subject: AnomalySubject,
    observed_at_s: f64,
    detail: String,
    limit: &'static str,
    detector: &str,
) -> Anomaly {
    Anomaly {
        kind,
        subject,
        observed_at_s,
        detail,
        limit,
        detector: detector.to_string(),
    }
}

/// Loitering: persistent slow motion inside a watched area.
pub fn detect_loitering(tracks: &[TrackSnapshot], settings: &AnomalySettings) -> Vec<Anomaly> {
    let Some(s) = settings.loitering else {
        return Vec::new();
    };
    tracks
        .iter()
        .filter(|t| {
            t.in_watched_area
                && !t.is_stale
                && t.speed_mps <= s.max_speed_mps
                && t.slow_for_s >= s.min_duration_s
        })
        .map(|t| {
            anomaly(
                AnomalyKind::Loitering,
                AnomalySubject::Track(t.track),
                t.mission_time_s,
                format!(
                    "below {:.1} m/s for {:.0} s inside a watched area",
                    s.max_speed_mps, t.slow_for_s
                ),
                "whether loitering is suspicious here; the watched area is set by a person",
                "loitering@1",
            )
        })
        .collect()
}

/// Kinematics outside the envelope of any known class.
pub fn detect_implausible_kinematics(
    tracks: &[TrackSnapshot],
    settings: &AnomalySettings,
) -> Vec<Anomaly> {
    let Some(e) = settings.kinematics else {
        return Vec::new();
    };
    tracks
        .iter()
        .filter(|t| !t.is_stale)
        .filter_map(|t| {
            let climb = t.climb_rate_mps.abs();
            let reason = if t.speed_mps > e.max_speed_mps {
                format!("speed {:.0} m/s exceeds every class envelope", t.speed_mps)
            } else if climb > e.max_climb_rate_mps {
                format!("vertical rate {climb:.0} m/s exceeds every class envelope")
            } else {
                return None;
            };
            Some(anomaly(
                AnomalyKind::ImplausibleKinematics,
                AnomalySubject::Track(t.track),
                t.mission_time_s,
                reason,
                "whether the track is real and unusual or a tracking artefact",
                "implausible-kinematics@1",
            ))
        })
        .collect()
}

/// A cooperative identity that was present and stopped, and reports that disagree
/// with the track they claim to be.
pub fn detect_cooperative(tracks: &[TrackSnapshot], settings: &AnomalySettings) -> Vec<Anomaly> {
    let Some(s) = settings.cooperative else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for t in tracks.iter().filter(|t| !t.is_stale) {
        if let Some(since) = t.since_cooperative_s {
            if since >= s.lost_after_s {
                found.push(anomaly(
                    AnomalyKind::CooperativeIdentityLost,
                    AnomalySubject::Track(t.track),
                    t.mission_time_s,
                    format!("no cooperative report for {since:.0} s"),
                    "whether the transmitter failed or was switched off",
                    "cooperative-lost@1",
                ));
            }
        }
        let far_apart = t
            .cooperative_separation_m
            .is_some_and(|d| d > s.max_separation_m);
        if t.cooperative_disagrees || far_apart {
            let detail = match (far_apart, t.cooperative_separation_m) {
                (true, Some(d)) => {
                    format!("cooperative report is {d:.0} m from the fused track")
                }
                _ => "cooperative report disagrees with the picture's identity".to_string(),
            };
            found.push(anomaly(
                AnomalyKind::CooperativeMismatch,
                AnomalySubject::Track(t.track),
                t.mission_time_s,
                detail,
                "which of the two is wrong",
                "cooperative-mismatch@1",
            ));
        }
    }
    found
}

/// Feeds that stopped, or whose rate or rejections depart from their own history.
pub fn detect_feed_anomalies(
    feeds: &[SensorHealthSnapshot],
    now_s: f64,
    settings: &AnomalySettings,
) -> Vec<Anomaly> {
    let Some(s) = settings.feed else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for f in feeds {
        // Silence a mode change explains is not anomalous.
        if !f.mode_change_explains_silence
            && f.expected_interval_s > 0.0
            && f.since_last_message_s > f.expected_interval_s * s.silence_factor
        {
            found.push(anomaly(
                AnomalyKind::FeedSilent,
                AnomalySubject::Sensor(f.sensor),
                now_s,
                format!(
                    "silent for {:.0} s against an expected {:.0} s interval",
                    f.since_last_message_s, f.expected_interval_s
                ),
                "whether the link or the sensor failed",
                "feed-silent@1",
            ));
        }
        if f.rejected_recently > s.max_rejected {
            found.push(anomaly(
                AnomalyKind::FeedImplausible,
                AnomalySubject::Sensor(f.sensor),
                now_s,
                format!(
                    "{} messages rejected in the recent window",
                    f.rejected_recently
                ),
                "whether the sensor is faulty, jammed, or spoofed",
                "feed-implausible@1",
            ));
        } else if f.baseline_rate_hz > 0.0 {
            let departure = (f.message_rate_hz - f.baseline_rate_hz).abs() / f.baseline_rate_hz;
            if departure > s.rate_tolerance {
                found.push(anomaly(
                    AnomalyKind::FeedImplausible,
                    AnomalySubject::Sensor(f.sensor),
                    now_s,
                    format!(
                        "rate {:.2} Hz departs from its baseline {:.2} Hz",
                        f.message_rate_hz, f.baseline_rate_hz
                    ),
                    "whether the sensor is faulty, jammed, or spoofed",
                    "feed-implausible@1",
                ));
            }
        }
    }
    found
}

/// Every detector the settings enable, over one snapshot.
///
/// The binaries call this on the tick and raise each anomaly as an alert through
/// `gungnir-observability`. It returns anomalies and changes nothing, which is
/// decision D-13's whole point.
pub fn detect_all(
    tracks: &[TrackSnapshot],
    feeds: &[SensorHealthSnapshot],
    now_s: f64,
    settings: &AnomalySettings,
) -> Vec<Anomaly> {
    let mut all = detect_loitering(tracks, settings);
    all.extend(detect_implausible_kinematics(tracks, settings));
    all.extend(detect_cooperative(tracks, settings));
    all.extend(detect_feed_anomalies(feeds, now_s, settings));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: u64, speed_mps: f64, climb_rate_mps: f64, is_stale: bool) -> TrackSnapshot {
        TrackSnapshot {
            track: id,
            mission_time_s: 42.0,
            speed_mps,
            climb_rate_mps,
            is_stale,
            slow_for_s: 0.0,
            since_cooperative_s: None,
            in_watched_area: false,
            cooperative_disagrees: false,
            cooperative_separation_m: None,
        }
    }

    fn all_on() -> AnomalySettings {
        AnomalySettings {
            loitering: Some(LoiteringSettings {
                max_speed_mps: 2.0,
                min_duration_s: 120.0,
            }),
            kinematics: Some(KinematicEnvelope {
                max_speed_mps: 400.0,
                max_climb_rate_mps: 100.0,
            }),
            feed: Some(FeedSettings {
                silence_factor: 3.0,
                rate_tolerance: 0.5,
                max_rejected: 10,
            }),
            cooperative: Some(CooperativeSettings {
                lost_after_s: 60.0,
                max_separation_m: 200.0,
            }),
        }
    }

    fn feed(sensor: u32) -> SensorHealthSnapshot {
        SensorHealthSnapshot {
            sensor,
            since_last_message_s: 1.0,
            expected_interval_s: 1.0,
            message_rate_hz: 1.0,
            baseline_rate_hz: 1.0,
            rejected_recently: 0,
            mode_change_explains_silence: false,
        }
    }

    #[test]
    fn an_unconfigured_detector_produces_nothing_and_reports_itself_off() {
        let settings = AnomalySettings::default();
        assert!(settings.enabled().is_empty());
        let mut t = track(1, 0.5, 0.0, false);
        t.in_watched_area = true;
        t.slow_for_s = 600.0;
        assert!(detect_loitering(&[t], &settings).is_empty());
        assert!(detect_feed_anomalies(&[feed(1)], 0.0, &settings).is_empty());
        assert!(detect_all(&[t], &[feed(1)], 0.0, &settings).is_empty());
    }

    #[test]
    fn every_enabled_detector_is_listed_for_the_health_summary() {
        let on = all_on().enabled();
        assert_eq!(on.len(), 4);
        assert!(on.contains(&"loitering"));
        assert!(on.contains(&"feed"));
    }

    #[test]
    fn loitering_fires_on_its_positive_case_and_is_silent_otherwise() {
        let s = all_on();

        let mut positive = track(1, 0.5, 0.0, false);
        positive.in_watched_area = true;
        positive.slow_for_s = 300.0;
        let found = detect_loitering(&[positive], &s);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AnomalyKind::Loitering);

        let mut brief = positive;
        brief.slow_for_s = 10.0;
        assert!(
            detect_loitering(&[brief], &s).is_empty(),
            "slow but not long"
        );

        let mut elsewhere = positive;
        elsewhere.in_watched_area = false;
        assert!(
            detect_loitering(&[elsewhere], &s).is_empty(),
            "outside any watched area"
        );
    }

    #[test]
    fn implausible_kinematics_fires_on_speed_and_on_vertical_rate() {
        let s = all_on();
        assert_eq!(
            detect_implausible_kinematics(&[track(1, 900.0, 0.0, false)], &s).len(),
            1
        );
        assert_eq!(
            detect_implausible_kinematics(&[track(2, 10.0, -250.0, false)], &s).len(),
            1,
            "a dive is as implausible as a climb"
        );
        assert!(detect_implausible_kinematics(&[track(3, 80.0, 5.0, false)], &s).is_empty());
    }

    #[test]
    fn a_stale_track_is_never_reported() {
        let s = all_on();
        let mut t = track(1, 900.0, 0.0, true);
        t.in_watched_area = true;
        t.slow_for_s = 600.0;
        t.since_cooperative_s = Some(600.0);
        t.cooperative_disagrees = true;
        assert!(detect_all(&[t], &[], 0.0, &s).is_empty());
    }

    #[test]
    fn a_silent_feed_fires_unless_a_mode_change_explains_it() {
        let s = all_on();
        let mut silent = feed(1);
        silent.since_last_message_s = 30.0;
        let found = detect_feed_anomalies(&[silent], 0.0, &s);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AnomalyKind::FeedSilent);

        silent.mode_change_explains_silence = true;
        assert!(detect_feed_anomalies(&[silent], 0.0, &s).is_empty());
    }

    #[test]
    fn a_feed_rate_departing_from_its_own_history_fires() {
        let s = all_on();
        let mut drifting = feed(1);
        drifting.message_rate_hz = 5.0;
        let found = detect_feed_anomalies(&[drifting], 0.0, &s);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AnomalyKind::FeedImplausible);
    }

    #[test]
    fn a_cooperative_identity_that_stops_fires_but_one_that_never_existed_does_not() {
        let s = all_on();
        let mut lost = track(1, 10.0, 0.0, false);
        lost.since_cooperative_s = Some(300.0);
        let found = detect_cooperative(&[lost], &s);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AnomalyKind::CooperativeIdentityLost);

        // Never had one: not an anomaly, just an uncooperative track.
        assert!(detect_cooperative(&[track(2, 10.0, 0.0, false)], &s).is_empty());
    }

    #[test]
    fn a_cooperative_mismatch_fires_on_separation_or_on_disagreement() {
        let s = all_on();

        let mut far = track(1, 10.0, 0.0, false);
        far.cooperative_separation_m = Some(900.0);
        assert_eq!(detect_cooperative(&[far], &s).len(), 1);

        let mut disagrees = track(2, 10.0, 0.0, false);
        disagrees.cooperative_disagrees = true;
        assert_eq!(detect_cooperative(&[disagrees], &s).len(), 1);

        let mut agrees = track(3, 10.0, 0.0, false);
        agrees.cooperative_separation_m = Some(10.0);
        assert!(detect_cooperative(&[agrees], &s).is_empty());
    }

    #[test]
    fn every_anomaly_names_its_detector_and_states_what_it_cannot_know() {
        let s = all_on();
        let mut loiterer = track(1, 900.0, 0.0, false);
        loiterer.in_watched_area = true;
        loiterer.slow_for_s = 600.0;
        let mut silent = feed(1);
        silent.since_last_message_s = 30.0;
        let found = detect_all(&[loiterer], &[silent], 7.0, &s);
        assert!(!found.is_empty());
        for a in &found {
            assert!(!a.detail.is_empty(), "the alert must say why");
            assert!(!a.limit.is_empty(), "and what it cannot know");
            assert!(
                a.detector.contains('@'),
                "the detector names itself and its version: {}",
                a.detector
            );
            assert!((a.observed_at_s - 7.0).abs() < f64::EPSILON || a.observed_at_s > 0.0);
        }
    }
}
