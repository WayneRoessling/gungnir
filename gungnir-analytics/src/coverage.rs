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
use gungnir_model::SensorId;
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
fn sample(approach: &[[f64; 3]], spacing_m: f64) -> Vec<([f64; 3], f64)> {
    if approach.is_empty() || !(spacing_m.is_finite() && spacing_m > 0.0) {
        return Vec::new();
    }
    let mut out = vec![(approach[0], 0.0)];
    let mut along = 0.0;
    for pair in approach.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let segment = distance(a, b);
        if segment <= f64::EPSILON {
            continue;
        }
        // Walk the segment in fixed steps. `offset` stays an f64 throughout, so no
        // float-to-integer conversion is needed and a very long approach cannot
        // overflow a step counter.
        let mut offset = spacing_m;
        while offset <= segment {
            let t = offset / segment;
            out.push((
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ],
                along + offset,
            ));
            offset += spacing_m;
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
/// `to_enu` converts a sensor's geodetic position into the frame the approaches use;
/// the caller supplies it, because the conversion lives in `gungnir-coord`.
pub fn coverage_from_registry(
    registry: &dyn SensorRegistry,
    min_elevation_rad: f64,
    mut to_enu: impl FnMut(&SensorRecord) -> [f64; 3],
) -> Vec<(SensorId, CoverageVolume)> {
    registry
        .sensors()
        .iter()
        .filter(|s| matches!(s.mode, SensorMode::Search | SensorMode::Track))
        .map(|s| {
            (
                s.id,
                CoverageVolume {
                    sensor_enu: to_enu(s),
                    max_range_m: s.max_range_m,
                    min_elevation_rad,
                },
            )
        })
        .collect()
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
            },
            SensorConfig {
                id: 2,
                modality: "radar".into(),
                position: [0.0, 0.0, 0.0],
                max_range_m: 1_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            },
        ];
        let mut registry = InMemorySensorRegistry::from_config(&configs, "cal-1");
        registry
            .set_mode(SensorId(1), SensorMode::Search)
            .expect("searching");
        registry
            .set_mode(SensorId(2), SensorMode::Standby)
            .expect("standby");

        let volumes = coverage_from_registry(&registry, 0.0, |_| [0.0; 3]);
        assert_eq!(volumes.len(), 1, "a standby sensor contributes nothing");
        assert_eq!(volumes[0].0, SensorId(1));
    }
}
