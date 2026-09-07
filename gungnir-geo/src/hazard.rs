//! Physical obstacles and hazards: booms, nets, barriers, wrecks, shoals.
//!
//! Design: docs/design/DN-14-hazard-layer.md. Capability CAP-2.5; mission thread
//! MT-04 defends a port, and the booms and barriers the defence rests on are what
//! this layer holds.
//!
//! **A hazard is descriptive, not a rule.** A geofence says where we may act; a
//! hazard says what is there. Keeping them apart is the reason for a separate type:
//! a design that let a boom deny an intercept would put a survey artefact into the
//! authority chain. Nothing here is ever consulted by `gungnir-policy`, and the
//! verification row for CAP-2.5 tests exactly that.

use crate::{great_circle_distance_m, Geodetic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HazardKind {
    Boom,
    Net,
    Barrier,
    Wreck,
    Shoal,
    Other,
}

/// A hazard is a line more often than an area: a boom across a harbour mouth is a
/// segment, not a circle.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "shape", rename_all = "kebab-case")]
pub enum HazardExtent {
    Polyline { points: Vec<Geodetic> },
    Circle { center: Geodetic, radius_m: f64 },
}

/// A physical obstacle or hazard.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hazard {
    pub name: String,
    pub kind: HazardKind,
    pub extent: HazardExtent,
    /// True when the obstacle stops a surface craft.
    ///
    /// Stated rather than inferred from the kind: a boom does; a shoal does for a
    /// deep-draught vessel and not for a jet ski.
    pub blocks_surface: bool,
    /// Height above the surface, metres, where it constrains air movement.
    pub height_m: Option<f64>,
}

/// The hazard layer as published, carrying the baseline version it came from.
///
/// A static layer that claims to be current is a layer that lies: a boom removed
/// last week is still in a survey. The version is what lets the panel show when the
/// layer was last updated, which is the honest limit of a static layer.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HazardLayer {
    /// `ConfigBaseline::revision`, advanced on every promotion (DN-14 amendment 1). The
    /// schema version would be the same number for every survey ever loaded.
    pub baseline_version: u32,
    pub hazards: Vec<Hazard>,
}

/// Shortest distance from `p` to the segment `a`-`b`, metres.
///
/// Projects onto a local tangent plane, which is accurate over the hundreds of
/// metres a boom or a barrier spans and is what the harbour cases need.
fn distance_to_segment_m(p: Geodetic, a: Geodetic, b: Geodetic) -> f64 {
    // Local east-north offsets from `a`, metres.
    let scale_lat = MEAN_METRES_PER_RADIAN;
    let scale_lon = MEAN_METRES_PER_RADIAN * a.lat_rad.cos();
    let to_local = |q: Geodetic| {
        [
            (q.lon_rad - a.lon_rad) * scale_lon,
            (q.lat_rad - a.lat_rad) * scale_lat,
        ]
    };
    let pl = to_local(p);
    let bl = to_local(b);
    let seg_len_sq = bl[0] * bl[0] + bl[1] * bl[1];
    if seg_len_sq <= f64::EPSILON {
        return great_circle_distance_m(p, a);
    }
    let t = ((pl[0] * bl[0] + pl[1] * bl[1]) / seg_len_sq).clamp(0.0, 1.0);
    let closest = [bl[0] * t, bl[1] * t];
    let dx = pl[0] - closest[0];
    let dy = pl[1] - closest[1];
    (dx * dx + dy * dy).sqrt()
}

const MEAN_METRES_PER_RADIAN: f64 = crate::MEAN_EARTH_RADIUS_M;

impl Hazard {
    /// Shortest distance from a position to this hazard, metres; zero inside a
    /// circular one.
    pub fn distance_m(&self, position: Geodetic) -> f64 {
        match &self.extent {
            HazardExtent::Circle { center, radius_m } => {
                (great_circle_distance_m(*center, position) - radius_m).max(0.0)
            }
            HazardExtent::Polyline { points } => points
                .windows(2)
                .map(|w| distance_to_segment_m(position, w[0], w[1]))
                .fold(f64::INFINITY, f64::min),
        }
    }

    /// True when a route passing within `tolerance_m` would meet this hazard.
    pub fn is_crossed_by(&self, route: &[Geodetic], tolerance_m: f64) -> bool {
        route.iter().any(|p| self.distance_m(*p) <= tolerance_m)
    }
}

/// True if any vertex of the route comes within `tolerance_m` of a hazard that
/// blocks surface movement.
///
/// Companion to `route_crosses_no_go` in `gungnir-analytics`, and deliberately
/// separate from it: this answers "is something in the way", not "may we act here".
/// Segment crossings between vertices are not checked; densify the route first if
/// that matters, exactly as the no-go check already requires.
pub fn route_crosses_hazard(route: &[Geodetic], hazards: &[Hazard], tolerance_m: f64) -> bool {
    hazards
        .iter()
        .filter(|h| h.blocks_surface)
        .any(|h| h.is_crossed_by(route, tolerance_m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(lat_deg: f64, lon_deg: f64) -> Geodetic {
        Geodetic {
            lat_rad: lat_deg.to_radians(),
            lon_rad: lon_deg.to_radians(),
            alt_m: 0.0,
        }
    }

    fn boom(blocks_surface: bool) -> Hazard {
        Hazard {
            name: "harbour boom".into(),
            kind: HazardKind::Boom,
            extent: HazardExtent::Polyline {
                points: vec![g(0.0, 0.0), g(0.0, 0.01)],
            },
            blocks_surface,
            height_m: Some(2.0),
        }
    }

    #[test]
    fn a_point_on_the_boom_is_at_zero_distance() {
        let h = boom(true);
        assert!(h.distance_m(g(0.0, 0.005)) < 1.0);
    }

    #[test]
    fn distance_grows_away_from_the_boom() {
        let h = boom(true);
        let near = h.distance_m(g(0.001, 0.005));
        let far = h.distance_m(g(0.01, 0.005));
        assert!(near < far);
        // One thousandth of a degree of latitude is about 111 m.
        assert!((near - 111.0).abs() < 5.0, "{near}");
    }

    #[test]
    fn a_point_beyond_the_end_measures_to_the_endpoint() {
        let h = boom(true);
        // Well east of the eastern end, on the same latitude.
        let d = h.distance_m(g(0.0, 0.02));
        let expected = great_circle_distance_m(g(0.0, 0.01), g(0.0, 0.02));
        assert!((d - expected).abs() < 5.0, "{d} vs {expected}");
    }

    #[test]
    fn a_route_crossing_a_blocking_hazard_is_detected() {
        let hazards = vec![boom(true)];
        let route = vec![g(0.005, 0.005), g(0.0, 0.005), g(-0.005, 0.005)];
        assert!(route_crosses_hazard(&route, &hazards, 50.0));
    }

    #[test]
    fn a_route_crossing_a_non_blocking_hazard_is_not_detected() {
        // A shoal a jet ski passes over is a hazard on the layer and not an
        // obstacle to this craft.
        let hazards = vec![boom(false)];
        let route = vec![g(0.005, 0.005), g(0.0, 0.005), g(-0.005, 0.005)];
        assert!(!route_crosses_hazard(&route, &hazards, 50.0));
    }

    #[test]
    fn a_route_that_misses_is_not_detected() {
        let hazards = vec![boom(true)];
        let route = vec![g(0.02, 0.005), g(0.03, 0.005)];
        assert!(!route_crosses_hazard(&route, &hazards, 50.0));
    }

    #[test]
    fn a_circular_hazard_is_measured_to_its_edge() {
        let wreck = Hazard {
            name: "wreck".into(),
            kind: HazardKind::Wreck,
            extent: HazardExtent::Circle {
                center: g(0.0, 0.0),
                radius_m: 100.0,
            },
            blocks_surface: true,
            height_m: None,
        };
        assert!(
            wreck.distance_m(g(0.0, 0.0)) < f64::EPSILON,
            "inside is zero"
        );
        let outside = wreck.distance_m(g(0.001, 0.0));
        assert!(
            (outside - 11.0).abs() < 5.0,
            "111 m out, 100 m radius: {outside}"
        );
    }

    #[test]
    fn a_layer_carries_its_baseline_version() {
        // A static layer that claims to be current is a layer that lies, so the
        // version travels with the hazards rather than beside them.
        let layer = HazardLayer {
            baseline_version: 7,
            hazards: vec![boom(true)],
        };
        assert_eq!(layer.baseline_version, 7);
        assert_eq!(layer.hazards.len(), 1);
        assert!(HazardLayer::default().hazards.is_empty());
    }
}
