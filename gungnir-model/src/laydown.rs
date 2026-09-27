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
    /// Where this placement points the sensor, when it differs from the sensor's own
    /// declaration (GAP-118, D-84): bearings against true north at the placement, like
    /// every sector in a baseline.
    ///
    /// **Absent means the declared sensor's sector**, which is itself absent -- the full
    /// circle -- for a sensor that declares none. A laydown that moves a sectored radar and
    /// re-aims it says so here; one that moves it without re-aiming keeps its boresight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azimuth_sector: Option<crate::AzimuthSector>,
    /// The elevations this placement lets the sensor see, when they differ from the
    /// sensor's own declaration (GAP-158, D-111): a floor and a ceiling against the local
    /// vertical at the placement.
    ///
    /// **Absent means the declared sensor's band**, and a sensor that declares none has the
    /// baseline's `analytics.coverage_min_elevation_rad` as its floor and the zenith as its
    /// ceiling. A placement that states a band states the whole band: it replaces the
    /// declaration's, never half of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_band: Option<crate::ElevationBand>,
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

/// One of the eleven committed test-track scenarios (`docs/test-tracks/scenario-library.md`),
/// naming which fixture a rehearsal replayed a laydown against (GAP-045): the ten plan-07
/// scenarios, one per mission vignette, and TT-11, usability round 1's raid down its own
/// declared approach, which a round-1 laydown rehearsal re-observes (GAP-147, D-112).
///
/// Lives here rather than beside the rehearsal harness that reads the fixture files,
/// because `gungnir-ui` draws PN-16's rehearsal control and picks this from a list, and
/// `gungnir-ui` depends on this crate and nothing that reads a filesystem.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TestTrackNumber(pub u8);

impl TestTrackNumber {
    /// The eleven committed scenarios, in order, for a picker to offer.
    pub const ALL: [TestTrackNumber; 11] = [
        TestTrackNumber(1),
        TestTrackNumber(2),
        TestTrackNumber(3),
        TestTrackNumber(4),
        TestTrackNumber(5),
        TestTrackNumber(6),
        TestTrackNumber(7),
        TestTrackNumber(8),
        TestTrackNumber(9),
        TestTrackNumber(10),
        TestTrackNumber(11),
    ];

    #[must_use]
    pub fn label(self) -> String {
        format!("TT-{:02}", self.0)
    }
}

/// How a coverage gap falls short: detectable by one sensor, or watched by nothing
/// (`docs/design/DN-12-coverage-and-gaps.md` §3).
///
/// **Declared here and re-exported by `gungnir-analytics`**, which computes it: a gap
/// acceptance on the journal names one (GAP-106,
/// `docs/design/DN-33-accepting-a-coverage-gap.md` §3), and this crate may not depend on the
/// one that computes coverage (AP-06). The spelling on the wire is unchanged.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum GapSeverity {
    /// Covered by one sensor only: detectable, not fusible.
    SingleSensor,
    /// Covered by nothing.
    Uncovered,
}

impl GapSeverity {
    /// The words PN-11 and the record use for it.
    #[must_use]
    pub fn words(self) -> &'static str {
        match self {
            GapSeverity::SingleSensor => "single-sensor",
            GapSeverity::Uncovered => "uncovered",
        }
    }
}

/// A coverage gap as an acceptance names it: enough to find the same gap in a later
/// coverage report (DN-33 §4, §5).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AcceptedGap {
    /// The approach, by the name the baseline declares, not by its position in a list.
    pub approach: String,
    pub severity: GapSeverity,
    /// Along the approach, metres: where the gap starts and ends.
    pub from_m: f64,
    pub to_m: f64,
    /// The spacing the report that found it sampled at (DN-12 §5 rule 4): a gap found every
    /// 500 m and one found every 100 m are different answers about the same ground.
    pub sample_spacing_m: f64,
    /// Whether that report masked line of sight against terrain; flat and masked are
    /// different answers too (DN-26 §5).
    pub terrain_masking_applied: bool,
}

impl AcceptedGap {
    /// Approach metres the gap spans.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.to_m - self.from_m
    }

    /// The gap in words: the approach, where along it, and how it falls short.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{} {:.1}-{:.1} km ({})",
            self.approach,
            self.from_m / 1000.0,
            self.to_m / 1000.0,
            self.severity.words()
        )
    }
}

/// Identifies one gap acceptance: a per-deployment serial, continued past the highest the
/// journal holds (DN-33 §8 rule 6).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct GapAcceptanceId(pub u64);

impl std::fmt::Display for GapAcceptanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A commander's acceptance of a coverage gap (GAP-106, D-118, DN-33 §4): the gap, why,
/// who, and what it holds for.
///
/// **It holds for the baseline revision and the laydown it was made under**, and re-opens
/// by itself when either changes or the gap's own shape changes (DN-33 §5). An accepted gap
/// is still drawn and still counted; this names it, it does not remove it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GapAcceptance {
    pub id: GapAcceptanceId,
    pub gap: AcceptedGap,
    /// Why, in the commander's words. Never empty: an acceptance without a reason is a gap
    /// somebody clicked past, not a risk somebody took.
    pub reason: String,
    /// The signed-in operator who accepted it (DN-33 §6: never an unattributed session).
    pub operator: String,
    /// The role their session carried, in `gungnir_security::Role`'s debug spelling.
    pub role: String,
    /// The baseline revision in force when it was accepted.
    pub revision: u32,
    /// The laydown marked current when it was accepted; `None` for a deployment that
    /// declares no laydown.
    pub laydown: Option<LaydownId>,
    pub at: crate::MissionTime,
}

/// Why a gap acceptance stopped holding (DN-33 §5), in the order the checks run.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ReopenedBecause {
    /// The running baseline is at another revision.
    RevisionChanged { from: u32, to: u32 },
    /// Another laydown is marked current, or none is.
    LaydownChanged {
        from: Option<LaydownId>,
        to: Option<LaydownId>,
    },
    /// No gap of the accepted shape is on the approach now. `now` is what the approach
    /// holds instead where the accepted gap was -- empty when that stretch is covered.
    ShapeChanged { now: Vec<AcceptedGap> },
    /// Coverage cannot be computed at all any more, so nothing can hold.
    NotMeasured { reason: String },
}

impl ReopenedBecause {
    /// The reason in words, for PN-11, the alert and the audit entry.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            ReopenedBecause::RevisionChanged { from, to } => {
                format!("the baseline moved from revision {from} to revision {to}")
            }
            ReopenedBecause::LaydownChanged { from, to } => {
                let name = |l: &Option<LaydownId>| {
                    l.as_ref()
                        .map_or_else(|| "no laydown".to_owned(), |l| format!("laydown {l}"))
                };
                format!(
                    "the laydown in force changed from {} to {}",
                    name(from),
                    name(to)
                )
            }
            ReopenedBecause::ShapeChanged { now } if now.is_empty() => {
                "the stretch it named is covered now".to_owned()
            }
            ReopenedBecause::ShapeChanged { now } => format!(
                "the gap there is now {}",
                now.iter()
                    .map(AcceptedGap::describe)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ReopenedBecause::NotMeasured { reason } => {
                format!("coverage can no longer be computed: {reason}")
            }
        }
    }
}

/// What a laydown rehearsal ran under (`docs/design/DN-26-laydown-options.md` §11 item 3,
/// D-119): five SHA-256 digests, each over the canonical JSON of one part of what the
/// throwaway desktop took from the deployment (DN-32 §14, D-113). Two rehearsals ran under
/// the same thing exactly when every part is equal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RehearsalBasis {
    /// Every sensor's position, mode, sector and band, and every resource's position, as
    /// the laydown places them.
    pub placements: String,
    /// The sensors the laydown places, as their declarations reach the run.
    pub sensors: String,
    /// The deployment's resources, as they reach the run.
    pub resources: String,
    /// The whole `policy`, the geofences with the origin they are placed from, and the
    /// allocation horizon.
    pub policy: String,
    /// The algorithm candidates, mission and tracking profiles, the profile in force and
    /// the late-data policy.
    pub tracking: String,
}

impl RehearsalBasis {
    /// The parts that differ between two bases, by name, in a fixed order.
    #[must_use]
    pub fn differs_in(&self, other: &RehearsalBasis) -> Vec<&'static str> {
        [
            ("placements", self.placements != other.placements),
            ("sensors", self.sensors != other.sensors),
            ("resources", self.resources != other.resources),
            ("policy", self.policy != other.policy),
            ("tracking", self.tracking != other.tracking),
        ]
        .into_iter()
        .filter_map(|(name, differs)| differs.then_some(name))
        .collect()
    }
}

/// A laydown rehearsal as the record holds it (`docs/design/DN-26-laydown-options.md` §11
/// item 2, D-120): which laydown, against which recording, under which baseline, when, and
/// what it ran under.
///
/// The run's figures are not here: they are the desktop's, for its session. This is what
/// lets a later session, or the decision on a plan, say whether the laydown in force was
/// ever rehearsed under what is running.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RehearsalStamp {
    pub laydown: LaydownId,
    pub scenario: TestTrackNumber,
    pub seed: u64,
    /// The baseline revision the rehearsal ran under.
    pub revision: u32,
    pub basis: RehearsalBasis,
    /// The live desktop's mission time when it was run.
    pub ran_at: crate::MissionTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sensor(id: u32, position: [f64; 3]) -> SensorPlacement {
        SensorPlacement {
            sensor: SensorId(id),
            position_enu: position,
            mode: SensorMode::Search,
            azimuth_sector: None,
            elevation_band: None,
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
