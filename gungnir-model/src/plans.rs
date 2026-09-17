// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

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
///
/// **A UUID v7 since GAP-130** (D-56): it was a counter restarting at 1 in every
/// workflow, so an effector report naming decision 1 reached every desktop's decision 1
/// (`crate::handoff::accept_report` matches on this alone). Written as the hyphenated
/// UUID and read from that or a pre-change number (D-60); shown by
/// [`DecisionId::short`] on screen and in full everywhere else (D-61). How is
/// [`crate::identifier`]'s.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DecisionId(pub u128);

impl serde::Serialize for DecisionId {
    /// The hyphenated UUID string (D-60).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::identifier::wire::serialize(&self.0, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for DecisionId {
    /// That string, or the number a pre-change journal holds (D-60).
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::identifier::wire::deserialize(deserializer).map(Self)
    }
}

impl DecisionId {
    /// The on-screen tag, `…9f3a61c2` (D-61): for a panel or an alert, never a record.
    #[must_use]
    pub fn short(self) -> String {
        crate::identifier::short(self.0)
    }
}

impl std::fmt::Display for DecisionId {
    /// The whole identifier, for an audit entry, a log field and PN-07 (D-61).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::identifier::fmt_full(self.0, f)
    }
}

impl std::str::FromStr for DecisionId {
    type Err = crate::identifier::IdentifierError;

    /// The hyphenated UUID, or a pre-change decimal number (D-60).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        crate::identifier::parse(text).map(Self)
    }
}

/// Identifies one item in an approval queue.
///
/// `gungnir-command` mints it in `submit_for_approval`, and re-exports this type rather
/// than declaring its own. **It moved here from `gungnir-command` in GAP-132**, for the
/// reason [`DecisionId`] is here rather than there: `CommandEvent::Queued` names the item
/// a node queued, and the model may not depend on the crate that holds the queue
/// (`docs/design/DN-31-node-approval-queue.md` §5.3). Nothing about the type changed --
/// the same `u128`, the same written form, the same tag -- so no journal, payload or
/// fixture reads differently.
///
/// **A UUID v7 since GAP-130** (D-56): it was a counter restarting at 1 in every
/// workflow, and DN-31 puts queue items from a node and from a cut-off desktop on the
/// same record. Written as the hyphenated UUID and read from that or a pre-change number
/// (D-60); shown by [`PendingApprovalId::short`] on screen and in full everywhere else
/// (D-61).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PendingApprovalId(pub u128);

impl serde::Serialize for PendingApprovalId {
    /// The hyphenated UUID string (D-60).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::identifier::wire::serialize(&self.0, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PendingApprovalId {
    /// That string, or the number a pre-change journal holds (D-60).
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::identifier::wire::deserialize(deserializer).map(Self)
    }
}

impl PendingApprovalId {
    /// The on-screen tag, `…9f3a61c2` (D-61): for a panel or an alert, never a record.
    #[must_use]
    pub fn short(self) -> String {
        crate::identifier::short(self.0)
    }
}

impl std::fmt::Display for PendingApprovalId {
    /// The whole identifier, for an audit entry and a log field (D-61).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::identifier::fmt_full(self.0, f)
    }
}

impl std::str::FromStr for PendingApprovalId {
    type Err = crate::identifier::IdentifierError;

    /// The hyphenated UUID, or a pre-change decimal number (D-60).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        crate::identifier::parse(text).map(Self)
    }
}

/// A client's idempotency key for one decision (DN-31 §5.2).
///
/// **Chosen by the client, not minted here**, which is why it is not a UUID under D-60:
/// its whole purpose is that a client which did not hear the answer can ask the same
/// question again and be told the first outcome rather than take a second decision. A
/// node compares it for equality and journals it beside the decision; it never orders by
/// it, parses meaning out of it, or shows it to anybody.
///
/// Bounded and non-empty, because it is untrusted text that reaches an append-only
/// record: an empty key would make two unrelated requests the same request, and an
/// unbounded one would let a caller decide how much of the node's record it writes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct RequestId(String);

/// The longest request key a node accepts. A UUID is 36 characters; this leaves room for
/// a client that names its own console and sequence as well.
pub const REQUEST_ID_MAX_LEN: usize = 128;

impl RequestId {
    /// Read a client's key.
    ///
    /// # Errors
    ///
    /// [`crate::ModelError`] when the key is empty, longer than
    /// [`REQUEST_ID_MAX_LEN`], or holds a control character -- text that would reach a
    /// log line or an audit entry and not read back as what was sent.
    pub fn new(text: impl Into<String>) -> Result<Self, crate::ModelError> {
        let text = text.into();
        if text.is_empty() {
            return Err(crate::ModelError::Invalid(
                "a decision's request key is empty; two requests with no key would be one \
                 request"
                    .into(),
            ));
        }
        if text.chars().count() > REQUEST_ID_MAX_LEN {
            return Err(crate::ModelError::Invalid(format!(
                "a decision's request key is longer than {REQUEST_ID_MAX_LEN} characters"
            )));
        }
        if text.chars().any(char::is_control) {
            return Err(crate::ModelError::Invalid(
                "a decision's request key holds a control character".into(),
            ));
        }
        Ok(Self(text))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for RequestId {
    /// Validated on the way in, so nothing downstream holds a key this node would refuse.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(text).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

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

/// One deconfliction check and how it came out.
///
/// Carries `detail` whether it passed or failed, because a check that failed has to say
/// why in the words the panel shows, and one that could not be evaluated has to say that
/// rather than read as a pass.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeconflictionCheck {
    pub kind: DeconflictionKind,
    pub passed: bool,
    /// Why, in the words the panel shows. Never empty on a failure, and never
    /// empty when the check could not be evaluated.
    pub detail: String,
}

/// What a deconfliction check examined before a fires task is offered.
///
/// A closed set rather than free text, so the panel can name every check it displays and
/// two deployments' results mean the same thing to whoever compares them.
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
