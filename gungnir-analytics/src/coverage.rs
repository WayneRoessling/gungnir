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
use gungnir_model::{
    AcceptedGap, AzimuthSector, ElevationBand, GapAcceptance, Geodetic, LaydownId, LocalFrame,
    ReopenedBecause, SensorId,
};
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

/// Declared in `gungnir-model` since GAP-106, because a gap acceptance on the journal names
/// one (`docs/design/DN-33-accepting-a-coverage-gap.md` §3), and re-exported here where it
/// was computed all along.
pub use gungnir_model::GapSeverity;

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
/// sensor, which the frame's `+n` axis is only at the origin (GAP-118, D-84). So is its
/// vertical, which its elevation band is measured against (GAP-158, D-111).
///
/// `default_floor_rad` is the baseline's `analytics.coverage_min_elevation_rad`: the floor
/// of a sensor that declares no `elevation_band`, whose ceiling is then the zenith. A
/// sensor that declares a band is credited with that band alone.
pub fn coverage_from_registry(
    registry: &dyn SensorRegistry,
    default_floor_rad: f64,
    frame: &LocalFrame,
) -> Vec<(SensorId, CoverageVolume)> {
    registry
        .sensors()
        .iter()
        .filter(|s| matches!(s.mode, SensorMode::Search | SensorMode::Track))
        .map(|s| (s.id, volume_of(s, default_floor_rad, frame)))
        .collect()
}

/// One sensor's volume in `frame`, whatever mode it is in: for a caller asking what a
/// sensor *would* cover, such as a re-tasking candidate (DN-13).
///
/// Its band is the record's declared one, else `default_floor_rad` up to the zenith
/// ([`band_or_default`]).
#[must_use]
pub fn volume_of(
    record: &SensorRecord,
    default_floor_rad: f64,
    frame: &LocalFrame,
) -> CoverageVolume {
    volume_in_frame(
        frame,
        record.position,
        record.max_range_m,
        record.azimuth_sector,
        band_or_default(record.elevation_band, default_floor_rad),
    )
}

/// The band a sensor is credited with: the one stated, whole, or the baseline's floor up
/// to the zenith when none is (GAP-158, D-111).
///
/// A caller with a laydown passes the placement's band `or` the declaration's, so the
/// precedence is placement, then declaration, then the baseline -- and a band is always
/// taken whole from one of them, never a floor from one and a ceiling from another.
#[must_use]
pub fn band_or_default(stated: Option<ElevationBand>, default_floor_rad: f64) -> ElevationBand {
    stated.unwrap_or_else(|| ElevationBand::below_zenith(default_floor_rad))
}

/// A sensor's volume in `frame` from its declared geometry, each part placed where the
/// sensor stands: its position; its sector, surveyed against true north there
/// ([`LocalFrame::sector_in_frame`]); and its band, measured against its own vertical
/// ([`LocalFrame::vertical_at`]).
///
/// The one place a volume is built from a declaration, so the registry's path, DN-13's
/// candidates and a laydown's placements cannot place the same sensor differently.
#[must_use]
pub fn volume_in_frame(
    frame: &LocalFrame,
    position: Geodetic,
    max_range_m: f64,
    sector: Option<AzimuthSector>,
    band: ElevationBand,
) -> CoverageVolume {
    CoverageVolume {
        sensor_enu: frame.to_enu(position),
        max_range_m,
        min_elevation_rad: band.floor_rad,
        max_elevation_rad: band.ceiling_rad,
        vertical: frame.vertical_at(position),
        azimuth: sector.map(|sector| frame.sector_in_frame(sector, position)),
    }
}

/// A gap in a report as an acceptance names it (GAP-106,
/// `docs/design/DN-33-accepting-a-coverage-gap.md` §4): the approach by name, how it falls
/// short, where, and what the report was computed with.
#[must_use]
pub fn accepted_gap(
    gap: &CoverageGap,
    approach: &str,
    parameters: CoverageParameters,
) -> AcceptedGap {
    AcceptedGap {
        approach: approach.to_owned(),
        severity: gap.severity,
        from_m: gap.from_m,
        to_m: gap.to_m,
        sample_spacing_m: parameters.sample_spacing_m,
        terrain_masking_applied: parameters.terrain_masking_applied,
    }
}

/// Whether two named gaps are the same gap (DN-33 §5): the same approach, severity, sample
/// spacing and terrain-masking flag, and each end within **half a sample spacing**.
///
/// Every sample sits at a multiple of the spacing along the whole polyline ([`sample`]), and
/// a gap's ends are samples, so a real change moves an end by at least one spacing while
/// computing the same report again moves it only by rounding. Half a spacing is the widest
/// tolerance that cannot absorb a one-sample change. Two reports at different spacings, or
/// one flat and one masked, are different answers and never the same gap. The comparison
/// is exact for the spacing and the flag because both are read from the baseline, never
/// computed.
#[must_use]
// The spacing is compared exactly: it is the same configured value or it is not.
#[allow(clippy::float_cmp)]
pub fn same_gap(a: &AcceptedGap, b: &AcceptedGap) -> bool {
    let tolerance = a.sample_spacing_m / 2.0;
    a.approach == b.approach
        && a.severity == b.severity
        && a.sample_spacing_m == b.sample_spacing_m
        && a.terrain_masking_applied == b.terrain_masking_applied
        && tolerance.is_finite()
        && (a.from_m - b.from_m).abs() <= tolerance
        && (a.to_m - b.to_m).abs() <= tolerance
}

/// What a coverage report says about one acceptance (DN-33 §5).
#[derive(Debug, Clone, PartialEq)]
pub enum Standing {
    /// It holds, for this gap of the report.
    Holds { gap: usize },
    /// It stopped holding, and why.
    Reopens(ReopenedBecause),
}

/// Whether `acceptance` still holds (DN-33 §5), checked **in order**: the revision it was
/// made under, the laydown it was made under, then a gap of its shape in `report`.
///
/// `report` is the coverage report now, or the reason there is none. `approaches` are the
/// declared approaches' names, in the report's order. `revision` and `laydown` are what is
/// in force now: the running baseline's revision and the laydown it marks current.
#[must_use]
pub fn standing(
    acceptance: &GapAcceptance,
    report: Result<&CoverageReport, &str>,
    approaches: &[String],
    revision: u32,
    laydown: Option<&LaydownId>,
) -> Standing {
    if acceptance.revision != revision {
        return Standing::Reopens(ReopenedBecause::RevisionChanged {
            from: acceptance.revision,
            to: revision,
        });
    }
    if acceptance.laydown.as_ref() != laydown {
        return Standing::Reopens(ReopenedBecause::LaydownChanged {
            from: acceptance.laydown.clone(),
            to: laydown.cloned(),
        });
    }
    let report = match report {
        Ok(report) => report,
        Err(reason) => {
            return Standing::Reopens(ReopenedBecause::NotMeasured {
                reason: reason.to_owned(),
            })
        }
    };
    let named: Vec<(usize, AcceptedGap)> = report
        .gaps
        .iter()
        .enumerate()
        .filter_map(|(i, gap)| {
            let approach = approaches.get(gap.approach)?;
            Some((i, accepted_gap(gap, approach, report.parameters)))
        })
        .collect();
    if let Some((i, _)) = named.iter().find(|(_, g)| same_gap(&acceptance.gap, g)) {
        return Standing::Holds { gap: *i };
    }
    // What is there instead: every gap on the same approach that overlaps the accepted
    // stretch, whatever its severity. None at all means the stretch is covered now.
    let accepted = &acceptance.gap;
    Standing::Reopens(ReopenedBecause::ShapeChanged {
        now: named
            .into_iter()
            .map(|(_, g)| g)
            .filter(|g| {
                g.approach == accepted.approach
                    && g.from_m <= accepted.to_m
                    && g.to_m >= accepted.from_m
            })
            .collect(),
    })
}

/// DN-12's coverage measure with acceptance reported beside it, never taken out of it
/// (GAP-106, D-118, DN-33 §7): an accepted gap still counts, and the accepted part is a
/// part of each total rather than a subtraction from it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CoverageMeasure {
    /// Gap segments on the approaches, accepted or not.
    pub segments: usize,
    /// Of those, the segments a standing acceptance names.
    pub accepted_segments: usize,
    /// Approach metres covered by nothing, accepted or not.
    pub uncovered_m: f64,
    /// Of `uncovered_m`, the metres a standing acceptance names.
    pub uncovered_accepted_m: f64,
    /// Approach metres covered by one sensor only, accepted or not.
    pub single_sensor_m: f64,
    /// Of `single_sensor_m`, the metres a standing acceptance names.
    pub single_sensor_accepted_m: f64,
}

impl CoverageMeasure {
    /// The measure of `report`, where `accepted[i]` says whether `report.gaps[i]` is named
    /// by a standing acceptance; a gap with no entry is not.
    #[must_use]
    pub fn of(report: &CoverageReport, accepted: &[bool]) -> Self {
        let mut m = Self::default();
        for (i, gap) in report.gaps.iter().enumerate() {
            let is_accepted = accepted.get(i).copied().unwrap_or(false);
            let length = gap.to_m - gap.from_m;
            m.segments += 1;
            if is_accepted {
                m.accepted_segments += 1;
            }
            let (total, part) = match gap.severity {
                GapSeverity::Uncovered => (&mut m.uncovered_m, &mut m.uncovered_accepted_m),
                GapSeverity::SingleSensor => {
                    (&mut m.single_sensor_m, &mut m.single_sensor_accepted_m)
                }
            };
            *total += length;
            if is_accepted {
                *part += length;
            }
        }
        m
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
            max_elevation_rad: std::f64::consts::FRAC_PI_2,
            vertical: [0.0, 0.0, 1.0],
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
                elevation_band: None,
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
                elevation_band: None,
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
            elevation_band: None,
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

    /// A report whose one gap is where one sensor at `east` metres leaves a 2 km approach.
    fn report_with_sensor_at(east: f64) -> CoverageReport {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        combined_coverage(
            &[
                (SensorId(1), volume(east, 900.0)),
                (SensorId(2), volume(east, 900.0)),
            ],
            &los,
            &[route.as_slice()],
            parameters(),
        )
    }

    fn acceptance_of(report: &CoverageReport, gap: usize) -> GapAcceptance {
        GapAcceptance {
            id: gungnir_model::GapAcceptanceId(1),
            gap: accepted_gap(&report.gaps[gap], "east", report.parameters),
            reason: "the ridge closes it".into(),
            operator: "7".into(),
            role: "Commander".into(),
            revision: 3,
            laydown: Some(LaydownId("current".into())),
            at: gungnir_model::MissionTime(10.0),
        }
    }

    /// GAP-106, DN-33 §5: the tolerance absorbs rounding and not one sample. An end moved
    /// by a hair is the same gap; an end moved by one sample spacing either way is not; a
    /// different severity, spacing, masking or approach is never the same gap.
    #[test]
    #[allow(clippy::float_cmp)]
    fn the_same_gap_is_the_same_to_within_rounding_and_not_to_within_one_sample() {
        let report = report_with_sensor_at(0.0);
        let a = accepted_gap(&report.gaps[0], "east", report.parameters);
        assert_eq!(a.sample_spacing_m, 100.0);
        let moved = |from: f64, to: f64| AcceptedGap {
            from_m: a.from_m + from,
            to_m: a.to_m + to,
            ..a.clone()
        };
        assert!(same_gap(&a, &a));
        assert!(
            same_gap(&a, &moved(1e-6, -1e-6)),
            "rounding is not a change"
        );
        for one_sample in [100.0, -100.0] {
            assert!(!same_gap(&a, &moved(one_sample, 0.0)), "{one_sample}");
            assert!(!same_gap(&a, &moved(0.0, one_sample)), "{one_sample}");
        }
        assert!(!same_gap(
            &a,
            &AcceptedGap {
                severity: GapSeverity::SingleSensor,
                ..a.clone()
            }
        ));
        assert!(!same_gap(
            &a,
            &AcceptedGap {
                sample_spacing_m: 50.0,
                ..a.clone()
            }
        ));
        assert!(!same_gap(
            &a,
            &AcceptedGap {
                terrain_masking_applied: true,
                ..a.clone()
            }
        ));
        assert!(!same_gap(
            &a,
            &AcceptedGap {
                approach: "west".into(),
                ..a.clone()
            }
        ));
    }

    /// DN-33 §5: the checks run in order -- revision, laydown, shape -- and a changed shape
    /// names what is there now.
    #[test]
    fn an_acceptance_holds_until_the_revision_the_laydown_or_the_shape_changes() {
        let report = report_with_sensor_at(0.0);
        let names = vec!["east".to_owned()];
        let acceptance = acceptance_of(&report, 0);
        let current = LaydownId("current".into());
        assert_eq!(
            standing(&acceptance, Ok(&report), &names, 3, Some(&current)),
            Standing::Holds { gap: 0 }
        );
        assert_eq!(
            standing(&acceptance, Ok(&report), &names, 4, Some(&current)),
            Standing::Reopens(ReopenedBecause::RevisionChanged { from: 3, to: 4 })
        );
        let other = LaydownId("c".into());
        assert_eq!(
            standing(&acceptance, Ok(&report), &names, 3, Some(&other)),
            Standing::Reopens(ReopenedBecause::LaydownChanged {
                from: Some(current.clone()),
                to: Some(other),
            })
        );
        // Both sensors moved 300 m along: the gap on the approach is now 300 m shorter.
        let shifted = report_with_sensor_at(300.0);
        match standing(&acceptance, Ok(&shifted), &names, 3, Some(&current)) {
            Standing::Reopens(ReopenedBecause::ShapeChanged { now }) => {
                assert_eq!(now.len(), 1, "{now:?}");
                assert!(now[0].from_m > acceptance.gap.from_m, "{now:?}");
            }
            other => panic!("expected a shape change, got {other:?}"),
        }
        // The stretch covered: a sensor on each end and one in the middle.
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let covered = combined_coverage(
            &[
                (SensorId(1), volume(1_000.0, 5_000.0)),
                (SensorId(2), volume(1_000.0, 5_000.0)),
            ],
            &los,
            &[route.as_slice()],
            parameters(),
        );
        assert_eq!(
            standing(&acceptance, Ok(&covered), &names, 3, Some(&current)),
            Standing::Reopens(ReopenedBecause::ShapeChanged { now: Vec::new() })
        );
        assert!(matches!(
            standing(&acceptance, Err("no origin"), &names, 3, Some(&current)),
            Standing::Reopens(ReopenedBecause::NotMeasured { .. })
        ));
    }

    /// D-118: an accepted gap still counts, and the accepted part is reported beside the
    /// total rather than taken out of it.
    #[test]
    fn the_measure_reports_accepted_metres_inside_the_total() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        // One sensor at the west end: single-sensor for 900 m, then uncovered.
        let report = combined_coverage(
            &[(SensorId(1), volume(0.0, 900.0))],
            &los,
            &[route.as_slice()],
            parameters(),
        );
        assert_eq!(report.gaps.len(), 2, "{:?}", report.gaps);
        let none = CoverageMeasure::of(&report, &[]);
        let uncovered = report
            .gaps
            .iter()
            .position(|g| g.severity == GapSeverity::Uncovered)
            .expect("an uncovered stretch");
        let mut flags = vec![false; report.gaps.len()];
        flags[uncovered] = true;
        let some = CoverageMeasure::of(&report, &flags);
        assert!(
            (some.uncovered_m - none.uncovered_m).abs() < 1e-9,
            "never subtracted"
        );
        assert!((some.uncovered_accepted_m - none.uncovered_m).abs() < 1e-9);
        assert!(some.single_sensor_accepted_m.abs() < 1e-9);
        assert_eq!((some.segments, some.accepted_segments), (2, 1));
        assert_eq!(none.accepted_segments, 0);
    }
}
