// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Combined coverage and gap detection across the sensor registry.
//!
//! Design: docs/design/DN-12-coverage-and-gaps.md. Capability CAP-1.4; mission
//! threads MT-07 step 3 (restore coverage after a loss) and MT-09 (laydown).
//!
//! [`combined_coverage`] is a pure function over sensor volumes, so a property test
//! can drive it without a registry. [`coverage_from_registry`] is the convenience
//! that builds its input from live sensor records, and it is the reason this crate
//! gained an edge to `gungnir-sensor-management`: the engineering reviewer accepted
//! that edge on 2026-09-05, and it is drawn in ARCHITECTURE.md §7.1. If it is ever
//! withdrawn, only that one function moves, because the pure function takes volumes
//! rather than a registry.
//!
//! **Single-sensor coverage is a gap of its own severity, not coverage.** One sensor
//! gives a bearing and a range; it does not give a fusible track, and a laydown that
//! looks covered but is single-sensor everywhere fails on the first loss.

use crate::{CoverageVolume, LineOfSight};
use gungnir_model::{LocalFrame, SensorId};
use gungnir_sensor_management::{SensorMode, SensorRecord, SensorRegistry};

/// Coverage of one point by the registry as a whole.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PointCoverage {
    pub position_enu: [f64; 3],
    /// Sensors that cover it, after range, elevation, and terrain masking.
    pub sensors: Vec<SensorId>,
}

impl PointCoverage {
    /// Covered by at least two sensors, which is what makes a track fusible rather
    /// than merely detectable.
    pub fn is_redundant(&self) -> bool {
        self.sensors.len() >= 2
    }

    pub fn severity(&self) -> Option<GapSeverity> {
        match self.sensors.len() {
            0 => Some(GapSeverity::Uncovered),
            1 => Some(GapSeverity::SingleSensor),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum GapSeverity {
    /// Covered by one sensor only: detectable, not fusible.
    SingleSensor,
    /// Covered by nothing.
    Uncovered,
}

/// A hole along an approach: a contiguous run of samples below the threshold.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageGap {
    /// The approach this gap lies on, by index into the input.
    pub approach: usize,
    /// Distance along the approach at which the gap starts and ends, metres.
    pub from_m: f64,
    pub to_m: f64,
    /// Sample positions, so the viewport draws the gap and not a guess at it.
    pub samples: Vec<[f64; 3]>,
    pub severity: GapSeverity,
}

/// What a coverage run was computed with, carried on the result so a coarse run
/// cannot be mistaken for a fine one.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageParameters {
    pub sample_spacing_m: f64,
    /// True when a terrain model was available.
    ///
    /// A flat-terrain answer is optimistic by construction, and the difference in a
    /// valley is the whole answer, so its absence is reported rather than assumed
    /// away.
    pub terrain_masking_applied: bool,
}

/// The gaps found, with the parameters that found them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageReport {
    pub parameters: CoverageParameters,
    pub gaps: Vec<CoverageGap>,
}

impl CoverageReport {
    /// Approach metres that are uncovered or single-sensor.
    pub fn gap_length_m(&self, severity: GapSeverity) -> f64 {
        self.gaps
            .iter()
            .filter(|g| g.severity == severity)
            .map(|g| g.to_m - g.from_m)
            .sum()
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Samples one approach polyline at the given spacing, with the along-track
/// distance of each sample.
///
/// **The spacing is along the whole polyline, not restarted at each vertex** (GAP-118).
/// It was restarted, so a segment shorter than the spacing contributed no sample at all:
/// an approach drawn with vertices closer together than the spacing -- a curved axis, a
/// digitised route -- was judged by its first point alone, and a longer one was sampled
/// unevenly, each segment's remainder dropped at its end. The coverage-accuracy fixture's
/// arc probes found it.
fn sample(approach: &[[f64; 3]], spacing_m: f64) -> Vec<([f64; 3], f64)> {
    if approach.is_empty() || !(spacing_m.is_finite() && spacing_m > 0.0) {
        return Vec::new();
    }
    let mut out = vec![(approach[0], 0.0)];
    // Along-track distance at the start of the current segment.
    let mut along = 0.0;
    // The next sample is the `k`-th, at `k * spacing` along the whole polyline. `k` is
    // an f64 so no float-to-integer conversion is needed, and multiplying rather than
    // accumulating keeps a long approach's samples from drifting.
    let mut k = 1.0_f64;
    for pair in approach.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let segment = distance(a, b);
        if segment <= f64::EPSILON {
            continue;
        }
        loop {
            let at = k * spacing_m;
            if at > along + segment {
                break;
            }
            let t = (at - along) / segment;
            out.push((
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ],
                at,
            ));
            k += 1.0;
        }
        along += segment;
    }
    out
}

/// Coverage of a set of approaches by a set of sensors at one moment.
///
/// Takes sensor volumes rather than a registry, which is what keeps it a pure
/// function a property test can drive.
pub fn combined_coverage(
    sensors: &[(SensorId, CoverageVolume)],
    los: &dyn LineOfSight,
    approaches: &[&[[f64; 3]]],
    parameters: CoverageParameters,
) -> CoverageReport {
    let mut gaps = Vec::new();
    for (index, approach) in approaches.iter().enumerate() {
        let samples = sample(approach, parameters.sample_spacing_m);
        let mut run: Option<(GapSeverity, f64, f64, Vec<[f64; 3]>)> = None;
        for (position, along) in samples {
            let covering: Vec<SensorId> = sensors
                .iter()
                .filter(|(_, volume)| {
                    volume.covers(position) && los.visible(volume.sensor_enu, position)
                })
                .map(|(id, _)| *id)
                .collect();
            let severity = PointCoverage {
                position_enu: position,
                sensors: covering,
            }
            .severity();
            match (severity, &mut run) {
                (Some(s), Some(open)) if open.0 == s => {
                    open.2 = along;
                    open.3.push(position);
                }
                (Some(s), open) => {
                    if let Some(finished) = open.take() {
                        gaps.push(CoverageGap {
                            approach: index,
                            from_m: finished.1,
                            to_m: finished.2,
                            samples: finished.3,
                            severity: finished.0,
                        });
                    }
                    *open = Some((s, along, along, vec![position]));
                }
                (None, open) => {
                    if let Some(finished) = open.take() {
                        gaps.push(CoverageGap {
                            approach: index,
                            from_m: finished.1,
                            to_m: finished.2,
                            samples: finished.3,
                            severity: finished.0,
                        });
                    }
                }
            }
        }
        if let Some(finished) = run.take() {
            gaps.push(CoverageGap {
                approach: index,
                from_m: finished.1,
                to_m: finished.2,
                samples: finished.3,
                severity: finished.0,
            });
        }
    }
    CoverageReport { parameters, gaps }
}

/// Builds coverage input from live sensor records.
///
/// **Only sensors that are actually searching or tracking contribute.** A sensor at
/// standby gives nothing, which is what makes the coverage view useful during MT-07.
///
/// `frame` is the local frame the approaches are in. Each sensor's position is placed in
/// it, and so is its azimuth sector: a sector is surveyed against true north at the
/// sensor, which the frame's `+n` axis is only at the origin (GAP-118, D-84).
pub fn coverage_from_registry(
    registry: &dyn SensorRegistry,
    min_elevation_rad: f64,
    frame: &LocalFrame,
) -> Vec<(SensorId, CoverageVolume)> {
    registry
        .sensors()
        .iter()
        .filter(|s| matches!(s.mode, SensorMode::Search | SensorMode::Track))
        .map(|s| (s.id, volume_of(s, min_elevation_rad, frame)))
        .collect()
}

/// One sensor's volume in `frame`, whatever mode it is in: for a caller asking what a
/// sensor *would* cover, such as a re-tasking candidate (DN-13).
#[must_use]
pub fn volume_of(
    record: &SensorRecord,
    min_elevation_rad: f64,
    frame: &LocalFrame,
) -> CoverageVolume {
    CoverageVolume {
        sensor_enu: frame.to_enu(record.position),
        max_range_m: record.max_range_m,
        min_elevation_rad,
        azimuth: record
            .azimuth_sector
            .map(|sector| frame.sector_in_frame(sector, record.position)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FlatTerrainLineOfSight;

    fn volume(east: f64, range: f64) -> CoverageVolume {
        CoverageVolume {
            sensor_enu: [east, 0.0, 0.0],
            max_range_m: range,
            min_elevation_rad: -std::f64::consts::FRAC_PI_2,
            azimuth: None,
        }
    }

    fn parameters() -> CoverageParameters {
        CoverageParameters {
            sample_spacing_m: 100.0,
            terrain_masking_applied: false,
        }
    }

    /// A straight approach along the east axis from 0 to 2000 m.
    fn approach() -> Vec<[f64; 3]> {
        vec![[0.0, 0.0, 0.0], [2_000.0, 0.0, 0.0]]
    }

    #[test]
    fn a_point_inside_exactly_one_volume_is_single_sensor_not_covered() {
        let one = PointCoverage {
            position_enu: [0.0; 3],
            sensors: vec![SensorId(1)],
        };
        assert!(!one.is_redundant());
        assert_eq!(one.severity(), Some(GapSeverity::SingleSensor));

        let two = PointCoverage {
            position_enu: [0.0; 3],
            sensors: vec![SensorId(1), SensorId(2)],
        };
        assert!(two.is_redundant());
        assert_eq!(two.severity(), None, "redundant coverage is not a gap");
    }

    #[test]
    fn removing_a_sensor_never_shrinks_the_reported_gap_set() {
        // The monotonicity property that catches most coverage-routine bugs.
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let all = [
            (SensorId(1), volume(0.0, 900.0)),
            (SensorId(2), volume(1_000.0, 900.0)),
            (SensorId(3), volume(2_000.0, 900.0)),
        ];
        let full = combined_coverage(&all, &los, &approaches, parameters());
        for drop in 0..all.len() {
            let fewer: Vec<_> = all
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != drop)
                .map(|(_, s)| *s)
                .collect();
            let reduced = combined_coverage(&fewer, &los, &approaches, parameters());
            let full_uncovered = full.gap_length_m(GapSeverity::Uncovered);
            let reduced_uncovered = reduced.gap_length_m(GapSeverity::Uncovered);
            assert!(
                reduced_uncovered >= full_uncovered,
                "dropping sensor {drop} shrank the uncovered length"
            );
        }
    }

    #[test]
    fn a_single_covered_run_is_reported_as_a_gap_of_its_own_severity() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        // One sensor covers the whole approach: detectable, not fusible.
        let one = [(SensorId(1), volume(1_000.0, 5_000.0))];
        let report = combined_coverage(&one, &los, &approaches, parameters());
        assert!(report.gap_length_m(GapSeverity::SingleSensor) > 0.0);
        assert!(
            report.gap_length_m(GapSeverity::Uncovered).abs() < f64::EPSILON,
            "it is covered, just not redundantly"
        );
    }

    #[test]
    fn two_overlapping_sensors_leave_no_gap() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let both = [
            (SensorId(1), volume(1_000.0, 5_000.0)),
            (SensorId(2), volume(1_000.0, 5_000.0)),
        ];
        let report = combined_coverage(&both, &los, &approaches, parameters());
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn an_uncovered_approach_is_entirely_a_gap() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let report = combined_coverage(&[], &los, &approaches, parameters());
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(report.gaps[0].severity, GapSeverity::Uncovered);
        assert_eq!(report.gaps[0].approach, 0);
        assert!(
            !report.gaps[0].samples.is_empty(),
            "the viewport draws these"
        );
    }

    #[test]
    fn the_report_carries_its_parameters() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let report = combined_coverage(&[], &los, &[route.as_slice()], parameters());
        assert!((report.parameters.sample_spacing_m - 100.0).abs() < f64::EPSILON);
        assert!(
            !report.parameters.terrain_masking_applied,
            "a flat-terrain answer says so"
        );
    }

    /// GAP-118: an approach whose vertices are closer together than the spacing is
    /// sampled at the spacing along its whole length, not judged by its first point; and
    /// a long one is sampled evenly across its vertices rather than restarting at each.
    #[test]
    fn samples_are_spaced_along_the_whole_polyline_not_per_segment() {
        // 2 km in 200 segments of 10 m, sampled every 100 m.
        let dense: Vec<[f64; 3]> = (0..=200).map(|i| [f64::from(i) * 10.0, 0.0, 0.0]).collect();
        let samples = sample(&dense, 100.0);
        assert_eq!(samples.len(), 21, "0, 100, ..., 2000 m");
        for (i, (position, along)) in samples.iter().enumerate() {
            let expected = f64::from(u32::try_from(i).expect("small")) * 100.0;
            assert!((along - expected).abs() < 1e-6, "sample {i} at {along} m");
            assert!((position[0] - expected).abs() < 1e-6);
        }
        // Two 150 m segments at 100 m spacing: 0, 100, 200, 300 -- not 0, 100, 250.
        let bent = [[0.0, 0.0, 0.0], [150.0, 0.0, 0.0], [150.0, 150.0, 0.0]];
        let along: Vec<f64> = sample(&bent, 100.0).iter().map(|(_, a)| *a).collect();
        assert_eq!(along.len(), 4, "{along:?}");
        for (got, want) in along.iter().zip([0.0, 100.0, 200.0, 300.0]) {
            assert!((got - want).abs() < 1e-9, "{along:?}");
        }
        // And a sample on the second segment is on it, not on the first's extension.
        let second = sample(&bent, 100.0)[2].0;
        assert!((second[0] - 150.0).abs() < 1e-9 && (second[1] - 50.0).abs() < 1e-9);

        // The coverage answer changes with it: a sensor covering only the far end of a
        // densely drawn approach is found there.
        let los = FlatTerrainLineOfSight;
        let report = combined_coverage(
            &[(SensorId(1), volume(2_000.0, 500.0))],
            &los,
            &[dense.as_slice()],
            parameters(),
        );
        assert!(
            report.gap_length_m(GapSeverity::SingleSensor) > 0.0,
            "the covered far end was never sampled"
        );
    }

    #[test]
    fn a_non_positive_spacing_produces_no_samples_rather_than_looping() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        for bad in [0.0, -1.0, f64::NAN] {
            let report = combined_coverage(
                &[],
                &los,
                &[route.as_slice()],
                CoverageParameters {
                    sample_spacing_m: bad,
                    terrain_masking_applied: false,
                },
            );
            assert!(report.gaps.is_empty());
        }
    }

    #[test]
    fn only_searching_or_tracking_sensors_contribute() {
        use gungnir_config::SensorConfig;
        use gungnir_sensor_management::InMemorySensorRegistry;

        let configs = vec![
            SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.0, 0.0, 0.0],
                max_range_m: 1_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
                detection_model: None,
                azimuth_sector: None,
            },
            SensorConfig {
                id: 2,
                modality: "radar".into(),
                position: [0.0, 0.0, 0.0],
                max_range_m: 1_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
                detection_model: None,
                azimuth_sector: None,
            },
        ];
        let mut registry = InMemorySensorRegistry::from_config(&configs, "cal-1");
        registry
            .set_mode(SensorId(1), SensorMode::Search)
            .expect("searching");
        registry
            .set_mode(SensorId(2), SensorMode::Standby)
            .expect("standby");

        let frame = LocalFrame::new(gungnir_model::Geodetic {
            lat_rad: 0.0,
            lon_rad: 0.0,
            alt_m: 0.0,
        });
        let volumes = coverage_from_registry(&registry, 0.0, &frame);
        assert_eq!(volumes.len(), 1, "a standby sensor contributes nothing");
        assert_eq!(volumes[0].0, SensorId(1));
    }

    /// GAP-118, D-84: a sector surveyed on true north at a sensor well east of the origin
    /// reaches the volume rotated into the frame by the meridian convergence, and a sensor
    /// with no sector reaches it as the full circle.
    #[test]
    fn a_registry_sector_is_placed_in_the_frame() {
        use gungnir_config::SensorConfig;
        use gungnir_sensor_management::InMemorySensorRegistry;

        let origin = [45_f64.to_radians(), 10_f64.to_radians(), 0.0];
        // Half a degree of longitude east: about 39 km, a convergence of about 0.35 deg.
        let east = [origin[0], origin[1] + 0.5_f64.to_radians(), 0.0];
        let sector = gungnir_model::AzimuthSector::new(90_f64.to_radians(), 60_f64.to_radians())
            .expect("a legal sector");
        let config = |id: u32, position: [f64; 3], azimuth_sector| SensorConfig {
            id,
            modality: "radar".into(),
            position,
            max_range_m: 20_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
            azimuth_sector,
            detection_model: None,
        };
        let mut registry = InMemorySensorRegistry::from_config(
            &[config(1, east, Some(sector)), config(2, origin, None)],
            "cal-1",
        );
        registry
            .set_mode(SensorId(1), SensorMode::Track)
            .expect("track");
        registry
            .set_mode(SensorId(2), SensorMode::Track)
            .expect("track");
        let frame = LocalFrame::new(gungnir_model::Geodetic {
            lat_rad: origin[0],
            lon_rad: origin[1],
            alt_m: 0.0,
        });
        let volumes = coverage_from_registry(&registry, 0.0, &frame);
        let placed = volumes[0].1.azimuth.expect("the declared sector");
        let convergence = 0.5_f64.to_radians() * origin[0].sin();
        let turned = placed.boresight() - 90_f64.to_radians();
        assert!(
            (turned + convergence).abs() < 0.01_f64.to_radians(),
            "the boresight turned by {} deg, expected {} deg",
            turned.to_degrees(),
            -convergence.to_degrees()
        );
        assert!((placed.width_rad - 60_f64.to_radians()).abs() < 1e-12);
        assert_eq!(volumes[1].1.azimuth, None, "no sector is the full circle");
    }
}
