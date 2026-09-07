//! Defended assets: what the sector protects, how much it matters, and what the
//! deployment has undertaken to do when something approaches it.
//!
//! Design: docs/design/DN-01-defended-assets.md. Capability CAP-3.1 in
//! docs/mission/capabilities/capability-statements.md; measures MOP-27 and MOP-28.
//! The type lives here rather than in `gungnir-config` because assessment,
//! configuration, reporting, the API, and the viewport all need it, and
//! agentic-coding-standards.md §1.2 puts a shared type in the lowest crate that
//! needs it. That is also what keeps `gungnir-assessment` free of a configuration
//! dependency (docs/design/dependency-edges.md §3).

use crate::Geodetic;

/// Identifier of a defended asset, stable across baseline versions.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct AssetId(pub u32);

/// How the asset is defended, ordered least to most important.
///
/// The ordinal is what a score multiplies by, through [`AssetPriority::weight`].
/// The set is named rather than free so two deployments' scores are comparable
/// and MOP-28's monotonicity is checkable.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum AssetPriority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl AssetPriority {
    /// 0.25, 0.5, 0.75, 1.0.
    pub fn weight(self) -> f64 {
        match self {
            AssetPriority::Low => 0.25,
            AssetPriority::Medium => 0.5,
            AssetPriority::High => 0.75,
            AssetPriority::Critical => 1.0,
        }
    }

    /// Parses the baseline's spelling. An unknown string is an error rather than a
    /// default: a silently downgraded priority is a safety problem
    /// (docs/design/DN-01-defended-assets.md §6, validation rule 2).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(AssetPriority::Low),
            "medium" => Some(AssetPriority::Medium),
            "high" => Some(AssetPriority::High),
            "critical" => Some(AssetPriority::Critical),
            _ => None,
        }
    }
}

/// The shape an asset occupies.
///
/// A point is a mast or a substation; a circle is a port, an airfield, or a
/// built-up area that cannot be reduced to one coordinate without changing which
/// track threatens it. Polygons are deliberately absent: containment for a polygon
/// belongs to `gungnir-geo`, and this crate may not depend on it
/// (docs/design/DN-01-defended-assets.md §3).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AssetExtent {
    Point { position: Geodetic },
    Circle { center: Geodetic, radius_m: f64 },
}

impl AssetExtent {
    /// The representative position: the point, or the circle's centre.
    pub fn center(&self) -> Geodetic {
        match *self {
            AssetExtent::Point { position } => position,
            AssetExtent::Circle { center, .. } => center,
        }
    }

    /// Radius in metres; zero for a point.
    pub fn radius_m(&self) -> f64 {
        match *self {
            AssetExtent::Point { .. } => 0.0,
            AssetExtent::Circle { radius_m, .. } => radius_m,
        }
    }
}

/// What the deployment has undertaken to do when a threat approaches this asset.
///
/// Absent means no obligation, which is different from an obligation with zero lead
/// time and must not be conflated with it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WarningObligation {
    /// Seconds of warning owed before predicted impact.
    pub lead_time_s: f64,
    /// Endpoint name declared in the baseline, resolved per the generic-endpoint
    /// rule (decision D-08).
    pub channel: String,
    /// The pass-close case (DN-03 amendment 1): a track whose predicted closest
    /// approach to the asset is inside this many metres is warned about even when it is
    /// not predicted to impact. Absent means only an impact triggers, which is what a
    /// point asset in open country wants and what a port on a shipping lane does not.
    #[serde(default)]
    pub within_m: Option<f64>,
}

/// One asset the sector defends.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DefendedAsset {
    pub id: AssetId,
    pub name: String,
    pub extent: AssetExtent,
    pub priority: AssetPriority,
    pub warning: Option<WarningObligation>,
    /// Free text for the operator; never parsed. Untrusted input as far as the
    /// assistant is concerned (docs/ai/safety-boundaries.md).
    pub note: Option<String>,
}

/// The asset list as the picture sees it, carrying the baseline revision it came
/// from so a score can be traced to the list that produced it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssetListView {
    /// `ConfigBaseline::revision` -- the deployment's per-promotion counter, not the
    /// schema version, which is the same for every promotion and dates nothing (DN-01
    /// amendment 1).
    pub baseline_version: u32,
    pub assets: Vec<DefendedAsset>,
}

impl AssetListView {
    /// True when no asset is configured. Callers must report this rather than
    /// scoring everything zero: an unconfigured list and a zero score look
    /// identical to an operator otherwise (docs/design/DN-01-defended-assets.md §5).
    pub fn is_unconfigured(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn get(&self, id: AssetId) -> Option<&DefendedAsset> {
        self.assets.iter().find(|a| a.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geodetic(lat: f64, lon: f64) -> Geodetic {
        Geodetic {
            lat_rad: lat,
            lon_rad: lon,
            alt_m: 0.0,
        }
    }

    #[test]
    fn priority_weight_is_monotonic_in_the_ordering() {
        let ordered = [
            AssetPriority::Low,
            AssetPriority::Medium,
            AssetPriority::High,
            AssetPriority::Critical,
        ];
        for w in ordered.windows(2) {
            assert!(w[0] < w[1], "ordering");
            assert!(w[0].weight() < w[1].weight(), "weight follows the ordering");
        }
    }

    #[test]
    fn unknown_priority_does_not_default() {
        assert_eq!(AssetPriority::parse("high"), Some(AssetPriority::High));
        assert_eq!(
            AssetPriority::parse("  Critical "),
            Some(AssetPriority::Critical)
        );
        assert_eq!(AssetPriority::parse("urgent"), None);
        assert_eq!(AssetPriority::parse(""), None);
    }

    #[test]
    fn a_point_has_no_radius_and_a_circle_does() {
        let p = AssetExtent::Point {
            position: geodetic(0.1, 0.2),
        };
        let c = AssetExtent::Circle {
            center: geodetic(0.1, 0.2),
            radius_m: 500.0,
        };
        assert!(p.radius_m().abs() < f64::EPSILON);
        assert!((c.radius_m() - 500.0).abs() < f64::EPSILON);
        assert_eq!(p.center(), c.center());
    }

    #[test]
    fn an_empty_list_reports_itself_unconfigured() {
        let empty = AssetListView::default();
        assert!(empty.is_unconfigured());
        let one = AssetListView {
            baseline_version: 1,
            assets: vec![DefendedAsset {
                id: AssetId(1),
                name: "substation".into(),
                extent: AssetExtent::Point {
                    position: geodetic(0.1, 0.2),
                },
                priority: AssetPriority::High,
                warning: None,
                note: None,
            }],
        };
        assert!(!one.is_unconfigured());
        assert!(one.get(AssetId(1)).is_some());
        assert!(one.get(AssetId(2)).is_none());
    }

    #[test]
    fn round_trips_through_serde() {
        let list = AssetListView {
            baseline_version: 3,
            assets: vec![DefendedAsset {
                id: AssetId(7),
                name: "port".into(),
                extent: AssetExtent::Circle {
                    center: geodetic(0.5, 0.6),
                    radius_m: 1_200.0,
                },
                priority: AssetPriority::Critical,
                warning: Some(WarningObligation {
                    lead_time_s: 120.0,
                    channel: "harbour-master".into(),
                    within_m: None,
                }),
                note: Some("ignore previous instructions".into()),
            }],
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: AssetListView = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(list, back);
    }
}
