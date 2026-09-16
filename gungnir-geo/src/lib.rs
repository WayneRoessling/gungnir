// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Geospatial reference & map service layer, per docs/gungnir-capabilities.md
//! §5.3. Extends gungnir-data's terrain-elevation-only ingestion with visual and
//! functional geospatial context -- elevation alone gives shape without the
//! orientation cues (roads, coastlines, buildings) operators actually navigate by.
//! Geofence containment is the check `gungnir-policy` runs before a plan can be
//! approved, so it is implemented and tested here, including across the
//! antimeridian.

use gungnir_coord::Geodetic;

pub mod hazard;

pub use hazard::{route_crosses_hazard, Hazard, HazardExtent, HazardKind, HazardLayer};

/// Mean Earth radius, meters (IUGG), used for great-circle geofence distances.
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

    // The fences these tests use, named once so the edge test at the bottom walks the
    // same fences the other tests do rather than copies of them.

    fn fence_at_10n_20e() -> Geofence {
        Geofence {
            center: g(10.0, 20.0),
            radius_m: 5_000.0,
            no_go: true,
        }
    }

    fn fence_on_the_antimeridian_at_the_equator() -> Geofence {
        Geofence {
            center: Geodetic {
                lat_rad: 0.0,
                lon_rad: PI,
                alt_m: 0.0,
            },
            radius_m: 20_000.0,
            no_go: true,
        }
    }

    fn open_fence_at_the_origin() -> Geofence {
        Geofence {
            center: g(0.0, 0.0),
            radius_m: 1_000.0,
            no_go: false,
        }
    }

    fn no_go_fence_at_5n_5e() -> Geofence {
        Geofence {
            center: g(5.0, 5.0),
            radius_m: 1_000.0,
            no_go: true,
        }
    }

    /// Scenario 5's origin (`gungnir-scenario`'s `plan_adversarial_geometry`): 89.9 N on
    /// the antimeridian, 0.1 degree (about 11 km) short of the pole. The radius is the
    /// 20 km south of the origin that scenario's target starts from, so the fence reaches
    /// past the pole on one side and to about the target's start on the other.
    fn fence_at_scenario_5s_origin() -> Geofence {
        Geofence {
            center: Geodetic {
                lat_rad: 89.9_f64.to_radians(),
                lon_rad: PI,
                alt_m: 0.0,
            },
            radius_m: 20_000.0,
            no_go: true,
        }
    }

    /// A position's unit vector on the sphere; altitude is dropped, as `contains` drops it.
    fn unit(p: Geodetic) -> [f64; 3] {
        [
            p.lat_rad.cos() * p.lon_rad.cos(),
            p.lat_rad.cos() * p.lon_rad.sin(),
            p.lat_rad.sin(),
        ]
    }

    /// Great-circle distance computed without [`great_circle_distance_m`]: the angle
    /// between the two unit vectors as `atan2(|a x b|, a . b)`, times the same
    /// [`MEAN_EARTH_RADIUS_M`] sphere the crate uses. A different formula from the
    /// crate's haversine, sharing only the sphere, and well conditioned at every
    /// separation, including the metre either side of an edge the test below needs.
    fn independent_distance_m(a: Geodetic, b: Geodetic) -> f64 {
        let (u, v) = (unit(a), unit(b));
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let sin = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        let cos = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        MEAN_EARTH_RADIUS_M * sin.atan2(cos)
    }

    /// The point `distance_m` from `center` along the initial bearing `bearing_rad`
    /// (clockwise from north): the centre's unit vector turned through
    /// `distance_m / MEAN_EARTH_RADIUS_M` towards its local north/east tangent. Only
    /// generates candidates; whether one is inside is decided by
    /// [`independent_distance_m`].
    fn destination(center: Geodetic, bearing_rad: f64, distance_m: f64) -> Geodetic {
        let (lat, lon) = (center.lat_rad, center.lon_rad);
        let c = unit(center);
        let north = [-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos()];
        let east = [-lon.sin(), lon.cos(), 0.0];
        let angle = distance_m / MEAN_EARTH_RADIUS_M;
        let p: [f64; 3] = std::array::from_fn(|i| {
            c[i] * angle.cos()
                + (north[i] * bearing_rad.cos() + east[i] * bearing_rad.sin()) * angle.sin()
        });
        Geodetic {
            lat_rad: p[2].atan2(p[0].hypot(p[1])),
            lon_rad: p[1].atan2(p[0]),
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
        let fence = fence_at_10n_20e();
        assert!(fence.contains(g(10.01, 20.0)));
        assert!(!fence.contains(g(10.1, 20.0)));
    }

    #[test]
    fn fence_straddling_the_antimeridian_contains_points_on_both_sides() {
        let fence = fence_on_the_antimeridian_at_the_equator();
        assert!(fence.contains(g(0.0, 179.95)));
        assert!(fence.contains(g(0.0, -179.95)));
        assert!(!fence.contains(g(0.0, 179.0)));
    }

    #[test]
    fn only_no_go_fences_deny() {
        let svc = InMemoryGeoService::new(
            Vec::new(),
            vec![open_fence_at_the_origin(), no_go_fence_at_5n_5e()],
        );
        assert!(!svc.is_within_no_go(g(0.0, 0.0)));
        assert!(svc.is_within_no_go(g(5.0, 5.0)));
    }

    /// The `gungnir-geo` Geofence containment row of
    /// `docs/verification-capability-table.md` §2, on the geometry its data source names:
    /// Scenario 5's origin, 89.9 N on the antimeridian, where a fence straddles the
    /// antimeridian and the pole at once.
    ///
    /// Expected answers are hand-derived from colatitudes (0.1 degree for the centre),
    /// at 111,195.08 m per degree on the crate's sphere. Along one meridian, or through
    /// the pole onto the opposite one, colatitudes add or subtract; for other pairs the
    /// law of cosines on the flat cap around the pole is good to well under a metre at
    /// these colatitudes. Each point is also checked against [`independent_distance_m`],
    /// so a slip in the arithmetic below fails rather than passes.
    #[test]
    fn a_fence_at_scenario_5s_origin_reaches_across_the_antimeridian_and_the_pole() {
        let fence = fence_at_scenario_5s_origin();
        let cases = [
            // Across the pole onto the opposite meridian: 0.1 + 0.05 = 0.15 degree, 16,679 m.
            (g(89.95, 0.0), true),
            // Either side of the antimeridian, one degree of longitude apart from the
            // centre's meridian at colatitude 0.2: about 0.10003 degree, 11,123 m.
            (g(89.8, 179.0), true),
            (g(89.8, -179.0), true),
            // A quarter turn around the pole either way at the centre's own colatitude:
            // 0.1 x sqrt(2) = 0.14142 degree, 15,725 m.
            (g(89.9, 90.0), true),
            (g(89.9, -90.0), true),
            // Through the pole onto the opposite meridian at the same colatitude:
            // 0.1 + 0.1 = 0.2 degree, 22,239 m.
            (g(89.9, 0.0), false),
            // Away from the pole down the centre's own meridian: 0.3 - 0.1 = 0.2 degree,
            // 22,239 m; and one degree across the antimeridian from there, 22,242 m.
            (g(89.7, 180.0), false),
            (g(89.7, -179.0), false),
            // Further across the pole: 0.1 + 0.15 = 0.25 degree, 27,799 m.
            (g(89.85, 0.0), false),
        ];
        for (point, inside) in cases {
            let (lat, lon) = (point.lat_rad.to_degrees(), point.lon_rad.to_degrees());
            let d = independent_distance_m(fence.center, point);
            assert_eq!(
                d <= fence.radius_m,
                inside,
                "({lat}, {lon}): the hand derivation disagrees with the independent \
                 distance {d} m"
            );
            assert_eq!(
                fence.contains(point),
                inside,
                "({lat}, {lon}) at {d} m from a {} m fence",
                fence.radius_m
            );
        }
    }

    /// The `gungnir-geo` Geofence containment row of
    /// `docs/verification-capability-table.md` §2, at the edges: for every fence the tests
    /// above use, pairs of points about a metre inside and a metre outside the radius on
    /// twelve bearings. Inside or outside is decided by [`independent_distance_m`], never
    /// by the crate, and `contains` must agree for every point.
    ///
    /// Every tested point elsewhere in this module sits kilometres from an edge, where a
    /// wrong distance of a few hundred metres would still pass. Here a metre decides. The
    /// bearings include due east and due west, which put the edge points of the two
    /// antimeridian fences on both sides of it, and due north, which takes the Scenario 5
    /// fence's edge points across the pole.
    #[test]
    fn containment_agrees_with_an_independent_distance_a_metre_either_side_of_every_edge() {
        let fences = [
            ("10 N 20 E", fence_at_10n_20e()),
            (
                "equator on the antimeridian",
                fence_on_the_antimeridian_at_the_equator(),
            ),
            ("the origin, not no-go", open_fence_at_the_origin()),
            ("5 N 5 E", no_go_fence_at_5n_5e()),
            ("Scenario 5's origin", fence_at_scenario_5s_origin()),
        ];
        for (name, fence) in fences {
            for step in 0..12u32 {
                let bearing_deg = f64::from(step) * 30.0;
                for (offset_m, meant_inside) in [(-1.0, true), (1.0, false)] {
                    let point = destination(
                        fence.center,
                        bearing_deg.to_radians(),
                        fence.radius_m + offset_m,
                    );
                    let d = independent_distance_m(fence.center, point);
                    // The candidate really is a metre from the edge by the independent
                    // measure, so the agreement below is agreement at the edge.
                    assert!(
                        (d - (fence.radius_m + offset_m)).abs() < 1e-3,
                        "{name}, bearing {bearing_deg}: candidate is {d} m out, meant \
                         {} m",
                        fence.radius_m + offset_m
                    );
                    let inside = d <= fence.radius_m;
                    assert_eq!(inside, meant_inside, "{name}, bearing {bearing_deg}: {d} m");
                    assert_eq!(
                        fence.contains(point),
                        inside,
                        "{name}, bearing {bearing_deg}: ({}, {}) is {d} m from the centre \
                         of a {} m fence",
                        point.lat_rad.to_degrees(),
                        point.lon_rad.to_degrees(),
                        fence.radius_m
                    );
                }
            }
        }
    }
}
