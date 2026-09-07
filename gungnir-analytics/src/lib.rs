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
    combined_coverage, coverage_from_registry, CoverageGap, CoverageParameters, CoverageReport,
    GapSeverity, PointCoverage,
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

/// A sensor's coverage: a range limit and a minimum elevation angle above the
/// sensor's horizon (terrain masking is applied by combining with a
/// [`LineOfSight`]).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageVolume {
    pub sensor_enu: [f64; 3],
    pub max_range_m: f64,
    pub min_elevation_rad: f64,
}

impl CoverageVolume {
    pub fn covers(&self, p: [f64; 3]) -> bool {
        let d = [
            p[0] - self.sensor_enu[0],
            p[1] - self.sensor_enu[1],
            p[2] - self.sensor_enu[2],
        ];
        let horizontal = (d[0] * d[0] + d[1] * d[1]).sqrt();
        let range = (horizontal * horizontal + d[2] * d[2]).sqrt();
        if range > self.max_range_m || range == 0.0 {
            return range == 0.0;
        }
        d[2].atan2(horizontal) >= self.min_elevation_rad
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
        };
        assert!(cov.covers([100.0, 0.0, 50.0]));
        assert!(
            !cov.covers([100.0, 0.0, 1.0]),
            "below the minimum elevation"
        );
        assert!(!cov.covers([2000.0, 0.0, 500.0]), "out of range");
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
