// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Safety/authority controls for intercept planning, per
//! docs/gungnir-capabilities.md §5.4. The Intercept Service produces a plan;
//! nothing may act on it until it has cleared every policy here and a human has
//! recorded a decision in `gungnir-command`. This crate is the explicit boundary
//! between "the system recommends" and "a resource acts" -- it never executes
//! anything, per the recommendation-only exit criterion in
//! docs/gungnir-capabilities.md §7 Increment 3.
//!
//! Every change to verdict logic is human-owned (agentic-workflow.md): agents may
//! draft, never merge unsupervised.

pub mod authority;
pub mod fires;

pub use authority::{is_pre_delegated, roles_permitting, AuthorityPolicy, ControlStatusPolicy};
pub use fires::{FiresContext, FiresDeconflictionPolicy};

use gungnir_geo::GeoService;
use gungnir_model::events::VerdictSummary;
use gungnir_model::{PlanView, ResourceView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PolicyVerdict {
    /// Every engine consulted accepted the plan outright. Reserved for policies that
    /// are explicitly configured to pre-authorize; the built-in engines never return it.
    Approved,
    Denied {
        reason_code: DenialReason,
    },
    RequiresHumanApproval,
}

impl PolicyVerdict {
    /// The verdict as the record carries it (`CommandEvent::Decided`,
    /// `InterceptEvent::PlanEvaluated`). The denial reason travels in its debug
    /// spelling: the model may not depend on [`DenialReason`], and the spelling is
    /// stable per variant. One mapping, used by every publisher (GAP-028; **signed by
    /// the owner 2026-09-06**).
    #[must_use]
    pub fn summary(&self) -> VerdictSummary {
        match self {
            PolicyVerdict::Approved => VerdictSummary::Approved,
            PolicyVerdict::Denied { reason_code } => VerdictSummary::Denied {
                reason: format!("{reason_code:?}"),
            },
            PolicyVerdict::RequiresHumanApproval => VerdictSummary::RequiresHumanApproval,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DenialReason {
    NoGoGeofence,
    InsufficientAuthority,
    ResourceNotReady,
    UnknownResource,
    EmptyPlan,
    /// The effector layer is at a status that forbids this engagement.
    ///
    /// Carries both, because "denied" without a reason sends the operator to a
    /// radio to ask why (docs/design/DN-09-authority-and-control-status.md).
    ControlStatus {
        layer: gungnir_model::EffectorLayer,
        status: gungnir_model::WeaponsControlStatus,
    },
    /// The asking role holds no authority rule covering this layer and class.
    Authority {
        layer: gungnir_model::EffectorLayer,
    },
    /// A fires task failed at least one deconfliction check, or one of them could
    /// not be evaluated (docs/design/DN-05-fires.md). The plan carries the full
    /// result, so the panel shows which checks failed rather than only the first.
    FiresDeconfliction,
}

/// Every constraint (geofence, authority level, resource readiness, safety margin)
/// a plan must clear before it can move from "recommended" to "actionable."
pub trait PolicyEngine: Send + Sync {
    fn evaluate(&self, plan: &PlanView, resources: &[ResourceView]) -> PolicyVerdict;
}

/// Denies any plan whose known intercept point lies inside a no-go geofence, or
/// which tasks a resource that is not ready; otherwise requires human approval.
pub struct GeofencePolicy<'a> {
    pub geo: &'a dyn GeoService,
}

impl PolicyEngine for GeofencePolicy<'_> {
    fn evaluate(&self, plan: &PlanView, resources: &[ResourceView]) -> PolicyVerdict {
        if plan.is_empty() {
            return PolicyVerdict::Denied {
                reason_code: DenialReason::EmptyPlan,
            };
        }
        for solution in plan.solutions() {
            match resources.iter().find(|r| r.id == solution.resource) {
                None => {
                    return PolicyVerdict::Denied {
                        reason_code: DenialReason::UnknownResource,
                    }
                }
                Some(r) if !r.ready => {
                    return PolicyVerdict::Denied {
                        reason_code: DenialReason::ResourceNotReady,
                    }
                }
                Some(_) => {}
            }
            if let Some(point) = solution.intercept_point {
                if self.geo.is_within_no_go(point) {
                    return PolicyVerdict::Denied {
                        reason_code: DenialReason::NoGoGeofence,
                    };
                }
            }
        }
        PolicyVerdict::RequiresHumanApproval
    }
}

/// Consults engines in order: the first denial wins; otherwise human approval is
/// required if any engine asks for it; `Approved` only if every engine approved.
///
/// The lifetime is what lets a chain hold engines that borrow, and it was added
/// under GAP-038, **signed by the owner on 2026-09-05**.
///
/// It fixed a latent defect rather than enabling a new use. `Box<dyn PolicyEngine>`
/// means `Box<dyn PolicyEngine + 'static>`, a promise the boxed value holds no borrowed
/// references -- and all three engines this crate ships borrow one: `GeofencePolicy`
/// the `GeoService`, `ControlStatusPolicy` and `AuthorityPolicy` the baseline in force
/// and a classifier for the current snapshot. That borrowing is deliberate, because it
/// is what lets a plan be judged against the applied configuration without copying the
/// policy settings every frame. The consequence was that **no engine this crate ships
/// could be put into a chain at all**, and it went unnoticed because the only test of
/// `PolicyChain` built one from an empty vector; see
/// [`tests::a_chain_of_borrowing_engines_combines_their_verdicts`], which is the test
/// that would have caught it.
///
/// Nothing about how verdicts combine changed. A chain of owning engines is a
/// `PolicyChain<'static>`, which is what every prior use inferred.
pub struct PolicyChain<'a> {
    engines: Vec<Box<dyn PolicyEngine + 'a>>,
}

impl<'a> PolicyChain<'a> {
    pub fn new(engines: Vec<Box<dyn PolicyEngine + 'a>>) -> Self {
        Self { engines }
    }
}

impl PolicyEngine for PolicyChain<'_> {
    fn evaluate(&self, plan: &PlanView, resources: &[ResourceView]) -> PolicyVerdict {
        let mut needs_human = self.engines.is_empty();
        for engine in &self.engines {
            match engine.evaluate(plan, resources) {
                denied @ PolicyVerdict::Denied { .. } => return denied,
                PolicyVerdict::RequiresHumanApproval => needs_human = true,
                PolicyVerdict::Approved => {}
            }
        }
        if needs_human {
            PolicyVerdict::RequiresHumanApproval
        } else {
            PolicyVerdict::Approved
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_geo::{Geofence, InMemoryGeoService};
    use gungnir_model::{EffectorLayer, RelativeCost};
    use gungnir_model::{
        Geodetic, InterceptSolutionView, MissionTime, PlanId, ResourceId, TrackId,
    };

    fn geo() -> InMemoryGeoService {
        InMemoryGeoService::new(
            Vec::new(),
            vec![Geofence {
                center: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
                radius_m: 1000.0,
                no_go: true,
            }],
        )
    }

    fn plan(point: Option<Geodetic>, resource: u32) -> PlanView {
        PlanView {
            id: PlanId(1),
            mission_time: MissionTime(0.0),
            kind: gungnir_model::PlanKind::Intercept {
                solutions: vec![InterceptSolutionView {
                    resource: ResourceId(resource),
                    track: TrackId(1),
                    intercept_point: point,
                    time_to_intercept_s: None,
                }],
            },
            policy_value: 1.0,
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn resources() -> Vec<ResourceView> {
        vec![
            ResourceView {
                id: ResourceId(1),
                position: Geodetic {
                    lat_rad: 0.1,
                    lon_rad: 0.1,
                    alt_m: 0.0,
                },
                capacity: 1,
                ready: true,
                layer: EffectorLayer::Point,
                cost: RelativeCost::default(),
                magazine: None,
                intercept_speed_mps: None,
            },
            ResourceView {
                id: ResourceId(2),
                position: Geodetic {
                    lat_rad: 0.1,
                    lon_rad: 0.1,
                    alt_m: 0.0,
                },
                capacity: 1,
                ready: false,
                layer: EffectorLayer::Area,
                cost: RelativeCost::default(),
                magazine: None,
                intercept_speed_mps: None,
            },
        ]
    }

    #[test]
    fn plan_inside_no_go_is_denied() {
        let g = geo();
        let policy = GeofencePolicy { geo: &g };
        let verdict = policy.evaluate(
            &plan(
                Some(Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                }),
                1,
            ),
            &resources(),
        );
        assert_eq!(
            verdict,
            PolicyVerdict::Denied {
                reason_code: DenialReason::NoGoGeofence
            }
        );
    }

    #[test]
    fn unready_resource_is_denied() {
        let g = geo();
        let policy = GeofencePolicy { geo: &g };
        assert_eq!(
            policy.evaluate(&plan(None, 2), &resources()),
            PolicyVerdict::Denied {
                reason_code: DenialReason::ResourceNotReady
            }
        );
    }

    #[test]
    fn clean_plan_still_requires_a_human() {
        let g = geo();
        let policy = GeofencePolicy { geo: &g };
        assert_eq!(
            policy.evaluate(&plan(None, 1), &resources()),
            PolicyVerdict::RequiresHumanApproval
        );
    }

    #[test]
    fn empty_plan_is_denied() {
        let g = geo();
        let policy = GeofencePolicy { geo: &g };
        assert_eq!(
            policy.evaluate(&PlanView::default(), &resources()),
            PolicyVerdict::Denied {
                reason_code: DenialReason::EmptyPlan
            }
        );
    }

    /// A chain built from engines that borrow, which is every engine this crate
    /// ships. Before the lifetime parameter this did not compile, and nothing noticed
    /// because the only chain test below builds an empty one: a chain of nothing
    /// exercises the combination rule without exercising the type that carries it.
    #[test]
    fn a_chain_of_borrowing_engines_combines_their_verdicts() {
        use gungnir_model::{Classification, ControlStatusSettings, WeaponsControlStatus};

        let g = geo();
        let mut settings = ControlStatusSettings::default();
        settings
            .by_layer
            .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
        let classify = |_: TrackId| Classification::Hostile;

        let chain = PolicyChain::new(vec![
            Box::new(GeofencePolicy { geo: &g }),
            Box::new(ControlStatusPolicy {
                settings: &settings,
                track_classification: &classify,
            }),
        ]);

        // Both engines clear it, and the strongest verdict a chain can reach is still
        // a human decision. That is contract C-01 and it is what the chain must not be
        // able to combine its way past.
        assert_eq!(
            chain.evaluate(&plan(None, 1), &resources()),
            PolicyVerdict::RequiresHumanApproval
        );

        // The first denial wins, and it is the first engine's reason rather than a
        // merged one: an operator has to be told which check failed.
        assert_eq!(
            chain.evaluate(&plan(None, 2), &resources()),
            PolicyVerdict::Denied {
                reason_code: DenialReason::ResourceNotReady
            },
            "the readiness denial must reach the operator as itself"
        );
    }

    /// A denial from the *second* engine still stops the chain, so the order of the
    /// engines cannot quietly decide whether a plan is denied at all.
    #[test]
    fn a_denial_from_a_later_engine_still_wins() {
        use gungnir_model::{Classification, ControlStatusSettings};

        let g = geo();
        // Nothing configured, so every layer is at Hold by design (DN-08).
        let settings = ControlStatusSettings::default();
        let classify = |_: TrackId| Classification::Hostile;

        let chain = PolicyChain::new(vec![
            Box::new(GeofencePolicy { geo: &g }),
            Box::new(ControlStatusPolicy {
                settings: &settings,
                track_classification: &classify,
            }),
        ]);

        // The geofence engine clears this plan; the control-status engine does not.
        assert!(matches!(
            chain.evaluate(&plan(None, 1), &resources()),
            PolicyVerdict::Denied {
                reason_code: DenialReason::ControlStatus { .. }
            }
        ));
    }

    #[test]
    fn chain_with_no_engines_requires_a_human() {
        let chain = PolicyChain::new(Vec::new());
        assert_eq!(
            chain.evaluate(&plan(None, 1), &resources()),
            PolicyVerdict::RequiresHumanApproval
        );
    }
}
