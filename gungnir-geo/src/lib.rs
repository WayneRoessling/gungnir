//! Geospatial reference & map service layer, per docs/gungnir-capabilities.md
//! §5.3. Extends gungnir-data's terrain-elevation-only ingestion with visual and
//! functional geospatial context -- elevation alone gives shape without the
//! orientation cues (roads, coastlines, buildings) operators actually navigate by.
//! Geofence containment is the check `gungnir-policy` runs before a plan can be
//! approved, so it is implemented and tested here, including across the
//! antimeridian.

use gungnir_coord::Geodetic;

/// Mean Earth radius, meters (IUGG), used for great-circle geofence distances.
pub mod hazard;

pub use hazard::{route_crosses_hazard, Hazard, HazardExtent, HazardKind, HazardLayer};

pub const MEAN_EARTH_RADIUS_M: f64 = 6_371_008.8;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MapLayer {
    pub name: String,
    pub kind: LayerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LayerKind {
    RasterImagery,
    VectorFeatures,
    Geofence,
    /// Physical obstacles and hazards: booms, nets, barriers, wrecks, shoals
    /// (docs/design/DN-14-hazard-layer.md). Descriptive, never a rule.
    Hazard,
    /// Areas artillery may not strike (docs/design/DN-05-fires.md). Distinct from
    /// a no-go geofence, which constrains our own interceptors instead.
    NoFireArea,
}

/// A circular geofence on the sphere. `no_go` fences deny intercepts inside them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Geofence {
    pub center: Geodetic,
    pub radius_m: f64,
    pub no_go: bool,
}

impl Geofence {
    /// Great-circle containment; altitude is ignored.
    pub fn contains(&self, position: Geodetic) -> bool {
        great_circle_distance_m(self.center, position) <= self.radius_m
    }
}

/// Haversine great-circle distance on the mean sphere, meters. Robust across the
/// antimeridian because it works on angular differences, not raw longitudes.
pub fn great_circle_distance_m(a: Geodetic, b: Geodetic) -> f64 {
    let dlat = b.lat_rad - a.lat_rad;
    let dlon = b.lon_rad - a.lon_rad;
    let h =
        (dlat / 2.0).sin().powi(2) + a.lat_rad.cos() * b.lat_rad.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * MEAN_EARTH_RADIUS_M * h.sqrt().clamp(0.0, 1.0).asin()
}

pub trait GeoService: Send + Sync {
    fn layers(&self) -> &[MapLayer];
    fn geofences(&self) -> &[Geofence];
    /// True if the given position falls inside any `no_go` geofence -- the check
    /// gungnir-policy calls before approving an intercept plan.
    fn is_within_no_go(&self, position: Geodetic) -> bool;
}

/// Layers and fences held in memory, loaded from config or an operator action.
#[derive(Debug, Clone, Default)]
pub struct InMemoryGeoService {
    layers: Vec<MapLayer>,
    geofences: Vec<Geofence>,
}

impl InMemoryGeoService {
    pub fn new(layers: Vec<MapLayer>, geofences: Vec<Geofence>) -> Self {
        Self { layers, geofences }
    }

    pub fn add_geofence(&mut self, fence: Geofence) {
        self.geofences.push(fence);
    }
}

impl GeoService for InMemoryGeoService {
    fn layers(&self) -> &[MapLayer] {
        &self.layers
    }

    fn geofences(&self) -> &[Geofence] {
        &self.geofences
    }

    fn is_within_no_go(&self, position: Geodetic) -> bool {
        self.geofences
            .iter()
            .any(|f| f.no_go && f.contains(position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn g(lat_deg: f64, lon_deg: f64) -> Geodetic {
        Geodetic {
            lat_rad: lat_deg.to_radians(),
            lon_rad: lon_deg.to_radians(),
            alt_m: 0.0,
        }
    }

    #[test]
    fn one_degree_of_latitude_is_about_111_km() {
        let d = great_circle_distance_m(g(0.0, 0.0), g(1.0, 0.0));
        assert!((d - 111_195.0).abs() < 50.0, "{d}");
    }

    #[test]
    fn containment_inside_and_outside() {
        let fence = Geofence {
            center: g(10.0, 20.0),
            radius_m: 5_000.0,
            no_go: true,
        };
        assert!(fence.contains(g(10.01, 20.0)));
        assert!(!fence.contains(g(10.1, 20.0)));
    }

    #[test]
    fn fence_straddling_the_antimeridian_contains_points_on_both_sides() {
        let fence = Geofence {
            center: Geodetic {
                lat_rad: 0.0,
                lon_rad: PI,
                alt_m: 0.0,
            },
            radius_m: 20_000.0,
            no_go: true,
        };
        assert!(fence.contains(g(0.0, 179.95)));
        assert!(fence.contains(g(0.0, -179.95)));
        assert!(!fence.contains(g(0.0, 179.0)));
    }

    #[test]
    fn only_no_go_fences_deny() {
        let svc = InMemoryGeoService::new(
            Vec::new(),
            vec![
                Geofence {
                    center: g(0.0, 0.0),
                    radius_m: 1_000.0,
                    no_go: false,
                },
                Geofence {
                    center: g(5.0, 5.0),
                    radius_m: 1_000.0,
                    no_go: true,
                },
            ],
        );
        assert!(!svc.is_within_no_go(g(0.0, 0.0)));
        assert!(svc.is_within_no_go(g(5.0, 5.0)));
    }
}
