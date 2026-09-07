// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Feature extraction from model views (`docs/ml/architecture.md` §4).
//!
//! Features come from `gungnir-model` views only, never a crate's internals, so the
//! extractor cannot drift from what the rest of the system sees. The window state
//! (per-track history) lives here, is bounded, and is cleared when a track is forgotten.
//! The column order is the schema: [`FEATURE_NAMES`] is written into every dataset and
//! every manifest, and [`SCHEMA_VERSION`] changes whenever it does.

use crate::{FeatureBatch, FeatureExtractor};
use gungnir_model::{MissionTime, TrackId, TrackView};
use std::collections::{HashMap, HashSet, VecDeque};

/// The ML-01 feature schema, version 1 (`docs/ml/data-pipeline.md` §2).
pub const SCHEMA_VERSION: u32 = 1;

/// The columns, in order.
pub const FEATURE_NAMES: [&str; 11] = [
    "speed_mps",
    "altitude_m",
    "climb_mps",
    "turn_rate_dps",
    "speed_var",
    "heading_var",
    "age_s",
    "association_confidence",
    "sensor_count",
    "sensor_mix",
    "cooperative_present",
];

/// One sample of a track's kinematics.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    at: MissionTime,
    speed: f64,
    heading: f64,
    altitude: f64,
    climb: f64,
}

/// Kinematic features over a bounded window per track.
#[derive(Debug)]
pub struct KinematicExtractor {
    window: usize,
    names: Vec<String>,
    history: HashMap<TrackId, VecDeque<Sample>>,
}

impl Default for KinematicExtractor {
    fn default() -> Self {
        Self::new(10)
    }
}

impl KinematicExtractor {
    /// `window` samples per track are kept; the statistics are over those.
    #[must_use]
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(2),
            names: FEATURE_NAMES.iter().map(|n| (*n).to_owned()).collect(),
            history: HashMap::new(),
        }
    }

    /// Fold this snapshot into the windows and extract one row per track.
    ///
    /// `cooperative` names the tracks a cooperative source (AIS, ADS-B) has declared.
    #[must_use]
    pub fn extract(
        &mut self,
        tracks: &[TrackView],
        cooperative: &HashSet<TrackId>,
    ) -> FeatureBatch {
        let mut ids = Vec::with_capacity(tracks.len());
        let mut rows = Vec::with_capacity(tracks.len());
        for track in tracks {
            let sample = sample_of(track);
            let window = self.history.entry(track.id).or_default();
            if window.len() >= self.window {
                window.pop_front();
            }
            window.push_back(sample);
            ids.push(track.id);
            rows.push(row(window, track, cooperative.contains(&track.id)));
        }
        FeatureBatch {
            feature_schema_version: SCHEMA_VERSION,
            feature_names: self.names.clone(),
            tracks: ids,
            rows,
        }
    }

    /// The track is gone; so is its window (§4).
    pub fn forget(&mut self, track: TrackId) {
        self.history.remove(&track);
    }

    #[must_use]
    pub fn tracked(&self) -> usize {
        self.history.len()
    }
}

impl FeatureExtractor for KinematicExtractor {
    fn schema_version(&self) -> u32 {
        SCHEMA_VERSION
    }

    fn feature_names(&self) -> &[String] {
        &self.names
    }
}

fn sample_of(track: &TrackView) -> Sample {
    let s = &track.state;
    let (ve, vn, vu) = (s[3], s[4], s[5]);
    Sample {
        at: track.mission_time,
        speed: (ve * ve + vn * vn).sqrt(),
        heading: ve.atan2(vn).to_degrees(),
        altitude: s[2],
        climb: vu,
    }
}

/// Variance of a sequence; zero for fewer than two values.
fn variance(values: impl Iterator<Item = f64> + Clone) -> f64 {
    let n = values.clone().count();
    if n < 2 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let n_f = n as f64;
    let mean = values.clone().sum::<f64>() / n_f;
    values.map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n_f - 1.0)
}

/// Heading change per second between the window's ends, degrees, wrapped to ±180.
fn turn_rate(window: &VecDeque<Sample>) -> f64 {
    let (Some(first), Some(last)) = (window.front(), window.back()) else {
        return 0.0;
    };
    let dt = last.at.0 - first.at.0;
    if dt <= 0.0 {
        return 0.0;
    }
    let mut d = last.heading - first.heading;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d / dt
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn row(window: &VecDeque<Sample>, track: &TrackView, cooperative: bool) -> Vec<f32> {
    let last = window.back().copied().unwrap_or_else(|| sample_of(track));
    let age_s = window.front().map_or(0.0, |first| last.at.0 - first.at.0);
    let sensors = &track.provenance.source_sensor_ids;
    let mut distinct: Vec<u32> = sensors.clone();
    distinct.sort_unstable();
    distinct.dedup();
    // The mix is a stable, order-free hash of the distinct sensor ids into `0..1`, so
    // "the same sensors" gives the same value and a model can learn per-mix behaviour
    // without the ids themselves meaning anything.
    let mix = if distinct.is_empty() {
        0.0
    } else {
        let h = distinct
            .iter()
            .fold(0u32, |acc, id| acc.wrapping_mul(31).wrapping_add(*id));
        f64::from(h % 1000) / 1000.0
    };
    vec![
        last.speed as f32,
        last.altitude as f32,
        last.climb as f32,
        turn_rate(window) as f32,
        variance(window.iter().map(|s| s.speed)) as f32,
        variance(window.iter().map(|s| s.heading)) as f32,
        age_s as f32,
        track.quality.association_confidence,
        distinct.len() as f32,
        mix as f32,
        if cooperative { 1.0 } else { 0.0 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, Provenance, Quality, Releasability, TrackStatus};

    fn track(id: u64, t: f64, ve: f64, vn: f64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::new(0.0, 0.0, 100.0, ve, vn, 0.0),
            covariance: nalgebra::SMatrix::identity(),
            classification: Classification::Unknown,
            provenance: Provenance {
                source_sensor_ids: vec![2, 1, 2],
                ..Provenance::default()
            },
            quality: Quality::default(),
            mission_time: MissionTime(t),
            releasability: Releasability::default(),
        }
    }

    #[test]
    fn the_window_is_bounded_and_a_turn_shows_in_the_rate() {
        let mut x = KinematicExtractor::new(3);
        let none = HashSet::new();
        for i in 0..5 {
            // Heading swings 10 degrees a second.
            let h = f64::from(i) * 10.0f64.to_radians();
            let _ = x.extract(
                &[track(1, f64::from(i), h.sin() * 10.0, h.cos() * 10.0)],
                &none,
            );
        }
        assert_eq!(x.history[&TrackId(1)].len(), 3);
        let batch = x.extract(&[track(1, 5.0, 0.0, 10.0)], &none);
        assert_eq!(batch.feature_names.len(), FEATURE_NAMES.len());
        assert_eq!(batch.rows[0].len(), FEATURE_NAMES.len());
        assert!(
            (batch.rows[0][8] - 2.0).abs() < f32::EPSILON,
            "two distinct sensors"
        );
        assert!(
            batch.rows[0][3].abs() > 1.0,
            "a turn rate, degrees per second"
        );
        x.forget(TrackId(1));
        assert_eq!(x.tracked(), 0);
    }
}
