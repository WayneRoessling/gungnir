// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Coverage accuracy (GAP-118): the `gungnir-analytics` Coverage accuracy row of
//! `docs/verification-capability-table.md` §2.
//!
//! A fixture of sensor volumes with stated range, azimuth sector and elevation limits.
//! Each limit is **recovered from computed coverage** -- `combined_coverage`, the function
//! PN-11, PN-16, the node's `/v2/coverage` and the re-tasking search all call -- by probing
//! it along paths that cross the limit, and compared with the stated geometry against the
//! row's criterion: range within 1 percent, bearing and elevation within 0.1 degree.
//!
//! **How a limit is recovered.** One sensor at a time, so a sample is either covered by that
//! one sensor (`SingleSensor`) or by nothing (`Uncovered`), and `combined_coverage` reports
//! both as runs of samples in order. A limit lies between the last sample of one run and the
//! first of the next; the recovered value is the midpoint, so its error is at most half the
//! spacing. Range probes are rays sampled at exactly 1 percent of the range, the row's
//! coarsest allowed spacing; bearing and elevation probes are arcs at half the range sampled
//! at 0.1 percent of it, 0.11 degree between samples, because 1 percent there would be 1.1
//! degrees and could not resolve 0.1.
//!
//! **Two paths.** The first states each volume in the local frame and checks the
//! computation itself. The second declares sensors geodetically, sectors against true north
//! at each sensor as a survey states them, builds the volumes through
//! `coverage_from_registry` and a `LocalFrame`, and turns each recovered frame bearing back
//! to true north at the sensor: the path a deployment's baseline actually takes. One of its
//! sensors sits 39 km east of the origin, where true north and the frame's north differ by
//! 0.35 degree, so a sector placed without that rotation fails the criterion there.
//!
//! **What the elevation limit is.** A `CoverageVolume` has a lower elevation limit, measured
//! against the frame's vertical, and no upper one (the zenith). The fixture states the lower
//! limit and the test recovers it; per-sensor and upper elevation limits, and the tilt of a
//! distant sensor's own vertical against the frame's, are GAP-158.

use gungnir_analytics::{
    combined_coverage, coverage_from_registry, CoverageParameters, CoverageVolume, GapSeverity,
    LineOfSight,
};
use gungnir_model::{AzimuthSector, Geodetic, LocalFrame, SensorId, SensorMode};

/// Nothing masks anything: the row is about the volume, not the terrain.
struct Unobstructed;

impl LineOfSight for Unobstructed {
    fn visible(&self, _from: [f64; 3], _to: [f64; 3]) -> bool {
        true
    }
}

/// The row's criterion.
const RANGE_TOLERANCE_FRACTION: f64 = 0.01;
const ANGLE_TOLERANCE_DEG: f64 = 0.1;

/// One fixture volume, with the geometry it states.
struct Stated {
    name: &'static str,
    sensor_enu: [f64; 3],
    range_m: f64,
    min_elevation_deg: f64,
    /// `(boresight, width)` in degrees clockwise from north; `None` is the full circle.
    sector_deg: Option<(f64, f64)>,
}

impl Stated {
    fn sector(&self) -> Option<AzimuthSector> {
        self.sector_deg.map(|(boresight, width)| {
            AzimuthSector::new(boresight.to_radians(), width.to_radians())
                .expect("the fixture states legal sectors")
        })
    }

    fn volume(&self) -> CoverageVolume {
        CoverageVolume {
            sensor_enu: self.sensor_enu,
            max_range_m: self.range_m,
            min_elevation_rad: self.min_elevation_deg.to_radians(),
            azimuth: self.sector(),
        }
    }

    /// A bearing inside the sector to probe range and elevation along.
    fn probe_bearing_deg(&self) -> f64 {
        self.sector_deg.map_or(123.0, |(boresight, _)| boresight)
    }

    /// An elevation comfortably inside the volume.
    fn probe_elevation_deg(&self) -> f64 {
        self.min_elevation_deg + 5.0
    }
}

/// The frame-stated fixture: a panel radar, a sector straddling north, a narrow sector off
/// the cardinal points, a full circle by omission, the full circle stated, and a sector that
/// is almost the full circle -- its gap is a 10 degree notch.
fn fixture() -> Vec<Stated> {
    vec![
        Stated {
            name: "panel radar, north-east",
            sensor_enu: [0.0, 0.0, 50.0],
            range_m: 12_000.0,
            min_elevation_deg: -2.0,
            sector_deg: Some((45.0, 90.0)),
        },
        Stated {
            name: "sector across north",
            sensor_enu: [5_000.0, -3_000.0, 20.0],
            range_m: 8_000.0,
            min_elevation_deg: 0.5,
            sector_deg: Some((350.0, 40.0)),
        },
        Stated {
            name: "narrow sector off the cardinal points",
            sensor_enu: [-2_500.0, 7_500.0, 5.0],
            range_m: 20_000.0,
            min_elevation_deg: 3.0,
            sector_deg: Some((200.3, 7.5)),
        },
        Stated {
            name: "full circle by omission",
            sensor_enu: [-4_000.0, 2_000.0, 10.0],
            range_m: 5_000.0,
            min_elevation_deg: -1.0,
            sector_deg: None,
        },
        Stated {
            name: "full circle stated",
            sensor_enu: [1_000.0, 1_000.0, 0.0],
            range_m: 3_000.0,
            min_elevation_deg: 10.0,
            sector_deg: Some((0.0, 360.0)),
        },
        Stated {
            name: "all but a notch",
            sensor_enu: [300.0, -700.0, 15.0],
            range_m: 15_000.0,
            min_elevation_deg: -4.5,
            sector_deg: Some((275.0, 350.0)),
        },
    ]
}

fn point(sensor: [f64; 3], slant_m: f64, bearing_deg: f64, elevation_deg: f64) -> [f64; 3] {
    let (b, e) = (bearing_deg.to_radians(), elevation_deg.to_radians());
    [
        sensor[0] + slant_m * e.cos() * b.sin(),
        sensor[1] + slant_m * e.cos() * b.cos(),
        sensor[2] + slant_m * e.sin(),
    ]
}

/// The transitions along one probe: for each change between covered and uncovered, the
/// midpoint of the last sample before it and the first after it, with whether the probe
/// was entering coverage there.
fn transitions(
    volume: CoverageVolume,
    probe: &[[f64; 3]],
    spacing_m: f64,
) -> Vec<([f64; 3], bool)> {
    let report = combined_coverage(
        &[(SensorId(1), volume)],
        &Unobstructed,
        &[probe],
        CoverageParameters {
            sample_spacing_m: spacing_m,
            terrain_masking_applied: false,
        },
    );
    assert!(
        (report.parameters.sample_spacing_m - spacing_m).abs() < f64::EPSILON,
        "the report carries the spacing it was computed at"
    );
    report
        .gaps
        .windows(2)
        .map(|pair| {
            let before = *pair[0].samples.last().expect("a run has samples");
            let after = pair[1].samples[0];
            let midpoint = [
                f64::midpoint(before[0], after[0]),
                f64::midpoint(before[1], after[1]),
                f64::midpoint(before[2], after[2]),
            ];
            (midpoint, pair[1].severity == GapSeverity::SingleSensor)
        })
        .collect()
}

fn slant(from: [f64; 3], to: [f64; 3]) -> f64 {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn bearing_deg(from: [f64; 3], to: [f64; 3]) -> f64 {
    gungnir_model::bearing_rad(to[0] - from[0], to[1] - from[1])
        .expect("a probe point off the vertical")
        .to_degrees()
}

fn elevation_deg(from: [f64; 3], to: [f64; 3]) -> f64 {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    d[2].atan2((d[0] * d[0] + d[1] * d[1]).sqrt()).to_degrees()
}

/// Smallest signed difference between two bearings, degrees.
fn bearing_error_deg(a: f64, b: f64) -> f64 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

/// A ray from the sensor out to one and a half times the range, sampled at exactly 1
/// percent of the range.
fn recover_range(volume: CoverageVolume, bearing_deg: f64, elevation_deg: f64) -> f64 {
    let s = volume.sensor_enu;
    let ray = [
        s,
        point(s, 1.5 * volume.max_range_m, bearing_deg, elevation_deg),
    ];
    let spacing = 0.01 * volume.max_range_m;
    let found = transitions(volume, &ray, spacing);
    assert_eq!(found.len(), 1, "one limit along a ray out of the volume");
    assert!(!found[0].1, "the ray leaves coverage there");
    slant(s, found[0].0)
}

/// A horizontal circle at half the range, at `elevation_deg` from the sensor, swept
/// clockwise from `start_deg` through the full circle. Vertices every 0.05 degree; samples
/// every 0.1 percent of the range.
fn circle_probe(
    volume: CoverageVolume,
    start_deg: f64,
    elevation_deg: f64,
) -> Vec<([f64; 3], bool)> {
    let s = volume.sensor_enu;
    let r = 0.5 * volume.max_range_m;
    let arc: Vec<[f64; 3]> = (0..=7_200)
        .map(|i| point(s, r, start_deg + f64::from(i) * 0.05, elevation_deg))
        .collect();
    transitions(volume, &arc, 0.001 * volume.max_range_m)
}

/// Both edges of a sector, recovered: the sweep starts opposite the boresight, so it
/// enters the sector at its anticlockwise edge and leaves at its clockwise one.
fn recover_sector(volume: CoverageVolume, stated: &Stated) -> Option<(f64, f64)> {
    let (boresight, _) = stated.sector_deg?;
    let found = circle_probe(volume, boresight + 180.0, stated.probe_elevation_deg());
    if found.is_empty() {
        return None;
    }
    assert_eq!(found.len(), 2, "{}: a sector has two edges", stated.name);
    assert!(found[0].1 && !found[1].1, "{}: in, then out", stated.name);
    let s = volume.sensor_enu;
    Some((bearing_deg(s, found[0].0), bearing_deg(s, found[1].0)))
}

/// A vertical arc at half the range, on `bearing_deg`, from ten degrees below the stated
/// limit to ten above; samples every 0.1 percent of the range.
fn recover_min_elevation(volume: CoverageVolume, bearing_deg: f64, stated_deg: f64) -> f64 {
    let s = volume.sensor_enu;
    let r = 0.5 * volume.max_range_m;
    let from = (stated_deg - 10.0).max(-89.0);
    let arc: Vec<[f64; 3]> = (0..=400)
        .map(|i| point(s, r, bearing_deg, from + f64::from(i) * 0.05))
        .collect();
    let found = transitions(volume, &arc, 0.001 * volume.max_range_m);
    assert_eq!(found.len(), 1, "one elevation limit on the arc");
    assert!(found[0].1, "the arc climbs into coverage there");
    elevation_deg(s, found[0].0)
}

fn check_range(name: &str, recovered: f64, stated: f64) {
    let error = (recovered - stated).abs() / stated;
    assert!(
        error <= RANGE_TOLERANCE_FRACTION,
        "{name}: range recovered as {recovered:.1} m against {stated} m stated ({:.3} %)",
        error * 100.0
    );
}

fn check_angle(name: &str, what: &str, recovered_deg: f64, stated_deg: f64) {
    let error = bearing_error_deg(recovered_deg, stated_deg).abs();
    assert!(
        error <= ANGLE_TOLERANCE_DEG,
        "{name}: {what} recovered as {recovered_deg:.4} deg against {stated_deg} deg stated \
         ({error:.4} deg)"
    );
}

/// Every stated limit of every fixture volume, recovered from computed coverage in the
/// frame the volume is stated in, within the criterion.
#[test]
fn every_stated_limit_is_recovered_within_the_criterion() {
    for stated in fixture() {
        let volume = stated.volume();
        let bearing = stated.probe_bearing_deg();

        let range = recover_range(volume, bearing, stated.probe_elevation_deg());
        check_range(stated.name, range, stated.range_m);

        let elevation = recover_min_elevation(volume, bearing, stated.min_elevation_deg);
        check_angle(
            stated.name,
            "the minimum elevation",
            elevation,
            stated.min_elevation_deg,
        );

        match (stated.sector_deg, recover_sector(volume, &stated)) {
            (Some((boresight, width)), Some((start, end))) if width < 360.0 => {
                check_angle(
                    stated.name,
                    "the sector's start",
                    start,
                    boresight - width / 2.0,
                );
                check_angle(
                    stated.name,
                    "the sector's end",
                    end,
                    boresight + width / 2.0,
                );
            }
            (Some((_, width)), None) if width >= 360.0 => {}
            (None, _) => {
                // No sector: the whole circle is covered, with no edge anywhere on it.
                let found = circle_probe(volume, 0.0, stated.probe_elevation_deg());
                assert!(
                    found.is_empty(),
                    "{}: a volume with no sector has a bearing limit at {:?}",
                    stated.name,
                    found
                );
            }
            (sector, recovered) => panic!(
                "{}: stated sector {sector:?}, recovered {recovered:?}",
                stated.name
            ),
        }
    }
}

/// The spacing matters and the criterion's bound is the coarsest that passes: the same
/// range probe at 1 percent recovers within 1 percent, and within half of it, because the
/// recovered limit is a midpoint.
#[test]
fn a_range_probe_at_the_coarsest_allowed_spacing_is_within_half_a_sample() {
    for stated in fixture() {
        let range = recover_range(
            stated.volume(),
            stated.probe_bearing_deg(),
            stated.probe_elevation_deg(),
        );
        assert!(
            (range - stated.range_m).abs() <= 0.005 * stated.range_m + 1e-6,
            "{}: {range} m against {} m",
            stated.name,
            stated.range_m
        );
    }
}

/// The deployment's path: sensors declared geodetically with sectors against true north at
/// each sensor, volumes built by `coverage_from_registry` in a `LocalFrame`, and every
/// recovered bearing turned back to true north at the sensor before it is compared.
#[test]
#[allow(clippy::too_many_lines)] // one fixture, stated in full where it is checked
fn a_baseline_sector_is_recovered_against_true_north_through_the_frame() {
    use gungnir_config::SensorConfig;
    use gungnir_sensor_management::{InMemorySensorRegistry, SensorRegistry};

    let origin = Geodetic {
        lat_rad: 45_f64.to_radians(),
        lon_rad: 10_f64.to_radians(),
        alt_m: 0.0,
    };
    let frame = LocalFrame::new(origin);
    let min_elevation_deg: f64 = -1.5;
    // (id, position, range, (boresight, width) against true north)
    let declared = [
        // At the origin, where true north is the frame's north.
        (
            1,
            [origin.lat_rad, origin.lon_rad, 30.0],
            10_000.0,
            (120.0, 70.0),
        ),
        // Half a degree of longitude east, 39 km: a 0.35 degree convergence.
        (
            2,
            [origin.lat_rad, origin.lon_rad + 0.5_f64.to_radians(), 30.0],
            18_000.0,
            (355.0, 30.0),
        ),
        // South-west of the origin, straddling due west.
        (
            3,
            [
                origin.lat_rad - 0.2_f64.to_radians(),
                origin.lon_rad - 0.3_f64.to_radians(),
                12.0,
            ],
            6_000.0,
            (270.0, 45.0),
        ),
    ];
    let configs: Vec<SensorConfig> = declared
        .iter()
        .map(|(id, position, range, (boresight, width))| SensorConfig {
            id: *id,
            modality: "radar".into(),
            position: *position,
            max_range_m: *range,
            control_endpoint: None,
            maintenance: Vec::new(),
            azimuth_sector: Some(
                AzimuthSector::new(f64::to_radians(*boresight), f64::to_radians(*width))
                    .expect("legal"),
            ),
            detection_model: None,
        })
        .collect();
    let mut registry = InMemorySensorRegistry::from_config(&configs, "cal-1");
    for (id, ..) in &declared {
        registry
            .set_mode(SensorId(*id), SensorMode::Track)
            .expect("track");
    }
    let volumes = coverage_from_registry(&registry, min_elevation_deg.to_radians(), &frame);
    assert_eq!(volumes.len(), declared.len());

    for ((id, position, range, (boresight, width)), (volume_id, volume)) in
        declared.iter().zip(&volumes)
    {
        assert_eq!(SensorId(*id), *volume_id);
        let name = format!("sensor {id}");
        let at = Geodetic {
            lat_rad: position[0],
            lon_rad: position[1],
            alt_m: position[2],
        };
        let north_in_frame_deg = frame.true_north_at(at).to_degrees();
        let stated = Stated {
            name: "declared",
            sensor_enu: volume.sensor_enu,
            range_m: *range,
            min_elevation_deg,
            // The probe sweeps in the frame, so it starts from the frame boresight.
            sector_deg: Some((boresight + north_in_frame_deg, *width)),
        };
        let recovered_range = recover_range(
            *volume,
            stated.probe_bearing_deg(),
            stated.probe_elevation_deg(),
        );
        check_range(&name, recovered_range, *range);
        let (start, end) = recover_sector(*volume, &stated).expect("a sector has edges");
        // Back to true north at the sensor, which is what the baseline stated.
        let (start_true, end_true) = (start - north_in_frame_deg, end - north_in_frame_deg);
        check_angle(
            &name,
            "the sector's start",
            start_true,
            boresight - width / 2.0,
        );
        check_angle(&name, "the sector's end", end_true, boresight + width / 2.0);
        let elevation =
            recover_min_elevation(*volume, stated.probe_bearing_deg(), min_elevation_deg);
        check_angle(&name, "the minimum elevation", elevation, min_elevation_deg);
    }

    // The rotation is what passes sensor 2: without it its edges are off by the
    // convergence, more than three times the criterion.
    let east = Geodetic {
        lat_rad: declared[1].1[0],
        lon_rad: declared[1].1[1],
        alt_m: declared[1].1[2],
    };
    assert!(
        frame.true_north_at(east).to_degrees().abs() > 3.0 * ANGLE_TOLERANCE_DEG,
        "the fixture no longer places a sensor where the frame's north and true north differ"
    );
    let registry_sectors: Vec<_> = registry
        .sensors()
        .iter()
        .map(|s| s.azimuth_sector)
        .collect();
    assert!(registry_sectors.iter().all(Option::is_some));
}
