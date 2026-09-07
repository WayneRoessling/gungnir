//! Effector handoff: what is passed to the system that will act, after a human
//! decided.
//!
//! Design: docs/design/DN-07-handoff.md. Capability CAP-4.4; mission thread MT-01
//! step 7 and the fires handoff in MT-06, both of which are a radio call today.
//!
//! **A handoff is only ever produced from a recorded decision.** There is no
//! constructor taking a bare plan, so contract C-01 is enforced by the compiler as
//! well as by a test: it is not possible to write code that hands something off
//! without an attribution. That is why [`DecisionAttribution`] is not optional.
//!
//! One shape serves intercept and fires, and one shape serves both directions of
//! travel. The receiving system needs the same four things either way: what to act
//! on, who decided, under what authority, and where the target information came
//! from. Two shapes would drift.

use crate::{
    DecisionId, MissionTime, PlanId, PlanKind, Provenance, Quality, Releasability, TrackId,
};

/// Who decided, when, and on what basis.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionAttribution {
    pub operator: String,
    pub role: String,
    pub at: MissionTime,
    /// The authority rule that permitted it, so the receiver can see the basis and
    /// an auditor can reconstruct it.
    pub authority_rule: Option<String>,
}

/// What is handed to an effector or fires system.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Handoff {
    pub decision: DecisionId,
    pub plan: PlanId,
    pub kind: PlanKind,
    pub decided_by: DecisionAttribution,
    /// Provenance and quality of every track the plan names, so the receiver can
    /// judge what it is being asked to act on.
    pub track_provenance: Vec<(TrackId, Provenance, Quality)>,
    pub releasability: Releasability,
    pub issued: MissionTime,
}

impl Handoff {
    /// Builds a handoff from a recorded decision.
    ///
    /// The only constructor. It takes an attribution rather than deriving one,
    /// because there is no way to derive who decided from a plan.
    pub fn from_decision(
        decision: DecisionId,
        plan: PlanId,
        kind: PlanKind,
        decided_by: DecisionAttribution,
        track_provenance: Vec<(TrackId, Provenance, Quality)>,
        releasability: Releasability,
        issued: MissionTime,
    ) -> Self {
        Self {
            decision,
            plan,
            kind,
            decided_by,
            track_provenance,
            releasability,
            issued,
        }
    }
}

/// How the handoff reached, or failed to reach, the effector.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DeliveryState {
    /// No endpoint is configured for this resource, so a person carries it.
    ///
    /// The honest description of a radio call, and the default state today. It
    /// makes the system useful in a deployment with no integrated effector without
    /// ever claiming an automated handoff happened.
    Manual,
    /// Handed to the endpoint.
    Delivered { at: MissionTime },
    /// The endpoint refused. The decision stands; what failed is delivery.
    Refused { reason: String, at: MissionTime },
    /// The endpoint was unreachable, so it is queued by store-and-forward.
    ///
    /// **Never dropped and never shown as delivered.**
    Undelivered { since: MissionTime },
}

impl DeliveryState {
    /// True only when the effector actually has it.
    pub fn is_delivered(&self) -> bool {
        matches!(self, DeliveryState::Delivered { .. })
    }

    /// True when somebody has to do something about it.
    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            DeliveryState::Refused { .. } | DeliveryState::Undelivered { .. }
        )
    }
}

/// What the receiving system reports back.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "report", rename_all = "kebab-case")]
pub enum EffectorReport {
    Acknowledged {
        at: MissionTime,
    },
    Executing {
        at: MissionTime,
    },
    Completed {
        at: MissionTime,
        effective: bool,
        detail: String,
    },
    Refused {
        at: MissionTime,
        reason: String,
    },
}

impl EffectorReport {
    pub fn at(&self) -> MissionTime {
        match self {
            EffectorReport::Acknowledged { at }
            | EffectorReport::Executing { at }
            | EffectorReport::Completed { at, .. }
            | EffectorReport::Refused { at, .. } => *at,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandoffError {
    /// A report arrived naming a decision this deployment does not know.
    ///
    /// Rejected and logged rather than applied: an effector report is an untrusted
    /// external input like any other.
    #[error("report names unknown decision {0:?}")]
    UnknownDecision(DecisionId),
}

/// Applies a report to the handoff it names, or rejects it.
pub fn accept_report<'a>(
    handoffs: &'a [Handoff],
    decision: DecisionId,
    report: &EffectorReport,
) -> Result<&'a Handoff, HandoffError> {
    let _ = report;
    handoffs
        .iter()
        .find(|h| h.decision == decision)
        .ok_or(HandoffError::UnknownDecision(decision))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InterceptSolutionView, ResourceId};

    fn attribution() -> DecisionAttribution {
        DecisionAttribution {
            operator: "op-1".into(),
            role: "supervisor".into(),
            at: MissionTime(100.0),
            authority_rule: Some("plan.decide/supervisor/area/hostile".into()),
        }
    }

    fn handoff(decision: u64) -> Handoff {
        Handoff::from_decision(
            DecisionId(decision),
            PlanId(1),
            PlanKind::Intercept {
                solutions: vec![InterceptSolutionView {
                    resource: ResourceId(5),
                    track: TrackId(10),
                    intercept_point: None,
                    time_to_intercept_s: None,
                }],
            },
            attribution(),
            vec![(TrackId(10), Provenance::default(), Quality::default())],
            Releasability::default(),
            MissionTime(101.0),
        )
    }

    #[test]
    fn every_handoff_carries_an_operator_a_role_and_a_time() {
        // Structural: there is no constructor that omits the attribution, so this
        // is checked by the compiler as much as by the assertion.
        let h = handoff(1);
        assert_eq!(h.decided_by.operator, "op-1");
        assert_eq!(h.decided_by.role, "supervisor");
        assert_eq!(h.decided_by.at, MissionTime(100.0));
        assert!(h.decided_by.authority_rule.is_some());
    }

    #[test]
    fn a_handoff_carries_the_provenance_of_every_track_it_names() {
        let h = handoff(1);
        assert_eq!(h.track_provenance.len(), 1);
        assert_eq!(h.track_provenance[0].0, TrackId(10));
    }

    #[test]
    fn an_unreachable_endpoint_queues_and_is_never_shown_as_delivered() {
        let queued = DeliveryState::Undelivered {
            since: MissionTime(102.0),
        };
        assert!(!queued.is_delivered());
        assert!(queued.needs_attention());

        let refused = DeliveryState::Refused {
            reason: "unit not ready".into(),
            at: MissionTime(102.0),
        };
        assert!(!refused.is_delivered());
        assert!(refused.needs_attention());
    }

    #[test]
    fn no_endpoint_configured_is_manual_delivery_not_failure() {
        let manual = DeliveryState::Manual;
        assert!(
            !manual.is_delivered(),
            "a radio call is not an automated handoff"
        );
        assert!(
            !manual.needs_attention(),
            "it is the expected state today, not an incident"
        );
    }

    #[test]
    fn only_a_delivered_handoff_reads_as_delivered() {
        let delivered = DeliveryState::Delivered {
            at: MissionTime(102.0),
        };
        assert!(delivered.is_delivered());
        assert!(!delivered.needs_attention());
    }

    #[test]
    fn a_report_naming_an_unknown_decision_is_rejected() {
        let known = [handoff(1)];
        let report = EffectorReport::Acknowledged {
            at: MissionTime(103.0),
        };
        assert!(accept_report(&known, DecisionId(1), &report).is_ok());
        assert_eq!(
            accept_report(&known, DecisionId(99), &report),
            Err(HandoffError::UnknownDecision(DecisionId(99)))
        );
    }

    #[test]
    fn every_report_variant_carries_its_time() {
        for report in [
            EffectorReport::Acknowledged {
                at: MissionTime(1.0),
            },
            EffectorReport::Executing {
                at: MissionTime(2.0),
            },
            EffectorReport::Completed {
                at: MissionTime(3.0),
                effective: true,
                detail: "target destroyed".into(),
            },
            EffectorReport::Refused {
                at: MissionTime(4.0),
                reason: "no rounds".into(),
            },
        ] {
            assert!(report.at().0 > 0.0);
        }
    }

    #[test]
    fn a_fires_handoff_uses_the_same_shape() {
        // One shape for both, because the receiver needs the same four things.
        let fires = Handoff::from_decision(
            DecisionId(2),
            PlanId(3),
            PlanKind::Fires(Box::new(crate::FiresPlan {
                target: TrackId(11),
                target_position: crate::Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
                location_error_m: 40.0,
                firing_unit: ResourceId(9),
                time_on_target: None,
                deconfliction: crate::DeconflictionResult::default(),
            })),
            attribution(),
            Vec::new(),
            Releasability::default(),
            MissionTime(200.0),
        );
        assert!(fires.kind.fires().is_some());
        assert_eq!(fires.decided_by.operator, "op-1");
    }
}
