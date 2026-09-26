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
//! **What the elevation limits are** (GAP-158, D-111). A `CoverageVolume` has a floor and a
//! ceiling -- the ceiling `π/2`, the zenith, when none is stated -- measured against the
//! local vertical at the sensor. The frame-stated fixture states both for most volumes and
//! recovers both; a vertical arc that climbs past a ceiling of the zenith finds no ceiling,
//! which is what "no ceiling" means. The third test declares bands on sensors up to 47 km
//! from the origin, where a sensor's vertical leans 0.42 degree from the frame's, builds
//! them through the registry, and recovers each floor and ceiling **in a frame anchored at
//! the sensor itself** -- an independent statement of that sensor's own vertical and true
//! north -- and then shows that the same volume measured against the frame's vertical
//! misses the criterion there.

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
    /// The ceiling, degrees; 90 is the zenith, which is no ceiling.
    max_elevation_deg: f64,
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
            max_elevation_rad: self.max_elevation_deg.to_radians(),
            vertical: [0.0, 0.0, 1.0],
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
            max_elevation_deg: 45.0,
            sector_deg: Some((45.0, 90.0)),
        },
        Stated {
            name: "sector across north",
            sensor_enu: [5_000.0, -3_000.0, 20.0],
            range_m: 8_000.0,
            min_elevation_deg: 0.5,
            max_elevation_deg: 70.0,
            sector_deg: Some((350.0, 40.0)),
        },
        Stated {
            name: "narrow sector off the cardinal points",
            sensor_enu: [-2_500.0, 7_500.0, 5.0],
            range_m: 20_000.0,
            min_elevation_deg: 3.0,
            max_elevation_deg: 30.0,
            sector_deg: Some((200.3, 7.5)),
        },
        Stated {
            name: "full circle by omission",
            sensor_enu: [-4_000.0, 2_000.0, 10.0],
            range_m: 5_000.0,
            min_elevation_deg: -1.0,
            max_elevation_deg: 90.0,
            sector_deg: None,
        },
        Stated {
            name: "full circle stated",
            sensor_enu: [1_000.0, 1_000.0, 0.0],
            range_m: 3_000.0,
            min_elevation_deg: 10.0,
            max_elevation_deg: 60.0,
            sector_deg: Some((0.0, 360.0)),
        },
        Stated {
            name: "all but a notch",
            sensor_enu: [300.0, -700.0, 15.0],
            range_m: 15_000.0,
            min_elevation_deg: -4.5,
            max_elevation_deg: 80.0,
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

/// A vertical arc at half the range, on `bearing_deg`, climbing from ten degrees below the
/// stated floor to ten above the stated ceiling, or to 89 degrees when the ceiling is the
/// zenith; vertices every 0.05 degree, samples every 0.1 percent of the range.
///
/// `to_frame` places a point stated about the sensor into the frame the volume is in, and
/// `from_frame` takes a recovered point back, so the same probe serves a volume stated in
/// the frame (both the identity about the sensor) and one declared at a distant sensor,
/// probed in a frame anchored at that sensor.
fn recover_band(
    volume: CoverageVolume,
    bearing_deg: f64,
    (floor_deg, ceiling_deg): (f64, f64),
    to_frame: &dyn Fn([f64; 3]) -> [f64; 3],
    from_frame: &dyn Fn([f64; 3]) -> [f64; 3],
) -> (f64, Option<f64>) {
    let r = 0.5 * volume.max_range_m;
    let from = (floor_deg - 10.0).max(-89.0);
    let to = (ceiling_deg + 10.0).min(89.0);
    // `to - from` is a few tens of degrees at most, so the vertex count is small.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = ((to - from) / 0.05).round() as u32;
    let arc: Vec<[f64; 3]> = (0..=steps)
        .map(|i| to_frame(point([0.0; 3], r, bearing_deg, from + f64::from(i) * 0.05)))
        .collect();
    let found = transitions(volume, &arc, 0.001 * volume.max_range_m);
    assert!(
        !found.is_empty() && found.len() <= 2,
        "a floor, and a ceiling below the zenith: {found:?}"
    );
    assert!(found[0].1, "the arc climbs into coverage at the floor");
    let floor = elevation_deg([0.0; 3], from_frame(found[0].0));
    let ceiling = found.get(1).map(|(at, entering)| {
        assert!(!entering, "and leaves it at the ceiling");
        elevation_deg([0.0; 3], from_frame(*at))
    });
    (floor, ceiling)
}

/// [`recover_band`] for a volume stated in the frame: the sensor's own vertical is the
/// frame's `u`.
fn recover_band_in_frame(
    volume: CoverageVolume,
    bearing_deg: f64,
    band_deg: (f64, f64),
) -> (f64, Option<f64>) {
    let s = volume.sensor_enu;
    recover_band(
        volume,
        bearing_deg,
        band_deg,
        &|p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]],
        &|p| [p[0] - s[0], p[1] - s[1], p[2] - s[2]],
    )
}

/// The stated ceiling against the recovered one: none when the stated ceiling is the
/// zenith, and within the criterion otherwise.
fn check_ceiling(name: &str, recovered: Option<f64>, stated_deg: f64) {
    if stated_deg >= 90.0 {
        assert_eq!(
            recovered, None,
            "{name}: no ceiling is stated, none is found"
        );
    } else {
        let found = recovered.unwrap_or_else(|| panic!("{name}: a {stated_deg} deg ceiling"));
        check_angle(name, "the ceiling", found, stated_deg);
    }
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

        let (floor, ceiling) = recover_band_in_frame(
            volume,
            bearing,
            (stated.min_elevation_deg, stated.max_elevation_deg),
        );
        check_angle(stated.name, "the floor", floor, stated.min_elevation_deg);
        check_ceiling(stated.name, ceiling, stated.max_elevation_deg);

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
            elevation_band: None,
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
            max_elevation_deg: 90.0,
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
        // The floor is the baseline's (none of these sensors declares a band), measured
        // against the sensor's own vertical, so it is probed in a frame anchored there.
        let (floor, ceiling) =
            recover_band_at(&frame, at, *volume, *boresight, (min_elevation_deg, 90.0));
        check_angle(&name, "the floor", floor, min_elevation_deg);
        check_ceiling(&name, ceiling, 90.0);
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

/// [`recover_band`] for a sensor declared at `at`: the arc is stated in a frame anchored at
/// the sensor -- its own vertical and true north -- on `true_bearing_deg`, placed into
/// `frame` through the geodetic conversion, and each recovered point is taken back.
fn recover_band_at(
    frame: &LocalFrame,
    at: Geodetic,
    volume: CoverageVolume,
    true_bearing_deg: f64,
    band_deg: (f64, f64),
) -> (f64, Option<f64>) {
    let own = LocalFrame::new(at);
    recover_band(
        volume,
        true_bearing_deg,
        band_deg,
        &|p| frame.to_enu(own.to_geodetic(p)),
        &|p| own.to_enu(frame.to_geodetic(p)),
    )
}

/// GAP-158, D-111: every sensor's own floor and ceiling, declared on the sensor, carried
/// through the registry to its volume, and recovered from computed coverage against **its
/// own vertical** within the criterion -- including sensors 33 to 47 km from the origin,
/// whose verticals lean 0.3 to 0.42 degree from the frame's. A sensor that declares no
/// band has the baseline's floor and no ceiling.
///
/// Then the correction is shown to matter: the same volume measured against the frame's
/// vertical instead of the sensor's recovers a floor and a ceiling off by more than three
/// times the criterion.
#[test]
#[allow(clippy::too_many_lines)] // one fixture, stated in full where it is checked
fn each_sensors_band_is_recovered_against_its_own_vertical() {
    use gungnir_config::SensorConfig;
    use gungnir_model::ElevationBand;
    use gungnir_sensor_management::{InMemorySensorRegistry, SensorRegistry};

    let origin = Geodetic {
        lat_rad: 45_f64.to_radians(),
        lon_rad: 10_f64.to_radians(),
        alt_m: 0.0,
    };
    let frame = LocalFrame::new(origin);
    let baseline_floor_deg: f64 = -1.5;
    let deg = f64::to_radians;
    // (id, (lat, lon, alt) offsets from the origin in degrees and metres, range, band in
    // degrees or none, (boresight, width) or none, the true bearing to probe along)
    type Declared = (
        u32,
        [f64; 3],
        f64,
        Option<(f64, f64)>,
        Option<(f64, f64)>,
        f64,
    );
    let declared: [Declared; 4] = [
        // At the origin: its vertical is the frame's.
        (
            1,
            [0.0, 0.0, 25.0],
            12_000.0,
            Some((-2.0, 60.0)),
            None,
            30.0,
        ),
        // 47 km east, facing east: the probe runs along the lean, the worst case.
        (
            2,
            [0.0, 0.6, 40.0],
            20_000.0,
            Some((1.5, 30.0)),
            Some((90.0, 60.0)),
            90.0,
        ),
        // 33 km north and 31 km west, probed away from the origin.
        (
            3,
            [0.3, -0.4, 15.0],
            8_000.0,
            Some((0.5, 75.0)),
            None,
            315.0,
        ),
        // 28 km south, declaring no band: the baseline's floor and the zenith.
        (4, [-0.25, 0.0, 10.0], 10_000.0, None, None, 180.0),
    ];
    let position = |offset: [f64; 3]| Geodetic {
        lat_rad: origin.lat_rad + deg(offset[0]),
        lon_rad: origin.lon_rad + deg(offset[1]),
        alt_m: offset[2],
    };
    let configs: Vec<SensorConfig> = declared
        .iter()
        .map(|(id, offset, range, band, sector, _)| {
            let at = position(*offset);
            SensorConfig {
                id: *id,
                modality: "radar".into(),
                position: [at.lat_rad, at.lon_rad, at.alt_m],
                max_range_m: *range,
                control_endpoint: None,
                maintenance: Vec::new(),
                azimuth_sector: sector
                    .map(|(b, w)| AzimuthSector::new(deg(b), deg(w)).expect("a legal sector")),
                elevation_band: band.map(|(floor, ceiling)| {
                    ElevationBand::new(deg(floor), deg(ceiling)).expect("a legal band")
                }),
                detection_model: None,
            }
        })
        .collect();
    let baseline = gungnir_config::ConfigBaseline {
        sensors: configs.clone(),
        ..gungnir_config::ConfigBaseline::default()
    };
    gungnir_config::validate(&baseline).expect("every declared band is legal");
    let mut registry = InMemorySensorRegistry::from_config(&configs, "cal-1");
    for (id, ..) in &declared {
        registry
            .set_mode(SensorId(*id), SensorMode::Search)
            .expect("search");
    }
    let volumes = coverage_from_registry(&registry, deg(baseline_floor_deg), &frame);
    assert_eq!(volumes.len(), declared.len());

    for ((id, offset, _, band, _, bearing), (volume_id, volume)) in declared.iter().zip(&volumes) {
        assert_eq!(SensorId(*id), *volume_id);
        let name = format!("sensor {id}");
        let (floor_deg, ceiling_deg) = band.unwrap_or((baseline_floor_deg, 90.0));
        let (floor, ceiling) = recover_band_at(
            &frame,
            position(*offset),
            *volume,
            *bearing,
            (floor_deg, ceiling_deg),
        );
        println!("{name}: floor {floor:.4} deg (stated {floor_deg}), ceiling {ceiling:?} (stated {ceiling_deg})");
        check_angle(&name, "the floor", floor, floor_deg);
        check_ceiling(&name, ceiling, ceiling_deg);
    }

    // The correction matters: sensor 2's vertical leans from the frame's by more than three
    // times the criterion, and measured against the frame's vertical its floor and ceiling
    // are recovered that far off.
    let (_, east_offset, _, east_band, _, east_bearing) = declared[1];
    let east = position(east_offset);
    let v = volumes[1].1.vertical;
    let lean_deg = v[2].clamp(-1.0, 1.0).acos().to_degrees();
    assert!(
        lean_deg > 3.0 * ANGLE_TOLERANCE_DEG,
        "the fixture no longer places a sensor where its vertical and the frame's differ: \
         {lean_deg} deg"
    );
    let (floor_deg, ceiling_deg) = east_band.expect("sensor 2 declares a band");
    let uncorrected = CoverageVolume {
        vertical: [0.0, 0.0, 1.0],
        ..volumes[1].1
    };
    let (floor, ceiling) = recover_band_at(
        &frame,
        east,
        uncorrected,
        east_bearing,
        (floor_deg, ceiling_deg),
    );
    let ceiling = ceiling.expect("a ceiling below the zenith");
    for (what, recovered, stated) in [
        ("floor", floor, floor_deg),
        ("ceiling", ceiling, ceiling_deg),
    ] {
        let error = (recovered - stated).abs();
        println!("against the frame's vertical, sensor 2's {what}: {recovered:.4} deg against {stated} ({error:.4} deg off; its vertical leans {lean_deg:.4} deg)");
        assert!(
            error > 3.0 * ANGLE_TOLERANCE_DEG,
            "against the frame's vertical the {what} is off by only {error} deg; the \
             correction is not what passes this test"
        );
    }
}
