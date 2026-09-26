// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Advanced 3D analytical functions, per docs/gungnir-capabilities.md §5.3:
//! analysis, not display. The viewport renders geometry; this crate answers "can
//! this sensor actually see that location", "which of these points are visible
//! from here", "does this route cross a no-go area". Kept separate from
//! `gungnir-viewport3d` so it is unit-testable without a GPU, consistent with how
//! the rest of the workspace separates computation from rendering.
//!
//! All positions are local ENU meters unless a `Geodetic` is named.

pub mod anomaly;
pub mod coverage;

pub use coverage::{
    band_or_default, combined_coverage, coverage_from_registry, volume_in_frame, volume_of,
    CoverageGap, CoverageParameters, CoverageReport, GapSeverity, PointCoverage,
};

pub use anomaly::{
    detect_all, detect_cooperative, detect_feed_anomalies, detect_implausible_kinematics,
    detect_loitering, Anomaly, AnomalyKind, AnomalySettings, AnomalySubject, CooperativeSettings,
    FeedSettings, KinematicEnvelope, LoiteringSettings, SensorHealthSnapshot, TrackSnapshot,
};

use gungnir_coord::Geodetic;
use gungnir_data::geospatial::TerrainMesh;
use gungnir_geo::Geofence;

pub trait LineOfSight: Send + Sync {
    /// True if the straight segment from `from` to `to` is unobstructed.
    fn visible(&self, from: [f64; 3], to: [f64; 3]) -> bool;
}

/// Terrain is the plane u = 0. A segment is visible when it never dips below it,
/// which for a straight segment means both endpoints are at or above it.
#[derive(Debug, Default, Clone, Copy)]
pub struct FlatTerrainLineOfSight;

impl LineOfSight for FlatTerrainLineOfSight {
    fn visible(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        from[2] >= 0.0 && to[2] >= 0.0
    }
}

/// Samples the segment every `sample_spacing_m` and compares against the terrain
/// height at each sample (nearest vertex in the E,N plane). A reference
/// implementation: correct, O(vertices) per sample, and the baseline any spatially
/// indexed version must agree with.
pub struct TerrainMaskLineOfSight<'a> {
    pub terrain: &'a TerrainMesh,
    pub sample_spacing_m: f64,
}

impl TerrainMaskLineOfSight<'_> {
    /// Height of the nearest terrain vertex in the E,N plane; `None` for an empty mesh.
    pub fn height_at(&self, e: f64, n: f64) -> Option<f64> {
        self.terrain
            .positions
            .iter()
            .map(|p| {
                let de = f64::from(p[0]) - e;
                let dn = f64::from(p[1]) - n;
                (de * de + dn * dn, f64::from(p[2]))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, h)| h)
    }
}

impl LineOfSight for TerrainMaskLineOfSight<'_> {
    // The sample count is a small positive integer derived from a clamped ratio, so
    // the usize/f64 conversions cannot truncate or lose sign in practice.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn visible(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let spacing = self.sample_spacing_m.max(f64::EPSILON);
        let steps = ((length / spacing).ceil() as usize).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let p = [from[0] + d[0] * t, from[1] + d[1] * t, from[2] + d[2] * t];
            if let Some(h) = self.height_at(p[0], p[1]) {
                if p[2] < h {
                    return false;
                }
            }
        }
        true
    }
}

/// Which of `points` an observer at `observer` can see.
pub fn viewshed(los: &dyn LineOfSight, observer: [f64; 3], points: &[[f64; 3]]) -> Vec<bool> {
    points.iter().map(|p| los.visible(observer, *p)).collect()
}

/// A sensor's coverage: a range limit, an elevation band above the sensor's own horizon,
/// and an azimuth sector (terrain masking is applied by combining with a
/// [`LineOfSight`]).
///
/// Positions are in the local ENU frame. **Angles are the sensor's own** (GAP-158, D-111):
/// elevation is measured against `vertical`, the local vertical at the sensor placed in
/// the frame, and bearing in the plane square to it, clockwise from the frame's `+n` axis
/// projected into that plane. At the origin that is exactly the frame's `u` and `+n`; a
/// sensor 11 km away has a vertical a tenth of a degree off the frame's, which is the
/// coverage-accuracy criterion, so its band is measured against its own.
///
/// A sector surveyed against true north is turned into the frame by
/// [`gungnir_model::LocalFrame::sector_in_frame`] before it is put here, and the vertical
/// is [`gungnir_model::LocalFrame::vertical_at`]; [`coverage::volume_of`] does both
/// (`docs/design/DN-12-coverage-and-gaps.md` amendments 1 and 2; GAP-118, D-84).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageVolume {
    pub sensor_enu: [f64; 3],
    pub max_range_m: f64,
    /// The lowest elevation the sensor sees, radians above its own horizon.
    pub min_elevation_rad: f64,
    /// The highest elevation the sensor sees, radians above its own horizon. **`π/2` is the
    /// zenith, which is no ceiling**, and is what a volume that states none has, including
    /// every volume serialized before ceilings existed.
    #[serde(default = "zenith")]
    pub max_elevation_rad: f64,
    /// The local vertical at the sensor, a unit vector in the frame. **`[0, 0, 1]` is the
    /// frame's own**, exact at the origin, and what a volume serialized before this field
    /// existed has.
    #[serde(default = "frame_up")]
    pub vertical: [f64; 3],
    /// The bearings the sensor sees, in the frame. **`None` is the full circle**: a
    /// rotating radar or an omnidirectional receiver, and every volume built before
    /// sectors existed.
    #[serde(default)]
    pub azimuth: Option<gungnir_model::AzimuthSector>,
}

fn zenith() -> f64 {
    std::f64::consts::FRAC_PI_2
}

fn frame_up() -> [f64; 3] {
    [0.0, 0.0, 1.0]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v` scaled to unit length, or `None` for a zero or non-finite vector.
fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(v, v).sqrt();
    (length.is_finite() && length > f64::EPSILON)
        .then(|| [v[0] / length, v[1] / length, v[2] / length])
}

impl CoverageVolume {
    /// Whether the volume contains `p`: within range, inside the elevation band, and
    /// inside the sector, with elevation and bearing measured against the sensor's own
    /// vertical.
    ///
    /// The sensor's own position is covered. A point directly above or below the sensor
    /// has no bearing, so the sector does not exclude it and the band decides: straight up
    /// is covered only by a volume whose ceiling is the zenith.
    ///
    /// **A vertical that is no direction covers nothing** but the sensor's own position:
    /// a zero or non-finite vector gives no elevation to judge, and reporting a point
    /// uncovered is the direction that shows a gap rather than hides one.
    pub fn covers(&self, p: [f64; 3]) -> bool {
        let d = [
            p[0] - self.sensor_enu[0],
            p[1] - self.sensor_enu[1],
            p[2] - self.sensor_enu[2],
        ];
        let range = dot(d, d).sqrt();
        if range > self.max_range_m || range == 0.0 {
            return range == 0.0;
        }
        let Some(up) = unit(self.vertical) else {
            return false;
        };
        let along = dot(d, up);
        let level = [
            d[0] - along * up[0],
            d[1] - along * up[1],
            d[2] - along * up[2],
        ];
        let elevation = along.atan2(dot(level, level).sqrt());
        if elevation < self.min_elevation_rad || elevation > self.max_elevation_rad {
            return false;
        }
        let Some(sector) = self.azimuth else {
            return true;
        };
        // North and east in the sensor's own horizontal plane: the frame's `+n` with its
        // vertical part removed, and north cross up. Both are the frame's own axes when
        // the vertical is.
        let Some(north) = unit([-up[1] * up[0], 1.0 - up[1] * up[1], -up[1] * up[2]]) else {
            return false;
        };
        let east = [
            north[1] * up[2] - north[2] * up[1],
            north[2] * up[0] - north[0] * up[2],
            north[0] * up[1] - north[1] * up[0],
        ];
        match gungnir_model::bearing_rad(dot(level, east), dot(level, north)) {
            Some(bearing) => sector.contains(bearing),
            // No bearing to judge a sector by: straight up or down.
            None => true,
        }
    }
}

/// True if any vertex of the route lies inside a no-go fence. Segment crossings
/// between vertices are not checked; densify the route first if that matters.
pub fn route_crosses_no_go(route: &[Geodetic], fences: &[Geofence]) -> bool {
    route
        .iter()
        .any(|p| fences.iter().any(|f| f.no_go && f.contains(*p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_terrain_blocks_below_ground_endpoints() {
        let los = FlatTerrainLineOfSight;
        assert!(los.visible([0.0, 0.0, 10.0], [100.0, 0.0, 5.0]));
        assert!(!los.visible([0.0, 0.0, 10.0], [100.0, 0.0, -1.0]));
    }

    #[test]
    fn terrain_ridge_masks_the_far_side() {
        let terrain = TerrainMesh {
            positions: vec![[0.0, 0.0, 0.0], [50.0, 0.0, 30.0], [100.0, 0.0, 0.0]],
            indices: vec![0, 1, 2],
            ..TerrainMesh::default()
        };
        let los = TerrainMaskLineOfSight {
            terrain: &terrain,
            sample_spacing_m: 5.0,
        };
        assert!(
            !los.visible([0.0, 0.0, 5.0], [100.0, 0.0, 5.0]),
            "ridge at 30 m should block"
        );
        assert!(
            los.visible([0.0, 0.0, 40.0], [100.0, 0.0, 40.0]),
            "above the ridge is clear"
        );
        assert_eq!(
            viewshed(
                &los,
                [0.0, 0.0, 5.0],
                &[[10.0, 0.0, 5.0], [100.0, 0.0, 5.0]]
            ),
            vec![true, false]
        );
    }

    #[test]
    fn coverage_volume_respects_range_and_elevation() {
        let cov = CoverageVolume {
            sensor_enu: [0.0, 0.0, 0.0],
            max_range_m: 1000.0,
            min_elevation_rad: 0.1,
            max_elevation_rad: std::f64::consts::FRAC_PI_2,
            vertical: [0.0, 0.0, 1.0],
            azimuth: None,
        };
        assert!(cov.covers([100.0, 0.0, 50.0]));
        assert!(
            !cov.covers([100.0, 0.0, 1.0]),
            "below the minimum elevation"
        );
        assert!(!cov.covers([2000.0, 0.0, 500.0]), "out of range");
    }

    /// GAP-118: a sectored volume covers inside its sector, across north, and nothing
    /// outside it; the sensor's own position and the point overhead are not excluded by a
    /// bearing they do not have.
    #[test]
    fn coverage_volume_respects_its_sector_across_north() {
        let cov = CoverageVolume {
            sensor_enu: [100.0, 200.0, 0.0],
            max_range_m: 1000.0,
            min_elevation_rad: -0.5,
            max_elevation_rad: std::f64::consts::FRAC_PI_2,
            vertical: [0.0, 0.0, 1.0],
            azimuth: Some(
                gungnir_model::AzimuthSector::new(350_f64.to_radians(), 40_f64.to_radians())
                    .expect("a legal sector"),
            ),
        };
        let at = |bearing_deg: f64| {
            let b = bearing_deg.to_radians();
            [100.0 + 500.0 * b.sin(), 200.0 + 500.0 * b.cos(), 0.0]
        };
        for inside in [331.0, 350.0, 0.0, 9.0] {
            assert!(cov.covers(at(inside)), "{inside} deg is inside 330..10");
        }
        for outside in [329.0, 11.0, 90.0, 180.0, 270.0] {
            assert!(!cov.covers(at(outside)), "{outside} deg is outside 330..10");
        }
        assert!(cov.covers([100.0, 200.0, 0.0]), "the sensor's own position");
        assert!(
            cov.covers([100.0, 200.0, 300.0]),
            "straight up has no bearing"
        );
    }

    /// GAP-158: a ceiling takes the cone overhead out of the volume -- straight up
    /// included -- and leaves everything below it.
    #[test]
    fn coverage_volume_respects_its_ceiling_and_the_cone_of_silence_is_uncovered() {
        let cov = CoverageVolume {
            sensor_enu: [0.0, 0.0, 10.0],
            max_range_m: 1000.0,
            min_elevation_rad: 2_f64.to_radians(),
            max_elevation_rad: 60_f64.to_radians(),
            vertical: [0.0, 0.0, 1.0],
            azimuth: None,
        };
        let at = |elevation_deg: f64| {
            let e = elevation_deg.to_radians();
            [500.0 * e.cos(), 0.0, 10.0 + 500.0 * e.sin()]
        };
        for inside in [2.01, 30.0, 59.9] {
            assert!(cov.covers(at(inside)), "{inside} deg is in 2..60");
        }
        for outside in [1.9, 60.1, 80.0] {
            assert!(!cov.covers(at(outside)), "{outside} deg is outside 2..60");
        }
        assert!(
            !cov.covers([0.0, 0.0, 500.0]),
            "straight up is above the ceiling"
        );
        assert!(cov.covers([0.0, 0.0, 10.0]), "the sensor's own position");
    }

    /// GAP-158: elevation is measured against the volume's own vertical. A vertical tilted
    /// half a degree east tilts the sensor's horizon down on its east side, so a point 1.5
    /// degrees above the frame's horizon due east is two degrees above the sensor's, and
    /// one due west is one degree above it.
    #[test]
    fn elevation_is_measured_against_the_sensors_own_vertical() {
        let tilt = 0.5_f64.to_radians();
        let cov = CoverageVolume {
            sensor_enu: [0.0; 3],
            max_range_m: 10_000.0,
            min_elevation_rad: 1.25_f64.to_radians(),
            max_elevation_rad: std::f64::consts::FRAC_PI_2,
            vertical: [tilt.sin(), 0.0, tilt.cos()],
            azimuth: None,
        };
        let e = 1.5_f64.to_radians();
        let east = [1000.0 * e.cos(), 0.0, 1000.0 * e.sin()];
        let west = [-1000.0 * e.cos(), 0.0, 1000.0 * e.sin()];
        assert!(cov.covers(east), "2.0 deg above its own horizon");
        assert!(
            !cov.covers(west),
            "1.0 deg above its own horizon, below the floor"
        );

        // With the frame's vertical, both are at 1.5 degrees and both covered.
        let level = CoverageVolume {
            vertical: [0.0, 0.0, 1.0],
            ..cov
        };
        assert!(level.covers(east) && level.covers(west));

        // A vertical that is no direction covers nothing but the sensor's own position.
        for bad in [[0.0; 3], [f64::NAN, 0.0, 1.0]] {
            let broken = CoverageVolume {
                vertical: bad,
                ..cov
            };
            assert!(!broken.covers(east));
            assert!(broken.covers([0.0; 3]));
        }
    }

    /// A volume serialized before ceilings and verticals existed reads as no ceiling and
    /// the frame's vertical, so it means what it meant.
    #[test]
    fn a_volume_without_a_ceiling_or_vertical_reads_as_the_zenith_and_the_frames_up() {
        let old = r#"{"sensor_enu":[1.0,2.0,3.0],"max_range_m":100.0,"min_elevation_rad":0.0}"#;
        let v: CoverageVolume = serde_json::from_str(old).expect("parses");
        assert!((v.max_elevation_rad - std::f64::consts::FRAC_PI_2).abs() < f64::EPSILON);
        assert_eq!(v.vertical, [0.0, 0.0, 1.0]);
        assert_eq!(v.azimuth, None);
    }

    #[test]
    fn route_through_a_no_go_fence_is_flagged() {
        let fence = Geofence {
            center: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            radius_m: 5_000.0,
            no_go: true,
        };
        let inside = Geodetic {
            lat_rad: 0.0001,
            lon_rad: 0.0,
            alt_m: 0.0,
        };
        let outside = Geodetic {
            lat_rad: 0.1,
            lon_rad: 0.1,
            alt_m: 0.0,
        };
        assert!(route_crosses_no_go(&[outside, inside], &[fence]));
        assert!(!route_crosses_no_go(&[outside], &[fence]));
    }
}
