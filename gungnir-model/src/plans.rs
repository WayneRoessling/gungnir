//! What a plan proposes: an intercept, or a fires task.
//!
//! Design: docs/design/DN-05-fires.md and docs/design/DN-06-engagement-and-effect.md.
//! Capabilities CAP-3.8 and CAP-4.6; decision D-07 put fires in the first release.
//!
//! **This module carries the design set's one breaking change.** `PlanView.solutions`
//! became `PlanView.kind`, so a plan can be an intercept or a fires task. Changing a
//! field's type needs a new schema version and a new path version under the contract's
//! own compatibility rules, and the owner took option B on 2026-09-05: replace
//! outright, `SCHEMA_VERSION` 1 to 2, path `/v1` to `/v2`, no deprecated mirror.
//! Removing the old path meets the contract's condition rather than excepting it,
//! because no client is deployed against it (docs/gungnir-api-v1.md, "Version 2").

use crate::{Geodetic, InterceptSolutionView, MissionTime, ResourceId, TrackId};

/// Identifies one recorded decision.
///
/// `gungnir-command` mints it; everything else refers to it. Introduced so
/// engagement state can key on a decision without anyone depending on the crate
/// that records decisions: `gungnir-intercept-service` is a service facade and may
/// not depend on a productization crate (docs/design/DN-06-engagement-and-effect.md
/// §2).
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct DecisionId(pub u64);

/// What a plan proposes. `Intercept` is the behaviour that existed before fires.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PlanKind {
    Intercept {
        solutions: Vec<InterceptSolutionView>,
    },
    Fires(Box<FiresPlan>),
}

impl Default for PlanKind {
    fn default() -> Self {
        PlanKind::Intercept {
            solutions: Vec::new(),
        }
    }
}

impl PlanKind {
    /// The intercept solutions, empty for a fires plan.
    pub fn solutions(&self) -> &[InterceptSolutionView] {
        match self {
            PlanKind::Intercept { solutions } => solutions,
            PlanKind::Fires(_) => &[],
        }
    }

    /// The fires task, if this is one.
    pub fn fires(&self) -> Option<&FiresPlan> {
        match self {
            PlanKind::Fires(f) => Some(f),
            PlanKind::Intercept { .. } => None,
        }
    }

    /// True when the plan proposes nothing at all.
    pub fn is_empty(&self) -> bool {
        match self {
            PlanKind::Intercept { solutions } => solutions.is_empty(),
            PlanKind::Fires(_) => false,
        }
    }
}

/// A fires task against a located ground target.
///
/// Distinct from an intercept because the target does not move toward us, the
/// effect is on the ground, and the deconfliction question is about who else is
/// there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FiresPlan {
    pub target: TrackId,
    pub target_position: Geodetic,
    /// One-sigma target location error, metres.
    ///
    /// Fires against a poorly located target is a different decision from fires
    /// against a well located one, and the operator must see which they are being
    /// asked to approve.
    pub location_error_m: f64,
    pub firing_unit: ResourceId,
    /// Requested time on target, if the task is time-constrained.
    pub time_on_target: Option<MissionTime>,
    pub deconfliction: DeconflictionResult,
}

/// Every deconfliction check, whether it passed or failed.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeconflictionResult {
    pub checks: Vec<DeconflictionCheck>,
}

impl DeconflictionResult {
    /// True only when every check ran and every one passed.
    ///
    /// An empty result is **not** clear: no check having run is not the same as
    /// every check having passed.
    pub fn is_clear(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|c| c.passed)
    }

    /// The checks that failed, for the panel.
    pub fn failures(&self) -> impl Iterator<Item = &DeconflictionCheck> {
        self.checks.iter().filter(|c| !c.passed)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeconflictionCheck {
    pub kind: DeconflictionKind,
    pub passed: bool,
    /// Why, in the words the panel shows. Never empty on a failure, and never
    /// empty when the check could not be evaluated.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeconflictionKind {
    FriendlyPosition,
    NoFireArea,
    AirspaceMeasure,
    InterceptorTrajectory,
    /// The target's location error exceeds what the policy permits for fires.
    LocationAccuracy,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(kind: DeconflictionKind, passed: bool) -> DeconflictionCheck {
        DeconflictionCheck {
            kind,
            passed,
            detail: "because".into(),
        }
    }

    #[test]
    fn an_intercept_plan_exposes_its_solutions_and_a_fires_plan_does_not() {
        let intercept = PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(1),
                track: TrackId(2),
                intercept_point: None,
                time_to_intercept_s: None,
            }],
        };
        assert_eq!(intercept.solutions().len(), 1);
        assert!(intercept.fires().is_none());
        assert!(!intercept.is_empty());

        let fires = PlanKind::Fires(Box::new(FiresPlan {
            target: TrackId(2),
            target_position: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            location_error_m: 40.0,
            firing_unit: ResourceId(9),
            time_on_target: None,
            deconfliction: DeconflictionResult::default(),
        }));
        assert!(fires.solutions().is_empty());
        assert!(fires.fires().is_some());
        assert!(!fires.is_empty(), "a fires task proposes something");
    }

    #[test]
    fn an_empty_deconfliction_result_is_not_clear() {
        // No check having run is not the same as every check having passed. This is
        // the honest-status rule applied to a safety check.
        assert!(!DeconflictionResult::default().is_clear());
    }

    #[test]
    fn a_result_is_clear_only_when_every_check_passed() {
        let all_pass = DeconflictionResult {
            checks: vec![
                check(DeconflictionKind::FriendlyPosition, true),
                check(DeconflictionKind::NoFireArea, true),
            ],
        };
        assert!(all_pass.is_clear());
        assert_eq!(all_pass.failures().count(), 0);

        let one_fails = DeconflictionResult {
            checks: vec![
                check(DeconflictionKind::FriendlyPosition, true),
                check(DeconflictionKind::NoFireArea, false),
            ],
        };
        assert!(!one_fails.is_clear());
        assert_eq!(one_fails.failures().count(), 1);
    }

    #[test]
    fn every_failure_is_reported_not_just_the_first() {
        // A verdict naming only one obstacle lets the operator clear it and believe
        // the task is clean.
        let several = DeconflictionResult {
            checks: vec![
                check(DeconflictionKind::FriendlyPosition, false),
                check(DeconflictionKind::NoFireArea, false),
                check(DeconflictionKind::AirspaceMeasure, true),
            ],
        };
        assert_eq!(several.failures().count(), 2);
    }

    #[test]
    fn plan_kinds_round_trip_through_serde() {
        let fires = PlanKind::Fires(Box::new(FiresPlan {
            target: TrackId(2),
            target_position: Geodetic {
                lat_rad: 0.1,
                lon_rad: 0.2,
                alt_m: 0.0,
            },
            location_error_m: 40.0,
            firing_unit: ResourceId(9),
            time_on_target: Some(MissionTime(120.0)),
            deconfliction: DeconflictionResult {
                checks: vec![check(DeconflictionKind::LocationAccuracy, true)],
            },
        }));
        let json = serde_json::to_string(&fires).expect("serialize");
        let back: PlanKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fires, back);
    }
}
