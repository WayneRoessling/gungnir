// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Laydowns: the placements a deployment could adopt
//! (`docs/design/DN-26-laydown-options.md` §4, GAP-087).
//!
//! **Why the type exists before the panel does.** PN-16 is an options table, a comparison
//! between options, and a rehearsal. `ConfigBaseline` carried one set of sensor and
//! resource positions, so there was one laydown, it was the deployment's current one, and
//! there was no way to describe a second. An options table built on that has exactly one
//! row and a comparison has nothing to compare. DN-26 §1 records that as the same shape
//! DN-24 had to fix before GAP-053 could be built.
//!
//! A laydown is **complete, not a delta**. A laydown that described only what differs
//! from the current one would be unreadable the moment two of them differed from each
//! other, and a planner comparing three options needs three whole answers rather than
//! three diffs against a fourth thing (§3).
//!
//! A laydown is **not a plan**. A plan assigns resources to tracks and is decided in
//! minutes; a laydown decides where the resources physically are and is decided in days.
//! DN-06's engagement authority does not reach a laydown, and nothing here adopts one:
//! moving a sensor is a physical act with an authority chain this system does not model
//! (§6 rule 4).

use crate::{SensorId, SensorMode};
use gungnir_core::ResourceId;

/// A named candidate placement of a deployment's sensors and effectors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LaydownId(pub String);

impl std::fmt::Display for LaydownId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where one sensor sits in a laydown, and what it is doing there.
///
/// The mode is carried because a laydown that moved a radar without saying whether it is
/// searching or tracking has not described a coverage answer, and coverage is the thing
/// laydowns are compared on (DN-26 §4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorPlacement {
    pub sensor: SensorId,
    /// Local ENU metres, in the deployment's own frame.
    pub position_enu: [f64; 3],
    pub mode: SensorMode,
}

/// Where one effector sits in a laydown.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResourcePlacement {
    pub resource: ResourceId,
    /// Local ENU metres, in the deployment's own frame.
    pub position_enu: [f64; 3],
}

/// A complete placement a deployment could adopt.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Laydown {
    pub id: LaydownId,
    /// What this option is for, in the planner's own words.
    ///
    /// Shown in the options table; a laydown whose reason for existing is not written
    /// down is one nobody can choose between (DN-26 §4).
    pub intent: String,
    pub sensors: Vec<SensorPlacement>,
    pub resources: Vec<ResourcePlacement>,
    /// True for exactly one laydown in a deployment: the placement actually in force.
    ///
    /// A configuration that declares laydowns and marks none current is refused rather
    /// than defaulted. Guessing which placement is the real one is exactly the quiet
    /// assumption that makes a comparison meaningless (DN-26 §3).
    pub current: bool,
}

impl Laydown {
    /// The sensors this laydown places, as identifiers.
    #[must_use]
    pub fn sensor_ids(&self) -> Vec<SensorId> {
        self.sensors.iter().map(|s| s.sensor).collect()
    }

    /// The resources this laydown places, as identifiers.
    #[must_use]
    pub fn resource_ids(&self) -> Vec<ResourceId> {
        self.resources.iter().map(|r| r.resource).collect()
    }

    /// Whether any coordinate in this laydown is not a finite number.
    ///
    /// A non-finite coordinate would propagate silently into a coverage answer, which is
    /// why DN-26 §4 rule 5 refuses the baseline rather than the placement.
    #[must_use]
    pub fn has_non_finite_coordinate(&self) -> bool {
        self.sensors
            .iter()
            .flat_map(|s| s.position_enu)
            .chain(self.resources.iter().flat_map(|r| r.position_enu))
            .any(|v| !v.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sensor(id: u32, position: [f64; 3]) -> SensorPlacement {
        SensorPlacement {
            sensor: SensorId(id),
            position_enu: position,
            mode: SensorMode::Search,
        }
    }

    fn laydown(id: &str, current: bool) -> Laydown {
        Laydown {
            id: LaydownId(id.into()),
            intent: "cover the western approach".into(),
            sensors: vec![sensor(1, [0.0, 0.0, 10.0])],
            resources: vec![ResourcePlacement {
                resource: ResourceId(1),
                position_enu: [50.0, 0.0, 0.0],
            }],
            current,
        }
    }

    #[test]
    fn a_laydown_reports_what_it_places() {
        let l = laydown("baseline", true);
        assert_eq!(l.sensor_ids(), vec![SensorId(1)]);
        assert_eq!(l.resource_ids(), vec![ResourceId(1)]);
        assert!(!l.has_non_finite_coordinate());
    }

    #[test]
    fn a_non_finite_coordinate_is_visible_wherever_it_sits() {
        let mut l = laydown("broken", false);
        l.sensors[0].position_enu[2] = f64::NAN;
        assert!(l.has_non_finite_coordinate(), "a sensor coordinate counts");

        let mut l = laydown("broken", false);
        l.resources[0].position_enu[0] = f64::INFINITY;
        assert!(
            l.has_non_finite_coordinate(),
            "a resource coordinate counts"
        );
    }

    #[test]
    fn a_laydown_survives_a_round_trip_through_json() {
        let l = laydown("western", true);
        let text = serde_json::to_string(&l).expect("serialized");
        let back: Laydown = serde_json::from_str(&text).expect("deserialized");
        assert_eq!(l, back);
    }
}
